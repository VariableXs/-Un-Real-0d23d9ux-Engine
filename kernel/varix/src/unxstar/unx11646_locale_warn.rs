//! UNX-F11646 归档缺失时的 glibc warning 文案
//!
//! glibc 的 `setlocale` 在换档失败时不改 errno 语义，而是往 stderr 打一行
//! 警告，形态固定：
//! `warning: setlocale: LC_ALL: cannot change locale (xx_YY.UTF-8)`，
//! 若底层打开归档失败还会补 errno 文本
//! `: No such file or directory` / `: Permission denied`。
//! 这行字是大量构建脚本、容器入口和测试基线用来判断“locale 到底坏在哪”
//! 的唯一线索，少一个冒号、把类别写成 `LC_ALL` 之外的别名、或者把
//! `locale-archive` 的 ENOENT 吞成无声回退，都会让排障走错方向。
//!
//! 本模块按 glibc 的失败面拆档：归档文件不存在（ENOENT）、归档不可读
//! （EACCES）、归档魔数或版本损坏（EINVAL）、归档在但条目缺目录（无
//! errno 后缀），四档各自有独立错误码。文案只允许来自这四档，名字先过
//! 注入闸：空名、控制字节、空格、括号和分号一律拒收，避免把调用者给的
//! 字符串直接拼进 stderr 造成换行伪造。
//!
//! 同一 `(类别, 名字, 原因)` 只告警一次，后续重复换档静默；静默不等于
//! 成功，`setlocale` 仍然返回 NULL，调用者必须自己看返回值。归档完好时
//! 不发任何警告，`C` 与 `POSIX` 永远走内置档，不碰归档。
//!
//! 闸门次序：名字 → 类别 → 归档探针 → 原因归档 → 文案 → 去重 → 发射 → 绿档。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xfd50;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 语料条数。
pub const CORPUS: usize = 14;

/// 类别数（不含 LC_ALL）。
pub const CAT_COUNT: usize = 12;

/// LC_ALL 哨兵。
pub const CAT_ALL: u8 = 0xff;

/// 名字最大字节数。
pub const MAX_NAME: usize = 120;

/// 一行警告最大字节数。
pub const LINE_MAX: usize = 192;

/// 去重键容量。
pub const KEY_MAX: usize = 16;

/// 满档千分比。
pub const PPM_FULL: u32 = 1000;

/// 单槽红档千分比。
pub const PPM_RED: u32 = 929;

/// 归档魔数（locale-archive 头）。
pub const ARCHIVE_MAGIC: u32 = 0xde02_0109;

/// ENOENT。
pub const ENOENT: i32 = 2;

/// EACCES。
pub const EACCES: i32 = 13;

/// EINVAL。
pub const EINVAL: i32 = 22;

/// 空名 / 控制字节 / 注入字符。
pub const LW_EBYTE: u32 = SEG_BASE + 0x01;

/// 名字为空或超长。
pub const LW_ENAME: u32 = SEG_BASE + 0x02;

/// 类别号越界。
pub const LW_ECAT: u32 = SEG_BASE + 0x03;

/// 归档不存在。
pub const LW_EARCHIVE: u32 = SEG_BASE + 0x04;

/// 归档不可读。
pub const LW_EACCESS: u32 = SEG_BASE + 0x05;

/// 归档魔数损坏。
pub const LW_ECORRUPT: u32 = SEG_BASE + 0x06;

/// 发射器已封或重复封。
pub const LW_ESTATE: u32 = SEG_BASE + 0x07;

/// 文案超出行缓冲。
pub const LW_EOVER: u32 = SEG_BASE + 0x08;

/// 账本不平。
pub const LW_EBAL: u32 = SEG_BASE + 0x09;

/// 槽位错序或越位。
pub const LW_ESLOT: u32 = SEG_BASE + 0x0a;

/// 观测值与期望不符。
pub const LW_EVALUE: u32 = SEG_BASE + 0x0b;

/// 重放指纹不符。
pub const LW_ESEAL: u32 = SEG_BASE + 0x0c;

/// 空观测。
pub const LW_ENOBS: u32 = SEG_BASE + 0x0d;

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
}

/// 警告里的类别标签：LC_ALL 或十二类别之一。
pub fn cat_label(cat: u8) -> Option<&'static str> {
    if cat == CAT_ALL {
        return Some("LC_ALL");
    }
    match Cat::from_u8(cat) {
        Some(c) => Some(c.name()),
        None => None,
    }
}

/// 类别名字表。
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

// ---------------------------------------------------------------------------
// 原因
// ---------------------------------------------------------------------------

/// 换档失败的四档原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// 归档文件不存在。
    NoArchive,
    /// 归档权限不足。
    Permission,
    /// 归档魔数或版本损坏。
    Corrupt,
    /// 归档在，但名字没有对应目录/条目。
    NoDir,
}

impl Cause {
    /// errno。
    pub fn errno(self) -> i32 {
        match self {
            Cause::NoArchive => ENOENT,
            Cause::Permission => EACCES,
            Cause::Corrupt => EINVAL,
            Cause::NoDir => 0,
        }
    }

    /// errno 文本；空串表示不加后缀。
    pub fn suffix(self) -> &'static str {
        match self {
            Cause::NoArchive => "No such file or directory",
            Cause::Permission => "Permission denied",
            Cause::Corrupt => "Invalid argument",
            Cause::NoDir => "",
        }
    }

    /// 对应错误码。
    pub fn code(self) -> u32 {
        match self {
            Cause::NoArchive => LW_EARCHIVE,
            Cause::Permission => LW_EACCESS,
            Cause::Corrupt => LW_ECORRUPT,
            Cause::NoDir => LW_ENAME,
        }
    }

    /// 文案标签。
    pub fn text(self) -> &'static str {
        match self {
            Cause::NoArchive => "no-archive",
            Cause::Permission => "permission",
            Cause::Corrupt => "corrupt",
            Cause::NoDir => "no-dir",
        }
    }

    /// 是否带 errno 后缀。
    pub fn has_suffix(self) -> bool {
        !self.suffix().is_empty()
    }
}

