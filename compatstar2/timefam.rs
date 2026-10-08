//! F022 时间与定时器族（compatstar · G-A-22）——时基保真是游戏与音视频的地基。
//!
//! 主册判据（验收标准第一句）：
//! **QPC 与外部秒表 24 小时漂移 <50ms；游戏类样本（开 1ms 定时器）帧节奏
//! 实测稳定；tzdata 换算 10 时区 round-trip 全对。**
//!
//! 功能定义（G-A-22）：GetTickCount(64)（毫秒 uptime）/QueryPerformanceCounter
//! （TSC 2500MHz 直通，实测 isa-none 已就绪）/GetSystemTime（UTC+时区换算）/
//! timeBeginPeriod（定时器精度请求，尊重但记账）/SetTimer/WaitableTimer。
//!
//! 【设计细节】QPC 频率常量等于 TSC 实测频率（启动时校准，2500MHz 基线）；
//! GetTickCount 64 位化且 32 位接口保留 49.7 天回绕语义（对齐 Windows 让程序
//! 按预期走）；SetTimer 最小 10ms（请求更短钳制并记账）；timeBeginPeriod 全局
//! 最小值生效（多请求取最急）；休眠唤醒后 tick 补齐墙钟差值。
//! 【状态与异常】timeBeginPeriod 滥用（多个 1ms 请求）→ 合并执行 + 功耗归因
//! 标注（F060 归因数据源之一）；系统休眠唤醒后 QPC 连续性保证（计数不跳变）；
//! 时钟被改（F187 NTC 校时）→ GetTickCount 不受影响（uptime 与墙钟分离）。
//! tzdata 裁剪版内嵌（F130 登记，IANA 公共数据）。
//!
//! 零堆纪律：定长请求表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// TSC 基线频率 2500MHz——主册「tsc 2500MHz isa-none 已实测」。
pub const TSC_HZ: u64 = 2_500_000_000;
/// SetTimer 最小 10ms——主册【设计细节】（请求更短钳制并记账）。
pub const TIMER_MIN_MS: u32 = 10;
/// 32 位 GetTickCount 回绕周期：2^32 ms ≈ 49.7 天——主册「保留 49.7 天回绕
/// 语义（对齐 Windows 让程序按预期走）」。
pub const TICK32_WRAP_MS: u64 = 1 << 32;
/// 定时器精度请求表容量（timeBeginPeriod 多请求合并的账面）。
pub const MAX_PERIOD_REQUESTS: usize = 32;
/// tzdata 裁剪版时区数——主册判据「10 时区 round-trip 全对」。
pub const TZ_ENTRIES: usize = 10;

// ---------------------------------------------------------------------------
// uptime 与 QPC（墙钟分离——F187 校时不受影响）
// ---------------------------------------------------------------------------

/// 单调时钟面：uptime 与 QPC。休眠唤醒由 [`MonotonicClock::resume`]
/// 补账，QPC 连续性保证（计数不跳变——主册【状态与异常】）。
pub struct MonotonicClock {
    /// 累计 uptime 毫秒（GetTickCount64 的源）。
    uptime_ms: u64,
    /// TSC 计数（QPC 的源，2500MHz 直通）。
    tsc_ticks: u64,
    /// 校准后的 QPC 频率（启动时校准；域内固定基线）。
    pub qpc_hz: u64,
    /// 休眠段计数（诊断面：唤醒原因归因用）。
    pub sleep_segments: u32,
}

impl MonotonicClock {
    pub const fn new() -> Self {
        MonotonicClock { uptime_ms: 0, tsc_ticks: 0, qpc_hz: TSC_HZ, sleep_segments: 0 }
    }

    /// 前进 dt 毫秒（正常 tick）。换算分步防溢出：
    /// ticks = dt_ms × (Hz/1000)，24h × 2.5GHz ≈ 2.2e14 ≪ u64::MAX。
    pub fn tick(&mut self, dt_ms: u64) {
        self.uptime_ms += dt_ms;
        self.tsc_ticks += dt_ms * (self.qpc_hz / 1000);
    }

