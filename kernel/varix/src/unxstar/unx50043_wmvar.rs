//! UNX-F50043 动态变量水印（页码/日期/用户名占位符族）（0x43180 段）
//!
//! 锚点：UNX-F50043-J1 占位符 ×10 变量注入逐项展开一致；判据锚定 §AI-63 §2 专题二。
//!
//! 数据段 `SEG_BASE = 0x43180`：诊断码闭集 10 条（1..=10），全部在注入核或
//! 判据里被真实构造。
//!
//! 结构清单：
//! * `VAR_TABLE` 注册表：10 个占位符（page/pages/date/time/user/docid/dept/
//!   copy/seq/host）各带种类码与合法性规则；
//! * `Template` 解析器：`${name}` / `${name:03}` 零填充 / `${date:%Y-%m-%d}`
//!   格式串 / `$$` 转义；解析期就把未知变量与非法规格挡掉；
//! * `VarContext` 求值上下文 + `render` 展开器（输出上限 `MAX_OUT`）；
//! * `VarLedger` 账本：按变量种类记账（次数/字节），摘要用于确定性断言；
//! * 日期内核：`civil_from_days`/`days_from_civil` 双向互逆（Hinnant 算法），
//!   年积日与星期都是纯整数推导，不依赖任何时区数据库；
//! * 注入核：九类坏模板（空/未闭合/孤立 $/空名/大写名/未知变量/坏补零/
//!   超宽补零/坏格式）逐类断言具体诊断码。
//!
//! 为什么解析期就拒绝未知变量：水印一旦带着 `${user}` 原样印到纸上，等于把
//! 模板 bug 直接交付给读者。宁可渲染失败，也不做「原样透传」的兜底。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43180 独占 0x20）。
pub const SEG_BASE: u32 = 0x43180;

/// 展开结果字节上限。水印是「一行字」，超过 512 字节必然是被模板放大攻击。
pub const MAX_OUT: usize = 512;
/// 零填充宽度上限。再宽就不是水印而是编号牌了。
pub const PAD_MAX: u8 = 32;

pub const E_EMPTY: u16 = 1;
pub const E_UNTERMINATED: u16 = 2;
pub const E_LONE_DOLLAR: u16 = 3;
pub const E_EMPTY_NAME: u16 = 4;
pub const E_BAD_NAME: u16 = 5;
pub const E_UNKNOWN_VAR: u16 = 6;
pub const E_BAD_PAD: u16 = 7;
pub const E_PAD_TOO_WIDE: u16 = 8;
pub const E_BAD_FMT: u16 = 9;
pub const E_TOO_LONG: u16 = 10;

/// 变量水印域错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarError {
    Empty,
    Unterminated,
    LoneDollar,
    EmptyName,
    BadName,
    UnknownVar,
    BadPad,
    PadTooWide,
    BadFmt,
    TooLong,
}

impl VarError {
    pub fn code(self) -> u32 {
        let n = match self {
            VarError::Empty => E_EMPTY,
            VarError::Unterminated => E_UNTERMINATED,
            VarError::LoneDollar => E_LONE_DOLLAR,
            VarError::EmptyName => E_EMPTY_NAME,
            VarError::BadName => E_BAD_NAME,
            VarError::UnknownVar => E_UNKNOWN_VAR,
            VarError::BadPad => E_BAD_PAD,
            VarError::PadTooWide => E_PAD_TOO_WIDE,
            VarError::BadFmt => E_BAD_FMT,
            VarError::TooLong => E_TOO_LONG,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            VarError::Empty => "template is empty",
            VarError::Unterminated => "placeholder missing closing brace",
            VarError::LoneDollar => "dollar not followed by dollar or brace",
            VarError::EmptyName => "placeholder name is empty",
            VarError::BadName => "placeholder name charset invalid",
            VarError::UnknownVar => "placeholder not in registry",
            VarError::BadPad => "padding spec invalid",
            VarError::PadTooWide => "padding width out of range",
            VarError::BadFmt => "datetime format token invalid",
            VarError::TooLong => "expanded output exceeds cap",
        }
    }
}

/// 占位符种类（与 `VAR_TABLE` 一一对应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    Page,
    Pages,
    Date,
    Time,
    User,
    DocId,
    Dept,
    Copy,
    Seq,
    Host,
}

