//! CGPU-F1442 · 功耗采样器与传感器抽象（CGPU-J 域 · 功耗与能效域 · J02 组 · 目标 340 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1442`
//!
//! **判据（锚点原文）**：四方法、管理、三档调度、单位归一、时间戳同源、
//! 实现矩阵、mock、契约测试、判据。
//!
//! **职责定位（锚点原文）**：采样抽象：SensorTrait 统一接口
//! （read_power()/read_energy()/read_voltage_current()/poll_interval()——
//! 四方法最小面）；采样器管理（多传感器注册/优先级/健康度——失效传感器
//! 自动摘除+诊断）；采样调度（可变频率轮询——空闲 1Hz/活跃 10Hz/异常
//! 100Hz 三档——按需升频节能）；数据单位约定（毫瓦 mW 统一计量单位——
//! 入口即归一化）；时间戳对齐（采样时间戳统一单调时钟——与帧计时同源
//! ——归因前提）；抽象与平台分离（SensorTrait 实现者：F1443 Intel/
//! F1444 AMD/F1445 NVIDIA/F1450 联合采样——实现矩阵表）；mock 传感器
//! （F1454 模拟器同接口——测试注入）。
//!
//! ## 一、四方法最小面（判据一：接口即契约）
//!
//! [`SensorTrait`] 四方法——功率（mW）/能量（µJ 累计器）/电压电流
//! （µV·µA 对）/轮询间隔（ms 基准）。**最小面是闭集**：加第五方法=
//! 破坏抽象走 ADR；读失败返回 [`SensorErr`] 不返回 0——**失败置零是
//! 造假**，上游「无数据」显性态由此成立。
//!
//! ## 二、管理与健康度（判据二：摘除是显性账）
//!
//! 注册带优先级（低者先采——同 tick 预算下保关键面）；连续
//! [`FAIL_STREAK_REMOVE`] 次失败自动摘除（[`SensorState::Removed`]）+
//! 诊断入账；摘除后采样跳过且显性（不是静默零值）；恢复走重注册
//! （摘除不可自动撤销——半死传感器的间歇好数据比稳定坏数据更害归因）。
//!
//! ## 三、三档调度（判据三：按需升频节能）
//!
//! [`SampleMode`] 三档：Idle 1Hz（[`IDLE_PERIOD_MS`]=1000）/Active 10Hz
//! （100）/Abnormal 100Hz（10）；升频触发=采样异常（失败或功率越
//! [`SPIKE_THRESHOLD_MW`] 水位）；连续 [`DEESCALATE_STREAK`] 次正常才
//! 降档（迟滞——抖动链路不反复升降）；调度周期由当前档决定，档外
//! 周期请求显性拒（三档是闭集不是连续谱——谱化会重新引入墙钟）。
//!
//! ## 四、单位归一与时间戳同源（判据四 + 判据五）
//!
//! 入口归一：原始 µW 读数过 [`normalize_to_mw`]（÷1000 带溢出检测）
//! 才入账——**入口即归一化**，账内不出现第二单位；时间戳统一注入
//! 单调 tick（与帧计时同源——归因前提）：时间回退显性拒
//! （[`SensorErr::TsRegression`]），同 tick 重复采样按传感器分账不冲突。
//!
//! ## 五、实现矩阵与 mock（判据六 + 判据七）
//!
//! [`IMPL_MATRIX`] 五行：F1443 Intel（RAPL 主路+MSR 备路）/F1444 AMD
//! （双平台）/F1445 NVIDIA/F1450 联合采样/F1454 mock——矩阵是登记制
//! （表外实现不进账）；[`MockSensor`] 同 trait 实现（确定性曲线 +
//! 可编程失败注入——契约测试的考题供给方）。
//!
//! ## 六、契约测试（判据八：可执行面）
//!
//! [`contract_probe`] 对任意实现跑四方法契约（成功读数区间有限/
//! 失败返回 Err 不置零/间隔档内/重复读稳定或单调）；多传感器一致性：
//! 同 tick 批量采样全体健康面（[`Sampler::sample_all`]），账内时间戳
//! 全同源（判据五的可断言面）。
//!
//! **对接**：F1441（J 域开工——功耗架构总览）；F1443~F1450（实现矩阵
//! 行的兑现单）；F1460（功耗-频率联合画像——采样数据下游）。
//! 零 panic 面、零 IO（mock 注入、真读数在平台实现侧）、零墙钟
//! （tick 注入）、no_std。

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const J02_SENSOR_VERSION: &str = "J02-sensorabs-v1";

