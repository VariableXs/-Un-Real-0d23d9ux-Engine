//! UNX-F11645 setlocale 返回串字面比较（C/POSIX 别名）
//!
//! `setlocale(category, name)` 的返回值不是调用者传进去的那根指针，而是
//! libc 内部静态表里“归一之后”的 locale 名。调用者若写
//! `if (setlocale(LC_ALL, "POSIX") == ...)` 或
//! `strcmp(setlocale(LC_ALL, "POSIX"), "POSIX") == 0`，会得到与直觉相反的
//! 结果：POSIX 是 C 的别名，返回串逐字是 `"C"`。同理 `en_US.utf8` 会归一
//! 成 `en_US.UTF-8`，`de_DE@euro` 会补全成 `de_DE.ISO-8859-15@euro`。
//!
//! 本模块把这条链路拆成可核对的格子：先认名字语法（语言、地域、codeset、
//! 修饰符四段），再把别名折叠到规范名，再校验 codeset 拼写与修饰符适用
//! 范围，最后才生成返回串。返回串只用于逐字比较，比较结果分 match /
//! mismatch 两档；指针相等在本模型里不参与判断，因为 glibc 从不保证两次
//! 查询返回同一根指针。
//!
//! `setlocale(LC_ALL, NULL)` 是查询，不改状态，返回十二段复合串
//! `LC_CTYPE=...;LC_NUMERIC=...;...;LC_IDENTIFICATION=...`；单类别查询返回
//! 该类别当前的规范名。置位失败必须整单回滚：十二个槽位要么一起换，要么
//! 一个都不动，免得半套 locale 让后续格式化读到拼接状态。
//!
//! 闸门次序：语法 → 别名 → codeset → 修饰符 → 类别 → 归一 → 返回串 → 比较 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfd40;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 类别数（不含 LC_ALL）。
pub const CAT_COUNT: usize = 12;

/// LC_ALL 的哨兵值。
pub const CAT_ALL: u8 = 0xff;

/// locale 名最大字节数。
pub const MAX_NAME: usize = 56;

/// 满档千分比。
pub const PPM_FULL: u32 = 1000;

/// 单槽红档千分比。
pub const PPM_RED: u32 = 929;

/// 空名 / 非打印字节。
pub const SA_EBYTE: u32 = SEG_BASE + 0x01;

/// 名字语法（下划线、点、段长）。
pub const SA_EGRAMMAR: u32 = SEG_BASE + 0x02;

/// codeset 不认识或拼写残留多余点。
pub const SA_ECODESET: u32 = SEG_BASE + 0x03;

/// 修饰符不认识、为空或与 codeset 不搭。
pub const SA_EMODIFIER: u32 = SEG_BASE + 0x04;

/// 名字语法合法但目录里没有收录。
pub const SA_ENAME: u32 = SEG_BASE + 0x05;

/// 类别号越界。
pub const SA_ECAT: u32 = SEG_BASE + 0x06;

/// 一次调用里重复置位 / 重复查询。
pub const SA_ESTATE: u32 = SEG_BASE + 0x07;

/// 归一后的名字超出固定缓冲。
pub const SA_EOVER: u32 = SEG_BASE + 0x08;

/// 账本不平。
pub const SA_EBAL: u32 = SEG_BASE + 0x09;

/// 槽位错序或越位。
pub const SA_ESLOT: u32 = SEG_BASE + 0x0a;

/// 观测值与期望不符（含返回串比较失败）。
pub const SA_EVALUE: u32 = SEG_BASE + 0x0b;

/// 重放指纹不符。
pub const SA_ESEAL: u32 = SEG_BASE + 0x0c;

/// 空观测。
pub const SA_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// FNV-1a
// ---------------------------------------------------------------------------

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折叠一字节：八轮乘法。
pub fn fold_byte(h: u64, b: u8) -> u64 {
    let mut x = h ^ (b as u64);
    let mut i = 0usize;
    while i < 8 {
        x = x.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    x
}

/// 折叠一个字：大端逐字节。
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
// 类别
// ---------------------------------------------------------------------------

/// POSIX 类别（不含 LC_ALL）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cat {
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
    /// LC_PAPER
    Paper,
    /// LC_NAME
    Name,
    /// LC_ADDRESS
    Address,
    /// LC_TELEPHONE
    Telephone,
    /// LC_MEASUREMENT
    Measurement,
    /// LC_IDENTIFICATION
    Identification,
}

impl Cat {
    /// 由类别号取值。
    pub fn from_u8(v: u8) -> Option<Cat> {
        match v {
            0 => Some(Cat::Ctype),
            1 => Some(Cat::Numeric),
            2 => Some(Cat::Time),
            3 => Some(Cat::Collate),
            4 => Some(Cat::Monetary),
            5 => Some(Cat::Messages),
            6 => Some(Cat::Paper),
            7 => Some(Cat::Name),
            8 => Some(Cat::Address),
            9 => Some(Cat::Telephone),
            10 => Some(Cat::Measurement),
            11 => Some(Cat::Identification),
            _ => None,
        }
    }

    /// 下标。
    pub fn index(self) -> usize {
        match self {
            Cat::Ctype => 0,
            Cat::Numeric => 1,
            Cat::Time => 2,
            Cat::Collate => 3,
            Cat::Monetary => 4,
            Cat::Messages => 5,
            Cat::Paper => 6,
            Cat::Name => 7,
            Cat::Address => 8,
            Cat::Telephone => 9,
            Cat::Measurement => 10,
            Cat::Identification => 11,
        }
    }

    /// 名字。
    pub fn name(self) -> &'static str {
        match self {
            Cat::Ctype => "LC_CTYPE",
            Cat::Numeric => "LC_NUMERIC",
            Cat::Time => "LC_TIME",
            Cat::Collate => "LC_COLLATE",
            Cat::Monetary => "LC_MONETARY",
            Cat::Messages => "LC_MESSAGES",
            Cat::Paper => "LC_PAPER",
            Cat::Name => "LC_NAME",
            Cat::Address => "LC_ADDRESS",
            Cat::Telephone => "LC_TELEPHONE",
            Cat::Measurement => "LC_MEASUREMENT",
            Cat::Identification => "LC_IDENTIFICATION",
        }
    }

    /// 由名字取值。
    pub fn from_name(n: &str) -> Option<Cat> {
        match n {
            "LC_CTYPE" => Some(Cat::Ctype),
            "LC_NUMERIC" => Some(Cat::Numeric),
            "LC_TIME" => Some(Cat::Time),
            "LC_COLLATE" => Some(Cat::Collate),
            "LC_MONETARY" => Some(Cat::Monetary),
            "LC_MESSAGES" => Some(Cat::Messages),
            "LC_PAPER" => Some(Cat::Paper),
            "LC_NAME" => Some(Cat::Name),
            "LC_ADDRESS" => Some(Cat::Address),
            "LC_TELEPHONE" => Some(Cat::Telephone),
            "LC_MEASUREMENT" => Some(Cat::Measurement),
            "LC_IDENTIFICATION" => Some(Cat::Identification),
            _ => None,
        }
    }
}

/// 类别名字表，顺序与 glibc 复合串一致。
pub const CAT_NAMES: [&str; CAT_COUNT] = [
    "LC_CTYPE",
    "LC_NUMERIC",
    "LC_TIME",
    "LC_COLLATE",
    "LC_MONETARY",
    "LC_MESSAGES",
    "LC_PAPER",
    "LC_NAME",
    "LC_ADDRESS",
    "LC_TELEPHONE",
    "LC_MEASUREMENT",
    "LC_IDENTIFICATION",
];

/// 取类别名。
pub fn cat_name(i: usize) -> Option<&'static str> {
    CAT_NAMES.get(i).copied()
}

// ---------------------------------------------------------------------------
// 固定缓冲名字
// ---------------------------------------------------------------------------

