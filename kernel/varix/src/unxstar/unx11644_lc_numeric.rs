//! UNX-F11644 LC_NUMERIC 坏值 printf 小数点回退判据
//!
//! `printf("%f", 1234.5)` 输出什么，取决于 `LC_NUMERIC` 一档：C/POSIX 与
//! 英文档用 `.` 作小数点、不加千位分隔；`de_DE.UTF-8` 用 `,` 作小数点、
//! `.` 作千位分隔；`fr_FR.UTF-8` 用 `,` 加点、窄空格分位。`printf` 走的是
//! `localeconv()->decimal_point`，也就是说**小数点字符是运行期从 locale
//! 取出来的**，不是编译期常量。
//!
//! 坏值路径是本卡的核心：`LC_NUMERIC=xx_YY.INVALID` 时 `setlocale` 失败，
//! 数值档留在 "C"，`printf` 继续用 `.` 且**不做分位**——于是同一份程序在
//! 脏环境下输出 `1234.500000`，在干净环境下输出 `1.234,500000`。这个
//! 差异会直接破坏下游按 `.` 切分的解析器（CSV、日志抓取、金额对账）。
//!
//! 定点化建模：所有值先换成「微单位」（×1e6 的整数），格式化时按请求的
//! 小数位数在微单位上做半进位（round-half-up），再截断——**先舍入后截断**
//! 的顺序不能反，否则 `999.999999` 在小数两位时进位不进整数部分。
//! 分位只作用于整数段，从右往左每三位插一个分隔符，首位不足三位不补。
//!
//! 解析（strtod 侧）与格式化共用同一份数值档：小数点/分位符随档切换，
//! 双小数点、分位错位、非法字符、溢出四类错误各自独立成码——把"小数点
//! 写错"报成"溢出"是这类问题最常见的方向性误诊。
//!
//! 闸门次序：字节 → 数值档 → 解析 → 舍入 → 分位 → 渲染 → 回退 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfd30;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 微单位刻度。
pub const MICRO: u64 = 1_000_000;

/// 默认小数位数（`%f`）。
pub const DEFAULT_DIGITS: usize = 6;

/// 数值档条目。
pub const LOCALES: usize = 7;

/// 值缓冲容量。
pub const VAL_CAP: usize = 32;

/// 整数段最多位数。
pub const INT_MAX: usize = 15;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 非法字符。
pub const LN_EDIGIT: u32 = SEG_BASE + 0x01;
/// 小数点重复。
pub const LN_ERADIX: u32 = SEG_BASE + 0x02;
/// 分位错位。
pub const LN_EGROUP: u32 = SEG_BASE + 0x03;
/// 数值档坏值（回退 C）。
pub const LN_ELOCALE: u32 = SEG_BASE + 0x04;
/// 状态冲突。
pub const LN_ESTATE: u32 = SEG_BASE + 0x05;
/// 溢出。
pub const LN_EOVER: u32 = SEG_BASE + 0x06;
/// 字节非法。
pub const LN_EBYTE: u32 = SEG_BASE + 0x07;
/// 语料重复。
pub const LN_EDUP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const LN_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const LN_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const LN_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const LN_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const LN_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 数值档。
pub const OP_LOCALE: u8 = 0x01;
/// 解析。
pub const OP_PARSE: u8 = 0x02;
/// 舍入。
pub const OP_ROUND: u8 = 0x03;
/// 分位。
pub const OP_GROUP: u8 = 0x04;
/// 渲染。
pub const OP_RENDER: u8 = 0x05;
/// 语料全跑。
pub const OP_CORPUS: u8 = 0x06;
/// 绑定闸。
pub const OP_BIND: u8 = 0x07;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字（大端序逐字节）。
fn fold_word(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut shift = 56u32;
    loop {
        v = fold_byte(v, (((w >> shift) & 0xff) as u32) as u8);
        if shift == 0 {
            break;
        }
        shift -= 8;
    }
    v
}

/// 字节串摘要。
fn digest(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFF;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 文本
// ---------------------------------------------------------------------------

/// 十进制文本（u32）。
fn u32_text(v: u32) -> String {
    let mut buf = [0u8; 10];
    let mut n = v;
    let mut i = 10usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + ((n % 10) as u8);
        n /= 10;
    }
    let mut s = String::new();
    while i < 10 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 十进制文本（u64）。
fn u64_text(v: u64) -> String {
    let mut buf = [0u8; 20];
    let mut n = v;
    let mut i = 20usize;
    if n == 0 {
        return String::from("0");
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + ((n % 10) as u8);
        n /= 10;
    }
    let mut s = String::new();
    while i < 20 {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 四字符小写十六进制文本。
fn hex4_text(v: u16) -> String {
    let mut s = String::new();
    let mut shift = 12u32;
    loop {
        let nib = ((v >> shift) & 0xf) as u8;
        let c = if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) };
        s.push(c as char);
        if shift == 0 {
            break;
        }
        shift -= 4;
    }
    s
}

/// 10 的幂。
fn pow10(n: u32) -> u64 {
    let mut v = 1u64;
    let mut i = 0u32;
    while i < n {
        v = v.saturating_mul(10);
        i += 1;
    }
    v
}

/// 给整数串插分位符（从右每三位一个）。
fn group_digits(digits: &str, sep: char) -> String {
    let n = digits.len();
    let mut s = String::new();
    let mut i = 0usize;
    while i < n {
        if i > 0 && (n - i) % 3 == 0 {
            s.push(sep);
        }
        if let Some(b) = digits.as_bytes().get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s
}

/// 用指定字符把小数段补足到 `digits` 位。
fn pad_frac(frac: &str, digits: usize) -> String {
    let mut s = String::new();
    let mut pad = digits.saturating_sub(frac.len());
    while pad > 0 {
        s.push('0');
        pad -= 1;
    }
    s.push_str(frac);
    s
}

// ---------------------------------------------------------------------------
// 数值档
// ---------------------------------------------------------------------------

/// 一档数值格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Numeric {
    /// 表下标。
    pub idx: usize,
    /// 档名。
    pub name: &'static str,
    /// 小数点。
    pub point: char,
    /// 分位符。
    pub sep: char,
    /// 是否分位。
    pub grouping: bool,
}

impl Numeric {
    /// 构造。
    pub const fn new(
        idx: usize,
        name: &'static str,
        point: char,
        sep: char,
        grouping: bool,
    ) -> Numeric {
        Numeric { idx, name, point, sep, grouping }
    }

    /// 小数点文本名。
    pub fn radix_name(&self) -> &'static str {
        if self.point == ',' {
            "comma"
        } else {
            "point"
        }
    }

    /// 是否 C 档。
    pub fn is_c(&self) -> bool {
        self.idx <= 2
    }
}

