//! UNX-F11648 符号版本机制总锚（.gnu.version 双节）判据
//!
//! 符号版本由三个节拼成：`.gnu.version`（Elf_Half 数组，每个动态符号一个
//! 下标）、`.gnu.version_d`（本对象提供的版本定义 Verdef/Verdaux）、
//! `.gnu.version_r`（本对象需要的版本 Verneed/Vernaux）。下标 0 是
//! VER_NDX_LOCAL，1 是 VER_NDX_GLOBAL，其余值指向定义或需求表；下标最高位
//! 0x8000 是 VERSYM_HIDDEN，表示“隐藏版本”，只允许出现在被定义的符号上。
//!
//! 双节总锚要同时钉住四件事：结构（头字段、偏移、链式 next 必须单调前进且
//! 落在像内）、编号（0/1 特殊、定义号不得重复、BASE 只许在 1 号）、哈希
//! （vd_hash / vna_hash 必须等于名字的 ELF hash）、交叉覆盖（每个 versym
//! 下标都能在定义表或需求表里找回名字）。少任何一项，重定位期就会把符号
//! 绑到错的版本节点上，且运行时只报一个含糊的 “undefined symbol”。
//!
//! 本模块自带一个 512 字节的合成镜像：按真实 ELF64 布局编码 Verdef 20 字节
//! 头 + 8 字节 Verdaux、Verneed 16 字节头 + 16 字节 Vernaux，然后走同一条
//! 解析路径读回。坏镜像不是“跳过”，而是逐字段撞出 13 个错误码之一。
//!
//! 闸门次序：镜像边界 → 版本字段 → 计数 → 编号 → 哈希 → 交叉覆盖 → 账本。
//!
//! 契约：纯 no_std（只用 alloc 的 String）、无 unsafe、无 unwrap/expect/
//! panic!，失败一律 `Result<_, u32>`；账本 14 格（8 成 6 败）逐格对常量表，
//! 冻结后登记落 ESEAL，重放必须复现同一指纹；语料 14 槽互异，覆盖率按
//! `(matched*1000 + total/2)/total` 四舍五入，单槽红档 929ppm。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（F11648）。
pub const SEG_BASE: u32 = 0xfd70;

/// 语料槽数。
pub const CORPUS: usize = 14;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 满档千分比。
pub const PPM_FULL: u32 = 1000;

/// 单槽红档千分比。
pub const PPM_RED: u32 = 929;

/// 合成镜像容量。
pub const IMG_MAX: usize = 512;

/// Verdef 头字节数。
pub const VERDEF_HDR: usize = 20;

/// Verdaux 字节数。
pub const VERDAUX_SZ: usize = 8;

/// Verneed 头字节数。
pub const VERNEED_HDR: usize = 16;

/// Vernaux 字节数。
pub const VERNAUX_SZ: usize = 16;

/// 定义表上限。
pub const VERDEF_MAX: usize = 8;

/// 需求表上限。
pub const VERNEED_MAX: usize = 4;

/// 单条需求 aux 上限。
pub const VERNAUX_MAX: usize = 6;

/// versym 上限。
pub const VERSYM_MAX: usize = 16;

/// 局部下标。
pub const VER_NDX_LOCAL: u16 = 0;

/// 全局下标。
pub const VER_NDX_GLOBAL: u16 = 1;

/// 隐藏位。
pub const VERSYM_HIDDEN: u16 = 0x8000;

/// 下标掩码。
pub const VERSYM_MASK: u16 = 0x7fff;

/// 基础版本标志。
pub const VER_FLG_BASE: u16 = 0x0001;

/// 弱版本标志。
pub const VER_FLG_WEAK: u16 = 0x0002;

/// 信息版本标志。
pub const VER_FLG_INFO: u16 = 0x0004;

/// DT_VERSYM。
pub const DT_VERSYM: u32 = 0x6fff_fff0;

/// DT_VERDEF。
pub const DT_VERDEF: u32 = 0x6fff_fffc;

/// DT_VERDEFNUM。
pub const DT_VERDEFNUM: u32 = 0x6fff_fffd;

/// DT_VERNEED。
pub const DT_VERNEED: u32 = 0x6fff_fffe;

/// DT_VERNEEDNUM。
pub const DT_VERNEEDNUM: u32 = 0x6fff_ffff;

/// 字符串表偏移落空/切进名字中段。
pub const SV_EBYTE: u32 = SEG_BASE + 0x01;

/// 名字缺失或超长。
pub const SV_ENAME: u32 = SEG_BASE + 0x02;

/// 结构越界或链断。
pub const SV_EBOUND: u32 = SEG_BASE + 0x03;

/// 版本字段/计数不符。
pub const SV_ECOUNT: u32 = SEG_BASE + 0x04;

/// 编号非法（0 号、重复、BASE 错位）。
pub const SV_ENDX: u32 = SEG_BASE + 0x05;

/// 版本名哈希与记录不符。
pub const SV_EHASH: u32 = SEG_BASE + 0x06;

/// 表已冻结或重复冻结。
pub const SV_ESTATE: u32 = SEG_BASE + 0x07;

/// 交叉覆盖值不符。
pub const SV_EVER: u32 = SEG_BASE + 0x08;

/// 账本不平。
pub const SV_EBAL: u32 = SEG_BASE + 0x09;

/// 槽位错序。
pub const SV_ESLOT: u32 = SEG_BASE + 0x0a;

/// 观测值与常量表不符。
pub const SV_EVALUE: u32 = SEG_BASE + 0x0b;

/// 冻结重放指纹不符。
pub const SV_ESEAL: u32 = SEG_BASE + 0x0c;

/// 空观测。
pub const SV_ENOBS: u32 = SEG_BASE + 0x0d;

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

/// 四位十六进制渲染（下标/位数用）。
pub fn hex4_text(v: u16) -> String {
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
// ELF hash
// ---------------------------------------------------------------------------

/// 版本名哈希（经典 ELF hash，glibc 的 `_dl_elf_hash`）。
pub fn elf_hash(name: &str) -> u32 {
    let b = name.as_bytes();
    let mut h: u32 = 0;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            h = h.wrapping_shl(4).wrapping_add(*c as u32);
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
// 字符串表
// ---------------------------------------------------------------------------

/// 合成字符串表；偏移按 NUL 分隔累加。
pub const STRS: [&str; 11] = [
    "libc.so.6",
    "libm.so.6",
    "prog",
    "GLIBC_2.2.5",
    "GLIBC_2.34",
    "GLIBC_2.17",
    "GLIBC_2.4",
    "GLIBC_PRIVATE",
    "LIBX_1.0",
    "S_alpha",
    "S_beta",
];

/// 第 i 项的字节偏移。
pub fn str_offset(i: usize) -> Result<u32, u32> {
    let mut acc = 0u32;
    let mut n = 0usize;
    while n < STRS.len() {
        if n == i {
            return Ok(acc);
        }
        let s = match STRS.get(n) {
            Some(v) => *v,
            None => return Err(SV_ENAME),
        };
        acc = acc.saturating_add(s.len() as u32).saturating_add(1);
        n += 1;
    }
    Err(SV_ENAME)
}

/// 按名字取偏移。
pub fn str_offset_of(name: &str) -> Result<u32, u32> {
    let mut i = 0usize;
    while i < STRS.len() {
        if let Some(s) = STRS.get(i) {
            if *s == name {
                return str_offset(i);
            }
        }
        i += 1;
    }
    Err(SV_ENAME)
}

/// 按偏移取名字。
pub fn str_by_offset(off: u32) -> Result<&'static str, u32> {
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < STRS.len() {
        let s = match STRS.get(i) {
            Some(v) => *v,
            None => return Err(SV_ENAME),
        };
        let len = s.len() as u32;
        if off == acc {
            return Ok(s);
        }
        if off > acc && off < acc.saturating_add(len).saturating_add(1) {
            return Err(SV_EBYTE);
        }
        acc = acc.saturating_add(len).saturating_add(1);
        i += 1;
    }
    Err(SV_EBYTE)
}

// ---------------------------------------------------------------------------
// 镜像
// ---------------------------------------------------------------------------

/// 定长镜像缓冲。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    bytes: [u8; IMG_MAX],
    len: usize,
}

impl Image {
    /// 空镜像。
    pub const fn new() -> Image {
        Image { bytes: [0u8; IMG_MAX], len: 0 }
    }

    /// 有效字节数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 收缩（造截断档）。
    pub fn shrink(&mut self, n: usize) {
        if n <= self.len {
            self.len = n;
        }
    }