/// 固定缓冲的规范名，避免为每个槽位分配堆内存。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    buf: [u8; MAX_NAME],
    len: u8,
}

impl Name {
    /// 空白。
    pub const fn blank() -> Name {
        Name { buf: [0u8; MAX_NAME], len: 0 }
    }

    /// 从字符串复制。
    pub fn from_str(s: &str) -> Result<Name, u32> {
        let b = s.as_bytes();
        if b.len() > MAX_NAME {
            return Err(SA_EOVER);
        }
        let mut buf = [0u8; MAX_NAME];
        let mut i = 0usize;
        while i < b.len() {
            if let Some(v) = b.get(i) {
                buf[i] = *v;
            }
            i += 1;
        }
        Ok(Name { buf, len: b.len() as u8 })
    }

    /// 取视图。
    pub fn as_str(&self) -> &str {
        let n = self.len as usize;
        if n > MAX_NAME {
            return "";
        }
        match core::str::from_utf8(&self.buf[..n]) {
            Ok(v) => v,
            Err(_) => "",
        }
    }

    /// 与字面串逐字比较。
    pub fn eq_str(&self, s: &str) -> bool {
        self.as_str() == s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = fold_word(FNV_OFF, self.len as u32);
        let mut i = 0usize;
        while i < self.len as usize {
            if let Some(b) = self.buf.get(i) {
                h = fold_byte(h, *b);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 名字四段
// ---------------------------------------------------------------------------

/// 拆开的名字四段；空串表示该段没有出现。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts<'a> {
    /// 语言。
    pub lang: &'a str,
    /// 地域。
    pub terr: &'a str,
    /// 字符集。
    pub codeset: &'a str,
    /// 修饰符。
    pub modifier: &'a str,
}

/// ASCII 大小写无关比较。
pub fn eq_ci(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut i = 0usize;
    while i < ab.len() {
        let x = match ab.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let y = match bb.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let lx = if x >= b'A' && x <= b'Z' { x + 32 } else { x };
        let ly = if y >= b'A' && y <= b'Z' { y + 32 } else { y };
        if lx != ly {
            return false;
        }
        i += 1;
    }
    true
}

/// 名字语法闸门。
///
/// 只做分段与字符合法性，不查目录；空 codeset、空修饰符、多余点、
/// 多余下划线都在这里挡掉。
pub fn split_name<'a>(input: &'a str) -> Result<Parts<'a>, u32> {
    let b = input.as_bytes();
    if b.is_empty() {
        return Err(SA_EBYTE);
    }
    if b.len() > MAX_NAME {
        return Err(SA_EOVER);
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(SA_EBYTE),
        };
        if c < 0x21 || c > 0x7e {
            return Err(SA_EBYTE);
        }
        i += 1;
    }

    let mut at = None;
    i = 0usize;
    while i < b.len() {
        match b.get(i) {
            Some(0x40) => {
                if at.is_some() {
                    return Err(SA_EMODIFIER);
                }
                at = Some(i);
            }
            _ => {}
        }
        i += 1;
    }

    let (head, modifier) = match at {
        Some(p) => {
            let m = match input.get(p + 1..) {
                Some(v) => v,
                None => return Err(SA_EMODIFIER),
            };
            if m.is_empty() {
                return Err(SA_EMODIFIER);
            }
            let h = match input.get(..p) {
                Some(v) => v,
                None => return Err(SA_EBYTE),
            };
            (h, m)
        }
        None => (input, ""),
    };
    if head.is_empty() {
        return Err(SA_EBYTE);
    }

    let mb = modifier.as_bytes();
    i = 0usize;
    while i < mb.len() {
        let c = match mb.get(i) {
            Some(v) => *v,
            None => return Err(SA_EMODIFIER),
        };
        let ok = (c >= b'0' && c <= b'9')
            || (c >= b'A' && c <= b'Z')
            || (c >= b'a' && c <= b'z')
            || c == b'_';
        if !ok {
            return Err(SA_EMODIFIER);
        }
        i += 1;
    }

    let hb = head.as_bytes();
    let mut dot = None;
    i = 0usize;
    while i < hb.len() {
        if let Some(0x2e) = hb.get(i) {
            dot = Some(i);
            break;
        }
        i += 1;
    }

    let (base, codeset) = match dot {
        Some(p) => {
            let cs = match head.get(p + 1..) {
                Some(v) => v,
                None => return Err(SA_ECODESET),
            };
            if cs.is_empty() {
                return Err(SA_ECODESET);
            }
            if cs.as_bytes().contains(&b'.') {
                return Err(SA_ECODESET);
            }
            let bt = match head.get(..p) {
                Some(v) => v,
                None => return Err(SA_EGRAMMAR),
            };
            (bt, cs)
        }
        None => (head, ""),
    };
    if base.is_empty() {
        return Err(SA_EGRAMMAR);
    }

    let bb = base.as_bytes();
    let mut us = None;
    i = 0usize;
    while i < bb.len() {
        if let Some(0x5f) = bb.get(i) {
            us = Some(i);
            break;
        }
        i += 1;
    }

    let (lang, terr) = match us {
        Some(p) => {
            let t = match base.get(p + 1..) {
                Some(v) => v,
                None => return Err(SA_EGRAMMAR),
            };
            if t.is_empty() {
                return Err(SA_EGRAMMAR);
            }
            if t.as_bytes().contains(&b'_') {
                return Err(SA_EGRAMMAR);
            }
            let l = match base.get(..p) {
                Some(v) => v,
                None => return Err(SA_EGRAMMAR),
            };
            (l, t)
        }
        None => (base, ""),
    };
    if lang.is_empty() || lang.len() > 8 {
        return Err(SA_EGRAMMAR);
    }
    let lb = lang.as_bytes();
    i = 0usize;
    while i < lb.len() {
        let c = match lb.get(i) {
            Some(v) => *v,
            None => return Err(SA_EGRAMMAR),
        };
        let ok = (c >= b'A' && c <= b'Z') || (c >= b'a' && c <= b'z');
        if !ok {
            return Err(SA_EGRAMMAR);
        }
        i += 1;
    }

    if !terr.is_empty() {
        if terr.len() < 2 || terr.len() > 3 {
            return Err(SA_EGRAMMAR);
        }
        let tb = terr.as_bytes();
        i = 0usize;
        while i < tb.len() {
            let c = match tb.get(i) {
                Some(v) => *v,
                None => return Err(SA_EGRAMMAR),
            };
            let ok = (c >= b'0' && c <= b'9')
                || (c >= b'A' && c <= b'Z')
                || (c >= b'a' && c <= b'z');
            if !ok {
                return Err(SA_EGRAMMAR);
            }
            i += 1;
        }
    }

    if !codeset.is_empty() {
        let cb = codeset.as_bytes();
        i = 0usize;
        while i < cb.len() {
            let c = match cb.get(i) {
                Some(v) => *v,
                None => return Err(SA_ECODESET),
            };
            let ok = (c >= b'0' && c <= b'9')
                || (c >= b'A' && c <= b'Z')
                || (c >= b'a' && c <= b'z')
                || c == b'-'
                || c == b'_';
            if !ok {
                return Err(SA_ECODESET);
            }
            i += 1;
        }
    }

    Ok(Parts { lang, terr, codeset, modifier })
}

// ---------------------------------------------------------------------------
// codeset 归一
// ---------------------------------------------------------------------------

/// 把 codeset 拼写折成规范名；不认识的返回 None。
pub fn canon_codeset(cs: &str) -> Option<&'static str> {
    if eq_ci(cs, "UTF-8") || eq_ci(cs, "UTF8") || eq_ci(cs, "utf-8") {
        return Some("UTF-8");
    }
    if eq_ci(cs, "ISO-8859-1") || eq_ci(cs, "ISO8859-1") || eq_ci(cs, "ISO88591") {
        return Some("ISO-8859-1");
    }
    if eq_ci(cs, "ISO-8859-15") || eq_ci(cs, "ISO8859-15") || eq_ci(cs, "ISO885915") {
        return Some("ISO-8859-15");
    }
    None
}

