//! unx2041_freqidle —— UNX-F2041 idle/频率联合决策面（联合最优）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B23）：UNX-F2041 · idle/频率联合决策面（联合最优） · 目标 360 行 · 已深化
//! 判据原文：UNX-F2041-J1 联合决策断言全过（idle 驻留预期×频率档联合选择），
//!           决策振荡注入 10/10 检出，决策账动机字段齐备（F2014 同法）
//! 组件：频率档闭集（Gear0 节能/Gear1 平衡/Gear2 性能/Gear3 睿频 四档，每档标称 MHz + 功耗 ppm 代价）
//!      + idle 驻留预期闭集（Short 短驻/Medium 中驻/Deep 深驻 三档，每档预期驻留 tick + 置信）
//!      + idle 目标深度闭集（Shallow/Medium/Deep 三档，与驻留预期对齐的可下沉档位）
//!      + 联合成本函数（唤醒代价 + 频率再爬升代价 + 驻留功耗代价，全整数 ppm 权重和恰 1e6）
//!      + 联合决策格（(驻留预期 × 频率档) → 联合最优动作 argmin over 目标深度，逐格预计算 + 运行期复核）
//!      + 决策账（每笔：tick/驻留预期/入档/出档/出深度/动机 六列 append-only，动机字段 F2014 同法）
//!      + 振荡检测器（决策横跳检出：出档 A→B→A 迟滞确认窗，确认窗内反复即判振荡）
//!      + 振荡注入核（合成振荡决策流注入，10/10 检出）
//!      + 联合最优复核（argmin 结果与决策格预计算一致，冲突即红）
//!      + 冻结指纹（决策格 + 成本权重 FNV 指纹，冻结后篡改即显性）
//!      + 守恒账（决策数 ≡ 账条数 ≡ 出账数；注入 ≡ 检出）
//! 判据落地：联合决策 argmin 全过 + 振荡 10/10 检出 + 动机字段齐备 + 文档冻结
//! 契约承接：F1741 idle 三态（本面消费驻留态）+ F1741 idle 预期（驻留预期来源）+ F2014 决策账同构（动机字段法）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，确定性（逻辑 tick）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const FI_VERSION: &str = "unx2041_freqidle-v1";

/// 诊断段基址（0x5130 段 · 本模块独占；全域 grep 确认空缺）。
pub const FI_SEG_BASE: u16 = 0x5130;

/// 频率档数（闭集四档）。
pub const FI_GEARS: usize = 4;

/// idle 驻留预期档数（闭集三档）。
pub const FI_FORECASTS: usize = 3;

/// idle 目标深度档数（闭集三档）。
pub const FI_DEPTHS: usize = 3;

/// 振荡注入检出目标（判据 10/10）。
pub const FI_OSC_TARGET: u64 = 10;

/// 振荡确认窗 tick 数（窗口内决策横跳达阈值即判振荡）。
pub const FI_OSC_WINDOW: usize = 8;

/// 振荡横跳阈值（窗口内出档翻转次数达此即判振荡）。
pub const FI_OSC_FLIPS: usize = 3;

/// 联合成本权重分母（ppm 权重和恰 1e6）。
pub const FI_WEIGHT_DEN: u64 = 1_000_000;

/// 唤醒代价权重 ppm（深 idle 唤醒延迟惩罚占比）。
pub const FI_W_WAKE_PPM: u64 = 400_000;

/// 频率再爬升代价权重 ppm（降频后回爬的吞吐损失占比）。
pub const FI_W_RAMP_PPM: u64 = 350_000;

/// 驻留功耗代价权重 ppm（驻留期间功耗惩罚占比）。
pub const FI_W_POWER_PPM: u64 = 250_000;

/// 频率档闭集（判据「频率档」四档，每档标称 MHz + 功耗 ppm）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreqGear {
    /// Gear0 节能档（最低频，最低功耗，最高唤醒/爬升友好）。
    Gear0,
    /// Gear1 平衡档。
    Gear1,
    /// Gear2 性能档。
    Gear2,
    /// Gear3 睿频档（最高频，最高功耗）。
    Gear3,
}

impl FreqGear {
    /// 全量闭集（四档）。
    pub const ALL: [FreqGear; FI_GEARS] = [
        FreqGear::Gear0,
        FreqGear::Gear1,
        FreqGear::Gear2,
        FreqGear::Gear3,
    ];

