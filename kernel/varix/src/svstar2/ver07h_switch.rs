//! VE-F3448 · 变体即时切换全链 —— 两阶段事务、影响面分级、交互不中断
//!
//! ## 职责拆解
//! 变体切换是主题切换的轻量版：**快，但仍走完整安全链**。
//! 1. **两阶段**（`Switcher::begin/prepare/commit`）：先算影响面（prepare），
//!    再一次性落地（commit）。跳过 prepare 直接 commit 会被拒——那不是快，是跳步。
//! 2. **影响面分级**（`Impact`）：按「改了几条轴」分三级。
//!    1 轴 = Light（只重算该轴令牌）、2~3 轴 = Medium（跨轴级联）、
//!    4~5 轴 = Full（等价于换肤，走 E02 的全链）。
//!    分级不是装饰：Light 可以同步完成，Full 必须给进度反馈。
//! 3. **交互不中断**（`input` / `replayed`）：切换事务期间到达的点击先入队，
//!    commit 后按序重放。**中断一秒钟的点击给用户的感受是「卡」**，
//!    而重放一个点击的代价几乎为零。
//! 4. **可回滚**（`rollback_last`）：committed 之后仍能退回上一组合。
//!    切换是即时的，误切也是即时的——所以回滚必须同样即时。
//! 5. **失败不留半个变体**：abort 后活跃组合必须**一字未变**，
//!    这是 E02「失败必回滚」律在变体轴上的复述。
//!
//! ## 为什么这么拆
//! 「即时切换」四个字最容易导致两个极端：要么做成同步硬切（大影响面时卡顿），
//! 要么做成异步排队（用户点了没反应）。本册取第三条路：**同步事务 + 影响面分级**
//! ——所有工作都在一次调用里完成，但调用方拿到 `Impact` 后可以选择是否先给进度。
//! 这样既没有异步状态机，也不会在 Full 档静默卡住。
//!
//! ## 复杂度
//! - begin/prepare/commit/abort 全 O(轴数) = 5。
//! - 输入缓冲 O(1) 入队、O(k) 重放，k ≤ `PENDING_CAP`(16)。
//!
//! ## 零 panic 面
//! - 状态机转换全部 `match` 穷举；非法转换返回错误而非 `unreachable!()`。
//! - 无索引切片；`Vec::remove(0)` 前先判空；计数器 `saturating_*`。

use alloc::format;
use alloc::string::ToString;
use alloc::string::String;
use alloc::vec::Vec;

use super::ver07a_variants::{Axis, Combo};

/// 事务期间可缓冲的输入上限。超过后丢弃最旧的一条——旧点击的目标可能已消失，
/// 保留它反而会重放到错误位置。
pub const PENDING_CAP: usize = 16;

/// 每条轴变化对应的令牌重算量（估算值，用于影响面报告）。
pub const TOKENS_PER_AXIS: usize = 24;

/// 诊断码（E18 段 · 5657~5666）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum SwitchCode {
    /// 已有在途事务。
    InFlight = 5657,
    /// 没有在途事务。
    NoTxn = 5658,
    /// 切到当前组合（空转）。
    NoOp = 5659,
    /// 未 prepare 就 commit。
    NotPrepared = 5660,
    /// 状态机非法转换。
    BadPhase = 5661,
    /// 目标组合非法（命中互斥）。
    IllegalTarget = 5662,
    /// 档位越界。
    LevelOutOfRange = 5663,
    /// 回滚历史为空。
    NothingToRollback = 5664,
    /// 提交后活跃值与目标不一致（事务未真正落地）。
    CommitMismatch = 5665,
    /// 影响面报告与变更轴数不一致。
    ImpactSkew = 5666,
}

impl SwitchCode {
    pub fn code(self) -> u16 {
        self as u16
    }

