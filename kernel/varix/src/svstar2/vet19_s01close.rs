//! VE-F3820 · S01 组收口双签（VE-T 域 · 无障碍渲染 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3820`
//!
//! **判据（锚点原文）**：九族、断链硬门、预算硬门、真源单源、双签、判据。
//!
//! **本条是什么**：S01 组收口——接口总账（九族签名表：六段架构/态采集/
//! 策略映射/注入点/替代渲染/协同断言/预算表/调试器/测试生成器）；
//! 映射断言；20 项自检；四联动核验（断链零/预算实测/策略公开/真源单源）；
//! 双签收口。断链检出 = P0 硬门阻断。
//!
//! 诊断码段：独占 0xCDxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xCDxx）
// ─────────────────────────────────────────────────────────────

/// F3820 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CDCode(pub u16);

impl CDCode {
    /// 映射缺——阻断。
    pub const MAP_MISS: CDCode = CDCode(0xCD01);
    /// 断链检出——P0 硬门阻断。
    pub const BREAK_GATE: CDCode = CDCode(0xCD02);
    /// 预算超未修——阻断。
    pub const BUDGET_GATE: CDCode = CDCode(0xCD03);
    /// 联动分歧——两轮上限。
    pub const LINK_DIFF: CDCode = CDCode(0xCD04);
    /// 九族缺族。
    pub const FAMILY_MISS: CDCode = CDCode(0xCD05);
    /// 双签缺。
    pub const SIGN_MISS: CDCode = CDCode(0xCD06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xCD01 => "映射缺失，收口阻断。",
            0xCD02 => "断链检出，P0 硬门阻断。",
            0xCD03 => "预算超支未修，硬门阻断。",
            0xCD04 => "联动分歧，两轮上限。",
            0xCD05 => "九族签名表缺族。",
            0xCD06 => "双签缺失，收口不生效。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 九族签名表
// ─────────────────────────────────────────────────────────────

/// S01 API 族（九族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum S01Family {
    /// 六段架构。
    Arch,
    /// 态采集。
    StateCollect,
    /// 策略映射。
    PolicyMap,
    /// 注入点。
    InjectPoint,
    /// 替代渲染。
    AltRender,
    /// 协同断言。
    CoAssert,
    /// 预算表。
    BudgetTable,
    /// 调试器。
    Debugger,
    /// 测试生成器。
    TestGen,
}

impl S01Family {
    /// 全集。
    pub const ALL: [S01Family; 9] = [
        S01Family::Arch,
        S01Family::StateCollect,
        S01Family::PolicyMap,
        S01Family::InjectPoint,
        S01Family::AltRender,
        S01Family::CoAssert,
        S01Family::BudgetTable,
        S01Family::Debugger,
        S01Family::TestGen,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            S01Family::Arch => 0,
            S01Family::StateCollect => 1,
            S01Family::PolicyMap => 2,
            S01Family::InjectPoint => 3,
            S01Family::AltRender => 4,
            S01Family::CoAssert => 5,
            S01Family::BudgetTable => 6,
            S01Family::Debugger => 7,
            S01Family::TestGen => 8,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            S01Family::Arch => "六段架构",
            S01Family::StateCollect => "态采集",
            S01Family::PolicyMap => "策略映射",
            S01Family::InjectPoint => "注入点",
            S01Family::AltRender => "替代渲染",
            S01Family::CoAssert => "协同断言",
            S01Family::BudgetTable => "预算表",
            S01Family::Debugger => "调试器",
            S01Family::TestGen => "测试生成器",
        }
    }
}

/// 签名总账。
#[derive(Clone, Debug)]
pub struct FamilyLedger {
    counts: [u16; 9],
    /// 映射断言通过。
    pub map_ok: bool,
}

impl FamilyLedger {
    /// 新建。
    pub const fn new() -> Self {
        FamilyLedger {
            counts: [0; 9],
            map_ok: false,
        }
    }

    /// 记签名。
    pub fn add(&mut self, f: S01Family) -> u16 {
        let i = f.ordinal();
        self.counts[i] = self.counts[i].saturating_add(1);
        self.counts[i]
    }

    /// 缺族数。
    pub fn missing(&self) -> usize {
        self.counts.iter().filter(|c| **c == 0).count()
    }

    /// 九族齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// 总签名。
    pub fn total(&self) -> u32 {
        self.counts.iter().map(|c| *c as u32).sum()
    }
}

// ─────────────────────────────────────────────────────────────
// 硬门（断链/预算）
// ─────────────────────────────────────────────────────────────

/// 硬门状态。
#[derive(Clone, Copy, Debug)]
pub struct HardGate {
    /// 断链数（须 0）。
    pub breaks: u32,
    /// 预算实测微秒（须 ≤1000 且已修）。
    pub budget_us: u32,
    /// 预算已修。
    pub budget_fixed: bool,
    /// 策略公开。
    pub policy_public: bool,
    /// 真源单源。
    pub sole_source: bool,
}

impl HardGate {
    /// 新建。
    pub const fn new() -> Self {
        HardGate {
            breaks: 1,
            budget_us: 0,
            budget_fixed: false,
            policy_public: false,
            sole_source: false,
        }
    }

    /// 断链硬门。
    pub fn break_gate(&self) -> bool {
        self.breaks == 0
    }

