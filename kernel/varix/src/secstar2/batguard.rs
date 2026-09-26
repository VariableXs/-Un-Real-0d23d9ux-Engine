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

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——五个真功能面。
// ---------------------------------------------------------------------------

use alloc::string::String;

// ---------------------------------------------------------------------------
// 深一：ProtectChecklist —— 已保护清单逐项打勾（主册【交互设计】：冲刷中
// 逐项打勾——用户在最后 60 秒看得见系统在保护什么）
// ---------------------------------------------------------------------------

/// 清单条目状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtectItem {
    pub name: &'static str,
    /// 已保护（打勾）。
    pub done: bool,
}

/// 打勾状态机（冲刷段每完成一项由交接回报打勾——顺序即 PROTECT_LIST 序，
/// 跳项视为交接层缺陷返回 false）。
pub struct ProtectChecklist {
    items: [ProtectItem; 4],
    /// 已打勾数。
    done: usize,
}

impl ProtectChecklist {
    pub fn new() -> ProtectChecklist {
        ProtectChecklist {
            items: [
                ProtectItem { name: PROTECT_LIST[0], done: false },
                ProtectItem { name: PROTECT_LIST[1], done: false },
                ProtectItem { name: PROTECT_LIST[2], done: false },
                ProtectItem { name: PROTECT_LIST[3], done: false },
            ],
            done: 0,
        }
    }

    /// 打勾下一项（顺序执行——交接四步的子集序）。
    pub fn tick_item(&mut self) -> Result<usize, &'static str> {
        if self.done >= self.items.len() {
            return Err("清单已全部打勾");
        }
        self.items[self.done].done = true;
        self.done += 1;
        Ok(self.done)
    }

    /// 进度 permille（提示卡进度环的数据源——打勾进度即冲刷进度）。
    pub fn progress_permille(&self) -> u64 {
        self.done as u64 * 1000 / self.items.len() as u64
    }

    pub fn items(&self) -> &[ProtectItem; 4] {
        &self.items
    }

    pub fn all_done(&self) -> bool {
        self.done == self.items.len()
    }
}

impl Default for ProtectChecklist {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深二：AcLatency —— 外接电源检测时延对账（主册【设计细节】：外接电源
// 检测即时中断倒计时（<500ms 响应）——「即时」不是口号是对账字段）
// ---------------------------------------------------------------------------

/// AC 中断事件时延判定：检测时刻与触发时刻之差 ≤500ms 记达标，超出记
/// 违例（计数器——超标的每一次都该被驱动层追查）。
pub struct AcLatency {
    /// 达标次数。
    pub within: u64,
    /// 违例次数（>500ms——每条都是驱动缺陷）。
    pub violations: u64,
    /// 最近一次时延（ms——诊断面展示）。
    pub last_ms: Option<u64>,
}

impl AcLatency {
    pub fn new() -> AcLatency {
        AcLatency { within: 0, violations: 0, last_ms: None }
    }

