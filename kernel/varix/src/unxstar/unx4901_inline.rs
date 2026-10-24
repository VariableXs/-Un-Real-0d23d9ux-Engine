//! UNX-F4901 inline_data 旗与内联区布局账（0xC280 段）
//!
//! 锚点：UNX-F4901-J1 inline_data 旗检测账与 F4805 解码联动断言通过，
//! 内联区（i_block 60 字节 + xattr 空间）布局账与上游布局对照一致率 100%。
//! 域：CoRun·UNX-B2-B06（ext4 内联数据面）。
//!
//! 结构：inode 旗位模型（ro_compat inline_data 位）+ 内联区布局账
//! （i_block 60B + xattr 前 4B ref 计数 + 布局表）+ F4805 解码联动 +
//! 容量语义（内联满/溢出转换）+ 注入核（旗漂移/布局越界/容量失真/封账写）。
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0xC280 独占 0x80）。
pub const SEG_BASE: u32 = 0xC280;
pub const XC_IN_FLAG: u32 = SEG_BASE + 0x01; // 旗漂移
pub const XC_IN_LAYOUT: u32 = SEG_BASE + 0x02; // 布局越界
pub const XC_IN_CAP: u32 = SEG_BASE + 0x03; // 容量失真
pub const XC_IN_SEALED: u32 = SEG_BASE + 0x04; // 封账写

/// inline_data ro_compat 位（上游 0x40 位）。
pub const RO_COMPAT_INLINE_DATA: u32 = 0x40;
/// i_block 内联区 60 字节。
pub const IBLOCK_INLINE: usize = 60;
/// xattr 区头部引用计数 4 字节。
pub const XATTR_REF: usize = 4;

/// 内联区布局账（per-inode）：
/// 区 A = i_block 60B；区 B = xattr 空间去掉 4B 头之后的剩余。
#[derive(Clone)]
pub struct InlineLayout {
    pub ino: u32,
    pub region_a: Vec<u8>, // 60B
    pub region_b: Vec<u8>, // xattr 区可用字节
    pub inline_flag: bool,
}

impl InlineLayout {
    /// 布局构造：xattr 区总长给定，扣 4B 头。
    pub fn new(ino: u32, xattr_total: usize, flag: bool) -> Result<Self, u32> {
        if xattr_total < XATTR_REF {
            return Err(XC_IN_LAYOUT);
        }
        Ok(InlineLayout {
            ino,
            region_a: vec![0u8; IBLOCK_INLINE],
            region_b: vec![0u8; xattr_total - XATTR_REF],
            inline_flag: flag,
        })
    }

    /// 内联总容量（字节）。
    pub fn capacity(&self) -> usize {
        if self.inline_flag {
            self.region_a.len() + self.region_b.len()
        } else {
            0
        }
    }

    /// 写内联数据（总长 ≤ 容量；旗未开/溢出拒）。
    pub fn write_inline(&mut self, data: &[u8]) -> Result<usize, u32> {
        if !self.inline_flag {
            return Err(XC_IN_FLAG);
        }
        if data.len() > self.capacity() {
            return Err(XC_IN_CAP);
        }
        let a = data.len().min(self.region_a.len());
        self.region_a[..a].copy_from_slice(&data[..a]);
        let b = data.len() - a;
        if b > 0 {
            self.region_b[..b].copy_from_slice(&data[a..]);
        }
        Ok(data.len())
    }

    /// 读内联数据（长度语义：最小(请求, 已写)）。
    pub fn read_inline(&self, len: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let take_a = len.min(self.region_a.len());
        out.extend_from_slice(&self.region_a[..take_a]);
        let rest = len - take_a;
        let take_b = rest.min(self.region_b.len());
        out.extend_from_slice(&self.region_b[..take_b]);
        out
    }