impl VarKind {
    pub fn code(self) -> u8 {
        match self {
            VarKind::Page => 0,
            VarKind::Pages => 1,
            VarKind::Date => 2,
            VarKind::Time => 3,
            VarKind::User => 4,
            VarKind::DocId => 5,
            VarKind::Dept => 6,
            VarKind::Copy => 7,
            VarKind::Seq => 8,
            VarKind::Host => 9,
        }
    }

    /// 数字类变量才允许零填充——给「研发中心」补零没有语义。
    pub fn is_numeric(self) -> bool {
        matches!(
            self,
            VarKind::Page | VarKind::Pages | VarKind::Copy | VarKind::Seq
        )
    }
}

/// 注册表条目。`code` 冗余保存种类码，导出给 API 时不必再查种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VarDef {
    pub name: &'static str,
    pub kind: VarKind,
    pub code: u8,
}

/// 占位符注册表（10 条，顺序即文档顺序，查表用线性扫描——10 条不值得哈希）。
pub const VAR_TABLE: [VarDef; 10] = [
    VarDef { name: "page", kind: VarKind::Page, code: 0 },
    VarDef { name: "pages", kind: VarKind::Pages, code: 1 },
    VarDef { name: "date", kind: VarKind::Date, code: 2 },
    VarDef { name: "time", kind: VarKind::Time, code: 3 },
    VarDef { name: "user", kind: VarKind::User, code: 4 },
    VarDef { name: "docid", kind: VarKind::DocId, code: 5 },
    VarDef { name: "dept", kind: VarKind::Dept, code: 6 },
    VarDef { name: "copy", kind: VarKind::Copy, code: 7 },
    VarDef { name: "seq", kind: VarKind::Seq, code: 8 },
    VarDef { name: "host", kind: VarKind::Host, code: 9 },
];

/// 按名字查注册表；未登记返回 `None`（调用方转成 `UnknownVar`）。
pub fn lookup(name: &str) -> Option<VarDef> {
    let mut i = 0usize;
    while i < VAR_TABLE.len() {
        if let Some(d) = VAR_TABLE.get(i) {
            if d.name == name {
                return Some(*d);
            }
        }
        i += 1;
    }
    None
}

/// 日期时间格式记号。刻意不含 `String`：注册表与默认格式表都要能做成 `const`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FmtTok {
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    DayOfYear,
    Weekday,
    Sep(char),
}

const DATE_DEFAULT: [FmtTok; 5] = [
    FmtTok::Year,
    FmtTok::Sep('-'),
    FmtTok::Month,
    FmtTok::Sep('-'),
    FmtTok::Day,
];

const TIME_DEFAULT: [FmtTok; 5] = [
    FmtTok::Hour,
    FmtTok::Sep(':'),
    FmtTok::Minute,
    FmtTok::Sep(':'),
    FmtTok::Second,
];

const WEEKDAY_CN: [&str; 7] = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

/// 一个已解析的占位符。`name` 保留原文便于账本与报错定位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarSpec {
    pub name: String,
    pub kind: VarKind,
    pub pad: u8,
    pub fmt: Vec<FmtTok>,
}

/// 模板片段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg {
    Lit(String),
    Var(VarSpec),
}

/// 解析后的模板。`var_count`/`lit_bytes` 是解析期算好的统计量，
/// 渲染时不必再数一遍（渲染是热路径，水印每页都要刷）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub segs: Vec<Seg>,
    pub var_count: usize,
    pub lit_bytes: usize,
}

/// 求值上下文。日期用「自 1970-01-01 起的天数 + 当日秒数」表达，
/// 不用 `tm` 结构：内核里没有时区库，也不需要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarContext {
    pub page: u32,
    pub pages: u32,
    pub epoch_day: i64,
    pub secs: u32,
    pub user: String,
    pub docid: String,
    pub dept: String,
    pub copy: u32,
    pub seq: u32,
    pub host: String,
}

impl VarContext {
    /// 判据用的固定上下文（第 7/120 页 · 2024-02-17 · 09:05:03 · 研发中心）。
    pub fn demo() -> VarContext {
        VarContext {
            page: 7,
            pages: 120,
            epoch_day: 19770,
            secs: 9 * 3600 + 5 * 60 + 3,
            user: String::from("li.wei"),
            docid: String::from("DOC-2026-017"),
            dept: String::from("研发中心"),
            copy: 2,
            seq: 42,
            host: String::from("wks-07"),
        }
    }
}

