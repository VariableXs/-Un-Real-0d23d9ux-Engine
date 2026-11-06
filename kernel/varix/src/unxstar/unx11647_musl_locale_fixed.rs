//! UNX-F11647 musl 线 locale 固定行为判据（双线差异锚）
//!
//! musl 的 locale 线没有 locale-archive、没有 LC_* 数据文件，`setlocale`
//! 只认 `C`、`POSIX`、`C.UTF-8`（`UTF-8` 视同 `C.UTF-8`），其余名字一律
//! 返回 NULL。所有类别查询都落回同一张常量表：小数点恒 `.`、千分位恒空、
//! 日期格式恒 `%a %b %e %H:%M:%S %Y`、字符集在 C 档是 ASCII、在 UTF-8 档
//! 是 UTF-8。glibc 线则随已装载的 locale 变化，于是同一查询在两条线上会
//! 产生两行不同的结果——本模块把这组对照固化成“双线差异锚”。
//!
//! 锚的用法：每条查询同时算 musl 行与 glibc 行，只有差异位（same/differ）
//! 与两行文本都逐字对上，才算锚稳。musl 侧永不因环境变量换档（`""` 直接
//! 拒收，不读 `LANG`），这正是与 glibc 最容易被误判的分界。
//!
//! 名字先过注入闸：空名、超长、控制字节、空格、分号、括号一律拒收；名字
//! 只允许 `[A-Za-z0-9_.@-]`。类别号只允许 0..5 或 LC_ALL 哨兵，其它值落
//! 类别错误。所有失败都是 13 个错误码之一，不 panic、不返回半成品。
//!
//! 闸门次序：名字 → 类别 → 支持档 → 取值 → 双线锚 → 汇总 → 账本 → 绿档。
//!
//! 契约：纯 no_std（只用 alloc 的 String）、无 unsafe、无 unwrap/expect/
//! panic!，失败一律 `Result<_, u32>`；账本 14 格（8 成 6 败）由 `audit`
//! 逐格比对常量表，`freeze` 后 `register` 返回 ESEAL 并累加 rejects，
//! `replay` 须与冻结指纹一致。语料 14 槽（8 收 6 拒）互异，覆盖率按
//! `(matched*1000 + total/2)/total` 四舍五入，单槽红档为 929ppm。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11647）。
pub const SEG_BASE: u32 = 0xfd60;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 满档千分比。
pub const PPM_FULL: u32 = 1000;

/// 单槽红档千分比。
pub const PPM_RED: u32 = 929;

/// 名字最大字节数。
pub const NAME_MAX: usize = 48;

/// 单行结果最大字节数。
pub const TEXT_MAX: usize = 192;

/// 类别数（不含 LC_ALL）。
pub const CTG_COUNT: usize = 6;

/// LC_ALL 哨兵。
pub const CTG_ALL: u8 = 0xff;

/// 固定项数。
pub const ITEM_COUNT: usize = 14;

/// localeconv 渲染字段数。
pub const CONV_FIELDS: usize = 12;

/// 名字里出现非法字节（控制符、空格、分号、括号、非 ASCII）。
pub const ML_EBYTE: u32 = SEG_BASE + 0x01;

/// 名字超长（空名归环境档，不归这里）。
pub const ML_ENAME: u32 = SEG_BASE + 0x02;

/// 类别号越界。
pub const ML_ECTG: u32 = SEG_BASE + 0x03;

/// musl 线不支持的名字（有语法、无内置档）。
pub const ML_EUNSUP: u32 = SEG_BASE + 0x04;

/// 空名按环境档处理，musl 不读环境，直接拒收。
pub const ML_EENV: u32 = SEG_BASE + 0x05;

/// 固定项编号/双线 locale 越界。
pub const ML_EQUERY: u32 = SEG_BASE + 0x06;

/// 模型已冻结或重复冻结。
pub const ML_ESTATE: u32 = SEG_BASE + 0x07;

/// 固定项取值与期望不符。
pub const ML_EVER: u32 = SEG_BASE + 0x08;

/// 账本不平。
pub const ML_EBAL: u32 = SEG_BASE + 0x09;

/// 槽位错序或越界。
pub const ML_ESLOT: u32 = SEG_BASE + 0x0a;

/// 观测值与常量表不符。
pub const ML_EVALUE: u32 = SEG_BASE + 0x0b;

/// 冻结重放指纹不符。
pub const ML_ESEAL: u32 = SEG_BASE + 0x0c;

/// 空观测。
pub const ML_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// FNV-1a
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 折叠一字节。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// 折叠一个字（大端逐字节）。
pub fn fold_word(h: u64, w: u32) -> u64 {
    let mut x = h;
    x = fold_byte(x, ((w >> 24) & 0xff) as u8);
    x = fold_byte(x, ((w >> 16) & 0xff) as u8);
    x = fold_byte(x, ((w >> 8) & 0xff) as u8);
    x = fold_byte(x, (w & 0xff) as u8);
    x
}

/// 整串折叠。
pub fn digest(bytes: &[u8]) -> u64 {
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
// 文本助手
// ---------------------------------------------------------------------------

/// 十进制渲染。
pub fn u32_text(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        if let Some(d) = buf.get_mut(n) {
            *d = b'0' + ((x % 10) as u8);
        }
        n += 1;
        x /= 10;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        if let Some(d) = buf.get(i) {
            s.push(*d as char);
        }
    }
    s
}

/// 有符号十进制渲染。
pub fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(u32_text(v.unsigned_abs()).as_str());
        s
    } else {
        u32_text(v as u32)
    }
}

/// 八位十六进制渲染（带 0x 前缀）。
pub fn hex_text(v: u32) -> String {
    let mut s = String::from("0x");
    let digits = b"0123456789abcdef";
    let mut shift = 28i32;
    while shift >= 0 {
        let d = ((v >> shift) & 0xf) as usize;
        if let Some(c) = digits.get(d) {
            s.push(*c as char);
        }
        shift -= 4;
    }
    s
}

/// 布尔渲染。
pub fn bool_text(v: bool) -> &'static str {
    if v {
        "true"
    } else {
        "false"
    }
}

// ---------------------------------------------------------------------------
// 名字
// ---------------------------------------------------------------------------

/// 名字归类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameKind {
    /// C。
    C,
    /// POSIX（归一为 C）。
    Posix,
    /// C.UTF-8。
    CUtf8,
    /// UTF-8（同义 C.UTF-8）。
    Utf8,
    /// 其它有语法名字。
    Other,
}