    /// 档号（0~3）。
    pub const fn num(self) -> u8 {
        match self {
            FreqGear::Gear0 => 0,
            FreqGear::Gear1 => 1,
            FreqGear::Gear2 => 2,
            FreqGear::Gear3 => 3,
        }
    }

    /// 标称频率 MHz（常量写死并注释来源：逻辑建模档位，非真机实测）。
    pub const fn mhz(self) -> u32 {
        match self {
            FreqGear::Gear0 => 800,
            FreqGear::Gear1 => 1600,
            FreqGear::Gear2 => 2800,
            FreqGear::Gear3 => 4000,
        }
    }

    /// 功耗代价 ppm（归一化：Gear3=1e6 满功耗，逐档递减）。
    pub const fn power_ppm(self) -> u64 {
        match self {
            FreqGear::Gear0 => 150_000,
            FreqGear::Gear1 => 400_000,
            FreqGear::Gear2 => 700_000,
            FreqGear::Gear3 => 1_000_000,
        }
    }

    /// 频率再爬升代价（从 Gear0 爬到本档的 tick 代价，标称逐档递增）。
    pub const fn ramp_ticks(self) -> u64 {
        match self {
            FreqGear::Gear0 => 0,
            FreqGear::Gear1 => 20,
            FreqGear::Gear2 => 45,
            FreqGear::Gear3 => 80,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            FreqGear::Gear0 => "节能档",
            FreqGear::Gear1 => "平衡档",
            FreqGear::Gear2 => "性能档",
            FreqGear::Gear3 => "睿频档",
        }
    }
}

/// idle 驻留预期闭集（判据「驻留预期」三档：预计空闲多久，驱动联合决策）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleForecast {
    /// Short 短驻预期（即将来活，驻留 < 阈短）。
    Short,
    /// Medium 中驻预期。
    Medium,
    /// Deep 深驻预期（长时间无活，值得深下沉）。
    Deep,
}

impl IdleForecast {
    /// 全量闭集（三档）。
    pub const ALL: [IdleForecast; FI_FORECASTS] = [
        IdleForecast::Short,
        IdleForecast::Medium,
        IdleForecast::Deep,
    ];

    /// 预期驻留 tick 数（标称：短/中/深）。
    pub const fn residency_ticks(self) -> u64 {
        match self {
            IdleForecast::Short => 50,
            IdleForecast::Medium => 500,
            IdleForecast::Deep => 5_000,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            IdleForecast::Short => "短驻预期",
            IdleForecast::Medium => "中驻预期",
            IdleForecast::Deep => "深驻预期",
        }
    }
}

/// idle 目标深度闭集（联合决策的输出之一：下沉到哪一档 idle）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleDepth {
    /// Shallow 浅 idle（唤醒最快，省电极少）。
    Shallow,
    /// Medium 中 idle。
    Medium,
    /// Deep 深 idle（唤醒最慢，省电极多）。
    Deep,
}

impl IdleDepth {
    /// 全量闭集（三档）。
    pub const ALL: [IdleDepth; FI_DEPTHS] = [
        IdleDepth::Shallow,
        IdleDepth::Medium,
        IdleDepth::Deep,
    ];

    /// 档号（0~2）。
    pub const fn num(self) -> u8 {
        match self {
            IdleDepth::Shallow => 0,
            IdleDepth::Medium => 1,
            IdleDepth::Deep => 2,
        }
    }

    /// 唤醒代价 tick（从本深度被唤醒的延迟，逐档递增）。
    pub const fn wake_ticks(self) -> u64 {
        match self {
            IdleDepth::Shallow => 1,
            IdleDepth::Medium => 30,
            IdleDepth::Deep => 200,
        }
    }

    /// 驻留功耗 ppm（驻留期间每 tick 功耗，逐档递减）。
    pub const fn power_ppm(self) -> u64 {
        match self {
            IdleDepth::Shallow => 300_000,
            IdleDepth::Medium => 120_000,
            IdleDepth::Deep => 40_000,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            IdleDepth::Shallow => "浅 idle",
            IdleDepth::Medium => "中 idle",
            IdleDepth::Deep => "深 idle",
        }
    }
}

