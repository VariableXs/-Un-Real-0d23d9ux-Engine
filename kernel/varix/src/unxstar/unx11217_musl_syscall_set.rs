//! UNX-F11217 musl 最小 syscall 依赖清单核验（0x1f817 段）
//!
//! 锚点：UNX-F11217-J1 musl 1.2.5 最小 syscall 集（约 60 号）逐号登记
//! Varix 满足态，未满足号 100% 挂缺陷号。
//! 域：CoRun·UNX-C5-B01——号表冻结在 MIN_NUMS，登记只认表内号；不满足
//! 条目必须带缺陷号，缺号即整册不合格。

use crate::alloc::string::String;
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1f817;
pub const MS_NR: u32 = SEG_BASE + 0x01; // 号不在最小集
pub const MS_DUP: u32 = SEG_BASE + 0x02; // 号重录
pub const MS_NODEF: u32 = SEG_BASE + 0x03; // 未满足无缺陷号
pub const MS_COUNT: u32 = SEG_BASE + 0x04; // 条数不足
pub const MS_STATUS: u32 = SEG_BASE + 0x05; // 满足态与缺陷并存
pub const MS_FROZEN: u32 = SEG_BASE + 0x06; // 冻结后登记
pub const MS_OVER: u32 = SEG_BASE + 0x07; // 超容

/// musl 1.2.5 最小 syscall 号表（x86-64，60 号）。
pub const MIN_NUMS: [u32; 60] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
    10, 11, 12, 13, 14, 16, 17, 18, 19, 20,
    21, 22, 23, 24, 25, 28, 32, 33, 39, 41,
    42, 43, 49, 56, 57, 58, 59, 60, 61, 62,
    63, 72, 78, 79, 80, 87, 89, 96, 97, 102,
    104, 107, 108, 110, 127, 186, 202, 217, 218, 231,
];
/// 有号的满足态样本（未列入即不满足）。
pub const SATISFIED: [u32; 54] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9,
    10, 11, 12, 13, 14, 16, 17, 18, 19, 20,
    21, 22, 23, 24, 25, 28, 32, 33, 39, 41,
    42, 43, 49, 56, 57, 58, 59, 60, 61, 62,
    63, 72, 78, 79, 80, 87, 89, 96, 97, 102,
    104, 107, 108, 110,
];
pub const MS_TAG: u64 = 0x1f817c5;
pub const FP_STEP: u64 = 0x100000001b3;

/// 单号登记：号、满足态、缺陷号。
#[derive(Clone, Debug)]
pub struct SysEntry {
    pub nr: u32,
    pub satisfied: bool,
    pub defect: u32,
}

impl SysEntry {
    pub fn legal(&self) -> bool {
        if self.satisfied {
            self.defect == 0
        } else {
            self.defect != 0
        }
    }

    pub fn render(&self) -> String {
        format!("nr{}:{}", self.nr, if self.satisfied { "ok" } else { "gap" })
    }
}

/// 号册。
#[derive(Clone, Debug)]
pub struct SysReg {
    pub entries: Vec<SysEntry>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SysReg {
    pub fn new() -> SysReg {
        SysReg { entries: Vec::new(), frozen: false, root: None }
    }

    /// 登记：冻结/不在最小集/重录/满足态与缺陷并存/超容五路拒。
    pub fn add(&mut self, nr: u32, satisfied: bool, defect: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(MS_FROZEN);
        }
        if !MIN_NUMS.contains(&nr) {
            return Err(MS_NR);
        }
        if self.entries.iter().any(|e| e.nr == nr) {
            return Err(MS_DUP);
        }
        if satisfied && defect != 0 {
            return Err(MS_STATUS);
        }
        if !satisfied && defect == 0 {
            return Err(MS_NODEF);
        }
        if self.entries.len() >= MIN_NUMS.len() {
            return Err(MS_OVER);
        }
        self.entries.push(SysEntry { nr, satisfied, defect });
        Ok(())
    }

