//! UNX-F11577 错误消息表 locale=C 英文固定判据
//!
//! `strerror(3)` 与 `catgets(3)` 在 C locale 下返回的是一张**写死的英文表**：
//! 内核不分词、不翻译、不按调用者语言拼装，错误号与消息之间是常量映射。
//! 本模块把这张表完整搬进内核，并给出三条锚定判据：
//!
//! * 序号锚：[`strerror`] 只接受 0..=34（0 为 "Success"），35 以上与负数一律
//!   `MC_ESTR`；消息首字节必须是 ASCII 大写字母，尾部不得有空白。
//! * 文本锚：`2` 必须是 "No such file or directory"，`13` 必须是
//!   "Permission denied"，`34` 必须是 "Numerical result out of range"；
//!   这些串逐字节比较，任何本地化替换都会改变长度并让指纹漂移。
//! * 目录锚：[`Catalog`] 是 catgets 的集合/消息两级目录（20 条、3 个集合），
//!   只有 C / POSIX / C.UTF-8 能打开；de_DE、fr_FR、ja_JP 的样本串仍然存在
//!   于[`foreign_sample`] 中，但它们过不了 [`audit_bytes`]（非 ASCII 字节），
//!   以此来证明「外语表存在 ≠ 外语表被采用」。
//!
//! 目录查询带状态机：未打开即查 → `MC_ESTATE`；重复打开/重复关闭 → 同一个
//! 错误码；集合 0 与消息 0 是保留号，分别落 `MC_ESET` / `MC_EMSG`。未命中的
//! 键**不是错误**：catgets 语义要求回退到调用者给的缺省串，本模块用
//! [`Catalog::get_or`] 表达这一层，并把命中/未命中次数记在账上。
//!
//! 模板展开是另一条独立路径：[`expand`] 只认 `%s`、`%d`、`%%`，按出现次序
//! 消费实参，多给少给都落 `MC_EARG`，未知转换落 `MC_EEXPAND`。它不做宽度、
//! 精度、标志位——那是 printf 模块的事，这里只验证消息串的占位符数量与类型。
//!
//! 闸门次序：错误号 → 英文表 → 目录键 → 状态机 → 本地化拒绝 → 模板 → 审计。
//!
//! 全模块无 std、无 unsafe、无 panic：表是 `&'static str` 常量，缓冲只用
//! `String` 的追加接口，索引一律 `get()`，失败全部落 13 个错误码。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf940;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 目录消息条数上界。
pub const MSG_CAP: usize = 24;

/// 用户界面集合。
pub const SET_UI: u32 = 1;
/// 输入输出集合。
pub const SET_IO: u32 = 2;
/// 网络集合。
pub const SET_NET: u32 = 3;

/// 未命中时 catgets 风格的缺省串。
pub const FALLBACK: &str = "unknown message";

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 错误号越界或消息不存在。
pub const MC_ESTR: u32 = SEG_BASE + 0x01;
/// 集合号为 0（保留）。
pub const MC_ESET: u32 = SEG_BASE + 0x02;
/// 消息号为 0（保留）。
pub const MC_EMSG: u32 = SEG_BASE + 0x03;
/// 非 C locale，目录拒绝打开。
pub const MC_ELOCALE: u32 = SEG_BASE + 0x04;
/// 目录状态冲突（未开、重复开、重复关）。
pub const MC_ESTATE: u32 = SEG_BASE + 0x05;
/// 非 ASCII 或控制字节。
pub const MC_EBYTE: u32 = SEG_BASE + 0x06;
/// 模板语法错误。
pub const MC_EEXPAND: u32 = SEG_BASE + 0x07;
/// 实参数量或类型不符。
pub const MC_EARG: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const MC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const MC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const MC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const MC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const MC_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 取错误消息。
pub const OP_STRERROR: u8 = 0x01;
/// 目录查询。
pub const OP_CATGET: u8 = 0x02;
/// 模板展开。
pub const OP_EXPAND: u8 = 0x03;
/// ASCII 审计。
pub const OP_AUDIT: u8 = 0x04;
/// locale 判定。
pub const OP_LOCALE: u8 = 0x05;
/// 错误分类。
pub const OP_CLASS: u8 = 0x06;
/// 目录状态。
pub const OP_STATE: u8 = 0x07;

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

/// 把无符号数按十进制追加。
fn push_u32(out: &mut String, mut v: u32) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 10];
    let mut n = 0usize;
    while v > 0 && n < 10 {
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

// ---------------------------------------------------------------------------
// locale
// ---------------------------------------------------------------------------

/// 受支持的 locale 名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    /// `C`。
    C,
    /// `POSIX`。
    Posix,
    /// `C.UTF-8`（仍是 C 的消息面）。
    CUtf8,
    /// `de_DE.UTF-8`。
    De,
    /// `fr_FR.UTF-8`。
    Fr,
    /// `ja_JP.UTF-8`。
    Ja,
}