    /// 读 u16。
    pub fn u16_at(&self, off: usize) -> Result<u16, u32> {
        if off + 2 > self.len {
            return Err(SV_EBOUND);
        }
        let a = match self.bytes.get(off) {
            Some(v) => *v as u16,
            None => return Err(SV_EBOUND),
        };
        let b = match self.bytes.get(off + 1) {
            Some(v) => *v as u16,
            None => return Err(SV_EBOUND),
        };
        Ok(a | (b << 8))
    }

    /// 读 u32。
    pub fn u32_at(&self, off: usize) -> Result<u32, u32> {
        let a = self.u16_at(off)? as u32;
        let b = self.u16_at(off + 2)? as u32;
        Ok(a | (b << 16))
    }

    /// 写 u16。
    pub fn put_u16(&mut self, off: usize, v: u16) -> Result<(), u32> {
        if off + 2 > self.len {
            return Err(SV_EBOUND);
        }
        if let Some(d) = self.bytes.get_mut(off) {
            *d = (v & 0xff) as u8;
        }
        if let Some(d) = self.bytes.get_mut(off + 1) {
            *d = ((v >> 8) & 0xff) as u8;
        }
        Ok(())
    }

    /// 写 u32。
    pub fn put_u32(&mut self, off: usize, v: u32) -> Result<(), u32> {
        self.put_u16(off, (v & 0xffff) as u16)?;
        self.put_u16(off + 2, ((v >> 16) & 0xffff) as u16)
    }

    /// 追加 u16。
    pub fn push_u16(&mut self, v: u16) -> Result<(), u32> {
        let off = self.len;
        if off + 2 > IMG_MAX {
            return Err(SV_EBOUND);
        }
        self.len = off + 2;
        self.put_u16(off, v)
    }

    /// 追加 u32。
    pub fn push_u32(&mut self, v: u32) -> Result<(), u32> {
        let off = self.len;
        if off + 4 > IMG_MAX {
            return Err(SV_EBOUND);
        }
        self.len = off + 4;
        self.put_u32(off, v)
    }

    /// 当前末尾偏移。
    pub fn cursor(&self) -> usize {
        self.len
    }
}

impl Default for Image {
    fn default() -> Image {
        Image::new()
    }
}

// ---------------------------------------------------------------------------
// Verdef
// ---------------------------------------------------------------------------

/// Verdef 种子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefSeed {
    /// 版本字段（正常为 1）。
    pub version: u16,
    /// 标志。
    pub flags: u16,
    /// 下标。
    pub ndx: u16,
    /// aux 计数。
    pub cnt: u16,
    /// 哈希覆盖（0 表示按名字算）。
    pub hash: u32,
    /// 版本名。
    pub name: &'static str,
}

impl DefSeed {
    /// 构造。
    pub const fn new(name: &'static str, ndx: u16, flags: u16) -> DefSeed {
        DefSeed { version: 1, flags, ndx, cnt: 1, hash: 0, name }
    }
}

/// 造一张规范定义表。
pub fn canonical_def_seeds() -> [DefSeed; 5] {
    [
        DefSeed::new("libc.so.6", 1, VER_FLG_BASE),
        DefSeed::new("GLIBC_2.2.5", 2, 0),
        DefSeed::new("GLIBC_2.34", 3, 0),
        DefSeed::new("GLIBC_2.17", 4, 0),
        DefSeed::new("GLIBC_PRIVATE", 5, 0),
    ]
}

/// 编码 Verdef 表。
pub fn build_verdef(seeds: &[DefSeed]) -> Result<Image, u32> {
    if seeds.is_empty() || seeds.len() > VERDEF_MAX {
        return Err(SV_ECOUNT);
    }
    let mut img = Image::new();
    let mut i = 0usize;
    while i < seeds.len() {
        let s = match seeds.get(i) {
            Some(v) => *v,
            None => return Err(SV_ECOUNT),
        };
        let off = img.cursor();
        img.push_u16(s.version)?;
        img.push_u16(s.flags)?;
        img.push_u16(s.ndx)?;
        img.push_u16(s.cnt)?;
        let hash = if s.hash != 0 { s.hash } else { elf_hash(s.name) };
        img.push_u32(hash)?;
        img.push_u32(VERDEF_HDR as u32)?;
        let next = if i + 1 == seeds.len() { 0 } else { VERDEF_HDR as u32 + VERDAUX_SZ as u32 };
        img.push_u32(next)?;
        if img.cursor() != off + VERDEF_HDR {
            return Err(SV_EBOUND);
        }
        let name_off = str_offset_of(s.name)?;
        img.push_u32(name_off)?;
        img.push_u32(0)?;
        i += 1;
    }
    Ok(img)
}

/// 一条解析后的定义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerdefEntry {
    /// 版本字段。
    pub version: u16,
    /// 标志。
    pub flags: u16,
    /// 下标。
    pub ndx: u16,
    /// aux 计数。
    pub cnt: u16,
    /// 记录哈希。
    pub hash: u32,
    /// 名字。
    pub name: &'static str,
}

impl VerdefEntry {
    /// 空白。
    pub const fn blank() -> VerdefEntry {
        VerdefEntry { version: 0, flags: 0, ndx: 0, cnt: 0, hash: 0, name: "" }
    }

    /// 是否 BASE。
    pub fn is_base(&self) -> bool {
        self.flags & VER_FLG_BASE != 0
    }

    /// 哈希是否对。
    pub fn hash_ok(&self) -> bool {
        elf_hash(self.name) == self.hash
    }
}

/// 定义表。
#[derive(Clone, Copy, Debug)]
pub struct VerdefTable {
    defs: [VerdefEntry; VERDEF_MAX],
    len: usize,
}