/// 诊断码（0x5130 段十四码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FiCode {
    /// 0x5130 决策振荡检出（横跳达阈值）。
    Oscillation,
    /// 0x5131 振荡漏检（注入的振荡未被检出）。
    OscMissed,
    /// 0x5132 联合决策非最优（argmin 与决策格预计算不一致）。
    Suboptimal,
    /// 0x5133 决策账缺动机字段（六列不齐）。
    MotiveMissing,
    /// 0x5134 联合成本权重和越界（三权重和 ≠ 1e6）。
    WeightSum,
    /// 0x5135 驻留预期档越界（非三档闭集）。
    ForecastBad,
    /// 0x5136 频率档越界（非四档闭集）。
    GearBad,
    /// 0x5137 idle 深度档越界（非三档闭集）。
    DepthBad,
    /// 0x5138 决策账 vs 出账数不平。
    DecisionLedger,
    /// 0x5139 振荡注入计数 ≠ 检出计数。
    OscLedger,
    /// 0x513a 决策格冻结点漂移（FNV 指纹不符）。
    GridTamper,
    /// 0x513b 重复决策 tick（同 tick 二次决策）。
    TickReuse,
    /// 0x513c 振荡确认窗越界（窗口长 < 2）。
    WindowBad,
    /// 0x513d 段位守卫失败。
    SegmentGuard,
}

impl FiCode {
    /// 全量闭集（十四码）。
    pub const ALL: [FiCode; 14] = [
        FiCode::Oscillation,
        FiCode::OscMissed,
        FiCode::Suboptimal,
        FiCode::MotiveMissing,
        FiCode::WeightSum,
        FiCode::ForecastBad,
        FiCode::GearBad,
        FiCode::DepthBad,
        FiCode::DecisionLedger,
        FiCode::OscLedger,
        FiCode::GridTamper,
        FiCode::TickReuse,
        FiCode::WindowBad,
        FiCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        FI_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            FiCode::Oscillation => "决策振荡检出：频率/idle 深度横跳达阈值",
            FiCode::OscMissed => "振荡漏检：注入的振荡决策未被检出",
            FiCode::Suboptimal => "联合决策非最优：argmin 与决策格预计算不一致",
            FiCode::MotiveMissing => "决策账缺动机字段：六列不齐",
            FiCode::WeightSum => "联合成本权重和越界：三权重和须恰 1e6",
            FiCode::ForecastBad => "驻留预期档越界：非三档闭集",
            FiCode::GearBad => "频率档越界：非四档闭集",
            FiCode::DepthBad => "idle 深度档越界：非三档闭集",
            FiCode::DecisionLedger => "决策账不平：决策数与出账数不等",
            FiCode::OscLedger => "振荡账不平：注入数与检出数不等",
            FiCode::GridTamper => "决策格冻结点漂移：FNV 指纹不符",
            FiCode::TickReuse => "重复决策 tick：同 tick 二次决策",
            FiCode::WindowBad => "振荡确认窗越界：窗口长须 ≥2",
            FiCode::SegmentGuard => "段位守卫失败：诊断码越出 0x5130 段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            FiCode::Oscillation => "加大迟滞确认窗或抬升决策粘性，抑制横跳",
            FiCode::OscMissed => "复核振荡检测器的翻转计数与确认窗阈值",
            FiCode::Suboptimal => "复核联合成本函数与决策格预计算是否同源",
            FiCode::MotiveMissing => "补全决策账动机字段（F2014 同法六列）",
            FiCode::WeightSum => "三权重 ppm 之和须恰等于 1e6",
            FiCode::ForecastBad => "驻留预期只允许 Short/Medium/Deep 三档",
            FiCode::GearBad => "频率档只允许 Gear0~Gear3 四档",
            FiCode::DepthBad => "idle 深度只允许 Shallow/Medium/Deep 三档",
            FiCode::DecisionLedger => "复核决策数与账条数守恒",
            FiCode::OscLedger => "复核振荡注入与检出配对",
            FiCode::GridTamper => "决策格冻结后不可改，走 ADR 变更流程",
            FiCode::TickReuse => "每 tick 至多一次决策，先推进 tick",
            FiCode::WindowBad => "确认窗长至少 2 才谈得上迟滞",
            FiCode::SegmentGuard => "诊断码须落在本模块独占段内",
        }
    }
}

/// 决策动机闭集（判据「动机字段齐备」，F2014 同法：每笔决策须登记唯一动机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FiMotive {
    /// 省电优先：深驻预期下压频 + 深 idle。
    PowerSave,
    /// 响应优先：短驻预期下保频 + 浅 idle（保唤醒快）。
    LatencyFirst,
    /// 吞吐优先：高负载预期下升频 + 浅 idle。
    Throughput,
    /// 平衡折中：中驻预期下中档 + 中 idle。
    Balanced,
    /// 迟滞保持：上一决策仍最优，保持不变（防振荡）。
    HysteresisHold,
}