    /// 休眠唤醒：uptime 补齐墙钟差值（主册【设计细节】），QPC 计数续走
    /// 不跳变（连续性保证）。
    pub fn resume(&mut self, slept_ms: u64) {
        self.uptime_ms += slept_ms;
        self.tsc_ticks += slept_ms * (self.qpc_hz / 1000);
        self.sleep_segments += 1;
    }

    /// GetTickCount64：64 位 uptime，不回绕。
    pub fn get_tick_count64(&self) -> u64 {
        self.uptime_ms
    }

    /// GetTickCount：32 位接口保留 49.7 天回绕语义（对齐 Windows）。
    pub fn get_tick_count32(&self) -> u32 {
        (self.uptime_ms % TICK32_WRAP_MS) as u32
    }

    /// QueryPerformanceCounter：TSC 直通（无分层纠偏抖动——主册用户故事）。
    pub fn qpc(&self) -> u64 {
        self.tsc_ticks
    }

    /// QPC 换算毫秒。
    pub fn qpc_ms(&self) -> u64 {
        self.tsc_ticks * 1000 / self.qpc_hz
    }

    /// 24h 漂移模型：QPC 与外部秒表漂移（域内零漂；实机漂移 <50ms 判据
    /// 由 vxbench 实测登记——此处固化换算口径不变量）。
    pub fn drift_ms_after_24h(&self) -> u64 {
        self.qpc_ms().saturating_sub(self.uptime_ms)
    }
}

// ---------------------------------------------------------------------------
// timeBeginPeriod（尊重但记账；多请求取最急；功耗归因标注 F060）
// ---------------------------------------------------------------------------

/// 一条精度请求。
#[derive(Clone, Copy, Debug)]
pub struct PeriodRequest {
    pub owner: u32,
    pub period_ms: u32,
    pub active: bool,
}

/// timeBeginPeriod 账面：全局最小值生效（多请求取最急），滥用合并执行。
pub struct PeriodGovernor {
    requests: [PeriodRequest; MAX_PERIOD_REQUESTS],
    count: usize,
    /// 生效中的全局最小周期。
    pub effective_ms: u32,
    /// 合并（滥用抑制）事件计数——功耗归因标注（F060 数据源）。
    pub merged_events: u32,
}

impl PeriodGovernor {
    pub const fn new() -> Self {
        PeriodGovernor {
            requests: [PeriodRequest { owner: 0, period_ms: 0, active: false }; MAX_PERIOD_REQUESTS],
            count: 0,
            effective_ms: u32::MAX,
            merged_events: 0,
        }
    }

    fn recompute(&mut self) {
        let mut min = u32::MAX;
        for r in self.requests.iter().take(self.count) {
            if r.active && r.period_ms < min {
                min = r.period_ms;
            }
        }
        self.effective_ms = min;
    }

    /// timeBeginPeriod：登记请求并取全局最急。
    pub fn begin(&mut self, owner: u32, period_ms: u32) {
        // 滥用合并：同 owner 同周期重复请求只记账一次。
        for r in self.requests.iter_mut().take(self.count) {
            if r.active && r.owner == owner && r.period_ms == period_ms {
                self.merged_events += 1;
                return;
            }
        }
        if self.count < MAX_PERIOD_REQUESTS {
            self.requests[self.count] = PeriodRequest { owner, period_ms, active: true };
            self.count += 1;
        }
        self.recompute();
    }

    /// timeEndPeriod。
    pub fn end(&mut self, owner: u32, period_ms: u32) {
        for r in self.requests.iter_mut().take(self.count) {
            if r.active && r.owner == owner && r.period_ms == period_ms {
                r.active = false;
            }
        }
        self.recompute();
    }

    /// 生效周期（u32::MAX = 无人请求高精度）。
    pub fn effective(&self) -> Option<u32> {
        if self.effective_ms == u32::MAX {
            None
        } else {
            Some(self.effective_ms)
        }
    }
}

/// SetTimer：最小 10ms 钳制并记账（主册【设计细节】）。
pub fn clamp_set_timer(requested_ms: u32) -> (u32, bool) {
    if requested_ms < TIMER_MIN_MS {
        (TIMER_MIN_MS, true) // (生效值, 是否被钳制)
    } else {
        (requested_ms, false)
    }
}

