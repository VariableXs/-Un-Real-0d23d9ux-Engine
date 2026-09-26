//! 深化层四 · F150 生态域总判据（2026-09-27 深化批次四 · g 层）。
//!
//! 综合评分卡（人群×代次×四步网格）、任务库版本 diff、新人引导流
//! （可跳过步骤与完成门）、全域完成度仪表盘数据、回归看板聚合。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 综合评分卡：每行 (人群, 代次, 四步实际) → 该人群达标判定
// ---------------------------------------------------------------------------

pub struct ScoreRow {
    pub cohort: u8,
    pub generation: u32,
    pub actuals: [u32; 4],
}

/// 预算复用 f150f 常量口径（一处一事实：5/10/10/5）。
pub fn row_pass(r: &ScoreRow) -> bool {
    const BUDGETS: [u32; 4] = [5, 10, 10, 5];
    r.actuals.iter().zip(BUDGETS.iter()).all(|(a, b)| a <= b)
}

/// 评分卡：可比人群（同代次）中达标率千分比。
pub fn card_pass_per_mille(rows: &[ScoreRow], generation: u32) -> u32 {
    let sel: alloc::vec::Vec<&ScoreRow> =
        rows.iter().filter(|r| r.generation == generation).collect();
    if sel.is_empty() {
        return 0;
    }
    let passed = sel.iter().filter(|r| row_pass(r)).count();
    passed as u32 * 1000 / sel.len() as u32
}

// ---------------------------------------------------------------------------
// 任务库版本 diff：任务项 (id, 内容指纹) 增/删/改
// ---------------------------------------------------------------------------

pub struct BankDiff {
    pub added: alloc::vec::Vec<u32>,
    pub removed: alloc::vec::Vec<u32>,
    pub changed: alloc::vec::Vec<u32>,
}

pub fn bank_diff(
    old: &[(u32, u64)],
    new: &[(u32, u64)],
) -> BankDiff {
    let mut d = BankDiff { added: alloc::vec::Vec::new(), removed: alloc::vec::Vec::new(), changed: alloc::vec::Vec::new() };
    for (id, fp) in old {
        match new.iter().find(|(n, _)| n == id) {
            None => d.removed.push(*id),
            Some((_, fp2)) if fp2 != fp => d.changed.push(*id),
            _ => {}
        }
    }
    for (id, _) in new {
        if !old.iter().any(|o| o.0 == *id) {
            d.added.push(*id);
        }
    }
    d
}

/// 任务库纪律：版本间「改+删」合计不得超总数四成（任务稳定性线）。
pub fn bank_stable(d: &BankDiff, old_total: usize) -> bool {
    if old_total == 0 {
        return true;
    }
    (d.changed.len() + d.removed.len()) * 10 <= old_total * 4
}

// ---------------------------------------------------------------------------
// 新人引导流：步骤可跳过标记 + 完成门（不可跳步必须全走）
// ---------------------------------------------------------------------------

pub struct OnboardStep {
    pub name: &'static str,
    pub skippable: bool,
    pub done: bool,
}

/// 完成门：所有不可跳步骤已 done；可跳步骤允许 false。
pub fn onboard_complete(steps: &[OnboardStep]) -> bool {
    steps.iter().all(|s| s.skippable || s.done)
}

/// 必做清单（未完成的不可跳步骤点名）。
pub fn onboard_pending(steps: &[OnboardStep]) -> alloc::vec::Vec<&'static str> {
    steps.iter().filter(|s| !s.skippable && !s.done).map(|s| s.name).collect()
}

// ---------------------------------------------------------------------------
// 全域完成度仪表盘：每域 (行数, 目标) → 千分比 + 域名序渲染
// ---------------------------------------------------------------------------

pub fn completion_per_mille(actual: usize, target: usize) -> u32 {
    if target == 0 {
        return 0;
    }
    (actual as u64 * 1000 / target as u64).min(1000) as u32
}

// ---------------------------------------------------------------------------
// 回归看板：各季红名单聚合（季序 → 红项数）
// ---------------------------------------------------------------------------

