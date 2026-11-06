//! UNX-F11556 numfmt 数字格式化
//!
//! `numfmt` 的职责是把「带单位后缀的数字」和「纯数字」互相搬运：`--to` 把
//! `1500` 变成 `1.5K`，`--from` 把 `1536Ki` 变回 `1572864`，`--round` 决定
//! 进位方向，`--grouping` 决定是否夹千位逗号。它不碰浮点——基准机同样用
//! 定点算术，因为 `0.1 + 0.2` 这类误差会让「与基准机逐字节一致」直接破产：
//!
//! 1. **定点表示**：一个数写成「符号 × 尾数 / 10^小数位 × 基数^幂」，基数在
//!    解析时由 `--from` 定（`si`=1000、`iec`=1024、`auto` 看有没有 `i`）。
//!    全部换算走 `u128` 整数乘除，溢出报 `NM_ERANGE`，绝不回绕。
//! 2. **输出存一位小数**：`1500` → `1.5K`、`1000` → `1.0K`、`999` → `999`；
//!    进位后若跨到下一个单位（`999.96K` → `1.0M`）必须再升一级，不能出
//!    `1000.0K` 这种基线不认的串。
//! 3. **行内替换**：`--field` 选第几段、`--delimiter` 选分隔符（缺省按空白
//!    连续段切），`--padding` 右对齐、`--suffix` 贴尾、`--header` 跳表头、
//!    `--invalid` 决定坏数整行放行还是中止。
//!
//! 闸门次序：选项字面（`NM_EINVAL`）→ 选项长度（`NM_ELEN`）→ 字节合法性
//! （`NM_EBYTE`）→ 数值越界（`NM_ERANGE`）→ 重复（`NM_EDUP`）→ 单位不明
//! （`NM_EUNIT`）→ 格式自相矛盾（`NM_EFORMAT`）→ 状态冲突（`NM_ESTATE`）。
//! 单位在越界之后：`99999999999999999999Q` 先报超范围再谈单位，因为前者是
//! 「值根本装不下」，后者是「装下了但不认」。
//!
//! 账本语义：十四格 = 八格成功（解析幂次、si 渲染、iec-i 渲染、最近进位、
//! 千位分组、行内字段、补齐宽度、整行改写）+ 六格失败（单位不明、格式矛盾、
//! 空串、坏字节、超范围、选项重复）。`audit` 认「八值六码」，`seal` 做
//! FNV-1a 折叠，`replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf3d0;

/// 选项字面非法。
pub const NM_EINVAL: u32 = SEG_BASE + 0x01;
/// 选项或数字过长。
pub const NM_ELEN: u32 = SEG_BASE + 0x02;
/// 字节不合法。
pub const NM_EBYTE: u32 = SEG_BASE + 0x03;
/// 数值越界。
pub const NM_ERANGE: u32 = SEG_BASE + 0x04;
/// 选项重复。
pub const NM_EDUP: u32 = SEG_BASE + 0x05;
/// 单位不明。
pub const NM_EUNIT: u32 = SEG_BASE + 0x06;
/// 格式自相矛盾。
pub const NM_EFORMAT: u32 = SEG_BASE + 0x07;
/// 选项组合冲突。
pub const NM_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const NM_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位。
pub const NM_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const NM_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const NM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const NM_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 数字串长度上限。
pub const NUMCAP: usize = 32;

/// 外部后缀长度上限。
pub const SUFCAP: usize = 8;

/// 参数个数上限。
pub const ARGCAP: usize = 32;

/// 补齐宽度上限。
pub const PADMAX: u32 = 64;

/// 字段序号上限。
pub const FIELDMAX: u32 = 64;

/// 最高幂次（E，即 10^18 / 2^60）。
pub const POWMAX: u8 = 6;

/// 操作码：解析。
pub const OP_PARSE: u8 = 0x01;
/// 操作码：输出渲染。
pub const OP_FORMAT: u8 = 0x02;
/// 操作码：进位。
pub const OP_ROUND: u8 = 0x03;
/// 操作码：分组。
pub const OP_GROUP: u8 = 0x04;
/// 操作码：字段。
pub const OP_FIELD: u8 = 0x05;
/// 操作码：补齐。
pub const OP_PAD: u8 = 0x06;
/// 操作码：整行。
pub const OP_LINE: u8 = 0x07;
/// 操作码：选项。
pub const OP_OPT: u8 = 0x08;

/// FNV-1a 偏移。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold(h: u64, b: u64) -> u64 {
    let mut v = h ^ (b & 0xff);
    let mut i = 0u32;
    while i < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    v
}

/// 十进制拼接。
fn push_dec(s: &mut String, mut v: u128) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 40];
    let mut i = 40usize;
    while v > 0 && i > 0 {
        i -= 1;
        if let Some(d) = buf.get_mut(i) {
            *d = b'0' + (v % 10) as u8;
        }
        v /= 10;
    }
    let mut k = i;
    while k < 40 {
        if let Some(d) = buf.get(k) {
            s.push(*d as char);
        }
        k += 1;
    }
}

/// 十进制文本。
fn dec_text(v: u128) -> String {
    let mut s = String::new();
    push_dec(&mut s, v);
    s
}

/// 10 的幂（`exp` ≤ 18 时必不溢出）。
fn pow10(exp: u8) -> u128 {
    let mut v = 1u128;
    let mut i = 0u8;
    while i < exp {
        v = v.saturating_mul(10);
        i += 1;
    }
    v
}

/// 基数幂次，溢出给 `None`。
fn pow_base(base: u64, k: u8) -> Option<u128> {
    let mut v = 1u128;
    let mut i = 0u8;
    while i < k {
        v = v.checked_mul(base as u128)?;
        i += 1;
    }
    Some(v)
}

/// 千位分组。
pub fn group_digits(digits: &str) -> String {
    let n = digits.len();
    let mut s = String::new();
    let mut i = 0usize;
    while i < n {
        if i > 0 && (n - i) % 3 == 0 {
            s.push(',');
        }
        if let Some(c) = digits.as_bytes().get(i) {
            s.push(*c as char);
        }
        i += 1;
    }
    s
}

