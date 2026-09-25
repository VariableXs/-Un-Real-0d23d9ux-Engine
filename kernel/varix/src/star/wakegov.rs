//! F065 唤醒源治理 · 完整设计（STAR I 主册 G-B-25）。
//!
//! **判据（主册）**：合盖 2 小时实测唤醒 ≤2 次、掉电 ≤2%；唤醒原因记录
//! 100% 可解释。
//!
//! **设计要点（主册）**：
//! - 睡眠态唤醒源白名单：默认仅电源键/盖开合/USB 键鼠；每分钟唤醒次数
//!   ≤1 达标；唤醒原因全记录；
//! - 白名单分级：**硬源**（电源键不可移）/ **软源**（USB 设备可逐个关）；
//! - 唤醒事件日志入账本（保留 30 天）：时间/来源/耗时；
//! - 未知唤醒源（固件行为）→ 记录「未知」并建议排查——解释率如实下降
//!   （把 Unknown 计入「已解释」会把判据变成恒真的装饰品）；
//! - 唤醒后 60 秒无操作 → 自动回睡（可关）；
//! - 唤醒抖动保护：醒后 5s 内再睡视为抖动计数（>3 次/小时告警）；
//! - RTC 唤醒仅限用户闹钟（F100 显式预约）；包内唤醒（WoL）默认关。
//!
//! 白名单思路参照 Windows Modern Standby 与 Linux wakeup sources；实现
//! 自研（电源面 F WP-106 扩展）。一切时间注入式（秒戳/分钟戳），宿主
//! 测试确定复现。

use crate::checks::CheckSet;
use crate::star::sbase::{MinuteBook, RingLog};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 抖动判定：醒后 5s 内再睡视为抖动。
pub const JITTER_WINDOW_S: u64 = 5;

/// 抖动告警：1 小时窗口内 >3 次抖动即告警。
pub const JITTER_ALARM_PER_HOUR: usize = 3;

/// 自动回睡：唤醒后 60 秒无操作。
pub const AUTO_RESLEEP_IDLE_S: u64 = 60;

/// 唤醒事件保留窗（秒）。
pub const EVENT_RETENTION_S: u64 = 30 * 24 * 3600;

/// 每分钟唤醒次数上限（达标线）。
pub const WAKES_PER_MIN_LIMIT: u64 = 1;

/// 会话判据：合盖 2 小时。
pub const SESSION_SPAN_S: u64 = 2 * 3600;

/// 会话判据：唤醒 ≤2 次。
pub const SESSION_WAKE_LIMIT: usize = 2;

/// 会话判据：掉电 ≤2%（permille 口径 = 20‰——电量计读数即 permille）。
pub const SESSION_DRAIN_LIMIT_PERMILLE: u32 = 20;

/// 事件定容（滚动覆盖即物理删除；典型唤醒频次 30 天远低于此）。
const EVENT_RING_CAP: usize = 1024;

/// 抖动时间戳环容量。
const JITTER_RING_CAP: usize = 32;

/// 电量读数环容量（会话掉电判据数据源）。
const GAUGE_RING_CAP: usize = 256;

/// 每分钟唤醒账本保留窗（分钟）。
const WAKE_BOOK_MIN: u64 = 1440;

// ---------------------------------------------------------------------------
// 唤醒源与白名单
// ---------------------------------------------------------------------------

/// 唤醒源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeSource {
    /// 电源键（硬源，不可移）。
    PowerButton,
    /// 盖开合（硬源，不可移）。
    LidOpen,
    /// USB 键盘（软源，默认开）。
    UsbKbd,
    /// USB 鼠标（软源，默认开）。
    UsbMouse,
    /// 包内唤醒 WoL（软源，默认关）。
    WakeOnLan,
    /// RTC 闹钟（仅限 F100 用户显式预约）。
    RtcAlarm,
    /// 未知源（固件行为——记录「未知」并建议排查）。
    Unknown,
}

impl WakeSource {
    /// 硬源判定（不可移除）。
    pub fn is_hard(self) -> bool {
        matches!(self, WakeSource::PowerButton | WakeSource::LidOpen)
    }
}

