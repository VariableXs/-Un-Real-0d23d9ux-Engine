//! UNX-F10997 全域回归套件固化（B16–B30 判据清单）（0x1dc20 段）
//!
//! 锚点：UNX-F10997-J1 全域清单固化可复跑——B16~B30 十五段判据清单
//! 逐段入册、指纹冻结、重放一致；J2 覆盖对账零缺口——清单登记的段号
//! 与判据落点全覆盖、无漏段无重号。
//! 域：CoRun·UNX-C4（多进程压力域）。
//!
//! 结构：回归清单（段号 + 判据计数 + 指纹链）+ 固化器（逐段登记→链式
//! fold→冻结根）+ 重放核（冻结后重放逐段对拍）+ 注入核（重号/漏段/
//! 越界段/封账写入四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1dc20 独占 0x20）。
pub const SEG_BASE: u32 = 0x1dc20;
pub const RG_DUP: u32 = SEG_BASE + 0x01; // 段重号
pub const RG_GAP: u32 = SEG_BASE + 0x02; // 段缺口（非连续）
pub const RG_RANGE: u32 = SEG_BASE + 0x03; // 越界段（B16~B30 外）
pub const RG_FROZEN: u32 = SEG_BASE + 0x04; // 封账后写清单
pub const RG_NOENT: u32 = SEG_BASE + 0x05; // 段不在册
pub const RG_MISMATCH: u32 = SEG_BASE + 0x06; // 重放对拍不一致

/// 回归段界：B16~B30。
pub const B_LO: u32 = 16;
pub const B_HI: u32 = 30;
/// 回归域标。
pub const RG_TAG: u64 = 0x1dc2_0297;

/// 段判据项：段号 + 判据计数 + 全绿旗。
#[derive(Clone, Debug, PartialEq)]
pub struct SegEntry {
    pub seg: u32,
    pub checks: u32,
    pub green: bool,
}

/// 回归清单册。
pub struct RegenBook {
    pub entries: Vec<SegEntry>,
    pub sealed: bool,
    pub root: Option<u64>,
}

impl RegenBook {
    pub fn new() -> RegenBook {
        RegenBook { entries: Vec::new(), sealed: false, root: None }
    }

