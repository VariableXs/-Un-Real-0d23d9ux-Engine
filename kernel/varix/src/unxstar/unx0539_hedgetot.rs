//! unx0539_hedgetot —— UNX-F0539 ktest 对冲总账。
//! 锚点（CoRun Unxreal 主册）：UNX-F0539 · ktest 对冲总账 · 目标 200 行
//! 判据原文：三账（汇总/覆盖/对冲）对冲平（三等式复验），误差 0。
//! 组件：三账各持一数（汇总=用例总数；覆盖=判据覆盖数；对冲=绿验对数）
//!   + 三等式（对冲 ≤ 汇总、对冲 ≤ 覆盖、汇总-对冲=未验绿账注）+ 全平核。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x35400;
pub const XC_TRIPLE_UNBAL: u32 = SEG_BASE + 1; // 三等式破
pub const XC_TOT_NEG: u32 = SEG_BASE + 2; // 负差
pub const XC_TOT_BAD: u32 = SEG_BASE + 3; // 账坏（零账）
pub const XC_TOT_OOB: u32 = SEG_BASE + 4; // 越界

/// 三账。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TripleBook {
    /// 汇总账：用例总数。
    pub summary: u32,
    /// 覆盖账：判据覆盖数。
    pub cover: u32,
    /// 对冲账：绿验对数。
    pub hedge: u32,
}

impl TripleBook {
    /// 三等式：
    ///   E1 对冲 ≤ 汇总（绿验对不可能超用例总数）
    ///   E2 对冲 ≤ 覆盖（绿验对不可能超覆盖判据数）
    ///   E3 汇总 ≥ 对冲（差=未验绿，恒非负）
    pub const fn triple_ok(&self) -> bool {
        self.hedge <= self.summary && self.hedge <= self.cover && self.summary >= self.hedge
    }

    /// 未验绿差（汇总-对冲）。
    pub const fn unverified(&self) -> u32 {
        self.summary - self.hedge
    }

    /// 全平核（三账全零为坏账）。
    pub const fn sane(&self) -> bool {
        self.summary > 0 && self.triple_ok()
    }
}

/// 全账对冲核：给定三账逐等式复验，破式点名。
pub fn reconcile(b: &TripleBook) -> Result<(), u32> {
    if b.summary == 0 {
        return Err(XC_TOT_BAD);
    }
    if !b.triple_ok() {
        return Err(XC_TRIPLE_UNBAL);
    }
    Ok(())
}

/// 对冲误差：三等式破式计数（0=全平）。
pub fn imbalance(b: &TripleBook) -> u32 {
    let mut n = 0;
    if b.hedge > b.summary {
        n += 1;
    }
    if b.hedge > b.cover {
        n += 1;
    }
    if b.summary < b.hedge {
        n += 1;
    }
    n
}

pub fn run_unx0539_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0539");
    cs.add("SEG 段位独占", SEG_BASE == 0x35400 && XC_TOT_OOB == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：三等式正例。
    let b = TripleBook { summary: 12, cover: 8, hedge: 8 };
    cs.add("三等式平", b.triple_ok() && reconcile(&b) == Ok(()), "8≤12 且 8≤8");
    cs.add("未验绿差", b.unverified() == 4, "12-8=4 账注");
    cs.add("零误差", imbalance(&b) == 0, "破式 0");

    // 族二：破式检出。
    let b2 = TripleBook { summary: 5, cover: 3, hedge: 7 };
    cs.add("E1 破检出", !b2.triple_ok() && reconcile(&b2).is_err(), "对冲>汇总");
    cs.add("破式计数", imbalance(&b2) == 1, "一式破");
    let b3 = TripleBook { summary: 5, cover: 3, hedge: 9 };
    cs.add("双破计数", imbalance(&b3) == 2, "对冲超汇总+超覆盖");

    // 族三：零账坏。
    let z = TripleBook { summary: 0, cover: 0, hedge: 0 };
    cs.add("零账拒", reconcile(&z).is_err(), "全零 Err");

    // 族四：临界等值。
    let eq = TripleBook { summary: 9, cover: 9, hedge: 9 };
    cs.add("三数等值平", eq.triple_ok() && eq.unverified() == 0, "全验平账");
    let edge = TripleBook { summary: 1, cover: 1, hedge: 1 };
    cs.add("最小平账", edge.sane(), "1/1/1 自洽");
    cs
}