/// 白名单管理（硬源恒允许；软源逐个开关；RTC 看预约位）。
pub struct WakeAllowlist {
    usb_kbd: bool,
    usb_mouse: bool,
    wol: bool,
    rtc_armed: bool,
}

impl WakeAllowlist {
    pub fn new() -> WakeAllowlist {
        WakeAllowlist { usb_kbd: true, usb_mouse: true, wol: false, rtc_armed: false }
    }

    /// 白名单裁决。
    pub fn is_allowed(&self, s: WakeSource) -> bool {
        match s {
            WakeSource::PowerButton | WakeSource::LidOpen => true,
            WakeSource::UsbKbd => self.usb_kbd,
            WakeSource::UsbMouse => self.usb_mouse,
            WakeSource::WakeOnLan => self.wol,
            WakeSource::RtcAlarm => self.rtc_armed,
            WakeSource::Unknown => false,
        }
    }

    /// 关闭软源。硬源拒绝（不可移纪律），未知源无从关闭（恒不允许）。
    pub fn disable(&mut self, s: WakeSource) -> Result<(), &'static str> {
        match s {
            WakeSource::PowerButton | WakeSource::LidOpen => Err("hard source cannot be removed"),
            WakeSource::UsbKbd => {
                self.usb_kbd = false;
                Ok(())
            }
            WakeSource::UsbMouse => {
                self.usb_mouse = false;
                Ok(())
            }
            WakeSource::WakeOnLan => {
                self.wol = false;
                Ok(())
            }
            WakeSource::RtcAlarm => {
                self.rtc_armed = false;
                Ok(())
            }
            WakeSource::Unknown => Err("unknown source has no switch"),
        }
    }

    /// 打开软源。
    pub fn enable(&mut self, s: WakeSource) -> Result<(), &'static str> {
        match s {
            WakeSource::PowerButton | WakeSource::LidOpen => Err("hard source is always on"),
            WakeSource::UsbKbd => {
                self.usb_kbd = true;
                Ok(())
            }
            WakeSource::UsbMouse => {
                self.usb_mouse = true;
                Ok(())
            }
            WakeSource::WakeOnLan => {
                self.wol = true;
                Ok(())
            }
            WakeSource::RtcAlarm => {
                self.rtc_armed = true;
                Ok(())
            }
            WakeSource::Unknown => Err("unknown source has no switch"),
        }
    }

    /// F100 闹钟预约口（预约窗口建立 → RTC 放行）。
    pub fn arm_rtc(&mut self, armed: bool) {
        self.rtc_armed = armed;
    }
}

impl Default for WakeAllowlist {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 唤醒事件与治理器
// ---------------------------------------------------------------------------

/// 白名单裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeVerdict {
    Allowed,
    Rejected(&'static str),
}

/// 入睡类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SleepKind {
    /// 无清醒中的会话（幂等保护）。
    NotAwake,
    /// 手动/常规入睡。
    Manual,
    /// 60 秒无操作自动回睡。
    AutoResleep,
    /// 醒后 5s 内再睡（抖动）。
    Jitter,
}

/// 一条唤醒事件（时间/来源/裁决/耗时——唤醒原因全记录）。
#[derive(Clone, Copy, Debug)]
pub struct WakeEvent {
    /// 唤醒时刻（秒戳）。
    pub ts_s: u64,
    pub source: WakeSource,
    /// 白名单裁决（被拒的尝试也入账——解释率覆盖全部记录）。
    pub allowed: bool,
    /// 本次清醒是否抖动（醒后 <5s 再睡）。
    pub jitter: bool,
    /// 清醒时长（ms；被拒尝试为 0）。
    pub awake_ms: u32,
    /// 是否自动回睡收尾。
    pub auto_reslept: bool,
}

impl WakeEvent {
    /// 解释性：来源已知（含被拒的已知源）→ 可解释；Unknown → 不可解释。
    pub fn explained(&self) -> bool {
        self.source != WakeSource::Unknown
    }
}

/// 电量读数点（会话掉电判据数据源）。
#[derive(Clone, Copy, Debug)]
pub struct GaugePoint {
    pub ts_s: u64,
    /// 电量计读数（‰）。
    pub permille: u32,
}

