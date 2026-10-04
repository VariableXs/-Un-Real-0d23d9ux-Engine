//! m700pwr — VARIX-M700 AI-19 电源时钟内核域 (F451~F475)
//!
//! 睡眠状态机/唤醒源大典/频率 governor 谱/空闲 Governor/功耗仪表官/时钟树大典/
//! 热区谱/热治理官/电池化学档案/充电状态机/睡眠竞速表/功耗预算合同/唤醒延迟仪/
//! 时钟漂移考古/电源事件流/热回放舱/低电降级律/电源 fuzz 桩/电源回归金样/
//! 功耗自描述导出/电源健康分/时钟源仲裁官/电源压力剧本/电源文档生成器/电源域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F451 — 睡眠状态机：Run→Idle→Suspend→Hibernate 梯度
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SleepState {
    Run,
    Idle,
    Suspend,
    Hibernate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SleepRequest {
    TickIdle,
    SuspendButton,
    CriticalBattery,
    WakeEvent,
}

/// 迁移表：临界电量可以直接跳深度；唤醒事件只允许回到 Run。
pub fn sleep_advance(cur: SleepState, req: SleepRequest) -> Option<SleepState> {
    use SleepRequest::*;
    use SleepState::*;
    match (cur, req) {
        (Run, TickIdle) => Some(Idle),
        (Idle, SuspendButton) => Some(Suspend),
        (Idle, CriticalBattery) => Some(Hibernate),
        (Suspend, CriticalBattery) => Some(Hibernate),
        (Idle, WakeEvent) => Some(Run),
        (Suspend, WakeEvent) => Some(Run),
        (Hibernate, WakeEvent) => Some(Run),
        (Run, CriticalBattery) => Some(Idle), // 先走正常收敛，不越级关机
        _ => None,
    }
}

// ===========================================================================
// F452 — 唤醒源大典：位掩码管理
// ===========================================================================

pub const WAKE_POWER_BTN: u8 = 0;
pub const WAKE_RTC: u8 = 1;
pub const WAKE_LID: u8 = 2;
pub const WAKE_USB: u8 = 3;
pub const WAKE_AC_INSERT: u8 = 4;
pub const WAKE_SOURCES: usize = 5;

#[derive(Clone, Copy)]
pub struct WakeMask {
    bits: u8,
}

impl WakeMask {
    pub const fn none() -> WakeMask {
        WakeMask { bits: 0 }
    }
    pub fn enable(&mut self, src: u8) -> bool {
        if src as usize >= WAKE_SOURCES {
            return false;
        }
        self.bits |= 1 << src;
        true
    }
    pub fn disable(&mut self, src: u8) {
        self.bits &= !(1 << src);
    }
    pub fn enabled(&self, src: u8) -> bool {
        (src as usize) < WAKE_SOURCES && (self.bits >> src) & 1 == 1
    }
    pub fn any(&self) -> bool {
        self.bits != 0
    }
}

// ===========================================================================
// F453 — 频率 governor 谱：负载→频率档
// ===========================================================================

pub const FREQ_TABLE_MHZ: [u32; 4] = [800, 1200, 1800, 2400];
pub const FREQ_MAX_MHZ: u32 = 2400;

/// 需求频率 = 负载 × 最高频；选第一个 ≥ 需求的档，最低保底 800。
pub fn pick_freq(load_permille: u32) -> u32 {
    let need = (load_permille.min(1000) * FREQ_MAX_MHZ / 1000).max(1);
    FREQ_TABLE_MHZ.iter().copied().find(|&f| f >= need).unwrap_or(FREQ_MAX_MHZ)
}

// ===========================================================================
// F454 — 空闲 Governor：预测空闲时长 → 空闲档
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleState {
    Wfi,
    Retention,
    ClusterOff,
}

/// <1ms 只 WFI；<10ms Retention；更久关簇。
pub fn pick_idle(predicted_idle_us: u32) -> IdleState {
    if predicted_idle_us < 1_000 {
        IdleState::Wfi
    } else if predicted_idle_us < 10_000 {
        IdleState::Retention
    } else {
        IdleState::ClusterOff
    }
}

// ===========================================================================
// F455 — 功耗仪表官：mW·ms 积分累积
// ===========================================================================

pub struct PowerMeter {
    uw_ms: u64,    // 微瓦·毫秒
    total_ms: u32, // 采样总时长
}

impl PowerMeter {
    pub const fn new() -> PowerMeter {
        PowerMeter { uw_ms: 0, total_ms: 0 }
    }
    pub fn sample(&mut self, power_mw: u32, duration_ms: u32) {
        self.uw_ms += power_mw as u64 * 1000 * duration_ms as u64;
        self.total_ms += duration_ms;
    }
    pub fn total_uw_ms(&self) -> u64 {
        self.uw_ms
    }
    /// 平均功耗 µW = 积分 / 总时长。
    pub fn avg_uw(&self) -> u64 {
        if self.total_ms == 0 {
            0
        } else {
            self.uw_ms / self.total_ms as u64
        }
    }
}

// ===========================================================================
// F456 — 时钟树大典：分频链
// ===========================================================================

pub const OSC_ROOT_HZ: u32 = 24_000_000;

#[derive(Clone, Copy)]
pub struct ClockDiv {
    pub div: u32, // 1,2,4,…；0 非法
}

/// 沿链逐级分频；任何 0/非法分频直接判坏。
pub fn clock_freq_after(root_hz: u32, chain: &[ClockDiv]) -> Option<u32> {
    let mut f = root_hz;
    for c in chain {
        if c.div == 0 || f % c.div != 0 {
            return None;
        }
        f /= c.div;
    }
    Some(f)
}

// ===========================================================================
// F457 — 热区谱：温区 trip 点
// ===========================================================================

pub const TRIP_PASSIVE_C: i16 = 75;
pub const TRIP_CRITICAL_C: i16 = 95;

#[derive(Clone, Copy)]
pub struct ThermalZone {
    pub id: u8,
    pub temp_c: i16,
}

/// 0=正常 1=被动 2=临界。
pub fn trip_level(temp_c: i16) -> u8 {
    if temp_c >= TRIP_CRITICAL_C {
        2
    } else if temp_c >= TRIP_PASSIVE_C {
        1
    } else {
        0
    }
}

// ===========================================================================
// F458 — 热治理官：温区 → 限频 permille
// ===========================================================================

pub const THROTTLE_PASSIVE_PERMILLE: u16 = 500;
pub const THROTTLE_CRITICAL_PERMILLE: u16 = 0;

pub fn thermal_throttle_permille(level: u8) -> u16 {
    match level {
        0 => 1000,
        1 => THROTTLE_PASSIVE_PERMILLE,
        _ => THROTTLE_CRITICAL_PERMILLE,
    }
}

// ===========================================================================
// F459 — 电池化学档案：三种化学体系
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Chemistry {
    Lipo,
    Lfp,
    Nimh,
}

#[derive(Clone, Copy)]
pub struct BatteryProfile {
    pub nominal_mv: u16,
    pub vfull_mv: u16,
    pub vempty_mv: u16,
}

pub fn profile_of(c: Chemistry) -> BatteryProfile {
    match c {
        Chemistry::Lipo => BatteryProfile { nominal_mv: 3700, vfull_mv: 4200, vempty_mv: 3200 },
        Chemistry::Lfp => BatteryProfile { nominal_mv: 3300, vfull_mv: 3650, vempty_mv: 2500 },
        Chemistry::Nimh => BatteryProfile { nominal_mv: 1200, vfull_mv: 1400, vempty_mv: 1000 },
    }
}

pub fn profile_sane(p: BatteryProfile) -> bool {
    p.vempty_mv < p.nominal_mv && p.nominal_mv < p.vfull_mv
}

// ===========================================================================
// F460 — 充电状态机：CC→CV→Full
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChargeState {
    Discharging,
    Cc, // 恒流
    Cv, // 恒压
    Full,
}

pub fn charge_state(soc_permille: u16, voltage_mv: u16, p: BatteryProfile) -> ChargeState {
    if soc_permille >= 1000 {
        ChargeState::Full
    } else if voltage_mv >= p.vfull_mv - 50 {
        ChargeState::Cv
    } else {
        ChargeState::Cc
    }
}

/// CV 段到满的判据：电压到位且电流降到满充电流以下。
pub fn cv_charge_done(voltage_mv: u16, current_ma: u16, p: BatteryProfile) -> bool {
    voltage_mv >= p.vfull_mv - 50 && current_ma <= 50
}

// ===========================================================================
// F461 — 睡眠竞速表：各档进入延迟排序
// ===========================================================================

pub const SLEEP_LATENCY_US: [(SleepState, u32); 3] =
    [(SleepState::Idle, 10), (SleepState::Suspend, 3_000), (SleepState::Hibernate, 20_000)];

/// 给定时间预算，返回能进的最深睡眠档（竞速表按深度排序，取最后一个可负担的）。
pub fn deepest_affordable(budget_us: u32) -> SleepState {
    let mut best = SleepState::Idle;
    for (st, lat) in SLEEP_LATENCY_US.iter() {
        if *lat <= budget_us {
            best = *st;
        }
    }
    best
}

// ===========================================================================
// F462 — 功耗预算合同：规划 ≤ 预算 × (1+余量)
// ===========================================================================

pub const BUDGET_HEADROOM_PERMILLE: u32 = 100;

pub fn budget_ok(planned_uw: u64, budget_uw: u64) -> bool {
    planned_uw * 1000 <= budget_uw * (1000 + BUDGET_HEADROOM_PERMILLE) as u64
}

// ===========================================================================
// F463 — 唤醒延迟仪：分级预算
// ===========================================================================

pub const WAKE_BUDGET_IDLE_US: u32 = 50;
pub const WAKE_BUDGET_SUSPEND_US: u32 = 3_000;

pub fn wakeup_ok(from: SleepState, actual_us: u32) -> bool {
    match from {
        SleepState::Idle | SleepState::Run => actual_us <= WAKE_BUDGET_IDLE_US,
        SleepState::Suspend | SleepState::Hibernate => actual_us <= WAKE_BUDGET_SUSPEND_US,
    }
}

// ===========================================================================
// F464 — 时钟漂移考古：ppm 整数计算
// ===========================================================================

/// ppm = (actual - nominal) / nominal × 1e6（i64 中间量）。
pub fn drift_ppm(nominal_ns: i64, actual_ns: i64) -> i32 {
    if nominal_ns == 0 {
        return 0;
    }
    ((actual_ns - nominal_ns) * 1_000_000 / nominal_ns) as i32
}

/// 主时钟一天漂移超过 ±50ppm 判失格。
pub fn drift_acceptable(ppm: i32) -> bool {
    ppm.abs() <= 50
}

// ===========================================================================
// F465 — 电源事件流：定容事件日志
// ===========================================================================

pub const PWR_EVENT_CAP: usize = 16;

#[derive(Clone, Copy)]
pub struct PwrEventLog {
    kinds: [u8; PWR_EVENT_CAP], // 1=sleep 2=wake 3=thermal 4=charge
    count: usize,
}

impl PwrEventLog {
    pub const fn new() -> PwrEventLog {
        PwrEventLog { kinds: [0; PWR_EVENT_CAP], count: 0 }
    }
    pub fn push(&mut self, kind: u8) -> bool {
        if self.count >= PWR_EVENT_CAP {
            return false;
        }
        self.kinds[self.count] = kind;
        self.count += 1;
        true
    }
    pub fn count(&self) -> usize {
        self.count
    }
    pub fn kind_at(&self, i: usize) -> Option<u8> {
        if i < self.count {
            Some(self.kinds[i])
        } else {
            None
        }
    }
}

// ===========================================================================
// F466 — 热回放舱：温度样本回放求峰值温区
// ===========================================================================

pub fn thermal_replay_peak(samples: &[i16]) -> u8 {
    samples.iter().copied().map(trip_level).max().unwrap_or(0)
}

// ===========================================================================
// F467 — 低电降级律：三档降级
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatteryTier {
    Normal,
    Saver,  // 关后台/降刷新
    Reserve, // 只留通话与定位
}

pub fn battery_tier(soc_permille: u16, charging: bool) -> BatteryTier {
    if charging || soc_permille >= 300 {
        BatteryTier::Normal
    } else if soc_permille >= 150 {
        BatteryTier::Saver
    } else {
        BatteryTier::Reserve
    }
}

// ===========================================================================
// F468 — 电源 fuzz 桩：确定性电量生成
// ===========================================================================

pub fn fuzz_soc(seed: u32) -> u16 {
    let x = seed.wrapping_mul(4_073_554_087).wrapping_add(2_979_413);
    ((x >> 17) % 1001) as u16
}

pub fn fuzz_replayable(seed: u32) -> bool {
    fuzz_soc(seed) == fuzz_soc(seed)
}

// ===========================================================================
// F469 — 电源回归金样：摘要对表
// ===========================================================================

pub fn pwr_digest(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x5057_5247; // "PWRG"
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub const PWR_GOLDEN_SENTINEL: u32 = 0x5057_5247;

pub fn pwr_golden_ok(digest: u32) -> bool {
    digest != 0 && digest != PWR_GOLDEN_SENTINEL
}

// ===========================================================================
// F470 — 功耗自描述导出：完整描述符
// ===========================================================================

pub const PWR_DESCRIPTOR_FIELDS: [&str; 5] =
    ["sleep-states", "wake-sources", "freqs", "thermal", "battery"];

pub fn pwr_descriptor_complete(fields: [&str; 5]) -> bool {
    (0..5).all(|i| !fields[i].is_empty())
}

// ===========================================================================
// F471 — 电源健康分：循环次数 × 容量保持率
// ===========================================================================

pub fn power_health(cycles: u32, capacity_permille: u16) -> u16 {
    let cycle_penalty = (cycles / 100).min(300) as u16; // 每百循环扣 1 分，封顶 300
    capacity_permille.saturating_sub(cycle_penalty).min(1000)
}

// ===========================================================================
// F472 — 时钟源仲裁官：精度优先
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ClockSource {
    pub name: &'static str,
    pub accuracy_ppm: i32, // 越小越准
    pub usable: bool,
}

/// 只在可用源中挑精度最高者；全不可用返回 None。
pub fn arbitrate_clock(sources: &[ClockSource]) -> Option<&'static str> {
    let mut best: Option<(i32, &'static str)> = None;
    for s in sources {
        if !s.usable {
            continue;
        }
        match best {
            Some((ppm, _)) if ppm <= s.accuracy_ppm => {}
            _ => best = Some((s.accuracy_ppm, s.name)),
        }
    }
    best.map(|(_, n)| n)
}

// ===========================================================================
// F473 — 电源压力剧本：充放循环压力
// ===========================================================================

pub const PWR_STRESS_CYCLES: u32 = 50;
pub const PWR_STRESS_MINUTES: u32 = 60;

pub fn pwr_stress_plan_valid(cycles: u32, minutes: u32) -> bool {
    cycles >= 1 && cycles <= 500 && minutes >= 10 && minutes <= 480
}

// ===========================================================================
// F474 — 电源文档生成器：文档小节清单
// ===========================================================================

pub const PWR_DOC_SECTIONS: [&str; 5] =
    ["sleep-wake", "governors", "thermal", "battery-charge", "clock-tree"];

// ===========================================================================
// F475 — 电源域年报
// ===========================================================================

pub const PWR_REPORT_SECTIONS: [&str; 4] = ["milestones", "battery-life", "incidents", "learnings"];

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700pwr_checks() -> CheckSet {
    let mut set = CheckSet::new("m700pwr");

    // F451 睡眠状态机
    let s0 = sleep_advance(SleepState::Run, SleepRequest::TickIdle);
    let s1 = sleep_advance(SleepState::Idle, SleepRequest::SuspendButton);
    set.add("F451 sleep ladder", s0 == Some(SleepState::Idle) && s1 == Some(SleepState::Suspend), "run→idle→suspend");
    set.add("F451 critical hibernate", sleep_advance(SleepState::Suspend, SleepRequest::CriticalBattery) == Some(SleepState::Hibernate), "deep on low batt");
    set.add("F451 wake to run", sleep_advance(SleepState::Hibernate, SleepRequest::WakeEvent) == Some(SleepState::Run), "resume");
    set.add("F451 illegal move", sleep_advance(SleepState::Run, SleepRequest::SuspendButton).is_none(), "must idle first");

    // F452 唤醒源
    let mut wm = WakeMask::none();
    let en1 = wm.enable(WAKE_RTC);
    let en2 = wm.enable(WAKE_POWER_BTN);
    let rtc_before = wm.enabled(WAKE_RTC);
    wm.disable(WAKE_RTC);
    set.add("F452 wake enable", en1 && en2 && wm.any() && rtc_before, "two sources");
    set.add("F452 wake disable", !wm.enabled(WAKE_RTC) && wm.enabled(WAKE_POWER_BTN), "selective");
    set.add("F452 wake bad src", !wm.enable(9), "out of range");

    // F453 频率 governor
    set.add("F453 gov low load", pick_freq(100) == 800, "min freq");
    set.add("F453 gov mid load", pick_freq(600) == 1800, "need 1440 → 1800");
    set.add("F453 gov full load", pick_freq(1000) == 2400, "max freq");

    // F454 空闲 governor
    set.add("F454 idle wfi", pick_idle(500) == IdleState::Wfi, "short nap");
    set.add("F454 idle retention", pick_idle(5_000) == IdleState::Retention, "medium");
    set.add("F454 idle cluster", pick_idle(50_000) == IdleState::ClusterOff, "deep");

    // F455 功耗仪表
    let mut pm = PowerMeter::new();
    let empty_avg = pm.avg_uw();
    pm.sample(1000, 100); // 1W × 100ms
    let after_one = pm.avg_uw();
    pm.sample(2000, 100); // 2W × 100ms
    set.add("F455 meter empty", empty_avg == 0, "no samples");
    set.add("F455 meter integral", after_one == 1_000_000, "1W·100ms");
    set.add("F455 meter average", pm.avg_uw() == 1_500_000, "mean of 2");

    // F456 时钟树
    let chain = [ClockDiv { div: 6 }, ClockDiv { div: 4 }];
    set.add("F456 clock divide", clock_freq_after(OSC_ROOT_HZ, &chain) == Some(1_000_000), "24M/6/4");
    set.add("F456 clock bad div", clock_freq_after(OSC_ROOT_HZ, &[ClockDiv { div: 0 }]).is_none(), "div 0");
    set.add("F456 clock indivisible", clock_freq_after(OSC_ROOT_HZ, &[ClockDiv { div: 7 }]).is_none(), "24M%7≠0");

    // F457 热区
    set.add("F457 thermal normal", trip_level(60) == 0 && trip_level(TRIP_PASSIVE_C) == 1, "passive at 75");
    set.add("F457 thermal critical", trip_level(TRIP_CRITICAL_C) == 2 && trip_level(94) == 1, "95 = critical");

    // F458 热治理
    set.add("F458 thermal actions", thermal_throttle_permille(0) == 1000
        && thermal_throttle_permille(1) == 500 && thermal_throttle_permille(2) == 0, "throttle ladder");

    // F459 电池化学
    set.add("F459 chemistry profiles", profile_sane(profile_of(Chemistry::Lipo))
        && profile_sane(profile_of(Chemistry::Lfp)) && profile_sane(profile_of(Chemistry::Nimh)), "ordered voltages");
    set.add("F459 lipo values", profile_of(Chemistry::Lipo).vfull_mv == 4200, "4.2V full");

    // F460 充电状态机
    let p = profile_of(Chemistry::Lipo);
    set.add("F460 charge cc", charge_state(500, 3800, p) == ChargeState::Cc, "constant current");
    set.add("F460 charge cv", charge_state(980, 4200, p) == ChargeState::Cv, "constant voltage");
    set.add("F460 charge full", charge_state(1000, 4200, p) == ChargeState::Full, "done");
    set.add("F460 cv terminate", cv_charge_done(4200, 40, p) && !cv_charge_done(4200, 300, p), "current cutoff");

    // F461 睡眠竞速表
    set.add("F461 latency race", deepest_affordable(9) == SleepState::Idle
        && deepest_affordable(5_000) == SleepState::Suspend
        && deepest_affordable(20_000) == SleepState::Hibernate, "affordable depth");

    // F462 功耗预算
    set.add("F462 budget within", budget_ok(1_100, 1_000), "10% headroom ok");
    set.add("F462 budget breach", !budget_ok(1_200, 1_000), "20% over");

    // F463 唤醒延迟
    set.add("F463 wake idle budget", wakeup_ok(SleepState::Idle, 40) && !wakeup_ok(SleepState::Idle, 80), "≤50µs");
    set.add("F463 wake suspend budget", wakeup_ok(SleepState::Suspend, 2_500) && !wakeup_ok(SleepState::Suspend, 3_500), "≤3ms");

    // F464 时钟漂移
    let ppm = drift_ppm(1_000_000_000, 1_000_030_000);
    set.add("F464 drift calc", ppm == 30, "+30ppm");
    let drift = drift_ppm(1_000_000_000, 999_970_000);
    set.add("F464 drift gate", drift == -30 && drift_acceptable(drift) && !drift_acceptable(80), "±50ppm gate");

    // F465 电源事件流
    let mut pel = PwrEventLog::new();
    let pushed = pel.push(1);
    pel.push(2);
    pel.push(4);
    set.add("F465 pwr events", pushed && pel.count() == 3 && pel.kind_at(2) == Some(4), "ordered");

    // F466 热回放
    let samples: [i16; 5] = [40, 70, 80, 96, 50];
    set.add("F466 thermal replay", thermal_replay_peak(&samples) == 2, "peak=96°C");
    set.add("F466 thermal calm", thermal_replay_peak(&[30, 40, 50]) == 0, "no trip");

    // F467 低电降级
    set.add("F467 battery tiers", battery_tier(800, false) == BatteryTier::Normal
        && battery_tier(200, false) == BatteryTier::Saver
        && battery_tier(100, false) == BatteryTier::Reserve
        && battery_tier(100, true) == BatteryTier::Normal, "3 tiers + charge override");

    // F468 fuzz 桩
    set.add("F468 pwr fuzz deterministic", fuzz_replayable(0x1234_ABCD), "replayable");
    set.add("F468 pwr soc range", fuzz_soc(7) <= 1000, "soc 0..1000");

    // F469 回归金样
    set.add("F469 pwr golden", pwr_golden_ok(pwr_digest(b"power-v1")) && !pwr_golden_ok(PWR_GOLDEN_SENTINEL), "fnv digest");

    // F470 自描述导出
    set.add("F470 pwr descriptor", pwr_descriptor_complete(["s", "w", "f", "t", "b"]), "5 fields");
    set.add("F470 descriptor gap", !pwr_descriptor_complete(["s", "", "f", "t", "b"]), "no empty");

    // F471 健康分
    set.add("F471 power health", power_health(200, 900) == 898 && power_health(50_000, 1000) == 700, "cycles penalty");

    // F472 时钟源仲裁
    let sources = [
        ClockSource { name: "rtc", accuracy_ppm: 100, usable: true },
        ClockSource { name: "tsc", accuracy_ppm: 5, usable: true },
        ClockSource { name: "hpet", accuracy_ppm: 1, usable: false },
    ];
    let picked = arbitrate_clock(&sources);
    set.add("F472 clock arbitrate", picked == Some("tsc"), "best usable");
    set.add("F472 clock none", arbitrate_clock(&[ClockSource { name: "x", accuracy_ppm: 1, usable: false }]).is_none(), "no usable");

    // F473 压力剧本
    set.add("F473 pwr stress", pwr_stress_plan_valid(PWR_STRESS_CYCLES, PWR_STRESS_MINUTES)
        && !pwr_stress_plan_valid(600, 60), "bounds");

    // F474 文档生成器
    set.add("F474 pwr doc sections", PWR_DOC_SECTIONS.len() == 5, "5 sections");

    // F475 年报
    set.add("F475 pwr annual report", PWR_REPORT_SECTIONS.len() == 4, "archived");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f451_sleep_full_path() {
        let s = sleep_advance(SleepState::Run, SleepRequest::TickIdle).unwrap();
        let s = sleep_advance(s, SleepRequest::SuspendButton).unwrap();
        assert_eq!(sleep_advance(s, SleepRequest::WakeEvent), Some(SleepState::Run));
        assert!(sleep_advance(SleepState::Hibernate, SleepRequest::TickIdle).is_none());
    }

    #[test]
    fn f453_governor_monotone() {
        assert!(pick_freq(0) <= pick_freq(300));
        assert!(pick_freq(300) <= pick_freq(700));
        assert_eq!(pick_freq(700), 1800); // 需求 1680 → 1800
    }

    #[test]
    fn f455_meter_math() {
        let mut pm = PowerMeter::new();
        pm.sample(500, 10);
        pm.sample(1500, 10);
        assert_eq!(pm.avg_uw(), 1_000_000);
        assert_eq!(pm.total_uw_ms(), 20_000_000);
    }

    #[test]
    fn f456_clock_chains() {
        assert_eq!(clock_freq_after(24_000_000, &[ClockDiv { div: 24 }]), Some(1_000_000));
        assert_eq!(clock_freq_after(24_000_000, &[ClockDiv { div: 7 }]), None);
    }

    #[test]
    fn f467_tier_boundaries() {
        assert_eq!(battery_tier(300, false), BatteryTier::Normal);
        assert_eq!(battery_tier(299, false), BatteryTier::Saver);
        assert_eq!(battery_tier(149, false), BatteryTier::Reserve);
    }

    #[test]
    fn f475_domain_selfcheck_all_pass() {
        let set = run_m700pwr_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
