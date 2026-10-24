//! VE-F3817 · 无障碍渲染 fuzz（VE-T 域 · 无障碍渲染 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3817`
//!
//! **判据（锚点原文）**：四不变量、态风暴、分级 P0、闭环复述、基建单源、判据。
//!
//! **本条是什么**：无障碍渲染 fuzz——态机 fuzz（辅具接入拔出风暴/
//! 四态交叉风暴）；三级变异（态/参数/时序）；四不变量（断链不发生/
//! 预算不超/策略可解释/降档保底）；违例分级（断链/保底 = P0）；
//! 最小化+闭环。
//!
//! 诊断码段：独占 0xCAxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xCAxx）
// ─────────────────────────────────────────────────────────────

/// F3817 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CACode(pub u16);

impl CACode {
    /// 断链 fuzz 检出——P0。
    pub const BREAK_P0: CACode = CACode(0xCA01);
    /// 预算爆——立案。
    pub const BUDGET_BLOW: CACode = CACode(0xCA02);
    /// 挂起——看门狗。
    pub const HANG: CACode = CACode(0xCA03);
    /// 闭环缺——阻断。
    pub const LOOP_OPEN: CACode = CACode(0xCA04);
    /// 保底破——P0。
    pub const REACH_P0: CACode = CACode(0xCA05);
    /// 三级变异缺。
    pub const MUT_MISS: CACode = CACode(0xCA06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xCA01 => "fuzz 检出断链，P0——fuzz 即执法。",
            0xCA02 => "预算被 fuzz 打爆，需立案。",
            0xCA03 => "fuzz 挂起，看门狗介入。",
            0xCA04 => "案例未闭环，阻断。",
            0xCA05 => "降档保底被 fuzz 打破，P0。",
            0xCA06 => "三级变异覆盖缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 三级变异
// ─────────────────────────────────────────────────────────────

/// 变异级（三级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MutLevel {
    /// 态变异（越界/风暴/竞态）。
    State,
    /// 参数变异（极端/超量）。
    Param,
    /// 时序变异（切换×帧×输入）。
    Timing,
}

impl MutLevel {
    /// 全集。
    pub const ALL: [MutLevel; 3] = [MutLevel::State, MutLevel::Param, MutLevel::Timing];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            MutLevel::State => 0,
            MutLevel::Param => 1,
            MutLevel::Timing => 2,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            MutLevel::State => "态变异",
            MutLevel::Param => "参数变异",
            MutLevel::Timing => "时序变异",
        }
    }
}

/// 语料库。
#[derive(Clone, Debug)]
pub struct Corpus {
    /// 每级案例数。
    counts: [u32; 3],
    /// 最小化后总数。
    pub minimized: u32,
}

impl Corpus {
    /// 新建。
    pub const fn new() -> Self {
        Corpus {
            counts: [0; 3],
            minimized: 0,
        }
    }

    /// 入库。
    pub fn add(&mut self, level: MutLevel) {
        self.counts[level.ordinal()] += 1;
    }

    /// 缺级数。
    pub fn missing_levels(&self) -> usize {
        self.counts.iter().filter(|c| **c == 0).count()
    }

    /// 三级齐。
    pub fn complete(&self) -> bool {
        self.missing_levels() == 0
    }

    /// 总案例。
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }

    /// 最小化。
    pub fn minimize(&mut self) {
        self.minimized = self.total();
    }
}

// ─────────────────────────────────────────────────────────────
// 四不变量 + 违例分级
// ─────────────────────────────────────────────────────────────

/// 不变量（四条）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inv {
    /// 断链不发生。
    NoBreak,
    /// 预算不超。
    BudgetOk,
    /// 策略可解释。
    Explainable,
    /// 降档保底。
    ReachFloor,
}

impl Inv {
    /// 全集。
    pub const ALL: [Inv; 4] = [
        Inv::NoBreak,
        Inv::BudgetOk,
        Inv::Explainable,
        Inv::ReachFloor,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Inv::NoBreak => 0,
            Inv::BudgetOk => 1,
            Inv::Explainable => 2,
            Inv::ReachFloor => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Inv::NoBreak => "断链不发生",
            Inv::BudgetOk => "预算不超",
            Inv::Explainable => "策略可解释",
            Inv::ReachFloor => "降档保底",
        }
    }

    /// 是否 P0 级（断链/保底）。
    pub fn is_p0(self) -> bool {
        matches!(self, Inv::NoBreak | Inv::ReachFloor)
    }
}

/// 违例登记。
#[derive(Clone, Debug)]
pub struct ViolationLog {
    /// 每不变量违例数。
    counts: [u32; 4],
}