/// 账本单条：哪类变量展开出了多少字节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expand {
    pub kind: VarKind,
    pub out_len: u16,
}

/// 展开账本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarLedger {
    pub entries: Vec<Expand>,
    pub var_bytes: usize,
    pub out_bytes: usize,
}

impl VarLedger {
    pub fn new() -> VarLedger {
        VarLedger {
            entries: Vec::new(),
            var_bytes: 0,
            out_bytes: 0,
        }
    }

    pub fn record(&mut self, kind: VarKind, len: usize) {
        self.var_bytes += len;
        self.entries.push(Expand {
            kind,
            out_len: len as u16,
        });
    }

    pub fn count(&self, kind: VarKind) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.kind == kind {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h ^= e.kind.code() as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= e.out_len as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 日期内核（纯整数，无时区）
// ---------------------------------------------------------------------------

/// 自 1970-01-01 起的天数 → 公历年月日（Hinnant civil_from_days）。
pub fn civil_from_days(z0: i64) -> (i32, u32, u32) {
    let z = z0 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m, d)
}

/// 公历年月日 → 自 1970-01-01 起的天数（`civil_from_days` 的逆）。
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let yy = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = if yy >= 0 { yy } else { yy - 399 } / 400;
    let yoe = yy - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 1 基年积日。
pub fn day_of_year(epoch_day: i64) -> u32 {
    let (y, _, _) = civil_from_days(epoch_day);
    let d = epoch_day - days_from_civil(y, 1, 1) + 1;
    if d < 1 {
        1
    } else {
        d as u32
    }
}

/// 星期索引（0=周日）。1970-01-01 是周四 → 偏置 +4。
pub fn weekday_index(epoch_day: i64) -> u32 {
    let mut w = (epoch_day + 4) % 7;
    if w < 0 {
        w += 7;
    }
    w as u32
}

/// 按记号表格式化日期时间。`fmt` 为空时由调用方给默认表。
pub fn format_epoch(epoch_day: i64, secs: u32, fmt: &[FmtTok]) -> String {
    let (y, m, d) = civil_from_days(epoch_day);
    let hh = secs / 3600;
    let mi = (secs / 60) % 60;
    let ss = secs % 60;
    let doy = day_of_year(epoch_day);
    let wd = weekday_index(epoch_day);
    let mut out = String::new();
    let mut i = 0usize;
    while i < fmt.len() {
        match fmt.get(i) {
            Some(FmtTok::Year) => out.push_str(&format!("{:04}", y)),
            Some(FmtTok::Month) => out.push_str(&format!("{:02}", m)),
            Some(FmtTok::Day) => out.push_str(&format!("{:02}", d)),
            Some(FmtTok::Hour) => out.push_str(&format!("{:02}", hh)),
            Some(FmtTok::Minute) => out.push_str(&format!("{:02}", mi)),
            Some(FmtTok::Second) => out.push_str(&format!("{:02}", ss)),
            Some(FmtTok::DayOfYear) => out.push_str(&format!("{:03}", doy)),
            Some(FmtTok::Weekday) => {
                let name = match WEEKDAY_CN.get(wd as usize) {
                    Some(v) => *v,
                    None => "??",
                };
                out.push_str(name);
            }
            Some(FmtTok::Sep(c)) => out.push(*c),
            None => break,
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 解析器
// ---------------------------------------------------------------------------

fn valid_name(s: &str) -> bool {
    let b = s.as_bytes();
    let first = match b.first() {
        Some(v) => *v,
        None => return false,
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let mut i = 1usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => break,
        };
        let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_';
        if !ok {
            return false;
        }
        i += 1;
    }
    true
}

fn parse_pad(s: &str) -> Result<u8, VarError> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(VarError::BadPad);
    }
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => break,
        };
        if !b.is_ascii_digit() {
            return Err(VarError::BadPad);
        }
        v = v * 10 + (b - b'0') as u32;
        if v > 99 {
            return Err(VarError::PadTooWide);
        }
        i += 1;
    }
    if v == 0 || v > PAD_MAX as u32 {
        return Err(VarError::PadTooWide);
    }
    Ok(v as u8)
}

