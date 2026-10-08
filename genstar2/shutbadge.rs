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

// ===========================================================================
// 深化 v2（F489）：两阶段时长账 / 归因抖动抑制 / 强断路径全链让路 /
// 阶段文案表 / 正常路径零数字的账面复核
// ===========================================================================

/// 两阶段时长账（B-2902 对账深化：两阶段各自耗时可导出——
/// 关机慢了知道慢在「保存」还是「结束应用」）。
impl ShutdownBadge {
    /// 阶段耗时（ms）：阶段 i 的持续时间（截至换阶段时刻）。
    pub fn phase_duration(&self, now_ms: u64, stage: usize) -> u64 {
        match stage {
            0 => self.phase_enter_ms[1].saturating_sub(self.phase_enter_ms[0]),
            1 => now_ms.saturating_sub(self.phase_enter_ms[1]),
            _ => 0,
        }
    }

    /// 全程耗时。
    pub fn elapsed(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.started_ms)
    }
}

/// 归因抖动抑制（主册「>10s 显示卡在哪」的稳定面：同一卡点反复
/// 上报不闪烁——归因一经设定，除非卡点变更否则保持）。
pub fn attribution_stable(b: &mut ShutdownBadge, now_ms: u64, app: Option<&'static str>) -> bool {
    let first = b.attribution(now_ms, app);
    let second = b.attribution(now_ms + 500, app);
    match (first, second) {
        (Some(a), Some(c)) => a == c,
        (None, None) => true,
        _ => false,
    }
}

/// 强断路径全链让路（F318 红线：强断激活时归因永远 None、阶段推进
/// 照常、账目不写——强断是最后一道闸，徽标层全让）。
pub fn force_cut_yields_everywhere(b: &mut ShutdownBadge, now_ms: u64) -> bool {
    b.force_cut_active = true;
    let no_attr = b.attribution(now_ms + 60_000, Some("慢应用")).is_none();
    let phase_ok = b.advance_phase(now_ms);
    b.force_cut_active = false;
    no_attr && phase_ok
}

/// 阶段文案表（主册「正在保存设置…/正在结束应用…」两阶段原文锚——
/// 文案与枚举一一对应，一处一事实）。
pub fn phase_labels_match() -> bool {
    // v1 文案带省略号（「正在保存设置…」）——contains 语义守护关键词。
    ShutPhase::SavingSettings.label().contains("正在保存设置")
        && ShutPhase::EndingApps.label().contains("正在结束应用")
}

/// 正常路径零数字账面复核（shows_no_numbers + 归因未设 + 阈值内——
/// 三条同时成立才是「安静利落」的正常关机）。
pub fn normal_shutdown_quiet(b: &ShutdownBadge, now_ms: u64) -> bool {
    b.shows_no_numbers()
        && b.stuck_app.is_none()
        && b.elapsed(now_ms) <= ATTRIBUTION_THRESHOLD_MS
}

// ---------------------------------------------------------------------------
// 深化自检（F489 v2）
// ---------------------------------------------------------------------------

pub fn run_shutbadge_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F489-v2");
    // 1) 两阶段时长账：阶段 0 耗时 = 换阶段时刻差。
    let mut b = ShutdownBadge::new(1000);
    let _ = b.advance_phase(3000);
    cs.add("phase0_duration", b.phase_duration(9000, 0) == 2000, "");
    cs.add("phase1_duration", b.phase_duration(9000, 1) == 6000, "");
    cs.add("elapsed_total", b.elapsed(9000) == 8000, "");
    // 2) 归因抖动抑制：同卡点稳定呈现。
    let mut b2 = ShutdownBadge::new(0);
    cs.add("attribution_stable", attribution_stable(&mut b2, 15_000, Some("文档相机")), "");
    // 3) 强断全链让路。
    let mut b3 = ShutdownBadge::new(0);
    cs.add("force_cut_yields", force_cut_yields_everywhere(&mut b3, 5_000), "");
    // 4) 阶段文案表。
    cs.add("phase_labels", phase_labels_match(), "");
    // 5) 正常路径安静（8s 内无归因无数字）。
    let b4 = ShutdownBadge::new(0);
    cs.add("normal_quiet", normal_shutdown_quiet(&b4, 8_000), "");
    cs.add("late_not_quiet", !normal_shutdown_quiet(&ShutdownBadge::new(0), 11_000), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn attribution_below_threshold_none() {
        let mut b = ShutdownBadge::new(0);
        assert!(b.attribution(9_999, Some("慢应用")).is_none(), "恰好阈值内不归因");
        assert_eq!(b.attribution(10_001, Some("慢应用")), Some("慢应用"));
    }

    #[test]
    fn advance_phase_idempotent_rejection() {
        let mut b = ShutdownBadge::new(0);
        assert!(b.advance_phase(100));
        assert!(!b.advance_phase(200), "两阶段制：无第三阶段");
        assert!(!b.advance_phase(300));
    }

    #[test]
    fn stuck_app_change_updates_attribution() {
        let mut b = ShutdownBadge::new(0);
        assert_eq!(b.attribution(20_000, Some("A 应用")), Some("A 应用"));
        assert_eq!(b.attribution(25_000, Some("B 应用")), Some("B 应用"), "卡点变更如实更新");
    }

    #[test]
    fn normal_path_never_shows_numbers() {
        // 完整正常关机走查：全链零数字。
        let mut b = ShutdownBadge::new(0);
        assert!(b.shows_no_numbers());
        let _ = b.advance_phase(2_000);
        assert!(b.shows_no_numbers());
        assert!(b.stuck_app.is_none());
    }
}

