//! 深化层三 · F150 生态域总判据（2026-09-26 深化批次三）。
//!
//! 补深计时与统计工程面（主册 G-D-25）：四步分段计时核（逐段预算
//! 5/10/10/5 分钟）、可比性分层统计（外部志愿者+同代次才可比）、
//! 回退门（逐季不回退——超基线即红名单）、卡壳回流工单生成器。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 四步分段计时：预算 5/10/10/5（总 30 分钟线）
// ---------------------------------------------------------------------------

pub const STEP_BUDGETS_MIN: [u32; 4] = [5, 10, 10, 5];

pub struct StepTimer {
    /// 各步实际用时（分钟）。
    pub actuals: [Option<u32>; 4],
}

impl StepTimer {
    pub fn new() -> StepTimer {
        StepTimer { actuals: [None; 4] }
    }

    pub fn record(&mut self, step: usize, minutes: u32) -> Result<(), &'static str> {
        if step >= 4 {
            return Err("步序越界：四步制");
        }
        if self.actuals[step].is_some() {
            return Err("重复计时：同步只记一次");
        }
        self.actuals[step] = Some(minutes);
        Ok(())
    }

    /// 逐步裁决：(步序, 达标)。未记满四步 → Err（不完整不评）。
    pub fn verdict(&self) -> Result<alloc::vec::Vec<(usize, bool)>, &'static str> {
        if self.actuals.iter().any(|a| a.is_none()) {
            return Err("计时不完整：四步未记满");
        }
        Ok(self
            .actuals
            .iter()
            .zip(STEP_BUDGETS_MIN.iter())
            .enumerate()
            .map(|(i, (a, b))| (i, a.unwrap_or(0) <= *b))
            .collect())
    }

    pub fn total_min(&self) -> u32 {
        self.actuals.iter().map(|a| a.unwrap_or(0)).sum()
    }

    pub fn within_30(&self) -> bool {
        self.actuals.iter().all(|a| a.is_some()) && self.total_min() <= 30
    }
}

// ---------------------------------------------------------------------------
// 可比性分层：外部志愿者 + 同基线代次才可比（AI 自测降级不可比）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Cohort {
    pub external: bool,
    pub baseline_generation: u32,
}

/// 可比：双方均为外部志愿者且代次一致。
pub fn comparable(a: Cohort, b: Cohort) -> bool {
    a.external && b.external && a.baseline_generation == b.baseline_generation
}

// ---------------------------------------------------------------------------
// 回退门：本季各步 vs 基线各步——任一步显著变慢（>10%）入红名单
// ---------------------------------------------------------------------------

/// 红名单：actual > baseline * 110 / 100 的步序。
pub fn regression_steps(baseline: &[u32; 4], actual: &[u32; 4]) -> alloc::vec::Vec<usize> {
    baseline
        .iter()
        .zip(actual.iter())
        .enumerate()
        .filter(|(_, (b, a))| **a > **b * 110 / 100)
        .map(|(i, _)| i)
        .collect()
}

/// 逐季不回退判据：红名单为空。
pub fn no_regression(baseline: &[u32; 4], actual: &[u32; 4]) -> bool {
    regression_steps(baseline, actual).is_empty()
}

// ---------------------------------------------------------------------------
// 卡壳回流工单：卡壳分类 → 工单字段（类别/下一步动作）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StuckKind {
    DocMissing,
    ToolError,
    FlowDetour,
}

pub struct BacklogTicket {
    pub kind: StuckKind,
    /// 工单标题（静态模板——机器生成不虚构事实）。
    pub title: &'static str,
    pub next_action: &'static str,
}