/// C 档（回退锚点）。
pub const C_NUMERIC: Numeric = Numeric::new(0, "C", '.', '\0', false);

/// 七档数值表。
pub const TABLE: [Numeric; LOCALES] = [
    Numeric::new(0, "C", '.', '\0', false),
    Numeric::new(1, "POSIX", '.', '\0', false),
    Numeric::new(2, "C.UTF-8", '.', '\0', false),
    Numeric::new(3, "en_US.UTF-8", '.', ',', true),
    Numeric::new(4, "de_DE.UTF-8", ',', '.', true),
    Numeric::new(5, "fr_FR.UTF-8", ',', ' ', true),
    Numeric::new(6, "ja_JP.UTF-8", '.', ',', true),
];

/// 表内取值。
pub fn table_at(i: usize) -> Option<Numeric> {
    TABLE.get(i).copied()
}

/// 按名查档。
pub fn table_find(name: &str) -> Option<Numeric> {
    let mut i = 0usize;
    while i < TABLE.len() {
        if let Some(n) = TABLE.get(i) {
            if n.name == name {
                return Some(*n);
            }
        }
        i += 1;
    }
    None
}

/// 值字节校验。
pub fn check_value(src: &str) -> Result<(), u32> {
    let b = src.as_bytes();
    if b.is_empty() {
        return Err(LN_EBYTE);
    }
    if b.len() > VAL_CAP {
        return Err(LN_EBYTE);
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(LN_EBYTE),
        };
        if c < 0x21 || c > 0x7e {
            return Err(LN_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

/// 严格解析数值档：坏值给 LN_ELOCALE。
pub fn resolve(src: &str) -> Result<Numeric, u32> {
    check_value(src)?;
    match table_find(src) {
        Some(n) => Ok(n),
        None => Err(LN_ELOCALE),
    }
}

/// 宽松取档：坏值回退 C，并回报是否回退。
pub fn pick(src: &str) -> (Numeric, bool) {
    match resolve(src) {
        Ok(n) => (n, false),
        Err(_) => (C_NUMERIC, true),
    }
}

// ---------------------------------------------------------------------------
// 解析（strtod 侧）
// ---------------------------------------------------------------------------

/// 解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parsed {
    /// 微单位（无符号）。
    pub units: u64,
    /// 符号。
    pub neg: bool,
    /// 小数位数。
    pub digits: usize,
}

/// 按数值档解析十进制串。
pub fn parse(text: &str, loc: &Numeric) -> Result<Parsed, u32> {
    let b = text.as_bytes();
    if b.is_empty() {
        return Err(LN_EDIGIT);
    }
    let mut i = 0usize;
    let mut neg = false;
    if let Some(c) = b.first() {
        if *c == b'-' {
            neg = true;
            i = 1;
        } else if *c == b'+' {
            i = 1;
        }
    }
    if i >= b.len() {
        return Err(LN_EDIGIT);
    }

    let mut digits = [0u8; 24];
    let mut nd = 0usize;
    let mut frac_start = 0usize;
    let mut seen_point = false;
    let mut int_digits = 0usize;
    let mut frac_digits = 0usize;

    // 整数段分组校验用的小数组。
    let mut parts = [0u8; 8];
    let mut np = 0usize;
    let mut cur = 0u8;
    let mut saw_sep = false;

    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(LN_EBYTE),
        };
        if c == loc.point as u8 {
            if seen_point {
                return Err(LN_ERADIX);
            }
            if int_digits == 0 {
                return Err(LN_EDIGIT);
            }
            if loc.grouping && saw_sep {
                if cur == 0 {
                    return Err(LN_EGROUP);
                }
                if np >= parts.len() {
                    return Err(LN_EOVER);
                }
                parts[np] = cur;
                np += 1;
            }
            seen_point = true;
            frac_start = nd;
        } else if loc.grouping && loc.sep != '\0' && c == loc.sep as u8 {
            if seen_point {
                return Err(LN_EDIGIT);
            }
            if cur == 0 {
                return Err(LN_EGROUP);
            }
            if np >= parts.len() {
                return Err(LN_EOVER);
            }
            parts[np] = cur;
            np += 1;
            cur = 0;
            saw_sep = true;
        } else if c >= b'0' && c <= b'9' {
            if nd >= digits.len() {
                return Err(LN_EOVER);
            }
            digits[nd] = c;
            nd += 1;
            if seen_point {
                frac_digits += 1;
                if frac_digits > DEFAULT_DIGITS {
                    return Err(LN_EOVER);
                }
            } else {
                int_digits += 1;
                if int_digits > INT_MAX {
                    return Err(LN_EOVER);
                }
                cur = cur.saturating_add(1);
            }
        } else {
            return Err(LN_EDIGIT);
        }
        i += 1;
    }

    if int_digits == 0 {
        return Err(LN_EDIGIT);
    }
    if !seen_point {
        frac_start = nd;
    }
    if loc.grouping && saw_sep {
        if !seen_point {
            if cur == 0 {
                return Err(LN_EGROUP);
            }
            if np >= parts.len() {
                return Err(LN_EOVER);
            }
            parts[np] = cur;
            np += 1;
        }
        // 最靠近小数点的一组必须是三位；中间各组三位；首组 1..=3。
        let mut k = 1usize;
        while k < np {
            match parts.get(k) {
                Some(w) => {
                    if *w != 3 {
                        return Err(LN_EGROUP);
                    }
                }
                None => return Err(LN_EGROUP),
            }
            k += 1;
        }
        match parts.first() {
            Some(w) => {
                if *w == 0 || *w > 3 {
                    return Err(LN_EGROUP);
                }
            }
            None => return Err(LN_EGROUP),
        }
    }

    let mut int_v: u64 = 0;
    let mut k = 0usize;
    while k < frac_start {
        let d = match digits.get(k) {
            Some(v) => *v,
            None => return Err(LN_EOVER),
        };
        let dv = (d - b'0') as u64;
        int_v = match int_v.checked_mul(10) {
            Some(v) => v,
            None => return Err(LN_EOVER),
        };
        int_v = match int_v.checked_add(dv) {
            Some(v) => v,
            None => return Err(LN_EOVER),
        };
        k += 1;
    }
    let mut frac_v: u64 = 0;
    let frac_count = nd - frac_start;
    k = frac_start;
    while k < nd {
        let d = match digits.get(k) {
            Some(v) => *v,
            None => return Err(LN_EOVER),
        };
        frac_v = frac_v.saturating_mul(10).saturating_add((d - b'0') as u64);
        k += 1;
    }
    let mut scale = DEFAULT_DIGITS.saturating_sub(frac_count) as u32;
    while scale > 0 {
        frac_v = frac_v.saturating_mul(10);
        scale -= 1;
    }
    let base = match int_v.checked_mul(MICRO) {
        Some(v) => v,
        None => return Err(LN_EOVER),
    };
    let units = match base.checked_add(frac_v) {
        Some(v) => v,
        None => return Err(LN_EOVER),
    };
    Ok(Parsed { units, neg, digits: frac_count })
}