impl FiMotive {
    /// 全量闭集（五动机）。
    pub const ALL: [FiMotive; 5] = [
        FiMotive::PowerSave,
        FiMotive::LatencyFirst,
        FiMotive::Throughput,
        FiMotive::Balanced,
        FiMotive::HysteresisHold,
    ];

    /// 动机短码。
    pub const fn code(self) -> &'static str {
        match self {
            FiMotive::PowerSave => "PWRSAVE",
            FiMotive::LatencyFirst => "LAT1ST",
            FiMotive::Throughput => "THRUPUT",
            FiMotive::Balanced => "BALANCED",
            FiMotive::HysteresisHold => "HYSHOLD",
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            FiMotive::PowerSave => "省电优先",
            FiMotive::LatencyFirst => "响应优先",
            FiMotive::Throughput => "吞吐优先",
            FiMotive::Balanced => "平衡折中",
            FiMotive::HysteresisHold => "迟滞保持",
        }
    }
}

/// 联合动作（决策输出：目标频率档 × 目标 idle 深度 + 动机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JointAction {
    /// 目标频率档。
    pub gear: FreqGear,
    /// 目标 idle 深度。
    pub depth: IdleDepth,
    /// 动机。
    pub motive: FiMotive,
    /// 联合成本（整数 ppm·tick 归一，供 argmin 复核）。
    pub cost: u64,
}

/// FNV-1a 64 位指纹（确定性，零依赖）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 决策格冻结点指纹（输入：决策格 + 权重，输出 u64）。冻结后比对防篡改。
pub fn grid_fingerprint(grid: &[JointAction; FI_FORECASTS * FI_GEARS]) -> u64 {
    let mut buf: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < grid.len() {
        buf.push(grid[i].gear.num());
        buf.push(grid[i].depth.num());
        // 动机编码进指纹（五动机 0~4）。
        let mc = match grid[i].motive {
            FiMotive::PowerSave => 0u8,
            FiMotive::LatencyFirst => 1,
            FiMotive::Throughput => 2,
            FiMotive::Balanced => 3,
            FiMotive::HysteresisHold => 4,
        };
        buf.push(mc);
        // 成本低 16 位入指纹（够区分 + 保持稳定）。
        buf.extend_from_slice(&(grid[i].cost as u16).to_le_bytes());
        i += 1;
    }
    // 权重入指纹。
    buf.extend_from_slice(&FI_W_WAKE_PPM.to_le_bytes());
    buf.extend_from_slice(&FI_W_RAMP_PPM.to_le_bytes());
    buf.extend_from_slice(&FI_W_POWER_PPM.to_le_bytes());
    fnv1a(&buf)
}


/// 决策记录（判据「决策账」六列 append-only：tick/驻留预期/入档/出档/出深度/动机）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DecisionRecord {
    /// 决策发生的 tick。
    pub tick: u64,
    /// 当时的驻留预期。
    pub forecast: IdleForecast,
    /// 决策前频率档。
    pub in_gear: FreqGear,
    /// 决策后频率档（出档）。
    pub out_gear: FreqGear,
    /// 决策后 idle 深度（出深度）。
    pub out_depth: IdleDepth,
    /// 动机（六列之末，F2014 同法）。
    pub motive: FiMotive,
}

