//! F065 唤醒源治理（perfstar2 · G-B-25）——待机耗电从查不到变成有账可查。
//!
//! 主册判据（验收标准第一句）：
//! **合盖 2 小时实测唤醒 ≤2 次、掉电 ≤2%；唤醒原因记录 100% 可解释。**
//!
//! 功能定义（G-B-25）：睡眠态唤醒源白名单——默认仅电源键/盖开合/USB 键鼠；
//! 每分钟唤醒次数 ≤1 达标；唤醒原因全记录。
//!
//! 【交互设计】设置中心「电源和电池-唤醒源」页：白名单管理 + 最近 10 次
//! 唤醒原因列表（时间/来源/耗时）。
//! 【数据与存储】唤醒事件日志入账本（保留 30 天）；白名单存配置层。
//! 【状态与异常】未知唤醒源（固件行为）→ 记录「未知」并建议排查；唤醒后
//! 60 秒无操作 → 自动回睡（可关）；闹钟类（F100）按用户预约显式唤醒。
//! 【设计细节】白名单分级：硬源（电源键不可移）/软源（USB 设备可逐个关）；
//! 唤醒抖动保护：醒后 5s 内再睡视为抖动计数（>3 次/小时告警）；RTC 唤醒
//! 仅限用户闹钟；包内唤醒（WoL）默认关。
//!
//! 零堆纪律：定长白名单 + 定长事件账本，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 合盖观察窗：2 小时（主册判据口径）。
pub const LID_WINDOW_MS: u64 = 2 * 3_600_000;
/// 观察窗内唤醒上限：≤2 次。
pub const LID_WINDOW_WAKE_CAP: u32 = 2;
/// 观察窗内掉电上限：≤2%。
pub const LID_WINDOW_DRAIN_CAP_PCT: u32 = 2;
/// 每分钟唤醒达标线：≤1 次。
pub const WAKE_PER_MIN_CAP: u32 = 1;
/// 自动回睡：唤醒后 60 秒无操作（可关）。
pub const AUTO_RESLEEP_IDLE_MS: u64 = 60_000;
/// 抖动判定：醒后 5s 内再睡。
pub const BOUNCE_WINDOW_MS: u64 = 5_000;
/// 抖动告警线：>3 次/小时。
pub const BOUNCE_ALERT_PER_HOUR: u32 = 3;
/// 事件账本保留 30 天（天粒度环形）。
pub const LOG_DAYS: usize = 30;
/// 最近唤醒列表容量（设置页显示 10 条）。
pub const RECENT_WAKE_CAP: usize = 10;

/// 唤醒源类别（白名单分级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WakeSource {
    /// 电源键——硬源（不可移除）。
    PowerButton,
    /// 盖开合——硬源。
    LidSwitch,
    /// USB 键鼠——软源（可逐个关）。
    UsbHid,
    /// RTC——仅限用户闹钟（F100 预约显式唤醒）。
    RtcAlarm,
    /// 局域网包唤醒（WoL）——默认关。
    WakeOnLan,
    /// 未知源（固件行为）——记录并建议排查。
    Unknown,
}

impl WakeSource {
    /// 硬源不可移除（白名单分级纪律）。
    pub fn is_hard_source(self) -> bool {
        matches!(self, WakeSource::PowerButton | WakeSource::LidSwitch)
    }

    /// 100% 可解释：所有类别都有解释路径（Unknown 也有——「固件行为待排查」）。
    pub fn explain(self) -> &'static str {
        match self {
            WakeSource::PowerButton => "用户按电源键唤醒",
            WakeSource::LidSwitch => "用户开盖唤醒",
            WakeSource::UsbHid => "USB 键鼠输入唤醒",
            WakeSource::RtcAlarm => "用户预约闹钟唤醒（F100）",
            WakeSource::WakeOnLan => "网络包唤醒（WoL，默认关闭）",
            WakeSource::Unknown => "未知源（固件行为）——建议排查驱动/固件设置",
        }
    }
}