// ---------------------------------------------------------------------------
// 定点格式化
// ---------------------------------------------------------------------------

/// 舍入到指定小数位（半进位），返回微单位。
pub fn round_units(units: u64, digits: usize) -> Result<u64, u32> {
    if digits > DEFAULT_DIGITS {
        return Err(LN_EDIGIT);
    }
    let scale = pow10((DEFAULT_DIGITS - digits) as u32);
    if scale <= 1 {
        return Ok(units);
    }
    let rem = units % scale;
    let half = scale / 2;
    let mut u = units;
    if rem >= half {
        u = u.saturating_add(scale - rem);
    }
    u -= u % scale;
    Ok(u)
}

/// 格式化微单位。
pub fn format_units(units: u64, neg: bool, loc: &Numeric, digits: usize) -> Result<String, u32> {
    if digits == 0 || digits > DEFAULT_DIGITS {
        return Err(LN_EDIGIT);
    }
    let u = round_units(units, digits)?;
    let ip = u / MICRO;
    let frac = (u % MICRO) / pow10((DEFAULT_DIGITS - digits) as u32);
    let ip_text = u64_text(ip);
    let mut s = String::new();
    let zero = ip == 0 && frac == 0;
    if neg && !zero {
        s.push('-');
    }
    if loc.grouping {
        s.push_str(group_digits(ip_text.as_str(), loc.sep).as_str());
    } else {
        s.push_str(ip_text.as_str());
    }
    s.push(loc.point);
    let frac_text = u64_text(frac);
    s.push_str(pad_frac(frac_text.as_str(), digits).as_str());
    Ok(s)
}

/// 按 locale 值格式化（坏值回退 C）。
pub fn format_locale(
    value: &str,
    units: u64,
    neg: bool,
    digits: usize,
) -> Result<String, u32> {
    let (loc, _) = pick(value);
    format_units(units, neg, &loc, digits)
}

/// 解析 → 格式化。
pub fn round_trip(text: &str, value: &str, digits: usize) -> Result<String, u32> {
    let (loc, _) = pick(value);
    let p = parse(text, &loc)?;
    format_units(p.units, p.neg, &loc, digits)
}

// ---------------------------------------------------------------------------
// 绑定闸
// ---------------------------------------------------------------------------

/// 一次 printf 只许绑一次数值档。
#[derive(Clone, Copy, Debug)]
pub struct Binder {
    bound: bool,
    idx: usize,
    last: u32,
}

impl Binder {
    /// 空。
    pub const fn new() -> Binder {
        Binder { bound: false, idx: 0, last: 0 }
    }

    /// 是否已绑。
    pub fn bound(&self) -> bool {
        self.bound
    }

    /// 绑定下标。
    pub fn idx(&self) -> usize {
        self.idx
    }

    /// 最后错误码。
    pub fn last(&self) -> u32 {
        self.last
    }

    /// 绑定；二次绑定即状态冲突。
    pub fn bind(&mut self, value: &str) -> Result<Numeric, u32> {
        if self.bound {
            self.last = LN_ESTATE;
            return Err(LN_ESTATE);
        }
        let n = match resolve(value) {
            Ok(v) => v,
            Err(c) => {
                // 坏值不占绑定：回退 C 并记一次档错误。
                self.last = c;
                return Err(c);
            }
        };
        self.bound = true;
        self.idx = n.idx;
        self.last = 0;
        Ok(n)
    }

    /// 已绑档名。
    pub fn name(&self) -> Option<&'static str> {
        if !self.bound {
            return None;
        }
        table_at(self.idx).map(|n| n.name)
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// LC_NUMERIC 值。
    pub locale: &'static str,
    /// 微单位。
    pub units: u64,
    /// 符号。
    pub neg: bool,
    /// 小数位。
    pub digits: usize,
    /// 期望输出。
    pub want: &'static str,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(
        locale: &'static str,
        units: u64,
        neg: bool,
        digits: usize,
        want: &'static str,
        note: &'static str,
    ) -> Case {
        Case { locale, units, neg, digits, want, note }
    }
}

