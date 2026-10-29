//! UNX 卡 10153 — 越界访问检出正反双判据（守卫页达成与越界复现受控）
//! 段位 seg=0x10640
//! 判据原文：UNX-F10153-J1 守卫页达成（守卫页触发受控 SIGSEGV 拦截 10/10 次）
//! 锚点：守卫页部署 → 越界触界拦截 SIGSEGV → 守卫翻起/撤防 → 越界复现受控
//! 域：GuardBook 守卫页册（区 + 守卫边界 + 触界账）
//! 结构：挂区设守卫 → probe 触界 → SIGSEGV 拦截账 → 撤防后通行 → 误用受控 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x10640;
pub const GB_NO_GUARD: u32 = SEG_BASE + 0x01;
pub const GB_SEALED: u32 = SEG_BASE + 0x02;
pub const GB_BAD_ADDR: u32 = SEG_BASE + 0x03;
pub const GB_CAP: u32 = SEG_BASE + 0x04;
pub const GB_DOUBLE: u32 = SEG_BASE + 0x05;

/// 区容量与页粒度
pub const REGION_CAP: usize = 0x20;
pub const PAGE: usize = 0x1000;

/// 一个带守卫的区
#[derive(Clone, Copy, Debug)]
pub struct GuardRegion {
    pub start: usize,
    pub npages: usize,
    pub guard_armed: bool,
    pub faults: u32,
}

/// 守卫页册
pub struct GuardBook {
    pub regions: Vec<GuardRegion>,
    pub caught: u32,
    sealed: bool,
}

impl GuardBook {
    pub fn new() -> Self {
        GuardBook {
            regions: Vec::new(),
            caught: 0,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂区（未设守卫）
    pub fn attach(&mut self, start: usize, npages: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(GB_SEALED);
        }
        if npages == 0 || start % PAGE != 0 {
            return Err(GB_BAD_ADDR);
        }
        if self.regions.len() >= REGION_CAP {
            return Err(GB_CAP);
        }
        self.regions.push(GuardRegion {
            start,
            npages,
            guard_armed: false,
            faults: 0,
        });
        Ok(self.regions.len() - 1)
    }

    /// 设守卫（重复设受控）
    pub fn arm(&mut self, idx: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(GB_SEALED);
        }
        let r = self.regions.get_mut(idx).ok_or(GB_NO_GUARD)?;
        if r.guard_armed {
            return Err(GB_DOUBLE);
        }
        r.guard_armed = true;
        Ok(())
    }

    /// 撤防
    pub fn disarm(&mut self, idx: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(GB_SEALED);
        }
        let r = self.regions.get_mut(idx).ok_or(GB_NO_GUARD)?;
        if !r.guard_armed {
            return Err(GB_NO_GUARD);
        }
        r.guard_armed = false;
        Ok(())
    }

    /// probe 访问：区内通行；越尾且守卫武装 → SIGSEGV 拦截账 +1
    pub fn probe(&mut self, addr: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(GB_SEALED);
        }
        for r in self.regions.iter_mut() {
            let end = r.start + r.npages * PAGE;
            let in_data = addr >= r.start && addr < end;
            let on_guard = r.guard_armed && (addr == end || (addr >= end && addr < end + PAGE));
            if in_data {
                return Ok(());
            }
            if on_guard {
                r.faults += 1;
                self.caught += 1;
                return Err(GB_NO_GUARD); // 复用为 SIGSEGV 拦截码
            }
        }
        Err(GB_BAD_ADDR)
    }

    /// 拦截账合计
    pub fn caught_total(&self) -> u32 {
        self.caught
    }

    /// 守卫武装数
    pub fn armed_count(&self) -> usize {
        self.regions.iter().filter(|r| r.guard_armed).count()
    }

    /// 撤防后通行核：撤防前拦截、撤防后不再守卫拦截且账不增
    pub fn pass_after_disarm(&mut self, idx: usize, beyond: usize) -> bool {
        let r = match self.regions.get(idx) {
            Some(r) => *r,
            None => return false,
        };
        let end = r.start + r.npages * PAGE;
        let addr = end + beyond;
        if self.probe(addr) != Err(GB_NO_GUARD) {
            return false;
        }
        if self.disarm(idx).is_err() {
            return false;
        }
        let c = self.caught;
        self.probe(addr) == Err(GB_BAD_ADDR) && self.caught == c
    }

    /// 指纹
    pub fn render(&self) -> String {
        let mut s = String::new();
        for r in &self.regions {
            s.push_str(&format!("{}:{}{};", r.start, r.npages, if r.guard_armed { "G" } else { "-" }));
        }
        s
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = GuardBook::new();
    let i = b.attach(0x1000, 2).unwrap_or(0);
    // 1) 未设守卫越界不得误判拦截（应为野址码而非守卫拦截）
    let _ = b.probe(0x1000 + 2 * PAGE);
    got[0] = b.caught_total() == 0;
    // 2) 重复设守卫必拒
    let _ = b.arm(i);
    got[1] = b.arm(i).is_err();
    // 3) 非对齐挂区必拒
    got[2] = b.attach(0x1800, 1).is_err();
    // 4) 野地址探针不误记守卫账
    let c0 = b.caught_total();
    let _ = b.probe(0xDEAD_0000);
    got[3] = b.caught_total() == c0;
    // 5) 封册后必拒
    b.seal();
    got[4] = b.probe(0x1000).is_err();
    got
}

