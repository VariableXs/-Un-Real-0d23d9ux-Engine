//! UNX 卡 10155 — malloc/free 压力面正反双判据（分配守恒达成与双重释放误用受控）
//! 段位 seg=0x10680
//! 判据原文：UNX-F10155-J1 分配守恒达成（malloc/free 循环账守恒 10/10 次）
//! 锚点：malloc 挂块 → free 还块 → 双重释放受控 → 循环账守恒 → 块账指纹
//! 域：HeapBook 堆册（块表 + 活账 + 释放账）
//! 结构：malloc 建块 → free 摘块 → double-free 拒 → 循环守恒 → 压力面 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x10680;
pub const HB_DOUBLE_FREE: u32 = SEG_BASE + 0x01;
pub const HB_ENOMEM: u32 = SEG_BASE + 0x02;
pub const HB_BAD_SIZE: u32 = SEG_BASE + 0x03;
pub const HB_SEALED: u32 = SEG_BASE + 0x04;
pub const HB_BLOCK_CAP: u32 = SEG_BASE + 0x05;

/// 堆容量（字节）与块账容量
pub const HEAP_SIZE: usize = 0x10_0000;
pub const BLOCK_CAP: usize = 0x100;

/// 一个堆块
#[derive(Clone, Copy, Debug)]
pub struct HeapBlock {
    pub id: usize,
    pub size: usize,
    pub live: bool,
}

/// 堆册
pub struct HeapBook {
    pub blocks: Vec<HeapBlock>,
    pub used: usize,
    next_id: usize,
    /// 释放次数账
    pub frees: usize,
    /// 分配次数账
    pub allocs: usize,
    sealed: bool,
}