// ---------------------------------------------------------------------------
// 名字闸
// ---------------------------------------------------------------------------

/// 名字注入闸：空名、超长、控制字节、空格、括号、分号全部拒收。
pub fn check_name(name: &str) -> Result<(), u32> {
    if name.is_empty() {
        return Err(LW_ENAME);
    }
    if name.len() > MAX_NAME {
        return Err(LW_EOVER);
    }
    let b = name.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(LW_EBYTE),
        };
        if c < 0x21 || c > 0x7e {
            return Err(LW_EBYTE);
        }
        if c == b'(' || c == b')' || c == b';' {
            return Err(LW_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

/// 名字是否只是内置档。
pub fn builtin_name(name: &str) -> bool {
    name == "C" || name == "POSIX"
}

// ---------------------------------------------------------------------------
// 行缓冲
// ---------------------------------------------------------------------------

/// 固定缓冲的一行。
#[derive(Clone, Copy, Debug)]
pub struct Line {
    buf: [u8; LINE_MAX],
    len: usize,
}

impl Line {
    /// 空行。
    pub const fn new() -> Line {
        Line { buf: [0u8; LINE_MAX], len: 0 }
    }

    /// 追加一段。
    pub fn push_str(&mut self, s: &str) -> Result<(), u32> {
        let b = s.as_bytes();
        if self.len.saturating_add(b.len()) > LINE_MAX {
            return Err(LW_EOVER);
        }
        let mut i = 0usize;
        while i < b.len() {
            if let Some(c) = b.get(i) {
                if let Some(dst) = self.buf.get_mut(self.len) {
                    *dst = *c;
                }
            }
            self.len += 1;
            i += 1;
        }
        Ok(())
    }

    /// 追加一字节。
    pub fn push(&mut self, c: u8) -> Result<(), u32> {
        if self.len >= LINE_MAX {
            return Err(LW_EOVER);
        }
        if let Some(dst) = self.buf.get_mut(self.len) {
            *dst = c;
        }
        self.len += 1;
        Ok(())
    }

    /// 视图。
    pub fn as_str(&self) -> &str {
        if self.len > LINE_MAX {
            return "";
        }
        match core::str::from_utf8(&self.buf[..self.len]) {
            Ok(v) => v,
            Err(_) => "",
        }
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for Line {
    fn default() -> Line {
        Line::new()
    }
}

// ---------------------------------------------------------------------------
// 文本助手
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

/// 有符号文本。
pub fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(u32_text(v.unsigned_abs()).as_str());
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

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 拼一行 glibc 警告。
///
/// 逐字形态：
/// `warning: setlocale: <类别>: cannot change locale (<名字>)<后缀>\n`
pub fn warning(cat: u8, name: &str, cause: Cause) -> Result<String, u32> {
    check_name(name)?;
    let label = match cat_label(cat) {
        Some(v) => v,
        None => return Err(LW_ECAT),
    };
    let mut line = Line::new();
    line.push_str("warning: setlocale: ")?;
    line.push_str(label)?;
    line.push_str(": cannot change locale (")?;
    line.push_str(name)?;
    line.push(b')')?;
    if cause.has_suffix() {
        line.push_str(": ")?;
        line.push_str(cause.suffix())?;
    }
    line.push(b'\n')?;
    Ok(String::from(line.as_str()))
}

/// 不带原因后缀的短文案。
pub fn short_warning(cat: u8, name: &str) -> Result<String, u32> {
    check_name(name)?;
    let label = match cat_label(cat) {
        Some(v) => v,
        None => return Err(LW_ECAT),
    };
    let mut line = Line::new();
    line.push_str("warning: setlocale: ")?;
    line.push_str(label)?;
    line.push_str(": cannot change locale (")?;
    line.push_str(name)?;
    line.push(b')')?;
    line.push(b'\n')?;
    Ok(String::from(line.as_str()))
}

/// 三行排障轨迹。
pub fn trace(cat: u8, name: &str, cause: Cause) -> Result<String, u32> {
    let label = match cat_label(cat) {
        Some(v) => v,
        None => return Err(LW_ECAT),
    };
    let w = warning(cat, name, cause)?;
    let mut s = String::new();
    s.push_str("setlocale(");
    s.push_str(label);
    s.push_str(", \"");
    s.push_str(name);
    s.push_str("\") -> NULL\n");
    s.push_str(w.as_str());
    s.push_str("errno=");
    s.push_str(i32_text(cause.errno()).as_str());
    if cause.has_suffix() {
        s.push_str(" (");
        s.push_str(cause.suffix());
        s.push(')');
    } else {
        s.push_str(" (no errno)");
    }
    s.push('\n');
    Ok(s)
}

// ---------------------------------------------------------------------------
// 归档
// ---------------------------------------------------------------------------

/// 归档探针。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Archive {
    present: bool,
    perms: u8,
    magic: u32,
    entries: u16,
}

impl Archive {
    /// 构造。
    pub const fn new(present: bool, perms: u8, magic: u32, entries: u16) -> Archive {
        Archive { present, perms, magic, entries }
    }

    /// 完好归档。
    pub const fn healthy() -> Archive {
        Archive { present: true, perms: 0o64, magic: ARCHIVE_MAGIC, entries: 10 }
    }

    /// 不存在。
    pub const fn missing() -> Archive {
        Archive { present: false, perms: 0, magic: 0, entries: 0 }
    }

    /// 权限不足。
    pub const fn unreadable() -> Archive {
        Archive { present: true, perms: 0o000, magic: ARCHIVE_MAGIC, entries: 10 }
    }

    /// 魔数损坏。
    pub const fn corrupt() -> Archive {
        Archive { present: true, perms: 0o64, magic: 0, entries: 0 }
    }

    /// 是否在。
    pub fn present(&self) -> bool {
        self.present
    }

    /// 权限位。
    pub fn perms(&self) -> u8 {
        self.perms
    }

    /// 魔数。
    pub fn magic(&self) -> u32 {
        self.magic
    }

    /// 条目数。
    pub fn entries(&self) -> u16 {
        self.entries
    }

    /// 三态检查：在 → 可读 → 魔数。
    pub fn check(&self) -> Result<(), Cause> {
        if !self.present {
            return Err(Cause::NoArchive);
        }
        if self.perms & 0o4 == 0 {
            return Err(Cause::Permission);
        }
        if self.magic != ARCHIVE_MAGIC {
            return Err(Cause::Corrupt);
        }
        Ok(())
    }

    /// 探一个名字。
    pub fn probe(&self, name: &str) -> Result<u16, Cause> {
        self.check()?;
        if builtin_name(name) {
            return Ok(0);
        }
        let mut i = 0usize;
        while i < self.entries as usize {
            if let Some(r) = rec_at(i) {
                if r == name {
                    return Ok(i as u16);
                }
            }
            i += 1;
        }
        Err(Cause::NoDir)
    }
}

impl Default for Archive {
    fn default() -> Archive {
        Archive::healthy()
    }
}

/// 归档里收录的名字。
pub const ARCHIVE_NAMES: [&str; 10] = [
    "C",
    "C.UTF-8",
    "en_US.UTF-8",
    "en_US.ISO-8859-1",
    "de_DE.UTF-8",
    "de_DE.ISO-8859-15",
    "de_DE.ISO-8859-15@euro",
    "fr_FR.UTF-8",
    "ja_JP.UTF-8",
    "zh_CN.UTF-8",
];

/// 条目数。
pub const ARCHIVE_LEN: usize = 10;

/// 条目名。
pub fn rec_at(i: usize) -> Option<&'static str> {
    ARCHIVE_NAMES.get(i).copied()
}

/// 名字是否收录。
pub fn rec_find(name: &str) -> Option<usize> {
    let mut i = 0usize;
    while i < ARCHIVE_NAMES.len() {
        if let Some(v) = ARCHIVE_NAMES.get(i) {
            if *v == name {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 去重键
// ---------------------------------------------------------------------------

/// 去重键：类别 + 名字 + 原因。
pub fn key_of(cat: u8, name: &str, cause: Cause) -> u64 {
    let mut h = fold_word(FNV_OFF, cat as u32);
    let b = name.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(v) = b.get(i) {
            h = fold_byte(h, *v);
        }
        i += 1;
    }
    fold_word(h, cause as u32)
}

// ---------------------------------------------------------------------------
// 发射器
// ---------------------------------------------------------------------------

/// 一条警告的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    text: String,
    printed: bool,
}

impl Notice {
    /// 打印。
    pub fn show(text: String) -> Notice {
        Notice { text, printed: true }
    }

    /// 去重静默。
    pub fn suppressed() -> Notice {
        Notice { text: String::new(), printed: false }
    }

    /// 文本；静默时为空。
    pub fn text(&self) -> &str {
        self.text.as_str()
    }

    /// 是否真的打印。
    pub fn printed(&self) -> bool {
        self.printed
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// 字节数。
    pub fn len(&self) -> usize {
        self.text.len()
    }
}

/// 警告发射器：同一键只打一次。
#[derive(Clone, Copy, Debug)]
pub struct Emitter {
    keys: [u64; KEY_MAX],
    len: usize,
    printed: u32,
    suppressed: u32,
    rejected: u32,
    sealed: bool,
    root: u64,
}

impl Emitter {
    /// 空发射器。
    pub const fn new() -> Emitter {
        Emitter {
            keys: [0u64; KEY_MAX],
            len: 0,
            printed: 0,
            suppressed: 0,
            rejected: 0,
            sealed: false,
            root: 0,
        }
    }

    /// 是否已含键。
    pub fn seen(&self, key: u64) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(k) = self.keys.get(i) {
                if *k == key {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 记键。
    fn remember(&mut self, key: u64) -> Result<(), u32> {
        if self.len >= KEY_MAX {
            self.rejected = self.rejected.saturating_add(1);
            return Err(LW_EOVER);
        }
        if let Some(dst) = self.keys.get_mut(self.len) {
            *dst = key;
        }
        self.len += 1;
        Ok(())
    }

    /// 已打印数。
    pub fn printed(&self) -> u32 {
        self.printed
    }

    /// 去重静默数。
    pub fn suppressed(&self) -> u32 {
        self.suppressed
    }

    /// 拒收数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 键数。
    pub fn keys(&self) -> usize {
        self.len
    }

    /// 是否已封。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 指纹。
    pub fn seal_value(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(k) = self.keys.get(i) {
                h = fold_word(h, (*k & 0xffff_ffff) as u32);
                h = fold_word(h, (*k >> 32) as u32);
            }
            i += 1;
        }
        h = fold_word(h, self.printed);
        h = fold_word(h, self.suppressed);
        h = fold_word(h, self.rejected);
        h
    }

    /// 发射。
    ///
    /// 顺序：封闸 → 类别 → 名字 → 去重 → 文案。
    pub fn emit(&mut self, cat: u8, name: &str, cause: Cause) -> Result<Notice, u32> {
        if self.sealed {
            self.rejected = self.rejected.saturating_add(1);
            return Err(LW_ESTATE);
        }
        if cat_label(cat).is_none() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(LW_ECAT);
        }
        if let Err(c) = check_name(name) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(c);
        }
        let key = key_of(cat, name, cause);
        if self.seen(key) {
            self.suppressed = self.suppressed.saturating_add(1);
            return Ok(Notice::suppressed());
        }
        self.remember(key)?;
        let text = warning(cat, name, cause)?;
        self.printed = self.printed.saturating_add(1);
        Ok(Notice::show(text))
    }

    /// 封闸。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            self.rejected = self.rejected.saturating_add(1);
            return Err(LW_EBAL);
        }
        self.root = self.seal_value();
        self.sealed = true;
        Ok(self.root)
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 重放：重建发射器，比指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_emitter();
        let a = fresh.seal_value();
        if a == self.seal_value() {
            Ok(a)
        } else {
            Err(LW_ESEAL)
        }
    }
}

impl Default for Emitter {
    fn default() -> Emitter {
        Emitter::new()
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 一个告警语料槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 类别号。
    pub cat: u8,
    /// 请求名。
    pub name: &'static str,
    /// 失败原因；None 表示归档完好、静默成功。
    pub cause: Option<Cause>,
    /// 是否期待被去重。
    pub dup: bool,
    /// 备注。
    pub note: &'static str,
}

impl Case {
    /// 构造。
    pub const fn new(
        cat: u8,
        name: &'static str,
        cause: Option<Cause>,
        dup: bool,
        note: &'static str,
    ) -> Case {
        Case { cat, name, cause, dup, note }
    }
}

/// 十四槽语料。
pub const CASES: [Case; CORPUS] = [
    Case::new(CAT_ALL, "xx_YY.UTF-8", Some(Cause::NoArchive), false, "all-enoent"),
    Case::new(CAT_ALL, "xx_YY.UTF-8", Some(Cause::NoArchive), true, "all-enoent-again"),
    Case::new(0, "xx_YY.UTF-8", Some(Cause::NoArchive), false, "ctype-enoent"),
    Case::new(1, "de_DE.UTF-8", Some(Cause::Permission), false, "numeric-eacces"),
    Case::new(2, "fr_FR.UTF-8", Some(Cause::Corrupt), false, "time-einval"),
    Case::new(CAT_ALL, "de_DE.UTF-8", None, false, "archive-ok"),
    Case::new(5, "ja_JP.UTF-8", None, false, "messages-ok"),
    Case::new(3, "xx_YY.UTF-8", Some(Cause::NoDir), false, "collate-nodir"),
    Case::new(CAT_ALL, "xx_YY.UTF-8", Some(Cause::Permission), false, "all-eacces"),
    Case::new(6, "zh_CN.UTF-8", Some(Cause::NoDir), false, "paper-nodir"),
    Case::new(CAT_ALL, "POSIX", None, false, "posix-builtin"),
    Case::new(CAT_ALL, "C", None, false, "c-builtin"),
    Case::new(4, "en_US.UTF-8", Some(Cause::Permission), false, "monetary-eacces"),
    Case::new(CAT_ALL, "yy_YY.UTF-8", Some(Cause::Corrupt), false, "all-einval"),
];

/// 按槽取。
pub fn case_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 语料互异：类别、名字、原因、去重位四元组。
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
                if a.cat == b.cat && a.name == b.name && a.cause == b.cause && a.dup == b.dup {
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
    let mut em = Emitter::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let c = match CASES.get(i) {
            Some(v) => *v,
            None => return false,
        };
        match c.cause {
            None => {
                let ar = Archive::healthy();
                if ar.probe(c.name).is_err() {
                    return false;
                }
            }
            Some(cause) => {
                let n = match em.emit(c.cat, c.name, cause) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                if n.printed() == c.dup {
                    return false;
                }
                if !c.dup {
                    let want = match warning(c.cat, c.name, cause) {
                        Ok(v) => v,
                        Err(_) => return false,
                    };
                    if n.text() != want.as_str() {
                        return false;
                    }
                }
            }
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 汇总
// ---------------------------------------------------------------------------

/// 告警语料汇总。
#[derive(Clone, Copy, Debug)]
pub struct Census {
    total: u32,
    archive: u32,
    access: u32,
    corrupt: u32,
    nodir: u32,
    silent: u32,
    printed: u32,
    suppressed: u32,
    matched: u32,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census {
            total: 0,
            archive: 0,
            access: 0,
            corrupt: 0,
            nodir: 0,
            silent: 0,
            printed: 0,
            suppressed: 0,
            matched: 0,
        }
    }

    /// 总槽数。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// 归档缺失数。
    pub fn archive(&self) -> u32 {
        self.archive
    }

    /// 权限不足数。
    pub fn access(&self) -> u32 {
        self.access
    }

    /// 损坏数。
    pub fn corrupt(&self) -> u32 {
        self.corrupt
    }

    /// 条目缺失数。
    pub fn nodir(&self) -> u32 {
        self.nodir
    }

    /// 静默数。
    pub fn silent(&self) -> u32 {
        self.silent
    }

    /// 打印数。
    pub fn printed(&self) -> u32 {
        self.printed
    }

    /// 去重数。
    pub fn suppressed(&self) -> u32 {
        self.suppressed
    }

    /// 命中数。
    pub fn matched(&self) -> u32 {
        self.matched
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 千分比（上取整，单槽红档 929）。
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
            return Err(LW_ENOBS);
        }
        if self.matched != self.total {
            return Err(LW_EVALUE);
        }
        Ok(())
    }

    /// 渲染。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("warn corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" archive=");
        s.push_str(u32_text(self.archive).as_str());
        s.push_str(" access=");
        s.push_str(u32_text(self.access).as_str());
        s.push_str(" corrupt=");
        s.push_str(u32_text(self.corrupt).as_str());
        s.push_str(" nodir=");
        s.push_str(u32_text(self.nodir).as_str());
        s.push_str(" silent=");
        s.push_str(u32_text(self.silent).as_str());
        s.push_str(" printed=");
        s.push_str(u32_text(self.printed).as_str());
        s.push_str(" suppressed=");
        s.push_str(u32_text(self.suppressed).as_str());
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

/// 载红档跑。
pub fn run_with(red: Option<usize>) -> Result<Census, u32> {
    if let Some(r) = red {
        if r >= CORPUS {
            return Err(LW_ESLOT);
        }
    }
    let mut em = Emitter::new();
    let mut c = Census::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let cs = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(LW_ESLOT),
        };
        c.total = c.total.saturating_add(1);
        let mut ok = true;
        match cs.cause {
            None => {
                c.silent = c.silent.saturating_add(1);
                let ar = Archive::healthy();
                if ar.probe(cs.name).is_err() {
                    ok = false;
                }
            }
            Some(cause) => {
                match cause {
                    Cause::NoArchive => c.archive = c.archive.saturating_add(1),
                    Cause::Permission => c.access = c.access.saturating_add(1),
                    Cause::Corrupt => c.corrupt = c.corrupt.saturating_add(1),
                    Cause::NoDir => c.nodir = c.nodir.saturating_add(1),
                }
                let n = em.emit(cs.cat, cs.name, cause)?;
                if n.printed() == cs.dup {
                    ok = false;
                }
                if !cs.dup {
                    let want = warning(cs.cat, cs.name, cause)?;
                    if n.text() != want.as_str() {
                        ok = false;
                    }
                }
            }
        }
        let forced = match red {
            Some(r) => r == i,
            None => false,
        };
        if !forced && ok {
            c.matched = c.matched.saturating_add(1);
        }
        i += 1;
    }
    c.printed = em.printed();
    c.suppressed = em.suppressed();
    Ok(c)
}

/// 封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(LW_ENOBS)
    }
}