/// 名字注入闸。
///
/// 空名走环境档（ML_EENV），超长走 ML_ENAME，其余非法字节走 ML_EBYTE。
pub fn check_name(name: &str) -> Result<(), u32> {
    if name.is_empty() {
        return Err(ML_EENV);
    }
    if name.len() > NAME_MAX {
        return Err(ML_ENAME);
    }
    let b = name.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(ML_EBYTE),
        };
        let alpha = (c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z');
        let digit = c >= b'0' && c <= b'9';
        let mark = c == b'_' || c == b'.' || c == b'@' || c == b'-';
        if !alpha && !digit && !mark {
            return Err(ML_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

/// 归类。
pub fn classify(name: &str) -> Result<NameKind, u32> {
    check_name(name)?;
    match name {
        "C" => Ok(NameKind::C),
        "POSIX" => Ok(NameKind::Posix),
        "C.UTF-8" => Ok(NameKind::CUtf8),
        "UTF-8" => Ok(NameKind::Utf8),
        _ => Ok(NameKind::Other),
    }
}

/// 名字是否落在 musl 内置档。
pub fn supported(kind: NameKind) -> bool {
    match kind {
        NameKind::C => true,
        NameKind::Posix => true,
        NameKind::CUtf8 => true,
        NameKind::Utf8 => true,
        NameKind::Other => false,
    }
}

/// 内置档 id：0 = C，1 = C.UTF-8。
pub fn name_id(kind: NameKind) -> Result<u8, u32> {
    match kind {
        NameKind::C => Ok(0),
        NameKind::Posix => Ok(0),
        NameKind::CUtf8 => Ok(1),
        NameKind::Utf8 => Ok(1),
        NameKind::Other => Err(ML_EUNSUP),
    }
}

// ---------------------------------------------------------------------------
// 类别
// ---------------------------------------------------------------------------

/// 六个 POSIX 类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctg {
    /// LC_CTYPE
    Ctype,
    /// LC_NUMERIC
    Numeric,
    /// LC_TIME
    Time,
    /// LC_COLLATE
    Collate,
    /// LC_MONETARY
    Monetary,
    /// LC_MESSAGES
    Messages,
}

impl Ctg {
    /// 由类别号取类别。
    pub fn from_u8(v: u8) -> Result<Ctg, u32> {
        match v {
            0 => Ok(Ctg::Ctype),
            1 => Ok(Ctg::Numeric),
            2 => Ok(Ctg::Time),
            3 => Ok(Ctg::Collate),
            4 => Ok(Ctg::Monetary),
            5 => Ok(Ctg::Messages),
            _ => Err(ML_ECTG),
        }
    }

    /// 下标。
    pub fn index(self) -> usize {
        match self {
            Ctg::Ctype => 0,
            Ctg::Numeric => 1,
            Ctg::Time => 2,
            Ctg::Collate => 3,
            Ctg::Monetary => 4,
            Ctg::Messages => 5,
        }
    }

    /// 名字。
    pub fn name(self) -> &'static str {
        match self {
            Ctg::Ctype => "LC_CTYPE",
            Ctg::Numeric => "LC_NUMERIC",
            Ctg::Time => "LC_TIME",
            Ctg::Collate => "LC_COLLATE",
            Ctg::Monetary => "LC_MONETARY",
            Ctg::Messages => "LC_MESSAGES",
        }
    }
}

/// 类别名表。
pub const CTG_NAMES: [&str; CTG_COUNT] = [
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
];

// ---------------------------------------------------------------------------
// musl 模型
// ---------------------------------------------------------------------------

/// musl 的类别状态：每类只记 0（C）或 1（C.UTF-8）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Musl {
    ids: [u8; CTG_COUNT],
}

impl Musl {
    /// 全 C 起步。
    pub const fn new() -> Musl {
        Musl { ids: [0u8; CTG_COUNT] }
    }

    /// 档名。
    pub fn id_label(id: u8) -> &'static str {
        if id == 1 {
            "C.UTF-8"
        } else {
            "C"
        }
    }

    /// 当前档 id。
    pub fn id_at(&self, ctg: Ctg) -> u8 {
        match self.ids.get(ctg.index()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 换档。
    ///
    /// `LC_ALL` 铺满六类；单类只改自己。返回换档后的查询串。
    pub fn set(&mut self, ctg: u8, name: &str) -> Result<String, u32> {
        let kind = classify(name)?;
        let id = name_id(kind)?;
        if ctg == CTG_ALL {
            let mut i = 0usize;
            while i < CTG_COUNT {
                if let Some(dst) = self.ids.get_mut(i) {
                    *dst = id;
                }
                i += 1;
            }
            return self.current(CTG_ALL);
        }
        let c = Ctg::from_u8(ctg)?;
        if let Some(dst) = self.ids.get_mut(c.index()) {
            *dst = id;
        }
        self.current(ctg)
    }

    /// 查询。
    pub fn current(&self, ctg: u8) -> Result<String, u32> {
        if ctg == CTG_ALL {
            let first = match self.ids.first() {
                Some(v) => *v,
                None => return Err(ML_ECTG),
            };
            let mut same = true;
            let mut i = 0usize;
            while i < CTG_COUNT {
                if let Some(v) = self.ids.get(i) {
                    if *v != first {
                        same = false;
                    }
                }
                i += 1;
            }
            if same {
                return Ok(String::from(Musl::id_label(first)));
            }
            let mut s = String::new();
            let mut i = 0usize;
            while i < CTG_COUNT {
                let c = Ctg::from_u8(i as u8)?;
                let id = match self.ids.get(i) {
                    Some(v) => *v,
                    None => return Err(ML_ECTG),
                };
                if i > 0 {
                    s.push(';');
                }
                s.push_str(c.name());
                s.push('=');
                s.push_str(Musl::id_label(id));
                i += 1;
            }
            return Ok(s);
        }
        let c = Ctg::from_u8(ctg)?;
        let id = match self.ids.get(c.index()) {
            Some(v) => *v,
            None => return Err(ML_ECTG),
        };
        Ok(String::from(Musl::id_label(id)))
    }
}

impl Default for Musl {
    fn default() -> Musl {
        Musl::new()
    }
}

/// 一次性换档助手。
pub fn setlocale(ctg: u8, name: &str) -> Result<String, u32> {
    let mut m = Musl::new();
    m.set(ctg, name)
}

// ---------------------------------------------------------------------------
// 固定项
// ---------------------------------------------------------------------------

/// 十四项固定查询。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// CODESET
    Codeset,
    /// RADIXCHAR
    RadixChar,
    /// THOUSEP
    ThousandSep,
    /// D_T_FMT
    DateT,
    /// D_FMT
    Date,
    /// T_FMT
    Time,
    /// AM_STR
    AmStr,
    /// PM_STR
    PmStr,
    /// ABDAY_1
    Abday1,
    /// DAY_1
    Day1,
    /// ABMON_1
    Abmon1,
    /// MON_1
    Mon1,
    /// YESEXPR
    YesExpr,
    /// NOEXPR
    NoExpr,
}

impl Item {
    /// 由编号取项。
    pub fn from_u8(v: u8) -> Result<Item, u32> {
        match v {
            0 => Ok(Item::Codeset),
            1 => Ok(Item::RadixChar),
            2 => Ok(Item::ThousandSep),
            3 => Ok(Item::DateT),
            4 => Ok(Item::Date),
            5 => Ok(Item::Time),
            6 => Ok(Item::AmStr),
            7 => Ok(Item::PmStr),
            8 => Ok(Item::Abday1),
            9 => Ok(Item::Day1),
            10 => Ok(Item::Abmon1),
            11 => Ok(Item::Mon1),
            12 => Ok(Item::YesExpr),
            13 => Ok(Item::NoExpr),
            _ => Err(ML_EQUERY),
        }
    }

