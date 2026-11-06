//! UNX-F11651 VERNEED 指向不存在符号判据
//!
//! `.gnu.version_r`（VERNEED）描述“本对象需要依赖方提供哪些版本”。每个
//! Verneed 头挂一条 Verneed 文件串与一串 Vernaux：`hash`、`flags`、`other`、
//! `name`、`next`。装载器拿着 Vernaux 去依赖对象的版本表里找对应符号；如果
//! 依赖里根本没有这个符号，强需求直接解析失败，弱需求（VER_FLG_WEAK）只打
//! 一行 `weak symbol foo, version V not found (required by libc.so.6)` 便放行。
//!
//! 判据分成两层。结构层先过：版本号必须为 1、cnt 与 aux 链长度一致、file 与
//! name 偏移都落在缓冲区内、串必须以 NUL 结尾且不超过上限、other 非零、aux
//! 的 hash 必须等于版本串的 ELF 哈希（`_dl_elf_hash`）、flags 只允许 0 或
//! WEAK。语义层再过：依赖对象必须已装载且有版本节；符号名与版本必须精确
//! 命中依赖导出的二元组；名字在而版本不在落 EMSVER，名字不在落 ESYM，
//! 依赖缺失或无版本信息落 EDEP。
//!
//! 本模块把 VERNEED 表写进 512 字节定长缓冲再解析回来，偏移、链、哈希都要
//! 自洽，坏块在解析阶段一律落 EMETA。解析出来的符号串与版本串在缓冲里相邻
//! 存放：name 偏移指向符号串，符号串的 NUL 之后紧接版本串，这样 16 字节的
//! Vernaux 无需额外字段也能同时带住“符号 + 版本”二元组。
//!
//! 版本串只允许 `PREFIX_A.B.C` 形态；空串落 ENAME，注入字节落 EBYTE，结构
//! 坏按不可匹配落 EMSVER。依赖名与符号名都走同一套注入闸。
//!
//! 闸门次序：依赖名 → 符号名 → 版本语法 → 标志 → VERNEED 块 → 依赖版本表 → 账本。
//!
//! 契约：纯 no_std（只用 alloc 的 String）、无 unsafe、无 unwrap/expect/
//! panic!，失败一律 `Result<_, u32>`；账本 14 格（8 成 6 败）逐格对常量表，
//! 冻结后登记落 ESEAL，重放必须复现同一指纹；语料 14 槽互异，覆盖率按
//! `(matched*1000 + total/2)/total` 四舍五入，单槽红档 929ppm。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11651）。
pub const SEG_BASE: u32 = 0xfda0;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 满档千分比。
pub const PPM_FULL: u32 = 1000;

/// 单槽红档千分比。
pub const PPM_RED: u32 = 929;

/// 字符串上限。
pub const TEXT_MAX: usize = 32;

/// VERNEED 缓冲上限。
pub const BLOB_MAX: usize = 512;

/// Need 条目上限。
pub const NEED_MAX: usize = 3;

/// 单个 Need 的 aux 上限。
pub const AUX_MAX: usize = 4;

/// 依赖对象上限。
pub const DEP_MAX: usize = 4;

/// 单个依赖的导出上限。
pub const EXP_MAX: usize = 8;

/// 版本串上限。
pub const VERSION_MAX: usize = 32;

/// 版本前缀上限。
pub const PREFIX_MAX: usize = 16;

/// 版本段上限。
pub const SEG_MAX: usize = 4;

/// 版本段最大值。
pub const SEG_LIMIT: u32 = 9999;

/// Need 头长度。
pub const NEED_HDR: usize = 16;

/// Vernaux 长度。
pub const AUX_SIZE: usize = 16;

/// VER_FLG_BASE（aux 里不允许）。
pub const VER_FLG_BASE: u16 = 0x1;

/// VER_FLG_WEAK。
pub const VER_FLG_WEAK: u16 = 0x2;

/// VER_FLG_INFO（aux 里不允许）。
pub const VER_FLG_INFO: u16 = 0x4;

/// 输入里有非法字节。
pub const VN_EBYTE: u32 = SEG_BASE + 0x01;

/// 名字为空或超长。
pub const VN_ENAME: u32 = SEG_BASE + 0x02;

/// 依赖缺失或依赖没有版本信息。
pub const VN_EDEP: u32 = SEG_BASE + 0x03;

/// 依赖里没有该符号。
pub const VN_ESYM: u32 = SEG_BASE + 0x04;

/// 版本元数据坏（flags、hash、偏移、链）。
pub const VN_EMETA: u32 = SEG_BASE + 0x05;

/// 符号在但版本不在。
pub const VN_EMSVER: u32 = SEG_BASE + 0x06;

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
// ELF 哈希
// ---------------------------------------------------------------------------

/// 经典 ELF 哈希（`_dl_elf_hash`）。
pub fn elf_hash(name: &str) -> u32 {
    let b = name.as_bytes();
    let mut h = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            h = (h << 4).wrapping_add(*c as u32);
            let g = h & 0xf000_0000;
            if g != 0 {
                h ^= g >> 24;
            }
            h &= !g;
        }
        i += 1;
    }
    h
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
    pub plen: usize,
    /// 段值。
    pub segs: [u32; SEG_MAX],
    /// 段数。
    pub n: usize,
}

impl Version {
    /// 空版本。
    pub const fn empty() -> Version {
        Version { prefix: [0u8; PREFIX_MAX], plen: 0, segs: [0u32; SEG_MAX], n: 0 }
    }

    /// 前缀文本。
    pub fn prefix_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.plen {
            if let Some(b) = self.prefix.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 规范文本。
    pub fn text(&self) -> String {
        let mut s = self.prefix_text();
        s.push('_');
        let mut i = 0usize;
        while i < self.n {
            if i > 0 {
                s.push('.');
            }
            let v = match self.segs.get(i) {
                Some(x) => *x,
                None => 0,
            };
            s.push_str(u32_text(v).as_str());
            i += 1;
        }
        s
    }

    /// 同前缀。
    pub fn same_prefix(&self, other: &Version) -> bool {
        if self.plen != other.plen {
            return false;
        }
        let mut i = 0usize;
        while i < self.plen {
            let a = match self.prefix.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let b = match other.prefix.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a != b {
                return false;
            }
            i += 1;
        }
        true
    }
}

fn ver_byte(b: u8) -> bool {
    (b >= b'A' && b <= b'Z')
        || (b >= b'a' && b <= b'z')
        || (b >= b'0' && b <= b'9')
        || b == b'_'
        || b == b'.'
}

/// 解析版本串。
pub fn parse_version(s: &str) -> Result<Version, u32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.len() > VERSION_MAX {
        return Err(VN_ENAME);
    }
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if !ver_byte(*b) {
                return Err(VN_EBYTE);
            }
        }
        i += 1;
    }
    let mut us = usize::MAX;
    i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if *b == b'_' {
                us = i;
                break;
            }
        }
        i += 1;
    }
    if us == usize::MAX || us == 0 || us > PREFIX_MAX {
        return Err(VN_EMSVER);
    }
    let mut v = Version::empty();
    i = 0usize;
    while i < us {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(VN_EMSVER),
        };
        let ok = if i == 0 {
            b >= b'A' && b <= b'Z'
        } else {
            (b >= b'A' && b <= b'Z') || (b >= b'0' && b <= b'9')
        };
        if !ok {
            return Err(VN_EMSVER);
        }
        if let Some(d) = v.prefix.get_mut(i) {
            *d = b;
        }
        v.plen += 1;
        i += 1;
    }
    let tail = match bytes.get(us + 1..) {
        Some(t) => t,
        None => return Err(VN_EMSVER),
    };
    if tail.is_empty() {
        return Err(VN_EMSVER);
    }
    let mut start = 0usize;
    let mut seg = 0usize;
    while start < tail.len() {
        if seg >= SEG_MAX {
            return Err(VN_EMSVER);
        }
        let mut end = start;
        while end < tail.len() {
            match tail.get(end) {
                Some(b) => {
                    if *b == b'.' {
                        break;
                    }
                }
                None => return Err(VN_EMSVER),
            }
            end += 1;
        }
        if end == start {
            return Err(VN_EMSVER);
        }
        let mut val = 0u32;
        let mut k = start;
        while k < end {
            let b = match tail.get(k) {
                Some(x) => *x,
                None => return Err(VN_EMSVER),
            };
            if b < b'0' || b > b'9' {
                return Err(VN_EMSVER);
            }
            val = val * 10 + ((b - b'0') as u32);
            if val > SEG_LIMIT {
                return Err(VN_EMSVER);
            }
            k += 1;
        }
        if let Some(d) = v.segs.get_mut(seg) {
            *d = val;
        }
        seg += 1;
        if end < tail.len() {
            if end + 1 >= tail.len() {
                return Err(VN_EMSVER);
            }
            start = end + 1;
        } else {
            start = tail.len();
        }
    }
    v.n = seg;
    if v.n == 0 {
        return Err(VN_EMSVER);
    }
    Ok(v)
}

