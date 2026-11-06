//! UNX-F11649 链接期版本要求运行库缺版本报错判据
//!
//! 链接期（更准确说是装载期解析 DT_VERNEED）一旦发现强版本需求在运行库里
//! 找不到，glibc 的诊断是逐字固定的：
//! `./prog: /lib64/libc.so.6: version `GLIBC_2.34' not found (required by ./prog)`。
//! 冒号、反引号、单引号、括号、“required by” 的顺序都不能动；版本串必须原样
//! 出现，程序名在首尾各出现一次。若运行库根本没有版本信息，文案换成
//! `no version information available (required by ./prog)`。
//!
//! 本模块把链接期判定拆成四段：程序名/路径注入闸、版本串语法、运行库版本表、
//! 强弱需求语义。版本串只允许 `PREFIX_A.B.C` 形态：前缀字母开头、下划线分隔、
//! 最多四段十进制、每段不超过 9999；超过四段落 EORDER，缺段落 EFORM。这样
//! 既能覆盖 `GLIBC_2.2.5` 这类深版本，也能把伪造的 `GLIBC_2.34\n` 挡在
//! 文案之外。
//!
//! 弱需求（VER_FLG_WEAK）找不到时**不拒载**，但仍产出一行带 weak 前缀的
//! 诊断；强需求找不到则 Err(EMISS)，调用方必须让链接失败。运行库版本表为空
//! 时落 ENOINFO，而不是把“没有版本信息”误报成“版本不存在”——这两行文案在
//! 排障时指向完全不同的根因。
//!
//! 闸门次序：程序名 → 路径 → 版本语法 → 版本表 → 强弱语义 → 文案 → 账本。
//!
//! 契约：纯 no_std（只用 alloc 的 String）、无 unsafe、无 unwrap/expect/
//! panic!，失败一律 `Result<_, u32>`；账本 14 格（8 成 6 败）逐格对常量表，
//! 冻结后登记落 ESEAL，重放必须复现同一指纹；语料 14 槽互异，覆盖率按
//! `(matched*1000 + total/2)/total` 四舍五入，单槽红档 929ppm。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11649）。
pub const SEG_BASE: u32 = 0xfd80;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 满档千分比。
pub const PPM_FULL: u32 = 1000;

/// 单槽红档千分比。
pub const PPM_RED: u32 = 929;

/// 版本串最大字节数。
pub const VERSION_MAX: usize = 32;

/// 版本段上限。
pub const SEG_MAX: usize = 4;

/// 版本段最大值。
pub const SEG_LIMIT: u32 = 9999;

/// 前缀最大字节数。
pub const PREFIX_MAX: usize = 16;

/// 程序名最大字节数。
pub const PROG_MAX: usize = 64;

/// 路径最大字节数。
pub const PATH_MAX: usize = 96;

/// 一行诊断最大字节数。
pub const LINE_MAX: usize = 256;

/// 运行库版本表上限。
pub const PROV_MAX: usize = 8;

/// 输入里有非法字节（空白、引号、反引号、控制符）。
pub const VN_EBYTE: u32 = SEG_BASE + 0x01;

/// 名字为空或超长。
pub const VN_ENAME: u32 = SEG_BASE + 0x02;

/// 版本串语法坏（缺前缀、缺下划线、空段、段里混字母）。
pub const VN_EFORM: u32 = SEG_BASE + 0x03;

/// 强需求缺版本。
pub const VN_EMISS: u32 = SEG_BASE + 0x04;

/// 运行库没有版本信息。
pub const VN_ENOINFO: u32 = SEG_BASE + 0x05;

/// 版本段数或数值越界。
pub const VN_EORDER: u32 = SEG_BASE + 0x06;

/// 表已冻结或重复冻结。
pub const VN_ESTATE: u32 = SEG_BASE + 0x07;

/// 语义与期望不符。
pub const VN_EVER: u32 = SEG_BASE + 0x08;

/// 账本不平。
pub const VN_EBAL: u32 = SEG_BASE + 0x09;

/// 槽位错序。
pub const VN_ESLOT: u32 = SEG_BASE + 0x0a;

/// 观测值与常量表不符。
pub const VN_EVALUE: u32 = SEG_BASE + 0x0b;

/// 冻结重放指纹不符。
pub const VN_ESEAL: u32 = SEG_BASE + 0x0c;

/// 空观测。
pub const VN_ENOBS: u32 = SEG_BASE + 0x0d;

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

/// 折叠一个字（大端）。
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

/// 有符号渲染。
pub fn i32_text(v: i32) -> String {
    if v < 0 {
        let mut s = String::from("-");
        s.push_str(u32_text(v.unsigned_abs()).as_str());
        s
    } else {
        u32_text(v as u32)
    }
}

/// 八位十六进制渲染。
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
// 版本串
// ---------------------------------------------------------------------------

/// 解析后的版本号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    /// 前缀字节。
    pub prefix: [u8; PREFIX_MAX],
    /// 前缀长度。
    pub prefix_len: usize,
    /// 数字段。
    pub segs: [u32; SEG_MAX],
    /// 段数。
    pub n: usize,
    /// 纯字母后缀（如 PRIVATE），与数字段互斥。
    pub tag: [u8; PREFIX_MAX],
    /// 后缀长度。
    pub tag_len: usize,
}

impl Version {
    /// 空白。
    pub const fn blank() -> Version {
        Version {
            prefix: [0u8; PREFIX_MAX],
            prefix_len: 0,
            segs: [0u32; SEG_MAX],
            n: 0,
            tag: [0u8; PREFIX_MAX],
            tag_len: 0,
        }
    }

    /// 前缀文本。
    pub fn prefix_text(&self) -> &str {
        if self.prefix_len > PREFIX_MAX {
            return "";
        }
        core::str::from_utf8(&self.prefix[..self.prefix_len]).unwrap_or("")
    }

    /// 后缀文本。
    pub fn tag_text(&self) -> &str {
        if self.tag_len > PREFIX_MAX {
            return "";
        }
        core::str::from_utf8(&self.tag[..self.tag_len]).unwrap_or("")
    }

    /// 重建成文本。
    pub fn text(&self) -> String {
        let mut s = String::from(self.prefix_text());
        s.push('_');
        if self.tag_len > 0 {
            s.push_str(self.tag_text());
            return s;
        }
        let mut i = 0usize;
        while i < self.n {
            if i > 0 {
                s.push('.');
            }
            if let Some(v) = self.segs.get(i) {
                s.push_str(u32_text(*v).as_str());
            }
            i += 1;
        }
        s
    }

    /// 是否与前缀同族。
    pub fn same_family(&self, other: &Version) -> bool {
        self.prefix_len == other.prefix_len && self.prefix_text() == other.prefix_text()
    }
}