    pub fn text(self) -> &'static str {
        match self {
            SwitchCode::InFlight => "已有在途事务",
            SwitchCode::NoTxn => "没有在途事务",
            SwitchCode::NoOp => "切到当前组合（空转）",
            SwitchCode::NotPrepared => "未准备就提交",
            SwitchCode::BadPhase => "状态机非法转换",
            SwitchCode::IllegalTarget => "目标组合非法",
            SwitchCode::LevelOutOfRange => "档位越界",
            SwitchCode::NothingToRollback => "无可回滚历史",
            SwitchCode::CommitMismatch => "提交后活跃值与目标不一致",
            SwitchCode::ImpactSkew => "影响面与变更轴数不一致",
        }
    }
}

/// 事务阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Began,
    Prepared,
    Committed,
    Aborted,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Began => "已开始",
            Phase::Prepared => "已准备",
            Phase::Committed => "已提交",
            Phase::Aborted => "已放弃",
        }
    }
}

/// 影响面等级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ImpactLevel {
    Light,
    Medium,
    Full,
}

impl ImpactLevel {
    pub fn label(self) -> &'static str {
        match self {
            ImpactLevel::Light => "轻",
            ImpactLevel::Medium => "中",
            ImpactLevel::Full => "重",
        }
    }

    /// 是否需要给用户进度反馈。轻档同步完成，不需要。
    pub fn needs_progress(self) -> bool {
        !matches!(self, ImpactLevel::Light)
    }
}

/// 影响面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Impact {
    pub changed: u8,
    pub level: ImpactLevel,
    pub tokens: usize,
    pub permille: u32,
}

impl Impact {
    /// 由变更轴数算出。分级阈值写死在这里，别处不再判断。
    pub fn of(changed: u8) -> Self {
        let level = if changed <= 1 {
            ImpactLevel::Light
        } else if changed <= 3 {
            ImpactLevel::Medium
        } else {
            ImpactLevel::Full
        };
        let axes = usize::from(changed.min(5));
        Self {
            changed,
            level,
            tokens: axes.saturating_mul(TOKENS_PER_AXIS),
            permille: u32::from(changed.min(5)) * 200,
        }
    }

    /// 是否需要进度反馈（转发到等级，调用方不必二次判断）。
    pub fn needs_progress(&self) -> bool {
        self.level.needs_progress()
    }

    pub fn line(&self) -> String {
        format!(
            "影响面 {}：{} 轴 / {} 令牌 / {}‰",
            self.level.label(),
            self.changed,
            self.tokens,
            self.permille
        )
    }
}

/// 一次切换事务。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Txn {
    pub id: u64,
    pub base: Combo,
    pub target: Combo,
    pub phase: Phase,
    pub impact: Option<Impact>,
}

/// 切换器。
#[derive(Clone, Debug)]
pub struct Switcher {
    active: Combo,
    txn: Option<Txn>,
    next_id: u64,
    pending: Vec<u32>,
    history: Vec<Combo>,
    pub applied: usize,
    pub replayed: usize,
    pub dropped: usize,
}

impl Switcher {
    pub fn new(initial: Combo) -> Self {
        Self {
            active: initial,
            txn: None,
            next_id: 1,
            pending: Vec::new(),
            history: Vec::new(),
            applied: 0,
            replayed: 0,
            dropped: 0,
        }
    }

    pub fn active(&self) -> Combo {
        self.active
    }

