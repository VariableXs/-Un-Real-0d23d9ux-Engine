//! m600pwr — VARIX-M600 AI-06 电源与热域 (F126~F150)
//!
//! 全景功耗仪表/电池健康教练/充电曲线策展/热优雅降级/风扇声景/
//! 睡眠深度谱/唤醒源审计/现代待机调优/功耗信用系统/场景功耗剧本/
//! 低电量优雅模式/外设功耗门房/屏幕亮度策展/充电护城河/热历史回放/
//! 温度云图/功耗预算表/待机吞噬者猎手/睡眠失败解剖/唤醒延迟赛跑/
//! 能效基线库/节能模式自学习/电量预估先知/功耗异常哨兵/能源年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F126 — 全景功耗仪表：cpu + gpu + display + other = total 恒等式
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PwrGauge {
    pub cpu_mw: u32,
    pub gpu_mw: u32,
    pub display_mw: u32,
    pub other_mw: u32,
}

impl PwrGauge {
    pub fn total_mw(&self) -> u32 {
        self.cpu_mw + self.gpu_mw + self.display_mw + self.other_mw
    }

    /// 某分量占总功耗的比例（‰）。总功耗为 0 返回 0。
    pub fn share_permille(&self, component_mw: u32) -> u32 {
        let total = self.total_mw();
        if total == 0 {
            0
        } else {
            component_mw * 1000 / total
        }
    }
}

// ===========================================================================
// F127 — 电池健康教练：满电容量 / 设计容量 = 健康度
// ===========================================================================

pub const PWR_HEALTH_GOOD_PERMILLE: u32 = 900;
pub const PWR_HEALTH_FAIR_PERMILLE: u32 = 600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwrHealthGrade {
    Good,
    Fair,
    Poor,
}

#[derive(Clone, Copy, Debug)]
pub struct PwrBattery {
    pub design_mah: u32,
    pub full_mah: u32,
    pub cycles: u32,
}

impl PwrBattery {
    /// 健康度（‰）。设计容量为 0 视为无效，返回 0。
    pub fn health_permille(&self) -> u32 {
        if self.design_mah == 0 {
            0
        } else {
            self.full_mah * 1000 / self.design_mah
        }
    }

    pub fn grade(&self) -> PwrHealthGrade {
        let h = self.health_permille();
        if h >= PWR_HEALTH_GOOD_PERMILLE {
            PwrHealthGrade::Good
        } else if h >= PWR_HEALTH_FAIR_PERMILLE {
            PwrHealthGrade::Fair
        } else {
            PwrHealthGrade::Poor
        }
    }
}

// ===========================================================================
// F128 — 充电曲线策展：高电量段电流递减（taper）
// ===========================================================================

pub const PWR_TAPER_AT_PERMILLE: u32 = 800;
pub const PWR_TRICKLE_AT_PERMILLE: u32 = 900;
pub const PWR_CHARGE_FULL_MA: u32 = 3000;
pub const PWR_CHARGE_TAPER_MA: u32 = 1200;
pub const PWR_CHARGE_TRICKLE_MA: u32 = 500;

/// 800‰ 以下全流充电；800~900‰ 收敛；900‰ 以上涓流。
pub fn pwr_charge_current_ma(batt_permille: u32) -> u32 {
    if batt_permille > PWR_TRICKLE_AT_PERMILLE {
        PWR_CHARGE_TRICKLE_MA
    } else if batt_permille >= PWR_TAPER_AT_PERMILLE {
        PWR_CHARGE_TAPER_MA
    } else {
        PWR_CHARGE_FULL_MA
    }
}

// ===========================================================================
// F129 — 热优雅降级：温度 → 节流等级 0~4
// ===========================================================================

pub const PWR_THROTTLE_LEVELS: u32 = 5;

/// 温度以 0.1°C 计（dc）。600/700/800/900 为升级线。
pub fn pwr_throttle_level(temp_dc: u32) -> u32 {
    if temp_dc >= 900 {
        4
    } else if temp_dc >= 800 {
        3
    } else if temp_dc >= 700 {
        2
    } else if temp_dc >= 600 {
        1
    } else {
        0
    }
}

// ===========================================================================
// F130 — 风扇声景：温度 → 占空比 permille 曲线
// ===========================================================================

pub const PWR_FAN_START_DC: u32 = 400;
pub const PWR_FAN_FULL_DC: u32 = 800;
pub const PWR_FAN_IDLE_DUTY: u32 = 200;

/// 40°C 以下停转；80°C 顶格；中间从 200‰ 线性爬升到 1000‰。
pub fn pwr_fan_duty_permille(temp_dc: u32) -> u32 {
    if temp_dc >= PWR_FAN_FULL_DC {
        1000
    } else if temp_dc <= PWR_FAN_START_DC {
        0
    } else {
        PWR_FAN_IDLE_DUTY + (temp_dc - PWR_FAN_START_DC) * (1000 - PWR_FAN_IDLE_DUTY)
            / (PWR_FAN_FULL_DC - PWR_FAN_START_DC)
    }
}