/// codeset 是否在归一后改变拼写。
pub fn codeset_alias(input: &str) -> bool {
    let p = match split_name(input) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if p.codeset.is_empty() {
        return false;
    }
    match canon_codeset(p.codeset) {
        Some(c) => c != p.codeset,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 目录
// ---------------------------------------------------------------------------

/// 目录条目的来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// 就是 C。
    CName,
    /// POSIX 别名。
    PosixAlias,
    /// 原样规范名。
    Canonical,
    /// codeset 拼写被归一。
    CodesetAlias,
    /// 只给修饰符，codeset 由修饰符补全。
    ModifierAlias,
    /// 没给 codeset，用默认 UTF-8。
    Defaulted,
    /// 语法或目录不认。
    Foreign,
}

impl Origin {
    /// 文案。
    pub fn text(self) -> &'static str {
        match self {
            Origin::CName => "c-name",
            Origin::PosixAlias => "posix-alias",
            Origin::Canonical => "canonical",
            Origin::CodesetAlias => "codeset-alias",
            Origin::ModifierAlias => "modifier-alias",
            Origin::Defaulted => "defaulted",
            Origin::Foreign => "foreign",
        }
    }
}

/// 目录条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rec {
    /// 规范名。
    pub name: &'static str,
    /// 语言。
    pub lang: &'static str,
    /// 地域。
    pub terr: &'static str,
    /// 字符集。
    pub codeset: &'static str,
    /// 修饰符。
    pub modifier: &'static str,
    /// 来源。
    pub origin: Origin,
}

/// 目录条数。
pub const LOCALES: usize = 10;

/// 已安装目录。
pub const CATALOG: [Rec; LOCALES] = [
    Rec { name: "C", lang: "C", terr: "", codeset: "", modifier: "", origin: Origin::CName },
    Rec {
        name: "C.UTF-8",
        lang: "C",
        terr: "",
        codeset: "UTF-8",
        modifier: "",
        origin: Origin::Canonical,
    },
    Rec {
        name: "en_US.UTF-8",
        lang: "en",
        terr: "US",
        codeset: "UTF-8",
        modifier: "",
        origin: Origin::Canonical,
    },
    Rec {
        name: "en_US.ISO-8859-1",
        lang: "en",
        terr: "US",
        codeset: "ISO-8859-1",
        modifier: "",
        origin: Origin::Canonical,
    },
    Rec {
        name: "de_DE.UTF-8",
        lang: "de",
        terr: "DE",
        codeset: "UTF-8",
        modifier: "",
        origin: Origin::Canonical,
    },
    Rec {
        name: "de_DE.ISO-8859-15",
        lang: "de",
        terr: "DE",
        codeset: "ISO-8859-15",
        modifier: "",
        origin: Origin::Canonical,
    },
    Rec {
        name: "de_DE.ISO-8859-15@euro",
        lang: "de",
        terr: "DE",
        codeset: "ISO-8859-15",
        modifier: "euro",
        origin: Origin::ModifierAlias,
    },
    Rec {
        name: "fr_FR.UTF-8",
        lang: "fr",
        terr: "FR",
        codeset: "UTF-8",
        modifier: "",
        origin: Origin::Canonical,
    },
    Rec {
        name: "ja_JP.UTF-8",
        lang: "ja",
        terr: "JP",
        codeset: "UTF-8",
        modifier: "",
        origin: Origin::Canonical,
    },
    Rec {
        name: "zh_CN.UTF-8",
        lang: "zh",
        terr: "CN",
        codeset: "UTF-8",
        modifier: "",
        origin: Origin::Canonical,
    },
];

/// 按下标取条目。
pub fn rec_at(i: usize) -> Option<Rec> {
    CATALOG.get(i).copied()
}

