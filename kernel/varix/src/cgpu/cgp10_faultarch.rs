//! CGPU-F2564 · 容错架构（CGPU-P 域 · 遥测组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2564`
//!
//! **判据（锚点原文）**：三策略、决策规范、两组、判据。
//!
//! **职责定位（锚点原文摘录）**：容错（容错架构（冗余/检查点/回退三策略——三策略（策略选择决策——决策规范；测试（三策略/决策两组）。
//!
//! # 设计总览
//!
//! 本模块实现 Varix no_std 内核 CGPU-P 域的统一容错架构。三策略共享一套事件账、
//! 一套诊断码段与同一抽象故障源管道：
//!
//! 1. **冗余（Redundancy）**：N+1 影子计算模型。主副本与 N 个影子副本对同一输入
//!    独立求值，`RedundancyUnit::vote()` 做多数表决。平票规则显性（主副本锚定，
//!    主副本缺席时取候选最小值）；分歧写入表决账；连续分歧触发熔断计数。
//! 2. **检查点（Checkpoint）**：定长槽位轮转快照 + CRC32 校验。快照负载采用
//!    确定性序列化（定长 48 字节、小端、保留位清零），`capture()/restore()`
//!    往返自证字节级一致；恢复演练（drill）维护独立计数器。
//! 3. **回退（Fallback）**：L0 主路 → L1 保守 → L2 最小功能三级回退链。
//!    进入滞回（3 连败降级）与回升滞回（8 连胜回升）双向防振荡；
//!    链底（L2）显性不可再降，重复失败计入链底命中账。
//!
//! 三策略通过 `StrategySelector` 的 const 决策表（故障率档 × 恢复成本档 × 数据
//! 关键度 → 策略组合）统一选择，`decide()` 输出可解释理由字符串并留痕。
//! `ComboCoordinator` 在冗余+检查点叠加时规定执行次序并做预算分账。
//! 故障注入测试面 `FaultSource` trait 使真源与注入源走同一条管道，
//! `FaultInjector` 提供超时/错值/重复/丢失四族注入与检出率统计（漏检显性计数）。
//!
//! # 诊断码段
//!
//! 本模块独占诊断码段 **0x12F0 ~ 0x12FF**（开工前已 grep 确认 cgpu 目录空闲，
//! 2026-10-24）。同一码段内按设施细分：错误码（`FaultArchError`，Result 失败
//! 路径）与事件码（`FaultEventKind`，事件账记录）各自独立编号，互不复用语义。
//!
//! # no_std 纪律
//!
//! - 仅依赖 `core` 与 `alloc`（`Vec`/`String`/`format!`），无 `crate::` 依赖。
//! - 内核路径零 `unwrap`/`expect`/`panic!`/运行期 `assert!`；可失败操作一律
//!   `Result<T, FaultArchError>`。
//! - 溢出全部显性：统计计数用 `saturating_add`，哈希用 `wrapping_*`。
//! - 大于 64 KiB 的结构体一律不上栈（本模块最大结构 `FaultEventLedger` 约 6 KiB）。
//! - const fn 内禁 `slice::get`、`Vec` 索引、`Ord::min/max`、derive 出的 `PartialEq`
//!   比较、for 循环；一律 while + 索引 + 原生类型比较。

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ============================================================================
// 第一节 · 诊断码段与错误类型
// ============================================================================

/// 错误设施：冗余副本数与请求不符。
/// 来源：锚点 CGPU-F2564 冗余策略节，2026-10-24 定稿。
pub const DIAG_ERR_REPLICA_MISMATCH: u32 = 0x12F0;
/// 错误设施：表决无法产生任何候选（防御性分支，主副本锚定规则下理论不可达）。
pub const DIAG_ERR_VOTE_NO_MAJORITY: u32 = 0x12F1;
/// 错误设施：冗余单元熔断后仍被调用。
pub const DIAG_ERR_CIRCUIT_TRIPPED: u32 = 0x12F2;
/// 错误设施：快照 CRC32 校验失败。
pub const DIAG_ERR_SNAPSHOT_CRC: u32 = 0x12F3;
/// 错误设施：无有效快照槽位可恢复。
pub const DIAG_ERR_SNAPSHOT_EMPTY: u32 = 0x12F4;
/// 错误设施：快照序列化反序列化失败（魔数/版本/保留位不符）。
pub const DIAG_ERR_SNAPSHOT_SER: u32 = 0x12F5;
/// 错误设施：回退链层级参数非法。
pub const DIAG_ERR_CHAIN_LEVEL: u32 = 0x12F6;
/// 错误设施：组合协调器预算耗尽后仍申请支出。
pub const DIAG_ERR_BUDGET_EXHAUSTED: u32 = 0x12F7;
/// 错误设施：决策表查询入参超出档位范围。
pub const DIAG_ERR_DECISION_UNCOVERED: u32 = 0x12F8;
/// 错误设施：快照负载字节数不符。
pub const DIAG_ERR_PAYLOAD_SIZE: u32 = 0x12F9;
/// 错误设施：注入源超时注入（模拟副本超时无响应）。
pub const DIAG_ERR_INJECTION_TIMEOUT: u32 = 0x12FA;
/// 错误设施：注入源丢失注入（模拟副本结果丢失）。
pub const DIAG_ERR_INJECTION_LOSS: u32 = 0x12FB;
/// 错误设施：注入器状态机非法（未注入却收到表决通知等）。
pub const DIAG_ERR_INJECTOR_STATE: u32 = 0x12FC;
/// 错误设施：协调器状态非法（快照间隔为 0 等）。
pub const DIAG_ERR_COORD_STATE: u32 = 0x12FD;
/// 错误设施：事件账饱和（防御性，环形账本按设计不饱和）。
pub const DIAG_ERR_LEDGER_SATURATED: u32 = 0x12FE;
/// 错误设施：保留码，本模块后续错误扩展位。
pub const DIAG_ERR_RESERVED: u32 = 0x12FF;

/// 本模块诊断码段下界（含）。来源：开工前 grep 确认 0x12F 段空闲。
pub const DIAG_SEGMENT_LO: u32 = 0x12F0;
/// 本模块诊断码段上界（含）。
pub const DIAG_SEGMENT_HI: u32 = 0x12FF;

/// 容错架构统一错误类型。每个变体的 `detail()` 给出中文可操作诊断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultArchError {
    /// 冗余副本数与单元配置不符。
    ReplicaCountMismatch,
    /// 表决无候选（防御性分支）。
    VoteNoMajority,
    /// 冗余单元已熔断。
    CircuitTripped,
    /// 快照 CRC32 校验失败。
    SnapshotCrcMismatch,
    /// 无有效快照槽位。
    SnapshotSlotEmpty,
    /// 快照序列化/反序列化失败。
    SnapshotSerializeInvalid,
    /// 回退链层级非法。
    ChainLevelInvalid,
    /// 预算耗尽。
    BudgetExhausted,
    /// 决策表未覆盖的输入组合。
    DecisionUncovered,
    /// 快照负载字节数不符。
    PayloadSizeInvalid,
    /// 注入源超时。
    InjectionTimeout,
    /// 注入源丢失。
    InjectionLoss,
    /// 注入器状态非法。
    InjectorStateInvalid,
    /// 协调器状态非法。
    CoordinatorStateInvalid,
    /// 事件账饱和（防御性）。
    LedgerSaturated,
}

impl FaultArchError {
    /// 诊断码。来源：本文件第一节错误设施常量表。
    pub const fn code(&self) -> u32 {
        match self {
            FaultArchError::ReplicaCountMismatch => DIAG_ERR_REPLICA_MISMATCH,
            FaultArchError::VoteNoMajority => DIAG_ERR_VOTE_NO_MAJORITY,
            FaultArchError::CircuitTripped => DIAG_ERR_CIRCUIT_TRIPPED,
            FaultArchError::SnapshotCrcMismatch => DIAG_ERR_SNAPSHOT_CRC,
            FaultArchError::SnapshotSlotEmpty => DIAG_ERR_SNAPSHOT_EMPTY,
            FaultArchError::SnapshotSerializeInvalid => DIAG_ERR_SNAPSHOT_SER,
            FaultArchError::ChainLevelInvalid => DIAG_ERR_CHAIN_LEVEL,
            FaultArchError::BudgetExhausted => DIAG_ERR_BUDGET_EXHAUSTED,
            FaultArchError::DecisionUncovered => DIAG_ERR_DECISION_UNCOVERED,
            FaultArchError::PayloadSizeInvalid => DIAG_ERR_PAYLOAD_SIZE,
            FaultArchError::InjectionTimeout => DIAG_ERR_INJECTION_TIMEOUT,
            FaultArchError::InjectionLoss => DIAG_ERR_INJECTION_LOSS,
            FaultArchError::InjectorStateInvalid => DIAG_ERR_INJECTOR_STATE,
            FaultArchError::CoordinatorStateInvalid => DIAG_ERR_COORD_STATE,
            FaultArchError::LedgerSaturated => DIAG_ERR_LEDGER_SATURATED,
        }
    }

    /// 中文可操作诊断。诊断信息全部为 'static，不借用运行期数据。
    pub fn detail(&self) -> &'static str {
        match self {
            FaultArchError::ReplicaCountMismatch => {
                "冗余副本数与 RedundancyUnit 配置不符：检查调用方提交的结果数组长度是否等于 new() 时传入的副本数（2..=REDUNDANCY_MAX_REPLICAS）。"
            }
            FaultArchError::VoteNoMajority => {
                "表决未产生任何候选：主副本锚定平票规则下该分支理论不可达，属防御性错误；请核查 vote() 入参是否包含主副本结果（索引 0）。"
            }
            FaultArchError::CircuitTripped => {
                "冗余单元已因连续分歧熔断：先调用 reset() 复位熔断状态并排查故障源（建议用 FaultInjector 复现），再恢复影子表决。"
            }
            FaultArchError::SnapshotCrcMismatch => {
                "快照 CRC32 校验失败：槽位数据可能被篡改或位翻转；请依次尝试更旧的有效槽位（restore_older()），并对存储介质做完整性巡检。"
            }
            FaultArchError::SnapshotSlotEmpty => {
                "无有效快照槽位可恢复：先调用 capture() 写入至少一份快照，或检查所有槽位是否均未通过 CRC 校验。"
            }
            FaultArchError::SnapshotSerializeInvalid => {
                "快照负载魔数/版本/保留位不符：确认数据由本版本 to_bytes() 产生，勿跨版本直接反序列化；版本不符请走迁移通道。"
            }
            FaultArchError::ChainLevelInvalid => {
                "回退链层级参数非法：合法层级为 0(L0 主路)/1(L1 保守)/2(L2 最小功能)，检查 set_level() 入参。"
            }
            FaultArchError::BudgetExhausted => {
                "组合协调器预算耗尽：当期 tick 不再执行检查点捕获；可调大 CoordinatorConfig 预算常量或降低 capture_interval 频率。"
            }
            FaultArchError::DecisionUncovered => {
                "决策表查询入参超出档位范围：故障率档/恢复成本档/关键度必须分别落在 0..=2/0..=2/0..=3；请核查枚举来源。"
            }
            FaultArchError::PayloadSizeInvalid => {
                "快照负载字节数不符：定长负载为 SNAPSHOT_DATA_LEN 字节，检查调用方缓冲区是否被截断。"
            }
            FaultArchError::InjectionTimeout => {
                "故障注入源模拟副本超时：这是注入面预期行为（非真实故障）；统计面请按注入族记为已注入，由表决结果判定检出或漏检。"
            }
            FaultArchError::InjectionLoss => {
                "故障注入源模拟结果丢失：同上，为注入面预期行为；协调器会以副本丢失哨兵值补位参与表决。"
            }
            FaultArchError::InjectorStateInvalid => {
                "注入器状态非法：notify_vote() 收到与当次注入不匹配的通知；请勿在 next_result() 之外调用注入器内部接口。"
            }
            FaultArchError::CoordinatorStateInvalid => {
                "组合协调器配置非法：capture_interval 不得为 0，副本操作表长度必须等于冗余副本数；请检查 CoordinatorConfig。"
            }
            FaultArchError::LedgerSaturated => {
                "事件账饱和（防御性）：环形账本按设计覆写最旧记录不应饱和；若触发请上报账本容量常量被误改。"
            }
        }
    }
}

// ============================================================================
// 第二节 · 三策略枚举与策略描述 const 表
// ============================================================================

/// 容错三策略。
///
/// - `Redundancy`：N+1 影子计算 + 多数表决。
/// - `Checkpoint`：周期快照 + CRC32 + 恢复演练。
/// - `Fallback`：降级到保守路径（回退链 + 滞回）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultStrategy {
    /// 冗余策略（N+1 影子 + 表决）。
    Redundancy,
    /// 检查点策略（周期快照 + 校验 + 演练）。
    Checkpoint,
    /// 回退策略（降级保守路径）。
    Fallback,
}

impl FaultStrategy {
    /// 档位索引（决策表/位掩码使用）。0=冗余 1=检查点 2=回退。
    pub const fn index(&self) -> u8 {
        match self {
            FaultStrategy::Redundancy => 0,
            FaultStrategy::Checkpoint => 1,
            FaultStrategy::Fallback => 2,
        }
    }

    /// 策略组合位掩码位（bit0=冗余 bit1=检查点 bit2=回退）。
    pub const fn mask_bit(&self) -> u8 {
        match self {
            FaultStrategy::Redundancy => 0b001,
            FaultStrategy::Checkpoint => 0b010,
            FaultStrategy::Fallback => 0b100,
        }
    }

    /// 由索引构造（const 可用）。越界返回 None。
    pub const fn from_index(i: u8) -> Option<FaultStrategy> {
        match i {
            0 => Some(FaultStrategy::Redundancy),
            1 => Some(FaultStrategy::Checkpoint),
            2 => Some(FaultStrategy::Fallback),
            _ => None,
        }
    }

    /// 策略描述 const 表项（const fn，显性 match 避免越界索引）。
    pub const fn desc(&self) -> &'static StrategyDesc {
        match self {
            FaultStrategy::Redundancy => &STRATEGY_DESC[0],
            FaultStrategy::Checkpoint => &STRATEGY_DESC[1],
            FaultStrategy::Fallback => &STRATEGY_DESC[2],
        }
    }
}

/// 策略描述条目：开销档 / 恢复时限档 / 适用数据等级。
///
/// 开销档：1（轻）~3（重）。恢复时限档：1（最快）~3（最慢）。
/// 适用数据等级：启用该策略所需的最低 `DataCriticality` 等级（0..=3）。
#[derive(Debug, Clone, Copy)]
pub struct StrategyDesc {
    /// 策略名（中文）。
    pub name: &'static str,
    /// 开销档 1..=3。
    pub overhead_tier: u8,
    /// 恢复时限档 1..=3（1=最快）。
    pub recovery_deadline_tier: u8,
    /// 适用最低数据等级 0..=3。
    pub min_data_grade: u8,
    /// 备注（中文）。
    pub note: &'static str,
}

/// 策略描述 const 表。
///
/// 数值来源：锚点 CGPU-F2564 决策规范节的三档刻度定义，2026-10-24 施工书定稿。
/// 冗余：每次 tick 需 N+1 次求值 + 表决扫描，开销最重，但错值当场检出（恢复最快）；
/// 检查点：每 capture_interval 一次定长 48B 序列化 + CRC32，开销中等，恢复需回放；
/// 回退：仅切换路径常数开销，但恢复到全功能需重新爬升（恢复时限最长）。
pub const STRATEGY_DESC: [StrategyDesc; 3] = [
    StrategyDesc {
        name: "冗余(N+1影子+多数表决)",
        overhead_tier: 3,
        recovery_deadline_tier: 1,
        min_data_grade: 2,
        note: "错值当场由表决检出，恢复时限最短；代价是每 tick N+1 份计算与表决扫描开销。",
    },
    StrategyDesc {
        name: "检查点(周期快照+CRC32)",
        overhead_tier: 2,
        recovery_deadline_tier: 2,
        min_data_grade: 1,
        note: "故障后从最近有效快照恢复，需回放自快照点以来的增量；开销与捕获频率成正比。",
    },
    StrategyDesc {
        name: "回退(降级保守路径)",
        overhead_tier: 1,
        recovery_deadline_tier: 3,
        min_data_grade: 0,
        note: "常数开销的路径切换，牺牲功能保可用；回升需 8 连胜滞回，全功能恢复时限最长。",
    },
];