/// 版本串注入闸 + 语法闸。
pub fn parse_version(s: &str) -> Result<Version, u32> {
    if s.is_empty() {
        return Err(VN_ENAME);
    }
    if s.len() > VERSION_MAX {
        return Err(VN_ENAME);
    }
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(VN_EBYTE),
        };
        let alpha = (c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z');
        let digit = c >= b'0' && c <= b'9';
        if !alpha && !digit && c != b'_' && c != b'.' {
            return Err(VN_EBYTE);
        }
        i += 1;
    }
    let mut p = 0usize;
    while p < b.len() {
        let c = match b.get(p) {
            Some(v) => *v,
            None => return Err(VN_EBYTE),
        };
        if c == b'_' {
            break;
        }
        let alpha = (c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z');
        if p == 0 && !alpha {
            return Err(VN_EFORM);
        }
        p += 1;
    }
    if p == 0 || p == b.len() {
        return Err(VN_EFORM);
    }
    if p > PREFIX_MAX {
        return Err(VN_EFORM);
    }
    let mut v = Version::blank();
    let mut k = 0usize;
    while k < p {
        if let Some(c) = b.get(k) {
            if let Some(dst) = v.prefix.get_mut(k) {
                *dst = *c;
            }
        }
        k += 1;
    }
    v.prefix_len = p;
    let mut j = p + 1;
    let first = match b.get(j) {
        Some(x) => *x,
        None => return Err(VN_EFORM),
    };
    let first_alpha = (first >= b'a' && first <= b'z') || (first >= b'A' && first <= b'Z');
    if first_alpha {
        let mut t = 0usize;
        while j < b.len() {
            let c = match b.get(j) {
                Some(x) => *x,
                None => return Err(VN_EBYTE),
            };
            let alpha = (c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z');
            if !alpha {
                return Err(VN_EFORM);
            }
            if t >= PREFIX_MAX {
                return Err(VN_EORDER);
            }
            if let Some(dst) = v.tag.get_mut(t) {
                *dst = c;
            }
            t += 1;
            j += 1;
        }
        if t == 0 {
            return Err(VN_EFORM);
        }
        v.tag_len = t;
        return Ok(v);
    }
    let mut val = 0u32;
    let mut digits = 0usize;
    while j < b.len() {
        let c = match b.get(j) {
            Some(x) => *x,
            None => return Err(VN_EBYTE),
        };
        if c == b'.' {
            if digits == 0 {
                return Err(VN_EFORM);
            }
            if v.n >= SEG_MAX {
                return Err(VN_EORDER);
            }
            if let Some(dst) = v.segs.get_mut(v.n) {
                *dst = val;
            }
            v.n += 1;
            val = 0;
            digits = 0;
        } else if c >= b'0' && c <= b'9' {
            val = match val.checked_mul(10).and_then(|x| x.checked_add((c - b'0') as u32)) {
                Some(x) => x,
                None => return Err(VN_EORDER),
            };
            if val > SEG_LIMIT {
                return Err(VN_EORDER);
            }
            digits += 1;
        } else {
            return Err(VN_EFORM);
        }
        j += 1;
    }
    if digits == 0 {
        return Err(VN_EFORM);
    }
    if v.n >= SEG_MAX {
        return Err(VN_EORDER);
    }
    if let Some(dst) = v.segs.get_mut(v.n) {
        *dst = val;
    }
    v.n += 1;
    Ok(v)
}

/// 比较同族版本：-1 / 0 / 1。
pub fn cmp_ver(a: &Version, b: &Version) -> i32 {
    let pa = a.prefix_text().as_bytes();
    let pb = b.prefix_text().as_bytes();
    let mut i = 0usize;
    while i < pa.len() && i < pb.len() {
        let x = match pa.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let y = match pb.get(i) {
            Some(v) => *v,
            None => 0,
        };
        if x < y {
            return -1;
        }
        if x > y {
            return 1;
        }
        i += 1;
    }
    if pa.len() < pb.len() {
        return -1;
    }
    if pa.len() > pb.len() {
        return 1;
    }
    let n = if a.n > b.n { a.n } else { b.n };
    let mut k = 0usize;
    while k < n {
        let x = a.segs.get(k).copied().unwrap_or(0);
        let y = b.segs.get(k).copied().unwrap_or(0);
        if x < y {
            return -1;
        }
        if x > y {
            return 1;
        }
        k += 1;
    }
    let ta = a.tag_text().as_bytes();
    let tb = b.tag_text().as_bytes();
    let mut q = 0usize;
    while q < ta.len() && q < tb.len() {
        let x = match ta.get(q) {
            Some(v) => *v,
            None => 0,
        };
        let y = match tb.get(q) {
            Some(v) => *v,
            None => 0,
        };
        if x < y {
            return -1;
        }
        if x > y {
            return 1;
        }
        q += 1;
    }
    if ta.len() < tb.len() {
        return -1;
    }
    if ta.len() > tb.len() {
        return 1;
    }
    0
}

/// a 是否不低于 b。
pub fn at_least(a: &Version, b: &Version) -> bool {
    cmp_ver(a, b) >= 0
}

// ---------------------------------------------------------------------------
// 名字闸
// ---------------------------------------------------------------------------

/// 程序名闸。
pub fn check_prog(p: &str) -> Result<(), u32> {
    if p.is_empty() {
        return Err(VN_ENAME);
    }
    if p.len() > PROG_MAX {
        return Err(VN_ENAME);
    }
    let b = p.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(VN_EBYTE),
        };
        let ok = (c >= b'a' && c <= b'z')
            || (c >= b'A' && c <= b'Z')
            || (c >= b'0' && c <= b'9')
            || c == b'_'
            || c == b'.'
            || c == b'/'
            || c == b'-'
            || c == b'+'
            || c == b'~';
        if !ok {
            return Err(VN_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

/// 路径闸；空路径允许（表示“无运行库路径”）。
pub fn check_path(p: &str) -> Result<(), u32> {
    if p.is_empty() {
        return Ok(());
    }
    if p.len() > PATH_MAX {
        return Err(VN_ENAME);
    }
    let b = p.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(VN_EBYTE),
        };
        let ok = (c >= b'a' && c <= b'z')
            || (c >= b'A' && c <= b'Z')
            || (c >= b'0' && c <= b'9')
            || c == b'_'
            || c == b'.'
            || c == b'/'
            || c == b'-'
            || c == b'+'
            || c == b'~';
        if !ok {
            return Err(VN_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 运行库版本表
// ---------------------------------------------------------------------------

/// 运行库。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Provider {
    /// 磁盘路径；空串表示诊断不带路径。
    pub path: &'static str,
    /// soname。
    pub soname: &'static str,
    /// 版本串表。
    pub versions: [&'static str; PROV_MAX],
    /// 条数。
    pub len: usize,
}

impl Provider {
    /// 空表。
    pub const fn empty(path: &'static str, soname: &'static str) -> Provider {
        Provider { path, soname, versions: [""; PROV_MAX], len: 0 }
    }

    /// 造表：逐条过版本语法闸，条数上限 8。
    pub fn new(
        path: &'static str,
        soname: &'static str,
        list: &[&'static str],
    ) -> Result<Provider, u32> {
        check_path(path)?;
        if soname.is_empty() || soname.len() > PROG_MAX {
            return Err(VN_ENAME);
        }
        if list.len() > PROV_MAX {
            return Err(VN_EORDER);
        }
        let mut p = Provider::empty(path, soname);
        let mut i = 0usize;
        while i < list.len() {
            let s = match list.get(i) {
                Some(v) => *v,
                None => return Err(VN_EORDER),
            };
            let _ = parse_version(s)?;
            if let Some(dst) = p.versions.get_mut(i) {
                *dst = s;
            }
            i += 1;
        }
        p.len = list.len();
        Ok(p)
    }

    /// 是否有版本信息。
    pub fn info(&self) -> bool {
        self.len > 0
    }

    /// 精确命中。
    pub fn has(&self, v: &Version) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.versions.get(i) {
                if let Ok(x) = parse_version(s) {
                    if x == *v {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    /// 按文本精确命中。
    pub fn has_text(&self, s: &str) -> bool {
        match parse_version(s) {
            Ok(v) => self.has(&v),
            Err(_) => false,
        }
    }

    /// 最高版本（同族内取最大；跨族按前缀文本比较）。
    pub fn highest(&self) -> Option<&'static str> {
        let mut best: Option<&'static str> = None;
        let mut best_v: Option<Version> = None;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.versions.get(i) {
                if let Ok(v) = parse_version(s) {
                    let take = match &best_v {
                        Some(b) => cmp_ver(&v, b) > 0,
                        None => true,
                    };
                    if take {
                        best = Some(*s);
                        best_v = Some(v);
                    }
                }
            }
            i += 1;
        }
        best
    }

    /// 表内名字全非空。
    pub fn names_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.versions.get(i) {
                if s.is_empty() {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// glibc 版本表样本。
pub const LIBC_VERSIONS: [&str; 8] = [
    "GLIBC_2.2.5",
    "GLIBC_2.4",
    "GLIBC_2.17",
    "GLIBC_2.28",
    "GLIBC_2.31",
    "GLIBC_2.34",
    "GLIBC_2.35",
    "GLIBC_PRIVATE",
];

/// 受限运行库（只到 2.31）。
pub const LIBC_OLD: [&str; 5] = [
    "GLIBC_2.2.5",
    "GLIBC_2.4",
    "GLIBC_2.17",
    "GLIBC_2.28",
    "GLIBC_2.31",
];

/// libm 版本表。
pub const LIBM_VERSIONS: [&str; 4] = ["GLIBC_2.2.5", "GLIBC_2.18", "GLIBC_2.29", "GLIBC_2.35"];

/// OpenSSL 版本表。
pub const OPENSSL_VERSIONS: [&str; 3] = ["OPENSSL_1.1.0", "OPENSSL_1.1.1", "OPENSSL_3.0.0"];

// ---------------------------------------------------------------------------
// 需求与判定
// ---------------------------------------------------------------------------

/// 一条版本需求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Requirement {
    /// 版本串；空串表示无版本需求。
    pub version: &'static str,
    /// 是否弱需求。
    pub weak: bool,
}

impl Requirement {
    /// 构造。
    pub const fn new(version: &'static str, weak: bool) -> Requirement {
        Requirement { version, weak }
    }

    /// 无版本需求。
    pub const fn versionless() -> Requirement {
        Requirement { version: "", weak: false }
    }
}

/// 判定种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 命中。
    Satisfied,
    /// 无版本需求。
    VersionLess,
    /// 弱需求缺版本，容忍。
    WeakMissing,
    /// 强需求缺版本，拒载。
    Missing,
    /// 运行库无版本信息。
    NoInfo,
}

impl Kind {
    /// 文本。
    pub fn text(self) -> &'static str {
        match self {
            Kind::Satisfied => "satisfied",
            Kind::VersionLess => "versionless",
            Kind::WeakMissing => "weak-missing",
            Kind::Missing => "missing",
            Kind::NoInfo => "no-info",
        }
    }

    /// 是否放行。
    pub fn accepted(self) -> bool {
        match self {
            Kind::Satisfied => true,
            Kind::VersionLess => true,
            Kind::WeakMissing => true,
            Kind::Missing => false,
            Kind::NoInfo => false,
        }
    }
}

/// 判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 种类。
    pub kind: Kind,
    /// 诊断文本；命中/无需求为空。
    pub text: String,
}

impl Outcome {
    /// 命中。
    pub fn satisfied() -> Outcome {
        Outcome { kind: Kind::Satisfied, text: String::new() }
    }

    /// 无版本需求。
    pub fn versionless() -> Outcome {
        Outcome { kind: Kind::VersionLess, text: String::new() }
    }
}

// ---------------------------------------------------------------------------
// 文案
// ---------------------------------------------------------------------------

/// 收尾：长度闸 + 换行。
fn seal_line(mut s: String) -> Result<String, u32> {
    if s.len() >= LINE_MAX {
        return Err(VN_EORDER);
    }
    s.push('\n');
    Ok(s)
}

/// 头段：`prog: path: ` 或 `prog: `。
fn head(prog: &str, path: &str) -> Result<String, u32> {
    check_prog(prog)?;
    check_path(path)?;
    let mut s = String::from(prog);
    s.push_str(": ");
    if !path.is_empty() {
        s.push_str(path);
        s.push_str(": ");
    }
    Ok(s)
}

/// 缺版本文案。
///
/// `prog: path: version `V' not found (required by prog)\n`
pub fn missing_text(prog: &str, req: &Requirement, prov: &Provider) -> Result<String, u32> {
    let v = parse_version(req.version)?;
    let _ = v;
    let mut s = head(prog, prov.path)?;
    s.push_str("version `");
    s.push_str(req.version);
    s.push_str("' not found (required by ");
    s.push_str(prog);
    s.push(')');
    seal_line(s)
}

/// 弱需求缺版本文案。
///
/// `prog: path: weak version `V' not found (required by prog)\n`
pub fn weak_text(prog: &str, req: &Requirement, prov: &Provider) -> Result<String, u32> {
    let _ = parse_version(req.version)?;
    let mut s = head(prog, prov.path)?;
    s.push_str("weak version `");
    s.push_str(req.version);
    s.push_str("' not found (required by ");
    s.push_str(prog);
    s.push(')');
    seal_line(s)
}

/// 无版本信息文案。
///
/// `prog: path: no version information available (required by prog)\n`
pub fn noinfo_text(prog: &str, prov: &Provider) -> Result<String, u32> {
    let mut s = head(prog, prov.path)?;
    s.push_str("no version information available (required by ");
    s.push_str(prog);
    s.push(')');
    seal_line(s)
}

/// 命中/无需求不发诊断。
pub fn satisfied_text() -> String {
    String::new()
}

/// 判定。
///
/// 顺序：程序名 → 版本语法 → 版本表信息 → 精确命中 → 强弱语义。
pub fn check(prog: &str, req: &Requirement, prov: &Provider) -> Result<Outcome, u32> {
    check_prog(prog)?;
    check_path(prov.path)?;
    if !prov.names_ok() {
        return Err(VN_ENAME);
    }
    if req.version.is_empty() {
        return Ok(Outcome::versionless());
    }
    let v = parse_version(req.version)?;
    if !prov.info() {
        return Err(VN_ENOINFO);
    }
    if prov.has(&v) {
        return Ok(Outcome::satisfied());
    }
    if req.weak {
        let text = weak_text(prog, req, prov)?;
        return Ok(Outcome { kind: Kind::WeakMissing, text });
    }
    Err(VN_EMISS)
}

/// 判定并总是给出文案（强缺也返回 Outcome，但 kind=Missing、code 非零）。
pub fn diagnose(prog: &str, req: &Requirement, prov: &Provider) -> Result<Outcome, u32> {
    match check(prog, req, prov) {
        Ok(o) => Ok(o),
        Err(VN_EMISS) => {
            let text = missing_text(prog, req, prov)?;
            Ok(Outcome { kind: Kind::Missing, text })
        }
        Err(VN_ENOINFO) => {
            let text = noinfo_text(prog, prov)?;
            Ok(Outcome { kind: Kind::NoInfo, text })
        }
        Err(c) => Err(c),
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 命中。
    Hit,
    /// 弱需求。
    Weak,
    /// 无版本需求。
    NoNeed,
    /// 强缺。
    Miss,
    /// 无版本信息。
    NoInfo,
    /// 语法坏。
    Bad,
}

/// 探针。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// 判定。
    Check {
        /// 程序名。
        prog: &'static str,
        /// 需求版本。
        ver: &'static str,
        /// 弱需求。
        weak: bool,
        /// 运行库路径。
        path: &'static str,
        /// soname。
        soname: &'static str,
        /// 版本表。
        versions: &'static [&'static str],
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

/// 语料槽。
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
        "./prog -> libc GLIBC_2.34",
        Face::Hit,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_2.34",
            weak: false,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &LIBC_VERSIONS,
        },
        Want::Text(""),
        "hit-latest",
    ),
    Case::new(
        "./prog -> libc GLIBC_2.17",
        Face::Hit,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_2.17",
            weak: false,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &LIBC_VERSIONS,
        },
        Want::Text(""),
        "hit-old",
    ),
    Case::new(
        "./prog -> libc GLIBC_2.2.5",
        Face::Hit,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_2.2.5",
            weak: false,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &LIBC_VERSIONS,
        },
        Want::Text(""),
        "hit-deep",
    ),
    Case::new(
        "./prog -> libc GLIBC_PRIVATE",
        Face::Hit,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_PRIVATE",
            weak: false,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &LIBC_VERSIONS,
        },
        Want::Text(""),
        "hit-private",
    ),
    Case::new(
        "./prog -> libm GLIBC_2.35",
        Face::Hit,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_2.35",
            weak: false,
            path: "/lib64/libm.so.6",
            soname: "libm.so.6",
            versions: &LIBM_VERSIONS,
        },
        Want::Text(""),
        "hit-libm",
    ),
    Case::new(
        "./prog -> libssl OPENSSL_3.0.0",
        Face::Hit,
        Probe::Check {
            prog: "./prog",
            ver: "OPENSSL_3.0.0",
            weak: false,
            path: "/usr/lib/libssl.so.3",
            soname: "libssl.so.3",
            versions: &OPENSSL_VERSIONS,
        },
        Want::Text(""),
        "hit-openssl",
    ),
    Case::new(
        "./prog -> libc weak GLIBC_2.99",
        Face::Weak,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_2.99",
            weak: true,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &LIBC_VERSIONS,
        },
        Want::Text("./prog: /lib64/libc.so.6: weak version `GLIBC_2.99' not found (required by ./prog)\n"),
        "weak-miss",
    ),
    Case::new(
        "./prog -> libc weak no path",
        Face::Weak,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_2.99",
            weak: true,
            path: "",
            soname: "libc.so.6",
            versions: &LIBC_VERSIONS,
        },
        Want::Text("./prog: weak version `GLIBC_2.99' not found (required by ./prog)\n"),
        "weak-nopath",
    ),
    Case::new(
        "./prog -> libc GLIBC_2.34 (old libc)",
        Face::Miss,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_2.34",
            weak: false,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &LIBC_OLD,
        },
        Want::Code(VN_EMISS),
        "miss-strong",
    ),
    Case::new(
        "./prog -> libc GLIBC_2.99",
        Face::Miss,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_2.99",
            weak: false,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &LIBC_VERSIONS,
        },
        Want::Code(VN_EMISS),
        "miss-new",
    ),
    Case::new(
        "./prog -> libc no version info",
        Face::NoInfo,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_2.34",
            weak: false,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &[],
        },
        Want::Code(VN_ENOINFO),
        "noinfo",
    ),
    Case::new(
        "./prog -> libc GLIBC_",
        Face::Bad,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_",
            weak: false,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &LIBC_VERSIONS,
        },
        Want::Code(VN_EFORM),
        "bad-empty-seg",
    ),
    Case::new(
        "./prog -> libc GLIBC_1.2.3.4.5",
        Face::Bad,
        Probe::Check {
            prog: "./prog",
            ver: "GLIBC_1.2.3.4.5",
            weak: false,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &LIBC_VERSIONS,
        },
        Want::Code(VN_EORDER),
        "bad-many-segs",
    ),
    Case::new(
        "./t;rm -> libc GLIBC_2.34",
        Face::Bad,
        Probe::Check {
            prog: "./t;rm",
            ver: "GLIBC_2.34",
            weak: false,
            path: "/lib64/libc.so.6",
            soname: "libc.so.6",
            versions: &LIBC_VERSIONS,
        },
        Want::Code(VN_EBYTE),
        "bad-prog",
    ),
];