impl HeapBook {
    pub fn new() -> Self {
        HeapBook {
            blocks: Vec::new(),
            used: 0,
            next_id: 1,
            frees: 0,
            allocs: 0,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// malloc：挂活块
    pub fn malloc(&mut self, size: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(HB_SEALED);
        }
        if size == 0 {
            return Err(HB_BAD_SIZE);
        }
        if self.used + size > HEAP_SIZE {
            return Err(HB_ENOMEM);
        }
        if self.blocks.len() >= BLOCK_CAP {
            return Err(HB_BLOCK_CAP);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.used += size;
        self.allocs += 1;
        self.blocks.push(HeapBlock { id, size, live: true });
        Ok(id)
    }

    /// free：还块（双重释放受控）
    pub fn free(&mut self, id: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(HB_SEALED);
        }
        let i = match self.blocks.iter().position(|b| b.id == id) {
            Some(v) => v,
            None => return Err(HB_DOUBLE_FREE),
        };
        if !self.blocks[i].live {
            return Err(HB_DOUBLE_FREE);
        }
        let n = self.blocks[i].size;
        self.blocks[i].live = false;
        self.used -= n;
        self.frees += 1;
        Ok(n)
    }

    /// 活块页账合计
    pub fn live_bytes(&self) -> usize {
        self.blocks.iter().filter(|b| b.live).map(|b| b.size).sum()
    }

    /// 守恒：used == live_bytes 且 ≤ 堆容
    pub fn conservation(&self) -> bool {
        self.used == self.live_bytes() && self.used <= HEAP_SIZE
    }

    /// 循环压力面：分配 n 块再全释放，账归零
    pub fn stress_cycle(&mut self, n: usize, size: usize) -> Result<bool, u32> {
        let mut ids = Vec::new();
        for _ in 0..n {
            ids.push(self.malloc(size)?);
        }
        for id in ids {
            self.free(id)?;
        }
        Ok(self.used == 0)
    }

    /// 活块计数
    pub fn live_count(&self) -> usize {
        self.blocks.iter().filter(|b| b.live).count()
    }

    /// 分配/释放账对平
    pub fn acct_balanced(&self) -> bool {
        self.allocs >= self.frees && self.allocs - self.frees == self.live_count()
    }

    /// 指纹
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x106800;
        for b in &self.blocks {
            h = h.wrapping_mul(31).wrapping_add(b.id as u32);
            h = h.wrapping_mul(31).wrapping_add(if b.live { b.size as u32 } else { 0 });
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 块账笔数
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// 已还字节合计
    pub fn freed_bytes(&self) -> usize {
        self.blocks.iter().filter(|b| !b.live).map(|b| b.size).sum()
    }

    /// 死块笔数
    pub fn dead_blocks(&self) -> usize {
        self.blocks.iter().filter(|b| !b.live).count()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = HeapBook::new();
    // 1) 双重释放必拒
    let id = b.malloc(16).unwrap_or(0);
    let _ = b.free(id);
    got[0] = b.free(id).is_err();
    // 2) 零长分配必拒
    got[1] = b.malloc(0).is_err();
    // 3) 超堆分配必拒
    got[2] = b.malloc(HEAP_SIZE + 1).is_err();
    // 4) 双释后账守恒
    got[3] = b.conservation();
    // 5) 封册后必拒
    b.seal();
    got[4] = b.malloc(16).is_err();
    got
}

/// 判据入口：UNX-F10155
pub fn run_unx10155_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10155");
    let mut b = HeapBook::new();

    // J1 循环账守恒 10/10：十轮 malloc/free 全还
    let mut j1 = true;
    for i in 0..10usize {
        let id = b.malloc(16 * (i + 1));
        match id {
            Ok(v) => {
                if b.free(v) != Ok(16 * (i + 1)) {
                    j1 = false;
                }
            }
            Err(_) => j1 = false,
        }
    }
    s.add("J1_cycle_10x", j1 && b.used == 0, "malloc/free 循环账守恒 10/10");

    // J2 双重释放受控
    let id = b.malloc(32).unwrap_or(0);
    let _ = b.free(id);
    let j2 = b.free(id) == Err(HB_DOUBLE_FREE);
    s.add("J2_double_free", j2, "双重释放拒 DOUBLE_FREE");

    // J3 零长/超堆受控
    let j3 = b.malloc(0) == Err(HB_BAD_SIZE)
        && b.malloc(HEAP_SIZE + 1) == Err(HB_ENOMEM);
    s.add("J3_bounds", j3, "零长与超堆双拒");

    // J4 并存多块守恒
    let a = b.malloc(0x100).unwrap_or(0);
    let c = b.malloc(0x200).unwrap_or(0);
    let d = b.malloc(0x40).unwrap_or(0);
    let j4 = b.used == 0x340 && b.conservation() && b.live_count() == 3;
    let _ = (a, c, d);
    s.add("J4_multi", j4, "三块并存 0x340 字节账平");

    // J5 部分释放
    let _ = b.free(c);
    let j5 = b.used == 0x140 && b.conservation();
    s.add("J5_partial", j5, "摘中块余 0x140 对平");

    // J6 压力循环
    let mut b2 = HeapBook::new();
    let r = b2.stress_cycle(50, 0x100);
    s.add("J6_stress", r == Ok(true) && b2.acct_balanced(), "50 块压力循环账归零");

    // J7 堆满受控与再释放再分配
    let mut b3 = HeapBook::new();
    let big = b3.malloc(HEAP_SIZE);
    let tail = b3.malloc(16);
    let j7 = big.is_ok() && tail == Err(HB_ENOMEM) && b3.free(big.unwrap_or(0)).is_ok() && b3.malloc(16).is_ok();
    s.add("J7_fill_cycle", j7, "吃满→拒→还→再分通");

    // J8 账面对平
    s.add("J8_acct", b.acct_balanced() && b.allocs >= 4, "分配/释放差 = 活块数");

    // J9 块账容量守门
    let mut b4 = HeapBook::new();
    let mut fill_ok = true;
    for _ in 0..BLOCK_CAP {
        if b4.malloc(8).is_err() {
            fill_ok = false;
        }
    }
    let j9 = fill_ok && b4.malloc(8) == Err(HB_BLOCK_CAP);
    s.add("J9_block_cap", j9, "块满 256 拒 BLOCK_CAP");

    // J10 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J10_inject_all", caught == 5, "5 注入点全检出");

    // J11 封册受控
    let mut b5 = HeapBook::new();
    let z = b5.malloc(16).unwrap_or(0);
    b5.seal();
    let j11 = b5.malloc(16) == Err(HB_SEALED) && b5.free(z) == Err(HB_SEALED);
    s.add("J11_sealed", j11, "封册后分配/释放皆拒 SEALED");

    // J12 野柄受控
    let j12 = b.free(9999) == Err(HB_DOUBLE_FREE);
    s.add("J12_wild_id", j12, "野块柄同码受控");

    // J13 指纹稳定
    let f1 = b.fold();
    let f2 = b.fold();
    s.add("J13_fingerprint", f1 == f2, "块账折叠指纹一致");

    // J14 id 单调
    let ids: Vec<usize> = b.blocks.iter().map(|bl| bl.id).collect();
    let mut mono = true;
    for w in ids.windows(2) {
        if w[1] <= w[0] {
            mono = false;
        }
    }
    s.add("J14_id_mono", mono, "块 id 单调");

    // J15 千分位占用
    let pm = b.used * 1000 / HEAP_SIZE;
    let want = b.used * 1000 / HEAP_SIZE;
    s.add("J15_permille", pm == want && b.used == 0x140, "堆占比千分位与账对平");

    // J16 块账笔数
    let j16 = b.block_count() == 14;
    s.add("J16_blocks", j16, "十循环加四压力块共 14 笔");

    // J17 已还字节对平
    let freed = b.freed_bytes();
    let j17 = freed == 880 + 32 + 0x200 && b.live_bytes() == b.used;
    s.add("J17_freed", j17, "已还 1424 字节（含部分释放）与活账双平");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap55_cycle() {
        let mut b = HeapBook::new();
        let id = b.malloc(64).unwrap();
        assert_eq!(b.used, 64);
        assert_eq!(b.free(id), Ok(64));
        assert_eq!(b.used, 0);
        assert!(b.conservation());
    }

    #[test]
    fn test_heap55_double_free() {
        let mut b = HeapBook::new();
        let id = b.malloc(16).unwrap();
        let _ = b.free(id);
        assert_eq!(b.free(id), Err(HB_DOUBLE_FREE));
    }

    #[test]
    fn test_heap55_checks() {
        let cs = run_unx10155_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_heap55_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