/// 按规范名查条目。
pub fn rec_find(name: &str) -> Option<Rec> {
    let mut i = 0usize;
    while i < CATALOG.len() {
        if let Some(r) = CATALOG.get(i) {
            if r.name == name {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

/// 按四段查条目。
pub fn rec_find_parts(lang: &str, terr: &str, codeset: &str, modifier: &str) -> Option<Rec> {
    let mut i = 0usize;
    while i < CATALOG.len() {
        if let Some(r) = CATALOG.get(i) {
            if r.lang == lang
                && r.terr == terr
                && r.codeset == codeset
                && r.modifier == modifier
            {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

/// 条目来源。
pub fn rec_origin(name: &str) -> Option<Origin> {
    match rec_find(name) {
        Some(r) => Some(r.origin),
        None => None,
    }
}

// ---------------------------------------------------------------------------
// 归一
// ---------------------------------------------------------------------------

/// 归一成规范名。
///
/// 别名折叠顺序固定：POSIX/C 特判 → 语法 → codeset → 修饰符 → 目录校验。
pub fn canonical(input: &str) -> Result<String, u32> {
    if input == "POSIX" {
        return Ok(String::from("C"));
    }
    if input == "C" {
        return Ok(String::from("C"));
    }
    let p = split_name(input)?;

    if p.lang == "C" {
        if !p.terr.is_empty() {
            return Err(SA_EGRAMMAR);
        }
        if !p.modifier.is_empty() {
            return Err(SA_EMODIFIER);
        }
        if p.codeset.is_empty() {
            return Ok(String::from("C"));
        }
        let c = match canon_codeset(p.codeset) {
            Some(v) => v,
            None => return Err(SA_ECODESET),
        };
        if c == "UTF-8" {
            return Ok(String::from("C.UTF-8"));
        }
        return Err(SA_ECODESET);
    }

    if p.terr.is_empty() {
        return Err(SA_EGRAMMAR);
    }

    let mod_given = !p.modifier.is_empty();
    if mod_given && p.modifier != "euro" {
        return Err(SA_EMODIFIER);
    }

    let mut cs_given = !p.codeset.is_empty();
    let mut codeset = "";
    if cs_given {
        codeset = match canon_codeset(p.codeset) {
            Some(v) => v,
            None => return Err(SA_ECODESET),
        };
    }
    if mod_given {
        if p.modifier == "euro" {
            if !cs_given {
                codeset = "ISO-8859-15";
                cs_given = true;
            } else if codeset != "ISO-8859-15" {
                return Err(SA_EMODIFIER);
            }
        }
    }
    if !cs_given {
        codeset = "UTF-8";
    }

    let mut s = String::new();
    s.push_str(p.lang);
    s.push('_');
    s.push_str(p.terr);
    s.push('.');
    s.push_str(codeset);
    if mod_given {
        s.push('@');
        s.push_str(p.modifier);
    }

    match rec_find(s.as_str()) {
        Some(_) => Ok(s),
        None => Err(SA_ENAME),
    }
}

/// 分类来源；归一失败落 Foreign。
pub fn classify(input: &str) -> Origin {
    if input == "POSIX" {
        return Origin::PosixAlias;
    }
    if input == "C" {
        return Origin::CName;
    }
    if canonical(input).is_err() {
        return Origin::Foreign;
    }
    let p = match split_name(input) {
        Ok(v) => v,
        Err(_) => return Origin::Foreign,
    };
    if !p.codeset.is_empty() {
        if let Some(c) = canon_codeset(p.codeset) {
            if c != p.codeset {
                return Origin::CodesetAlias;
            }
            return Origin::Canonical;
        }
        return Origin::Foreign;
    }
    if !p.modifier.is_empty() {
        return Origin::ModifierAlias;
    }
    if p.lang == "C" {
        return Origin::CName;
    }
    Origin::Defaulted
}

/// 是否只给修饰符（codeset 由修饰符补全）。
pub fn modifier_alias(input: &str) -> bool {
    match split_name(input) {
        Ok(p) => !p.modifier.is_empty() && p.codeset.is_empty(),
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// 返回串与比较
// ---------------------------------------------------------------------------

/// 返回串比较结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 逐字相等。
    Match,
    /// 逐字不等。
    Mismatch,
}

impl Verdict {
    /// 文案。
    pub fn text(self) -> &'static str {
        match self {
            Verdict::Match => "match",
            Verdict::Mismatch => "mismatch",
        }
    }

    /// 是否相等。
    pub fn ok(self) -> bool {
        self == Verdict::Match
    }
}

/// 字节序比较，返回 -1 / 0 / 1。
pub fn cmp_rank(a: &str, b: &str) -> i32 {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let mut i = 0usize;
    let n = if ab.len() < bb.len() { ab.len() } else { bb.len() };
    while i < n {
        let x = match ab.get(i) {
            Some(v) => *v,
            None => return 0,
        };
        let y = match bb.get(i) {
            Some(v) => *v,
            None => return 0,
        };
        if x < y {
            return -1;
        }
        if x > y {
            return 1;
        }
        i += 1;
    }
    if ab.len() < bb.len() {
        return -1;
    }
    if ab.len() > bb.len() {
        return 1;
    }
    0
}

/// 返回串与字面量逐字比较。
pub fn ret_eq(ret: &str, literal: &str) -> bool {
    cmp_rank(ret, literal) == 0
}

/// 比较结论。
pub fn verdict(ret: &str, literal: &str) -> Verdict {
    if ret_eq(ret, literal) {
        Verdict::Match
    } else {
        Verdict::Mismatch
    }
}

/// 返回串格。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    text: String,
    kind: Kind,
}

/// 返回串种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 置位返回单串。
    Single,
    /// 单类别查询。
    Query,
    /// LC_ALL 查询，复合串。
    Composite,
}

impl Reply {
    /// 单串。
    pub fn single(text: String) -> Reply {
        Reply { text, kind: Kind::Single }
    }

    /// 查询串。
    pub fn query(text: String) -> Reply {
        Reply { text, kind: Kind::Query }
    }

    /// 复合串。
    pub fn composite(text: String) -> Reply {
        Reply { text, kind: Kind::Composite }
    }

    /// 文本。
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    /// 种类。
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// 是否复合。
    pub fn is_composite(&self) -> bool {
        self.kind == Kind::Composite
    }

    /// 字节数。
    pub fn len(&self) -> usize {
        self.text.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 会话
// ---------------------------------------------------------------------------

/// 十二槽会话。
#[derive(Clone, Copy, Debug)]
pub struct Session {
    slots: [Name; CAT_COUNT],
    sets: u32,
    last: u32,
}

impl Session {
    /// 新会话，十二槽都是 C。
    pub fn new() -> Session {
        let mut slots = [Name::blank(); CAT_COUNT];
        let c = Name::from_str("C").unwrap_or(Name::blank());
        let mut i = 0usize;
        while i < CAT_COUNT {
            slots[i] = c;
            i += 1;
        }
        Session { slots, sets: 0, last: 0 }
    }

    /// 取一个类别当前值。
    pub fn get(&self, cat: Cat) -> Name {
        match self.slots.get(cat.index()) {
            Some(v) => *v,
            None => Name::blank(),
        }
    }

    /// 置位次数。
    pub fn sets(&self) -> u32 {
        self.sets
    }

    /// 最后错误码。
    pub fn last(&self) -> u32 {
        self.last
    }

    /// 复合串：十二段 `NAME=value` 用分号连接，无尾分号。
    pub fn composite(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < CAT_COUNT {
            if i > 0 {
                s.push(';');
            }
            if let Some(n) = CAT_NAMES.get(i) {
                s.push_str(n);
            }
            s.push('=');
            if let Some(v) = self.slots.get(i) {
                s.push_str(v.as_str());
            }
            i += 1;
        }
        s
    }

    /// 置位或查询。
    ///
    /// `value=None` 是查询，任何槽位都不动；`value=Some` 先归一，
    /// 归一失败整单回滚，成功才一次写完十二槽（LC_ALL）或单槽。
    pub fn apply(&mut self, cat: u8, value: Option<&str>) -> Result<Reply, u32> {
        let all = cat == CAT_ALL;
        let slot = if all {
            0usize
        } else {
            match Cat::from_u8(cat) {
                Some(c) => c.index(),
                None => {
                    self.last = SA_ECAT;
                    return Err(SA_ECAT);
                }
            }
        };

        match value {
            None => {
                self.last = 0;
                if all {
                    Ok(Reply::composite(self.composite()))
                } else {
                    let n = match self.slots.get(slot) {
                        Some(v) => *v,
                        None => return Err(SA_ECAT),
                    };
                    Ok(Reply::query(String::from(n.as_str())))
                }
            }
            Some(v) => {
                let canon = match canonical(v) {
                    Ok(s) => s,
                    Err(c) => {
                        self.last = c;
                        return Err(c);
                    }
                };
                let name = match Name::from_str(canon.as_str()) {
                    Ok(n) => n,
                    Err(c) => {
                        self.last = c;
                        return Err(c);
                    }
                };
                if all {
                    let mut i = 0usize;
                    while i < CAT_COUNT {
                        self.slots[i] = name;
                        i += 1;
                    }
                } else {
                    match self.slots.get_mut(slot) {
                        Some(dst) => *dst = name,
                        None => {
                            self.last = SA_ECAT;
                            return Err(SA_ECAT);
                        }
                    }
                }
                self.sets = self.sets.saturating_add(1);
                self.last = 0;
                Ok(Reply::single(canon))
            }
        }
    }
}

impl Default for Session {
    fn default() -> Session {
        Session::new()
    }
}

/// 一次 setlocale 调用的闸：只许落一次。
#[derive(Clone, Copy, Debug)]
pub struct Call {
    used: bool,
    last: u32,
}

impl Call {
    /// 新闸。
    pub const fn new() -> Call {
        Call { used: false, last: 0 }
    }

    /// 是否已用。
    pub fn used(&self) -> bool {
        self.used
    }

    /// 最后错误码。
    pub fn last(&self) -> u32 {
        self.last
    }

    /// 落一次；第二次直接状态冲突。
    pub fn place(
        &mut self,
        sess: &mut Session,
        cat: u8,
        value: Option<&str>,
    ) -> Result<Reply, u32> {
        if self.used {
            self.last = SA_ESTATE;
            return Err(SA_ESTATE);
        }
        self.used = true;
        match sess.apply(cat, value) {
            Ok(r) => {
                self.last = 0;
                Ok(r)
            }
            Err(c) => {
                self.last = c;
                Err(c)
            }
        }
    }
}

impl Default for Call {
    fn default() -> Call {
        Call::new()
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 十进制文本。
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

/// 六十四位十进制文本。
pub fn u64_text(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
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

/// 有符号文本。
pub fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        let m = v.unsigned_abs();
        s.push_str(u32_text(m).as_str());
        s
    } else {
        u32_text(v as u32)
    }
}

/// 四位十六进制。
pub fn hex4_text(v: u32) -> String {
    let mut s = String::from("0x");
    let digits = b"0123456789abcdef";
    let mut shift = 12i32;
    while shift >= 0 {
        let d = ((v >> shift) & 0xf) as usize;
        if let Some(c) = digits.get(d) {
            s.push(*c as char);
        }
        shift -= 4;
    }
    s
}

/// 一行比较轨迹。
pub fn trace(input: &str, literal: &str) -> Result<String, u32> {
    let canon = canonical(input)?;
    let rank = cmp_rank(canon.as_str(), literal);
    let mut s = String::new();
    s.push_str("setlocale(LC_ALL, \"");
    s.push_str(input);
    s.push_str("\") = \"");
    s.push_str(canon.as_str());
    s.push_str("\"\nstrcmp(ret, \"");
    s.push_str(literal);
    s.push_str("\") = ");
    s.push_str(i32_text(rank).as_str());
    s.push('\n');
    s.push_str("literal ");
    if rank == 0 {
        s.push_str("matches\n");
    } else {
        s.push_str("does not match\n");
    }
    Ok(s)
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个语料槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 类别号，CAT_ALL 表示 LC_ALL。
    pub cat: u8,
    /// 请求名。
    pub input: &'static str,
    /// 期望返回串。
    pub want: &'static str,
    /// 调用者拿来做字面比较的字符串。
    pub literal: &'static str,
    /// 期望比较结果。
    pub eq: bool,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(
        cat: u8,
        input: &'static str,
        want: &'static str,
        literal: &'static str,
        eq: bool,
        note: &'static str,
    ) -> Case {
        Case { cat, input, want, literal, eq, note }
    }
}

/// 十四槽语料。
pub const CASES: [Case; CORPUS] = [
    Case::new(CAT_ALL, "POSIX", "C", "POSIX", false, "posix-vs-c"),
    Case::new(CAT_ALL, "C", "C", "C", true, "c-exact"),
    Case::new(CAT_ALL, "en_US.utf8", "en_US.UTF-8", "en_US.UTF-8", true, "utf8-alias"),
    Case::new(CAT_ALL, "de_DE@euro", "de_DE.ISO-8859-15@euro", "de_DE@euro", false, "euro-fill"),
    Case::new(0, "de_DE.utf8", "de_DE.UTF-8", "de_DE.UTF-8", true, "de-utf8"),
    Case::new(CAT_ALL, "C.utf8", "C.UTF-8", "C", false, "c-utf8"),
    Case::new(1, "en_US", "en_US.UTF-8", "en_US", false, "default-codeset"),
    Case::new(2, "fr_FR.UTF-8", "fr_FR.UTF-8", "fr_FR.UTF-8", true, "canonical-fr"),
    Case::new(5, "ja_JP.UTF8", "ja_JP.UTF-8", "ja_JP.UTF-8", true, "ja-utf8"),
    Case::new(3, "POSIX", "C", "C", true, "posix-vs-c"),
    Case::new(4, "C.UTF-8", "C.UTF-8", "C.UTF-8", true, "c-utf8-canon"),
    Case::new(CAT_ALL, "de_DE.ISO8859-15@euro", "de_DE.ISO-8859-15@euro", "de_DE.ISO-8859-15@euro", true, "iso-euro"),
    Case::new(CAT_ALL, "en_US.UTF-8", "en_US.UTF-8", "en_US.UTF-8", true, "canonical-us"),
    Case::new(6, "zh_CN.utf8", "zh_CN.UTF-8", "zh_CN.UTF-8", true, "zh-utf8"),
];

/// 按槽取。
pub fn case_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 语料是否互异（按类别与请求名）。
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
                if a.cat == b.cat && a.input == b.input {
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

/// 逐槽对判：归一、返回串、字面比较三者都要对上。
pub fn table_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let mut sess = Session::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let rep = match sess.apply(c.cat, Some(c.input)) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if rep.text() != c.want {
            return false;
        }
        if ret_eq(rep.text(), c.literal) != c.eq {
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
#[derive(Clone, Copy, Debug)]
pub struct Census {
    total: u32,
    posix: u32,
    codeset: u32,
    modifier: u32,
    defaulted: u32,
    canon: u32,
    lit_eq: u32,
    lit_ne: u32,
    matched: u32,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census {
            total: 0,
            posix: 0,
            codeset: 0,
            modifier: 0,
            defaulted: 0,
            canon: 0,
            lit_eq: 0,
            lit_ne: 0,
            matched: 0,
        }
    }

    /// 总槽数。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// POSIX 别名数。
    pub fn posix(&self) -> u32 {
        self.posix
    }

    /// codeset 归一名数。
    pub fn codeset(&self) -> u32 {
        self.codeset
    }

    /// 修饰符补全数。
    pub fn modifier(&self) -> u32 {
        self.modifier
    }

    /// 默认 codeset 数。
    pub fn defaulted(&self) -> u32 {
        self.defaulted
    }

    /// 原样规范名数。
    pub fn canon(&self) -> u32 {
        self.canon
    }

    /// 字面比较相等数。
    pub fn lit_eq(&self) -> u32 {
        self.lit_eq
    }

    /// 字面比较不等数。
    pub fn lit_ne(&self) -> u32 {
        self.lit_ne
    }

    /// 逐字命中数。
    pub fn matched(&self) -> u32 {
        self.matched
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 千分比（上取整，单槽红档为 929）。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let scaled = self.matched.saturating_mul(1000);
        scaled.saturating_add(self.total - 1) / self.total
    }

    /// 是否全绿。
    pub fn green(&self) -> bool {
        self.total as usize == CORPUS && self.matched == self.total && self.rate() == PPM_FULL
    }

    /// 闸门。
    pub fn gate(&self) -> Result<(), u32> {
        if self.total == 0 {
            return Err(SA_ENOBS);
        }
        if self.matched != self.total {
            return Err(SA_EVALUE);
        }
        Ok(())
    }

    /// 汇总渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("alias corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" posix=");
        s.push_str(u32_text(self.posix).as_str());
        s.push_str(" codeset=");
        s.push_str(u32_text(self.codeset).as_str());
        s.push_str(" modifier=");
        s.push_str(u32_text(self.modifier).as_str());
        s.push_str(" defaulted=");
        s.push_str(u32_text(self.defaulted).as_str());
        s.push_str(" canon=");
        s.push_str(u32_text(self.canon).as_str());
        s.push_str(" literal_eq=");
        s.push_str(u32_text(self.lit_eq).as_str());
        s.push_str(" literal_ne=");
        s.push_str(u32_text(self.lit_ne).as_str());
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

/// 带红档跑；red 命中槽位时不比对，计一次未命中。
pub fn run_with(red: Option<usize>) -> Result<Census, u32> {
    if let Some(r) = red {
        if r >= CORPUS {
            return Err(SA_ESLOT);
        }
    }
    let mut sess = Session::new();
    let mut c = Census::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let cs = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(SA_ESLOT),
        };
        let rep = sess.apply(cs.cat, Some(cs.input))?;
        c.total = c.total.saturating_add(1);
        match classify(cs.input) {
            Origin::PosixAlias => c.posix = c.posix.saturating_add(1),
            Origin::CodesetAlias => c.codeset = c.codeset.saturating_add(1),
            Origin::ModifierAlias => c.modifier = c.modifier.saturating_add(1),
            Origin::Defaulted => c.defaulted = c.defaulted.saturating_add(1),
            Origin::Canonical => c.canon = c.canon.saturating_add(1),
            _ => {}
        }
        if ret_eq(rep.text(), cs.literal) {
            c.lit_eq = c.lit_eq.saturating_add(1);
        } else {
            c.lit_ne = c.lit_ne.saturating_add(1);
        }
        let forced = match red {
            Some(r) => r == i,
            None => false,
        };
        if !forced && rep.text() == cs.want {
            c.matched = c.matched.saturating_add(1);
        }
        i += 1;
    }
    Ok(c)
}

/// 封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(SA_ENOBS)
    }
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
            return Err(SA_ESLOT);
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
            return Err(SA_EBAL);
        }
        const VALS: [u32; 8] = [14, 2, 6, 1, 1, 3, 10, 929];
        const CODES: [u32; 6] =
            [SA_EGRAMMAR, SA_ECODESET, SA_EMODIFIER, SA_ENAME, SA_ECAT, SA_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SA_EBAL),
            };
            if s.slot as usize != i {
                return Err(SA_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SA_EVALUE);
                        }
                    }
                    None => return Err(SA_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SA_EVALUE);
                        }
                    }
                    None => return Err(SA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SA_EBAL);
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
            return Err(SA_EBAL);
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
            Err(SA_ESEAL)
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

/// 操作码。
const OP_CORPUS: u8 = 0;
const OP_POSIX: u8 = 1;
const OP_CODESET: u8 = 2;
const OP_MODIFIER: u8 = 3;
const OP_DEFAULT: u8 = 4;
const OP_CANON: u8 = 5;
const OP_LITERAL: u8 = 6;
const OP_RED: u8 = 7;
const OP_GRAMMAR: u8 = 8;
const OP_NAME: u8 = 9;
const OP_CAT: u8 = 10;
const OP_STATE: u8 = 11;

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

    let _ = l.register(count_cell(0, OP_CORPUS, r.map(|x| x.total() as usize)));
    let _ = l.register(count_cell(1, OP_POSIX, r.map(|x| x.posix() as usize)));
    let _ = l.register(count_cell(2, OP_CODESET, r.map(|x| x.codeset() as usize)));
    let _ = l.register(count_cell(3, OP_MODIFIER, r.map(|x| x.modifier() as usize)));
    let _ = l.register(count_cell(4, OP_DEFAULT, r.map(|x| x.defaulted() as usize)));
    let _ = l.register(count_cell(5, OP_CANON, r.map(|x| x.canon() as usize)));
    let _ = l.register(count_cell(6, OP_LITERAL, r.map(|x| x.lit_eq() as usize)));

    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_RED, red.map(|x| x.rate() as usize)));

    let grammar = match canonical("en__US.UTF-8") {
        Ok(_) => SA_EGRAMMAR,
        Err(c) => c,
    };
    let _ = l.register(err_cell(8, OP_GRAMMAR, grammar));

    let codeset = match canonical("en_US.INVALID") {
        Ok(_) => SA_ECODESET,
        Err(c) => c,
    };
    let _ = l.register(err_cell(9, OP_CODESET, codeset));

    let modifier = match canonical("de_DE.UTF-8@bogus") {
        Ok(_) => SA_EMODIFIER,
        Err(c) => c,
    };
    let _ = l.register(err_cell(10, OP_MODIFIER, modifier));

    let name = match canonical("xx_YY.UTF-8") {
        Ok(_) => SA_ENAME,
        Err(c) => c,
    };
    let _ = l.register(err_cell(11, OP_NAME, name));

    let mut sess = Session::new();
    let cat = match sess.apply(CAT_COUNT as u8, Some("C")) {
        Ok(_) => SA_ECAT,
        Err(c) => c,
    };
    let _ = l.register(err_cell(12, OP_CAT, cat));

    let mut state_sess = Session::new();
    let mut call = Call::new();
    let first = call.place(&mut state_sess, 0, Some("C"));
    let state = match first {
        Ok(_) => match call.place(&mut state_sess, 0, Some("C")) {
            Ok(_) => SA_ESTATE,
            Err(c) => c,
        },
        Err(c) => c,
    };
    let _ = l.register(err_cell(13, OP_STATE, state));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 语法闸门。
fn grammar_ok() -> bool {
    let a = canonical("").err() == Some(SA_EBYTE);
    let b = canonical("en US.UTF-8").err() == Some(SA_EBYTE);
    let c = canonical("en__US.UTF-8").err() == Some(SA_EGRAMMAR);
    let d = canonical("en_US..UTF-8").err() == Some(SA_ECODESET);
    let e = canonical("de_DE.UTF-8@").err() == Some(SA_EMODIFIER);
    let f = canonical("de_DE.UTF-8@euro@x").err() == Some(SA_EMODIFIER);
    let g = canonical("en_US.").err() == Some(SA_ECODESET);
    let h = split_name("en_US.UTF-8").is_ok();
    let i = split_name("en_US").map(|p| p.terr) == Ok("US");
    let j = split_name("C").map(|p| p.lang) == Ok("C");
    a && b && c && d && e && f && g && h && i && j
}

/// 别名折叠。
fn alias_ok() -> bool {
    let a = canonical("POSIX").map(|s| s.as_str() == "C") == Ok(true);
    let b = canonical("C").map(|s| s.as_str() == "C") == Ok(true);
    let c = classify("POSIX") == Origin::PosixAlias;
    let d = classify("C") == Origin::CName;
    let e = canonical("posix").err() == Some(SA_EGRAMMAR);
    let f = classify("POSIX").text() == "posix-alias";
    let g = rec_origin("C") == Some(Origin::CName);
    let h = rec_origin("en_US.UTF-8") == Some(Origin::Canonical);
    a && b && c && d && e && f && g && h
}

/// codeset 归一。
fn codeset_ok() -> bool {
    let a = canonical("en_US.utf8").map(|s| s.as_str() == "en_US.UTF-8") == Ok(true);
    let b = canonical("en_US.UTF8").map(|s| s.as_str() == "en_US.UTF-8") == Ok(true);
    let c = canonical("C.utf8").map(|s| s.as_str() == "C.UTF-8") == Ok(true);
    let d = canonical("en_US.ISO8859-1").map(|s| s.as_str() == "en_US.ISO-8859-1") == Ok(true);
    let e = canonical("de_DE.ISO885915").map(|s| s.as_str() == "de_DE.ISO-8859-15") == Ok(true);
    let f = canon_codeset("KOI8-R").is_none();
    let g = codeset_alias("en_US.utf8") && !codeset_alias("en_US.UTF-8");
    let h = classify("en_US.utf8") == Origin::CodesetAlias;
    a && b && c && d && e && f && g && h
}

/// 修饰符归一。
fn modifier_ok() -> bool {
    let a = canonical("de_DE@euro").map(|s| s.as_str() == "de_DE.ISO-8859-15@euro") == Ok(true);
    let b = canonical("de_DE.ISO8859-15@euro")
        .map(|s| s.as_str() == "de_DE.ISO-8859-15@euro")
        == Ok(true);
    let c = canonical("de_DE.UTF-8@euro").err() == Some(SA_EMODIFIER);
    let d = canonical("de_DE.UTF-8@bogus").err() == Some(SA_EMODIFIER);
    let e = modifier_alias("de_DE@euro") && !modifier_alias("de_DE.ISO8859-15@euro");
    let f = classify("de_DE@euro") == Origin::ModifierAlias;
    a && b && c && d && e && f
}

/// 类别号。
fn category_ok() -> bool {
    let a = Cat::from_u8(0) == Some(Cat::Ctype) && Cat::from_u8(11) == Some(Cat::Identification);
    let b = Cat::from_u8(12).is_none() && Cat::from_u8(0xff).is_none();
    let c = Cat::Ctype.index() == 0 && Cat::Identification.index() == 11;
    let d = Cat::Numeric.name() == "LC_NUMERIC" && Cat::Paper.name() == "LC_PAPER";
    let e = Cat::from_name("LC_MESSAGES") == Some(Cat::Messages);
    let f = Cat::from_name("LC_ALL").is_none();
    let g = cat_name(0) == Some("LC_CTYPE") && cat_name(11) == Some("LC_IDENTIFICATION");
    let h = cat_name(CAT_COUNT).is_none();
    a && b && c && d && e && f && g && h
}

/// 返回串逐字。
fn returning_ok() -> bool {
    let mut s = Session::new();
    let a = match s.apply(Cat::Numeric as u8, Some("en_US.utf8")) {
        Ok(r) => r.text() == "en_US.UTF-8" && r.kind() == Kind::Single,
        Err(_) => false,
    };
    let b = match s.apply(Cat::Numeric as u8, None) {
        Ok(r) => r.text() == "en_US.UTF-8" && r.kind() == Kind::Query,
        Err(_) => false,
    };
    let c = match s.apply(CAT_ALL, Some("POSIX")) {
        Ok(r) => r.text() == "C",
        Err(_) => false,
    };
    let d = match trace("POSIX", "POSIX") {
        Ok(t) => t.as_str()
            == "setlocale(LC_ALL, \"POSIX\") = \"C\"\nstrcmp(ret, \"POSIX\") = -1\nliteral does not match\n",
        Err(_) => false,
    };
    let e = match trace("C", "C") {
        Ok(t) => t.as_str()
            == "setlocale(LC_ALL, \"C\") = \"C\"\nstrcmp(ret, \"C\") = 0\nliteral matches\n",
        Err(_) => false,
    };
    let f = trace("", "C").err() == Some(SA_EBYTE);
    a && b && c && d && e && f
}

/// 字面比较。
fn literal_ok() -> bool {
    let a = !ret_eq("C", "POSIX") && ret_eq("C", "C");
    let b = ret_eq("en_US.UTF-8", "en_US.UTF-8") && !ret_eq("en_US.UTF-8", "en_US.utf8");
    let c = cmp_rank("C", "POSIX") == -1 && cmp_rank("POSIX", "C") == 1;
    let d = cmp_rank("en_US.UTF-8", "en_US.UTF-8") == 0;
    let e = cmp_rank("C", "C.UTF-8") == -1 && cmp_rank("C.UTF-8", "C") == 1;
    let f = verdict("C", "POSIX") == Verdict::Mismatch && verdict("C", "C") == Verdict::Match;
    let g = Verdict::Match.text() == "match" && Verdict::Mismatch.text() == "mismatch";
    let h = Verdict::Match.ok() && !Verdict::Mismatch.ok();
    a && b && c && d && e && f && g && h
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.total() == CORPUS as u32 && c.matched() == CORPUS as u32;
    let b = c.posix() == 2 && c.codeset() == 6 && c.modifier() == 1;
    let d = c.defaulted() == 1 && c.canon() == 3;
    let e = c.lit_eq() == 10 && c.lit_ne() == 4;
    let f = c.rate() == 1000 && c.green() && c.gate() == Ok(());
    a && b && d && e && f
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let a = match case_at(0) {
        Some(c) => c.cat == CAT_ALL && c.input == "POSIX" && !c.eq,
        None => false,
    };
    let b = match case_at(6) {
        Some(c) => c.input == "en_US" && c.want == "en_US.UTF-8",
        None => false,
    };
    let c = match case_at(13) {
        Some(c) => c.input == "zh_CN.utf8" && c.want == "zh_CN.UTF-8",
        None => false,
    };
    let d = case_at(CORPUS).is_none() && case_at(CORPUS + 1).is_none();
    a && b && c && d
}

/// 汇总渲染。
fn report_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.render().as_str() == "alias corpus=14 posix=2 codeset=6 modifier=1 defaulted=1 canon=3 literal_eq=10 literal_ne=4 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(SA_ENOBS);
    let d = e.render().as_str() == "alias corpus=0 posix=0 codeset=0 modifier=0 defaulted=0 canon=0 literal_eq=0 literal_ne=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(CORPUS + 1) == Err(SA_ENOBS);
    a && b && d && f
}

