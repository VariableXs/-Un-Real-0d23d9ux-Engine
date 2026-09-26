//! F196 电池保护策略（secstar2 · G-G-26）——没电也要体面地睡。
//!
//! **判据（主册）**：两级阈值触发实测（可调电源模拟）；倒计时取消路径即时；
//! 关机账目完整（B-2902 对拍）；滤波防抖实测（注入跳变样本）。
//!
//! **功能定义（主册 G-G-26）**：宿主电池保护：电量 <15% 提示 toast（一次性）/
//! <5% 主动冲刷（F046 全量）+安全关机（B-2902 关机账目照走）+关机前最后提示
//! （60s 倒计时可取消外接电源）。
//!
//! 【交互设计】15% toast 黄（含预估剩余时长 F060 数据）；5% 全屏柔和提示卡
//! （非 panic 风格——是预告不是事故）：倒计时环 60s+「已接通电源？取消关机」
//! 大钮+已保护项清单（冲刷中逐项打勾）；关机画面复用 C-3 电源链动画。
//! 【数据与存储】阈值配置层（15/5% 可调 10-20/3-10 界内）；关机记录（原因=
//! 低电）入诊断。
//! 【状态与异常】电量计读数跳变（老化电池）→ 滑动平均滤波（30s 窗）防误触
//! 发；关机中途接电 → 立即中止关机流程（可逆窗口设计——冲刷完成后进入不可
//! 逆段前均有出口）；关机失败（冲刷卡死 30s）→ 强制下电+下次开机修复流程
//! （F189 自愈族）。
//! 【设计细节】滤波窗口 30s 采样 1s（跳变 >8% 视为噪声丢弃）；提示卡配色
//! 琥珀（警示非恐慌——与 F173 星陨视觉族区分）；已保护清单逐项（未保存
//! 文档草稿/剪贴板/窗口清单——交接四步的子集复用）；预估剩余时长标注
//! 「估算」；外接电源检测即时中断倒计时（<500ms 响应）。
//!
//! 接缝纪律：冲刷与关机链 WP-106/B-2902 既有——本模块是策略与状态机层，
//! 冲刷执行由调用方回报进度；关机账目（原因=低电）注出到 B-2902 链。
//! 依赖锚点：F046（全量冲刷）、F060（剩余时长）、F077（toast）、F189（修复族）、F173（视觉族区分）。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 一级阈值：15%（可调 10-20）。
pub const LEVEL_WARN_DEFAULT: u64 = 15;
pub const LEVEL_WARN_MIN: u64 = 10;
pub const LEVEL_WARN_MAX: u64 = 20;

/// 二级阈值：5%（可调 3-10）。
pub const LEVEL_CRIT_DEFAULT: u64 = 5;
pub const LEVEL_CRIT_MIN: u64 = 3;
pub const LEVEL_CRIT_MAX: u64 = 10;

/// 关机倒计时：60 秒。
pub const COUNTDOWN_S: u64 = 60;

/// 滤波窗口：30 秒（采样 1s）。
pub const FILTER_WINDOW_S: usize = 30;
/// 跳变噪声判定：>8% 视为噪声丢弃。
pub const JUMP_NOISE_PERMILLE: u64 = 80;

/// 冲刷卡死超时：30s → 强制下电。
pub const FLUSH_STUCK_TIMEOUT_S: u64 = 30;

/// 外接电源检测即时中断：500ms。
pub const AC_DETECT_MS: u64 = 500;

/// 提示卡标题（琥珀警示——非恐慌）。
pub const CRIT_CARD_TITLE: &str = "电量不足，系统将保护性关机";
pub const CANCEL_TEXT: &str = "已接通电源？取消关机";
/// 估算标注。
pub const ESTIMATED_TAG: &str = "估算";

/// 已保护清单（交接四步子集——冲刷中逐项打勾）。
pub const PROTECT_LIST: [&str; 4] = [
    "未保存文档草稿",
    "剪贴板",
    "窗口清单",
    "系统状态快照",
];