/// 按槽取。
pub fn case_at(i: usize) -> Option<Case> {
    CASES.get(i).copied()
}

/// 语料互异。
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

/// 运行探针。
pub fn judge(p: Probe) -> Result<String, u32> {
    match p {
        Probe::Check { prog, ver, weak, path, soname, versions } => {
            let prov = Provider::new(path, soname, versions)?;
            let req = Requirement { version: ver, weak };
            match check(prog, &req, &prov) {
                Ok(o) => Ok(o.text),
                Err(c) => Err(c),
            }
        }
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
    /// 语法类。
    pub form: u32,
    /// 强缺类。
    pub miss: u32,
    /// 无信息类。
    pub noinfo: u32,
    /// 名字类。
    pub name: u32,
    /// 命中数。
    pub matched: u32,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census { total: 0, accept: 0, reject: 0, form: 0, miss: 0, noinfo: 0, name: 0, matched: 0 }
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 千分比。
    pub fn rate(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let scaled = self.matched.saturating_mul(1000);
        scaled.saturating_add(self.total / 2) / self.total
    }

    /// 全绿。
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
            return Err(VN_ENOBS);
        }
        if self.matched != self.total {
            return Err(VN_EVALUE);
        }
        Ok(())
    }

    fn bump(&mut self, code: u32) {
        match code {
            VN_EFORM | VN_EORDER => self.form = self.form.saturating_add(1),
            VN_EMISS => self.miss = self.miss.saturating_add(1),
            VN_ENOINFO => self.noinfo = self.noinfo.saturating_add(1),
            _ => self.name = self.name.saturating_add(1),
        }
    }

    /// 固定渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("vnf corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" form=");
        s.push_str(u32_text(self.form).as_str());
        s.push_str(" miss=");
        s.push_str(u32_text(self.miss).as_str());
        s.push_str(" noinfo=");
        s.push_str(u32_text(self.noinfo).as_str());
        s.push_str(" name=");
        s.push_str(u32_text(self.name).as_str());
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
            return Err(VN_ESLOT);
        }
    }
    let mut c = Census::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let cs = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(VN_ESLOT),
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