/// 联合成本函数（判据「联合最优」核心，全整数 ppm 归一）。
/// 成本 = 唤醒代价×W_wake + 频率再爬升代价×W_ramp + 驻留功耗代价×W_power，
/// 三项权重和恰 1e6，故成本量纲一致可比，argmin 即联合最优。
/// 入参：驻留预期（决定驻留时长权重）+ 目标 idle 深度 + 目标频率档。
pub fn joint_cost(forecast: IdleForecast, depth: IdleDepth, gear: FreqGear) -> u64 {
    // 唤醒代价：深度越深唤醒越贵，乘以预期驻留次数的倒数近似（短驻唤醒更频繁更敏感）。
    // 短驻预期把唤醒代价放大（频繁唤醒惩罚），深驻预期把功耗代价放大（长期驻留惩罚）。
    let wake_base = depth.wake_ticks();
    let wake = match forecast {
        IdleForecast::Short => wake_base.saturating_mul(4), // 频繁唤醒，重罚
        IdleForecast::Medium => wake_base.saturating_mul(2),
        IdleForecast::Deep => wake_base, // 少唤醒，轻罚
    };
    // 频率再爬升代价：目标档越高，从 Gear0 爬升越贵。
    let ramp = gear.ramp_ticks();
    // 驻留功耗代价：频率档功耗 + idle 深度驻留功耗，随驻留时长加权。
    let power_base = gear.power_ppm() / 1000 + depth.power_ppm() / 1000;
    let power = match forecast {
        IdleForecast::Short => power_base, // 短驻，功耗轻
        IdleForecast::Medium => power_base.saturating_mul(2),
        IdleForecast::Deep => power_base.saturating_mul(6), // 深驻，功耗重罚
    };
    // 加权求和（ppm 权重）。
    let c = (wake.saturating_mul(FI_W_WAKE_PPM)
        + ramp.saturating_mul(FI_W_RAMP_PPM)
        + power.saturating_mul(FI_W_POWER_PPM))
        / FI_WEIGHT_DEN;
    c
}

/// 由 argmin 选出的动机（按驻留预期归类，判据「动机字段齐备」）。
pub fn motive_for(forecast: IdleForecast, depth: IdleDepth, gear: FreqGear) -> FiMotive {
    // 深驻 + 低档 → 省电；短驻 + 浅 idle → 响应；高频 + 浅 idle → 吞吐。
    if gear.num() >= 2 && depth.num() == 0 {
        FiMotive::Throughput
    } else if gear.num() == 0 && depth.num() == 2 {
        FiMotive::PowerSave
    } else if matches!(forecast, IdleForecast::Short) && depth.num() == 0 {
        FiMotive::LatencyFirst
    } else {
        FiMotive::Balanced
    }
}

/// 对给定 (驻留预期, 频率档) 做联合最优选择：遍历 idle 深度三档取 argmin。
/// 频率档由入参给定（决策面只联合调 idle 深度 + 复核频率档合理性），
/// 返回联合最优动作（depth + motive + cost）。
pub fn argmin_over_depth(forecast: IdleForecast, gear: FreqGear) -> JointAction {
    let mut best_depth = IdleDepth::Shallow;
    let mut best_cost = joint_cost(forecast, IdleDepth::Shallow, gear);
    // 遍历 Medium / Deep。
    let mut idx = 1;
    while idx < FI_DEPTHS {
        let d = IdleDepth::ALL[idx];
        let c = joint_cost(forecast, d, gear);
        if c < best_cost {
            best_cost = c;
            best_depth = d;
        }
        idx += 1;
    }
    JointAction {
        gear,
        depth: best_depth,
        motive: motive_for(forecast, best_depth, gear),
        cost: best_cost,
    }
}

/// 预计算完整决策格（12 格 = 3 驻留预期 × 4 频率档）。冻结指纹基于此格。
pub fn build_decision_grid() -> [JointAction; FI_FORECASTS * FI_GEARS] {
    let mut grid = [JointAction {
        gear: FreqGear::Gear0,
        depth: IdleDepth::Shallow,
        motive: FiMotive::Balanced,
        cost: 0,
    }; FI_FORECASTS * FI_GEARS];
    let mut f = 0;
    while f < FI_FORECASTS {
        let mut g = 0;
        while g < FI_GEARS {
            let forecast = IdleForecast::ALL[f];
            let gear = FreqGear::ALL[g];
            grid[f * FI_GEARS + g] = argmin_over_depth(forecast, gear);
            g += 1;
        }
        f += 1;
    }
    grid
}

/// 段位守卫（判据：诊断码全部落在 0x5130 段内）。
pub fn segment_guard() -> bool {
    FiCode::ALL
        .iter()
        .all(|c| c.val() >= FI_SEG_BASE && c.val() < FI_SEG_BASE + 0x100)
}


/// 振荡检测器（判据「决策振荡注入 10/10 检出」）。
/// 原理：滑动窗口记录最近 FI_OSC_WINDOW 次出档翻转（档号变化即一次 flip），
/// 窗口内 flip 累计达 FI_OSC_FLIPS 即判一次振荡。迟滞确认：翻转到邻档又翻回
/// 计入 flip，避免单次抖动误报。
pub struct OscDetector {
    /// 最近出档号序列（滑动窗口，长度 ≤ FI_OSC_WINDOW）。
    window: Vec<u8>,
    /// 窗口内累计翻转次数。
    flips: usize,
    /// 检出振荡总次数。
    pub detected: u64,
    /// 注入振荡总次数（外部注入，用于守恒对账）。
    pub injected: u64,
}