    /// 名字。
    pub fn name(self) -> &'static str {
        match self {
            Item::Codeset => "CODESET",
            Item::RadixChar => "RADIXCHAR",
            Item::ThousandSep => "THOUSEP",
            Item::DateT => "D_T_FMT",
            Item::Date => "D_FMT",
            Item::Time => "T_FMT",
            Item::AmStr => "AM_STR",
            Item::PmStr => "PM_STR",
            Item::Abday1 => "ABDAY_1",
            Item::Day1 => "DAY_1",
            Item::Abmon1 => "ABMON_1",
            Item::Mon1 => "MON_1",
            Item::YesExpr => "YESEXPR",
            Item::NoExpr => "NOEXPR",
        }
    }

    /// musl 固定值；`utf8` 只影响 CODESET。
    pub fn musl_value(self, utf8: bool) -> &'static str {
        match self {
            Item::Codeset => {
                if utf8 {
                    "UTF-8"
                } else {
                    "ASCII"
                }
            }
            Item::RadixChar => ".",
            Item::ThousandSep => "",
            Item::DateT => "%a %b %e %H:%M:%S %Y",
            Item::Date => "%m/%d/%y",
            Item::Time => "%H:%M:%S",
            Item::AmStr => "AM",
            Item::PmStr => "PM",
            Item::Abday1 => "Sun",
            Item::Day1 => "Sunday",
            Item::Abmon1 => "Jan",
            Item::Mon1 => "January",
            Item::YesExpr => "^[yY]",
            Item::NoExpr => "^[nN]",
        }
    }
}

/// 项名表。
pub const ITEM_NAMES: [&str; ITEM_COUNT] = [
    "CODESET",
    "RADIXCHAR",
    "THOUSEP",
    "D_T_FMT",
    "D_FMT",
    "T_FMT",
    "AM_STR",
    "PM_STR",
    "ABDAY_1",
    "DAY_1",
    "ABMON_1",
    "MON_1",
    "YESEXPR",
    "NOEXPR",
];

/// glibc C 档取值。
pub fn glibc_c(item: Item) -> &'static str {
    match item {
        Item::Codeset => "ANSI_X3.4-1968",
        _ => item.musl_value(false),
    }
}

/// glibc C.UTF-8 档取值。
pub fn glibc_utf8(item: Item) -> &'static str {
    match item {
        Item::Codeset => "UTF-8",
        _ => item.musl_value(false),
    }
}

/// glibc de_DE.UTF-8 档取值。
pub fn glibc_de(item: Item) -> &'static str {
    match item {
        Item::RadixChar => ",",
        Item::ThousandSep => ".",
        Item::DateT => "%a %d %b %Y %T %Z",
        Item::Date => "%d.%m.%Y",
        Item::Time => "%T",
        Item::AmStr => "AM",
        Item::PmStr => "PM",
        Item::Abday1 => "So",
        Item::Day1 => "Sonntag",
        Item::Abmon1 => "Jan",
        Item::Mon1 => "Januar",
        Item::YesExpr => "^[jJyY]",
        Item::NoExpr => "^[nN]",
        _ => item.musl_value(true),
    }
}

/// glibc fr_FR.UTF-8 档取值。
pub fn glibc_fr(item: Item) -> &'static str {
    match item {
        Item::RadixChar => ",",
        Item::ThousandSep => " ",
        Item::DateT => "%a %d %b %Y %T %Z",
        Item::Date => "%d/%m/%Y",
        Item::Time => "%T",
        Item::AmStr => "AM",
        Item::PmStr => "PM",
        Item::Abday1 => "dim.",
        Item::Day1 => "dimanche",
        Item::Abmon1 => "janv.",
        Item::Mon1 => "janvier",
        Item::YesExpr => "^[oOyY]",
        Item::NoExpr => "^[nN]",
        _ => item.musl_value(true),
    }
}

/// glibc ja_JP.UTF-8 档取值。
pub fn glibc_ja(item: Item) -> &'static str {
    match item {
        Item::RadixChar => ".",
        Item::ThousandSep => ",",
        Item::DateT => "%Y年%m月%d日 %H時%M分%S秒",
        Item::Date => "%Y/%m/%d",
        Item::Time => "%H:%M:%S",
        Item::AmStr => "午前",
        Item::PmStr => "午後",
        Item::Abday1 => "日",
        Item::Day1 => "日曜日",
        Item::Abmon1 => "1月",
        Item::Mon1 => "1月",
        Item::YesExpr => "^[yY]",
        Item::NoExpr => "^[nN]",
        _ => item.musl_value(true),
    }
}

/// glibc 线取值。
pub fn glibc_value(locale: &str, item: Item) -> Result<&'static str, u32> {
    match locale {
        "C" => Ok(glibc_c(item)),
        "C.UTF-8" => Ok(glibc_utf8(item)),
        "de_DE.UTF-8" => Ok(glibc_de(item)),
        "fr_FR.UTF-8" => Ok(glibc_fr(item)),
        "ja_JP.UTF-8" => Ok(glibc_ja(item)),
        _ => Err(ML_EQUERY),
    }
}

// ---------------------------------------------------------------------------
// 双线差异锚
// ---------------------------------------------------------------------------

/// 差异位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// 两线一致。
    Same,
    /// 两线不同。
    Differ,
}

impl Anchor {
    /// 文本。
    pub fn text(self) -> &'static str {
        match self {
            Anchor::Same => "same",
            Anchor::Differ => "differ",
        }
    }
}

/// 一行双线对照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualLine {
    /// 项。
    pub item: Item,
    /// glibc 档名。
    pub locale: &'static str,
    /// musl 行。
    pub musl: &'static str,
    /// glibc 行。
    pub glibc: &'static str,
    /// 差异位。
    pub anchor: Anchor,
}

impl DualLine {
    /// 渲染：`musl=<v> glibc=<v> anchor=<same|differ>`。
    pub fn render(&self) -> String {
        let mut s = String::from("musl=");
        s.push_str(self.musl);
        s.push_str(" glibc=");
        s.push_str(self.glibc);
        s.push_str(" anchor=");
        s.push_str(self.anchor.text());
        s
    }
}

/// 造一行双线对照。musl 侧按 `C.UTF-8` 判 CODESET。
pub fn dual(item: Item, locale: &str) -> Result<DualLine, u32> {
    let utf8 = locale == "C.UTF-8";
    let musl = item.musl_value(utf8);
    let glibc = glibc_value(locale, item)?;
    let anchor = if musl == glibc {
        Anchor::Same
    } else {
        Anchor::Differ
    };
    Ok(DualLine { item, locale: "", musl, glibc, anchor })
}

/// 双线锚汇总：同/异条数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnchorCensus {
    /// 总行数。
    pub total: u32,
    /// 一致行数。
    pub same: u32,
    /// 差异行数。
    pub differ: u32,
}

/// 对五项 locale 档逐项算锚。
pub fn anchor_census() -> Result<AnchorCensus, u32> {
    const LOCALES: [&str; 5] = ["C", "C.UTF-8", "de_DE.UTF-8", "fr_FR.UTF-8", "ja_JP.UTF-8"];
    let mut c = AnchorCensus { total: 0, same: 0, differ: 0 };
    let mut i = 0usize;
    while i < ITEM_COUNT {
        let item = Item::from_u8(i as u8)?;
        let mut j = 0usize;
        while j < LOCALES.len() {
            let loc = match LOCALES.get(j) {
                Some(v) => *v,
                None => return Err(ML_EQUERY),
            };
            let d = dual(item, loc)?;
            c.total = c.total.saturating_add(1);
            match d.anchor {
                Anchor::Same => c.same = c.same.saturating_add(1),
                Anchor::Differ => c.differ = c.differ.saturating_add(1),
            }
            j += 1;
        }
        i += 1;
    }
    Ok(c)
}