/// 唤醒事件（时间/来源/耗时——设置页最近 10 条的字段）。
#[derive(Clone, Copy, Debug)]
pub struct WakeEvent {
    pub at_ms: u64,
    pub source: WakeSource,
    /// 本次唤醒处理耗时（ms）。
    pub resume_ms: u32,
    /// 是否通过白名单（非法源 = 治理对象）。
    pub allowed: bool,
}

// ---------------------------------------------------------------------------
// 治理器
// ---------------------------------------------------------------------------

/// 唤醒源治理器。
pub struct WakeGovernor {
    /// 软源开关（硬源永开——不可移除纪律）。
    usb_hid_enabled: bool,
    wol_enabled: bool,
    /// RTC 白名单：仅限已预约闹钟。
    rtc_alarm_armed: bool,
    /// 自动回睡开关（可关——主册明文）。
    auto_resleep_enabled: bool,
    /// 事件账本（环形 10 条近期视图；30 天按天聚合账）。
    recent: [Option<WakeEvent>; RECENT_WAKE_CAP],
    recent_head: usize,
    recent_n: usize,
    daily_wakes: [u16; LOG_DAYS],
    daily_tag: [u16; LOG_DAYS],
    /// 抖动账（当前小时内次数）。
    bounce_this_hour: u32,
    bounce_hour_tag: u64,
    bounce_alert: bool,
    /// 合盖观察窗状态（2h 唤醒/掉电双指标）。
    lid_since_ms: Option<u64>,
    lid_wakes: u32,
    lid_drain_permille: u32,
    now_ms: u64,
}

impl WakeGovernor {
    pub const fn new() -> Self {
        WakeGovernor {
            usb_hid_enabled: true,
            wol_enabled: false, // WoL 默认关（主册明文）
            rtc_alarm_armed: false,
            auto_resleep_enabled: true,
            recent: [None; RECENT_WAKE_CAP],
            recent_head: 0,
            recent_n: 0,
            daily_wakes: [0; LOG_DAYS],
            daily_tag: [0xFFFF; LOG_DAYS],
            bounce_this_hour: 0,
            bounce_hour_tag: u64::MAX,
            bounce_alert: false,
            lid_since_ms: None,
            lid_wakes: 0,
            lid_drain_permille: 0,
            now_ms: 0,
        }
    }

    /// 白名单裁决：唤醒源到来 → 是否放行。
    /// 硬源永通；软源按开关；RTC 仅已预约闹钟；WoL 默认关；Unknown 记录不放行。
    pub fn admiss(&mut self, source: WakeSource, at_ms: u64) -> bool {
        self.now_ms = at_ms;
        let allowed = match source {
            WakeSource::PowerButton | WakeSource::LidSwitch => true,
            WakeSource::UsbHid => self.usb_hid_enabled,
            WakeSource::RtcAlarm => self.rtc_alarm_armed,
            WakeSource::WakeOnLan => self.wol_enabled,
            WakeSource::Unknown => false,
        };
        // 非法源也要记录（唤醒原因 100% 可解释——含被拒的尝试）。
        self.log_event(WakeEvent { at_ms, source, resume_ms: 0, allowed });
        if allowed {
            self.lid_wakes = self.lid_wakes.saturating_add(1);
            self.bump_daily(at_ms);
            self.check_bounce(at_ms);
        }
        allowed
    }

    fn log_event(&mut self, e: WakeEvent) {
        self.recent[self.recent_head] = Some(e);
        self.recent_head = (self.recent_head + 1) % RECENT_WAKE_CAP;
        self.recent_n = (self.recent_n + 1).min(RECENT_WAKE_CAP);
    }

