//! UNX-F11113 域 700 条里程碑账（F11120 锚定）（0x1f593 段）
//!
//! 锚点：UNX-F11113-J1 里程碑账三数对平（10/10），锚定断言成立（10/10）。
//! 域：CoRun·UNX-C4-B36（里程碑）。
//!
//! 结构：里程碑账（MilestoneBook：三数落账 → 每批行数整除校验 → F1120
//! 锚定 → 对平）+ 里程碑三数（Milestone）+ 冻结根 + 注入核（零值/错误锚点/
//! 三数不符/除不尽四路必抓）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x1f593 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f593;
pub const MS_ZERO: u32 = SEG_BASE + 0x01; // 三数含零
pub const MS_ANCHOR: u32 = SEG_BASE + 0x02; // 锚点号不符
pub const MS_MISMATCH: u32 = SEG_BASE + 0x03; // 三数与期望不符
pub const MS_DIV: u32 = SEG_BASE + 0x04; // 每批行数除不尽
pub const MS_UNSET: u32 = SEG_BASE + 0x05; // 未落账先锚定
pub const MS_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账
pub const MS_RED: u32 = SEG_BASE + 0x08; // 里程碑未闭合

/// 里程碑期望三数：700 判据 / 36 批 / 216000 行。
pub const EXPECT_CRITERIA: u32 = 700;
pub const EXPECT_BATCHES: u32 = 36;
pub const EXPECT_LINES: u32 = 216_000;
/// 每批行数（240000/40 = 6000 同口径）。
pub const LINES_PER_BATCH: u32 = 6_000;
/// 锚点卡号（F11120）。
pub const ANCHOR_ID: u32 = 1120;
/// 里程碑标（fold 种子）。
pub const MS_TAG: u64 = 0x1f5_9393;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 里程碑三数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Milestone {
    pub criteria: u32,
    pub batches: u32,
    pub lines: u32,
}

impl Milestone {
    pub fn new(criteria: u32, batches: u32, lines: u32) -> Milestone {
        Milestone { criteria, batches, lines }
    }

    pub fn zero(&self) -> bool {
        self.criteria == 0 || self.batches == 0 || self.lines == 0
    }

    /// 每批行数：整除才有意义。
    pub fn per_batch(&self) -> Option<u32> {
        if self.batches == 0 || self.lines % self.batches != 0 {
            None
        } else {
            Some(self.lines / self.batches)
        }
    }

    pub fn render(&self) -> alloc::string::String {
        format!("c{}|b{}|l{}", self.criteria, self.batches, self.lines)
    }
}

/// 里程碑账：三数 + 锚点 + 冻结根。
#[derive(Clone, Debug)]
pub struct MilestoneBook {
    pub m: Option<Milestone>,
    pub anchor: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl MilestoneBook {
    pub fn new() -> MilestoneBook {
        MilestoneBook { m: None, anchor: 0, frozen: false, root: None }
    }

    /// 三数落账：零值拒、除不尽拒、重复落账拒。
    pub fn settle(&mut self, criteria: u32, batches: u32, lines: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(MS_FROZEN);
        }
        if self.m.is_some() {
            return Err(MS_MISMATCH);
        }
        let m = Milestone::new(criteria, batches, lines);
        if m.zero() {
            return Err(MS_ZERO);
        }
        if m.per_batch().is_none() {
            return Err(MS_DIV);
        }
        self.m = Some(m);
        Ok(())
    }

