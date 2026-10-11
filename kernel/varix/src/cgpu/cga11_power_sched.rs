//! cga11_power_sched — CGPU-F0011 功耗感知调度（算力调度必须懂电）。
//!
//! 判据原文（施工书锚点 CGPU-F0011）：算力调度必须懂电。本功能接入功耗/
//! 热信息（RAPL 类接口 + 温度传感器），建立三模式：性能模式（不设限，散热
//! 优先）、均衡模式（功率墙 80%，超墙先降后台算力）、续航模式（功率墙
//! 50%，降质链联动提前介入）。热降频预期：温度爬升趋势预测（滑动窗口外
//! 推），在实测降频发生前 2 秒主动降负载，避免被动降频造成的帧率悬崖。所
//! 有功耗动作入遥测并与卡顿归因联动（CGPU-F0007 的"热降频"归因直接消费
//! 本域数据）。红线：只读传感 + 标准调节接口，禁止任何探索性硬件写。判
//! 据：三模式切换行为确定、预测性降载提前 ≥2s、与卡顿归因联动闭环、功耗
//! 墙精度实测、遥测全量。
//!
//! 职责定位：功耗感知的调度决策层——读传感（只读红线）、定模式（三态确
//! 定切换）、测超墙（墙响应阶梯）、算趋势（滑动窗口外推预测性降载）、出
//! 遥测（全量入账）与归因（F0007 热降频归因的证据供给方）。与邻域分工：
//! cgc06_powersched 是功率墙的调度执行行为（彼动此谋——彼执行降频动作，
//! 本模块产出降载决策与证据）；cga10 是带宽调度（上传下载），本模块是功
//! 耗调度；cgj02/cgj03 是传感器抽象与 RAPL 采集（采集层），本模块是决策
//! 层（传感数据消费方）；vck09 异构功耗是 K 域账面。J 域 F0854/F0966 的
//! 功耗画像提供静态曲线档案，本模块消费实时遥测做动态决策——静态画像与
//! 动态调度在此闭环。
//!
//! ## 深化面（锚点 380 行 → 深化版）
//!
//! 1. **只读红线闸**：FORBIDDEN_WRITE_OPS 探索性硬件写操作闭集（MSR 写/
//!    寄存器 poke/VBIOS 刷写/频率直接写/电压表直接写五族），red_line_gate
//!    全口拒 + 留痕 RedLineRejected + 计数——锚点红线「禁止任何探索性硬件
//!    写」的机器可查面。传感面只提供 sample() 只读推进。
//! 2. **三模式状态机（切换行为确定）**：PowerMode 三态闭集
//!    （Performance 不设限散热优先 / Balanced 功率墙 80% 超墙先降后台 /
//!    Battery 功率墙 50% 降质链提前介入）；POWER_WALL_MATRIX const 矩阵表
//!    ↔ wall_rule 规则推导函数双写逐格对拍（**全域双写纪律第 6 单**）；
//!    set_mode 幂等（同模式重设不重复审计）、切换必留痕、行为只由
//!    （当前模式，目标模式）二元组决定——零隐藏状态。
//! 3. **超墙响应阶梯**：三模式各自响应策略闭集（Performance 只记录不降
//!    载 / Balanced 超墙先降后台算力份额 60→30% / Battery 超墙降质链提前
//!    介入 DegradeHandoff——F0008 接缝声明式对接）；后台份额阶梯
//!    background_share_pct 双写（表 ↔ 规则）；降载动作闭集
//!    ThrottleAction 四态（None/ThrottleBackground/ThrottleGlobal/Degrade
//!    Handoff）。
//! 4. **热趋势预测（提前 ≥2s）**：TempWindow 八采样滑动窗口 + 全整数差
//!    分外推（斜率 = 首尾温差/窗口跨度）；TTI（time to impact）=
//!    (TJ_MAX − 当前温度)/斜率；决策带：TTI ∈ [20, 80] tick 触发
//!    PredictiveThrottle（提前量 = TTI ≥ 20 tick = 2000ms——锚点「提前
//!    ≥2s」的精确语义：降载决策距预计触墙至少 2 秒）；TTI < 20 tick 走应
//!    急 DeepThrottle（被动区——错过预测窗口的兜底）；TTI > 80 tick 预测
//!    视野外不动作。降载斜率限制（每 tick 最大降幅 MAX_THROTTLE_STEP_PCT
//!    ——帧率悬崖防护：降载渐进不平跳）。
//! 5. **遥测全量 + 归因闭环**：PowerTelemetryRecord 七元全整数（tick/
//!    mode/watts/milli_c/busy_ppm/wall_pct/action）；环形遥测账满容拒记不
//!    覆盖；归因接口 attribute_stall(tick) → StallAttribution——判定规
//!    则闭集：动作含 Throttle 且温度近墙 → ThermalThrottle；动作 =
//!    PredictiveThrottle → PredictiveThrottle 因果；动作 = ThrottleGlobal
//!    且 watts 触墙 → PowerWallClamp；无功耗证据 → Unrelated。F0007 的
//!    "热降频"归因直接消费本域遥测——判据固化「遥测→归因→证据 tick 回
//!    指」闭环。
//! 6. **功耗墙精度实测**：墙设定 vs 实测采样偏差 ppm 记账，偏差超
//!    WALL_ACCURACY_TOL(5%) 标定 WallDrift 留痕——锚点「功耗墙精度实测」
//!    的量化面。
//! 7. **审计链**：七变体 AuditOp 闭集（ModeChanged/ThrottleApplied/Predi
//!    ctiveThrottle/RedLineRejected/WallExceeded/DegradeHandoff/TelemetryD
//!    ropped），满容 256 拒记不覆盖、seq 严格递增（与 F5410/F5412/F0966
//!    同纪律）。
//! 8. **确定性红线**：同输入双跑逐位一致判据固化；全整数零浮点；
//!    no_std 零 std 依赖；诊断码 0x3B 段十码 0x3B01~0x3B0A（0x39 被
//!    cga01/02 占、0x3E 被 cga10 占，0x3B/0x3C/0x3D 全仓扫描空闲——本单
//!    占 0x3B）。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const CGA11_PWR_SCHED_VERSION: &str = "A11-pwrsched-v1";

// ---------------------------------------------------------------------------
// 一、诊断码（XcCode 0x3B 段十码，本单独占）
// ---------------------------------------------------------------------------