/// 合盖会话报告（2 小时判据直读面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionReport {
    pub from_s: u64,
    pub to_s: u64,
    /// 合法唤醒次数。
    pub wakes: usize,
    /// 其中抖动次数。
    pub jitter_wakes: usize,
    /// 被拒唤醒尝试次数。
    pub rejected_attempts: usize,
    /// 会话掉电（‰；无边界读数时 None——不猜）。
    pub drain_permille: Option<u32>,
    /// 每分钟唤醒 >1 次的违规分钟数。
    pub minute_violations: usize,
    /// 判据：唤醒 ≤2 且掉电 ≤2‰×10 且无分钟违规。
    pub meets: bool,
}

/// 睡眠态唤醒源治理器。
pub struct SleepGovernor {
    allowlist: WakeAllowlist,
    /// 唤醒事件账（定容滚动；新事件追加，超容移除最旧——保留窗物理语义）。
    /// 用 Vec 而非 RingLog：入睡回填需要就地改写事件条目。
    events: Vec<WakeEvent>,
    jitter_ts: RingLog<u64, JITTER_RING_CAP>,
    /// 每分钟合法唤醒计数（单列分钟账本）。
    wake_book: MinuteBook,
    gauges: RingLog<GaugePoint, GAUGE_RING_CAP>,
    auto_resleep: bool,
    /// 清醒中会话的唤醒时刻。
    awake_open: Option<u64>,
}

impl SleepGovernor {
    pub fn new() -> SleepGovernor {
        SleepGovernor {
            allowlist: WakeAllowlist::new(),
            events: Vec::new(),
            jitter_ts: RingLog::new(),
            wake_book: MinuteBook::new(1, WAKE_BOOK_MIN),
            gauges: RingLog::new(),
            auto_resleep: true,
            awake_open: None,
        }
    }

    /// 白名单只读访问。
    pub fn allowlist(&self) -> &WakeAllowlist {
        &self.allowlist
    }

    /// 白名单可变访问（设置页管理入口）。
    pub fn allowlist_mut(&mut self) -> &mut WakeAllowlist {
        &mut self.allowlist
    }

    /// 自动回睡开关（可关——主册明示）。
    pub fn set_auto_resleep(&mut self, on: bool) {
        self.auto_resleep = on;
    }

    /// 请求唤醒（软件可拦路径 + 硬件已发生路径统一入口）。
    /// 被拒尝试同样入账（唤醒原因全记录）。
    pub fn wake(&mut self, ts_s: u64, source: WakeSource) -> WakeVerdict {
        let allowed = self.allowlist.is_allowed(source);
        if self.events.len() >= EVENT_RING_CAP {
            self.events.remove(0);
        }
        self.events.push(WakeEvent {
            ts_s,
            source,
            allowed,
            jitter: false,
            awake_ms: 0,
            auto_reslept: false,
        });
        if allowed {
            self.awake_open = Some(ts_s);
            self.wake_book.record_minute(ts_s / 60, &[1]);
            WakeVerdict::Allowed
        } else {
            WakeVerdict::Rejected(match source {
                WakeSource::UsbKbd | WakeSource::UsbMouse => "soft source disabled",
                WakeSource::WakeOnLan => "WoL disabled by default",
                WakeSource::RtcAlarm => "RTC alarm not armed",
                WakeSource::Unknown => "unknown source not allowed",
                WakeSource::PowerButton | WakeSource::LidOpen => "impossible: hard source",
            })
        }
    }

    /// 入睡（回填清醒时长并做抖动判定）。
    pub fn sleep(&mut self, ts_s: u64, idle_s: Option<u64>) -> SleepKind {
        let wake_ts = match self.awake_open.take() {
            Some(t) => t,
            None => return SleepKind::NotAwake,
        };
        let awake_ms = ts_s.saturating_sub(wake_ts) as u32 * 1000;
        let jitter = awake_ms < (JITTER_WINDOW_S * 1000) as u32;
        let auto = self.auto_resleep && matches!(idle_s, Some(i) if i >= AUTO_RESLEEP_IDLE_S);
        let kind = if auto {
            SleepKind::AutoResleep
        } else if jitter {
            SleepKind::Jitter
        } else {
            SleepKind::Manual
        };
        // 就地回填该次唤醒的事件条目（按唤醒时刻匹配 Allowed 事件）。
        if let Some(ev) = self.events.iter_mut().rev().find(|e| e.allowed && e.ts_s == wake_ts) {
            ev.awake_ms = awake_ms;
            ev.jitter = jitter;
            ev.auto_reslept = auto;
        }
        if jitter {
            self.jitter_ts.push(ts_s);
        }
        kind
    }