/// 二级状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 正常（电量高于一级阈值）。
    Normal,
    /// 一级提示已发（toast 黄）。
    Warned,
    /// 二级：倒计时进行中（可取消）。
    Countdown { left_s: u64 },
    /// 冲刷+关机执行中（不可逆段之前仍可取消——可逆窗口设计）。
    Flushing { elapsed_s: u64 },
    /// 交接完成，等待下电（B-2902「可拔电」语义）。
    Handoff,
}

/// 关机账目条目（原因=低电——B-2902 对拍源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownLedgerEntry {
    pub at_s: u64,
    /// 关机时电量（permille）。
    pub level_permille: u64,
    /// 原因分类。
    pub reason: &'static str,
    /// 是否被取消（取消也入账——完整账目）。
    pub cancelled: bool,
}

/// 电池保护主体。
pub struct BatteryGuard {
    /// 阈值（界内可调）。
    pub warn_level: u64,
    pub crit_level: u64,
    pub phase: Phase,
    /// 滤波环（30s 窗，1s 采样）。
    samples: RingLog<u64, FILTER_WINDOW_S>,
    /// 上一有效滤波值（permille）。
    filtered_permille: Option<u64>,
    /// toast 一次性标志。
    warn_toast_sent: bool,
    /// 噪声丢弃计数（滤波对账）。
    pub noise_dropped: u64,
    /// 关机账目（最近 8 条——原因=低电）。
    pub ledger: RingLog<ShutdownLedgerEntry, 8>,
    /// 强制下电计数（冲刷卡死路径）。
    pub forced_poweroffs: u64,
}

impl BatteryGuard {
    pub fn new() -> BatteryGuard {
        BatteryGuard {
            warn_level: LEVEL_WARN_DEFAULT,
            crit_level: LEVEL_CRIT_DEFAULT,
            phase: Phase::Normal,
            samples: RingLog::new(),
            filtered_permille: None,
            warn_toast_sent: false,
            noise_dropped: 0,
            ledger: RingLog::new(),
            forced_poweroffs: 0,
        }
    }

    /// 调阈值（两级各自钳界+交叉防呆：crit 不得高于 warn）。
    pub fn set_levels(&mut self, warn: u64, crit: u64) {
        self.warn_level = warn.clamp(LEVEL_WARN_MIN, LEVEL_WARN_MAX);
        self.crit_level = crit.clamp(LEVEL_CRIT_MIN, LEVEL_CRIT_MAX).min(self.warn_level);
    }

    /// **电量上报主路**（判据一二三的入口）：滤波 → 阈值判定 → 相位推进。
    /// 返回本采样触发的事件（供 UI/通知层消费）。
    pub fn report_level(&mut self, raw_permille: u64) -> Option<GuardEvent> {
        // 滤波：跳变 >8% 丢弃（噪声）；窗内滑动平均。
        if let Some(prev) = self.filtered_permille {
            let diff = prev.abs_diff(raw_permille);
            if diff > JUMP_NOISE_PERMILLE {
                self.noise_dropped += 1;
                return None; // 噪声样本不入窗不触发。
            }
        }
        self.samples.push(raw_permille);
        // 滑动平均（窗内样本）。
        let win = self.samples.newest_first();
        let sum: u64 = win.iter().sum();
        self.filtered_permille = Some(sum / win.len() as u64);
        let level = self.filtered_permille.unwrap();

        // 相位推进。
        match self.phase {
            Phase::Normal | Phase::Warned => {
                if level <= self.crit_level * 10 {
                    self.phase = Phase::Countdown { left_s: COUNTDOWN_S };
                    Some(GuardEvent::CriticalCard)
                } else if level <= self.warn_level * 10 && !self.warn_toast_sent {
                    self.warn_toast_sent = true;
                    self.phase = Phase::Warned;
                    Some(GuardEvent::WarnToast)
                } else {
                    None
                }
            }
            _ => None, // 倒计时/冲刷/交接段由 tick/cancel 驱动，电量采样不再推进。
        }
    }