    /// 锚定：未落账拒、锚点卡号不符拒。
    pub fn anchor_with(&mut self, id: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(MS_FROZEN);
        }
        if self.m.is_none() {
            return Err(MS_UNSET);
        }
        if id != ANCHOR_ID {
            return Err(MS_ANCHOR);
        }
        self.anchor = id;
        Ok(())
    }

    /// 三数对平：与期望三数逐项相等。
    pub fn three_way(&self) -> bool {
        match self.m {
            Some(m) => {
                m.criteria == EXPECT_CRITERIA
                    && m.batches == EXPECT_BATCHES
                    && m.lines == EXPECT_LINES
            }
            None => false,
        }
    }

    /// 每批行数对平。
    pub fn per_batch_ok(&self) -> bool {
        match self.m {
            Some(m) => m.per_batch() == Some(LINES_PER_BATCH),
            None => false,
        }
    }

    /// 差异点名：返回与期望不符的项编码（1 判据 / 2 批数 / 3 行数）。
    pub fn diffs(&self) -> alloc::vec::Vec<u32> {
        let mut v: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
        match self.m {
            Some(m) => {
                if m.criteria != EXPECT_CRITERIA {
                    v.push(1);
                }
                if m.batches != EXPECT_BATCHES {
                    v.push(2);
                }
                if m.lines != EXPECT_LINES {
                    v.push(3);
                }
            }
            None => {
                v.push(1);
                v.push(2);
                v.push(3);
            }
        }
        v
    }

    /// 闭合：三数对平、每批整除、锚定成立。
    pub fn closed(&self) -> bool {
        self.three_way() && self.per_batch_ok() && self.anchor == ANCHOR_ID
    }

    /// 冻结并落根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 账指纹：三数 + 锚点链式入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = MS_TAG;
        if let Some(m) = self.m {
            h = (h ^ m.criteria as u64).wrapping_mul(FP_STEP);
            h = (h ^ m.batches as u64).wrapping_mul(FP_STEP);
            h = (h ^ m.lines as u64).wrapping_mul(FP_STEP);
        }
        if self.anchor != 0 {
            h = (h ^ self.anchor as u64).wrapping_mul(FP_STEP);
        }
        h
    }
}