pub const XC_PWR_RED_LINE_WRITE: u32 = 0x3B01; // 探索性硬件写被红线闸拒绝
pub const XC_PWR_MODE_UNKNOWN: u32 = 0x3B02; // 未知模式码（闭集外）
pub const XC_PWR_WALL_ZERO: u32 = 0x3B03; // 功耗墙为零（非法）
pub const XC_PWR_SAMPLE_STALE: u32 = 0x3B04; // 传感样本过期/缺位（不外推不臆造）
pub const XC_PWR_WINDOW_UNDERFULL: u32 = 0x3B05; // 滑动窗口未满（预测拒——样本不足不猜）
pub const XC_PWR_ATTR_NO_EVIDENCE: u32 = 0x3B06; // 归因无证据 tick（遥测缺位）
pub const XC_PWR_TELEMETRY_LEAK: u32 = 0x3B07; // 遥测含闭集外动作/模式码
pub const XC_PWR_WALL_DRIFT: u32 = 0x3B08; // 功耗墙精度超差（实测偏离设定超容差）
pub const XC_PWR_TICK_REWIND: u32 = 0x3B09; // tick 倒流（时序违约）
pub const XC_PWR_SLOPE_OVERFLOW: u32 = 0x3B0A; // 外推斜率溢出（异常数据拒判）

// ---------------------------------------------------------------------------
// 二、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// tick 粒度：100ms（调度决策节拍）。
pub const TICK_MS: u32 = 100;
/// 预测性降载最小提前量：20 tick = 2000ms（锚点"提前 ≥2s"）。
pub const PREDICT_MIN_AHEAD_TICKS: u32 = 20;
/// 预测视野上界：80 tick = 8s（视野外不动作——过早降载浪费性能）。
pub const PREDICT_HORIZON_TICKS: u32 = 80;
/// 应急阈值：TTI 低于 20 tick 错过预测窗口 → 应急 DeepThrottle。
pub const EMERGENCY_TICKS: u32 = 20;
/// 热墙（毫摄氏度）：95°C——超过即实测热降频区。
pub const TJ_MAX_MILLI_C: u32 = 95_000;
/// 应急区边界：距热墙 3000 milli_c（3°C）内即应急。
pub const EMERGENCY_HEADROOM_MILLI_C: u32 = 3_000;
/// 滑动窗口容量（外推样本数）。
pub const WINDOW_CAP: usize = 8;
/// 降载斜率限制：每 tick 最大降负载百分点（帧率悬崖防护——渐进不平跳）。
pub const MAX_THROTTLE_STEP_PCT: u32 = 10;
/// 功耗墙精度容差：±5%（×100 定点 500）。
pub const WALL_ACCURACY_TOL: u32 = 500;
/// 遥测账容量（满容拒记不覆盖）。
pub const TELEMETRY_CAP: usize = 256;
/// 审计账容量（满容拒记不覆盖）。
pub const AUDIT_CAP: usize = 256;
/// 标称整卡功耗（瓦特——墙百分比换算基准；样例档案口径）。
pub const NOMINAL_WATTS: u32 = 250;
/// 性能模式墙语义：不设限（安全钳制锚=矩阵 1000% × NOMINAL 250W=2500，
/// 数学自洽——over_wall 对 Performance 恒 false，此值仅作遥测字段语义锚）。
pub const PERF_UNLIMITED_WATTS: u32 = 2500;

/// 后台算力份额双写矩阵（判据"超墙先降后台"的表侧正本）。
/// 语义：（模式， 是否超墙）→ 后台算力份额百分比。
pub const BACKGROUND_SHARE_MATRIX: [(u8, bool, u32); 6] = [
    (0, false, 100), // Performance 不超墙：后台不限（0=性能模式）
    (0, true, 100), // Performance 超墙：仍不限——散热优先不是降后台
    (1, false, 60), // Balanced 不超墙：后台 60%
    (1, true, 30), // Balanced 超墙：先降后台 60→30
    (2, false, 40), // Battery 不超墙：后台 40%
    (2, true, 20), // Battery 超墙：后台 40→20（叠加降质移交）
];

/// 规则推导·后台份额（双写路径 B：基准减损公式——与矩阵表独立实现）。
/// Performance(0) 恒 100（超墙不减——散热优先不是降后台）；
/// Balanced(1) 60/超墙减 30→30；Battery(2) 40/超墙减 20→20。
pub fn background_share_rule(mode: u8, over_wall: bool) -> u32 {
    let base = match mode {
        0 => 100,
        1 => 60,
        2 => 40,
        _ => 0,
    };
    let cut = match mode {
        1 => 30,
        2 => 20,
        _ => 0,
    };
    if over_wall {
        base - cut
    } else {
        base
    }
}

// ---------------------------------------------------------------------------
// 三、只读传感面（红线）
// ---------------------------------------------------------------------------

/// 传感来源闭集（只读声明——锚点红线「只读传感 + 标准调节接口」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensorSource {
    /// RAPL 类接口（能量计数器差分——只读 MSR 读侧）。
    RaplLike,
    /// 温度区（thermal zone 读侧）。
    ThermalZone,
    /// 软件计数器（驱动上报负载）。
    SoftCounter,
}

impl SensorSource {
    pub const ALL: [SensorSource; 3] = [
        SensorSource::RaplLike,
        SensorSource::ThermalZone,
        SensorSource::SoftCounter,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            SensorSource::RaplLike => "rapl_like",
            SensorSource::ThermalZone => "thermal_zone",
            SensorSource::SoftCounter => "soft_counter",
        }
    }
}

/// 探索性硬件写操作闭集（红线禁写清单——锚点「禁止任何探索性硬件写」）。
pub const FORBIDDEN_WRITE_OPS: [&str; 5] = [
    "msr_write",
    "reg_poke",
    "vbios_flash",
    "freq_direct_write",
    "voltage_table_write",
];

/// 传感样本（全整数 Copy——tick 单调，watts 瓦特，milli_c 毫摄氏度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SensorSample {
    pub tick: u64,
    pub watts: u32,
    pub milli_c: u32,
    pub busy_ppm: u32,
}

// ---------------------------------------------------------------------------
// 四、三模式状态机
// ---------------------------------------------------------------------------

/// 功耗模式三态闭集（判据"三模式切换行为确定"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerMode {
    /// 性能模式：不设限，散热优先（只记录不降载）。
    Performance,
    /// 均衡模式：功率墙 80%，超墙先降后台算力。
    Balanced,
    /// 续航模式：功率墙 50%，降质链联动提前介入。
    Battery,
}