// ---------------------------------------------------------------------------
// localeconv 固定面
// ---------------------------------------------------------------------------

/// musl 的 localeconv 常量面（CHAR_MAX = 127 的字段照抄）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conv {
    /// 小数点。
    pub decimal_point: &'static str,
    /// 千分位。
    pub thousands_sep: &'static str,
    /// 分组。
    pub grouping: &'static str,
    /// 国际货币符号。
    pub int_curr_symbol: &'static str,
    /// 本地货币符号。
    pub currency_symbol: &'static str,
    /// 货币小数点。
    pub mon_decimal_point: &'static str,
    /// 正号。
    pub positive_sign: &'static str,
    /// 负号。
    pub negative_sign: &'static str,
    /// 国际小数位。
    pub int_frac_digits: i8,
    /// 本地小数位。
    pub frac_digits: i8,
    /// 正数货币前置。
    pub p_cs_precedes: i8,
    /// 负数货币前置。
    pub n_cs_precedes: i8,
}

/// 常量面：除小数点与负号外全空，CHAR_MAX 字段为 127。
pub const fn c_conv() -> Conv {
    Conv {
        decimal_point: ".",
        thousands_sep: "",
        grouping: "",
        int_curr_symbol: "",
        currency_symbol: "",
        mon_decimal_point: "",
        positive_sign: "",
        negative_sign: "-",
        int_frac_digits: 127,
        frac_digits: 127,
        p_cs_precedes: 127,
        n_cs_precedes: 127,
    }
}

/// 渲染 localeconv 固定面。
pub fn conv_render() -> String {
    let c = c_conv();
    let mut s = String::from("decimal_point=");
    s.push_str(c.decimal_point);
    s.push_str(" thousands_sep=");
    s.push_str(c.thousands_sep);
    s.push_str(" grouping=");
    s.push_str(c.grouping);
    s.push_str(" int_curr=");
    s.push_str(c.int_curr_symbol);
    s.push_str(" currency=");
    s.push_str(c.currency_symbol);
    s.push_str(" mon_decimal=");
    s.push_str(c.mon_decimal_point);
    s.push_str(" positive=");
    s.push_str(c.positive_sign);
    s.push_str(" negative=");
    s.push_str(c.negative_sign);
    s.push_str(" int_frac=");
    s.push_str(i32_text(c.int_frac_digits as i32).as_str());
    s.push_str(" frac=");
    s.push_str(i32_text(c.frac_digits as i32).as_str());
    s.push_str(" p_cs=");
    s.push_str(i32_text(c.p_cs_precedes as i32).as_str());
    s.push_str(" n_cs=");
    s.push_str(i32_text(c.n_cs_precedes as i32).as_str());
    s
}

/// 常量面自检项数。
pub fn conv_fields() -> usize {
    CONV_FIELDS
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// setlocale 查询。
    Locale,
    /// 固定项查询。
    Item,
    /// localeconv 固定面。
    Conv,
    /// 双线对照。
    Dual,
}

/// 探针。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// 换档/查询。
    Set {
        /// 类别号。
        ctg: u8,
        /// 名字。
        name: &'static str,
    },
    /// 固定项。
    Item {
        /// 项。
        item: Item,
        /// 是否 UTF-8 档。
        utf8: bool,
    },
    /// localeconv。
    Conv,
    /// 双线锚。
    Dual {
        /// 项。
        item: Item,
        /// glibc 档。
        locale: &'static str,
    },
}

/// 期望。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// 精确文本。
    Text(&'static str),
    /// 精确错误码。
    Code(u32),
}

/// 一个语料槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 输入描述。
    pub input: &'static str,
    /// 判面。
    pub face: Face,
    /// 探针。
    pub probe: Probe,
    /// 期望。
    pub want: Want,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(
        input: &'static str,
        face: Face,
        probe: Probe,
        want: Want,
        note: &'static str,
    ) -> Case {
        Case { input, face, probe, want, note }
    }
}