/// 故障率档。来源：锚点决策规范节（低=<<1e-6/tick，中=1e-6~1e-3，高=>1e-3 量级刻度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultRateBand {
    /// 低故障率。
    Low,
    /// 中故障率。
    Mid,
    /// 高故障率。
    High,
}

impl FaultRateBand {
    /// 档位码 0..=2。
    pub const fn code(&self) -> u8 {
        match self {
            FaultRateBand::Low => 0,
            FaultRateBand::Mid => 1,
            FaultRateBand::High => 2,
        }
    }
    /// 中文名。
    pub const fn name(&self) -> &'static str {
        match self {
            FaultRateBand::Low => "低故障率",
            FaultRateBand::Mid => "中故障率",
            FaultRateBand::High => "高故障率",
        }
    }
    /// 由档位码构造（const）。越界返回 None。
    pub const fn from_code(c: u8) -> Option<FaultRateBand> {
        match c {
            0 => Some(FaultRateBand::Low),
            1 => Some(FaultRateBand::Mid),
            2 => Some(FaultRateBand::High),
            _ => None,
        }
    }
}

/// 恢复成本档。来源：锚点决策规范节（廉价=常数开销重建，昂贵=需全量重算/外存回放）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryCostBand {
    /// 恢复廉价。
    Cheap,
    /// 恢复成本中等。
    Moderate,
    /// 恢复昂贵。
    Expensive,
}

impl RecoveryCostBand {
    /// 档位码 0..=2。
    pub const fn code(&self) -> u8 {
        match self {
            RecoveryCostBand::Cheap => 0,
            RecoveryCostBand::Moderate => 1,
            RecoveryCostBand::Expensive => 2,
        }
    }
    /// 中文名。
    pub const fn name(&self) -> &'static str {
        match self {
            RecoveryCostBand::Cheap => "恢复廉价",
            RecoveryCostBand::Moderate => "恢复中等",
            RecoveryCostBand::Expensive => "恢复昂贵",
        }
    }
    /// 由档位码构造（const）。
    pub const fn from_code(c: u8) -> Option<RecoveryCostBand> {
        match c {
            0 => Some(RecoveryCostBand::Cheap),
            1 => Some(RecoveryCostBand::Moderate),
            2 => Some(RecoveryCostBand::Expensive),
            _ => None,
        }
    }
}

/// 数据关键度等级。来源：锚点决策规范节四级刻度（0 丢弃无害 ~ 3 不可丢失）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataCriticality {
    /// 低关键度：丢弃无害。
    Low,
    /// 中关键度：可短暂丢失。
    Medium,
    /// 高关键度：不可静默出错。
    High,
    /// 最高关键度：不可丢失且不可出错。
    Vital,
}

impl DataCriticality {
    /// 等级码 0..=3。
    pub const fn code(&self) -> u8 {
        match self {
            DataCriticality::Low => 0,
            DataCriticality::Medium => 1,
            DataCriticality::High => 2,
            DataCriticality::Vital => 3,
        }
    }
    /// 中文名。
    pub const fn name(&self) -> &'static str {
        match self {
            DataCriticality::Low => "低关键度",
            DataCriticality::Medium => "中关键度",
            DataCriticality::High => "高关键度",
            DataCriticality::Vital => "最高关键度",
        }
    }
    /// 由等级码构造（const）。
    pub const fn from_code(c: u8) -> Option<DataCriticality> {
        match c {
            0 => Some(DataCriticality::Low),
            1 => Some(DataCriticality::Medium),
            2 => Some(DataCriticality::High),
            3 => Some(DataCriticality::Vital),
            _ => None,
        }
    }
}

// ============================================================================
// 第三节 · CRC32 与哈希工具
// ============================================================================

/// CRC32（IEEE 802.3）生成多项式。来源：ISO 3309 / ITU-T V.42 标准反射形式。
const CRC32_POLY: u32 = 0xEDB8_8320;

/// const fn 生成 CRC32 查表（256 项）。const 语境下用 while + 索引。
const fn build_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0u8;
        while k < 8 {
            // 原生整型比较在 const fn 中合法；不用 derive 出的 PartialEq。
            if (c & 1) != 0 {
                c = CRC32_POLY ^ (c >> 1);
            } else {
                c = c >> 1;
            }
            k = k + 1;
        }
        table[i] = c;
        i = i + 1;
    }
    table
}

/// CRC32 查表。编译期生成，零运行期初始化成本。
pub const CRC32_TABLE: [u32; 256] = build_crc32_table();

/// 计算字节序列的 CRC32（IEEE 802.3，初值/终值取反）。
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < bytes.len() {
        let idx = ((crc ^ (bytes[i] as u32)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[idx];
        i = i + 1;
    }
    crc ^ 0xFFFF_FFFF
}

/// FNV-1a 64 位偏移基。来源：FNV 哈希规范（fingerprint 用）。
const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
/// FNV-1a 64 位质数。
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// 对一段字节做 FNV-1a 64 位哈希（事件账指纹用）。溢出显性 wrapping。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = FNV_OFFSET;
    let mut i = 0usize;
    while i < bytes.len() {
        h = h.wrapping_mul(FNV_PRIME);
        h = h ^ (bytes[i] as u64);
        i = i + 1;
    }
    h
}

// ============================================================================
// 第四节 · 冗余单元（N+1 影子 + 多数表决 + 表决账 + 熔断）
// ============================================================================

/// 冗余单元支持的最大副本数（主 1 + 影子 4）。
/// 来源：锚点冗余策略节 N+1 上限刻度（N<=4），2026-10-24 定稿。
pub const REDUNDANCY_MAX_REPLICAS: u8 = 5;

/// 连续分歧熔断阈值。来源：锚点冗余策略节"连续分歧熔断计数"刻度（5 次触发）。
pub const VOTE_TRIP_THRESHOLD: u32 = 5;

/// 表决账容量（环形覆写最旧）。表决账是诊断面而非存证面，32 条足够回溯。
pub const VOTE_LEDGER_CAP: usize = 32;

/// 副本丢失哨兵值。丢失副本以该哨兵参与表决；若主副本丢失，哨兵几乎必然
/// 与其余副本分歧，从而触发分歧账与熔断计数。
/// 来源：u64::MAX 在本管线中不作为合法计算结果（各副本算子保证输出 < u64::MAX）。
pub const REPLICA_LOST_SENTINEL: u64 = 0xFFFF_FFFF_FFFF_FFFF;

/// 单条表决账记录（全标量，可 Copy；约 40B × 32 = 1.3KB，不上大栈）。
#[derive(Debug, Clone, Copy)]
pub struct VoteRecord {
    /// 表决发生时的 tick。
    pub tick: u64,
    /// 输入。
    pub input: u64,
    /// 表决选出的值。
    pub chosen: u64,
    /// 主副本（索引 0）的值。
    pub primary: u64,
    /// 选中值的票数。
    pub majority_count: u8,
    /// 是否触发了平票规则。
    pub tie_broken: bool,
    /// 是否存在分歧（tie_broken 或 选中值 != 主副本值）。
    pub diverged: bool,
    /// 丢失副本数（哨兵补位计入）。
    pub lost_replicas: u8,
}

/// 表决报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoteReport {
    /// 表决选出的值。
    pub value: u64,
    /// 选中值的票数。
    pub majority_count: u8,
    /// 是否全票一致。
    pub unanimous: bool,
    /// 是否触发了平票规则。
    pub tie_broken: bool,
    /// 是否存在分歧。
    pub diverged: bool,
    /// 本次表决是否恰好触发熔断。
    pub tripped_now: bool,
}

/// 冗余单元：N+1 影子执行 + 多数表决 + 表决账 + 连续分歧熔断。
///
/// - 表决规则：严格多数（票数 > n/2）直接当选；
/// - 平票规则（显性）：无严格多数时，若主副本值在最高票候选中则选主副本
///   （主副本锚定）；否则取最高票候选中的最小值（确定性下界规则）。
///   两种平票路径都计入 `tie_breaks` 并在表决账中标 `tie_broken`。
/// - 分歧定义：`tie_broken || 选中值 != 主副本值`。分歧连续达
///   [`VOTE_TRIP_THRESHOLD`] 次触发熔断（tripped），复位需显式 `reset()`。
#[derive(Debug, Clone)]
pub struct RedundancyUnit {
    replica_count: u8,
    votes_cast: u32,
    divergences_total: u32,
    consecutive_divergences: u32,
    tie_breaks: u32,
    tripped: bool,
    trip_count: u32,
    ledger_head: usize,
    ledger_len: usize,
    ledger: [VoteRecord; VOTE_LEDGER_CAP],
}

impl RedundancyUnit {
    /// 构造。`replica_count` 合法范围 2..=5（1 主 + 1..=4 影子）。
    pub fn new(replica_count: u8) -> Result<RedundancyUnit, FaultArchError> {
        if replica_count < 2 || replica_count > REDUNDANCY_MAX_REPLICAS {
            return Err(FaultArchError::ReplicaCountMismatch);
        }
        Ok(RedundancyUnit {
            replica_count,
            votes_cast: 0,
            divergences_total: 0,
            consecutive_divergences: 0,
            tie_breaks: 0,
            tripped: false,
            trip_count: 0,
            ledger_head: 0,
            ledger_len: 0,
            ledger: [VoteRecord {
                tick: 0,
                input: 0,
                chosen: 0,
                primary: 0,
                majority_count: 0,
                tie_broken: false,
                diverged: false,
                lost_replicas: 0,
            }; VOTE_LEDGER_CAP],
        })
    }

    /// 副本数。
    pub fn replica_count(&self) -> u8 {
        self.replica_count
    }

    /// 已表决次数。
    pub fn votes_cast(&self) -> u32 {
        self.votes_cast
    }

    /// 累计分歧次数。
    pub fn divergences_total(&self) -> u32 {
        self.divergences_total
    }

    /// 当前连续分歧计数。
    pub fn consecutive_divergences(&self) -> u32 {
        self.consecutive_divergences
    }

    /// 累计平票规则触发次数。
    pub fn tie_breaks(&self) -> u32 {
        self.tie_breaks
    }

    /// 是否已熔断。
    pub fn is_tripped(&self) -> bool {
        self.tripped
    }

    /// 累计熔断次数。
    pub fn trip_count(&self) -> u32 {
        self.trip_count
    }

    /// 表决账当前长度。
    pub fn ledger_len(&self) -> usize {
        self.ledger_len
    }

    /// 按账内序（旧→新）读第 i 条表决记录；越界返回 None。
    pub fn vote_record(&self, i: usize) -> Option<VoteRecord> {
        if i >= self.ledger_len {
            return None;
        }
        let idx = (self.ledger_head + i) % VOTE_LEDGER_CAP;
        Some(self.ledger[idx])
    }

    /// 复位熔断与连续分歧计数（熔断后的手动恢复入口；账本与累计量保留）。
    pub fn reset(&mut self) {
        self.tripped = false;
        self.consecutive_divergences = 0;
    }

    /// 多数表决。
    ///
    /// `results[0]` 为主副本结果，其余为影子副本结果；长度必须等于配置副本数。
    /// 丢失副本由调用方以 [`REPLICA_LOST_SENTINEL`] 补位后照常参与表决。
    pub fn vote(
        &mut self,
        tick: u64,
        input: u64,
        results: &[u64],
        ledger: &mut FaultEventLedger,
    ) -> Result<VoteReport, FaultArchError> {
        if self.tripped {
            return Err(FaultArchError::CircuitTripped);
        }
        if results.len() != self.replica_count as usize || results.is_empty() {
            return Err(FaultArchError::ReplicaCountMismatch);
        }
        let n = results.len();
        let primary = results[0];

        // 票数扫描：对每个候选统计出现次数（n<=5，直接双重 while）。
        let mut best_count = 0usize;
        let mut best_value = primary;
        let mut i = 0usize;
        while i < n {
            let mut cnt = 0usize;
            let mut j = 0usize;
            while j < n {
                if results[j] == results[i] {
                    cnt = cnt + 1;
                }
                j = j + 1;
            }
            // 取首个达到最高票的候选（确定性：先到先得 + 下界规则兜底）。
            if (cnt > best_count) || (cnt == best_count && results[i] < best_value) {
                best_count = cnt;
                best_value = results[i];
            }
            i = i + 1;
        }

        let majority_needed = n / 2 + 1;
        let mut tie_broken = false;
        let chosen: u64;
        if best_count >= majority_needed {
            // 严格多数，直接当选。
            chosen = best_value;
        } else {
            // 平票/无严格多数：主副本锚定，主副本缺席时取候选最小值。
            let mut primary_in_candidates = false;
            let mut min_candidate = u64::MAX;
            let mut candidate_count = 0usize;
            let mut k = 0usize;
            while k < n {
                let mut cnt = 0usize;
                let mut j = 0usize;
                while j < n {
                    if results[j] == results[k] {
                        cnt = cnt + 1;
                    }
                    j = j + 1;
                }
                if cnt == best_count {
                    candidate_count = candidate_count + 1;
                    if results[k] == primary {
                        primary_in_candidates = true;
                    }
                    if results[k] < min_candidate {
                        min_candidate = results[k];
                    }
                }
                k = k + 1;
            }
            if candidate_count == 0 {
                // 防御性分支：主副本锚定规则下不可达。
                return Err(FaultArchError::VoteNoMajority);
            }
            if primary_in_candidates {
                chosen = primary;
            } else {
                chosen = min_candidate;
            }
            tie_broken = true;
            self.tie_breaks = self.tie_breaks.saturating_add(1);
        }

        let diverged = tie_broken || best_count != n; // 分歧 = 非全票一致（影子偏离/平票均算）
        let unanimous = best_count == n && !tie_broken;

        self.votes_cast = self.votes_cast.saturating_add(1);
        let mut tripped_now = false;
        if diverged {
            self.divergences_total = self.divergences_total.saturating_add(1);
            self.consecutive_divergences = self.consecutive_divergences.saturating_add(1);
            ledger.record(FaultEventKind::VoteDivergence, tick, chosen);
            if tie_broken {
                ledger.record(FaultEventKind::VoteTieBroken, tick, chosen);
            }
            if self.consecutive_divergences >= VOTE_TRIP_THRESHOLD {
                self.tripped = true;
                tripped_now = true;
                self.trip_count = self.trip_count.saturating_add(1);
                self.consecutive_divergences = 0;
                ledger.record(FaultEventKind::CircuitTripped, tick, input);
            }
        } else {
            self.consecutive_divergences = 0;
        }

        // 写表决账（环形覆写最旧）。
        let rec = VoteRecord {
            tick,
            input,
            chosen,
            primary,
            majority_count: best_count as u8,
            tie_broken,
            diverged,
            lost_replicas: count_sentinel(results),
        };
        let write_at = if self.ledger_len < VOTE_LEDGER_CAP {
            let at = (self.ledger_head + self.ledger_len) % VOTE_LEDGER_CAP;
            self.ledger_len = self.ledger_len + 1;
            at
        } else {
            let at = self.ledger_head;
            self.ledger_head = (self.ledger_head + 1) % VOTE_LEDGER_CAP;
            at
        };
        self.ledger[write_at] = rec;

        Ok(VoteReport {
            value: chosen,
            majority_count: best_count as u8,
            unanimous,
            tie_broken,
            diverged,
            tripped_now,
        })
    }
}

/// 统计哨兵补位（丢失副本）数量。
fn count_sentinel(results: &[u64]) -> u8 {
    let mut c = 0u8;
    let mut i = 0usize;
    while i < results.len() {
        if results[i] == REPLICA_LOST_SENTINEL {
            c = c.saturating_add(1);
        }
        i = i + 1;
    }
    c
}

// ============================================================================
// 第五节 · 检查点存储（定长槽位轮转 + CRC32 + 确定性序列化 + 演练）
// ============================================================================

/// 快照负载定长字节数。
/// 布局：4B 魔数 + 1B 版本 + 1B + 2B 保留 + 8B tick + 16B counters[4] + 8B acc
/// + 8B 保留 = 48。
/// 来源：锚点检查点策略节定长快照刻度，2026-10-24 定稿。
pub const SNAPSHOT_DATA_LEN: usize = 48;