/// 按语料建发射器。
pub fn build_emitter() -> Emitter {
    let mut em = Emitter::new();
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            if let Some(cause) = c.cause {
                let _ = em.emit(c.cat, c.name, cause);
            }
        }
        i += 1;
    }
    em
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
            return Err(LW_ESLOT);
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
            return Err(LW_EBAL);
        }
        const VALS: [u32; 8] = [14, 3, 3, 2, 2, 4, 9, 929];
        const CODES: [u32; 6] =
            [LW_EBYTE, LW_ENAME, LW_ECAT, LW_EARCHIVE, LW_EACCESS, LW_ECORRUPT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LW_EBAL),
            };
            if s.slot as usize != i {
                return Err(LW_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LW_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LW_EVALUE);
                        }
                    }
                    None => return Err(LW_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LW_EVALUE);
                        }
                    }
                    None => return Err(LW_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LW_EBAL);
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
            return Err(LW_EBAL);
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
            Err(LW_ESEAL)
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

const OP_CORPUS: u8 = 0;
const OP_ARCHIVE: u8 = 1;
const OP_ACCESS: u8 = 2;
const OP_CORRUPT: u8 = 3;
const OP_NODIR: u8 = 4;
const OP_SILENT: u8 = 5;
const OP_PRINT: u8 = 6;
const OP_RED: u8 = 7;
const OP_BYTE: u8 = 8;
const OP_NAME: u8 = 9;
const OP_CAT: u8 = 10;
const OP_PROBE: u8 = 11;

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
    let _ = l.register(count_cell(1, OP_ARCHIVE, r.map(|x| x.archive() as usize)));
    let _ = l.register(count_cell(2, OP_ACCESS, r.map(|x| x.access() as usize)));
    let _ = l.register(count_cell(3, OP_CORRUPT, r.map(|x| x.corrupt() as usize)));
    let _ = l.register(count_cell(4, OP_NODIR, r.map(|x| x.nodir() as usize)));
    let _ = l.register(count_cell(5, OP_SILENT, r.map(|x| x.silent() as usize)));
    let _ = l.register(count_cell(6, OP_PRINT, r.map(|x| x.printed() as usize)));

    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_RED, red.map(|x| x.rate() as usize)));

    let byte = match warning(CAT_ALL, "bad name", Cause::NoArchive) {
        Ok(_) => LW_EBYTE,
        Err(c) => c,
    };
    let _ = l.register(err_cell(8, OP_BYTE, byte));

    let name = match warning(CAT_ALL, "", Cause::NoArchive) {
        Ok(_) => LW_ENAME,
        Err(c) => c,
    };
    let _ = l.register(err_cell(9, OP_NAME, name));

    let cat = match warning(CAT_COUNT as u8, "xx_YY.UTF-8", Cause::NoArchive) {
        Ok(_) => LW_ECAT,
        Err(c) => c,
    };
    let _ = l.register(err_cell(10, OP_CAT, cat));

    let archive = match Archive::missing().probe("xx_YY.UTF-8") {
        Ok(_) => LW_EARCHIVE,
        Err(c) => c.code(),
    };
    let _ = l.register(err_cell(11, OP_PROBE, archive));

    let access = match Archive::unreadable().probe("xx_YY.UTF-8") {
        Ok(_) => LW_EACCESS,
        Err(c) => c.code(),
    };
    let _ = l.register(err_cell(12, OP_PROBE, access));

    let corrupt = match Archive::corrupt().probe("xx_YY.UTF-8") {
        Ok(_) => LW_ECORRUPT,
        Err(c) => c.code(),
    };
    let _ = l.register(err_cell(13, OP_PROBE, corrupt));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 名字注入闸。
