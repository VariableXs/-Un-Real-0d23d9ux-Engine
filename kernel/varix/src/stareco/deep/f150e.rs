//! 深化层二 · F150 生态域总判据（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】计时归档与【设计细节】志愿者画像与卡壳
//! 回流（主册 G-D-25）：四步分段计时（逐步预算+超时定位）、志愿者
//! 画像可比性分组、卡壳点回流工单（F135/F127 生成+闭环确认）、基线
//! 重置断点册、逐季耗时曲线与回退红标。

use crate::checks::CheckSet;
use crate::stareco::ecogate::{Step, StuckKind, TOTAL_BUDGET_MIN};

// ---------------------------------------------------------------------------
// 四步分段计时：逐步预算 + 超时定位（哪步慢修哪步）
// ---------------------------------------------------------------------------

pub struct StepTiming {
    pub step: Step,
    pub minutes: u32,
}

/// 分段审计：逐步对照 limit_min，返回超时步清单（预算定位）。
pub fn timeout_steps(timings: &[StepTiming]) -> alloc::vec::Vec<Step> {
    timings.iter().filter(|t| t.minutes > t.step.limit_min()).map(|t| t.step).collect()
}

/// 总预算复核：与基础层 TOTAL_BUDGET_MIN 对账（一处一事实的互证实现）。
pub fn total_within_budget(timings: &[StepTiming]) -> bool {
    let total: u32 = timings.iter().map(|t| t.minutes).sum();
    total <= TOTAL_BUDGET_MIN
}

/// 步占比（万分比）——超时归因的权重证据。
pub fn step_share_bp(t: &StepTiming, total: u32) -> u32 {
    if total == 0 {
        return 0;
    }
    ((t.minutes * 10_000) / total).min(10_000)
}

// ---------------------------------------------------------------------------
// 志愿者画像可比性分组（年限分档——AI 自测不与外部真人混算）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ExperienceTier {
    /// < 1 年。
    Novice,
    /// 1-5 年。
    Junior,
    /// > 5 年。
    Senior,
}

pub fn tier_of(years: u8) -> ExperienceTier {
    match years {
        0 => ExperienceTier::Novice,
        1..=5 => ExperienceTier::Junior,
        _ => ExperienceTier::Senior,
    }
}

/// 可比性：同档 + 同流程代次（与基础层 comparable_with 互证）。
pub fn comparable(a: (ExperienceTier, u32), b: (ExperienceTier, u32)) -> bool {
    a.0 == b.0 && a.1 == b.1
}

// ---------------------------------------------------------------------------
// 卡壳点回流工单：分类 → 目标域 → 闭环确认
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BacklogTarget {
    /// 文档缺 → F135。
    Docs,
    /// 工具错 → F127。
    Tooling,
    /// 流程绕 → 治理（F148 规则修订提案）。
    Process,
}

pub fn backlog_target(kind: StuckKind) -> BacklogTarget {
    match kind {
        StuckKind::DocMissing => BacklogTarget::Docs,
        StuckKind::ToolBug => BacklogTarget::Tooling,
        StuckKind::ProcessWinding => BacklogTarget::Process,
    }
}

pub struct BacklogTicket {
    pub target: BacklogTarget,
    pub kind: StuckKind,
    /// 工单指纹（闭环对拍锚）。
    pub fp: u64,
    pub closed: bool,
}

pub struct BacklogFlow {
    tickets: alloc::vec::Vec<BacklogTicket>,
}

impl BacklogFlow {
    pub fn new() -> BacklogFlow {
        BacklogFlow { tickets: alloc::vec::Vec::new() }
    }