impl Locale {
    /// 由名字解析。
    pub fn parse(name: &[u8]) -> Result<Locale, u32> {
        if eq_bytes(name, b"C") {
            Ok(Locale::C)
        } else if eq_bytes(name, b"POSIX") {
            Ok(Locale::Posix)
        } else if eq_bytes(name, b"C.UTF-8") || eq_bytes(name, b"C.utf8") {
            Ok(Locale::CUtf8)
        } else if eq_bytes(name, b"de_DE.UTF-8") || eq_bytes(name, b"de_DE") {
            Ok(Locale::De)
        } else if eq_bytes(name, b"fr_FR.UTF-8") || eq_bytes(name, b"fr_FR") {
            Ok(Locale::Fr)
        } else if eq_bytes(name, b"ja_JP.UTF-8") || eq_bytes(name, b"ja_JP") {
            Ok(Locale::Ja)
        } else {
            Err(MC_ELOCALE)
        }
    }

    /// 名字。
    pub fn name(&self) -> &'static str {
        match self {
            Locale::C => "C",
            Locale::Posix => "POSIX",
            Locale::CUtf8 => "C.UTF-8",
            Locale::De => "de_DE.UTF-8",
            Locale::Fr => "fr_FR.UTF-8",
            Locale::Ja => "ja_JP.UTF-8",
        }
    }

    /// 是否属于 C 家族。
    pub fn is_c(&self) -> bool {
        match self {
            Locale::C | Locale::Posix | Locale::CUtf8 => true,
            _ => false,
        }
    }
}

/// 外语样本串：存在，但不会被 C 目录采用。
pub fn foreign_sample(loc: Locale, n: u32) -> Option<&'static str> {
    const DE: [&str; 3] = [
        "Datei nicht gefunden",
        "Zugriff verweigert – Pfad prüfen",
        "Unbekannter Fehler",
    ];
    const FR: [&str; 3] = [
        "Fichier introuvable",
        "Accès refusé",
        "Erreur inconnue",
    ];
    const JA: [&str; 3] = [
        "ファイルが見つかりません",
        "アクセスが拒否されました",
        "不明なエラー",
    ];
    let i = n as usize;
    match loc {
        Locale::De => DE.get(i).copied(),
        Locale::Fr => FR.get(i).copied(),
        Locale::Ja => JA.get(i).copied(),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// strerror 表
// ---------------------------------------------------------------------------

/// C locale 的 35 条消息：0 为 Success，1..=34 与 Linux 错误号一致。
pub const STRERRORS: [&str; 35] = [
    "Success",
    "Operation not permitted",
    "No such file or directory",
    "No such process",
    "Interrupted system call",
    "Input/output error",
    "No such device or address",
    "Argument list too long",
    "Exec format error",
    "Bad file descriptor",
    "No child processes",
    "Resource temporarily unavailable",
    "Cannot allocate memory",
    "Permission denied",
    "Bad address",
    "Block device required",
    "Device or resource busy",
    "File exists",
    "Invalid cross-device link",
    "No such device",
    "Not a directory",
    "Is a directory",
    "Invalid argument",
    "Too many open files in system",
    "Too many open files",
    "Inappropriate ioctl for device",
    "Text file busy",
    "File too large",
    "No space left on device",
    "Illegal seek",
    "Read-only file system",
    "Too many links",
    "Broken pipe",
    "Numerical argument out of domain",
    "Numerical result out of range",
];

/// 表长（含 0 号）。
pub fn table_len() -> usize {
    STRERRORS.len()
}

/// 取错误消息：仅 0..=34。
pub fn strerror(num: u32) -> Result<&'static str, u32> {
    let i = num as usize;
    match STRERRORS.get(i) {
        Some(s) => Ok(*s),
        None => Err(MC_ESTR),
    }
}

/// 首字节是否 ASCII 大写。
fn starts_upper(s: &str) -> bool {
    match s.as_bytes().get(0) {
        Some(b) => *b >= b'A' && *b <= b'Z',
        None => false,
    }
}

/// 尾部是否无空白。
fn no_trailing_space(s: &str) -> bool {
    match s.as_bytes().last() {
        Some(b) => *b != b' ' && *b != b'\t' && *b != b'\n',
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 消息目录
// ---------------------------------------------------------------------------

/// 一条目录消息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Msg {
    /// 集合号（非 0）。
    pub set: u32,
    /// 消息号（非 0）。
    pub msg: u32,
    /// 英文正文。
    pub text: &'static str,
    /// 用途标签。
    pub note: &'static str,
}

/// 目录正文：3 个集合、20 条消息。
pub const CATALOG: [Msg; 20] = [
    Msg { set: SET_UI, msg: 1, text: "usage: %s [-v] file", note: "usage" },
    Msg { set: SET_UI, msg: 2, text: "try '%s --help' for more information", note: "hint" },
    Msg { set: SET_UI, msg: 3, text: "unknown option: %s", note: "option" },
    Msg { set: SET_UI, msg: 4, text: "missing operand after %s", note: "operand" },
    Msg { set: SET_UI, msg: 5, text: "%s: not a regular file", note: "type" },
    Msg { set: SET_UI, msg: 6, text: "cannot open %s", note: "open" },
    Msg { set: SET_IO, msg: 1, text: "%s: %s", note: "path" },
    Msg { set: SET_IO, msg: 2, text: "read error on %s", note: "read" },
    Msg { set: SET_IO, msg: 3, text: "write error on %s", note: "write" },
    Msg { set: SET_IO, msg: 4, text: "cannot remove %s: %s", note: "remove" },
    Msg { set: SET_IO, msg: 5, text: "%s: disk quota exceeded", note: "quota" },
    Msg { set: SET_IO, msg: 6, text: "short read: expected %d bytes", note: "short" },
    Msg { set: SET_IO, msg: 7, text: "checksum mismatch at block %d", note: "checksum" },
    Msg { set: SET_NET, msg: 1, text: "cannot resolve host %s", note: "resolve" },
    Msg { set: SET_NET, msg: 2, text: "connection refused by %s", note: "refused" },
    Msg { set: SET_NET, msg: 3, text: "timeout after %d seconds", note: "timeout" },
    Msg { set: SET_NET, msg: 4, text: "%s: network unreachable", note: "unreach" },
    Msg { set: SET_NET, msg: 5, text: "protocol error: %d", note: "proto" },
    Msg { set: SET_NET, msg: 6, text: "%s: certificate expired", note: "cert" },
    Msg { set: SET_NET, msg: 7, text: "too many redirects from %s", note: "redirect" },
];

/// 目录条数。
pub fn catalog_len() -> usize {
    CATALOG.len()
}

/// 按集合+消息取正文。
pub fn catalog_text(set: u32, msg: u32) -> Option<&'static str> {
    let mut i = 0usize;
    while i < CATALOG.len() {
        if let Some(m) = CATALOG.get(i) {
            if m.set == set && m.msg == msg {
                return Some(m.text);
            }
        }
        i += 1;
    }
    None
}