/// 版本比较。
pub fn cmp_ver(a: &Version, b: &Version) -> i32 {
    let maxp = if a.plen > b.plen { a.plen } else { b.plen };
    let mut i = 0usize;
    while i < maxp {
        let x = match a.prefix.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let y = match b.prefix.get(i) {
            Some(v) => *v,
            None => 0,
        };
        if x != y {
            return if x < y { -1 } else { 1 };
        }
        i += 1;
    }
    let maxn = if a.n > b.n { a.n } else { b.n };
    i = 0usize;
    while i < maxn {
        let x = match a.segs.get(i) {
            Some(v) => *v,
            None => 0,
        };
        let y = match b.segs.get(i) {
            Some(v) => *v,
            None => 0,
        };
        if x != y {
            return if x < y { -1 } else { 1 };
        }
        i += 1;
    }
    0
}

/// `a` 不低于 `b`。
pub fn at_least(a: &Version, b: &Version) -> bool {
    cmp_ver(a, b) >= 0
}

// ---------------------------------------------------------------------------
// 名字闸
// ---------------------------------------------------------------------------

fn sym_byte(b: u8) -> bool {
    (b >= b'A' && b <= b'Z')
        || (b >= b'a' && b <= b'z')
        || (b >= b'0' && b <= b'9')
        || b == b'_'
        || b == b'.'
        || b == b'$'
}

fn name_byte(b: u8) -> bool {
    (b >= b'A' && b <= b'Z')
        || (b >= b'a' && b <= b'z')
        || (b >= b'0' && b <= b'9')
        || b == b'_'
        || b == b'.'
        || b == b'-'
        || b == b'+'
}

fn path_byte(b: u8) -> bool {
    name_byte(b) || b == b'/'
}