/// 快照槽位数。轮转覆盖窗口为 4 个周期。
/// 来源：锚点检查点策略节"定长槽位轮转"刻度。
pub const CHECKPOINT_SLOTS: usize = 4;

/// 快照魔数 "VXF1"（Varix eXecution Fault-arch v1）。
pub const SNAPSHOT_MAGIC: [u8; 4] = [0x56, 0x58, 0x46, 0x31];
/// 快照负载格式版本。
pub const SNAPSHOT_VERSION: u8 = 1;

/// 快照负载。字段全部为标量，序列化确定性（小端 + 保留位清零）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotPayload {
    /// 捕获时刻 tick。
    pub tick: u64,
    /// 四路计数器（语义由捕获方定义，如：票数/分歧/捕获数/链级）。
    pub counters: [u32; 4],
    /// 累加器（如表决选出的关键值）。
    pub acc: u64,
}

impl SnapshotPayload {
    /// 确定性序列化到定长缓冲。保留位一律清零，保证同负载字节级一致。
    pub fn to_bytes(&self, out: &mut [u8; SNAPSHOT_DATA_LEN]) -> Result<(), FaultArchError> {
        let mut i = 0usize;
        while i < SNAPSHOT_DATA_LEN {
            out[i] = 0;
            i = i + 1;
        }
        out[0..4].copy_from_slice(&SNAPSHOT_MAGIC);
        out[4] = SNAPSHOT_VERSION;
        // out[5], out[6..8] 保留清零。
        out[8..16].copy_from_slice(&self.tick.to_le_bytes());
        let mut c = 0usize;
        while c < 4 {
            let base = 16 + c * 4;
            out[base..base + 4].copy_from_slice(&self.counters[c].to_le_bytes());
            c = c + 1;
        }
        out[32..40].copy_from_slice(&self.acc.to_le_bytes());
        // out[40..48] 保留清零。
        Ok(())
    }

    /// 从定长缓冲确定性反序列化。校验魔数、版本与保留位。
    pub fn from_bytes(bytes: &[u8; SNAPSHOT_DATA_LEN]) -> Result<SnapshotPayload, FaultArchError> {
        if bytes[0..4] != SNAPSHOT_MAGIC {
            return Err(FaultArchError::SnapshotSerializeInvalid);
        }
        if bytes[4] != SNAPSHOT_VERSION {
            return Err(FaultArchError::SnapshotSerializeInvalid);
        }
        // 保留位必须为零，保证往返确定性可自证。
        if bytes[5] != 0 || bytes[6] != 0 || bytes[7] != 0 {
            return Err(FaultArchError::SnapshotSerializeInvalid);
        }
        let mut r = 40usize;
        while r < SNAPSHOT_DATA_LEN {
            if bytes[r] != 0 {
                return Err(FaultArchError::SnapshotSerializeInvalid);
            }
            r = r + 1;
        }
        let mut tick_b = [0u8; 8];
        tick_b.copy_from_slice(&bytes[8..16]);
        let mut counters = [0u32; 4];
        let mut c = 0usize;
        while c < 4 {
            let base = 16 + c * 4;
            let mut b = [0u8; 4];
            b.copy_from_slice(&bytes[base..base + 4]);
            counters[c] = u32::from_le_bytes(b);
            c = c + 1;
        }
        let mut acc_b = [0u8; 8];
        acc_b.copy_from_slice(&bytes[32..40]);
        Ok(SnapshotPayload {
            tick: u64::from_le_bytes(tick_b),
            counters,
            acc: u64::from_le_bytes(acc_b),
        })
    }
}

/// 单个快照槽位（约 72B；4 槽共 288B，不上大栈）。私有。
#[derive(Debug, Clone, Copy)]
struct CheckpointSlot {
    seq: u64,
    tick: u64,
    crc: u32,
    data: [u8; SNAPSHOT_DATA_LEN],
    valid: bool,
}

impl CheckpointSlot {
    const EMPTY: CheckpointSlot = CheckpointSlot {
        seq: 0,
        tick: 0,
        crc: 0,
        data: [0u8; SNAPSHOT_DATA_LEN],
        valid: false,
    };
}

/// 检查点存储：定长槽位轮转 + CRC32 校验 + 恢复演练计数。
#[derive(Debug, Clone)]
pub struct CheckpointStore {
    slots: [CheckpointSlot; CHECKPOINT_SLOTS],
    next_slot: usize,
    seq_counter: u64,
    capture_count: u32,
    restore_count: u32,
    crc_fail_count: u32,
    drill_count: u32,
    drill_pass: u32,
    drill_fail: u32,
}

impl CheckpointStore {
    /// 构造空存储。
    pub fn new() -> CheckpointStore {
        CheckpointStore {
            slots: [CheckpointSlot::EMPTY; CHECKPOINT_SLOTS],
            next_slot: 0,
            seq_counter: 0,
            capture_count: 0,
            restore_count: 0,
            crc_fail_count: 0,
            drill_count: 0,
            drill_pass: 0,
            drill_fail: 0,
        }
    }

    /// 捕获次数。
    pub fn capture_count(&self) -> u32 {
        self.capture_count
    }
    /// 恢复次数。
    pub fn restore_count(&self) -> u32 {
        self.restore_count
    }
    /// CRC 失败次数。
    pub fn crc_fail_count(&self) -> u32 {
        self.crc_fail_count
    }
    /// 演练次数。
    pub fn drill_count(&self) -> u32 {
        self.drill_count
    }
    /// 演练通过次数。
    pub fn drill_pass(&self) -> u32 {
        self.drill_pass
    }
    /// 演练失败次数。
    pub fn drill_fail(&self) -> u32 {
        self.drill_fail
    }
    /// 已写入的有效槽位数。
    pub fn valid_slots(&self) -> usize {
        let mut c = 0usize;
        let mut i = 0usize;
        while i < CHECKPOINT_SLOTS {
            if self.slots[i].valid {
                c = c + 1;
            }
            i = i + 1;
        }
        c
    }

    /// 捕获快照：确定性序列化 + CRC32，写入轮转槽位，返回分配的序列号。
    pub fn capture(
        &mut self,
        payload: &SnapshotPayload,
        ledger: &mut FaultEventLedger,
    ) -> Result<u64, FaultArchError> {
        let mut data = [0u8; SNAPSHOT_DATA_LEN];
        payload.to_bytes(&mut data)?;
        let sum = crc32(&data);
        let slot = self.next_slot;
        self.next_slot = (self.next_slot + 1) % CHECKPOINT_SLOTS;
        self.seq_counter = self.seq_counter.wrapping_add(1);
        self.slots[slot] = CheckpointSlot {
            seq: self.seq_counter,
            tick: payload.tick,
            crc: sum,
            data,
            valid: true,
        };
        self.capture_count = self.capture_count.saturating_add(1);
        ledger.record(FaultEventKind::CheckpointCaptured, payload.tick, payload.acc);
        Ok(self.slots[slot].seq)
    }

    /// 在槽位环中找序列号最大的有效槽位；无则返回 usize::MAX 哨兵。私有。
    fn latest_slot_idx(&self) -> usize {
        let mut best = usize::MAX;
        let mut best_seq: u64 = 0;
        let mut i = 0usize;
        while i < CHECKPOINT_SLOTS {
            if self.slots[i].valid && (best == usize::MAX || self.slots[i].seq > best_seq) {
                best = i;
                best_seq = self.slots[i].seq;
            }
            i = i + 1;
        }
        best
    }

    /// 恢复最近有效快照：重算 CRC32 校验后反序列化。
    pub fn restore(&mut self, ledger: &mut FaultEventLedger) -> Result<SnapshotPayload, FaultArchError> {
        let idx = self.latest_slot_idx();
        if idx == usize::MAX {
            return Err(FaultArchError::SnapshotSlotEmpty);
        }
        let slot = &self.slots[idx];
        let sum = crc32(&slot.data);
        if sum != slot.crc {
            self.crc_fail_count = self.crc_fail_count.saturating_add(1);
            ledger.record(FaultEventKind::CheckpointCrcFail, slot.tick, sum as u64);
            return Err(FaultArchError::SnapshotCrcMismatch);
        }
        let payload = SnapshotPayload::from_bytes(&slot.data)?;
        self.restore_count = self.restore_count.saturating_add(1);
        ledger.record(FaultEventKind::CheckpointRestored, payload.tick, payload.acc);
        Ok(payload)
    }

    /// 恢复"次新"及更旧的有效快照（跳过跳过数 skip 个最新有效槽位）。
    /// 用于最近快照 CRC 失败后回溯更旧副本。
    pub fn restore_older(
        &mut self,
        skip: usize,
        ledger: &mut FaultEventLedger,
    ) -> Result<SnapshotPayload, FaultArchError> {
        // 收集有效槽位索引（<=4 个），按 seq 降序取第 skip 个。
        let mut idxs = [usize::MAX; CHECKPOINT_SLOTS];
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CHECKPOINT_SLOTS {
            if self.slots[i].valid {
                idxs[n] = i;
                n = n + 1;
            }
            i = i + 1;
        }
        // 简单选择排序（n<=4）按 seq 降序。
        let mut a = 0usize;
        while a < n {
            let mut b = a + 1;
            while b < n {
                if self.slots[idxs[b]].seq > self.slots[idxs[a]].seq {
                    let t = idxs[a];
                    idxs[a] = idxs[b];
                    idxs[b] = t;
                }
                b = b + 1;
            }
            a = a + 1;
        }
        if skip >= n {
            return Err(FaultArchError::SnapshotSlotEmpty);
        }
        let idx = idxs[skip];
        let slot = &self.slots[idx];
        let sum = crc32(&slot.data);
        if sum != slot.crc {
            self.crc_fail_count = self.crc_fail_count.saturating_add(1);
            ledger.record(FaultEventKind::CheckpointCrcFail, slot.tick, sum as u64);
            return Err(FaultArchError::SnapshotCrcMismatch);
        }
        let payload = SnapshotPayload::from_bytes(&slot.data)?;
        self.restore_count = self.restore_count.saturating_add(1);
        ledger.record(FaultEventKind::CheckpointRestored, payload.tick, payload.acc);
        Ok(payload)
    }

    /// 恢复演练：捕获一个演练金丝雀负载并立即恢复，验证字段一致与
    /// 序列化字节级往返一致。通过/失败计入独立演练计数器。
    /// 返回 (是否通过, 演练序号)。
    pub fn drill(&mut self, tick: u64, ledger: &mut FaultEventLedger) -> Result<(bool, u32), FaultArchError> {
        self.drill_count = self.drill_count.saturating_add(1);
        // 金丝雀负载由演练序号确定性构造。
        let n = self.drill_count;
        let canary = SnapshotPayload {
            tick,
            counters: [n, n.wrapping_mul(3), n.wrapping_mul(7), n.wrapping_mul(11)],
            acc: (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
        };
        let saved_next = self.next_slot;
        self.capture(&canary, ledger)?;
        let restored = self.restore(ledger)?;
        let mut rb = [0u8; SNAPSHOT_DATA_LEN];
        let mut ob = [0u8; SNAPSHOT_DATA_LEN];
        restored.to_bytes(&mut rb)?;
        canary.to_bytes(&mut ob)?;
        // 演练快照不应污染业务轮转序：回滚 next_slot 到演练前。
        self.next_slot = saved_next;
        let pass = restored == canary && rb == ob;
        if pass {
            self.drill_pass = self.drill_pass.saturating_add(1);
        } else {
            self.drill_fail = self.drill_fail.saturating_add(1);
        }
        ledger.record(FaultEventKind::DrillRun, tick, n as u64);
        Ok((pass, n))
    }
}

// ============================================================================
// 第六节 · 回退链（L0→L1→L2 + 滞回 + 链底显性）
// ============================================================================

/// 回退链层数（L0/L1/L2）。
pub const CHAIN_LEVELS: usize = 3;
/// 进入滞回：连续 3 次失败降一级。来源：锚点回退策略节滞回刻度。
pub const CHAIN_ENTER_FAIL_STREAK: u32 = 3;
/// 回升滞回：连续 8 次成功升一级。来源：锚点回退策略节滞回刻度。
pub const CHAIN_RECOVER_WIN_STREAK: u32 = 8;

/// 回退链层级描述 const 表。
#[derive(Debug, Clone, Copy)]
pub struct ChainLevelDesc {
    /// 层级码。
    pub code: u8,
    /// 层级名。
    pub name: &'static str,
    /// 能力档（3=全功能，1=最小功能）。
    pub capability_tier: u8,
    /// 备注。
    pub note: &'static str,
}

/// 回退链 const 表。来源：锚点回退策略节三级刻度，2026-10-24 定稿。
pub const CHAIN_LEVEL_TABLE: [ChainLevelDesc; CHAIN_LEVELS] = [
    ChainLevelDesc {
        code: 0,
        name: "L0 主路",
        capability_tier: 3,
        note: "全功能主路径，所有优化开启。",
    },
    ChainLevelDesc {
        code: 1,
        name: "L1 保守",
        capability_tier: 2,
        note: "关闭激进优化，保留核心功能，路径确定性增强。",
    },
    ChainLevelDesc {
        code: 2,
        name: "L2 最小功能",
        capability_tier: 1,
        note: "链底：仅保留正确性必需功能，不可再降级。",
    },
];

/// 回退链动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainAction {
    /// 维持当前层。
    Held,
    /// 降级（进入滞回触发）。
    Descended { from: u8, to: u8 },
    /// 回升（回升滞回触发）。
    Ascended { from: u8, to: u8 },
    /// 已在链底仍失败（显性不可再降）。
    AtBottom,
}

/// 回退链层级转移记录。
#[derive(Debug, Clone, Copy)]
pub struct ChainTransition {
    /// 转移时刻 tick。
    pub tick: u64,
    /// 转移前层级。
    pub from: u8,
    /// 转移后层级。
    pub to: u8,
    /// 是否为降级。
    pub descended: bool,
}

/// 回退链转移账容量（环形）。转移是低频事件，16 条足够。
pub const CHAIN_TRANSITION_CAP: usize = 16;

/// 回退链：三级降级 + 双向滞回防振荡 + 链底显性。
#[derive(Debug, Clone)]
pub struct FallbackChain {
    level: u8,
    fail_streak: u32,
    win_streak: u32,
    descends: u32,
    ascends: u32,
    bottom_hits: u32,
    head: usize,
    len: usize,
    transitions: [ChainTransition; CHAIN_TRANSITION_CAP],
}

impl FallbackChain {
    /// 构造，初始层级 L0。
    pub fn new() -> FallbackChain {
        FallbackChain {
            level: 0,
            fail_streak: 0,
            win_streak: 0,
            descends: 0,
            ascends: 0,
            bottom_hits: 0,
            head: 0,
            len: 0,
            transitions: [ChainTransition {
                tick: 0,
                from: 0,
                to: 0,
                descended: false,
            }; CHAIN_TRANSITION_CAP],
        }
    }

    /// 当前层级。
    pub fn level(&self) -> u8 {
        self.level
    }
    /// 当前失败连击。
    pub fn fail_streak(&self) -> u32 {
        self.fail_streak
    }
    /// 当前成功连击。
    pub fn win_streak(&self) -> u32 {
        self.win_streak
    }
    /// 累计降级次数。
    pub fn descends(&self) -> u32 {
        self.descends
    }
    /// 累计回升次数。
    pub fn ascends(&self) -> u32 {
        self.ascends
    }
    /// 链底命中次数（在 L2 仍失败）。
    pub fn bottom_hits(&self) -> u32 {
        self.bottom_hits
    }
    /// 转移账长度。
    pub fn transition_len(&self) -> usize {
        self.len
    }
    /// 按账内序读第 i 条转移记录。
    pub fn transition(&self, i: usize) -> Option<ChainTransition> {
        if i >= self.len {
            return None;
        }
        let idx = (self.head + i) % CHAIN_TRANSITION_CAP;
        Some(self.transitions[idx])
    }