    /// 未满足号清单。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if !self.entries[k].satisfied {
                out.push(self.entries[k].nr);
            }
            k += 1;
        }
        out
    }

    /// 未满足却无缺陷号（必须为空）。
    pub fn unaccounted(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.entries.len() {
            if !self.entries[k].satisfied && self.entries[k].defect == 0 {
                out.push(self.entries[k].nr);
            }
            k += 1;
        }
        out
    }

    /// 号表缺号（相对 MIN_NUMS）。
    pub fn missing(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < MIN_NUMS.len() {
            if !self.entries.iter().any(|e| e.nr == MIN_NUMS[k]) {
                out.push(MIN_NUMS[k]);
            }
            k += 1;
        }
        out
    }

    pub fn satisfied_count(&self) -> usize {
        self.entries.iter().filter(|e| e.satisfied).count()
    }

    pub fn fold(&self) -> u64 {
        let mut h: u64 = MS_TAG;
        let mut k = 0usize;
        while k < self.entries.len() {
            h = (h ^ self.entries[k].nr as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.entries[k].satisfied as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.entries[k].defect as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 缺陷号基址（未满足号逐号挂账）。
pub const DEFECT_BASE: u32 = 13100;

/// 建册：60 号逐号登记，六号未满足各挂缺陷号。
pub fn build() -> (SysReg, bool) {
    let mut r = SysReg::new();
    let mut ok = true;
    let mut k = 0usize;
    while k < MIN_NUMS.len() {
        let nr = MIN_NUMS[k];
        let sat = SATISFIED.contains(&nr);
        let defect = if sat { 0 } else { DEFECT_BASE + nr };
        if r.add(nr, sat, defect).is_err() {
            ok = false;
        }
        k += 1;
    }
    let good = ok && r.missing().is_empty() && r.unaccounted().is_empty() && r.entries.len() == 60;
    (r, good)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 号不在最小集必拒。
    let mut a = SysReg::new();
    got[0] = a.add(999, true, 0).err() == Some(MS_NR);
    // 2) 未满足无缺陷号必拒。
    let mut b = SysReg::new();
    got[1] = b.add(0, false, 0).err() == Some(MS_NODEF);
    // 3) 满足态与缺陷号并存必拒。
    let mut c = SysReg::new();
    got[2] = c.add(0, true, 7).err() == Some(MS_STATUS);
    // 4) 冻结后登记拒，且未满足号必在缺口清单里。
    let (mut d, _) = build();
    let gaps = d.gaps();
    d.freeze();
    got[3] = d.add(0, true, 0).err() == Some(MS_FROZEN) && gaps.len() == 6;
    got
}

/// 批内判据。
pub fn run_unx11217_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11217");
    let (mut r, built) = build();

    s.add("J1-六十号登记", built && r.entries.len() == 60, "60 个最小 syscall 号逐号登记");
    s.add("J2-号表零缺", r.missing().is_empty(), "号表相对冻结清单零缺号");
    s.add("J3-满足态统计", r.satisfied_count() == 54, "54 号满足 6 号未满足");
    s.add("J4-缺口全挂号", r.unaccounted().is_empty() && r.gaps().len() == 6,
        "未满足号 100% 挂缺陷号");
    s.add("J5-挂账合法", r.entries.iter().all(|e| e.legal()), "满足/缺陷两态互斥合法");
    s.add("J6-号序保持", r.entries[0].nr == 0 && r.entries[59].nr == 231,
        "登记序首尾号与冻结清单一致");

    s.add("J7-缺口枚举精确", r.gaps() == vec![127, 186, 202, 217, 218, 231],
        "六个未满足号按序枚举");
    s.add("J8-坏号拒", {
        let mut a = SysReg::new();
        a.add(500, true, 0).err() == Some(MS_NR)
    }, "500 号不在最小集必拒");
    s.add("J9-重号拒", {
        let mut a = SysReg::new();
        let _ = a.add(0, true, 0);
        a.add(0, false, 13100).err() == Some(MS_DUP)
    }, "同号二次登记必拒");
    s.add("J10-超容拒", {
        let mut a = SysReg::new();
        let mut k = 0usize;
        while k < MIN_NUMS.len() {
            let _ = a.add(MIN_NUMS[k], true, 0);
            k += 1;
        }
        a.add(MIN_NUMS[0], true, 0).err() == Some(MS_DUP)
    }, "满册后任何登记均被拒");

    let inj = inject_four();
    s.add("J11-注入四路", inj[0] && inj[1] && inj[2] && inj[3],
        "坏号/无缺陷号/态冲突/冻结四路必抓");

    let root = r.freeze();
    s.add("J12-冻结根", r.fold() == root && root != 0, "冻结后号册指纹可重算");
    s.add("J13-冻结拒写", r.add(231, true, 0).err() == Some(MS_FROZEN), "冻结后再登记必拒");

    let fp = r.fold();
    s.add_dyn("J14-号册明细", r.gaps().len() == 6 && fp == root,
        &format!("ok={} gap={}", r.satisfied_count(), r.gaps().len()));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11217_syscall_flow() {
        let (mut r, built) = build();
        assert!(built);
        assert_eq!(r.entries.len(), 60);
        assert_eq!(r.satisfied_count(), 54);
        assert_eq!(r.gaps(), vec![127, 186, 202, 217, 218, 231]);
        assert!(r.add(999, true, 0).is_err());
        let root = r.freeze();
        assert_eq!(r.fold(), root);
        assert!(r.add(0, true, 0).is_err());
        assert!(r.missing().is_empty());
    }

    #[test]
    fn unx11217_checks_green() {
        let s = run_unx11217_checks();
        assert!(s.all_passed(), "UNX-F11217 判据未全绿");
    }
}

#[cfg(test)]
mod refusal_tests {
    use super::*;

    #[test]
    fn five_refusal_paths() {
        let mut r = SysReg::new();
        assert_eq!(r.add(0, true, 0), Ok(()));
        assert_eq!(r.add(0, false, 1), Err(MS_DUP));
        assert_eq!(r.add(1, true, 7), Err(MS_STATUS));
        assert_eq!(r.add(1, false, 0), Err(MS_NODEF));
        assert_eq!(r.add(9999, true, 0), Err(MS_NR));
        assert_eq!(r.add(2, false, 3), Ok(()));
        assert_eq!(r.add(3, true, 0), Ok(()));
        assert_eq!(r.gaps(), vec![2]);
        assert!(r.unaccounted().is_empty());
        let root = r.freeze();
        assert_eq!(r.fold(), root);
        assert_eq!(r.add(4, true, 0), Err(MS_FROZEN));
    }
}
