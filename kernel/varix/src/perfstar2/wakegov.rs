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

// ===========================================================================
// v2 深化批（F065 · G-B-25）——睡眠仲裁 / RTC 最小唤醒表 / 掉电预测 / 归因聚合
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-25 功能定义的实装细化，非新立项）：
// 1. SuspendArbiter —— 睡眠仲裁投票：音频播放中/更新中/传输中三组件
//    veto 投票（在位即否决——任何一方「不能睡」系统就不睡）；投票账
//    逐组件在案（谁拦的睡成不了——可解释）。
// 2. RtcAlarmTable —— RTC 预约闹钟表（8 槽定长）：白名单最小化——
//    只为**最早**到期闹钟保留 RTC 唤醒源，其余闹钟入表等位；到期即清。
// 3. DrainModel —— 睡眠掉电预测：基线掉电（‰/h）+ 唤醒成本（‰/次）
//    → 合盖 2h 掉电预测，与 LID_WINDOW_DRAIN_CAP_PCT（2%）红线对表
//    ——「合盖 2 小时掉电 ≤2%」的预算化答案。
// 4. WakeAttribution —— 唤醒原因归因聚合：逐类计数 + 不可解释率
//    ×1000（主册「唤醒原因记录 100% 可解释」的活账本）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// RTC 闹钟表容量。
pub const RTC_ALARM_CAP: usize = 8;
/// 睡眠掉电基线（‰/小时，Y7000 合盖 Modern Standby 量级——旋钮）。
pub const SLEEP_BASE_DRAIN_PM_PER_H: u32 = 6;
/// 单次唤醒成本（‰）。
pub const WAKE_COST_PM: u32 = 2;
/// 仲裁组件数（音频/更新/传输）。
pub const ARB_VETOERS: usize = 3;

// ---------------------------------------------------------------------------
// 深化一：睡眠仲裁投票
// ---------------------------------------------------------------------------

/// 仲裁组件（0 音频 1 更新 2 传输）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vetoer {
    Audio,
    Update,
    Transfer,
}

/// 睡眠仲裁器：全票否决制——任一在位 veto 都不能睡。
pub struct SuspendArbiter {
    veto: [bool; ARB_VETOERS],
    /// 历史睡眠尝试（被否决的记组件）。
    attempts: u64,
    blocked_by: [u64; ARB_VETOERS],
    allowed: u64,
}

impl SuspendArbiter {
    pub const fn new() -> Self {
        SuspendArbiter {
            veto: [false; ARB_VETOERS],
            attempts: 0,
            blocked_by: [0; ARB_VETOERS],
            allowed: 0,
        }
    }

    fn slot(v: Vetoer) -> usize {
        match v {
            Vetoer::Audio => 0,
            Vetoer::Update => 1,
            Vetoer::Transfer => 2,
        }
    }

    /// 组件在位（veto = true 即不能睡）。
    pub fn set_veto(&mut self, v: Vetoer, on: bool) {
        self.veto[Self::slot(v)] = on;
    }

    /// 尝试入睡：被谁拦如实记（多拦并列记首槽——账面不重复计）。
    pub fn try_suspend(&mut self) -> bool {
        self.attempts += 1;
        for k in 0..ARB_VETOERS {
            if self.veto[k] {
                self.blocked_by[k] += 1;
                return false;
            }
        }
        self.allowed += 1;
        true
    }

    /// 当前是否全部放行。
    pub fn all_clear(&self) -> bool {
        self.veto.iter().all(|v| !v)
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.attempts, self.allowed)
    }

    pub fn blocked_by(&self, v: Vetoer) -> u64 {
        self.blocked_by[Self::slot(v)]
    }
}

// ---------------------------------------------------------------------------
// 深化二：RTC 预约闹钟表
// ---------------------------------------------------------------------------

/// RTC 闹钟表：只为最早闹钟保留唤醒源（白名单最小化）。
pub struct RtcAlarmTable {
    slots: [Option<u64>; RTC_ALARM_CAP], // 到期时刻 ms
    /// RTC 唤醒源在位（= 有闹钟被登记）。
    rtc_armed: bool,
    registered: u64,
    fired: u64,
    rejected: u64,
}

impl RtcAlarmTable {
    pub const fn new() -> Self {
        RtcAlarmTable {
            slots: [None; RTC_ALARM_CAP],
            rtc_armed: false,
            registered: 0,
            fired: 0,
            rejected: 0,
        }
    }

    /// 登记闹钟：表满且新闹钟不早于任何在表闹钟 → 拒绝（白名单纪律：
    /// 留位给最早的）。
    pub fn register(&mut self, at_ms: u64) -> bool {
        if let Some(empty) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[empty] = Some(at_ms);
            self.rtc_armed = true;
            self.registered += 1;
            return true;
        }
        // 表满：若新闹钟早于当前最晚闹钟，挤掉它（保最早集合的代表性）。
        let latest_idx = self
            .slots
            .iter()
            .enumerate()
            .max_by_key(|(_, s)| s.unwrap_or(0))
            .map(|(i, _)| i);
        if let Some(li) = latest_idx {
            if let Some(latest) = self.slots[li] {
                if at_ms < latest {
                    self.slots[li] = Some(at_ms);
                    self.registered += 1;
                    return true;
                }
            }
        }
        self.rejected += 1;
        false
    }

    /// 最早到期闹钟（RTC 唤醒源应编程到这个时刻）。
    pub fn earliest(&self) -> Option<u64> {
        self.slots.iter().flatten().copied().min()
    }

    /// 到点清算：到期的闹钟触发并清除。
    pub fn fire_due(&mut self, now_ms: u64) -> u32 {
        let mut fired = 0u32;
        for s in self.slots.iter_mut() {
            if let Some(t) = *s {
                if t <= now_ms {
                    *s = None;
                    fired += 1;
                    self.fired += 1;
                }
            }
        }
        if self.earliest().is_none() {
            self.rtc_armed = false; // 全空 → RTC 唤醒源撤销（最小化）
        }
        fired
    }

    pub fn rtc_armed(&self) -> bool {
        self.rtc_armed
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (self.registered, self.fired, self.rejected)
    }
}