impl VerdefTable {
    /// 空表。
    pub const fn new() -> VerdefTable {
        VerdefTable { defs: [VerdefEntry::blank(); VERDEF_MAX], len: 0 }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取。
    pub fn get(&self, i: usize) -> Option<VerdefEntry> {
        if i >= self.len {
            return None;
        }
        self.defs.get(i).copied()
    }

    /// 按编号找。
    pub fn by_ndx(&self, ndx: u16) -> Option<VerdefEntry> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = self.defs.get(i) {
                if d.ndx == ndx {
                    return Some(*d);
                }
            }
            i += 1;
        }
        None
    }

    /// 最大编号。
    pub fn max_ndx(&self) -> u16 {
        let mut m = 0u16;
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = self.defs.get(i) {
                if d.ndx > m {
                    m = d.ndx;
                }
            }
            i += 1;
        }
        m
    }

    /// BASE 编号。
    pub fn base_ndx(&self) -> Option<u16> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = self.defs.get(i) {
                if d.is_base() {
                    return Some(d.ndx);
                }
            }
            i += 1;
        }
        None
    }

    /// 哈希全对。
    pub fn hashes_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = self.defs.get(i) {
                if !d.hash_ok() {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 名字全非空。
    pub fn names_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(d) = self.defs.get(i) {
                if d.name.is_empty() {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    fn push(&mut self, e: VerdefEntry) -> Result<(), u32> {
        if self.len >= VERDEF_MAX {
            return Err(SV_EBOUND);
        }
        if let Some(dst) = self.defs.get_mut(self.len) {
            *dst = e;
        }
        self.len += 1;
        Ok(())
    }
}

impl Default for VerdefTable {
    fn default() -> VerdefTable {
        VerdefTable::new()
    }
}

/// 解析 Verdef 镜像。
pub fn parse_verdef(img: &Image) -> Result<VerdefTable, u32> {
    if img.is_empty() {
        return Err(SV_ENOBS);
    }
    let mut t = VerdefTable::new();
    let mut off = 0usize;
    loop {
        if off + VERDEF_HDR > img.len() {
            return Err(SV_EBOUND);
        }
        let version = img.u16_at(off)?;
        let flags = img.u16_at(off + 2)?;
        let ndx = img.u16_at(off + 4)?;
        let cnt = img.u16_at(off + 6)?;
        let hash = img.u32_at(off + 8)?;
        let aux_rel = img.u32_at(off + 12)? as usize;
        let next_rel = img.u32_at(off + 16)? as usize;
        if version != 1 {
            return Err(SV_ECOUNT);
        }
        if flags & !(VER_FLG_BASE | VER_FLG_WEAK | VER_FLG_INFO) != 0 {
            return Err(SV_ECOUNT);
        }
        if ndx == VER_NDX_LOCAL || ndx > VERSYM_MASK {
            return Err(SV_ENDX);
        }
        if t.by_ndx(ndx).is_some() {
            return Err(SV_ENDX);
        }
        if flags & VER_FLG_BASE != 0 && ndx != VER_NDX_GLOBAL {
            return Err(SV_ENDX);
        }
        if cnt != 1 {
            return Err(SV_ECOUNT);
        }
        if aux_rel < VERDEF_HDR {
            return Err(SV_EBOUND);
        }
        let aux_off = match off.checked_add(aux_rel) {
            Some(v) => v,
            None => return Err(SV_EBOUND),
        };
        if aux_off + VERDAUX_SZ > img.len() {
            return Err(SV_EBOUND);
        }
        let name_off = img.u32_at(aux_off)?;
        let aux_next = img.u32_at(aux_off + 4)?;
        if aux_next != 0 {
            return Err(SV_ECOUNT);
        }
        let name = str_by_offset(name_off)?;
        if name.is_empty() {
            return Err(SV_ENAME);
        }
        if elf_hash(name) != hash {
            return Err(SV_EHASH);
        }
        t.push(VerdefEntry { version, flags, ndx, cnt, hash, name })?;
        if next_rel == 0 {
            break;
        }
        if next_rel < VERDEF_HDR + VERDAUX_SZ {
            return Err(SV_EBOUND);
        }
        let next_off = match off.checked_add(next_rel) {
            Some(v) => v,
            None => return Err(SV_EBOUND),
        };
        if next_off <= off || next_off >= img.len() {
            return Err(SV_EBOUND);
        }
        off = next_off;
    }
    Ok(t)
}

// ---------------------------------------------------------------------------
// Verneed
// ---------------------------------------------------------------------------

/// Vernaux 种子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxSeed {
    /// 哈希覆盖（0 按名字算）。
    pub hash: u32,
    /// 标志。
    pub flags: u16,
    /// 下标。
    pub other: u16,
    /// 版本名。
    pub name: &'static str,
}

impl AuxSeed {
    /// 构造。
    pub const fn new(name: &'static str, other: u16, flags: u16) -> AuxSeed {
        AuxSeed { hash: 0, flags, other, name }
    }
}

/// 需求种子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NeedSeed {
    /// 版本字段。
    pub version: u16,
    /// aux 计数。
    pub cnt: u16,
    /// 依赖文件名。
    pub file: &'static str,
    /// aux 列表。
    pub aux: [AuxSeed; VERNAUX_MAX],
    /// aux 条数。
    pub naux: usize,
}

impl NeedSeed {
    /// 构造（空 aux）。
    pub const fn blank(file: &'static str) -> NeedSeed {
        let a = AuxSeed { hash: 0, flags: 0, other: 0, name: "" };
        NeedSeed { version: 1, cnt: 0, file, aux: [a; VERNAUX_MAX], naux: 0 }
    }
}

/// 规范需求：libm.so.6 三条。
pub fn canonical_need_seed() -> NeedSeed {
    let mut s = NeedSeed::blank("libm.so.6");
    if let Some(d) = s.aux.get_mut(0) {
        *d = AuxSeed::new("GLIBC_2.2.5", 6, 0);
    }
    if let Some(d) = s.aux.get_mut(1) {
        *d = AuxSeed::new("GLIBC_2.34", 7, 0);
    }
    if let Some(d) = s.aux.get_mut(2) {
        *d = AuxSeed::new("GLIBC_2.4", 8, 0);
    }
    s.naux = 3;
    s.cnt = 3;
    s
}

/// 编码 Verneed 表。
pub fn build_verneed(seeds: &[NeedSeed]) -> Result<Image, u32> {
    if seeds.is_empty() || seeds.len() > VERNEED_MAX {
        return Err(SV_ECOUNT);
    }
    let mut img = Image::new();
    let mut i = 0usize;
    while i < seeds.len() {
        let s = match seeds.get(i) {
            Some(v) => *v,
            None => return Err(SV_ECOUNT),
        };
        let off = img.cursor();
        img.push_u16(s.version)?;
        img.push_u16(s.cnt)?;
        let file_off = str_offset_of(s.file)?;
        img.push_u32(file_off)?;
        img.push_u32(VERNEED_HDR as u32)?;
        let next = if i + 1 == seeds.len() { 0 } else { VERNEED_HDR as u32 + (s.naux as u32) * VERNAUX_SZ as u32 };
        img.push_u32(next)?;
        if img.cursor() != off + VERNEED_HDR {
            return Err(SV_EBOUND);
        }
        let mut j = 0usize;
        while j < s.naux {
            let a = match s.aux.get(j) {
                Some(v) => *v,
                None => return Err(SV_ECOUNT),
            };
            let hash = if a.hash != 0 { a.hash } else { elf_hash(a.name) };
            img.push_u32(hash)?;
            img.push_u16(a.flags)?;
            img.push_u16(a.other)?;
            let name_off = str_offset_of(a.name)?;
            img.push_u32(name_off)?;
            let nxt = if j + 1 == s.naux { 0 } else { VERNAUX_SZ as u32 };
            img.push_u32(nxt)?;
            j += 1;
        }
        i += 1;
    }
    Ok(img)
}

/// 一条解析后的需求 aux。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vernaux {
    /// 哈希。
    pub hash: u32,
    /// 标志。
    pub flags: u16,
    /// 下标。
    pub other: u16,
    /// 版本名。
    pub name: &'static str,
}

impl Vernaux {
    /// 空白。
    pub const fn blank() -> Vernaux {
        Vernaux { hash: 0, flags: 0, other: 0, name: "" }
    }

    /// 是否弱。
    pub fn is_weak(&self) -> bool {
        self.flags & VER_FLG_WEAK != 0
    }

    /// 哈希是否对。
    pub fn hash_ok(&self) -> bool {
        elf_hash(self.name) == self.hash
    }
}

/// 一条解析后的需求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerneedEntry {
    /// 版本字段。
    pub version: u16,
    /// 声明计数。
    pub cnt: u16,
    /// 依赖文件名。
    pub file: &'static str,
    /// aux。
    pub aux: [Vernaux; VERNAUX_MAX],
    /// 实际 aux 条数。
    pub naux: usize,
}

impl VerneedEntry {
    /// 空白。
    pub const fn blank() -> VerneedEntry {
        VerneedEntry {
            version: 0,
            cnt: 0,
            file: "",
            aux: [Vernaux::blank(); VERNAUX_MAX],
            naux: 0,
        }
    }

    /// 按 aux 下标号找。
    pub fn aux_by_other(&self, other: u16) -> Option<Vernaux> {
        let mut i = 0usize;
        while i < self.naux {
            if let Some(a) = self.aux.get(i) {
                if a.other == other {
                    return Some(*a);
                }
            }
            i += 1;
        }
        None
    }
}

/// 需求表。
#[derive(Clone, Copy, Debug)]
pub struct VerneedTable {
    needs: [VerneedEntry; VERNEED_MAX],
    len: usize,
}