    pub fn raise(&mut self, kind: StuckKind, fp: u64) -> Result<BacklogTarget, &'static str> {
        if fp == 0 {
            return Err("工单指纹缺失：不可对拍的卡壳单不收");
        }
        let target = backlog_target(kind);
        self.tickets.push(BacklogTicket { target, kind, fp, closed: false });
        Ok(target)
    }

    /// 闭环确认：同指纹工单标记已修（闭环率进入季报）。
    pub fn confirm_closed(&mut self, fp: u64) -> Result<(), &'static str> {
        let t = self.tickets.iter_mut().find(|t| t.fp == fp).ok_or("工单不存在")?;
        if t.closed {
            return Err("已闭环：不重复确认");
        }
        t.closed = true;
        Ok(())
    }

    /// 闭环率（万分比）。
    pub fn close_rate_bp(&self) -> u32 {
        if self.tickets.is_empty() {
            return 10_000; // 无卡壳 = 满闭环（不是 0）
        }
        let closed = self.tickets.iter().filter(|t| t.closed).count();
        ((closed * 10_000) / self.tickets.len()) as u32
    }

    pub fn len(&self) -> usize {
        self.tickets.len()
    }
}

// ---------------------------------------------------------------------------
// 基线重置断点册：流程大改 → 断点标注 + 新基线（口径连续性）
// ---------------------------------------------------------------------------

pub struct BaselineBreak {
    pub season: u32,
    pub reason: &'static str,
    pub old_baseline_min: u32,
    pub new_baseline_min: u32,
}

pub struct BaselineBook {
    /// (季, 基线分钟) 有序——断点显式在册。
    points: alloc::vec::Vec<(u32, u32)>,
    breaks: alloc::vec::Vec<BaselineBreak>,
}

impl BaselineBook {
    pub fn new(first_season: u32, baseline_min: u32) -> BaselineBook {
        BaselineBook { points: alloc::vec![(first_season, baseline_min)], breaks: alloc::vec::Vec::new() }
    }

    /// 重置：断点记录 + 新基线追加（旧数据保留在册——历史不重写）。
    pub fn reset(&mut self, b: BaselineBreak) -> Result<(), &'static str> {
        if b.reason.is_empty() {
            return Err("重置必带理由：断点不明 = 口径漂移");
        }
        if b.new_baseline_min == 0 {
            return Err("新基线零值无效");
        }
        self.breaks.push(b.clone_shim(b.reason));
        self.points.push((b.season, b.new_baseline_min));
        Ok(())
    }

    pub fn latest_baseline(&self) -> (u32, u32) {
        *self.points.last().expect("至少一个基线")
    }

    /// 断点数（季报口径说明的依据）。
    pub fn break_count(&self) -> usize {
        self.breaks.len()
    }
}

impl BaselineBreak {
    fn clone_shim(&self, _reason: &'static str) -> BaselineBreak {
        BaselineBreak {
            season: self.season,
            reason: self.reason,
            old_baseline_min: self.old_baseline_min,
            new_baseline_min: self.new_baseline_min,
        }
    }
}

// ---------------------------------------------------------------------------
// 逐季曲线与回退红标（外部志愿者口径——自测降级不可比）
// ---------------------------------------------------------------------------

