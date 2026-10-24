//! VE-F4025 · 方向切换动画
//!
//! LTR↔RTL 切换动画：切换原子（复用事务模式跨域单源——原子复述）；
//! 切换过渡（布局镜像过渡动画——元素位移过渡——过渡 300ms 预算——过
//! 渡断言）；切换打断（切换中再切——合并复用）；切换无障碍（reduce
//! 态直达——直达复述）。
//! 数据结构：事务复用；过渡断言（300ms）；合并复用；直达复述。
//! 错误路径：半态镜像（新旧混合）→P1；过渡超→预算立案；打断风暴→
//! 合并；reduce 不直达→修正。
//! 性能：原子 O(事务复用)；过渡 O(300ms)；合并 O(1)；直达 O(复述)。
//! 跨批对接：F2889/F3025 复用单源声明；F4022 镜像对端；F3146 reduce 对端。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6A40）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6A40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T31ACode(pub u16);

impl T31ACode {
    pub const OK: Self = Self(SEG_BASE);
    pub const HALF_STATE: Self = Self(SEG_BASE + 1);
    pub const OVER_BUDGET: Self = Self(SEG_BASE + 2);
    pub const REDIRECT_MISS: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::HALF_STATE => "半态镜像→P1",
            Self::OVER_BUDGET => "过渡超→预算立案",
            Self::REDIRECT_MISS => "reduce 不直达→修正",
            _ => "未知",
        }
    }
}

// ── 切换原子（事务复用）─────────────────────────────────────────────

/// 方向态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirState {
    Ltr,
    Rtl,
}

/// 切换事务：begin → apply → commit 三段原子（复用单源事务模式）。
#[derive(Debug, Clone)]
pub struct DirTx {
    /// 事务开。
    pub opened: bool,
    /// 已应用。
    pub applied: bool,
    /// 已提交。
    pub committed: bool,
    /// 目标方向。
    pub target: Option<DirState>,
}

impl DirTx {
    pub fn new() -> Self {
        Self { opened: false, applied: false, committed: false, target: None }
    }

    /// 开启事务（原子三段——不开就 apply 即半态）。
    pub fn begin(&mut self, target: DirState) {
        self.opened = true;
        self.target = Some(target);
    }

    /// 应用（前置：已开）。
    pub fn apply(&mut self) -> T31ACode {
        if !self.opened {
            return T31ACode::HALF_STATE;
        }
        self.applied = true;
        T31ACode::OK
    }

    /// 提交（前置：已应用）。
    pub fn commit(&mut self) -> T31ACode {
        if !self.applied {
            return T31ACode::HALF_STATE;
        }
        self.committed = true;
        T31ACode::OK
    }

    /// 原子断言：三段齐 = 无半态。
    pub fn atomic(&self) -> bool {
        self.opened && self.applied && self.committed
    }
}

// ── 过渡预算（300ms）────────────────────────────────────────────────

pub const TRANSITION_BUDGET_MS: u32 = 300;

/// 过渡条目：元素 + 位移时长。
#[derive(Debug, Clone)]
pub struct TransitionItem {
    pub element: String,
    pub duration_ms: u32,
}

/// 过渡断言。
pub fn transition_assert(items: &[TransitionItem]) -> T31ACode {
    if items.iter().any(|i| i.duration_ms > TRANSITION_BUDGET_MS) {
        T31ACode::OVER_BUDGET
    } else {
        T31ACode::OK
    }
}

// ── 切换打断合并 ────────────────────────────────────────────────────

/// 打断风暴：进行中切换收到新切换 → 合并为一（只保留最新目标）。
#[derive(Debug, Clone)]
pub struct InterruptionMux {
    /// 已合并次数。
    pub merged: u32,
    /// 当前目标。
    pub current: Option<DirState>,
}

impl InterruptionMux {
    pub fn new() -> Self {
        Self { merged: 0, current: None }
    }

    /// 请求切换（合并语义）。
    pub fn request(&mut self, target: DirState, in_flight: bool) -> u32 {
        if in_flight && self.current.is_some() {
            self.merged += 1;
        }
        self.current = Some(target);
        self.merged
    }
}

// ── reduce 态直达 ───────────────────────────────────────────────────