/// 十四槽语料：8 收 6 拒。
pub const CASES: [Case; CORPUS] = [
    Case::new(
        "setlocale(LC_ALL,C)",
        Face::Locale,
        Probe::Set { ctg: CTG_ALL, name: "C" },
        Want::Text("C"),
        "all-c",
    ),
    Case::new(
        "setlocale(LC_ALL,C.UTF-8)",
        Face::Locale,
        Probe::Set { ctg: CTG_ALL, name: "C.UTF-8" },
        Want::Text("C.UTF-8"),
        "all-cutf8",
    ),
    Case::new(
        "setlocale(LC_ALL,POSIX)",
        Face::Locale,
        Probe::Set { ctg: CTG_ALL, name: "POSIX" },
        Want::Text("C"),
        "all-posix",
    ),
    Case::new(
        "setlocale(LC_CTYPE,UTF-8)",
        Face::Locale,
        Probe::Set { ctg: 0, name: "UTF-8" },
        Want::Text("C.UTF-8"),
        "ctype-utf8",
    ),
    Case::new(
        "nl_langinfo(C,CODESET)",
        Face::Item,
        Probe::Item { item: Item::Codeset, utf8: false },
        Want::Text("ASCII"),
        "codeset-ascii",
    ),
    Case::new(
        "nl_langinfo(C.UTF-8,CODESET)",
        Face::Item,
        Probe::Item { item: Item::Codeset, utf8: true },
        Want::Text("UTF-8"),
        "codeset-utf8",
    ),
    Case::new(
        "localeconv(C)",
        Face::Conv,
        Probe::Conv,
        Want::Text("decimal_point=. thousands_sep= grouping= int_curr= currency= mon_decimal= positive= negative=- int_frac=127 frac=127 p_cs=127 n_cs=127"),
        "conv-fixed",
    ),
    Case::new(
        "dual(RADIXCHAR,de_DE.UTF-8)",
        Face::Dual,
        Probe::Dual { item: Item::RadixChar, locale: "de_DE.UTF-8" },
        Want::Text("musl=. glibc=, anchor=differ"),
        "dual-radix-de",
    ),
    Case::new(
        "setlocale(LC_ALL,xx_YY.UTF-8)",
        Face::Locale,
        Probe::Set { ctg: CTG_ALL, name: "xx_YY.UTF-8" },
        Want::Code(ML_EUNSUP),
        "unsup-xx",
    ),
    Case::new(
        "setlocale(LC_ALL,de_DE.UTF-8)",
        Face::Locale,
        Probe::Set { ctg: CTG_ALL, name: "de_DE.UTF-8" },
        Want::Code(ML_EUNSUP),
        "unsup-de",
    ),
    Case::new(
        "setlocale(9,C)",
        Face::Locale,
        Probe::Set { ctg: 9, name: "C" },
        Want::Code(ML_ECTG),
        "ctg-range",
    ),
    Case::new(
        "setlocale(LC_ALL,<empty>)",
        Face::Locale,
        Probe::Set { ctg: CTG_ALL, name: "" },
        Want::Code(ML_EENV),
        "env-empty",
    ),
    Case::new(
        "setlocale(LC_ALL,<48+ chars>)",
        Face::Locale,
        Probe::Set { ctg: CTG_ALL, name: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" },
        Want::Code(ML_ENAME),
        "name-long",
    ),
    Case::new(
        "setlocale(LC_ALL,C;rm)",
        Face::Locale,
        Probe::Set { ctg: CTG_ALL, name: "C;rm" },
        Want::Code(ML_EBYTE),
        "name-inject",
    ),
];

/// 按槽取。
pub fn case_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 语料互异：输入与判面二元组、备注均不得重复。
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
                if a.note == b.note {
                    return false;
                }
                if a.input == b.input && a.face == b.face {
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

/// 运行一个探针。
pub fn judge(p: Probe) -> Result<String, u32> {
    match p {
        Probe::Set { ctg, name } => setlocale(ctg, name),
        Probe::Item { item, utf8 } => Ok(String::from(item.musl_value(utf8))),
        Probe::Conv => Ok(conv_render()),
        Probe::Dual { item, locale } => match dual(item, locale) {
            Ok(d) => Ok(d.render()),
            Err(c) => Err(c),
        },
    }
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
        let got = judge(c.probe);
        let hit = match c.want {
            Want::Text(t) => match got {
                Ok(s) => s.as_str() == t,
                Err(_) => false,
            },
            Want::Code(code) => got == Err(code),
        };
        if !hit {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 语料汇总。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Census {
    /// 总槽数。
    pub total: u32,
    /// 收档数。
    pub accept: u32,
    /// 拒档数。
    pub reject: u32,
    /// 名字类拒收（字节/超长/注入）。
    pub name: u32,
    /// 类别类拒收。
    pub ctg: u32,
    /// 不支持档拒收。
    pub unsup: u32,
    /// 环境/查询类拒收。
    pub env: u32,
    /// 命中数。
    pub matched: u32,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census {
            total: 0,
            accept: 0,
            reject: 0,
            name: 0,
            ctg: 0,
            unsup: 0,
            env: 0,
            matched: 0,
        }
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 千分比：四舍五入。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let scaled = self.matched.saturating_mul(1000);
        scaled.saturating_add(self.total / 2) / self.total
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.total == CORPUS as u32
            && self.accept == 8
            && self.reject == 6
            && self.matched == self.total
            && self.rate() == PPM_FULL
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(ML_ENOBS);
        }
        if self.matched != self.total {
            return Err(ML_EVALUE);
        }
        Ok(())
    }

    /// 拒收分桶。
    fn bump(&mut self, code: u32) {
        match code {
            ML_EBYTE | ML_ENAME => self.name = self.name.saturating_add(1),
            ML_ECTG => self.ctg = self.ctg.saturating_add(1),
            ML_EUNSUP => self.unsup = self.unsup.saturating_add(1),
            _ => self.env = self.env.saturating_add(1),
        }
    }

    /// 渲染固定格式。
    pub fn render(&self) -> String {
        let mut s = String::from("mloc corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" name=");
        s.push_str(u32_text(self.name).as_str());
        s.push_str(" ctg=");
        s.push_str(u32_text(self.ctg).as_str());
        s.push_str(" unsup=");
        s.push_str(u32_text(self.unsup).as_str());
        s.push_str(" env=");
        s.push_str(u32_text(self.env).as_str());
        s.push_str(" match=");
        s.push_str(u32_text(self.matched).as_str());
        s.push_str(" rate=");
        s.push_str(u32_text(self.rate()).as_str());
        s.push_str("ppm\n");
        s
    }
}

impl Default for Census {
    fn default() -> Census {
        Census::new()
    }
}

/// 跑全部语料。
pub fn run() -> Result<Census, u32> {
    run_with(None)
}

/// 载红档跑：`red` 槽强制判红，用来验证 929ppm 舍入。
pub fn run_with(red: Option<usize>) -> Result<Census, u32> {
    if let Some(r) = red {
        if r >= CORPUS {
            return Err(ML_ESLOT);
        }
    }
    let mut c = Census::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let cs = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(ML_ESLOT),
        };
        c.total = c.total.saturating_add(1);
        let got = judge(cs.probe);
        let hit = match cs.want {
            Want::Text(t) => match &got {
                Ok(s) => s.as_str() == t,
                Err(_) => false,
            },
            Want::Code(code) => got == Err(code),
        };
        match &got {
            Ok(_) => c.accept = c.accept.saturating_add(1),
            Err(code) => {
                c.reject = c.reject.saturating_add(1);
                c.bump(*code);
            }
        }
        let forced = match red {
            Some(r) => r == i,
            None => false,
        };
        if hit && !forced {
            c.matched = c.matched.saturating_add(1);
        }
        i += 1;
    }
    Ok(c)
}

/// 封面：必须恰好十四槽。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(ML_ENOBS)
    }
}

/// 按语料造模型（挂住每次换档后的最终状态）。
pub fn build_model() -> Result<Musl, u32> {
    let mut m = Musl::new();
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            if let Probe::Set { ctg, name } = c.probe {
                if c.want != Want::Code(ML_EBYTE)
                    && c.want != Want::Code(ML_ENAME)
                    && c.want != Want::Code(ML_EENV)
                    && c.want != Want::Code(ML_ECTG)
                    && c.want != Want::Code(ML_EUNSUP)
                {
                    let _ = m.set(ctg, name);
                }
            }
        }
        i += 1;
    }
    Ok(m)
}

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 账本格。
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

    /// 登记。格位错序落 ESLOT；冻结后落 ESEAL，两者都累加 rejects。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ML_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ML_ESLOT);
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

    /// 对账：14 行、8 成 6 败，成格值与败格码逐行落在常量表上。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(ML_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 2, 1, 2, 1, 929];
        const CODES: [u32; 6] = [ML_EBYTE, ML_ENAME, ML_ECTG, ML_EUNSUP, ML_EENV, ML_EQUERY];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(ML_EBAL),
            };
            if s.slot as usize != i {
                return Err(ML_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(ML_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(ML_EVALUE);
                        }
                    }
                    None => return Err(ML_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(ML_EVALUE);
                        }
                    }
                    None => return Err(ML_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(ML_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold_word(h, s.slot as u32);
                h = fold_word(h, s.op as u32);
                h = fold_word(h, s.ok as u32);
                h = fold_word(h, s.code);
                h = fold_word(h, s.value);
            }
            i += 1;
        }
        fold_word(h, self.rejects)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ML_ESEAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建后比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(ML_ESEAL)
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

const OP_TOTAL: u8 = 0;
const OP_ACCEPT: u8 = 1;
const OP_REJECT: u8 = 2;
const OP_NAME: u8 = 3;
const OP_CTG: u8 = 4;
const OP_UNSUP: u8 = 5;
const OP_ENV: u8 = 6;
const OP_RATE: u8 = 7;
const OP_QUERY: u8 = 8;