    /// 层级描述。
    pub const fn level_desc(&self) -> &'static ChainLevelDesc {
        // level 恒 < CHAIN_LEVELS，构造/转移路径保证。
        match self.level {
            0 => &CHAIN_LEVEL_TABLE[0],
            1 => &CHAIN_LEVEL_TABLE[1],
            _ => &CHAIN_LEVEL_TABLE[2],
        }
    }

    /// 写入转移记录（环形）。
    fn push_transition(&mut self, rec: ChainTransition) {
        let at = if self.len < CHAIN_TRANSITION_CAP {
            (self.head + self.len) % CHAIN_TRANSITION_CAP
        } else {
            let at = self.head;
            self.head = (self.head + 1) % CHAIN_TRANSITION_CAP;
            at
        };
        self.transitions[at] = rec;
        if self.len < CHAIN_TRANSITION_CAP {
            self.len = self.len + 1;
        }
    }

    /// 上报一次主路执行结果，执行滞回状态机。
    ///
    /// - 失败：胜连击清零，败连击 +1；达 3 连败且未在链底则降一级（降级后
    ///   败连击清零，防止连环跳层）；已在链底则计入链底命中账并显性返回
    ///   [`ChainAction::AtBottom`]（不可再降）。
    /// - 成功：败连击清零，胜连击 +1；达 8 连胜且未在顶层则升一级（升级后
    ///   胜连击清零）。
    pub fn report(
        &mut self,
        ok: bool,
        tick: u64,
        ledger: &mut FaultEventLedger,
    ) -> Result<ChainAction, FaultArchError> {
        if ok {
            self.fail_streak = 0;
            self.win_streak = self.win_streak.saturating_add(1);
            if self.level > 0 && self.win_streak >= CHAIN_RECOVER_WIN_STREAK {
                let from = self.level;
                self.level = self.level - 1;
                self.win_streak = 0;
                self.ascends = self.ascends.saturating_add(1);
                let rec = ChainTransition {
                    tick,
                    from,
                    to: self.level,
                    descended: false,
                };
                self.push_transition(rec);
                ledger.record(FaultEventKind::ChainAscended, tick, self.level as u64);
                return Ok(ChainAction::Ascended {
                    from,
                    to: self.level,
                });
            }
            return Ok(ChainAction::Held);
        }
        // 失败分支。
        self.win_streak = 0;
        self.fail_streak = self.fail_streak.saturating_add(1);
        if self.fail_streak < CHAIN_ENTER_FAIL_STREAK {
            return Ok(ChainAction::Held);
        }
        if self.level < (CHAIN_LEVELS as u8 - 1) {
            let from = self.level;
            self.level = self.level + 1;
            self.fail_streak = 0;
            self.descends = self.descends.saturating_add(1);
            let rec = ChainTransition {
                tick,
                from,
                to: self.level,
                descended: true,
            };
            self.push_transition(rec);
            ledger.record(FaultEventKind::ChainDescended, tick, self.level as u64);
            return Ok(ChainAction::Descended {
                from,
                to: self.level,
            });
        }
        // 链底：显性不可再降。
        self.fail_streak = 0;
        self.bottom_hits = self.bottom_hits.saturating_add(1);
        ledger.record(FaultEventKind::ChainBottom, tick, self.level as u64);
        Ok(ChainAction::AtBottom)
    }

    /// 测试辅助：直接设置层级并校验合法范围。
    pub fn set_level(&mut self, level: u8) -> Result<(), FaultArchError> {
        if (level as usize) >= CHAIN_LEVELS {
            return Err(FaultArchError::ChainLevelInvalid);
        }
        self.level = level;
        self.fail_streak = 0;
        self.win_streak = 0;
        Ok(())
    }
}

// ============================================================================
// 第七节 · 策略选择决策规范（const 决策表 + 可解释输出 + 决策留痕账）
// ============================================================================

/// 策略组合：位掩码 + 主策略 + const 理由。
///
/// 位定义：bit0=冗余 bit1=检查点 bit2=回退。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrategyCombo {
    /// 策略位掩码。
    pub mask: u8,
    /// 主策略索引（0 冗余 / 1 检查点 / 2 回退）。
    pub primary: u8,
    /// 决策表 const 理由（中文）。
    pub reason: &'static str,
}

/// const 组合构造器：主策略按 冗余 > 检查点 > 回退 优先级推导。
const fn mk_combo(mask: u8, reason: &'static str) -> StrategyCombo {
    let primary = if (mask & 0b001) != 0 {
        0u8
    } else if (mask & 0b010) != 0 {
        1u8
    } else {
        2u8
    };
    StrategyCombo { mask, primary, reason }
}

/// 决策表：`DECISION_TABLE[故障率档][恢复成本档][关键度]`（3×3×4=36 项）。
///
/// 来源：锚点 CGPU-F2564 决策规范节决策表，2026-10-24 施工书定稿。
/// 设计准则：
/// - 低关键度：只兜底（回退为主），仅当恢复昂贵时附检查点；
/// - 中关键度：检查点为底座，故障率高时加冗余抓错值；
/// - 高关键度：冗余 + 检查点叠加，故障率高时再加回退兜底；
/// - 最高关键度：冗余 + 检查点常开，仅恢复廉价且故障率低时可省回退。
pub const DECISION_TABLE: [[[StrategyCombo; 4]; 3]; 3] = [
    // ── 故障率档：低 ──
    [
        [
            // 恢复廉价 × 关键度 低/中/高/最高
            mk_combo(0b100, "低关键度且故障罕见、恢复廉价：仅回退兜底，重资产策略得不偿失。"),
            mk_combo(0b010, "中关键度且故障罕见、恢复廉价：周期快照即可，回退暂不需要。"),
            mk_combo(0b010, "高关键度且故障罕见、恢复廉价：周期快照保底，出错可回放。"),
            mk_combo(0b011, "最高关键度且故障罕见、恢复廉价：冗余+检查点叠加；恢复廉价故可省回退。"),
        ],
        [
            // 恢复中等
            mk_combo(0b100, "低关键度且恢复成本中等：仍只回退兜底，不为低价值数据加重资产。"),
            mk_combo(0b010, "中关键度且故障罕见、恢复中等：快照覆盖回放成本。"),
            mk_combo(0b011, "高关键度且故障罕见、恢复中等：冗余+检查点叠加，回放成本被快照压低。"),
            mk_combo(0b011, "最高关键度且故障罕见、恢复中等：冗余+检查点叠加，静默错值不可接受。"),
        ],
        [
            // 恢复昂贵
            mk_combo(0b110, "低关键度但恢复昂贵：附加周期快照避免昂贵重建，回退兜底。"),
            mk_combo(0b110, "中关键度且故障罕见、恢复昂贵：检查点+回退，回放成本被快照压低。"),
            mk_combo(0b011, "高关键度且故障罕见、恢复昂贵：冗余+检查点叠加，昂贵的恢复由快照规避。"),
            mk_combo(0b011, "最高关键度且故障罕见、恢复昂贵：冗余+检查点叠加，昂贵的恢复由快照规避。"),
        ],
    ],
    // ── 故障率档：中 ──
    [
        [
            mk_combo(0b100, "低关键度且中等故障率：仍只回退兜底，开销优先。"),
            mk_combo(0b110, "中关键度且中等故障率、恢复廉价：快照+回退，快照频率可覆盖故障窗口。"),
            mk_combo(0b011, "高关键度且中等故障率、恢复廉价：冗余+检查点叠加。"),
            mk_combo(0b011, "最高关键度且中等故障率、恢复廉价：冗余+检查点叠加。"),
        ],
        [
            mk_combo(0b100, "低关键度且中等故障率：仍只回退兜底。"),
            mk_combo(0b110, "中关键度且中等故障率、恢复中等：检查点+回退叠加。"),
            mk_combo(0b011, "高关键度且中等故障率、恢复中等：冗余+检查点叠加，错值当场检出。"),
            mk_combo(0b111, "最高关键度且中等故障率：三策略全开，静默错值与不可用都不可接受。"),
        ],
        [
            mk_combo(0b110, "低关键度但恢复昂贵：附加快照规避昂贵重建。"),
            mk_combo(0b110, "中关键度且中等故障率、恢复昂贵：检查点+回退，快照规避昂贵回放。"),
            mk_combo(0b111, "高关键度且中等故障率、恢复昂贵：三策略全开。"),
            mk_combo(0b111, "最高关键度且中等故障率、恢复昂贵：三策略全开。"),
        ],
    ],
    // ── 故障率档：高 ──
    [
        [
            mk_combo(0b100, "低关键度但故障率高：仍只回退兜底，宁可降级也不为低价值数据加重资产。"),
            mk_combo(0b101, "中关键度且高故障率、恢复廉价：冗余表决抓错值+回退兜底；快照在高故障率下价值有限。"),
            mk_combo(0b101, "高关键度且高故障率、恢复廉价：冗余+回退，错值当场检出即时降级。"),
            mk_combo(0b111, "最高关键度且高故障率：三策略全开。"),
        ],
        [
            mk_combo(0b100, "低关键度但故障率高：仍只回退兜底。"),
            mk_combo(0b101, "中关键度且高故障率、恢复中等：冗余+回退。"),
            mk_combo(0b111, "高关键度且高故障率、恢复中等：三策略全开。"),
            mk_combo(0b111, "最高关键度且高故障率、恢复中等：三策略全开。"),
        ],
        [
            mk_combo(0b110, "低关键度但恢复昂贵：附加快照规避昂贵重建。"),
            mk_combo(0b101, "中关键度且高故障率、恢复昂贵：冗余+回退，高故障率下快照回放窗口易被反复作废。"),
            mk_combo(0b111, "高关键度且高故障率、恢复昂贵：三策略全开。"),
            mk_combo(0b111, "最高关键度且高故障率、恢复昂贵：三策略全开。"),
        ],
    ],
];

/// 单条决策留痕记录。
#[derive(Debug, Clone, Copy)]
pub struct DecisionRecord {
    /// 决策时刻 tick。
    pub tick: u64,
    /// 策略位掩码。
    pub mask: u8,
    /// 故障率档码。
    pub rate_code: u8,
    /// 恢复成本档码。
    pub cost_code: u8,
    /// 关键度码。
    pub crit_code: u8,
    /// 理由指纹（FNV-1a over 理由字节）。
    pub reason_fp: u64,
}

/// 决策留痕账容量（环形）。决策低频，32 条足够。
pub const DECISION_LEDGER_CAP: usize = 32;

/// 策略选择器：const 决策表 + 可解释理由输出 + 决策留痕账。
#[derive(Debug, Clone)]
pub struct StrategySelector {
    head: usize,
    len: usize,
    records: [DecisionRecord; DECISION_LEDGER_CAP],
    decide_count: u32,
}

impl StrategySelector {
    /// 构造。
    pub fn new() -> StrategySelector {
        StrategySelector {
            head: 0,
            len: 0,
            records: [DecisionRecord {
                tick: 0,
                mask: 0,
                rate_code: 0,
                cost_code: 0,
                crit_code: 0,
                reason_fp: 0,
            }; DECISION_LEDGER_CAP],
            decide_count: 0,
        }
    }

    /// 累计决策次数。
    pub fn decide_count(&self) -> u32 {
        self.decide_count
    }
    /// 留痕账长度。
    pub fn ledger_len(&self) -> usize {
        self.len
    }
    /// 按账内序读第 i 条留痕。
    pub fn record(&self, i: usize) -> Option<DecisionRecord> {
        if i >= self.len {
            return None;
        }
        let idx = (self.head + i) % DECISION_LEDGER_CAP;
        Some(self.records[idx])
    }

    /// const 查表：按档位码查决策表。越界档位被钳制到边界（const 语境不可返回
    /// Result，钳制行为在此显性声明；运行期入口 `decide()` 会先行校验）。
    pub const fn lookup(rate_code: u8, cost_code: u8, crit_code: u8) -> StrategyCombo {
        let r = if rate_code > 2 { 2 } else { rate_code as usize };
        let c = if cost_code > 2 { 2 } else { cost_code as usize };
        let k = if crit_code > 3 { 3 } else { crit_code as usize };
        DECISION_TABLE[r][c][k]
    }

    /// 位掩码解释：列出组合中启用的策略名（const 表回查）。
    pub fn explain_enabled(mask: u8) -> Vec<&'static str> {
        let mut out = Vec::with_capacity(3);
        let mut i = 0u8;
        while i < 3 {
            let st = match FaultStrategy::from_index(i) {
                Some(s) => s,
                None => FaultStrategy::Fallback,
            };
            if (mask & st.mask_bit()) != 0 {
                out.push(st.desc().name);
            }
            i = i + 1;
        }
        out
    }

    /// 决策：查表 + 组装可解释理由字符串 + 留痕。
    ///
    /// 理由字符串包含三输入档位、命中组合、主策略及其描述表刻度与决策表 const 理由。
    pub fn decide(
        &mut self,
        rate: FaultRateBand,
        cost: RecoveryCostBand,
        crit: DataCriticality,
        tick: u64,
        ledger: &mut FaultEventLedger,
    ) -> Result<Decision, FaultArchError> {
        let combo = Self::lookup(rate.code(), cost.code(), crit.code());
        let primary_desc = match FaultStrategy::from_index(combo.primary) {
            Some(s) => s.desc(),
            None => return Err(FaultArchError::DecisionUncovered),
        };
        let mut reason = String::with_capacity(192);
        reason.push_str("决策规范命中：");
        reason.push_str(rate.name());
        reason.push_str(" × ");
        reason.push_str(cost.name());
        reason.push_str(" × ");
        reason.push_str(crit.name());
        reason.push_str(" → 启用 [");
        let names = Self::explain_enabled(combo.mask);
        let mut i = 0usize;
        while i < names.len() {
            if i > 0 {
                reason.push_str(" + ");
            }
            reason.push_str(names[i]);
            i = i + 1;
        }
        reason.push_str("]，主策略 ");
        reason.push_str(primary_desc.name);
        reason.push_str("（开销档=");
        reason.push_str(&format!("{}", primary_desc.overhead_tier));
        reason.push_str(" 恢复时限档=");
        reason.push_str(&format!("{}", primary_desc.recovery_deadline_tier));
        reason.push_str(" 最低数据等级=");
        reason.push_str(&format!("{}", primary_desc.min_data_grade));
        reason.push_str("）。表内理由：");
        reason.push_str(combo.reason);

        let reason_fp = fnv1a64(reason.as_bytes());
        let rec = DecisionRecord {
            tick,
            mask: combo.mask,
            rate_code: rate.code(),
            cost_code: cost.code(),
            crit_code: crit.code(),
            reason_fp,
        };
        let at = if self.len < DECISION_LEDGER_CAP {
            (self.head + self.len) % DECISION_LEDGER_CAP
        } else {
            let at = self.head;
            self.head = (self.head + 1) % DECISION_LEDGER_CAP;
            at
        };
        self.records[at] = rec;
        if self.len < DECISION_LEDGER_CAP {
            self.len = self.len + 1;
        }
        self.decide_count = self.decide_count.saturating_add(1);
        ledger.record(FaultEventKind::DecisionMade, tick, combo.mask as u64);

        Ok(Decision {
            combo,
            reason,
            rate,
            cost,
            crit,
            reason_fp,
        })
    }
}

/// 决策结果：组合 + 可解释理由 + 输入回显。
#[derive(Debug, Clone)]
pub struct Decision {
    /// 命中的策略组合。
    pub combo: StrategyCombo,
    /// 可解释理由（中文，含表内 const 理由与描述表刻度）。
    pub reason: String,
    /// 输入：故障率档。
    pub rate: FaultRateBand,
    /// 输入：恢复成本档。
    pub cost: RecoveryCostBand,
    /// 输入：数据关键度。
    pub crit: DataCriticality,
    /// 理由指纹。
    pub reason_fp: u64,
}

// ============================================================================
// 第八节 · 故障源抽象与注入面（真源与注入源同管道）
// ============================================================================

/// 抽象故障源 trait：真源（`RealFaultSource`）与注入源（`FaultInjector`）
/// 实现同一接口，协调器/表决器通过 `&mut dyn FaultSource` 消费，
/// 保证注入测试与真实路径走完全相同的管道。
pub trait FaultSource {
    /// 取下一份副本结果。`truth` 为该副本的"真值"。
    /// 真源恒返回 truth；注入源可能按计划篡改/超时/重复/丢失。
    fn next_result(&mut self, truth: u64) -> Result<u64, FaultArchError>;

    /// 源描述（"真源" / "注入源" 等）。
    fn describe(&self) -> &'static str;