/// 输出口径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    /// 不缩放，只做分组/补齐。
    None,
    /// 1000 进位。
    Si,
    /// 1024 进位，后缀不带 `i`。
    Iec,
    /// 1024 进位，后缀带 `i`。
    IecI,
}

impl Scale {
    /// 基数。
    pub fn base(&self) -> u64 {
        if *self == Scale::None {
            1
        } else if *self == Scale::Si {
            1000
        } else {
            1024
        }
    }

    /// 单位字母（幂次 0 为空）。
    pub fn letter(&self, k: u8) -> &'static str {
        if *self == Scale::None {
            return "";
        }
        let l = match k {
            1 => "K",
            2 => "M",
            3 => "G",
            4 => "T",
            5 => "P",
            6 => "E",
            _ => "",
        };
        if *self == Scale::IecI && k > 0 {
            match k {
                1 => "Ki",
                2 => "Mi",
                3 => "Gi",
                4 => "Ti",
                5 => "Pi",
                _ => "Ei",
            }
        } else {
            l
        }
    }

    /// 名称。
    pub fn text(&self) -> &'static str {
        if *self == Scale::None {
            "auto"
        } else if *self == Scale::Si {
            "si"
        } else if *self == Scale::Iec {
            "iec"
        } else {
            "iec-i"
        }
    }
}

/// 输入口径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PMode {
    /// 不认后缀，只做纯数字穿过。
    None,
    /// 有 `i` 走 1024，没有走 1000。
    Auto,
    /// 1000 进位。
    Si,
    /// 1024 进位，禁止 `i`。
    Iec,
    /// 1024 进位，必须有 `i`。
    IecI,
}

impl PMode {
    /// 名称。
    pub fn text(&self) -> &'static str {
        match self {
            PMode::None => "none",
            PMode::Auto => "auto",
            PMode::Si => "si",
            PMode::Iec => "iec",
            PMode::IecI => "iec-i",
        }
    }
}

/// 进位方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    /// 远离零。
    Up,
    /// 靠近零。
    Down,
    /// 远离零（别名）。
    FromZero,
    /// 靠近零（别名）。
    TowardsZero,
    /// 最近；半值远离零。
    Nearest,
}

impl Round {
    /// 名称。
    pub fn text(&self) -> &'static str {
        match self {
            Round::Up => "up",
            Round::Down => "down",
            Round::FromZero => "from-zero",
            Round::TowardsZero => "towards-zero",
            Round::Nearest => "nearest",
        }
    }

    /// 是否远离零。
    fn away(&self) -> bool {
        *self == Round::Up || *self == Round::FromZero
    }

    /// 对商 `q`、余 `r`、分母 `d` 做取舍。
    pub fn apply(&self, q: u128, r: u128, d: u128) -> u128 {
        if r == 0 || d == 0 {
            return q;
        }
        if self.away() {
            return q.saturating_add(1);
        }
        if *self == Round::Down || *self == Round::TowardsZero {
            return q;
        }
        if r.saturating_mul(2) >= d {
            q.saturating_add(1)
        } else {
            q
        }
    }
}

/// 外部后缀。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Suffix {
    buf: [u8; SUFCAP],
    len: u8,
}

impl Suffix {
    /// 空后缀。
    pub const fn empty() -> Suffix {
        Suffix { buf: [0; SUFCAP], len: 0 }
    }

    /// 解析：≤ `SUFCAP`、ASCII 可打印。
    pub fn parse(raw: &[u8]) -> Result<Suffix, u32> {
        if raw.len() > SUFCAP {
            return Err(NM_ELEN);
        }
        let mut buf = [0u8; SUFCAP];
        let mut i = 0usize;
        while i < raw.len() {
            match raw.get(i) {
                Some(b) => {
                    let c = *b;
                    if c < 0x20 || c >= 0x7f {
                        return Err(NM_EBYTE);
                    }
                    if let Some(d) = buf.get_mut(i) {
                        *d = c;
                    }
                }
                None => return Err(NM_EBYTE),
            }
            i += 1;
        }
        Ok(Suffix { buf, len: raw.len() as u8 })
    }

    /// 文本解析。
    pub fn parse_text(text: &str) -> Result<Suffix, u32> {
        Suffix::parse(text.as_bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 字节视图。
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len as usize).unwrap_or(&[])
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(c) = self.buf.get(i) {
                s.push(*c as char);
            }
            i += 1;
        }
        s
    }

    /// 前缀相等（用于剥离尾缀）。
    pub fn matches(&self, raw: &[u8]) -> bool {
        if raw.len() < self.len as usize {
            return false;
        }
        let head = raw.get(..self.len as usize).unwrap_or(&[]);
        head == self.as_bytes()
    }
}

/// 一个定点数：`±mant / 10^exp × base^power`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Amount {
    /// 是否负数。
    pub neg: bool,
    /// 尾数。
    pub mant: u64,
    /// 小数位数。
    pub exp: u8,
    /// 幂次。
    pub power: u8,
    /// 基数（1 / 1000 / 1024）。
    pub base: u64,
}

impl Amount {
    /// 零。
    pub const fn zero() -> Amount {
        Amount { neg: false, mant: 0, exp: 0, power: 0, base: 1 }
    }