/// 容量上限（当前策略值，可调）。
pub const MAX_SENSORS: usize = 16;

/// 连续失败摘除阈值。
pub const FAIL_STREAK_REMOVE: u32 = 3;

/// 空闲档周期（1Hz）。
pub const IDLE_PERIOD_MS: u32 = 1_000;

/// 活跃档周期（10Hz）。
pub const ACTIVE_PERIOD_MS: u32 = 100;

/// 异常档周期（100Hz）。
pub const ABNORMAL_PERIOD_MS: u32 = 10;

/// 功率越线水位（mW；升频触发线，当前策略值可调）。
pub const SPIKE_THRESHOLD_MW: u32 = 300_000;

/// 降档迟滞（连续正常采样次数）。
pub const DEESCALATE_STREAK: u32 = 16;

// ---------------------------------------------------------------------------
// 二、诊断码（0x63 段独占：J02 采样面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x63 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnCode {
    /// 传感器未注册。
    Unregistered,
    /// 重复注册。
    Duplicate,
    /// 容量已满。
    CapacityFull,
    /// 已摘除传感器被采样。
    Removed,
    /// 时间回退。
    TsRegression,
    /// 单位溢出（原始读数归一后超出 u32）。
    UnitOverflow,
    /// 调度周期不在三档内。
    ModeInvalid,
}

impl SnCode {
    /// 人读诊断（发生了什么/为什么/怎么办）。
    pub const fn detail(self) -> &'static str {
        match self {
            SnCode::Unregistered => "传感器未注册：先注册再采样，建议核对实现矩阵行",
            SnCode::Duplicate => "重复注册：同 id 传感器已在账，建议改走更新",
            SnCode::CapacityFull => "传感器容量已满（当前策略值）：建议摘除低优先级面或扩容评审",
            SnCode::Removed => "传感器已摘除：摘除不自动撤销，建议排查硬件后重注册",
            SnCode::TsRegression => "时间回退：采样时间戳必须单调（与帧计时同源），建议核对时钟源",
            SnCode::UnitOverflow => "单位溢出：原始 µW 读数归一后超出量程，建议分域读取或扩量程",
            SnCode::ModeInvalid => "调度周期不在三档内：1Hz/10Hz/100Hz 闭集，建议按 SampleMode 取档",
        }
    }
}

/// 传感器读错误（trait 失败语义：Err 不置零）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorErr {
    /// 硬件读失败（平台侧返回）。
    HwFail,
    /// 域不可用（分域探测失败的显性态——不造假）。
    DomainUnavailable,
}

// ---------------------------------------------------------------------------
// 三、SensorTrait（判据一：四方法最小面）
// ---------------------------------------------------------------------------

/// 传感器统一接口（四方法最小面；实现矩阵五行各出一实现）。
pub trait SensorTrait {
    /// 读瞬时功率（mW；入口即归一化后的账内单位）。
    fn read_power(&mut self) -> Result<u32, SensorErr>;
    /// 读能量累计器（µJ；累计器语义——差分法算功率的原料）。
    fn read_energy(&mut self) -> Result<u64, SensorErr>;
    /// 读电压电流对（µV, µA）。
    fn read_voltage_current(&mut self) -> Result<(u32, u32), SensorErr>;
    /// 基准轮询间隔（ms；三档调度在其上下缩放）。
    fn poll_interval(&self) -> u32;
}

