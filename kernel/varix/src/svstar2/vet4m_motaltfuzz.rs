#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xDC00）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xDC00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CDCCode(pub u16);

impl CDCCode {
    pub const URGENT_LOST: Self = Self(SEG_BASE);     // 紧急丢
    pub const SEMANTIC_BREAK: Self = Self(SEG_BASE + 1); // 语义破
    pub const HANG: Self = Self(SEG_BASE + 2);        // 挂起
    pub const LOOP_MISSING: Self = Self(SEG_BASE + 3); // 闭环缺
    pub const INVARIANT_BREAK: Self = Self(SEG_BASE + 4); // 不变量破

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::URGENT_LOST => "紧急丢",
            Self::SEMANTIC_BREAK => "语义破",
            Self::HANG => "挂起",
            Self::LOOP_MISSING => "闭环缺",
            Self::INVARIANT_BREAK => "不变量破",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::URGENT_LOST, Self::SEMANTIC_BREAK, Self::HANG,
        Self::LOOP_MISSING, Self::INVARIANT_BREAK,
    ];
}

// ── 四不变量 ────────────────────────────────────────────────────────

/// 不变量等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity { P0, P1 }

/// 四不变量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuzzInvariant {
    /// 对齐完整。
    AlignComplete,
    /// 语义一致。
    SemanticConsistent,
    /// 开销节省。
    OverheadSaved,
    /// 紧急保真。
    UrgentFidelity,
}

impl FuzzInvariant {
    pub const ALL: [Self; 4] = [
        Self::AlignComplete, Self::SemanticConsistent, Self::OverheadSaved, Self::UrgentFidelity,
    ];

    pub fn severity(self) -> Severity {
        match self {
            Self::UrgentFidelity | Self::SemanticConsistent => Severity::P0,
            _ => Severity::P1,
        }
    }
}

// ── 三级变异 ────────────────────────────────────────────────────────

/// 变异类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    /// 清单变异（缺项注入）。
    ListMutation,
    /// 策略变异（参数极端）。
    StrategyMutation,
    /// 引擎变异（指令流风暴）。
    EngineMutation,
}

impl MutationKind {
    pub const ALL: [Self; 3] = [Self::ListMutation, Self::StrategyMutation, Self::EngineMutation];
}

/// 变异案例。
#[derive(Debug, Clone)]
pub struct MutationCase {
    pub id: u32,
    pub kind: MutationKind,
    pub input: Vec<u8>,
    pub minimized: bool,
    pub closed_loop: bool,
}

// ── 看门狗 ──────────────────────────────────────────────────────────

/// 看门狗（防挂起）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Watchdog {
    pub timeout_ms: u32,
    pub triggered: bool,
}

impl Watchdog {
    pub fn new(timeout_ms: u32) -> Self { Self { timeout_ms, triggered: false } }
    /// 检测挂起。
    pub fn check(&mut self, elapsed_ms: u32) -> bool {
        if elapsed_ms > self.timeout_ms {
            self.triggered = true;
            false
        } else {
            true
        }
    }
}

// ── 语料库 ──────────────────────────────────────────────────────────

/// Fuzz 语料库。
#[derive(Debug, Clone)]
pub struct FuzzCorpus {
    pub cases: Vec<MutationCase>,
}

impl FuzzCorpus {
    pub fn new() -> Self { Self { cases: Vec::new() } }

    /// 回放：全部案例通过不变量。
    pub fn replay(&self, invariants: &[bool]) -> bool {
        invariants.len() == 4 && invariants.iter().all(|&x| x)
    }

    /// 闭环完整。
    pub fn all_closed(&self) -> bool {
        self.cases.iter().all(|c| c.closed_loop)
    }
}

// ── Fuzz 驱动 ───────────────────────────────────────────────────────

/// 动效无障碍 Fuzz 驱动。
#[derive(Debug, Clone)]
pub struct MotionA11yFuzz {
    pub corpus: FuzzCorpus,
    pub watchdog: Watchdog,
    pub p0_violations: u32,
    pub p1_violations: u32,
}

impl MotionA11yFuzz {
    pub fn new() -> Self {
        Self { corpus: FuzzCorpus::new(), watchdog: Watchdog::new(5000), p0_violations: 0, p1_violations: 0 }
    }