pub fn regression_board(rows: &[(u32, usize)]) -> alloc::vec::Vec<(u32, usize)> {
    let mut agg: alloc::vec::Vec<(u32, usize)> = alloc::vec::Vec::new();
    for (q, n) in rows {
        match agg.iter_mut().find(|(a, _)| a == q) {
            Some(e) => e.1 += n,
            None => agg.push((*q, *n)),
        }
    }
    agg.sort_by_key(|(q, _)| *q);
    agg
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F150G_TAG: &str = "stareco-F150-deep4";

pub fn run_f150_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F150G_TAG);

    // 评分卡
    let rows = [
        ScoreRow { cohort: 1, generation: 3, actuals: [4, 9, 9, 5] },
        ScoreRow { cohort: 2, generation: 3, actuals: [6, 12, 9, 5] },
        ScoreRow { cohort: 3, generation: 2, actuals: [1, 1, 1, 1] },
    ];
    set.add("f150g row pass", row_pass(&rows[0]) && !row_pass(&rows[1]), "逐行四步判定");
    set.add("f150g card g3", card_pass_per_mille(&rows, 3) == 500, "同代达标率 500‰");
    set.add("f150g card g2", card_pass_per_mille(&rows, 2) == 1000, "异代不计入");
    set.add("f150g card none", card_pass_per_mille(&rows, 9) == 0, "无人群如实 0");

    // 任务库 diff
    let old = [(1u32, 10u64), (2, 20), (3, 30), (4, 40), (5, 50)];
    let new = [(1u32, 10), (2, 21), (3, 30), (5, 50)];
    let d = bank_diff(&old, &new);
    set.add(
        "f150g bank diff",
        d.added.is_empty() && d.removed == alloc::vec![4] && d.changed == alloc::vec![2],
        "增删改三向",
    );
    set.add("f150g bank stable", bank_stable(&d, 5), "改动 2/5 ≤ 四成");
    let unstable = BankDiff { added: alloc::vec::Vec::new(), removed: alloc::vec![1, 2, 3], changed: alloc::vec![4, 5] };
    set.add("f150g bank churn", !bank_stable(&unstable, 5), "大改版如实拦截");

    // 引导流
    let steps = [
        OnboardStep { name: "装系统", skippable: false, done: true },
        OnboardStep { name: "看欢迎", skippable: true, done: false },
        OnboardStep { name: "建还原点", skippable: false, done: false },
    ];
    set.add("f150g onboard pending", onboard_pending(&steps) == alloc::vec!["建还原点"], "必做未完点名");
    set.add("f150g onboard gate", !onboard_complete(&steps), "完成门未开");
    let all_done = [
        OnboardStep { name: "a", skippable: false, done: true },
        OnboardStep { name: "b", skippable: true, done: false },
    ];
    set.add("f150g onboard ok", onboard_complete(&all_done), "可跳步骤不算欠账");

    // 仪表盘
    set.add(
        "f150g dash",
        completion_per_mille(635, 1000) == 635 && completion_per_mille(0, 100) == 0,
        "完成度千分比直算",
    );
    set.add(
        "f150g dash cap",
        completion_per_mille(500, 100) == 1000,
        "超额封顶",
    );

    // 回归看板
    let board = regression_board(&[(3, 1), (1, 2), (3, 1)]);
    set.add(
        "f150g board",
        board == alloc::vec![(1, 2), (3, 2)],
        "季度聚合升序",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn scorecard_budget_boundary() {
        let exact = ScoreRow { cohort: 1, generation: 1, actuals: [5, 10, 10, 5] };
        assert!(row_pass(&exact)); // 恰达预算放行
        let over = ScoreRow { cohort: 1, generation: 1, actuals: [6, 10, 10, 5] };
        assert!(!row_pass(&over));
    }

    #[test]
    fn bank_add_only_stable() {
        let d = BankDiff { added: alloc::vec![9], removed: alloc::vec![], changed: alloc::vec![] };
        assert!(bank_stable(&d, 5)); // 纯新增不算扰动
    }
}