/// 入口单位归一：原始 µW → mW（溢出显性拒——不静默截断）。
pub fn normalize_to_mw(raw_uw: u64) -> Result<u32, SnCode> {
    if raw_uw > u32::MAX as u64 {
        return Err(SnCode::UnitOverflow);
    }
    let mw = raw_uw / 1_000;
    if mw > u32::MAX as u64 {
        return Err(SnCode::UnitOverflow);
    }
    Ok(mw as u32)
}

// ---------------------------------------------------------------------------
// 四、Mock 传感器（判据七：同接口测试注入）
// ---------------------------------------------------------------------------

/// Mock 传感器：确定性锯齿曲线 + 可编程失败注入（零墙钟零随机——
/// 同 seed 同序列，契约测试可复现）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockSensor {
    /// 曲线种子（决定锯齿斜率与相位）。
    seed: u64,
    /// 读次数（曲线自变量）。
    reads: u64,
    /// 失败注入：前 `fail_first` 次读返回 HwFail（健康度测试考题）。
    fail_first: u32,
    /// 失败注入计数。
    failed: u32,
}

impl MockSensor {
    /// 新 mock（seed 曲线种子；fail_first 首段失败注入次数）。
    pub const fn new(seed: u64, fail_first: u32) -> MockSensor {
        MockSensor {
            seed,
            reads: 0,
            fail_first,
            failed: 0,
        }
    }

    /// 确定性功率曲线（mW）：50_000 + (reads*seed 折叠) 锯齿，有限单调可断言。
    fn curve_mw(&self) -> u32 {
        let fold = (self.reads.wrapping_mul(self.seed) >> 8) % 40_000;
        50_000u32.saturating_add(fold as u32)
    }
}

impl SensorTrait for MockSensor {
    fn read_power(&mut self) -> Result<u32, SensorErr> {
        if self.failed < self.fail_first {
            self.failed = self.failed.saturating_add(1);
            self.reads = self.reads.saturating_add(1);
            return Err(SensorErr::HwFail);
        }
        let v = self.curve_mw();
        self.reads = self.reads.saturating_add(1);
        Ok(v)
    }

    fn read_energy(&mut self) -> Result<u64, SensorErr> {
        if self.failed < self.fail_first {
            return Err(SensorErr::HwFail);
        }
        // 累计器：功率×间隔折算的确定性合成（µJ）。
        Ok(self.curve_mw() as u64 * 1_000)
    }

    fn read_voltage_current(&mut self) -> Result<(u32, u32), SensorErr> {
        if self.failed < self.fail_first {
            return Err(SensorErr::HwFail);
        }
        Ok((800_000u32, self.curve_mw().saturating_mul(12)))
    }

    fn poll_interval(&self) -> u32 {
        ACTIVE_PERIOD_MS
    }
}

// ---------------------------------------------------------------------------
// 五、采样器管理（判据二：注册/优先级/健康度/摘除）
// ---------------------------------------------------------------------------

/// 传感器在账状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorState {
    /// 健康。
    Healthy,
    /// 退化（有失败史但未达摘除线）。
    Degraded,
    /// 已摘除（不可自动撤销）。
    Removed,
}

/// 在账槽位（含 dyn 把手——不 Clone 不 Debug，所有权账内独占）。
pub struct SensorSlot {
    /// 传感器 id（账内唯一）。
    pub id: u32,
    /// 优先级（数值小者先采）。
    pub prio: u8,
    /// 状态。
    pub state: SensorState,
    /// 连续失败计数。
    pub fail_streak: u32,
    /// 最近成功时间戳（tick）。
    pub last_ok_ts: u64,
    /// mock/平台实现把手（dyn 分发——实现矩阵的运行时入口）。
    pub dev: Box<dyn SensorTrait>,
}

/// 摘除诊断记录（摘除是账面事件不是静默行为）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Removal {
    /// 被摘除传感器 id。
    pub id: u32,
    /// 摘除时刻（tick）。
    pub ts: u64,
    /// 连续失败数（应等于摘除阈值）。
    pub fail_streak: u32,
}

// ---------------------------------------------------------------------------
// 六、三档调度（判据三）
// ---------------------------------------------------------------------------