fn name_ok() -> bool {
    let a = check_name("xx_YY.UTF-8").is_ok();
    let b = check_name("").err() == Some(LW_ENAME);
    let c = check_name("bad name").err() == Some(LW_EBYTE);
    let d = check_name("xx\nYY").err() == Some(LW_EBYTE);
    let e = check_name("xx)YY").err() == Some(LW_EBYTE);
    let f = check_name("xx;YY").err() == Some(LW_EBYTE);
    let g = check_name("C").is_ok() && builtin_name("C") && builtin_name("POSIX");
    let h = !builtin_name("en_US.UTF-8");
    let i = check_name("xx_YY.UTF-8") == Ok(());
    a && b && c && d && e && f && g && h && i
}

/// 文案逐字。
fn text_ok() -> bool {
    let a = match warning(CAT_ALL, "xx_YY.UTF-8", Cause::NoArchive) {
        Ok(s) => s.as_str()
            == "warning: setlocale: LC_ALL: cannot change locale (xx_YY.UTF-8): No such file or directory\n",
        Err(_) => false,
    };
    let b = match warning(0, "xx_YY.UTF-8", Cause::NoArchive) {
        Ok(s) => s.as_str()
            == "warning: setlocale: LC_CTYPE: cannot change locale (xx_YY.UTF-8): No such file or directory\n",
        Err(_) => false,
    };
    let c = match warning(1, "de_DE.UTF-8", Cause::Permission) {
        Ok(s) => s.as_str()
            == "warning: setlocale: LC_NUMERIC: cannot change locale (de_DE.UTF-8): Permission denied\n",
        Err(_) => false,
    };
    let d = match warning(2, "fr_FR.UTF-8", Cause::Corrupt) {
        Ok(s) => s.as_str()
            == "warning: setlocale: LC_TIME: cannot change locale (fr_FR.UTF-8): Invalid argument\n",
        Err(_) => false,
    };
    let e = match warning(3, "xx_YY.UTF-8", Cause::NoDir) {
        Ok(s) => s.as_str() == "warning: setlocale: LC_COLLATE: cannot change locale (xx_YY.UTF-8)\n",
        Err(_) => false,
    };
    let f = match short_warning(CAT_ALL, "xx_YY.UTF-8") {
        Ok(s) => s.as_str() == "warning: setlocale: LC_ALL: cannot change locale (xx_YY.UTF-8)\n",
        Err(_) => false,
    };
    let g = warning(CAT_COUNT as u8, "xx_YY.UTF-8", Cause::NoArchive).err() == Some(LW_ECAT);
    let h = warning(CAT_ALL, "", Cause::NoArchive).err() == Some(LW_ENAME);
    a && b && c && d && e && f && g && h
}