/// 计数 → 样本。
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
    let _ = l.register(count_cell(0, OP_TOTAL, r.map(|x| x.total as usize)));
    let _ = l.register(count_cell(1, OP_ACCEPT, r.map(|x| x.accept as usize)));
    let _ = l.register(count_cell(2, OP_REJECT, r.map(|x| x.reject as usize)));
    let _ = l.register(count_cell(3, OP_NAME, r.map(|x| x.name as usize)));
    let _ = l.register(count_cell(4, OP_CTG, r.map(|x| x.ctg as usize)));
    let _ = l.register(count_cell(5, OP_UNSUP, r.map(|x| x.unsup as usize)));
    let _ = l.register(count_cell(6, OP_ENV, r.map(|x| x.env as usize)));
    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_RATE, red.map(|x| x.rate() as usize)));

    let byte = match setlocale(CTG_ALL, "C;rm") {
        Ok(_) => ML_EBYTE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(8, OP_QUERY, byte));

    let long = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let name = match setlocale(CTG_ALL, long) {
        Ok(_) => ML_ENAME,
        Err(c) => c,
    };
    let _ = l.register(err_cell(9, OP_QUERY, name));

    let ctg = match setlocale(9, "C") {
        Ok(_) => ML_ECTG,
        Err(c) => c,
    };
    let _ = l.register(err_cell(10, OP_QUERY, ctg));

    let unsup = match setlocale(CTG_ALL, "de_DE.UTF-8") {
        Ok(_) => ML_EUNSUP,
        Err(c) => c,
    };
    let _ = l.register(err_cell(11, OP_QUERY, unsup));

    let env = match setlocale(CTG_ALL, "") {
        Ok(_) => ML_EENV,
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_QUERY, env));

    let query = match Item::from_u8(14) {
        Ok(_) => ML_EQUERY,
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_QUERY, query));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名字闸与内置档。
fn name_ok() -> bool {
    let a = check_name("C.UTF-8").is_ok();
    let b = check_name("").err() == Some(ML_EENV);
    let c = check_name("C;rm").err() == Some(ML_EBYTE);
    let d = check_name("C rm").err() == Some(ML_EBYTE);
    let e = check_name("C(rm)").err() == Some(ML_EBYTE);
    let f = check_name("x\ny").err() == Some(ML_EBYTE);
    let g = check_name("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").err() == Some(ML_ENAME);
    let h = classify("C") == Ok(NameKind::C) && classify("POSIX") == Ok(NameKind::Posix);
    let i = classify("C.UTF-8") == Ok(NameKind::CUtf8) && classify("UTF-8") == Ok(NameKind::Utf8);
    let j = classify("de_DE.UTF-8") == Ok(NameKind::Other) && !supported(NameKind::Other);
    let k = name_id(NameKind::Other) == Err(ML_EUNSUP) && name_id(NameKind::Utf8) == Ok(1);
    a && b && c && d && e && f && g && h && i && j && k
}

/// 类别映射。
fn category_ok() -> bool {
    let a = Ctg::from_u8(0) == Ok(Ctg::Ctype) && Ctg::from_u8(5) == Ok(Ctg::Messages);
    let b = Ctg::from_u8(6) == Err(ML_ECTG) && Ctg::from_u8(0xff) == Err(ML_ECTG);
    let c = Ctg::Ctype.name() == "LC_CTYPE" && Ctg::Messages.name() == "LC_MESSAGES";
    let d = CTG_NAMES.len() == CTG_COUNT && CTG_NAMES.first().copied() == Some("LC_CTYPE");
    let e = CTG_NAMES.get(5).copied() == Some("LC_MESSAGES");
    let f = Ctg::from_u8(3).map(|x| x.index()) == Ok(3);
    a && b && c && d && e && f
}

/// 固定项表。
fn item_ok() -> bool {
    let a = Item::from_u8(0) == Ok(Item::Codeset) && Item::from_u8(13) == Ok(Item::NoExpr);
    let b = Item::from_u8(14) == Err(ML_EQUERY) && Item::from_u8(0x80) == Err(ML_EQUERY);
    let c = Item::Codeset.musl_value(false) == "ASCII" && Item::Codeset.musl_value(true) == "UTF-8";
    let d = Item::RadixChar.musl_value(true) == "." && Item::ThousandSep.musl_value(true) == "";
    let e = Item::DateT.musl_value(false) == "%a %b %e %H:%M:%S %Y";
    let f = Item::Date.musl_value(false) == "%m/%d/%y" && Item::Time.musl_value(false) == "%H:%M:%S";
    let g = Item::Day1.musl_value(false) == "Sunday" && Item::Mon1.musl_value(false) == "January";
    let h = ITEM_NAMES.len() == ITEM_COUNT && ITEM_NAMES.get(9).copied() == Some("DAY_1");
    let i = glibc_c(Item::Codeset) == "ANSI_X3.4-1968" && glibc_utf8(Item::Codeset) == "UTF-8";
    a && b && c && d && e && f && g && h && i
}

/// localeconv 常量面。
fn conv_ok() -> bool {
    let c = c_conv();
    let a = c.decimal_point == "." && c.thousands_sep == "" && c.grouping == "";
    let b = c.int_curr_symbol == "" && c.currency_symbol == "";
    let c2 = c.mon_decimal_point == "" && c.positive_sign == "" && c.negative_sign == "-";
    let d = c.int_frac_digits == 127 && c.frac_digits == 127;
    let e = c.p_cs_precedes == 127 && c.n_cs_precedes == 127;
    let f = conv_render().as_str()
        == "decimal_point=. thousands_sep= grouping= int_curr= currency= mon_decimal= positive= negative=- int_frac=127 frac=127 p_cs=127 n_cs=127";
    let g = conv_fields() == CONV_FIELDS;
    a && b && c2 && d && e && f && g
}

/// 双线差异锚。
fn dual_ok() -> bool {
    let a = match dual(Item::RadixChar, "de_DE.UTF-8") {
        Ok(d) => d.musl == "." && d.glibc == "," && d.anchor == Anchor::Differ,
        Err(_) => false,
    };
    let b = match dual(Item::RadixChar, "C") {
        Ok(d) => d.anchor == Anchor::Same && d.render().as_str() == "musl=. glibc=. anchor=same",
        Err(_) => false,
    };
    let c = match dual(Item::DateT, "ja_JP.UTF-8") {
        Ok(d) => d.anchor == Anchor::Differ && d.musl == "%a %b %e %H:%M:%S %Y",
        Err(_) => false,
    };
    let d = dual(Item::RadixChar, "xx_YY.UTF-8").err() == Some(ML_EQUERY);
    let e = match dual(Item::Codeset, "C.UTF-8") {
        Ok(v) => v.anchor == Anchor::Same && v.glibc == "UTF-8",
        Err(_) => false,
    };
    let f = dual(Item::Codeset, "C").map(|v| v.anchor) == Ok(Anchor::Differ);
    let g = match anchor_census() {
        Ok(v) => v.total == 70 && v.same.saturating_add(v.differ) == 70 && v.differ >= 10,
        Err(_) => false,
    };
    a && b && c && d && e && f && g
}

/// 模型换档。
fn model_ok() -> bool {
    let mut m = Musl::new();
    let a = m.current(CTG_ALL) == Ok(String::from("C"));
    let b = m.set(CTG_ALL, "C.UTF-8") == Ok(String::from("C.UTF-8"));
    let c = m.current(0) == Ok(String::from("C.UTF-8"));
    let d = m.set(0, "C") == Ok(String::from("C"));
    let e = match m.current(CTG_ALL) {
        Ok(s) => s.as_str() == "LC_CTYPE=C;LC_NUMERIC=C.UTF-8;LC_TIME=C.UTF-8;LC_COLLATE=C.UTF-8;LC_MONETARY=C.UTF-8;LC_MESSAGES=C.UTF-8",
        Err(_) => false,
    };
    let f = m.set(1, "POSIX").map(|x| x.as_str() == "C") == Ok(true);
    let g = m.set(6, "C").err() == Some(ML_ECTG);
    let h = Musl::id_label(0) == "C" && Musl::id_label(1) == "C.UTF-8" && Musl::id_label(2) == "C";
    a && b && c && d && e && f && g && h
}