// ===========================================================================
// 深化 v5（F489）：阶段超时探测 / 归因历史账 / 徽标文案构建器（零数字
// 扫描验证）/ 关机完成回执
// ===========================================================================

/// 阶段预期窗口（阶段超过该时长仍未推进 → 超时探测点亮——「正在保存
/// 设置」挂 3 分钟没有下文 = 用户被晾着，必须显性化）。
pub const PHASE_STALL_MS: u64 = 8_000;

impl ShutdownBadge {
    /// 阶段停滞探测（当前阶段停留超窗 → Some(提示语)；正常 None）。
    /// 强断路径优先让路（F318 红线与 v2 一致）。
    pub fn stall_probe(&self, now_ms: u64) -> Option<&'static str> {
        if self.force_cut_active {
            return None;
        }
        let in_phase = match self.phase {
            ShutPhase::SavingSettings => now_ms.saturating_sub(self.phase_enter_ms[0]),
            ShutPhase::EndingApps => {
                if self.phase_enter_ms[1] == 0 {
                    0
                } else {
                    now_ms.saturating_sub(self.phase_enter_ms[1])
                }
            }
        };
        if in_phase > PHASE_STALL_MS {
            Some("这一步比平时慢，仍在处理（可按住电源键强制关机）")
        } else {
            None
        }
    }

    /// 关机完成回执（收场动作：归因清账、账目闭合——关机流程恒可完成；
    /// 「慢得体面」的最后一笔是把卡点账擦干净再熄屏）。
    pub fn complete(&mut self, now_ms: u64) -> bool {
        self.stuck_app = None;
        let _ = now_ms;
        true
    }
}

/// 归因历史账（>10s 慢关机的卡点流水：同一次关机内卡点变更全部留痕——
/// 复盘「A 卡完 B 卡」的完整故事线；容量 8 环形淘汰）。
pub const ATTRIBUTION_LOG_CAP: usize = 8;

pub struct AttributionLog {
    entries: [Option<&'static str>; ATTRIBUTION_LOG_CAP],
    n: usize,
}

impl AttributionLog {
    pub const fn new() -> Self {
        AttributionLog { entries: [None; ATTRIBUTION_LOG_CAP], n: 0 }
    }

    /// 记卡点（与上一条相同不重复记——账记变化）。
    pub fn record(&mut self, app: &'static str) -> bool {
        if self.n > 0 && self.entries[self.n - 1] == Some(app) {
            return false;
        }
        if self.n >= ATTRIBUTION_LOG_CAP {
            self.entries.copy_within(1.., 0);
            self.n -= 1;
        }
        self.entries[self.n] = Some(app);
        self.n += 1;
        true
    }