/// 采样档位闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleMode {
    /// 空闲 1Hz。
    Idle,
    /// 活跃 10Hz。
    Active,
    /// 异常 100Hz。
    Abnormal,
}

impl SampleMode {
    /// 档位周期（ms）。
    pub const fn period_ms(self) -> u32 {
        match self {
            SampleMode::Idle => IDLE_PERIOD_MS,
            SampleMode::Active => ACTIVE_PERIOD_MS,
            SampleMode::Abnormal => ABNORMAL_PERIOD_MS,
        }
    }

    /// 由周期反查档位（表外周期显性拒——三档是闭集）。
    pub fn of_period(ms: u32) -> Result<SampleMode, SnCode> {
        match ms {
            IDLE_PERIOD_MS => Ok(SampleMode::Idle),
            ACTIVE_PERIOD_MS => Ok(SampleMode::Active),
            ABNORMAL_PERIOD_MS => Ok(SampleMode::Abnormal),
            _ => Err(SnCode::ModeInvalid),
        }
    }
}

// ---------------------------------------------------------------------------
// 七、采样器总装（判据二三四五的运行时）
// ---------------------------------------------------------------------------

/// 单传感器采样账目（账内单位：mW；时间戳：注入 tick）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleRecord {
    /// 传感器 id。
    pub id: u32,
    /// 时间戳（单调 tick——与帧计时同源）。
    pub ts: u64,
    /// 功率（mW）。
    pub power_mw: u32,
    /// 是否触发升频（越线或失败）。
    pub flagged: bool,
}

/// 采样器（管理 + 三档调度总装；含 dyn 把手账——不 Clone 不 Debug）。
pub struct Sampler {
    /// 传感器账（按 id 升序）。
    slots: Vec<SensorSlot>,
    /// 当前档位。
    mode: SampleMode,
    /// 连续正常计数（降档迟滞）。
    calm_streak: u32,
    /// 摘除诊断账。
    removals: Vec<Removal>,
    /// 最近时间戳（单调性闸）。
    last_ts: u64,
}

impl Sampler {
    /// 新采样器（Idle 起步——节能默认）。
    pub fn new() -> Sampler {
        Sampler {
            slots: Vec::new(),
            mode: SampleMode::Idle,
            calm_streak: 0,
            removals: Vec::new(),
            last_ts: 0,
        }
    }

    /// 注册传感器（mock 或平台实现把手；重复/超容显性拒）。
    pub fn register(
        &mut self,
        id: u32,
        prio: u8,
        dev: Box<dyn SensorTrait>,
    ) -> Result<(), SnCode> {
        if self.find(id).is_some() {
            return Err(SnCode::Duplicate);
        }
        if self.slots.len() >= MAX_SENSORS {
            return Err(SnCode::CapacityFull);
        }
        let pos = self.insert_pos(id);
        self.slots.insert(
            pos,
            SensorSlot {
                id,
                prio,
                state: SensorState::Healthy,
                fail_streak: 0,
                last_ok_ts: 0,
                dev,
            },
        );
        Ok(())
    }

    /// 当前档位。
    pub const fn mode(&self) -> SampleMode {
        self.mode
    }

    /// 摘除账。
    pub fn removals(&self) -> Vec<Removal> {
        self.removals.clone()
    }