/// 集合内消息条数。
pub fn count_set(set: u32) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < CATALOG.len() {
        if let Some(m) = CATALOG.get(i) {
            if m.set == set {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

/// 目录是否有重复键。
pub fn has_duplicate_key() -> bool {
    let mut i = 0usize;
    while i < CATALOG.len() {
        let a = match CATALOG.get(i) {
            Some(v) => v,
            None => return true,
        };
        let mut j = i + 1;
        while j < CATALOG.len() {
            if let Some(b) = CATALOG.get(j) {
                if a.set == b.set && a.msg == b.msg {
                    return true;
                }
            }
            j += 1;
        }
        i += 1;
    }
    false
}

/// 目录句柄：catgets 风格的集合/消息查找与状态。
pub struct Catalog {
    open: bool,
    default_set: u32,
    hits: u32,
    misses: u32,
}

impl Catalog {
    /// 关闭态、缺省集合为 1。
    pub const fn new() -> Catalog {
        Catalog { open: false, default_set: SET_UI, hits: 0, misses: 0 }
    }

    /// 打开：只接受 C 家族。
    pub fn open(&mut self, loc: Locale) -> Result<(), u32> {
        if self.open {
            return Err(MC_ESTATE);
        }
        if !loc.is_c() {
            return Err(MC_ELOCALE);
        }
        self.open = true;
        Ok(())
    }

    /// 关闭。
    pub fn close(&mut self) -> Result<(), u32> {
        if !self.open {
            return Err(MC_ESTATE);
        }
        self.open = false;
        Ok(())
    }

    /// 是否打开。
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// 缺省集合。
    pub fn default_set(&self) -> u32 {
        self.default_set
    }

    /// 改缺省集合。
    pub fn set_default(&mut self, set: u32) -> Result<(), u32> {
        if !self.open {
            return Err(MC_ESTATE);
        }
        if set == 0 {
            return Err(MC_ESET);
        }
        self.default_set = set;
        Ok(())
    }

    /// 严格查询：未命中回退到 `FALLBACK`，计数未命中。
    pub fn get(&mut self, set: u32, msg: u32) -> Result<&'static str, u32> {
        self.get_or(set, msg, FALLBACK)
    }

    /// 查询，未命中回退到调用者缺省串。
    pub fn get_or(
        &mut self,
        set: u32,
        msg: u32,
        fallback: &'static str,
    ) -> Result<&'static str, u32> {
        if !self.open {
            return Err(MC_ESTATE);
        }
        if set == 0 {
            return Err(MC_ESET);
        }
        if msg == 0 {
            return Err(MC_EMSG);
        }
        match catalog_text(set, msg) {
            Some(t) => {
                self.hits = self.hits.saturating_add(1);
                Ok(t)
            }
            None => {
                self.misses = self.misses.saturating_add(1);
                Ok(fallback)
            }
        }
    }

    /// 按缺省集合查询。
    pub fn get_default(&mut self, msg: u32) -> Result<&'static str, u32> {
        let set = self.default_set;
        self.get(set, msg)
    }

    /// 命中次数。
    pub fn hits(&self) -> u32 {
        self.hits
    }

    /// 未命中次数。
    pub fn misses(&self) -> u32 {
        self.misses
    }
}

// ---------------------------------------------------------------------------
// 模板展开
// ---------------------------------------------------------------------------

/// 展开实参。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// 字符串。
    S(&'static str),
    /// 无符号整数。
    I(u32),
}

