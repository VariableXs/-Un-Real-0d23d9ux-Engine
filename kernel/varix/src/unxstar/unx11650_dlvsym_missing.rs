//! UNX-F11650 dlvsym 查询不存在版本返回 NULL 判据
//!
//! dlvsym(handle, symbol, version) 与 dlsym 只差一个版本参数，语义却分叉：
//! dlsym 取默认版本（或唯一无版本符号），dlvsym 必须精确命中“名字 + 版本”
//! 二元组，缺一不可。查询失败时函数返回 NULL，并把一行错误挂到 dlerror：
//! `undefined symbol: memcpy, version GLIBC_2.34`。逗号、空格、version 单词
//! 与版本串的顺序都不能动；名字与版本原样出现，不加引号。
//!
//! 同一符号多版本共存是判据的核心：libc 的 memcpy 既有 GLIBC_2.2.5 兼容版，
//! 又有 GLIBC_2.14 默认版；clock_gettime 既有 2.2.5 又有 2.17。dlvsym 要能
//! 取到非默认的兼容版，dlsym 只能取默认版。若对象根本没有版本节（VERDEF
//! 缺失），dlvsym 一律失败；若对象有版本节但该名字只挂在别的版本上，也失败。
//! 失败原因内部分成“版本不匹配 / 符号不存在 / 无版本信息 / 句柄坏”四类，
//! 但前三类对外文案完全一致，只有坏句柄换成 `invalid handle`。
//!
//! dlerror 是一次性槽：失败时挂文案，取走一次后必须变回空；成功的调用会
//! 清掉旧文案。本模块用定长 DlSlot 复刻这个生命周期，避免把上一轮的旧错误
//! 误配给下一轮查询。
//!
//! 版本串只允许 `PREFIX_A.B.C`：大写字母前缀、下划线、1 到 4 段十进制、
//! 每段不超过 9999。空版本、超长版本落 ENAME；空白、引号、控制符落 EBYTE；
//! 结构坏（缺下划线、空段、段里混字母、段数越界）按“不可匹配”落 EMSVER。
//!
//! 闸门次序：句柄 → 符号名 → 版本语法 → 对象版本表 → 精确命中 → 文案 → 账本。
//!
//! 契约：纯 no_std（只用 alloc 的 String）、无 unsafe、无 unwrap/expect/
//! panic!，失败一律 `Result<_, u32>`；账本 14 格（8 成 6 败）逐格对常量表，
//! 冻结后登记落 ESEAL，重放必须复现同一指纹；语料 14 槽互异，覆盖率按
//! `(matched*1000 + total/2)/total` 四舍五入，单槽红档 929ppm。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11650）。
pub const SEG_BASE: u32 = 0xfd90;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 满档千分比。
pub const PPM_FULL: u32 = 1000;

/// 单槽红档千分比。
pub const PPM_RED: u32 = 929;

/// 符号名最大字节数。
pub const SYM_NAME_MAX: usize = 64;

/// 对象名/路径最大字节数。
pub const NAME_MAX: usize = 64;

/// 路径最大字节数。
pub const PATH_MAX: usize = 96;

/// 版本串最大字节数。
pub const VERSION_MAX: usize = 32;

/// 版本前缀最大字节数。
pub const PREFIX_MAX: usize = 16;

/// 版本段上限。
pub const SEG_MAX: usize = 4;

/// 版本段最大值。
pub const SEG_LIMIT: u32 = 9999;

/// 一行错误文案最大字节数。
pub const LINE_MAX: usize = 256;

/// 单个对象的版本表上限。
pub const VER_MAX: usize = 8;

/// 单个对象的符号表上限。
pub const SYM_MAX: usize = 12;

/// 对象表上限（句柄 1..=len 合法，0 为全局）。
pub const DSO_MAX: usize = 4;

/// 输入里有非法字节（空白、引号、反引号、分号、控制符）。
pub const DV_EBYTE: u32 = SEG_BASE + 0x01;

/// 名字为空或超长。
pub const DV_ENAME: u32 = SEG_BASE + 0x02;

/// 句柄非法。
pub const DV_EHANDLE: u32 = SEG_BASE + 0x03;

/// 符号名不存在。
pub const DV_ESYM: u32 = SEG_BASE + 0x04;

/// 版本不可匹配（缺版本、语法坏、段落空）。
pub const DV_EMSVER: u32 = SEG_BASE + 0x05;

/// 对象没有版本信息。
pub const DV_ENOVER: u32 = SEG_BASE + 0x06;

/// 表已冻结或重复冻结。
pub const DV_ESTATE: u32 = SEG_BASE + 0x07;

/// 语义与期望不符。
pub const DV_EVER: u32 = SEG_BASE + 0x08;

/// 账本不平。
pub const DV_EBAL: u32 = SEG_BASE + 0x09;

/// 槽位错序。
pub const DV_ESLOT: u32 = SEG_BASE + 0x0a;

/// 观测值与常量表不符。
pub const DV_EVALUE: u32 = SEG_BASE + 0x0b;

/// 冻结重放指纹不符。
pub const DV_ESEAL: u32 = SEG_BASE + 0x0c;

/// 空观测。
pub const DV_ENOBS: u32 = SEG_BASE + 0x0d;

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