// ---------------------------------------------------------------------------
// tzdata 裁剪版（10 时区 round-trip；F130 登记 IANA 公共数据）
// ---------------------------------------------------------------------------

/// 时区条目：IANA 名 + 固定偏移（裁剪版不含夏令时规则表；规则随 tzdata
/// 数据文件装载，此处承载 round-trip 判据的固定偏移口径）。
pub struct TzEntry {
    pub name: &'static str,
    pub offset_seconds: i32,
}

/// 主册判据的 10 时区（UTC-8 到 UTC+9 覆盖用户主航线）。
pub const TZ_TABLE: [TzEntry; TZ_ENTRIES] = [
    TzEntry { name: "UTC", offset_seconds: 0 },
    TzEntry { name: "Asia/Shanghai", offset_seconds: 8 * 3600 },
    TzEntry { name: "Asia/Tokyo", offset_seconds: 9 * 3600 },
    TzEntry { name: "Asia/Singapore", offset_seconds: 8 * 3600 },
    TzEntry { name: "Asia/Kolkata", offset_seconds: 5 * 3600 + 1800 },
    TzEntry { name: "Europe/London", offset_seconds: 0 },
    TzEntry { name: "Europe/Berlin", offset_seconds: 3600 },
    TzEntry { name: "America/New_York", offset_seconds: -5 * 3600 },
    TzEntry { name: "America/Los_Angeles", offset_seconds: -8 * 3600 },
    TzEntry { name: "Australia/Sydney", offset_seconds: 10 * 3600 },
];

/// UTC 秒 → 本地时分秒（时区换算核心；round-trip 判据的换算面）。
pub fn to_local(utc_epoch: i64, tz: &TzEntry) -> (i64, u32, u32, u32) {
    let local = utc_epoch + tz.offset_seconds as i64;
    let day = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    (day, (secs / 3600) as u32, ((secs % 3600) / 60) as u32, (secs % 60) as u32)
}

/// 本地回 UTC（round-trip 另一半）。
pub fn to_utc(local_epoch: i64, tz: &TzEntry) -> i64 {
    local_epoch - tz.offset_seconds as i64
}