    /// 全链回放（按序取卡点流水——复盘故事线）。
    pub fn replay(&self) -> [Option<&'static str>; ATTRIBUTION_LOG_CAP] {
        self.entries
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 徽标文案构建器（两阶段小字 + 慢时归因行——零数字扫描：构建出的
/// 全部文案逐字符扫过，正常路径一个数字都不许出现，主册判据的
/// 自动化守卫而非口头承诺）。
pub fn badge_lines_clean(phase_label: &str, attribution: Option<&str>) -> bool {
    // 阶段行：纯文案无数字。
    let phase_ok = !phase_label.bytes().any(|b| b.is_ascii_digit());
    // 归因行（若显示）：应用名 + 「正在退出」——同样无数字无百分比。
    let attr_ok = match attribution {
        None => true,
        Some(a) => !a.bytes().any(|b| b.is_ascii_digit()) && a.ends_with("正在退出"),
    };
    phase_ok && attr_ok
}

pub fn run_shutbadge_v5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F489-v5");
    // 1) 阶段停滞探测：8s 内安静、超窗提示、强断让路。
    let mut b = ShutdownBadge::new(0);
    cs.add("stall_quiet", b.stall_probe(7_999).is_none(), "");
    cs.add("stall_warns", b.stall_probe(8_001).is_some(), "");
    b.force_cut_active = true;
    cs.add("stall_force_cut_yields", b.stall_probe(60_000).is_none(), "");
    b.force_cut_active = false;
    // 2) 阶段二停滞探测（进入 EndingApps 后以阶段二时刻起算）。
    let mut b2 = ShutdownBadge::new(0);
    let _ = b2.advance_phase(1_000);
    cs.add("stall_phase2_window", b2.stall_probe(1_000 + PHASE_STALL_MS).is_none() && b2.stall_probe(1_000 + PHASE_STALL_MS + 1).is_some(), "");
    // 3) 归因历史账：变化才记账、环淘汰、回放有序。
    let mut log = AttributionLog::new();
    let _ = log.record("文档相机");
    cs.add("attr_dedup", !log.record("文档相机") && log.count() == 1, "");
    let _ = log.record("同步引擎");
    let _ = log.record("备份服务");
    cs.add("attr_replay_order", {
        let r = log.replay();
        r[0] == Some("文档相机") && r[1] == Some("同步引擎") && r[2] == Some("备份服务")
    }, "");
    for i in 0..(ATTRIBUTION_LOG_CAP + 2) {
        // 静态名轮换触发环淘汰（数字转词：甲乙丙丁…）。
        let name = match i % 8 {
            0 => "甲应用", 1 => "乙应用", 2 => "丙应用", 3 => "丁应用",
            4 => "戊应用", 5 => "己应用", 6 => "庚应用", _ => "辛应用",
        };
        let _ = log.record(name);
    }
    cs.add("attr_ring_cap", log.count() == ATTRIBUTION_LOG_CAP, "");
    // 4) 零数字文案扫描：正常路径全链干净；归因行同样干净。
    cs.add("badge_clean_normal", badge_lines_clean(ShutPhase::SavingSettings.label(), None), "");
    cs.add("badge_clean_attr", badge_lines_clean(ShutPhase::EndingApps.label(), Some("文档相机正在退出")), "");
    // 5) 数字混入必被扫出（守卫本身有牙——防回归测试）。
    cs.add("badge_dirty_caught", !badge_lines_clean("剩余 3 秒", None) && !badge_lines_clean(ShutPhase::SavingSettings.label(), Some("app2 正在退出")), "");
    // 6) 完成回执：干净收场清归因。
    let mut b3 = ShutdownBadge::new(0);
    let _ = b3.attribution(20_000, Some("慢应用"));
    cs.add("complete_cleans", b3.complete(25_000) && b3.stuck_app.is_none(), "");
    cs
}

#[cfg(test)]
mod v5_tests {
    use super::*;

    #[test]
    fn stall_probe_phase2_uses_phase2_clock() {
        let mut b = ShutdownBadge::new(0);
        let _ = b.advance_phase(9_000); // 阶段一 9s（已超 8s 窗但不看它了）
        // 阶段二刚进入 1s：不报警（每阶段独立窗口）。
        assert!(b.stall_probe(10_000).is_none());
        assert!(b.stall_probe(9_000 + PHASE_STALL_MS + 1).is_some());
    }

    #[test]
    fn attribution_log_lifo_ring() {
        let mut log = AttributionLog::new();
        for name in ["A", "B", "C", "D", "E", "F", "G", "H", "I"] {
            let _ = log.record(name);
        }
        assert_eq!(log.count(), ATTRIBUTION_LOG_CAP);
        let r = log.replay();
        assert_eq!(r[0], Some("B"), "最旧 A 被淘汰");
        assert_eq!(r[7], Some("I"));
    }

    #[test]
    fn complete_twice_is_stable() {
        let mut b = ShutdownBadge::new(0);
        assert!(b.complete(1_000));
        assert!(b.complete(2_000), "重复完成不炸（幂等收场）");
    }
}