    fn bump_daily(&mut self, at_ms: u64) {
        let day = at_ms / 86_400_000;
        let di = (day % LOG_DAYS as u64) as usize;
        let tag = (day % 0xFFFE) as u16;
        if self.daily_tag[di] != tag {
            self.daily_tag[di] = tag;
            self.daily_wakes[di] = 0;
        }
        self.daily_wakes[di] = self.daily_wakes[di].saturating_add(1);
    }

    /// 抖动保护：醒后 5s 内再睡（= 短时间内第二次唤醒）计数；>3 次/小时告警。
    fn check_bounce(&mut self, at_ms: u64) {
        let hour = at_ms / 3_600_000;
        if self.bounce_hour_tag != hour {
            self.bounce_hour_tag = hour;
            self.bounce_this_hour = 0;
        }
        // 与上一事件间隔 <5s 视为抖动。
        if self.recent_n >= 2 {
            let last_idx = (self.recent_head + RECENT_WAKE_CAP - 1) % RECENT_WAKE_CAP;
            let prev_idx = (self.recent_head + RECENT_WAKE_CAP - 2) % RECENT_WAKE_CAP;
            if let (Some(a), Some(b)) = (self.recent[last_idx], self.recent[prev_idx]) {
                if a.at_ms.saturating_sub(b.at_ms) < BOUNCE_WINDOW_MS {
                    self.bounce_this_hour += 1;
                    if self.bounce_this_hour > BOUNCE_ALERT_PER_HOUR {
                        self.bounce_alert = true;
                    }
                }
            }
        }
    }

    /// 软源管理（硬源调用 = 拒绝——不可移除纪律）。
    pub fn set_soft_source(&mut self, source: WakeSource, enabled: bool) -> bool {
        match source {
            WakeSource::UsbHid => {
                self.usb_hid_enabled = enabled;
                true
            }
            WakeSource::WakeOnLan => {
                self.wol_enabled = enabled;
                true
            }
            WakeSource::RtcAlarm => {
                self.rtc_alarm_armed = enabled;
                true
            }
            _ => false, // 硬源与 Unknown 不可配置
        }
    }

    pub fn set_auto_resleep(&mut self, enabled: bool) {
        self.auto_resleep_enabled = enabled;
    }

    /// 自动回睡判定：唤醒后 60s 无操作 → 回睡（可关；纯函数口径）。
    pub fn should_resleep(&self, now_ms: u64, last_input_ms: u64) -> bool {
        self.auto_resleep_enabled && now_ms.saturating_sub(last_input_ms) >= AUTO_RESLEEP_IDLE_MS
    }

    /// 合盖观察窗开启（合盖时刻）。
    pub fn begin_lid_window(&mut self, at_ms: u64) {
        self.lid_since_ms = Some(at_ms);
        self.lid_wakes = 0;
        self.lid_drain_permille = 0;
    }

    /// 掉电采样（观察窗内：千分比累计）。
    pub fn sample_drain(&mut self, drain_permille_delta: u32) {
        self.lid_drain_permille = self.lid_drain_permille.saturating_add(drain_permille_delta);
    }

    /// 合盖观察窗结算：唤醒 ≤2 次且掉电 ≤2%（主册判据）。
    pub fn lid_window_verdict(&self) -> (u32, u32, bool) {
        // 2% = 千分之 20。
        let pass = self.lid_wakes <= LID_WINDOW_WAKE_CAP && self.lid_drain_permille <= LID_WINDOW_DRAIN_CAP_PCT * 10;
        (self.lid_wakes, self.lid_drain_permille, pass)
    }

    /// 每分钟唤醒达标线查询（≤1 次/分钟）：
    /// 折算速率 = 唤醒次数 × 60_000 / 窗口 ms，与上限比较。
    pub fn wakes_per_min_ok(&self, window_ms: u64, wakes: u32) -> bool {
        wakes as u64 * 60_000 / window_ms.max(1) <= WAKE_PER_MIN_CAP as u64
    }