/// 封面。
pub fn cover(n: usize) -> Result<(), u32> {
    if n == CORPUS {
        Ok(())
    } else {
        Err(VN_ENOBS)
    }
}

/// 按语料统计强弱需求条数。
pub fn need_census() -> Result<(u32, u32), u32> {
    let mut strong = 0u32;
    let mut weak = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            let Probe::Check { ver, weak: w, .. } = c.probe;
                if !ver.is_empty() {
                    if w {
                        weak = weak.saturating_add(1);
                    } else {
                        strong = strong.saturating_add(1);
                    }
                }
        }
        i += 1;
    }
    Ok((strong, weak))
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
    /// 空白。
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
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(VN_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(VN_ESLOT);
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

    /// 取格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成格数。
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

    /// 败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(VN_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 2, 2, 1, 1, 929];
        const CODES: [u32; 6] =
            [VN_EMISS, VN_EMISS, VN_ENOINFO, VN_EFORM, VN_EORDER, VN_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(VN_EBAL),
            };
            if s.slot as usize != i {
                return Err(VN_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(VN_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(VN_EVALUE);
                        }
                    }
                    None => return Err(VN_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(VN_EVALUE);
                        }
                    }
                    None => return Err(VN_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(VN_EBAL);
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
            return Err(VN_ESEAL);
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
            Err(VN_ESEAL)
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

    /// 拒收数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

const OP_TOTAL: u8 = 0;
const OP_ACCEPT: u8 = 1;
const OP_REJECT: u8 = 2;
const OP_FORM: u8 = 3;
const OP_MISS: u8 = 4;
const OP_NOINFO: u8 = 5;
const OP_NAME: u8 = 6;
const OP_RATE: u8 = 7;
const OP_QUERY: u8 = 8;

fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

fn err_cell(slot: u8, op: u8, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 从判定结果抽码（成功给 fallback）。
fn code_of(r: Result<Outcome, u32>, fallback: u32) -> u32 {
    match r {
        Ok(_) => fallback,
        Err(c) => c,
    }
}

/// 建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let r = run();
    let _ = l.register(count_cell(0, OP_TOTAL, r.map(|x| x.total as usize)));
    let _ = l.register(count_cell(1, OP_ACCEPT, r.map(|x| x.accept as usize)));
    let _ = l.register(count_cell(2, OP_REJECT, r.map(|x| x.reject as usize)));
    let _ = l.register(count_cell(3, OP_FORM, r.map(|x| x.form as usize)));
    let _ = l.register(count_cell(4, OP_MISS, r.map(|x| x.miss as usize)));
    let _ = l.register(count_cell(5, OP_NOINFO, r.map(|x| x.noinfo as usize)));
    let _ = l.register(count_cell(6, OP_NAME, r.map(|x| x.name as usize)));
    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_RATE, red.map(|x| x.rate() as usize)));

    let old = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_OLD) {
        Ok(v) => v,
        Err(_) => Provider::empty("/lib64/libc.so.6", "libc.so.6"),
    };
    let miss = Requirement::new("GLIBC_2.34", false);
    let _ = l.register(err_cell(8, OP_QUERY, code_of(check("./prog", &miss, &old), VN_EMISS)));

    let libc = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_VERSIONS) {
        Ok(v) => v,
        Err(_) => Provider::empty("/lib64/libc.so.6", "libc.so.6"),
    };
    let miss2 = Requirement::new("GLIBC_2.99", false);
    let _ = l.register(err_cell(9, OP_QUERY, code_of(check("./prog", &miss2, &libc), VN_EMISS)));

    let none = Provider::empty("/lib64/libc.so.6", "libc.so.6");
    let _ = l.register(err_cell(10, OP_QUERY, code_of(check("./prog", &miss, &none), VN_ENOINFO)));

    let form = Requirement::new("GLIBC_", false);
    let _ = l.register(err_cell(11, OP_QUERY, code_of(check("./prog", &form, &libc), VN_EFORM)));

    let order = Requirement::new("GLIBC_1.2.3.4.5", false);
    let _ = l.register(err_cell(12, OP_QUERY, code_of(check("./prog", &order, &libc), VN_EORDER)));

    let _ = l.register(err_cell(13, OP_QUERY, code_of(check("./t;rm", &miss, &libc), VN_EBYTE)));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 版本语法。