/// 回退判定：同基线段内，本季耗时 > 上季耗时（且均为外部口径）→ 回退红。
pub fn regression_red(this_min: u32, prev_min: u32, both_external: bool, same_baseline: bool) -> bool {
    both_external && same_baseline && this_min > prev_min
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F150E_TAG: &str = "stareco-F150-deep2";

pub fn run_f150_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F150E_TAG);

    // 分段计时
    let timings = alloc::vec![
        StepTiming { step: Step::FindSpec, minutes: 4 },
        StepTiming { step: Step::PackApp, minutes: 9 },
        StepTiming { step: Step::SubmitCase, minutes: 11 },
        StepTiming { step: Step::CheckStatus, minutes: 4 },
    ];
    set.add("f150e no timeout", timeout_steps(&timings).is_empty(), "四步全在预算内");
    let slow = alloc::vec![StepTiming { step: Step::PackApp, minutes: 15 }];
    set.add(
        "f150e timeout located",
        timeout_steps(&slow) == alloc::vec![Step::PackApp],
        "超时步定位",
    );
    set.add("f150e total budget", total_within_budget(&timings), "总 28 分钟 ≤ 30 线");
    let over = alloc::vec![StepTiming { step: Step::FindSpec, minutes: 31 }];
    set.add("f150e total over", !total_within_budget(&over), "单步爆总账");
    set.add(
        "f150e share",
        step_share_bp(&StepTiming { step: Step::PackApp, minutes: 10 }, 30) == 3_333,
        "步占比万分比",
    );

    // 画像分组
    set.add(
        "f150e tiers",
        tier_of(0) == ExperienceTier::Novice && tier_of(3) == ExperienceTier::Junior && tier_of(9) == ExperienceTier::Senior,
        "三档年限分组",
    );
    set.add(
        "f150e comparable same",
        comparable((ExperienceTier::Junior, 2), (ExperienceTier::Junior, 2)),
        "同档同代次可比",
    );
    set.add(
        "f150e incomparable",
        !comparable((ExperienceTier::Novice, 2), (ExperienceTier::Senior, 2)),
        "跨档不可比",
    );

    // 回流工单
    let mut flow = BacklogFlow::new();
    let t1 = flow.raise(StuckKind::DocMissing, 111).expect("t1");
    let t2 = flow.raise(StuckKind::ToolBug, 222).expect("t2");
    set.add(
        "f150e targets",
        t1 == BacklogTarget::Docs && t2 == BacklogTarget::Tooling,
        "卡壳分类直连目标域",
    );
    set.add("f150e close one", flow.confirm_closed(111).is_ok() && flow.close_rate_bp() == 5_000, "一单一闭环 50%");
    set.add("f150e close dup", flow.confirm_closed(111).is_err(), "重复闭环拒绝");
    set.add("f150e close unknown", flow.confirm_closed(999).is_err(), "未知工单拒绝");
    set.add("f150e raise zero fp", flow.raise(StuckKind::ProcessWinding, 0).is_err(), "零指纹拒收");
    let empty_flow = BacklogFlow::new();
    set.add("f150e no stuck full", empty_flow.close_rate_bp() == 10_000, "无卡壳 = 满闭环");

    // 基线断点
    let mut book = BaselineBook::new(1, 30);
    set.add("f150e baseline first", book.latest_baseline() == (1, 30), "首季基线");
    let _ = book.reset(BaselineBreak { season: 3, reason: "四步定义细化", old_baseline_min: 30, new_baseline_min: 26 });
    set.add("f150e baseline reset", book.latest_baseline() == (3, 26), "重置生效");
    set.add("f150e break recorded", book.break_count() == 1, "断点在册");
    set.add(
        "f150e reset no reason",
        book.reset(BaselineBreak { season: 4, reason: "", old_baseline_min: 26, new_baseline_min: 20 }).is_err(),
        "无理由重置拒绝",
    );

    // 回退红标
    set.add(
        "f150e regression red",
        regression_red(28, 26, true, true),
        "同基线外部口径变慢 = 红",
    );
    set.add(
        "f150e not regression",
        !regression_red(28, 26, false, true),
        "自测口径不可比（不判红）",
    );
    set.add(
        "f150e baseline change exempt",
        !regression_red(28, 26, true, false),
        "断点后不比",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn step_limits_sane() {
        // 四步预算 5+10+10+5 = 30（主册口径）
        let total: u32 = [Step::FindSpec, Step::PackApp, Step::SubmitCase, Step::CheckStatus]
            .iter()
            .map(|s| s.limit_min())
            .sum();
        assert_eq!(total, TOTAL_BUDGET_MIN);
    }

    #[test]
    fn backlog_all_targets() {
        assert_eq!(backlog_target(StuckKind::DocMissing), BacklogTarget::Docs);
        assert_eq!(backlog_target(StuckKind::ToolBug), BacklogTarget::Tooling);
        assert_eq!(backlog_target(StuckKind::ProcessWinding), BacklogTarget::Process);
    }

    #[test]
    fn close_rate_full() {
        let mut f = BacklogFlow::new();
        let _ = f.raise(StuckKind::ToolBug, 5);
        let _ = f.raise(StuckKind::ToolBug, 6);
        let _ = f.confirm_closed(5);
        let _ = f.confirm_closed(6);
        assert_eq!(f.close_rate_bp(), 10_000);
    }
}