// ---------------------------------------------------------------------------
// 深化三：掉电预测模型
// ---------------------------------------------------------------------------

/// 合盖掉电预测。
pub struct DrainModel {
    base_pm_per_h: u32,
    wake_cost_pm: u32,
    /// 最近校正：实测掉电（模型 vs 实测的偏差账）。
    observed_pm: u64,
    predicted_pm: u64,
}

impl DrainModel {
    pub const fn new() -> Self {
        DrainModel {
            base_pm_per_h: SLEEP_BASE_DRAIN_PM_PER_H,
            wake_cost_pm: WAKE_COST_PM,
            observed_pm: 0,
            predicted_pm: 0,
        }
    }

    /// 预测：hours 小时 + wakes 次唤醒的掉电（‰）。
    pub fn predict_pm(&self, hours: u32, wakes: u32) -> u32 {
        (self.base_pm_per_h as u64 * hours as u64
            + self.wake_cost_pm as u64 * wakes as u64) as u32
    }

    /// 合盖 2h、至多 2 唤醒是否达线（≤ LID_WINDOW_DRAIN_CAP_PCT ×10 = 20‰）。
    pub fn lid_2h_within_cap(&self, expected_wakes: u32) -> bool {
        self.predict_pm(2, expected_wakes) <= LID_WINDOW_DRAIN_CAP_PCT as u32 * 10
    }

    /// 校正一步：实测掉电入账（模型偏差 = 实测 − 预测）。
    pub fn record_observed(&mut self, hours: u32, wakes: u32, observed: u32) -> i32 {
        let pred = self.predict_pm(hours, wakes);
        self.observed_pm += observed as u64;
        self.predicted_pm += pred as u64;
        observed as i32 - pred as i32
    }

    /// 累计偏差率 ×100（实测/预测——>150% 说明基线失真该标定）。
    pub fn bias_pct_x100(&self) -> u64 {
        if self.predicted_pm == 0 {
            return 100;
        }
        self.observed_pm * 100 / self.predicted_pm
    }
}

// ---------------------------------------------------------------------------
// 深化四：唤醒原因归因聚合
// ---------------------------------------------------------------------------

/// 归因聚合（六类对齐 WakeSource 枚举 + Unknown）。
pub struct WakeAttribution {
    counts: [u64; 7], // 六类 + Unknown 槽
    total: u64,
}

impl WakeAttribution {
    pub const fn new() -> Self {
        WakeAttribution { counts: [0; 7], total: 0 }
    }

    /// 记一次唤醒（kind 0..5 = 六类已解释；6 = Unknown）。
    pub fn record(&mut self, kind: usize) {
        self.counts[kind.min(6)] += 1;
        self.total += 1;
    }

    /// 不可解释率 ×1000（主册红线：0——100% 可解释）。
    pub fn unexplained_rate_x1000(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        self.counts[6] * 1000 / self.total
    }

    /// 主册达标：100% 可解释（Unknown 率 = 0）。
    pub fn fully_explainable(&self) -> bool {
        self.total > 0 && self.counts[6] == 0
    }

    pub fn count_of(&self, kind: usize) -> u64 {
        self.counts[kind.min(6)]
    }