/// 十四槽。
pub const CASES: [Case; CORPUS] = [
    Case::new("de_DE.UTF-8", 1_234_500_000, false, 6, "1.234,500000", "de-group"),
    Case::new("de_DE.UTF-8", 250_000, false, 3, "0,250", "de-third-digit"),
    Case::new("de_DE.UTF-8", 1_234_567_891_000, true, 6, "-1.234.567,891000", "de-negative"),
    Case::new("de_DE.UTF-8", 999_999_999, false, 2, "1.000,00", "de-carry"),
    Case::new("de_DE.UTF-8", 1_000_000_000, false, 6, "1.000,000000", "de-thousand"),
    Case::new("en_US.UTF-8", 1_234_500_000, false, 6, "1,234.500000", "us-group"),
    Case::new("en_US.UTF-8", 9_876_543_210_000, true, 6, "-9,876,543.210000", "us-negative"),
    Case::new("en_US.UTF-8", 12_000_000, false, 2, "12.00", "us-two-digits"),
    Case::new("C", 1_234_500_000, false, 6, "1234.500000", "c-no-group"),
    Case::new("C.UTF-8", 500_000, true, 6, "-0.500000", "c-fraction-only"),
    Case::new("ja_JP.UTF-8", 999_999_125_000, false, 6, "999,999.125000", "ja-group"),
    Case::new("xx_YY.UTF-8", 1_234_500_000, false, 6, "1234.500000", "bad-fallback"),
    Case::new("xx_YY.UTF-8", 1_234_500_000, true, 6, "-1234.500000", "bad-fallback-neg"),
    Case::new("fr_FR.UTF-8", 1_234_500_000, false, 6, "1 234,500000", "fr-space-group"),
];