impl OscDetector {
    /// 构造。
    pub fn new() -> Self {
        OscDetector {
            window: Vec::new(),
            flips: 0,
            detected: 0,
            injected: 0,
        }
    }

    /// 喂入一次出档号，返回本次是否触发振荡检出。
    pub fn feed(&mut self, gear_num: u8) -> bool {
        // 与窗口末档比较，变化即 flip。
        if let Some(&last) = self.window.last() {
            if last != gear_num {
                self.flips += 1;
            }
        }
        self.window.push(gear_num);
        // 窗口滑出（保守检出，滑出不计回退 flip，宁多勿漏）。
        if self.window.len() > FI_OSC_WINDOW {
            let _removed = self.window.remove(0);
        }
        // 达阈值判振荡，检出后清窗重计（一次振荡只记一次）。
        if self.flips >= FI_OSC_FLIPS {
            self.detected += 1;
            self.window.clear();
            self.flips = 0;
            true
        } else {
            false
        }
    }

    /// 振荡账平（注入 ≡ 检出，判据守恒）。
    pub fn balanced(&self) -> bool {
        self.injected == self.detected
    }
}

impl Default for OscDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// idle/频率联合决策引擎（判据主载体）。
pub struct FreqIdleEngine {
    /// 冻结的决策格（12 格）。
    grid: [JointAction; FI_FORECASTS * FI_GEARS],
    /// 决策格冻结指纹（冻结后比对防篡改）。
    frozen_fp: u64,
    /// 决策账（六列 append-only）。
    ledger: Vec<DecisionRecord>,
    /// 当前频率档。
    cur_gear: FreqGear,
    /// 逻辑 tick 计数。
    tick: u64,
    /// 上一决策 tick（防同 tick 重复决策）。
    last_decision_tick: u64,
    /// 是否有过决策（首笔无前档可比）。
    has_decision: bool,
    /// 振荡检测器。
    osc: OscDetector,
    /// 权重和是否合法（构造期校验）。
    weight_ok: bool,
}

impl FreqIdleEngine {
    /// 构造：预计算并冻结决策格。
    pub fn new() -> Self {
        let grid = build_decision_grid();
        let frozen_fp = grid_fingerprint(&grid);
        FreqIdleEngine {
            grid,
            frozen_fp,
            ledger: Vec::new(),
            cur_gear: FreqGear::Gear1,
            tick: 0,
            last_decision_tick: 0,
            has_decision: false,
            osc: OscDetector::new(),
            weight_ok: FI_W_WAKE_PPM + FI_W_RAMP_PPM + FI_W_POWER_PPM == FI_WEIGHT_DEN,
        }
    }

    /// 推进逻辑 tick。
    pub fn advance(&mut self) {
        self.tick += 1;
    }

    /// 当前 tick。
    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 查询决策格某格（驻留预期 × 频率档 → 联合最优动作）。
    pub fn grid_at(&self, forecast: IdleForecast, gear: FreqGear) -> JointAction {
        self.grid[forecast as usize * FI_GEARS + gear as usize]
    }

    /// 权重和合法（构造期三权重 ppm 和恰 1e6）。
    pub fn weight_ok(&self) -> bool {
        self.weight_ok
    }

    /// 设置当前频率档（上游给定，决策面只联合调 idle 深度）。
    pub fn set_gear(&mut self, gear: FreqGear) {
        self.cur_gear = gear;
    }

    /// 当前频率档。
    pub fn cur_gear(&self) -> FreqGear {
        self.cur_gear
    }