    /// 最近唤醒列表（时间/来源/耗时——设置页 10 条）。
    pub fn recent_events(&self) -> impl Iterator<Item = WakeEvent> + '_ {
        let start = (self.recent_head + RECENT_WAKE_CAP - self.recent_n) % RECENT_WAKE_CAP;
        (0..self.recent_n).filter_map(move |i| self.recent[(start + i) % RECENT_WAKE_CAP])
    }

    pub fn bounce_alert(&self) -> bool {
        self.bounce_alert
    }

    /// 唤醒原因可解释率（100% 判据：每条事件都有解释串）。
    pub fn explain_rate_permille(&self) -> u32 {
        if self.recent_n == 0 {
            return 0;
        }
        let explained = self.recent_events().filter(|e| !e.source.explain().is_empty()).count();
        (explained * 1000 / self.recent_n) as u32
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_wakegov_checks() -> CheckSet {
    let mut cs = CheckSet::new("F065-wakegov");
    // 1) 默认白名单：电源键/盖开合通；WoL 默认关；未知源拒。
    let mut g = WakeGovernor::new();
    cs.add("power_btn_allowed", g.admiss(WakeSource::PowerButton, 1_000), "");
    cs.add("lid_allowed", g.admiss(WakeSource::LidSwitch, 2_000), "");
    cs.add("wol_default_off", !g.admiss(WakeSource::WakeOnLan, 3_000), "");
    cs.add("unknown_denied", !g.admiss(WakeSource::Unknown, 4_000), "");
    // 2) 硬源不可移除：set_soft_source 拒绝。
    cs.add("hard_source_immovable", !g.set_soft_source(WakeSource::PowerButton, false), "");
    // 3) 软源可逐个关：USB 键鼠关 → 拒唤醒；开 → 放行。
    cs.add("usb_soft_toggle_off", g.set_soft_source(WakeSource::UsbHid, false) && !g.admiss(WakeSource::UsbHid, 5_000), "");
    cs.add("usb_soft_toggle_on", g.set_soft_source(WakeSource::UsbHid, true) && g.admiss(WakeSource::UsbHid, 6_000), "");
    // 4) RTC 仅限已预约闹钟。
    cs.add("rtc_needs_reservation", !g.admiss(WakeSource::RtcAlarm, 7_000), "");
    cs.add("rtc_with_reservation", g.set_soft_source(WakeSource::RtcAlarm, true) && g.admiss(WakeSource::RtcAlarm, 8_000), "");
    // 5) 被拒事件也在账（100% 可解释含拒）：power/lid/wol拒/unknown拒/
    //    usb拒/usb许/rtc拒/rtc许 = 8 条。
    let total: usize = g.recent_events().count();
    let explained_permille = g.explain_rate_permille();
    cs.add("denied_still_logged", total == 8 && explained_permille == 1000, "");
    // 6) 合盖 2 小时：0 唤醒 0 掉电 → 过；2 唤醒 + 1.5% → 过；3 唤醒 → 不过。
    let mut g6 = WakeGovernor::new();
    g6.begin_lid_window(0);
    let (_, drain6, pass6) = g6.lid_window_verdict();
    cs.add("lid_window_quiet_pass", pass6 && drain6 == 0, "");
    let mut g6c = WakeGovernor::new();
    g6c.begin_lid_window(0);
    let _ = g6c.admiss(WakeSource::PowerButton, 10_000);
    let _ = g6c.admiss(WakeSource::LidSwitch, LID_WINDOW_MS / 2);
    g6c.sample_drain(15); // 1.5%
    let (_, drain6c, pass6c) = g6c.lid_window_verdict();
    cs.add("lid_window_2wakes_15pct_pass", pass6c && drain6c == 15, "");
    let mut g6d = WakeGovernor::new();
    g6d.begin_lid_window(0);
    for k in 0..3u64 {
        let _ = g6d.admiss(WakeSource::PowerButton, 10_000 + k * 1_000_000);
    }
    let (_, _, pass6d) = g6d.lid_window_verdict();
    cs.add("lid_window_3wakes_fail", !pass6d, "");
    // 7) 每分钟唤醒 ≤1：2 次/分钟超线。
    let g7 = WakeGovernor::new();
    cs.add("wakes_per_min_cap", g7.wakes_per_min_ok(60_000, 1) && !g7.wakes_per_min_ok(60_000, 2), "");
    // 8) 抖动保护：5s 内双醒计抖动；>3 次/小时告警。
    let mut g8 = WakeGovernor::new();
    let _ = g8.admiss(WakeSource::PowerButton, 1_000_000);
    let _ = g8.admiss(WakeSource::PowerButton, 1_003_000); // 3s 后 → 抖动
    let _ = g8.admiss(WakeSource::PowerButton, 1_006_000);
    let _ = g8.admiss(WakeSource::PowerButton, 1_009_000);
    let _ = g8.admiss(WakeSource::PowerButton, 1_012_000); // 第 4 次抖动 → 告警
    cs.add("bounce_alert_over_3_per_hour", g8.bounce_alert(), "");
    // 9) 自动回睡：60s 无操作 → 回睡；关闭后不回。
    let mut g9 = WakeGovernor::new();
    let _ = g9.admiss(WakeSource::PowerButton, 0);
    cs.add("auto_resleep_after_60s", g9.should_resleep(61_000, 0), "");
    g9.set_auto_resleep(false);
    cs.add("auto_resleep_cancellable", !g9.should_resleep(61_000, 0), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_ring_keeps_10() {
        let mut g = WakeGovernor::new();
        for k in 0..15u64 {
            let _ = g.admiss(WakeSource::PowerButton, k * 1_000_000);
        }
        assert_eq!(g.recent_events().count(), RECENT_WAKE_CAP);
        // 最旧出环：最早可见事件为第 6 次（k=5）。
        let first = g.recent_events().next().unwrap();
        assert_eq!(first.at_ms, 5_000_000);
    }

    #[test]
    fn explain_strings_nonempty_for_all_sources() {
        // 100% 可解释：六类全部有解释（含 Unknown 的排查指引）。
        for s in [
            WakeSource::PowerButton,
            WakeSource::LidSwitch,
            WakeSource::UsbHid,
            WakeSource::RtcAlarm,
            WakeSource::WakeOnLan,
            WakeSource::Unknown,
        ] {
            assert!(!s.explain().is_empty());
        }
    }

    #[test]
    fn bounce_within_5s_only() {
        let mut g = WakeGovernor::new();
        let _ = g.admiss(WakeSource::PowerButton, 1_000_000);
        let _ = g.admiss(WakeSource::PowerButton, 1_000_000 + BOUNCE_WINDOW_MS); // 恰 5s = 不算抖动
        assert!(!g.bounce_alert());
        assert_eq!(g.bounce_this_hour, 0);
    }

    #[test]
    fn daily_log_rolls_30_days() {
        let mut g = WakeGovernor::new();
        for d in 0..40u64 {
            let _ = g.admiss(WakeSource::PowerButton, d * 86_400_000);
        }
        // 30 天环形账只覆盖最近 30 天（40 天前的槽被覆盖）。
        let mut filled = 0usize;
        for i in 0..LOG_DAYS {
            if g.daily_tag[i] != 0xFFFF {
                filled += 1;
            }
        }
        assert_eq!(filled, LOG_DAYS);
    }

    #[test]
    fn drain_cap_boundary() {
        let mut g = WakeGovernor::new();
        g.begin_lid_window(0);
        g.sample_drain(20); // 恰 2.0% = 千分之 20 = 上限内（≤2%）
        let (_, drain, pass) = g.lid_window_verdict();
        assert_eq!(drain, 20);
        assert!(pass, "恰 2% 达标（判据 ≤2%）");
        g.sample_drain(1); // 2.1% 越线
        let (_, _, pass2) = g.lid_window_verdict();
        assert!(!pass2);
    }
}