    /// 抖动告警：最近 1 小时内抖动次数 >3。
    pub fn jitter_alarm(&self, now_s: u64) -> bool {
        self.jitter_ts
            .newest_first()
            .iter()
            .filter(|t| **t <= now_s && now_s - **t <= 3600)
            .count()
            > JITTER_ALARM_PER_HOUR
    }

    /// 电量读数入环（会话掉电判据数据源；F060 电量账本对接缝）。
    pub fn record_gauge(&mut self, ts_s: u64, permille: u32) {
        self.gauges.push(GaugePoint { ts_s, permille });
    }

    /// 最近 N 条唤醒事件（新→旧；设置页「最近 10 次」直读）。
    pub fn recent_events(&self, n: usize) -> Vec<WakeEvent> {
        self.events.iter().rev().take(n).copied().collect()
    }

    /// 保留窗内事件数（30 天窗过滤——定容滚动 + 查询窗双保险）。
    pub fn retained_count(&self, now_s: u64) -> usize {
        self.events
            .iter()
            .filter(|e| now_s >= e.ts_s && now_s - e.ts_s <= EVENT_RETENTION_S)
            .count()
    }

    /// 解释率（万分比）：可解释唤醒 / 全部记录。Unknown 拉低——诚实口径。
    pub fn explained_ppt(&self) -> Option<u32> {
        let n = self.events.len();
        if n == 0 {
            return None;
        }
        let ok = self.events.iter().filter(|e| e.explained()).count();
        Some(ok as u32 * 10_000 / n as u32)
    }

    /// 指定分钟的合法唤醒数（每分钟 ≤1 达标线直读）。
    pub fn wakes_in_minute(&self, minute: u64) -> u64 {
        self.wake_book.range_sum(minute, minute).iter().sum()
    }

    /// 合盖会话报告（判据直读：唤醒 ≤2、掉电 ≤2%、分钟无违规）。
    pub fn session_report(&self, from_s: u64, to_s: u64) -> SessionReport {
        let in_win: Vec<&WakeEvent> =
            self.events.iter().filter(|e| e.ts_s >= from_s && e.ts_s <= to_s).collect();
        let wakes = in_win.iter().filter(|e| e.allowed).count();
        let jitter_wakes = in_win.iter().filter(|e| e.allowed && e.jitter).count();
        let rejected = in_win.iter().filter(|e| !e.allowed).count();

        // 每分钟违规：合法唤醒事件按分钟聚合后查账本。
        let mut minute_violations = 0usize;
        let mut seen_min: Vec<u64> = Vec::new();
        for e in in_win.iter().filter(|e| e.allowed) {
            let m = e.ts_s / 60;
            if !seen_min.contains(&m) {
                seen_min.push(m);
                if self.wakes_in_minute(m) > WAKES_PER_MIN_LIMIT {
                    minute_violations += 1;
                }
            }
        }

        // 掉电：≤from 最近读数为基线、≤to 最近读数为终点（无读数不猜）。
        let gvec = self.gauges.newest_first();
        let base: Option<GaugePoint> = gvec.iter().filter(|g| g.ts_s <= from_s).max_by_key(|g| g.ts_s).copied();
        let end: Option<GaugePoint> = gvec.iter().filter(|g| g.ts_s <= to_s).max_by_key(|g| g.ts_s).copied();
        let drain = match (base, end) {
            (Some(b), Some(t)) => Some(b.permille.saturating_sub(t.permille)),
            _ => None,
        };

        let meets = wakes <= SESSION_WAKE_LIMIT
            && minute_violations == 0
            && matches!(drain, Some(d) if d <= SESSION_DRAIN_LIMIT_PERMILLE);
        SessionReport {
            from_s,
            to_s,
            wakes,
            jitter_wakes,
            rejected_attempts: rejected,
            drain_permille: drain,
            minute_violations,
            meets,
        }
    }
}