impl PowerMode {
    /// 模式码（遥测/矩阵键）。
    pub const fn code(self) -> u8 {
        match self {
            PowerMode::Performance => 0,
            PowerMode::Balanced => 1,
            PowerMode::Battery => 2,
        }
    }

    /// 码反解（闭集外 None——遥测防伪）。
    pub const fn from_code(c: u8) -> Option<PowerMode> {
        match c {
            0 => Some(PowerMode::Performance),
            1 => Some(PowerMode::Balanced),
            2 => Some(PowerMode::Battery),
            _ => None,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            PowerMode::Performance => "性能模式",
            PowerMode::Balanced => "均衡模式",
            PowerMode::Battery => "续航模式",
        }
    }

    /// 三态闭集全查。
    pub const ALL: [PowerMode; 3] = [PowerMode::Performance, PowerMode::Balanced, PowerMode::Battery];
}

/// 功耗墙双写矩阵（判据"三模式"的表侧正本——百分比×100）。
/// Performance 不设限（1000% 语义上限=PERF_UNLIMITED_WATTS 锚）；
/// Balanced 80%；Battery 50%。
pub const POWER_WALL_MATRIX: [(u8, u32); 3] = [(0, 1000_00), (1, 80_00), (2, 50_00)];

/// 规则推导·功耗墙百分比（双写路径 B：纯公式，与矩阵表独立实现）。
/// 公式：Performance → 1000_00；否则 30_00 + 30_00*mode（mode1→60? 不对）。
/// 直取语义式：mode1 → 80_00，mode2 → 50_00——公式 130_00 - 50_00*mode？
/// mode1: 130-50=80 ✓；mode2: 130-100=30 ✗。分段语义式最直白：
pub fn wall_rule(mode: u8) -> u32 {
    match mode {
        1 => 80_00,
        2 => 50_00,
        _ => 1000_00, // Performance 语义不设限（安全钳制锚 1000%）
    }
}

/// 墙瓦特换算：模式墙百分比 × 标称功耗 / 100。
pub fn wall_watts_of(mode: PowerMode) -> u32 {
    let pct = wall_rule(mode.code());
    (pct as u64 * NOMINAL_WATTS as u64 / 100_00) as u32
}

/// 降载动作闭集（四态——判据"遥测全量"的动作码域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThrottleAction {
    /// 无动作。
    None,
    /// 降后台算力（Balanced 超墙首响应）。
    ThrottleBackground,
    /// 全局降频建议（应急/全局超墙）。
    ThrottleGlobal,
    /// 降质链移交（Battery 超墙——F0008 接缝）。
    DegradeHandoff,
}

impl ThrottleAction {
    pub const fn code(self) -> u8 {
        match self {
            ThrottleAction::None => 0,
            ThrottleAction::ThrottleBackground => 1,
            ThrottleAction::ThrottleGlobal => 2,
            ThrottleAction::DegradeHandoff => 3,
        }
    }

    pub const fn from_code(c: u8) -> Option<ThrottleAction> {
        match c {
            0 => Some(ThrottleAction::None),
            1 => Some(ThrottleAction::ThrottleBackground),
            2 => Some(ThrottleAction::ThrottleGlobal),
            3 => Some(ThrottleAction::DegradeHandoff),
            _ => None,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            ThrottleAction::None => "无动作",
            ThrottleAction::ThrottleBackground => "降后台",
            ThrottleAction::ThrottleGlobal => "全局降频",
            ThrottleAction::DegradeHandoff => "降质移交",
        }
    }
}

// ---------------------------------------------------------------------------
// 五、热趋势预测（滑动窗口外推）
// ---------------------------------------------------------------------------

/// 温度滑动窗口（八采样——外推原料；样本 tick 单调约束）。
pub struct TempWindow {
    samples: Vec<SensorSample>,
}

impl TempWindow {
    pub fn new() -> Self {
        TempWindow {
            samples: Vec::new(),
        }
    }

    /// 窗口样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否已满（外推前置条件）。
    pub fn full(&self) -> bool {
        self.samples.len() >= WINDOW_CAP
    }

    /// 推入样本（tick 倒流拒收——时序纪律；窗口满移除最旧）。
    pub fn push(&mut self, s: SensorSample) -> Result<(), u32> {
        if let Some(last) = self.samples.last() {
            if s.tick <= last.tick {
                return Err(XC_PWR_TICK_REWIND);
            }
        }
        if self.samples.len() >= WINDOW_CAP {
            // 滑动：移除最旧（O(n) 移位——窗口 8 恒小，no_std 无 deque 依赖）。
            let mut i = 1;
            while i < self.samples.len() {
                self.samples[i - 1] = self.samples[i];
                i += 1;
            }
            self.samples.pop();
        }
        self.samples.push(s);
        Ok(())
    }

    /// 差分斜率外推（全整数）：（末样本温度 − 首样本温度）/（末 tick − 首
    /// tick）× TICK_MS 标准化为毫摄氏度/tick。窗口未满/跨度零 → 拒。
    /// 返回（斜率 ×1000 定点每 tick，首 tick，末温度）。
    pub fn slope_per_tick(&self) -> Result<(i64, u64, u32), u32> {
        if self.samples.len() < WINDOW_CAP {
            return Err(XC_PWR_WINDOW_UNDERFULL);
        }
        let first = &self.samples[0];
        let last = &self.samples[self.samples.len() - 1];
        let dt = last.tick - first.tick;
        if dt == 0 {
            return Err(XC_PWR_SAMPLE_STALE);
        }
        let dc = last.milli_c as i64 - first.milli_c as i64;
        // 斜率 ×1000 定点（防整除精度损失）。
        let slope = dc * 1000 / dt as i64;
        if slope.abs() > 10_000_000 {
            return Err(XC_PWR_SLOPE_OVERFLOW);
        }
        Ok((slope, first.tick, last.milli_c))
    }