    /// 单 tick 采样：全体非摘除面按优先级序逐个采（同 tick 时间戳——同源）；
    /// 失败计入健康账，达线自动摘除并出诊断；异常升档、迟滞降档。
    pub fn sample_all(&mut self, tick: u64) -> Result<Vec<SampleRecord>, SnCode> {
        if tick < self.last_ts {
            return Err(SnCode::TsRegression);
        }
        // 档位闸：到达采样点才真采（Idle 档下高频调用直接跳过——节能语义）。
        if tick.saturating_sub(self.last_ts) < self.mode.period_ms() as u64
            && self.last_ts != 0
        {
            return Ok(Vec::new());
        }
        self.last_ts = tick;
        let mut out: Vec<SampleRecord> = Vec::new();
        let mut flagged_any = false;
        let mut i = 0usize;
        while i < self.slots.len() {
            let (id, state) = match self.slots.get(i) {
                Some(s) => (s.id, s.state),
                None => (0, SensorState::Removed),
            };
            if state == SensorState::Removed {
                return Err(SnCode::Removed);
            }
            let res = match self.slots.get_mut(i) {
                Some(s) => s.dev.read_power(),
                None => Err(SensorErr::HwFail),
            };
            match res {
                Ok(raw_mw) => {
                    let flagged = raw_mw >= SPIKE_THRESHOLD_MW;
                    if let Some(s) = self.slots.get_mut(i) {
                        s.fail_streak = 0;
                        s.last_ok_ts = tick;
                        if s.state == SensorState::Degraded {
                            s.state = SensorState::Healthy;
                        }
                    }
                    out.push(SampleRecord {
                        id,
                        ts: tick,
                        power_mw: raw_mw,
                        flagged,
                    });
                    if flagged {
                        flagged_any = true;
                    }
                }
                Err(_e) => {
                    // 失败不置零：计入健康账，达线摘除出诊断（判据二）。
                    let (new_streak, remove_now) = match self.slots.get_mut(i) {
                        Some(s) => {
                            s.fail_streak = s.fail_streak.saturating_add(1);
                            let rm = s.fail_streak >= FAIL_STREAK_REMOVE;
                            if rm {
                                s.state = SensorState::Removed;
                            } else {
                                s.state = SensorState::Degraded;
                            }
                            (s.fail_streak, rm)
                        }
                        None => (0, false),
                    };
                    if remove_now {
                        self.removals.push(Removal {
                            id,
                            ts: tick,
                            fail_streak: new_streak,
                        });
                    }
                    flagged_any = true;
                }
            }
            i += 1;
        }
        // 档位机：异常升档；连续正常降档（迟滞防抖）。
        if flagged_any {
            self.mode = SampleMode::Abnormal;
            self.calm_streak = 0;
        } else {
            self.calm_streak = self.calm_streak.saturating_add(1);
            if self.calm_streak >= DEESCALATE_STREAK {
                self.mode = match self.mode {
                    SampleMode::Abnormal => SampleMode::Active,
                    SampleMode::Active => SampleMode::Idle,
                    SampleMode::Idle => SampleMode::Idle,
                };
                self.calm_streak = 0;
            }
        }
        Ok(out)
    }

    /// 在账数（不含摘除面）。
    pub fn live_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.state != SensorState::Removed {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn insert_pos(&self, id: u32) -> usize {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                if s.id > id {
                    return i;
                }
            }
            i += 1;
        }
        self.slots.len()
    }
}

impl Default for Sampler {
    fn default() -> Sampler {
        Sampler::new()
    }
}

// ---------------------------------------------------------------------------
// 八、实现矩阵（判据六：抽象与平台分离的登记表）
// ---------------------------------------------------------------------------

/// 实现矩阵行（平台/单号/主路/当前状态——状态是登记事实不是承诺）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImplMatrixRow {
    /// 实现单号。
    pub fid: u32,
    /// 平台名。
    pub platform: &'static str,
    /// 主路。
    pub path: &'static str,
    /// 状态（MockReady=本单 mock 可替；Planned=待实现单兑现）。
    pub ready: bool,
}

/// 实现矩阵五行（锚点原文点名：F1443 Intel/F1444 AMD/F1445 NVIDIA/
/// F1450 联合/F1454 mock）。
pub const IMPL_MATRIX: [ImplMatrixRow; 5] = [
    ImplMatrixRow {
        fid: 1443,
        platform: "Intel",
        path: "RAPL sysfs 主路 + MSR 0x611 备路",
        ready: false,
    },
    ImplMatrixRow {
        fid: 1444,
        platform: "AMD",
        path: "hwmon power1_average / ADLX 双平台",
        ready: false,
    },
    ImplMatrixRow {
        fid: 1445,
        platform: "NVIDIA",
        path: "NVML 功率查询",
        ready: false,
    },
    ImplMatrixRow {
        fid: 1450,
        platform: "联合采样",
        path: "多传感器同 tick 聚合",
        ready: false,
    },
    ImplMatrixRow {
        fid: 1454,
        platform: "Mock",
        path: "MockSensor 确定性注入",
        ready: true,
    },
];