impl Default for SleepGovernor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F065 自检（判据：合盖 2h 唤醒 ≤2 掉电 ≤2%；唤醒原因 100% 可解释）。
pub fn run_wakegov_checks() -> CheckSet {
    let mut set = CheckSet::new("F065-wakegov");

    // 1. 默认白名单：仅电源键/盖开合/USB 键鼠（WoL 默认关）。
    let al = WakeAllowlist::new();
    set.add(
        "default allowlist per spec",
        al.is_allowed(WakeSource::PowerButton)
            && al.is_allowed(WakeSource::LidOpen)
            && al.is_allowed(WakeSource::UsbKbd)
            && al.is_allowed(WakeSource::UsbMouse)
            && !al.is_allowed(WakeSource::WakeOnLan)
            && !al.is_allowed(WakeSource::RtcAlarm)
            && !al.is_allowed(WakeSource::Unknown),
        "",
    );

    // 2. 硬源不可移。
    let mut al2 = WakeAllowlist::new();
    set.add(
        "hard source cannot be removed",
        al2.disable(WakeSource::PowerButton).is_err() && al2.disable(WakeSource::LidOpen).is_err(),
        "",
    );

    // 3. 软源逐个关：关鼠标 → 鼠标唤醒拒绝、键盘不受影响。
    let mut gov = SleepGovernor::new();
    assert!(gov.allowlist_mut().disable(WakeSource::UsbMouse).is_ok());
    let v = gov.wake(1_000, WakeSource::UsbMouse);
    let v2 = gov.wake(1_001, WakeSource::UsbKbd);
    set.add(
        "usb devices toggleable individually",
        v == WakeVerdict::Rejected("soft source disabled")
            && v2 == WakeVerdict::Allowed
            && gov.allowlist().is_allowed(WakeSource::UsbKbd),
        "",
    );
    gov.sleep(2_000, None);

    // 4. WoL 默认关 + RTC 未预约拒绝 / 预约放行（F100 接缝）。
    let mut gov2 = SleepGovernor::new();
    let wol = gov2.wake(10, WakeSource::WakeOnLan);
    let rtc0 = gov2.wake(11, WakeSource::RtcAlarm);
    gov2.allowlist_mut().arm_rtc(true);
    let rtc1 = gov2.wake(12, WakeSource::RtcAlarm);
    set.add(
        "WoL off by default, RTC only via F100 booking",
        wol == WakeVerdict::Rejected("WoL disabled by default")
            && rtc0 == WakeVerdict::Rejected("RTC alarm not armed")
            && rtc1 == WakeVerdict::Allowed,
        "",
    );
    gov2.sleep(2_000, None);

    // 5. 未知源：记录「未知」、不放行、解释率如实 <100%。
    let mut gov3 = SleepGovernor::new();
    let u = gov3.wake(100, WakeSource::Unknown);
    let k = gov3.wake(101, WakeSource::PowerButton);
    gov3.sleep(2_000, None);
    set.add(
        "unknown source recorded honestly",
        u == WakeVerdict::Rejected("unknown source not allowed")
            && k == WakeVerdict::Allowed
            && gov3.explained_ppt() == Some(5_000),
        "",
    );

    // 6. 全已知源 → 解释率 100%（判据正演）。
    let mut gov4 = SleepGovernor::new();
    for (i, s) in [WakeSource::PowerButton, WakeSource::LidOpen, WakeSource::UsbKbd]
        .into_iter()
        .enumerate()
    {
        let _ = gov4.wake(1_000 + i as u64 * 1_200, s);
        let _ = gov4.sleep(1_000 + i as u64 * 1_200 + 10_000, None);
    }
    set.add("all known sources => 100% explained", gov4.explained_ppt() == Some(10_000), "");

    // 7. 抖动：醒后 <5s 再睡 → Jitter；1 小时内第 4 次 → 告警。
    let mut gov5 = SleepGovernor::new();
    for i in 0..4u64 {
        let t = i * 600; // 间隔 10 分钟
        assert!(matches!(gov5.wake(t, WakeSource::PowerButton), WakeVerdict::Allowed));
        assert_eq!(gov5.sleep(t + 3, None), SleepKind::Jitter, "3s < 5s 窗");
    }
    set.add("jitter counted, alarm on 4th in hour", gov5.jitter_alarm(4 * 600), "");

    // 8. 正常清醒（>5s）不判抖动。
    let mut gov6 = SleepGovernor::new();
    let _ = gov6.wake(0, WakeSource::PowerButton);
    set.add("normal wake not jitter", gov6.sleep(10_000, None) == SleepKind::Manual, "");

    // 9. 自动回睡：60s 无操作 → AutoResleep；开关关掉 → Manual。
    let mut gov7 = SleepGovernor::new();
    let _ = gov7.wake(0, WakeSource::PowerButton);
    set.add("auto resleep after 60s idle", gov7.sleep(65_000, Some(62)) == SleepKind::AutoResleep, "");
    gov7.set_auto_resleep(false);
    let _ = gov7.wake(70_000, WakeSource::PowerButton);
    set.add("auto resleep toggleable off", gov7.sleep(140_000, Some(65)) == SleepKind::Manual, "");
    // 醒后未满 60s 不自动回睡。
    gov7.set_auto_resleep(true);
    let _ = gov7.wake(150_000, WakeSource::PowerButton);
    set.add("under 60s idle stays manual", gov7.sleep(200_000, Some(50)) == SleepKind::Manual, "");

    // 10. 2 小时会话判据正演：2 次合法唤醒 + 掉电 15‰ → meets。
    let mut gov8 = SleepGovernor::new();
    gov8.record_gauge(0, 950);
    let _ = gov8.wake(600, WakeSource::PowerButton);
    let _ = gov8.sleep(60_600, None);
    let _ = gov8.wake(3_600, WakeSource::LidOpen);
    let _ = gov8.sleep(120_600, None);
    gov8.record_gauge(7_200, 935);
    let rep = gov8.session_report(0, 7_200);
    set.add(
        "session 2h passes with 2 wakes and 15ppm drain",
        rep.meets && rep.wakes == 2 && rep.drain_permille == Some(15),
        "",
    );

    // 11. 会话判据红面：3 次唤醒 → 不达标。
    let mut gov9 = SleepGovernor::new();
    gov9.record_gauge(0, 950);
    for i in 0..3u64 {
        let _ = gov9.wake(600 + i * 2_400, WakeSource::PowerButton);
        let _ = gov9.sleep(600 + i * 2_400 + 1_200, None);
    }
    gov9.record_gauge(7_200, 940);
    let rep9 = gov9.session_report(0, 7_200);
    set.add("session with 3 wakes fails honestly", !rep9.meets && rep9.wakes == 3, "");

    // 12. 每分钟唤醒 ≤1：同分钟两次合法唤醒 → 分钟违规。
    let mut gov10 = SleepGovernor::new();
    let _ = gov10.wake(120, WakeSource::PowerButton); // 分钟 2
    let _ = gov10.sleep(1_120, None);
    let _ = gov10.wake(130, WakeSource::LidOpen); // 仍分钟 2
    let _ = gov10.sleep(1_130, None);
    let rep10 = gov10.session_report(0, 7_200);
    set.add(
        "minute wake limit >1 flagged",
        gov10.wakes_in_minute(2) == 2 && rep10.minute_violations == 1,
        "",
    );

    // 13. 保留窗：30 天外事件不计入 retained。
    let mut gov11 = SleepGovernor::new();
    let _ = gov11.wake(0, WakeSource::PowerButton);
    let _ = gov11.wake(EVENT_RETENTION_S + 1, WakeSource::PowerButton);
    set.add(
        "30-day retention window filter",
        gov11.retained_count(EVENT_RETENTION_S + 2) == 1,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_roundtrip() {
        let mut al = WakeAllowlist::new();
        assert!(al.enable(WakeSource::WakeOnLan).is_ok());
        assert!(al.is_allowed(WakeSource::WakeOnLan));
        assert!(al.disable(WakeSource::WakeOnLan).is_ok());
        assert!(!al.is_allowed(WakeSource::WakeOnLan));
        assert!(al.enable(WakeSource::Unknown).is_err());
        assert!(al.enable(WakeSource::PowerButton).is_err());
    }

    #[test]
    fn rtc_arm_roundtrip() {
        let mut al = WakeAllowlist::new();
        al.arm_rtc(true);
        assert!(al.is_allowed(WakeSource::RtcAlarm));
        al.arm_rtc(false);
        assert!(!al.is_allowed(WakeSource::RtcAlarm));
    }

    #[test]
    fn sleep_without_wake_is_noop() {
        let mut gov = SleepGovernor::new();
        assert_eq!(gov.sleep(100, None), SleepKind::NotAwake);
        assert!(gov.recent_events(10).is_empty());
    }

    #[test]
    fn double_wake_opens_latest() {
        // 两次 wake 未 sleep：回填时按 wake_ts 匹配最近一次。
        let mut gov = SleepGovernor::new();
        let _ = gov.wake(1_000, WakeSource::PowerButton);
        let _ = gov.wake(2_000, WakeSource::LidOpen);
        assert_eq!(gov.sleep(3_000, None), SleepKind::Manual);
        let evs = gov.recent_events(2);
        assert_eq!(evs[0].ts_s, 2_000);
        assert_eq!(evs[0].awake_ms, 1_000_000, "1000 秒清醒 = 1_000_000ms");
        assert_eq!(evs[1].awake_ms, 0, "旧 wake 未回填");
    }

    #[test]
    fn jitter_ring_and_alarm_boundary() {
        let mut gov = SleepGovernor::new();
        for i in 0..3u64 {
            let _ = gov.wake(i * 600, WakeSource::PowerButton);
            let _ = gov.sleep(i * 600 + 2, None);
        }
        assert!(!gov.jitter_alarm(1_800), "3 次不告警（>3 才告警）");
        let _ = gov.wake(2_400, WakeSource::PowerButton);
        let _ = gov.sleep(2_402, None);
        assert!(gov.jitter_alarm(2_402), "第 4 次告警");
        // 1 小时后滑出窗口。
        assert!(!gov.jitter_alarm(2_402 + 3_600), "窗口滑出后解除");
    }

    #[test]
    fn event_ring_rolls_over() {
        let mut gov = SleepGovernor::new();
        for i in 0..(EVENT_RING_CAP + 50) as u64 {
            let _ = gov.wake(i * 60, WakeSource::PowerButton);
            let _ = gov.sleep(i * 60 + 30, None);
        }
        let evs = gov.recent_events(usize::MAX);
        assert_eq!(evs.len(), EVENT_RING_CAP, "ring 定容滚动");
        assert_eq!(evs[0].ts_s, (EVENT_RING_CAP + 49) as u64 * 60, "最新在前");
    }

    #[test]
    fn session_drain_missing_gauge_is_none() {
        let mut gov = SleepGovernor::new();
        let _ = gov.wake(600, WakeSource::PowerButton);
        let _ = gov.sleep(60_600, None);
        let rep = gov.session_report(0, 7_200);
        assert_eq!(rep.drain_permille, None, "无电量读数不猜");
        assert!(!rep.meets);
    }

    #[test]
    fn rejected_attempts_counted_in_session() {
        let mut gov = SleepGovernor::new();
        let _ = gov.wake(600, WakeSource::WakeOnLan); // 拒绝
        let _ = gov.wake(1_200, WakeSource::PowerButton); // 合法
        let _ = gov.sleep(61_200, None);
        let rep = gov.session_report(0, 7_200);
        assert_eq!(rep.rejected_attempts, 1);
        assert_eq!(rep.wakes, 1);
    }

    #[test]
    fn recent_events_page() {
        let mut gov = SleepGovernor::new();
        for i in 0..15u64 {
            let _ = gov.wake(i * 100, WakeSource::PowerButton);
            let _ = gov.sleep(i * 100 + 50, None);
        }
        let top10 = gov.recent_events(10);
        assert_eq!(top10.len(), 10);
        assert_eq!(top10[0].ts_s, 1_400, "新→旧");
        assert_eq!(top10[9].ts_s, 500);
    }
}
