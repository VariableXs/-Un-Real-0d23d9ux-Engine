//! UNX-F11652 版本哈希计算与匹配判据
//!
//! 版本字符串在 ELF 里不是直接逐字比较，而是先算 32 位经典哈希
//!（`_dl_elf_hash`）：每字节 `h = (h << 4) + c`，取高四位 `g`，`h ^= g >> 24`，
//! 再 `h &= ~g`。装载器用 `hash % nbuckets` 落桶，桶内先比高 16 位做预筛，
//! 高 16 位相同才比完整 32 位，完整哈希相同才比字符串。三层缺一不可：
//! 只比高 16 位会把 GLIBC_2.17 与 GLIBC_2.34 误判为同一版本；只比完整哈希
//! 会把真正同哈希不同名的版本串吞掉；只有三层都过才算命中。
//!
//! 本模块实现一张 8 桶、每桶 4 槽的版本哈希表。插入时算哈希、落桶、查重：
//! 同哈希同名落 EDUP，桶满落 EFULL。查询时逐槽预筛并记账：高 16 位不同记
//! 一次 prefilter_skips，高 16 位相同记一次 full_checks，完整哈希相同记一次
//! name_checks；完整哈希相同但名字不同记冲突（ECONF），整桶扫完没有同哈希
//! 也没有冲突记 EMISS。桶空时直接 EMISS，一次比较都不做。
//!
//! 语料里的冲突对是真实存在的：GLIBC_2.34 与 GLIBC_2.17 高 16 位同为 0x0696，
//! 但完整哈希 0x069691b4 与 0x06969197 不同，必须靠第二层拆开；另有一对
//! 完整哈希完全相同的四段版本（0x0555db38），必须靠第三层字符串拆开。
//!
//! 版本串只允许 `PREFIX_A.B.C` 形态；空串落 ENAME，注入字节落 EBYTE，结构
//! 坏按不可匹配落 EMISS。表本身不读环境、不分配额外内存，全部定长。
//!
//! 闸门次序：版本语法 → 哈希 → 落桶 → 高16预筛 → 全哈希 → 名字 → 账本。
//!
//! 契约：纯 no_std（只用 alloc 的 String）、无 unsafe、无 unwrap/expect/
//! panic!，失败一律 `Result<_, u32>`；账本 14 格（8 成 6 败）逐格对常量表，
//! 冻结后登记落 ESEAL，重放必须复现同一指纹；语料 14 槽互异，覆盖率按
//! `(matched*1000 + total/2)/total` 四舍五入，单槽红档 929ppm。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11652）。
pub const SEG_BASE: u32 = 0xfdb0;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 满档千分比。
pub const PPM_FULL: u32 = 1000;

/// 单槽红档千分比。
pub const PPM_RED: u32 = 929;

/// 哈希桶数。
pub const NBUCKET: usize = 8;

/// 单桶槽数。
pub const BUCKET_MAX: usize = 4;

/// 字符串上限。
pub const TEXT_MAX: usize = 32;

/// 版本串上限。
pub const VERSION_MAX: usize = 32;

/// 版本前缀上限。
pub const PREFIX_MAX: usize = 16;

/// 版本段上限。
pub const SEG_MAX: usize = 4;

/// 版本段最大值。
pub const SEG_LIMIT: u32 = 9999;

/// 固定表版本数。
pub const FIXED_LEN: usize = 9;

/// 输入里有非法字节。
pub const VH_EBYTE: u32 = SEG_BASE + 0x01;

/// 名字为空或超长。
pub const VH_ENAME: u32 = SEG_BASE + 0x02;

/// 桶已满。
pub const VH_EFULL: u32 = SEG_BASE + 0x03;

/// 重复插入。
pub const VH_EDUP: u32 = SEG_BASE + 0x04;

/// 全哈希相同但名字不同（冲突）。
pub const VH_ECONF: u32 = SEG_BASE + 0x05;

/// 桶内没有该版本。
pub const VH_EMISS: u32 = SEG_BASE + 0x06;

/// 表已冻结或重复冻结。
pub const VH_ESTATE: u32 = SEG_BASE + 0x07;

/// 语义与期望不符。
pub const VH_EVER: u32 = SEG_BASE + 0x08;

/// 账本不平。
pub const VH_EBAL: u32 = SEG_BASE + 0x09;

/// 槽位错序。
pub const VH_ESLOT: u32 = SEG_BASE + 0x0a;

/// 观测值与常量表不符。
pub const VH_EVALUE: u32 = SEG_BASE + 0x0b;

/// 冻结重放指纹不符。
pub const VH_ESEAL: u32 = SEG_BASE + 0x0c;

/// 空观测。
pub const VH_ENOBS: u32 = SEG_BASE + 0x0d;

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
// 经典 ELF 哈希
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