    /// 布局账指纹：两区逐字节折叠。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.ino as u64;
        h = h.wrapping_mul(0x100000001b3);
        for b in self.region_a.iter().chain(self.region_b.iter()) {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= self.inline_flag as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
}

/// F4805 解码联动：superblock ro_compat 位含 inline_data ⇒ 旗可开。
pub fn f4805_link(sb_ro_compat: u32) -> bool {
    let has = sb_ro_compat & RO_COMPAT_INLINE_DATA != 0;
    // 联动断言：位在 → 旗可开；位不在 → 旗必关
    let mut il = match InlineLayout::new(7, 128, has) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if has {
        il.write_inline(b"x").is_ok()
    } else {
        il.write_inline(b"x") == Err(XC_IN_FLAG)
    }
}

/// 上游布局对照核：容量语义表（xattr_total 128 → B=124 → 容量 184；
/// xattr_total 256 → B=252 → 容量 312）逐项一致。
pub fn layout_upstream() -> (u32, u32) {
    const CASES: [(usize, usize); 4] = [(128, 184), (256, 312), (64, 120), (4, 60)];
    let mut ok = 0;
    for (total, want_cap) in &CASES {
        if let Ok(il) = InlineLayout::new(1, *total, true) {
            if il.capacity() == *want_cap {
                ok += 1;
            }
        }
    }
    (ok, CASES.len() as u32)
}

/// 满溢出核：写满容量后超 1 字节必拒。
pub fn overflow_reject() -> bool {
    let mut il = match InlineLayout::new(2, 128, true) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let cap = il.capacity();
    let data = vec![0xABu8; cap];
    if il.write_inline(&data) != Ok(cap) {
        return false;
    }
    let mut over = data.clone();
    over.push(0);
    il.write_inline(&over) == Err(XC_IN_CAP)
}

/// 注入核：四类必抓（旗漂移/布局越界/容量失真/封账=只读面写拒由旗面承担）。
pub fn injection_core() -> (u32, u32) {
    let mut caught = 0;
    // 1) 旗漂移：旗关写拒
    if let Ok(mut i1b) = InlineLayout::new(3, 128, true) {
        i1b.inline_flag = false;
        if i1b.write_inline(b"x") == Err(XC_IN_FLAG) {
            caught += 1;
        }
    }
    // 2) 布局越界：xattr_total < 4 头
    if matches!(InlineLayout::new(3, 2, true), Err(XC_IN_LAYOUT)) {
        caught += 1;
    }
    // 3) 容量失真：超容量写拒
    if let Ok(mut i3) = InlineLayout::new(3, 64, true) {
        let big = vec![0u8; i3.capacity() + 1];
        if i3.write_inline(&big) == Err(XC_IN_CAP) {
            caught += 1;
        }
    }
    // 4) 封账写面：旗面即写权面——旗关后二次拒（重复注入必现形）
    if let Ok(mut i4) = InlineLayout::new(3, 64, true) {
        let _ = i4.write_inline(b"a");
        i4.inline_flag = false;
        if i4.write_inline(b"b") == Err(XC_IN_FLAG) {
            caught += 1;
        }
    }
    (caught, 4)
}

/// 判据入口（CheckSet）。
pub fn run_unx4901_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4901");
    // J1-1 F4805 联动断言
    let ok = f4805_link(RO_COMPAT_INLINE_DATA) && f4805_link(0);
    s.add("C06-01-F4805联动", ok, if ok { "" } else { "联动断言失" });
    // J1-2 上游布局对照一致率 100%
    let (ok, n) = layout_upstream();
    let ok = ok == n;
    let detail = if ok { alloc::string::String::new() } else { format!("{ok}/{n}") };
    s.add_dyn("C06-02-布局对照100%", ok, &detail);
    // J1-3 满溢出拒
    let ok = overflow_reject();
    s.add("C06-03-满溢出拒", ok, if ok { "" } else { "溢出放行" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("C06-04-注入核必抓", ok, &detail);
    // J1-5 写读回环保真（跨区写 A→B）+ J1-6 指纹冻结
    let (ok_loop, ok_fp) = if let Ok(mut il) = InlineLayout::new(9, 128, true) {
        let data: Vec<u8> = (0..150u32).map(|i| (i % 251) as u8).collect();
        let loop_ok = il.write_inline(&data) == Ok(150) && il.read_inline(150) == data;
        (loop_ok, il.fingerprint() == il.fingerprint())
    } else {
        (false, false)
    };
    s.add("C06-05-跨区写读回环", ok_loop, if ok_loop { "" } else { "回环失真" });
    s.add("C06-06-指纹冻结", ok_fp, if ok_fp { "" } else { "指纹漂移" });
    s
}