/// 判据入口：UNX-F10153
pub fn run_unx10153_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10153");
    let mut b = GuardBook::new();

    // J1 守卫页达成 10/10：十区设守卫逐区触界拦截
    let mut j1 = true;
    for i in 0..10usize {
        let idx = b.attach(0x1000 * (i + 1) * 4, 2);
        let idx = match idx {
            Ok(v) => v,
            Err(_) => {
                j1 = false;
                break;
            }
        };
        if b.arm(idx).is_err() {
            j1 = false;
        }
        let addr = 0x1000 * (i + 1) * 4 + 2 * PAGE; // 区尾后一字节
        if b.probe(addr) != Err(GB_NO_GUARD) {
            j1 = false;
        }
    }
    s.add("J1_guard_10x", j1 && b.caught_total() == 10, "守卫页触发拦截 10/10");

    // J2 区内通行
    let j2 = b.probe(0x4000) == Ok(()) && b.probe(0x4000 + PAGE + 0x10) == Ok(());
    s.add("J2_in_region", j2, "数据页访问通行不拦截");

    // J3 拦截账与区账对平
    let sum: u32 = b.regions.iter().map(|r| r.faults).sum();
    s.add("J3_faults", b.caught_total() == sum && sum == 10, "区账 10 触界与总账对平");

    // J4 撤防后通行
    let j4 = b.pass_after_disarm(0, 0);
    s.add("J4_disarm_pass", j4, "撤防后守卫位通行");

    // J5 重复设守卫受控
    let a1 = b.attach(0x9000, 1).unwrap_or(0);
    let _ = b.arm(a1);
    s.add("J5_double_arm", b.arm(a1) == Err(GB_DOUBLE), "重复设守卫拒 DOUBLE");

    // J6 未设守卫先撤受控
    let a2 = b.attach(0xA000, 1).unwrap_or(0);
    s.add("J6_disarm_fresh", b.disarm(a2) == Err(GB_NO_GUARD), "未武装撤防拒 NO_GUARD");

    // J7 非对齐挂区受控
    let j7 = b.attach(0x1800, 1) == Err(GB_BAD_ADDR)
        && b.attach(0xB000, 0) == Err(GB_BAD_ADDR);
    s.add("J7_bad_attach", j7, "非对齐与零页挂区受控");

    // J8 野地址受控
    let c0 = b.caught_total();
    let j8 = b.probe(0xDEAD_0000) == Err(GB_BAD_ADDR) && b.caught_total() == c0;
    s.add("J8_wild", j8, "野地址 BAD_ADDR 且不动守卫账");

    // J9 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J9_inject_all", caught == 5, "5 注入点全检出");

    // J10 封册受控
    let mut b2 = GuardBook::new();
    let z = b2.attach(0x1000, 1).unwrap_or(0);
    b2.seal();
    let j10 = b2.probe(0x1000) == Err(GB_SEALED)
        && b2.arm(z) == Err(GB_SEALED)
        && b2.attach(0x2000, 1) == Err(GB_SEALED);
    s.add("J10_sealed", j10, "封册后三操作皆拒 SEALED");

    // J11 区容量守门
    let mut b3 = GuardBook::new();
    let mut fill_ok = true;
    for i in 0..REGION_CAP {
        if b3.attach(0x1000 * (i + 1) * 8, 1).is_err() {
            fill_ok = false;
        }
    }
    let j11 = fill_ok && b3.attach(0, 1) == Err(GB_CAP);
    s.add("J11_cap", j11, "区满 32 拒 CAP");

    // J12 指纹稳定
    let f1 = b.render();
    let f2 = b.render();
    s.add("J12_fingerprint", f1 == f2 && f1.contains(":2G;"), "守卫面指纹稳定");

    // J13 武装计数
    s.add("J13_armed", b.armed_count() == 10, "武装守卫 10 区（1 区已撤）");

    // J14 守卫页宽度
    let mut b4 = GuardBook::new();
    let g = b4.attach(0x1_0000, 1).unwrap_or(0);
    let _ = b4.arm(g);
    let edge = b4.probe(0x1_0000 + PAGE); // 守卫页内
    let over = b4.probe(0x1_0000 + 2 * PAGE); // 守卫页外
    s.add("J14_guard_width", edge == Err(GB_NO_GUARD) && over == Err(GB_BAD_ADDR), "守卫页内拦、页外野址拒");

    // J15 拦截不递增通行的账
    let ok0 = b.caught_total();
    let _ = b.probe(0x4000 + 5);
    s.add("J15_no_fault_on_ok", b.caught_total() == ok0, "通行访问不增拦截账");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guard53_fault() {
        let mut b = GuardBook::new();
        let i = b.attach(0x1000, 1).unwrap();
        b.arm(i).unwrap();
        assert_eq!(b.probe(0x1000), Ok(()));
        assert_eq!(b.probe(0x2000), Err(GB_NO_GUARD));
        assert_eq!(b.caught_total(), 1);
    }

    #[test]
    fn test_guard53_misuse() {
        let mut b = GuardBook::new();
        let i = b.attach(0x1000, 1).unwrap();
        assert!(b.arm(i).is_ok());
        assert_eq!(b.arm(i), Err(GB_DOUBLE));
        assert_eq!(b.attach(0x1800, 1), Err(GB_BAD_ADDR));
    }

    #[test]
    fn test_guard53_checks() {
        let cs = run_unx10153_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_guard53_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