    pub fn txn(&self) -> Option<&Txn> {
        self.txn.as_ref()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 开始切换。目标与当前相同直接拒（空转），已有在途事务也拒（不嵌套）。
    pub fn begin(&mut self, target: Combo) -> Result<u64, SwitchCode> {
        if self.txn.is_some() {
            return Err(SwitchCode::InFlight);
        }
        if target == self.active {
            return Err(SwitchCode::NoOp);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.txn = Some(Txn {
            id,
            base: self.active,
            target,
            phase: Phase::Began,
            impact: None,
        });
        Ok(id)
    }

    /// 准备：算影响面。**不碰活跃值**——准备阶段失败必须等于什么都没发生。
    pub fn prepare(&mut self) -> Result<Impact, SwitchCode> {
        let (base, target) = match &self.txn {
            None => return Err(SwitchCode::NoTxn),
            Some(t) => (t.base, t.target),
        };
        let changed = base.changed_axes(&target);
        let impact = Impact::of(changed.len() as u8);
        match self.txn.as_mut() {
            None => return Err(SwitchCode::NoTxn),
            Some(t) => {
                if t.phase != Phase::Began {
                    return Err(SwitchCode::BadPhase);
                }
                t.phase = Phase::Prepared;
                t.impact = Some(impact);
            }
        }
        Ok(impact)
    }

    /// 提交：落地 + 重放缓冲输入。未 prepare 一律拒。
    pub fn commit(&mut self) -> Result<Impact, SwitchCode> {
        let t = match &self.txn {
            None => return Err(SwitchCode::NoTxn),
            Some(t) => t.clone(),
        };
        if t.phase != Phase::Prepared {
            return Err(SwitchCode::NotPrepared);
        }
        let impact = match t.impact {
            Some(i) => i,
            None => return Err(SwitchCode::NotPrepared),
        };
        // 影响面自检：变更轴数必须与影响面一致（防止「算了但没用」）。
        if usize::from(impact.changed) != t.base.changed_axes(&t.target).len() {
            return Err(SwitchCode::ImpactSkew);
        }
        self.active = t.target;
        self.history.push(t.base);
        // 重放缓冲输入
        let n = self.pending.len();
        self.replayed = self.replayed.saturating_add(n);
        self.pending.clear();
        if let Some(tx) = self.txn.as_mut() {
            tx.phase = Phase::Committed;
        }
        self.txn = None;
        if self.active != t.target {
            return Err(SwitchCode::CommitMismatch);
        }
        Ok(impact)
    }

    /// 放弃：活跃值一字未动，缓冲输入丢弃并计数。
    pub fn abort(&mut self) -> Result<(), SwitchCode> {
        if self.txn.is_none() {
            return Err(SwitchCode::NoTxn);
        }
        self.dropped = self.dropped.saturating_add(self.pending.len());
        self.pending.clear();
        if let Some(tx) = self.txn.as_mut() {
            tx.phase = Phase::Aborted;
        }
        self.txn = None;
        Ok(())
    }

    /// 事务期间的输入。有在途事务则入队，否则立即生效。
    pub fn input(&mut self, id: u32) -> bool {
        if self.txn.is_some() {
            if self.pending.len() >= PENDING_CAP {
                // 丢最旧：旧点击的目标可能已经不在原位。
                self.pending.remove(0);
                self.dropped = self.dropped.saturating_add(1);
            }
            self.pending.push(id);
            false
        } else {
            self.applied = self.applied.saturating_add(1);
            true
        }
    }

    /// 回滚到上一组合（committed 之后可用）。
    pub fn rollback_last(&mut self) -> Result<Combo, SwitchCode> {
        if self.txn.is_some() {
            return Err(SwitchCode::InFlight);
        }
        match self.history.pop() {
            Some(prev) => {
                self.active = prev;
                Ok(prev)
            }
            None => Err(SwitchCode::NothingToRollback),
        }
    }

    pub fn history_depth(&self) -> usize {
        self.history.len()
    }
}

/// 自检：跑一遍典型链路（准备→提交→回滚），确认每步的状态与不变量。
pub fn self_check(initial: Combo, target: Combo) -> Vec<SwitchCode> {
    let mut out = Vec::new();
    let mut s = Switcher::new(initial);
    if !initial.changed_axes(&target).is_empty() {
        match s.begin(target) {
            Ok(_) => {}
            Err(e) => {
                out.push(e);
                return out;
            }
        }
        // 未准备就提交必须拒
        if s.commit().is_ok() {
            out.push(SwitchCode::NotPrepared);
        }
        let impact = match s.prepare() {
            Ok(i) => i,
            Err(e) => {
                out.push(e);
                return out;
            }
        };
        if usize::from(impact.changed) != initial.changed_axes(&target).len() {
            out.push(SwitchCode::ImpactSkew);
        }
        if s.commit().is_err() {
            out.push(SwitchCode::CommitMismatch);
        }
        if s.active() != target {
            out.push(SwitchCode::CommitMismatch);
        }
        if s.rollback_last() != Ok(initial) {
            out.push(SwitchCode::NothingToRollback);
        }
    }
    out
}

/// 读屏清单：三级影响面的含义。
pub fn impact_lines() -> Vec<String> {
    let mut out = Vec::new();
    out.push("影响面 | 变更轴 | 令牌 | 需进度".to_string());
    for n in 1u8..=5 {
        let i = Impact::of(n);
        out.push(format!(
            "{:<6} {:>4} 轴 {:>4} 个 {}",
            i.level.label(),
            i.changed,
            i.tokens,
            if i.level.needs_progress() { "是" } else { "否" }
        ));
    }
    out
}

/// 便捷：一轴切换的完整链路。返回影响面。
pub fn quick_switch(s: &mut Switcher, axis: Axis, level: u8) -> Result<Impact, SwitchCode> {
    let target = match s.active().with(axis, level) {
        Ok(c) => c,
        Err(_) => return Err(SwitchCode::LevelOutOfRange),
    };
    s.begin(target)?;
    s.prepare()?;
    s.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo() -> Combo {
        Combo::default_combo()
    }

    #[test]
    fn two_phase_happy_path() {
        let base = combo();
        let target = base.with(Axis::Density, 0).unwrap_or(base);
        let mut s = Switcher::new(base);
        let id = s.begin(target).unwrap_or_else(|_| panic!("begin 应成功"));
        assert_eq!(id, 1);
        assert_eq!(s.txn().map(|t| t.phase), Some(Phase::Began));
        let impact = s.prepare().unwrap_or_else(|_| panic!("prepare 应成功"));
        assert_eq!(impact.changed, 1);
        assert_eq!(impact.level, ImpactLevel::Light);
        assert!(!impact.level.needs_progress());
        assert!(s.commit().is_ok());
        assert_eq!(s.active(), target);
        assert_eq!(s.txn(), None);
        assert_eq!(s.history_depth(), 1);
    }

    #[test]
    fn commit_without_prepare_is_refused() {
        let base = combo();
        let target = base.with(Axis::Motion, 2).unwrap_or(base);
        let mut s = Switcher::new(base);
        assert!(s.begin(target).is_ok());
        assert_eq!(s.commit(), Err(SwitchCode::NotPrepared));
        // 拒绝后活跃值未变，事务仍在途
        assert_eq!(s.active(), base);
        assert!(s.txn().is_some());
    }

    #[test]
    fn no_nested_txn_and_no_noop() {
        let base = combo();
        let t1 = base.with(Axis::Radius, 0).unwrap_or(base);
        let t2 = base.with(Axis::Radius, 2).unwrap_or(base);
        let mut s = Switcher::new(base);
        assert_eq!(s.begin(base), Err(SwitchCode::NoOp));
        assert!(s.begin(t1).is_ok());
        assert_eq!(s.begin(t2), Err(SwitchCode::InFlight));
    }

    #[test]
    fn abort_leaves_active_untouched_and_counts_dropped() {
        let base = combo();
        let target = base.with(Axis::FocusRing, 2).unwrap_or(base);
        let mut s = Switcher::new(base);
        assert!(s.begin(target).is_ok());
        assert_eq!(s.input(101), false, "在途时应入队");
        assert_eq!(s.input(102), false);
        assert_eq!(s.pending_len(), 2);
        assert!(s.abort().is_ok());
        assert_eq!(s.active(), base, "放弃后一字未动");
        assert_eq!(s.pending_len(), 0);
        assert_eq!(s.dropped, 2);
        assert_eq!(s.replayed, 0);
    }

    #[test]
    fn inputs_are_replayed_on_commit() {
        let base = combo();
        let target = base.with(Axis::Density, 2).unwrap_or(base);
        let mut s = Switcher::new(base);
        assert!(s.begin(target).is_ok());
        assert_eq!(s.input(1), false);
        assert_eq!(s.input(2), false);
        assert!(s.prepare().is_ok());
        assert!(s.commit().is_ok());
        assert_eq!(s.replayed, 2);
        assert_eq!(s.pending_len(), 0);
        // 事务结束后输入立即生效
        assert_eq!(s.input(3), true);
        assert_eq!(s.applied, 1);
    }

    #[test]
    fn pending_buffer_drops_oldest_on_overflow() {
        let base = combo();
        let target = base.with(Axis::Density, 0).unwrap_or(base);
        let mut s = Switcher::new(base);
        assert!(s.begin(target).is_ok());
        for i in 0..(PENDING_CAP as u32 + 3) {
            let _ = s.input(i);
        }
        assert_eq!(s.pending_len(), PENDING_CAP);
        assert_eq!(s.dropped, 3);
        assert!(s.prepare().is_ok());
        assert!(s.commit().is_ok());
        assert_eq!(s.replayed, PENDING_CAP);
    }

    #[test]
    fn rollback_returns_previous_combo_and_refuses_during_txn() {
        let base = combo();
        let a = base.with(Axis::Density, 0).unwrap_or(base);
        let b = a.with(Axis::Radius, 2).unwrap_or(a);
        let mut s = Switcher::new(base);
        assert!(quick_switch(&mut s, Axis::Density, 0).is_ok());
        assert!(quick_switch(&mut s, Axis::Radius, 2).is_ok());
        assert_eq!(s.active(), b);
        assert_eq!(s.rollback_last(), Ok(a));
        assert_eq!(s.rollback_last(), Ok(base));
        assert_eq!(s.rollback_last(), Err(SwitchCode::NothingToRollback));
        // 在途时不许回滚
        assert!(s.begin(a).is_ok());
        assert_eq!(s.rollback_last(), Err(SwitchCode::InFlight));
    }

    #[test]
    fn impact_levels_are_thresholded_at_1_and_3() {
        assert_eq!(Impact::of(1).level, ImpactLevel::Light);
        assert_eq!(Impact::of(2).level, ImpactLevel::Medium);
        assert_eq!(Impact::of(3).level, ImpactLevel::Medium);
        assert_eq!(Impact::of(4).level, ImpactLevel::Full);
        assert_eq!(Impact::of(5).level, ImpactLevel::Full);
        assert_eq!(Impact::of(9).changed, 9);
        assert_eq!(Impact::of(9).tokens, 5 * TOKENS_PER_AXIS, "令牌数按 5 轴封顶");
        assert!(Impact::of(5).needs_progress());
    }

    #[test]
    fn self_check_passes_on_standard_combo() {
        let base = combo();
        let target = base
            .with(Axis::Density, 2)
            .and_then(|c| c.with(Axis::Motion, 2))
            .unwrap_or(base);
        assert!(self_check(base, target).is_empty());
        assert!(self_check(base, base).is_empty(), "空转不应产生问题");
    }

    #[test]
    fn level_out_of_range_surfaces() {
        let base = combo();
        let mut s = Switcher::new(base);
        assert_eq!(quick_switch(&mut s, Axis::Density, 7), Err(SwitchCode::LevelOutOfRange));
    }

    #[test]
    fn impact_lines_are_readable() {
        let l = impact_lines();
        assert_eq!(l.len(), 6);
        assert!(l[0].contains("影响面"));
    }
}