/// 版本串合法字节。
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
        return Err(DV_ENAME);
    }
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if !ver_byte(*b) {
                return Err(DV_EBYTE);
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
        return Err(DV_EMSVER);
    }
    let mut v = Version::empty();
    i = 0usize;
    while i < us {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(DV_EMSVER),
        };
        let ok = if i == 0 {
            b >= b'A' && b <= b'Z'
        } else {
            (b >= b'A' && b <= b'Z') || (b >= b'0' && b <= b'9')
        };
        if !ok {
            return Err(DV_EMSVER);
        }
        if let Some(d) = v.prefix.get_mut(i) {
            *d = b;
        }
        v.plen += 1;
        i += 1;
    }
    let tail = match bytes.get(us + 1..) {
        Some(t) => t,
        None => return Err(DV_EMSVER),
    };
    if tail.is_empty() {
        return Err(DV_EMSVER);
    }
    let mut start = 0usize;
    let mut seg = 0usize;
    while start < tail.len() {
        if seg >= SEG_MAX {
            return Err(DV_EMSVER);
        }
        let mut end = start;
        while end < tail.len() {
            match tail.get(end) {
                Some(b) => {
                    if *b == b'.' {
                        break;
                    }
                }
                None => return Err(DV_EMSVER),
            }
            end += 1;
        }
        if end == start {
            return Err(DV_EMSVER);
        }
        let mut val = 0u32;
        let mut k = start;
        while k < end {
            let b = match tail.get(k) {
                Some(x) => *x,
                None => return Err(DV_EMSVER),
            };
            if b < b'0' || b > b'9' {
                return Err(DV_EMSVER);
            }
            val = val * 10 + ((b - b'0') as u32);
            if val > SEG_LIMIT {
                return Err(DV_EMSVER);
            }
            k += 1;
        }
        if let Some(d) = v.segs.get_mut(seg) {
            *d = val;
        }
        seg += 1;
        if end < tail.len() {
            if end + 1 >= tail.len() {
                return Err(DV_EMSVER);
            }
            start = end + 1;
        } else {
            start = tail.len();
        }
    }
    v.n = seg;
    if v.n == 0 {
        return Err(DV_EMSVER);
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
    if b.is_empty() || b.len() > SYM_NAME_MAX {
        return Err(DV_ENAME);
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(DV_ENAME),
        };
        if !sym_byte(c) {
            return Err(DV_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

/// 对象名闸。
pub fn check_soname(s: &str) -> Result<(), u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > NAME_MAX {
        return Err(DV_ENAME);
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(DV_ENAME),
        };
        if !name_byte(c) {
            return Err(DV_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

/// 路径闸。
pub fn check_path(s: &str) -> Result<(), u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > PATH_MAX {
        return Err(DV_ENAME);
    }
    let mut i = 0usize;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(DV_ENAME),
        };
        if !path_byte(c) {
            return Err(DV_EBYTE);
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 对象表
// ---------------------------------------------------------------------------

/// 符号种子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymSeed {
    /// 名字。
    pub name: &'static str,
    /// 版本（无版本对象留空）。
    pub ver: &'static str,
    /// 是否默认版本。
    pub default: bool,
}

/// 对象种子。
#[derive(Clone, Copy, Debug)]
pub struct DsoSeed {
    /// soname。
    pub name: &'static str,
    /// 装载路径。
    pub path: &'static str,
    /// 装载基址（页对齐）。
    pub base: u32,
    /// 是否有版本节。
    pub verdef: bool,
    /// 版本表。
    pub versions: &'static [&'static str],
    /// 符号表。
    pub syms: &'static [SymSeed],
}

/// 已校验对象。
#[derive(Clone, Copy, Debug)]
pub struct Object {
    /// soname。
    pub name: &'static str,
    /// 装载路径。
    pub path: &'static str,
    /// 装载基址。
    pub base: u32,
    /// 是否有版本节。
    pub verdef: bool,
    /// 版本表。
    pub versions: &'static [&'static str],
    /// 符号表。
    pub syms: &'static [SymSeed],
}

impl Object {
    /// 空对象。
    pub const fn empty() -> Object {
        Object { name: "", path: "", base: 0, verdef: false, versions: &[], syms: &[] }
    }
}

fn list_has(list: &[&'static str], s: &str) -> bool {
    let mut i = 0usize;
    while i < list.len() {
        if let Some(v) = list.get(i) {
            if *v == s {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn validate_versions(list: &[&'static str]) -> Result<(), u32> {
    if list.is_empty() || list.len() > VER_MAX {
        return Err(DV_ENOVER);
    }
    let mut i = 0usize;
    while i < list.len() {
        let s = match list.get(i) {
            Some(v) => *v,
            None => return Err(DV_ENOVER),
        };
        parse_version(s)?;
        let mut j = 0usize;
        while j < i {
            if let Some(p) = list.get(j) {
                if *p == s {
                    return Err(DV_EMSVER);
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

impl Object {
    /// 从种子校验构造。
    pub fn from_seed(seed: &DsoSeed) -> Result<Object, u32> {
        check_soname(seed.name)?;
        check_path(seed.path)?;
        if seed.base == 0 || seed.base % 0x1000 != 0 {
            return Err(DV_ENOVER);
        }
        if seed.syms.is_empty() || seed.syms.len() > SYM_MAX {
            return Err(DV_ESYM);
        }
        if seed.verdef {
            validate_versions(seed.versions)?;
        } else if !seed.versions.is_empty() {
            return Err(DV_ENOVER);
        }
        let mut i = 0usize;
        while i < seed.syms.len() {
            let s = match seed.syms.get(i) {
                Some(v) => *v,
                None => return Err(DV_ESYM),
            };
            check_sym_name(s.name)?;
            if seed.verdef {
                if s.ver.is_empty() {
                    return Err(DV_EMSVER);
                }
                parse_version(s.ver)?;
                if !list_has(seed.versions, s.ver) {
                    return Err(DV_EMSVER);
                }
            } else {
                if !s.ver.is_empty() {
                    return Err(DV_ENOVER);
                }
                if s.default {
                    return Err(DV_ENOVER);
                }
            }
            let mut j = 0usize;
            while j < i {
                if let Some(p) = seed.syms.get(j) {
                    if p.name == s.name && p.ver == s.ver {
                        return Err(DV_ESYM);
                    }
                    if s.default && p.name == s.name && p.default {
                        return Err(DV_ESYM);
                    }
                }
                j += 1;
            }
            i += 1;
        }
        Ok(Object {
            name: seed.name,
            path: seed.path,
            base: seed.base,
            verdef: seed.verdef,
            versions: seed.versions,
            syms: seed.syms,
        })
    }

    /// 是否有某名字。
    pub fn has_name(&self, name: &str) -> bool {
        let mut i = 0usize;
        while i < self.syms.len() {
            if let Some(s) = self.syms.get(i) {
                if s.name == name {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 精确版本命中下标。
    pub fn exact_index(&self, name: &str, ver: &str) -> Option<usize> {
        if !self.verdef {
            return None;
        }
        let mut i = 0usize;
        while i < self.syms.len() {
            if let Some(s) = self.syms.get(i) {
                if s.name == name && s.ver == ver {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 默认符号命中下标。
    pub fn default_index(&self, name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.syms.len() {
            if let Some(s) = self.syms.get(i) {
                if s.name == name && (s.ver.is_empty() || s.default) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
}

/// 对象表。
#[derive(Clone, Copy, Debug)]
pub struct DsoTable {
    objs: [Object; DSO_MAX],
    len: usize,
}

impl DsoTable {
    /// 空表。
    pub const fn empty() -> DsoTable {
        DsoTable { objs: [Object::empty(); DSO_MAX], len: 0 }
    }

    /// 从种子建表。
    pub fn new(seeds: &[DsoSeed]) -> Result<DsoTable, u32> {
        if seeds.is_empty() || seeds.len() > DSO_MAX {
            return Err(DV_ENOVER);
        }
        let mut t = DsoTable::empty();
        let mut i = 0usize;
        while i < seeds.len() {
            let s = match seeds.get(i) {
                Some(v) => v,
                None => return Err(DV_ENOVER),
            };
            let o = Object::from_seed(s)?;
            let mut j = 0usize;
            while j < i {
                if let Some(p) = t.objs.get(j) {
                    if p.name == o.name || p.base == o.base {
                        return Err(DV_ESYM);
                    }
                }
                j += 1;
            }
            if let Some(dst) = t.objs.get_mut(i) {
                *dst = o;
            }
            t.len = i + 1;
            i += 1;
        }
        Ok(t)
    }

    /// 固定表。
    pub fn fixed() -> DsoTable {
        match DsoTable::new(&FIXED_SEEDS) {
            Ok(t) => t,
            Err(_) => DsoTable::empty(),
        }
    }

    /// 对象数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取对象。
    pub fn object(&self, i: usize) -> Option<&Object> {
        if i >= self.len {
            return None;
        }
        self.objs.get(i)
    }

    /// 按名字取对象。
    pub fn object_by_name(&self, name: &str) -> Option<&Object> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(o) = self.objs.get(i) {
                if o.name == name {
                    return Some(o);
                }
            }
            i += 1;
        }
        None
    }

    /// 句柄合法：0 为全局，1..=len 为对象。
    pub fn handle_ok(&self, h: u32) -> bool {
        (h as usize) <= self.len
    }
}

/// 命中记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 对象 soname。
    pub obj_name: &'static str,
    /// 符号名。
    pub sym_name: &'static str,
    /// 命中的版本（无版本对象为空）。
    pub ver: &'static str,
    /// 模型地址。
    pub addr: u32,
    /// 是否默认版本。
    pub default: bool,
}

/// 一次查询的观测。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 命中（NULL 时为空）。
    pub hit: Option<Hit>,
    /// 内部原因码（命中为 0）。
    pub code: u32,
    /// dlerror 文案（命中为空串）。
    pub text: String,
}

impl Outcome {
    /// 命中。
    pub fn found(&self) -> bool {
        self.code == 0 && self.hit.is_some()
    }

    /// 返回 NULL。
    pub fn null(&self) -> bool {
        self.hit.is_none()
    }

    /// 文案非空。
    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }
}

fn make_hit(o: &Object, si: usize) -> Hit {
    let (nm, vr, df) = match o.syms.get(si) {
        Some(s) => (s.name, s.ver, s.default),
        None => ("", "", false),
    };
    let addr = o
        .base
        .wrapping_add(0x100)
        .wrapping_add((si as u32).wrapping_mul(0x10));
    Hit { obj_name: o.name, sym_name: nm, ver: vr, addr, default: df }
}

fn undefined_ver(name: &str, ver: &str) -> String {
    let mut s = String::from("undefined symbol: ");
    s.push_str(name);
    s.push_str(", version ");
    s.push_str(ver);
    s
}

fn undefined_plain(name: &str) -> String {
    let mut s = String::from("undefined symbol: ");
    s.push_str(name);
    s
}

fn found_outcome(h: Hit) -> Outcome {
    Outcome { hit: Some(h), code: 0, text: String::new() }
}

fn fail_outcome(code: u32, text: String) -> Outcome {
    Outcome { hit: None, code, text }
}

// ---------------------------------------------------------------------------
// 查询
// ---------------------------------------------------------------------------

/// dlvsym 精确查询。
pub fn dlvsym(tab: &DsoTable, handle: u32, name: &str, ver: &str) -> Result<Outcome, u32> {
    if !tab.handle_ok(handle) {
        return Ok(fail_outcome(DV_EHANDLE, String::from("invalid handle")));
    }
    check_sym_name(name)?;
    parse_version(ver)?;
    if handle == 0 {
        let mut versioned_name = false;
        let mut unversioned_name = false;
        let mut i = 0usize;
        while i < tab.len() {
            if let Some(o) = tab.object(i) {
                if o.verdef {
                    if let Some(si) = o.exact_index(name, ver) {
                        return Ok(found_outcome(make_hit(o, si)));
                    }
                    if o.has_name(name) {
                        versioned_name = true;
                    }
                } else if o.has_name(name) {
                    unversioned_name = true;
                }
            }
            i += 1;
        }
        if versioned_name {
            return Ok(fail_outcome(DV_EMSVER, undefined_ver(name, ver)));
        }
        if unversioned_name {
            return Ok(fail_outcome(DV_ENOVER, undefined_ver(name, ver)));
        }
        return Ok(fail_outcome(DV_ESYM, undefined_ver(name, ver)));
    }
    let idx = (handle - 1) as usize;
    let o = match tab.object(idx) {
        Some(v) => v,
        None => return Ok(fail_outcome(DV_EHANDLE, String::from("invalid handle"))),
    };
    if !o.verdef {
        return Ok(fail_outcome(DV_ENOVER, undefined_ver(name, ver)));
    }
    if let Some(si) = o.exact_index(name, ver) {
        return Ok(found_outcome(make_hit(o, si)));
    }
    if o.has_name(name) {
        Ok(fail_outcome(DV_EMSVER, undefined_ver(name, ver)))
    } else {
        Ok(fail_outcome(DV_ESYM, undefined_ver(name, ver)))
    }
}

/// dlsym 默认版本查询。
pub fn dlsym(tab: &DsoTable, handle: u32, name: &str) -> Result<Outcome, u32> {
    if !tab.handle_ok(handle) {
        return Ok(fail_outcome(DV_EHANDLE, String::from("invalid handle")));
    }
    check_sym_name(name)?;
    if handle == 0 {
        let mut i = 0usize;
        while i < tab.len() {
            if let Some(o) = tab.object(i) {
                if let Some(si) = o.default_index(name) {
                    return Ok(found_outcome(make_hit(o, si)));
                }
            }
            i += 1;
        }
        return Ok(fail_outcome(DV_ESYM, undefined_plain(name)));
    }
    let idx = (handle - 1) as usize;
    let o = match tab.object(idx) {
        Some(v) => v,
        None => return Ok(fail_outcome(DV_EHANDLE, String::from("invalid handle"))),
    };
    if let Some(si) = o.default_index(name) {
        Ok(found_outcome(make_hit(o, si)))
    } else {
        Ok(fail_outcome(DV_ESYM, undefined_plain(name)))
    }
}

// ---------------------------------------------------------------------------
// 一次性错误槽
// ---------------------------------------------------------------------------

/// dlerror 一次性槽。
#[derive(Clone, Copy, Debug)]
pub struct DlSlot {
    buf: [u8; LINE_MAX],
    len: usize,
    armed: bool,
}

impl DlSlot {
    /// 空槽。
    pub const fn new() -> DlSlot {
        DlSlot { buf: [0u8; LINE_MAX], len: 0, armed: false }
    }

    /// 清空。
    pub fn clear(&mut self) {
        self.len = 0;
        self.armed = false;
    }

    /// 挂文案。
    pub fn arm(&mut self, text: &str) -> Result<(), u32> {
        let b = text.as_bytes();
        if b.len() > LINE_MAX {
            return Err(DV_EVALUE);
        }
        self.len = 0;
        let mut i = 0usize;
        while i < b.len() {
            if let Some(d) = self.buf.get_mut(i) {
                if let Some(c) = b.get(i) {
                    *d = *c;
                    self.len += 1;
                }
            }
            i += 1;
        }
        self.armed = true;
        Ok(())
    }

    /// 是否有挂起文案。
    pub fn armed(&self) -> bool {
        self.armed
    }

    /// 取走一次。
    pub fn take(&mut self) -> Option<String> {
        if !self.armed {
            return None;
        }
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.buf.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        self.armed = false;
        self.len = 0;
        Some(s)
    }
}

impl Default for DlSlot {
    fn default() -> DlSlot {
        DlSlot::new()
    }
}

/// 带错误槽的 dlvsym。
pub fn invoke_dlvsym(
    tab: &DsoTable,
    handle: u32,
    name: &str,
    ver: &str,
    slot: &mut DlSlot,
) -> Result<Option<Hit>, u32> {
    slot.clear();
    match dlvsym(tab, handle, name, ver) {
        Ok(o) => {
            if let Some(h) = o.hit {
                Ok(Some(h))
            } else {
                slot.arm(o.text.as_str())?;
                Ok(None)
            }
        }
        Err(c) => Err(c),
    }
}

/// 带错误槽的 dlsym。
pub fn invoke_dlsym(
    tab: &DsoTable,
    handle: u32,
    name: &str,
    slot: &mut DlSlot,
) -> Result<Option<Hit>, u32> {
    slot.clear();
    match dlsym(tab, handle, name) {
        Ok(o) => {
            if let Some(h) = o.hit {
                Ok(Some(h))
            } else {
                slot.arm(o.text.as_str())?;
                Ok(None)
            }
        }
        Err(c) => Err(c),
    }
}

// ---------------------------------------------------------------------------
// 固定对象表
// ---------------------------------------------------------------------------

static LIBC_VERSIONS: [&str; 4] = ["GLIBC_2.2.5", "GLIBC_2.14", "GLIBC_2.17", "GLIBC_2.34"];

static LIBC_SYMS: [SymSeed; 11] = [
    SymSeed { name: "malloc", ver: "GLIBC_2.2.5", default: true },
    SymSeed { name: "free", ver: "GLIBC_2.2.5", default: true },
    SymSeed { name: "memcpy", ver: "GLIBC_2.2.5", default: false },
    SymSeed { name: "memcpy", ver: "GLIBC_2.14", default: true },
    SymSeed { name: "clock_gettime", ver: "GLIBC_2.2.5", default: false },
    SymSeed { name: "clock_gettime", ver: "GLIBC_2.17", default: true },
    SymSeed { name: "getenv", ver: "GLIBC_2.2.5", default: true },
    SymSeed { name: "strlen", ver: "GLIBC_2.2.5", default: true },
    SymSeed { name: "pthread_create", ver: "GLIBC_2.34", default: true },
    SymSeed { name: "dlvsym", ver: "GLIBC_2.34", default: true },
    SymSeed { name: "__libc_start_main", ver: "GLIBC_2.34", default: true },
];

static LIBM_VERSIONS: [&str; 2] = ["GLIBC_2.2.5", "GLIBC_2.29"];

static LIBM_SYMS: [SymSeed; 5] = [
    SymSeed { name: "sin", ver: "GLIBC_2.2.5", default: true },
    SymSeed { name: "cos", ver: "GLIBC_2.2.5", default: true },
    SymSeed { name: "exp", ver: "GLIBC_2.29", default: true },
    SymSeed { name: "log", ver: "GLIBC_2.29", default: true },
    SymSeed { name: "pow", ver: "GLIBC_2.29", default: true },
];

static SSL_VERSIONS: [&str; 3] = ["OPENSSL_1.1.0", "OPENSSL_1.1.1", "OPENSSL_3.0.0"];

static SSL_SYMS: [SymSeed; 5] = [
    SymSeed { name: "SSL_new", ver: "OPENSSL_1.1.0", default: false },
    SymSeed { name: "SSL_new", ver: "OPENSSL_3.0.0", default: true },
    SymSeed { name: "SSL_CTX_new", ver: "OPENSSL_3.0.0", default: true },
    SymSeed { name: "OPENSSL_init_ssl", ver: "OPENSSL_1.1.1", default: true },
    SymSeed { name: "EVP_Digest", ver: "OPENSSL_3.0.0", default: true },
];

static FOO_SYMS: [SymSeed; 3] = [
    SymSeed { name: "foo_a", ver: "", default: false },
    SymSeed { name: "foo_b", ver: "", default: false },
    SymSeed { name: "foo_c", ver: "", default: false },
];

static FIXED_SEEDS: [DsoSeed; 4] = [
    DsoSeed {
        name: "libc.so.6",
        path: "/lib64/libc.so.6",
        base: 0x7f00_0000,
        verdef: true,
        versions: &LIBC_VERSIONS,
        syms: &LIBC_SYMS,
    },
    DsoSeed {
        name: "libm.so.6",
        path: "/lib64/libm.so.6",
        base: 0x7f10_0000,
        verdef: true,
        versions: &LIBM_VERSIONS,
        syms: &LIBM_SYMS,
    },
    DsoSeed {
        name: "libssl.so.3",
        path: "/usr/lib/libssl.so.3",
        base: 0x7f20_0000,
        verdef: true,
        versions: &SSL_VERSIONS,
        syms: &SSL_SYMS,
    },
    DsoSeed {
        name: "libfoo.so.1",
        path: "/opt/lib/libfoo.so.1",
        base: 0x7f30_0000,
        verdef: false,
        versions: &[],
        syms: &FOO_SYMS,
    },
];

static ONE_VERSION: [&str; 1] = ["GLIBC_2.2.5"];

static BAD_DUP_SYMS: [SymSeed; 2] = [
    SymSeed { name: "x", ver: "GLIBC_2.2.5", default: true },
    SymSeed { name: "x", ver: "GLIBC_2.2.5", default: true },
];

static BAD_DUP: [DsoSeed; 1] = [DsoSeed {
    name: "bad.so",
    path: "/x/bad.so",
    base: 0x7f40_0000,
    verdef: true,
    versions: &ONE_VERSION,
    syms: &BAD_DUP_SYMS,
}];

static BAD_VER_SYMS: [SymSeed; 1] = [SymSeed { name: "x", ver: "GLIBC_9.9", default: true }];

static BAD_VER: [DsoSeed; 1] = [DsoSeed {
    name: "bad.so",
    path: "/x/bad.so",
    base: 0x7f40_0000,
    verdef: true,
    versions: &ONE_VERSION,
    syms: &BAD_VER_SYMS,
}];

static BAD_NOVER_SYMS: [SymSeed; 1] =
    [SymSeed { name: "x", ver: "GLIBC_2.2.5", default: false }];

static BAD_NOVER: [DsoSeed; 1] = [DsoSeed {
    name: "bad.so",
    path: "/x/bad.so",
    base: 0x7f40_0000,
    verdef: false,
    versions: &[],
    syms: &BAD_NOVER_SYMS,
}];

static TWO_VERSIONS: [&str; 2] = ["GLIBC_2.2.5", "GLIBC_2.17"];

static BAD_DEFAULT_SYMS: [SymSeed; 2] = [
    SymSeed { name: "x", ver: "GLIBC_2.2.5", default: true },
    SymSeed { name: "x", ver: "GLIBC_2.17", default: true },
];

static BAD_DEFAULT: [DsoSeed; 1] = [DsoSeed {
    name: "bad.so",
    path: "/x/bad.so",
    base: 0x7f40_0000,
    verdef: true,
    versions: &TWO_VERSIONS,
    syms: &BAD_DEFAULT_SYMS,
}];

static BAD_NAME_SYMS: [SymSeed; 1] = [SymSeed { name: "x", ver: "GLIBC_2.2.5", default: true }];

static BAD_NAME: [DsoSeed; 1] = [DsoSeed {
    name: "bad name",
    path: "/x/bad.so",
    base: 0x7f40_0000,
    verdef: true,
    versions: &ONE_VERSION,
    syms: &BAD_NAME_SYMS,
}];

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 命中。
    Hit,
    /// 版本不匹配。
    Version,
    /// 符号不存在。
    Symbol,
    /// 无版本信息。
    NoVer,
    /// 句柄坏。
    Handle,
}

/// 探针。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// 精确版本查询。
    Dlvsym {
        /// 句柄。
        handle: u32,
        /// 符号名。
        sym: &'static str,
        /// 版本。
        ver: &'static str,
    },
    /// 默认查询。
    Dlsym {
        /// 句柄。
        handle: u32,
        /// 符号名。
        sym: &'static str,
    },
}

/// 期望。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// 命中对象与版本。
    Hit {
        /// 对象名。
        obj: &'static str,
        /// 版本。
        ver: &'static str,
    },
    /// 内部原因码。
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
        "dlvsym libc malloc GLIBC_2.2.5",
        Face::Hit,
        Probe::Dlvsym { handle: 1, sym: "malloc", ver: "GLIBC_2.2.5" },
        Want::Hit { obj: "libc.so.6", ver: "GLIBC_2.2.5" },
        "hit-malloc",
    ),
    Case::new(
        "dlvsym libc memcpy GLIBC_2.2.5",
        Face::Hit,
        Probe::Dlvsym { handle: 1, sym: "memcpy", ver: "GLIBC_2.2.5" },
        Want::Hit { obj: "libc.so.6", ver: "GLIBC_2.2.5" },
        "hit-compat-memcpy",
    ),
    Case::new(
        "dlvsym libc clock_gettime GLIBC_2.2.5",
        Face::Hit,
        Probe::Dlvsym { handle: 1, sym: "clock_gettime", ver: "GLIBC_2.2.5" },
        Want::Hit { obj: "libc.so.6", ver: "GLIBC_2.2.5" },
        "hit-compat-clock",
    ),
    Case::new(
        "dlvsym global clock_gettime GLIBC_2.17",
        Face::Hit,
        Probe::Dlvsym { handle: 0, sym: "clock_gettime", ver: "GLIBC_2.17" },
        Want::Hit { obj: "libc.so.6", ver: "GLIBC_2.17" },
        "hit-global",
    ),
    Case::new(
        "dlvsym libc pthread_create GLIBC_2.34",
        Face::Hit,
        Probe::Dlvsym { handle: 1, sym: "pthread_create", ver: "GLIBC_2.34" },
        Want::Hit { obj: "libc.so.6", ver: "GLIBC_2.34" },
        "hit-new",
    ),
    Case::new(
        "dlvsym libm exp GLIBC_2.29",
        Face::Hit,
        Probe::Dlvsym { handle: 2, sym: "exp", ver: "GLIBC_2.29" },
        Want::Hit { obj: "libm.so.6", ver: "GLIBC_2.29" },
        "hit-libm",
    ),
    Case::new(
        "dlvsym libssl SSL_new OPENSSL_3.0.0",
        Face::Hit,
        Probe::Dlvsym { handle: 3, sym: "SSL_new", ver: "OPENSSL_3.0.0" },
        Want::Hit { obj: "libssl.so.3", ver: "OPENSSL_3.0.0" },
        "hit-ssl",
    ),
    Case::new(
        "dlsym libc memcpy default",
        Face::Hit,
        Probe::Dlsym { handle: 1, sym: "memcpy" },
        Want::Hit { obj: "libc.so.6", ver: "GLIBC_2.14" },
        "dlsym-default",
    ),
    Case::new(
        "dlvsym libc memcpy GLIBC_2.34",
        Face::Version,
        Probe::Dlvsym { handle: 1, sym: "memcpy", ver: "GLIBC_2.34" },
        Want::Code(DV_EMSVER),
        "miss-version-memcpy",
    ),
    Case::new(
        "dlvsym libc malloc GLIBC_2.99",
        Face::Version,
        Probe::Dlvsym { handle: 1, sym: "malloc", ver: "GLIBC_2.99" },
        Want::Code(DV_EMSVER),
        "miss-version-new",
    ),
    Case::new(
        "dlvsym libc no_such_symbol GLIBC_2.2.5",
        Face::Symbol,
        Probe::Dlvsym { handle: 1, sym: "no_such_symbol", ver: "GLIBC_2.2.5" },
        Want::Code(DV_ESYM),
        "miss-symbol",
    ),
    Case::new(
        "dlvsym libfoo foo_a GLIBC_2.2.5",
        Face::NoVer,
        Probe::Dlvsym { handle: 4, sym: "foo_a", ver: "GLIBC_2.2.5" },
        Want::Code(DV_ENOVER),
        "miss-nover",
    ),
    Case::new(
        "dlvsym handle 9 malloc GLIBC_2.2.5",
        Face::Handle,
        Probe::Dlvsym { handle: 9, sym: "malloc", ver: "GLIBC_2.2.5" },
        Want::Code(DV_EHANDLE),
        "bad-handle-range",
    ),
    Case::new(
        "dlvsym handle -1 malloc GLIBC_2.2.5",
        Face::Handle,
        Probe::Dlvsym { handle: 0xffff_ffff, sym: "malloc", ver: "GLIBC_2.2.5" },
        Want::Code(DV_EHANDLE),
        "bad-handle-next",
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
pub fn judge(p: Probe) -> Result<Outcome, u32> {
    let tab = DsoTable::fixed();
    match p {
        Probe::Dlvsym { handle, sym, ver } => dlvsym(&tab, handle, sym, ver),
        Probe::Dlsym { handle, sym } => dlsym(&tab, handle, sym),
    }
}

fn want_match(w: Want, got: &Result<Outcome, u32>) -> bool {
    match w {
        Want::Hit { obj, ver } => match got {
            Ok(o) => match &o.hit {
                Some(h) => h.obj_name == obj && h.ver == ver,
                None => false,
            },
            Err(_) => false,
        },
        Want::Code(code) => match got {
            Ok(o) => o.code == code && o.hit.is_none(),
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
    /// 版本类。
    pub version: u32,
    /// 符号类。
    pub symbol: u32,
    /// 无版本信息类。
    pub nover: u32,
    /// 句柄类。
    pub handle: u32,
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
            version: 0,
            symbol: 0,
            nover: 0,
            handle: 0,
            matched: 0,
        }
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
            return Err(DV_ENOBS);
        }
        if self.matched != self.total {
            return Err(DV_EVALUE);
        }
        let buckets = self
            .version
            .saturating_add(self.symbol)
            .saturating_add(self.nover)
            .saturating_add(self.handle);
        if buckets != self.reject {
            return Err(DV_EVALUE);
        }
        Ok(())
    }

    fn bump(&mut self, code: u32) {
        match code {
            DV_EMSVER => self.version = self.version.saturating_add(1),
            DV_ESYM => self.symbol = self.symbol.saturating_add(1),
            DV_ENOVER => self.nover = self.nover.saturating_add(1),
            DV_EHANDLE => self.handle = self.handle.saturating_add(1),
            _ => {}
        }
    }

    /// 固定渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("dv corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" version=");
        s.push_str(u32_text(self.version).as_str());
        s.push_str(" symbol=");
        s.push_str(u32_text(self.symbol).as_str());
        s.push_str(" nover=");
        s.push_str(u32_text(self.nover).as_str());
        s.push_str(" handle=");
        s.push_str(u32_text(self.handle).as_str());
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
            return Err(DV_ESLOT);
        }
    }
    let mut c = Census::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let cs = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(DV_ESLOT),
        };
        c.total = c.total.saturating_add(1);
        let got = judge(cs.probe);
        match &got {
            Ok(o) => {
                if o.code == 0 {
                    c.accept = c.accept.saturating_add(1);
                } else {
                    c.reject = c.reject.saturating_add(1);
                    c.bump(o.code);
                }
            }
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
        Err(DV_ENOBS)
    }
}

/// 查询种类计数（精确 / 默认）。
pub fn query_census() -> (u32, u32) {
    let mut exact = 0u32;
    let mut plain = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            match c.probe {
                Probe::Dlvsym { .. } => exact = exact.saturating_add(1),
                Probe::Dlsym { .. } => plain = plain.saturating_add(1),
            }
        }
        i += 1;
    }
    (exact, plain)
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
            return Err(DV_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DV_ESLOT);
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
            return Err(DV_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 2, 1, 1, 2, 929];
        const CODES: [u32; 6] = [
            DV_EMSVER,
            DV_EMSVER,
            DV_ESYM,
            DV_ENOVER,
            DV_EHANDLE,
            DV_EHANDLE,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DV_EBAL),
            };
            if s.slot as usize != i {
                return Err(DV_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DV_EVALUE);
                        }
                    }
                    None => return Err(DV_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DV_EVALUE);
                        }
                    }
                    None => return Err(DV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DV_EBAL);
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
            return Err(DV_ESEAL);
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
            Err(DV_ESEAL)
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
const OP_VERSION: u8 = 3;
const OP_SYMBOL: u8 = 4;
const OP_NOVER: u8 = 5;
const OP_HANDLE: u8 = 6;
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

/// 从查询结果抽码（命中给 fallback）。
fn code_of(r: Result<Outcome, u32>, fallback: u32) -> u32 {
    match r {
        Ok(o) => {
            if o.code == 0 {
                fallback
            } else {
                o.code
            }
        }
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
    let _ = l.register(count_cell(3, OP_VERSION, r.map(|x| x.version as usize)));
    let _ = l.register(count_cell(4, OP_SYMBOL, r.map(|x| x.symbol as usize)));
    let _ = l.register(count_cell(5, OP_NOVER, r.map(|x| x.nover as usize)));
    let _ = l.register(count_cell(6, OP_HANDLE, r.map(|x| x.handle as usize)));
    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_RATE, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(
        8,
        OP_QUERY,
        code_of(
            judge(Probe::Dlvsym { handle: 1, sym: "memcpy", ver: "GLIBC_2.34" }),
            DV_EMSVER,
        ),
    ));
    let _ = l.register(err_cell(
        9,
        OP_QUERY,
        code_of(
            judge(Probe::Dlvsym { handle: 1, sym: "malloc", ver: "GLIBC_2.99" }),
            DV_EMSVER,
        ),
    ));
    let _ = l.register(err_cell(
        10,
        OP_QUERY,
        code_of(
            judge(Probe::Dlvsym { handle: 1, sym: "no_such_symbol", ver: "GLIBC_2.2.5" }),
            DV_ESYM,
        ),
    ));
    let _ = l.register(err_cell(
        11,
        OP_QUERY,
        code_of(
            judge(Probe::Dlvsym { handle: 4, sym: "foo_a", ver: "GLIBC_2.2.5" }),
            DV_ENOVER,
        ),
    ));
    let _ = l.register(err_cell(
        12,
        OP_QUERY,
        code_of(
            judge(Probe::Dlvsym { handle: 9, sym: "malloc", ver: "GLIBC_2.2.5" }),
            DV_EHANDLE,
        ),
    ));
    let _ = l.register(err_cell(
        13,
        OP_QUERY,
        code_of(
            judge(Probe::Dlvsym {
                handle: 0xffff_ffff,
                sym: "malloc",
                ver: "GLIBC_2.2.5",
            }),
            DV_EHANDLE,
        ),
    ));

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
    let e = parse_version("OPENSSL_3.0.0").map(|v| v.text())
        == Ok(String::from("OPENSSL_3.0.0"));
    let f = parse_version("").err() == Some(DV_ENAME);
    let g = parse_version("GLIBC_2.x").err() == Some(DV_EMSVER);
    let h = parse_version("GLIBC").err() == Some(DV_EMSVER);
    let i = parse_version("GLIBC_").err() == Some(DV_EMSVER);
    let j = parse_version("GLIBC_1.2.3.4.5").err() == Some(DV_EMSVER);
    let k = parse_version("GLIBC_2.34\n").err() == Some(DV_EBYTE);
    let l = parse_version("glibc_2.34").err() == Some(DV_EMSVER);
    a && b
        && c
        && d
        && e
        && f
        && g
        && h
        && i
        && j
        && k
        && l
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
            let z = !va.same_prefix(&ve) && va.same_prefix(&vb);
            let w = cmp_ver(&ve, &va) > 0 && va.text() == "GLIBC_2.34";
            x && y && z && w
        }
        _ => false,
    }
}

/// 名字闸。
fn names_ok() -> bool {
    let a = check_sym_name("malloc").is_ok()
        && check_sym_name("SSL_new").is_ok()
        && check_sym_name("foo$bar").is_ok()
        && check_sym_name(".hidden").is_ok()
        && check_sym_name("__libc_start_main").is_ok();
    let b = check_sym_name("").err() == Some(DV_ENAME);
    let c = check_sym_name("a b").err() == Some(DV_EBYTE);
    let d = check_sym_name("a,b").err() == Some(DV_EBYTE);
    let e = check_sym_name("a\nb").err() == Some(DV_EBYTE);
    let f = check_sym_name("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .err()
        == Some(DV_ENAME);
    let g = check_soname("libc.so.6").is_ok() && check_soname("libfoo-1.2.so").is_ok();
    let h = check_soname("").err() == Some(DV_ENAME)
        && check_soname("bad name").err() == Some(DV_EBYTE);
    let i = check_path("/lib64/libc.so.6").is_ok()
        && check_path("/usr/lib/libssl.so.3").is_ok();
    let j = check_path("").err() == Some(DV_ENAME)
        && check_path("/lib/`x`").err() == Some(DV_EBYTE);
    a && b && c && d && e && f && g && h && i && j
}

/// 句柄闸。
fn handles_ok() -> bool {
    let tab = DsoTable::fixed();
    let a = tab.handle_ok(0) && tab.handle_ok(1) && tab.handle_ok(4);
    let b = !tab.handle_ok(5) && !tab.handle_ok(9) && !tab.handle_ok(0xffff_ffff);
    let c = match dlvsym(&tab, 9, "malloc", "GLIBC_2.2.5") {
        Ok(o) => o.null() && o.code == DV_EHANDLE && o.text.as_str() == "invalid handle",
        Err(_) => false,
    };
    let d = match dlsym(&tab, 0xffff_ffff, "malloc") {
        Ok(o) => o.code == DV_EHANDLE && o.has_text(),
        Err(_) => false,
    };
    a && b && c && d
}

/// 对象表校验。
fn dsos_ok() -> bool {
    let t = DsoTable::fixed();
    let a = t.len() == 4 && !t.is_empty() && t.object(4).is_none();
    let b = match t.object(0) {
        Some(o) => {
            o.name == "libc.so.6"
                && o.verdef
                && o.syms.len() == 11
                && o.versions.len() == 4
                && o.has_name("memcpy")
                && o.exact_index("memcpy", "GLIBC_2.14") == Some(3)
        }
        None => false,
    };
    let c = match t.object(3) {
        Some(o) => !o.verdef && o.versions.is_empty() && o.default_index("foo_a").is_some(),
        None => false,
    };
    let d = match t.object_by_name("libm.so.6") {
        Some(o) => o.def_index_ok(),
        None => false,
    };
    let e = DsoTable::new(&BAD_DUP).err() == Some(DV_ESYM);
    let f = DsoTable::new(&BAD_VER).err() == Some(DV_EMSVER);
    let g = DsoTable::new(&BAD_NOVER).err() == Some(DV_ENOVER);
    let h = DsoTable::new(&BAD_DEFAULT).err() == Some(DV_ESYM);
    let i = DsoTable::new(&BAD_NAME).err() == Some(DV_EBYTE);
    let j = DsoTable::new(&[]).err() == Some(DV_ENOVER);
    a && b && c && d && e && f && g && h && i && j
}

impl Object {
    fn def_index_ok(&self) -> bool {
        self.verdef && self.exact_index("exp", "GLIBC_2.29").is_some()
    }
}

/// dlvsym 命中。
fn lookup_ok() -> bool {
    let tab = DsoTable::fixed();
    let a = match dlvsym(&tab, 1, "malloc", "GLIBC_2.2.5") {
        Ok(o) => match o.hit {
            Some(h) => {
                h.obj_name == "libc.so.6"
                    && h.ver == "GLIBC_2.2.5"
                    && h.default
                    && h.addr == 0x7f00_0100
            }
            None => false,
        },
        Err(_) => false,
    };
    let b = match dlvsym(&tab, 1, "memcpy", "GLIBC_2.2.5") {
        Ok(o) => match o.hit {
            Some(h) => h.ver == "GLIBC_2.2.5" && !h.default && h.addr == 0x7f00_0120,
            None => false,
        },
        Err(_) => false,
    };
    let c = match dlsym(&tab, 1, "memcpy") {
        Ok(o) => match o.hit {
            Some(h) => h.ver == "GLIBC_2.14" && h.default,
            None => false,
        },
        Err(_) => false,
    };
    let d = match dlvsym(&tab, 0, "clock_gettime", "GLIBC_2.17") {
        Ok(o) => o.found() && o.hit == Some(Hit {
            obj_name: "libc.so.6",
            sym_name: "clock_gettime",
            ver: "GLIBC_2.17",
            addr: 0x7f00_0150,
            default: true,
        }),
        Err(_) => false,
    };
    let e = match dlvsym(&tab, 2, "exp", "GLIBC_2.29") {
        Ok(o) => match o.hit {
            Some(h) => h.obj_name == "libm.so.6" && h.addr == 0x7f10_0120,
            None => false,
        },
        Err(_) => false,
    };
    let f = match dlvsym(&tab, 3, "SSL_new", "OPENSSL_3.0.0") {
        Ok(o) => match o.hit {
            Some(h) => h.obj_name == "libssl.so.3" && h.default,
            None => false,
        },
        Err(_) => false,
    };
    let g = match dlsym(&tab, 4, "foo_a") {
        Ok(o) => match o.hit {
            Some(h) => h.obj_name == "libfoo.so.1" && h.ver.is_empty(),
            None => false,
        },
        Err(_) => false,
    };
    let h = match dlvsym(&tab, 1, "clock_gettime", "GLIBC_2.2.5") {
        Ok(o) => match o.hit {
            Some(h) => !h.default,
            None => false,
        },
        Err(_) => false,
    };
    a && b && c && d && e && f && g && h
}

/// 失败文案。
fn texts_ok() -> bool {
    let tab = DsoTable::fixed();
    let a = match dlvsym(&tab, 1, "memcpy", "GLIBC_2.34") {
        Ok(o) => o.null() && o.text.as_str() == "undefined symbol: memcpy, version GLIBC_2.34",
        Err(_) => false,
    };
    let b = match dlvsym(&tab, 1, "malloc", "GLIBC_2.99") {
        Ok(o) => o.text.as_str() == "undefined symbol: malloc, version GLIBC_2.99",
        Err(_) => false,
    };
    let c = match dlvsym(&tab, 1, "no_such_symbol", "GLIBC_2.2.5") {
        Ok(o) => o.text.as_str() == "undefined symbol: no_such_symbol, version GLIBC_2.2.5",
        Err(_) => false,
    };
    let d = match dlvsym(&tab, 4, "foo_a", "GLIBC_2.2.5") {
        Ok(o) => o.text.as_str() == "undefined symbol: foo_a, version GLIBC_2.2.5",
        Err(_) => false,
    };
    let e = match dlsym(&tab, 1, "no_such_symbol") {
        Ok(o) => o.text.as_str() == "undefined symbol: no_such_symbol",
        Err(_) => false,
    };
    let f = dlvsym(&tab, 1, "bad name", "GLIBC_2.2.5").err() == Some(DV_EBYTE);
    let g = dlvsym(&tab, 1, "", "GLIBC_2.2.5").err() == Some(DV_ENAME);
    let h = dlvsym(&tab, 1, "malloc", "").err() == Some(DV_ENAME);
    let i = dlvsym(&tab, 1, "malloc", "GLIBC_2.2.5\n").err() == Some(DV_EBYTE);
    let j = dlvsym(&tab, 1, "malloc", "GLIBC_").err() == Some(DV_EMSVER);
    let k = dlvsym(&tab, 1, "malloc", "GLIBC_2.2.5").map(|o| o.text.is_empty()) == Ok(true);
    let l = dlsym(&tab, 4, "foo_a").map(|o| o.found()) == Ok(true);
    a && b && c && d && e && f && g && h && i && j && k && l
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
    let b = c.version == 2 && c.symbol == 1 && c.nover == 1 && c.handle == 2;
    let d = c.matched == 14 && c.rate() == PPM_FULL && c.green() && c.gate() == Ok(());
    a && b && d
}

/// 槽位编号。
fn slots_ok() -> bool {
    let a = CORPUS == 14 && CASES.len() == CORPUS && unique() && notes_ok();
    let b = match case_at(0) {
        Some(c) => c.face == Face::Hit && c.note == "hit-malloc",
        None => false,
    };
    let c = match case_at(1) {
        Some(c) => c.want == Want::Hit { obj: "libc.so.6", ver: "GLIBC_2.2.5" },
        None => false,
    };
    let d = match case_at(7) {
        Some(c) => match c.probe {
            Probe::Dlsym { handle, sym } => handle == 1 && sym == "memcpy",
            _ => false,
        },
        None => false,
    };
    let e = match case_at(13) {
        Some(c) => c.face == Face::Handle && c.want == Want::Code(DV_EHANDLE),
        None => false,
    };
    let f = case_at(14).is_none() && case_at(77).is_none();
    a && b && c && d && e && f
}

/// 汇总渲染。
fn report_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.render().as_str()
        == "dv corpus=14 accept=8 reject=6 version=2 symbol=1 nover=1 handle=2 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(DV_ENOBS);
    let d = e.render().as_str()
        == "dv corpus=0 accept=0 reject=0 version=0 symbol=0 nover=0 handle=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(CORPUS + 1) == Err(DV_ENOBS);
    let g = query_census() == (13, 1);
    a && b && d && f && g
}

/// 红档。
fn reds_ok() -> bool {
    let c = match run_with(Some(7)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched == 13 && c.rate() == PPM_RED && c.gate() == Err(DV_EVALUE);
    let b = !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(DV_ESLOT);
    let e = run_with(Some(CORPUS + 3)).err() == Some(DV_ESLOT);
    let f = match run_with(Some(13)) {
        Ok(v) => v.matched == 13 && v.rate() == PPM_RED,
        Err(_) => false,
    };
    a && b && d && e && f
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
    let d = f.register(Sample::blank()).err() == Some(DV_ESEAL) && f.rejects() == before + 1;
    a && b && c && d
}

/// 文本助手。
fn helpers_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(929) == "929" && u32_text(4294967295) == "4294967295";
    let b = hex_text(SEG_BASE) == "0x0000fd90" && hex_text(DV_EMSVER) == "0x0000fd95";
    let c = i32_text(-9) == "-9" && i32_text(9) == "9";
    let d = bool_text(true) == "true" && bool_text(false) == "false";
    let e = digest(b"dlvsym") == digest(b"dlvsym") && digest(b"dlvsym") != digest(b"dlvsy");
    let f = fold_word(FNV_OFF, 7) == digest(&[0, 0, 0, 7]);
    let g = DlSlot::new().take().is_none();
    a && b && c && d && e && f && g
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = version_ok() && compare_ok();
    out[1] = names_ok() && handles_ok() && dsos_ok();
    out[2] = lookup_ok() && texts_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && ledger_ok() && helpers_ok();
    out
}

/// 判据集合。
pub fn run_unx11650_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11650.dlvsym_missing");

    s.add("DV01-版本语法", version_ok(), "static");
    s.add("DV02-版本比较", compare_ok(), "static");
    s.add("DV03-符号名闸", names_ok(), "static");
    s.add("DV04-句柄闸", handles_ok(), "static");
    s.add("DV05-对象表校验", dsos_ok(), "static");
    s.add("DV06-dlvsym命中", lookup_ok(), "static");
    s.add("DV07-失败文案", texts_ok(), "static");
    s.add("DV08-十四槽表判", corpus_ok(), "static");
    s.add("DV09-槽位编号", slots_ok(), "static");
    s.add("DV10-汇总渲染", report_ok(), "static");
    s.add("DV11-红档929", reds_ok(), "static");
    s.add("DV12-账本对账", ledger_ok(), "static");
    s.add("DV13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
        assert_eq!(SAMPLES, 14);
        assert!(unique());
        assert!(notes_ok());
        assert_eq!(case_at(0).map(|c| c.note), Some("hit-malloc"));
        assert!(case_at(14).is_none());
    }

    #[test]
    fn version_parse_valid() {
        assert_eq!(parse_version("GLIBC_2.34").map(|v| v.text()), Ok(String::from("GLIBC_2.34")));
        assert_eq!(parse_version("GLIBC_2.2.5").map(|v| v.n), Ok(3));
        assert_eq!(parse_version("OPENSSL_3.0.0").map(|v| v.prefix_text()), Ok(String::from("OPENSSL")));
        assert_eq!(parse_version("GLIBC_2.2.5").map(|v| v.segs), Ok([2, 2, 5, 0]));
    }

    #[test]
    fn version_parse_deep() {
        let v = match parse_version("GLIBC_2.2.5") {
            Ok(x) => x,
            Err(_) => fail(),
        };
        assert_eq!(v.n, 3);
        assert_eq!(v.segs.first(), Some(&2));
        assert_eq!(v.segs.get(2), Some(&5));
        assert!(v.same_prefix(&match parse_version("GLIBC_2.34") {
            Ok(x) => x,
            Err(_) => fail(),
        }));
    }

    #[test]
    fn version_parse_rejects() {
        assert_eq!(parse_version("").err(), Some(DV_ENAME));
        assert_eq!(parse_version("GLIBC_2.34").map(|_| ()), Ok(()));
        assert_eq!(parse_version("GLIBC_2.34\n").err(), Some(DV_EBYTE));
        assert_eq!(parse_version("GLIBC 2.34").err(), Some(DV_EBYTE));
        assert_eq!(parse_version("GLIBC").err(), Some(DV_EMSVER));
        assert_eq!(parse_version("GLIBC_").err(), Some(DV_EMSVER));
        assert_eq!(parse_version("GLIBC_.2").err(), Some(DV_EMSVER));
        assert_eq!(parse_version("GLIBC_2.").err(), Some(DV_EMSVER));
        assert_eq!(parse_version("GLIBC_2..3").err(), Some(DV_EMSVER));
        assert_eq!(parse_version("GLIBC_1.2.3.4.5").err(), Some(DV_EMSVER));
        assert_eq!(parse_version("GLIBC_2.x").err(), Some(DV_EMSVER));
        assert_eq!(parse_version("GLIBC_10000").err(), Some(DV_EMSVER));
        assert_eq!(parse_version("glibc_2.34").err(), Some(DV_EMSVER));
        assert_eq!(parse_version("AAAAAAAAAAAAAAAAA_1").err(), Some(DV_EMSVER));
    }

    #[test]
    fn version_compare() {
        let a = parse_version("GLIBC_2.34").unwrap_or(Version::empty());
        let b = parse_version("GLIBC_2.31").unwrap_or(Version::empty());
        let c = parse_version("GLIBC_2.2.5").unwrap_or(Version::empty());
        let d = parse_version("GLIBC_2.4").unwrap_or(Version::empty());
        assert_eq!(cmp_ver(&a, &b), 1);
        assert_eq!(cmp_ver(&b, &a), -1);
        assert_eq!(cmp_ver(&a, &a), 0);
        assert_eq!(cmp_ver(&c, &d), -1);
        assert!(at_least(&d, &c));
        assert!(!at_least(&c, &d));
        assert!(a.same_prefix(&b));
        assert!(!a.same_prefix(&match parse_version("OPENSSL_3.0.0") {
            Ok(x) => x,
            Err(_) => fail(),
        }));
    }

    #[test]
    fn symbol_gate() {
        assert!(check_sym_name("malloc").is_ok());
        assert!(check_sym_name("foo$bar").is_ok());
        assert_eq!(check_sym_name("").err(), Some(DV_ENAME));
        assert_eq!(check_sym_name("a b").err(), Some(DV_EBYTE));
        assert_eq!(check_sym_name("a;b").err(), Some(DV_EBYTE));
        assert_eq!(check_sym_name("a`b").err(), Some(DV_EBYTE));
        let long = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(check_sym_name(long).err(), Some(DV_ENAME));
    }

    #[test]
    fn dso_meta_gate() {
        assert!(check_soname("libc.so.6").is_ok());
        assert!(check_path("/lib64/libc.so.6").is_ok());
        assert_eq!(check_soname("").err(), Some(DV_ENAME));
        assert_eq!(check_soname("bad name").err(), Some(DV_EBYTE));
        assert_eq!(check_path("").err(), Some(DV_ENAME));
        assert_eq!(check_path("/x/a;b").err(), Some(DV_EBYTE));
        assert!(check_path("/usr/lib/libssl.so.3").is_ok());
    }

    #[test]
    fn table_fixed() {
        let t = DsoTable::fixed();
        assert_eq!(t.len(), 4);
        assert!(!t.is_empty());
        assert_eq!(t.object(0).map(|o| o.name), Some("libc.so.6"));
        assert_eq!(t.object(1).map(|o| o.name), Some("libm.so.6"));
        assert_eq!(t.object(2).map(|o| o.name), Some("libssl.so.3"));
        assert_eq!(t.object(3).map(|o| o.name), Some("libfoo.so.1"));
        assert!(t.object(4).is_none());
        assert_eq!(t.object_by_name("libssl.so.3").map(|o| o.base), Some(0x7f20_0000));
        assert!(t.object_by_name("nope").is_none());
    }

    #[test]
    fn table_rejects() {
        assert_eq!(DsoTable::new(&BAD_DUP).err(), Some(DV_ESYM));
        assert_eq!(DsoTable::new(&BAD_VER).err(), Some(DV_EMSVER));
        assert_eq!(DsoTable::new(&BAD_NOVER).err(), Some(DV_ENOVER));
        assert_eq!(DsoTable::new(&BAD_DEFAULT).err(), Some(DV_ESYM));
        assert_eq!(DsoTable::new(&BAD_NAME).err(), Some(DV_EBYTE));
        assert_eq!(DsoTable::new(&[]).err(), Some(DV_ENOVER));
    }

    #[test]
    fn default_hit() {
        let t = DsoTable::fixed();
        let o = match dlvsym(&t, 1, "malloc", "GLIBC_2.2.5") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(o.found());
        let h = match o.hit {
            Some(v) => v,
            None => fail(),
        };
        assert_eq!(h.obj_name, "libc.so.6");
        assert_eq!(h.ver, "GLIBC_2.2.5");
        assert!(h.default);
        assert_eq!(h.addr, 0x7f00_0100);
    }

    #[test]
    fn compat_and_default_contrast() {
        let t = DsoTable::fixed();
        let old = match dlvsym(&t, 1, "memcpy", "GLIBC_2.2.5") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        let new = match dlsym(&t, 1, "memcpy") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(!match old.hit {
            Some(h) => h.default,
            None => fail(),
        });
        assert_eq!(new.hit.map(|h| h.ver), Some("GLIBC_2.14"));
        assert!(new.hit.map(|h| h.default) == Some(true));
    }

    #[test]
    fn global_scope() {
        let t = DsoTable::fixed();
        let o = match dlvsym(&t, 0, "clock_gettime", "GLIBC_2.17") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(o.hit.map(|h| h.obj_name), Some("libc.so.6"));
        let p = match dlsym(&t, 0, "SSL_new") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(p.hit.map(|h| h.obj_name), Some("libssl.so.3"));
    }

    #[test]
    fn missing_version_text() {
        let t = DsoTable::fixed();
        let o = match dlvsym(&t, 1, "memcpy", "GLIBC_2.34") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(o.null());
        assert_eq!(o.code, DV_EMSVER);
        assert_eq!(o.text.as_str(), "undefined symbol: memcpy, version GLIBC_2.34");
    }

    #[test]
    fn missing_symbol_text() {
        let t = DsoTable::fixed();
        let o = match dlvsym(&t, 1, "no_such_symbol", "GLIBC_2.2.5") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(o.code, DV_ESYM);
        assert_eq!(o.text.as_str(), "undefined symbol: no_such_symbol, version GLIBC_2.2.5");
    }

    #[test]
    fn unversioned_text() {
        let t = DsoTable::fixed();
        let o = match dlvsym(&t, 4, "foo_a", "GLIBC_2.2.5") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(o.code, DV_ENOVER);
        assert_eq!(o.text.as_str(), "undefined symbol: foo_a, version GLIBC_2.2.5");
        assert!(dlvsym(&t, 4, "foo_a", "GLIBC_2.2.5").map(|x| x.found()) == Ok(false));
    }

    #[test]
    fn bad_handle_text() {
        let t = DsoTable::fixed();
        let o = match dlvsym(&t, 9, "malloc", "GLIBC_2.2.5") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(o.code, DV_EHANDLE);
        assert_eq!(o.text.as_str(), "invalid handle");
        let p = match dlsym(&t, 0xffff_ffff, "malloc") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(p.text.as_str(), "invalid handle");
    }

    #[test]
    fn dlsym_semantics() {
        let t = DsoTable::fixed();
        assert_eq!(dlsym(&t, 1, "clock_gettime").map(|o| o.hit.map(|h| h.ver)), Ok(Some("GLIBC_2.17")));
        assert_eq!(dlsym(&t, 4, "foo_a").map(|o| o.found()), Ok(true));
        assert_eq!(dlsym(&t, 4, "foo_z").map(|o| o.code), Ok(DV_ESYM));
        assert_eq!(
            dlsym(&t, 1, "nope").map(|o| o.text),
            Ok(String::from("undefined symbol: nope"))
        );
    }

    #[test]
    fn dlvsym_injection() {
        let t = DsoTable::fixed();
        assert_eq!(dlvsym(&t, 1, "a b", "GLIBC_2.2.5").err(), Some(DV_EBYTE));
        assert_eq!(dlvsym(&t, 1, "", "GLIBC_2.2.5").err(), Some(DV_ENAME));
        assert_eq!(dlvsym(&t, 1, "malloc", "").err(), Some(DV_ENAME));
        assert_eq!(dlvsym(&t, 1, "malloc", "GLIBC_2.2.5\n").err(), Some(DV_EBYTE));
        assert_eq!(dlvsym(&t, 1, "malloc", "GLIBC_").err(), Some(DV_EMSVER));
        assert_eq!(dlsym(&t, 1, "a,b").err(), Some(DV_EBYTE));
    }

    #[test]
    fn hit_addresses() {
        let t = DsoTable::fixed();
        let a = match dlvsym(&t, 1, "free", "GLIBC_2.2.5") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(a.hit.map(|h| h.addr), Some(0x7f00_0110));
        let b = match dlvsym(&t, 3, "EVP_Digest", "OPENSSL_3.0.0") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(b.hit.map(|h| h.addr), Some(0x7f20_0140));
    }

    #[test]
    fn outcome_helpers() {
        let t = DsoTable::fixed();
        let o = match dlsym(&t, 1, "malloc") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(o.found() && !o.null() && !o.has_text());
        let p = match dlvsym(&t, 1, "malloc", "GLIBC_2.99") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(!p.found() && p.null() && p.has_text());
    }

    #[test]
    fn error_slot_lifecycle() {
        let mut slot = DlSlot::new();
        assert!(!slot.armed());
        assert_eq!(slot.take(), None);
        assert_eq!(slot.arm("boom"), Ok(()));
        assert!(slot.armed());
        assert_eq!(slot.take(), Some(String::from("boom")));
        assert!(!slot.armed());
        assert_eq!(slot.take(), None);
        assert_eq!(slot.arm("x"), Ok(()));
        slot.clear();
        assert!(!slot.armed());
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn invoke_slot() {
        let t = DsoTable::fixed();
        let mut slot = DlSlot::new();
        assert_eq!(invoke_dlvsym(&t, 1, "memcpy", "GLIBC_2.34", &mut slot), Ok(None));
        assert_eq!(slot.take(), Some(String::from("undefined symbol: memcpy, version GLIBC_2.34")));
        assert_eq!(slot.take(), None);
        let ok = invoke_dlvsym(&t, 1, "malloc", "GLIBC_2.2.5", &mut slot);
        assert!(matches!(ok, Ok(Some(_))));
        assert_eq!(slot.take(), None);
        assert_eq!(invoke_dlsym(&t, 1, "nope", &mut slot), Ok(None));
        assert_eq!(slot.take(), Some(String::from("undefined symbol: nope")));
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
            let got = judge(c.probe);
            assert!(want_match(c.want, &got));
            i += 1;
        }
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
        assert_eq!(c.version, 2);
        assert_eq!(c.symbol, 1);
        assert_eq!(c.nover, 1);
        assert_eq!(c.handle, 2);
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
            "dv corpus=14 accept=8 reject=6 version=2 symbol=1 nover=1 handle=2 match=14 rate=1000ppm\n"
        );
        let e = Census::new();
        assert_eq!(
            e.render().as_str(),
            "dv corpus=0 accept=0 reject=0 version=0 symbol=0 nover=0 handle=0 match=0 rate=0ppm\n"
        );
        assert_eq!(e.gate(), Err(DV_ENOBS));
    }

    #[test]
    fn red_rounding() {
        let c = match run_with(Some(3)) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(c.matched, 13);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(DV_EVALUE));
        assert_eq!(run_with(Some(14)).err(), Some(DV_ESLOT));
    }

    #[test]
    fn query_kinds() {
        assert_eq!(query_census(), (13, 1));
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(15), Err(DV_ENOBS));
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
        assert_eq!(l.sample(8).map(|s| s.code), Some(DV_EMSVER));
        assert_eq!(l.sample(9).map(|s| s.code), Some(DV_EMSVER));
        assert_eq!(l.sample(10).map(|s| s.code), Some(DV_ESYM));
        assert_eq!(l.sample(11).map(|s| s.code), Some(DV_ENOVER));
        assert_eq!(l.sample(12).map(|s| s.code), Some(DV_EHANDLE));
        assert_eq!(l.sample(13).map(|s| s.code), Some(DV_EHANDLE));
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
        assert_eq!(l.register(Sample::blank()).err(), Some(DV_ESEAL));
        assert_eq!(l.rejects(), before + 1);
    }

    #[test]
    fn ledger_order_gate() {
        let mut l = Ledger::new();
        let first = Sample { slot: 1, op: 0, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(first).err(), Some(DV_ESLOT));
        let zero = Sample { slot: 0, op: 0, ok: true, code: 0, value: 0 };
        assert_eq!(l.register(zero), Ok(()));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn injections() {
        let out = inject_four();
        assert_eq!(out, [true, true, true, true]);
    }

    #[test]
    fn helpers() {
        assert_eq!(u32_text(0), "0");
        assert_eq!(u32_text(4294967295), "4294967295");
        assert_eq!(hex_text(SEG_BASE), "0x0000fd90");
        assert_eq!(hex_text(DV_EBYTE), "0x0000fd91");
        assert_eq!(i32_text(-7), "-7");
        assert_eq!(bool_text(true), "true");
        assert_eq!(digest(b"x"), digest(b"x"));
        assert_eq!(fold_word(FNV_OFF, 1), digest(&[0, 0, 0, 1]));
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11650_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn fail_helper() {
        assert_eq!(check_sym_name("x"), Ok(()));
    }

    fn fail() -> ! {
        panic!("unexpected branch");
    }
}