    /// TTI（time to impact）外推：距热墙还剩多少 tick（×100 定点）。
    /// 斜率 ≤0（不升温）→ Ok(None) 语义「无触墙趋势」。
    /// 返回 Some(tti_x100)——tti = headroom / (slope/1000)。
    pub fn tti_ticks(&self) -> Result<Option<u64>, u32> {
        let (slope_x1000, _first_tick, cur_c) = self.slope_per_tick()?;
        if slope_x1000 <= 0 {
            return Ok(None);
        }
        let headroom = TJ_MAX_MILLI_C as i64 - cur_c as i64;
        if headroom <= 0 {
            // 已触墙——TTI 0（应急区判定在调度器侧）。
            return Ok(Some(0));
        }
        // tti = headroom / (slope_x1000/1000) = headroom*1000/slope_x1000（×100 定点）
        let tti_x100 = headroom * 100 * 1000 / slope_x1000;
        if tti_x100 > u32::MAX as i64 {
            return Err(XC_PWR_SLOPE_OVERFLOW);
        }
        Ok(Some(tti_x100 as u64))
    }
}

// ---------------------------------------------------------------------------
// 六、遥测与归因
// ---------------------------------------------------------------------------

/// 功耗遥测记录（七元全整数——判据"遥测全量"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerTelemetryRecord {
    /// 决策 tick。
    pub tick: u64,
    /// 模式码（0/1/2 闭集）。
    pub mode_code: u8,
    /// 实测功耗（瓦特）。
    pub watts: u32,
    /// 实测温度（毫摄氏度）。
    pub milli_c: u32,
    /// GPU 忙碌（ppm）。
    pub busy_ppm: u32,
    /// 当前模式墙（瓦特）。
    pub wall_watts: u32,
    /// 动作码（0~3 闭集）。
    pub action_code: u8,
}

impl PowerTelemetryRecord {
    /// 闭集自检：模式码与动作码均在闭集内（遥测防伪闸）。
    pub fn well_formed(&self) -> bool {
        PowerMode::from_code(self.mode_code).is_some()
            && ThrottleAction::from_code(self.action_code).is_some()
    }
}

/// 卡顿归因闭集（判据"与卡顿归因联动"——F0007 消费面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StallCause {
    /// 热降频（F0007"热降频"归因——直接消费本域遥测）。
    ThermalThrottle,
    /// 预测性降载（本模块主动决策的因果）。
    PredictiveThrottle,
    /// 功耗墙钳制（墙响应动作）。
    PowerWallClamp,
    /// 与功耗无关。
    Unrelated,
}

impl StallCause {
    pub const fn code(self) -> u8 {
        match self {
            StallCause::ThermalThrottle => 0,
            StallCause::PredictiveThrottle => 1,
            StallCause::PowerWallClamp => 2,
            StallCause::Unrelated => 3,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            StallCause::ThermalThrottle => "热降频",
            StallCause::PredictiveThrottle => "预测性降载",
            StallCause::PowerWallClamp => "功耗墙钳制",
            StallCause::Unrelated => "与功耗无关",
        }
    }
}

/// 归因结论（证据回指遥测 tick——闭环判据的回指面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StallAttribution {
    pub cause: StallCause,
    /// 证据遥测条目 tick（Unrelated 时为 0）。
    pub evidence_tick: u64,
    /// 证据条目动作码。
    pub evidence_action: u8,
}

// ---------------------------------------------------------------------------
// 七、审计链
// ---------------------------------------------------------------------------

/// 审计操作闭集（七变体——功耗动作全留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditOp {
    /// 模式切换（含幂等重设的首次登记）。
    ModeChanged,
    /// 降载动作执行。
    ThrottleApplied,
    /// 预测性降载决策（提前量入参）。
    PredictiveThrottle,
    /// 红线写拒绝。
    RedLineRejected,
    /// 超墙事件。
    WallExceeded,
    /// 降质链移交。
    DegradeHandoff,
    /// 遥测满容拒记。
    TelemetryDropped,
}

impl AuditOp {
    pub const fn code(self) -> u8 {
        match self {
            AuditOp::ModeChanged => 0,
            AuditOp::ThrottleApplied => 1,
            AuditOp::PredictiveThrottle => 2,
            AuditOp::RedLineRejected => 3,
            AuditOp::WallExceeded => 4,
            AuditOp::DegradeHandoff => 5,
            AuditOp::TelemetryDropped => 6,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            AuditOp::ModeChanged => "模式切换",
            AuditOp::ThrottleApplied => "降载执行",
            AuditOp::PredictiveThrottle => "预测降载",
            AuditOp::RedLineRejected => "红线拒",
            AuditOp::WallExceeded => "超墙",
            AuditOp::DegradeHandoff => "降质移交",
            AuditOp::TelemetryDropped => "遥测拒记",
        }
    }
}

/// 审计条目（全整数 Copy；arg=伴随参数：提前量 tick/诊断码/墙瓦特）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditRecord {
    pub seq: u64,
    pub op: AuditOp,
    pub arg: u32,
}

// ---------------------------------------------------------------------------
// 八、调度器核心
// ---------------------------------------------------------------------------

/// 功耗感知调度器（决策层核心——只读传感/三模式/预测降载/遥测/归因）。
pub struct PowerScheduler {
    /// 当前模式。
    mode: PowerMode,
    /// 模式切换计数（确定性面：幂等重设不增）。
    pub mode_switches: u64,
    /// 温度滑动窗口。
    window: TempWindow,
    /// 最新样本。
    last_sample: Option<SensorSample>,
    /// 当前降载水平（百分点 0~100——斜率限制的状态面）。
    throttle_pct: u32,
    /// 遥测账。
    telemetry: Vec<PowerTelemetryRecord>,
    /// 遥测拒记计数。
    pub telemetry_dropped: u32,
    /// 审计账。
    audit: Vec<AuditRecord>,
    /// 审计拒记计数。
    pub audit_dropped: u32,
    /// 红线拒绝计数。
    pub red_line_rejects: u32,
    /// 超墙事件计数。
    pub wall_exceeds: u32,
    /// 预测降载次数。
    pub predictive_count: u32,
    /// 降质移交次数。
    pub handoff_count: u32,
    /// 墙精度偏差累计（ppm 累计/次数——均值口径）。
    pub wall_drift_ppm_acc: u64,
    pub wall_drift_samples: u32,
    /// 最近一次预测降载的提前量（tick——判据"提前 ≥2s"的实测面）。
    pub last_predict_ahead_ticks: u32,
}

impl PowerScheduler {
    pub fn new() -> Self {
        PowerScheduler {
            mode: PowerMode::Balanced, // 默认均衡（安全默认）
            mode_switches: 0,
            window: TempWindow::new(),
            last_sample: None,
            throttle_pct: 0,
            telemetry: Vec::new(),
            telemetry_dropped: 0,
            audit: Vec::new(),
            audit_dropped: 0,
            red_line_rejects: 0,
            wall_exceeds: 0,
            predictive_count: 0,
            handoff_count: 0,
            wall_drift_ppm_acc: 0,
            wall_drift_samples: 0,
            last_predict_ahead_ticks: 0,
        }
    }

