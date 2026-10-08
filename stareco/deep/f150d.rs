//! 深化层 · F150 生态域总判据（2026-09-26 回炉补深化）。
//!
//! 补深：计时记录归档（逐次明细账）、卡壳点汇总→回流清单（分给
//! F135 文档/F127 工具/流程三方）、志愿者画像册（可比性分桶）、
//! 流程检查单模板（公开）。

use crate::checks::CheckSet;
use crate::stareco::ecogate::{Step, StuckKind, Walkthrough, TOTAL_BUDGET_MIN};

// ---------------------------------------------------------------------------
// 计时记录归档（逐次明细账）
// ---------------------------------------------------------------------------

pub struct WalkRecord {
    pub season: u32,
    pub volunteer_years: u8,
    pub external: bool,
    pub total_minutes: u32,
    pub stuck_count: usize,
}

pub struct WalkArchive {
    records: alloc::vec::Vec<WalkRecord>,
}

impl WalkArchive {
    pub fn new() -> WalkArchive {
        WalkArchive { records: alloc::vec::Vec::new() }
    }

    pub fn record(&mut self, w: &Walkthrough, season: u32) {
        self.records.push(WalkRecord {
            season,
            volunteer_years: w.volunteer_years,
            external: w.external_volunteer,
            total_minutes: w.total_minutes(),
            stuck_count: w.stuck_view().len(),
        });
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 某季平均耗时（仅外部志愿者口径——AI01 自测不入均值）。
    pub fn season_avg_external(&self, season: u32) -> Option<u32> {
        let vals: alloc::vec::Vec<u32> = self
            .records
            .iter()
            .filter(|r| r.season == season && r.external)
            .map(|r| r.total_minutes)
            .collect();
        if vals.is_empty() {
            return None;
        }
        Some(vals.iter().sum::<u32>() / vals.len() as u32)
    }
}

// ---------------------------------------------------------------------------
// 卡壳点汇总 → 回流清单
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StuckEntry {
    pub step: Step,
    pub kind: StuckKind,
}

/// 回流目的地：文档缺→F135；工具错→F127 工具族；流程绕→流程修订。
pub fn reflow_target(kind: StuckKind) -> &'static str {
    match kind {
        StuckKind::DocMissing => "F135-docs",
        StuckKind::ToolBug => "F127-toolchain",
        StuckKind::ProcessWinding => "process-review",
    }
}

/// 汇总多轮走查的卡壳点，按目的地分组计数（回流工作清单）。
pub fn reflow_summary(all: &[StuckEntry]) -> [(&'static str, usize); 3] {
    let mut doc = 0;
    let mut tool = 0;
    let mut proc = 0;
    for e in all {
        match e.kind {
            StuckKind::DocMissing => doc += 1,
            StuckKind::ToolBug => tool += 1,
            StuckKind::ProcessWinding => proc += 1,
        }
    }
    [("F135-docs", doc), ("F127-toolchain", tool), ("process-review", proc)]
}

// ---------------------------------------------------------------------------
// 志愿者画像册（可比性分桶）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ExperienceBucket {
    Student,
    Junior,
    Mid,
    Senior,
}

/// 年限分桶：0-2 学生 / 3-5 初级 / 6-9 中级 / 10+ 高级。
pub fn bucket_of(years: u8) -> ExperienceBucket {
    match years {
        0..=2 => ExperienceBucket::Student,
        3..=5 => ExperienceBucket::Junior,
        6..=9 => ExperienceBucket::Mid,
        _ => ExperienceBucket::Senior,
    }
}

// ---------------------------------------------------------------------------
// 流程检查单模板（公开）
// ---------------------------------------------------------------------------

pub const WALKTHROUGH_CHECKLIST: [&str; 6] = [
    "①打开 F126 规范页（秒表起）",
    "②按规范打 vxapp 包",
    "③按 F129 流程提交判例",
    "④用编号查处理状态",
    "⑤逐步记录卡壳点与分级",
    "⑥记录志愿者年限与是否外部",
];

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F150D_TAG: &str = "stareco-F150-deep";

pub fn run_f150_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F150D_TAG);

    // 完整走查 → 归档 → 季均值
    let mut w = Walkthrough::new(true, 4, 1);
    for (i, s) in Step::ALL.iter().enumerate() {
        w.complete(*s, [4, 9, 8, 4][i]).expect("step");
    }
    let mut arch = WalkArchive::new();
    arch.record(&w, 1);
    arch.record(&w, 1);
    set.add(
        "f150d archive and season avg",
        arch.len() == 2 && arch.season_avg_external(1) == Some(w.total_minutes()),
        "外部口径均值",
    );
    let mut self_test = Walkthrough::new(false, 9, 1);
    for s in Step::ALL.iter() {
        self_test.complete(*s, s.limit_min()).ok();
    }
    arch.record(&self_test, 1);
    set.add(
        "f150d selftest excluded from avg",
        arch.season_avg_external(1) == Some(w.total_minutes()),
        "AI01 自测不污染均值",
    );
    set.add("f150d empty season none", arch.season_avg_external(9).is_none(), "无数据诚实 None");

    // 回流清单
    let stuck = [
        StuckEntry { step: Step::PackApp, kind: StuckKind::DocMissing },
        StuckEntry { step: Step::PackApp, kind: StuckKind::DocMissing },
        StuckEntry { step: Step::SubmitCase, kind: StuckKind::ToolBug },
        StuckEntry { step: Step::FindSpec, kind: StuckKind::ProcessWinding },
    ];
    let summary = reflow_summary(&stuck);
    set.add(
        "f150d reflow grouped counts",
        summary == [("F135-docs", 2usize), ("F127-toolchain", 1), ("process-review", 1)],
        "按目的地分组",
    );
    set.add(
        "f150d reflow targets",
        reflow_target(StuckKind::DocMissing) == "F135-docs"
            && reflow_target(StuckKind::ToolBug) == "F127-toolchain"
            && reflow_target(StuckKind::ProcessWinding) == "process-review",
        "三类三目的地",
    );

    // 志愿者分桶
    set.add(
        "f150d experience buckets",
        bucket_of(1) == ExperienceBucket::Student
            && bucket_of(4) == ExperienceBucket::Junior
            && bucket_of(7) == ExperienceBucket::Mid
            && bucket_of(15) == ExperienceBucket::Senior,
        "四桶可比",
    );

    // 检查单
    set.add(
        "f150d checklist six steps",
        WALKTHROUGH_CHECKLIST.len() == 6 && WALKTHROUGH_CHECKLIST.iter().all(|s| !s.is_empty()),
        "模板公开",
    );

    // 总预算复诵
    set.add("f150d budget 30", TOTAL_BUDGET_MIN == 30, "30 分钟线");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn reflow_empty() {
        let s = reflow_summary(&[]);
        assert!(s.iter().all(|(_, n)| *n == 0));
    }
}