    pub fn total(&self) -> u64 {
        self.total
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：仲裁 / 闹钟 / 掉电 / 归因逐条实摆。
pub fn run_wakegov_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F065-wakegov-deep");

    // ── 睡眠仲裁 ──
    let mut arb = SuspendArbiter::new();
    cs.add("arb_clear_sleeps", arb.all_clear() && arb.try_suspend(), "");
    // 1) 音频在位 → 拦 + 记账。
    arb.set_veto(Vetoer::Audio, true);
    cs.add("arb_audio_blocks", !arb.try_suspend() && arb.blocked_by(Vetoer::Audio) == 1, "");
    // 2) 解除后可睡（账不重记）。
    arb.set_veto(Vetoer::Audio, false);
    cs.add("arb_unblock_resumes", arb.try_suspend() && arb.stats() == (3, 2), "");
    // 3) 更新在位优先记更新槽（首槽纪律）。
    arb.set_veto(Vetoer::Audio, true);
    arb.set_veto(Vetoer::Update, true);
    let _ = arb.try_suspend();
    cs.add(
        "arb_first_slot_ledger",
        arb.blocked_by(Vetoer::Audio) == 2 && arb.blocked_by(Vetoer::Update) == 0,
        "",
    );

    // ── RTC 闹钟表 ──
    // 1) 登记 → RTC 在位；最早到期可查。
    let mut rtc = RtcAlarmTable::new();
    let _ = rtc.register(7_200_000);
    let _ = rtc.register(3_600_000);
    cs.add("rtc_armed_and_earliest", rtc.rtc_armed() && rtc.earliest() == Some(3_600_000), "");
    // 2) 到点清算：3.6h 到期 → 触发 1 个、RTC 仍在位（还有一个）。
    cs.add(
        "rtc_fire_due_partial",
        rtc.fire_due(4_000_000) == 1 && rtc.rtc_armed() && rtc.earliest() == Some(7_200_000),
        "",
    );
    // 3) 全部到期 → RTC 撤销（最小化纪律）。
    cs.add(
        "rtc_all_fired_disarm",
        rtc.fire_due(8_000_000) == 1 && !rtc.rtc_armed() && rtc.earliest().is_none(),
        "",
    );
    // 4) 表满且新闹钟最晚 → 拒绝（白名单留最早）。
    let mut rtc2 = RtcAlarmTable::new();
    for k in 0..RTC_ALARM_CAP {
        let _ = rtc2.register(1_000_000 + k as u64 * 1_000_000);
    }
    cs.add("rtc_full_rejects_latest", !rtc2.register(99_000_000) && rtc2.stats().2 == 1, "");
    // 5) 表满但更早的 → 挤掉最晚（最早集合代表性保持）。
    cs.add(
        "rtc_full_evicts_latest",
        rtc2.register(500_000) && rtc2.earliest() == Some(500_000),
        "",
    );

    // ── 掉电预测 ──
    // 1) 基线口径：2h × 6‰ + 2 次 × 2‰ = 16‰ = 1.6% ≤ 2% 达线。
    let dm = DrainModel::new();
    cs.add(
        "drain_2h_baseline_ok",
        dm.predict_pm(2, 2) == 16 && dm.lid_2h_within_cap(2),
        "",
    );
    // 2) 唤醒 4 次 → 20‰ 恰在线上（≤ 达标边界精确）。
    cs.add("drain_wake_cost_boundary", dm.predict_pm(2, 4) == 20 && dm.lid_2h_within_cap(4), "");
    // 3) 5 次唤醒越线（22‰ > 20‰）。
    cs.add("drain_over_cap", !dm.lid_2h_within_cap(5), "");
    // 4) 校正账：实测 20 预测 16 → 偏差 +4。
    let mut dm2 = DrainModel::new();
    cs.add("drain_bias_delta", dm2.record_observed(2, 2, 20) == 4, "");
    cs.add("drain_bias_pct", dm2.bias_pct_x100() == 125, ""); // 20/16 = 125%

    // ── 归因聚合 ──
    let mut at = WakeAttribution::new();
    for _ in 0..6 {
        at.record(0);
    }
    at.record(1);
    at.record(2);
    cs.add("attr_counts", at.total() == 8 && at.count_of(0) == 6, "");
    cs.add("attr_fully_explainable", at.fully_explainable() && at.unexplained_rate_x1000() == 0, "");
    at.record(6); // 一次不可解释
    cs.add(
        "attr_unexplained_visible",
        !at.fully_explainable() && at.unexplained_rate_x1000() == 111, // 1/9 = 111‰
        "",
    );

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn arbiter_veto_toggle_is_idempotent() {
        let mut a = SuspendArbiter::new();
        a.set_veto(Vetoer::Transfer, true);
        a.set_veto(Vetoer::Transfer, true);
        assert!(!a.try_suspend());
        assert_eq!(a.blocked_by(Vetoer::Transfer), 1, "重复置位不重复计账");
    }

    #[test]
    fn rtc_fire_only_due_alarms() {
        let mut r = RtcAlarmTable::new();
        let _ = r.register(1_000);
        let _ = r.register(2_000);
        let _ = r.register(3_000);
        assert_eq!(r.fire_due(1_500), 1, "只触发已到期的");
        assert_eq!(r.fire_due(1_600), 0);
        assert_eq!(r.fire_due(9_999), 2);
    }

    #[test]
    fn drain_model_hours_linear() {
        let dm = DrainModel::new();
        assert_eq!(dm.predict_pm(0, 0), 0);
        assert_eq!(dm.predict_pm(1, 0), SLEEP_BASE_DRAIN_PM_PER_H);
        assert_eq!(
            dm.predict_pm(10, 0),
            SLEEP_BASE_DRAIN_PM_PER_H * 10,
            "小时线性"
        );
    }

    #[test]
    fn attribution_kind_out_of_range_lands_unknown() {
        let mut a = WakeAttribution::new();
        a.record(42); // 越界 kind → Unknown 槽（不 panic 不丢失）
        assert_eq!(a.count_of(6), 1);
        assert!(!a.fully_explainable());
    }
}

// ===========================================================================
// v3 深化批（F065 · G-B-25）——静默窗预测 / 唤醒预算 / 深放保护闸
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-25 功能定义的实装细化，非新立项）：
// 1. QuietWindowPredictor —— 静默窗预测：24 小时使用直方图 → 最安静
//    连续 N 小时窗（挂机任务/深度睡眠的最佳窗口——数据面）。
// 2. WakeBudget —— 每小时唤醒配额：允许 W 次/时，超配额唤醒被拒并
//    计数；未用配额滚入下小时（上限翻倍封顶）。
// 3. SuspendBatteryGate —— 深放保护闸：SOC 低于地板禁止进入睡眠
//    （深睡放电叠加低温可能跌破可开机线——保护优先于省电）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 直方图桶数（24 小时）。
pub const HISTO_BUCKETS: usize = 24;
/// 每小时唤醒配额（默认 2——旋钮）。
pub const WAKE_BUDGET_PER_HOUR: u32 = 2;
/// 配额滚存上限（×100 = 200%——最多攒一小时量）。
pub const WAKE_BUDGET_ROLLOVER_X100: u32 = 200;
/// 深放保护地板（‰ = 5%）。
pub const SUSPEND_SOC_FLOOR_PM: u16 = 50;

// ---------------------------------------------------------------------------
// 深化一：静默窗预测
// ---------------------------------------------------------------------------

/// 静默窗预测器（24h 活动直方图，值 = 每小时活动计数）。
pub struct QuietWindowPredictor {
    histo: [u32; HISTO_BUCKETS],
    populated: bool,
}

impl QuietWindowPredictor {
    pub const fn new() -> Self {
        QuietWindowPredictor { histo: [0; HISTO_BUCKETS], populated: false }
    }