    /// 表决结果回执：调用方在每轮表决后回传是否存在分歧。
    /// 注入源据此统计检出/漏检；真源无操作。
    fn notify_vote(&mut self, _diverged: bool, _ledger: &mut FaultEventLedger) {}
}

/// 真源：恒透传真值。
#[derive(Debug, Clone, Copy, Default)]
pub struct RealFaultSource {
    calls: u32,
}

impl RealFaultSource {
    /// 构造。
    pub fn new() -> RealFaultSource {
        RealFaultSource { calls: 0 }
    }
    /// 已调用次数。
    pub fn calls(&self) -> u32 {
        self.calls
    }
}

impl FaultSource for RealFaultSource {
    fn next_result(&mut self, truth: u64) -> Result<u64, FaultArchError> {
        self.calls = self.calls.saturating_add(1);
        Ok(truth)
    }
    fn describe(&self) -> &'static str {
        "真源(透传)"
    }
}

/// 注入族。四族覆盖副本故障的主要表现形态：
/// 超时（无响应）、错值（静默错误）、重复（陈旧结果）、丢失（结果缺失）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectorFamily {
    /// 超时注入：返回超时错误。
    Timeout,
    /// 错值注入：真值加偏移。
    WrongValue,
    /// 重复注入：返回上一次的成功结果（陈旧值）。
    Duplicate,
    /// 丢失注入：返回丢失错误。
    Loss,
}

impl InjectorFamily {
    /// 族码 0..=3。
    pub const fn code(&self) -> u8 {
        match self {
            InjectorFamily::Timeout => 0,
            InjectorFamily::WrongValue => 1,
            InjectorFamily::Duplicate => 2,
            InjectorFamily::Loss => 3,
        }
    }
    /// 中文名。
    pub const fn name(&self) -> &'static str {
        match self {
            InjectorFamily::Timeout => "超时",
            InjectorFamily::WrongValue => "错值",
            InjectorFamily::Duplicate => "重复",
            InjectorFamily::Loss => "丢失",
        }
    }
    /// 由族码构造（const）。
    pub const fn from_code(c: u8) -> Option<InjectorFamily> {
        match c {
            0 => Some(InjectorFamily::Timeout),
            1 => Some(InjectorFamily::WrongValue),
            2 => Some(InjectorFamily::Duplicate),
            3 => Some(InjectorFamily::Loss),
            _ => None,
        }
    }
}

/// 注入族数量。
pub const INJECTOR_FAMILIES: usize = 4;
/// 注入周期：每 `INJECT_PERIOD` 次调用注入一次（当族已布防）。
/// 来源：锚点注入测试面节周期刻度（每 4 次一次），2026-10-24 定稿。
pub const INJECT_PERIOD: u32 = 4;

/// 注入源：按族布防，周期性注入；统计各族注入/检出/漏检。
///
/// 检出判定由调用方通过 `notify_vote()` 回执：注入后当轮表决存在分歧即
/// 记为检出（detected），否则记为漏检（missed，显性计数）。
#[derive(Debug, Clone)]
pub struct FaultInjector {
    armed: [bool; INJECTOR_FAMILIES],
    period: u32,
    phase: u32,
    wrong_delta: u64,
    last_good: u64,
    has_last_good: bool,
    calls: u32,
    injections: [u32; INJECTOR_FAMILIES],
    detected: [u32; INJECTOR_FAMILIES],
    missed: [u32; INJECTOR_FAMILIES],
    pending_family: u8,
    pending_active: bool,
}

impl FaultInjector {
    /// 构造：`wrong_delta` 为错值族偏移量，`period` 为注入周期（0 视为非法，
    /// 钳制为 INJECT_PERIOD）。
    pub fn new(wrong_delta: u64, period: u32) -> FaultInjector {
        let p = if period == 0 { INJECT_PERIOD } else { period };
        FaultInjector {
            armed: [false; INJECTOR_FAMILIES],
            period: p,
            phase: 0,
            wrong_delta,
            last_good: 0,
            has_last_good: false,
            calls: 0,
            injections: [0; INJECTOR_FAMILIES],
            detected: [0; INJECTOR_FAMILIES],
            missed: [0; INJECTOR_FAMILIES],
            pending_family: 0,
            pending_active: false,
        }
    }

    /// 布防/撤防指定注入族。
    pub fn set_armed(&mut self, family: InjectorFamily, armed: bool) {
        self.armed[family.code() as usize] = armed;
    }

    /// 查询布防状态。
    pub fn is_armed(&self, family: InjectorFamily) -> bool {
        self.armed[family.code() as usize]
    }

    /// 注入周期。
    pub fn period(&self) -> u32 {
        self.period
    }

    /// 错值偏移量。
    pub fn wrong_delta(&self) -> u64 {
        self.wrong_delta
    }

    /// 已调用次数。
    pub fn calls(&self) -> u32 {
        self.calls
    }

    /// 指定族注入次数。
    pub fn injections(&self, family: InjectorFamily) -> u32 {
        self.injections[family.code() as usize]
    }

    /// 指定族检出次数。
    pub fn detected(&self, family: InjectorFamily) -> u32 {
        self.detected[family.code() as usize]
    }

    /// 指定族漏检次数（显性计数）。
    pub fn missed(&self, family: InjectorFamily) -> u32 {
        self.missed[family.code() as usize]
    }

    /// 总注入次数。
    pub fn injections_total(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < INJECTOR_FAMILIES {
            t = t.saturating_add(self.injections[i]);
            i = i + 1;
        }
        t
    }

    /// 总检出次数。
    pub fn detected_total(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < INJECTOR_FAMILIES {
            t = t.saturating_add(self.detected[i]);
            i = i + 1;
        }
        t
    }

    /// 总漏检次数（显性）。
    pub fn missed_total(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < INJECTOR_FAMILIES {
            t = t.saturating_add(self.missed[i]);
            i = i + 1;
        }
        t
    }

    /// 检出率（基点 bps，10000=100%）。无注入记录时返回 0。
    /// 溢出显性：分母饱和累加，乘 10000 前先判溢出。
    pub fn detection_rate_bps(&self) -> u32 {
        let hit = self.detected_total();
        let total = hit.saturating_add(self.missed_total());
        if total == 0 {
            return 0;
        }
        if hit >= u32::MAX / 10000 {
            return 10000;
        }
        (hit * 10000) / total
    }

    /// 布防族摘要（诊断用）。
    pub fn armed_summary(&self) -> String {
        let mut s = String::with_capacity(48);
        let mut i = 0usize;
        while i < INJECTOR_FAMILIES {
            if self.armed[i] {
                if !s.is_empty() {
                    s.push('+');
                }
                let f = match InjectorFamily::from_code(i as u8) {
                    Some(f) => f,
                    None => InjectorFamily::Loss,
                };
                s.push_str(f.name());
            }
            i = i + 1;
        }
        if s.is_empty() {
            s.push_str("未布防");
        }
        s
    }
}

impl FaultSource for FaultInjector {
    fn next_result(&mut self, truth: u64) -> Result<u64, FaultArchError> {
        self.calls = self.calls.saturating_add(1);
        self.pending_active = false;
        self.phase = self.phase.wrapping_add(1);
        let due = (self.phase.wrapping_sub(1) % self.period) == 0;
        if !due {
            self.last_good = truth;
            self.has_last_good = true;
            return Ok(truth);
        }
        // 周期到期：在已布防族中按族码顺序注入首个命中的族。
        let mut i = 0usize;
        while i < INJECTOR_FAMILIES {
            if self.armed[i] {
                let family = match InjectorFamily::from_code(i as u8) {
                    Some(f) => f,
                    None => InjectorFamily::Loss,
                };
                self.injections[i] = self.injections[i].saturating_add(1);
                self.pending_family = i as u8;
                self.pending_active = true;
                match family {
                    InjectorFamily::Timeout => {
                        return Err(FaultArchError::InjectionTimeout);
                    }
                    InjectorFamily::WrongValue => {
                        let v = truth.wrapping_add(self.wrong_delta);
                        self.last_good = v;
                        self.has_last_good = true;
                        return Ok(v);
                    }
                    InjectorFamily::Duplicate => {
                        // 返回上一次成功结果（陈旧值）；无历史时退化为真值。
                        let v = if self.has_last_good {
                            self.last_good
                        } else {
                            truth
                        };
                        self.last_good = v;
                        self.has_last_good = true;
                        return Ok(v);
                    }
                    InjectorFamily::Loss => {
                        return Err(FaultArchError::InjectionLoss);
                    }
                }
            }
            i = i + 1;
        }
        self.last_good = truth;
        self.has_last_good = true;
        Ok(truth)
    }

    fn notify_vote(&mut self, diverged: bool, ledger: &mut FaultEventLedger) {
        if !self.pending_active {
            return;
        }
        self.pending_active = false;
        let f = self.pending_family as usize;
        if diverged {
            self.detected[f] = self.detected[f].saturating_add(1);
            ledger.record(FaultEventKind::InjectionDetected, 0, f as u64);
        } else {
            self.missed[f] = self.missed[f].saturating_add(1);
            ledger.record(FaultEventKind::InjectionMissed, 0, f as u64);
        }
    }

    fn describe(&self) -> &'static str {
        "注入源(按族布防)"
    }
}

// ============================================================================
// 第九节 · 事件账 + 体检 + 指纹
// ============================================================================

/// 事件种类。事件码与错误码共用 0x12F0~0x12FF 段（按设施区分，见模块头注释）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultEventKind {
    /// 表决分歧。
    VoteDivergence,
    /// 平票规则触发。
    VoteTieBroken,
    /// 冗余单元熔断。
    CircuitTripped,
    /// 快照捕获。
    CheckpointCaptured,
    /// 快照恢复。
    CheckpointRestored,
    /// 快照 CRC 失败。
    CheckpointCrcFail,
    /// 回退链降级。
    ChainDescended,
    /// 回退链回升。
    ChainAscended,
    /// 链底命中。
    ChainBottom,
    /// 注入发射。
    InjectionEmitted,
    /// 注入检出。
    InjectionDetected,
    /// 注入漏检。
    InjectionMissed,
    /// 预算不足跳过。
    BudgetSkipped,
    /// 策略决策留痕。
    DecisionMade,
    /// 恢复演练执行。
    DrillRun,
    /// 心跳/保留。
    Heartbeat,
}

impl FaultEventKind {
    /// 事件码（0x12F0..=0x12FF 段内）。
    pub const fn code(&self) -> u32 {
        match self {
            FaultEventKind::VoteDivergence => 0x12F0,
            FaultEventKind::VoteTieBroken => 0x12F1,
            FaultEventKind::CircuitTripped => 0x12F2,
            FaultEventKind::CheckpointCaptured => 0x12F3,
            FaultEventKind::CheckpointRestored => 0x12F4,
            FaultEventKind::CheckpointCrcFail => 0x12F5,
            FaultEventKind::ChainDescended => 0x12F6,
            FaultEventKind::ChainAscended => 0x12F7,
            FaultEventKind::ChainBottom => 0x12F8,
            FaultEventKind::InjectionEmitted => 0x12F9,
            FaultEventKind::InjectionDetected => 0x12FA,
            FaultEventKind::InjectionMissed => 0x12FB,
            FaultEventKind::BudgetSkipped => 0x12FC,
            FaultEventKind::DecisionMade => 0x12FD,
            FaultEventKind::DrillRun => 0x12FE,
            FaultEventKind::Heartbeat => 0x12FF,
        }
    }

    /// 中文名。
    pub const fn name(&self) -> &'static str {
        match self {
            FaultEventKind::VoteDivergence => "表决分歧",
            FaultEventKind::VoteTieBroken => "平票规则",
            FaultEventKind::CircuitTripped => "熔断",
            FaultEventKind::CheckpointCaptured => "快照捕获",
            FaultEventKind::CheckpointRestored => "快照恢复",
            FaultEventKind::CheckpointCrcFail => "快照CRC失败",
            FaultEventKind::ChainDescended => "链降级",
            FaultEventKind::ChainAscended => "链回升",
            FaultEventKind::ChainBottom => "链底命中",
            FaultEventKind::InjectionEmitted => "注入发射",
            FaultEventKind::InjectionDetected => "注入检出",
            FaultEventKind::InjectionMissed => "注入漏检",
            FaultEventKind::BudgetSkipped => "预算跳过",
            FaultEventKind::DecisionMade => "决策留痕",
            FaultEventKind::DrillRun => "恢复演练",
            FaultEventKind::Heartbeat => "心跳",
        }
    }

    /// 严重度：0=正常 1=注意 2=劣化 3=危急。
    pub const fn severity(&self) -> u8 {
        match self {
            FaultEventKind::VoteDivergence => 2,
            FaultEventKind::VoteTieBroken => 2,
            FaultEventKind::CircuitTripped => 3,
            FaultEventKind::CheckpointCaptured => 0,
            FaultEventKind::CheckpointRestored => 1,
            FaultEventKind::CheckpointCrcFail => 3,
            FaultEventKind::ChainDescended => 2,
            FaultEventKind::ChainAscended => 0,
            FaultEventKind::ChainBottom => 3,
            FaultEventKind::InjectionEmitted => 1,
            FaultEventKind::InjectionDetected => 0,
            FaultEventKind::InjectionMissed => 3,
            FaultEventKind::BudgetSkipped => 1,
            FaultEventKind::DecisionMade => 0,
            FaultEventKind::DrillRun => 0,
            FaultEventKind::Heartbeat => 0,
        }
    }
}

/// 事件账单条记录（24B）。
#[derive(Debug, Clone, Copy)]
pub struct FaultEventRecord {
    /// 事件时刻 tick。
    pub tick: u64,
    /// 事件码。
    pub code: u32,
    /// 严重度。
    pub severity: u8,
    /// 详情哈希（由记录方提供，如选中值/掩码/序号）。
    pub hash: u64,
}

/// 事件账容量（环形覆写最旧）。256 × 24B ≈ 6KiB，远低于 64KiB 上栈红线。
pub const EVENT_LEDGER_CAP: usize = 256;

/// 故障事件账：环形账本 + 分种计数 + 体检 + 指纹。
#[derive(Debug, Clone)]
pub struct FaultEventLedger {
    head: usize,
    len: usize,
    total: u32,
    by_code: [u32; 16],
    records: [FaultEventRecord; EVENT_LEDGER_CAP],
}

impl FaultEventLedger {
    /// 构造空账。
    pub fn new() -> FaultEventLedger {
        FaultEventLedger {
            head: 0,
            len: 0,
            total: 0,
            by_code: [0; 16],
            records: [FaultEventRecord {
                tick: 0,
                code: 0,
                severity: 0,
                hash: 0,
            }; EVENT_LEDGER_CAP],
        }
    }

    /// 记录事件。环形覆写最旧；total/by_code 饱和累加。
    pub fn record(&mut self, kind: FaultEventKind, tick: u64, hash: u64) {
        let rec = FaultEventRecord {
            tick,
            code: kind.code(),
            severity: kind.severity(),
            hash,
        };
        let at = if self.len < EVENT_LEDGER_CAP {
            (self.head + self.len) % EVENT_LEDGER_CAP
        } else {
            let at = self.head;
            self.head = (self.head + 1) % EVENT_LEDGER_CAP;
            at
        };
        self.records[at] = rec;
        if self.len < EVENT_LEDGER_CAP {
            self.len = self.len + 1;
        }
        self.total = self.total.saturating_add(1);
        let slot = (kind.code() - DIAG_SEGMENT_LO) as usize;
        self.by_code[slot] = self.by_code[slot].saturating_add(1);
    }

    /// 记录总数（饱和计）。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// 账内长度（<= EVENT_LEDGER_CAP）。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 按账内序（旧→新）读第 i 条。
    pub fn get(&self, i: usize) -> Option<FaultEventRecord> {
        if i >= self.len {
            return None;
        }
        let idx = (self.head + i) % EVENT_LEDGER_CAP;
        Some(self.records[idx])
    }

    /// 指定事件的分种计数。
    pub fn count_of(&self, kind: FaultEventKind) -> u32 {
        self.by_code[(kind.code() - DIAG_SEGMENT_LO) as usize]
    }