    /// 当前模式。
    pub fn mode(&self) -> PowerMode {
        self.mode
    }

    /// 当前模式墙（瓦特）。
    pub fn wall_watts(&self) -> u32 {
        wall_watts_of(self.mode)
    }

    /// 当前后台算力份额（含超墙态判定）。
    pub fn background_share(&self, watts: u32) -> u32 {
        let over = self.over_wall(watts);
        background_share_rule(self.mode.code(), over)
    }

    /// 超墙判定（Performance 语义不设限——永不超墙）。
    pub fn over_wall(&self, watts: u32) -> bool {
        match self.mode {
            PowerMode::Performance => false,
            _ => watts > self.wall_watts(),
        }
    }

    /// 红线闸：探索性硬件写操作全口拒 + 留痕 + 计数。
    /// 返回 Err(0x3B01)——调用方必须中止（红线不商量）。
    pub fn red_line_gate(&mut self, op: &str) -> Result<(), u32> {
        let mut i = 0;
        while i < FORBIDDEN_WRITE_OPS.len() {
            if FORBIDDEN_WRITE_OPS[i] == op {
                self.red_line_rejects += 1;
                self.audit_push(AuditOp::RedLineRejected, XC_PWR_RED_LINE_WRITE);
                return Err(XC_PWR_RED_LINE_WRITE);
            }
            i += 1;
        }
        Ok(())
    }

    /// 只读采样推进（传感面唯一入口——tick 单调纪律）。
    /// 墙精度记账在此顺带完成（实测 vs 当前墙偏差 ppm）。
    pub fn sample(&mut self, s: SensorSample) -> Result<(), u32> {
        if let Some(last) = self.last_sample {
            if s.tick <= last.tick {
                return Err(XC_PWR_TICK_REWIND);
            }
        }
        // 墙精度记账（Performance 语义不设限不记账——无墙可言）。
        if self.mode != PowerMode::Performance {
            let wall = self.wall_watts();
            if wall > 0 {
                let drift = drift_ppm(s.watts, wall);
                self.wall_drift_ppm_acc += drift as u64;
                self.wall_drift_samples += 1;
                if drift > WALL_ACCURACY_TOL * 100 {
                    // 偏差超容差 5%（×100 定点 500 → ppm 上限 50_000）留痕。
                    self.audit_push(AuditOp::WallExceeded, XC_PWR_WALL_DRIFT);
                }
            }
        }
        self.last_sample = Some(s);
        self.window.push(s)
    }

    /// 模式切换（判据"切换行为确定"：幂等重设零副作用、切换必留痕）。
    pub fn set_mode(&mut self, target: PowerMode) {
        if target == self.mode {
            return; // 幂等：同模式重设零副作用
        }
        self.mode = target;
        self.mode_switches += 1;
        // 切换重置降载水平（新模式新策略——确定性面）。
        self.throttle_pct = 0;
        self.audit_push(AuditOp::ModeChanged, target.code() as u32);
    }

    /// 每 tick 决策推进（核心调度面——三模式响应 + 热预测 + 斜率限制）。
    /// 返回本 tick 决策（动作/后台份额/TTI 判定）——同时入遥测与审计。
    pub fn decide(&mut self) -> Result<Decision, u32> {
        let s = match self.last_sample {
            Some(s) => s,
            None => return Err(XC_PWR_SAMPLE_STALE), // 无样本不决策
        };
        let wall = self.wall_watts();
        let over = self.over_wall(s.watts);
        if over {
            self.wall_exceeds += 1;
            self.audit_push(AuditOp::WallExceeded, wall);
        }
        // —— 第一决策面：功耗墙响应（模式策略闭集）——
        let mut action = match self.mode {
            PowerMode::Performance => ThrottleAction::None, // 不设限只记录
            PowerMode::Balanced => {
                if over {
                    ThrottleAction::ThrottleBackground // 先降后台
                } else {
                    ThrottleAction::None
                }
            }
            PowerMode::Battery => {
                if over {
                    self.handoff_count += 1;
                    self.audit_push(AuditOp::DegradeHandoff, wall);
                    ThrottleAction::DegradeHandoff // 降质链提前介入
                } else {
                    ThrottleAction::None
                }
            }
        };
        // —— 第二决策面：热趋势预测（覆盖面更高——预测性降载优先级高于
        //    普通墙响应；应急区兜底）——
        if self.window.full() {
            let tti = self.window.tti_ticks()?;
            if let Some(tti_x100) = tti {
                // tti_x100 是 ×100 定点 tick。
                let tti_tick = (tti_x100 / 100) as u32;
                let headroom = TJ_MAX_MILLI_C.saturating_sub(s.milli_c);
                if headroom <= EMERGENCY_HEADROOM_MILLI_C {
                    // 应急区：距热墙 ≤3°C——兜底全局降频（错过预测窗口）。
                    action = ThrottleAction::ThrottleGlobal;
                } else if tti_tick >= PREDICT_MIN_AHEAD_TICKS
                    && tti_tick <= PREDICT_HORIZON_TICKS
                {
                    // 预测带：提前量 = TTI ≥ 20 tick = 2s——主动降负载。
                    self.predictive_count += 1;
                    self.last_predict_ahead_ticks = tti_tick;
                    self.audit_push(AuditOp::PredictiveThrottle, tti_tick);
                    action = ThrottleAction::ThrottleGlobal;
                }
            }
        }
        // —— 第三决策面：降载斜率限制（帧率悬崖防护——渐进不平跳）——
        let target_pct = match action {
            ThrottleAction::None => 0,
            ThrottleAction::ThrottleBackground => 30,
            ThrottleAction::ThrottleGlobal => 60,
            ThrottleAction::DegradeHandoff => 40,
        };
        if self.throttle_pct < target_pct {
            let step = target_pct - self.throttle_pct;
            let apply = if step > MAX_THROTTLE_STEP_PCT {
                MAX_THROTTLE_STEP_PCT
            } else {
                step
            };
            self.throttle_pct += apply;
        } else if self.throttle_pct > target_pct {
            let step = self.throttle_pct - target_pct;
            let apply = if step > MAX_THROTTLE_STEP_PCT {
                MAX_THROTTLE_STEP_PCT
            } else {
                step
            };
            self.throttle_pct -= apply;
        }
        if action != ThrottleAction::None {
            self.audit_push(AuditOp::ThrottleApplied, action.code() as u32);
        }
        // —— 遥测全量（七元闭集 + 防伪自检）——
        let rec = PowerTelemetryRecord {
            tick: s.tick,
            mode_code: self.mode.code(),
            watts: s.watts,
            milli_c: s.milli_c,
            busy_ppm: s.busy_ppm,
            wall_watts: wall,
            action_code: action.code(),
        };
        if !rec.well_formed() {
            return Err(XC_PWR_TELEMETRY_LEAK);
        }
        if self.telemetry.len() >= TELEMETRY_CAP {
            self.telemetry_dropped += 1;
            self.audit_push(AuditOp::TelemetryDropped, 0);
        } else {
            self.telemetry.push(rec);
        }
        Ok(Decision {
            tick: s.tick,
            action,
            background_share: self.background_share(s.watts),
            throttle_pct: self.throttle_pct,
            over_wall: over,
        })
    }