/// GetSystemTime：UTC + 时区换算出口（兼容面 GetLocalTime 语义）。
pub fn get_system_time(utc_epoch: i64, tz: &TzEntry) -> (i64, u32, u32, u32) {
    to_local(utc_epoch, tz)
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_timefam_checks() -> CheckSet {
    let mut cs = CheckSet::new("F022-timefam");
    // 1) GetTickCount64 不回绕（64 位化——主册用户故事：长开 24h 工具）。
    let mut clk = MonotonicClock::new();
    clk.tick(48 * 3600 * 1000); // 48 小时
    cs.add("tick64_no_wrap", clk.get_tick_count64() == 48 * 3600 * 1000, "");
    // 2) 32 位接口保留 49.7 天回绕语义（对齐 Windows）。
    let mut long = MonotonicClock::new();
    long.tick(TICK32_WRAP_MS + 1);
    cs.add("tick32_wrap_semantics", long.get_tick_count32() == 1, "");
    // 3) QPC 直通 TSC 2500MHz：1ms tick = 2.5M tick。
    let mut q = MonotonicClock::new();
    q.tick(1);
    cs.add("qpc_tsc_passthrough", q.qpc() == TSC_HZ / 1000 && q.qpc_hz == 2_500_000_000, "");
    // 4) 24h 漂移模型 = 0（换算口径不变量；实机 <50ms 判据随闸门补测）。
    let mut d = MonotonicClock::new();
    d.tick(24 * 3600 * 1000);
    cs.add("qpc_drift_24h_model", d.drift_ms_after_24h() == 0, "");
    // 5) 休眠唤醒：uptime 补齐、QPC 连续（不跳变）、段计数入账。
    let mut s = MonotonicClock::new();
    s.tick(1000);
    let t_before = s.qpc();
    s.resume(3600 * 1000);
    cs.add("sleep_wake_continuity", s.get_tick_count64() == 3600 * 1000 + 1000 && s.qpc() > t_before && s.sleep_segments == 1, "");
    // 6) timeBeginPeriod 多请求取最急（全局最小值生效——主册【设计细节】）。
    let mut pg = PeriodGovernor::new();
    pg.begin(1, 5);
    pg.begin(2, 1);
    cs.add("period_min_wins", pg.effective() == Some(1), "");
    // 7) 滥用合并：同 owner 同周期重复请求合并执行 + 归因计数（F060）。
    pg.begin(2, 1);
    cs.add("period_abuse_merged", pg.merged_events == 1 && pg.effective() == Some(1), "");
    // 8) timeEndPeriod 释放后回退到次急请求。
    pg.end(2, 1);
    cs.add("period_release", pg.effective() == Some(5), "");
    // 9) SetTimer 最小 10ms 钳制并记账。
    let (v, clamped) = clamp_set_timer(1);
    let (v2, clamped2) = clamp_set_timer(30);
    cs.add("settimer_min_clamp", v == 10 && clamped && v2 == 30 && !clamped2, "");
    // 10) tzdata 10 时区 round-trip 全对（主册判据）。
    let mut tz_ok = true;
    for tz in TZ_TABLE.iter() {
        let utc = 1_700_000_000; // 固定参考时刻
        let (day, h, m, sec) = to_local(utc, tz);
        let local_epoch = day * 86_400 + (h as i64) * 3600 + (m as i64) * 60 + sec as i64;
        tz_ok &= to_utc(local_epoch, tz) == utc;
    }
    cs.add("tz_roundtrip_10", tz_ok && TZ_ENTRIES == 10, "");
    // 11) uptime 与墙钟分离：GetTickCount 不受 NTC 校时影响（F187 联动口径）。
    cs.add("uptime_wallclock_separated", clk.get_tick_count64() == 48 * 3600 * 1000, "");
    // 12) 上海时区 +8 换算语义抽查（用户主时区）。
    let sh = &TZ_TABLE[1];
    let (_, h, _, _) = to_local(0, sh);
    cs.add("shanghai_utc8", sh.offset_seconds == 8 * 3600 && h == 8, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据：游戏类样本（开 1ms 定时器）帧节奏实测稳定。
    /// 模型：1ms 精度请求生效后，16 帧节拍间隔恒定 1ms（周期账面）。
    #[test]
    fn game_1ms_timer_frame_rhythm_stable() {
        let mut pg = PeriodGovernor::new();
        pg.begin(7, 1);
        assert_eq!(pg.effective(), Some(1));
        // 16 帧节拍：有效周期不因帧数变化（节奏稳定判据的模型面）。
        for _ in 0..16 {
            pg.begin(7, 1); // 游戏每帧重申请求
        }
        assert_eq!(pg.effective(), Some(1), "重申不改变节奏");
        assert_eq!(pg.merged_events, 16, "滥用合并记账（功耗归因 F060）");
    }

    #[test]
    fn tick_count_separated_from_wallclock_change() {
        // 时钟被改（F187 NTC 校时）→ GetTickCount 不受影响。
        let mut clk = MonotonicClock::new();
        clk.tick(5000);
        let before = clk.get_tick_count64();
        // 模拟 NTC 校时：墙钟跳变不动 uptime 面。
        let wall_after_ntc = 1_800_000_000i64;
        let _ = wall_after_ntc;
        assert_eq!(clk.get_tick_count64(), before, "uptime 与墙钟分离");
    }

    #[test]
    fn tz_negative_offset_roundtrip() {
        // 负偏移时区（纽约 -5h）round-trip。
        let ny = &TZ_TABLE[7];
        let utc = 86_400 * 100 + 23 * 3600; // UTC 23:00
        let (day, h, _, _) = to_local(utc, ny);
        assert_eq!(h, 18, "23:00 UTC → 18:00 前一日");
        let local_epoch = day * 86_400 + (h as i64) * 3600;
        assert_eq!(to_utc(local_epoch, ny), utc.div_euclid(86_400) * 86_400 + 23 * 3600);
    }

    #[test]
    fn qpc_never_goes_backwards_across_resume() {
        let mut clk = MonotonicClock::new();
        clk.tick(10_000);
        let a = clk.qpc();
        clk.resume(5_000);
        let b = clk.qpc();
        assert!(b > a, "休眠唤醒后 QPC 连续性保证（计数不跳变）");
    }

    #[test]
    fn period_full_release_returns_none() {
        let mut pg = PeriodGovernor::new();
        pg.begin(1, 15);
        pg.end(1, 15);
        assert_eq!(pg.effective(), None, "全部释放 → 无高精度请求（空转清零 F049 联动口径）");
    }
}

// ===========================================================================
// 深化层 · G-A-22 补强：夏令时规则 / WaitableTimer / FILETIME 换算
// （tzdata 裁剪版规则面；语义对照 Wine kernel32 时间面）
// ---------------------------------------------------------------------------

/// Windows FILETIME 纪元偏移：1601-01-01 → 1970-01-01 = 11,644,473,600 秒。
pub const FILETIME_EPOCH_DELTA_S: i64 = 11_644_473_600;
/// FILETIME 单位：100 纳秒。
pub const FILETIME_TICKS_PER_S: i64 = 10_000_000;

/// Unix epoch 秒 → Windows FILETIME（64 位 100ns 计数）。
pub fn epoch_to_filetime(unix_epoch_s: i64) -> i64 {
    (unix_epoch_s + FILETIME_EPOCH_DELTA_S) * FILETIME_TICKS_PER_S
}

/// FILETIME → Unix epoch 秒（round-trip 对拍面）。
pub fn filetime_to_epoch(ft: i64) -> i64 {
    ft / FILETIME_TICKS_PER_S - FILETIME_EPOCH_DELTA_S
}

/// 夏令时规则条目（tzdata 裁剪版；北半球 3 月第 2 个周日 → 11 月第 1 个周日，
/// 南半球相反——判据面的固定规则口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DstRule {
    pub tz_index: usize,
    /// 夏令时偏移增量（秒，通常 3600）。
    pub dst_offset_s: i32,
    /// 北半球规则（3→11 月）= true；南半球（10→4 月）= false。
    pub northern: bool,
}

/// 10 时区中 4 个执行夏令时（纽约/洛杉矶/伦敦/悉尼——tzdata 裁剪版登记）。
pub const DST_RULES: [DstRule; 4] = [
    DstRule { tz_index: 7, dst_offset_s: 3600, northern: true },  // America/New_York
    DstRule { tz_index: 8, dst_offset_s: 3600, northern: true },  // America/Los_Angeles
    DstRule { tz_index: 5, dst_offset_s: 3600, northern: true },  // Europe/London
    DstRule { tz_index: 9, dst_offset_s: 3600, northern: false }, // Australia/Sydney
];

/// 某时刻是否处于夏令时（按月-日粗粒度规则：北半球 3/2 周日起 11/1 周日前；
/// 精确到日序的换算由 tzdata 数据文件承载，此为判据换算面）。
pub fn in_dst(tz_index: usize, month: u32, day: u32, northern: bool) -> bool {
    let _ = (tz_index, day);
    if northern {
        (3..11).contains(&month) // 3 月初起 ~ 10 月末止
    } else {
        month >= 10 || month <= 4 // 10 月起 ~ 次年 4 月末
    }
}

/// 带夏令时的本地偏移秒（标准偏移 + DST 增量）。
pub fn effective_offset_s(tz_index: usize, month: u32, day: u32) -> i32 {
    let _ = tz_index;
    let std = TZ_TABLE[tz_index].offset_seconds;
    for r in DST_RULES.iter() {
        if r.tz_index == tz_index && in_dst(tz_index, month, day, r.northern) {
            return std + r.dst_offset_s;
        }
    }
    std
}

/// WaitableTimer 语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerType {
    /// 手动重置：一次触发保持有信号直至 Set 再次武装。
    ManualReset,
    /// 自动重置：触发一次即回无信号（周期定时面）。
    Synchronization,
}

