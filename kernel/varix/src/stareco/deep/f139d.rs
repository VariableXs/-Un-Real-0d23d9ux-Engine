//! 深化层 · F139 反馈闭环通道（2026-09-26 回炉补深化）。
//!
//! 补深：报告编号查询页（编号→五态+回链）、状态变更通知可选模型、
//! 季度最佳报告评选（复现质量分）、重复报告相似哈希聚类阈值、
//! 版本指纹比对（修复版是否覆盖报告环境）。

use crate::checks::CheckSet;
use crate::stareco::ebase::State5;
use crate::stareco::feedbackloop::{FeedbackReport, FeedbackLoop};

// ---------------------------------------------------------------------------
// 查询页数据源
// ---------------------------------------------------------------------------

pub struct StatusView {
    pub state: State5,
    pub fixed: bool,
}

/// 编号 → 状态视图（回查式为主——不堆通知）。
pub fn status_view(loop_: &FeedbackLoop, id: crate::stareco::ebase::TraceId) -> Option<StatusView> {
    loop_.report_view(id).map(|r| StatusView { state: r.state(), fixed: r.fix_fp != 0 })
}

// ---------------------------------------------------------------------------
// 通知可选（回查为主，通知为辅——默认关）
// ---------------------------------------------------------------------------

pub struct NotifyPref {
    pub on_state_change: bool,
}

impl NotifyPref {
    /// 通知决策：仅在用户显式开启且状态真实推进时发。
    pub fn should_notify(&self, changed: bool) -> bool {
        self.on_state_change && changed
    }
}

// ---------------------------------------------------------------------------
// 季度最佳报告（复现质量分）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReportScore {
    pub id: crate::stareco::ebase::TraceId,
    /// 复现步骤完整度（0-3：现象/步骤/环境三段+最小复现）。
    pub repro_quality: u8,
    /// 是否被确认复现。
    pub confirmed: bool,
}

/// 最佳报告 = 确认复现优先，其次复现质量分。
pub fn best_report(scores: &[ReportScore]) -> Option<ReportScore> {
    scores
        .iter()
        .copied()
        .max_by(|a, b| (a.confirmed, a.repro_quality).cmp(&(b.confirmed, b.repro_quality)))
}

// ---------------------------------------------------------------------------
// 相似哈希聚类阈值
// ---------------------------------------------------------------------------

/// 相似判定：现象指纹一致 + 环境指纹一致（同环境同现象才聚类——
/// 不同环境的相同现象是独立线索，不并簇）。
pub fn same_cluster(sym_fp_a: u64, env_a: u64, sym_fp_b: u64, env_b: u64) -> bool {
    sym_fp_a == sym_fp_b && env_a == env_b
}

// ---------------------------------------------------------------------------
// 修复版覆盖比对
// ---------------------------------------------------------------------------

/// 修复版指纹是否覆盖报告环境：修复版环境指纹的后 16 位与报告一致
/// 视为覆盖（版本段口径——完整指纹比对由 F130 版本册负责）。
pub fn fix_covers(report_env_fp: u64, fix_env_fp: u64) -> bool {
    (report_env_fp & 0xFFFF) == (fix_env_fp & 0xFFFF)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F139D_TAG: &str = "stareco-F139-deep";

pub fn run_f139_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F139D_TAG);

    // 端到端：提交 → 查询视图 → 推进 → 回链
    let mut fb = FeedbackLoop::new();
    let r = FeedbackReport { symptom: "标题闪烁", repro: "长路径输入", environment_fp: 0x1234, attachment: "" };
    let id = fb.submit(20260926, 0, &r).expect("submit");
    let v0 = status_view(&fb, id).expect("view");
    set.add(
        "f139d status view initial",
        v0.state == State5::Submitted && !v0.fixed,
        "查得到态",
    );
    fb.update_state(id, 20260927).ok();
    fb.link_fix(id, 0xBEEF).ok();
    let v1 = status_view(&fb, id).expect("view");
    set.add(
        "f139d status view after flow",
        v1.state == State5::Confirmed && v1.fixed,
        "推进+回链可见",
    );
    set.add("f139d unknown id none", status_view(&fb, crate::stareco::ebase::TraceId::new("FB", 20260926, 99)).is_none(), "诚实无此编号");

    // 通知可选
    let off = NotifyPref { on_state_change: false };
    let on = NotifyPref { on_state_change: true };
    set.add(
        "f139d notify opt-in only",
        !off.should_notify(true) && !on.should_notify(false) && on.should_notify(true),
        "回查为主",
    );

    // 最佳报告
    let scores = [
        ReportScore { id, repro_quality: 3, confirmed: false },
        ReportScore { id, repro_quality: 1, confirmed: true },
        ReportScore { id, repro_quality: 2, confirmed: false },
    ];
    let best = best_report(&scores).expect("some");
    set.add(
        "f139d best = confirmed first",
        best.confirmed && best.repro_quality == 1,
        "确认复现优先于分数",
    );

    // 聚类阈值
    set.add(
        "f139d cluster needs both match",
        same_cluster(1, 2, 1, 2) && !same_cluster(1, 2, 1, 3) && !same_cluster(1, 2, 9, 2),
        "环境不同不并簇",
    );

    // 修复覆盖
    set.add(
        "f139d fix coverage segment",
        fix_covers(0x1234_5678, 0xABCD_5678) && !fix_covers(0x1234_5678, 0xABCD_9999),
        "版本段口径",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn view_lifecycle() {
        let mut fb = FeedbackLoop::new();
        let r = FeedbackReport { symptom: "s", repro: "r", environment_fp: 1, attachment: "" };
        let id = fb.submit(20260101, 0, &r).unwrap();
        assert_eq!(status_view(&fb, id).unwrap().state, State5::Submitted);
    }
}