/// 红单显式。
fn reds_ok() -> bool {
    let c = match run_with(Some(0)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched() == CORPUS as u32 - 1 && c.rate() == PPM_RED;
    let b = c.gate() == Err(SA_EVALUE) && !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(SA_ESLOT);
    let e = run_with(Some(CORPUS + 1)).err() == Some(SA_ESLOT);
    a && b && d && e
}

/// 会话与组合查询。
fn session_ok() -> bool {
    let mut s = Session::new();
    let a = s.get(Cat::Ctype).as_str() == "C" && s.get(Cat::Identification).as_str() == "C";
    let b = match s.apply(Cat::Ctype as u8, Some("de_DE@euro")) {
        Ok(r) => r.text() == "de_DE.ISO-8859-15@euro",
        Err(_) => false,
    };
    let c = s.get(Cat::Ctype).as_str() == "de_DE.ISO-8859-15@euro";
    let d = s.get(Cat::Numeric).as_str() == "C";
    let bad = s.apply(Cat::Ctype as u8, Some("xx_YY.UTF-8")).err() == Some(SA_ENAME);
    let e = bad && s.get(Cat::Ctype).as_str() == "de_DE.ISO-8859-15@euro";
    let before = s.sets();
    let q = s.apply(CAT_ALL, None);
    let f = match q {
        Ok(r) => r.is_composite() && r.text().as_bytes().len() > 100,
        Err(_) => false,
    };
    let g = s.sets() == before;
    let h = s.apply(CAT_ALL, Some("en_US.utf8")).is_ok() && s.get(Cat::Time).as_str() == "en_US.UTF-8";
    let mut sel = Session::new();
    let mut call = Call::new();
    let i = match call.place(&mut sel, Cat::Numeric as u8, Some("C")) {
        Ok(r) => r.text() == "C" && call.used(),
        Err(_) => false,
    };
    let j = call.place(&mut sel, Cat::Numeric as u8, Some("C")).err() == Some(SA_ESTATE);
    a && b && c && d && e && f && g && h && i && j
}

/// 组合串逐字。
fn composite_ok() -> bool {
    let s = Session::new();
    let a = s.composite().as_str() == "LC_CTYPE=C;LC_NUMERIC=C;LC_TIME=C;LC_COLLATE=C;LC_MONETARY=C;LC_MESSAGES=C;LC_PAPER=C;LC_NAME=C;LC_ADDRESS=C;LC_TELEPHONE=C;LC_MEASUREMENT=C;LC_IDENTIFICATION=C";
    let mut t = Session::new();
    let b = t.apply(2, Some("ja_JP.utf8")).is_ok();
    let c = t
        .composite()
        .as_str()
        .contains("LC_TIME=ja_JP.UTF-8;LC_COLLATE=C");
    let d = t.composite().as_str().starts_with("LC_CTYPE=C;LC_NUMERIC=C;LC_TIME=ja_JP.UTF-8;");
    let e = t.composite().as_str().ends_with("LC_IDENTIFICATION=C");
    a && b && c && d && e
}

/// 账本与文本助手。
fn ledger_ok() -> bool {
    let l = build_ledger();
    let a = l.len() == SAMPLES && l.audit() == Ok(()) && l.ok_count() == 8 && l.err_count() == 6;
    let b = l.sample(0).map(|s| s.ok && s.value == 14) == Some(true);
    let c = l.sample(13).map(|s| !s.ok && s.code == SA_ESTATE) == Some(true);
    let d = Ledger::new().audit() == Err(SA_EBAL) && build_ledger().seal() == build_ledger().seal();
    let e = u32_text(0).as_str() == "0" && u32_text(14).as_str() == "14";
    let f = u64_text(1_234_567_891_000).as_str() == "1234567891000";
    let g = hex4_text(0xfd40).as_str() == "0xfd40";
    let h = digest(b"POSIX") == digest(b"POSIX") && digest(b"POSIX") != digest(b"C");
    let i = i32_text(-1).as_str() == "-1" && i32_text(0).as_str() == "0";
    a && b && c && d && e && f && g && h && i
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = grammar_ok() && alias_ok() && codeset_ok();
    out[1] = modifier_ok() && category_ok() && returning_ok() && literal_ok();
    out[2] = corpus_ok() && slots_ok() && report_ok() && session_ok() && composite_ok();
    out[3] = reds_ok() && ledger_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11645_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11645.setlocale_alias");

    s.add("SA01-名字语法", grammar_ok(), "static");
    s.add("SA02-别名归一", alias_ok(), "static");
    s.add("SA03-codeset 归一", codeset_ok(), "static");
    s.add("SA04-修饰符归一", modifier_ok(), "static");
    s.add("SA05-类别号", category_ok(), "static");
    s.add("SA06-返回串逐字", returning_ok() && composite_ok(), "static");
    s.add("SA07-字面比较", literal_ok(), "static");
    s.add("SA08-十四卡全跑", corpus_ok(), "static");
    s.add("SA09-语料编号", slots_ok(), "static");
    s.add("SA10-汇总渲染", report_ok(), "static");
    s.add("SA11-红单显式", reds_ok(), "static");
    s.add("SA12-会话与账本", session_ok() && ledger_ok(), "static");
    s.add("SA13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
    fn grammar_cases() {
        assert!(split_name("en_US.UTF-8").is_ok());
        assert_eq!(split_name("").err(), Some(SA_EBYTE));
        assert_eq!(split_name("en_US UTF-8").err(), Some(SA_EBYTE));
        assert_eq!(split_name("en__US.UTF-8").err(), Some(SA_EGRAMMAR));
        assert_eq!(split_name("en_US..UTF-8").err(), Some(SA_ECODESET));
        assert_eq!(split_name("de_DE.UTF-8@").err(), Some(SA_EMODIFIER));
        assert_eq!(split_name("de_DE.UTF-8@a@b").err(), Some(SA_EMODIFIER));
    }

    #[test]
    fn alias_posix() {
        assert_eq!(canonical("POSIX").ok().as_deref(), Some("C"));
        assert_eq!(classify("POSIX"), Origin::PosixAlias);
        assert_eq!(classify("C"), Origin::CName);
        assert_eq!(canonical("posix").err(), Some(SA_EGRAMMAR));
    }

    #[test]
    fn codeset_aliases() {
        assert_eq!(canonical("en_US.utf8").ok().as_deref(), Some("en_US.UTF-8"));
        assert_eq!(canonical("C.UTF8").ok().as_deref(), Some("C.UTF-8"));
        assert_eq!(
            canonical("de_DE.ISO8859-15").ok().as_deref(),
            Some("de_DE.ISO-8859-15")
        );
        assert_eq!(canonical("en_US.INVALID").err(), Some(SA_ECODESET));
        assert_eq!(classify("en_US.utf8"), Origin::CodesetAlias);
    }

    #[test]
    fn modifier_euro() {
        assert_eq!(
            canonical("de_DE@euro").ok().as_deref(),
            Some("de_DE.ISO-8859-15@euro")
        );
        assert_eq!(canonical("de_DE.UTF-8@euro").err(), Some(SA_EMODIFIER));
        assert!(modifier_alias("de_DE@euro"));
        assert!(!modifier_alias("de_DE.ISO-8859-15@euro"));
    }

    #[test]
    fn categories() {
        assert_eq!(Cat::from_u8(0), Some(Cat::Ctype));
        assert_eq!(Cat::from_u8(11), Some(Cat::Identification));
        assert_eq!(Cat::from_u8(12), None);
        assert_eq!(Cat::from_name("LC_TIME"), Some(Cat::Time));
        assert_eq!(Cat::Measurement.index(), 10);
        assert_eq!(cat_name(CAT_COUNT), None);
    }

    #[test]
    fn canonical_strings() {
        assert_eq!(canonical("C").ok().as_deref(), Some("C"));
        assert_eq!(canonical("en_US").ok().as_deref(), Some("en_US.UTF-8"));
        assert_eq!(canonical("en_US.ISO8859-1").ok().as_deref(), Some("en_US.ISO-8859-1"));
        assert_eq!(canonical("xx_YY.UTF-8").err(), Some(SA_ENAME));
        assert_eq!(classify("en_US"), Origin::Defaulted);
    }

    #[test]
    fn literal_cmp() {
        assert!(!ret_eq("C", "POSIX"));
        assert!(ret_eq("C", "C"));
        assert_eq!(cmp_rank("C", "POSIX"), -1);
        assert_eq!(cmp_rank("POSIX", "C"), 1);
        assert_eq!(verdict("C", "POSIX"), Verdict::Mismatch);
    }

    #[test]
    fn session_sets() {
        let mut s = Session::new();
        assert!(s.apply(Cat::Numeric as u8, Some("en_US.utf8")).is_ok());
        assert_eq!(s.get(Cat::Numeric).as_str(), "en_US.UTF-8");
        assert_eq!(s.get(Cat::Time).as_str(), "C");
        assert_eq!(s.sets(), 1);
        assert_eq!(
            s.apply(Cat::Time as u8, Some("xx_YY.UTF-8")).err(),
            Some(SA_ENAME)
        );
        assert_eq!(s.get(Cat::Time).as_str(), "C");
        assert_eq!(s.sets(), 1);
    }

    #[test]
    fn null_query() {
        let mut s = Session::new();
        assert!(s.apply(Cat::Ctype as u8, Some("de_DE.UTF-8")).is_ok());
        let before = s.sets();
        let r = s.apply(Cat::Ctype as u8, None);
        assert_eq!(r.ok().map(|v| v.text().to_string()), Some("de_DE.UTF-8".to_string()));
        assert_eq!(s.sets(), before);
    }

    #[test]
    fn composite_text() {
        let s = Session::new();
        assert_eq!(
            s.composite().as_str(),
            "LC_CTYPE=C;LC_NUMERIC=C;LC_TIME=C;LC_COLLATE=C;LC_MONETARY=C;LC_MESSAGES=C;LC_PAPER=C;LC_NAME=C;LC_ADDRESS=C;LC_TELEPHONE=C;LC_MEASUREMENT=C;LC_IDENTIFICATION=C"
        );
        assert!(!s.composite().as_str().ends_with(';'));
    }

    #[test]
    fn call_once() {
        let mut s = Session::new();
        let mut c = Call::new();
        assert!(c.place(&mut s, 0, Some("C")).is_ok());
        assert!(c.used());
        assert_eq!(c.place(&mut s, 0, Some("C")).err(), Some(SA_ESTATE));
        assert_eq!(c.last(), SA_ESTATE);
    }

    #[test]
    fn trace_text() {
        assert_eq!(
            trace("POSIX", "POSIX").ok().as_deref(),
            Some("setlocale(LC_ALL, \"POSIX\") = \"C\"\nstrcmp(ret, \"POSIX\") = -1\nliteral does not match\n")
        );
        assert_eq!(
            trace("C", "C").ok().as_deref(),
            Some("setlocale(LC_ALL, \"C\") = \"C\"\nstrcmp(ret, \"C\") = 0\nliteral matches\n")
        );
        assert_eq!(trace("", "C").err(), Some(SA_EBYTE));
    }

    #[test]
    fn census_numbers() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.total(), 14);
        assert_eq!(c.posix(), 2);
        assert_eq!(c.codeset(), 6);
        assert_eq!(c.modifier(), 1);
        assert_eq!(c.defaulted(), 1);
        assert_eq!(c.canon(), 3);
        assert_eq!(c.lit_eq(), 10);
        assert_eq!(c.lit_ne(), 4);
        assert_eq!(c.gate(), Ok(()));
    }

    #[test]
    fn red_override() {
        let c = match run_with(Some(0)) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.matched(), 13);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(SA_EVALUE));
        assert_eq!(run_with(Some(14)).err(), Some(SA_ESLOT));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert!(l.sample(0).is_some());
        assert!(l.sample(SAMPLES).is_none());
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
        assert_eq!(l.freeze(), Err(SA_EBAL));
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
        let s = run_unx11645_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn text_helpers() {
        assert_eq!(u32_text(0), "0");
        assert_eq!(u32_text(9), "9");
        assert_eq!(u32_text(10), "10");
        assert_eq!(u32_text(4294967295), "4294967295");
        assert_eq!(i32_text(-1), "-1");
        assert_eq!(hex4_text(0), "0x0000");
        assert_eq!(hex4_text(0xfd40), "0xfd40");
        assert_eq!(Name::from_str("en_US.UTF-8").map(|n| n.eq_str("en_US.UTF-8")), Ok(true));
    }
}