pub fn ticket_for(kind: StuckKind) -> BacklogTicket {
    match kind {
        StuckKind::DocMissing => BacklogTicket {
            kind,
            title: "文档缺：流程找不到入口",
            next_action: "定位缺失页 → 补文档 → 回流重测",
        },
        StuckKind::ToolError => BacklogTicket {
            kind,
            title: "工具错：工具行为与手册不符",
            next_action: "复现最小样本 → 归因工具缺陷 → 修工具",
        },
        StuckKind::FlowDetour => BacklogTicket {
            kind,
            title: "流程绕：正规路径走不通",
            next_action: "记录绕行路径 → 收敛为正规路径 → 更新四步表",
        },
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F150F_TAG: &str = "stareco-F150-deep3";

pub fn run_f150_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F150F_TAG);

    // 计时核
    let mut t = StepTimer::new();
    set.add("f150f incomplete", t.verdict().is_err(), "未记满不评");
    set.add("f150f step bound", t.record(4, 1).is_err(), "步序越界拒");
    for (i, m) in [4u32, 9, 8, 5].iter().enumerate() {
        let _ = t.record(i, *m);
    }
    set.add("f150f dup record", t.record(0, 3).is_err(), "重复计时拒");
    let v = t.verdict().expect("complete");
    set.add("f150f all pass", v.iter().all(|(_, ok)| *ok), "四步全部达标");
    set.add("f150f within 30", t.within_30() && t.total_min() == 26, "26 分钟 30 分线内");

    let mut t2 = StepTimer::new();
    for (i, m) in [4u32, 12, 8, 5].iter().enumerate() {
        let _ = t2.record(i, *m);
    }
    let v2 = t2.verdict().expect("complete");
    set.add(
        "f150f step2 over",
        v2.iter().find(|(i, _)| *i == 1).map(|(_, ok)| !ok).unwrap_or(false),
        "第二步 12>10 红",
    );
    set.add("f150f over 30 total", t2.total_min() == 29 && t2.within_30(), "29 仍在 30 线内");

    // 可比性
    let ext_g1 = Cohort { external: true, baseline_generation: 1 };
    let ext_g2 = Cohort { external: true, baseline_generation: 2 };
    let ai_g1 = Cohort { external: false, baseline_generation: 1 };
    set.add("f150f comparable ok", comparable(ext_g1, ext_g1), "外部同代可比");
    set.add("f150f comparable gen", !comparable(ext_g1, ext_g2), "异代不可比");
    set.add("f150f comparable ai", !comparable(ext_g1, ai_g1), "AI 自测降级不可比");

    // 回退门
    let base = [5u32, 10, 10, 5];
    set.add(
        "f150f no regress",
        no_regression(&base, &[5, 10, 9, 5]),
        "持平或变快零红",
    );
    set.add(
        "f150f regress list",
        regression_steps(&base, &[6, 12, 10, 4]) == alloc::vec![0, 1],
        "6>5 越线、12>11 越线——两步入红名单",
    );
    set.add(
        "f150f boundary 110",
        regression_steps(&base, &[5, 11, 10, 5]).is_empty(),
        "恰 110% 不算回退",
    );

    // 工单
    let doc = ticket_for(StuckKind::DocMissing);
    let tool = ticket_for(StuckKind::ToolError);
    let flow = ticket_for(StuckKind::FlowDetour);
    set.add("f150f ticket doc", doc.title.contains("文档缺"), "文档缺工单");
    set.add("f150f ticket tool", tool.next_action.contains("工具缺陷"), "工具错工单");
    set.add("f150f ticket flow", flow.next_action.contains("正规路径"), "流程绕工单");
    set.add(
        "f150f ticket kinds distinct",
        doc.title != tool.title && tool.title != flow.title && doc.title != flow.title,
        "三分类标题互异",
    );

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn regression_math() {
        let base = [5u32, 10, 10, 5];
        // 110% 线：5→5.5→整数域 6 才越线；10→11→整数域 12 才越线。
        assert!(regression_steps(&base, &[6, 11, 10, 5]) == alloc::vec![0]);
        assert!(regression_steps(&base, &[5, 12, 10, 5]) == alloc::vec![1]);
        assert!(regression_steps(&base, &[5, 10, 10, 6]) == alloc::vec![3]);
    }

    #[test]
    fn timer_order_independent_records() {
        let mut t = StepTimer::new();
        let _ = t.record(2, 7);
        let _ = t.record(0, 4);
        assert!(t.verdict().is_err()); // 1、3 未记
        let _ = t.record(1, 10);
        let _ = t.record(3, 5);
        assert!(t.verdict().is_ok());
        assert_eq!(t.total_min(), 26);
    }
}