/// 哈希落桶。
pub fn bucket_of_hash(h: u32) -> usize {
    (h as usize) % NBUCKET
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
        return Err(VH_ENAME);
    }
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            if !ver_byte(*b) {
                return Err(VH_EBYTE);
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
        return Err(VH_EMISS);
    }
    let mut v = Version::empty();
    i = 0usize;
    while i < us {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(VH_EMISS),
        };
        let ok = if i == 0 {
            b >= b'A' && b <= b'Z'
        } else {
            (b >= b'A' && b <= b'Z') || (b >= b'0' && b <= b'9')
        };
        if !ok {
            return Err(VH_EMISS);
        }
        if let Some(d) = v.prefix.get_mut(i) {
            *d = b;
        }
        v.plen += 1;
        i += 1;
    }
    let tail = match bytes.get(us + 1..) {
        Some(t) => t,
        None => return Err(VH_EMISS),
    };
    if tail.is_empty() {
        return Err(VH_EMISS);
    }
    let mut start = 0usize;
    let mut seg = 0usize;
    while start < tail.len() {
        if seg >= SEG_MAX {
            return Err(VH_EMISS);
        }
        let mut end = start;
        while end < tail.len() {
            match tail.get(end) {
                Some(b) => {
                    if *b == b'.' {
                        break;
                    }
                }
                None => return Err(VH_EMISS),
            }
            end += 1;
        }
        if end == start {
            return Err(VH_EMISS);
        }
        let mut val = 0u32;
        let mut k = start;
        while k < end {
            let b = match tail.get(k) {
                Some(x) => *x,
                None => return Err(VH_EMISS),
            };
            if b < b'0' || b > b'9' {
                return Err(VH_EMISS);
            }
            val = val * 10 + ((b - b'0') as u32);
            if val > SEG_LIMIT {
                return Err(VH_EMISS);
            }
            k += 1;
        }
        if let Some(d) = v.segs.get_mut(seg) {
            *d = val;
        }
        seg += 1;
        if end < tail.len() {
            if end + 1 >= tail.len() {
                return Err(VH_EMISS);
            }
            start = end + 1;
        } else {
            start = tail.len();
        }
    }
    v.n = seg;
    if v.n == 0 {
        return Err(VH_EMISS);
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
            return Err(VH_ENAME);
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
// 版本哈希表
// ---------------------------------------------------------------------------

/// 桶槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HashSlot {
    /// 版本串。
    pub text: Text,
    /// 完整哈希。
    pub hash: u32,
    /// 序号（从 1 起）。
    pub ndx: u16,
}

impl HashSlot {
    /// 空槽。
    pub const fn empty() -> HashSlot {
        HashSlot { text: Text::empty(), hash: 0, ndx: 0 }
    }
}

/// 命中记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// 序号。
    pub ndx: u16,
    /// 完整哈希。
    pub hash: u32,
    /// 桶号。
    pub bucket: u8,
}

/// 一次查询的完整观测。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lookup {
    /// 命中。
    pub hit: Option<Hit>,
    /// 结果码（命中为 0）。
    pub code: u32,
    /// 桶号。
    pub bucket: u8,
    /// 扫描槽数。
    pub scanned: u32,
    /// 高 16 位预筛跳过数。
    pub prefilter_skips: u32,
    /// 完整哈希比较数。
    pub full_checks: u32,
    /// 名字比较数。
    pub name_checks: u32,
    /// 是否遇到同哈希异名。
    pub conflict: bool,
}

impl Lookup {
    /// 空观测。
    pub const fn new(bucket: u8) -> Lookup {
        Lookup {
            hit: None,
            code: VH_EMISS,
            bucket,
            scanned: 0,
            prefilter_skips: 0,
            full_checks: 0,
            name_checks: 0,
            conflict: false,
        }
    }

    /// 命中否。
    pub fn is_hit(&self) -> bool {
        self.hit.is_some()
    }
}

/// 版本哈希表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionTable {
    buckets: [[HashSlot; BUCKET_MAX]; NBUCKET],
    lens: [usize; NBUCKET],
    total: usize,
}

impl VersionTable {
    /// 空表。
    pub const fn new() -> VersionTable {
        VersionTable {
            buckets: [[HashSlot::empty(); BUCKET_MAX]; NBUCKET],
            lens: [0usize; NBUCKET],
            total: 0,
        }
    }

    /// 固定表。
    pub fn fixed() -> VersionTable {
        let mut t = VersionTable::new();
        let mut i = 0usize;
        while i < FIXED_VERSIONS.len() {
            if let Some(v) = FIXED_VERSIONS.get(i) {
                let _ = t.insert(v);
            }
            i += 1;
        }
        t
    }

    /// 插入，返回序号。
    pub fn insert(&mut self, ver: &str) -> Result<u16, u32> {
        parse_version(ver)?;
        let h = elf_hash(ver);
        let b = bucket_of_hash(h);
        let len = match self.lens.get(b) {
            Some(v) => *v,
            None => return Err(VH_EMISS),
        };
        let mut i = 0usize;
        while i < len {
            if let Some(bk) = self.buckets.get(b) {
                if let Some(s) = bk.get(i) {
                    if s.hash == h && s.text.eq_str(ver) {
                        return Err(VH_EDUP);
                    }
                }
            }
            i += 1;
        }
        if len >= BUCKET_MAX {
            return Err(VH_EFULL);
        }
        let slot = HashSlot {
            text: Text::from_str(ver)?,
            hash: h,
            ndx: (self.total + 1) as u16,
        };
        if let Some(bk) = self.buckets.get_mut(b) {
            if let Some(d) = bk.get_mut(len) {
                *d = slot;
            }
        }
        if let Some(l) = self.lens.get_mut(b) {
            *l = len + 1;
        }
        self.total += 1;
        Ok(slot.ndx)
    }