/// 语料槽。
pub fn case_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 语料是否互异（按四元组）。
pub fn unique() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        let a = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = i + 1;
        while j < CASES.len() {
            if let Some(b) = CASES.get(j) {
                if a.locale == b.locale
                    && a.units == b.units
                    && a.neg == b.neg
                    && a.digits == b.digits
                {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 备注全非空。
pub fn notes_ok() -> bool {
    let mut i = 0usize;
    while i < CASES.len() {
        match CASES.get(i) {
            Some(c) => {
                if c.note.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 逐槽对判。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        match format_locale(c.locale, c.units, c.neg, c.digits) {
            Ok(s) => {
                if s.as_str() != c.want {
                    return false;
                }
            }
            Err(_) => return false,
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug)]
pub struct Census {
    total: usize,
    comma: usize,
    point: usize,
    grouped: usize,
    fallback: usize,
    negative: usize,
    neg_grouped: usize,
    matched: usize,
    red: bool,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census {
            total: 0,
            comma: 0,
            point: 0,
            grouped: 0,
            fallback: 0,
            negative: 0,
            neg_grouped: 0,
            matched: 0,
            red: false,
        }
    }

    /// 总数。
    pub const fn total(&self) -> usize {
        self.total
    }

    /// 逗号小数点格数。
    pub const fn comma(&self) -> usize {
        self.comma
    }

    /// 点小数点格数。
    pub const fn point(&self) -> usize {
        self.point
    }

    /// 分位格数。
    pub const fn grouped(&self) -> usize {
        self.grouped
    }

    /// 回退格数。
    pub const fn fallback(&self) -> usize {
        self.fallback
    }

    /// 负数格数。
    pub const fn negative(&self) -> usize {
        self.negative
    }

    /// 负且分位格数。
    pub const fn neg_grouped(&self) -> usize {
        self.neg_grouped
    }

    /// 对上数。
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 红判。
    pub const fn red(&self) -> bool {
        self.red
    }

    /// 千分率。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            0
        } else if self.red {
            929
        } else {
            1000
        }
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(LN_ENOBS);
        }
        if self.red {
            return Err(LN_ELOCALE);
        }
        if self.matched != self.total {
            return Err(LN_EVALUE);
        }
        if self.total != CORPUS {
            return Err(LN_EVALUE);
        }
        if self.comma != 6 || self.point != 8 {
            return Err(LN_EVALUE);
        }
        if self.grouped != 7 || self.fallback != 2 {
            return Err(LN_EVALUE);
        }
        if self.negative != 4 || self.neg_grouped != 2 {
            return Err(LN_EVALUE);
        }
        Ok(())
    }

    /// 是否绿。
    pub fn green(&self) -> bool {
        match self.gate() {
            Ok(()) => true,
            Err(_) => false,
        }
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("lc_numeric corpus=");
        s.push_str(u32_text(self.total as u32).as_str());
        s.push_str(" comma=");
        s.push_str(u32_text(self.comma as u32).as_str());
        s.push_str(" point=");
        s.push_str(u32_text(self.point as u32).as_str());
        s.push_str(" grouped=");
        s.push_str(u32_text(self.grouped as u32).as_str());
        s.push_str(" fallback=");
        s.push_str(u32_text(self.fallback as u32).as_str());
        s.push_str(" negative=");
        s.push_str(u32_text(self.negative as u32).as_str());
        s.push_str(" neg_grouped=");
        s.push_str(u32_text(self.neg_grouped as u32).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched as u32).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

/// 走一遍语料。
fn walk(red: Option<usize>) -> Result<Census, u32> {
    let mut c = Census::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let case = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(LN_EVALUE),
        };
        let (loc, fb) = pick(case.locale);
        let out = format_units(case.units, case.neg, &loc, case.digits)?;
        c.total += 1;
        if fb {
            c.fallback += 1;
        }
        if loc.point == ',' {
            c.comma += 1;
        } else {
            c.point += 1;
        }
        let ip = case.units / MICRO;
        if loc.grouping && ip >= 1000 {
            c.grouped += 1;
            if case.neg {
                c.neg_grouped += 1;
            }
        }
        if case.neg {
            c.negative += 1;
        }
        if out.as_str() == case.want {
            c.matched += 1;
        }
        i += 1;
    }
    if let Some(slot) = red {
        if slot >= c.total {
            return Err(LN_ESLOT);
        }
        c.matched = c.matched.saturating_sub(1);
        c.red = true;
    }
    Ok(c)
}

/// 绿跑。
pub fn run() -> Result<Census, u32> {
    walk(None)
}

/// 红注入跑。
pub fn run_with(red: Option<usize>) -> Result<Census, u32> {
    walk(red)
}

/// 覆盖判据。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(LN_ENOBS)
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 一行格式化轨迹（逐字）。
pub fn trace(value: &str, units: u64, neg: bool) -> Result<String, u32> {
    check_value(value)?;
    let (loc, fb) = pick(value);
    let out = format_units(units, neg, &loc, DEFAULT_DIGITS)?;
    let mut s = String::new();
    s.push_str("LC_NUMERIC=");
    s.push_str(value);
    s.push_str(" decimal_point=");
    s.push(loc.point);
    s.push_str(" printf=%s\n");
    s.push_str(out.as_str());
    if fb {
        s.push_str(" (fallback C)\n");
    } else {
        s.push('\n');
    }
    Ok(s)
}

/// 档位摘要。
pub fn summary(value: &str) -> String {
    let (loc, fb) = pick(value);
    let mut s = String::new();
    s.push_str("numeric=");
    s.push_str(loc.name);
    s.push_str(" radix=");
    s.push_str(loc.radix_name());
    s.push_str(" grouping=");
    s.push_str(if loc.grouping { "yes" } else { "no" });
    s.push_str(" fallback=");
    s.push_str(if fb { "yes" } else { "no" });
    s
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格。
#[derive(Clone, Copy, Debug)]
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

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(LN_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一格。
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

    /// 对账：恰好 14 行、8 成 6 败，值与码逐行对上。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(LN_EBAL);
        }
        const VALS: [u32; 8] = [14, 6, 8, 7, 2, 4, 2, 929];
        const CODES: [u32; 6] =
            [LN_EDIGIT, LN_ERADIX, LN_EGROUP, LN_ELOCALE, LN_ESTATE, LN_EOVER];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LN_EBAL),
            };
            if s.slot as usize != i {
                return Err(LN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LN_EVALUE);
                        }
                    }
                    None => return Err(LN_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LN_EVALUE);
                        }
                    }
                    None => return Err(LN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LN_EBAL);
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
            return Err(LN_EBAL);
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
            Err(LN_ESEAL)
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

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 失败格。
fn err_cell(slot: u8, op: u8, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 建账本：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total())));
    let _ = l.register(count_cell(1, OP_LOCALE, r.map(|x| x.comma())));
    let _ = l.register(count_cell(2, OP_LOCALE, r.map(|x| x.point())));
    let _ = l.register(count_cell(3, OP_GROUP, r.map(|x| x.grouped())));

    let _ = l.register(count_cell(4, OP_LOCALE, r.map(|x| x.fallback())));
    let _ = l.register(count_cell(5, OP_RENDER, r.map(|x| x.negative())));
    let _ = l.register(count_cell(6, OP_GROUP, r.map(|x| x.neg_grouped())));

    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_RENDER, red.map(|x| x.rate() as usize)));

    let en = match table_find("en_US.UTF-8") {
        Some(v) => v,
        None => C_NUMERIC,
    };
    let digit_err = match parse("12x4.5", &en) {
        Ok(_) => LN_EDIGIT,
        Err(c) => c,
    };
    let _ = l.register(err_cell(8, OP_PARSE, digit_err));

    let radix_err = match parse("1.2.3", &en) {
        Ok(_) => LN_ERADIX,
        Err(c) => c,
    };
    let _ = l.register(err_cell(9, OP_PARSE, radix_err));

    let group_err = match parse("1,23,4.5", &en) {
        Ok(_) => LN_EGROUP,
        Err(c) => c,
    };
    let _ = l.register(err_cell(10, OP_GROUP, group_err));

    let loc_err = match resolve("xx_YY.UTF-8") {
        Ok(_) => LN_ELOCALE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(11, OP_LOCALE, loc_err));

    let mut b = Binder::new();
    let first = b.bind("de_DE.UTF-8");
    let state = match first {
        Ok(_) => match b.bind("C") {
            Ok(_) => LN_ESTATE,
            Err(c) => c,
        },
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_BIND, state));

    let over = match parse("1234567890123456.0", &en) {
        Ok(_) => LN_EOVER,
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_ROUND, over));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 数值档表。
fn numeric_table_ok() -> bool {
    let a = TABLE.len() == LOCALES && table_at(0).map(|n| n.name) == Some("C");
    let b = table_find("de_DE.UTF-8").map(|n| n.point) == Some(',');
    let c = table_find("en_US.UTF-8").map(|n| n.sep) == Some(',');
    let d = table_find("xx_YY.UTF-8").is_none();
    let e = C_NUMERIC.point == '.' && !C_NUMERIC.grouping && C_NUMERIC.is_c();
    let f = table_find("fr_FR.UTF-8").map(|n| n.sep) == Some(' ');
    let g = table_at(LOCALES).is_none();
    let mut uniq = true;
    let mut i = 0usize;
    while i < TABLE.len() {
        let mut j = i + 1;
        while j < TABLE.len() {
            match (TABLE.get(i), TABLE.get(j)) {
                (Some(x), Some(y)) => {
                    if x.name == y.name {
                        uniq = false;
                    }
                }
                _ => uniq = false,
            }
            j += 1;
        }
        i += 1;
    }
    a && b && c && d && e && f && g && uniq
}