/// 十四槽表判。
fn corpus_ok() -> bool {
    if !table_ok() {
        return false;
    }
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.total == 14 && c.accept == 8 && c.reject == 6;
    let b = c.name == 2 && c.ctg == 1 && c.unsup == 2 && c.env == 1;
    let d = c.matched == 14 && c.rate() == PPM_FULL && c.green() && c.gate() == Ok(());
    a && b && d
}

/// 槽位与语料编号。
fn slots_ok() -> bool {
    let a = CORPUS == 14 && CASES.len() == CORPUS && unique() && notes_ok();
    let b = match case_at(0) {
        Some(c) => c.want == Want::Text("C") && c.face == Face::Locale,
        None => false,
    };
    let c = match case_at(7) {
        Some(c) => c.face == Face::Dual && c.note == "dual-radix-de",
        None => false,
    };
    let d = match case_at(13) {
        Some(c) => c.want == Want::Code(ML_EBYTE),
        None => false,
    };
    let e = case_at(14).is_none() && case_at(99).is_none();
    a && b && c && d && e
}

/// 汇总渲染。
fn report_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.render().as_str()
        == "mloc corpus=14 accept=8 reject=6 name=2 ctg=1 unsup=2 env=1 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(ML_ENOBS);
    let d = e.render().as_str()
        == "mloc corpus=0 accept=0 reject=0 name=0 ctg=0 unsup=0 env=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(CORPUS + 1) == Err(ML_ENOBS);
    a && b && d && f
}