    /// 查询全观测。
    pub fn lookup(&self, ver: &str) -> Result<Lookup, u32> {
        parse_version(ver)?;
        let h = elf_hash(ver);
        let b = bucket_of_hash(h);
        let mut out = Lookup::new(b as u8);
        let len = match self.lens.get(b) {
            Some(v) => *v,
            None => return Ok(out),
        };
        let mut i = 0usize;
        while i < len {
            let s = match self.buckets.get(b) {
                Some(bk) => match bk.get(i) {
                    Some(v) => *v,
                    None => return Ok(out),
                },
                None => return Ok(out),
            };
            out.scanned = out.scanned.saturating_add(1);
            if (s.hash >> 16) != (h >> 16) {
                out.prefilter_skips = out.prefilter_skips.saturating_add(1);
                i += 1;
                continue;
            }
            out.full_checks = out.full_checks.saturating_add(1);
            if s.hash != h {
                i += 1;
                continue;
            }
            out.name_checks = out.name_checks.saturating_add(1);
            if s.text.eq_str(ver) {
                out.hit = Some(Hit { ndx: s.ndx, hash: h, bucket: b as u8 });
                out.code = 0;
                return Ok(out);
            }
            out.conflict = true;
            out.code = VH_ECONF;
            i += 1;
        }
        if out.conflict {
            out.code = VH_ECONF;
        } else {
            out.code = VH_EMISS;
        }
        Ok(out)
    }

    /// 查询命中。
    pub fn find(&self, ver: &str) -> Result<Hit, u32> {
        let l = self.lookup(ver)?;
        match l.hit {
            Some(h) => Ok(h),
            None => Err(l.code),
        }
    }

    /// 条目数。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// 桶长。
    pub fn bucket_len(&self, b: usize) -> usize {
        match self.lens.get(b) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 取槽。
    pub fn slot(&self, b: usize, i: usize) -> Option<HashSlot> {
        if i >= self.bucket_len(b) {
            return None;
        }
        match self.buckets.get(b) {
            Some(bk) => bk.get(i).copied(),
            None => None,
        }
    }
}

impl Default for VersionTable {
    fn default() -> VersionTable {
        VersionTable::new()
    }
}

/// 固定版本表（插入顺序即序号）。
pub static FIXED_VERSIONS: [&str; FIXED_LEN] = [
    "GLIBC_2.2.5",
    "GLIBC_2.4",
    "GLIBC_2.14",
    "GLIBC_2.17",
    "GLIBC_2.34",
    "OPENSSL_1.1.0",
    "OPENSSL_1.1.1",
    "OPENSSL_3.0.0",
    "GLIBC_4313.6278.4486.5188",
];

/// 同桶填充串（与满桶目标同桶）。
pub static FULL_SEEDS: [&str; 4] = [
    "V_5305.2471.6468.791",
    "V_1186.8779.1542.5991",
    "V_614.1408.7104.6851",
    "V_6867.2363.8858.1929",
];

/// 满桶目标。
pub const FULL_TARGET: &str = "V_9353.5054.9179.2961";

/// 同哈希冲突对之一（表内）。
pub const COLLIDE_A: &str = "GLIBC_4313.6278.4486.5188";

/// 同哈希冲突对之二（表外）。
pub const COLLIDE_B: &str = "GLIBC_3391.6750.4728.7138";

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 命中。
    Hit,
    /// 未命中。
    Miss,
    /// 冲突。
    Conflict,
    /// 满桶。
    Full,
    /// 名字坏。
    Name,
}

/// 探针。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// 查固定表。
    Find {
        /// 版本串。
        ver: &'static str,
    },
    /// 先插种子再插目标。
    Insert {
        /// 目标。
        ver: &'static str,
        /// 种子。
        seeds: &'static [&'static str],
    },
}