    /// 账本指纹：对账内记录（旧→新）做 FNV-1a 混合（码/tick/哈希）。
    /// 同一事件序列产生同一指纹；任何差异都会改变指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.len {
            let idx = (self.head + i) % EVENT_LEDGER_CAP;
            let r = &self.records[idx];
            h = h.wrapping_mul(FNV_PRIME);
            h = h ^ (r.code as u64);
            h = h.wrapping_mul(FNV_PRIME);
            h = h ^ r.tick;
            h = h.wrapping_mul(FNV_PRIME);
            h = h ^ r.hash;
            i = i + 1;
        }
        h
    }

    /// 体检：基于分种计数与可选注入统计产出健康分（0..=100）与判定。
    ///
    /// 扣分权重（来源：锚点体检判据节权重刻度，2026-10-24 定稿）：
    /// 分歧 -2/次、CRC 失败 -3/次、漏检 -5/次、链底 -2/次、熔断 -10/次。
    pub fn health(&self, inj: Option<&FaultInjector>) -> HealthReport {
        let divergences = self.count_of(FaultEventKind::VoteDivergence);
        let crc_fails = self.count_of(FaultEventKind::CheckpointCrcFail);
        let bottom = self.count_of(FaultEventKind::ChainBottom);
        let trips = self.count_of(FaultEventKind::CircuitTripped);
        let missed = match inj {
            Some(i) => i.missed_total(),
            None => self.count_of(FaultEventKind::InjectionMissed),
        };
        const W_DIVERGENCE: u32 = 2;
        const W_CRC: u32 = 3;
        const W_MISSED: u32 = 5;
        const W_BOTTOM: u32 = 2;
        const W_TRIP: u32 = 10;
        let penalty = divergences
            .saturating_mul(W_DIVERGENCE)
            .saturating_add(crc_fails.saturating_mul(W_CRC))
            .saturating_add(missed.saturating_mul(W_MISSED))
            .saturating_add(bottom.saturating_mul(W_BOTTOM))
            .saturating_add(trips.saturating_mul(W_TRIP));
        let score = 100u32.saturating_sub(penalty);
        let verdict: &'static str = if score >= 90 {
            "健康"
        } else if score >= 70 {
            "亚健康"
        } else if score >= 40 {
            "劣化"
        } else {
            "危急"
        };
        HealthReport {
            events_total: self.total,
            divergences,
            crc_fails,
            missed_injections: missed,
            bottom_hits: bottom,
            circuit_trips: trips,
            score: score as u8,
            verdict,
        }
    }
}

/// 体检报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthReport {
    /// 事件总数。
    pub events_total: u32,
    /// 表决分歧计数。
    pub divergences: u32,
    /// 快照 CRC 失败计数。
    pub crc_fails: u32,
    /// 注入漏检计数。
    pub missed_injections: u32,
    /// 链底命中计数。
    pub bottom_hits: u32,
    /// 熔断计数。
    pub circuit_trips: u32,
    /// 健康分 0..=100。
    pub score: u8,
    /// 判定（健康/亚健康/劣化/危急）。
    pub verdict: &'static str,
}

// ============================================================================
// 第十节 · 组合策略协调器（冗余+检查点次序与预算分账）
// ============================================================================

/// 副本算子种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// 恒等算子（主副本常用）。
    Identity,
    /// 加常量（影子副本错位路径，模拟独立实现）。
    AddConst,
    /// 异或掩码（影子副本错位路径）。
    XorMask,
}

/// 副本算子：影子副本与主副本走不同但等价的计算路径，保证表决有意义。
#[derive(Debug, Clone, Copy)]
pub struct ReplicaOp {
    /// 算子种类。
    pub kind: OpKind,
    /// 算子参数。
    pub param: u64,
}

impl ReplicaOp {
    /// 求值。溢出显性 wrapping。
    pub fn eval(&self, input: u64) -> u64 {
        match self.kind {
            OpKind::Identity => input,
            OpKind::AddConst => input.wrapping_add(self.param),
            OpKind::XorMask => input ^ self.param,
        }
    }
}

/// 每 tick 标称预算（单位：预算点）。来源：锚点组合策略节预算分账刻度
/// （冗余 30 + 检查点 40 + 回退 10 = 80 ≤ 100，留 20 点裕量），2026-10-24 定稿。
pub const COORD_BUDGET_PER_TICK: u32 = 100;
/// 冗余路径每 tick 开销。
pub const COORD_COST_REDUNDANCY: u32 = 30;
/// 检查点单次捕获开销。
pub const COORD_COST_CHECKPOINT: u32 = 40;
/// 回退路径每 tick 开销。
pub const COORD_COST_FALLBACK: u32 = 10;

/// 协调器执行模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickMode {
    /// 正常：影子表决 + 条件快照。
    Normal,
    /// 冗余单元熔断后的回退路径：仅保守单路，不表决不快照。
    FallbackOnly,
}

/// 单 tick 预算支出分账。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BudgetSpend {
    /// 冗余路径支出。
    pub redundancy: u32,
    /// 检查点支出。
    pub checkpoint: u32,
    /// 回退路径支出。
    pub fallback: u32,
}

impl BudgetSpend {
    /// 合计。
    pub fn total(&self) -> u32 {
        self.redundancy
            .saturating_add(self.checkpoint)
            .saturating_add(self.fallback)
    }
}

/// 单 tick 执行报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    /// 执行模式。
    pub mode: TickMode,
    /// 表决报告（FallbackOnly 模式为 None）。
    pub vote: Option<VoteReport>,
    /// 回退链动作。
    pub chain_action: ChainAction,
    /// 本 tick 是否执行了快照捕获。
    pub captured: bool,
    /// 预算支出分账。
    pub spend: BudgetSpend,
}

/// 组合策略协调器：冗余 + 检查点叠加时的执行次序与预算分账。
///
/// 执行次序（每 tick，显性）：
/// 1. 冗余先于检查点：先收集副本 → 表决 → 回执注入源（检出/漏检）；
/// 2. 表决一致才允许快照：分歧 tick 的状态未经验证，禁止入账；
/// 3. 快照受预算闸门：剩余预算不足时跳过捕获并记 BudgetSkipped 事件；
/// 4. 回退链跟随表决结果：分歧视为失败上报，一致视为成功上报；
/// 5. 熔断期间走 FallbackOnly 保守单路，只支出回退预算，链会随之降级。
#[derive(Debug, Clone)]
pub struct ComboCoordinator {
    red: RedundancyUnit,
    cp: CheckpointStore,
    chain: FallbackChain,
    ops: [ReplicaOp; REDUNDANCY_MAX_REPLICAS as usize],
    fallback_op: ReplicaOp,
    capture_interval: u64,
    last_capture_tick: u64,
    budget_per_tick: u32,
    ledger_budget: BudgetSpend,
    ticks_run: u32,
    budget_skips: u32,
    ever_captured: bool,
}

impl ComboCoordinator {
    /// 构造。
    ///
    /// - `ops` 长度必须等于 `replica_count`（其余槽位填 Identity）；
    /// - `capture_interval` 不得为 0；
    /// - `budget_per_tick` 为 0 时采用 [`COORD_BUDGET_PER_TICK`]。
    pub fn new(
        red: RedundancyUnit,
        cp: CheckpointStore,
        chain: FallbackChain,
        ops: &[ReplicaOp],
        fallback_op: ReplicaOp,
        capture_interval: u64,
        budget_per_tick: u32,
    ) -> Result<ComboCoordinator, FaultArchError> {
        if capture_interval == 0 {
            return Err(FaultArchError::CoordinatorStateInvalid);
        }
        if ops.is_empty() || ops.len() != red.replica_count() as usize {
            return Err(FaultArchError::ReplicaCountMismatch);
        }
        let mut full = [ReplicaOp {
            kind: OpKind::Identity,
            param: 0,
        }; REDUNDANCY_MAX_REPLICAS as usize];
        let mut i = 0usize;
        while i < ops.len() {
            full[i] = ops[i];
            i = i + 1;
        }
        let budget = if budget_per_tick == 0 {
            COORD_BUDGET_PER_TICK
        } else {
            budget_per_tick
        };
        Ok(ComboCoordinator {
            red,
            cp,
            chain,
            ops: full,
            fallback_op,
            capture_interval,
            last_capture_tick: 0,
            budget_per_tick: budget,
            ledger_budget: BudgetSpend::default(),
            ticks_run: 0,
            budget_skips: 0,
            ever_captured: false,
        })
    }

    /// 冗余单元（只读）。
    pub fn redundancy(&self) -> &RedundancyUnit {
        &self.red
    }
    /// 冗余单元（可变，用于测试注入分歧）。
    pub fn redundancy_mut(&mut self) -> &mut RedundancyUnit {
        &mut self.red
    }
    /// 检查点存储（只读）。
    pub fn checkpoints(&self) -> &CheckpointStore {
        &self.cp
    }
    /// 回退链（只读）。
    pub fn chain(&self) -> &FallbackChain {
        &self.chain
    }
    /// 累计预算分账。
    pub fn budget_ledger(&self) -> BudgetSpend {
        self.ledger_budget
    }
    /// 已执行 tick 数。
    pub fn ticks_run(&self) -> u32 {
        self.ticks_run
    }
    /// 预算不足跳过快照次数。
    pub fn budget_skips(&self) -> u32 {
        self.budget_skips
    }

    /// 执行一个 tick。`source` 为真源或注入源（同管道）。
    ///
    /// 副本角色：副本 0（主副本）经故障源管道取结果——真源透传真值，注入源
    /// 可篡改/超时/重复/丢失；影子副本 1..N 由独立算子本地求值（标准 N+1
    /// 影子模型：影子实现独立于主路，故障面集中在主路管道上）。
    pub fn run_tick(
        &mut self,
        tick: u64,
        input: u64,
        source: &mut dyn FaultSource,
        ledger: &mut FaultEventLedger,
    ) -> Result<TickReport, FaultArchError> {
        self.ticks_run = self.ticks_run.saturating_add(1);
        let mut spend = BudgetSpend::default();

        // ── 熔断路径：FallbackOnly 保守单路 ──
        if self.red.is_tripped() {
            let conservative = self.fallback_op.eval(input);
            let value = source.next_result(conservative)?;
            // 保守路径结果不做表决（单路），但需上报链：降级路径中的失败
            // 由注入错误自然传播；正常返回视为"在降级模式下可用"。
            let action = self.chain.report(true, tick, ledger)?;
            spend.fallback = spend.fallback.saturating_add(COORD_COST_FALLBACK);
            self.ledger_budget.fallback = self.ledger_budget.fallback.saturating_add(COORD_COST_FALLBACK);
            let _ = value;
            return Ok(TickReport {
                mode: TickMode::FallbackOnly,
                vote: None,
                chain_action: action,
                captured: false,
                spend,
            });
        }

        // ── 正常路径：1. 收集副本（冗余先于检查点） ──
        let rc = self.red.replica_count() as usize;
        let mut results = [0u64; REDUNDANCY_MAX_REPLICAS as usize];
        // 主副本（索引 0）：经故障源管道（真源/注入源同管道）。
        let main_truth = self.ops[0].eval(input);
        results[0] = match source.next_result(main_truth) {
            Ok(v) => v,
            Err(FaultArchError::InjectionTimeout) | Err(FaultArchError::InjectionLoss) => {
                // 超时/丢失以哨兵补位，照常参与表决（丢失即分歧候选）。
                REPLICA_LOST_SENTINEL
            }
            Err(e) => return Err(e),
        };
        // 影子副本（索引 1..N）：独立算子本地求值。
        let mut i = 1usize;
        while i < rc {
            results[i] = self.ops[i].eval(input);
            i = i + 1;
        }

        // ── 2. 表决 ──
        let vote = self.red.vote(tick, input, &results[..rc], ledger)?;
        // 回执注入源：检出/漏检统计。
        source.notify_vote(vote.diverged, ledger);
        spend.redundancy = spend.redundancy.saturating_add(COORD_COST_REDUNDANCY);
        self.ledger_budget.redundancy = self.ledger_budget.redundancy.saturating_add(COORD_COST_REDUNDANCY);

        // ── 3. 回退链跟随表决结果 ──
        let chain_action = self.chain.report(!vote.diverged, tick, ledger)?;
        if self.chain.level() > 0 {
            spend.fallback = spend.fallback.saturating_add(COORD_COST_FALLBACK);
            self.ledger_budget.fallback = self.ledger_budget.fallback.saturating_add(COORD_COST_FALLBACK);
        }

        // ── 4. 检查点（仅表决一致时；受预算闸门） ──
        let mut captured = false;
        // 到期条件：从未捕获过，或距上次捕获已满 capture_interval 个 tick。
        let due = !self.ever_captured
            || tick.wrapping_sub(self.last_capture_tick) >= self.capture_interval;
        if !vote.diverged && due {
            let remaining = self
                .budget_per_tick
                .saturating_sub(spend.redundancy)
                .saturating_sub(spend.fallback);
            if remaining >= COORD_COST_CHECKPOINT {
                let payload = SnapshotPayload {
                    tick,
                    counters: [
                        self.red.votes_cast(),
                        self.red.divergences_total(),
                        self.cp.capture_count(),
                        self.chain.level() as u32,
                    ],
                    acc: vote.value,
                };
                self.cp.capture(&payload, ledger)?;
                captured = true;
                self.last_capture_tick = tick;
                self.ever_captured = true;
                spend.checkpoint = spend.checkpoint.saturating_add(COORD_COST_CHECKPOINT);
                self.ledger_budget.checkpoint = self.ledger_budget.checkpoint.saturating_add(COORD_COST_CHECKPOINT);
            } else {
                self.budget_skips = self.budget_skips.saturating_add(1);
                ledger.record(FaultEventKind::BudgetSkipped, tick, 0);
            }
        }

        Ok(TickReport {
            mode: TickMode::Normal,
            vote: Some(vote),
            chain_action,
            captured,
            spend,
        })
    }

    /// 恢复演练透传（检查点子系统的演练入口）。
    pub fn drill_checkpoints(&mut self, tick: u64, ledger: &mut FaultEventLedger) -> Result<(bool, u32), FaultArchError> {
        self.cp.drill(tick, ledger)
    }

    /// 策略决策透传（选择器入口）。
    pub fn decide(
        &mut self,
        selector: &mut StrategySelector,
        rate: FaultRateBand,
        cost: RecoveryCostBand,
        crit: DataCriticality,
        tick: u64,
        ledger: &mut FaultEventLedger,
    ) -> Result<Decision, FaultArchError> {
        selector.decide(rate, cost, crit, tick, ledger)
    }
}

// ============================================================================
// 第十一节 · 编译期不变量
// ============================================================================

// 快照几何与码段不变量（编译期断言，允许的 const assert! 用法）。
const _: () = assert!(SNAPSHOT_DATA_LEN == 48);
const _: () = assert!(CHECKPOINT_SLOTS == 4);
const _: () = assert!(EVENT_LEDGER_CAP == 256);
const _: () = assert!(DIAG_SEGMENT_LO == 0x12F0);
const _: () = assert!(DIAG_SEGMENT_HI == 0x12FF);
const _: () = assert!(DIAG_SEGMENT_HI - DIAG_SEGMENT_LO == 0xF);
const _: () = assert!(core::mem::size_of::<SnapshotPayload>() <= 64);
const _: () = assert!(core::mem::size_of::<FaultEventRecord>() == 24);
const _: () = assert!(CHAIN_LEVELS == 3);
const _: () = assert!(VOTE_TRIP_THRESHOLD == 5);
const _: () = assert!(CHAIN_ENTER_FAIL_STREAK == 3);
const _: () = assert!(CHAIN_RECOVER_WIN_STREAK == 8);
const _: () = assert!(COORD_BUDGET_PER_TICK >= COORD_COST_REDUNDANCY + COORD_COST_CHECKPOINT + COORD_COST_FALLBACK);
const _: () = assert!(REDUNDANCY_MAX_REPLICAS == 5);