    /// 解析一个字段。
    pub fn parse(raw: &[u8], mode: PMode, suffix: &Suffix) -> Result<Amount, u32> {
        if raw.is_empty() {
            return Err(NM_EINVAL);
        }
        if raw.len() > NUMCAP {
            return Err(NM_ELEN);
        }
        let mut i = 0usize;
        let mut neg = false;
        match raw.first() {
            Some(b'-') => {
                neg = true;
                i = 1;
            }
            Some(b'+') => {
                i = 1;
            }
            _ => {}
        }
        let mut mant = 0u64;
        let mut digits = 0u32;
        let mut exp = 0u8;
        let mut seen_dot = false;
        while i < raw.len() {
            let c = match raw.get(i) {
                Some(v) => *v,
                None => break,
            };
            if c >= b'0' && c <= b'9' {
                if seen_dot {
                    if exp >= 18 {
                        return Err(NM_ELEN);
                    }
                    exp += 1;
                }
                mant = match mant.checked_mul(10) {
                    Some(v) => v,
                    None => return Err(NM_ERANGE),
                };
                mant = match mant.checked_add((c - b'0') as u64) {
                    Some(v) => v,
                    None => return Err(NM_ERANGE),
                };
                digits += 1;
            } else if c == b'.' {
                if seen_dot {
                    return Err(NM_EBYTE);
                }
                seen_dot = true;
            } else {
                break;
            }
            i += 1;
        }
        if digits == 0 {
            return Err(NM_EBYTE);
        }
        let mut power = 0u8;
        let mut has_i = false;
        if i < raw.len() {
            let c = match raw.get(i) {
                Some(v) => *v,
                None => return Err(NM_EBYTE),
            };
            let p = match c {
                b'K' | b'k' => 1u8,
                b'M' | b'm' => 2,
                b'G' | b'g' => 3,
                b'T' | b't' => 4,
                b'P' | b'p' => 5,
                b'E' | b'e' => 6,
                _ => return Err(NM_EUNIT),
            };
            power = p;
            i += 1;
            if raw.get(i) == Some(&b'i') || raw.get(i) == Some(&b'I') {
                has_i = true;
                i += 1;
            }
        }
        let tail = raw.get(i..).unwrap_or(&[]);
        if !tail.is_empty() {
            if suffix.is_empty() || !suffix.matches(tail) {
                return Err(NM_EBYTE);
            }
            i += suffix.len();
            if raw.get(i..).unwrap_or(&[]).len() > 0 {
                return Err(NM_EBYTE);
            }
        }
        let base = match mode {
            PMode::None => {
                if power != 0 || has_i {
                    return Err(NM_EFORMAT);
                }
                1u64
            }
            PMode::Si => {
                if has_i {
                    return Err(NM_EFORMAT);
                }
                1000u64
            }
            PMode::Iec => {
                if has_i {
                    return Err(NM_EFORMAT);
                }
                1024u64
            }
            PMode::IecI => {
                if power != 0 && !has_i {
                    return Err(NM_EFORMAT);
                }
                if power == 0 && has_i {
                    return Err(NM_EFORMAT);
                }
                1024u64
            }
            PMode::Auto => {
                if has_i {
                    1024u64
                } else {
                    1000u64
                }
            }
        };
        Ok(Amount { neg, mant, exp, power, base })
    }

    /// 文本解析。
    pub fn parse_text(text: &str, mode: PMode, suffix: &Suffix) -> Result<Amount, u32> {
        Amount::parse(text.as_bytes(), mode, suffix)
    }

    /// 是否为零。
    pub fn is_zero(&self) -> bool {
        self.mant == 0
    }

    /// 折算成「整数 + 余数/分母」的真值。
    pub fn normalize(&self) -> Result<Norm, u32> {
        let mut units = self.mant as u128;
        let mut k = 0u8;
        while k < self.power {
            units = match units.checked_mul(self.base as u128) {
                Some(v) => v,
                None => return Err(NM_ERANGE),
            };
            k += 1;
        }
        let den = pow10(self.exp);
        if den == 0 {
            return Err(NM_ERANGE);
        }
        Ok(Norm { neg: self.neg, int: units / den, rem: units % den, den })
    }
}

/// 归一化真值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Norm {
    /// 是否负数。
    pub neg: bool,
    /// 整数部分（基准单位）。
    pub int: u128,
    /// 余数。
    pub rem: u128,
    /// 分母。
    pub den: u128,
}

impl Norm {
    /// 是否为零。
    pub fn is_zero(&self) -> bool {
        self.int == 0 && self.rem == 0
    }

    /// 是否真的负数（负零按正零处理）。
    pub fn negative(&self) -> bool {
        self.neg && !self.is_zero()
    }

    /// 数值分子（对给定分母）。
    fn scaled(&self, mult: u128) -> Result<u128, u32> {
        let a = match self.int.checked_mul(mult) {
            Some(v) => v,
            None => return Err(NM_ERANGE),
        };
        let b = match self.rem.checked_mul(mult) {
            Some(v) => v,
            None => return Err(NM_ERANGE),
        };
        match a.checked_add(b) {
            Some(v) => Ok(v),
            None => Err(NM_ERANGE),
        }
    }
}

/// 十进位的选择。
fn pick_power(n: &Norm, base: u64) -> u8 {
    let mut k = 0u8;
    while k < POWMAX {
        match pow_base(base, k + 1) {
            Some(p) => {
                if n.int >= p {
                    k += 1;
                } else {
                    break;
                }
            }
            None => break,
        }
    }
    k
}

/// 把真值渲染到十分位：返回（十分位商，幂次）。
pub fn tenths(n: &Norm, base: u64, round: Round) -> Result<(u128, u8), u32> {
    if base <= 1 {
        return Ok((round.apply(n.int, n.rem, n.den), 0));
    }
    let mut k = pick_power(n, base);
    let p = match pow_base(base, k) {
        Some(v) => v,
        None => return Err(NM_ERANGE),
    };
    let d = match p.checked_mul(n.den) {
        Some(v) => v,
        None => return Err(NM_ERANGE),
    };
    if k == 0 {
        let q = round.apply(n.int, n.rem, n.den);
        return Ok((q, 0));
    }
    let num = n.scaled(10)?;
    let mut q = num / d;
    let r = num % d;
    q = round.apply(q, r, d);
    if k < POWMAX && q >= (base as u128).saturating_mul(10) {
        k += 1;
        q /= base as u128;
    }
    Ok((q, k))
}

/// 选项配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conf {
    /// 输入口径。
    pub from: PMode,
    /// 输出口径。
    pub to: Scale,
    /// 进位。
    pub round: Round,
    /// 补齐宽度。
    pub padding: u32,
    /// 千位分组。
    pub grouping: bool,
    /// 字段序号（1 起）。
    pub field: u32,
    /// 分隔符；`None` 走空白切分。
    pub delim: Option<u8>,
    /// 外部后缀。
    pub suffix: Suffix,
    /// 表头行数。
    pub header: u32,
    /// 坏数处置。
    pub invalid: Invalid,
    /// 参数个数。
    pub argc: usize,
}

