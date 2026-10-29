//! UNX 卡 10151 — mlock 正反双判据（锁定达成与超限锁定误用受控）
//! 段位 seg=0x10600
//! 判据原文：UNX-F10151-J1 锁定达成（mlock 页驻留语义 10/10 次）
//! 锚点：mlock 页驻留锁定 + munlock 解锁 + 超限锁定受控 + 锁账守恒
//! 域：LockBook 锁定册（区间锁位 + 限额账）
//! 结构：mmap 区挂账 → mlock 锁页 → munlock 解锁 → 超限受控 → 锁账守恒 → 注入检出

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x10600;
pub const LK_ENOMEM: u32 = SEG_BASE + 0x01;
pub const LK_UNMAPPED: u32 = SEG_BASE + 0x02;
pub const LK_SEALED: u32 = SEG_BASE + 0x03;
pub const LK_ZERO_LEN: u32 = SEG_BASE + 0x04;
pub const LK_DOUBLE: u32 = SEG_BASE + 0x05;

/// 锁限额（页）与区间容量
pub const RLIMIT_PAGES: usize = 0x20;
pub const REGION_CAP: usize = 0x40;

/// 一个可锁区间
#[derive(Clone, Copy, Debug)]
pub struct LockRegion {
    pub start: usize,
    pub npages: usize,
    pub locked: bool,
}

/// 锁定册
pub struct LockBook {
    pub regions: Vec<LockRegion>,
    pub locked_pages: usize,
    sealed: bool,
}

impl LockBook {
    pub fn new() -> Self {
        LockBook {
            regions: Vec::new(),
            locked_pages: 0,
            sealed: false,
        }
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 挂区间
    pub fn attach(&mut self, start: usize, npages: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(LK_SEALED);
        }
        if npages == 0 {
            return Err(LK_ZERO_LEN);
        }
        if self.regions.len() >= REGION_CAP {
            return Err(LK_ENOMEM);
        }
        self.regions.push(LockRegion {
            start,
            npages,
            locked: false,
        });
        Ok(self.regions.len() - 1)
    }