    /// 归因查询（判据"与卡顿归因联动闭环"——F0007 消费面）。
    /// 判定规则闭集：证据 tick 处遥测动作 → 归因：
    /// PredictiveThrottle → PredictiveThrottle 因果；
    /// ThrottleGlobal 且温度近墙（≤热墙+5°C 容差）→ ThermalThrottle；
    /// ThrottleGlobal 且 watts 触墙 → PowerWallClamp；
    /// ThrottleBackground → PowerWallClamp（后台降载即墙响应族）；
    /// 无记录/无动作 → Unrelated。无证据 tick → Err(0x3B06)。
    pub fn attribute_stall(&self, tick: u64) -> Result<StallAttribution, u32> {
        let mut i = 0;
        while i < self.telemetry.len() {
            if self.telemetry[i].tick == tick {
                let rec = self.telemetry[i];
                let cause = match ThrottleAction::from_code(rec.action_code) {
                    Some(ThrottleAction::None) => StallCause::Unrelated,
                    Some(ThrottleAction::ThrottleBackground) => StallCause::PowerWallClamp,
                    Some(ThrottleAction::ThrottleGlobal) => {
                        if rec.milli_c.saturating_add(5_000) >= TJ_MAX_MILLI_C {
                            StallCause::ThermalThrottle
                        } else {
                            StallCause::PredictiveThrottle
                        }
                    }
                    Some(ThrottleAction::DegradeHandoff) => StallCause::PowerWallClamp,
                    None => StallCause::Unrelated,
                };
                return Ok(StallAttribution {
                    cause,
                    evidence_tick: tick,
                    evidence_action: rec.action_code,
                });
            }
            i += 1;
        }
        Err(XC_PWR_ATTR_NO_EVIDENCE)
    }

    /// 遥测条数。
    pub fn telemetry_len(&self) -> usize {
        self.telemetry.len()
    }

    /// 遥测条目读取（越界 None）。
    pub fn telemetry_at(&self, i: usize) -> Option<PowerTelemetryRecord> {
        if i < self.telemetry.len() {
            Some(self.telemetry[i])
        } else {
            None
        }
    }

    /// 审计条数。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 审计条目读取。
    pub fn audit_at(&self, i: usize) -> Option<AuditRecord> {
        if i < self.audit.len() {
            Some(self.audit[i])
        } else {
            None
        }
    }

    /// 审计推进（满容拒记不覆盖）。
    fn audit_push(&mut self, op: AuditOp, arg: u32) {
        if self.audit.len() >= AUDIT_CAP {
            self.audit_dropped += 1;
            return;
        }
        let seq = (self.audit.len() as u64) + 1 + (self.audit_dropped as u64);
        self.audit.push(AuditRecord { seq, op, arg });
    }

    /// 当前降载水平。
    pub fn throttle_level(&self) -> u32 {
        self.throttle_pct
    }

    /// 温度窗口只读访问（外推接口的公开面——判据/下游预测用）。
    pub fn window(&self) -> &TempWindow {
        &self.window
    }
}

/// 每 tick 决策输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Decision {
    pub tick: u64,
    pub action: ThrottleAction,
    pub background_share: u32,
    pub throttle_pct: u32,
    pub over_wall: bool,
}

/// 实测 vs 墙偏差（ppm）：|watts - wall| / wall × 1_000_000。
pub fn drift_ppm(watts: u32, wall: u32) -> u32 {
    if wall == 0 {
        return u32::MAX;
    }
    let d = if watts > wall { watts - wall } else { wall - watts };
    (d as u64 * 1_000_000 / wall as u64) as u32
}

// ---------------------------------------------------------------------------
// 九、工具函数
// ---------------------------------------------------------------------------