/// 坏数处置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// 中止。
    Abort,
    /// 失败。
    Fail,
    /// 告警。
    Warn,
    /// 忽略（原样放行）。
    Ignore,
}

impl Invalid {
    /// 名称。
    pub fn text(&self) -> &'static str {
        match self {
            Invalid::Abort => "abort",
            Invalid::Fail => "fail",
            Invalid::Warn => "warn",
            Invalid::Ignore => "ignore",
        }
    }

    /// 是否放行坏数。
    pub fn passes(&self) -> bool {
        *self == Invalid::Ignore
    }
}

impl Conf {
    /// 缺省配置：`--to=si --round=nearest --field=1`。
    pub const fn blank() -> Conf {
        Conf {
            from: PMode::Auto,
            to: Scale::Si,
            round: Round::Nearest,
            padding: 0,
            grouping: false,
            field: 1,
            delim: None,
            suffix: Suffix::empty(),
            header: 0,
            invalid: Invalid::Abort,
            argc: 0,
        }
    }
}

/// 解析一个十进制计数（选项参数）。
fn parse_u32(raw: &[u8], max: u32) -> Result<u32, u32> {
    if raw.is_empty() {
        return Err(NM_EINVAL);
    }
    if raw.len() > 4 {
        return Err(NM_ERANGE);
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < raw.len() {
        match raw.get(i) {
            Some(c) => {
                if *c < b'0' || *c > b'9' {
                    return Err(NM_EBYTE);
                }
                v = match v.checked_mul(10) {
                    Some(x) => x,
                    None => return Err(NM_ERANGE),
                };
                v = match v.checked_add((*c - b'0') as u32) {
                    Some(x) => x,
                    None => return Err(NM_ERANGE),
                };
            }
            None => return Err(NM_EBYTE),
        }
        i += 1;
    }
    if v > max {
        return Err(NM_ERANGE);
    }
    Ok(v)
}

/// 解析选项。
pub fn parse_conf(args: &[&str]) -> Result<Conf, u32> {
    let mut c = Conf::blank();
    let mut seen_to = false;
    let mut seen_from = false;
    let mut seen_round = false;
    let mut seen_suffix = false;
    let mut seen_pad = false;
    let mut seen_group = false;
    let mut seen_field = false;
    let mut seen_delim = false;
    let mut seen_header = false;
    let mut seen_invalid = false;
    let mut i = 0usize;
    while i < args.len() {
        if i >= ARGCAP {
            return Err(NM_ELEN);
        }
        let a = match args.get(i) {
            Some(v) => *v,
            None => break,
        };
        if a.len() > 64 {
            return Err(NM_ELEN);
        }
        if let Some(rest) = a.strip_prefix("--to=") {
            if seen_to {
                return Err(NM_EDUP);
            }
            seen_to = true;
            c.to = if rest == "si" {
                Scale::Si
            } else if rest == "auto" || rest == "none" {
                Scale::None
            } else if rest == "iec" {
                Scale::Iec
            } else if rest == "iec-i" {
                Scale::IecI
            } else {
                return Err(NM_EFORMAT);
            };
        } else if let Some(rest) = a.strip_prefix("--from=") {
            if seen_from {
                return Err(NM_EDUP);
            }
            seen_from = true;
            c.from = if rest == "none" {
                PMode::None
            } else if rest == "auto" {
                PMode::Auto
            } else if rest == "si" {
                PMode::Si
            } else if rest == "iec" {
                PMode::Iec
            } else if rest == "iec-i" {
                PMode::IecI
            } else {
                return Err(NM_EFORMAT);
            };
        } else if let Some(rest) = a.strip_prefix("--round=") {
            if seen_round {
                return Err(NM_EDUP);
            }
            seen_round = true;
            c.round = if rest == "up" {
                Round::Up
            } else if rest == "down" {
                Round::Down
            } else if rest == "from-zero" {
                Round::FromZero
            } else if rest == "towards-zero" || rest == "to-zero" {
                Round::TowardsZero
            } else if rest == "nearest" {
                Round::Nearest
            } else {
                return Err(NM_EFORMAT);
            };
        } else if let Some(rest) = a.strip_prefix("--suffix=") {
            if seen_suffix {
                return Err(NM_EDUP);
            }
            seen_suffix = true;
            c.suffix = Suffix::parse_text(rest)?;
        } else if let Some(rest) = a.strip_prefix("--padding=") {
            if seen_pad {
                return Err(NM_EDUP);
            }
            seen_pad = true;
            c.padding = parse_u32(rest.as_bytes(), PADMAX)?;
        } else if a == "--grouping" {
            if seen_group {
                return Err(NM_EDUP);
            }
            seen_group = true;
            c.grouping = true;
        } else if let Some(rest) = a.strip_prefix("--field=") {
            if seen_field {
                return Err(NM_EDUP);
            }
            seen_field = true;
            let v = parse_u32(rest.as_bytes(), FIELDMAX)?;
            if v == 0 {
                return Err(NM_ERANGE);
            }
            c.field = v;
        } else if let Some(rest) = a.strip_prefix("--delimiter=") {
            if seen_delim {
                return Err(NM_EDUP);
            }
            seen_delim = true;
            let b = rest.as_bytes();
            if b.len() > 1 {
                return Err(NM_EFORMAT);
            }
            c.delim = if b.is_empty() { None } else { b.first().copied() };
        } else if a == "--header" {
            if seen_header {
                return Err(NM_EDUP);
            }
            seen_header = true;
            c.header = 1;
        } else if let Some(rest) = a.strip_prefix("--header=") {
            if seen_header {
                return Err(NM_EDUP);
            }
            seen_header = true;
            c.header = parse_u32(rest.as_bytes(), FIELDMAX)?;
        } else if let Some(rest) = a.strip_prefix("--invalid=") {
            if seen_invalid {
                return Err(NM_EDUP);
            }
            seen_invalid = true;
            c.invalid = if rest == "abort" {
                Invalid::Abort
            } else if rest == "fail" {
                Invalid::Fail
            } else if rest == "warn" {
                Invalid::Warn
            } else if rest == "ignore" {
                Invalid::Ignore
            } else {
                return Err(NM_EFORMAT);
            };
        } else {
            return Err(NM_EINVAL);
        }
        c.argc += 1;
        i += 1;
    }
    if let Some(first) = c.suffix.as_bytes().first() {
        let f = *first;
        if f == b'K' || f == b'k' || f == b'M' || f == b'm' || f == b'G' || f == b'g'
            || f == b'T' || f == b't' || f == b'P' || f == b'p' || f == b'E' || f == b'e'
        {
            return Err(NM_ESTATE);
        }
    }
    Ok(c)
}

/// 十位十分。
fn fmt_tenths(q: u128, k: u8, scale: Scale, grouping: bool, suffix: &Suffix) -> String {
    let mut s = String::new();
    if k == 0 {
        let digits = dec_text(q);
        if grouping {
            s.push_str(&group_digits(&digits));
        } else {
            s.push_str(&digits);
        }
    } else {
        let whole = q / 10;
        let frac = q % 10;
        let digits = dec_text(whole);
        if grouping {
            s.push_str(&group_digits(&digits));
        } else {
            s.push_str(&digits);
        }
        s.push('.');
        s.push((b'0' + frac as u8) as char);
        s.push_str(scale.letter(k));
    }
    s.push_str(&suffix.text());
    s
}

/// 渲染一个数字。
pub fn format_amount(a: &Amount, c: &Conf) -> Result<String, u32> {
    let n = a.normalize()?;
    if n.is_zero() {
        let mut s = String::from("0");
        s.push_str(&c.suffix.text());
        return Ok(pad(&s, c.padding));
    }
    let (q, k) = tenths(&n, c.to.base(), c.round)?;
    let mut s = String::new();
    if n.negative() {
        s.push('-');
    }
    s.push_str(&fmt_tenths(q, k, c.to, c.grouping, &c.suffix));
    Ok(pad(&s, c.padding))
}

/// 右对齐补齐。
pub fn pad(text: &str, width: u32) -> String {
    let mut s = String::new();
    let n = text.len();
    if width as usize > n {
        let mut k = n;
        while k < width as usize {
            s.push(' ');
            k += 1;
        }
    }
    s.push_str(text);
    s
}

/// 字段切分：返回（起始，结束）字节区间。
pub struct FieldRange {
    /// 起点。
    pub start: usize,
    /// 终点（不含）。
    pub end: usize,
}

/// 找第 `field` 段（1 起）；越界给 `None`。
pub fn find_field(line: &[u8], field: u32, delim: Option<u8>) -> Option<FieldRange> {
    if field == 0 {
        return None;
    }
    let mut n = 0u32;
    let mut i = 0usize;
    let single = delim;
    if let Some(sep) = single {
        let mut start = 0usize;
        loop {
            let mut end = i;
            while end < line.len() {
                match line.get(end) {
                    Some(c) => {
                        if *c == sep {
                            break;
                        }
                    }
                    None => break,
                }
                end += 1;
            }
            n += 1;
            if n == field {
                return Some(FieldRange { start, end });
            }
            if end >= line.len() {
                return None;
            }
            end += 1;
            start = end;
            i = end;
            if start > line.len() {
                return None;
            }
        }
    }
    // 空白模式：跳过分隔符后取一段非空白。
    while i < line.len() {
        while i < line.len() {
            match line.get(i) {
                Some(c) => {
                    if *c == b' ' || *c == b'\t' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
        let start = i;
        while i < line.len() {
            match line.get(i) {
                Some(c) => {
                    if *c == b' ' || *c == b'\t' {
                        break;
                    }
                }
                None => break,
            }
            i += 1;
        }
        if start < i {
            n += 1;
            if n == field {
                return Some(FieldRange { start, end: i });
            }
        }
        if i >= line.len() {
            break;
        }
    }
    None
}

/// 行内替换：选中字段换算，其余原样。
pub fn process_line(line: &str, c: &Conf) -> Result<String, u32> {
    let bytes = line.as_bytes();
    let f = match find_field(bytes, c.field, c.delim) {
        Some(v) => v,
        None => return Err(NM_EFORMAT),
    };
    let num = bytes.get(f.start..f.end).unwrap_or(&[]);
    let amount = match Amount::parse(num, c.from, &c.suffix) {
        Ok(a) => a,
        Err(e) => {
            if c.invalid.passes() {
                return Ok(String::from(line));
            }
            return Err(e);
        }
    };
    let rendered = match format_amount(&amount, c) {
        Ok(v) => v,
        Err(e) => {
            if c.invalid.passes() {
                return Ok(String::from(line));
            }
            return Err(e);
        }
    };
    let mut s = String::new();
    let mut i = 0usize;
    while i < f.start {
        if let Some(b) = bytes.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    s.push_str(&rendered);
    i = f.end;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
    Ok(s)
}

/// 一个账本样本。
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

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
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
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
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

    /// 对账：八成功值 + 六失败码。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(NM_EBAL);
        }
        const VALS: [u32; 8] = [1, 4, 5, 20, 9, 8, 8, 11];
        const CODES: [u32; 6] = [NM_EUNIT, NM_EFORMAT, NM_EINVAL, NM_EBYTE, NM_ERANGE, NM_EDUP];
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
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
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
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
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

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let empty = Suffix::empty();

    let _ = l.register(word_cell(
        0,
        OP_PARSE,
        Amount::parse(b"1.5K", PMode::Si, &empty).map(|a| a.power as u32),
    ));

    let _ = l.register(count_cell(1, OP_FORMAT, {
        let c = Conf::blank();
        Amount::parse(b"1500", PMode::Si, &empty).and_then(|a| format_amount(&a, &c)).map(|s| s.len())
    }));

    let _ = l.register(count_cell(2, OP_FORMAT, {
        let mut c = Conf::blank();
        c.to = Scale::IecI;
        Amount::parse(b"1536Ki", PMode::IecI, &empty)
            .and_then(|a| format_amount(&a, &c))
            .map(|s| s.len())
    }));

    let _ = l.register(word_cell(3, OP_ROUND, {
        let mut c = Conf::blank();
        c.round = Round::Nearest;
        Amount::parse(b"1999", PMode::Si, &empty)
            .and_then(|a| a.normalize())
            .and_then(|n| tenths(&n, c.to.base(), c.round))
            .map(|(q, _)| q as u32)
    }));

    let _ = l.register(count_cell(4, OP_GROUP, Ok(group_digits("1234567").len())));

    let _ = l.register(count_cell(5, OP_FIELD, {
        let mut c = Conf::blank();
        c.field = 2;
        process_line("a 1500K c", &c).map(|s| s.len())
    }));

    let _ = l.register(count_cell(6, OP_PAD, {
        let mut c = Conf::blank();
        c.padding = 8;
        Amount::parse(b"1500", PMode::Si, &empty).and_then(|a| format_amount(&a, &c)).map(|s| s.len())
    }));

    let _ = l.register(count_cell(7, OP_LINE, {
        let mut c = Conf::blank();
        c.field = 2;
        c.from = PMode::IecI;
        c.suffix = Suffix::parse(b"B").unwrap_or(Suffix::empty());
        process_line("usage 1536KiB", &c).map(|s| s.len())
    }));

    let _ = l.register(word_cell(
        8,
        OP_PARSE,
        Amount::parse(b"1.5Q", PMode::Si, &empty).map(|a| a.mant as u32),
    ));

    let _ = l.register(word_cell(
        9,
        OP_PARSE,
        Amount::parse(b"1.5K", PMode::None, &empty).map(|a| a.mant as u32),
    ));

    let _ = l.register(word_cell(
        10,
        OP_PARSE,
        Amount::parse(b"", PMode::Si, &empty).map(|a| a.mant as u32),
    ));

    let _ = l.register(word_cell(
        11,
        OP_PARSE,
        Amount::parse(b"1.5.5K", PMode::Si, &empty).map(|a| a.mant as u32),
    ));

    let _ = l.register(word_cell(
        12,
        OP_PARSE,
        Amount::parse(b"99999999999999999999", PMode::Si, &empty).map(|a| a.mant as u32),
    ));

    let _ = l.register(word_cell(13, OP_OPT, parse_conf(&["--to=si", "--to=iec"]).map(|c| c.argc as u32)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let empty = Suffix::empty();

    out[0] = {
        let a = Amount::parse(b"1536", PMode::Auto, &empty).is_ok();
        let b = match Amount::parse_text("1.5K", PMode::Auto, &empty) {
            Ok(v) => v.power == 1 && v.mant == 15 && v.exp == 1,
            Err(_) => false,
        };
        let c = Amount::parse_text("1.5Ki", PMode::Auto, &empty).map(|v| v.base) == Ok(1024);
        a && b && c
    };

    out[1] = {
        let c = Conf::blank();
        let rendered = Amount::parse_text("1500", PMode::Si, &empty)
            .and_then(|a| format_amount(&a, &c));
        let rendered2 = Amount::parse_text("1536", PMode::Si, &empty)
            .and_then(|a| format_amount(&a, &c));
        match (rendered, rendered2) {
            (Ok(a), Ok(b)) => a.as_str() == "1.5K" && b.as_str() == "1.5K",
            _ => false,
        }
    };

    out[2] = {
        let a = Amount::parse_text("1536Ki", PMode::IecI, &empty)
            .and_then(|v| v.normalize())
            .map(|n| n.int);
        a == Ok(1572864)
    };

    out[3] = {
        let mut c = Conf::blank();
        c.field = 2;
        c.to = Scale::Iec;
        let line = process_line("x 2048 y", &c);
        line == Ok(String::from("x 2.0K y"))
    };

    out
}

/// 判据集合。
pub fn run_unx11556_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11556.numfmt");

    s.add(
        "NM01-si 解析",
        {
            let e = Suffix::empty();
            let a = Amount::parse(b"1500", PMode::Si, &e).map(|v| (v.mant, v.exp, v.power)) == Ok((1500, 0, 0));
            let b = Amount::parse(b"1.5K", PMode::Si, &e).map(|v| (v.mant, v.exp, v.power)) == Ok((15, 1, 1));
            let c = Amount::parse(b"2M", PMode::Si, &e).map(|v| v.power) == Ok(2);
            let d = Amount::parse(b"-1.5G", PMode::Si, &e).map(|v| (v.neg, v.power)) == Ok((true, 3));
            let f = Amount::parse(b"1.5Ki", PMode::Si, &e) == Err(NM_EFORMAT);
            a && b && c && d && f
        },
        "static",
    );

    s.add(
        "NM02-iec-i 解析",
        {
            let e = Suffix::empty();
            let a = Amount::parse(b"1536Ki", PMode::IecI, &e).map(|v| v.base) == Ok(1024);
            let b = Amount::parse(b"1536K", PMode::IecI, &e) == Err(NM_EFORMAT);
            let c = Amount::parse(b"1536K", PMode::Iec, &e).map(|v| v.power) == Ok(1);
            let d = Amount::parse(b"1536Ki", PMode::Iec, &e) == Err(NM_EFORMAT);
            let f = Amount::parse(b"1536KiB", PMode::IecI, &Suffix::parse(b"B").unwrap_or(e))
                .map(|v| v.power)
                == Ok(1);
            a && b && c && d && f
        },
        "static",
    );

    s.add(
        "NM03-to=si 人读换算",
        {
            let e = Suffix::empty();
            let c = Conf::blank();
            let a = Amount::parse(b"1500", PMode::Si, &e).and_then(|v| format_amount(&v, &c))
                == Ok(String::from("1.5K"));
            let b = Amount::parse(b"1000", PMode::Si, &e).and_then(|v| format_amount(&v, &c))
                == Ok(String::from("1.0K"));
            let d = Amount::parse(b"999", PMode::Si, &e).and_then(|v| format_amount(&v, &c))
                == Ok(String::from("999"));
            let f = Amount::parse(b"2500000", PMode::Si, &e).and_then(|v| format_amount(&v, &c))
                == Ok(String::from("2.5M"));
            let g = Amount::parse(b"-1500", PMode::Si, &e).and_then(|v| format_amount(&v, &c))
                == Ok(String::from("-1.5K"));
            a && b && d && f && g
        },
        "static",
    );

    s.add(
        "NM04-to=iec/iec-i",
        {
            let e = Suffix::empty();
            let mut ci = Conf::blank();
            ci.to = Scale::Iec;
            let mut cii = Conf::blank();
            cii.to = Scale::IecI;
            let a = Amount::parse(b"1536", PMode::Auto, &e).and_then(|v| format_amount(&v, &ci))
                == Ok(String::from("1.5K"));
            let b = Amount::parse(b"1536", PMode::Auto, &e).and_then(|v| format_amount(&v, &cii))
                == Ok(String::from("1.5Ki"));
            let c = Amount::parse(b"1048576", PMode::Auto, &e).and_then(|v| format_amount(&v, &cii))
                == Ok(String::from("1.0Mi"));
            let d = Amount::parse(b"1536Ki", PMode::IecI, &e)
                .and_then(|v| v.normalize())
                .map(|n| n.int)
                == Ok(1572864);
            a && b && c && d
        },
        "static",
    );

    s.add(
        "NM05-进位方向",
        {
            let e = Suffix::empty();
            let run = |raw: &[u8], r: Round| -> Result<String, u32> {
                let mut c = Conf::blank();
                c.round = r;
                Amount::parse(raw, PMode::Si, &e).and_then(|v| format_amount(&v, &c))
            };
            let a = run(b"1999", Round::Down) == Ok(String::from("1.9K"));
            let b = run(b"1999", Round::Nearest) == Ok(String::from("2.0K"));
            let c = run(b"1999", Round::Up) == Ok(String::from("2.0K"));
            let d = run(b"1900", Round::Nearest) == Ok(String::from("1.9K"));
            let f = run(b"999999", Round::Nearest) == Ok(String::from("1.0M"));
            a && b && c && d && f
        },
        "static",
    );

    s.add(
        "NM06-千位分组",
        {
            group_digits("1234567").as_str() == "1,234,567"
                && group_digits("999").as_str() == "999"
                && group_digits("1000").as_str() == "1,000"
                && group_digits("0").as_str() == "0"
                && group_digits("100000").as_str() == "100,000"
        },
        "static",
    );

    s.add(
        "NM07-字段与分隔符",
        {
            let mut c = Conf::blank();
            c.field = 2;
            let a = process_line("a 1500 c", &c) == Ok(String::from("a 1.5K c"));
            let mut d = Conf::blank();
            d.field = 2;
            d.delim = Some(b',');
            let b = process_line("a,1500,c", &d) == Ok(String::from("a,1.5K,c"));
            let mut e = Conf::blank();
            e.field = 9;
            let f = process_line("a 1500", &e) == Err(NM_EFORMAT);
            let g = find_field(b"a,1500,c", 2, Some(b',')).map(|r| r.start) == Some(2);
            a && b && f && g
        },
        "static",
    );

    s.add(
        "NM08-补齐与后缀",
        {
            let e = Suffix::parse(b"B").unwrap_or(Suffix::empty());
            let mut c = Conf::blank();
            c.padding = 8;
            c.suffix = e;
            let a = Amount::parse(b"1500", PMode::Si, &c.suffix).and_then(|v| format_amount(&v, &c))
                == Ok(String::from("   1.5KB"));
            let b = pad("x", 3) == String::from("  x");
            let d = pad("abcd", 2) == String::from("abcd");
            let f = Suffix::parse(b"abcdefghij") == Err(NM_ELEN);
            a && b && d && f
        },
        "static",
    );

    s.add(
        "NM09-表头与坏数处置",
        {
            let mut c = Conf::blank();
            c.invalid = Invalid::Ignore;
            let a = process_line("head", &c) == Ok(String::from("head"));
            let mut d = Conf::blank();
            d.invalid = Invalid::Abort;
            let b = process_line("head", &d) == Err(NM_EBYTE);
            let e = Invalid::Ignore.passes() && !Invalid::Warn.passes();
            let f = parse_conf(&["--header", "--header=2"]) == Err(NM_EDUP);
            a && b && e && f
        },
        "static",
    );

    s.add(
        "NM10-选项矩阵",
        {
            let a = match parse_conf(&["--to=iec-i", "--from=iec", "--round=down", "--padding=8", "--grouping"]) {
                Ok(c) => {
                    c.to == Scale::IecI
                        && c.from == PMode::Iec
                        && c.round == Round::Down
                        && c.padding == 8
                        && c.grouping
                        && c.argc == 5
                }
                Err(_) => false,
            };
            let b = parse_conf(&["--to=si", "--to=si"]) == Err(NM_EDUP);
            let c = parse_conf(&["--to=wat"]) == Err(NM_EFORMAT);
            let d = parse_conf(&["--nope"]) == Err(NM_EINVAL);
            let e = parse_conf(&["--padding=999"]) == Err(NM_ERANGE);
            let f = parse_conf(&["--field=0"]) == Err(NM_ERANGE);
            let g = parse_conf(&["--delimiter=ab"]) == Err(NM_EFORMAT);
            let h = parse_conf(&["--suffix=K"]) == Err(NM_ESTATE);
            a && b && c && d && e && f && g && h
        },
        "static",
    );

    s.add(
        "NM11-闸门次序",
        {
            let e = Suffix::empty();
            let a = Amount::parse(b"", PMode::Si, &e) == Err(NM_EINVAL);
            let b = Amount::parse(b"1.5.5K", PMode::Si, &e) == Err(NM_EBYTE);
            let c = Amount::parse(b"1.5Q", PMode::Si, &e) == Err(NM_EUNIT);
            let d = Amount::parse(b"99999999999999999999", PMode::Si, &e) == Err(NM_ERANGE);
            let f = Amount::parse(b"1.5K", PMode::None, &e) == Err(NM_EFORMAT);
            let g = parse_u32(b"99999", PADMAX) == Err(NM_ERANGE);
            a && b && c && d && f && g
        },
        "static",
    );

    s.add(
        "NM12-账本对账与重放",
        {
            let l = build_ledger();
            l.audit().is_ok()
                && l.replay().is_ok()
                && l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.seal() != 0
        },
        "static",
    );

    s.add(
        "NM13-冻结重放四注入",
        {
            let mut l = build_ledger();
            let frozen = l.freeze().is_ok();
            let rejected = l.register(Sample::blank()).is_err() && l.rejects() == 1;
            let root = l.root() != 0 && l.is_frozen();
            let inj = inject_four();
            frozen && rejected && root && inj[0] && inj[1] && inj[2] && inj[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conf() -> Conf {
        Conf::blank()
    }

    #[test]
    fn parse_plain() {
        let e = Suffix::empty();
        assert_eq!(Amount::parse(b"0", PMode::Si, &e).map(|a| a.mant), Ok(0));
        assert_eq!(Amount::parse(b"123456", PMode::Si, &e).map(|a| a.mant), Ok(123456));
        assert_eq!(Amount::parse(b"1.5", PMode::Si, &e).map(|a| (a.mant, a.exp)), Ok((15, 1)));
        assert_eq!(Amount::parse(b"+42", PMode::Si, &e).map(|a| a.mant), Ok(42));
    }

    #[test]
    fn parse_errors() {
        let e = Suffix::empty();
        assert_eq!(Amount::parse(b"-", PMode::Si, &e), Err(NM_EBYTE));
        assert_eq!(Amount::parse(b".5", PMode::Si, &e).map(|a| (a.mant, a.exp)), Ok((5, 1)));
        assert_eq!(Amount::parse(b"1..5", PMode::Si, &e), Err(NM_EBYTE));
        assert_eq!(Amount::parse(b"1.5Z", PMode::Si, &e), Err(NM_EUNIT));
        assert_eq!(Amount::parse(b"1.5Kx", PMode::Si, &e), Err(NM_EBYTE));
    }

    #[test]
    fn normalize_values() {
        let e = Suffix::empty();
        assert_eq!(
            Amount::parse(b"1536Ki", PMode::IecI, &e).and_then(|a| a.normalize()).map(|n| n.int),
            Ok(1572864)
        );
        assert_eq!(
            Amount::parse(b"1.5K", PMode::Si, &e).and_then(|a| a.normalize()).map(|n| n.int),
            Ok(1500)
        );
        assert_eq!(
            Amount::parse(b"2.5", PMode::Si, &e).and_then(|a| a.normalize()).map(|n| (n.int, n.rem, n.den)),
            Ok((2, 5, 10))
        );
    }

    #[test]
    fn rounding_table() {
        assert_eq!(Round::Nearest.apply(19, 9, 10), 20);
        assert_eq!(Round::Nearest.apply(19, 4, 10), 19);
        assert_eq!(Round::Down.apply(19, 9, 10), 19);
        assert_eq!(Round::Up.apply(19, 1, 10), 20);
        assert_eq!(Round::TowardsZero.apply(3, 5, 10), 3);
        assert_eq!(Round::FromZero.apply(3, 1, 10), 4);
        assert_eq!(Round::Nearest.apply(7, 0, 10), 7);
    }

    #[test]
    fn format_table() {
        let e = Suffix::empty();
        let c = conf();
        let f = |raw: &[u8]| {
            Amount::parse(raw, PMode::Auto, &e).and_then(|a| format_amount(&a, &c))
        };
        assert_eq!(f(b"0"), Ok(String::from("0")));
        assert_eq!(f(b"1"), Ok(String::from("1")));
        assert_eq!(f(b"1500"), Ok(String::from("1.5K")));
        assert_eq!(f(b"1000000"), Ok(String::from("1.0M")));
        assert_eq!(f(b"1500000000"), Ok(String::from("1.5G")));
        assert_eq!(f(b"-2000"), Ok(String::from("-2.0K")));
    }

    #[test]
    fn line_rewrite() {
        let mut c = conf();
        c.field = 3;
        assert_eq!(process_line("a b 1500", &c), Ok(String::from("a b 1.5K")));
        c.delim = Some(b':');
        c.field = 1;
        assert_eq!(process_line("1500:ok", &c), Ok(String::from("1.5K:ok")));
        c.delim = None;
        c.field = 2;
        c.from = PMode::None;
        assert_eq!(process_line("x 1500K", &c), Err(NM_EFORMAT));
    }

    #[test]
    fn grouping_and_padding() {
        let mut c = conf();
        c.to = Scale::None;
        c.grouping = true;
        let e = Suffix::empty();
        assert_eq!(
            Amount::parse(b"1234567", PMode::Si, &e).and_then(|a| format_amount(&a, &c)),
            Ok(String::from("1,234,567"))
        );
        c.padding = 12;
        assert_eq!(
            Amount::parse(b"1234567", PMode::Si, &e).and_then(|a| format_amount(&a, &c)),
            Ok(String::from("   1,234,567"))
        );
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(8).map(|s| s.code), Some(NM_EUNIT));
    }

    #[test]
    fn opts_gates() {
        assert_eq!(parse_conf(&["--to=si", "--to=si"]), Err(NM_EDUP));
        assert_eq!(parse_conf(&["--from=wat"]), Err(NM_EFORMAT));
        assert_eq!(parse_conf(&["--invalid=wat"]), Err(NM_EFORMAT));
        assert_eq!(parse_conf(&["-z"]), Err(NM_EINVAL));
        let c = parse_conf(&["--from=none", "--suffix=B"]).unwrap_or(Conf::blank());
        assert_eq!(c.from, PMode::None);
        assert_eq!(c.suffix.text(), String::from("B"));
    }
}