fn version_ok() -> bool {
    let a = match parse_version("GLIBC_2.34") {
        Ok(v) => v.prefix_text() == "GLIBC" && v.n == 2 && v.segs.first() == Some(&2),
        Err(_) => false,
    };
    let b = parse_version("GLIBC_2.34").map(|v| v.text()) == Ok(String::from("GLIBC_2.34"));
    let c = parse_version("GLIBC_2.2.5").map(|v| v.n) == Ok(3);
    let d = parse_version("GLIBC_2.2.5").map(|v| v.text()) == Ok(String::from("GLIBC_2.2.5"));
    let e = parse_version("GLIBC_").err() == Some(VN_EFORM);
    let f = parse_version("GLIBC").err() == Some(VN_EFORM);
    let g = parse_version("GLIBC_2.x").err() == Some(VN_EFORM);
    let h = parse_version("GLIBC_1.2.3.4.5").err() == Some(VN_EORDER);
    let i = parse_version("GLIBC_2.34\n").err() == Some(VN_EBYTE);
    let j = parse_version("").err() == Some(VN_ENAME);
    a && b && c && d && e && f && g && h && i && j
}

/// 版本比较。
fn compare_ok() -> bool {
    let a = parse_version("GLIBC_2.34");
    let b = parse_version("GLIBC_2.31");
    let c = parse_version("GLIBC_2.2.5");
    let d = parse_version("GLIBC_2.4");
    let e = parse_version("OPENSSL_3.0.0");
    match (a, b, c, d, e) {
        (Ok(va), Ok(vb), Ok(vc), Ok(vd), Ok(ve)) => {
            let x = cmp_ver(&va, &vb) == 1 && cmp_ver(&vb, &va) == -1 && cmp_ver(&va, &va) == 0;
            let y = cmp_ver(&vc, &vd) == -1 && at_least(&vd, &vc) && !at_least(&vc, &vd);
            let z = !va.same_family(&ve) && va.same_family(&vb);
            let w = cmp_ver(&ve, &va) > 0;
            x && y && z && w
        }
        _ => false,
    }
}