    /// 倒计时 tick（每秒）。到 0 → 进入冲刷段。
    pub fn tick(&mut self, now_s: u64) -> Option<GuardEvent> {
        match self.phase {
            Phase::Countdown { left_s } => {
                if left_s <= 1 {
                    self.phase = Phase::Flushing { elapsed_s: 0 };
                    Some(GuardEvent::FlushBegin)
                } else {
                    self.phase = Phase::Countdown { left_s: left_s - 1 };
                    None
                }
            }
            Phase::Flushing { elapsed_s } => {
                if elapsed_s >= FLUSH_STUCK_TIMEOUT_S {
                    // 冲刷卡死 → 强制下电 + 下次开机修复流程（F189 自愈族）。
                    self.forced_poweroffs += 1;
                    self.ledger.push(ShutdownLedgerEntry {
                        at_s: now_s,
                        level_permille: self.filtered_permille.unwrap_or(0),
                        reason: "low-battery-forced",
                        cancelled: false,
                    });
                    self.phase = Phase::Handoff;
                    Some(GuardEvent::ForcedPoweroff)
                } else {
                    self.phase = Phase::Flushing { elapsed_s: elapsed_s + 1 };
                    None
                }
            }
            _ => None,
        }
    }

    /// **取消路径**（判据二）：外接电源接入（<500ms 响应语义——调用方即时
    /// 调用）→ 立即中止，无论处于倒计时还是冲刷段（可逆窗口设计）。
    pub fn cancel_by_ac(&mut self, now_s: u64) -> bool {
        match self.phase {
            Phase::Countdown { .. } | Phase::Flushing { .. } => {
                self.ledger.push(ShutdownLedgerEntry {
                    at_s: now_s,
                    level_permille: self.filtered_permille.unwrap_or(0),
                    reason: "low-battery",
                    cancelled: true,
                });
                self.phase = Phase::Normal;
                self.warn_toast_sent = false; // 回正常段：下次触线重新提示。
                true
            }
            _ => false,
        }
    }

    /// 冲刷完成回报（交接四步子集逐项打勾后调用）→ 交接完成态。
    pub fn finish_flush(&mut self, now_s: u64) -> bool {
        if let Phase::Flushing { .. } = self.phase {
            self.ledger.push(ShutdownLedgerEntry {
                at_s: now_s,
                level_permille: self.filtered_permille.unwrap_or(0),
                reason: "low-battery",
                cancelled: false,
            });
            self.phase = Phase::Handoff;
            true
        } else {
            false
        }
    }

    /// 已保护清单（提示卡逐项打勾数据）。
    pub fn protect_list(&self) -> &'static [&'static str; 4] {
        &PROTECT_LIST
    }

    /// 剩余时长文案（估算标注——F060 数据注入）。
    pub fn remaining_text(&self, est_min: u64) -> (&'static str, u64) {
        (ESTIMATED_TAG, est_min)
    }

    /// 最近账目（B-2902 对拍——新→旧）。
    pub fn recent_ledger(&self) -> Vec<ShutdownLedgerEntry> {
        self.ledger.newest_first()
    }

    /// 接电恢复正常（电量回升离开阈值带——复位一次性标志与相位）。
    /// Warned 也是可恢复态（它只是提示态——紧急段 Countdown/Flushing/
    /// Handoff 的退出走 cancel_by_ac/finish_flush）。
    pub fn recover(&mut self) {
        if let Some(level) = self.filtered_permille {
            if matches!(self.phase, Phase::Normal | Phase::Warned) && level > self.warn_level * 10 {
                self.warn_toast_sent = false;
                self.phase = Phase::Normal;
            }
        }
    }
}

/// 保护事件（UI/通知层消费）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardEvent {
    /// 一级 toast 黄。
    WarnToast,
    /// 二级全屏柔和提示卡（琥珀）。
    CriticalCard,
    /// 冲刷开始。
    FlushBegin,
    /// 冲刷卡死强制下电。
    ForcedPoweroff,
}


// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F196 自检（聚合进 secstar2 域）。
pub fn run_batguard_checks() -> CheckSet {
    let mut set = CheckSet::new("F196-batguard");

    // 判据一：两级阈值触发（可调电源模拟=直接喂 permille）。
    // 渐变放电序列（每步 2‰——0.2%/s 的快放速率；30s 滑动窗滞后 ≈3%）。
    let mut g = BatteryGuard::new();
    let mut got_toast = false;
    let mut got_card = false;
    let mut level = 800u64;
    while level > 2 {
        level -= 2;
        match g.report_level(level) {
            Some(GuardEvent::WarnToast) => got_toast = true,
            Some(GuardEvent::CriticalCard) => got_card = true,
            _ => {}
        }
    }
    set.add("warn toast fires", got_toast, "");
    set.add("crit card fires", got_card, "");
    set.add("phase countdown", matches!(g.phase, Phase::Countdown { left_s: 60 }), "");

    // 判据二：倒计时取消即时。
    for _ in 0..10 {
        g.tick(0);
    }
    set.add("countdown ticks", matches!(g.phase, Phase::Countdown { left_s: 50 }), "");
    set.add("cancel instant", g.cancel_by_ac(10), "");
    set.add("cancel restores normal", g.phase == Phase::Normal, "");
    set.add("cancel in ledger", g.recent_ledger()[0].cancelled, "");

    // 冲刷链：倒计时走完 → 冲刷 → 完成 → 交接。
    let mut g2 = BatteryGuard::new();
    let _ = g2.report_level(30);
    for _ in 0..COUNTDOWN_S {
        g2.tick(0);
    }
    set.add("flush after countdown", matches!(g2.phase, Phase::Flushing { .. }), "");
    set.add("flush cancellable", g2.cancel_by_ac(1), "reversible window before handoff");
    // 重走到冲刷并完成。
    let _ = g2.report_level(30);
    for _ in 0..COUNTDOWN_S {
        g2.tick(100);
    }
    set.add("finish flush", g2.finish_flush(200), "");
    set.add("handoff phase", g2.phase == Phase::Handoff, "");
    set.add("ledger complete", g2.recent_ledger()[0].reason == "low-battery" && !g2.recent_ledger()[0].cancelled, "");

    // 冲刷卡死 → 强制下电（多走一个 tick 让 30s 超时被观察到）。
    let mut g3 = BatteryGuard::new();
    let _ = g3.report_level(30);
    for _ in 0..COUNTDOWN_S {
        g3.tick(0);
    }
    for _ in 0..=FLUSH_STUCK_TIMEOUT_S {
        g3.tick(0);
    }
    set.add("stuck forced", g3.forced_poweroffs == 1, "");
    set.add("forced ledger", g3.recent_ledger()[0].reason == "low-battery-forced", "");

    // 判据四：滤波防抖（注入跳变样本——>8% 丢弃）。
    let mut g4 = BatteryGuard::new();
    set.add("sample 500", g4.report_level(500).is_none(), "");
    // 前一滤波值 500，跳到 100（40%）→ 噪声丢弃。
    set.add("jump dropped", g4.report_level(100).is_none(), "");
    set.add("noise counted", g4.noise_dropped == 1, "");
    // 平滑样本（<8%）入窗。
    set.add("smooth kept", g4.report_level(520).is_none(), "");

    // 阈值交叉防呆：crit 钳到 warn 之下。
    let mut g5 = BatteryGuard::new();
    g5.set_levels(10, 20);
    set.add("crit clamp", g5.crit_level <= g5.warn_level, "");

    // 提示卡文案与保护清单。
    set.add("card title", CRIT_CARD_TITLE.contains("保护性关机"), "");
    set.add("cancel text", CANCEL_TEXT.contains("取消"), "");
    set.add("protect 4 items", g2.protect_list().len() == 4, "");
    set.add("estimated tag", g2.remaining_text(30).0 == ESTIMATED_TAG, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f196_threshold_bounds_clamp() {
        let mut g = BatteryGuard::new();
        g.set_levels(99, 99); // 两级都超上界 → warn=20, crit=min(20,10)=10
        assert_eq!(g.warn_level, LEVEL_WARN_MAX);
        assert_eq!(g.crit_level, LEVEL_CRIT_MAX);
        g.set_levels(10, 3);
        assert_eq!(g.warn_level, LEVEL_WARN_MIN);
        assert_eq!(g.crit_level, LEVEL_CRIT_MIN);
    }

    #[test]
    fn f196_sliding_average_smooths() {
        let mut g = BatteryGuard::new();
        // 500 → 490 → 480：每次 <8% 平滑入窗；滤波值逐步下降。
        let _ = g.report_level(500);
        let _ = g.report_level(490);
        let _ = g.report_level(480);
        // 滤波均值 = (500+490+480)/3 ≈ 490。
        assert!(g.filtered_permille.unwrap() >= 488 && g.filtered_permille.unwrap() <= 491);
    }

    #[test]
    fn f196_noise_never_triggers_shutdown() {
        // 老化电池跳变：即使跳到 0 也不触发关键卡（噪声被滤）。
        let mut g = BatteryGuard::new();
        let _ = g.report_level(300);
        let ev = g.report_level(0);
        assert_eq!(ev, None, "40%+ jump is noise");
        assert_eq!(g.phase, Phase::Normal);
    }

    #[test]
    fn f196_recover_resets_toast_once() {
        let mut g = BatteryGuard::new();
        g.set_levels(20, 10); // warn=200‰ crit=100‰（阈值带放宽便于确定性）
        // 平滑放电（2‰/采样）降到 warn 线下 → toast 触发一次。
        let mut level = 400u64;
        let mut got = false;
        while level > 100 {
            level -= 2;
            if g.report_level(level) == Some(GuardEvent::WarnToast) {
                got = true;
            }
        }
        assert!(got);
        // 继续徘徊在 warn 下：一次性——不再触发。
        assert_eq!(g.report_level(160), None);
        assert_eq!(g.report_level(162), None);
        // 平滑回升（2‰/采样）到 30% 且滤波值越过 warn 线 → recover 复位。
        while level < 320 {
            level += 2;
            let _ = g.report_level(level);
        }
        for _ in 0..FILTER_WINDOW_S {
            let _ = g.report_level(320);
        }
        g.recover();
        // 再次平滑放电到 warn 线下 → toast 再次触发（按周期计的一次性）。
        let mut got2 = false;
        while level > 100 {
            level -= 2;
            if g.report_level(level) == Some(GuardEvent::WarnToast) {
                got2 = true;
            }
        }
        assert!(got2);
    }

    #[test]
    fn f196_countdown_full_journey() {
        let mut g = BatteryGuard::new();
        let _ = g.report_level(40);
        let mut flush_started = false;
        for i in 0..(COUNTDOWN_S + 2) {
            if let Some(ev) = g.tick(i) {
                if ev == GuardEvent::FlushBegin {
                    flush_started = true;
                }
            }
        }
        assert!(flush_started);
        // 冲刷 30s 未完成回报 → 强制下电。
        for i in 0..FLUSH_STUCK_TIMEOUT_S {
            let _ = g.tick(100 + i);
        }
        assert_eq!(g.forced_poweroffs, 1);
    }

    #[test]
    fn f196_ledger_newest_first() {
        let mut g = BatteryGuard::new();
        let _ = g.report_level(40);
        for i in 0..COUNTDOWN_S {
            let _ = g.tick(i);
        }
        let _ = g.finish_flush(999);
        let l = g.recent_ledger();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].at_s, 999);
        assert_eq!(l[0].level_permille, 40);
    }

    #[test]
    fn f196_run_checks_pass() {
        assert!(run_batguard_checks().all_passed());
    }
}