fn parse_fmt(s: &str) -> Result<Vec<FmtTok>, VarError> {
    let chars: Vec<char> = s.chars().collect();
    let mut out: Vec<FmtTok> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = match chars.get(i) {
            Some(v) => *v,
            None => break,
        };
        if c != '%' {
            out.push(FmtTok::Sep(c));
            i += 1;
            continue;
        }
        let d = match chars.get(i + 1) {
            Some(v) => *v,
            None => return Err(VarError::BadFmt),
        };
        let tok = match d {
            'Y' => FmtTok::Year,
            'm' => FmtTok::Month,
            'd' => FmtTok::Day,
            'H' => FmtTok::Hour,
            'M' => FmtTok::Minute,
            'S' => FmtTok::Second,
            'j' => FmtTok::DayOfYear,
            'a' => FmtTok::Weekday,
            '%' => FmtTok::Sep('%'),
            _ => return Err(VarError::BadFmt),
        };
        out.push(tok);
        i += 2;
    }
    if out.is_empty() {
        return Err(VarError::BadFmt);
    }
    Ok(out)
}

fn parse_body(body: &str) -> Result<VarSpec, VarError> {
    if body.is_empty() {
        return Err(VarError::EmptyName);
    }
    let bytes = body.as_bytes();
    let mut cut: Option<usize> = None;
    let mut k = 0usize;
    while k < bytes.len() {
        if let Some(b':') = bytes.get(k) {
            cut = Some(k);
            break;
        }
        k += 1;
    }
    let (name, spec) = match cut {
        Some(pos) => {
            let a = match body.get(..pos) {
                Some(v) => v,
                None => return Err(VarError::BadName),
            };
            let b = match body.get(pos + 1..) {
                Some(v) => v,
                None => return Err(VarError::BadPad),
            };
            (a, Some(b))
        }
        None => (body, None),
    };
    if !valid_name(name) {
        return Err(VarError::BadName);
    }
    let def = match lookup(name) {
        Some(d) => d,
        None => return Err(VarError::UnknownVar),
    };
    let mut pad = 0u8;
    let mut fmt: Vec<FmtTok> = Vec::new();
    if let Some(s) = spec {
        if s.is_empty() {
            return Err(VarError::BadPad);
        }
        if s.starts_with('%') {
            fmt = parse_fmt(s)?;
        } else {
            pad = parse_pad(s)?;
            // 非数字变量不允许补零：`${dept:03}` 是无意义配置，及早拒。
            if !def.kind.is_numeric() {
                return Err(VarError::BadPad);
            }
        }
    }
    Ok(VarSpec {
        name: String::from(name),
        kind: def.kind,
        pad,
        fmt,
    })
}

/// 解析模板。空模板、未闭合、孤立 `$`、非法名字/规格都在这里失败。
pub fn parse_template(src: &str) -> Result<Template, VarError> {
    if src.is_empty() {
        return Err(VarError::Empty);
    }
    let chars: Vec<char> = src.chars().collect();
    let mut segs: Vec<Seg> = Vec::new();
    let mut lit = String::new();
    let mut var_count = 0usize;
    let mut lit_bytes = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        let c = match chars.get(i) {
            Some(v) => *v,
            None => break,
        };
        if c != '$' {
            lit.push(c);
            lit_bytes += c.len_utf8();
            i += 1;
            continue;
        }
        let nxt = match chars.get(i + 1) {
            Some(v) => *v,
            None => return Err(VarError::LoneDollar),
        };
        if nxt == '$' {
            // `$$` 是转义：合法模板里要出现字面 `$` 只有这一条路。
            lit.push('$');
            lit_bytes += 1;
            i += 2;
            continue;
        }
        if nxt != '{' {
            return Err(VarError::LoneDollar);
        }
        let mut j = i + 2;
        let mut closed = false;
        while j < chars.len() {
            let is_close = match chars.get(j) {
                Some('}') => true,
                _ => false,
            };
            if is_close {
                closed = true;
                break;
            }
            j += 1;
        }
        if !closed {
            return Err(VarError::Unterminated);
        }
        let body: String = match chars.get(i + 2..j) {
            Some(sl) => sl.iter().collect(),
            None => return Err(VarError::Unterminated),
        };
        if !lit.is_empty() {
            segs.push(Seg::Lit(lit));
            lit = String::new();
        }
        segs.push(Seg::Var(parse_body(&body)?));
        var_count += 1;
        i = j + 1;
    }
    if !lit.is_empty() {
        segs.push(Seg::Lit(lit));
    }
    if segs.is_empty() {
        return Err(VarError::Empty);
    }
    Ok(Template {
        segs,
        var_count,
        lit_bytes,
    })
}