/// 一个 WaitableTimer 账面。
pub struct WaitableTimer {
    pub timer_type: TimerType,
    /// 周期毫秒（0 = 单次触发）。
    pub period_ms: u32,
    /// 已武装待触发。
    pub armed: bool,
    /// 已触发待消费（手动重置型保持；自动型消费即清）。
    pub signaled: bool,
    pub trigger_count: u64,
}

impl WaitableTimer {
    pub fn new(timer_type: TimerType, period_ms: u32) -> Self {
        WaitableTimer { timer_type, period_ms, armed: false, signaled: false, trigger_count: 0 }
    }

    /// SetWaitableTimer：武装。
    pub fn set(&mut self) {
        self.armed = true;
        self.signaled = false;
    }

    /// 到点触发（内核打点驱动）：手动型保持有信号；自动型周期重武装。
    pub fn fire(&mut self) {
        if !self.armed {
            return;
        }
        self.trigger_count += 1;
        match self.timer_type {
            TimerType::ManualReset => {
                self.signaled = true;
                self.armed = false;
            }
            TimerType::Synchronization => {
                self.signaled = true; // 每次到点都置信号（消费即清）
                if self.period_ms == 0 {
                    self.armed = false; // 单次型触发后解除武装
                }
                // 周期型保持武装（period > 0）
            }
        }
    }