    /// mlock：锁区间页；超限 ENOMEM；重复锁受控
    pub fn mlock(&mut self, idx: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(LK_SEALED);
        }
        let r = self.regions.get_mut(idx).ok_or(LK_UNMAPPED)?;
        if r.locked {
            return Err(LK_DOUBLE);
        }
        if self.locked_pages + r.npages > RLIMIT_PAGES {
            return Err(LK_ENOMEM);
        }
        r.locked = true;
        self.locked_pages += r.npages;
        Ok(r.npages)
    }

    /// munlock：解锁；未锁受控
    pub fn munlock(&mut self, idx: usize) -> Result<usize, u32> {
        if self.sealed {
            return Err(LK_SEALED);
        }
        let r = self.regions.get_mut(idx).ok_or(LK_UNMAPPED)?;
        if !r.locked {
            return Err(LK_DOUBLE);
        }
        r.locked = false;
        self.locked_pages -= r.npages;
        Ok(r.npages)
    }

    /// 锁位查询
    pub fn locked_of(&self, idx: usize) -> Result<bool, u32> {
        self.regions.get(idx).map(|r| r.locked).ok_or(LK_UNMAPPED)
    }

    /// 守恒：locked_pages == 在锁区间页数合计
    pub fn conservation(&self) -> bool {
        let sum: usize = self.regions.iter().filter(|r| r.locked).map(|r| r.npages).sum();
        sum == self.locked_pages && self.locked_pages <= RLIMIT_PAGES
    }

    /// 千分位占用
    pub fn lock_permille(&self) -> usize {
        self.locked_pages * 1000 / RLIMIT_PAGES
    }

    /// 锁面直方
    pub fn lock_hist(&self) -> (usize, usize) {
        let l = self.regions.iter().filter(|r| r.locked).count();
        (l, self.regions.len() - l)
    }

    /// 指纹
    pub fn render(&self) -> String {
        let mut s = String::new();
        for r in &self.regions {
            s.push_str(&format!("{}:{}{};", r.start, r.npages, if r.locked { "L" } else { "u" }));
        }
        s
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 区间数
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// 未锁页数
    pub fn unlocked_pages(&self) -> usize {
        self.regions.iter().filter(|r| !r.locked).map(|r| r.npages).sum()
    }

    /// 按起始地址查锁位
    pub fn is_locked(&self, start: usize) -> bool {
        self.regions
            .iter()
            .find(|r| r.start == start)
            .map(|r| r.locked)
            .unwrap_or(false)
    }

    /// 在锁区间计数
    pub fn locked_regions(&self) -> usize {
        self.regions.iter().filter(|r| r.locked).count()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = LockBook::new();
    let a = b.attach(0x1000, 2).unwrap_or(0);
    // 1) 超限锁定必拒
    let big = b.attach(0x100000, RLIMIT_PAGES + 1).unwrap_or(1);
    got[0] = b.mlock(big).is_err();
    // 2) 重复锁必拒
    let _ = b.mlock(a);
    got[1] = b.mlock(a).is_err();
    // 3) 未锁先解必拒
    got[2] = b.munlock(big).is_err();
    // 4) 野柄必拒
    got[3] = b.mlock(99).is_err();
    // 5) 封册后必拒
    b.seal();
    got[4] = b.munlock(a).is_err();
    got
}

/// 判据入口：UNX-F10151
pub fn run_unx10151_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10151");
    let mut b = LockBook::new();

    // J1 锁定达成 10/10：十区间各 2 页逐锁
    let mut idxs = Vec::new();
    let mut j1 = true;
    for i in 0..10usize {
        let idx = b.attach(0x1000 * (i + 1), 2);
        match idx {
            Ok(v) => {
                if b.mlock(v) != Ok(2) {
                    j1 = false;
                }
                idxs.push(v);
            }
            Err(_) => j1 = false,
        }
    }
    s.add("J1_mlock_10x", j1 && b.locked_pages == 20, "十区间页驻留锁定 10/10");

    // J2 解锁归还
    let u0 = idxs[0];
    let j2 = b.munlock(u0) == Ok(2) && b.locked_pages == 18 && b.locked_of(u0) == Ok(false);
    s.add("J2_munlock", j2, "解锁归还 2 页");

    // J3 重复锁受控
    let j3 = b.mlock(u0) == Ok(2) && b.mlock(u0) == Err(LK_DOUBLE);
    s.add("J3_double_lock", j3, "重锁后解再锁通、三次锁拒 DOUBLE");

    // J4 超限受控
    let mut b2 = LockBook::new();
    let _ = b2.attach(0x1000, RLIMIT_PAGES);
    let over = b2.attach(0x100000, RLIMIT_PAGES + 1);
    let j4 = over.is_ok() && b2.mlock(over.unwrap_or(1)) == Err(LK_ENOMEM);
    s.add("J4_enomem", j4, "越 RLIMIT 锁定拒 ENOMEM");

    // J5 吃满→解→再锁
    let r0 = b2.attach(0x2000, 1).unwrap_or(2);
    let j5 = b2.mlock(0) == Ok(RLIMIT_PAGES as usize)
        && b2.munlock(0) == Ok(RLIMIT_PAGES as usize)
        && b2.mlock(r0) == Ok(1);
    s.add("J5_cycle", j5, "吃满→解→补锁循环通");

    // J6 未锁先解受控
    let mut b3 = LockBook::new();
    let n = b3.attach(0x1000, 1).unwrap_or(0);
    s.add("J6_unlock_fresh", b3.munlock(n) == Err(LK_DOUBLE), "未锁先解拒 DOUBLE");

    // J7 野柄受控
    let j7 = b3.mlock(99) == Err(LK_UNMAPPED) && b3.munlock(99) == Err(LK_UNMAPPED);
    s.add("J7_bad_idx", j7, "野柄双路受控 UNMAPPED");

    // J8 守恒
    s.add("J8_conservation", b.conservation() && b2.conservation(), "双册锁账守恒");

    // J9 千分位
    let pm = b.lock_permille();
    let want = b.locked_pages * 1000 / RLIMIT_PAGES;
    s.add("J9_permille", pm == want && pm > 0, "锁占比千分位对平");

    // J10 锁面直方
    let (l, u) = b.lock_hist();
    s.add("J10_hist", l == 10 && u == 0, "十区间全在锁");

    // J11 注入 5 项全抓
    let inj = inject_five();
    let caught = inj.iter().filter(|&&x| x).count();
    s.add_dyn("J11_inject_all", caught == 5, "5 注入点全检出");

    // J12 封册受控
    let mut b4 = LockBook::new();
    let z = b4.attach(0x1000, 1).unwrap_or(0);
    b4.seal();
    let j12 = b4.mlock(z) == Err(LK_SEALED) && b4.attach(0x2000, 1) == Err(LK_SEALED);
    s.add("J12_sealed", j12, "封册后锁定/挂账皆拒 SEALED");

    // J13 零长受控（独立未封册）
    let mut b6 = LockBook::new();
    s.add("J13_zero", b6.attach(0x3000, 0) == Err(LK_ZERO_LEN), "零页挂账受控");

    // J14 指纹稳定
    let f1 = b.render();
    let f2 = b.render();
    s.add("J14_fingerprint", f1 == f2 && f1.contains(":2L;"), "锁面指纹稳定");

    // J15 锁页边界恰满
    let mut b5 = LockBook::new();
    let a5 = b5.attach(0x1000, RLIMIT_PAGES).unwrap_or(0);
    let b5b = b5.attach(0x2000, 1).unwrap_or(1);
    let j15 = b5.mlock(a5).is_ok() && b5.mlock(b5b) == Err(LK_ENOMEM) && b5.lock_permille() == 1000;
    s.add("J15_exact_fit", j15, "恰满 32 页后补页拒且占比满额");

    // J16 区间计数
    let j16 = b.region_count() == 10 && b2.region_count() == 3;
    s.add("J16_regions", j16, "主册十区、限册三区");

    // J17 未锁页数
    let j17 = b.unlocked_pages() == 0 && b3.unlocked_pages() == 1;
    s.add("J17_unlocked", j17, "主册全锁、fresh 册一页未锁");

    // J18 按址查锁
    let j18 = b.is_locked(0x1000 * 4) && !b.is_locked(0xDEAD_0000);
    s.add("J18_is_locked", j18, "按址命中锁位、野址判未锁");

    // J19 解锁再计数
    let _ = b.munlock(idxs[1]);
    let j19 = b.locked_pages == 18 && b.unlocked_pages() == 2;
    s.add("J19_after_unlock", j19, "再解 2 页余 18 在锁");

    // J20 解后重锁
    let r20 = b.mlock(idxs[1]);
    let j20 = r20 == Ok(2) && b.conservation();
    s.add("J20_relock", j20, "重锁归位且守恒");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock51_flow() {
        let mut b = LockBook::new();
        let i = b.attach(0x1000, 3).unwrap();
        assert_eq!(b.mlock(i), Ok(3));
        assert_eq!(b.locked_pages, 3);
        assert_eq!(b.munlock(i), Ok(3));
        assert!(b.conservation());
    }

    #[test]
    fn test_lock51_enomem() {
        let mut b = LockBook::new();
        let a = b.attach(0x1000, RLIMIT_PAGES).unwrap();
        let c = b.attach(0x2000, 1).unwrap();
        assert!(b.mlock(a).is_ok());
        assert_eq!(b.mlock(c), Err(LK_ENOMEM));
    }

    #[test]
    fn test_lock51_checks() {
        let cs = run_unx10151_checks();
        assert!(cs.all_passed());
    }

    #[test]
    fn test_lock51_inject() {
        assert!(inject_five().iter().all(|&x| x));
    }
}
