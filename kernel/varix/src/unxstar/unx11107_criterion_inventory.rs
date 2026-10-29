//! UNX-F11107 判据库总盘点（域 700 判据全量在册核验）（0x1f587 段）
//!
//! 锚点：UNX-F11107-J1 判据全量盘点对平（10/10），编号连续零断档（10/10）。
//! 域：CoRun·UNX-C4-B36（判据库）。
//!
//! 结构：判据总册（CriterionRoll：1..700 逐号登记 → 盘点点数 → 连续性
//! 判定 → 断档点名 → 申报对平）+ 冻结根 + 注入核（重号/越界/断档/零号
//! 四路必抓）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f587 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f587;
pub const CR_ZERO: u32 = SEG_BASE + 0x01; // 零号判据
pub const CR_DUP: u32 = SEG_BASE + 0x02; // 判据重号
pub const CR_RANGE: u32 = SEG_BASE + 0x03; // 判据号越出 700
pub const CR_GAP: u32 = SEG_BASE + 0x04; // 编号断档
pub const CR_SHORT: u32 = SEG_BASE + 0x05; // 盘点条数不足
pub const CR_UNBAL: u32 = SEG_BASE + 0x06; // 申报与实点不平
pub const CR_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const CR_OVER: u32 = SEG_BASE + 0x08; // 超定容