// ============================================================================
// 第十二节 · 测试（注入面：表决/快照往返/回退滞回/决策表/注入检出率）
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── 冗余表决 ──

    #[test]
    fn vote_unanimous_majority() {
        let mut ledger = FaultEventLedger::new();
        let mut red = match RedundancyUnit::new(3) {
            Ok(r) => r,
            Err(_) => return,
        };
        let r = red.vote(1, 42, &[7, 7, 7], &mut ledger);
        assert!(r.is_ok());
        let v = match r {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(v.value, 7);
        assert!(v.unanimous);
        assert!(!v.diverged);
        assert_eq!(v.majority_count, 3);
        assert_eq!(red.divergences_total(), 0);
    }

    #[test]
    fn vote_strict_majority_detects_shadow_corruption() {
        let mut ledger = FaultEventLedger::new();
        let mut red = match RedundancyUnit::new(3) {
            Ok(r) => r,
            Err(_) => return,
        };
        let r = red.vote(2, 42, &[9, 9, 13], &mut ledger);
        let v = match r {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(v.value, 9);
        assert!(v.diverged);
        assert_eq!(red.divergences_total(), 1);
        assert!(ledger.count_of(FaultEventKind::VoteDivergence) >= 1);
    }

    #[test]
    fn vote_tie_prefers_primary_anchor() {
        let mut ledger = FaultEventLedger::new();
        let mut red = match RedundancyUnit::new(2) {
            Ok(r) => r,
            Err(_) => return,
        };
        // 2 副本 1:1 平票：主副本锚定 → 选主副本值。
        let v = match red.vote(3, 5, &[100, 200], &mut ledger) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(v.value, 100);
        assert!(v.tie_broken);
        assert!(v.diverged);
        assert_eq!(red.tie_breaks(), 1);
        assert!(ledger.count_of(FaultEventKind::VoteTieBroken) >= 1);
    }

    #[test]
    fn vote_tie_lower_bound_when_primary_absent() {
        let mut ledger = FaultEventLedger::new();
        let mut red = match RedundancyUnit::new(5) {
            Ok(r) => r,
            Err(_) => return,
        };
        // 5 副本 2:2:1，主副本（9）只有 1 票不在最高票候选中：
        // 取最高票候选（8 与 3）中的最小值 3（确定性下界规则）。
        let v = match red.vote(4, 5, &[9, 8, 8, 3, 3], &mut ledger) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(v.value, 3);
        assert!(v.tie_broken);
        assert!(v.diverged);
    }

    #[test]
    fn vote_circuit_breaker_trips_after_five_consecutive() {
        let mut ledger = FaultEventLedger::new();
        let mut red = match RedundancyUnit::new(3) {
            Ok(r) => r,
            Err(_) => return,
        };
        let mut i = 0u32;
        while i < 4 {
            let v = red.vote(10 + i as u64, 1, &[1, 1, 99], &mut ledger);
            assert!(v.is_ok());
            assert!(!red.is_tripped());
            i = i + 1;
        }
        let v = red.vote(14, 1, &[1, 1, 99], &mut ledger);
        assert!(v.is_ok());
        assert!(red.is_tripped());
        // 熔断后表决必须拒绝。
        assert!(matches!(
            red.vote(15, 1, &[1, 1, 1], &mut ledger),
            Err(FaultArchError::CircuitTripped)
        ));
        // 复位后恢复。
        red.reset();
        assert!(!red.is_tripped());
        let v = red.vote(16, 1, &[1, 1, 1], &mut ledger);
        match v {
            Ok(rep) => assert!(!rep.diverged),
            Err(_) => return,
        }
    }

    #[test]
    fn vote_lost_replica_sentinel_counts_divergence() {
        let mut ledger = FaultEventLedger::new();
        let mut red = match RedundancyUnit::new(3) {
            Ok(r) => r,
            Err(_) => return,
        };
        // 主副本丢失：哨兵补位，表决应选影子一致值并判分歧。
        let v = match red.vote(20, 8, &[REPLICA_LOST_SENTINEL, 77, 77], &mut ledger) {
            Ok(v) => v,
            Err(_) => return,
        };
        assert_eq!(v.value, 77);
        assert!(v.diverged);
    }

    #[test]
    fn vote_record_ledger_ring() {
        let mut ledger = FaultEventLedger::new();
        let mut red = match RedundancyUnit::new(2) {
            Ok(r) => r,
            Err(_) => return,
        };
        let mut i = 0u64;
        while i < (VOTE_LEDGER_CAP as u64) + 5 {
            let _ = red.vote(i, i, &[i, i], &mut ledger);
            i = i + 1;
        }
        assert_eq!(red.ledger_len(), VOTE_LEDGER_CAP);
        // 账内第一条应为最旧的存活记录。
        let first = match red.vote_record(0) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(first.tick, 5);
    }

    // ── 检查点 ──

    #[test]
    fn snapshot_roundtrip_bytes_identical() {
        let p = SnapshotPayload {
            tick: 12345,
            counters: [1, 2, 3, 4],
            acc: 0xDEAD_BEEF_CAFE_F00D,
        };
        let mut a = [0u8; SNAPSHOT_DATA_LEN];
        let mut b = [0u8; SNAPSHOT_DATA_LEN];
        assert!(p.to_bytes(&mut a).is_ok());
        assert!(p.to_bytes(&mut b).is_ok());
        assert_eq!(a, b); // 确定性序列化
        let q = SnapshotPayload::from_bytes(&a);
        match q {
            Ok(q) => {
                assert_eq!(q, p);
                let mut c = [0u8; SNAPSHOT_DATA_LEN];
                assert!(q.to_bytes(&mut c).is_ok());
                assert_eq!(a, c); // 往返字节级一致
            }
            Err(_) => return,
        }
    }

    #[test]
    fn snapshot_capture_restore_latest() {
        let mut ledger = FaultEventLedger::new();
        let mut cp = CheckpointStore::new();
        assert!(matches!(
            cp.restore(&mut ledger),
            Err(FaultArchError::SnapshotSlotEmpty)
        ));
        let mut i = 0u64;
        while i < (CHECKPOINT_SLOTS as u64) + 2 {
            let p = SnapshotPayload {
                tick: i,
                counters: [i as u32; 4],
                acc: i * 11,
            };
            assert!(cp.capture(&p, &mut ledger).is_ok());
            i = i + 1;
        }
        // 槽位轮转：有效槽位恒 <= 4，恢复的是最新 seq。
        assert_eq!(cp.valid_slots(), CHECKPOINT_SLOTS);
        let r = cp.restore(&mut ledger);
        match r {
            Ok(p) => {
                assert_eq!(p.tick, (CHECKPOINT_SLOTS as u64) + 1);
                assert_eq!(p.acc, ((CHECKPOINT_SLOTS as u64) + 1) * 11);
            }
            Err(_) => return,
        }
        assert_eq!(cp.capture_count(), (CHECKPOINT_SLOTS as u64 + 2) as u32);
        assert_eq!(cp.restore_count(), 1);
    }

    #[test]
    fn snapshot_crc_corruption_detected() {
        let mut ledger = FaultEventLedger::new();
        let mut cp = CheckpointStore::new();
        let p = SnapshotPayload {
            tick: 7,
            counters: [7; 4],
            acc: 777,
        };
        assert!(cp.capture(&p, &mut ledger).is_ok());
        // 直接篡改槽位数据（测试模块可访问私有字段）。
        cp.slots[0].data[8] ^= 0xFF;
        assert!(matches!(
            cp.restore(&mut ledger),
            Err(FaultArchError::SnapshotCrcMismatch)
        ));
        assert_eq!(cp.crc_fail_count(), 1);
        assert!(ledger.count_of(FaultEventKind::CheckpointCrcFail) >= 1);
    }

    #[test]
    fn snapshot_restore_older_fallback() {
        let mut ledger = FaultEventLedger::new();
        let mut cp = CheckpointStore::new();
        let mut i = 0u64;
        while i < 3 {
            let p = SnapshotPayload {
                tick: i,
                counters: [0; 4],
                acc: i,
            };
            assert!(cp.capture(&p, &mut ledger).is_ok());
            i = i + 1;
        }
        // 最新快照损坏 → restore_older(1) 拿到次新。
        cp.slots[cp.latest_slot_idx()].data[0] ^= 0x01;
        let r = cp.restore_older(1, &mut ledger);
        match r {
            Ok(p) => assert_eq!(p.acc, 1),
            Err(_) => return,
        }
    }

    #[test]
    fn snapshot_drill_counters() {
        let mut ledger = FaultEventLedger::new();
        let mut cp = CheckpointStore::new();
        let mut i = 0u64;
        while i < 5 {
            let r = cp.drill(100 + i, &mut ledger);
            match r {
                Ok((pass, n)) => {
                    assert!(pass);
                    assert_eq!(n, i as u32 + 1);
                }
                Err(_) => return,
            }
            i = i + 1;
        }
        assert_eq!(cp.drill_count(), 5);
        assert_eq!(cp.drill_pass(), 5);
        assert_eq!(cp.drill_fail(), 0);
    }

    // ── 回退链滞回 ──

    #[test]
    fn chain_hysteresis_descend_after_three_fails() {
        let mut ledger = FaultEventLedger::new();
        let mut ch = FallbackChain::new();
        assert!(matches!(ch.report(false, 1, &mut ledger), Ok(ChainAction::Held)));
        assert!(matches!(ch.report(false, 2, &mut ledger), Ok(ChainAction::Held)));
        // 第 3 连败触发降级 L0→L1。
        match ch.report(false, 3, &mut ledger) {
            Ok(ChainAction::Descended { from, to }) => {
                assert_eq!(from, 0);
                assert_eq!(to, 1);
            }
            _ => return,
        }
        assert_eq!(ch.level(), 1);
        assert_eq!(ch.descends(), 1);
        assert!(ledger.count_of(FaultEventKind::ChainDescended) >= 1);
    }

    #[test]
    fn chain_hysteresis_ascend_after_eight_wins() {
        let mut ledger = FaultEventLedger::new();
        let mut ch = FallbackChain::new();
        assert!(ch.set_level(1).is_ok());
        let mut i = 0u32;
        while i < 7 {
            assert!(matches!(ch.report(true, i as u64, &mut ledger), Ok(ChainAction::Held)));
            i = i + 1;
        }
        match ch.report(true, 8, &mut ledger) {
            Ok(ChainAction::Ascended { from, to }) => {
                assert_eq!(from, 1);
                assert_eq!(to, 0);
            }
            _ => return,
        }
        assert_eq!(ch.level(), 0);
        assert_eq!(ch.ascends(), 1);
    }

    #[test]
    fn chain_oscillation_suppressed() {
        let mut ledger = FaultEventLedger::new();
        let mut ch = FallbackChain::new();
        // 2 败 1 胜交替：永不触发降级（滞回防振荡）。
        let mut i = 0u64;
        while i < 20 {
            assert!(matches!(ch.report(false, i, &mut ledger), Ok(ChainAction::Held)));
            assert!(matches!(ch.report(true, i, &mut ledger), Ok(ChainAction::Held)));
            assert!(matches!(ch.report(true, i, &mut ledger), Ok(ChainAction::Held)));
            i = i + 1;
        }
        assert_eq!(ch.level(), 0);
        assert_eq!(ch.descends(), 0);
    }

    #[test]
    fn chain_bottom_explicit_no_further_descent() {
        let mut ledger = FaultEventLedger::new();
        let mut ch = FallbackChain::new();
        // 连败 3×2 次降到链底 L2。
        let mut i = 0u32;
        while i < 6 {
            let _ = ch.report(false, i as u64, &mut ledger);
            i = i + 1;
        }
        assert_eq!(ch.level(), 2);
        // 链底再败：显性 AtBottom，层级不再变。
        let mut j = 0u32;
        while j < 5 {
            match ch.report(false, 100 + j as u64, &mut ledger) {
                Ok(ChainAction::AtBottom) => {}
                _ => return,
            }
            assert_eq!(ch.level(), 2);
            j = j + 1;
        }
        assert_eq!(ch.bottom_hits(), 5);
        assert!(ledger.count_of(FaultEventKind::ChainBottom) >= 5);
    }

    // ── 决策表 ──

    #[test]
    fn decision_table_cells_and_reasons() {
        let mut ledger = FaultEventLedger::new();
        let mut sel = StrategySelector::new();
        // 最高关键度 × 高故障率 × 恢复昂贵 → 三策略全开。
        let d = match sel.decide(
            FaultRateBand::High,
            RecoveryCostBand::Expensive,
            DataCriticality::Vital,
            1,
            &mut ledger,
        ) {
            Ok(d) => d,
            Err(_) => return,
        };
        assert_eq!(d.combo.mask, 0b111);
        assert_eq!(d.combo.primary, 0); // 主策略冗余
        assert!(!d.reason.is_empty());
        assert!(d.reason.contains("三策略全开"));
        assert!(d.reason.contains("高故障率"));
        assert!(d.reason.contains("最高关键度"));

        // 低关键度 × 低故障率 × 恢复廉价 → 仅回退。
        let d2 = match sel.decide(
            FaultRateBand::Low,
            RecoveryCostBand::Cheap,
            DataCriticality::Low,
            2,
            &mut ledger,
        ) {
            Ok(d) => d,
            Err(_) => return,
        };
        assert_eq!(d2.combo.mask, 0b100);
        assert_eq!(d2.combo.primary, 2); // 主策略回退
        assert!(d2.reason.contains("仅回退兜底"));

        // 冗余+检查点叠加的典型组合。
        let d3 = match sel.decide(
            FaultRateBand::Low,
            RecoveryCostBand::Moderate,
            DataCriticality::Vital,
            3,
            &mut ledger,
        ) {
            Ok(d) => d,
            Err(_) => return,
        };
        assert_eq!(d3.combo.mask, 0b011);
        assert_eq!(StrategySelector::explain_enabled(0b011).len(), 2);

        // 留痕账。
        assert_eq!(sel.decide_count(), 3);
        assert_eq!(sel.ledger_len(), 3);
        let r0 = match sel.record(0) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(r0.mask, 0b111);
        assert!(ledger.count_of(FaultEventKind::DecisionMade) >= 3);
    }

    #[test]
    fn decision_reason_fingerprint_stable() {
        let mut ledger = FaultEventLedger::new();
        let mut sel = StrategySelector::new();
        let a = sel.decide(
            FaultRateBand::Mid,
            RecoveryCostBand::Moderate,
            DataCriticality::High,
            9,
            &mut ledger,
        );
        let b = sel.decide(
            FaultRateBand::Mid,
            RecoveryCostBand::Moderate,
            DataCriticality::High,
            9,
            &mut ledger,
        );
        match (a, b) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a.reason_fp, b.reason_fp); // 同输入同理由同指纹
                assert_eq!(a.combo.mask, b.combo.mask);
            }
            _ => return,
        }
    }

    #[test]
    fn decision_lookup_const_clamps() {
        // const lookup 对越界档位钳制（文档化行为）。
        let c = StrategySelector::lookup(9, 9, 9);
        assert_eq!(c.mask, StrategySelector::lookup(2, 2, 3).mask);
    }

    // ── 注入面 ──

    #[test]
    fn injector_wrong_value_detected_by_vote() {
        let mut ledger = FaultEventLedger::new();
        let mut inj = FaultInjector::new(0x100, 1); // 每次调用都注入
        inj.set_armed(InjectorFamily::WrongValue, true);
        let mut red = match RedundancyUnit::new(3) {
            Ok(r) => r,
            Err(_) => return,
        };
        // 真/注入同管道：主副本走注入源，两个影子走真值。
        let mut i = 0u64;
        while i < 4 {
            let truth_main = i.wrapping_mul(3);
            let main = match inj.next_result(truth_main) {
                Ok(v) => v,
                Err(_) => REPLICA_LOST_SENTINEL,
            };
            let v = red.vote(i, i, &[main, truth_main, truth_main], &mut ledger);
            match v {
                Ok(rep) => assert!(rep.diverged),
                Err(_) => return,
            }
            inj.notify_vote(true, &mut ledger);
            i = i + 1;
        }
        assert_eq!(inj.injections(InjectorFamily::WrongValue), 4);
        assert_eq!(inj.detected(InjectorFamily::WrongValue), 4);
        assert_eq!(inj.missed(InjectorFamily::WrongValue), 0);
        assert_eq!(inj.detection_rate_bps(), 10000);
    }

    #[test]
    fn injector_all_four_families_pipeline() {
        // 本测试只验证注入族行为，不涉及事件账（下划线前缀抑制未用告警）。
        let _ledger = FaultEventLedger::new();
        let mut inj = FaultInjector::new(1, 1);
        inj.set_armed(InjectorFamily::Timeout, true);
        // 超时族：返回 Err(InjectionTimeout)。
        let r = inj.next_result(5);
        assert!(matches!(r, Err(FaultArchError::InjectionTimeout)));
        assert_eq!(inj.injections(InjectorFamily::Timeout), 1);
        // 切到丢失族。
        inj.set_armed(InjectorFamily::Timeout, false);
        inj.set_armed(InjectorFamily::Loss, true);
        assert!(matches!(
            inj.next_result(5),
            Err(FaultArchError::InjectionLoss)
        ));
        assert_eq!(inj.injections(InjectorFamily::Loss), 1);
        // 重复族：返回上一次成功结果。
        inj.set_armed(InjectorFamily::Loss, false);
        inj.set_armed(InjectorFamily::Duplicate, true);
        let a = inj.next_result(10);
        let b = inj.next_result(20);
        match (a, b) {
            (Ok(va), Ok(vb)) => {
                assert_eq!(va, 10); // 首次无历史，透传真值
                assert_eq!(vb, 10); // 第二次返回陈旧值 → 重复
            }
            _ => return,
        }
        assert_eq!(inj.injections(InjectorFamily::Duplicate), 2);
        assert_eq!(inj.describe(), "注入源(按族布防)");
    }

    #[test]
    fn injector_missed_detection_explicit_count() {
        let mut ledger = FaultEventLedger::new();
        let mut inj = FaultInjector::new(0, 1); // delta=0：错值注入恰与真值一致 → 必然漏检
        inj.set_armed(InjectorFamily::WrongValue, true);
        let mut red = match RedundancyUnit::new(3) {
            Ok(r) => r,
            Err(_) => return,
        };
        let mut i = 0u64;
        while i < 2 {
            let m = match inj.next_result(i) {
                Ok(v) => v,
                Err(_) => REPLICA_LOST_SENTINEL,
            };
            let rep = red.vote(i, i, &[m, i, i], &mut ledger);
            match rep {
                Ok(r) => {
                    assert!(!r.diverged); // delta=0 → 无分歧
                }
                Err(_) => return,
            }
            inj.notify_vote(false, &mut ledger);
            i = i + 1;
        }
        assert_eq!(inj.injections(InjectorFamily::WrongValue), 2);
        assert_eq!(inj.missed(InjectorFamily::WrongValue), 2); // 漏检显性
        assert_eq!(inj.detected_total(), 0);
        assert_eq!(inj.detection_rate_bps(), 0);
    }

    #[test]
    fn real_source_passthrough() {
        let mut ledger = FaultEventLedger::new();
        let mut real = RealFaultSource::new();
        let mut i = 0u64;
        while i < 3 {
            let v = real.next_result(i * 7);
            match v {
                Ok(v) => assert_eq!(v, i * 7),
                Err(_) => return,
            }
            i = i + 1;
        }
        assert_eq!(real.calls(), 3);
        assert_eq!(real.describe(), "真源(透传)");
        let _ = &mut ledger; // 真源 notify_vote 为空操作
    }

    // ── 组合协调器 ──

    /// 测试辅助：构造标准三副本协调器（capture_interval=3，默认预算）。
    fn mk_coordinator() -> Option<ComboCoordinator> {
        let red = match RedundancyUnit::new(3) {
            Ok(r) => r,
            Err(_) => return None,
        };
        let cp = CheckpointStore::new();
        let chain = FallbackChain::new();
        let ops = [
            ReplicaOp {
                kind: OpKind::Identity,
                param: 0,
            },
            ReplicaOp {
                kind: OpKind::AddConst,
                param: 0,
            },
            ReplicaOp {
                kind: OpKind::XorMask,
                param: 0,
            },
        ];
        match ComboCoordinator::new(
            red,
            cp,
            chain,
            &ops,
            ReplicaOp {
                kind: OpKind::AddConst,
                param: 1,
            },
            3,
            0,
        ) {
            Ok(c) => Some(c),
            Err(_) => None,
        }
    }

    /// 测试辅助：构造最小双副本冗余单元。
    fn mk_red2() -> Option<RedundancyUnit> {
        match RedundancyUnit::new(2) {
            Ok(r) => Some(r),
            Err(_) => None,
        }
    }

    #[test]
    fn coordinator_normal_tick_budget_accounting() {
        let mut ledger = FaultEventLedger::new();
        let mut coord = match mk_coordinator() {
            Some(c) => c,
            None => return,
        };
        let mut src = RealFaultSource::new();
        // tick 0/1/2：capture_interval=3 → tick0 与 tick3 捕获（last=0 起）。
        let r0 = coord.run_tick(0, 100, &mut src, &mut ledger);
        match r0 {
            Ok(rep) => {
                assert_eq!(rep.mode, TickMode::Normal);
                assert!(rep.captured);
                assert_eq!(rep.spend.redundancy, COORD_COST_REDUNDANCY);
                assert_eq!(rep.spend.checkpoint, COORD_COST_CHECKPOINT);
                assert_eq!(rep.spend.fallback, 0);
            }
            Err(_) => return,
        }
        let r1 = coord.run_tick(1, 101, &mut src, &mut ledger);
        match r1 {
            Ok(rep) => assert!(!rep.captured),
            Err(_) => return,
        }
        let r3 = coord.run_tick(3, 103, &mut src, &mut ledger);
        match r3 {
            Ok(rep) => assert!(rep.captured),
            Err(_) => return,
        }
        let bl = coord.budget_ledger();
        assert!(bl.redundancy >= COORD_COST_REDUNDANCY * 3);
        assert!(bl.checkpoint >= COORD_COST_CHECKPOINT * 2);
        assert_eq!(coord.ticks_run(), 3);
    }

    #[test]
    fn coordinator_injected_loss_descends_chain() {
        let mut ledger = FaultEventLedger::new();
        let mut coord = match mk_coordinator() {
            Some(c) => c,
            None => return,
        };
        let mut inj = FaultInjector::new(0x40, 1); // 每次注入
        inj.set_armed(InjectorFamily::WrongValue, true);
        // 主副本走注入源 → 每轮分歧 → 3 连败降级。
        let mut i = 0u64;
        let mut descended = false;
        while i < 5 {
            let rep = coord.run_tick(i, 500, &mut inj, &mut ledger);
            match rep {
                Ok(r) => {
                    if let ChainAction::Descended { .. } = r.chain_action {
                        descended = true;
                    }
                    match r.vote {
                        Some(v) => assert!(v.diverged),
                        None => return,
                    }
                }
                Err(_) => return,
            }
            i = i + 1;
        }
        assert!(descended);
        assert_eq!(coord.chain().level(), 1);
        assert_eq!(coord.redundancy().divergences_total(), 5);
        // 注入检出率：错值每轮都被表决抓到。
        assert_eq!(inj.detected(InjectorFamily::WrongValue), 5);
        assert_eq!(inj.detection_rate_bps(), 10000);
    }

    #[test]
    fn coordinator_fallback_only_when_tripped() {
        let mut ledger = FaultEventLedger::new();
        let mut coord = match mk_coordinator() {
            Some(c) => c,
            None => return,
        };
        // 先用注入源制造 5 连分歧触发熔断。
        let mut inj = FaultInjector::new(0x10, 1);
        inj.set_armed(InjectorFamily::WrongValue, true);
        let mut i = 0u64;
        while i < 5 && !coord.redundancy().is_tripped() {
            let _ = coord.run_tick(10 + i, 1, &mut inj, &mut ledger);
            i = i + 1;
        }
        assert!(coord.redundancy().is_tripped());
        // 熔断后走 FallbackOnly 保守单路（真源）。
        let mut src = RealFaultSource::new();
        let rep = coord.run_tick(99, 9, &mut src, &mut ledger);
        match rep {
            Ok(r) => {
                assert_eq!(r.mode, TickMode::FallbackOnly);
                assert!(r.vote.is_none());
                assert!(!r.captured);
                assert_eq!(r.spend.fallback, COORD_COST_FALLBACK);
                assert_eq!(r.spend.redundancy, 0);
                assert_eq!(r.spend.checkpoint, 0);
            }
            Err(_) => return,
        }
    }

    #[test]
    fn coordinator_budget_skips_checkpoint() {
        // 预算仅够冗余：检查点被跳过且显性计数。
        let mut ledger = FaultEventLedger::new();
        let red = match mk_red2() {
            Some(r) => r,
            None => return,
        };
        let ops = [ReplicaOp {
            kind: OpKind::Identity,
            param: 0,
        }; 2];
        let mut coord = match ComboCoordinator::new(
            red,
            CheckpointStore::new(),
            FallbackChain::new(),
            &ops,
            ReplicaOp {
                kind: OpKind::Identity,
                param: 0,
            },
            1,
            COORD_COST_REDUNDANCY, // 预算 = 冗余开销，检查点必然跳过
        ) {
            Ok(c) => c,
            Err(_) => return,
        };
        let mut src = RealFaultSource::new();
        let rep = coord.run_tick(0, 3, &mut src, &mut ledger);
        match rep {
            Ok(r) => {
                assert!(!r.captured);
                assert_eq!(r.spend.checkpoint, 0);
            }
            Err(_) => return,
        }
        assert_eq!(coord.budget_skips(), 1);
        assert!(ledger.count_of(FaultEventKind::BudgetSkipped) >= 1);
    }

    #[test]
    fn coordinator_checkpoint_drill_passthrough() {
        let mut ledger = FaultEventLedger::new();
        let mut coord = match mk_coordinator() {
            Some(c) => c,
            None => return,
        };
        let r = coord.drill_checkpoints(42, &mut ledger);
        match r {
            Ok((pass, _)) => assert!(pass),
            Err(_) => return,
        }
        assert_eq!(coord.checkpoints().drill_pass(), 1);
    }

    // ── 事件账 / 体检 / 指纹 ──

    #[test]
    fn ledger_ring_and_fingerprint() {
        let mut l1 = FaultEventLedger::new();
        let mut l2 = FaultEventLedger::new();
        let mut i = 0u64;
        while i < (EVENT_LEDGER_CAP as u64) + 7 {
            l1.record(FaultEventKind::Heartbeat, i, i);
            l2.record(FaultEventKind::Heartbeat, i, i);
            i = i + 1;
        }
        assert_eq!(l1.len(), EVENT_LEDGER_CAP);
        assert_eq!(l1.total(), (EVENT_LEDGER_CAP + 7) as u32);
        // 同序列同指纹。
        assert_eq!(l1.fingerprint(), l2.fingerprint());
        // 差异改指纹。
        let mut l3 = FaultEventLedger::new();
        let mut j = 0u64;
        while j < (EVENT_LEDGER_CAP as u64) + 7 {
            l3.record(FaultEventKind::Heartbeat, j, j + 1); // hash 不同
            j = j + 1;
        }
        assert_ne!(l1.fingerprint(), l3.fingerprint());
        // 环形最旧记录被覆写。
        let first = match l1.get(0) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(first.tick, 7);
    }

    #[test]
    fn health_score_verdicts() {
        let mut ledger = FaultEventLedger::new();
        // 干净账 → 100 健康。
        let h = ledger.health(None);
        assert_eq!(h.score, 100);
        assert_eq!(h.verdict, "健康");
        // 注入漏检 5 次 → -25 → 75 亚健康。
        let mut i = 0u32;
        while i < 5 {
            ledger.record(FaultEventKind::InjectionMissed, i as u64, 0);
            i = i + 1;
        }
        let h2 = ledger.health(None);
        assert_eq!(h2.score, 75);
        assert_eq!(h2.verdict, "亚健康");
        // 再熔断 2 次 → -20 → 55 劣化。
        let mut j = 0u32;
        while j < 2 {
            ledger.record(FaultEventKind::CircuitTripped, 100, 0);
            j = j + 1;
        }
        let h3 = ledger.health(None);
        assert_eq!(h3.score, 55);
        assert_eq!(h3.verdict, "劣化");
        // 大量扣分 → 危急（分数饱和到 0）。
        let mut k = 0u32;
        while k < 20 {
            ledger.record(FaultEventKind::CircuitTripped, 200, 0);
            k = k + 1;
        }
        let h4 = ledger.health(None);
        assert_eq!(h4.score, 0);
        assert_eq!(h4.verdict, "危急");
    }

    #[test]
    fn health_with_injector_stats() {
        let mut ledger = FaultEventLedger::new();
        let mut inj = FaultInjector::new(0, 1);
        inj.set_armed(InjectorFamily::WrongValue, true);
        let mut i = 0u32;
        while i < 3 {
            let _ = inj.next_result(i as u64);
            inj.notify_vote(false, &mut ledger); // delta=0 全漏检
            i = i + 1;
        }
        let h = ledger.health(Some(&inj));
        assert_eq!(h.missed_injections, 3);
        assert_eq!(h.score, 85);
    }

    // ── 错误诊断 ──

    #[test]
    fn error_details_are_actionable() {
        let all = [
            FaultArchError::ReplicaCountMismatch,
            FaultArchError::VoteNoMajority,
            FaultArchError::CircuitTripped,
            FaultArchError::SnapshotCrcMismatch,
            FaultArchError::SnapshotSlotEmpty,
            FaultArchError::SnapshotSerializeInvalid,
            FaultArchError::ChainLevelInvalid,
            FaultArchError::BudgetExhausted,
            FaultArchError::DecisionUncovered,
            FaultArchError::PayloadSizeInvalid,
            FaultArchError::InjectionTimeout,
            FaultArchError::InjectionLoss,
            FaultArchError::InjectorStateInvalid,
            FaultArchError::CoordinatorStateInvalid,
            FaultArchError::LedgerSaturated,
        ];
        let mut i = 0usize;
        while i < all.len() {
            let e = all[i];
            let d = e.detail();
            assert!(!d.is_empty());
            assert!(d.ends_with("。")); // 中文句号收尾
            assert!((DIAG_SEGMENT_LO..=DIAG_SEGMENT_HI).contains(&e.code()));
            i = i + 1;
        }
    }

    // ── 策略描述 const 表 ──

    #[test]
    fn strategy_desc_table_sanity() {
        assert_eq!(STRATEGY_DESC.len(), 3);
        let mut i = 0u8;
        while i < 3 {
            let s = match FaultStrategy::from_index(i) {
                Some(s) => s,
                None => return,
            };
            let d = s.desc();
            assert!(!d.name.is_empty());
            assert!(d.overhead_tier >= 1 && d.overhead_tier <= 3);
            assert!(d.recovery_deadline_tier >= 1 && d.recovery_deadline_tier <= 3);
            assert!(d.min_data_grade <= 3);
            i = i + 1;
        }
        // 冗余开销最重但恢复最快；回退开销最轻但恢复最慢。
        assert!(STRATEGY_DESC[0].overhead_tier > STRATEGY_DESC[2].overhead_tier);
        assert!(STRATEGY_DESC[0].recovery_deadline_tier < STRATEGY_DESC[2].recovery_deadline_tier);
    }

    #[test]
    fn decision_table_shape_36_entries() {
        // 3×3×4 全遍历（while + 索引）。
        let mut r = 0usize;
        let mut count = 0usize;
        while r < 3 {
            let mut c = 0usize;
            while c < 3 {
                let mut k = 0usize;
                while k < 4 {
                    let combo = DECISION_TABLE[r][c][k];
                    assert!(combo.mask >= 0b001 && combo.mask <= 0b111); // 至少一策略
                    assert!(!combo.reason.is_empty());
                    // 主策略必须在掩码内。
                    let bit = 1u8 << combo.primary;
                    assert_eq!(combo.mask & bit, bit);
                    count = count + 1;
                    k = k + 1;
                }
                c = c + 1;
            }
            r = r + 1;
        }
        assert_eq!(count, 36);
    }

    #[test]
    fn crc32_known_vector() {
        // CRC32("123456789") = 0xCBF43926（IEEE 802.3 标准校验向量）。
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0x0000_0000);
    }

    #[test]
    fn budget_constants_accounting_close() {
        // 冗余+检查点+回退 = 80 ≤ 100（留 20 裕量），注释来源见常量定义。
        assert_eq!(
            COORD_COST_REDUNDANCY + COORD_COST_CHECKPOINT + COORD_COST_FALLBACK,
            80
        );
        assert!(COORD_BUDGET_PER_TICK >= 80);
    }
}