/// 名字闸。
fn names_ok() -> bool {
    let a = check_prog("./prog").is_ok() && check_prog("/usr/bin/tool+").is_ok();
    let b = check_prog("").err() == Some(VN_ENAME);
    let c = check_prog("./t;rm").err() == Some(VN_EBYTE);
    let d = check_prog("./t rm").err() == Some(VN_EBYTE);
    let e = check_prog("./t\nrm").err() == Some(VN_EBYTE);
    let f = check_path("").is_ok() && check_path("/lib64/libc.so.6").is_ok();
    let g = check_path("/lib64/libc.so.6;rm").err() == Some(VN_EBYTE);
    let h = check_path("`x`").err() == Some(VN_EBYTE);
    a && b && c && d && e && f && g && h
}

/// 运行库表。
fn provider_ok() -> bool {
    let p = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_VERSIONS) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = p.len == 8 && p.info() && p.names_ok();
    let b = p.has_text("GLIBC_2.34") && p.has_text("GLIBC_2.2.5") && p.has_text("GLIBC_PRIVATE");
    let c = !p.has_text("GLIBC_2.99") && !p.has_text("GLIBC_");
    let d = p.highest() == Some("GLIBC_2.35");
    let e = !Provider::empty("", "libc.so.6").info();
    let f = Provider::new("", "libc.so.6", &[]).map(|v| v.len) == Ok(0);
    let g = Provider::new("/x", "x.so", &["BAD"]).err() == Some(VN_EFORM);
    let h = Provider::new("/x", "x.so", &["A_1", "A_2", "A_3", "A_4", "A_5", "A_6", "A_7", "A_8", "A_9"])
        .err()
        == Some(VN_EORDER);
    a && b && c && d && e && f && g && h
}

/// 文案逐字。
fn text_ok() -> bool {
    let p = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_OLD) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let req = Requirement::new("GLIBC_2.34", false);
    let a = missing_text("./prog", &req, &p).ok().as_deref()
        == Some("./prog: /lib64/libc.so.6: version `GLIBC_2.34' not found (required by ./prog)\n");
    let np = Provider::empty("", "libc.so.6");
    let b = missing_text("./prog", &req, &np).ok().as_deref()
        == Some("./prog: version `GLIBC_2.34' not found (required by ./prog)\n");
    let w = Requirement::new("GLIBC_2.99", true);
    let c = weak_text("./prog", &w, &p).ok().as_deref()
        == Some("./prog: /lib64/libc.so.6: weak version `GLIBC_2.99' not found (required by ./prog)\n");
    let d = noinfo_text("./prog", &np).ok().as_deref()
        == Some("./prog: no version information available (required by ./prog)\n");
    let e = noinfo_text("./prog", &p).ok().as_deref()
        == Some("./prog: /lib64/libc.so.6: no version information available (required by ./prog)\n");
    let f = missing_text("./t;rm",&req,&p).err() == Some(VN_EBYTE);
    let g = satisfied_text().is_empty();
    a && b && c && d && e && f && g
}

/// 强弱语义。
fn semantics_ok() -> bool {
    let libc = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_VERSIONS) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let old = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_OLD) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let hit = check("./prog", &Requirement::new("GLIBC_2.34", false), &libc);
    let a = match hit {
        Ok(o) => o.kind == Kind::Satisfied && o.text.is_empty(),
        Err(_) => false,
    };
    let b = check("./prog", &Requirement::new("GLIBC_2.34", false), &old).err() == Some(VN_EMISS);
    let c = match check("./prog", &Requirement::new("GLIBC_2.99", true), &libc) {
        Ok(o) => o.kind == Kind::WeakMissing && !o.text.is_empty(),
        Err(_) => false,
    };
    let d = check("./prog", &Requirement::versionless(), &old).map(|o| o.kind) == Ok(Kind::VersionLess);
    let e = check("./prog", &Requirement::new("GLIBC_2.34", false), &Provider::empty("", "libc.so.6"))
        .err()
        == Some(VN_ENOINFO);
    let f = match diagnose("./prog", &Requirement::new("GLIBC_2.34", false), &old) {
        Ok(o) => o.kind == Kind::Missing && o.text.as_str().contains("version `GLIBC_2.34' not found"),
        Err(_) => false,
    };
    let g = Kind::Satisfied.accepted() && Kind::WeakMissing.accepted() && !Kind::Missing.accepted();
    a && b && c && d && e && f && g
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
    let b = c.form == 2 && c.miss == 2 && c.noinfo == 1 && c.name == 1;
    let d = c.matched == 14 && c.rate() == PPM_FULL && c.green() && c.gate() == Ok(());
    a && b && d
}

/// 槽位编号。
fn slots_ok() -> bool {
    let a = CORPUS == 14 && CASES.len() == CORPUS && unique() && notes_ok();
    let b = match case_at(0) {
        Some(c) => c.face == Face::Hit && c.want == Want::Text(""),
        None => false,
    };
    let c = match case_at(6) {
        Some(c) => c.face == Face::Weak && c.note == "weak-miss",
        None => false,
    };
    let d = match case_at(13) {
        Some(c) => c.want == Want::Code(VN_EBYTE),
        None => false,
    };
    let e = case_at(14).is_none() && case_at(77).is_none();
    a && b && c && d && e
}

/// 汇总渲染。
fn report_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.render().as_str()
        == "vnf corpus=14 accept=8 reject=6 form=2 miss=2 noinfo=1 name=1 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(VN_ENOBS);
    let d = e.render().as_str()
        == "vnf corpus=0 accept=0 reject=0 form=0 miss=0 noinfo=0 name=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(CORPUS + 1) == Err(VN_ENOBS);
    let g = need_census() == Ok((12, 2));
    a && b && d && f && g
}