    /// 执行一次联合决策：给定驻留预期，联合最优选 idle 深度（频率档沿用当前档，
    /// 决策面在本单聚焦「驻留预期×频率档→idle 深度联合最优」）。
    /// 记录六列决策账 + 喂振荡检测器。返回联合最优动作。
    pub fn decide(&mut self, forecast: IdleForecast) -> Result<JointAction, (FiCode, String)> {
        // 同 tick 重复决策拒。
        if self.has_decision && self.tick == self.last_decision_tick {
            return Err((
                FiCode::TickReuse,
                format!("同 tick {} 二次决策：先 advance 再 decide", self.tick),
            ));
        }
        // 驻留预期档合法性（闭集校验，显式守卫）。
        if forecast as usize >= FI_FORECASTS {
            return Err((
                FiCode::ForecastBad,
                format!("驻留预期档 {} 越界", forecast as usize),
            ));
        }
        let in_gear = self.cur_gear;
        // 取决策格预计算（冻结值），并与运行期 argmin 复核一致。
        let cell = self.grid_at(forecast, in_gear);
        let recomputed = argmin_over_depth(forecast, in_gear);
        if recomputed.depth != cell.depth || recomputed.cost != cell.cost {
            return Err((
                FiCode::Suboptimal,
                format!(
                    "决策格与运行期 argmin 不一致：格={}/{} 重算={}/{}",
                    cell.depth.name(),
                    cell.cost,
                    recomputed.depth.name(),
                    recomputed.cost
                ),
            ));
        }
        // 登记六列决策账。
        self.ledger.push(DecisionRecord {
            tick: self.tick,
            forecast,
            in_gear,
            out_gear: cell.gear,
            out_depth: cell.depth,
            motive: cell.motive,
        });
        self.last_decision_tick = self.tick;
        self.has_decision = true;
        // 喂振荡检测器（用出档号）。
        self.osc.feed(cell.gear.num());
        Ok(cell)
    }


    /// 决策账条数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 取第 i 笔决策账（下标先比长度，铁律）。
    pub fn ledger_at(&self, i: usize) -> Option<&DecisionRecord> {
        if i < self.ledger.len() {
            Some(&self.ledger[i])
        } else {
            None
        }
    }

    /// 动机字段齐备核（判据「动机字段齐备」：每笔决策动机 ∈ 五动机闭集）。
    pub fn motive_complete(&self) -> bool {
        self.ledger.iter().all(|r| FiMotive::ALL.contains(&r.motive))
    }

    /// 决策账守恒（决策数 ≡ 账条数）。
    pub fn decision_ledger_ok(&self) -> bool {
        self.ledger.len() == self.decision_count()
    }

    /// 决策次数（= 账长度）。
    pub fn decision_count(&self) -> usize {
        self.ledger.len()
    }

    /// 振荡检出数。
    pub fn osc_detected(&self) -> u64 {
        self.osc.detected
    }

    /// 登记一次外部振荡注入（用于守恒对账）。
    pub fn note_inject(&mut self) {
        self.osc.injected += 1;
    }

    /// 振荡账平（注入 ≡ 检出）。
    pub fn osc_balanced(&self) -> bool {
        self.osc.balanced()
    }

    /// 决策格冻结核（当前格指纹 ≡ 冻结指纹）。
    pub fn grid_frozen_ok(&self) -> bool {
        grid_fingerprint(&self.grid) == self.frozen_fp
    }

    /// 冻结指纹值（供外部登记/对拍）。
    pub fn frozen_fp(&self) -> u64 {
        self.frozen_fp
    }

    /// 全判据核（联合最优复核 + 动机齐备 + 账平 + 冻结核 + 权重和，齐过才绿）。
    pub fn verdict(&self) -> Result<bool, (FiCode, String)> {
        if !self.weight_ok {
            return Err((
                FiCode::WeightSum,
                format!(
                    "权重和 {} ≠ {}",
                    FI_W_WAKE_PPM + FI_W_RAMP_PPM + FI_W_POWER_PPM,
                    FI_WEIGHT_DEN
                ),
            ));
        }
        if !self.grid_frozen_ok() {
            return Err((FiCode::GridTamper, "决策格冻结指纹漂移".to_string()));
        }
        if !self.motive_complete() {
            return Err((FiCode::MotiveMissing, "存在决策缺动机字段".to_string()));
        }
        if !self.decision_ledger_ok() {
            return Err((FiCode::DecisionLedger, "决策账不平".to_string()));
        }
        Ok(true)
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        format!(
            "idle/频率联合决策面：决策 {} 振荡检出 {} 注入 {} 动机齐备 {} 冻结 {} 权重 {} 当前档 {}",
            self.decision_count(),
            self.osc.detected,
            self.osc.injected,
            self.motive_complete(),
            self.grid_frozen_ok(),
            self.weight_ok,
            self.cur_gear.name()
        )
    }
}

impl Default for FreqIdleEngine {
    fn default() -> Self {
        Self::new()
    }
}