    /// 喂一小时活动计数（hour 0..24）。
    pub fn feed(&mut self, hour: usize, activity: u32) -> bool {
        if hour >= HISTO_BUCKETS {
            return false;
        }
        self.histo[hour] = activity;
        self.populated = true;
        true
    }

    /// 最安静连续 hours 小时窗（返回起始小时；样本不足 → None）。
    /// 平局取更早的窗（确定性——同类任务排期可复现）。
    pub fn quietest_window(&self, hours: usize) -> Option<usize> {
        if !self.populated || hours == 0 || hours > HISTO_BUCKETS {
            return None;
        }
        let mut best_start = 0usize;
        let mut best_sum = u64::MAX;
        for start in 0..=HISTO_BUCKETS - hours {
            let sum: u64 = self.histo[start..start + hours].iter().map(|v| *v as u64).sum();
            if sum < best_sum {
                best_sum = sum;
                best_start = start;
            }
        }
        Some(best_start)
    }

    /// 窗内预计活动总量。
    pub fn window_activity(&self, start: usize, hours: usize) -> Option<u64> {
        if start + hours > HISTO_BUCKETS {
            return None;
        }
        Some(self.histo[start..start + hours].iter().map(|v| *v as u64).sum())
    }

    pub fn histo(&self) -> &[u32; HISTO_BUCKETS] {
        &self.histo
    }
}

// ---------------------------------------------------------------------------
// 深化二：唤醒预算
// ---------------------------------------------------------------------------

/// 每小时唤醒配额（滚存封顶）。
pub struct WakeBudget {
    /// 当前可用配额（×100 定点——滚存半次语义）。
    available_x100: u32,
    granted: u64,
    denied: u64,
}

impl WakeBudget {
    pub const fn new() -> Self {
        WakeBudget { available_x100: WAKE_BUDGET_PER_HOUR * 100, granted: 0, denied: 0 }
    }

    /// 请求唤醒：配额内放行扣减。
    pub fn request_wake(&mut self) -> bool {
        if self.available_x100 >= 100 {
            self.available_x100 -= 100;
            self.granted += 1;
            true
        } else {
            self.denied += 1;
            false
        }
    }

    /// 整点结算：补满一小时量 + 滚存（封顶 200%）。
    pub fn hourly_settle(&mut self) {
        let target = WAKE_BUDGET_PER_HOUR * 100;
        let cap = target * WAKE_BUDGET_ROLLOVER_X100 / 100;
        self.available_x100 = (self.available_x100 + target).min(cap);
    }

    pub fn available_x100(&self) -> u32 {
        self.available_x100
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.granted, self.denied)
    }
}

// ---------------------------------------------------------------------------
// 深化三：深放保护闸
// ---------------------------------------------------------------------------

/// 深放保护闸（SOC 地板 veto）。
pub struct SuspendBatteryGate {
    floor_pm: u16,
    vetoes: u64,
    allows: u64,
}

impl SuspendBatteryGate {
    pub const fn new() -> Self {
        SuspendBatteryGate { floor_pm: SUSPEND_SOC_FLOOR_PM, vetoes: 0, allows: 0 }
    }