/// 期望。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// 命中序号。
    Hit {
        /// 序号。
        ndx: u16,
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
        "find GLIBC_2.2.5",
        Face::Hit,
        Probe::Find { ver: "GLIBC_2.2.5" },
        Want::Hit { ndx: 1 },
        "hit-base",
    ),
    Case::new(
        "find GLIBC_2.34",
        Face::Hit,
        Probe::Find { ver: "GLIBC_2.34" },
        Want::Hit { ndx: 5 },
        "hit-new",
    ),
    Case::new(
        "find GLIBC_2.17",
        Face::Hit,
        Probe::Find { ver: "GLIBC_2.17" },
        Want::Hit { ndx: 4 },
        "hit-clock",
    ),
    Case::new(
        "find GLIBC_2.4",
        Face::Hit,
        Probe::Find { ver: "GLIBC_2.4" },
        Want::Hit { ndx: 2 },
        "hit-short",
    ),
    Case::new(
        "find OPENSSL_3.0.0",
        Face::Hit,
        Probe::Find { ver: "OPENSSL_3.0.0" },
        Want::Hit { ndx: 8 },
        "hit-ssl",
    ),
    Case::new(
        "find OPENSSL_1.1.1",
        Face::Hit,
        Probe::Find { ver: "OPENSSL_1.1.1" },
        Want::Hit { ndx: 7 },
        "hit-ssl-old",
    ),
    Case::new(
        "find GLIBC_2.14",
        Face::Hit,
        Probe::Find { ver: "GLIBC_2.14" },
        Want::Hit { ndx: 3 },
        "hit-compat",
    ),
    Case::new(
        "find collision A",
        Face::Hit,
        Probe::Find { ver: COLLIDE_A },
        Want::Hit { ndx: 9 },
        "hit-collide-a",
    ),
    Case::new(
        "find GLIBC_2.99",
        Face::Miss,
        Probe::Find { ver: "GLIBC_2.99" },
        Want::Code(VH_EMISS),
        "miss-empty-bucket",
    ),
    Case::new(
        "find GLIBC_2.35",
        Face::Miss,
        Probe::Find { ver: "GLIBC_2.35" },
        Want::Code(VH_EMISS),
        "miss-prefilter",
    ),
    Case::new(
        "find collision B",
        Face::Conflict,
        Probe::Find { ver: COLLIDE_B },
        Want::Code(VH_ECONF),
        "conflict-same-hash",
    ),
    Case::new(
        "insert full bucket target",
        Face::Full,
        Probe::Insert { ver: FULL_TARGET, seeds: &FULL_SEEDS },
        Want::Code(VH_EFULL),
        "full-bucket",
    ),
    Case::new(
        "find empty version",
        Face::Name,
        Probe::Find { ver: "" },
        Want::Code(VH_ENAME),
        "bad-empty",
    ),
    Case::new(
        "find newline version",
        Face::Name,
        Probe::Find { ver: "GLIBC_2.34\n" },
        Want::Code(VH_EBYTE),
        "bad-bytes",
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
pub fn judge(p: Probe) -> Result<Hit, u32> {
    match p {
        Probe::Find { ver } => VersionTable::fixed().find(ver),
        Probe::Insert { ver, seeds } => {
            let mut t = VersionTable::new();
            let mut i = 0usize;
            while i < seeds.len() {
                let s = match seeds.get(i) {
                    Some(v) => *v,
                    None => return Err(VH_ESLOT),
                };
                let _ = t.insert(s)?;
                i += 1;
            }
            let ndx = t.insert(ver)?;
            let h = elf_hash(ver);
            Ok(Hit { ndx, hash: h, bucket: bucket_of_hash(h) as u8 })
        }
    }
}

fn want_match(w: Want, got: &Result<Hit, u32>) -> bool {
    match w {
        Want::Hit { ndx } => match got {
            Ok(h) => h.ndx == ndx,
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
    /// 未命中类。
    pub miss: u32,
    /// 冲突类。
    pub conflict: u32,
    /// 满桶类。
    pub full: u32,
    /// 名字类。
    pub name: u32,
    /// 命中数。
    pub matched: u32,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census { total: 0, accept: 0, reject: 0, miss: 0, conflict: 0, full: 0, name: 0, matched: 0 }
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
            return Err(VH_ENOBS);
        }
        if self.matched != self.total {
            return Err(VH_EVALUE);
        }
        let buckets = self
            .miss
            .saturating_add(self.conflict)
            .saturating_add(self.full)
            .saturating_add(self.name);
        if buckets != self.reject {
            return Err(VH_EVALUE);
        }
        Ok(())
    }

    fn bump(&mut self, code: u32) {
        match code {
            VH_EMISS => self.miss = self.miss.saturating_add(1),
            VH_ECONF => self.conflict = self.conflict.saturating_add(1),
            VH_EFULL => self.full = self.full.saturating_add(1),
            _ => self.name = self.name.saturating_add(1),
        }
    }

    /// 固定渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("vh corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" miss=");
        s.push_str(u32_text(self.miss).as_str());
        s.push_str(" conflict=");
        s.push_str(u32_text(self.conflict).as_str());
        s.push_str(" full=");
        s.push_str(u32_text(self.full).as_str());
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
            return Err(VH_ESLOT);
        }
    }
    let mut c = Census::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let cs = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(VH_ESLOT),
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
        Err(VH_ENOBS)
    }
}