    /// 违规登记。
    pub fn report_violation(&mut self, inv: FuzzInvariant) {
        match inv.severity() {
            Severity::P0 => self.p0_violations += 1,
            Severity::P1 => self.p1_violations += 1,
        }
    }

    /// P0 硬门：P0 违规必须为 0。
    pub fn p0_gate_ok(&self) -> bool { self.p0_violations == 0 }

    /// 总审计。
    pub fn audit(&self) -> FuzzAudit {
        FuzzAudit {
            p0_ok: self.p0_gate_ok(),
            watchdog_ok: !self.watchdog.triggered,
            closed_loop: self.corpus.all_closed(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuzzAudit {
    pub p0_ok: bool,
    pub watchdog_ok: bool,
    pub closed_loop: bool,
}
impl FuzzAudit {
    pub fn all_green(&self) -> bool { self.p0_ok && self.watchdog_ok && self.closed_loop }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3875_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4m_motaltfuzz");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 四不变量
    {
        chk(&mut set, "C3875-01", FuzzInvariant::ALL.len() == 4, "err");
    }
    // 2 P0 分级
    {
        chk(&mut set, "C3875-02", FuzzInvariant::UrgentFidelity.severity() == Severity::P0, "err");
    }
    // 3 P1 分级
    {
        chk(&mut set, "C3875-03", FuzzInvariant::AlignComplete.severity() == Severity::P1, "err");
    }
    // 4 三级变异
    {
        chk(&mut set, "C3875-04", MutationKind::ALL.len() == 3, "err");
    }
    // 5 看门狗正常
    {
        let mut w = Watchdog::new(1000);
        chk(&mut set, "C3875-05", w.check(500), "err");
    }
    // 6 看门狗触发
    {
        let mut w = Watchdog::new(1000);
        chk(&mut set, "C3875-06", !w.check(2000) && w.triggered, "err");
    }
    // 7 P0 硬门
    {
        let f = MotionA11yFuzz::new();
        chk(&mut set, "C3875-07", f.p0_gate_ok(), "err");
    }
    // 8 P0 违规
    {
        let mut f = MotionA11yFuzz::new();
        f.report_violation(FuzzInvariant::UrgentFidelity);
        chk(&mut set, "C3875-08", !f.p0_gate_ok(), "err");
    }
    // 9 P1 不破 P0 门
    {
        let mut f = MotionA11yFuzz::new();
        f.report_violation(FuzzInvariant::AlignComplete);
        chk(&mut set, "C3875-09", f.p0_gate_ok() && f.p1_violations == 1, "err");
    }
    // 10 回放通过
    {
        let c = FuzzCorpus::new();
        chk(&mut set, "C3875-10", c.replay(&[true, true, true, true]), "err");
    }
    // 11 回放失败
    {
        let c = FuzzCorpus::new();
        chk(&mut set, "C3875-11", !c.replay(&[true, false, true, true]), "err");
    }
    // 12 闭环完整
    {
        let mut c = FuzzCorpus::new();
        c.cases.push(MutationCase { id: 1, kind: MutationKind::ListMutation, input: vec![0], minimized: true, closed_loop: true });
        chk(&mut set, "C3875-12", c.all_closed(), "err");
    }
    // 13 闭环缺失
    {
        let mut c = FuzzCorpus::new();
        c.cases.push(MutationCase { id: 1, kind: MutationKind::EngineMutation, input: vec![0], minimized: true, closed_loop: false });
        chk(&mut set, "C3875-13", !c.all_closed(), "err");
    }
    // 14 审计全绿
    {
        let f = MotionA11yFuzz::new();
        chk(&mut set, "C3875-14", f.audit().all_green(), "err");
    }
    // 15 审计 P0 失败
    {
        let mut f = MotionA11yFuzz::new();
        f.report_violation(FuzzInvariant::SemanticConsistent);
        chk(&mut set, "C3875-15", !f.audit().all_green(), "err");
    }
    // 16 语义也是 P0
    {
        chk(&mut set, "C3875-16", FuzzInvariant::SemanticConsistent.severity() == Severity::P0, "err");
    }
    // 17 空语料全闭环
    {
        let c = FuzzCorpus::new();
        chk(&mut set, "C3875-17", c.all_closed(), "err");
    }
    // 18 看门狗超时值
    {
        let w = Watchdog::new(5000);
        chk(&mut set, "C3875-18", w.timeout_ms == 5000, "err");
    }

    set
}