// ---------------------------------------------------------------------------
// 求值
// ---------------------------------------------------------------------------

fn zero_pad(s: String, width: u8) -> String {
    let n = s.len();
    if n >= width as usize {
        return s;
    }
    let mut out = String::new();
    let mut i = n;
    while i < width as usize {
        out.push('0');
        i += 1;
    }
    out.push_str(&s);
    out
}

fn var_value(spec: &VarSpec, ctx: &VarContext) -> String {
    let raw = match spec.kind {
        VarKind::Page => format!("{}", ctx.page),
        VarKind::Pages => format!("{}", ctx.pages),
        VarKind::Date => {
            if spec.fmt.is_empty() {
                format_epoch(ctx.epoch_day, ctx.secs, &DATE_DEFAULT)
            } else {
                format_epoch(ctx.epoch_day, ctx.secs, &spec.fmt)
            }
        }
        VarKind::Time => {
            if spec.fmt.is_empty() {
                format_epoch(ctx.epoch_day, ctx.secs, &TIME_DEFAULT)
            } else {
                format_epoch(ctx.epoch_day, ctx.secs, &spec.fmt)
            }
        }
        VarKind::User => ctx.user.clone(),
        VarKind::DocId => ctx.docid.clone(),
        VarKind::Dept => ctx.dept.clone(),
        VarKind::Copy => format!("{}", ctx.copy),
        VarKind::Seq => format!("{}", ctx.seq),
        VarKind::Host => ctx.host.clone(),
    };
    if spec.pad > 0 && spec.kind.is_numeric() {
        zero_pad(raw, spec.pad)
    } else {
        raw
    }
}

/// 展开模板并把每条变量记进账本。输出超 `MAX_OUT` 返回 `TooLong`。
pub fn render_with_ledger(
    t: &Template,
    ctx: &VarContext,
    led: &mut VarLedger,
) -> Result<String, VarError> {
    let mut out = String::new();
    let mut i = 0usize;
    while i < t.segs.len() {
        match t.segs.get(i) {
            Some(Seg::Lit(s)) => {
                out.push_str(s);
                if out.len() > MAX_OUT {
                    return Err(VarError::TooLong);
                }
            }
            Some(Seg::Var(spec)) => {
                let v = var_value(spec, ctx);
                let n = v.len();
                out.push_str(&v);
                led.record(spec.kind, n);
                if out.len() > MAX_OUT {
                    return Err(VarError::TooLong);
                }
            }
            None => break,
        }
        i += 1;
    }
    led.out_bytes = out.len();
    Ok(out)
}

/// 展开模板（内部建一次性账本）。
pub fn render(t: &Template, ctx: &VarContext) -> Result<String, VarError> {
    let mut led = VarLedger::new();
    render_with_ledger(t, ctx, &mut led)
}

/// 一步到位：源码 → 展开文本。
pub fn expand_once(src: &str, ctx: &VarContext) -> Result<String, VarError> {
    let t = parse_template(src)?;
    render(&t, ctx)
}