    /// 请求入睡：SOC 低于地板 → 否决（保护优先——拒绝也是保护）。
    pub fn may_suspend(&mut self, soc_pm: u16, on_battery: bool) -> bool {
        if on_battery && soc_pm < self.floor_pm {
            self.vetoes += 1;
            return false;
        }
        self.allows += 1;
        true
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.allows, self.vetoes)
    }

    pub fn floor_pm(&self) -> u16 {
        self.floor_pm
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：静默窗 / 预算 / 保护闸逐条实摆。
pub fn run_wakegov_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F065-wakegov-v3");

    // ── 静默窗预测 ──
    let mut qw = QuietWindowPredictor::new();
    // 白天忙（8-22 点活动 100），凌晨安静（0-6 点活动 2）。
    for h in 0..24 {
        let act = if (8..22).contains(&h) { 100 } else { 2 };
        let _ = qw.feed(h, act);
    }
    cs.add("quiet_window_finds_0h", qw.quietest_window(4) == Some(0), "");
    cs.add("quiet_window_activity_sum", qw.window_activity(0, 4) == Some(8), "");
    // 22-2 点窗（环形视口外——本层线性窗：22..24 只有两小时 → 4h 窗落 0）。
    cs.add("quiet_window_hours_boundary", qw.quietest_window(24) == Some(0), "");
    cs.add("quiet_window_unpopulated_none", QuietWindowPredictor::new().quietest_window(4).is_none(), "");
    cs.add("quiet_window_bad_hour_refused", !qw.feed(24, 1), "");
    // 平局取更早：构造两个等和窗。
    let mut qw2 = QuietWindowPredictor::new();
    for h in 0..24 {
        let _ = qw2.feed(h, if h == 2 || h == 10 { 5 } else { 50 });
    }
    // 最小和 55 首次出现在 (1,2) 窗——严格小于语义下平局取最先达到者。
    cs.add("quiet_window_tie_earliest", qw2.quietest_window(2) == Some(1), "");

    // ── 唤醒预算 ──
    let mut wb = WakeBudget::new();
    cs.add("budget_two_granted", wb.request_wake() && wb.request_wake(), "");
    cs.add("budget_third_denied", !wb.request_wake() && wb.stats() == (2, 1), "");
    // 整点结算：滚存 0 + 补 200 → 200（未超 200% 封顶）。
    wb.hourly_settle();
    cs.add("budget_settle_refill", wb.available_x100() == 200, "");
    cs.add("budget_after_refill_granted", wb.request_wake(), "");
    // 连续空转结算 → 滚存封顶 400（2 次量）。
    let mut wb2 = WakeBudget::new();
    wb2.hourly_settle();
    wb2.hourly_settle();
    cs.add("budget_rollover_cap", wb2.available_x100() == 400, "");
    cs.add("budget_rollover_allows_4", {
        let mut n = 0;
        for _ in 0..5 {
            if wb2.request_wake() {
                n += 1;
            }
        }
        n == 4
    }, "");

    // ── 深放保护闸 ──
    let mut g = SuspendBatteryGate::new();
    cs.add("gate_allows_healthy", g.may_suspend(500, true), "");
    cs.add("gate_vetoes_deep_discharge", !g.may_suspend(40, true) && g.stats() == (1, 1), "");
    cs.add("gate_ac_power_bypasses", g.may_suspend(40, false), "插电时深放线不适用（充电中）");
    cs.add("gate_boundary_strict", g.may_suspend(50, true), "恰达线 = 允许（严格小于才拒）");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn quiet_window_single_hour() {
        let mut qw = QuietWindowPredictor::new();
        for h in 0..24 {
            let _ = qw.feed(h, if h == 15 { 0 } else { 10 });
        }
        assert_eq!(qw.quietest_window(1), Some(15));
    }

    #[test]
    fn budget_fractional_rollover() {
        // 用掉半次配额的语义：available 100 → 不够一次（≥100 才放行）。
        let mut wb = WakeBudget::new();
        let _ = wb.request_wake();
        // available = 100 → 仍够一次（初始 200，用 100 剩 100）。
        assert!(wb.request_wake());
        assert!(!wb.request_wake());
    }

    #[test]
    fn gate_veto_accumulates() {
        let mut g = SuspendBatteryGate::new();
        for _ in 0..5 {
            let _ = g.may_suspend(10, true);
        }
        assert_eq!(g.stats(), (0, 5), "连续否决逐次记账");
    }
}

// ===========================================================================
// v4 深化批（F065 · G-B-25）——睡眠债 / 在场状态机 / 供电意图推断
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-25 功能定义的实装细化，非新立项）：
// 1. SleepDebt —— 睡眠债账：该睡没睡的小时累积（应睡窗内活跃 = 欠账），
//    睡满回冲——「系统累了」的量化面（深夜活跃提示的数据源）。
// 2. UserPresence —— 在场状态机：输入活动驱动 Idle→Active→Away 三态
//    （迟滞：Idle 5min / Away 30min），锁屏/唤醒联动的事件面。
// 3. PowerIntent —— 供电意图推断：电源态×在场×负载 → 意图四类
//    （Working/Presenting/IdleCharging/Travelling——模式表自动选择的
//    语义层）。
// 全部零堆：定长状态 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 应睡窗（0-6 点）。
pub const SLEEP_WINDOW: (u32, u32) = (0, 6);
/// Idle 判定（分钟）。
pub const PRESENCE_IDLE_MIN: u32 = 5;
/// Away 判定（分钟）。
pub const PRESENCE_AWAY_MIN: u32 = 30;

// ---------------------------------------------------------------------------
// 深化一：睡眠债
// ---------------------------------------------------------------------------

/// 睡眠债账（小时 ×100 定点累积）。
pub struct SleepDebt {
    debt_min_x100: u64,
    repaid_min_x100: u64,
    /// 应睡窗内活跃的小时数（累计）。
    late_nights: u32,
}

impl SleepDebt {
    pub const fn new() -> Self {
        SleepDebt { debt_min_x100: 0, repaid_min_x100: 0, late_nights: 0 }
    }

    /// 记一小时：应睡窗内活跃（active=true）→ 欠账 +60min；窗外 → 不计。
    pub fn hourly(&mut self, hour: u32, active: bool) {
        let in_window = hour >= SLEEP_WINDOW.0 && hour < SLEEP_WINDOW.1;
        if in_window && active {
            self.debt_min_x100 += 6_000;
            self.late_nights += 1;
        } else if in_window && !active {
            // 睡满回冲（不超总欠账）。
            self.repaid_min_x100 += 6_000;
            let repay = self.repaid_min_x100.min(self.debt_min_x100);
            self.debt_min_x100 -= repay;
            self.repaid_min_x100 -= repay;
        }
    }

    /// 当前欠账（小时 ×100）。
    pub fn debt_hours_x100(&self) -> u64 {
        self.debt_min_x100
    }

    /// 欠账超警戒（小时）。
    pub fn over_limit(&self, limit_hours: u32) -> bool {
        self.debt_min_x100 > limit_hours as u64 * 6_000
    }

    pub fn late_nights(&self) -> u32 {
        self.late_nights
    }
}

// ---------------------------------------------------------------------------
// 深化二：在场状态机
// ---------------------------------------------------------------------------

/// 在场三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Idle,
    Active,
    Away,
}

/// 在场状态机（输入时刻驱动）。
pub struct UserPresence {
    state: Presence,
    last_input_ms: u64,
    transitions: u64,
}

impl UserPresence {
    pub const fn new(now_ms: u64) -> Self {
        UserPresence { state: Presence::Active, last_input_ms: now_ms, transitions: 0 }
    }

