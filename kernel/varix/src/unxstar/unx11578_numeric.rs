//! UNX-F11578 C locale 数值千分页分隔判据
//!
//! `printf("%f")`、`strtod(3)`、`scanf("%f")` 在 C locale 下共用一套**极简**
//! 数字约定：小数点固定为 `.`，没有千分位分隔符，`LC_NUMERIC` 的 `thousands_sep`
//! 是空串、`grouping` 是 `CHAR_MAX`（即不分页）。于是 "1234567.89" 必须原样
//! 出现七个连续数字位，任何插入 `,`、`.`、空格或窄空格的实现都会被本模块的
//! 逐字节判据抓住。
//!
//! 模型分四层：
//!
//! * 定点层：[`Fixed`] 用 `i64` 尾数加 `u8` 小数位表示十进制数，所有运算
//!   （解析、舍入、比较、格式化）都在整数上完成，不经过浮点，因此舍入是
//!   确定的「半值远离零」，不存在二进制浮点的表示误差。
//! * 解析层：[`parse_c`] 只收 `[+-]?digits[.digits?]`：逗号、空格、撇号、
//!   下划线一律 `NM_ESEP`；第二个小数点 `NM_EPOINT`；符号出现在中间
//!   `NM_ESIGN`；无数字 `NM_EDIGIT`；小数位超过上限 `NM_ESCALE`；尾数溢出
//!   `NM_ERANGE`。`"1.234,56"` 在 C 下必须在逗号处失败，这一条是核心锚。
//! * 渲染层：[`format_c`] 拒绝非 C locale（`NM_ELOCALE`），按目标小数位
//!   四舍五入后输出纯 `-?digits(.digits)?`，**永不分组**。作为对照，
//!   [`format_foreign`] 会把同样的值渲染成 de_DE 的 "1.234.567,89"、en_US 的
//!   "1,234,567.89"、fr_FR 的窄空格分组；这些串只用于证明差异存在，绝不
//!   回流到 C 路径。
//! * 对照层：[`parse_locale`] 按目标 locale 自己的小数点与分组符归一化后
//!   再走 C 解析，用来验证「同一个 "1.234,56" 在 de_DE 是 1234.56，在 C 是
//!   错误」——locale 决定的只是词法，不是数值。
//!
//! 闸门次序：小数点 → 分组缺席 → 严格解析 → 分隔拒绝 → 符号 → 小数位 →
//! 溢出 → 舍入 → 外语对照。
//!
//! 全模块无 std、无 unsafe、无 panic：数字一律整数运算，字符串只追加，
//! 索引一律 `get()`，失败全部落 13 个错误码。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf950;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 小数位上限。
pub const SCALE_MAX: u8 = 6;

/// C locale 的小数点。
pub const C_POINT: u8 = b'.';

/// C locale 的分组符：空。
pub const C_SEP: &str = "";

/// C locale 的分组宽度：0 表示不分页。
pub const C_GROUPING: u8 = 0;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 缺少数字或出现非数字字节。
pub const NM_EDIGIT: u32 = SEG_BASE + 0x01;
/// 小数点重复。
pub const NM_EPOINT: u32 = SEG_BASE + 0x02;
/// 出现分组分隔符。
pub const NM_ESEP: u32 = SEG_BASE + 0x03;
/// 小数位越界。
pub const NM_ESCALE: u32 = SEG_BASE + 0x04;
/// 尾数溢出。
pub const NM_ERANGE: u32 = SEG_BASE + 0x05;
/// 非 C locale。
pub const NM_ELOCALE: u32 = SEG_BASE + 0x06;
/// 符号位置非法。
pub const NM_ESIGN: u32 = SEG_BASE + 0x07;
/// 分组参数非法。
pub const NM_EGROUP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const NM_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const NM_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const NM_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const NM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const NM_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 解析。
pub const OP_PARSE: u8 = 0x01;
/// 格式化。
pub const OP_FORMAT: u8 = 0x02;
/// 舍入。
pub const OP_ROUND: u8 = 0x03;
/// 分组渲染。
pub const OP_GROUP: u8 = 0x04;
/// locale 判定。
pub const OP_LOCALE: u8 = 0x05;
/// 小数位。
pub const OP_SCALE: u8 = 0x06;
/// 比较。
pub const OP_CMP: u8 = 0x07;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（逐位八次乘法）。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut sh = 0u32;
    while sh < 64 {
        let b = ((w >> sh) & 0xff) as u8;
        v = fold_byte(v, b);
        sh += 8;
    }
    v
}

// ---------------------------------------------------------------------------
// 字节助手
// ---------------------------------------------------------------------------

/// 两个字节串是否相等。
fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a.get(i) != b.get(i) {
            return false;
        }
        i += 1;
    }
    true
}

/// `text[at..]` 是否以 `pat` 开头。
fn starts_with_at(text: &[u8], at: usize, pat: &[u8]) -> bool {
    if pat.is_empty() {
        return false;
    }
    let mut k = 0usize;
    while k < pat.len() {
        match (text.get(at + k), pat.get(k)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return false;
                }
            }
            _ => return false,
        }
        k += 1;
    }
    true
}