/// 红档。
fn reds_ok() -> bool {
    let c = match run_with(Some(7)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched == 13 && c.rate() == PPM_RED && c.gate() == Err(VN_EVALUE);
    let b = !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(VN_ESLOT);
    let e = run_with(Some(CORPUS + 3)).err() == Some(VN_ESLOT);
    a && b && d && e
}

/// 账本。
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
    let d = f.register(Sample::blank()).err() == Some(VN_ESEAL) && f.rejects() == before + 1;
    a && b && c && d
}

/// 文本助手。
fn helpers_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(929) == "929" && u32_text(4294967295) == "4294967295";
    let b = hex_text(SEG_BASE) == "0x0000fd80" && hex_text(VN_EMISS) == "0x0000fd84";
    let c = i32_text(-9) == "-9" && i32_text(9) == "9";
    let d = bool_text(true) == "true" && bool_text(false) == "false";
    let e = digest(b"version") == digest(b"version") && digest(b"version") != digest(b"versio");
    let f = fold_word(FNV_OFF, 7) == digest(&[0, 0, 0, 7]);
    a && b && c && d && e && f
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = version_ok() && compare_ok();
    out[1] = names_ok() && provider_ok() && text_ok();
    out[2] = semantics_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && ledger_ok() && helpers_ok();
    out
}