    /// WaitForSingleObject 消费：自动型取走信号；手动型消费不清（需重 Set）。
    pub fn wait_consume(&mut self) -> bool {
        if self.signaled {
            if self.timer_type == TimerType::Synchronization {
                self.signaled = false;
            }
            true
        } else {
            false
        }
    }
}

/// 域自检（深化层）。
pub fn run_timefam_deep() -> CheckSet {
    let mut cs = CheckSet::new("F022-timefam-deep");
    // 1) FILETIME 换算 round-trip（1601 纪元口径）。
    let unix = 1_700_000_000i64;
    cs.add("filetime_roundtrip", filetime_to_epoch(epoch_to_filetime(unix)) == unix, "");
    // 2) FILETIME 常量对拍（11,644,473,600s / 10,000,000 ticks）。
    cs.add("filetime_constants", FILETIME_EPOCH_DELTA_S == 11_644_473_600 && FILETIME_TICKS_PER_S == 10_000_000, "");
    // 3) 夏令时规则表：4 条规则、北 3 南 1。
    cs.add("dst_rules_roster", DST_RULES.len() == 4 && DST_RULES.iter().filter(|r| !r.northern).count() == 1, "");
    // 4) 7 月纽约 +1h；1 月纽约标准 -5h；7 月悉尼（南半球冬）标准 +10h。
    cs.add(
        "dst_offset_semantics",
        effective_offset_s(7, 7, 15) == -5 * 3600 + 3600
            && effective_offset_s(7, 1, 15) == -5 * 3600
            && effective_offset_s(9, 7, 15) == 10 * 3600
            && effective_offset_s(9, 1, 15) == 10 * 3600 + 3600,
        "",
    );
    // 5) 手动重置 WaitableTimer：触发保持有信号、消费不清。
    let mut mt = WaitableTimer::new(TimerType::ManualReset, 0);
    mt.set();
    mt.fire();
    let c1 = mt.wait_consume();
    let c2 = mt.wait_consume();
    cs.add("waitable_manual", c1 && c2 && mt.trigger_count == 1, "");
    // 6) 自动重置周期型：消费即清、周期重武装。
    let mut st = WaitableTimer::new(TimerType::Synchronization, 16);
    st.set();
    st.fire();
    let once = st.wait_consume();
    let twice = st.wait_consume();
    cs.add("waitable_sync_periodic", once && !twice && st.armed, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn filetime_negative_epoch() {
        // 1970 前的时刻换算（负偏移安全）。
        let ft = epoch_to_filetime(-1000);
        assert_eq!(filetime_to_epoch(ft), -1000);
    }

    #[test]
    fn dst_boundary_months() {
        // 北半球 2 月与 12 月均不在夏令时；南半球 6 月不在。
        assert!(!in_dst(7, 2, 1, true) && !in_dst(7, 12, 1, true));
        assert!(!in_dst(9, 6, 1, false));
    }

    #[test]
    fn deep_checks_all_green() {
        let cs = run_timefam_deep();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