    fn find(&self, seg: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.entries.len() {
            if self.entries[k].seg == seg {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记段判据：封账/重号/越界必拒。
    pub fn enroll(&mut self, seg: u32, checks: u32, green: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(RG_FROZEN);
        }
        if seg < B_LO || seg > B_HI {
            return Err(RG_RANGE);
        }
        if self.find(seg).is_some() {
            return Err(RG_DUP);
        }
        self.entries.push(SegEntry { seg, checks, green });
        Ok(())
    }

    /// 覆盖对账：段号严格连续无缺口。
    pub fn no_gaps(&self) -> bool {
        if self.entries.len() != (B_HI - B_LO + 1) as usize {
            return false;
        }
        let mut k = 1usize;
        while k < self.entries.len() {
            if self.entries[k].seg != self.entries[k - 1].seg + 1 {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 判据落点计数总和。
    pub fn total_checks(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < self.entries.len() {
            n = n.wrapping_add(self.entries[k].checks);
            k += 1;
        }
        n
    }

    /// 全绿裁断。
    pub fn all_green(&self) -> bool {
        let mut k = 0usize;
        while k < self.entries.len() {
            if !self.entries[k].green {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 链式 fold 指纹。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = RG_TAG;
        let mut k = 0usize;
        while k < self.entries.len() {
            let e = &self.entries[k];
            h = (h ^ e.seg as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.checks as u64).wrapping_mul(0x100000001b3);
            h = (h ^ e.green as u64).wrapping_mul(0x100000001b3);
            k += 1;
        }
        h
    }

    /// 冻结：翻封账位并落根。
    pub fn freeze(&mut self) -> u64 {
        self.sealed = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 重放对拍：冻结后重放清单必须与在册逐段一致。
    pub fn replay(&self, replayed: &[SegEntry]) -> Result<bool, u32> {
        if !self.sealed {
            return Err(RG_NOENT);
        }
        if replayed.len() != self.entries.len() {
            return Err(RG_MISMATCH);
        }
        let mut k = 0usize;
        while k < replayed.len() {
            if replayed[k] != self.entries[k] {
                return Err(RG_MISMATCH);
            }
            k += 1;
        }
        Ok(true)
    }
}

/// 确定性样本：段号 seg 的判据计数（贴域设计：低位扰动）。
pub fn rg_checks_of(seg: u32) -> u32 {
    seg.wrapping_mul(5).wrapping_add(8)
}

/// 构建全域清单（B16~B30 全段登记，判据计数确定性样本）。
pub fn build_full() -> (RegenBook, bool) {
    let mut b = RegenBook::new();
    let mut seg = B_LO;
    let mut ok = true;
    while seg <= B_HI {
        if b.enroll(seg, rg_checks_of(seg), true).is_err() {
            ok = false;
        }
        seg = seg.wrapping_add(1);
    }
    let b_ok = ok && b.no_gaps() && b.all_green();
    (b, b_ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 越界段必抓
    let mut b = RegenBook::new();
    got[0] = b.enroll(B_LO - 1, 1, true).err() == Some(RG_RANGE)
        && b.enroll(B_HI + 1, 1, true).err() == Some(RG_RANGE);
    // 2) 重号必抓
    let _ = b.enroll(B_LO, 3, true);
    got[1] = b.enroll(B_LO, 4, true).err() == Some(RG_DUP);
    // 3) 段缺口必抓（跳段登记）
    let mut c = RegenBook::new();
    let _ = c.enroll(B_LO, 2, true);
    let _ = c.enroll(B_LO + 2, 2, true);
    got[2] = !c.no_gaps();
    // 4) 封账写入必抓
    c.freeze();
    got[3] = c.enroll(B_HI, 1, true).err() == Some(RG_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10997_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10997");

    // J1 全域清单固化可复跑
    let (mut b, ok) = build_full();
    let root = b.freeze();
    let replay = b.entries.clone();
    let inj = inject_four();
    let j1 = ok && b.root == Some(root) && b.replay(&replay) == Ok(true) && inj[0];
    s.add("J1-清单固化复跑", j1, "B16~B30 全段入册指纹冻结且重放一致");

    // J2 覆盖对账零缺口
    let j2 = b.no_gaps() && b.entries.len() == (B_HI - B_LO + 1) as usize
        && b.total_checks() == {
            let mut t = 0u32;
            let mut seg = B_LO;
            while seg <= B_HI {
                t = t.wrapping_add(rg_checks_of(seg));
                seg = seg.wrapping_add(1);
            }
            t
        } && inj[2];
    s.add("J2-覆盖零缺口", j2, "段号连续无缺口且判据落点计数与样本一致");

    // J3 越界段必抓
    s.add("J3-越界段抓", inj[0], "B 界外段登记必拒");

    // J4 重号必抓
    s.add("J4-重号抓", inj[1], "段重号登记必拒");

    // J5 段缺口必抓
    s.add("J5-缺口抓", inj[2], "跳段清单必被覆盖对账抓出");

    // J6 封账写入必抓
    s.add("J6-封账拒写", inj[3], "冻结后清单登记必拒");

    // J7 重放对拍不一致必抓（篡改副本）
    let mut tampered = b.entries.clone();
    tampered[0].checks = tampered[0].checks.wrapping_add(1);
    let j7 = b.replay(&tampered).err() == Some(RG_MISMATCH);
    s.add("J7-篡改对拍抓", j7, "重放清单与在册不一致必报 MISMATCH");

    // J8 非绿段被抓出
    let mut c = RegenBook::new();
    let _ = c.enroll(B_LO, 3, true);
    let _ = c.enroll(B_LO + 1, 3, false);
    let j8 = !c.all_green() && !c.no_gaps();
    s.add("J8-非绿抓", j8, "非绿段使全绿裁断为假且缺口对账为假");

    // J9 未知段重放必拒（未冻结册）
    let d = RegenBook::new();
    let empty: Vec<SegEntry> = Vec::new();
    let j9 = d.replay(&empty).err() == Some(RG_NOENT);
    s.add("J9-未冻拒放", j9, "未冻结清单重放必拒");

    // J10 fold 指纹稳定性与冻结根一致
    let fp0 = b.fold();
    let j10 = fp0 == root && b.fold() == root && root != 0;
    s.add("J10-指纹稳定", j10, "冻结根与 fold 一致且二次 fold 零漂移");

    // J11 判据计数样本贴域：段号与计数单调可复核
    let mut mono = true;
    let mut seg = B_LO;
    let mut prev = 0u32;
    while seg <= B_HI {
        let cur = rg_checks_of(seg);
        if seg > B_LO && cur <= prev {
            mono = false;
        }
        prev = cur;
        seg = seg.wrapping_add(1);
    }
    s.add("J11-计数单调", mono, "段判据计数随段号严格递增可复核");

    // J12 动态明细：判据总数与段数落账
    s.add_dyn("J12-总量明细", b.total_checks() > 0 && b.entries.len() == 15,
        &format!("segs={} checks={}", b.entries.len(), b.total_checks()));

    // J13 动态明细：冻结根十六进制落账
    s.add_dyn("J13-冻结根", root != 0, &format!("root={:#x}", root));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10997_regen_flow() {
        let mut b = RegenBook::new();
        assert!(b.enroll(B_LO - 1, 1, true).err() == Some(RG_RANGE));
        assert!(b.enroll(B_LO, 3, true).is_ok());
        assert!(b.enroll(B_LO, 3, true).err() == Some(RG_DUP));
        assert!(b.enroll(B_LO + 1, 4, true).is_ok());
        assert!(!b.no_gaps());
        assert!(b.all_green());
        let root = b.freeze();
        assert!(b.root == Some(root));
        assert!(b.replay(&b.entries.clone()) == Ok(true));
        assert!(b.enroll(B_HI, 1, true).err() == Some(RG_FROZEN));
    }

    #[test]
    fn unx10997_checks_green() {
        let s = run_unx10997_checks();
        assert!(s.all_passed(), "UNX-F10997 判据未全绿");
    }
}