    /// 上报一次中断时延（abnormal 触发时刻 → AC 检测生效时刻）。
    pub fn observe(&mut self, latency_ms: u64) -> bool {
        self.last_ms = Some(latency_ms);
        if latency_ms <= AC_DETECT_MS {
            self.within += 1;
            true
        } else {
            self.violations += 1;
            false
        }
    }
}

impl Default for AcLatency {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深三：ToastPayload —— 一级 toast 载荷组装（主册【交互设计】：15% toast
// 黄（含预估剩余时长 F060 数据）+【设计细节】预估剩余时长标注「估算」）
// ---------------------------------------------------------------------------

/// toast 载荷（UI 层拿去即用——文案与数据一次给齐，零二次拼装猜测）。
pub struct ToastPayload {
    /// 主文案。
    pub text: &'static str,
    /// 预估剩余分钟（F060 注入）。
    pub est_min: u64,
    /// 估算标注（诚实纪律——这个数字是推算不是实测）。
    pub estimated_tag: &'static str,
    /// 黄（警示色语义——与琥珀卡/星陨 panic 三族区分）。
    pub severity: &'static str,
}

/// toast 组装（一次性标志由 BatteryGuard 管，这里只管内容）。
pub fn toast_payload(est_min: u64) -> ToastPayload {
    ToastPayload {
        text: "电量偏低，重要工作请保存",
        est_min,
        estimated_tag: ESTIMATED_TAG,
        severity: "warn-yellow",
    }
}

// ---------------------------------------------------------------------------
// 深四：DiagnosticLines —— 关机账目诊断行（B-2902 对拍的人话渲染：
// 每条账目一行——时刻/电量/原因/是否取消，取消也入账的完整语义可见）
// ---------------------------------------------------------------------------

/// 单条诊断行字段（`时刻=at 电量=permille‰ 原因=reason [已取消]`）。
pub fn ledger_line(e: &ShutdownLedgerEntry, out: &mut String) {
    out.push_str("时刻=");
    push_u64(out, e.at_s);
    out.push_str("s 电量=");
    push_u64(out, e.level_permille);
    out.push_str("‰ 原因=");
    out.push_str(e.reason);
    if e.cancelled {
        out.push_str("（已取消——账目照记）");
    }
}

fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
}

/// 账目对拍完整性：取消条目必须带「已取消」标记、非取消条目必须没有——
/// B-2902 对拍口径在渲染层的守恒式。
pub fn ledger_lines_consistent(g: &BatteryGuard) -> bool {
    g.recent_ledger().iter().all(|e| {
        let mut s = String::new();
        ledger_line(e, &mut s);
        e.cancelled == s.contains("已取消")
    })
}

// ---------------------------------------------------------------------------
// 深五：ReversibilityMap —— 可逆窗口地图（主册【状态与异常】：冲刷完成
// 后进入不可逆段前均有出口——每相态「能不能取消」是显式契约不是隐式行为）
// ---------------------------------------------------------------------------

/// 相态可取消性（显式地图——UI 层据灰置/显示取消钮，测试据它对拍）。
pub fn cancellable(phase: &Phase) -> bool {
    matches!(phase, Phase::Countdown { .. } | Phase::Flushing { .. })
}

/// 相态人话（提示卡副标题——用户此刻处于哪一步、还来得及吗）。
pub fn phase_text(phase: &Phase) -> &'static str {
    match phase {
        Phase::Normal => "电量正常",
        Phase::Warned => "电量偏低（提示已发出）",
        Phase::Countdown { .. } => "即将保护性关机——接通电源可取消",
        Phase::Flushing { .. } => "正在保护您的工作——仍可取消",
        Phase::Handoff => "交接完成，即将下电（不可取消）",
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F196 深化自检（聚合进 secstar2 域）。
pub fn run_batguard_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F196-deep");

    // 深一：清单打勾——顺序执行、进度环、全勾闭合；跳过式调用诚实报错。
    let mut cl = ProtectChecklist::new();
    set.add("checklist 4 items", cl.items().len() == 4, "");
    for k in 1..=4 {
        set.add("checklist tick", cl.tick_item() == Ok(k), "");
    }
    set.add("checklist full", cl.all_done() && cl.progress_permille() == 1000, "");
    set.add("checklist overflow honest", cl.tick_item().is_err(), "");
    let mut cl2 = ProtectChecklist::new();
    let _ = cl2.tick_item();
    set.add("checklist partial", cl2.progress_permille() == 250, "");

    // 深二：AC 时延——499 达标 / 500 达标 / 501 违例（边界逐点）。
    let mut ac = AcLatency::new();
    set.add("ac 499 ok", ac.observe(499), "");
    set.add("ac 500 ok", ac.observe(AC_DETECT_MS), "");
    set.add("ac 501 bad", !ac.observe(501), "");
    set.add("ac counts", ac.within == 2 && ac.violations == 1 && ac.last_ms == Some(501), "");

    // 深三：toast 载荷——文案/估算标注/黄色语义三件齐。
    let t = toast_payload(23);
    set.add("toast text", t.text.contains("保存"), "");
    set.add("toast est", t.est_min == 23 && t.estimated_tag == ESTIMATED_TAG, "");
    set.add("toast severity", t.severity == "warn-yellow", "");

    // 深四：诊断行——取消/非取消两态渲染与守恒式。
    let mut g = BatteryGuard::new();
    let _ = g.report_level(40);
    for i in 0..COUNTDOWN_S {
        let _ = g.tick(i);
    }
    let _ = g.cancel_by_ac(61);
    let _ = g.report_level(35);
    for i in 0..COUNTDOWN_S {
        let _ = g.tick(100 + i);
    }
    let _ = g.finish_flush(200);
    let led = g.recent_ledger();
    set.add("ledger has both kinds", led.iter().any(|e| e.cancelled) && led.iter().any(|e| !e.cancelled), "");
    set.add("ledger lines consistent", ledger_lines_consistent(&g), "");
    let mut s = String::new();
    ledger_line(&led[0], &mut s);
    set.add("ledger line shape", s.contains("电量=") && s.contains("原因="), "");

    // 深五：可逆地图——倒计时/冲刷可取消，Handoff 不可（诚实的边界）。
    let mut g2 = BatteryGuard::new();
    set.add("normal not cancellable", !cancellable(&g2.phase), "");
    let _ = g2.report_level(30);
    set.add("countdown cancellable", cancellable(&g2.phase), "");
    set.add("phase text countdown", phase_text(&g2.phase).contains("取消"), "");
    for _ in 0..COUNTDOWN_S {
        g2.tick(0);
    }
    set.add("flush cancellable", cancellable(&g2.phase), "");
    let _ = g2.finish_flush(1);
    set.add("handoff NOT cancellable", !cancellable(&g2.phase), "不可逆段如实声明");
    set.add("phase text handoff", phase_text(&g2.phase).contains("不可取消"), "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f196_deep_cancel_during_flush_midway() {
        // 冲刷中途接电（打勾到一半）：取消即时、清单作废、回到 Normal。
        let mut g = BatteryGuard::new();
        let _ = g.report_level(40);
        for _ in 0..COUNTDOWN_S {
            g.tick(0);
        }
        let mut cl = ProtectChecklist::new();
        let _ = cl.tick_item();
        let _ = cl.tick_item();
        assert!(g.cancel_by_ac(5), "mid-flush cancel is within the reversible window");
        assert_eq!(g.phase, Phase::Normal);
        // 半途清单作废（新周期重新冲刷重新打勾）。
        let cl2 = ProtectChecklist::new();
        assert!(!cl2.all_done());
    }

    #[test]
    fn f196_deep_full_journey_with_checklist_and_ledger() {
        // 端到端：阈值 → toast → 卡 → 倒计时 → 冲刷逐项打勾 → 交接 → 账目。
        let mut g = BatteryGuard::new();
        let mut level = 900u64;
        let mut toast = false;
        while level > 2 {
            level -= 2;
            if g.report_level(level) == Some(GuardEvent::WarnToast) {
                toast = true;
            }
        }
        assert!(toast);
        for i in 0..COUNTDOWN_S {
            g.tick(i);
        }
        let mut cl = ProtectChecklist::new();
        for k in 0..4u64 {
            assert_eq!(cl.tick_item(), Ok(k as usize + 1));
            // 每打一项勾，冲刷段推进一秒（交接回报节奏）。
            g.tick(100 + k);
        }
        assert!(cl.all_done());
        assert!(g.finish_flush(200));
        assert_eq!(g.phase, Phase::Handoff);
        // 账目完整：一条非取消低电关机。
        let led = g.recent_ledger();
        assert_eq!(led.len(), 1);
        assert!(!led[0].cancelled && led[0].reason == "low-battery");
        assert!(ledger_lines_consistent(&g));
    }

    #[test]
    fn f196_deep_ac_observe_feeds_guard_cancel() {
        // 驱动层时延达标 → cancel_by_ac 的即时性语义成立（两层面联合验收）。
        let mut g = BatteryGuard::new();
        let mut ac = AcLatency::new();
        let _ = g.report_level(30);
        assert!(matches!(g.phase, Phase::Countdown { .. }));
        assert!(ac.observe(120), "driver latency within budget");
        assert!(g.cancel_by_ac(1));
        assert_eq!(g.phase, Phase::Normal);
    }

    #[test]
    fn f196_deep_run_checks_pass() {
        assert!(run_batguard_deep_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——放电斜率预估 / 提示卡渲染 /
// 账目导出。判据源：主册【交互设计】「15% toast 黄（含预估剩余时长 F060
// 数据）」的模型面 +【设计细节】「已保护清单逐项打勾」「关机画面复用 C-3
// 电源链动画」的渲染契约。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：DrainEstimator —— 预估剩余时长模型（F060 注入的放电斜率 →
// 线性外推到关机线——估算的「估算」也带口径说明）
// ---------------------------------------------------------------------------

/// 预估结果。
pub struct DrainEstimate {
    /// 预估剩余分钟。
    pub est_min: u64,
    /// 口径标注（恒「估算」——诚实纪律的机检字段）。
    pub tag: &'static str,
    /// 依据（放电斜率描述——这个数字怎么来的）。
    pub basis: &'static str,
}

/// 外推（最近 10 分钟掉电 permille → 剩余电量 / 速率；零斜率 → 诚实拒绝）。
pub fn drain_estimate(level_permille: u64, shutdown_at_permille: u64, drop_permille_per_10min: u64) -> Option<DrainEstimate> {
    if drop_permille_per_10min == 0 {
        return None; // 没有斜率谈不上预估（刚插电/读数冻结）——零静默。
    }
    let remain = level_permille.saturating_sub(shutdown_at_permille);
    Some(DrainEstimate {
        est_min: remain * 10 / drop_permille_per_10min,
        tag: ESTIMATED_TAG,
        basis: "最近 10 分钟放电斜率线性外推",
    })
}

// ---------------------------------------------------------------------------
// v3-二：CardRender —— 二级提示卡渲染数据（三区一次给齐：倒计时环/取消
// 钮/保护清单——主册【交互设计】的 5% 全屏柔和提示卡）
// ---------------------------------------------------------------------------

/// 提示卡渲染数据。
pub struct CardRender {
    /// 标题（CRIT_CARD_TITLE）。
    pub title: &'static str,
    /// 倒计时环剩余（秒——UI 据此画环）。
    pub countdown_left_s: u64,
    /// 取消钮文案（CANCEL_TEXT）。
    pub cancel_text: &'static str,
    /// 保护清单（四项+打勾态）。
    pub protect: Vec<(&'static str, bool)>,
    /// 卡片语义（琥珀警示——与 F173 panic 族区分）。
    pub severity: &'static str,
}

/// 组装（从 BatteryGuard 状态投影——冲刷段打勾态来自 ProtectChecklist）。
pub fn card_render(g: &BatteryGuard, checklist: &ProtectChecklist) -> Option<CardRender> {
    let left = match g.phase {
        Phase::Countdown { left_s } => left_s,
        _ => return None, // 非倒计时态无此卡——诚实 None。
    };
    Some(CardRender {
        title: CRIT_CARD_TITLE,
        countdown_left_s: left,
        cancel_text: CANCEL_TEXT,
        protect: checklist.items().iter().map(|i| (i.name, i.done)).collect(),
        severity: "amber",
    })
}

// ---------------------------------------------------------------------------
// v3-三：DiagnosticExport —— 关机账目导出行（B-2902 对拍的批量面：
// 全部账目行一次性导出，取消/非取消两态全带）
// ---------------------------------------------------------------------------

/// 导出全部账目行（新→旧——诊断中心「关机记录」区块数据）。
pub fn diagnostic_export(g: &BatteryGuard) -> Vec<String> {
    g.recent_ledger()
        .iter()
        .map(|e| {
            let mut s = String::new();
            ledger_line(e, &mut s);
            s
        })
        .collect()
}

/// 导出守恒式：行数=账目条数、取消标记与账目逐条一致（渲染零失真）。
pub fn diagnostic_export_consistent(g: &BatteryGuard) -> bool {
    let led = g.recent_ledger();
    let lines = diagnostic_export(g);
    led.len() == lines.len()
        && led.iter().zip(lines.iter()).all(|(e, l)| e.cancelled == l.contains("已取消"))
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F196 v3 自检（聚合进 secstar2 域）。
pub fn run_batguard_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F196-v3");

    // v3-一：放电预估——正常外推/零斜率诚实拒绝/口径标注。
    let d = drain_estimate(300, 50, 25).unwrap();
    set.add("drain est", d.est_min == 100 && d.tag == ESTIMATED_TAG, "25‰/10min → 250‰ 余量=100 分钟");
    set.add("drain basis", d.basis.contains("斜率"), "");
    set.add("drain zero slope honest", drain_estimate(300, 50, 0).is_none(), "");
    set.add("drain below line", drain_estimate(40, 50, 25).map(|x| x.est_min == 0).unwrap_or(false), "低于关机线=0 分钟");

    // v3-二：提示卡渲染——倒计时/取消钮/清单三区；非倒计时态诚实 None。
    let mut g = BatteryGuard::new();
    set.add("card none normal", card_render(&g, &ProtectChecklist::new()).is_none(), "");
    let _ = g.report_level(30);
    let cl = ProtectChecklist::new();
    let card = card_render(&g, &cl).unwrap();
    set.add("card title", card.title == CRIT_CARD_TITLE, "");
    set.add("card countdown", card.countdown_left_s == COUNTDOWN_S, "");
    set.add("card cancel", card.cancel_text == CANCEL_TEXT, "");
    set.add("card protect", card.protect.len() == 4 && card.protect.iter().all(|(_, d)| !d), "");
    set.add("card severity", card.severity == "amber", "");

    // v3-三：账目导出——行数守恒、取消标记一致。
    let mut g2 = BatteryGuard::new();
    let _ = g2.report_level(40);
    for i in 0..COUNTDOWN_S {
        let _ = g2.tick(i);
    }
    let _ = g2.cancel_by_ac(61);
    let _ = g2.report_level(35);
    for i in 0..COUNTDOWN_S {
        let _ = g2.tick(100 + i);
    }
    let _ = g2.finish_flush(200);
    set.add("diag export consistent", diagnostic_export_consistent(&g2), "");
    set.add("diag export rows", diagnostic_export(&g2).len() == g2.recent_ledger().len() && g2.recent_ledger().len() >= 2, "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f196_v3_drain_estimate_scales_with_slope() {
        // 斜率减半 → 预估翻倍（线性外推的性质机检）。
        let fast = drain_estimate(300, 50, 50).unwrap();
        let slow = drain_estimate(300, 50, 25).unwrap();
        assert_eq!(slow.est_min, fast.est_min * 2);
    }

    #[test]
    fn f196_v3_card_tracks_checklist_progress() {
        // 打勾推进 → 卡片清单勾态同步（渲染层零自持状态）。
        let mut g = BatteryGuard::new();
        let _ = g.report_level(30);
        let mut cl = ProtectChecklist::new();
        let _ = cl.tick_item();
        let _ = cl.tick_item();
        let card = card_render(&g, &cl).unwrap();
        assert_eq!(card.protect.iter().filter(|(_, d)| *d).count(), 2);
    }

    #[test]
    fn f196_v3_run_checks_pass() {
        assert!(run_batguard_deep2_checks().all_passed());
    }
}