/// 统计某个字节出现次数。
pub fn count_byte(text: &[u8], b: u8) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < text.len() {
        if text.get(i) == Some(&b) {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 追加无符号十进制。
fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 && n < 20 {
        if let Some(d) = buf.get_mut(n) {
            *d = b'0' + (v % 10) as u8;
        }
        n += 1;
        v /= 10;
    }
    while n > 0 {
        n -= 1;
        if let Some(d) = buf.get(n) {
            out.push(*d as char);
        }
    }
}

/// 10 的幂（i64）。
fn pow10_i64(n: u8) -> Result<i64, u32> {
    match n {
        0 => Ok(1),
        1 => Ok(10),
        2 => Ok(100),
        3 => Ok(1_000),
        4 => Ok(10_000),
        5 => Ok(100_000),
        6 => Ok(1_000_000),
        _ => Err(NM_ESCALE),
    }
}

/// 10 的幂（u64）。
fn pow10_u64(n: u8) -> Result<u64, u32> {
    match pow10_i64(n) {
        Ok(v) => Ok(v as u64),
        Err(c) => Err(c),
    }
}

// ---------------------------------------------------------------------------
// 定点数
// ---------------------------------------------------------------------------

/// 定点十进制数：`units / 10^scale`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fixed {
    /// 带符号尾数。
    pub units: i64,
    /// 小数位（0..=SCALE_MAX）。
    pub scale: u8,
}

impl Fixed {
    /// 构造。
    pub const fn new(units: i64, scale: u8) -> Fixed {
        Fixed { units, scale }
    }

    /// 零。
    pub const fn zero() -> Fixed {
        Fixed { units: 0, scale: 0 }
    }

    /// 是否负数。
    pub fn is_negative(&self) -> bool {
        self.units < 0
    }

    /// 尾数绝对值（饱和）。
    pub fn magnitude(&self) -> u32 {
        let m = if self.units < 0 { self.units.saturating_neg() } else { self.units };
        if m > 0xffff_ffff {
            0xffff_ffff
        } else {
            m as u32
        }
    }

    /// 符号：-1 / 0 / 1。
    pub fn sign(&self) -> i32 {
        if self.units < 0 {
            -1
        } else if self.units > 0 {
            1
        } else {
            0
        }
    }
}

/// 舍入到目标小数位，返回对齐后的尾数。
pub fn round_units(v: Fixed, target: u8) -> Result<i64, u32> {
    if target > SCALE_MAX {
        return Err(NM_ESCALE);
    }
    if target >= v.scale {
        let p = pow10_i64(target - v.scale)?;
        match v.units.checked_mul(p) {
            Some(u) => Ok(u),
            None => Err(NM_ERANGE),
        }
    } else {
        let p = pow10_i64(v.scale - target)?;
        let q = v.units / p;
        let r = v.units % p;
        let mag = if r < 0 { r.saturating_neg() } else { r };
        if mag.saturating_mul(2) >= p {
            if v.units < 0 {
                match q.checked_sub(1) {
                    Some(u) => Ok(u),
                    None => Err(NM_ERANGE),
                }
            } else {
                match q.checked_add(1) {
                    Some(u) => Ok(u),
                    None => Err(NM_ERANGE),
                }
            }
        } else {
            Ok(q)
        }
    }
}