/// 域判据总数（700）。
pub const EXPECT: u32 = 700;
/// 总册定容（700 + 余量）。
pub const CAP: usize = 704;
/// 判据库标（fold 种子）。
pub const CR_TAG: u64 = 0x1f5_8787;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 判据总册：编号表 + 申报数 + 冻结根。
#[derive(Clone, Debug)]
pub struct CriterionRoll {
    pub nums: Vec<u32>,
    pub declared: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CriterionRoll {
    pub fn new() -> CriterionRoll {
        CriterionRoll { nums: Vec::new(), declared: 0, frozen: false, root: None }
    }

    /// 逐号登记：零号/重号/越界/超容皆拒，登记后升序定序。
    pub fn enroll(&mut self, no: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CR_FROZEN);
        }
        if no == 0 {
            return Err(CR_ZERO);
        }
        if no > EXPECT {
            return Err(CR_RANGE);
        }
        if self.nums.contains(&no) {
            return Err(CR_DUP);
        }
        if self.nums.len() >= CAP {
            return Err(CR_OVER);
        }
        self.nums.push(no);
        self.nums.sort_unstable();
        Ok(())
    }

    /// 盘点条数。
    pub fn count(&self) -> usize {
        self.nums.len()
    }

    /// 编号连续：恰为 1..=EXPECT 且逐号递增 1。
    pub fn continuous(&self) -> bool {
        if self.nums.len() != EXPECT as usize {
            return false;
        }
        let mut k = 0usize;
        while k < self.nums.len() {
            let want = (k as u32).wrapping_add(1);
            if self.nums[k] != want {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 断档点名（最多列 16 个，避免诊断文本膨胀）。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut no = 1u32;
        while no <= EXPECT {
            if !self.nums.contains(&no) {
                if g.len() < 16 {
                    g.push(no);
                }
            }
            no += 1;
        }
        g
    }

    /// 断档总数（不受点名上限影响）。
    pub fn gap_count(&self) -> u32 {
        let mut n = 0u32;
        let mut no = 1u32;
        while no <= EXPECT {
            if !self.nums.contains(&no) {
                n = n.wrapping_add(1);
            }
            no += 1;
        }
        n
    }

    /// 盘点申报：申报数须等于实点且等于域总数。
    pub fn declare(&mut self, total: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CR_FROZEN);
        }
        if total != self.count() as u32 || total != EXPECT {
            return Err(CR_UNBAL);
        }
        self.declared = total;
        Ok(())
    }

    /// 总盘闭合：700 条在册、连续零断档、申报对平。
    pub fn closed(&self) -> bool {
        self.count() == EXPECT as usize
            && self.continuous()
            && self.gap_count() == 0
            && self.declared == EXPECT
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 总册指纹：编号序 + 申报数链式入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = CR_TAG;
        let mut k = 0usize;
        while k < self.nums.len() {
            h = (h ^ self.nums[k] as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        if self.declared != 0 {
            h = (h ^ self.declared as u64).wrapping_mul(FP_STEP);
        }
        h
    }
}

/// 判据号短名（确定性）。
pub fn criterion_tag(no: u32) -> String {
    format!("J{}", no)
}

/// 标准总册：1..=700 逐号登记并申报 700。
pub fn build_roll() -> (CriterionRoll, bool) {
    let mut r = CriterionRoll::new();
    let mut ok = true;
    let mut no = 1u32;
    while no <= EXPECT {
        if r.enroll(no).is_err() {
            ok = false;
        }
        no += 1;
    }
    if r.declare(EXPECT).is_err() {
        ok = false;
    }
    (r, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 重号必拒
    let mut a = CriterionRoll::new();
    let _ = a.enroll(1);
    got[0] = a.enroll(1).err() == Some(CR_DUP);
    // 2) 越界必拒
    let mut b = CriterionRoll::new();
    got[1] = b.enroll(EXPECT + 1).err() == Some(CR_RANGE);
    // 3) 断档必检出（跳过 350 号）
    let mut c = CriterionRoll::new();
    let mut no = 1u32;
    while no <= EXPECT {
        if no != 350 {
            let _ = c.enroll(no);
        }
        no += 1;
    }
    let _ = c.declare(EXPECT - 1);
    got[2] = !c.continuous() && c.gap_count() == 1 && c.gaps().contains(&350);
    // 4) 零号必拒
    let mut d = CriterionRoll::new();
    got[3] = d.enroll(0).err() == Some(CR_ZERO) && d.count() == 0;
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11107_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11107");

    let (mut roll, built) = build_roll();
    // J1 判据全量盘点对平
    s.add(
        "J1-全量盘点对平",
        built && roll.count() == EXPECT as usize && roll.declared == EXPECT,
        "700 条判据逐号在册且申报对平",
    );

    // J2 编号连续零断档
    s.add(
        "J2-连续零断档",
        roll.continuous() && roll.gap_count() == 0 && roll.gaps().is_empty(),
        "编号 1..700 逐号连续",
    );

    // J3 盘点首末号
    let j3 = roll.nums.first() == Some(&1) && roll.nums.last() == Some(&EXPECT);
    s.add("J3-首末号定位", j3, "总册首号为 1、末号为 700");

    // J4 重号必拒
    let j4 = {
        let mut c = CriterionRoll::new();
        let _ = c.enroll(5);
        c.enroll(5).err() == Some(CR_DUP) && c.count() == 1
    };
    s.add("J4-重号拒", j4, "同号二次登记必拒且点数不变");

    // J5 越界必拒
    let j5 = {
        let mut c = CriterionRoll::new();
        let a = c.enroll(EXPECT + 1).err() == Some(CR_RANGE);
        let b = c.enroll(EXPECT).is_ok();
        a && b && c.count() == 1
    };
    s.add("J5-越界拒", j5, "701 号拒、700 号收");

    // J6 零号必拒
    let j6 = {
        let mut c = CriterionRoll::new();
        c.enroll(0).err() == Some(CR_ZERO) && c.count() == 0
    };
    s.add("J6-零号拒", j6, "零号判据不得入册");

    // J7 断档点名
    let j7 = {
        let mut c = CriterionRoll::new();
        let mut no = 1u32;
        while no <= EXPECT {
            if no != 349 && no != 350 {
                let _ = c.enroll(no);
            }
            no += 1;
        }
        let _ = c.declare(EXPECT - 2);
        !c.closed() && c.gap_count() == 2 && c.gaps().contains(&349) && c.gaps().contains(&350)
    };
    s.add("J7-断档点名", j7, "缺两号时断档点名与计数一致");

    // J8 条数不足申报拒
    let j8 = {
        let mut c = CriterionRoll::new();
        let _ = c.enroll(1);
        c.declare(EXPECT).err() == Some(CR_UNBAL)
    };
    s.add("J8-申报不平拒", j8, "实点 1 条申报 700 必拒");

    // J9 注入检出：重号 + 越界
    let inj = inject_four();
    s.add("J9-重号越界检出", inj[0] && inj[1], "判据重号与越界登记两路必抓");

    // J10 注入检出：断档 + 零号
    s.add("J10-断档零号检出", inj[2] && inj[3], "断档判定与零号拒收两路必抓");

    // J11 指纹对编号敏感
    let j11 = {
        let (a, _) = build_roll();
        let mut b = CriterionRoll::new();
        let mut no = 1u32;
        while no <= EXPECT {
            if no != 700 {
                let _ = b.enroll(no);
            }
            no += 1;
        }
        let _ = b.enroll(699);
        a.fold() != b.fold()
    };
    s.add("J11-指纹敏感", j11, "少一号换一号后总册指纹必变");

    // J12 冻结拒写
    let root = roll.seal();
    let j12 = roll.enroll(1).err() == Some(CR_FROZEN)
        && roll.declare(EXPECT).err() == Some(CR_FROZEN);
    s.add("J12-冻结拒写", j12, "封存后登记与申报两路必拒");

    // J13 冻结根稳定
    s.add(
        "J13-冻结根稳定",
        roll.root == Some(root) && root == roll.fold() && root != 0,
        "总册冻结根可重算零漂移",
    );

    // J14 动态明细
    s.add_dyn(
        "J14-盘点明细",
        roll.closed() && roll.fold() == root,
        &format!(
            "count={} gaps={} declared={} root={:#x}",
            roll.count(),
            roll.gap_count(),
            roll.declared,
            root
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11107_inventory_flow() {
        let mut r = CriterionRoll::new();
        let mut no = 1u32;
        while no <= 20 {
            assert!(r.enroll(no).is_ok());
            no += 1;
        }
        assert!(r.enroll(5).err() == Some(CR_DUP));
        assert!(r.enroll(701).err() == Some(CR_RANGE));
        assert!(!r.continuous());
        assert!(r.declare(20).err() == Some(CR_UNBAL));
        assert!(r.seal() != 0);
        assert!(r.enroll(21).err() == Some(CR_FROZEN));
    }

    #[test]
    fn unx11107_checks_green() {
        let s = run_unx11107_checks();
        assert!(s.all_passed(), "UNX-F11107 判据未全绿");
    }
}