/// 符号名闸。
pub fn check_sym_name(s: &str) -> Result<(), u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > TEXT_MAX {
        return Err(VN_ENAME);
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(VN_ENAME),
        };
        if !sym_byte(c) {
            return Err(VN_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

/// 对象名闸。
pub fn check_soname(s: &str) -> Result<(), u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > TEXT_MAX {
        return Err(VN_ENAME);
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(VN_ENAME),
        };
        if !name_byte(c) {
            return Err(VN_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

/// 路径闸。
pub fn check_path(s: &str) -> Result<(), u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 96 {
        return Err(VN_ENAME);
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(VN_ENAME),
        };
        if !path_byte(c) {
            return Err(VN_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 定长文本
// ---------------------------------------------------------------------------

/// 定长文本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Text {
    buf: [u8; TEXT_MAX],
    len: usize,
}

impl Text {
    /// 空串。
    pub const fn empty() -> Text {
        Text { buf: [0u8; TEXT_MAX], len: 0 }
    }

    /// 从 `&str` 复制。
    pub fn from_str(s: &str) -> Result<Text, u32> {
        let b = s.as_bytes();
        if b.len() > TEXT_MAX {
            return Err(VN_ENAME);
        }
        let mut t = Text::empty();
        let mut i = 0usize;
        while i < b.len() {
            if let Some(d) = t.buf.get_mut(i) {
                if let Some(c) = b.get(i) {
                    *d = *c;
                    t.len += 1;
                }
            }
            i += 1;
        }
        Ok(t)
    }

    /// 渲染。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 与 `&str` 比较。
    pub fn eq_str(&self, s: &str) -> bool {
        let b = s.as_bytes();
        if b.len() != self.len {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            let x = match self.buf.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let y = match b.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if x != y {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// ---------------------------------------------------------------------------
// VERNEED 表
// ---------------------------------------------------------------------------

/// Vernaux 种子。
#[derive(Clone, Copy, Debug)]
pub struct AuxSeed<'a> {
    /// 符号名。
    pub name: &'a str,
    /// 版本。
    pub ver: &'a str,
    /// 标志。
    pub flags: u16,
}

/// Need 种子。
#[derive(Clone, Copy, Debug)]
pub struct NeedSeed<'a> {
    /// 依赖文件（soname）。
    pub file: &'a str,
    /// aux 列表。
    pub aux: &'a [AuxSeed<'a>],
}

/// 解析后的 Vernaux。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aux {
    /// 符号名。
    pub name: Text,
    /// 版本。
    pub ver: Text,
    /// 标志。
    pub flags: u16,
    /// 版本串 ELF 哈希。
    pub hash: u32,
    /// 序号（非零）。
    pub other: u16,
}

impl Aux {
    /// 空。
    pub const fn empty() -> Aux {
        Aux { name: Text::empty(), ver: Text::empty(), flags: 0, hash: 0, other: 0 }
    }
}

/// 解析后的 Need。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeedDep {
    /// 文件。
    pub file: Text,
    /// aux 槽。
    pub aux: [Aux; AUX_MAX],
    /// aux 数。
    pub alen: usize,
}

impl NeedDep {
    /// 空。
    pub const fn empty() -> NeedDep {
        NeedDep { file: Text::empty(), aux: [Aux::empty(); AUX_MAX], alen: 0 }
    }

    /// 取 aux。
    pub fn aux_at(&self, i: usize) -> Option<Aux> {
        if i >= self.alen {
            return None;
        }
        self.aux.get(i).copied()
    }
}

/// VERNEED 表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeedTable {
    deps: [NeedDep; NEED_MAX],
    len: usize,
}

impl NeedTable {
    /// 空表。
    pub const fn empty() -> NeedTable {
        NeedTable { deps: [NeedDep::empty(); NEED_MAX], len: 0 }
    }

    /// 从种子校验构造。
    pub fn build(seeds: &[NeedSeed<'_>]) -> Result<NeedTable, u32> {
        if seeds.is_empty() || seeds.len() > NEED_MAX {
            return Err(VN_EDEP);
        }
        let mut t = NeedTable::empty();
        let mut i = 0usize;
        while i < seeds.len() {
            let s = match seeds.get(i) {
                Some(v) => v,
                None => return Err(VN_EDEP),
            };
            check_soname(s.file)?;
            if s.aux.is_empty() || s.aux.len() > AUX_MAX {
                return Err(VN_ENAME);
            }
            let mut dep = NeedDep::empty();
            dep.file = Text::from_str(s.file)?;
            let mut ai = 0usize;
            while ai < s.aux.len() {
                let a = match s.aux.get(ai) {
                    Some(v) => *v,
                    None => return Err(VN_EMETA),
                };
                check_sym_name(a.name)?;
                parse_version(a.ver)?;
                if a.flags & !VER_FLG_WEAK != 0 {
                    return Err(VN_EMETA);
                }
                let mut j = 0usize;
                while j < ai {
                    if let Some(p) = dep.aux.get(j) {
                        if p.name.eq_str(a.name) && p.ver.eq_str(a.ver) {
                            return Err(VN_ESYM);
                        }
                    }
                    j += 1;
                }
                let aux = Aux {
                    name: Text::from_str(a.name)?,
                    ver: Text::from_str(a.ver)?,
                    flags: a.flags,
                    hash: elf_hash(a.ver),
                    other: (ai + 1) as u16,
                };
                if let Some(dst) = dep.aux.get_mut(ai) {
                    *dst = aux;
                }
                ai += 1;
            }
            dep.alen = ai;
            if let Some(dst) = t.deps.get_mut(i) {
                *dst = dep;
            }
            t.len = i + 1;
            i += 1;
        }
        Ok(t)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取依赖。
    pub fn dep(&self, i: usize) -> Option<&NeedDep> {
        if i >= self.len {
            return None;
        }
        self.deps.get(i)
    }

    /// 取 aux。
    pub fn aux(&self, di: usize, ai: usize) -> Option<Aux> {
        match self.dep(di) {
            Some(d) => d.aux_at(ai),
            None => None,
        }
    }

    /// 序列化到定长缓冲。
    pub fn blob(&self) -> Result<[u8; BLOB_MAX], u32> {
        let mut img = [0u8; BLOB_MAX];
        let mut hdrs = [0usize; NEED_MAX];
        let mut cur = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let dep = match self.deps.get(i) {
                Some(v) => v,
                None => return Err(VN_EMETA),
            };
            let base = cur;
            if base + NEED_HDR > BLOB_MAX {
                return Err(VN_EMETA);
            }
            hdrs[i] = base;
            let cnt = dep.alen;
            let aux_base = base + NEED_HDR;
            if aux_base + cnt * AUX_SIZE > BLOB_MAX {
                return Err(VN_EMETA);
            }
            cur = aux_base + cnt * AUX_SIZE;
            let file_off = append_str(&mut img, &mut cur, dep.file.text().as_str())?;
            let mut name_offs = [0usize; AUX_MAX];
            let mut ai = 0usize;
            while ai < cnt {
                let a = match dep.aux.get(ai) {
                    Some(v) => *v,
                    None => return Err(VN_EMETA),
                };
                name_offs[ai] = append_str(&mut img, &mut cur, a.name.text().as_str())?;
                let _ = append_str(&mut img, &mut cur, a.ver.text().as_str())?;
                ai += 1;
            }
            put16(&mut img, base, 1)?;
            put16(&mut img, base + 2, cnt as u16)?;
            put32(&mut img, base + 4, file_off as u32)?;
            put32(&mut img, base + 8, aux_base as u32)?;
            put32(&mut img, base + 12, 0)?;
            ai = 0usize;
            while ai < cnt {
                let a = match dep.aux.get(ai) {
                    Some(v) => *v,
                    None => return Err(VN_EMETA),
                };
                let off = aux_base + ai * AUX_SIZE;
                put32(&mut img, off, a.hash)?;
                put16(&mut img, off + 4, a.flags)?;
                put16(&mut img, off + 6, a.other)?;
                put32(&mut img, off + 8, name_offs[ai] as u32)?;
                let next = if ai + 1 < cnt { (off + AUX_SIZE) as u32 } else { 0 };
                put32(&mut img, off + 12, next)?;
                ai += 1;
            }
            i += 1;
        }
        i = 0usize;
        while i + 1 < self.len {
            put32(&mut img, hdrs[i] + 12, hdrs[i + 1] as u32)?;
            i += 1;
        }
        Ok(img)
    }

    /// 从缓冲解析。
    pub fn parse_blob(img: &[u8]) -> Result<NeedTable, u32> {
        let mut t = NeedTable::empty();
        let mut off = 0usize;
        let mut i = 0usize;
        loop {
            if i >= NEED_MAX {
                return Err(VN_EMETA);
            }
            if off + NEED_HDR > img.len() {
                return Err(VN_EMETA);
            }
            let version = read16(img, off)?;
            if version != 1 {
                return Err(VN_EMETA);
            }
            let cnt = read16(img, off + 2)? as usize;
            if cnt == 0 || cnt > AUX_MAX {
                return Err(VN_EMETA);
            }
            let file_off = read32(img, off + 4)? as usize;
            let aux_off = read32(img, off + 8)? as usize;
            let next = read32(img, off + 12)? as usize;
            if aux_off < off + NEED_HDR || aux_off + cnt * AUX_SIZE > img.len() {
                return Err(VN_EMETA);
            }
            let (file_s, _) = text_at(img, file_off)?;
            check_soname(file_s.as_str())?;
            let mut dep = NeedDep::empty();
            dep.file = Text::from_str(file_s.as_str())?;
            let mut ai = 0usize;
            let mut cursor = aux_off;
            while ai < cnt {
                if cursor + AUX_SIZE > img.len() {
                    return Err(VN_EMETA);
                }
                let hash = read32(img, cursor)?;
                let flags = read16(img, cursor + 4)?;
                let other = read16(img, cursor + 6)?;
                let name_off = read32(img, cursor + 8)? as usize;
                let aux_next = read32(img, cursor + 12)? as usize;
                if flags & !VER_FLG_WEAK != 0 || other == 0 {
                    return Err(VN_EMETA);
                }
                let (ns, voff) = text_at(img, name_off)?;
                let (vs, _) = text_at(img, voff)?;
                check_sym_name(ns.as_str())?;
                parse_version(vs.as_str())?;
                if hash != elf_hash(vs.as_str()) {
                    return Err(VN_EMETA);
                }
                let expected = if ai + 1 < cnt { cursor + AUX_SIZE } else { 0 };
                if aux_next != expected {
                    return Err(VN_EMETA);
                }
                let mut j = 0usize;
                while j < ai {
                    if let Some(p) = dep.aux.get(j) {
                        if p.name.eq_str(ns.as_str()) && p.ver.eq_str(vs.as_str()) {
                            return Err(VN_ESYM);
                        }
                    }
                    j += 1;
                }
                let aux = Aux {
                    name: Text::from_str(ns.as_str())?,
                    ver: Text::from_str(vs.as_str())?,
                    flags,
                    hash,
                    other,
                };
                if let Some(dst) = dep.aux.get_mut(ai) {
                    *dst = aux;
                }
                cursor = cursor + AUX_SIZE;
                ai += 1;
            }
            dep.alen = ai;
            if let Some(dst) = t.deps.get_mut(i) {
                *dst = dep;
            }
            t.len = i + 1;
            if next == 0 {
                break;
            }
            if next <= off || next + NEED_HDR > img.len() {
                return Err(VN_EMETA);
            }
            off = next;
            i += 1;
        }
        Ok(t)
    }
}

fn put16(img: &mut [u8; BLOB_MAX], off: usize, v: u16) -> Result<(), u32> {
    if off + 2 > BLOB_MAX {
        return Err(VN_EMETA);
    }
    if let Some(d) = img.get_mut(off) {
        *d = ((v >> 8) & 0xff) as u8;
    }
    if let Some(d) = img.get_mut(off + 1) {
        *d = (v & 0xff) as u8;
    }
    Ok(())
}

fn put32(img: &mut [u8; BLOB_MAX], off: usize, v: u32) -> Result<(), u32> {
    if off + 4 > BLOB_MAX {
        return Err(VN_EMETA);
    }
    let mut i = 0usize;
    while i < 4 {
        if let Some(d) = img.get_mut(off + i) {
            *d = ((v >> (24 - i * 8)) & 0xff) as u8;
        }
        i += 1;
    }
    Ok(())
}

fn append_str(img: &mut [u8; BLOB_MAX], at: &mut usize, s: &str) -> Result<usize, u32> {
    let b = s.as_bytes();
    let off = *at;
    if off + b.len() + 1 > BLOB_MAX {
        return Err(VN_EMETA);
    }
    let mut i = 0usize;
    while i < b.len() {
        if let Some(d) = img.get_mut(off + i) {
            if let Some(c) = b.get(i) {
                *d = *c;
            }
        }
        i += 1;
    }
    if let Some(d) = img.get_mut(off + b.len()) {
        *d = 0;
    }
    *at = off + b.len() + 1;
    Ok(off)
}

fn read16(img: &[u8], off: usize) -> Result<u16, u32> {
    let a = match img.get(off) {
        Some(v) => *v,
        None => return Err(VN_EMETA),
    };
    let b = match img.get(off + 1) {
        Some(v) => *v,
        None => return Err(VN_EMETA),
    };
    Ok(((a as u16) << 8) | (b as u16))
}

fn read32(img: &[u8], off: usize) -> Result<u32, u32> {
    let mut v = 0u32;
    let mut i = 0usize;
    while i < 4 {
        let b = match img.get(off + i) {
            Some(x) => *x,
            None => return Err(VN_EMETA),
        };
        v = (v << 8) | (b as u32);
        i += 1;
    }
    Ok(v)
}

/// 读 NUL 结尾串，返回串与下一偏移。
fn text_at(img: &[u8], off: usize) -> Result<(String, usize), u32> {
    let mut s = String::new();
    let mut p = off;
    let mut n = 0usize;
    loop {
        let b = match img.get(p) {
            Some(v) => *v,
            None => return Err(VN_EMETA),
        };
        if b == 0 {
            return Ok((s, p + 1));
        }
        if n >= TEXT_MAX || b > 0x7f {
            return Err(VN_EMETA);
        }
        s.push(b as char);
        n += 1;
        p += 1;
    }
}

// ---------------------------------------------------------------------------
// 依赖表
// ---------------------------------------------------------------------------

/// 依赖导出。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Export {
    /// 符号。
    pub name: &'static str,
    /// 版本。
    pub ver: &'static str,
}

/// 依赖对象。
#[derive(Clone, Copy, Debug)]
pub struct Dep {
    /// soname。
    pub name: &'static str,
    /// 路径。
    pub path: &'static str,
    /// 是否有版本节。
    pub verdef: bool,
    /// 导出表。
    pub exports: &'static [Export],
}

impl Dep {
    /// 空。
    pub const fn empty() -> Dep {
        Dep { name: "", path: "", verdef: false, exports: &[] }
    }

    /// 精确命中下标。
    pub fn exact_index(&self, name: &str, ver: &str) -> Option<usize> {
        if !self.verdef {
            return None;
        }
        let mut i = 0usize;
        while i < self.exports.len() {
            if let Some(e) = self.exports.get(i) {
                if e.name == name && e.ver == ver {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 是否有某名字。
    pub fn has_name(&self, name: &str) -> bool {
        let mut i = 0usize;
        while i < self.exports.len() {
            if let Some(e) = self.exports.get(i) {
                if e.name == name {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

/// 依赖表。
#[derive(Clone, Copy, Debug)]
pub struct DepTable {
    deps: [Dep; DEP_MAX],
    len: usize,
}

impl DepTable {
    /// 空。
    pub const fn empty() -> DepTable {
        DepTable { deps: [Dep::empty(); DEP_MAX], len: 0 }
    }

    /// 从依赖列表建表。
    pub fn new(seeds: &[Dep]) -> Result<DepTable, u32> {
        if seeds.is_empty() || seeds.len() > DEP_MAX {
            return Err(VN_EDEP);
        }
        let mut t = DepTable::empty();
        let mut i = 0usize;
        while i < seeds.len() {
            let d = match seeds.get(i) {
                Some(v) => *v,
                None => return Err(VN_EDEP),
            };
            check_soname(d.name)?;
            check_path(d.path)?;
            if d.exports.is_empty() || d.exports.len() > EXP_MAX {
                return Err(VN_ENAME);
            }
            let mut j = 0usize;
            while j < d.exports.len() {
                let e = match d.exports.get(j) {
                    Some(v) => *v,
                    None => return Err(VN_ESYM),
                };
                check_sym_name(e.name)?;
                if d.verdef {
                    parse_version(e.ver)?;
                } else if !e.ver.is_empty() {
                    return Err(VN_EDEP);
                }
                let mut k = 0usize;
                while k < j {
                    if let Some(p) = d.exports.get(k) {
                        if p.name == e.name && p.ver == e.ver {
                            return Err(VN_ESYM);
                        }
                    }
                    k += 1;
                }
                j += 1;
            }
            let mut k = 0usize;
            while k < i {
                if let Some(p) = t.deps.get(k) {
                    if p.name == d.name {
                        return Err(VN_ESYM);
                    }
                }
                k += 1;
            }
            if let Some(dst) = t.deps.get_mut(i) {
                *dst = d;
            }
            t.len = i + 1;
            i += 1;
        }
        Ok(t)
    }

    /// 固定表。
    pub fn fixed() -> DepTable {
        match DepTable::new(&FIXED_DEPS) {
            Ok(t) => t,
            Err(_) => DepTable::empty(),
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取依赖。
    pub fn dep(&self, i: usize) -> Option<&Dep> {
        if i >= self.len {
            return None;
        }
        self.deps.get(i)
    }

    /// 按名字找。
    pub fn find(&self, name: &str) -> Option<&Dep> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = self.deps.get(i) {
                if d.name == name {
                    return Some(d);
                }
            }
            i += 1;
        }
        None
    }
}

static LIBC_EXPORTS: [Export; 8] = [
    Export { name: "malloc", ver: "GLIBC_2.2.5" },
    Export { name: "free", ver: "GLIBC_2.2.5" },
    Export { name: "memcpy", ver: "GLIBC_2.2.5" },
    Export { name: "memcpy", ver: "GLIBC_2.14" },
    Export { name: "clock_gettime", ver: "GLIBC_2.2.5" },
    Export { name: "clock_gettime", ver: "GLIBC_2.17" },
    Export { name: "pthread_create", ver: "GLIBC_2.34" },
    Export { name: "dlvsym", ver: "GLIBC_2.34" },
];

static LIBM_EXPORTS: [Export; 4] = [
    Export { name: "sin", ver: "GLIBC_2.2.5" },
    Export { name: "cos", ver: "GLIBC_2.2.5" },
    Export { name: "exp", ver: "GLIBC_2.29" },
    Export { name: "log", ver: "GLIBC_2.29" },
];

static SSL_EXPORTS: [Export; 2] = [
    Export { name: "SSL_new", ver: "OPENSSL_3.0.0" },
    Export { name: "SSL_CTX_new", ver: "OPENSSL_3.0.0" },
];

static FOO_EXPORTS: [Export; 2] = [
    Export { name: "foo_a", ver: "" },
    Export { name: "foo_b", ver: "" },
];

static FIXED_DEPS: [Dep; 4] = [
    Dep {
        name: "libc.so.6",
        path: "/lib64/libc.so.6",
        verdef: true,
        exports: &LIBC_EXPORTS,
    },
    Dep {
        name: "libm.so.6",
        path: "/lib64/libm.so.6",
        verdef: true,
        exports: &LIBM_EXPORTS,
    },
    Dep {
        name: "libssl.so.3",
        path: "/usr/lib/libssl.so.3",
        verdef: true,
        exports: &SSL_EXPORTS,
    },
    Dep {
        name: "libfoo.so.1",
        path: "/opt/lib/libfoo.so.1",
        verdef: false,
        exports: &FOO_EXPORTS,
    },
];

static EMPTY_EXPORTS: [Export; 0] = [];
static VERSIONED_ONLY: [Export; 1] = [Export { name: "x", ver: "GLIBC_2.2.5" }];
static DUP_EXPORTS: [Export; 2] = [
    Export { name: "x", ver: "GLIBC_2.2.5" },
    Export { name: "x", ver: "GLIBC_2.2.5" },
];

static BAD_DEP_EMPTY: [Dep; 1] = [Dep {
    name: "bad.so",
    path: "/x/bad.so",
    verdef: true,
    exports: &EMPTY_EXPORTS,
}];

static BAD_DEP_NOVER: [Dep; 1] = [Dep {
    name: "bad.so",
    path: "/x/bad.so",
    verdef: false,
    exports: &VERSIONED_ONLY,
}];

static BAD_DEP_DUP: [Dep; 1] = [Dep {
    name: "bad.so",
    path: "/x/bad.so",
    verdef: true,
    exports: &DUP_EXPORTS,
}];

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// 解析结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// 依赖名。
    pub dep: String,
    /// 版本。
    pub ver: String,
    /// 弱需求放行。
    pub weak: bool,
    /// 弱诊断文案。
    pub text: String,
}

/// 弱符号文案。
pub fn weak_text(dep: &str, sym: &str, ver: &str) -> String {
    let mut s = String::from("weak symbol ");
    s.push_str(sym);
    s.push_str(", version ");
    s.push_str(ver);
    s.push_str(" not found (required by ");
    s.push_str(dep);
    s.push_str(")\n");
    s
}

/// 解析一条 VERNEED 需求。
pub fn resolve(
    tab: &DepTable,
    dep_name: &str,
    sym: &str,
    ver: &str,
    flags: u16,
) -> Result<Found, u32> {
    check_soname(dep_name)?;
    check_sym_name(sym)?;
    parse_version(ver)?;
    if flags & !VER_FLG_WEAK != 0 {
        return Err(VN_EMETA);
    }
    let aux = [AuxSeed { name: sym, ver, flags }];
    let seeds = [NeedSeed { file: dep_name, aux: &aux }];
    let nt = NeedTable::build(&seeds)?;
    let blob = nt.blob()?;
    let parsed = NeedTable::parse_blob(&blob)?;
    let a = match parsed.aux(0, 0) {
        Some(v) => v,
        None => return Err(VN_EMETA),
    };
    if !a.name.eq_str(sym) || !a.ver.eq_str(ver) || a.flags != flags {
        return Err(VN_EMETA);
    }
    if a.hash != elf_hash(ver) {
        return Err(VN_EMETA);
    }
    let d = match tab.find(dep_name) {
        Some(v) => v,
        None => return Err(VN_EDEP),
    };
    if !d.verdef {
        return Err(VN_EDEP);
    }
    if let Some(ei) = d.exact_index(sym, ver) {
        let ev = match d.exports.get(ei) {
            Some(e) => e.ver,
            None => "",
        };
        return Ok(Found {
            dep: String::from(d.name),
            ver: String::from(ev),
            weak: false,
            text: String::new(),
        });
    }
    if flags & VER_FLG_WEAK != 0 {
        return Ok(Found {
            dep: String::from(d.name),
            ver: String::from(ver),
            weak: true,
            text: weak_text(dep_name, sym, ver),
        });
    }
    if d.has_name(sym) {
        Err(VN_EMSVER)
    } else {
        Err(VN_ESYM)
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 强命中。
    Hit,
    /// 弱放行。
    Weak,
    /// 符号不存在。
    Sym,
    /// 版本不存在。
    Ver,
    /// 依赖缺失。
    Dep,
    /// 元数据坏。
    Meta,
}

/// 探针。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// 一条需求。
    Need {
        /// 依赖名。
        dep: &'static str,
        /// 符号。
        sym: &'static str,
        /// 版本。
        ver: &'static str,
        /// 标志。
        flags: u16,
    },
}

/// 期望。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// 命中。
    Found {
        /// 依赖。
        dep: &'static str,
        /// 版本。
        ver: &'static str,
        /// 弱。
        weak: bool,
    },
    /// 错误码。
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
        "libc pthread_create GLIBC_2.34",
        Face::Hit,
        Probe::Need { dep: "libc.so.6", sym: "pthread_create", ver: "GLIBC_2.34", flags: 0 },
        Want::Found { dep: "libc.so.6", ver: "GLIBC_2.34", weak: false },
        "hit-new",
    ),
    Case::new(
        "libc memcpy GLIBC_2.2.5",
        Face::Hit,
        Probe::Need { dep: "libc.so.6", sym: "memcpy", ver: "GLIBC_2.2.5", flags: 0 },
        Want::Found { dep: "libc.so.6", ver: "GLIBC_2.2.5", weak: false },
        "hit-compat",
    ),
    Case::new(
        "libm exp GLIBC_2.29",
        Face::Hit,
        Probe::Need { dep: "libm.so.6", sym: "exp", ver: "GLIBC_2.29", flags: 0 },
        Want::Found { dep: "libm.so.6", ver: "GLIBC_2.29", weak: false },
        "hit-libm",
    ),
    Case::new(
        "libssl SSL_new OPENSSL_3.0.0",
        Face::Hit,
        Probe::Need { dep: "libssl.so.3", sym: "SSL_new", ver: "OPENSSL_3.0.0", flags: 0 },
        Want::Found { dep: "libssl.so.3", ver: "OPENSSL_3.0.0", weak: false },
        "hit-ssl",
    ),
    Case::new(
        "libc clock_gettime GLIBC_2.17",
        Face::Hit,
        Probe::Need { dep: "libc.so.6", sym: "clock_gettime", ver: "GLIBC_2.17", flags: 0 },
        Want::Found { dep: "libc.so.6", ver: "GLIBC_2.17", weak: false },
        "hit-clock",
    ),
    Case::new(
        "libm log GLIBC_2.29",
        Face::Hit,
        Probe::Need { dep: "libm.so.6", sym: "log", ver: "GLIBC_2.29", flags: 0 },
        Want::Found { dep: "libm.so.6", ver: "GLIBC_2.29", weak: false },
        "hit-log",
    ),
    Case::new(
        "libc missing_sym GLIBC_2.34 weak",
        Face::Weak,
        Probe::Need {
            dep: "libc.so.6",
            sym: "missing_sym",
            ver: "GLIBC_2.34",
            flags: VER_FLG_WEAK,
        },
        Want::Found { dep: "libc.so.6", ver: "GLIBC_2.34", weak: true },
        "weak-miss",
    ),
    Case::new(
        "libc free GLIBC_2.2.5",
        Face::Hit,
        Probe::Need { dep: "libc.so.6", sym: "free", ver: "GLIBC_2.2.5", flags: 0 },
        Want::Found { dep: "libc.so.6", ver: "GLIBC_2.2.5", weak: false },
        "hit-free",
    ),
    Case::new(
        "libc no_such_symbol GLIBC_2.2.5",
        Face::Sym,
        Probe::Need { dep: "libc.so.6", sym: "no_such_symbol", ver: "GLIBC_2.2.5", flags: 0 },
        Want::Code(VN_ESYM),
        "miss-sym-libc",
    ),
    Case::new(
        "libm no_such_symbol GLIBC_2.29",
        Face::Sym,
        Probe::Need { dep: "libm.so.6", sym: "no_such_symbol", ver: "GLIBC_2.29", flags: 0 },
        Want::Code(VN_ESYM),
        "miss-sym-libm",
    ),
    Case::new(
        "libc malloc GLIBC_2.99",
        Face::Ver,
        Probe::Need { dep: "libc.so.6", sym: "malloc", ver: "GLIBC_2.99", flags: 0 },
        Want::Code(VN_EMSVER),
        "miss-ver",
    ),
    Case::new(
        "libz foo GLIBC_2.2.5",
        Face::Dep,
        Probe::Need { dep: "libz.so.1", sym: "foo", ver: "GLIBC_2.2.5", flags: 0 },
        Want::Code(VN_EDEP),
        "miss-dep",
    ),
    Case::new(
        "libc malloc GLIBC_2.2.5 flags INFO",
        Face::Meta,
        Probe::Need { dep: "libc.so.6", sym: "malloc", ver: "GLIBC_2.2.5", flags: VER_FLG_INFO },
        Want::Code(VN_EMETA),
        "bad-flags",
    ),
    Case::new(
        "libc bad name GLIBC_2.2.5",
        Face::Meta,
        Probe::Need { dep: "libc.so.6", sym: "bad name", ver: "GLIBC_2.2.5", flags: 0 },
        Want::Code(VN_EBYTE),
        "bad-sym-bytes",
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
pub fn judge(p: Probe) -> Result<Found, u32> {
    let tab = DepTable::fixed();
    match p {
        Probe::Need { dep, sym, ver, flags } => resolve(&tab, dep, sym, ver, flags),
    }
}

fn want_match(w: Want, got: &Result<Found, u32>) -> bool {
    match w {
        Want::Found { dep, ver, weak } => match got {
            Ok(f) => f.dep == dep && f.ver == ver && f.weak == weak,
            Err(_) => false,
        },
        Want::Code(code) => match got {
            Ok(_) => false,
            Err(c) => *c == code,
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
        if !want_match(c.want, &got) {
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
    /// 符号类。
    pub sym: u32,
    /// 版本类。
    pub ver: u32,
    /// 依赖类。
    pub dep: u32,
    /// 其他类。
    pub other: u32,
    /// 命中数。
    pub matched: u32,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census { total: 0, accept: 0, reject: 0, sym: 0, ver: 0, dep: 0, other: 0, matched: 0 }
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
        let buckets = self
            .sym
            .saturating_add(self.ver)
            .saturating_add(self.dep)
            .saturating_add(self.other);
        if buckets != self.reject {
            return Err(VN_EVALUE);
        }
        Ok(())
    }

    fn bump(&mut self, code: u32) {
        match code {
            VN_ESYM => self.sym = self.sym.saturating_add(1),
            VN_EMSVER => self.ver = self.ver.saturating_add(1),
            VN_EDEP => self.dep = self.dep.saturating_add(1),
            _ => self.other = self.other.saturating_add(1),
        }
    }

    /// 固定渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("vn corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" sym=");
        s.push_str(u32_text(self.sym).as_str());
        s.push_str(" ver=");
        s.push_str(u32_text(self.ver).as_str());
        s.push_str(" dep=");
        s.push_str(u32_text(self.dep).as_str());
        s.push_str(" other=");
        s.push_str(u32_text(self.other).as_str());
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
        match &got {
            Ok(_) => c.accept = c.accept.saturating_add(1),
            Err(code) => {
                c.reject = c.reject.saturating_add(1);
                c.bump(*code);
            }
        }
        let hit = want_match(cs.want, &got);
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

/// 强弱需求条数。
pub fn weak_census() -> (u32, u32) {
    let mut strong = 0u32;
    let mut weak = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            match c.probe {
                Probe::Need { flags, .. } => {
                    if flags & VER_FLG_WEAK != 0 {
                        weak = weak.saturating_add(1);
                    } else {
                        strong = strong.saturating_add(1);
                    }
                }
            }
        }
        i += 1;
    }
    (strong, weak)
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
        const VALS: [u32; 8] = [14, 8, 6, 2, 1, 1, 2, 929];
        const CODES: [u32; 6] =
            [VN_ESYM, VN_ESYM, VN_EMSVER, VN_EDEP, VN_EMETA, VN_EBYTE];
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
const OP_SYM: u8 = 3;
const OP_VER: u8 = 4;
const OP_DEP: u8 = 5;
const OP_OTHER: u8 = 6;
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

fn code_of(r: Result<Found, u32>, fallback: u32) -> u32 {
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
    let _ = l.register(count_cell(3, OP_SYM, r.map(|x| x.sym as usize)));
    let _ = l.register(count_cell(4, OP_VER, r.map(|x| x.ver as usize)));
    let _ = l.register(count_cell(5, OP_DEP, r.map(|x| x.dep as usize)));
    let _ = l.register(count_cell(6, OP_OTHER, r.map(|x| x.other as usize)));
    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_RATE, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(
        8,
        OP_QUERY,
        code_of(
            judge(Probe::Need {
                dep: "libc.so.6",
                sym: "no_such_symbol",
                ver: "GLIBC_2.2.5",
                flags: 0,
            }),
            VN_ESYM,
        ),
    ));
    let _ = l.register(err_cell(
        9,
        OP_QUERY,
        code_of(
            judge(Probe::Need {
                dep: "libm.so.6",
                sym: "no_such_symbol",
                ver: "GLIBC_2.29",
                flags: 0,
            }),
            VN_ESYM,
        ),
    ));
    let _ = l.register(err_cell(
        10,
        OP_QUERY,
        code_of(
            judge(Probe::Need {
                dep: "libc.so.6",
                sym: "malloc",
                ver: "GLIBC_2.99",
                flags: 0,
            }),
            VN_EMSVER,
        ),
    ));
    let _ = l.register(err_cell(
        11,
        OP_QUERY,
        code_of(
            judge(Probe::Need {
                dep: "libz.so.1",
                sym: "foo",
                ver: "GLIBC_2.2.5",
                flags: 0,
            }),
            VN_EDEP,
        ),
    ));
    let _ = l.register(err_cell(
        12,
        OP_QUERY,
        code_of(
            judge(Probe::Need {
                dep: "libc.so.6",
                sym: "malloc",
                ver: "GLIBC_2.2.5",
                flags: VER_FLG_INFO,
            }),
            VN_EMETA,
        ),
    ));
    let _ = l.register(err_cell(
        13,
        OP_QUERY,
        code_of(
            judge(Probe::Need {
                dep: "libc.so.6",
                sym: "bad name",
                ver: "GLIBC_2.2.5",
                flags: 0,
            }),
            VN_EBYTE,
        ),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// ELF 哈希。
fn hash_ok() -> bool {
    let a = elf_hash("GLIBC_2.2.5") == 0x0969_1a75;
    let b = elf_hash("GLIBC_2.34") == 0x0696_91b4;
    let c = elf_hash("GLIBC_2.17") == 0x0696_9197;
    let d = elf_hash("GLIBC_2.4") == 0x0d69_6914;
    let e = elf_hash("") == 0 && elf_hash("a") == 0x61;
    let f = elf_hash("GLIBC_2.34") != elf_hash("GLIBC_2.17");
    a && b && c && d && e && f
}

/// 版本语法。
fn version_ok() -> bool {
    let a = parse_version("GLIBC_2.2.5").map(|v| v.text()) == Ok(String::from("GLIBC_2.2.5"));
    let b = parse_version("OPENSSL_3.0.0").map(|v| v.text())
        == Ok(String::from("OPENSSL_3.0.0"));
    let c = parse_version("").err() == Some(VN_ENAME);
    let d = parse_version("GLIBC_2.2.5\n").err() == Some(VN_EBYTE);
    let e = parse_version("GLIBC_").err() == Some(VN_EMSVER);
    let f = parse_version("GLIBC_1.2.3.4.5").err() == Some(VN_EMSVER);
    let g = parse_version("GLIBC_2.x").err() == Some(VN_EMSVER);
    a && b && c && d && e && f && g
}

/// 名字闸。
fn names_ok() -> bool {
    let a = check_sym_name("malloc").is_ok() && check_sym_name("SSL_new").is_ok();
    let b = check_sym_name("").err() == Some(VN_ENAME);
    let c = check_sym_name("a b").err() == Some(VN_EBYTE);
    let d = check_sym_name("a;b").err() == Some(VN_EBYTE);
    let e = check_soname("libc.so.6").is_ok() && check_soname("libfoo-1.2.so").is_ok();
    let f = check_soname("bad name").err() == Some(VN_EBYTE);
    let g = check_path("/lib64/libc.so.6").is_ok();
    let h = check_path("/x/`y`").err() == Some(VN_EBYTE);
    a && b && c && d && e && f && g && h
}

/// 依赖表。
fn deps_ok() -> bool {
    let t = DepTable::fixed();
    let a = t.len() == 4 && !t.is_empty() && t.dep(4).is_none();
    let b = match t.find("libc.so.6") {
        Some(d) => {
            d.verdef
                && d.exports.len() == 8
                && d.exact_index("memcpy", "GLIBC_2.14").is_some()
                && d.has_name("pthread_create")
        }
        None => false,
    };
    let c = match t.find("libfoo.so.1") {
        Some(d) => !d.verdef && d.has_name("foo_a"),
        None => false,
    };
    let d = DepTable::new(&BAD_DEP_EMPTY).err() == Some(VN_ENAME);
    let e = DepTable::new(&BAD_DEP_NOVER).err() == Some(VN_EDEP);
    let f = DepTable::new(&BAD_DEP_DUP).err() == Some(VN_ESYM);
    let two = [FIXED_DEPS[0], FIXED_DEPS[0]];
    let g = DepTable::new(&two).err() == Some(VN_ESYM);
    let h = DepTable::new(&[]).err() == Some(VN_EDEP);
    a && b && c && d && e && f && g && h
}

/// VERNEED 构建与回读。
fn build_ok() -> bool {
    let aux = [
        AuxSeed { name: "malloc", ver: "GLIBC_2.2.5", flags: 0 },
        AuxSeed { name: "memcpy", ver: "GLIBC_2.14", flags: VER_FLG_WEAK },
    ];
    let seeds = [NeedSeed { file: "libc.so.6", aux: &aux }];
    let t = match NeedTable::build(&seeds) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = t.len() == 1 && !t.is_empty();
    let b = match t.dep(0) {
        Some(d) => d.alen == 2 && d.file.eq_str("libc.so.6"),
        None => false,
    };
    let c = match t.aux(0, 0) {
        Some(x) => {
            x.name.eq_str("malloc")
                && x.ver.eq_str("GLIBC_2.2.5")
                && x.hash == elf_hash("GLIBC_2.2.5")
                && x.other == 1
                && x.flags == 0
        }
        None => false,
    };
    let d = match t.aux(0, 1) {
        Some(x) => x.flags == VER_FLG_WEAK && x.hash == elf_hash("GLIBC_2.14") && x.other == 2,
        None => false,
    };
    let e = t.aux(0, 2).is_none() && t.dep(1).is_none();
    let blob = match t.blob() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let p = match NeedTable::parse_blob(&blob) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let f = p == t;
    a && b && c && d && e && f
}

/// 坏块拒绝。
fn corrupt_ok() -> bool {
    let aux = [AuxSeed { name: "malloc", ver: "GLIBC_2.2.5", flags: 0 }];
    let seeds = [NeedSeed { file: "libc.so.6", aux: &aux }];
    let t = match NeedTable::build(&seeds) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let blob = match t.blob() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut bad = blob;
    bad[0] = 2;
    let a = NeedTable::parse_blob(&bad).err() == Some(VN_EMETA);
    let mut bad = blob;
    bad[2] = 0;
    bad[3] = 0;
    let b = NeedTable::parse_blob(&bad).err() == Some(VN_EMETA);
    let mut bad = blob;
    bad[16] = 0xff;
    bad[17] = 0xff;
    bad[18] = 0xff;
    bad[19] = 0xff;
    let c = NeedTable::parse_blob(&bad).err() == Some(VN_EMETA);
    let mut bad = blob;
    bad[24] = 0x03;
    bad[25] = 0x00;
    let d = NeedTable::parse_blob(&bad).err() == Some(VN_EMETA);
    let e = NeedTable::parse_blob(&[]).err() == Some(VN_EMETA);
    let mut bad = blob;
    bad[28] = 0x00;
    bad[29] = 0x00;
    bad[30] = 0x03;
    bad[31] = 0x20;
    let f = NeedTable::parse_blob(&bad).err() == Some(VN_EMETA);
    a && b && c && d && e && f
}

/// 解析命中。
fn resolve_ok() -> bool {
    let t = DepTable::fixed();
    let a = match resolve(&t, "libc.so.6", "pthread_create", "GLIBC_2.34", 0) {
        Ok(f) => f.dep == "libc.so.6" && f.ver == "GLIBC_2.34" && !f.weak && f.text.is_empty(),
        Err(_) => false,
    };
    let b = match resolve(&t, "libc.so.6", "memcpy", "GLIBC_2.2.5", 0) {
        Ok(f) => f.ver == "GLIBC_2.2.5" && !f.weak,
        Err(_) => false,
    };
    let c = match resolve(&t, "libm.so.6", "exp", "GLIBC_2.29", 0) {
        Ok(f) => f.dep == "libm.so.6",
        Err(_) => false,
    };
    let d = match resolve(&t, "libssl.so.3", "SSL_new", "OPENSSL_3.0.0", 0) {
        Ok(f) => f.ver == "OPENSSL_3.0.0",
        Err(_) => false,
    };
    let e = resolve(&t, "libz.so.1", "foo", "GLIBC_2.2.5", 0).err() == Some(VN_EDEP);
    let f = resolve(&t, "libfoo.so.1", "foo_a", "GLIBC_2.2.5", 0).err() == Some(VN_EDEP);
    let g = resolve(&t, "libc.so.6", "malloc", "GLIBC_2.99", 0).err() == Some(VN_EMSVER);
    let h = resolve(&t, "libc.so.6", "no_such", "GLIBC_2.2.5", 0).err() == Some(VN_ESYM);
    a && b && c && d && e && f && g && h
}

/// 弱语义。
fn weak_ok() -> bool {
    let t = DepTable::fixed();
    let a = match resolve(&t, "libc.so.6", "missing_sym", "GLIBC_2.34", VER_FLG_WEAK) {
        Ok(f) => {
            f.weak
                && f.text.as_str()
                    == "weak symbol missing_sym, version GLIBC_2.34 not found (required by libc.so.6)\n"
        }
        Err(_) => false,
    };
    let b = resolve(&t, "libc.so.6", "missing_sym", "GLIBC_2.34", 0).err() == Some(VN_ESYM);
    let c = match resolve(&t, "libc.so.6", "malloc", "GLIBC_2.99", VER_FLG_WEAK) {
        Ok(f) => f.weak && !f.text.is_empty(),
        Err(_) => false,
    };
    let d = resolve(&t, "libc.so.6", "malloc", "GLIBC_2.2.5", VER_FLG_WEAK | VER_FLG_INFO)
        .err()
        == Some(VN_EMETA);
    let e = weak_census() == (13, 1);
    a && b && c && d && e
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
    let b = c.sym == 2 && c.ver == 1 && c.dep == 1 && c.other == 2;
    let d = c.matched == 14 && c.rate() == PPM_FULL && c.green() && c.gate() == Ok(());
    a && b && d
}

/// 槽位编号。
fn slots_ok() -> bool {
    let a = CORPUS == 14 && CASES.len() == CORPUS && unique() && notes_ok();
    let b = match case_at(0) {
        Some(c) => c.face == Face::Hit && c.note == "hit-new",
        None => false,
    };
    let c = match case_at(6) {
        Some(c) => match c.probe {
            Probe::Need { flags, .. } => c.face == Face::Weak && flags == VER_FLG_WEAK,
        },
        None => false,
    };
    let d = match case_at(13) {
        Some(c) => c.face == Face::Meta && c.want == Want::Code(VN_EBYTE),
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
        == "vn corpus=14 accept=8 reject=6 sym=2 ver=1 dep=1 other=2 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(VN_ENOBS);
    let d = e.render().as_str()
        == "vn corpus=0 accept=0 reject=0 sym=0 ver=0 dep=0 other=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(CORPUS + 1) == Err(VN_ENOBS);
    a && b && d && f
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
    let e = match run_with(Some(13)) {
        Ok(v) => v.matched == 13 && v.rate() == PPM_RED,
        Err(_) => false,
    };
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
    let b = hex_text(SEG_BASE) == "0x0000fda0" && hex_text(VN_EMSVER) == "0x0000fda6";
    let c = i32_text(-9) == "-9" && i32_text(9) == "9";
    let d = bool_text(true) == "true" && bool_text(false) == "false";
    let e = digest(b"verneed") == digest(b"verneed") && digest(b"verneed") != digest(b"vernee");
    let f = fold_word(FNV_OFF, 7) == digest(&[0, 0, 0, 7]);
    a && b && c && d && e && f
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = hash_ok() && version_ok() && names_ok();
    out[1] = deps_ok() && build_ok() && corrupt_ok();
    out[2] = resolve_ok() && weak_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && ledger_ok() && helpers_ok();
    out
}

/// 判据集合。
pub fn run_unx11651_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11651.verneed_missing");

    s.add("VN01-ELF哈希", hash_ok(), "static");
    s.add("VN02-版本语法", version_ok(), "static");
    s.add("VN03-名字闸", names_ok(), "static");
    s.add("VN04-依赖表", deps_ok(), "static");
    s.add("VN05-VERNEED构建", build_ok(), "static");
    s.add("VN06-坏块拒绝", corrupt_ok(), "static");
    s.add("VN07-符号解析", resolve_ok(), "static");
    s.add("VN08-弱符号宽容", weak_ok(), "static");
    s.add("VN09-十四槽表判", corpus_ok(), "static");
    s.add("VN10-槽位编号", slots_ok(), "static");
    s.add("VN11-汇总渲染", report_ok(), "static");
    s.add("VN12-红档929", reds_ok(), "static");
    s.add("VN13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn fail() -> ! {
        panic!("unexpected branch");
    }

    #[test]
    fn corpus_shape() {
        assert_eq!(CASES.len(), CORPUS);
        assert_eq!(CORPUS, 14);
        assert_eq!(SAMPLES, 14);
        assert!(unique());
        assert!(notes_ok());
        assert_eq!(case_at(0).map(|c| c.note), Some("hit-new"));
        assert!(case_at(14).is_none());
    }

    #[test]
    fn hash_vectors() {
        assert_eq!(elf_hash("GLIBC_2.2.5"), 0x0969_1a75);
        assert_eq!(elf_hash("GLIBC_2.34"), 0x0696_91b4);
        assert_eq!(elf_hash("GLIBC_2.17"), 0x0696_9197);
        assert_eq!(elf_hash("GLIBC_2.4"), 0x0d69_6914);
        assert_eq!(elf_hash(""), 0);
        assert_eq!(elf_hash("a"), 0x61);
        assert_eq!(elf_hash("malloc"), 0x0738_3353);
    }

    #[test]
    fn version_parse() {
        assert_eq!(parse_version("GLIBC_2.2.5").map(|v| v.text()), Ok(String::from("GLIBC_2.2.5")));
        assert_eq!(parse_version("OPENSSL_3.0.0").map(|v| v.text()), Ok(String::from("OPENSSL_3.0.0")));
        assert_eq!(parse_version("").err(), Some(VN_ENAME));
        assert_eq!(parse_version("GLIBC_2.2.5\n").err(), Some(VN_EBYTE));
        assert_eq!(parse_version("GLIBC_").err(), Some(VN_EMSVER));
        assert_eq!(parse_version("GLIBC_2.").err(), Some(VN_EMSVER));
        assert_eq!(parse_version("GLIBC_1.2.3.4.5").err(), Some(VN_EMSVER));
    }

    #[test]
    fn version_compare() {
        let a = parse_version("GLIBC_2.34").unwrap_or(Version::empty());
        let b = parse_version("GLIBC_2.2.5").unwrap_or(Version::empty());
        assert_eq!(cmp_ver(&a, &b), 1);
        assert!(at_least(&a, &b));
        assert!(!at_least(&b, &a));
        assert!(a.same_prefix(&b));
    }

    #[test]
    fn name_gates() {
        assert!(check_sym_name("malloc").is_ok());
        assert!(check_soname("libc.so.6").is_ok());
        assert!(check_path("/lib64/libc.so.6").is_ok());
        assert_eq!(check_sym_name("bad name").err(), Some(VN_EBYTE));
        assert_eq!(check_soname("").err(), Some(VN_ENAME));
        assert_eq!(check_path("/x/`y`").err(), Some(VN_EBYTE));
    }

    #[test]
    fn text_type() {
        let t = match Text::from_str("libc.so.6") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(t.eq_str("libc.so.6"));
        assert!(!t.eq_str("libc.so.7"));
        assert!(!t.is_empty());
        assert_eq!(t.text(), "libc.so.6");
        assert!(Text::empty().is_empty());
        assert_eq!(Text::from_str("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").err(), Some(VN_ENAME));
    }

    #[test]
    fn dep_fixed() {
        let t = DepTable::fixed();
        assert_eq!(t.len(), 4);
        assert!(!t.is_empty());
        assert_eq!(t.dep(0).map(|d| d.name), Some("libc.so.6"));
        assert!(t.dep(4).is_none());
        assert!(t.find("libfoo.so.1").is_some());
        assert!(t.find("nope").is_none());
        let libc = match t.find("libc.so.6") {
            Some(v) => v,
            None => fail(),
        };
        assert_eq!(libc.exact_index("memcpy", "GLIBC_2.14"), Some(3));
        assert!(libc.exact_index("memcpy", "GLIBC_2.99").is_none());
        assert!(libc.has_name("pthread_create"));
    }

    #[test]
    fn dep_rejects() {
        assert_eq!(DepTable::new(&BAD_DEP_EMPTY).err(), Some(VN_ENAME));
        assert_eq!(DepTable::new(&BAD_DEP_NOVER).err(), Some(VN_EDEP));
        assert_eq!(DepTable::new(&BAD_DEP_DUP).err(), Some(VN_ESYM));
        let two = [FIXED_DEPS[0], FIXED_DEPS[0]];
        assert_eq!(DepTable::new(&two).err(), Some(VN_ESYM));
        assert_eq!(DepTable::new(&[]).err(), Some(VN_EDEP));
    }

    #[test]
    fn need_build_valid() {
        let aux = [AuxSeed { name: "malloc", ver: "GLIBC_2.2.5", flags: 0 }];
        let seeds = [NeedSeed { file: "libc.so.6", aux: &aux }];
        let t = match NeedTable::build(&seeds) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(t.len(), 1);
        let a = match t.aux(0, 0) {
            Some(v) => v,
            None => fail(),
        };
        assert!(a.name.eq_str("malloc"));
        assert!(a.ver.eq_str("GLIBC_2.2.5"));
        assert_eq!(a.hash, elf_hash("GLIBC_2.2.5"));
        assert_eq!(a.hash, 0x0969_1a75);
        assert_eq!(a.other, 1);
        assert_eq!(a.flags, 0);
    }

    #[test]
    fn need_build_rejects() {
        let none: [AuxSeed; 0] = [];
        let bad_file = [NeedSeed { file: "bad name", aux: &none }];
        assert_eq!(NeedTable::build(&bad_file).err(), Some(VN_EBYTE));
        let empty = [NeedSeed { file: "libc.so.6", aux: &none }];
        assert_eq!(NeedTable::build(&empty).err(), Some(VN_ENAME));
        let bad_sym = [AuxSeed { name: "bad name", ver: "GLIBC_2.2.5", flags: 0 }];
        assert_eq!(
            NeedTable::build(&[NeedSeed { file: "libc.so.6", aux: &bad_sym }]).err(),
            Some(VN_EBYTE)
        );
        let bad_ver = [AuxSeed { name: "malloc", ver: "GLIBC_", flags: 0 }];
        assert_eq!(
            NeedTable::build(&[NeedSeed { file: "libc.so.6", aux: &bad_ver }]).err(),
            Some(VN_EMSVER)
        );
        let bad_flags = [AuxSeed { name: "malloc", ver: "GLIBC_2.2.5", flags: VER_FLG_INFO }];
        assert_eq!(
            NeedTable::build(&[NeedSeed { file: "libc.so.6", aux: &bad_flags }]).err(),
            Some(VN_EMETA)
        );
        let dup = [
            AuxSeed { name: "malloc", ver: "GLIBC_2.2.5", flags: 0 },
            AuxSeed { name: "malloc", ver: "GLIBC_2.2.5", flags: 0 },
        ];
        assert_eq!(
            NeedTable::build(&[NeedSeed { file: "libc.so.6", aux: &dup }]).err(),
            Some(VN_ESYM)
        );
        assert_eq!(NeedTable::build(&[]).err(), Some(VN_EDEP));
    }

    #[test]
    fn blob_roundtrip() {
        let aux = [
            AuxSeed { name: "malloc", ver: "GLIBC_2.2.5", flags: 0 },
            AuxSeed { name: "memcpy", ver: "GLIBC_2.14", flags: VER_FLG_WEAK },
        ];
        let seeds = [NeedSeed { file: "libc.so.6", aux: &aux }];
        let t = match NeedTable::build(&seeds) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        let blob = match t.blob() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        let p = match NeedTable::parse_blob(&blob) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(p, t);
        assert_eq!(p.len(), 1);
        assert_eq!(p.aux(0, 0).map(|a| a.other), Some(1));
        assert_eq!(p.aux(0, 1).map(|a| a.other), Some(2));
        assert_eq!(p.aux(0, 2), None);
    }

    #[test]
    fn blob_corrupt() {
        let aux = [AuxSeed { name: "malloc", ver: "GLIBC_2.2.5", flags: 0 }];
        let t = match NeedTable::build(&[NeedSeed { file: "libc.so.6", aux: &aux }]) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        let blob = match t.blob() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        let mut a = blob;
        a[0] = 9;
        assert_eq!(NeedTable::parse_blob(&a).err(), Some(VN_EMETA));
        let mut b = blob;
        b[2] = 0;
        b[3] = 0;
        assert_eq!(NeedTable::parse_blob(&b).err(), Some(VN_EMETA));
        let mut c = blob;
        c[16] ^= 0xff;
        assert_eq!(NeedTable::parse_blob(&c).err(), Some(VN_EMETA));
        let mut d = blob;
        d[24] = 0x7f;
        d[25] = 0xff;
        assert_eq!(NeedTable::parse_blob(&d).err(), Some(VN_EMETA));
        assert_eq!(NeedTable::parse_blob(&[]).err(), Some(VN_EMETA));
    }

    #[test]
    fn resolve_hits() {
        let t = DepTable::fixed();
        let a = match resolve(&t, "libc.so.6", "pthread_create", "GLIBC_2.34", 0) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(a.dep, "libc.so.6");
        assert_eq!(a.ver, "GLIBC_2.34");
        assert!(!a.weak);
        assert!(a.text.is_empty());
        let b = match resolve(&t, "libm.so.6", "exp", "GLIBC_2.29", 0) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(b.ver, "GLIBC_2.29");
        let c = match resolve(&t, "libssl.so.3", "SSL_new", "OPENSSL_3.0.0", 0) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(c.dep, "libssl.so.3");
    }

    #[test]
    fn resolve_errors() {
        let t = DepTable::fixed();
        assert_eq!(
            resolve(&t, "libc.so.6", "no_such", "GLIBC_2.2.5", 0).err(),
            Some(VN_ESYM)
        );
        assert_eq!(
            resolve(&t, "libc.so.6", "malloc", "GLIBC_2.99", 0).err(),
            Some(VN_EMSVER)
        );
        assert_eq!(
            resolve(&t, "libz.so.1", "foo", "GLIBC_2.2.5", 0).err(),
            Some(VN_EDEP)
        );
        assert_eq!(
            resolve(&t, "libfoo.so.1", "foo_a", "GLIBC_2.2.5", 0).err(),
            Some(VN_EDEP)
        );
        assert_eq!(
            resolve(&t, "libc.so.6", "malloc", "GLIBC_2.2.5", VER_FLG_INFO).err(),
            Some(VN_EMETA)
        );
        assert_eq!(
            resolve(&t, "libc.so.6", "bad name", "GLIBC_2.2.5", 0).err(),
            Some(VN_EBYTE)
        );
    }

    #[test]
    fn weak_semantics() {
        let t = DepTable::fixed();
        let a = match resolve(&t, "libc.so.6", "missing_sym", "GLIBC_2.34", VER_FLG_WEAK) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(a.weak);
        assert_eq!(
            a.text.as_str(),
            "weak symbol missing_sym, version GLIBC_2.34 not found (required by libc.so.6)\n"
        );
        assert_eq!(
            resolve(&t, "libc.so.6", "missing_sym", "GLIBC_2.34", 0).err(),
            Some(VN_ESYM)
        );
        let c = match resolve(&t, "libc.so.6", "malloc", "GLIBC_2.99", VER_FLG_WEAK) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(c.weak);
        assert!(c.text.as_str().contains("malloc"));
    }

    #[test]
    fn corpus_each_slot() {
        assert!(table_ok());
        let mut i = 0usize;
        while i < CASES.len() {
            let c = match case_at(i) {
                Some(v) => v,
                None => fail(),
            };
            assert!(want_match(c.want, &judge(c.probe)));
            i += 1;
        }
    }

    #[test]
    fn strong_missing_codes() {
        let t = DepTable::fixed();
        assert_eq!(
            resolve(&t, "libc.so.6", "pthread_create", "GLIBC_2.2.5", 0).err(),
            Some(VN_EMSVER)
        );
        assert_eq!(
            resolve(&t, "libssl.so.3", "SSL_new", "OPENSSL_1.1.0", 0).err(),
            Some(VN_EMSVER)
        );
        assert_eq!(
            resolve(&t, "libm.so.6", "sin", "GLIBC_2.29", 0).err(),
            Some(VN_EMSVER)
        );
    }

    #[test]
    fn census_numbers() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(c.total, 14);
        assert_eq!(c.accept, 8);
        assert_eq!(c.reject, 6);
        assert_eq!(c.sym, 2);
        assert_eq!(c.ver, 1);
        assert_eq!(c.dep, 1);
        assert_eq!(c.other, 2);
        assert_eq!(c.matched, 14);
        assert_eq!(c.rate(), 1000);
        assert!(c.green());
        assert_eq!(c.gate(), Ok(()));
    }

    #[test]
    fn census_report_exact() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(
            c.render().as_str(),
            "vn corpus=14 accept=8 reject=6 sym=2 ver=1 dep=1 other=2 match=14 rate=1000ppm\n"
        );
        let e = Census::new();
        assert_eq!(
            e.render().as_str(),
            "vn corpus=0 accept=0 reject=0 sym=0 ver=0 dep=0 other=0 match=0 rate=0ppm\n"
        );
        assert_eq!(e.gate(), Err(VN_ENOBS));
    }

    #[test]
    fn red_rounding() {
        let c = match run_with(Some(3)) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(c.matched, 13);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(VN_EVALUE));
        assert_eq!(run_with(Some(14)).err(), Some(VN_ESLOT));
    }

    #[test]
    fn weak_count() {
        assert_eq!(weak_census(), (13, 1));
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(15), Err(VN_ENOBS));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(!l.is_empty());
        assert_eq!(l.audit(), Ok(()));
    }

    #[test]
    fn ledger_codes() {
        let l = build_ledger();
        assert_eq!(l.sample(8).map(|s| s.code), Some(VN_ESYM));
        assert_eq!(l.sample(9).map(|s| s.code), Some(VN_ESYM));
        assert_eq!(l.sample(10).map(|s| s.code), Some(VN_EMSVER));
        assert_eq!(l.sample(11).map(|s| s.code), Some(VN_EDEP));
        assert_eq!(l.sample(12).map(|s| s.code), Some(VN_EMETA));
        assert_eq!(l.sample(13).map(|s| s.code), Some(VN_EBYTE));
        assert_eq!(l.sample(14), None);
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(l.is_frozen());
        assert_eq!(l.root(), root);
        assert_eq!(l.replay(), Ok(root));
        let before = l.rejects();
        assert_eq!(l.register(Sample::blank()).err(), Some(VN_ESEAL));
        assert_eq!(l.rejects(), before + 1);
    }

    #[test]
    fn ledger_order_gate() {
        let mut l = Ledger::new();
        let first = Sample { slot: 1, op: 0, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(first).err(), Some(VN_ESLOT));
        let zero = Sample { slot: 0, op: 0, ok: true, code: 0, value: 0 };
        assert_eq!(l.register(zero), Ok(()));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn helpers() {
        assert_eq!(u32_text(0), "0");
        assert_eq!(u32_text(4294967295), "4294967295");
        assert_eq!(hex_text(SEG_BASE), "0x0000fda0");
        assert_eq!(hex_text(VN_EBYTE), "0x0000fda1");
        assert_eq!(i32_text(-7), "-7");
        assert_eq!(bool_text(true), "true");
        assert_eq!(digest(b"x"), digest(b"x"));
        assert_eq!(fold_word(FNV_OFF, 1), digest(&[0, 0, 0, 1]));
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11651_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