/// 标准里程碑账：三数落账并锚定 F11120。
pub fn build_book() -> (MilestoneBook, bool) {
    let mut b = MilestoneBook::new();
    let mut ok = true;
    if b.settle(EXPECT_CRITERIA, EXPECT_BATCHES, EXPECT_LINES).is_err() {
        ok = false;
    }
    if b.anchor_with(ANCHOR_ID).is_err() {
        ok = false;
    }
    (b, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 零值必拒
    let mut a = MilestoneBook::new();
    got[0] = a.settle(0, 36, 216_000).err() == Some(MS_ZERO)
        && a.settle(700, 0, 216_000).err() == Some(MS_ZERO)
        && a.m.is_none();
    // 2) 错误锚点必拒
    let mut b = MilestoneBook::new();
    let _ = b.settle(EXPECT_CRITERIA, EXPECT_BATCHES, EXPECT_LINES);
    got[1] = b.anchor_with(1121).err() == Some(MS_ANCHOR) && b.anchor == 0;
    // 3) 三数不符必点名（批数 35）
    let mut c = MilestoneBook::new();
    let _ = c.settle(EXPECT_CRITERIA, 35, EXPECT_LINES);
    let _ = c.anchor_with(ANCHOR_ID);
    got[2] = !c.three_way() && c.diffs().contains(&2) && !c.closed();
    // 4) 除不尽必拒（216001 行 / 36 批）
    let mut d = MilestoneBook::new();
    got[3] = d.settle(700, 36, 216_001).err() == Some(MS_DIV);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11113_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11113");

    let (mut book, built) = build_book();
    // J1 里程碑账三数对平
    s.add(
        "J1-三数对平",
        built && book.three_way() && book.diffs().is_empty(),
        "700 判据 / 36 批 / 216000 行逐项对平",
    );

    // J2 锚定断言成立
    s.add(
        "J2-锚定成立",
        book.anchor == ANCHOR_ID && book.closed(),
        "锚点 F11120 已落且账闭合",
    );

    // J3 每批行数整除 6000
    s.add(
        "J3-每批整除",
        book.per_batch_ok() && book.m.map(|m| m.per_batch()) == Some(Some(LINES_PER_BATCH)),
        "216000 / 36 = 6000 每批",
    );

    // J4 未落账先锚定拒
    let j4 = {
        let mut c = MilestoneBook::new();
        c.anchor_with(ANCHOR_ID).err() == Some(MS_UNSET) && c.anchor == 0
    };
    s.add("J4-未落账拒锚", j4, "三数未落账不得锚定");

    // J5 错误锚点拒
    let j5 = {
        let mut c = MilestoneBook::new();
        let _ = c.settle(700, 36, 216_000);
        c.anchor_with(999).err() == Some(MS_ANCHOR)
    };
    s.add("J5-错锚拒", j5, "非 F11120 锚点号必拒");

    // J6 零值拒
    let j6 = {
        let mut c = MilestoneBook::new();
        c.settle(0, 0, 0).err() == Some(MS_ZERO) && c.m.is_none()
    };
    s.add("J6-零值拒", j6, "三数含零不得落账");

    // J7 除不尽拒
    let j7 = {
        let mut c = MilestoneBook::new();
        c.settle(700, 7, 216_000).err() == Some(MS_DIV) && c.m.is_none()
    };
    s.add("J7-除不尽拒", j7, "行数不能整除批数必拒");

    // J8 三数不符点名
    let j8 = {
        let mut c = MilestoneBook::new();
        let _ = c.settle(701, 36, 216_000);
        let _ = c.anchor_with(ANCHOR_ID);
        !c.three_way() && c.diffs().len() == 1 && c.diffs().contains(&1)
    };
    s.add("J8-差异点名", j8, "判据数偏移只点第 1 项");

    // J9 注入检出：零值 + 错锚
    let inj = inject_four();
    s.add("J9-零值错锚检出", inj[0] && inj[1], "零值与错误锚点两路必抓");

    // J10 注入检出：三数不符 + 除不尽
    s.add("J10-不符整除检出", inj[2] && inj[3], "三数不符与除不尽两路必抓");

    // J11 重复落账拒
    let j11 = {
        let mut c = MilestoneBook::new();
        let _ = c.settle(700, 36, 216_000);
        c.settle(700, 36, 216_000).err() == Some(MS_MISMATCH)
    };
    s.add("J11-重复落账拒", j11, "三数只能落一次");

    // J12 冻结拒写
    let root = book.seal();
    let j12 = book.settle(1, 1, 1).err() == Some(MS_FROZEN)
        && book.anchor_with(ANCHOR_ID).err() == Some(MS_FROZEN);
    s.add("J12-冻结拒写", j12, "封存后落账与锚定两路必拒");

    // J13 冻结根稳定
    s.add(
        "J13-冻结根稳定",
        book.root == Some(root) && root == book.fold() && root != 0,
        "里程碑账根可重算零漂移",
    );

    // J14 渲染面
    let j14 = book.m.map(|m| m.render()).unwrap_or_default().contains("c700")
        && book.m.map(|m| m.render()).unwrap_or_default().contains("b36");
    s.add("J14-三数渲染", j14, "里程碑渲染含判据数与批数");

    // J15 动态明细
    s.add_dyn(
        "J15-里程碑明细",
        book.closed() && book.fold() == root,
        &format!(
            "c={} b={} l={} anchor={} root={:#x}",
            EXPECT_CRITERIA, EXPECT_BATCHES, EXPECT_LINES, book.anchor, root
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11113_milestone_flow() {
        let (mut b, built) = build_book();
        assert!(built);
        assert!(b.three_way());
        assert!(b.per_batch_ok());
        assert!(b.closed());
        assert!(b.seal() != 0);
        assert!(b.anchor_with(ANCHOR_ID).err() == Some(MS_FROZEN));
        let mut c = MilestoneBook::new();
        assert!(c.settle(700, 36, 216_001).err() == Some(MS_DIV));
    }

    #[test]
    fn unx11113_checks_green() {
        let s = run_unx11113_checks();
        assert!(s.all_passed(), "UNX-F11113 判据未全绿");
    }
}