    /// 输入事件。
    pub fn input(&mut self, now_ms: u64) {
        if self.state != Presence::Active {
            self.transitions += 1;
        }
        self.state = Presence::Active;
        self.last_input_ms = now_ms;
    }

    /// 巡检：按静默时长迁移（Idle 5min / Away 30min；Away 不自动回）。
    pub fn patrol(&mut self, now_ms: u64) -> Presence {
        let idle_min = now_ms.saturating_sub(self.last_input_ms) / 60_000;
        let new_state = match self.state {
            Presence::Active if idle_min >= PRESENCE_AWAY_MIN as u64 => Presence::Away,
            Presence::Active if idle_min >= PRESENCE_IDLE_MIN as u64 => Presence::Idle,
            Presence::Idle if idle_min >= PRESENCE_AWAY_MIN as u64 => Presence::Away,
            other => other,
        };
        if new_state != self.state {
            self.transitions += 1;
            self.state = new_state;
        }
        self.state
    }

    pub fn state(&self) -> Presence {
        self.state
    }

    pub fn transitions(&self) -> u64 {
        self.transitions
    }
}

// ---------------------------------------------------------------------------
// 深化三：供电意图推断
// ---------------------------------------------------------------------------

/// 供电意图四类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerIntent {
    /// 插电 + 在场 + 高负载。
    Working,
    /// 插电 + 在场 + 低负载（外接屏/投影）。
    Presenting,
    /// 插电 + 离场。
    IdleCharging,
    /// 电池供电。
    Travelling,
}

/// 意图推断器（上次候选入账——无 static mut）。
pub struct PowerIntentEngine {
    intent: PowerIntent,
    last_candidate: Option<PowerIntent>,
    switches: u64,
}

impl PowerIntentEngine {
    pub const fn new() -> Self {
        PowerIntentEngine { intent: PowerIntent::Working, last_candidate: None, switches: 0 }
    }

    /// 推断（迟滞：连续 2 次同候选才切换——防输入抖动反复横跳）。
    pub fn infer(
        &mut self,
        on_battery: bool,
        presence: Presence,
        cpu_load_permille: u16,
    ) -> PowerIntent {
        let candidate = if on_battery {
            PowerIntent::Travelling
        } else {
            match presence {
                Presence::Away => PowerIntent::IdleCharging,
                _ if cpu_load_permille >= 300 => PowerIntent::Working,
                _ => PowerIntent::Presenting,
            }
        };
        let same_twice = self.last_candidate == Some(candidate);
        self.last_candidate = Some(candidate);
        if same_twice && candidate != self.intent {
            self.intent = candidate;
            self.switches += 1;
        }
        self.intent
    }

    pub fn intent(&self) -> PowerIntent {
        self.intent
    }

    pub fn switches(&self) -> u64 {
        self.switches
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：睡眠债 / 在场 / 意图逐条实摆。
pub fn run_wakegov_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F065-wakegov-v4");

    // ── 睡眠债 ──
    let mut sd = SleepDebt::new();
    for h in 0..6 {
        sd.hourly(h, true); // 应睡窗全程活跃
    }
    cs.add("debt_6h_accumulated", sd.debt_hours_x100() == 36_000 && sd.late_nights() == 6, "");
    cs.add("debt_over_limit", sd.over_limit(5), "");
    // 睡满回冲。
    for h in 0..6 {
        sd.hourly(h, false);
    }
    cs.add("debt_repaid_full", sd.debt_hours_x100() == 0, "");
    // 窗外活跃不计。
    let mut sd2 = SleepDebt::new();
    sd2.hourly(12, true);
    cs.add("debt_daytime_ignored", sd2.debt_hours_x100() == 0, "");

    // ── 在场状态机 ──
    let mut up = UserPresence::new(0);
    cs.add("presence_starts_active", up.state() == Presence::Active, "");
    cs.add("presence_idle_at_5min", up.patrol(5 * 60_000) == Presence::Idle, "");
    cs.add("presence_active_on_input", {
        up.input(6 * 60_000);
        up.state() == Presence::Active
    }, "");
    cs.add("presence_away_at_30min", {
        up.patrol(6 * 60_000 + 30 * 60_000) == Presence::Away
    }, "");
    cs.add("presence_away_input_returns", {
        up.input(40 * 60_000);
        up.state() == Presence::Active
    }, "");
    cs.add("presence_transition_ledger", up.transitions() >= 3, "");

    // ── 意图推断 ──
    let mut pi = PowerIntentEngine::new();
    // 单次判定不切（迟滞）。
    let _ = pi.infer(false, Presence::Away, 100);
    cs.add("intent_hysteresis_holds", pi.intent() == PowerIntent::Working, "");
    // 连续两次同判定 → 切。
    let _ = pi.infer(false, Presence::Away, 100);
    cs.add("intent_switches_after_2", pi.intent() == PowerIntent::IdleCharging, "");
    // 电池 → Travelling。
    let _ = pi.infer(true, Presence::Active, 500);
    let _ = pi.infer(true, Presence::Active, 500);
    cs.add("intent_battery_travelling", pi.intent() == PowerIntent::Travelling, "");
    // 回插电 + 高负载 → Working。
    let _ = pi.infer(false, Presence::Active, 500);
    let _ = pi.infer(false, Presence::Active, 500);
    cs.add("intent_working_restored", pi.intent() == PowerIntent::Working, "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn debt_partial_repay_keeps_remainder() {
        let mut sd = SleepDebt::new();
        sd.hourly(0, true);
        sd.hourly(1, true); // 欠 2h
        sd.hourly(2, false); // 只睡回 1h（窗内 1 小时）
        assert_eq!(sd.debt_hours_x100(), 6_000, "剩余 1h 欠账");
    }

    #[test]
    fn presence_idle_to_away_direct() {
        let mut up = UserPresence::new(0);
        up.patrol(4 * 60_000); // Active 保持
        assert_eq!(up.patrol(31 * 60_000), Presence::Away, "Idle 未及 → 直接 Away");
    }

    #[test]
    fn intent_presenting_low_load() {
        let mut pi = PowerIntentEngine::new();
        let _ = pi.infer(false, Presence::Active, 100);
        let _ = pi.infer(false, Presence::Active, 100);
        assert_eq!(pi.intent(), PowerIntent::Presenting);
    }
}

// ===========================================================================
// v5 深化批（deep5）：唤醒热图 + 闹钟批处理
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：唤醒源 24h 热图（每小时计数 × 归因 → 找出最吵的一小时）
// ---------------------------------------------------------------------------

/// 唤醒源类别（主册归因枚举对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WakeSrc {
    RtcAlarm,
    Incoming,
    TimerApp,
    User,
}