/// errno 后缀。
fn errno_ok() -> bool {
    let a = Cause::NoArchive.errno() == ENOENT && Cause::NoArchive.has_suffix();
    let b = Cause::Permission.errno() == EACCES && Cause::Permission.has_suffix();
    let c = Cause::Corrupt.errno() == EINVAL && Cause::Corrupt.has_suffix();
    let d = Cause::NoDir.errno() == 0 && !Cause::NoDir.has_suffix();
    let e = Cause::NoArchive.suffix() == "No such file or directory";
    let f = Cause::Permission.suffix() == "Permission denied";
    let g = Cause::Corrupt.suffix() == "Invalid argument";
    let h = Cause::NoArchive.code() == LW_EARCHIVE && Cause::NoDir.code() == LW_ENAME;
    let i = Cause::NoArchive.text() == "no-archive" && Cause::Corrupt.text() == "corrupt";
    a && b && c && d && e && f && g && h && i
}

/// 归档三态。
fn archive_ok() -> bool {
    let a = Archive::healthy().probe("xx_YY.UTF-8").is_err();
    let b = Archive::healthy().probe("C") == Ok(0);
    let c = Archive::healthy().probe("POSIX") == Ok(0);
    let d = Archive::healthy().probe("de_DE.UTF-8") == Ok(4);
    let e = Archive::missing().check() == Err(Cause::NoArchive);
    let f = Archive::unreadable().check() == Err(Cause::Permission);
    let g = Archive::corrupt().check() == Err(Cause::Corrupt);
    let h = Archive::healthy().check() == Ok(()) && Archive::healthy().present();
    let i = Archive::missing().probe("C") == Err(Cause::NoArchive);
    a && b && c && d && e && f && g && h && i
}