/// 展开消息模板：`%s`、`%d`、`%%`。
pub fn expand(tpl: &str, args: &[Arg]) -> Result<String, u32> {
    let bytes = tpl.as_bytes();
    let mut out = String::new();
    let mut ai = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(MC_EEXPAND),
        };
        if b != b'%' {
            out.push(b as char);
            i += 1;
            continue;
        }
        i += 1;
        let c = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(MC_EEXPAND),
        };
        if c == b'%' {
            out.push('%');
            i += 1;
            continue;
        }
        if c != b's' && c != b'd' {
            return Err(MC_EEXPAND);
        }
        let a = match args.get(ai) {
            Some(v) => *v,
            None => return Err(MC_EARG),
        };
        match (c, a) {
            (b's', Arg::S(s)) => out.push_str(s),
            (b'd', Arg::I(n)) => push_u32(&mut out, n),
            _ => return Err(MC_EARG),
        }
        ai += 1;
        i += 1;
    }
    if ai != args.len() {
        return Err(MC_EARG);
    }
    Ok(out)
}

/// 统计模板占位符个数（不做类型绑定）。
pub fn count_slots(tpl: &str) -> Result<usize, u32> {
    let bytes = tpl.as_bytes();
    let mut n = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(MC_EEXPAND),
        };
        if b != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        let c = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(MC_EEXPAND),
        };
        if c == b'%' {
            i += 1;
        } else if c == b's' || c == b'd' {
            n += 1;
            i += 1;
        } else {
            return Err(MC_EEXPAND);
        }
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// ASCII 审计
// ---------------------------------------------------------------------------

/// 审计一段字节：只允许可打印 ASCII 与 `\n` `\t`。
pub fn audit_bytes(text: &[u8]) -> Result<usize, u32> {
    let mut i = 0usize;
    while i < text.len() {
        let b = match text.get(i) {
            Some(v) => *v,
            None => return Err(MC_EBYTE),
        };
        if b == b'\n' || b == b'\t' {
            i += 1;
            continue;
        }
        if b < 0x20 || b >= 0x7f {
            return Err(MC_EBYTE);
        }
        i += 1;
    }
    Ok(text.len())
}

/// 审计一个串。
pub fn audit_str(s: &str) -> Result<usize, u32> {
    audit_bytes(s.as_bytes())
}