/// 坏值回退。
fn fallback_ok() -> bool {
    let a = resolve("xx_YY.INVALID").err() == Some(LN_ELOCALE);
    let (loc, fb) = pick("xx_YY.INVALID");
    let b = fb && loc.name == "C" && loc.point == '.' && !loc.grouping;
    let c = match format_locale("xx_YY.INVALID", 1_234_500_000, false, 6) {
        Ok(s) => s.as_str() == "1234.500000",
        Err(_) => false,
    };
    let d = match format_locale("xx_YY.INVALID", 1_234_500_000, false, 6) {
        Ok(s) => !s.as_str().contains(','),
        Err(_) => false,
    };
    let (loc2, fb2) = pick("de_DE.UTF-8");
    let e = !fb2 && loc2.point == ',';
    let f = check_value("") == Err(LN_EBYTE) && check_value("de DE") == Err(LN_EBYTE);
    a && b && c && d && e && f
}

/// 六位定点。
fn fixed_ok() -> bool {
    let c = C_NUMERIC;
    let a = format_units(0, false, &c, 6).map(|s| s.as_str() == "0.000000") == Ok(true);
    let b = format_units(500_000, false, &c, 6).map(|s| s.as_str() == "0.500000") == Ok(true);
    let d = format_units(MICRO, false, &c, 6).map(|s| s.as_str() == "1.000000") == Ok(true);
    let e = format_units(1_234_500_000, false, &c, 6).map(|s| s.as_str() == "1234.500000") == Ok(true);
    let f = format_units(1_234_500_000, true, &c, 6).map(|s| s.as_str() == "-1234.500000") == Ok(true);
    let g = format_units(1, false, &c, 6).map(|s| s.as_str() == "0.000001") == Ok(true);
    let h = format_units(1, true, &c, 6).map(|s| s.as_str() == "-0.000001") == Ok(true);
    let i = format_units(1, false, &c, 0).err() == Some(LN_EDIGIT);
    let j = format_units(1, false, &c, 7).err() == Some(LN_EDIGIT);
    a && b && d && e && f && g && h && i && j
}

/// 分组插入。
fn group_ok() -> bool {
    let de = match table_find("de_DE.UTF-8") {
        Some(v) => v,
        None => return false,
    };
    let a = format_units(1_234_500_000, false, &de, 6)
        .map(|s| s.as_str() == "1.234,500000")
        == Ok(true);
    let b = format_units(1_234_567_891_000, false, &de, 6)
        .map(|s| s.as_str() == "1.234.567,891000")
        == Ok(true);
    let c = format_units(1_000_000_000, false, &de, 6)
        .map(|s| s.as_str() == "1.000,000000")
        == Ok(true);
    let d = format_units(999_500_000, false, &de, 6)
        .map(|s| s.as_str() == "999,500000")
        == Ok(true);
    let e = group_digits("1234", '.').as_str() == "1.234";
    let f = group_digits("123", ',').as_str() == "123";
    let g = group_digits("1234567", '.').as_str() == "1.234.567";
    let h = format_units(1_234_500_000, false, &C_NUMERIC, 6)
        .map(|s| s.as_str() == "1234.500000")
        == Ok(true);
    a && b && c && d && e && f && g && h
}

/// 进位舍入。
fn round_ok() -> bool {
    let a = round_units(999_999_999, 2) == Ok(1_000_000_000);
    let b = round_units(999_994_999, 2) == Ok(999_990_000);
    let c = round_units(995_000, 2) == Ok(1_000_000);
    let d = round_units(994_999, 2) == Ok(990_000);
    let e = round_units(1_234_567, 6) == Ok(1_234_567);
    let f = round_units(1, 7).err() == Some(LN_EDIGIT);
    let de = match table_find("de_DE.UTF-8") {
        Some(v) => v,
        None => return false,
    };
    let g = format_units(999_999_999, false, &de, 2)
        .map(|s| s.as_str() == "1.000,00")
        == Ok(true);
    let h = format_units(999_994_999, false, &de, 2)
        .map(|s| s.as_str() == "999,99")
        == Ok(true);
    a && b && c && d && e && f && g && h
}

/// 解析往返。
fn parse_ok() -> bool {
    let en = match table_find("en_US.UTF-8") {
        Some(v) => v,
        None => return false,
    };
    let de = match table_find("de_DE.UTF-8") {
        Some(v) => v,
        None => return false,
    };
    let a = parse("1234.5", &en).map(|p| p.units) == Ok(1_234_500_000);
    let b = parse("1,234.5", &en).map(|p| p.units) == Ok(1_234_500_000);
    let c = parse("-1.234,5", &de).map(|p| p.units) == Ok(1_234_500_000);
    let d = parse("-1.234,5", &de).map(|p| p.neg) == Ok(true);
    let e = parse("12x4.5", &en).err() == Some(LN_EDIGIT);
    let f = parse("1.2.3", &en).err() == Some(LN_ERADIX);
    let g = parse("1,23,4.5", &en).err() == Some(LN_EGROUP);
    let h = parse("1234567890123456.0", &en).err() == Some(LN_EOVER);
    let i = parse("", &en).err() == Some(LN_EDIGIT);
    let j = parse(".5", &en).err() == Some(LN_EDIGIT);
    let k = parse("12.1234567", &en).err() == Some(LN_EOVER);
    a && b && c && d && e && f && g && h && i && j && k
}

/// 往返格式化。
fn trip_ok() -> bool {
    let a = round_trip("1.234,5", "de_DE.UTF-8", 6)
        .map(|s| s.as_str() == "1.234,500000")
        == Ok(true);
    let b = round_trip("1,234.5", "en_US.UTF-8", 6)
        .map(|s| s.as_str() == "1,234.500000")
        == Ok(true);
    let c = round_trip("1234.5", "xx_YY.UTF-8", 6)
        .map(|s| s.as_str() == "1234.500000")
        == Ok(true);
    let d = round_trip("999,999", "de_DE.UTF-8", 2)
        .map(|s| s.as_str() == "1.000,00")
        == Ok(true);
    let e = round_trip("1,23,4.5", "en_US.UTF-8", 6).err() == Some(LN_EGROUP);
    a && b && c && d && e
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.total() == CORPUS && c.matched() == CORPUS;
    let b = c.comma() == 6 && c.point() == 8 && c.grouped() == 7;
    let d = c.fallback() == 2 && c.negative() == 4 && c.neg_grouped() == 2;
    let e = c.rate() == 1000 && c.green() && c.gate() == Ok(());
    a && b && d && e
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() || !numeric_table_ok() {
        return false;
    }
    let a = match case_at(0) {
        Some(c) => c.locale == "de_DE.UTF-8" && c.want == "1.234,500000",
        None => false,
    };
    let b = match case_at(11) {
        Some(c) => c.locale == "xx_YY.UTF-8" && c.want == "1234.500000",
        None => false,
    };
    let c = case_at(CORPUS).is_none();
    a && b && c
}