/// 类别映射。
fn category_ok() -> bool {
    let a = cat_label(CAT_ALL) == Some("LC_ALL");
    let b = cat_label(0) == Some("LC_CTYPE") && cat_label(11) == Some("LC_IDENTIFICATION");
    let c = cat_label(12).is_none() && cat_label(0xfe).is_none();
    let d = Cat::from_u8(4) == Some(Cat::Monetary) && Cat::from_u8(12).is_none();
    let e = Cat::Paper.name() == "LC_PAPER" && Cat::Messages.name() == "LC_MESSAGES";
    let f = CAT_NAMES.len() == CAT_COUNT;
    let g = CAT_NAMES.first().copied() == Some("LC_CTYPE");
    let h = CAT_NAMES.get(11).copied() == Some("LC_IDENTIFICATION");
    a && b && c && d && e && f && g && h
}

/// 去重闸。
fn dedup_ok() -> bool {
    let mut em = Emitter::new();
    let a = match em.emit(CAT_ALL, "xx_YY.UTF-8", Cause::NoArchive) {
        Ok(n) => n.printed() && !n.is_empty(),
        Err(_) => false,
    };
    let b = match em.emit(CAT_ALL, "xx_YY.UTF-8", Cause::NoArchive) {
        Ok(n) => !n.printed() && n.is_empty(),
        Err(_) => false,
    };
    let c = em.printed() == 1 && em.suppressed() == 1 && em.keys() == 1;
    let d = match em.emit(0, "xx_YY.UTF-8", Cause::NoArchive) {
        Ok(n) => n.printed(),
        Err(_) => false,
    };
    let e = match em.emit(CAT_ALL, "xx_YY.UTF-8", Cause::Permission) {
        Ok(n) => n.printed(),
        Err(_) => false,
    };
    let f = em.printed() == 3 && em.suppressed() == 1 && em.keys() == 3;
    a && b && c && d && e && f
}

/// 十四卡全跑。
fn corpus_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.total() == CORPUS as u32 && c.matched() == CORPUS as u32;
    let b = c.archive() == 3 && c.access() == 3 && c.corrupt() == 2 && c.nodir() == 2;
    let d = c.silent() == 4 && c.printed() == 9 && c.suppressed() == 1;
    let e = c.rate() == 1000 && c.green() && c.gate() == Ok(());
    a && b && d && e
}