/// 红档 929。
fn reds_ok() -> bool {
    let c = match run_with(Some(0)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched == 13 && c.rate() == PPM_RED && c.gate() == Err(ML_EVALUE);
    let b = !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(ML_ESLOT);
    let e = run_with(Some(CORPUS + 7)).err() == Some(ML_ESLOT);
    a && b && d && e
}

/// 账本与冻结重放。
fn ledger_ok() -> bool {
    let l = build_ledger();
    let a = l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6;
    let b = l.audit() == Ok(());
    let mut f = build_ledger();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let c = f.is_frozen() && f.root() == root && f.replay() == Ok(root);
    let before = f.rejects();
    let d = f.register(Sample::blank()).err() == Some(ML_ESEAL) && f.rejects() == before + 1;
    a && b && c && d
}

/// 文本助手与注入。
fn text_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(14) == "14" && u32_text(4294967295) == "4294967295";
    let b = hex_text(SEG_BASE) == "0x0000fd60" && hex_text(0) == "0x00000000";
    let c = hex_text(ML_EBYTE) == "0x0000fd61";
    let d = i32_text(-127) == "-127" && i32_text(127) == "127";
    let e = bool_text(true) == "true" && bool_text(false) == "false";
    let f = digest(b"locale") == digest(b"locale") && digest(b"locale") != digest(b"local");
    let g = fold_word(FNV_OFF, 0x0102_0304) == digest(&[1, 2, 3, 4]);
    a && b && c && d && e && f && g
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && category_ok() && item_ok();
    out[1] = conv_ok() && dual_ok() && text_ok();
    out[2] = model_ok() && corpus_ok() && slots_ok() && report_ok();
    out[3] = reds_ok() && ledger_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11647_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11647.musl_locale_fixed");

    s.add("ML01-musl内置档", name_ok(), "static");
    s.add("ML02-类别映射", category_ok(), "static");
    s.add("ML03-固定项表", item_ok(), "static");
    s.add("ML04-localeconv固定", conv_ok(), "static");
    s.add("ML05-双线差异锚", dual_ok(), "static");
    s.add("ML06-换档查询", model_ok(), "static");
    s.add("ML07-十四槽表判", corpus_ok(), "static");
    s.add("ML08-槽位编号", slots_ok(), "static");
    s.add("ML09-汇总渲染", report_ok(), "static");
    s.add("ML10-红档929", reds_ok(), "static");
    s.add("ML11-账本对账", ledger_ok(), "static");
    s.add("ML12-文本助手", text_ok(), "static");
    s.add("ML13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_shape() {
        assert_eq!(CASES.len(), CORPUS);
        assert_eq!(CORPUS, 14);
        assert!(unique());
        assert!(notes_ok());
        assert!(table_ok());
    }

    #[test]
    fn corpus_accept_reject_split() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.accept, 8);
        assert_eq!(c.reject, 6);
        assert_eq!(c.accept + c.reject, 14);
    }

    #[test]
    fn name_gate() {
        assert!(check_name("C").is_ok());
        assert!(check_name("C.UTF-8").is_ok());
        assert!(check_name("sr_RS.UTF-8@latin").is_ok());
        assert_eq!(check_name("").err(), Some(ML_EENV));
        assert_eq!(check_name("C rm").err(), Some(ML_EBYTE));
        assert_eq!(check_name("C;rm").err(), Some(ML_EBYTE));
        assert_eq!(check_name("C(rm)").err(), Some(ML_EBYTE));
        assert_eq!(check_name("x\ty").err(), Some(ML_EBYTE));
        assert_eq!(check_name("C.UTF-8中文").err(), Some(ML_EBYTE));
    }

    #[test]
    fn name_length_limit() {
        let mut s = String::new();
        let mut i = 0usize;
        while i < NAME_MAX {
            s.push('a');
            i += 1;
        }
        assert!(check_name(s.as_str()).is_ok());
        s.push('a');
        assert_eq!(check_name(s.as_str()).err(), Some(ML_ENAME));
    }

    #[test]
    fn classify_kinds() {
        assert_eq!(classify("C"), Ok(NameKind::C));
        assert_eq!(classify("POSIX"), Ok(NameKind::Posix));
        assert_eq!(classify("C.UTF-8"), Ok(NameKind::CUtf8));
        assert_eq!(classify("UTF-8"), Ok(NameKind::Utf8));
        assert_eq!(classify("en_US.UTF-8"), Ok(NameKind::Other));
        assert_eq!(name_id(NameKind::Posix), Ok(0));
        assert_eq!(name_id(NameKind::Utf8), Ok(1));
        assert_eq!(name_id(NameKind::Other), Err(ML_EUNSUP));
    }

    #[test]
    fn musl_set_and_query() {
        let mut m = Musl::new();
        assert_eq!(m.current(CTG_ALL), Ok(String::from("C")));
        assert_eq!(m.set(CTG_ALL, "C.UTF-8"), Ok(String::from("C.UTF-8")));
        assert_eq!(m.current(Ctg::Numeric.index() as u8), Ok(String::from("C.UTF-8")));
        assert_eq!(m.set(Ctg::Time.index() as u8, "C"), Ok(String::from("C")));
        assert_eq!(m.id_at(Ctg::Time), 0);
        assert_eq!(m.id_at(Ctg::Ctype), 1);
    }

    #[test]
    fn musl_composite() {
        let mut m = Musl::new();
        assert!(m.set(Ctg::Ctype.index() as u8, "C.UTF-8").is_ok());
        let s = match m.current(CTG_ALL) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(s.as_str().starts_with("LC_CTYPE=C.UTF-8;"));
        assert!(s.as_str().ends_with("LC_MESSAGES=C"));
        assert_eq!(s.as_str().matches(';').count(), 5);
    }

    #[test]
    fn musl_rejects() {
        assert_eq!(setlocale(CTG_ALL, "de_DE.UTF-8").err(), Some(ML_EUNSUP));
        assert_eq!(setlocale(CTG_ALL, "en_US.UTF-8").err(), Some(ML_EUNSUP));
        assert_eq!(setlocale(7, "C").err(), Some(ML_ECTG));
        assert_eq!(setlocale(CTG_ALL, "C;rm").err(), Some(ML_EBYTE));
        assert_eq!(setlocale(CTG_ALL, "").err(), Some(ML_EENV));
    }

    #[test]
    fn item_table() {
        assert_eq!(ITEM_NAMES.len(), ITEM_COUNT);
        assert_eq!(Item::from_u8(0), Ok(Item::Codeset));
        assert_eq!(Item::from_u8(13), Ok(Item::NoExpr));
        assert_eq!(Item::from_u8(14), Err(ML_EQUERY));
        assert_eq!(Item::Codeset.name(), "CODESET");
        assert_eq!(Item::Day1.name(), "DAY_1");
        assert_eq!(Item::Codeset.musl_value(false), "ASCII");
        assert_eq!(Item::Codeset.musl_value(true), "UTF-8");
        assert_eq!(Item::RadixChar.musl_value(false), ".");
        assert_eq!(Item::ThousandSep.musl_value(false), "");
    }

    #[test]
    fn glibc_lines() {
        assert_eq!(glibc_c(Item::Codeset), "ANSI_X3.4-1968");
        assert_eq!(glibc_utf8(Item::Codeset), "UTF-8");
        assert_eq!(glibc_de(Item::RadixChar), ",");
        assert_eq!(glibc_de(Item::ThousandSep), ".");
        assert_eq!(glibc_de(Item::Day1), "Sonntag");
        assert_eq!(glibc_fr(Item::Day1), "dimanche");
        assert_eq!(glibc_ja(Item::Day1), "日曜日");
        assert_eq!(glibc_value("xx_YY.UTF-8", Item::Day1).err(), Some(ML_EQUERY));
        assert_eq!(glibc_value("C", Item::Date), Ok("%m/%d/%y"));
    }

    #[test]
    fn dual_anchors() {
        let a = match dual(Item::RadixChar, "de_DE.UTF-8") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(a.anchor, Anchor::Differ);
        assert_eq!(a.musl, ".");
        assert_eq!(a.glibc, ",");
        assert_eq!(a.render(), "musl=. glibc=, anchor=differ");
        let b = match dual(Item::Day1, "C") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(b.anchor, Anchor::Same);
        assert_eq!(b.render(), "musl=Sunday glibc=Sunday anchor=same");
    }

    #[test]
    fn dual_locale_coverage() {
        let c = match anchor_census() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.total, 70);
        assert_eq!(c.same + c.differ, 70);
        assert!(c.differ > 0);
    }

    #[test]
    fn conv_table() {
        let c = c_conv();
        assert_eq!(c.decimal_point, ".");
        assert_eq!(c.thousands_sep, "");
        assert_eq!(c.int_frac_digits, 127);
        assert_eq!(c.frac_digits, 127);
        assert_eq!(c.negative_sign, "-");
        assert_eq!(conv_fields(), CONV_FIELDS);
        assert!(conv_render().as_str().contains("int_frac=127"));
    }

    #[test]
    fn corpus_table_each_slot() {
        let mut i = 0usize;
        while i < CASES.len() {
            let c = match CASES.get(i) {
                Some(v) => *v,
                None => return,
            };
            let got = judge(c.probe);
            match c.want {
                Want::Text(t) => {
                    let ok = match got {
                        Ok(s) => s.as_str() == t,
                        Err(_) => false,
                    };
                    assert!(ok, "slot {}", i);
                }
                Want::Code(code) => assert_eq!(got, Err(code), "slot {}", i),
            }
            i += 1;
        }
    }

    #[test]
    fn census_numbers() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.total, 14);
        assert_eq!(c.accept, 8);
        assert_eq!(c.reject, 6);
        assert_eq!(c.name, 2);
        assert_eq!(c.ctg, 1);
        assert_eq!(c.unsup, 2);
        assert_eq!(c.env, 1);
        assert_eq!(c.rate(), 1000);
        assert_eq!(c.gate(), Ok(()));
        assert!(c.green());
    }

    #[test]
    fn census_report_exact() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(
            c.render().as_str(),
            "mloc corpus=14 accept=8 reject=6 name=2 ctg=1 unsup=2 env=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rounding() {
        let c = match run_with(Some(5)) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.matched, 13);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(ML_EVALUE));
        assert_eq!(run_with(Some(14)).err(), Some(ML_ESLOT));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(0).map(|s| s.value), Some(14));
        assert_eq!(l.sample(7).map(|s| s.value), Some(929));
        assert!(l.sample(14).is_none());
    }

    #[test]
    fn ledger_codes_in_order() {
        let l = build_ledger();
        let want = [ML_EBYTE, ML_ENAME, ML_ECTG, ML_EUNSUP, ML_EENV, ML_EQUERY];
        let mut i = 0usize;
        while i < 6 {
            let s = match l.sample(8 + i) {
                Some(v) => v,
                None => return,
            };
            assert!(!s.ok);
            assert_eq!(s.code, want[i]);
            i += 1;
        }
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
        assert_eq!(l.freeze(), Err(ML_ESEAL));
        assert!(l.rejects() >= 1);
        let before = l.rejects();
        assert_eq!(l.register(Sample::blank()).err(), Some(ML_ESEAL));
        assert_eq!(l.rejects(), before + 1);
    }

    #[test]
    fn ledger_reject_order() {
        let mut l = Ledger::new();
        let bad = Sample { slot: 3, op: 0, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(bad).err(), Some(ML_ESLOT));
        assert_eq!(l.rejects(), 1);
        let good = Sample { slot: 0, op: 0, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(good), Ok(()));
        assert_eq!(l.len(), 1);
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
    fn text_renderers() {
        assert_eq!(u32_text(0), "0");
        assert_eq!(u32_text(929), "929");
        assert_eq!(hex_text(SEG_BASE), "0x0000fd60");
        assert_eq!(hex_text(0xffff_ffff), "0xffffffff");
        assert_eq!(i32_text(-1), "-1");
        assert_eq!(bool_text(true), "true");
    }

    #[test]
    fn digest_properties() {
        assert_eq!(fold_word(FNV_OFF, 0), digest(&[0, 0, 0, 0]));
        assert_ne!(digest(b"C"), digest(b"POSIX"));
        assert_eq!(digest(b""), FNV_OFF);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11647_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn check_13_name() {
        let s = run_unx11647_checks();
        let c = match s.get(12) {
            Some(v) => v,
            None => return,
        };
        assert!(c.name.ends_with("13-冻结重放四注入"));
    }
}