// ===========================================================================
// F131 — 睡眠深度谱：S0Idle / S3 / S4 深度阶梯
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwrSleepState {
    S0Idle,
    S3,
    S4,
}

/// 睡得越深，唤醒代价越大（深度 ‰）。
pub fn pwr_sleep_depth_permille(state: PwrSleepState) -> u32 {
    match state {
        PwrSleepState::S0Idle => 100,
        PwrSleepState::S3 => 700,
        PwrSleepState::S4 => 1000,
    }
}

// ===========================================================================
// F132 — 唤醒源审计：唤醒源登记册（id 去重 + 容量上限）
// ===========================================================================

pub const PWR_WAKE_SOURCE_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct PwrWakeAudit {
    ids: [u32; PWR_WAKE_SOURCE_CAP],
    armed: [bool; PWR_WAKE_SOURCE_CAP],
    len: usize,
}

impl PwrWakeAudit {
    pub const fn new() -> PwrWakeAudit {
        PwrWakeAudit {
            ids: [0; PWR_WAKE_SOURCE_CAP],
            armed: [false; PWR_WAKE_SOURCE_CAP],
            len: 0,
        }
    }

    /// 登记并武装一个唤醒源。同 id 去重、册满（≥ 8）拒绝。
    pub fn arm(&mut self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == id {
                return false;
            }
            i += 1;
        }
        if self.len >= PWR_WAKE_SOURCE_CAP {
            return false;
        }
        self.ids[self.len] = id;
        self.armed[self.len] = true;
        self.len += 1;
        true
    }

    /// 解除一个唤醒源。不存在返回 false。
    pub fn disarm(&mut self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == id {
                self.armed[i] = false;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn armed_count(&self) -> usize {
        self.armed[..self.len].iter().filter(|&&a| a).count()
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ===========================================================================
// F133 — 现代待机调优：入眠/唤醒双预算
// ===========================================================================

pub const PWR_STANDBY_ENTRY_BUDGET_MS: u32 = 2000;
pub const PWR_STANDBY_RESUME_BUDGET_MS: u32 = 5000;

/// 双预算都达标才算 Fast 待机。
pub fn pwr_standby_fast(entry_ms: u32, resume_ms: u32) -> bool {
    entry_ms <= PWR_STANDBY_ENTRY_BUDGET_MS && resume_ms <= PWR_STANDBY_RESUME_BUDGET_MS
}

// ===========================================================================
// F134 — 功耗信用系统：省电赚积分，突发花积分，封顶不透支
// ===========================================================================

pub const PWR_CREDIT_CAP: u32 = 1000;

#[derive(Clone, Copy, Debug)]
pub struct PwrCredit {
    balance: u32,
}

impl PwrCredit {
    pub const fn new() -> PwrCredit {
        PwrCredit { balance: 0 }
    }

    /// 赚积分。余额封顶 1000。
    pub fn earn(&mut self, n: u32) {
        let sum = self.balance + n;
        if sum > PWR_CREDIT_CAP {
            self.balance = PWR_CREDIT_CAP;
        } else {
            self.balance = sum;
        }
    }

    /// 花积分。余额不足拒绝，绝不透支。
    pub fn spend(&mut self, n: u32) -> bool {
        if n > self.balance {
            return false;
        }
        self.balance -= n;
        true
    }

    pub fn balance(&self) -> u32 {
        self.balance
    }
}

// ===========================================================================
// F135 — 场景功耗剧本：场景 → 功耗预算，实际不得超
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwrScenario {
    Idle,
    Video,
    Gaming,
}

pub fn pwr_scenario_budget_mw(scenario: PwrScenario) -> u32 {
    match scenario {
        PwrScenario::Idle => 2000,
        PwrScenario::Video => 6000,
        PwrScenario::Gaming => 15000,
    }
}

pub fn pwr_within_budget(scenario: PwrScenario, actual_mw: u32) -> bool {
    actual_mw <= pwr_scenario_budget_mw(scenario)
}

// ===========================================================================
// F136 — 低电量优雅模式：≤150‰ 进入省电，功耗打七折
// ===========================================================================

pub const PWR_SAVER_ENTER_PERMILLE: u32 = 150;
pub const PWR_SAVER_SCALE_PERMILLE: u32 = 700;

pub fn pwr_saver_active(batt_permille: u32) -> bool {
    batt_permille <= PWR_SAVER_ENTER_PERMILLE
}

/// 省电模式下的功耗折算。
pub fn pwr_saver_scale_mw(actual_mw: u32) -> u32 {
    actual_mw * PWR_SAVER_SCALE_PERMILLE / 1000
}

// ===========================================================================
// F137 — 外设功耗门房：外设用电准入（id 去重 + 预算闸门）
// ===========================================================================

pub const PWR_GATE_DEVICE_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct PwrPeripheralGate {
    budget_mw: u32,
    ids: [u32; PWR_GATE_DEVICE_CAP],
    draws: [u32; PWR_GATE_DEVICE_CAP],
    len: usize,
}

impl PwrPeripheralGate {
    pub const fn new(budget_mw: u32) -> PwrPeripheralGate {
        PwrPeripheralGate {
            budget_mw,
            ids: [0; PWR_GATE_DEVICE_CAP],
            draws: [0; PWR_GATE_DEVICE_CAP],
            len: 0,
        }
    }

    pub fn committed_mw(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            sum += self.draws[i];
            i += 1;
        }
        sum
    }

    /// 准入一个外设。同 id 去重、超预算或位满拒绝，承诺功耗绝不超额。
    pub fn admit(&mut self, id: u32, draw_mw: u32) -> bool {
        if self.len >= PWR_GATE_DEVICE_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == id {
                return false;
            }
            i += 1;
        }
        if self.committed_mw() + draw_mw > self.budget_mw {
            return false;
        }
        self.ids[self.len] = id;
        self.draws[self.len] = draw_mw;
        self.len += 1;
        true
    }

    /// 释放一个外设（原地压缩）。不存在返回 false。
    pub fn release(&mut self, id: u32) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == id {
                let mut w = i;
                while w + 1 < self.len {
                    self.ids[w] = self.ids[w + 1];
                    self.draws[w] = self.draws[w + 1];
                    w += 1;
                }
                self.len -= 1;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ===========================================================================
// F138 — 屏幕亮度策展：可见性地板 + 夜间压光
// ===========================================================================

pub const PWR_BRIGHTNESS_FLOOR: u32 = 50;
pub const PWR_NIGHT_SCALE_PERMILLE: u32 = 400;

/// 亮度钳制：不低于可见性地板，不超物理上限。
pub fn pwr_clamp_brightness(permille: u32) -> u32 {
    if permille < PWR_BRIGHTNESS_FLOOR {
        PWR_BRIGHTNESS_FLOOR
    } else if permille > 1000 {
        1000
    } else {
        permille
    }
}

/// 夜间亮度 = 钳制后压到 40%，但不低于可见性地板。
pub fn pwr_night_brightness(permille: u32) -> u32 {
    let scaled = pwr_clamp_brightness(permille) * PWR_NIGHT_SCALE_PERMILLE / 1000;
    if scaled < PWR_BRIGHTNESS_FLOOR {
        PWR_BRIGHTNESS_FLOOR
    } else {
        scaled
    }
}

// ===========================================================================
// F139 — 充电护城河：过热或接近满电即停充
// ===========================================================================

pub const PWR_CHARGE_MAX_TEMP_DC: u32 = 600;
pub const PWR_CHARGE_NEAR_FULL_PERMILLE: u32 = 900;

/// 只有"不太热且没充满"才允许充电（护城河规则）。
pub fn pwr_charge_allowed(batt_permille: u32, temp_dc: u32) -> bool {
    batt_permille < PWR_CHARGE_NEAR_FULL_PERMILLE && temp_dc < PWR_CHARGE_MAX_TEMP_DC
}

// ===========================================================================
// F140 — 热历史回放：定容环形温度事件流，覆盖记账
// ===========================================================================

pub const PWR_THERMAL_RING_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct PwrThermalRing {
    seqs: [u64; PWR_THERMAL_RING_CAP],
    temps: [u32; PWR_THERMAL_RING_CAP],
    head: usize,
    len: usize,
    pub overwritten: u64,
}

impl PwrThermalRing {
    pub const fn new() -> PwrThermalRing {
        PwrThermalRing {
            seqs: [0; PWR_THERMAL_RING_CAP],
            temps: [0; PWR_THERMAL_RING_CAP],
            head: 0,
            len: 0,
            overwritten: 0,
        }
    }

    /// 写入一条温度事件；环满后覆盖最旧事件并累计 overwritten。
    pub fn push(&mut self, seq: u64, temp_dc: u32) {
        if self.len < PWR_THERMAL_RING_CAP {
            self.len += 1;
        } else {
            self.overwritten += 1;
        }
        self.seqs[self.head] = seq;
        self.temps[self.head] = temp_dc;
        self.head = (self.head + 1) % PWR_THERMAL_RING_CAP;
    }

    pub fn seq_at(&self, i: usize) -> u64 {
        self.seqs[i % PWR_THERMAL_RING_CAP]
    }

    pub fn temp_at(&self, i: usize) -> u32 {
        self.temps[i % PWR_THERMAL_RING_CAP]
    }
}

// ===========================================================================
// F141 — 温度云图：固定 4 传感器网格，找热点
// ===========================================================================

pub const PWR_TEMP_SENSORS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct PwrTempMap {
    temps: [u32; PWR_TEMP_SENSORS],
    len: usize,
}

impl PwrTempMap {
    pub const fn new() -> PwrTempMap {
        PwrTempMap { temps: [0; PWR_TEMP_SENSORS], len: 0 }
    }

    /// 采一个传感器温度。满（≥ 4）拒绝。
    pub fn add_sensor(&mut self, temp_dc: u32) -> bool {
        if self.len >= PWR_TEMP_SENSORS {
            return false;
        }
        self.temps[self.len] = temp_dc;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 最热传感器下标（并列取第一个；空图返回 0）。
    pub fn hotspot_index(&self) -> usize {
        let mut best = 0usize;
        let mut i = 1usize;
        while i < self.len {
            if self.temps[i] > self.temps[best] {
                best = i;
            }
            i += 1;
        }
        best
    }

    pub fn max_dc(&self) -> u32 {
        if self.len == 0 {
            0
        } else {
            self.temps[self.hotspot_index()]
        }
    }

    pub fn min_dc(&self) -> u32 {
        if self.len == 0 {
            return 0;
        }
        let mut best = self.temps[0];
        let mut i = 1usize;
        while i < self.len {
            if self.temps[i] < best {
                best = self.temps[i];
            }
            i += 1;
        }
        best
    }

    /// 平均温度（0.1°C，整除截断）。空图返回 0。
    pub fn avg_dc(&self) -> u32 {
        if self.len == 0 {
            return 0;
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            sum += self.temps[i];
            i += 1;
        }
        sum / self.len as u32
    }

    /// 温差 = 最热 − 最冷。
    pub fn spread_dc(&self) -> u32 {
        self.max_dc() - self.min_dc()
    }
}

// ===========================================================================
// F142 — 功耗预算表：按子系统限量发放，绝不透支
// ===========================================================================

pub const PWR_BUDGET_SLOTS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct PwrBudgetTable {
    quotas: [u32; PWR_BUDGET_SLOTS],
    consumed: [u32; PWR_BUDGET_SLOTS],
}

impl PwrBudgetTable {
    pub const fn new(quotas: [u32; PWR_BUDGET_SLOTS]) -> PwrBudgetTable {
        PwrBudgetTable { quotas, consumed: [0; PWR_BUDGET_SLOTS] }
    }

    pub fn remaining(&self, slot: usize) -> u32 {
        if slot < PWR_BUDGET_SLOTS {
            self.quotas[slot] - self.consumed[slot]
        } else {
            0
        }
    }

    /// 申请 n mW 额度，返回实际批准数（不超过剩余）。
    pub fn take(&mut self, slot: usize, want: u32) -> u32 {
        if slot >= PWR_BUDGET_SLOTS {
            return 0;
        }
        let remaining = self.remaining(slot);
        let granted = if want > remaining { remaining } else { want };
        self.consumed[slot] += granted;
        granted
    }
}

// ===========================================================================
// F143 — 待机吞噬者猎手：待机时掉电过快即吞噬者
// ===========================================================================

pub const PWR_DRAIN_ALERT_PERMILLE: u32 = 500;
pub const PWR_DRAIN_MIN_HOURS: u32 = 1;

/// 掉电比例（‰）= (before − after) / before。
pub fn pwr_drain_permille(before_mwh: u32, after_mwh: u32) -> u32 {
    if before_mwh == 0 || after_mwh > before_mwh {
        return 0;
    }
    (before_mwh - after_mwh) * 1000 / before_mwh
}

/// 观察窗足够长且掉电 ≥ 500‰/窗 → 有吞噬者。
pub fn pwr_drain_predator(before_mwh: u32, after_mwh: u32, window_hours: u32) -> bool {
    window_hours >= PWR_DRAIN_MIN_HOURS
        && pwr_drain_permille(before_mwh, after_mwh) >= PWR_DRAIN_ALERT_PERMILLE
}

// ===========================================================================
// F144 — 睡眠失败解剖：失败分类 + 指数退避重试
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PwrSleepFail {
    EntryTimeout,
    ResumeCrash,
    SpuriousWake,
}

pub fn pwr_sleep_fail_retryable(f: PwrSleepFail) -> bool {
    matches!(f, PwrSleepFail::EntryTimeout | PwrSleepFail::SpuriousWake)
}

/// 500ms 起，每次翻倍，封顶 8000ms。
pub fn pwr_sleep_retry_ms(attempt: u32) -> u32 {
    let mut ms = 500u32;
    let mut i = 0u32;
    while i < attempt && ms < 8000 {
        ms *= 2;
        i += 1;
    }
    if ms > 8000 {
        8000
    } else {
        ms
    }
}

// ===========================================================================
// F145 — 唤醒延迟赛跑：多路径唤醒，选最快
// ===========================================================================

pub const PWR_WAKE_RACE_PATHS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct PwrWakeRace {
    lats: [u32; PWR_WAKE_RACE_PATHS],
    len: usize,
}

impl PwrWakeRace {
    pub const fn new() -> PwrWakeRace {
        PwrWakeRace { lats: [0; PWR_WAKE_RACE_PATHS], len: 0 }
    }

    /// 报名一条路径。满（≥ 4）拒绝。
    pub fn add_path(&mut self, latency_us: u32) -> bool {
        if self.len >= PWR_WAKE_RACE_PATHS {
            return false;
        }
        self.lats[self.len] = latency_us;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// 冠军延迟（并列取先报名者；空场返回 0）。
    pub fn best_us(&self) -> u32 {
        if self.len == 0 {
            return 0;
        }
        let mut best = self.lats[0];
        let mut i = 1usize;
        while i < self.len {
            if self.lats[i] < best {
                best = self.lats[i];
            }
            i += 1;
        }
        best
    }

    pub fn best_index(&self) -> usize {
        let mut best = 0usize;
        let mut i = 1usize;
        while i < self.len {
            if self.lats[i] < self.lats[best] {
                best = i;
            }
            i += 1;
        }
        best
    }
}

// ===========================================================================
// F146 — 能效基线库：场景基线登记（id 去重覆盖 + 容量上限）
// ===========================================================================

pub const PWR_BASELINE_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct PwrBaselineLib {
    ids: [u32; PWR_BASELINE_CAP],
    permilles: [u32; PWR_BASELINE_CAP],
    len: usize,
}

impl PwrBaselineLib {
    pub const fn new() -> PwrBaselineLib {
        PwrBaselineLib {
            ids: [0; PWR_BASELINE_CAP],
            permilles: [0; PWR_BASELINE_CAP],
            len: 0,
        }
    }

    /// 登记一个场景基线：已存在则覆盖更新（去重），否则占新槽；槽满拒绝。
    pub fn set_baseline(&mut self, scenario_id: u32, permille: u32) -> bool {
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == scenario_id {
                self.permilles[i] = permille;
                return true;
            }
            i += 1;
        }
        if self.len >= PWR_BASELINE_CAP {
            return false;
        }
        self.ids[self.len] = scenario_id;
        self.permilles[self.len] = permille;
        self.len += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn baseline_of(&self, scenario_id: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.len {
            if self.ids[i] == scenario_id {
                return Some(self.permilles[i]);
            }
            i += 1;
        }
        None
    }
}

// ===========================================================================
// F147 — 节能模式自学习：连续低负载样本自动开启
// ===========================================================================

pub const PWR_LEARN_THRESHOLD: u32 = 6;
pub const PWR_LEARN_LOW_USAGE_PERMILLE: u32 = 200;

#[derive(Clone, Copy, Debug)]
pub struct PwrLearnSaver {
    low_samples: u32,
    pub active: bool,
}

impl PwrLearnSaver {
    pub const fn new() -> PwrLearnSaver {
        PwrLearnSaver { low_samples: 0, active: false }
    }

    /// 喂入一个使用率读数，返回当前是否处于自学习省电态。
    /// 连续 ≥ 6 个低负载样本自动开启；任一高负载样本立即关闭并清零计数。
    pub fn observe(&mut self, usage_permille: u32) -> bool {
        if usage_permille <= PWR_LEARN_LOW_USAGE_PERMILLE {
            self.low_samples += 1;
            if self.low_samples >= PWR_LEARN_THRESHOLD {
                self.active = true;
            }
        } else {
            self.low_samples = 0;
            self.active = false;
        }
        self.active
    }

    pub fn low_sample_count(&self) -> u32 {
        self.low_samples
    }
}

// ===========================================================================
// F148 — 电量预估先知：剩余电量 / 平均功耗 = 可用分钟数
// ===========================================================================

/// mWh ÷ mW × 60 = 分钟。平均功耗为 0（未知）返回 0。
pub fn pwr_remaining_minutes(remaining_mwh: u32, avg_draw_mw: u32) -> u32 {
    if avg_draw_mw == 0 {
        0
    } else {
        remaining_mwh * 60 / avg_draw_mw
    }
}

// ===========================================================================
// F149 — 功耗异常哨兵：读数偏离基线 ≥ 300‰ 即异常
// ===========================================================================

pub const PWR_ANOMALY_PERMILLE: u32 = 300;

/// 双向偏离（|reading − baseline| / baseline，‰）。基线为 0 无法归一，返回 0。
pub fn pwr_deviation_permille(baseline: u32, reading: u32) -> u32 {
    if baseline == 0 {
        return 0;
    }
    if reading >= baseline {
        (reading - baseline) * 1000 / baseline
    } else {
        (baseline - reading) * 1000 / baseline
    }
}

pub fn pwr_anomaly(baseline: u32, reading: u32) -> bool {
    baseline != 0 && pwr_deviation_permille(baseline, reading) >= PWR_ANOMALY_PERMILLE
}

// ===========================================================================
// F150 — 能源年报：全年 52 周覆盖 + 章节完备
// ===========================================================================

pub const PWR_ANNUAL_WEEKS: u32 = 52;
pub const PWR_ANNUAL_SECTIONS: [&str; 5] =
    ["gauge", "battery", "thermal", "sleep", "budget"];

pub fn pwr_annual_complete(weeks_covered: u32, sections_filled: u32) -> bool {
    weeks_covered == PWR_ANNUAL_WEEKS && sections_filled >= PWR_ANNUAL_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600pwr_checks() -> CheckSet {
    let mut set = CheckSet::new("m600pwr");

    // F126 全景功耗仪表
    let gauge = PwrGauge { cpu_mw: 4000, gpu_mw: 2000, display_mw: 1000, other_mw: 500 };
    let gauge_total = gauge.total_mw();
    let cpu_share = gauge.share_permille(gauge.cpu_mw);
    set.add("F126 power gauge", gauge_total == 7500 && cpu_share == 533, "cpu share of total");
    set.add("F126 empty gauge", PwrGauge { cpu_mw: 0, gpu_mw: 0, display_mw: 0, other_mw: 0 }.share_permille(1) == 0, "no div-zero");

    // F127 电池健康教练
    let good = PwrBattery { design_mah: 4000, full_mah: 3600, cycles: 120 };
    let poor = PwrBattery { design_mah: 4000, full_mah: 2000, cycles: 900 };
    set.add(
        "F127 battery health",
        good.health_permille() == 900 && good.grade() == PwrHealthGrade::Good,
        "90% healthy",
    );
    set.add(
        "F127 battery poor",
        poor.health_permille() == 500 && poor.grade() == PwrHealthGrade::Poor,
        "half capacity",
    );

    // F128 充电曲线策展
    set.add(
        "F128 charge taper",
        pwr_charge_current_ma(799) == 3000
            && pwr_charge_current_ma(800) == 1200
            && pwr_charge_current_ma(900) == 1200
            && pwr_charge_current_ma(901) == 500,
        "full/taper/trickle",
    );

    // F129 热优雅降级
    set.add(
        "F129 throttle levels",
        pwr_throttle_level(599) == 0
            && pwr_throttle_level(600) == 1
            && pwr_throttle_level(899) == 3
            && pwr_throttle_level(900) == 4,
        "ladder 0..4",
    );
    set.add("F129 throttle monotonic", pwr_throttle_level(750) < pwr_throttle_level(850), "hotter means deeper");

    // F130 风扇声景
    set.add(
        "F130 fan curve",
        pwr_fan_duty_permille(400) == 0
            && pwr_fan_duty_permille(600) == 600
            && pwr_fan_duty_permille(800) == 1000,
        "stop/ramp/full",
    );

    // F131 睡眠深度谱
    set.add(
        "F131 sleep depths",
        pwr_sleep_depth_permille(PwrSleepState::S0Idle) == 100
            && pwr_sleep_depth_permille(PwrSleepState::S3) == 700
            && pwr_sleep_depth_permille(PwrSleepState::S4) == 1000,
        "depth ladder",
    );

    // F132 唤醒源审计
    let mut audit = PwrWakeAudit::new();
    let a1 = audit.arm(1);
    let a_dup = audit.arm(1);
    let a2 = audit.arm(2);
    let d1 = audit.disarm(1);
    let d_missing = audit.disarm(9);
    let armed_now = audit.armed_count();
    set.add(
        "F132 wake audit",
        a1 && !a_dup && a2 && d1 && !d_missing && armed_now == 1 && audit.len() == 2,
        "dedup + disarm",
    );
    let mut full_audit = PwrWakeAudit::new();
    let mut wi = 0u32;
    while wi < PWR_WAKE_SOURCE_CAP as u32 {
        full_audit.arm(wi);
        wi += 1;
    }
    let audit_over = full_audit.arm(999);
    set.add("F132 audit capped", !audit_over && full_audit.len() == PWR_WAKE_SOURCE_CAP, "cap rejects");

    // F133 现代待机调优
    set.add(
        "F133 standby budgets",
        pwr_standby_fast(1500, 4000) && !pwr_standby_fast(1500, 6000) && !pwr_standby_fast(3000, 1000),
        "entry + resume",
    );

    // F134 功耗信用系统
    let mut credit = PwrCredit::new();
    credit.earn(600);
    let bal_mid = credit.balance();
    let overspend = credit.spend(800);
    let bal_after_fail = credit.balance();
    let ok_spend = credit.spend(600);
    credit.earn(1200);
    let bal_capped = credit.balance();
    set.add(
        "F134 credit rules",
        bal_mid == 600 && !overspend && bal_after_fail == 600 && ok_spend && bal_capped == PWR_CREDIT_CAP,
        "no overdraft, capped",
    );

    // F135 场景功耗剧本
    set.add(
        "F135 scenario budgets",
        pwr_within_budget(PwrScenario::Idle, 1500)
            && !pwr_within_budget(PwrScenario::Idle, 2500)
            && pwr_within_budget(PwrScenario::Gaming, 15000)
            && pwr_scenario_budget_mw(PwrScenario::Video) == 6000,
        "per-scenario caps",
    );

    // F136 低电量优雅模式
    set.add(
        "F136 saver entry",
        pwr_saver_active(100) && pwr_saver_active(150) && !pwr_saver_active(151),
        "threshold 150",
    );
    set.add("F136 saver scale", pwr_saver_scale_mw(1000) == 700 && pwr_saver_scale_mw(100) == 70, "30% cut");

    // F137 外设功耗门房
    let mut gate = PwrPeripheralGate::new(5000);
    let g1 = gate.admit(1, 2000);
    let g_dup = gate.admit(1, 100);
    let g_over = gate.admit(2, 4000);
    let g2 = gate.admit(2, 3000);
    let committed_full = gate.committed_mw();
    let g_full = gate.admit(3, 1);
    let rel = gate.release(1);
    let committed_after = gate.committed_mw();
    let g3 = gate.admit(3, 1000);
    set.add(
        "F137 gate rules",
        g1 && !g_dup && !g_over && g2 && committed_full == 5000 && !g_full && rel
            && committed_after == 3000 && g3,
        "budget gate + release",
    );

    // F138 屏幕亮度策展
    set.add(
        "F138 brightness clamp",
        pwr_clamp_brightness(20) == 50 && pwr_clamp_brightness(500) == 500 && pwr_clamp_brightness(2000) == 1000,
        "floor and ceiling",
    );
    set.add(
        "F138 night dim",
        pwr_night_brightness(1000) == 400 && pwr_night_brightness(500) == 200 && pwr_night_brightness(20) == 50,
        "40% with floor",
    );

    // F139 充电护城河
    set.add(
        "F139 charge moat",
        pwr_charge_allowed(500, 450)
            && !pwr_charge_allowed(900, 450)
            && !pwr_charge_allowed(500, 600)
            && pwr_charge_allowed(899, 599),
        "hot or full blocks",
    );

    // F140 热历史回放
    let mut ring = PwrThermalRing::new();
    ring.push(1, 450);
    ring.push(2, 700);
    ring.push(3, 550);
    let ring_seq2 = ring.seq_at(1);
    let ring_temp3 = ring.temp_at(2);
    set.add(
        "F140 thermal ring",
        ring_seq2 == 2 && ring_temp3 == 550 && ring.overwritten == 0,
        "events kept",
    );
    let mut wrap = PwrThermalRing::new();
    let mut s = 1u64;
    while s <= 9 {
        wrap.push(s, 500);
        s += 1;
    }
    let wrap_seq8 = wrap.seq_at(7);
    let wrap_seq0 = wrap.seq_at(8);
    set.add(
        "F140 thermal wrap",
        wrap.overwritten == 1 && wrap_seq8 == 8 && wrap_seq0 == 9,
        "oldest overwritten once",
    );

    // F141 温度云图
    let mut tmap = PwrTempMap::new();
    tmap.add_sensor(450);
    tmap.add_sensor(700);
    tmap.add_sensor(600);
    tmap.add_sensor(700);
    let hot_idx = tmap.hotspot_index();
    let map_max = tmap.max_dc();
    let map_avg = tmap.avg_dc();
    let map_spread = tmap.spread_dc();
    set.add(
        "F141 temp map",
        tmap.len() == 4 && hot_idx == 1 && map_max == 700 && map_avg == 612 && map_spread == 250,
        "hotspot + spread",
    );
    let mut full_map = PwrTempMap::new();
    let mut ti = 0;
    while ti < PWR_TEMP_SENSORS {
        full_map.add_sensor(500);
        ti += 1;
    }
    let map_over = full_map.add_sensor(500);
    set.add("F141 map capped", !map_over && full_map.len() == PWR_TEMP_SENSORS, "cap rejects");

    // F142 功耗预算表
    let mut table = PwrBudgetTable::new([1000, 500, 500, 500]);
    let first_grant = table.take(0, 700);
    let mid_left = table.remaining(0);
    let second_grant = table.take(0, 500);
    let bad_slot = table.take(9, 100);
    set.add(
        "F142 budget table",
        first_grant == 700 && mid_left == 300 && second_grant == 300 && table.remaining(0) == 0 && bad_slot == 0,
        "partial grant, no overdraft",
    );

    // F143 待机吞噬者猎手
    set.add(
        "F143 drain math",
        pwr_drain_permille(1000, 400) == 600 && pwr_drain_permille(1000, 900) == 100,
        "drift permille",
    );
    set.add(
        "F143 predator verdict",
        pwr_drain_predator(1000, 400, 1) && !pwr_drain_predator(1000, 900, 1) && !pwr_drain_predator(1000, 400, 0),
        "threshold + window",
    );

    // F144 睡眠失败解剖
    set.add(
        "F144 failure classes",
        pwr_sleep_fail_retryable(PwrSleepFail::EntryTimeout)
            && pwr_sleep_fail_retryable(PwrSleepFail::SpuriousWake)
            && !pwr_sleep_fail_retryable(PwrSleepFail::ResumeCrash),
        "resume crash fatal",
    );
    set.add(
        "F144 retry backoff",
        pwr_sleep_retry_ms(0) == 500 && pwr_sleep_retry_ms(2) == 2000 && pwr_sleep_retry_ms(9) == 8000,
        "exponential capped",
    );

    // F145 唤醒延迟赛跑
    let mut race = PwrWakeRace::new();
    race.add_path(1200);
    race.add_path(800);
    race.add_path(800);
    let race_best = race.best_us();
    let race_idx = race.best_index();
    set.add(
        "F145 wake race",
        race.len() == 3 && race_best == 800 && race_idx == 1,
        "fastest path wins",
    );

    // F146 能效基线库
    let mut lib = PwrBaselineLib::new();
    let b1 = lib.set_baseline(1, 800);
    let b_update = lib.set_baseline(1, 850);
    let len_after_update = lib.len();
    let v1 = lib.baseline_of(1);
    let b2 = lib.set_baseline(2, 700);
    set.add(
        "F146 baseline lib",
        b1 && b_update && len_after_update == 1 && v1 == Some(850) && b2 && lib.len() == 2,
        "dedup updates in place",
    );

    // F147 节能模式自学习
    let mut saver = PwrLearnSaver::new();
    let mut n = 0u32;
    let mut became_active = false;
    while n < PWR_LEARN_THRESHOLD {
        became_active = saver.observe(100);
        n += 1;
    }
    let active_after_low = saver.observe(100);
    let active_after_high = saver.observe(900);
    set.add(
        "F147 learned saver",
        became_active && active_after_low && !active_after_high && saver.low_sample_count() == 0,
        "6 lows on, 1 high off",
    );

    // F148 电量预估先知
    set.add(
        "F148 remaining estimate",
        pwr_remaining_minutes(3000, 500) == 360
            && pwr_remaining_minutes(0, 500) == 0
            && pwr_remaining_minutes(1000, 0) == 0,
        "mwh over mw",
    );

    // F149 功耗异常哨兵
    set.add(
        "F149 anomaly math",
        pwr_deviation_permille(1000, 1400) == 400 && pwr_deviation_permille(1000, 1200) == 200,
        "deviation permille",
    );
    set.add(
        "F149 anomaly verdict",
        pwr_anomaly(1000, 1400) && !pwr_anomaly(1000, 1200) && !pwr_anomaly(0, 1400),
        "threshold + zero baseline",
    );

    // F150 能源年报
    set.add(
        "F150 annual report",
        PWR_ANNUAL_SECTIONS.len() == 5
            && pwr_annual_complete(52, 5)
            && !pwr_annual_complete(51, 5)
            && !pwr_annual_complete(52, 4),
        "weeks + sections",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f132_wake_audit_dedup_cap() {
        let mut a = PwrWakeAudit::new();
        assert!(a.arm(1));
        assert!(!a.arm(1)); // id 去重
        assert!(a.arm(2));
        assert!(a.disarm(2));
        assert_eq!(a.armed_count(), 1);
        let mut i = 0u32;
        while a.len() < PWR_WAKE_SOURCE_CAP {
            assert!(a.arm(10 + i));
            i += 1;
        }
        assert!(!a.arm(999)); // 容量上限
        assert_eq!(a.len(), PWR_WAKE_SOURCE_CAP);
    }

    #[test]
    fn f134_credit_bounds() {
        let mut c = PwrCredit::new();
        c.earn(PWR_CREDIT_CAP);
        c.earn(1); // 封顶不溢出
        assert_eq!(c.balance(), PWR_CREDIT_CAP);
        assert!(!c.spend(PWR_CREDIT_CAP + 1));
        assert!(c.spend(PWR_CREDIT_CAP));
        assert_eq!(c.balance(), 0);
        assert!(!c.spend(1));
    }

    #[test]
    fn f137_gate_never_over_budget() {
        let mut g = PwrPeripheralGate::new(1000);
        assert!(g.admit(1, 600));
        assert!(!g.admit(2, 500)); // 600+500 > 1000
        assert!(g.admit(2, 400));
        assert_eq!(g.committed_mw(), 1000);
        assert!(!g.admit(3, 1));
        assert!(g.release(1));
        assert!(g.admit(3, 1));
        assert_eq!(g.committed_mw(), 401);
    }

    #[test]
    fn f140_thermal_ring_wrap() {
        let mut r = PwrThermalRing::new();
        let mut s = 1u64;
        while s <= 20 {
            r.push(s, 500);
            s += 1;
        }
        assert_eq!(r.overwritten, 12);
        assert_eq!(r.seq_at(19), 20);
        // 槽 3 最近一次被 seq=20 覆盖（(20-1) % 8 == 3）
        assert_eq!(r.seq_at(11), 20);
    }

    #[test]
    fn f147_saver_learning_hysteresis() {
        let mut sv = PwrLearnSaver::new();
        let mut i = 0u32;
        while i < PWR_LEARN_THRESHOLD - 1 {
            assert!(!sv.observe(150));
            i += 1;
        }
        assert!(sv.observe(150)); // 第 6 个样本开启
        assert!(!sv.observe(900)); // 高负载立即关闭
        assert!(!sv.observe(150)); // 单个低样本不足以重启
        assert!(!sv.active);
    }

    #[test]
    fn f150_pwr_selfcheck_all_pass() {
        let set = run_m600pwr_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
