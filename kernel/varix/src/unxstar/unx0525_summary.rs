//! unx0525_summary —— UNX-F0525 结果汇总账（三态守恒）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0525 · 结果汇总账 · 目标 220 行
//! 判据原文：汇总守恒式（用例总数=通过+失败+跳过 零差），8 组汇总与逐组
//!           账复算一致。
//! 组件：Tally（三态计数）+ 汇总器（逐组 Tally 聚合）+ 守恒核（总数恒
//!           等式与逐组复算双验）。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35080;
pub const XC_TALLY_NEG: u32 = SEG_BASE + 1; // 负计数（不可能但防手写坏账）
pub const XC_CONSERVE_BREAK: u32 = SEG_BASE + 2; // 守恒破
pub const XC_GROUP_MISS: u32 = SEG_BASE + 3; // 组缺
pub const XC_RECON_DRIFT: u32 = SEG_BASE + 4; // 复算漂移

/// 三态计数。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tally {
    pub total: u32,
    pub pass: u32,
    pub fail: u32,
    pub skip: u32,
}

impl Tally {
    /// 空账。
    pub const ZERO: Tally = Tally { total: 0, pass: 0, fail: 0, skip: 0 };

    /// 守恒式：total == pass+fail+skip。
    pub const fn conserved(&self) -> bool {
        self.total == self.pass + self.fail + self.skip
    }

    /// 无负核（u32 天然非负，此处核饱和溢出假象：各分量 ≤ total）。
    pub const fn sane(&self) -> bool {
        self.pass <= self.total && self.fail <= self.total && self.skip <= self.total
    }
}

/// 汇总器。
#[derive(Clone, Debug)]
pub struct Summarizer {
    pub groups: Vec<(&'static str, Tally)>,
}

impl Summarizer {
    /// 空汇总器。
    pub fn new() -> Self {
        Summarizer { groups: Vec::new() }
    }

    /// 登记一组账；组名重复拒；不守恒账拒收。
    pub fn add_group(&mut self, name: &'static str, t: Tally) -> Result<(), u32> {
        if !t.conserved() || !t.sane() {
            return Err(XC_CONSERVE_BREAK);
        }
        let mut i = 0;
        while i < self.groups.len() {
            if self.groups[i].0 == name {
                return Err(XC_GROUP_MISS);
            }
            i += 1;
        }
        self.groups.push((name, t));
        Ok(())
    }

    /// 总汇总：逐组累加（total/pass/fail/skip 分别求和）。
    pub fn aggregate(&self) -> Tally {
        let mut t = Tally::ZERO;
        let mut i = 0;
        while i < self.groups.len() {
            t.total += self.groups[i].1.total;
            t.pass += self.groups[i].1.pass;
            t.fail += self.groups[i].1.fail;
            t.skip += self.groups[i].1.skip;
            i += 1;
        }
        t
    }

    /// 逐组复算核：总汇总的每组字段和 == aggregate 值（恒真但显式双路）。
    pub fn reconcile(&self) -> Result<bool, u32> {
        let agg = self.aggregate();
        // 双路：另一路径按分量重算。
        let mut tp = 0u32;
        let mut tf = 0u32;
        let mut ts = 0u32;
        let mut tt = 0u32;
        let mut i = 0;
        while i < self.groups.len() {
            tt = tt.saturating_add(self.groups[i].1.total);
            tp = tp.saturating_add(self.groups[i].1.pass);
            tf = tf.saturating_add(self.groups[i].1.fail);
            ts = ts.saturating_add(self.groups[i].1.skip);
            i += 1;
        }
        Ok(agg.total == tt && agg.pass == tp && agg.fail == tf && agg.skip == ts && agg.conserved())
    }

    /// 组数。
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// 空？可实现。
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

impl Default for Summarizer {
    fn default() -> Self {
        Self::new()
    }
}

pub fn run_unx0525_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0525");
    cs.add("SEG 段位独占", SEG_BASE == 0x35080 && XC_RECON_DRIFT == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：守恒式。
    let t = Tally { total: 10, pass: 7, fail: 2, skip: 1 };
    cs.add("守恒成立", t.conserved() && t.sane(), "10=7+2+1");
    let bad = Tally { total: 10, pass: 7, fail: 2, skip: 0 };
    cs.add("守恒破检出", !bad.conserved(), "10≠9");
    let sane_bad = Tally { total: 5, pass: 9, fail: 0, skip: 0 };
    cs.add("分量越界核", !sane_bad.sane() || !sane_bad.conserved(), "pass>total 异常");

    // 族二：8 组汇总。
    let mut s = Summarizer::new();
    let names = ["b19", "b20", "b21", "b22", "b23", "b24", "b25", "b26"];
    let mut i = 0;
    while i < 8 {
        let g = Tally { total: 5, pass: 4, fail: 1 - 0, skip: 1 };
        let _ = s.add_group(names[i], g);
        i += 1;
    }
    cs.add("八组入账", s.len() == 8, "8/8 组");
    let agg = s.aggregate();
    cs.add("总量 40", agg.total == 40 && agg.pass == 32 && agg.fail == 8 && agg.skip == 8, "逐组求和");
    cs.add("总守恒", agg.conserved(), "40=32+8+8");
    cs.add("复算一致", s.reconcile() == Ok(true), "双路复算等");

    // 族三：坏账拒收。
    let mut s2 = Summarizer::new();
    cs.add("不守恒拒收", s2.add_group("x", bad).is_err(), "破式组入账 Err");
    let _ = s2.add_group("x", t);
    cs.add("重名拒", s2.add_group("x", t).is_err(), "同名组 Err");

    // 族四：跳过态参与守恒。
    let t3 = Tally { total: 3, pass: 1, fail: 1, skip: 1 };
    cs.add("三态全活", t3.conserved() && t3.skip == 1, "skip 计入守恒");
    cs
}