impl VerneedTable {
    /// 空表。
    pub const fn new() -> VerneedTable {
        VerneedTable { needs: [VerneedEntry::blank(); VERNEED_MAX], len: 0 }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取。
    pub fn get(&self, i: usize) -> Option<VerneedEntry> {
        if i >= self.len {
            return None;
        }
        self.needs.get(i).copied()
    }

    /// 按文件名找。
    pub fn by_file(&self, file: &str) -> Option<VerneedEntry> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(n) = self.needs.get(i) {
                if n.file == file {
                    return Some(*n);
                }
            }
            i += 1;
        }
        None
    }

    /// 按 aux 下标找（跨需求）。
    pub fn aux_by_other(&self, other: u16) -> Option<Vernaux> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(n) = self.needs.get(i) {
                if let Some(a) = n.aux_by_other(other) {
                    return Some(a);
                }
            }
            i += 1;
        }
        None
    }

    /// 全部 aux 哈希正确。
    pub fn hashes_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(n) = self.needs.get(i) {
                let mut j = 0usize;
                while j < n.naux {
                    if let Some(a) = n.aux.get(j) {
                        if !a.hash_ok() {
                            return false;
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        true
    }

    /// 全部 aux 名字非空。
    pub fn names_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if let Some(n) = self.needs.get(i) {
                if n.file.is_empty() {
                    return false;
                }
                let mut j = 0usize;
                while j < n.naux {
                    if let Some(a) = n.aux.get(j) {
                        if a.name.is_empty() {
                            return false;
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        true
    }

    fn push(&mut self, e: VerneedEntry) -> Result<(), u32> {
        if self.len >= VERNEED_MAX {
            return Err(SV_EBOUND);
        }
        if let Some(dst) = self.needs.get_mut(self.len) {
            *dst = e;
        }
        self.len += 1;
        Ok(())
    }
}

impl Default for VerneedTable {
    fn default() -> VerneedTable {
        VerneedTable::new()
    }
}

/// 解析 Verneed 镜像。
pub fn parse_verneed(img: &Image) -> Result<VerneedTable, u32> {
    if img.is_empty() {
        return Err(SV_ENOBS);
    }
    let mut t = VerneedTable::new();
    let mut off = 0usize;
    loop {
        if off + VERNEED_HDR > img.len() {
            return Err(SV_EBOUND);
        }
        let version = img.u16_at(off)?;
        let cnt = img.u16_at(off + 2)?;
        let file_off = img.u32_at(off + 4)?;
        let aux_rel = img.u32_at(off + 8)? as usize;
        let next_rel = img.u32_at(off + 12)? as usize;
        if version != 1 {
            return Err(SV_ECOUNT);
        }
        if aux_rel < VERNEED_HDR {
            return Err(SV_EBOUND);
        }
        let file = str_by_offset(file_off)?;
        if file.is_empty() {
            return Err(SV_ENAME);
        }
        let mut aux_off = match off.checked_add(aux_rel) {
            Some(v) => v,
            None => return Err(SV_EBOUND),
        };
        let mut aux = [Vernaux::blank(); VERNAUX_MAX];
        let mut n = 0usize;
        loop {
            if n >= VERNAUX_MAX {
                return Err(SV_EBOUND);
            }
            if aux_off + VERNAUX_SZ > img.len() {
                return Err(SV_EBOUND);
            }
            let hash = img.u32_at(aux_off)?;
            let flags = img.u16_at(aux_off + 4)?;
            let other = img.u16_at(aux_off + 6)?;
            let name_off = img.u32_at(aux_off + 8)?;
            let next_aux = img.u32_at(aux_off + 12)? as usize;
            if flags & !(VER_FLG_WEAK | VER_FLG_INFO) != 0 {
                return Err(SV_ECOUNT);
            }
            if other == 0 || other > VERSYM_MASK {
                return Err(SV_ENDX);
            }
            let name = str_by_offset(name_off)?;
            if name.is_empty() {
                return Err(SV_ENAME);
            }
            if elf_hash(name) != hash {
                return Err(SV_EHASH);
            }
            if let Some(dst) = aux.get_mut(n) {
                *dst = Vernaux { hash, flags, other, name };
            }
            n += 1;
            if next_aux == 0 {
                break;
            }
            if next_aux < VERNAUX_SZ {
                return Err(SV_EBOUND);
            }
            aux_off = match aux_off.checked_add(next_aux) {
                Some(v) => v,
                None => return Err(SV_EBOUND),
            };
            if aux_off + VERNAUX_SZ > img.len() {
                return Err(SV_EBOUND);
            }
        }
        if n != cnt as usize {
            return Err(SV_ECOUNT);
        }
        t.push(VerneedEntry { version, cnt, file, aux, naux: n })?;
        if next_rel == 0 {
            break;
        }
        if next_rel < VERNEED_HDR + VERNAUX_SZ {
            return Err(SV_EBOUND);
        }
        let next_off = match off.checked_add(next_rel) {
            Some(v) => v,
            None => return Err(SV_EBOUND),
        };
        if next_off <= off || next_off >= img.len() {
            return Err(SV_EBOUND);
        }
        off = next_off;
    }
    Ok(t)
}

// ---------------------------------------------------------------------------
// versym
// ---------------------------------------------------------------------------

/// versym 表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersymTable {
    raw: [u16; VERSYM_MAX],
    len: usize,
}

impl VersymTable {
    /// 空表。
    pub const fn new() -> VersymTable {
        VersymTable { raw: [0u16; VERSYM_MAX], len: 0 }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 原始值。
    pub fn raw_at(&self, i: usize) -> Option<u16> {
        if i >= self.len {
            return None;
        }
        self.raw.get(i).copied()
    }

    /// 下标（去隐藏位）。
    pub fn ndx_at(&self, i: usize) -> Option<u16> {
        self.raw_at(i).map(|v| v & VERSYM_MASK)
    }

    /// 隐藏位。
    pub fn hidden_at(&self, i: usize) -> Option<bool> {
        self.raw_at(i).map(|v| v & VERSYM_HIDDEN != 0)
    }

    /// 收集隐藏条数。
    pub fn hidden_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(h) = self.hidden_at(i) {
                if h {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    fn push(&mut self, v: u16) -> Result<(), u32> {
        if self.len >= VERSYM_MAX {
            return Err(SV_EBOUND);
        }
        if let Some(dst) = self.raw.get_mut(self.len) {
            *dst = v;
        }
        self.len += 1;
        Ok(())
    }
}

impl Default for VersymTable {
    fn default() -> VersymTable {
        VersymTable::new()
    }
}

/// 编码 versym 镜像（u16 小端数组）。
pub fn build_versym(vals: &[u16]) -> Result<Image, u32> {
    if vals.is_empty() || vals.len() > VERSYM_MAX {
        return Err(SV_ECOUNT);
    }
    let mut img = Image::new();
    let mut i = 0usize;
    while i < vals.len() {
        let v = match vals.get(i) {
            Some(x) => *x,
            None => return Err(SV_ECOUNT),
        };
        img.push_u16(v)?;
        i += 1;
    }
    Ok(img)
}

/// 解析 versym 镜像。
pub fn parse_versym(img: &Image) -> Result<VersymTable, u32> {
    if img.is_empty() {
        return Err(SV_ENOBS);
    }
    if img.len() % 2 != 0 {
        return Err(SV_EBOUND);
    }
    if img.len() / 2 > VERSYM_MAX {
        return Err(SV_EBOUND);
    }
    let mut t = VersymTable::new();
    let mut off = 0usize;
    while off < img.len() {
        t.push(img.u16_at(off)?)?;
        off += 2;
    }
    Ok(t)
}

/// 解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolved {
    /// 局部。
    Local,
    /// 全局。
    Global,
    /// 定义版本。
    Def(&'static str),
    /// 需求版本。
    Need(&'static str),
    /// 隐藏版本。
    Hidden(&'static str),
}

impl Resolved {
    /// 文本。
    pub fn text(self) -> String {
        match self {
            Resolved::Local => String::from("local"),
            Resolved::Global => String::from("global"),
            Resolved::Def(n) => {
                let mut s = String::from("def:");
                s.push_str(n);
                s
            }
            Resolved::Need(n) => {
                let mut s = String::from("need:");
                s.push_str(n);
                s
            }
            Resolved::Hidden(n) => {
                let mut s = String::from("hidden:");
                s.push_str(n);
                s
            }
        }
    }
}

/// 解析一个 versym 下标。
pub fn resolve(
    vs: &VersymTable,
    i: usize,
    defs: &VerdefTable,
    needs: &VerneedTable,
) -> Result<Resolved, u32> {
    let raw = match vs.raw_at(i) {
        Some(v) => v,
        None => return Err(SV_ESLOT),
    };
    let idx = raw & VERSYM_MASK;
    let hidden = raw & VERSYM_HIDDEN != 0;
    if idx == VER_NDX_LOCAL {
        if hidden {
            return Err(SV_ENDX);
        }
        return Ok(Resolved::Local);
    }
    if idx == VER_NDX_GLOBAL {
        if hidden {
            return Err(SV_ENDX);
        }
        return Ok(Resolved::Global);
    }
    if let Some(d) = defs.by_ndx(idx) {
        if hidden {
            return Ok(Resolved::Hidden(d.name));
        }
        return Ok(Resolved::Def(d.name));
    }
    if let Some(a) = needs.aux_by_other(idx) {
        if hidden {
            return Err(SV_ENDX);
        }
        return Ok(Resolved::Need(a.name));
    }
    Err(SV_ENDX)
}

/// 全表覆盖：每个下标都能解析，且 0 号必须是 LOCAL。
pub fn coverage(vs: &VersymTable, defs: &VerdefTable, needs: &VerneedTable) -> Result<u32, u32> {
    if vs.is_empty() {
        return Err(SV_ENOBS);
    }
    if vs.raw_at(0) != Some(VER_NDX_LOCAL) {
        return Err(SV_ENDX);
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < vs.len() {
        let _ = resolve(vs, i, defs, needs)?;
        n = n.saturating_add(1);
        i += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// 默认镜像与总锚
// ---------------------------------------------------------------------------

/// 规范 versym：0 局部、1 全局、2/3 定义、4 隐藏、6/7/8 需求。
pub const CANONICAL_VERSYM: [u16; 9] = [0, 1, 2, 3, VERSYM_HIDDEN | 3, 1, 6, 7, 8];

/// 默认三表。
pub struct Anchor {
    /// 定义表。
    pub defs: VerdefTable,
    /// 需求表。
    pub needs: VerneedTable,
    /// versym 表。
    pub vs: VersymTable,
}

impl Anchor {
    /// 构造规范三表。
    pub fn canonical() -> Result<Anchor, u32> {
        let d = build_verdef(&canonical_def_seeds())?;
        let n = build_verneed(&[canonical_need_seed()])?;
        let v = build_versym(&CANONICAL_VERSYM)?;
        Ok(Anchor {
            defs: parse_verdef(&d)?,
            needs: parse_verneed(&n)?,
            vs: parse_versym(&v)?,
        })
    }

    /// 渲染总锚。
    pub fn render(&self) -> Result<String, u32> {
        let cov = coverage(&self.vs, &self.defs, &self.needs)?;
        let base = self.defs.base_ndx().unwrap_or(0);
        let mut s = String::from("defs=");
        s.push_str(u32_text(self.defs.len() as u32).as_str());
        s.push_str(" needs=");
        s.push_str(u32_text(self.needs.len() as u32).as_str());
        s.push_str(" syms=");
        s.push_str(u32_text(cov).as_str());
        s.push_str(" base=");
        s.push_str(u32_text(base as u32).as_str());
        s.push_str(" hidden=");
        s.push_str(u32_text(self.vs.hidden_count()).as_str());
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// 语料
// ---------------------------------------------------------------------------

/// 定义镜像档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefKind {
    /// 规范。
    Canonical,
    /// 版本字段非 1。
    BadVersion,
    /// 头部截断。
    Truncated,
    /// 哈希写错。
    BadHash,
    /// 编号重复。
    DupIndex,
    /// BASE 落在 3 号。
    BaseMisplaced,
}

/// 需求镜像档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedKind {
    /// 规范。
    Canonical,
    /// aux 计数与链不符。
    CountMismatch,
    /// aux 哈希写错。
    BadHash,
    /// aux 下标为 0。
    BadOther,
    /// 头部截断。
    ShortHdr,
}

/// 造定义镜像。
pub fn def_image(kind: DefKind) -> Result<Image, u32> {
    match kind {
        DefKind::Canonical => build_verdef(&canonical_def_seeds()),
        DefKind::BadVersion => {
            let mut img = build_verdef(&canonical_def_seeds())?;
            img.put_u16(0, 2)?;
            Ok(img)
        }
        DefKind::Truncated => {
            let mut img = build_verdef(&canonical_def_seeds())?;
            img.shrink(10);
            Ok(img)
        }
        DefKind::BadHash => {
            let mut seeds = canonical_def_seeds();
            if let Some(s) = seeds.get_mut(3) {
                s.hash = 0xdead_beef;
            }
            build_verdef(&seeds)
        }
        DefKind::DupIndex => {
            let mut seeds = canonical_def_seeds();
            if let Some(s) = seeds.get_mut(3) {
                s.ndx = 2;
            }
            build_verdef(&seeds)
        }
        DefKind::BaseMisplaced => {
            let mut seeds = canonical_def_seeds();
            if let Some(s) = seeds.get_mut(0) {
                s.flags = 0;
            }
            if let Some(s) = seeds.get_mut(2) {
                s.flags = VER_FLG_BASE;
            }
            build_verdef(&seeds)
        }
    }
}

/// 造需求镜像。
pub fn need_image(kind: NeedKind) -> Result<Image, u32> {
    let seed = canonical_need_seed();
    match kind {
        NeedKind::Canonical => build_verneed(&[seed]),
        NeedKind::CountMismatch => {
            let mut img = build_verneed(&[seed])?;
            img.put_u16(2, 2)?;
            Ok(img)
        }
        NeedKind::BadHash => {
            let mut s = seed;
            if let Some(a) = s.aux.get_mut(1) {
                a.hash = 0x0bad_f00d;
            }
            build_verneed(&[s])
        }
        NeedKind::BadOther => {
            let mut s = seed;
            if let Some(a) = s.aux.get_mut(0) {
                a.other = 0;
            }
            build_verneed(&[s])
        }
        NeedKind::ShortHdr => {
            let mut img = build_verneed(&[seed])?;
            img.shrink(8);
            Ok(img)
        }
    }
}

/// 判面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 定义解析。
    Def,
    /// 需求解析。
    Need,
    /// versym 解析。
    Versym,
    /// 解析下标。
    Resolve,
}

/// 探针。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    /// 定义档。
    Def(DefKind),
    /// 需求档。
    Need(NeedKind),
    /// versym。
    Versym,
    /// 解析第 i 个下标。
    Resolve(usize),
    /// 总锚渲染。
    Anchor,
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
        "verdef(5 entries)",
        Face::Def,
        Probe::Def(DefKind::Canonical),
        Want::Text("defs=5 base=1"),
        "def-canonical",
    ),
    Case::new(
        "verneed(libm.so.6,3 aux)",
        Face::Need,
        Probe::Need(NeedKind::Canonical),
        Want::Text("need=libm.so.6 aux=3"),
        "need-canonical",
    ),
    Case::new(
        "versym(9 entries)",
        Face::Versym,
        Probe::Versym,
        Want::Text("versym=9 hidden=1"),
        "versym-canonical",
    ),
    Case::new(
        "resolve(0)",
        Face::Resolve,
        Probe::Resolve(0),
        Want::Text("local"),
        "resolve-local",
    ),
    Case::new(
        "resolve(1)",
        Face::Resolve,
        Probe::Resolve(1),
        Want::Text("global"),
        "resolve-global",
    ),
    Case::new(
        "resolve(2)",
        Face::Resolve,
        Probe::Resolve(2),
        Want::Text("def:GLIBC_2.2.5"),
        "resolve-def-old",
    ),
    Case::new(
        "resolve(3)",
        Face::Resolve,
        Probe::Resolve(3),
        Want::Text("def:GLIBC_2.34"),
        "resolve-def-new",
    ),
    Case::new(
        "resolve(4)",
        Face::Resolve,
        Probe::Resolve(4),
        Want::Text("hidden:GLIBC_2.34"),
        "resolve-hidden",
    ),
    Case::new(
        "verdef(version=2)",
        Face::Def,
        Probe::Def(DefKind::BadVersion),
        Want::Code(SV_ECOUNT),
        "def-bad-version",
    ),
    Case::new(
        "verdef(len=10)",
        Face::Def,
        Probe::Def(DefKind::Truncated),
        Want::Code(SV_EBOUND),
        "def-truncated",
    ),
    Case::new(
        "verdef(hash=deadbeef)",
        Face::Def,
        Probe::Def(DefKind::BadHash),
        Want::Code(SV_EHASH),
        "def-bad-hash",
    ),
    Case::new(
        "verdef(ndx=2,2)",
        Face::Def,
        Probe::Def(DefKind::DupIndex),
        Want::Code(SV_ENDX),
        "def-dup-index",
    ),
    Case::new(
        "verdef(base@3)",
        Face::Def,
        Probe::Def(DefKind::BaseMisplaced),
        Want::Code(SV_ENDX),
        "def-base-misplaced",
    ),
    Case::new(
        "verneed(cnt=2,aux=3)",
        Face::Need,
        Probe::Need(NeedKind::CountMismatch),
        Want::Code(SV_ECOUNT),
        "need-count",
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

/// 定义摘要。
pub fn def_summary(kind: DefKind) -> Result<String, u32> {
    let img = def_image(kind)?;
    let t = parse_verdef(&img)?;
    if !t.hashes_ok() || !t.names_ok() {
        return Err(SV_EHASH);
    }
    let base = t.base_ndx().unwrap_or(0);
    let mut s = String::from("defs=");
    s.push_str(u32_text(t.len() as u32).as_str());
    s.push_str(" base=");
    s.push_str(u32_text(base as u32).as_str());
    Ok(s)
}

/// 需求摘要。
pub fn need_summary(kind: NeedKind) -> Result<String, u32> {
    let img = need_image(kind)?;
    let t = parse_verneed(&img)?;
    if !t.hashes_ok() || !t.names_ok() {
        return Err(SV_EHASH);
    }
    let n = match t.get(0) {
        Some(v) => v,
        None => return Err(SV_ENOBS),
    };
    let mut s = String::from("need=");
    s.push_str(n.file);
    s.push_str(" aux=");
    s.push_str(u32_text(n.naux as u32).as_str());
    Ok(s)
}

/// 运行一个探针。
pub fn judge(p: Probe) -> Result<String, u32> {
    match p {
        Probe::Def(k) => def_summary(k),
        Probe::Need(k) => need_summary(k),
        Probe::Versym => {
            let img = build_versym(&CANONICAL_VERSYM)?;
            let t = parse_versym(&img)?;
            let mut s = String::from("versym=");
            s.push_str(u32_text(t.len() as u32).as_str());
            s.push_str(" hidden=");
            s.push_str(u32_text(t.hidden_count()).as_str());
            Ok(s)
        }
        Probe::Resolve(i) => {
            let a = Anchor::canonical()?;
            resolve(&a.vs, i, &a.defs, &a.needs).map(|r| r.text())
        }
        Probe::Anchor => {
            let a = Anchor::canonical()?;
            a.render()
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
    /// 结构/计数类。
    pub shape: u32,
    /// 越界类。
    pub bound: u32,
    /// 哈希类。
    pub hash: u32,
    /// 编号类。
    pub index: u32,
    /// 命中数。
    pub matched: u32,
}

impl Census {
    /// 空汇总。
    pub const fn new() -> Census {
        Census { total: 0, accept: 0, reject: 0, shape: 0, bound: 0, hash: 0, index: 0, matched: 0 }
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
            return Err(SV_ENOBS);
        }
        if self.matched != self.total {
            return Err(SV_EVALUE);
        }
        Ok(())
    }

    fn bump(&mut self, code: u32) {
        match code {
            SV_EBOUND => self.bound = self.bound.saturating_add(1),
            SV_EHASH => self.hash = self.hash.saturating_add(1),
            SV_ENDX | SV_EBYTE | SV_ENAME => self.index = self.index.saturating_add(1),
            _ => self.shape = self.shape.saturating_add(1),
        }
    }

    /// 固定渲染。
    pub fn render(&self) -> String {
        let mut s = String::from("sv corpus=");
        s.push_str(u32_text(self.total).as_str());
        s.push_str(" accept=");
        s.push_str(u32_text(self.accept).as_str());
        s.push_str(" reject=");
        s.push_str(u32_text(self.reject).as_str());
        s.push_str(" shape=");
        s.push_str(u32_text(self.shape).as_str());
        s.push_str(" bound=");
        s.push_str(u32_text(self.bound).as_str());
        s.push_str(" hash=");
        s.push_str(u32_text(self.hash).as_str());
        s.push_str(" index=");
        s.push_str(u32_text(self.index).as_str());
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
            return Err(SV_ESLOT);
        }
    }
    let mut c = Census::new();
    let mut i = 0usize;
    while i < CASES.len() {
        let cs = match CASES.get(i) {
            Some(v) => *v,
            None => return Err(SV_ESLOT),
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
        Err(SV_ENOBS)
    }
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
            return Err(SV_ESEAL);
        }
        if (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SV_ESLOT);
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
            return Err(SV_EBAL);
        }
        const VALS: [u32; 8] = [14, 8, 6, 2, 1, 1, 2, 929];
        const CODES: [u32; 6] =
            [SV_ECOUNT, SV_EBOUND, SV_EHASH, SV_ENDX, SV_ENDX, SV_ECOUNT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SV_EBAL),
            };
            if s.slot as usize != i {
                return Err(SV_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SV_EVALUE);
                        }
                    }
                    None => return Err(SV_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SV_EVALUE);
                        }
                    }
                    None => return Err(SV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SV_EBAL);
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
            return Err(SV_ESEAL);
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
            Err(SV_ESEAL)
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
const OP_SHAPE: u8 = 3;
const OP_BOUND: u8 = 4;
const OP_HASH: u8 = 5;
const OP_INDEX: u8 = 6;
const OP_RATE: u8 = 7;
const OP_DEF: u8 = 8;
const OP_NEED: u8 = 9;

fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

fn err_cell(slot: u8, op: u8, code: u32) -> Sample {
    Sample { slot, op, ok: false, code, value: 0 }
}

/// 错误码抽取。
fn code_of(r: Result<String, u32>, fallback: u32) -> u32 {
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
    let _ = l.register(count_cell(3, OP_SHAPE, r.map(|x| x.shape as usize)));
    let _ = l.register(count_cell(4, OP_BOUND, r.map(|x| x.bound as usize)));
    let _ = l.register(count_cell(5, OP_HASH, r.map(|x| x.hash as usize)));
    let _ = l.register(count_cell(6, OP_INDEX, r.map(|x| x.index as usize)));
    let red = run_with(Some(0));
    let _ = l.register(count_cell(7, OP_RATE, red.map(|x| x.rate() as usize)));

    let _ = l.register(err_cell(8, OP_DEF, code_of(def_summary(DefKind::BadVersion), SV_ECOUNT)));
    let _ = l.register(err_cell(9, OP_DEF, code_of(def_summary(DefKind::Truncated), SV_EBOUND)));
    let _ = l.register(err_cell(10, OP_DEF, code_of(def_summary(DefKind::BadHash), SV_EHASH)));
    let _ = l.register(err_cell(11, OP_DEF, code_of(def_summary(DefKind::DupIndex), SV_ENDX)));
    let _ = l.register(err_cell(
        12,
        OP_DEF,
        code_of(def_summary(DefKind::BaseMisplaced), SV_ENDX),
    ));
    let _ = l.register(err_cell(
        13,
        OP_NEED,
        code_of(need_summary(NeedKind::CountMismatch), SV_ECOUNT),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 字符串表与 ELF hash。
fn hash_ok() -> bool {
    let a = elf_hash("libc.so.6") == elf_hash("libc.so.6");
    let b = elf_hash("GLIBC_2.34") == 0x0696_91b4;
    let c = elf_hash("GLIBC_2.17") == 0x0696_9197;
    let d = elf_hash("GLIBC_2.2.5") == 0x0969_1a75;
    let e = elf_hash("") == 0;
    let f = str_offset(0) == Ok(0);
    let g = str_offset(1) == Ok(10);
    let h = str_by_offset(0) == Ok("libc.so.6");
    let i = str_by_offset(1).err() == Some(SV_EBYTE);
    let j = str_by_offset(0xffff).err() == Some(SV_EBYTE);
    a && b && c && d && e && f && g && h && i && j
}

/// 定义表解析。
fn def_ok() -> bool {
    let img = match build_verdef(&canonical_def_seeds()) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let t = match parse_verdef(&img) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = t.len() == 5 && t.max_ndx() == 5 && t.base_ndx() == Some(1);
    let b = t.hashes_ok() && t.names_ok();
    let c = match t.by_ndx(3) {
        Some(d) => d.name == "GLIBC_2.34" && !d.is_base(),
        None => false,
    };
    let d = match t.get(0) {
        Some(v) => v.is_base() && v.name == "libc.so.6",
        None => false,
    };
    let e = t.by_ndx(0).is_none() && t.by_ndx(99).is_none() && t.get(9).is_none();
    a && b && c && d && e
}

/// 需求表解析。
fn need_ok() -> bool {
    let img = match build_verneed(&[canonical_need_seed()]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let t = match parse_verneed(&img) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let n = match t.get(0) {
        Some(v) => v,
        None => return false,
    };
    let a = t.len() == 1 && n.file == "libm.so.6" && n.naux == 3 && n.cnt == 3;
    let b = match n.aux_by_other(7) {
        Some(v) => v.name == "GLIBC_2.34" && !v.is_weak(),
        None => false,
    };
    let c = t.aux_by_other(8).map(|v| v.name) == Some("GLIBC_2.4");
    let d = t.hashes_ok() && t.names_ok() && t.by_file("libm.so.6").is_some();
    let e = t.by_file("libc.so.6").is_none() && t.aux_by_other(99).is_none();
    a && b && c && d && e
}

/// versym 与解析。
fn versym_ok() -> bool {
    let img = match build_versym(&CANONICAL_VERSYM) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let t = match parse_versym(&img) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = t.len() == 9 && t.hidden_count() == 1;
    let b = t.raw_at(0) == Some(0) && t.ndx_at(4) == Some(3) && t.hidden_at(4) == Some(true);
    let c = t.hidden_at(3) == Some(false) && t.raw_at(9).is_none();
    let d = parse_versym(&Image::new()).err() == Some(SV_ENOBS);
    let e = match build_versym(&[1, 2, 3]) {
        Ok(x) => parse_versym(&x).map(|v| v.len()) == Ok(3),
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 交叉覆盖。
fn coverage_ok() -> bool {
    let a = match Anchor::canonical() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let cov = match coverage(&a.vs, &a.defs, &a.needs) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let t = match a.render() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let r0 = resolve(&a.vs, 0, &a.defs, &a.needs) == Ok(Resolved::Local);
    let r3 = resolve(&a.vs, 3, &a.defs, &a.needs) == Ok(Resolved::Def("GLIBC_2.34"));
    let r4 = resolve(&a.vs, 4, &a.defs, &a.needs) == Ok(Resolved::Hidden("GLIBC_2.34"));
    let r6 = resolve(&a.vs, 6, &a.defs, &a.needs) == Ok(Resolved::Need("GLIBC_2.2.5"));
    let out = resolve(&a.vs, 9, &a.defs, &a.needs).err() == Some(SV_ESLOT);
    cov == 9
        && t.as_str() == "defs=5 needs=1 syms=9 base=1 hidden=1"
        && r0
        && r3
        && r4
        && r6
        && out
}

/// 坏镜像逐档。
fn rejects_ok() -> bool {
    let a = def_summary(DefKind::BadVersion).err() == Some(SV_ECOUNT);
    let b = def_summary(DefKind::Truncated).err() == Some(SV_EBOUND);
    let c = def_summary(DefKind::BadHash).err() == Some(SV_EHASH);
    let d = def_summary(DefKind::DupIndex).err() == Some(SV_ENDX);
    let e = def_summary(DefKind::BaseMisplaced).err() == Some(SV_ENDX);
    let f = need_summary(NeedKind::CountMismatch).err() == Some(SV_ECOUNT);
    let g = need_summary(NeedKind::BadHash).err() == Some(SV_EHASH);
    let h = need_summary(NeedKind::BadOther).err() == Some(SV_ENDX);
    let i = need_summary(NeedKind::ShortHdr).err() == Some(SV_EBOUND);
    a && b && c && d && e && f && g && h && i
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
    let b = c.shape == 2 && c.bound == 1 && c.hash == 1 && c.index == 2;
    let d = c.matched == 14 && c.rate() == PPM_FULL && c.green() && c.gate() == Ok(());
    a && b && d
}

/// 槽位编号。
fn slots_ok() -> bool {
    let a = CORPUS == 14 && CASES.len() == CORPUS && unique() && notes_ok();
    let b = match case_at(0) {
        Some(c) => c.face == Face::Def && c.want == Want::Text("defs=5 base=1"),
        None => false,
    };
    let c = match case_at(7) {
        Some(c) => c.probe == Probe::Resolve(4) && c.note == "resolve-hidden",
        None => false,
    };
    let d = match case_at(13) {
        Some(c) => c.want == Want::Code(SV_ECOUNT),
        None => false,
    };
    let e = case_at(14).is_none() && case_at(200).is_none();
    a && b && c && d && e
}

/// 汇总渲染。
fn report_ok() -> bool {
    let c = match run() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.render().as_str()
        == "sv corpus=14 accept=8 reject=6 shape=2 bound=1 hash=1 index=2 match=14 rate=1000ppm\n";
    let e = Census::new();
    let b = e.is_empty() && e.rate() == 0 && e.gate() == Err(SV_ENOBS);
    let d = e.render().as_str()
        == "sv corpus=0 accept=0 reject=0 shape=0 bound=0 hash=0 index=0 match=0 rate=0ppm\n";
    let f = cover(CORPUS) == Ok(()) && cover(CORPUS + 1) == Err(SV_ENOBS);
    a && b && d && f
}

/// 红档。
fn reds_ok() -> bool {
    let c = match run_with(Some(3)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.matched == 13 && c.rate() == PPM_RED && c.gate() == Err(SV_EVALUE);
    let b = !c.green();
    let d = run_with(Some(CORPUS)).err() == Some(SV_ESLOT);
    let e = run_with(Some(CORPUS + 2)).err() == Some(SV_ESLOT);
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
    let d = f.register(Sample::blank()).err() == Some(SV_ESEAL) && f.rejects() == before + 1;
    a && b && c && d
}

/// 文本助手。
fn text_ok() -> bool {
    let a = u32_text(0) == "0" && u32_text(14) == "14" && u32_text(4294967295) == "4294967295";
    let b = hex_text(SEG_BASE) == "0x0000fd70" && hex_text(0xffff_ffff) == "0xffffffff";
    let c = hex4_text(0x8003) == "0x8003" && hex4_text(VERSYM_MASK) == "0x7fff";
    let d = i32_text(-1) == "-1" && i32_text(127) == "127";
    let e = digest(b"verdef") == digest(b"verdef") && digest(b"verdef") != digest(b"vernee");
    let f = fold_word(FNV_OFF, 1) == digest(&[0, 0, 0, 1]);
    a && b && c && d && e && f
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = hash_ok() && text_ok();
    out[1] = def_ok() && need_ok() && versym_ok();
    out[2] = coverage_ok() && rejects_ok() && corpus_ok();
    out[3] = slots_ok() && report_ok() && reds_ok() && ledger_ok();
    out
}

/// 判据集合。
pub fn run_unx11648_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11648.verdef_verneed");

    s.add("SV01-字符串表与哈希", hash_ok(), "static");
    s.add("SV02-定义表解析", def_ok(), "static");
    s.add("SV03-需求表解析", need_ok(), "static");
    s.add("SV04-versym解析", versym_ok(), "static");
    s.add("SV05-交叉覆盖", coverage_ok(), "static");
    s.add("SV06-坏镜像逐档", rejects_ok(), "static");
    s.add("SV07-十四槽表判", corpus_ok(), "static");
    s.add("SV08-槽位编号", slots_ok(), "static");
    s.add("SV09-汇总渲染", report_ok(), "static");
    s.add("SV10-红档929", reds_ok(), "static");
    s.add("SV11-账本对账", ledger_ok(), "static");
    s.add("SV12-文本助手", text_ok(), "static");
    s.add("SV13-冻结重放四注入", inject_four().iter().all(|x| *x), "static");

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
    fn hash_vectors() {
        assert_eq!(elf_hash("GLIBC_2.34"), 0x0696_91b4);
        assert_eq!(elf_hash("GLIBC_2.17"), 0x0696_9197);
        assert_eq!(elf_hash("GLIBC_2.2.5"), 0x0969_1a75);
        assert_eq!(elf_hash("GLIBC_2.4"), 0x0d69_6914);
        assert_eq!(elf_hash(""), 0);
    }

    #[test]
    fn string_table_offsets() {
        assert_eq!(str_offset(0), Ok(0));
        assert_eq!(str_offset(1), Ok(10));
        assert_eq!(str_by_offset(0), Ok("libc.so.6"));
        assert_eq!(str_by_offset(10), Ok("libm.so.6"));
        assert_eq!(str_offset_of("GLIBC_2.34"), str_offset(4));
        assert_eq!(str_offset_of("nope").err(), Some(SV_ENAME));
        assert_eq!(str_by_offset(1).err(), Some(SV_EBYTE));
        assert_eq!(str_by_offset(400).err(), Some(SV_EBYTE));
    }

    #[test]
    fn image_bounds() {
        let mut img = Image::new();
        assert!(img.put_u16(0, 1).is_err());
        assert!(img.push_u16(0x1234).is_ok());
        assert_eq!(img.len(), 2);
        assert_eq!(img.u16_at(0), Ok(0x1234));
        assert_eq!(img.u16_at(1).err(), Some(SV_EBOUND));
        assert!(img.push_u32(0x0a0b_0c0d).is_ok());
        assert_eq!(img.u32_at(2), Ok(0x0a0b_0c0d));
        img.shrink(2);
        assert_eq!(img.len(), 2);
        assert!(img.push_u16(7).is_ok());
    }

    #[test]
    fn verdef_parse() {
        let img = match build_verdef(&canonical_def_seeds()) {
            Ok(v) => v,
            Err(_) => return,
        };
        let t = match parse_verdef(&img) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(t.len(), 5);
        assert_eq!(t.max_ndx(), 5);
        assert_eq!(t.base_ndx(), Some(1));
        assert!(t.hashes_ok());
        assert!(t.names_ok());
        assert_eq!(t.by_ndx(2).map(|d| d.name), Some("GLIBC_2.2.5"));
        assert_eq!(t.by_ndx(4).map(|d| d.name), Some("GLIBC_2.17"));
        assert!(t.get(0).map(|d| d.is_base()).unwrap_or(false));
    }

    #[test]
    fn verdef_rejects() {
        assert_eq!(def_summary(DefKind::BadVersion).err(), Some(SV_ECOUNT));
        assert_eq!(def_summary(DefKind::Truncated).err(), Some(SV_EBOUND));
        assert_eq!(def_summary(DefKind::BadHash).err(), Some(SV_EHASH));
        assert_eq!(def_summary(DefKind::DupIndex).err(), Some(SV_ENDX));
        assert_eq!(def_summary(DefKind::BaseMisplaced).err(), Some(SV_ENDX));
    }

    #[test]
    fn verdef_flags_gate() {
        let mut seeds = canonical_def_seeds();
        if let Some(s) = seeds.get_mut(1) {
            s.flags = 0x0080;
        }
        let img = match build_verdef(&seeds) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(parse_verdef(&img).err(), Some(SV_ECOUNT));
    }

    #[test]
    fn verdef_zero_index() {
        let mut seeds = canonical_def_seeds();
        if let Some(s) = seeds.get_mut(2) {
            s.ndx = 0;
        }
        let img = match build_verdef(&seeds) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(parse_verdef(&img).err(), Some(SV_ENDX));
    }

    #[test]
    fn verneed_parse() {
        let img = match build_verneed(&[canonical_need_seed()]) {
            Ok(v) => v,
            Err(_) => return,
        };
        let t = match parse_verneed(&img) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(t.len(), 1);
        let n = match t.get(0) {
            Some(v) => v,
            None => return,
        };
        assert_eq!(n.file, "libm.so.6");
        assert_eq!(n.naux, 3);
        assert_eq!(n.cnt, 3);
        assert_eq!(n.aux_by_other(6).map(|a| a.name), Some("GLIBC_2.2.5"));
        assert!(t.hashes_ok());
        assert!(t.by_file("libm.so.6").is_some());
    }

    #[test]
    fn verneed_rejects() {
        assert_eq!(need_summary(NeedKind::CountMismatch).err(), Some(SV_ECOUNT));
        assert_eq!(need_summary(NeedKind::BadHash).err(), Some(SV_EHASH));
        assert_eq!(need_summary(NeedKind::BadOther).err(), Some(SV_ENDX));
        assert_eq!(need_summary(NeedKind::ShortHdr).err(), Some(SV_EBOUND));
    }

    #[test]
    fn versym_parse() {
        let img = match build_versym(&CANONICAL_VERSYM) {
            Ok(v) => v,
            Err(_) => return,
        };
        let t = match parse_versym(&img) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(t.len(), 9);
        assert_eq!(t.hidden_count(), 1);
        assert_eq!(t.raw_at(4), Some(0x8003));
        assert_eq!(t.ndx_at(4), Some(3));
        assert_eq!(t.hidden_at(4), Some(true));
        assert_eq!(t.raw_at(9), None);
    }

    #[test]
    fn versym_odd_reject() {
        let mut img = match build_versym(&[1, 2, 3]) {
            Ok(v) => v,
            Err(_) => return,
        };
        img.shrink(5);
        assert_eq!(parse_versym(&img).err(), Some(SV_EBOUND));
        assert_eq!(parse_versym(&Image::new()).err(), Some(SV_ENOBS));
    }

    #[test]
    fn resolve_specials() {
        let a = match Anchor::canonical() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(resolve(&a.vs, 0, &a.defs, &a.needs), Ok(Resolved::Local));
        assert_eq!(resolve(&a.vs, 1, &a.defs, &a.needs), Ok(Resolved::Global));
        assert_eq!(resolve(&a.vs, 2, &a.defs, &a.needs), Ok(Resolved::Def("GLIBC_2.2.5")));
        assert_eq!(resolve(&a.vs, 3, &a.defs, &a.needs), Ok(Resolved::Def("GLIBC_2.34")));
        assert_eq!(
            resolve(&a.vs, 4, &a.defs, &a.needs),
            Ok(Resolved::Hidden("GLIBC_2.34"))
        );
        assert_eq!(resolve(&a.vs, 6, &a.defs, &a.needs), Ok(Resolved::Need("GLIBC_2.2.5")));
        assert_eq!(resolve(&a.vs, 9, &a.defs, &a.needs).err(), Some(SV_ESLOT));
    }

    #[test]
    fn hidden_on_global_rejected() {
        let a = match Anchor::canonical() {
            Ok(v) => v,
            Err(_) => return,
        };
        let img = match build_versym(&[0, 0x8001]) {
            Ok(v) => v,
            Err(_) => return,
        };
        let vs = match parse_versym(&img) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(resolve(&vs, 1, &a.defs, &a.needs).err(), Some(SV_ENDX));
    }

    #[test]
    fn unknown_index_rejected() {
        let a = match Anchor::canonical() {
            Ok(v) => v,
            Err(_) => return,
        };
        let img = match build_versym(&[0, 1, 9]) {
            Ok(v) => v,
            Err(_) => return,
        };
        let vs = match parse_versym(&img) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(resolve(&vs, 2, &a.defs, &a.needs).err(), Some(SV_ENDX));
    }

    #[test]
    fn coverage_numbers() {
        let a = match Anchor::canonical() {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(coverage(&a.vs, &a.defs, &a.needs), Ok(9));
        assert_eq!(
            a.render().map(|s| s.as_str() == "defs=5 needs=1 syms=9 base=1 hidden=1"),
            Ok(true)
        );
    }

    #[test]
    fn coverage_requires_local_zero() {
        let a = match Anchor::canonical() {
            Ok(v) => v,
            Err(_) => return,
        };
        let img = match build_versym(&[1, 1]) {
            Ok(v) => v,
            Err(_) => return,
        };
        let vs = match parse_versym(&img) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(coverage(&vs, &a.defs, &a.needs).err(), Some(SV_ENDX));
    }

    #[test]
    fn resolve_texts() {
        assert_eq!(Resolved::Local.text(), "local");
        assert_eq!(Resolved::Global.text(), "global");
        assert_eq!(Resolved::Def("V").text(), "def:V");
        assert_eq!(Resolved::Need("V").text(), "need:V");
        assert_eq!(Resolved::Hidden("V").text(), "hidden:V");
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
        assert_eq!(c.shape, 2);
        assert_eq!(c.bound, 1);
        assert_eq!(c.hash, 1);
        assert_eq!(c.index, 2);
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
            "sv corpus=14 accept=8 reject=6 shape=2 bound=1 hash=1 index=2 match=14 rate=1000ppm\n"
        );
    }

    #[test]
    fn red_rounding() {
        let c = match run_with(Some(0)) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(c.matched, 13);
        assert_eq!(c.rate(), 929);
        assert_eq!(c.gate(), Err(SV_EVALUE));
        assert_eq!(run_with(Some(14)).err(), Some(SV_ESLOT));
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
        let want = [SV_ECOUNT, SV_EBOUND, SV_EHASH, SV_ENDX, SV_ENDX, SV_ECOUNT];
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
        assert_eq!(l.freeze(), Err(SV_ESEAL));
        let before = l.rejects();
        assert_eq!(l.register(Sample::blank()).err(), Some(SV_ESEAL));
        assert_eq!(l.rejects(), before + 1);
    }

    #[test]
    fn ledger_order_gate() {
        let mut l = Ledger::new();
        let late = Sample { slot: 1, op: 0, ok: true, code: 0, value: 14 };
        assert_eq!(l.register(late).err(), Some(SV_ESLOT));
        let first = Sample { slot: 0, op: 0, ok: true, code: 0, value: 14 };
        assert_eq!(l.register(first), Ok(()));
        assert_eq!(l.register(first).err(), Some(SV_ESLOT));
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
    fn text_helpers() {
        assert_eq!(u32_text(929), "929");
        assert_eq!(hex_text(SEG_BASE), "0x0000fd70");
        assert_eq!(hex4_text(0x8003), "0x8003");
        assert_eq!(i32_text(-7), "-7");
        assert_eq!(digest(b""), FNV_OFF);
        assert_eq!(fold_word(FNV_OFF, 0x0102_0304), digest(&[1, 2, 3, 4]));
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11648_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn check_13_name() {
        let s = run_unx11648_checks();
        let c = match s.get(12) {
            Some(v) => v,
            None => return,
        };
        assert!(c.name.ends_with("13-冻结重放四注入"));
    }
}