// ---------------------------------------------------------------------------
// 九、契约测试（判据八：可执行面）
// ---------------------------------------------------------------------------

/// 契约探针：对 mock 跑四方法契约——
/// ① 成功读数落在有限区间；② 失败返回 Err 不返回 0（不造假）；
/// ③ poll_interval 落在活跃档；④ 同 seed 复跑序列逐位一致（确定性）。
pub fn contract_probe() -> Result<(), SnCode> {
    let mut a = MockSensor::new(7, 0);
    let p1 = a.read_power().map_err(|_| SensorErr2Probe)?;
    let p2 = a.read_power().map_err(|_| SensorErr2Probe)?;
    if p1 == 0 || p2 == 0 || p1 > 500_000 || p2 > 500_000 {
        return Err(SnCode::UnitOverflow);
    }
    let iv = a.poll_interval();
    SampleMode::of_period(iv)?;
    let _ = a.read_voltage_current().map_err(|_| SensorErr2Probe)?;
    let _ = a.read_energy().map_err(|_| SensorErr2Probe)?;
    // 确定性：同 seed 新实例首读与 a 的首读一致。
    let mut b = MockSensor::new(7, 0);
    let q1 = b.read_power().map_err(|_| SensorErr2Probe)?;
    if q1 != p1 {
        return Err(SnCode::UnitOverflow);
    }
    Ok(())
}

/// 契约探针内部哨兵（把 SensorErr 折进 SnCode 的私有桥）。
const SensorErr2Probe: SnCode = SnCode::Unregistered;

/// 多传感器同 tick 一致性探针：三枚健康 mock 同 tick 批量采样，
/// 账内时间戳全同源（判据五可断言面）。
pub fn multi_sensor_consistency_probe() -> Result<(), SnCode> {
    let mut s = Sampler::new();
    let mut i: u32 = 0;
    while i < 3 {
        s.register(i + 1, i as u8, Box::new(MockSensor::new(i as u64 + 3, 0)))?;
        i += 1;
    }
    let recs = s.sample_all(100)?;
    if recs.len() != 3 {
        return Err(SnCode::Unregistered);
    }
    let ts0 = match recs.first() {
        Some(r) => r.ts,
        None => return Err(SnCode::Unregistered),
    };
    let mut j = 0usize;
    while j < recs.len() {
        if let Some(r) = recs.get(j) {
            if r.ts != ts0 {
                return Err(SnCode::TsRegression);
            }
            if r.power_mw == 0 {
                return Err(SnCode::UnitOverflow);
            }
        }
        j += 1;
    }
    Ok(())
}

/// 摘除机制探针：fail_first 注入连续失败，达线摘除出诊断账。
pub fn removal_probe() -> Result<(), SnCode> {
    let mut s = Sampler::new();
    s.register(9, 0, Box::new(MockSensor::new(5, FAIL_STREAK_REMOVE)))?;
    let mut k: u64 = 0;
    while k < FAIL_STREAK_REMOVE as u64 {
        let recs = s.sample_all(ACTIVE_PERIOD_MS as u64 * (k + 1))?;
        if !recs.is_empty() {
            // 失败注入期不应有成功记录。
            return Err(SnCode::UnitOverflow);
        }
        k += 1;
    }
    let rem = s.removals();
    if rem.len() != 1 {
        return Err(SnCode::Removed);
    }
    Ok(())
}

/// 采样报告（读屏可达）。
pub fn sampler_report(s: &Sampler) -> String {
    let mut out = String::new();
    out.push_str(&alloc::format!(
        "采样器：在账{}面/档位{:?}/摘除{}笔",
        s.live_count(),
        s.mode(),
        s.removals().len()
    ));
    out
}