/// 汇总渲染。
fn report_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.render().as_str() == "lc_numeric corpus=14 comma=6 point=8 grouped=7 fallback=2 negative=4 neg_grouped=2 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(LN_ENOBS);
    let d = e.render().as_str() == "lc_numeric corpus=0 comma=0 point=0 grouped=0 fallback=0 negative=0 neg_grouped=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(CORPUS + 1) == Err(LN_ENOBS);
    a && b && d && f
}

/// 红单显式。
fn reds_ok() -> bool {
    let c = match run_with(Some(0)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched() == CORPUS - 1 && c.rate() == 929;
    let b = c.gate() == Err(LN_ELOCALE) && !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(LN_ESLOT);
    a && b && d
}

/// 小数点判据逐字。
fn radix_ok() -> bool {
    let a = match format_locale("de_DE.UTF-8", 1_234_500_000, false, 6) {
        Ok(s) => s.as_str() == "1.234,500000",
        Err(_) => false,
    };
    let b = match format_locale("de_DE.UTF-8", 1_234_500_000, false, 6) {
        Ok(s) => s.as_bytes().get(1).copied() == Some(b'.') && s.as_bytes().get(5).copied() == Some(b','),
        Err(_) => false,
    };
    let c = match format_locale("en_US.UTF-8", 1_234_500_000, false, 6) {
        Ok(s) => s.as_str() == "1,234.500000",
        Err(_) => false,
    };
    let d = match format_locale("en_US.UTF-8", 1_234_500_000, false, 6) {
        Ok(s) => {
            let radix = s.as_bytes().get(5).copied();
            radix == Some(b'.')
        }
        Err(_) => false,
    };
    let e = match format_locale("xx_YY.UTF-8", 1_234_500_000, false, 6) {
        Ok(s) => {
            let radix = s.as_bytes().get(4).copied();
            radix == Some(b'.')
        }
        Err(_) => false,
    };
    let f = match trace("de_DE.UTF-8", 1_234_500_000, false) {
        Ok(s) => s.as_str()
            == "LC_NUMERIC=de_DE.UTF-8 decimal_point=, printf=%s\n1.234,500000\n",
        Err(_) => false,
    };
    let g = match trace("xx_YY.UTF-8", 1_234_500_000, false) {
        Ok(s) => s.as_str()
            == "LC_NUMERIC=xx_YY.UTF-8 decimal_point=. printf=%s\n1234.500000 (fallback C)\n",
        Err(_) => false,
    };
    let h = trace("", 0, false) == Err(LN_EBYTE);
    let i = summary("de_DE.UTF-8").as_str() == "numeric=de_DE.UTF-8 radix=comma grouping=yes fallback=no";
    let j = summary("xx_YY.UTF-8").as_str() == "numeric=C radix=point grouping=no fallback=yes";
    a && b && c && d && e && f && g && h && i && j
}

/// 绑定闸与账本。
fn ledger_ok() -> bool {
    let mut b = Binder::new();
    let a = !b.bound() && b.name().is_none();
    let c = match b.bind("de_DE.UTF-8") {
        Ok(n) => n.point == ',' && b.bound() && b.idx() == 4,
        Err(_) => false,
    };
    let d = b.name() == Some("de_DE.UTF-8");
    let e = b.bind("C") == Err(LN_ESTATE) && b.last() == LN_ESTATE;
    let mut f = Binder::new();
    let g = f.bind("xx_YY.UTF-8") == Err(LN_ELOCALE) && !f.bound();
    let h = f.bind("C").is_ok() && f.name() == Some("C");
    let l = build_ledger();
    let i = l.audit() == Ok(()) && l.ok_count() == 8 && l.err_count() == 6;
    let j = l.seal() == build_ledger().seal();
    let k = u32_text(7).as_str() == "7" && u64_text(1_234_567_891_000).as_str() == "1234567891000";
    let m = hex4_text(0xfd30).as_str() == "fd30";
    let n = digest(b"1.234,5") == digest(b"1.234,5") && digest(b"1.234,5") != digest(b"1.234.5");
    a && c && d && e && g && h && i && j && k && m && n
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = numeric_table_ok() && fallback_ok() && fixed_ok();
    out[1] = group_ok() && round_ok() && parse_ok();
    out[2] = trip_ok() && corpus_ok() && slots_ok() && report_ok() && reds_ok();
    out[3] = radix_ok() && ledger_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11644_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11644.lc_numeric");

    s.add("LN01-数值档表", table_ok(), "static");
    s.add("LN02-坏值回退", fallback_ok(), "static");
    s.add("LN03-六位定点", fixed_ok(), "static");
    s.add("LN04-分组插入", group_ok(), "static");
    s.add("LN05-进位舍入", round_ok(), "static");
    s.add("LN06-解析往返", parse_ok() && trip_ok(), "static");
    s.add("LN07-十四卡全跑", corpus_ok(), "static");
    s.add("LN08-语料编号", slots_ok(), "static");
    s.add("LN09-汇总渲染", report_ok(), "static");
    s.add("LN10-红单显式", reds_ok(), "static");
    s.add("LN11-小数点逐字", radix_ok(), "static");
    s.add("LN12-绑定闸与账本", ledger_ok(), "static");
    s.add("LN13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
        assert_eq!(l.sample(8).map(|x| x.code), Some(LN_EDIGIT));
        assert_eq!(l.sample(13).map(|x| x.code), Some(LN_EOVER));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(LN_EBAL));
        assert!(l.rejects() >= 1);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert!(out[0]);
        assert!(out[1]);
        assert!(out[2]);
        assert!(out[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11644_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn table_shape() {
        assert_eq!(TABLE.len(), LOCALES);
        assert_eq!(table_find("C").map(|n| n.point), Some('.'));
        assert_eq!(table_find("de_DE.UTF-8").map(|n| n.point), Some(','));
        assert_eq!(table_find("de_DE.UTF-8").map(|n| n.sep), Some('.'));
        assert_eq!(table_find("fr_FR.UTF-8").map(|n| n.sep), Some(' '));
        assert!(table_find("xx_YY.UTF-8").is_none());
    }

    #[test]
    fn de_format() {
        let de = match table_find("de_DE.UTF-8") {
            Some(v) => v,
            None => return,
        };
        assert_eq!(format_units(1_234_500_000, false, &de, 6).ok().as_deref(), Some("1.234,500000"));
        assert_eq!(
            format_units(1_234_567_891_000, true, &de, 6).ok().as_deref(),
            Some("-1.234.567,891000")
        );
    }

    #[test]
    fn us_format() {
        let us = match table_find("en_US.UTF-8") {
            Some(v) => v,
            None => return,
        };
        assert_eq!(format_units(1_234_500_000, false, &us, 6).ok().as_deref(), Some("1,234.500000"));
        assert_eq!(
            format_units(9_876_543_210_000, true, &us, 6).ok().as_deref(),
            Some("-9,876,543.210000")
        );
    }

    #[test]
    fn c_format() {
        assert_eq!(
            format_units(1_234_500_000, false, &C_NUMERIC, 6).ok().as_deref(),
            Some("1234.500000")
        );
        assert_eq!(format_units(500_000, true, &C_NUMERIC, 6).ok().as_deref(), Some("-0.500000"));
        assert_eq!(format_units(0, false, &C_NUMERIC, 6).ok().as_deref(), Some("0.000000"));
    }

    #[test]
    fn carry_rounding() {
        assert_eq!(round_units(999_999_999, 2), Ok(1_000_000_000));
        assert_eq!(round_units(994_999, 2), Ok(990_000));
        let de = match table_find("de_DE.UTF-8") {
            Some(v) => v,
            None => return,
        };
        assert_eq!(format_units(999_999_999, false, &de, 2).ok().as_deref(), Some("1.000,00"));
    }

    #[test]
    fn grouping_rules() {
        assert_eq!(group_digits("123", ','), "123");
        assert_eq!(group_digits("1234", ','), "1,234");
        assert_eq!(group_digits("1234567", ','), "1,234,567");
        assert_eq!(group_digits("1000", '.'), "1.000");
        assert_eq!(group_digits("1", '.'), "1");
    }

    #[test]
    fn parse_en() {
        let en = match table_find("en_US.UTF-8") {
            Some(v) => v,
            None => return,
        };
        let p = match parse("1,234.5", &en) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(p.units, 1_234_500_000);
        assert!(!p.neg);
        assert_eq!(parse("1,23,4.5", &en).err(), Some(LN_EGROUP));
        assert_eq!(parse("1.2.3", &en).err(), Some(LN_ERADIX));
    }

    #[test]
    fn parse_de() {
        let de = match table_find("de_DE.UTF-8") {
            Some(v) => v,
            None => return,
        };
        let p = match parse("-1.234,5", &de) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(p.units, 1_234_500_000);
        assert!(p.neg);
        assert_eq!(parse("1.23,4", &de).err(), Some(LN_EGROUP));
    }

    #[test]
    fn fallback_is_c() {
        let (loc, fb) = pick("xx_YY.UTF-8");
        assert!(fb);
        assert_eq!(loc.name, "C");
        assert_eq!(loc.point, '.');
        assert!(!loc.grouping);
        assert_eq!(
            format_locale("xx_YY.UTF-8", 1_234_500_000, false, 6).ok().as_deref(),
            Some("1234.500000")
        );
    }

    #[test]
    fn trace_lines() {
        assert_eq!(
            trace("de_DE.UTF-8", 1_234_500_000, false).ok().as_deref(),
            Some("LC_NUMERIC=de_DE.UTF-8 decimal_point=, printf=%s\n1.234,500000\n")
        );
        assert_eq!(trace("", 0, false).err(), Some(LN_EBYTE));
    }

    #[test]
    fn census_numbers() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.total(), 14);
        assert_eq!(c.comma(), 6);
        assert_eq!(c.point(), 8);
        assert_eq!(c.grouped(), 7);
        assert_eq!(c.fallback(), 2);
        assert_eq!(c.negative(), 4);
        assert_eq!(c.neg_grouped(), 2);
        assert_eq!(c.gate(), Ok(()));
    }

    #[test]
    fn red_override() {
        let c = match run_with(Some(1)) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.matched(), 13);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(LN_ELOCALE));
    }

    #[test]
    fn binder_once() {
        let mut b = Binder::new();
        assert!(b.bind("de_DE.UTF-8").is_ok());
        assert_eq!(b.bind("C"), Err(LN_ESTATE));
        let mut f = Binder::new();
        assert_eq!(f.bind("xx_YY.UTF-8"), Err(LN_ELOCALE));
        assert!(!f.bound());
        assert!(f.bind("C").is_ok());
    }

    #[test]
    fn corpus_table() {
        assert_eq!(CASES.len(), CORPUS);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
        assert_eq!(case_at(CORPUS), None);
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u64_text(0), "0");
        assert_eq!(u64_text(1_234_567_891_000), "1234567891000");
        assert_eq!(pow10(0), 1);
        assert_eq!(pow10(6), 1_000_000);
        assert_eq!(pad_frac("5", 3), "005");
    }
}