/// 判据集合。
pub fn run_unx11649_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11649.version_missing_text");

    s.add("VN01-版本语法", version_ok(), "static");
    s.add("VN02-版本比较", compare_ok(), "static");
    s.add("VN03-名字闸", names_ok(), "static");
    s.add("VN04-运行库版本表", provider_ok(), "static");
    s.add("VN05-文案逐字", text_ok(), "static");
    s.add("VN06-强弱语义", semantics_ok(), "static");
    s.add("VN07-十四槽表判", corpus_ok(), "static");
    s.add("VN08-槽位编号", slots_ok(), "static");
    s.add("VN09-汇总渲染", report_ok(), "static");
    s.add("VN10-红档929", reds_ok(), "static");
    s.add("VN11-账本对账", ledger_ok(), "static");
    s.add("VN12-文本助手", helpers_ok(), "static");
    s.add("VN13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
    fn version_parse_valid() {
        let v = match parse_version("GLIBC_2.34") {
            Ok(x) => x,
            Err(_) => return,
        };
        assert_eq!(v.prefix_text(), "GLIBC");
        assert_eq!(v.n, 2);
        assert_eq!(v.segs.first(), Some(&2));
        assert_eq!(v.text(), "GLIBC_2.34");
        assert_eq!(v.text().as_str(), "GLIBC_2.34");
    }

    #[test]
    fn version_parse_deep() {
        let v = match parse_version("GLIBC_2.2.5") {
            Ok(x) => x,
            Err(_) => return,
        };
        assert_eq!(v.n, 3);
        assert_eq!(v.segs.get(2), Some(&5));
        assert_eq!(v.text(), "GLIBC_2.2.5");
    }

    #[test]
    fn version_parse_rejects() {
        assert_eq!(parse_version("").err(), Some(VN_ENAME));
        assert_eq!(parse_version("GLIBC").err(), Some(VN_EFORM));
        assert_eq!(parse_version("GLIBC_").err(), Some(VN_EFORM));
        assert_eq!(parse_version("GLIBC_2.").err(), Some(VN_EFORM));
        assert_eq!(parse_version("GLIBC_.2").err(), Some(VN_EFORM));
        assert_eq!(parse_version("GLIBC_2.x").err(), Some(VN_EFORM));
        assert_eq!(parse_version("2GLIBC_2").err(), Some(VN_EFORM));
        assert_eq!(parse_version("GLIBC_1.2.3.4.5").err(), Some(VN_EORDER));
        assert_eq!(parse_version("GLIBC_10000").err(), Some(VN_EORDER));
    }

    #[test]
    fn version_parse_injection() {
        assert_eq!(parse_version("GLIBC_2.34\n").err(), Some(VN_EBYTE));
        assert_eq!(parse_version("GLIBC_2 34").err(), Some(VN_EBYTE));
        assert_eq!(parse_version("GLIBC_2`34").err(), Some(VN_EBYTE));
        let long = "GLIBC_2.34.567890123456789012345678";
        assert_eq!(parse_version(long).err(), Some(VN_ENAME));
    }

    #[test]
    fn version_compare() {
        let a = match parse_version("GLIBC_2.34") {
            Ok(v) => v,
            Err(_) => return,
        };
        let b = match parse_version("GLIBC_2.31") {
            Ok(v) => v,
            Err(_) => return,
        };
        let c = match parse_version("GLIBC_2.2.5") {
            Ok(v) => v,
            Err(_) => return,
        };
        let d = match parse_version("GLIBC_2.4") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(cmp_ver(&a, &b), 1);
        assert_eq!(cmp_ver(&b, &a), -1);
        assert_eq!(cmp_ver(&a, &a), 0);
        assert_eq!(cmp_ver(&c, &d), -1);
        assert!(at_least(&d, &c));
        assert!(!at_least(&c, &d));
    }

    #[test]
    fn version_family() {
        let a = match parse_version("GLIBC_2.34") {
            Ok(v) => v,
            Err(_) => return,
        };
        let b = match parse_version("OPENSSL_3.0.0") {
            Ok(v) => v,
            Err(_) => return,
        };
        assert!(a.same_family(&a));
        assert!(!a.same_family(&b));
        assert_eq!(cmp_ver(&b, &a), 1);
    }

    #[test]
    fn prog_gate() {
        assert!(check_prog("./prog").is_ok());
        assert!(check_prog("/usr/bin/tool+").is_ok());
        assert_eq!(check_prog("").err(), Some(VN_ENAME));
        assert_eq!(check_prog("./t;rm").err(), Some(VN_EBYTE));
        assert_eq!(check_prog("./t rm").err(), Some(VN_EBYTE));
        assert_eq!(check_prog("./t`rm").err(), Some(VN_EBYTE));
        assert_eq!(check_prog("./t\nrm").err(), Some(VN_EBYTE));
    }

    #[test]
    fn path_gate() {
        assert!(check_path("").is_ok());
        assert!(check_path("/lib64/libc.so.6").is_ok());
        assert_eq!(check_path("/lib64/libc.so.6;rm").err(), Some(VN_EBYTE));
        assert_eq!(check_path("/lib/`x`").err(), Some(VN_EBYTE));
    }

    #[test]
    fn provider_table() {
        let p = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_VERSIONS) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(p.len, 8);
        assert!(p.info());
        assert!(p.names_ok());
        assert!(p.has_text("GLIBC_2.34"));
        assert!(p.has_text("GLIBC_2.2.5"));
        assert!(!p.has_text("GLIBC_2.99"));
        assert_eq!(p.highest(), Some("GLIBC_2.35"));
        assert_eq!(Provider::empty("", "x").len, 0);
        assert!(!Provider::empty("", "x").info());
    }

    #[test]
    fn provider_rejects() {
        assert_eq!(Provider::new("/x", "x.so", &["BAD"]).err(), Some(VN_EFORM));
        assert_eq!(Provider::new("/x", "x.so", &["A_1.2.3.4.5"]).err(), Some(VN_EORDER));
        assert_eq!(Provider::new("", "", &[]).err(), Some(VN_ENAME));
        let long = &["A_1", "A_2", "A_3", "A_4", "A_5", "A_6", "A_7", "A_8", "A_9"];
        assert_eq!(Provider::new("/x", "x.so", long).err(), Some(VN_EORDER));
    }

    #[test]
    fn missing_text_exact() {
        let p = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_OLD) {
            Ok(v) => v,
            Err(_) => return,
        };
        let req = Requirement::new("GLIBC_2.34", false);
        assert_eq!(
            missing_text("./prog", &req, &p).ok(),
            Some(String::from("./prog: /lib64/libc.so.6: version `GLIBC_2.34' not found (required by ./prog)\n"))
        );
        let np = Provider::empty("", "libc.so.6");
        assert_eq!(
            missing_text("./prog", &req, &np).ok(),
            Some(String::from("./prog: version `GLIBC_2.34' not found (required by ./prog)\n"))
        );
    }

    #[test]
    fn weak_text_exact() {
        let p = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_VERSIONS) {
            Ok(v) => v,
            Err(_) => return,
        };
        let req = Requirement::new("GLIBC_2.99", true);
        assert_eq!(
            weak_text("./prog", &req, &p).ok(),
            Some(String::from("./prog: /lib64/libc.so.6: weak version `GLIBC_2.99' not found (required by ./prog)\n"))
        );
    }

    #[test]
    fn noinfo_text_exact() {
        let p = Provider::empty("/lib64/libc.so.6", "libc.so.6");
        assert_eq!(
            noinfo_text("./prog", &p).ok(),
            Some(String::from("./prog: /lib64/libc.so.6: no version information available (required by ./prog)\n"))
        );
        assert_eq!(
            noinfo_text("./prog", &Provider::empty("", "libc.so.6")).ok(),
            Some(String::from("./prog: no version information available (required by ./prog)\n"))
        );
    }

    #[test]
    fn check_semantics() {
        let libc = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_VERSIONS) {
            Ok(v) => v,
            Err(_) => return,
        };
        let old = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_OLD) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(
            check("./prog", &Requirement::new("GLIBC_2.34", false), &libc).map(|o| o.kind),
            Ok(Kind::Satisfied)
        );
        assert_eq!(
            check("./prog", &Requirement::new("GLIBC_2.34", false), &old).err(),
            Some(VN_EMISS)
        );
        assert_eq!(
            check("./prog", &Requirement::new("GLIBC_2.99", true), &libc).map(|o| o.kind),
            Ok(Kind::WeakMissing)
        );
        assert_eq!(
            check("./prog", &Requirement::versionless(), &old).map(|o| o.kind),
            Ok(Kind::VersionLess)
        );
        assert_eq!(
            check("./prog", &Requirement::new("GLIBC_2.34", false), &Provider::empty("", "libc.so.6"))
                .err(),
            Some(VN_ENOINFO)
        );
    }

    #[test]
    fn diagnose_paths() {
        let old = match Provider::new("/lib64/libc.so.6", "libc.so.6", &LIBC_OLD) {
            Ok(v) => v,
            Err(_) => return,
        };
        let o = match diagnose("./prog", &Requirement::new("GLIBC_2.34", false), &old) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(o.kind, Kind::Missing);
        assert!(o.text.as_str().contains("version `GLIBC_2.34' not found"));
        let n = match diagnose("./prog", &Requirement::new("GLIBC_2.34", false), &Provider::empty("", "libc.so.6")) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(n.kind, Kind::NoInfo);
        assert!(n.text.as_str().contains("no version information available"));
    }

    #[test]
    fn outcome_helpers() {
        assert_eq!(Kind::Satisfied.text(), "satisfied");
        assert_eq!(Kind::WeakMissing.text(), "weak-missing");
        assert_eq!(Kind::NoInfo.text(), "no-info");
        assert!(Kind::VersionLess.accepted());
        assert!(!Kind::NoInfo.accepted());
        assert!(Outcome::satisfied().text.is_empty());
        assert_eq!(Outcome::versionless().kind, Kind::VersionLess);
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
        assert_eq!(c.form, 2);
        assert_eq!(c.miss, 2);
        assert_eq!(c.noinfo, 1);
        assert_eq!(c.name, 1);
        assert_eq!(c.rate(), 1000);
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
            "vnf corpus=14 accept=8 reject=6 form=2 miss=2 noinfo=1 name=1 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rounding() {
        let c = match run_with(Some(13)) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.matched, 13);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(VN_EVALUE));
        assert_eq!(run_with(Some(14)).err(), Some(VN_ESLOT));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.sample(7).map(|s| s.value), Some(929));
        assert!(l.sample(14).is_none());
    }

    #[test]
    fn ledger_codes() {
        let l = build_ledger();
        let want = [VN_EMISS, VN_EMISS, VN_ENOINFO, VN_EFORM, VN_EORDER, VN_EBYTE];
        let mut i = 0usize;
        while i < 6 {
            let s = match l.sample(8 + i) {
                Some(v) => v,
                None => return,
            };
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
        assert_eq!(l.freeze(), Err(VN_ESEAL));
        let before = l.rejects();
        assert_eq!(l.register(Sample::blank()).err(), Some(VN_ESEAL));
        assert_eq!(l.rejects(), before + 1);
    }

    #[test]
    fn ledger_order_gate() {
        let mut l = Ledger::new();
        let late = Sample { slot: 2, op: 0, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(late).err(), Some(VN_ESLOT));
        assert_eq!(l.rejects(), 1);
        let good = Sample { slot: 0, op: 0, ok: true, code: 0, value: 14 };
        assert_eq!(l.register(good), Ok(()));
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
    fn helpers() {
        assert_eq!(u32_text(4294967295), "4294967295");
        assert_eq!(hex_text(SEG_BASE), "0x0000fd80");
        assert_eq!(i32_text(-1), "-1");
        assert_eq!(bool_text(true), "true");
        assert_eq!(digest(b""), FNV_OFF);
        assert_eq!(fold_word(FNV_OFF, 0x0102_0304), digest(&[1, 2, 3, 4]));
    }

    #[test]
    fn need_census_pair() {
        assert_eq!(need_census(), Ok((12, 2)));
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11649_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn check_13_name() {
        let s = run_unx11649_checks();
        let c = match s.get(12) {
            Some(v) => v,
            None => return,
        };
        assert!(c.name.ends_with("13-冻结重放四注入"));
    }
}