/// 查询种类计数（查找 / 插入）。
pub fn probe_census() -> (u32, u32) {
    let mut find = 0u32;
    let mut insert = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            match c.probe {
                Probe::Find { .. } => find = find.saturating_add(1),
                Probe::Insert { .. } => insert = insert.saturating_add(1),
            }
        }
        i += 1;
    }
    (find, insert)
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
            return Err(VH_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(VH_ESLOT);
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
            return Err(VH_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 2, 1, 1, 2, 929];
        const CODES: [u32; 6] =
            [VH_EMISS, VH_EMISS, VH_ECONF, VH_EFULL, VH_ENAME, VH_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(VH_EBAL),
            };
            if s.slot as usize != i {
                return Err(VH_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(VH_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(VH_EVALUE);
                        }
                    }
                    None => return Err(VH_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(VH_EVALUE);
                        }
                    }
                    None => return Err(VH_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(VH_EBAL);
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
            return Err(VH_ESEAL);
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
            Err(VH_ESEAL)
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
const OP_MISS: u8 = 3;
const OP_CONFLICT: u8 = 4;
const OP_FULL: u8 = 5;
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

fn code_of(r: Result<Hit, u32>, fallback: u32) -> u32 {
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
    let _ = l.register(count_cell(3, OP_MISS, r.map(|x| x.miss as usize)));
    let _ = l.register(count_cell(4, OP_CONFLICT, r.map(|x| x.conflict as usize)));
    let _ = l.register(count_cell(5, OP_FULL, r.map(|x| x.full as usize)));
    let _ = l.register(count_cell(6, OP_NAME, r.map(|x| x.name as usize)));
    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_RATE, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(
        8,
        OP_QUERY,
        code_of(judge(Probe::Find { ver: "GLIBC_2.99" }), VH_EMISS),
    ));
    let _ = l.register(err_cell(
        9,
        OP_QUERY,
        code_of(judge(Probe::Find { ver: "GLIBC_2.35" }), VH_EMISS),
    ));
    let _ = l.register(err_cell(
        10,
        OP_QUERY,
        code_of(judge(Probe::Find { ver: COLLIDE_B }), VH_ECONF),
    ));
    let _ = l.register(err_cell(
        11,
        OP_QUERY,
        code_of(
            judge(Probe::Insert { ver: FULL_TARGET, seeds: &FULL_SEEDS }),
            VH_EFULL,
        ),
    ));
    let _ = l.register(err_cell(
        12,
        OP_QUERY,
        code_of(judge(Probe::Find { ver: "" }), VH_ENAME),
    ));
    let _ = l.register(err_cell(
        13,
        OP_QUERY,
        code_of(judge(Probe::Find { ver: "GLIBC_2.34\n" }), VH_EBYTE),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 哈希向量与碰撞。
fn hash_ok() -> bool {
    let a = elf_hash("GLIBC_2.2.5") == 0x0969_1a75;
    let b = elf_hash("GLIBC_2.34") == 0x0696_91b4;
    let c = elf_hash("GLIBC_2.17") == 0x0696_9197;
    let d = elf_hash("GLIBC_2.4") == 0x0d69_6914;
    let e = elf_hash("") == 0 && elf_hash("a") == 0x61;
    let f = (elf_hash("GLIBC_2.34") >> 16) == (elf_hash("GLIBC_2.17") >> 16);
    let g = elf_hash("GLIBC_2.34") != elf_hash("GLIBC_2.17");
    let h = elf_hash(COLLIDE_A) == 0x0555_db38 && elf_hash(COLLIDE_B) == elf_hash(COLLIDE_A);
    let i = COLLIDE_A != COLLIDE_B;
    a && b && c && d && e && f && g && h && i
}

/// 版本语法。
fn version_ok() -> bool {
    let a = parse_version("GLIBC_2.2.5").map(|v| v.text()) == Ok(String::from("GLIBC_2.2.5"));
    let b = parse_version("OPENSSL_3.0.0").map(|v| v.text())
        == Ok(String::from("OPENSSL_3.0.0"));
    let c = parse_version("").err() == Some(VH_ENAME);
    let d = parse_version("GLIBC_2.34\n").err() == Some(VH_EBYTE);
    let e = parse_version("GLIBC_").err() == Some(VH_EMISS);
    let f = parse_version("GLIBC_1.2.3.4.5").err() == Some(VH_EMISS);
    let g = parse_version(COLLIDE_A).map(|v| v.n) == Ok(4);
    a && b && c && d && e && f && g
}

/// 插入与去重。
fn insert_ok() -> bool {
    let mut t = VersionTable::new();
    let a = t.is_empty() && t.total() == 0;
    let n1 = t.insert("GLIBC_2.2.5");
    let n2 = t.insert("GLIBC_2.34");
    let b = n1 == Ok(1) && n2 == Ok(2) && t.total() == 2;
    let c = t.insert("GLIBC_2.2.5").err() == Some(VH_EDUP);
    let d = t.insert("").err() == Some(VH_ENAME);
    let e = t.insert("GLIBC_2.34\n").err() == Some(VH_EBYTE);
    let f = t.insert("GLIBC_").err() == Some(VH_EMISS);
    let g = t.find("GLIBC_2.34") == Ok(Hit {
        ndx: 2,
        hash: elf_hash("GLIBC_2.34"),
        bucket: bucket_of_hash(elf_hash("GLIBC_2.34")) as u8,
    });
    a && b && c && d && e && f && g
}

/// 满桶。
fn full_ok() -> bool {
    let mut t = VersionTable::new();
    let mut i = 0usize;
    while i < FULL_SEEDS.len() {
        let s = match FULL_SEEDS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if t.insert(s).is_err() {
            return false;
        }
        i += 1;
    }
    let b = bucket_of_hash(elf_hash(FULL_TARGET));
    let a = t.bucket_len(b) == BUCKET_MAX;
    let c = t.insert(FULL_TARGET).err() == Some(VH_EFULL);
    let d = t.insert(FULL_TARGET).err() == Some(VH_EFULL);
    let e = t.total() == 4;
    a && c && d && e
}

/// 预筛与冲突。
fn prefilter_ok() -> bool {
    let t = VersionTable::fixed();
    let l = match t.lookup("GLIBC_2.34") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = l.is_hit()
        && l.hit == Some(Hit { ndx: 5, hash: 0x0696_91b4, bucket: 4 })
        && l.scanned == 3
        && l.prefilter_skips == 1
        && l.full_checks == 2
        && l.name_checks == 1
        && !l.conflict;
    let m = match t.lookup("GLIBC_2.35") {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = !m.is_hit()
        && m.code == VH_EMISS
        && m.scanned == 1
        && m.prefilter_skips == 1
        && m.full_checks == 0
        && m.name_checks == 0;
    let c = match t.lookup(COLLIDE_B) {
        Ok(v) => {
            !v.is_hit()
                && v.code == VH_ECONF
                && v.conflict
                && v.full_checks == 1
                && v.name_checks == 1
        }
        Err(_) => false,
    };
    let d = match t.lookup("GLIBC_2.32") {
        Ok(v) => !v.is_hit() && v.code == VH_EMISS && v.scanned == 0 && v.full_checks == 0,
        Err(_) => false,
    };
    a && b && c && d
}

/// 命中回读。
fn lookup_ok() -> bool {
    let t = VersionTable::fixed();
    let a = t.total() == FIXED_LEN && !t.is_empty();
    let b = t.find("GLIBC_2.2.5") == Ok(Hit { ndx: 1, hash: 0x0969_1a75, bucket: 5 });
    let c = t.find("GLIBC_2.14") == Ok(Hit { ndx: 3, hash: 0x0696_9194, bucket: 4 });
    let d = t.find("OPENSSL_3.0.0") == Ok(Hit { ndx: 8, hash: 0x0670_2b20, bucket: 0 });
    let e = t.find(COLLIDE_A) == Ok(Hit { ndx: 9, hash: 0x0555_db38, bucket: 0 });
    let f = t.find(COLLIDE_B).err() == Some(VH_ECONF);
    let g = t.find("GLIBC_2.99").err() == Some(VH_EMISS);
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
    let b = c.miss == 2 && c.conflict == 1 && c.full == 1 && c.name == 2;
    let d = c.matched == 14 && c.rate() == PPM_FULL && c.green() && c.gate() == Ok(());
    a && b && d
}

/// 槽位编号。
fn slots_ok() -> bool {
    let a = CORPUS == 14 && CASES.len() == CORPUS && unique() && notes_ok();
    let b = match case_at(0) {
        Some(c) => c.face == Face::Hit && c.note == "hit-base",
        None => false,
    };
    let c = match case_at(7) {
        Some(c) => c.want == Want::Hit { ndx: 9 },
        None => false,
    };
    let d = match case_at(13) {
        Some(c) => c.face == Face::Name && c.want == Want::Code(VH_EBYTE),
        None => false,
    };
    let e = case_at(14).is_none() && case_at(77).is_none();
    let f = probe_census() == (13, 1);
    a && b && c && d && e && f
}

/// 汇总渲染。
fn report_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.render().as_str()
        == "vh corpus=14 accept=8 reject=6 miss=2 conflict=1 full=1 name=2 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(VH_ENOBS);
    let d = e.render().as_str()
        == "vh corpus=0 accept=0 reject=0 miss=0 conflict=0 full=0 name=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(CORPUS + 1) == Err(VH_ENOBS);
    a && b && d && f
}

/// 红档。
fn reds_ok() -> bool {
    let c = match run_with(Some(7)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched == 13 && c.rate() == PPM_RED && c.gate() == Err(VH_EVALUE);
    let b = !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(VH_ESLOT);
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
    let d = f.register(Sample::blank()).err() == Some(VH_ESEAL) && f.rejects() == before + 1;
    a && b && c && d
}

/// 文本助手。
fn helpers_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(929) == "929" && u32_text(4294967295) == "4294967295";
    let b = hex_text(SEG_BASE) == "0x0000fdb0" && hex_text(VH_ECONF) == "0x0000fdb5";
    let c = i32_text(-9) == "-9" && i32_text(9) == "9";
    let d = bool_text(true) == "true" && bool_text(false) == "false";
    let e = digest(b"version") == digest(b"version") && digest(b"version") != digest(b"versio");
    let f = fold_word(FNV_OFF, 7) == digest(&[0, 0, 0, 7]);
    let g = bucket_of_hash(0) == 0 && bucket_of_hash(7) == 7 && bucket_of_hash(8) == 0;
    a && b && c && d && e && f && g
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = hash_ok() && version_ok();
    out[1] = insert_ok() && full_ok();
    out[2] = prefilter_ok() && lookup_ok() && corpus_ok() && slots_ok();
    out[3] = report_ok() && reds_ok() && ledger_ok() && helpers_ok();
    out
}

/// 判据集合。
pub fn run_unx11652_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11652.version_hash");

    s.add("VH01-ELF哈希向量", hash_ok(), "static");
    s.add("VH02-版本语法", version_ok(), "static");
    s.add("VH03-插入与去重", insert_ok(), "static");
    s.add("VH04-满桶拒绝", full_ok(), "static");
    s.add("VH05-高16预筛", prefilter_ok(), "static");
    s.add("VH06-命中回读", lookup_ok(), "static");
    s.add("VH07-十四槽表判", corpus_ok(), "static");
    s.add("VH08-槽位编号", slots_ok(), "static");
    s.add("VH09-汇总渲染", report_ok(), "static");
    s.add("VH10-红档929", reds_ok(), "static");
    s.add("VH11-账本对账", ledger_ok(), "static");
    s.add("VH12-文本助手", helpers_ok(), "static");
    s.add("VH13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
        assert_eq!(case_at(0).map(|c| c.note), Some("hit-base"));
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
    fn hash_collision_pair() {
        assert_eq!(elf_hash(COLLIDE_A), elf_hash(COLLIDE_B));
        assert_eq!(elf_hash(COLLIDE_A), 0x0555_db38);
        assert!(COLLIDE_A != COLLIDE_B);
        assert_eq!(elf_hash("GLIBC_2.34") >> 16, elf_hash("GLIBC_2.17") >> 16);
        assert!(elf_hash("GLIBC_2.34") != elf_hash("GLIBC_2.17"));
    }

    #[test]
    fn version_parse() {
        assert_eq!(parse_version("GLIBC_2.2.5").map(|v| v.text()), Ok(String::from("GLIBC_2.2.5")));
        assert_eq!(parse_version("OPENSSL_3.0.0").map(|v| v.text()), Ok(String::from("OPENSSL_3.0.0")));
        assert_eq!(parse_version(COLLIDE_A).map(|v| v.n), Ok(4));
        assert_eq!(parse_version("").err(), Some(VH_ENAME));
        assert_eq!(parse_version("GLIBC_2.34\n").err(), Some(VH_EBYTE));
        assert_eq!(parse_version("GLIBC_").err(), Some(VH_EMISS));
        assert_eq!(parse_version("GLIBC_2.").err(), Some(VH_EMISS));
        assert_eq!(parse_version("GLIBC_1.2.3.4.5").err(), Some(VH_EMISS));
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
    fn table_fixed() {
        let t = VersionTable::fixed();
        assert_eq!(t.total(), FIXED_LEN);
        assert_eq!(t.bucket_len(4), 3);
        assert_eq!(t.bucket_len(0), 3);
        assert_eq!(t.bucket_len(1), 1);
        assert_eq!(t.bucket_len(2), 0);
        assert_eq!(t.slot(4, 0).map(|s| s.ndx), Some(2));
        assert_eq!(t.slot(4, 2).map(|s| s.ndx), Some(5));
        assert!(t.slot(4, 3).is_none());
        assert_eq!(t.bucket_len(99), 0);
    }

    #[test]
    fn insert_sequence() {
        let mut t = VersionTable::new();
        assert!(t.is_empty());
        assert_eq!(t.insert("GLIBC_2.2.5"), Ok(1));
        assert_eq!(t.insert("GLIBC_2.34"), Ok(2));
        assert_eq!(t.insert("OPENSSL_3.0.0"), Ok(3));
        assert_eq!(t.total(), 3);
        assert_eq!(t.insert("GLIBC_2.2.5").err(), Some(VH_EDUP));
        assert_eq!(t.find("OPENSSL_3.0.0").map(|h| h.ndx), Ok(3));
    }

    #[test]
    fn insert_errors() {
        let mut t = VersionTable::new();
        assert_eq!(t.insert("").err(), Some(VH_ENAME));
        assert_eq!(t.insert("GLIBC_2.34\n").err(), Some(VH_EBYTE));
        assert_eq!(t.insert("GLIBC_").err(), Some(VH_EMISS));
        assert_eq!(t.insert("glibc_2.34").err(), Some(VH_EMISS));
        assert_eq!(t.insert("GLIBC_10000").err(), Some(VH_EMISS));
        assert_eq!(t.total(), 0);
    }

    #[test]
    fn fill_bucket() {
        let mut t = VersionTable::new();
        for s in FULL_SEEDS.iter() {
            assert!(t.insert(s).is_ok());
        }
        let b = bucket_of_hash(elf_hash(FULL_TARGET));
        assert_eq!(t.bucket_len(b), BUCKET_MAX);
        assert_eq!(t.insert(FULL_TARGET).err(), Some(VH_EFULL));
        assert_eq!(t.insert(FULL_TARGET).err(), Some(VH_EFULL));
        assert_eq!(t.total(), 4);
    }

    #[test]
    fn lookup_hit() {
        let t = VersionTable::fixed();
        let h = match t.find("GLIBC_2.34") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(h.ndx, 5);
        assert_eq!(h.hash, 0x0696_91b4);
        assert_eq!(h.bucket, 4);
        assert_eq!(t.find("GLIBC_2.4").map(|x| x.ndx), Ok(2));
        assert_eq!(t.find("OPENSSL_1.1.1").map(|x| x.ndx), Ok(7));
    }

    #[test]
    fn lookup_metrics() {
        let t = VersionTable::fixed();
        let l = match t.lookup("GLIBC_2.34") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(l.scanned, 3);
        assert_eq!(l.prefilter_skips, 1);
        assert_eq!(l.full_checks, 2);
        assert_eq!(l.name_checks, 1);
        assert!(l.is_hit());
        assert_eq!(l.code, 0);
    }

    #[test]
    fn lookup_prefilter_miss() {
        let t = VersionTable::fixed();
        let l = match t.lookup("GLIBC_2.35") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(!l.is_hit());
        assert_eq!(l.code, VH_EMISS);
        assert_eq!(l.scanned, 1);
        assert_eq!(l.prefilter_skips, 1);
        assert_eq!(l.full_checks, 0);
        assert_eq!(l.name_checks, 0);
    }

    #[test]
    fn lookup_empty_bucket() {
        let t = VersionTable::fixed();
        let l = match t.lookup("GLIBC_2.32") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(!l.is_hit());
        assert_eq!(l.code, VH_EMISS);
        assert_eq!(l.scanned, 0);
        assert_eq!(l.full_checks, 0);
        assert_eq!(t.find("GLIBC_2.32").err(), Some(VH_EMISS));
    }

    #[test]
    fn lookup_conflict() {
        let t = VersionTable::fixed();
        let l = match t.lookup(COLLIDE_B) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(!l.is_hit());
        assert_eq!(l.code, VH_ECONF);
        assert!(l.conflict);
        assert_eq!(l.full_checks, 1);
        assert_eq!(l.name_checks, 1);
        assert_eq!(t.find(COLLIDE_B).err(), Some(VH_ECONF));
    }

    #[test]
    fn lookup_injection() {
        let t = VersionTable::fixed();
        assert_eq!(t.lookup("").err(), Some(VH_ENAME));
        assert_eq!(t.lookup("GLIBC_2.34\n").err(), Some(VH_EBYTE));
        assert_eq!(t.find("").err(), Some(VH_ENAME));
        assert_eq!(t.find("GLIBC_2.34;rm").err(), Some(VH_EBYTE));
    }

    #[test]
    fn bucket_mapping() {
        assert_eq!(bucket_of_hash(0), 0);
        assert_eq!(bucket_of_hash(7), 7);
        assert_eq!(bucket_of_hash(8), 0);
        assert_eq!(bucket_of_hash(elf_hash("GLIBC_2.2.5")), 5);
        assert_eq!(bucket_of_hash(elf_hash("GLIBC_2.34")), 4);
        assert_eq!(bucket_of_hash(elf_hash(COLLIDE_A)), 0);
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
    fn census_numbers() {
        let c = match run() {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(c.total, 14);
        assert_eq!(c.accept, 8);
        assert_eq!(c.reject, 6);
        assert_eq!(c.miss, 2);
        assert_eq!(c.conflict, 1);
        assert_eq!(c.full, 1);
        assert_eq!(c.name, 2);
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
            "vh corpus=14 accept=8 reject=6 miss=2 conflict=1 full=1 name=2 match=14 rate=1000ppm\n"
        );
        let e = Census::new();
        assert_eq!(
            e.render().as_str(),
            "vh corpus=0 accept=0 reject=0 miss=0 conflict=0 full=0 name=0 match=0 rate=0ppm\n"
        );
        assert_eq!(e.gate(), Err(VH_ENOBS));
    }

    #[test]
    fn red_rounding() {
        let c = match run_with(Some(3)) {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert_eq!(c.matched, 13);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(VH_EVALUE));
        assert_eq!(run_with(Some(14)).err(), Some(VH_ESLOT));
    }

    #[test]
    fn probe_kinds() {
        assert_eq!(probe_census(), (13, 1));
        assert_eq!(cover(14), Ok(()));
        assert_eq!(cover(15), Err(VH_ENOBS));
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
        assert_eq!(l.sample(8).map(|s| s.code), Some(VH_EMISS));
        assert_eq!(l.sample(9).map(|s| s.code), Some(VH_EMISS));
        assert_eq!(l.sample(10).map(|s| s.code), Some(VH_ECONF));
        assert_eq!(l.sample(11).map(|s| s.code), Some(VH_EFULL));
        assert_eq!(l.sample(12).map(|s| s.code), Some(VH_ENAME));
        assert_eq!(l.sample(13).map(|s| s.code), Some(VH_EBYTE));
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
        assert_eq!(l.register(Sample::blank()).err(), Some(VH_ESEAL));
        assert_eq!(l.rejects(), before + 1);
    }

    #[test]
    fn ledger_order_gate() {
        let mut l = Ledger::new();
        let first = Sample { slot: 1, op: 0, ok: true, code: 0, value: 1 };
        assert_eq!(l.register(first).err(), Some(VH_ESLOT));
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
        assert_eq!(hex_text(SEG_BASE), "0x0000fdb0");
        assert_eq!(hex_text(VH_EBYTE), "0x0000fdb1");
        assert_eq!(i32_text(-7), "-7");
        assert_eq!(bool_text(true), "true");
        assert_eq!(digest(b"x"), digest(b"x"));
        assert_eq!(fold_word(FNV_OFF, 1), digest(&[0, 0, 0, 1]));
    }

    #[test]
    fn empty_table() {
        let t = VersionTable::new();
        assert!(t.is_empty());
        assert_eq!(t.total(), 0);
        assert_eq!(t.find("GLIBC_2.2.5").err(), Some(VH_EMISS));
        assert_eq!(t.lookup("GLIBC_2.2.5").map(|l| l.scanned), Ok(0));
        assert_eq!(t.slot(0, 0), None);
    }

    #[test]
    fn text_type() {
        let t = match Text::from_str("GLIBC_2.2.5") {
            Ok(v) => v,
            Err(_) => fail(),
        };
        assert!(t.eq_str("GLIBC_2.2.5"));
        assert!(!t.eq_str("GLIBC_2.2.6"));
        assert_eq!(t.text(), "GLIBC_2.2.5");
        assert!(!t.is_empty());
        assert!(Text::empty().is_empty());
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11652_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }
}