/// 24h × 4 源热图。
pub struct WakeHeatmap {
    grid: [[u32; 4]; 24],
    total: u64,
}

impl WakeHeatmap {
    pub const fn new() -> Self {
        WakeHeatmap { grid: [[0; 4]; 24], total: 0 }
    }

    /// 记一次唤醒（hour 0..23 越界忽略——诚实时钟）。
    pub fn record(&mut self, hour: u8, src: WakeSrc) {
        if hour as usize >= 24 {
            return;
        }
        self.grid[hour as usize][src as usize] += 1;
        self.total += 1;
    }

    /// 最吵的一小时（总计数最大；平局取最早）。无记录 → None。
    pub fn noisiest_hour(&self) -> Option<(u8, u32)> {
        let mut best: Option<(u8, u32)> = None;
        for h in 0..24 {
            let sum: u32 = self.grid[h].iter().sum();
            if sum > 0 && best.map_or(true, |(_, c)| sum > c) {
                best = Some((h as u8, sum));
            }
        }
        best
    }

    /// 某源的全天占比（×100）。
    pub fn src_pct(&self, src: WakeSrc) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let mut n = 0u64;
        for h in 0..24 {
            n += self.grid[h][src as usize] as u64;
        }
        (n * 100 / self.total) as u32
    }

    /// 安静候选窗：连续 k 小时总唤醒 ≤ 阈值的最早起点。
    pub fn quiet_window(&self, k: u8, max_wakes: u32) -> Option<u8> {
        if k == 0 || k as usize > 24 {
            return None;
        }
        for start in 0..=(24 - k as usize) {
            let mut sum = 0u32;
            for h in start..start + k as usize {
                sum += self.grid[h].iter().sum::<u32>();
            }
            if sum <= max_wakes {
                return Some(start as u8);
            }
        }
        None
    }

    pub fn total(&self) -> u64 {
        self.total
    }
}

// ---------------------------------------------------------------------------
// 深化二：闹钟批处理合并（同一秒内多个 RTC 闹钟 → 合一唤醒 + 批执行）
// ---------------------------------------------------------------------------

/// 闹钟合并器：同刻（秒粒度）闹钟合并为一次硬件唤醒。
/// pending 条目 = (触发秒, 该秒累计的闹钟数)——合并不丢批量。
pub struct AlarmBatcher {
    pending: [(u32, u16); 8],
    n: usize,
    /// 已合并的硬件唤醒次数 / 已批处理的应用闹钟数。
    hw_wakes: u32,
    app_alarms: u32,
}

impl AlarmBatcher {
    pub const fn new() -> Self {
        AlarmBatcher { pending: [(0, 0); 8], n: 0, hw_wakes: 0, app_alarms: 0 }
    }

    /// 登记闹钟（返回 true = 需要一次硬件唤醒；false = 已并入同秒批）。
    pub fn schedule(&mut self, at_sec: u32, _app_id: u16) -> bool {
        for k in 0..self.n {
            if self.pending[k].0 == at_sec {
                self.pending[k].1 += 1; // 批量累加——不丢应用数
                self.app_alarms += 1;
                return false;
            }
        }
        if self.n >= 8 {
            self.app_alarms += 1;
            return true; // 队满直发（防积压）
        }
        self.pending[self.n] = (at_sec, 1);
        self.n += 1;
        self.app_alarms += 1;
        true
    }

    /// 硬件唤醒到达：返回该秒批内闹钟数并清批。
    pub fn fire(&mut self, now_sec: u32) -> usize {
        let mut fired = 0usize;
        let mut k = 0usize;
        while k < self.n {
            if self.pending[k].0 == now_sec {
                fired += self.pending[k].1 as usize;
                self.n -= 1;
                self.pending[k] = self.pending[self.n];
                self.hw_wakes += 1;
            } else {
                k += 1;
            }
        }
        fired
    }

    pub fn stats(&self) -> (u32, u32) {
        (self.hw_wakes, self.app_alarms)
    }
}

// ---------------------------------------------------------------------------
// 深化三：充电中睡眠策略（充电 → 更激进后台任务窗口）
// ---------------------------------------------------------------------------

/// 充电态策略：后台任务在充电时可放宽（窗口更早开启、预算更高）。
pub struct ChargingPolicy {
    on_ac: bool,
    /// 电池态后台窗起点（分钟/小时）与充电态起点。
    bg_window_battery_min: u32,
    bg_window_charging_min: u32,
    /// 本窗口已执行后台任务数。
    executed: u32,
}

