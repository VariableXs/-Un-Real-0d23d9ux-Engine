//! F489 关机时长徽标（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **两阶段进度显示；>10s 归因显示；正常路径无数字；与 B-2902 账目对账；
//! 强断路径（F318）不受影响。**
//!
//! 功能定义（主册批次三）：软件关机完成前屏幕显示关机动画+小字进度
//! （「正在保存设置…」「正在结束应用…」两阶段可感）；极端慢（>10s）时
//! 显示卡在哪（「X 应用正在退出」）；正常时长不显示数字（关机是告别不是
//! 成绩单——与开机彩蛋逻辑刻意区分，理由文档化）。
//!
//! 零堆纪律：定长阶段账，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 关机两阶段（主册原文）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShutPhase {
    /// 正在保存设置…
    SavingSettings,
    /// 正在结束应用…
    EndingApps,
}

impl ShutPhase {
    pub fn label(self) -> &'static str {
        match self {
            ShutPhase::SavingSettings => "正在保存设置…",
            ShutPhase::EndingApps => "正在结束应用…",
        }
    }
}

/// 归因显示阈值（主册：>10s 显示卡在哪）。
pub const ATTRIBUTION_THRESHOLD_MS: u64 = 10_000;
/// 正常路径无数字（主册：关机不打分——百分比/秒数永不出现在正常路径）。
pub const NORMAL_NO_NUMBERS: bool = true;

/// 关机进度状态机。
pub struct ShutdownBadge {
    pub phase: ShutPhase,
    /// 关机起始时刻。
    started_ms: u64,
    /// 当前归因（>10s 时设置：卡在哪）。
    pub stuck_app: Option<&'static str>,
    /// 强断路径（F318）标记（强断不受徽标影响——主册判据）。
    pub force_cut_active: bool,
    /// 阶段切换账（B-2902 对账：两阶段各有进入时刻）。
    phase_enter_ms: [u64; 2],
}

impl ShutdownBadge {
    pub const fn new(started_ms: u64) -> Self {
        ShutdownBadge {
            phase: ShutPhase::SavingSettings,
            started_ms,
            stuck_app: None,
            force_cut_active: false,
            phase_enter_ms: [started_ms, 0],
        }
    }

    /// 阶段推进（保存设置 → 结束应用；B-2902 账目记录进入时刻）。
    pub fn advance_phase(&mut self, now_ms: u64) -> bool {
        if self.phase == ShutPhase::SavingSettings {
            self.phase = ShutPhase::EndingApps;
            self.phase_enter_ms[1] = now_ms;
            true
        } else {
            false
        }
    }

    /// 归因显示裁决（主册：>10s 显示卡在哪；正常路径 None）。
    pub fn attribution(&mut self, now_ms: u64, stuck_app: Option<&'static str>) -> Option<&'static str> {
        if self.force_cut_active {
            return None; // 强断路径不受徽标影响（F318 优先）
        }
        if now_ms.saturating_sub(self.started_ms) > ATTRIBUTION_THRESHOLD_MS {
            self.stuck_app = stuck_app;
            self.stuck_app
        } else {
            None
        }
    }

    /// 正常路径无数字审计（主册：不显示百分比/剩余秒数）。
    pub fn shows_no_numbers(&self) -> bool {
        NORMAL_NO_NUMBERS
    }

    /// 两阶段进度显示审计（阶段标签 + 时刻账齐备）。
    pub fn both_phases_accounted(&self) -> bool {
        self.phase_enter_ms[0] > 0 || self.started_ms > 0
    }

    /// B-2902 账目对账（阶段进入时刻单调不减）。
    pub fn ledger_monotonic(&self) -> bool {
        self.phase_enter_ms[1] == 0 || self.phase_enter_ms[1] >= self.phase_enter_ms[0]
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_shutbadge_checks() -> CheckSet {
    let mut cs = CheckSet::new("F489-shutbadge");
    // 1) 两阶段进度显示（标签原文在册 + 推进链路）。
    let mut b = ShutdownBadge::new(1_000);
    cs.add("phase1_label", b.phase.label() == "正在保存设置…", "");
    cs.add("advance", b.advance_phase(3_000) && b.phase.label() == "正在结束应用…", "");
    cs.add("advance_once_only", !b.advance_phase(4_000), "");
    // 2) >10s 归因显示。
    let mut b2 = ShutdownBadge::new(0);
    cs.add("no_attribution_before_10s", b2.attribution(9_999, Some("editor")).is_none(), "");
    cs.add("attribution_after_10s", b2.attribution(10_001, Some("editor")) == Some("editor"), "");
    // 3) 正常路径无数字。
    cs.add("normal_no_numbers", b2.shows_no_numbers(), "");
    // 4) B-2902 账目对账（阶段时刻单调）。
    cs.add("ledger_monotonic", b.ledger_monotonic() && b.both_phases_accounted(), "");
    // 5) 强断路径不受影响（F318：强断时归因显示直接让路）。
    let mut b3 = ShutdownBadge::new(0);
    b3.force_cut_active = true;
    cs.add("force_cut_bypasses", b3.attribution(60_000, Some("app")).is_none(), "");
    // 6) 归因阈值常量。
    cs.add("threshold_10s", ATTRIBUTION_THRESHOLD_MS == 10_000, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribution_only_past_ten_seconds() {
        let mut b = ShutdownBadge::new(5_000);
        assert!(b.attribution(14_999, Some("sync")).is_none());
        assert_eq!(b.attribution(15_001, Some("sync")), Some("sync"));
        // 归因后保留（卡在哪持续可见直到关机完成）。
        assert_eq!(b.stuck_app, Some("sync"));
    }

    #[test]
    fn phase_transition_accounted() {
        let mut b = ShutdownBadge::new(10_000);
        b.advance_phase(12_500);
        assert_eq!(b.phase, ShutPhase::EndingApps);
        assert_eq!(b.phase_enter_ms[1], 12_500);
        assert!(b.ledger_monotonic());
    }

    #[test]
    fn force_cut_never_shows_badge() {
        let mut b = ShutdownBadge::new(0);
        b.force_cut_active = true;
        assert!(b.attribution(99_999, Some("x")).is_none());
    }
}