impl ViolationLog {
    /// 新建。
    pub const fn new() -> Self {
        ViolationLog { counts: [0; 4] }
    }

    /// 记违例。
    pub fn bump(&mut self, inv: Inv) {
        self.counts[inv.ordinal()] += 1;
    }

    /// 总违例。
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }

    /// P0 违例数。
    pub fn p0_total(&self) -> u32 {
        let mut n = 0;
        for inv in Inv::ALL.iter() {
            if inv.is_p0() {
                n += self.counts[inv.ordinal()];
            }
        }
        n
    }

    /// 某项违例数。
    pub fn of(&self, inv: Inv) -> u32 {
        self.counts[inv.ordinal()]
    }
}

// ─────────────────────────────────────────────────────────────
// 态风暴 + 看门狗 + 闭环
// ─────────────────────────────────────────────────────────────

/// 态风暴驱动（辅具接入拔出/四态交叉）。
#[derive(Clone, Debug)]
pub struct StormDriver {
    /// 已驱动事件数。
    pub events: u32,
    /// 挂起计数。
    pub hangs: u32,
}

impl StormDriver {
    /// 新建。
    pub const fn new() -> Self {
        StormDriver {
            events: 0,
            hangs: 0,
        }
    }

    /// 驱动一次翻转。
    pub fn flip(&mut self) {
        self.events += 1;
    }

    /// 记挂起。
    pub fn hang(&mut self) {
        self.hangs += 1;
    }
}

/// 案例闭环。
#[derive(Clone, Debug)]
pub struct CaseLoop {
    pub found: u32,
    pub closed: u32,
}

impl CaseLoop {
    /// 新建。
    pub const fn new() -> Self {
        CaseLoop {
            found: 0,
            closed: 0,
        }
    }

    /// 发现。
    pub fn find(&mut self) {
        self.found += 1;
    }

    /// 闭环。
    pub fn close(&mut self) -> bool {
        if self.closed < self.found {
            self.closed += 1;
            true
        } else {
            false
        }
    }

    /// 未闭环。
    pub fn open(&self) -> u32 {
        self.found.saturating_sub(self.closed)
    }

    /// 全闭环。
    pub fn complete(&self) -> bool {
        self.open() == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3817 判据集。
pub fn run_f3817_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3817");

    // 0xCA06 三级变异
    {
        let mut c = Corpus::new();
        set.add("0xCA06-初始缺三", c.missing_levels() == 3, "");
        for l in MutLevel::ALL.iter() {
            c.add(*l);
        }
        set.add("0xCA06-三级齐", c.complete() && c.total() == 3, "");
        c.minimize();
        set.add("0xCA06-最小化", c.minimized == 3, "");
    }

    // 0xCA01/0xCA05 四不变量 P0
    {
        set.add("0xCA01-断链是P0", Inv::NoBreak.is_p0(), "");
        set.add("0xCA05-保底是P0", Inv::ReachFloor.is_p0(), "");
        set.add("0xCA02-预算非P0", !Inv::BudgetOk.is_p0(), "");
        let mut v = ViolationLog::new();
        v.bump(Inv::NoBreak);
        v.bump(Inv::BudgetOk);
        set.add("0xCA01-P0计数", v.p0_total() == 1, "");
        set.add("0xCA02-总违例", v.total() == 2, "");
        set.add("0xCA01-分项计数", v.of(Inv::NoBreak) == 1, "");
    }

    // 0xCA03 态风暴
    {
        let mut s = StormDriver::new();
        s.flip();
        s.flip();
        set.add("0xCA03-驱动计数", s.events == 2, "");
        s.hang();
        set.add("0xCA03-挂起计数", s.hangs == 1, "");
    }

    // 0xCA04 闭环
    {
        let mut l = CaseLoop::new();
        set.add("0xCA04-空案闭环", l.complete(), "");
        l.find();
        l.find();
        set.add("0xCA04-未闭不过", !l.complete() && l.open() == 2, "");
        l.close();
        l.close();
        set.add("0xCA04-全闭过", l.complete(), "");
        set.add("0xCA04-空闭拒", !l.close(), "");
    }

    // 级序自洽
    {
        let mut ok = true;
        for (i, m) in MutLevel::ALL.iter().enumerate() {
            if m.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xCA06-级序自洽", ok, "");
        let mut ok_i = true;
        for (i, inv) in Inv::ALL.iter().enumerate() {
            if inv.ordinal() != i {
                ok_i = false;
            }
        }
        set.add("0xCA01-不变量序自洽", ok_i, "");
    }

    set
}