    /// 预算硬门。
    pub fn budget_gate(&self) -> bool {
        self.budget_us <= 1_000 && self.budget_fixed
    }

    /// 四联动全过。
    pub fn link_all(&self) -> bool {
        self.break_gate() && self.budget_gate() && self.policy_public && self.sole_source
    }

    /// 联动通过数。
    pub fn link_count(&self) -> u32 {
        let mut n = 0;
        if self.break_gate() {
            n += 1;
        }
        if self.budget_gate() {
            n += 1;
        }
        if self.policy_public {
            n += 1;
        }
        if self.sole_source {
            n += 1;
        }
        n
    }
}

// ─────────────────────────────────────────────────────────────
// 20 项自检 + 双签
// ─────────────────────────────────────────────────────────────

/// 20 项自检。
#[derive(Clone, Debug)]
pub struct SelfCheck20 {
    marks: [bool; 20],
}

impl SelfCheck20 {
    /// 新建。
    pub const fn new() -> Self {
        SelfCheck20 { marks: [false; 20] }
    }

    /// 勾一项。
    pub fn tick(&mut self, i: usize) -> bool {
        if i < 20 {
            self.marks[i] = true;
            true
        } else {
            false
        }
    }

    /// 已勾数。
    pub fn count(&self) -> usize {
        self.marks.iter().filter(|m| **m).count()
    }

    /// 全勾。
    pub fn complete(&self) -> bool {
        self.count() == 20
    }
}

/// 收口双签。
#[derive(Clone, Copy, Debug)]
pub struct CloseSign {
    /// 自查签。
    pub self_signed: bool,
    /// 核收签。
    pub recv_signed: bool,
}

impl CloseSign {
    /// 新建。
    pub const fn new() -> Self {
        CloseSign {
            self_signed: false,
            recv_signed: false,
        }
    }

    /// 双签齐。
    pub fn complete(self) -> bool {
        self.self_signed && self.recv_signed
    }
}

/// 收口器。
#[derive(Clone, Debug)]
pub struct Closer {
    /// 九族齐。
    pub family_ok: bool,
    /// 硬门全过。
    pub gates_ok: bool,
    /// 自检全勾。
    pub selfcheck_ok: bool,
    /// 双签。
    pub sign: CloseSign,
    /// 已收口。
    closed: bool,
}

impl Closer {
    /// 新建。
    pub const fn new() -> Self {
        Closer {
            family_ok: false,
            gates_ok: false,
            selfcheck_ok: false,
            sign: CloseSign::new(),
            closed: false,
        }
    }

    /// 可收口。
    pub fn ready(&self) -> bool {
        self.family_ok && self.gates_ok && self.selfcheck_ok && self.sign.complete()
    }

    /// 执行收口。
    pub fn close(&mut self) -> bool {
        if self.ready() {
            self.closed = true;
            true
        } else {
            false
        }
    }

    /// 已收口。
    pub fn closed(&self) -> bool {
        self.closed
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3820 判据集。
pub fn run_f3820_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3820");

    // 0xCD05 九族
    {
        let mut l = FamilyLedger::new();
        set.add("0xCD05-初始缺九", l.missing() == 9, "");
        for f in S01Family::ALL.iter() {
            l.add(*f);
        }
        set.add("0xCD05-九族齐", l.complete() && l.total() == 9, "");
    }

    // 0xCD02 断链硬门
    {
        let mut g = HardGate::new();
        set.add("0xCD02-初始断链拦", !g.break_gate() && !g.link_all(), "");
        g.breaks = 0;
        set.add("0xCD02-零断过门", g.break_gate(), "");
    }

    // 0xCD03 预算硬门
    {
        let mut g = HardGate::new();
        g.breaks = 0;
        g.budget_us = 800;
        set.add("0xCD03-未修不过门", !g.budget_gate(), "");
        g.budget_fixed = true;
        set.add("0xCD03-修后过门", g.budget_gate(), "");
        g.budget_us = 1200;
        set.add("0xCD03-超支不过", !g.budget_gate(), "");
    }

    // 四联动
    {
        let mut g = HardGate::new();
        g.breaks = 0;
        g.budget_us = 500;
        g.budget_fixed = true;
        g.policy_public = true;
        g.sole_source = true;
        set.add("0xCD04-四联动全过", g.link_all() && g.link_count() == 4, "");
        g.sole_source = false;
        set.add("0xCD04-缺一不过", !g.link_all() && g.link_count() == 3, "");
    }

    // 20 项自检
    {
        let mut c = SelfCheck20::new();
        for i in 0..20 {
            c.tick(i);
        }
        set.add("0xCD06-二十齐", c.complete(), "");
        set.add("0xCD06-越界拒", !c.tick(20), "");
    }

    // 0xCD06 双签收口
    {
        let mut r = Closer::new();
        set.add("0xCD06-初始不收", !r.close() && !r.closed(), "");
        r.family_ok = true;
        r.gates_ok = true;
        r.selfcheck_ok = true;
        r.sign.self_signed = true;
        set.add("0xCD06-缺签不收", !r.close(), "");
        r.sign.recv_signed = true;
        set.add("0xCD06-双签收口", r.close() && r.closed(), "");
    }

    // 族序自洽
    {
        let mut ok = true;
        for (i, f) in S01Family::ALL.iter().enumerate() {
            if f.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xCD05-族序自洽", ok, "");
    }

    set
}