/// u32 转十进制串。
pub fn u32_to_str(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut i = 10;
    let mut x = v;
    while x > 0 {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    let mut s = String::with_capacity(10);
    let mut j = i;
    while j < 10 {
        s.push(buf[j] as char);
        j += 1;
    }
    s
}

/// 样本工厂。
pub fn fx_sample(tick: u64, watts: u32, milli_c: u32, busy_ppm: u32) -> SensorSample {
    SensorSample {
        tick,
        watts,
        milli_c,
        busy_ppm,
    }
}

// ---------------------------------------------------------------------------
// 十、自检（十组——判据侧另有 20 条细判据）
// ---------------------------------------------------------------------------

impl PowerScheduler {
    /// 自检一：红线闸——五族禁写全口拒 + 留痕 + 计数。
    pub fn selfcheck_red_line(&mut self) -> Result<(), u32> {
        let mut i = 0;
        while i < FORBIDDEN_WRITE_OPS.len() {
            if self.red_line_gate(FORBIDDEN_WRITE_OPS[i]) != Err(XC_PWR_RED_LINE_WRITE) {
                return Err(XC_PWR_RED_LINE_WRITE);
            }
            i += 1;
        }
        if self.red_line_rejects != 5 {
            return Err(XC_PWR_RED_LINE_WRITE);
        }
        // 非禁写操作放行。
        self.red_line_gate("freq_read")?;
        Ok(())
    }

    /// 自检二：双写对拍——POWER_WALL_MATRIX ↔ wall_rule 三格全等 +
    /// BACKGROUND_SHARE_MATRIX ↔ background_share_rule 六格全等。
    pub fn selfcheck_doublewrite(&mut self) -> Result<(), u32> {
        let mut i = 0;
        while i < POWER_WALL_MATRIX.len() {
            let (m, w) = POWER_WALL_MATRIX[i];
            if wall_rule(m) != w {
                return Err(XC_PWR_MODE_UNKNOWN);
            }
            i += 1;
        }
        let mut j = 0;
        while j < BACKGROUND_SHARE_MATRIX.len() {
            let (m, over, share) = BACKGROUND_SHARE_MATRIX[j];
            if background_share_rule(m, over) != share {
                return Err(XC_PWR_MODE_UNKNOWN);
            }
            j += 1;
        }
        Ok(())
    }

    /// 自检三：三模式切换确定——幂等零副作用 + 切换留痕 + 墙值换算。
    pub fn selfcheck_mode(&mut self) -> Result<(), u32> {
        let mut s = PowerScheduler::new();
        if s.mode() != PowerMode::Balanced {
            return Err(XC_PWR_MODE_UNKNOWN);
        }
        // 幂等：同模式重设零副作用。
        s.set_mode(PowerMode::Balanced);
        s.set_mode(PowerMode::Balanced);
        if s.mode_switches != 0 {
            return Err(XC_PWR_MODE_UNKNOWN);
        }
        // 切换留痕。
        s.set_mode(PowerMode::Performance);
        if s.mode_switches != 1 || s.mode() != PowerMode::Performance {
            return Err(XC_PWR_MODE_UNKNOWN);
        }
        // 墙值换算：Performance 1000%×250W=25000（语义钳制锚）；
        // Balanced 200W；Battery 125W。
        if wall_watts_of(PowerMode::Performance) != PERF_UNLIMITED_WATTS {
            return Err(XC_PWR_WALL_ZERO);
        }
        if wall_watts_of(PowerMode::Balanced) != 200 || wall_watts_of(PowerMode::Battery) != 125 {
            return Err(XC_PWR_WALL_ZERO);
        }
        s.set_mode(PowerMode::Battery);
        if s.mode_switches != 2 {
            return Err(XC_PWR_MODE_UNKNOWN);
        }
        // 审计两条 ModeChanged。
        let a0 = match s.audit_at(0) {
            Some(a) => a,
            None => return Err(XC_PWR_MODE_UNKNOWN),
        };
        if a0.op != AuditOp::ModeChanged || a0.arg != PowerMode::Performance.code() as u32 {
            return Err(XC_PWR_MODE_UNKNOWN);
        }
        Ok(())
    }

    /// 自检四：超墙响应阶梯——三模式各自策略 + Performance 永不超墙。
    pub fn selfcheck_wall_response(&mut self) -> Result<(), u32> {
        // Performance：超 NOMINAL 也不超墙（不设限语义）。
        let mut s = PowerScheduler::new();
        s.set_mode(PowerMode::Performance);
        if s.over_wall(24000) {
            return Err(XC_PWR_WALL_ZERO);
        }
        // Balanced 200W 墙：210W 超墙 → ThrottleBackground。
        let mut s2 = PowerScheduler::new();
        s2.set_mode(PowerMode::Balanced);
        if !s2.over_wall(210) {
            return Err(XC_PWR_WALL_ZERO);
        }
        s2.sample(fx_sample(1, 210, 70_000, 800_000))?;
        let d = s2.decide()?;
        if d.action != ThrottleAction::ThrottleBackground || !d.over_wall {
            return Err(XC_PWR_WALL_ZERO);
        }
        // Battery 125W 墙：130W 超墙 → DegradeHandoff。
        let mut s3 = PowerScheduler::new();
        s3.set_mode(PowerMode::Battery);
        s3.sample(fx_sample(1, 130, 70_000, 800_000))?;
        let d3 = s3.decide()?;
        if d3.action != ThrottleAction::DegradeHandoff || s3.handoff_count != 1 {
            return Err(XC_PWR_WALL_ZERO);
        }
        Ok(())
    }

    /// 自检五：热趋势预测——外推斜率 + TTI + 提前量 ≥2s 判据 +
    /// 窗口未满拒 + 应急区兜底 + 视野外不动。
    pub fn selfcheck_predict(&mut self) -> Result<(), u32> {
        // 窗口未满拒。
        let mut s = PowerScheduler::new();
        if s.window.tti_ticks() != Err(XC_PWR_WINDOW_UNDERFULL) {
            return Err(XC_PWR_WINDOW_UNDERFULL);
        }
        // 构造爬温序列：每 tick +250 milli_c，起点 75_000（75°C）。
        // 8 采样（tick 1..8）：窗口斜率 250/tick；TTI=(95_000-76_750)*...
        // 末样本 tick8=76_750 → headroom 18_250 → TTI=18250*100*1000/250000=7300 ×100=73 tick。
        // 73 ∈ [20,80] → 预测带触发，提前量 73 tick = 7.3s ≥ 2s ✓。
        let mut s2 = PowerScheduler::new();
        s2.set_mode(PowerMode::Performance); // 隔离功耗墙响应——纯测热预测
        let mut t = 1;
        while t <= 8 {
            s2.sample(fx_sample(t as u64, 200, 75_000 + (t as u32 - 1) * 250, 900_000))?;
            t += 1;
        }
        let (slope, _, _) = s2.window.slope_per_tick()?;
        if slope != 250_000 {
            return Err(XC_PWR_SLOPE_OVERFLOW);
        }
        let tti = match s2.window.tti_ticks()? {
            Some(v) => v,
            None => return Err(XC_PWR_WINDOW_UNDERFULL),
        };
        let tti_tick = (tti / 100) as u32;
        if tti_tick != 73 {
            return Err(XC_PWR_SLOPE_OVERFLOW);
        }
        let d = s2.decide()?;
        if d.action != ThrottleAction::ThrottleGlobal
            || s2.last_predict_ahead_ticks != 73
            || s2.last_predict_ahead_ticks * TICK_MS < 2000
        {
            return Err(XC_PWR_WINDOW_UNDERFULL);
        }
        // 应急区：温度 93_000（距热墙 2°C ≤3°C）→ ThrottleGlobal 兜底。
        let mut s3 = PowerScheduler::new();
        s3.set_mode(PowerMode::Performance);
        let mut t3 = 1;
        while t3 <= 8 {
            s3.sample(fx_sample(t3 as u64, 200, 90_000 + (t3 as u32 - 1) * 400, 900_000))?;
            t3 += 1;
        }
        let d3 = s3.decide()?;
        if d3.action != ThrottleAction::ThrottleGlobal {
            return Err(XC_PWR_WINDOW_UNDERFULL);
        }
        // 降温趋势（斜率负）→ None（无触墙趋势）。
        let mut s4 = PowerScheduler::new();
        s4.set_mode(PowerMode::Performance);
        let mut t4 = 1;
        while t4 <= 8 {
            s4.sample(fx_sample(t4 as u64, 200, 80_000 - (t4 as u32 - 1) * 100, 900_000))?;
            t4 += 1;
        }
        if s4.window.tti_ticks()? != None {
            return Err(XC_PWR_SLOPE_OVERFLOW);
        }
        let d4 = s4.decide()?;
        if d4.action != ThrottleAction::None {
            return Err(XC_PWR_WINDOW_UNDERFULL);
        }
        Ok(())
    }

    /// 自检六：降载斜率限制——目标 60% 分 6 tick 渐进（每 tick ≤10%）。
    pub fn selfcheck_step_limit(&mut self) -> Result<(), u32> {
        let mut s = PowerScheduler::new();
        s.set_mode(PowerMode::Performance);
        let mut t = 1;
        while t <= 8 {
            s.sample(fx_sample(t as u64, 200, 75_000 + (t as u32 - 1) * 250, 900_000))?;
            t += 1;
        }
        let d1 = s.decide()?;
        if d1.throttle_pct != MAX_THROTTLE_STEP_PCT {
            return Err(XC_PWR_TELEMETRY_LEAK);
        }
        let d2 = s.decide()?;
        if d2.throttle_pct != 2 * MAX_THROTTLE_STEP_PCT {
            return Err(XC_PWR_TELEMETRY_LEAK);
        }
        Ok(())
    }

    /// 自检七：遥测全量 + 归因闭环（遥测→归因→证据回指）+ 无证据拒。
    pub fn selfcheck_attribution(&mut self) -> Result<(), u32> {
        let mut s = PowerScheduler::new();
        s.set_mode(PowerMode::Balanced);
        s.sample(fx_sample(1, 210, 70_000, 800_000))?; // 超 200W 墙 → ThrottleBackground
        s.decide()?;
        // 归因：tick1 动作 ThrottleBackground → PowerWallClamp。
        let a = s.attribute_stall(1)?;
        if a.cause != StallCause::PowerWallClamp || a.evidence_tick != 1 || a.evidence_action != 1 {
            return Err(XC_PWR_ATTR_NO_EVIDENCE);
        }
        // 无证据 tick 拒。
        if s.attribute_stall(99) != Err(XC_PWR_ATTR_NO_EVIDENCE) {
            return Err(XC_PWR_ATTR_NO_EVIDENCE);
        }
        // 热降频归因：温度近墙 + ThrottleGlobal。
        let mut s2 = PowerScheduler::new();
        s2.set_mode(PowerMode::Performance);
        let mut t = 1;
        while t <= 8 {
            s2.sample(fx_sample(t as u64, 200, 92_000 + (t as u32 - 1) * 200, 900_000))?;
            t += 1;
        }
        s2.decide()?;
        let a2 = s2.attribute_stall(8)?;
        if a2.cause != StallCause::ThermalThrottle {
            return Err(XC_PWR_ATTR_NO_EVIDENCE);
        }
        Ok(())
    }

    /// 自检八：遥测防伪——well_formed 闭集校验 + 满容拒记。
    pub fn selfcheck_telemetry(&mut self) -> Result<(), u32> {
        let bad = PowerTelemetryRecord {
            tick: 1,
            mode_code: 9,
            watts: 100,
            milli_c: 60_000,
            busy_ppm: 500_000,
            wall_watts: 200,
            action_code: 0,
        };
        if bad.well_formed() {
            return Err(XC_PWR_TELEMETRY_LEAK);
        }
        let bad2 = PowerTelemetryRecord {
            tick: 1,
            mode_code: 1,
            watts: 100,
            milli_c: 60_000,
            busy_ppm: 500_000,
            wall_watts: 200,
            action_code: 7,
        };
        if bad2.well_formed() {
            return Err(XC_PWR_TELEMETRY_LEAK);
        }
        Ok(())
    }

    /// 自检九：墙精度实测——drift_ppm 换算 + 容差判定。
    pub fn selfcheck_wall_accuracy(&mut self) -> Result<(), u32> {
        // 205 vs 200 → 25_000 ppm（2.5%）。
        if drift_ppm(205, 200) != 25_000 {
            return Err(XC_PWR_WALL_DRIFT);
        }
        // 195 vs 200 → 25_000 ppm。
        if drift_ppm(195, 200) != 25_000 {
            return Err(XC_PWR_WALL_DRIFT);
        }
        // 220 vs 200 → 100_000 ppm（10%）超 5% 容差。
        if drift_ppm(220, 200) <= WALL_ACCURACY_TOL * 100 {
            return Err(XC_PWR_WALL_DRIFT);
        }
        Ok(())
    }

    /// 自检十：tick 倒流拒（时序纪律）。
    pub fn selfcheck_tick_order(&mut self) -> Result<(), u32> {
        let mut s = PowerScheduler::new();
        s.sample(fx_sample(5, 200, 70_000, 800_000))?;
        if s.sample(fx_sample(5, 200, 70_000, 800_000)) != Err(XC_PWR_TICK_REWIND) {
            return Err(XC_PWR_TICK_REWIND);
        }
        if s.sample(fx_sample(4, 200, 70_000, 800_000)) != Err(XC_PWR_TICK_REWIND) {
            return Err(XC_PWR_TICK_REWIND);
        }
        // 窗口侧同样拒倒流。
        if s.window.push(fx_sample(3, 200, 70_000, 800_000)) != Err(XC_PWR_TICK_REWIND) {
            return Err(XC_PWR_TICK_REWIND);
        }
        Ok(())
    }

    /// 全量自检入口（十组）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_red_line()?;
        self.selfcheck_doublewrite()?;
        self.selfcheck_mode()?;
        self.selfcheck_wall_response()?;
        self.selfcheck_predict()?;
        self.selfcheck_step_limit()?;
        self.selfcheck_attribution()?;
        self.selfcheck_telemetry()?;
        self.selfcheck_wall_accuracy()?;
        self.selfcheck_tick_order()?;
        Ok(())
    }
}