/// 模板里出现的变量种类（按出现序）。
pub fn vars_in(t: &Template) -> Vec<VarKind> {
    let mut out: Vec<VarKind> = Vec::new();
    let mut i = 0usize;
    while i < t.segs.len() {
        if let Some(Seg::Var(v)) = t.segs.get(i) {
            out.push(v.kind);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 注入核：九类坏模板必须逐类抓到
// ---------------------------------------------------------------------------

/// 返回（抓到的坏样本数, 总数）。每类断言固定诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    if parse_template("").err() == Some(VarError::Empty) {
        caught += 1;
    }
    if parse_template("第${page 页").err() == Some(VarError::Unterminated) {
        caught += 1;
    }
    if parse_template("cost $5").err() == Some(VarError::LoneDollar) {
        caught += 1;
    }
    if parse_template("${}").err() == Some(VarError::EmptyName) {
        caught += 1;
    }
    if parse_template("${Page}").err() == Some(VarError::BadName) {
        caught += 1;
    }
    if parse_template("${nope}").err() == Some(VarError::UnknownVar) {
        caught += 1;
    }
    if parse_template("${page:0x}").err() == Some(VarError::BadPad) {
        caught += 1;
    }
    if parse_template("${page:44}").err() == Some(VarError::PadTooWide) {
        caught += 1;
    }
    if parse_template("${date:%Q}").err() == Some(VarError::BadFmt) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 用「独立于 FmtTok 管线」的格式化路径算出 10 个变量的期望值。
/// J1 要证明的是展开器与注册表一致，不是「自己等于自己」。
fn expected_values(ctx: &VarContext) -> Vec<(&'static str, String)> {
    let (y, m, d) = civil_from_days(ctx.epoch_day);
    let hh = ctx.secs / 3600;
    let mi = (ctx.secs / 60) % 60;
    let ss = ctx.secs % 60;
    let mut v: Vec<(&'static str, String)> = Vec::new();
    v.push(("page", format!("{}", ctx.page)));
    v.push(("pages", format!("{}", ctx.pages)));
    v.push(("date", format!("{:04}-{:02}-{:02}", y, m, d)));
    v.push(("time", format!("{:02}:{:02}:{:02}", hh, mi, ss)));
    v.push(("user", ctx.user.clone()));
    v.push(("docid", ctx.docid.clone()));
    v.push(("dept", ctx.dept.clone()));
    v.push(("copy", format!("{}", ctx.copy)));
    v.push(("seq", format!("{}", ctx.seq)));
    v.push(("host", ctx.host.clone()));
    v
}

pub fn run_unx50043_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50043");
    let ctx = VarContext::demo();

    // J1 主体：10 个变量逐个注入、逐项展开完全一致
    let wants = expected_values(&ctx);
    let mut hit = 0usize;
    let mut i = 0usize;
    while i < wants.len() {
        let tpl = format!("${{{}}}", wants[i].0);
        if let Ok(got) = expand_once(&tpl, &ctx) {
            if got == wants[i].1 {
                hit += 1;
            }
        }
        i += 1;
    }
    let ok = hit == 10;
    let det = format!("逐项 {}/10", hit);
    s.add_dyn("50043-J1-十字变量展开", ok, &det);

    // J1 之二：10 变量同串一次展开，变量段计数为 10、字面字节为 9 个分隔符
    let all = "${page}|${pages}|${date}|${time}|${user}|${docid}|${dept}|${copy}|${seq}|${host}";
    let mut all_ok = false;
    let mut joined = String::new();
    let mut vars = 0usize;
    if let Ok(t) = parse_template(all) {
        vars = t.var_count;
        let mut want = String::new();
        let mut k = 0usize;
        while k < wants.len() {
            if k > 0 {
                want.push('|');
            }
            want.push_str(&wants[k].1);
            k += 1;
        }
        if let Ok(got) = render(&t, &ctx) {
            all_ok = got == want && !got.contains('$') && t.lit_bytes == 9;
            joined = got;
        }
    }
    let ok = all_ok && vars == 10 && !joined.is_empty();
    let det = format!("变量段={} 字面字节=9", vars);
    s.add_dyn("50043-J1-全串展开", ok, &det);

    // J1 之三：账本按种类记账，10 类各出现一次
    let mut led = VarLedger::new();
    let mut led_ok = false;
    if let Ok(t) = parse_template(all) {
        if render_with_ledger(&t, &ctx, &mut led).is_ok() {
            let mut k = 0usize;
            let mut good = 0usize;
            while k < VAR_TABLE.len() {
                if let Some(d) = VAR_TABLE.get(k) {
                    if led.count(d.kind) == 1 {
                        good += 1;
                    }
                }
                k += 1;
            }
            led_ok = good == 10 && led.entries.len() == 10;
        }
    }
    let ok = led_ok;
    s.add("50043-J1-账本分类", ok, if ok { "" } else { "账本种类记账不完整" });

    // 注册表自身：10 条且名字唯一（重复登记会让 lookup 静默偏向前一条）
    let mut uniq = 0usize;
    let mut a = 0usize;
    while a < VAR_TABLE.len() {
        let mut b = a + 1;
        let mut dup = false;
        while b < VAR_TABLE.len() {
            if let (Some(x), Some(y)) = (VAR_TABLE.get(a), VAR_TABLE.get(b)) {
                if x.name == y.name {
                    dup = true;
                }
            }
            b += 1;
        }
        if !dup {
            uniq += 1;
        }
        a += 1;
    }
    let ok = VAR_TABLE.len() == 10 && uniq == 10;
    s.add("50043-J1-注册表唯一", ok, if ok { "" } else { "注册表重复或条数不对" });

    // 字面量模板往返：无占位符时渲染恒等于原文，且只有 1 个字面段
    let lit_src = "内部资料 · 请勿外传";
    let mut ok = false;
    if let Ok(t) = parse_template(lit_src) {
        if let Ok(got) = render(&t, &ctx) {
            ok = got == lit_src && t.var_count == 0 && t.segs.len() == 1;
        }
    }
    s.add("50043-J1-字面往返", ok, if ok { "" } else { "字面模板被改写" });

    // 幂等：对纯字面模板二次展开结果不变
    let mut ok = false;
    if let Ok(once) = expand_once(lit_src, &ctx) {
        if let Ok(twice) = expand_once(&once, &ctx) {
            ok = once == twice && twice == lit_src;
        }
    }
    s.add("50043-J1-幂等", ok, if ok { "" } else { "二次展开漂移" });

    // 确定性：两次展开文本与账本摘要逐位一致
    let mut ok = false;
    if let Ok(t) = parse_template(all) {
        let mut la = VarLedger::new();
        let mut lb = VarLedger::new();
        let ra = render_with_ledger(&t, &ctx, &mut la);
        let rb = render_with_ledger(&t, &ctx, &mut lb);
        match (ra, rb) {
            (Ok(x), Ok(y)) => ok = x == y && la.digest() == lb.digest() && la.out_bytes == x.len(),
            _ => ok = false,
        }
    }
    s.add("50043-J1-确定", ok, if ok { "" } else { "两次展开不一致" });

    // 零填充：${page:03} → 007；宽度 1..6 输出长度恒等于宽度
    let mut pad_ok = 0usize;
    let mut w = 1u8;
    while w <= 6 {
        let tpl = format!("${{page:0{}}}", w);
        if let Ok(got) = expand_once(&tpl, &ctx) {
            if got.len() == w as usize && got.ends_with('7') {
                pad_ok += 1;
            }
        }
        w += 1;
    }
    let mut exact = false;
    if let Ok(got) = expand_once("${page:03}", &ctx) {
        exact = got == "007";
    }
    let ok = pad_ok == 6 && exact;
    let det = format!("宽度1..6 {}/6", pad_ok);
    s.add_dyn("50043-J1-零填充", ok, &det);

    // 日期锚点：epoch 0 = 1970-01-01；day 31 = 1970-02-01；2000-03-01 往返
    let mut ok = false;
    if let (Ok(d0), Ok(d31), Ok(d2k)) = (
        expand_once("${date}", &ctx_with_day(0)),
        expand_once("${date}", &ctx_with_day(31)),
        expand_once("${date}", &ctx_with_day(11017)),
    ) {
        ok = d0 == "1970-01-01" && d31 == "1970-02-01" && d2k == "2000-03-01";
    }
    s.add("50043-J1-日期锚点", ok, if ok { "" } else { "日期锚点漂移" });

    // civil ↔ days 双向互逆（抽样 200 天，跨闰年与世纪末）
    let mut rt = 0usize;
    let mut n = 0usize;
    let mut z = -800i64;
    while z <= 32000 {
        let (y, m, d) = civil_from_days(z);
        if days_from_civil(y, m, d) == z {
            rt += 1;
        }
        n += 1;
        z += 164;
    }
    let ok = rt == n && n >= 200;
    let det = format!("互逆 {}/{}", rt, n);
    s.add_dyn("50043-J1-日历互逆", ok, &det);

    // 年积日：单调不减且首值为 001；%j 三位定宽
    let mut mono = true;
    let mut prev = 0u32;
    let mut day = 0i64;
    while day < 365 {
        let v = day_of_year(day);
        if day > 0 && v < prev {
            mono = false;
        }
        prev = v;
        day += 1;
    }
    let mut doy_txt = false;
    if let Ok(got) = expand_once("${date:%j}", &ctx_with_day(59)) {
        doy_txt = got == "060" && got.len() == 3;
    }
    let ok = mono && day_of_year(0) == 1 && doy_txt;
    s.add("50043-J1-年积日", ok, if ok { "" } else { "年积日不单调" });

    // 星期锚点：1970-01-01 周四、1970-01-04 周日
    let mut ok = false;
    if let (Ok(a), Ok(b)) = (
        expand_once("${date:%a}", &ctx_with_day(0)),
        expand_once("${date:%a}", &ctx_with_day(3)),
    ) {
        ok = a == "周四" && b == "周日" && weekday_index(0) == 4 && weekday_index(3) == 0;
    }
    s.add("50043-J1-星期锚点", ok, if ok { "" } else { "星期推导错" });

    // 转义：$${page} 渲染成字面 ${page}，变量段为 0
    let mut ok = false;
    if let Ok(t) = parse_template("$${page}") {
        if let Ok(got) = render(&t, &ctx) {
            ok = got == "${page}" && t.var_count == 0;
        }
    }
    s.add("50043-J1-转义", ok, if ok { "" } else { "$$ 未按字面输出" });

    // 四种解析期拒绝：未闭合 / 孤立 $ / 空名 / 大写名
    let ok = parse_template("${page").err() == Some(VarError::Unterminated)
        && parse_template("$x").err() == Some(VarError::LoneDollar)
        && parse_template("${}").err() == Some(VarError::EmptyName)
        && parse_template("${Page}").err() == Some(VarError::BadName);
    s.add("50043-J1-语法四拒", ok, if ok { "" } else { "坏语法漏过" });

    // 未知变量 / 坏补零 / 超宽补零 / 坏格式
    let ok = parse_template("${nope}").err() == Some(VarError::UnknownVar)
        && parse_template("${page:0x}").err() == Some(VarError::BadPad)
        && parse_template("${page:44}").err() == Some(VarError::PadTooWide)
        && parse_template("${date:%Q}").err() == Some(VarError::BadFmt);
    s.add("50043-J1-规格四拒", ok, if ok { "" } else { "坏规格漏过" });

    // 非数字变量禁止补零（语义性拒绝，不是语法错）
    let ok = parse_template("${dept:03}").err() == Some(VarError::BadPad);
    s.add("50043-J1-补零限数字", ok, if ok { "" } else { "文本变量被补零" });

    // 输出上限：100 个 ${user} 必然超 512 字节
    let mut src = String::new();
    let mut k = 0usize;
    while k < 100 {
        src.push_str("${user}");
        k += 1;
    }
    let mut over = false;
    if let Ok(t) = parse_template(&src) {
        over = render(&t, &ctx).err() == Some(VarError::TooLong);
    }
    s.add("50043-J1-超长必拒", over, if over { "" } else { "超长未拦" });

    // 注入核汇总：九类坏模板逐类命中具体码
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50043-J1-注入必抓", ok, &det);

    s
}

/// 以固定上下文为底、只替换日期的便捷构造（判据用）。
fn ctx_with_day(day: i64) -> VarContext {
    let mut c = VarContext::demo();
    c.epoch_day = day;
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50043_checks_green() {
        let set = run_unx50043_checks();
        assert!(set.all_passed(), "UNX-F50043 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50043_civil_roundtrip() {
        let mut z = -3000i64;
        while z < 40000 {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
            z += 97;
        }
    }

    #[test]
    fn unx50043_expand_ten_vars() {
        let ctx = VarContext::demo();
        let wants = expected_values(&ctx);
        let mut i = 0usize;
        while i < wants.len() {
            let tpl = format!("${{{}}}", wants[i].0);
            let got = expand_once(&tpl, &ctx);
            match got {
                Ok(v) => assert_eq!(v, wants[i].1),
                Err(e) => assert!(false, "expand failed: {:?}", e),
            }
            i += 1;
        }
    }
}