/// 审计整张英文表与目录，返回被审计的消息条数。
pub fn audit_table() -> Result<u32, u32> {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < STRERRORS.len() {
        match STRERRORS.get(i) {
            Some(s) => {
                audit_str(s)?;
                n += 1;
            }
            None => return Err(MC_EBYTE),
        }
        i += 1;
    }
    i = 0;
    while i < CATALOG.len() {
        match CATALOG.get(i) {
            Some(m) => {
                audit_str(m.text)?;
                n += 1;
            }
            None => return Err(MC_EBYTE),
        }
        i += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// 分类
// ---------------------------------------------------------------------------

/// 错误号的大类：1 参数/寻址，2 权限/冲突，3 资源/容量，4 设备/IO，
/// 5 数值，6 进程/信号；未知为 0。
pub fn classify(n: u32) -> u32 {
    match n {
        2 | 7 | 8 | 9 | 14 | 18 | 20 | 21 | 22 | 29 => 1,
        1 | 13 | 17 => 2,
        11 | 12 | 23 | 24 | 27 | 28 | 31 | 122 => 3,
        5 | 6 | 15 | 16 | 19 | 25 | 26 | 30 | 32 => 4,
        33 | 34 => 5,
        3 | 4 | 10 => 6,
        _ => 0,
    }
}

/// 1..=34 是否全部有分类。
pub fn all_classified() -> bool {
    let mut n = 1u32;
    while n <= 34 {
        if classify(n) == 0 {
            return false;
        }
        n += 1;
    }
    true
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
            return Err(MC_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        } else {
            return Err(MC_ESLOT);
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
            return Err(MC_EBAL);
        }
        const VALS: [u32; 8] = [1, 34, 0, 3, 1, 1, 2, 55];
        const CODES: [u32; 6] = [
            MC_ESTR,
            MC_EMSG,
            MC_ESET,
            MC_ELOCALE,
            MC_ESTATE,
            MC_EEXPAND,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MC_EBAL),
            };
            if s.slot as usize != i {
                return Err(MC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(MC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(MC_EVALUE);
                        }
                    }
                    None => return Err(MC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(MC_EVALUE);
                        }
                    }
                    None => return Err(MC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MC_EBAL);
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
            return Err(MC_EBAL);
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
            Err(MC_ESEAL)
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

/// 槽 0：1 号消息。
fn slot0() -> Result<u32, u32> {
    strerror(1).map(|_| 1)
}

/// 槽 1：34 号消息。
fn slot1() -> Result<u32, u32> {
    strerror(34).map(|_| 34)
}

/// 槽 2：0 号消息（Success）。
fn slot2() -> Result<u32, u32> {
    strerror(0).map(|_| 0)
}

/// 槽 3：IO 集合 3 号查询。
fn slot3() -> Result<u32, u32> {
    let mut c = Catalog::new();
    c.open(Locale::C)?;
    c.get(SET_IO, 3).map(|_| 3)
}

/// 槽 4：UI 集合 1 号查询。
fn slot4() -> Result<u32, u32> {
    let mut c = Catalog::new();
    c.open(Locale::Posix)?;
    c.get(SET_UI, 1).map(|_| 1)
}

/// 槽 5：占位符计数。
fn slot5() -> Result<u32, u32> {
    let tpl = match catalog_text(SET_UI, 1) {
        Some(v) => v,
        None => return Err(MC_ENOBS),
    };
    let args = [Arg::S("tool")];
    let _ = expand(tpl, &args)?;
    match count_slots(tpl) {
        Ok(n) => Ok(n as u32),
        Err(c) => Err(c),
    }
}

/// 槽 6：权限错误的分类。
fn slot6() -> Result<u32, u32> {
    Ok(classify(13))
}

/// 槽 7：整表 ASCII 审计条数。
fn slot7() -> Result<u32, u32> {
    audit_table()
}

/// 槽 8：越界错误号。
fn slot8() -> Result<u32, u32> {
    strerror(999).map(|_| 0)
}

/// 槽 9：消息号为 0。
fn slot9() -> Result<u32, u32> {
    let mut c = Catalog::new();
    c.open(Locale::C)?;
    c.get(SET_UI, 0).map(|_| 0)
}

/// 槽 10：集合号为 0。
fn slot10() -> Result<u32, u32> {
    let mut c = Catalog::new();
    c.open(Locale::C)?;
    c.get(0, 1).map(|_| 0)
}

/// 槽 11：德语目录被拒。
fn slot11() -> Result<u32, u32> {
    let mut c = Catalog::new();
    c.open(Locale::De).map(|_| 0)
}

/// 槽 12：关闭后查询。
fn slot12() -> Result<u32, u32> {
    let mut c = Catalog::new();
    c.open(Locale::CUtf8)?;
    c.close()?;
    c.get(SET_UI, 1).map(|_| 0)
}

/// 槽 13：未知转换符。
fn slot13() -> Result<u32, u32> {
    expand("usage: %q", &[]).map(|_| 0)
}

/// 组装 14 格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(cell(0, OP_STRERROR, slot0()));
    let _ = l.register(cell(1, OP_STRERROR, slot1()));
    let _ = l.register(cell(2, OP_STRERROR, slot2()));
    let _ = l.register(cell(3, OP_CATGET, slot3()));
    let _ = l.register(cell(4, OP_CATGET, slot4()));
    let _ = l.register(cell(5, OP_EXPAND, slot5()));
    let _ = l.register(cell(6, OP_CLASS, slot6()));
    let _ = l.register(cell(7, OP_AUDIT, slot7()));
    let _ = l.register(cell(8, OP_STRERROR, slot8()));
    let _ = l.register(cell(9, OP_CATGET, slot9()));
    let _ = l.register(cell(10, OP_CATGET, slot10()));
    let _ = l.register(cell(11, OP_LOCALE, slot11()));
    let _ = l.register(cell(12, OP_STATE, slot12()));
    let _ = l.register(cell(13, OP_EXPAND, slot13()));
    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 错误号边界面。
fn errno_ok() -> bool {
    let a = strerror(0) == Ok("Success");
    let b = strerror(2) == Ok("No such file or directory");
    let d = strerror(34) == Ok("Numerical result out of range");
    let e = strerror(35).err() == Some(MC_ESTR);
    let f = strerror(999).err() == Some(MC_ESTR);
    let g = table_len() == 35;
    a && b && d && e && f && g
}

/// 英文表锚定面。
fn anchors_ok() -> bool {
    let a = strerror(1) == Ok("Operation not permitted");
    let b = strerror(13) == Ok("Permission denied");
    let d = strerror(28) == Ok("No space left on device");
    let e = strerror(32) == Ok("Broken pipe");
    let f = table_len() == 35;
    let mut upper = true;
    let mut i = 0usize;
    while i < STRERRORS.len() {
        match STRERRORS.get(i) {
            Some(s) => {
                if !starts_upper(s) || !no_trailing_space(s) {
                    upper = false;
                }
            }
            None => upper = false,
        }
        i += 1;
    }
    a && b && d && e && f && upper
}

/// 目录查询面。
fn catalog_ok() -> bool {
    let a = catalog_len() == 20;
    let b = count_set(SET_UI) == 6;
    let d = count_set(SET_IO) == 7;
    let e = count_set(SET_NET) == 7;
    let f = catalog_text(SET_UI, 1) == Some("usage: %s [-v] file");
    let g = catalog_text(SET_IO, 3) == Some("write error on %s");
    let h = !has_duplicate_key();
    let mut c = Catalog::new();
    let i = c.open(Locale::C).is_ok();
    let j = c.get(SET_NET, 2) == Ok("connection refused by %s");
    let k = c.get(SET_NET, 2) == Ok("connection refused by %s");
    let m = c.hits() == 2 && c.misses() == 0;
    a && b && d && e && f && g && h && i && j && k && m
}

/// 缺省串回退面。
fn fallback_ok() -> bool {
    let mut c = Catalog::new();
    if c.open(Locale::C).is_err() {
        return false;
    }
    let a = c.get_or(9, 9, "no such entry") == Ok("no such entry");
    let b = c.misses() == 1;
    let d = c.get(SET_UI, 2) == Ok("try '%s --help' for more information");
    let e = c.hits() == 1;
    let f = c.get_or(SET_UI, 2, "ignored") == Ok("try '%s --help' for more information");
    a && b && d && e && f && c.misses() == 1
}

/// 目录开关面。
fn state_ok() -> bool {
    let mut c = Catalog::new();
    let a = !c.is_open();
    let b = c.get(SET_UI, 1).err() == Some(MC_ESTATE);
    let d = c.open(Locale::C).is_ok();
    let e = c.is_open();
    let f = c.open(Locale::Posix).err() == Some(MC_ESTATE);
    let g = c.set_default(0).err() == Some(MC_ESET);
    let h = c.set_default(SET_IO).is_ok();
    let i = c.default_set() == SET_IO;
    let j = c.close().is_ok();
    let k = c.close().err() == Some(MC_ESTATE);
    let m = c.get(SET_IO, 1).err() == Some(MC_ESTATE);
    a && b && d && e && f && g && h && i && j && k && m
}

/// 本地化拒绝面。
fn locale_ok() -> bool {
    let a = Locale::parse(b"C").map(|l| l.is_c()) == Ok(true);
    let b = Locale::parse(b"POSIX").map(|l| l.is_c()) == Ok(true);
    let d = Locale::parse(b"C.UTF-8") == Ok(Locale::CUtf8);
    let e = Locale::parse(b"de_DE.UTF-8") == Ok(Locale::De);
    let f = Locale::parse(b"ja_JP.UTF-8") == Ok(Locale::Ja);
    let g = Locale::parse(b"xx_XX").err() == Some(MC_ELOCALE);
    let h = Locale::De.is_c() == false;
    let i = foreign_sample(Locale::C, 0).is_none();
    let j = foreign_sample(Locale::De, 0) == Some("Datei nicht gefunden");
    let k = audit_str("Zugriff verweigert – Pfad prüfen").err() == Some(MC_EBYTE);
    let mut c = Catalog::new();
    let m = c.open(Locale::De).err() == Some(MC_ELOCALE);
    a && b && d && e && f && g && h && i && j && k && m
}

/// 模板展开面。
fn expand_ok() -> bool {
    let a = expand("usage: %s [-v] file", &[Arg::S("tool")])
        .map(|s| s == "usage: tool [-v] file")
        == Ok(true);
    let b = expand("%s: %s", &[Arg::S("cat"), Arg::S("x")])
        .map(|s| s == "cat: x")
        == Ok(true);
    let d = expand("100%% done", &[]).map(|s| s == "100% done") == Ok(true);
    let e = expand("%d items", &[Arg::I(42)]).map(|s| s == "42 items") == Ok(true);
    let f = expand("%s", &[]).err() == Some(MC_EARG);
    let g = expand("%s", &[Arg::S("a"), Arg::S("b")]).err() == Some(MC_EARG);
    let h = expand("%s %d", &[Arg::I(1), Arg::S("x")]).err() == Some(MC_EARG);
    let i = expand("%q", &[]).err() == Some(MC_EEXPAND);
    let j = expand("tail %", &[]).err() == Some(MC_EEXPAND);
    let k = count_slots("usage: %s [-v] file") == Ok(1);
    let m = count_slots("%s %d %%") == Ok(2);
    a && b && d && e && f && g && h && i && j && k && m
}

/// ASCII 审计面。
fn audit_ok() -> bool {
    let a = audit_str("plain text") == Ok(10);
    let b = audit_bytes(b"a\nb\tc") == Ok(5);
    let d = audit_bytes(&[0x80]).err() == Some(MC_EBYTE);
    let e = audit_bytes(&[0x1f]).err() == Some(MC_EBYTE);
    let f = audit_bytes(&[0x7f]).err() == Some(MC_EBYTE);
    let g = audit_table() == Ok(55);
    let h = audit_str("ファイル").err() == Some(MC_EBYTE);
    let i = audit_bytes(b"").is_ok();
    a && b && d && e && f && g && h && i
}

/// 错误分类面。
fn class_ok() -> bool {
    let a = classify(0) == 0;
    let b = classify(2) == 1;
    let d = classify(13) == 2;
    let e = classify(12) == 3;
    let f = classify(30) == 4;
    let g = classify(34) == 5;
    let h = classify(4) == 6;
    let i = classify(999) == 0;
    let j = all_classified();
    a && b && d && e && f && g && h && i && j
}

/// 容量与集合面。
fn count_ok() -> bool {
    let a = catalog_len() == 20;
    let b = table_len() == 35;
    let d = count_set(SET_UI) == 6;
    let e = count_set(SET_IO) == 7;
    let f = count_set(SET_NET) == 7;
    let g = count_set(0) == 0;
    let h = count_set(99) == 0;
    let i = catalog_len() + table_len() == 55;
    a && b && d && e && f && g && h && i
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
    if l2.register(Sample::blank()) != Err(MC_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 冻结面。
fn freeze_ok() -> bool {
    let mut l = build_ledger();
    let a = l.freeze();
    let root = match a {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = l.seal() == root;
    let d = l.replay() == Ok(root);
    let e = l.freeze().err() == Some(MC_EBAL);
    let f = l.rejects() == 1;
    b && d && e && f
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = errno_ok() && anchors_ok();
    out[1] = catalog_ok() && fallback_ok() && state_ok();
    out[2] = locale_ok() && expand_ok();
    out[3] = audit_ok() && class_ok() && count_ok() && ledger_ok() && freeze_ok();
    out
}

/// 判据集合。
pub fn run_unx11577_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11577.msg_cat");

    s.add("MC01-错误号边界", errno_ok(), "static");
    s.add("MC02-英文表锚定", anchors_ok(), "static");
    s.add("MC03-目录查询", catalog_ok(), "static");
    s.add("MC04-缺省串回退", fallback_ok(), "static");
    s.add("MC05-目录开关状态", state_ok(), "static");
    s.add("MC06-本地化拒绝", locale_ok(), "static");
    s.add("MC07-模板展开", expand_ok(), "static");
    s.add("MC08-ASCII 审计", audit_ok(), "static");
    s.add("MC09-消息分类", class_ok(), "static");
    s.add("MC10-容量与集合", count_ok(), "static");
    s.add("MC11-账本对账", ledger_ok(), "static");
    s.add("MC12-冻结指纹", freeze_ok(), "static");
    s.add("MC13-冻结重放四注入", inject_four() == [true, true, true, true], "static");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_shape() {
        assert_eq!(table_len(), 35);
        assert_eq!(strerror(0), Ok("Success"));
        assert_eq!(strerror(34), Ok("Numerical result out of range"));
        assert_eq!(strerror(35).err(), Some(MC_ESTR));
        assert_eq!(strerror(1000).err(), Some(MC_ESTR));
    }

    #[test]
    fn table_anchors() {
        assert_eq!(strerror(2), Ok("No such file or directory"));
        assert_eq!(strerror(13), Ok("Permission denied"));
        assert_eq!(strerror(28), Ok("No space left on device"));
        assert_eq!(strerror(32), Ok("Broken pipe"));
        assert_eq!(strerror(33), Ok("Numerical argument out of domain"));
    }

    #[test]
    fn table_style() {
        let mut i = 0usize;
        while i < table_len() {
            let s = match STRERRORS.get(i) {
                Some(v) => *v,
                None => "",
            };
            assert!(starts_upper(s));
            assert!(no_trailing_space(s));
            assert_eq!(audit_str(s).is_ok(), true);
            i += 1;
        }
    }

    #[test]
    fn catalog_keys() {
        assert!(!has_duplicate_key());
        assert_eq!(catalog_len(), 20);
        assert_eq!(count_set(SET_UI), 6);
        assert_eq!(count_set(SET_IO), 7);
        assert_eq!(count_set(SET_NET), 7);
        assert_eq!(count_set(0), 0);
    }

    #[test]
    fn catalog_texts() {
        assert_eq!(catalog_text(SET_UI, 1), Some("usage: %s [-v] file"));
        assert_eq!(catalog_text(SET_IO, 5), Some("%s: disk quota exceeded"));
        assert_eq!(catalog_text(SET_NET, 7), Some("too many redirects from %s"));
        assert_eq!(catalog_text(SET_UI, 99), None);
        assert_eq!(catalog_text(9, 1), None);
    }

    #[test]
    fn catalog_hits_and_misses() {
        let mut c = Catalog::new();
        assert!(!c.is_open());
        assert_eq!(c.get(SET_UI, 1).err(), Some(MC_ESTATE));
        assert_eq!(c.open(Locale::C), Ok(()));
        assert_eq!(c.get(SET_UI, 1), Ok("usage: %s [-v] file"));
        assert_eq!(c.get(SET_UI, 2), Ok("try '%s --help' for more information"));
        assert_eq!(c.hits(), 2);
        assert_eq!(c.get(SET_UI, 99), Ok(FALLBACK));
        assert_eq!(c.misses(), 1);
        assert_eq!(c.get_or(SET_UI, 99, "mine"), Ok("mine"));
        assert_eq!(c.misses(), 2);
    }

    #[test]
    fn catalog_state_machine() {
        let mut c = Catalog::new();
        assert_eq!(c.close().err(), Some(MC_ESTATE));
        assert_eq!(c.open(Locale::C), Ok(()));
        assert_eq!(c.open(Locale::C).err(), Some(MC_ESTATE));
        assert_eq!(c.set_default(SET_NET), Ok(()));
        assert_eq!(c.default_set(), SET_NET);
        assert_eq!(c.get_default(2), Ok("connection refused by %s"));
        assert_eq!(c.close(), Ok(()));
        assert_eq!(c.get(SET_UI, 1).err(), Some(MC_ESTATE));
    }

    #[test]
    fn catalog_reserved_zero() {
        let mut c = Catalog::new();
        assert_eq!(c.open(Locale::C), Ok(()));
        assert_eq!(c.get(0, 1).err(), Some(MC_ESET));
        assert_eq!(c.get(1, 0).err(), Some(MC_EMSG));
        assert_eq!(c.get(0, 0).err(), Some(MC_ESET));
    }

    #[test]
    fn locale_parsing() {
        assert_eq!(Locale::C.name(), "C");
        assert_eq!(Locale::parse(b"C"), Ok(Locale::C));
        assert_eq!(Locale::parse(b"POSIX"), Ok(Locale::Posix));
        assert_eq!(Locale::parse(b"C.UTF-8"), Ok(Locale::CUtf8));
        assert_eq!(Locale::parse(b"de_DE"), Ok(Locale::De));
        assert_eq!(Locale::parse(b"fr_FR.UTF-8"), Ok(Locale::Fr));
        assert_eq!(Locale::parse(b"en_US.UTF-8").err(), Some(MC_ELOCALE));
        assert!(Locale::CUtf8.is_c());
        assert!(!Locale::Ja.is_c());
    }

    #[test]
    fn foreign_tables_are_not_ascii() {
        assert_eq!(foreign_sample(Locale::De, 0), Some("Datei nicht gefunden"));
        assert_eq!(foreign_sample(Locale::Fr, 2), Some("Erreur inconnue"));
        assert_eq!(audit_str("Zugriff verweigert – Pfad prüfen").err(), Some(MC_EBYTE));
        assert_eq!(audit_str("ファイルが見つかりません").err(), Some(MC_EBYTE));
        assert!(foreign_sample(Locale::C, 0).is_none());
        assert!(foreign_sample(Locale::De, 9).is_none());
    }

    #[test]
    fn expansion_basic() {
        assert_eq!(
            expand("usage: %s [-v] file", &[Arg::S("tool")]).map(|s| s == "usage: tool [-v] file"),
            Ok(true)
        );
        assert_eq!(expand("%s: %s", &[Arg::S("cat"), Arg::S("x")]).map(|s| s == "cat: x"), Ok(true));
        assert_eq!(expand("100%% done", &[]).map(|s| s == "100% done"), Ok(true));
        assert_eq!(expand("%d items", &[Arg::I(42)]).map(|s| s == "42 items"), Ok(true));
        assert_eq!(expand("%d", &[Arg::I(0)]).map(|s| s == "0"), Ok(true));
    }

    #[test]
    fn expansion_errors() {
        assert_eq!(expand("%s", &[]).err(), Some(MC_EARG));
        assert_eq!(expand("%s", &[Arg::S("a"), Arg::S("b")]).err(), Some(MC_EARG));
        assert_eq!(expand("%s %d", &[Arg::I(1), Arg::S("x")]).err(), Some(MC_EARG));
        assert_eq!(expand("%q", &[Arg::S("a")]).err(), Some(MC_EEXPAND));
        assert_eq!(expand("%", &[]).err(), Some(MC_EEXPAND));
        assert_eq!(expand("%d", &[Arg::S("x")]).err(), Some(MC_EARG));
    }

    #[test]
    fn slot_counting() {
        assert_eq!(count_slots("plain"), Ok(0));
        assert_eq!(count_slots("%s"), Ok(1));
        assert_eq!(count_slots("%s %d %%"), Ok(2));
        assert_eq!(count_slots("%q"), Err(MC_EEXPAND));
        assert_eq!(count_slots("end %"), Err(MC_EEXPAND));
    }

    #[test]
    fn ascii_audit() {
        assert_eq!(audit_str("plain text"), Ok(10));
        assert_eq!(audit_bytes(b"a\nb\tc"), Ok(5));
        assert_eq!(audit_bytes(b""), Ok(0));
        assert_eq!(audit_bytes(&[0x80]), Err(MC_EBYTE));
        assert_eq!(audit_bytes(&[0x1f]), Err(MC_EBYTE));
        assert_eq!(audit_bytes(&[0x7f]), Err(MC_EBYTE));
        assert_eq!(audit_table(), Ok(55));
    }

    #[test]
    fn classification() {
        assert_eq!(classify(0), 0);
        assert_eq!(classify(2), 1);
        assert_eq!(classify(13), 2);
        assert_eq!(classify(12), 3);
        assert_eq!(classify(30), 4);
        assert_eq!(classify(34), 5);
        assert_eq!(classify(4), 6);
        assert_eq!(classify(122), 3);
        assert_eq!(classify(999), 0);
        assert!(all_classified());
    }

    #[test]
    fn ledger_balance() {
        let l = build_ledger();
        assert_eq!(l.len(), 14);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(0).map(|s| s.value), Some(1));
        assert_eq!(l.sample(7).map(|s| s.value), Some(55));
        assert_eq!(l.sample(8).map(|s| s.code), Some(MC_ESTR));
        assert_eq!(l.sample(13).map(|s| s.code), Some(MC_EEXPAND));
        assert!(l.sample(14).is_none());
    }

    #[test]
    fn ledger_seal_and_freeze() {
        let l = build_ledger();
        let root = l.seal();
        assert_eq!(root, build_ledger().seal());
        let mut m = build_ledger();
        assert_eq!(m.freeze(), Ok(root));
        assert!(m.is_frozen());
        assert_eq!(m.root(), root);
        assert_eq!(m.replay(), Ok(root));
        assert_eq!(m.freeze().err(), Some(MC_EBAL));
        assert_eq!(m.rejects(), 1);
    }

    #[test]
    fn ledger_slot_rules() {
        let mut l = Ledger::new();
        assert!(l.is_empty());
        let a = Sample { slot: 1, op: OP_STRERROR, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(a).err(), Some(MC_ESLOT));
        let b = Sample { slot: 0, op: OP_STRERROR, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(b), Ok(()));
        assert_eq!(l.len(), 1);
        assert_eq!(l.register(b).err(), Some(MC_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11577_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }
}