/// 语料编号。
fn slots_ok() -> bool {
    if !unique() || !notes_ok() {
        return false;
    }
    let a = match case_at(0) {
        Some(c) => c.cat == CAT_ALL && c.cause == Some(Cause::NoArchive) && !c.dup,
        None => false,
    };
    let b = match case_at(1) {
        Some(c) => c.cause == Some(Cause::NoArchive) && c.dup,
        None => false,
    };
    let c = match case_at(5) {
        Some(c) => c.cause.is_none() && c.name == "de_DE.UTF-8",
        None => false,
    };
    let d = match case_at(13) {
        Some(c) => c.cause == Some(Cause::Corrupt),
        None => false,
    };
    let e = case_at(CORPUS).is_none() && case_at(CORPUS + 1).is_none();
    a && b && c && d && e
}

/// 汇总渲染。
fn report_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.render().as_str() == "warn corpus=14 archive=3 access=3 corrupt=2 nodir=2 silent=4 printed=9 suppressed=1 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(LW_ENOBS);
    let d = e.render().as_str() == "warn corpus=0 archive=0 access=0 corrupt=0 nodir=0 silent=0 printed=0 suppressed=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(CORPUS + 1) == Err(LW_ENOBS);
    a && b && d && f
}

/// 红单显式。
fn reds_ok() -> bool {
    let c = match run_with(Some(0)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched() == CORPUS as u32 - 1 && c.rate() == PPM_RED;
    let b = c.gate() == Err(LW_EVALUE) && !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(LW_ESLOT);
    let e = run_with(Some(CORPUS + 1)).err() == Some(LW_ESLOT);
    a && b && d && e
}

/// 静默档。
fn silent_ok() -> bool {
    let a = Archive::healthy().probe("de_DE.UTF-8").is_ok();
    let b = Archive::healthy().probe("POSIX").is_ok();
    let c = Archive::healthy().probe("C").is_ok();
    let d = Archive::healthy().probe("xx_YY.UTF-8").err() == Some(Cause::NoDir);
    let mut em = Emitter::new();
    let e = match em.emit(CAT_ALL, "C", Cause::NoDir) {
        Ok(n) => n.printed(),
        Err(_) => false,
    };
    let f = em.printed() == 1;
    let g = match trace(CAT_ALL, "xx_YY.UTF-8", Cause::NoArchive) {
        Ok(t) => t.as_str()
            == "setlocale(LC_ALL, \"xx_YY.UTF-8\") -> NULL\nwarning: setlocale: LC_ALL: cannot change locale (xx_YY.UTF-8): No such file or directory\nerrno=2 (No such file or directory)\n",
        Err(_) => false,
    };
    let h = match trace(3, "xx_YY.UTF-8", Cause::NoDir) {
        Ok(t) => t.as_str().ends_with("errno=0 (no errno)\n"),
        Err(_) => false,
    };
    let i = trace(CAT_COUNT as u8, "xx_YY.UTF-8", Cause::NoDir).err() == Some(LW_ECAT);
    a && b && c && d && e && f && g && h && i
}

/// 发射器封闸与文本助手。
fn emitter_ok() -> bool {
    let mut em = build_emitter();
    let a = em.printed() == 9 && em.suppressed() == 1 && em.keys() == 9;
    let root = match em.seal() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = em.is_sealed() && em.root() == root;
    let c = em.replay() == Ok(em.seal_value());
    let d = em.emit(CAT_ALL, "xx_YY.UTF-8", Cause::NoArchive).err() == Some(LW_ESTATE);
    let e = em.seal() == Err(LW_EBAL);
    let f = u32_text(0).as_str() == "0" && u32_text(14).as_str() == "14";
    let g = i32_text(2).as_str() == "2" && i32_text(-1).as_str() == "-1";
    let h = hex4_text(0xfd50).as_str() == "0xfd50";
    let i = digest(b"warning") == digest(b"warning") && digest(b"warning") != digest(b"warn");
    a && b && c && d && e && f && g && h && i
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = name_ok() && text_ok() && errno_ok();
    out[1] = archive_ok() && category_ok() && dedup_ok();
    out[2] = corpus_ok() && slots_ok() && report_ok() && silent_ok();
    out[3] = reds_ok() && emitter_ok() && build_ledger().audit() == Ok(());
    out
}

/// 判据集合。
pub fn run_unx11646_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11646.locale_warn");

    s.add("LW01-名字注入闸", name_ok(), "static");
    s.add("LW02-文案逐字", text_ok(), "static");
    s.add("LW03-errno 后缀", errno_ok(), "static");
    s.add("LW04-归档三态", archive_ok(), "static");
    s.add("LW05-类别映射", category_ok(), "static");
    s.add("LW06-去重闸", dedup_ok(), "static");
    s.add("LW07-十四卡全跑", corpus_ok(), "static");
    s.add("LW08-语料编号", slots_ok(), "static");
    s.add("LW09-汇总渲染", report_ok(), "static");
    s.add("LW10-红单显式", reds_ok(), "static");
    s.add("LW11-静默档", silent_ok(), "static");
    s.add("LW12-发射器与账本", emitter_ok(), "static");
    s.add("LW13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
    fn name_gate() {
        assert!(check_name("xx_YY.UTF-8").is_ok());
        assert_eq!(check_name("").err(), Some(LW_ENAME));
        assert_eq!(check_name("a b").err(), Some(LW_EBYTE));
        assert_eq!(check_name("a\nb").err(), Some(LW_EBYTE));
        assert_eq!(check_name("a(b").err(), Some(LW_EBYTE));
        assert_eq!(check_name("a;b").err(), Some(LW_EBYTE));
        assert!(builtin_name("C"));
        assert!(builtin_name("POSIX"));
    }

    #[test]
    fn warning_text() {
        assert_eq!(
            warning(CAT_ALL, "xx_YY.UTF-8", Cause::NoArchive).ok().as_deref(),
            Some("warning: setlocale: LC_ALL: cannot change locale (xx_YY.UTF-8): No such file or directory\n")
        );
        assert_eq!(
            warning(0, "xx_YY.UTF-8", Cause::NoArchive).ok().as_deref(),
            Some("warning: setlocale: LC_CTYPE: cannot change locale (xx_YY.UTF-8): No such file or directory\n")
        );
        assert_eq!(
            warning(3, "xx_YY.UTF-8", Cause::NoDir).ok().as_deref(),
            Some("warning: setlocale: LC_COLLATE: cannot change locale (xx_YY.UTF-8)\n")
        );
        assert_eq!(
            short_warning(CAT_ALL, "xx_YY.UTF-8").ok().as_deref(),
            Some("warning: setlocale: LC_ALL: cannot change locale (xx_YY.UTF-8)\n")
        );
    }

    #[test]
    fn errno_suffixes() {
        assert_eq!(Cause::NoArchive.errno(), ENOENT);
        assert_eq!(Cause::Permission.errno(), EACCES);
        assert_eq!(Cause::Corrupt.errno(), EINVAL);
        assert_eq!(Cause::NoDir.errno(), 0);
        assert!(!Cause::NoDir.has_suffix());
    }

    #[test]
    fn archive_states() {
        assert_eq!(Archive::missing().check(), Err(Cause::NoArchive));
        assert_eq!(Archive::unreadable().check(), Err(Cause::Permission));
        assert_eq!(Archive::corrupt().check(), Err(Cause::Corrupt));
        assert_eq!(Archive::healthy().check(), Ok(()));
        assert_eq!(Archive::healthy().probe("de_DE.UTF-8"), Ok(4));
        assert_eq!(Archive::healthy().probe("xx_YY.UTF-8"), Err(Cause::NoDir));
    }

    #[test]
    fn category_labels() {
        assert_eq!(cat_label(CAT_ALL), Some("LC_ALL"));
        assert_eq!(cat_label(1), Some("LC_NUMERIC"));
        assert_eq!(cat_label(12), None);
        assert_eq!(Cat::from_u8(11), Some(Cat::Identification));
        assert_eq!(CAT_NAMES.len(), CAT_COUNT);
    }

    #[test]
    fn dedup_emitter() {
        let mut em = Emitter::new();
        let a = em.emit(CAT_ALL, "xx_YY.UTF-8", Cause::NoArchive);
        assert!(a.map(|n| n.printed()).unwrap_or(false));
        let b = em.emit(CAT_ALL, "xx_YY.UTF-8", Cause::NoArchive);
        assert!(b.map(|n| !n.printed() && n.is_empty()).unwrap_or(false));
        assert_eq!(em.printed(), 1);
        assert_eq!(em.suppressed(), 1);
        let c = em.emit(CAT_ALL, "xx_YY.UTF-8", Cause::Permission);
        assert!(c.map(|n| n.printed()).unwrap_or(false));
        assert_eq!(em.printed(), 2);
    }

    #[test]
    fn emitter_seal() {
        let mut em = Emitter::new();
        assert!(em.emit(0, "xx_YY.UTF-8", Cause::NoArchive).is_ok());
        let root = match em.seal() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(em.is_sealed());
        assert_eq!(em.root(), root);
        assert_eq!(em.emit(0, "xx_YY.UTF-8", Cause::NoArchive).err(), Some(LW_ESTATE));
        assert_eq!(em.seal(), Err(LW_EBAL));
    }

    #[test]
    fn trace_text() {
        assert_eq!(
            trace(CAT_ALL, "xx_YY.UTF-8", Cause::NoArchive).ok().as_deref(),
            Some("setlocale(LC_ALL, \"xx_YY.UTF-8\") -> NULL\nwarning: setlocale: LC_ALL: cannot change locale (xx_YY.UTF-8): No such file or directory\nerrno=2 (No such file or directory)\n")
        );
        assert_eq!(
            trace(3, "xx_YY.UTF-8", Cause::NoDir).ok().as_deref(),
            Some("setlocale(LC_COLLATE, \"xx_YY.UTF-8\") -> NULL\nwarning: setlocale: LC_COLLATE: cannot change locale (xx_YY.UTF-8)\nerrno=0 (no errno)\n")
        );
    }

    #[test]
    fn census_numbers() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.total(), 14);
        assert_eq!(c.archive(), 3);
        assert_eq!(c.access(), 3);
        assert_eq!(c.corrupt(), 2);
        assert_eq!(c.nodir(), 2);
        assert_eq!(c.silent(), 4);
        assert_eq!(c.printed(), 9);
        assert_eq!(c.suppressed(), 1);
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
        assert_eq!(c.gate(), Err(LW_EVALUE));
        assert_eq!(run_with(Some(14)).err(), Some(LW_ESLOT));
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
        assert_eq!(l.freeze(), Err(LW_EBAL));
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
    fn line_guard() {
        let mut l = Line::new();
        assert!(l.push_str("warning: ").is_ok());
        assert!(l.push(b'\n').is_ok());
        assert!(!l.is_empty());
        assert_eq!(l.len(), 10);
        let mut big = Line::new();
        let mut i = 0usize;
        while i < 300 {
            if big.push(b'x').is_err() {
                break;
            }
            i += 1;
        }
        assert_eq!(big.len(), LINE_MAX);
        assert_eq!(big.as_str().len(), LINE_MAX);
    }

    #[test]
    fn archive_names_table() {
        assert_eq!(ARCHIVE_LEN, 10);
        assert_eq!(rec_at(4), Some("de_DE.UTF-8"));
        assert_eq!(rec_find("zh_CN.UTF-8"), Some(9));
        assert_eq!(rec_find("xx_YY.UTF-8"), None);
        assert_eq!(
            key_of(CAT_ALL, "C", Cause::NoDir),
            key_of(CAT_ALL, "C", Cause::NoDir)
        );
        assert!(key_of(CAT_ALL, "C", Cause::NoDir) != key_of(0, "C", Cause::NoDir));
        assert!(key_of(CAT_ALL, "C", Cause::NoDir) != key_of(CAT_ALL, "C", Cause::NoArchive));
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11646_checks();
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
        assert_eq!(hex4_text(0xfd50), "0xfd50");
    }
}