/// 渲染对齐后的尾数：C 形状，小数点 `.`，无分组。
pub fn render_units(units: i64, scale: u8) -> String {
    let neg = units < 0;
    let mag = if neg { units.saturating_neg() } else { units };
    let p = match pow10_u64(scale) {
        Ok(v) => v,
        Err(_) => 1,
    };
    let m = mag as u64;
    let ip = m / p;
    let fp = m % p;
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    push_u64(&mut out, ip);
    if scale > 0 {
        out.push(C_POINT as char);
        let mut div = p / 10;
        while div > 0 {
            let d = (fp / div) % 10;
            out.push((b'0' + d as u8) as char);
            div /= 10;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// C locale 严格解析：`[+-]?digits[.digits?]`，小数位不超过 `max_scale`。
pub fn parse_c(text: &[u8], max_scale: u8) -> Result<Fixed, u32> {
    if max_scale > SCALE_MAX {
        return Err(NM_ESCALE);
    }
    if text.is_empty() {
        return Err(NM_EDIGIT);
    }
    let mut i = 0usize;
    let mut neg = false;
    let mut digits = 0usize;
    let mut units: i64 = 0;
    let mut frac: u8 = 0;
    let mut seen_point = false;
    while i < text.len() {
        let b = match text.get(i) {
            Some(v) => *v,
            None => return Err(NM_EDIGIT),
        };
        if b == b'+' || b == b'-' {
            if i == 0 {
                neg = b == b'-';
                i += 1;
                continue;
            }
            return Err(NM_ESIGN);
        }
        if b == C_POINT {
            if seen_point {
                return Err(NM_EPOINT);
            }
            seen_point = true;
            i += 1;
            continue;
        }
        if b >= b'0' && b <= b'9' {
            let d = (b - b'0') as i64;
            units = match units.checked_mul(10).and_then(|v| v.checked_add(d)) {
                Some(v) => v,
                None => return Err(NM_ERANGE),
            };
            if seen_point {
                frac = match frac.checked_add(1) {
                    Some(v) => v,
                    None => return Err(NM_ESCALE),
                };
                if frac > max_scale {
                    return Err(NM_ESCALE);
                }
            }
            digits += 1;
            i += 1;
            continue;
        }
        if b == b',' || b == b' ' || b == b'\'' || b == b'_' {
            return Err(NM_ESEP);
        }
        return Err(NM_EDIGIT);
    }
    if digits == 0 {
        return Err(NM_EDIGIT);
    }
    if neg {
        units = units.saturating_neg();
    }
    Ok(Fixed { units, scale: frac })
}

/// 比较两个定点数（先对齐到公共小数位）。
pub fn compare(a: Fixed, b: Fixed) -> Result<i32, u32> {
    let target = if a.scale > b.scale { a.scale } else { b.scale };
    let ua = round_units(a, target)?;
    let ub = round_units(b, target)?;
    if ua < ub {
        Ok(-1)
    } else if ua > ub {
        Ok(1)
    } else {
        Ok(0)
    }
}

// ---------------------------------------------------------------------------
// locale
// ---------------------------------------------------------------------------

/// 数值 locale。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumLocale {
    /// `C`。
    C,
    /// `POSIX`。
    Posix,
    /// `C.UTF-8`。
    CUtf8,
    /// `de_DE.UTF-8`：小数点 `,`，分组 `.`。
    De,
    /// `fr_FR.UTF-8`：小数点 `,`，分组窄空格。
    Fr,
    /// `en_US.UTF-8`：小数点 `.`，分组 `,`。
    En,
}

impl NumLocale {
    /// 名字。
    pub fn name(&self) -> &'static str {
        match self {
            NumLocale::C => "C",
            NumLocale::Posix => "POSIX",
            NumLocale::CUtf8 => "C.UTF-8",
            NumLocale::De => "de_DE.UTF-8",
            NumLocale::Fr => "fr_FR.UTF-8",
            NumLocale::En => "en_US.UTF-8",
        }
    }

    /// 是否 C 家族。
    pub fn is_c(&self) -> bool {
        match self {
            NumLocale::C | NumLocale::Posix | NumLocale::CUtf8 => true,
            _ => false,
        }
    }

    /// 小数点字节。
    pub fn point(&self) -> u8 {
        match self {
            NumLocale::C | NumLocale::Posix | NumLocale::CUtf8 | NumLocale::En => C_POINT,
            NumLocale::De | NumLocale::Fr => b',',
        }
    }

    /// 分组符。
    pub fn group_sep(&self) -> &'static str {
        match self {
            NumLocale::C | NumLocale::Posix | NumLocale::CUtf8 => C_SEP,
            NumLocale::De => ".",
            NumLocale::Fr => "\u{202f}",
            NumLocale::En => ",",
        }
    }

    /// 分组宽度（0 表示不分页）。
    pub fn grouping(&self) -> u8 {
        match self {
            NumLocale::C | NumLocale::Posix | NumLocale::CUtf8 => C_GROUPING,
            _ => 3,
        }
    }

    /// 由名字解析。
    pub fn parse(name: &[u8]) -> Result<NumLocale, u32> {
        if eq_bytes(name, b"C") {
            Ok(NumLocale::C)
        } else if eq_bytes(name, b"POSIX") {
            Ok(NumLocale::Posix)
        } else if eq_bytes(name, b"C.UTF-8") {
            Ok(NumLocale::CUtf8)
        } else if eq_bytes(name, b"de_DE.UTF-8") || eq_bytes(name, b"de_DE") {
            Ok(NumLocale::De)
        } else if eq_bytes(name, b"fr_FR.UTF-8") || eq_bytes(name, b"fr_FR") {
            Ok(NumLocale::Fr)
        } else if eq_bytes(name, b"en_US.UTF-8") || eq_bytes(name, b"en_US") {
            Ok(NumLocale::En)
        } else {
            Err(NM_ELOCALE)
        }
    }
}

// ---------------------------------------------------------------------------
// 格式化
// ---------------------------------------------------------------------------

/// C 锚定格式化：非 C locale 一律拒绝，输出不含分组符。
pub fn format_c(v: Fixed, target: u8, loc: NumLocale) -> Result<String, u32> {
    if !loc.is_c() {
        return Err(NM_ELOCALE);
    }
    if target > SCALE_MAX {
        return Err(NM_ESCALE);
    }
    let units = round_units(v, target)?;
    Ok(render_units(units, target))
}