/// reduce 态切换：跳过过渡直达目标。
pub fn reduce_direct(reduce_motion: bool, duration_ms: u32) -> T31ACode {
    if reduce_motion && duration_ms > 0 {
        return T31ACode::REDIRECT_MISS;
    }
    T31ACode::OK
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves265_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves265_s12anim");

    // C4025-01 诊断码段不越窗。
    chk(&mut set, "C4025-01",
        T31ACode::REDIRECT_MISS.0 < SEG_BASE + 16 && SEG_BASE == 0x6A40, "err");

    // C4025-02 原子三段：begin→apply→commit。
    let mut tx = DirTx::new();
    tx.begin(DirState::Rtl);
    tx.apply();
    tx.commit();
    chk(&mut set, "C4025-02", tx.atomic(), "err");

    // C4025-03 半态检出：未开事务直接 apply。
    let mut tx2 = DirTx::new();
    chk(&mut set, "C4025-03", tx2.apply() == T31ACode::HALF_STATE, "err");

    // C4025-04 半态检出：未 apply 直接 commit。
    let mut tx3 = DirTx::new();
    tx3.begin(DirState::Ltr);
    chk(&mut set, "C4025-04", tx3.commit() == T31ACode::HALF_STATE, "err");

    // C4025-05 原子未满（只开不交）。
    let mut tx4 = DirTx::new();
    tx4.begin(DirState::Rtl);
    tx4.apply();
    chk(&mut set, "C4025-05", !tx4.atomic(), "err");

    // C4025-06 过渡预算 300ms。
    chk(&mut set, "C4025-06", TRANSITION_BUDGET_MS == 300, "err");

    // C4025-07 过渡断言：预算内过。
    let items = [
        TransitionItem { element: String::from("侧栏"), duration_ms: 280 },
        TransitionItem { element: String::from("主区"), duration_ms: 300 },
    ];
    chk(&mut set, "C4025-07", transition_assert(&items) == T31ACode::OK, "err");

    // C4025-08 过渡超 → 立案。
    let items2 = [TransitionItem { element: String::from("卡片"), duration_ms: 450 }];
    chk(&mut set, "C4025-08", transition_assert(&items2) == T31ACode::OVER_BUDGET, "err");

    // C4025-09 打断合并：进行中再切 → 合并计数。
    let mut mux = InterruptionMux::new();
    mux.request(DirState::Rtl, false);
    let m1 = mux.request(DirState::Ltr, true);
    let m2 = mux.request(DirState::Rtl, true);
    chk(&mut set, "C4025-09", m1 == 1 && m2 == 2 && mux.current == Some(DirState::Rtl), "err");

    // C4025-10 非打断不合并。
    let mut mux2 = InterruptionMux::new();
    chk(&mut set, "C4025-10", mux2.request(DirState::Ltr, false) == 0, "err");

    // C4025-11 reduce 直达：时长 0。
    chk(&mut set, "C4025-11", reduce_direct(true, 0) == T31ACode::OK, "err");

    // C4025-12 reduce 不直达 → 修正。
    chk(&mut set, "C4025-12", reduce_direct(true, 250) == T31ACode::REDIRECT_MISS, "err");

    // C4025-13 正常态允许过渡。
    chk(&mut set, "C4025-13", reduce_direct(false, 250) == T31ACode::OK, "err");

    // C4025-14 方向枚举互异。
    chk(&mut set, "C4025-14", DirState::Ltr != DirState::Rtl, "err");

    // C4025-15 合并 O(1)（计数器语义）。
    let mut mux3 = InterruptionMux::new();
    for i in 0..100 {
        mux3.request(if i % 2 == 0 { DirState::Ltr } else { DirState::Rtl }, true);
    }
    chk(&mut set, "C4025-15", mux3.merged == 100, "err");

    // C4025-16 空过渡断言过。
    chk(&mut set, "C4025-16", transition_assert(&[]) == T31ACode::OK, "err");

    // C4025-17 半态码语义。
    chk(&mut set, "C4025-17",
        T31ACode::HALF_STATE.explain().contains("P1")
            && T31ACode::HALF_STATE.explain().contains("混合"), "err");

    // C4025-18 目标方向寄存。
    let mut tx5 = DirTx::new();
    tx5.begin(DirState::Ltr);
    chk(&mut set, "C4025-18", tx5.target == Some(DirState::Ltr), "err");

    // C4025-19 错误码互异。
    let codes = [T31ACode::OK.0, T31ACode::HALF_STATE.0, T31ACode::OVER_BUDGET.0, T31ACode::REDIRECT_MISS.0];
    chk(&mut set, "C4025-19",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    // C4025-20 原子复述单源：未开事务 commit 也拦（双端防护）。
    let mut tx6 = DirTx::new();
    chk(&mut set, "C4025-20",
        tx.atomic() && tx6.commit() == T31ACode::HALF_STATE, "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_tx() {
        let mut tx = DirTx::new();
        tx.begin(DirState::Rtl);
        assert_eq!(tx.apply(), T31ACode::OK);
        assert_eq!(tx.commit(), T31ACode::OK);
        assert!(tx.atomic());
    }

    #[test]
    fn half_state_detected() {
        let mut tx = DirTx::new();
        assert_eq!(tx.apply(), T31ACode::HALF_STATE);
    }

    #[test]
    fn reduce_direct_gate() {
        assert_eq!(reduce_direct(true, 200), T31ACode::REDIRECT_MISS);
        assert_eq!(reduce_direct(true, 0), T31ACode::OK);
    }
}