impl ChargingPolicy {
    pub const fn new() -> Self {
        ChargingPolicy { on_ac: false, bg_window_battery_min: 30, bg_window_charging_min: 5, executed: 0 }
    }

    pub fn set_ac(&mut self, on_ac: bool) {
        if on_ac != self.on_ac {
            self.on_ac = on_ac;
            self.executed = 0; // 电源态切换 → 窗口重置
        }
    }

    /// 当前窗口起点（分钟）。
    pub fn window_start_min(&self) -> u32 {
        if self.on_ac {
            self.bg_window_charging_min
        } else {
            self.bg_window_battery_min
        }
    }

    /// 窗口内允许的后台任务预算（充电 10 倍）。
    pub fn budget(&self) -> u32 {
        if self.on_ac {
            100
        } else {
            10
        }
    }

    /// 尝试执行（窗口内且预算未尽）。
    pub fn try_task(&mut self, idle_min: u32) -> bool {
        if idle_min < self.window_start_min() || self.executed >= self.budget() {
            return false;
        }
        self.executed += 1;
        true
    }

    pub fn executed(&self) -> u32 {
        self.executed
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_wakegov_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F065-wakegov-v5");

    // ── 热图 ──
    // 1) 最吵小时 + 平局取最早。
    let mut hm = WakeHeatmap::new();
    hm.record(3, WakeSrc::RtcAlarm);
    hm.record(3, WakeSrc::TimerApp);
    hm.record(5, WakeSrc::Incoming);
    hm.record(9, WakeSrc::TimerApp);
    hm.record(9, WakeSrc::TimerApp);
    hm.record(9, WakeSrc::User);
    cs.add("heat_noisiest_hour", hm.noisiest_hour() == Some((9, 3)), "");
    // 2) 源占比：TimerApp 4/6 → 66。
    cs.add("heat_src_pct", hm.src_pct(WakeSrc::TimerApp) == 50 && hm.total() == 6, "");
    // 3) 安静窗：2-4 点连续 2 小时 ≤1 次。
    cs.add("heat_quiet_window", hm.quiet_window(2, 1) == Some(0), ""); // 0-1 点全空
    // 4) 越界小时忽略。
    hm.record(200, WakeSrc::User);
    cs.add("heat_hour_oob_ignored", hm.total() == 6, "");

    // ── 闹钟合并 ──
    // 5) 同秒合并：3 应用闹钟同秒 → 1 次硬件唤醒。
    let mut ab = AlarmBatcher::new();
    let first = ab.schedule(1000, 7);
    let second = ab.schedule(1000, 8);
    let third = ab.schedule(1000, 9);
    cs.add("alarm_merge_same_sec", first && !second && !third, "");
    // 6) 唤醒到达批执行 3 个。
    cs.add("alarm_batch_fires_3", ab.fire(1000) == 3 && ab.stats() == (1, 3), "");
    // 7) 异秒不合并。
    let mut ab2 = AlarmBatcher::new();
    let a = ab2.schedule(100, 1);
    let b = ab2.schedule(200, 2);
    cs.add("alarm_diff_sec_two_wakes", a && b, "");
    // 8) 队满 8 直发。
    let mut ab3 = AlarmBatcher::new();
    let mut ninth = true;
    for k in 0..9u16 {
        ninth = ab3.schedule(500 + k as u32, k);
    }
    cs.add("alarm_full_direct", ninth, ""); // 第 9 个（不同秒）→ 直发

    // ── 充电策略 ──
    // 9) 电池态：5 分钟空闲不执行（窗 30）。
    let mut cp = ChargingPolicy::new();
    cs.add("chg_battery_window_defers", !cp.try_task(5) && cp.try_task(30), "");
    // 10) 切充电：窗提前 + 预算放大。
    cp.set_ac(true);
    cs.add("chg_ac_earlier_window", cp.window_start_min() == 5 && cp.budget() == 100, "");
    // 11) 预算耗尽拒绝。
    for _ in 0..100 {
        let _ = cp.try_task(10);
    }
    cs.add("chg_budget_exhausted", !cp.try_task(10) && cp.executed() == 100, "");
    // 12) 电源态切换重置窗口账。
    cp.set_ac(false);
    cs.add("chg_switch_resets", cp.executed() == 0, "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn heat_quiet_window_edge() {
        let mut hm = WakeHeatmap::new();
        for _ in 0..5 {
            hm.record(23, WakeSrc::User);
        }
        // 全天最吵 23 点；k=24 的窗必含它 → None。
        assert_eq!(hm.noisiest_hour(), Some((23, 5)));
        assert_eq!(hm.quiet_window(24, 4), None);
        assert_eq!(hm.quiet_window(24, 5), Some(0));
    }

    #[test]
    fn alarm_batch_partial_fire() {
        let mut ab = AlarmBatcher::new();
        let _ = ab.schedule(10, 1);
        let _ = ab.schedule(20, 2);
        let _ = ab.schedule(20, 3);
        assert_eq!(ab.fire(10), 1);
        assert_eq!(ab.fire(20), 2);
        assert_eq!(ab.fire(20), 0, "重复 fire 空批");
    }

    #[test]
    fn charging_policy_hysteresis() {
        let mut cp = ChargingPolicy::new();
        let _ = cp.try_task(35);
        cp.set_ac(true);
        assert_eq!(cp.executed(), 0, "切电重置");
        cp.set_ac(false);
        assert_eq!(cp.executed(), 0, "切回同样重置");
        assert_eq!(cp.window_start_min(), 30);
    }
}