/// 按分组方案渲染一个非负整数部分。
pub fn push_grouped(
    out: &mut String,
    mag: u64,
    sep: &str,
    group: u8,
) -> Result<(), u32> {
    if group == 0 || group > 9 {
        return Err(NM_EGROUP);
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    let mut v = mag;
    if v == 0 {
        if let Some(d) = buf.get_mut(0) {
            *d = b'0';
        }
        n = 1;
    } else {
        while v > 0 && n < 20 {
            if let Some(d) = buf.get_mut(n) {
                *d = b'0' + (v % 10) as u8;
            }
            n += 1;
            v /= 10;
        }
    }
    let mut k = 0usize;
    while k < n {
        if k > 0 && !sep.is_empty() {
            let remaining = n - k;
            if remaining % (group as usize) == 0 {
                out.push_str(sep);
            }
        }
        if let Some(d) = buf.get(n - 1 - k) {
            out.push(*d as char);
        }
        k += 1;
    }
    Ok(())
}

/// 分组渲染一个整数（不含小数点），供参数校验使用。
pub fn render_grouped(mag: u64, sep: &str, group: u8) -> Result<String, u32> {
    let mut out = String::new();
    push_grouped(&mut out, mag, sep, group)?;
    Ok(out)
}

/// 外语格式化：小数点与分组均按 locale。
pub fn format_foreign(loc: NumLocale, v: Fixed, target: u8) -> Result<String, u32> {
    if target > SCALE_MAX {
        return Err(NM_ESCALE);
    }
    let units = round_units(v, target)?;
    if loc.is_c() {
        return Ok(render_units(units, target));
    }
    let neg = units < 0;
    let mag = if neg { units.saturating_neg() } else { units };
    let p = pow10_u64(target)?;
    let m = mag as u64;
    let ip = m / p;
    let fp = m % p;
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    push_grouped(&mut out, ip, loc.group_sep(), loc.grouping())?;
    if target > 0 {
        out.push(loc.point() as char);
        let mut div = p / 10;
        while div > 0 {
            let d = (fp / div) % 10;
            out.push((b'0' + d as u8) as char);
            div /= 10;
        }
    }
    Ok(out)
}

/// 按 locale 词法解析：先归一化成本地形状，再交给 C 解析。
pub fn parse_locale(loc: NumLocale, text: &[u8]) -> Result<Fixed, u32> {
    if loc.is_c() {
        return parse_c(text, SCALE_MAX);
    }
    let point = loc.point();
    let sep = loc.group_sep().as_bytes();
    let mut norm = String::new();
    let mut i = 0usize;
    while i < text.len() {
        let b = match text.get(i) {
            Some(v) => *v,
            None => return Err(NM_EDIGIT),
        };
        if b == point {
            norm.push(C_POINT as char);
            i += 1;
            continue;
        }
        if !sep.is_empty() && starts_with_at(text, i, sep) {
            i += sep.len();
            continue;
        }
        if b == b'+' || b == b'-' {
            norm.push(b as char);
            i += 1;
            continue;
        }
        if b >= b'0' && b <= b'9' {
            norm.push(b as char);
            i += 1;
            continue;
        }
        if b == b'.' || b == b',' || b == b' ' {
            return Err(NM_ESEP);
        }
        return Err(NM_EDIGIT);
    }
    parse_c(norm.as_bytes(), SCALE_MAX)
}

// ---------------------------------------------------------------------------
// 样本与账本
// ---------------------------------------------------------------------------

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NM_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        } else {
            return Err(NM_ESLOT);
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(NM_EBAL);
        }
        const VALS: [u32; 8] = [0, 42, 325, 75, 1235, 2, 0, 3];
        const CODES: [u32; 6] = [
            NM_ESEP,
            NM_EPOINT,
            NM_EDIGIT,
            NM_ESCALE,
            NM_ERANGE,
            NM_ELOCALE,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NM_EBAL),
            };
            if s.slot as usize != i {
                return Err(NM_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NM_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NM_EVALUE);
                        }
                    }
                    None => return Err(NM_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NM_EVALUE);
                        }
                    }
                    None => return Err(NM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(NM_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u64);
                h = fold_word(h, s.op as u64);
                h = fold_word(h, s.ok as u64);
                h = fold_word(h, s.code as u64);
                h = fold_word(h, s.value as u64);
            }
            i += 1;
        }
        fold_word(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(NM_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(NM_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

// ---------------------------------------------------------------------------
// 账本组装
// ---------------------------------------------------------------------------

/// `Result<u32, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 解析并把尾数绝对值作为观测值。
fn parse_mag(text: &[u8], max_scale: u8) -> Result<u32, u32> {
    parse_c(text, max_scale).map(|f| f.magnitude())
}

/// 槽 0：零。
fn slot0() -> Result<u32, u32> {
    parse_mag(b"0", 0)
}

/// 槽 1：整数。
fn slot1() -> Result<u32, u32> {
    parse_mag(b"42", 0)
}

/// 槽 2：两位小数。
fn slot2() -> Result<u32, u32> {
    parse_mag(b"3.25", 2)
}

/// 槽 3：负数绝对值。
fn slot3() -> Result<u32, u32> {
    parse_mag(b"-7.5", 1)
}

/// 槽 4：半值舍入。
fn slot4() -> Result<u32, u32> {
    match round_units(Fixed::new(12345, 3), 2) {
        Ok(u) => Ok(if u < 0 { u.saturating_neg() as u32 } else { u as u32 }),
        Err(c) => Err(c),
    }
}

/// 槽 5：en_US 分组符个数。
fn slot5() -> Result<u32, u32> {
    let s = format_foreign(NumLocale::En, Fixed::new(123456789, 2), 2)?;
    Ok(count_byte(s.as_bytes(), b',') as u32)
}

/// 槽 6：1.5 与 1.50 相等。
fn slot6() -> Result<u32, u32> {
    let a = parse_c(b"1.5", 2)?;
    let b = parse_c(b"1.50", 2)?;
    compare(a, b).map(|r| r as u32)
}

/// 槽 7：小数位计数。
fn slot7() -> Result<u32, u32> {
    parse_c(b"3.140", 3).map(|f| f.scale as u32)
}

/// 槽 8：C 下逗号即分组符。
fn slot8() -> Result<u32, u32> {
    parse_mag(b"12,345", 2)
}

/// 槽 9：第二个小数点。
fn slot9() -> Result<u32, u32> {
    parse_mag(b"1.2.3", 2)
}

/// 槽 10：只有小数点。
fn slot10() -> Result<u32, u32> {
    parse_mag(b".", 2)
}

/// 槽 11：小数位超限。
fn slot11() -> Result<u32, u32> {
    parse_mag(b"1.23456", 3)
}

/// 槽 12：尾数溢出。
fn slot12() -> Result<u32, u32> {
    parse_mag(b"999999999999999999999", 2)
}

/// 槽 13：非 C locale 格式化。
fn slot13() -> Result<u32, u32> {
    format_c(Fixed::zero(), 0, NumLocale::De).map(|_| 0)
}

/// 组装 14 格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(cell(0, OP_PARSE, slot0()));
    let _ = l.register(cell(1, OP_PARSE, slot1()));
    let _ = l.register(cell(2, OP_PARSE, slot2()));
    let _ = l.register(cell(3, OP_PARSE, slot3()));
    let _ = l.register(cell(4, OP_ROUND, slot4()));
    let _ = l.register(cell(5, OP_GROUP, slot5()));
    let _ = l.register(cell(6, OP_CMP, slot6()));
    let _ = l.register(cell(7, OP_SCALE, slot7()));
    let _ = l.register(cell(8, OP_PARSE, slot8()));
    let _ = l.register(cell(9, OP_PARSE, slot9()));
    let _ = l.register(cell(10, OP_PARSE, slot10()));
    let _ = l.register(cell(11, OP_SCALE, slot11()));
    let _ = l.register(cell(12, OP_PARSE, slot12()));
    let _ = l.register(cell(13, OP_LOCALE, slot13()));
    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 小数点锚定面。
fn anchor_ok() -> bool {
    let a = C_POINT == b'.';
    let b = NumLocale::C.point() == b'.';
    let d = format_c(Fixed::new(1234, 2), 2, NumLocale::C).map(|s| s == "12.34") == Ok(true);
    let e = format_c(Fixed::new(5, 0), 0, NumLocale::C).map(|s| s == "5") == Ok(true);
    let f = format_c(Fixed::new(5, 0), 2, NumLocale::C).map(|s| s == "5.00") == Ok(true);
    let g = format_c(Fixed::new(-1, 1), 1, NumLocale::Posix)
        .map(|s| s == "-0.1")
        == Ok(true);
    a && b && d && e && f && g
}

/// 无千分位锚定面。
fn no_group_ok() -> bool {
    let a = C_SEP.is_empty();
    let b = NumLocale::C.grouping() == 0;
    let d = format_c(Fixed::new(123456789, 2), 2, NumLocale::C)
        .map(|s| s == "1234567.89")
        == Ok(true);
    let e = format_c(Fixed::new(123456789, 2), 2, NumLocale::C)
        .map(|s| count_byte(s.as_bytes(), b',') == 0)
        == Ok(true);
    let f = format_c(Fixed::new(123456789, 2), 2, NumLocale::C)
        .map(|s| count_byte(s.as_bytes(), b'.') == 1)
        == Ok(true);
    let g = format_c(Fixed::new(10_000_000, 0), 0, NumLocale::C)
        .map(|s| s == "10000000")
        == Ok(true);
    a && b && d && e && f && g
}

/// 严格解析面。
fn parse_ok() -> bool {
    let a = parse_c(b"0", 0) == Ok(Fixed::new(0, 0));
    let b = parse_c(b"42", 0) == Ok(Fixed::new(42, 0));
    let d = parse_c(b"3.25", 2) == Ok(Fixed::new(325, 2));
    let e = parse_c(b"-7.5", 1) == Ok(Fixed::new(-75, 1));
    let f = parse_c(b"+5", 0) == Ok(Fixed::new(5, 0));
    let g = parse_c(b"12.", 0) == Ok(Fixed::new(12, 0));
    let h = parse_c(b"0.5", 1) == Ok(Fixed::new(5, 1));
    let i = parse_c(b"1", 0).map(|f| f.magnitude()) == Ok(1);
    a && b && d && e && f && g && h && i
}

/// 分隔符拒绝面。
fn sep_ok() -> bool {
    let a = parse_c(b"1,234", 2).err() == Some(NM_ESEP);
    let b = parse_c(b"1 234", 2).err() == Some(NM_ESEP);
    let d = parse_c(b"1'234", 2).err() == Some(NM_ESEP);
    let e = parse_c(b"1_234", 2).err() == Some(NM_ESEP);
    let f = parse_c(b"1.234,56", 3).err() == Some(NM_ESEP);
    let g = parse_c(b"1,0", 2).err() == Some(NM_ESEP);
    a && b && d && e && f && g
}

/// 小数点错误面。
fn point_ok() -> bool {
    let a = parse_c(b"1.2.3", 2).err() == Some(NM_EPOINT);
    let b = parse_c(b"1..2", 2).err() == Some(NM_EPOINT);
    let d = parse_c(b".", 2).err() == Some(NM_EDIGIT);
    let e = parse_c(b"", 2).err() == Some(NM_EDIGIT);
    let f = parse_c(b"+", 1).err() == Some(NM_EDIGIT);
    let g = parse_c(b"-", 1).err() == Some(NM_EDIGIT);
    let h = parse_c(b"..", 1).err() == Some(NM_EPOINT);
    a && b && d && e && f && g && h
}

/// 符号面。
fn sign_ok() -> bool {
    let a = parse_c(b"1-2", 2).err() == Some(NM_ESIGN);
    let b = parse_c(b"1+2", 2).err() == Some(NM_ESIGN);
    let d = parse_c(b"--1", 2).err() == Some(NM_ESIGN);
    let e = parse_c(b"+-1", 2).err() == Some(NM_ESIGN);
    let f = parse_c(b"-1.5", 1) == Ok(Fixed::new(-15, 1));
    let g = parse_c(b"+1.5", 1) == Ok(Fixed::new(15, 1));
    a && b && d && e && f && g
}

/// 小数位面。
fn scale_ok() -> bool {
    let a = parse_c(b"1.234", 2).err() == Some(NM_ESCALE);
    let b = parse_c(b"1.2", 7).err() == Some(NM_ESCALE);
    let d = format_c(Fixed::zero(), 7, NumLocale::C).err() == Some(NM_ESCALE);
    let e = parse_c(b"1.234", 3) == Ok(Fixed::new(1234, 3));
    let f = parse_c(b"3.140", 3).map(|v| v.scale) == Ok(3);
    let g = round_units(Fixed::new(1, 0), 7).err() == Some(NM_ESCALE);
    a && b && d && e && f && g
}

/// 溢出与分组参数面。
fn range_ok() -> bool {
    let a = parse_c(b"999999999999999999999", 2).err() == Some(NM_ERANGE);
    let b = parse_c(b"9223372036854775808", 0).err() == Some(NM_ERANGE);
    let d = round_units(Fixed::new(1_000_000_000_000_000_000, 0), 6).err() == Some(NM_ERANGE);
    let e = render_grouped(1234567, ",", 3).map(|s| s == "1,234,567") == Ok(true);
    let f = render_grouped(1234567, ",", 0).err() == Some(NM_EGROUP);
    let g = render_grouped(123, "", 3).map(|s| s == "123") == Ok(true);
    a && b && d && e && f && g
}

/// 舍入面。
fn round_ok() -> bool {
    let a = round_units(Fixed::new(12345, 3), 2) == Ok(1235);
    let b = round_units(Fixed::new(-12345, 3), 2) == Ok(-1235);
    let d = round_units(Fixed::new(12344, 3), 2) == Ok(1234);
    let e = round_units(Fixed::new(5, 1), 0) == Ok(1);
    let f = round_units(Fixed::new(-5, 1), 0) == Ok(-1);
    let g = round_units(Fixed::new(25, 1), 0) == Ok(3);
    let h = format_c(Fixed::new(12345, 3), 2, NumLocale::C)
        .map(|s| s == "12.35")
        == Ok(true);
    let i = format_c(Fixed::new(-12345, 3), 2, NumLocale::C)
        .map(|s| s == "-12.35")
        == Ok(true);
    a && b && d && e && f && g && h && i
}

/// 外语对照面。
fn foreign_ok() -> bool {
    let v = Fixed::new(123456789, 2);
    let a = format_foreign(NumLocale::De, v, 2).map(|s| s == "1.234.567,89") == Ok(true);
    let b = format_foreign(NumLocale::En, v, 2).map(|s| s == "1,234,567.89") == Ok(true);
    let d = format_foreign(NumLocale::Fr, v, 2)
        .map(|s| s == "1\u{202f}234\u{202f}567,89")
        == Ok(true);
    let e = parse_locale(NumLocale::De, b"1.234,56") == Ok(Fixed::new(123456, 2));
    let f = parse_locale(NumLocale::En, b"1,234.56") == Ok(Fixed::new(123456, 2));
    let g = parse_c(b"1.234,56", 3).err() == Some(NM_ESEP);
    let h = NumLocale::De.point() == b','
        && NumLocale::Fr.point() == b','
        && NumLocale::En.point() == b'.';
    a && b && d && e && f && g && h
}

/// 账本闭环。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != 8 || l.err_count() != 6 {
        return false;
    }
    if l.is_empty() {
        return false;
    }
    let s1 = l.seal();
    if s1 != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root != s1 || !l2.is_frozen() || l2.root() != root {
        return false;
    }
    let replay = match l2.replay() {
        Ok(v) => v == root,
        Err(_) => false,
    };
    if !replay {
        return false;
    }
    if l2.register(Sample::blank()) != Err(NM_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 冻结面。
fn freeze_ok() -> bool {
    let mut l = build_ledger();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = l.seal() == root;
    let b = l.replay() == Ok(root);
    let d = l.freeze().err() == Some(NM_EBAL);
    let e = l.rejects() == 1;
    a && b && d && e
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = anchor_ok() && no_group_ok();
    out[1] = parse_ok() && sep_ok();
    out[2] = point_ok() && sign_ok() && scale_ok() && range_ok();
    out[3] = round_ok() && foreign_ok() && ledger_ok() && freeze_ok();
    out
}

/// 判据集合。
pub fn run_unx11578_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11578.numeric");

    s.add("NM01-小数点锚定", anchor_ok(), "static");
    s.add("NM02-无千分位", no_group_ok(), "static");
    s.add("NM03-严格解析", parse_ok(), "static");
    s.add("NM04-分隔符拒绝", sep_ok(), "static");
    s.add("NM05-小数点错误", point_ok(), "static");
    s.add("NM06-符号位置", sign_ok(), "static");
    s.add("NM07-小数位上限", scale_ok(), "static");
    s.add("NM08-溢出与分组", range_ok(), "static");
    s.add("NM09-定点舍入", round_ok(), "static");
    s.add("NM10-外语对照", foreign_ok(), "static");
    s.add("NM11-账本对账", ledger_ok(), "static");
    s.add("NM12-冻结指纹", freeze_ok(), "static");
    s.add("NM13-冻结重放四注入", inject_four() == [true, true, true, true], "static");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_is_dot() {
        assert_eq!(C_POINT, b'.');
        assert_eq!(NumLocale::C.point(), b'.');
        assert_eq!(NumLocale::Posix.point(), b'.');
        assert_eq!(NumLocale::CUtf8.point(), b'.');
        assert_eq!(NumLocale::De.point(), b',');
        assert_eq!(NumLocale::Fr.point(), b',');
        assert_eq!(NumLocale::En.point(), b'.');
    }

    #[test]
    fn no_grouping_in_c() {
        assert_eq!(C_SEP, "");
        assert_eq!(C_GROUPING, 0);
        assert_eq!(NumLocale::C.grouping(), 0);
        assert_eq!(NumLocale::C.group_sep(), "");
        let s = format_c(Fixed::new(123456789, 2), 2, NumLocale::C).expect("f");
        assert_eq!(s, "1234567.89");
        assert_eq!(count_byte(s.as_bytes(), b','), 0);
        assert_eq!(count_byte(s.as_bytes(), b'.'), 1);
    }

    #[test]
    fn parse_plain() {
        assert_eq!(parse_c(b"0", 0), Ok(Fixed::new(0, 0)));
        assert_eq!(parse_c(b"42", 0), Ok(Fixed::new(42, 0)));
        assert_eq!(parse_c(b"3.25", 2), Ok(Fixed::new(325, 2)));
        assert_eq!(parse_c(b"-7.5", 1), Ok(Fixed::new(-75, 1)));
        assert_eq!(parse_c(b"+5", 0), Ok(Fixed::new(5, 0)));
        assert_eq!(parse_c(b"12.", 0), Ok(Fixed::new(12, 0)));
        assert_eq!(parse_c(b"007", 0), Ok(Fixed::new(7, 0)));
    }

    #[test]
    fn parse_rejects_grouping() {
        assert_eq!(parse_c(b"1,234", 2).err(), Some(NM_ESEP));
        assert_eq!(parse_c(b"1 234", 2).err(), Some(NM_ESEP));
        assert_eq!(parse_c(b"1'234", 2).err(), Some(NM_ESEP));
        assert_eq!(parse_c(b"1_234", 2).err(), Some(NM_ESEP));
        assert_eq!(parse_c(b"1.234,56", 3).err(), Some(NM_ESEP));
        assert_eq!(parse_c(b"1.2,3", 2).err(), Some(NM_ESEP));
    }

    #[test]
    fn parse_rejects_points() {
        assert_eq!(parse_c(b"1.2.3", 2).err(), Some(NM_EPOINT));
        assert_eq!(parse_c(b"1..2", 2).err(), Some(NM_EPOINT));
        assert_eq!(parse_c(b".", 2).err(), Some(NM_EDIGIT));
        assert_eq!(parse_c(b"", 2).err(), Some(NM_EDIGIT));
        assert_eq!(parse_c(b"..", 2).err(), Some(NM_EPOINT));
        assert_eq!(parse_c(b"+", 2).err(), Some(NM_EDIGIT));
    }

    #[test]
    fn parse_rejects_signs() {
        assert_eq!(parse_c(b"1-2", 2).err(), Some(NM_ESIGN));
        assert_eq!(parse_c(b"1+2", 2).err(), Some(NM_ESIGN));
        assert_eq!(parse_c(b"--1", 2).err(), Some(NM_ESIGN));
        assert_eq!(parse_c(b"+-1", 2).err(), Some(NM_ESIGN));
    }

    #[test]
    fn parse_scale_limits() {
        assert_eq!(parse_c(b"1.234", 2).err(), Some(NM_ESCALE));
        assert_eq!(parse_c(b"1.234", 3), Ok(Fixed::new(1234, 3)));
        assert_eq!(parse_c(b"1.2", 7).err(), Some(NM_ESCALE));
        assert_eq!(parse_c(b"3.140", 3).map(|v| v.scale), Ok(3));
    }

    #[test]
    fn parse_overflow() {
        assert_eq!(parse_c(b"999999999999999999999", 2).err(), Some(NM_ERANGE));
        assert_eq!(parse_c(b"9223372036854775808", 0).err(), Some(NM_ERANGE));
        assert_eq!(parse_c(b"9223372036854775807", 0).map(|v| v.units), Ok(i64::MAX));
    }

    #[test]
    fn rounding_half_away() {
        assert_eq!(round_units(Fixed::new(12345, 3), 2), Ok(1235));
        assert_eq!(round_units(Fixed::new(-12345, 3), 2), Ok(-1235));
        assert_eq!(round_units(Fixed::new(12344, 3), 2), Ok(1234));
        assert_eq!(round_units(Fixed::new(5, 1), 0), Ok(1));
        assert_eq!(round_units(Fixed::new(-5, 1), 0), Ok(-1));
        assert_eq!(round_units(Fixed::new(5, 0), 2), Ok(500));
    }

    #[test]
    fn render_shapes() {
        assert_eq!(render_units(0, 0), "0");
        assert_eq!(render_units(0, 2), "0.00");
        assert_eq!(render_units(-5, 1), "-0.5");
        assert_eq!(render_units(123456789, 2), "1234567.89");
        assert_eq!(render_units(7, 6), "0.000007");
    }

    #[test]
    fn format_c_anchor() {
        assert_eq!(
            format_c(Fixed::new(1234, 2), 2, NumLocale::C).map(|s| s == "12.34"),
            Ok(true)
        );
        assert_eq!(
            format_c(Fixed::new(1234, 2), 2, NumLocale::CUtf8).map(|s| s == "12.34"),
            Ok(true)
        );
        assert_eq!(format_c(Fixed::zero(), 0, NumLocale::De).err(), Some(NM_ELOCALE));
        assert_eq!(format_c(Fixed::zero(), 0, NumLocale::Fr).err(), Some(NM_ELOCALE));
        assert_eq!(format_c(Fixed::zero(), 0, NumLocale::En).err(), Some(NM_ELOCALE));
        assert_eq!(format_c(Fixed::zero(), 7, NumLocale::C).err(), Some(NM_ESCALE));
    }

    #[test]
    fn grouping_shape() {
        assert_eq!(render_grouped(1234567, ",", 3).map(|s| s == "1,234,567"), Ok(true));
        assert_eq!(render_grouped(123, ",", 3).map(|s| s == "123"), Ok(true));
        assert_eq!(render_grouped(1000, ".", 3).map(|s| s == "1.000"), Ok(true));
        assert_eq!(render_grouped(1234567, ",", 0).err(), Some(NM_EGROUP));
        assert_eq!(render_grouped(1234567, ",", 10).err(), Some(NM_EGROUP));
    }

    #[test]
    fn foreign_renders() {
        let v = Fixed::new(123456789, 2);
        assert_eq!(
            format_foreign(NumLocale::De, v, 2).map(|s| s == "1.234.567,89"),
            Ok(true)
        );
        assert_eq!(
            format_foreign(NumLocale::En, v, 2).map(|s| s == "1,234,567.89"),
            Ok(true)
        );
        assert_eq!(
            format_foreign(NumLocale::Fr, v, 2).map(|s| s == "1\u{202f}234\u{202f}567,89"),
            Ok(true)
        );
        assert_eq!(
            format_foreign(NumLocale::C, v, 2).map(|s| s == "1234567.89"),
            Ok(true)
        );
    }

    #[test]
    fn locale_parsing() {
        assert_eq!(NumLocale::parse(b"C"), Ok(NumLocale::C));
        assert_eq!(NumLocale::parse(b"POSIX"), Ok(NumLocale::Posix));
        assert_eq!(NumLocale::parse(b"C.UTF-8"), Ok(NumLocale::CUtf8));
        assert_eq!(NumLocale::parse(b"de_DE.UTF-8"), Ok(NumLocale::De));
        assert_eq!(NumLocale::parse(b"fr_FR"), Ok(NumLocale::Fr));
        assert_eq!(NumLocale::parse(b"en_US.UTF-8"), Ok(NumLocale::En));
        assert_eq!(NumLocale::parse(b"ru_RU.UTF-8").err(), Some(NM_ELOCALE));
        assert!(NumLocale::C.is_c());
        assert!(!NumLocale::De.is_c());
    }

    #[test]
    fn locale_lexing() {
        assert_eq!(parse_locale(NumLocale::De, b"1.234,56"), Ok(Fixed::new(123456, 2)));
        assert_eq!(parse_locale(NumLocale::En, b"1,234.56"), Ok(Fixed::new(123456, 2)));
        assert_eq!(
            parse_locale(NumLocale::Fr, b"1\xe2\x80\xaf234,56"),
            Ok(Fixed::new(123456, 2))
        );
        assert_eq!(parse_locale(NumLocale::De, b"12,5"), Ok(Fixed::new(125, 1)));
        assert_eq!(parse_locale(NumLocale::De, b"1,2;3").err(), Some(NM_EDIGIT));
        assert_eq!(parse_locale(NumLocale::C, b"1.5"), Ok(Fixed::new(15, 1)));
    }

    #[test]
    fn comparisons() {
        let a = parse_c(b"1.5", 2).expect("a");
        let b = parse_c(b"1.50", 2).expect("b");
        assert_eq!(compare(a, b), Ok(0));
        let c = parse_c(b"1.51", 2).expect("c");
        assert_eq!(compare(a, c), Ok(-1));
        assert_eq!(compare(c, a), Ok(1));
    }

    #[test]
    fn ledger_balance() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(4).map(|s| s.value), Some(1235));
        assert_eq!(l.sample(8).map(|s| s.code), Some(NM_ESEP));
        assert_eq!(l.sample(13).map(|s| s.code), Some(NM_ELOCALE));
        assert!(l.sample(14).is_none());
    }

    #[test]
    fn ledger_freeze() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(root, build_ledger().seal());
        assert_eq!(l.freeze(), Ok(root));
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze().err(), Some(NM_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn ledger_slots() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        let wrong = Sample { slot: 2, op: OP_PARSE, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(wrong).err(), Some(NM_ESLOT));
        let good = Sample { slot: 0, op: OP_PARSE, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(good), Ok(()));
        assert_eq!(l.register(good).err(), Some(NM_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11578_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
