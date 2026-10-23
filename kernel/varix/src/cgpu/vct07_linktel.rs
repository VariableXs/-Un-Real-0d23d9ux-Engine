//! CGPU-F3047 · 对接遥测（CGPU-T 域 · 桥接组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3047`
//!
//! **判据（锚点原文）**：遥测复用、一组、判据。
//!
//! **职责定位（锚点原文摘录）**：遥测（对接吞吐/延迟遥测——遥测复用；测试（遥测一组）。
//!
//! ---------------------------------------------------------------------------
//!
//! 本模块是对接桥（宿主侧/客侧）的吞吐与延迟遥测子系统，逐条落实锚点判据：
//!
//! 1. **双向吞吐三口径**：桥两端（host->guest / guest->host）按
//!    字节 / 帧 / 批 三口径计数，滑动窗口聚合 + EWMA 平滑 + 峰值记账。
//! 2. **端到端延迟四段分解**：排队(queue)/拷贝(copy)/协议(proto)/等待(wait)
//!    四段，整数 µs 记账；每段滚动 600 样本环形缓冲 + 段级 P95/P99；
//!    段间守恒 Σ段 ≡ 端到端，破缺立即报码拒收。
//! 3. **往返/单向分开记账**：方向位（HostToGuest / GuestToHost）独立累计，
//!    往返（双向合计）与单向（按方向）在报告中分层呈现。
//! 4. **遥测复用**：预算口径镜像自 CGPU-T 域遥测预算表（vck25_telbudget
//!    同口径），本模块不另立口径，镜像常量在 §0 逐条注明复用声明。
//! 5. **滑动窗口聚合**：整数最近邻分位（P95/P99，最近邻秩法）+ 整数定点
//!    EWMA（乘子 273/1024，推导见常量注释），零浮点。
//! 6. **积压水位**：三级（正常/警戒/拥塞）滞回闸，背压触发的遥测旗 +
//!    拥塞期事件旗传播建议。
//! 7. **异常三类**：丢包/重传/超时独立计数 + 异常率 ppm（零分母显性）+
//!    连续异常熔断计数。
//! 8. **schema 版本化 + 发布包闭合纪律**：字段白名单 + 版本指纹 +
//!    `export_release_pack()` 发布闸（帧闭合闸/守恒闸/空窗闸/熔断闸/体检闸）。
//! 9. **诚实纪律**：空窗 Empty 显性报码；未闭合拒发布；零分母显性旗标；
//!    溢出全走 saturating 并留饱和自证计数。
//! 10. **隐私本地闭环**：发布包只含本地哈希与聚合计数，无任何原始标识
//!     字符串出域。
//! 11. **开销自证**：ingest 路径按步记账，预算镜像 T 域预算表，超限报码。
//! 12. **确定性**：同事件序列双跑对拍，报告指纹逐位一致方为通过。
//!
//! 纪律：no_std + alloc；内核路径零 unwrap/expect/panic!/运行期 assert!；
//! 可失败全走 `Result<T, LinkTelError>`；>64KB 结构体一律堆侧或拆分。
//! 诊断码段：0x1320~0x132F（十六码独占；开工前已 grep 确认段空闲）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ============================================================================
// §0 · 常量（全部写死来源；含遥测复用声明）
// ============================================================================

/// 模块版本号（来源：施工书 CGPU-F3047 首版）。
pub const F3047_VERSION: &str = "f3047-vct07_linktel-v1";

/// 诊断码段基址（来源：施工书诊断段分配表，开工前 grep 确认 0x132 段空闲）。
pub const F3047_SEG_BASE: u16 = 0x1320;

/// 每段滚动样本环形容量（来源：锚点判据"滚动 600 样本环形"）。
pub const RING_SAMPLES: usize = 600;

/// 窗口桶数（来源：T 域遥测预算表窗口口径 64 桶，镜像复用）。
pub const THROUGHPUT_BUCKETS: usize = 64;

/// 桶宽，单位 µs（来源：T 域预算表毫秒级窗口口径，1ms 一桶）。
pub const BUCKET_WIDTH_US: u64 = 1_000;

/// EWMA 整数定点乘子分子（来源：α = 273/1024 ≈ 0.2666；半衰期推导：
/// (1-α)^n = 1/2 → n = ln(1/2)/ln(727/1024) ≈ 0.6931/0.3425 ≈ 2.02 个
/// 更新周期，即约两桶半衰期；整数定点避免浮点）。
pub const EWMA_ALPHA_NUM: u64 = 273;

/// EWMA 定点分母（来源：取 2^10 = 1024 便于移位级整数除法，无浮点）。
pub const EWMA_ALPHA_DEN: u64 = 1024;

/// P95 最近邻秩下标（来源：最近邻秩法 ceil(600×0.95)=570 名，0 基 569）。
pub const P95_INDEX: usize = 569;

/// P99 最近邻秩下标（来源：最近邻秩法 ceil(600×0.99)=594 名，0 基 593）。
pub const P99_INDEX: usize = 593;

/// 警戒水位进入阈值（来源：T 域预算表积压水位三级表，4K 项深）。
pub const WATERMARK_WARN_ENTER: u32 = 4096;

/// 警戒水位退出阈值（滞回：进入阈值的 70%，4096×7/10=2867，向下取整；
/// 来源：滞回纪律，退出阈值必须低于进入阈值防抖动）。
pub const WATERMARK_WARN_EXIT: u32 = 2867;

/// 拥塞水位进入阈值（来源：T 域预算表三级表，警戒的 3 倍）。
pub const WATERMARK_CONGEST_ENTER: u32 = 12288;

/// 拥塞水位退出阈值（滞回：进入阈值的 70%，12288×7/10=8601.6 向下取整）。
pub const WATERMARK_CONGEST_EXIT: u32 = 8601;

/// 连续异常熔断阈值（来源：异常熔断纪律，连续 16 次异常即熔断记账）。
pub const CIRCUIT_CONSECUTIVE_LIMIT: u32 = 16;

/// —— 遥测复用声明 ——
/// 以下两条口径**镜像自 CGPU-T 域遥测预算表**（vck25_telbudget 同口径）：
/// 本模块不重新定义预算语义，仅按镜像值执行；若 T 域预算表修订，
/// 此处同步镜像，不另立口径。
/// 单事件遥测记账路径步预算（镜像：T 域单事件遥测步预算）。
pub const MIRROR_T_DOMAIN_INGEST_STEP_BUDGET: u32 = 256;

/// 单事件延迟分解段数（镜像：T 域四段延迟口径：排队/拷贝/协议/等待）。
pub const MIRROR_T_DOMAIN_EVENT_SEGMENTS: usize = 4;

/// ingest 路径设计步数（自证基线：与 `LinkTelHub::ingest` 内 8 处闸位
/// `steps += 1` 严格一致，漂移即记入 `step_drift`）。
pub const INGEST_PATH_STEPS_EXPECTED: u32 = 8;

/// schema 魔数（来源：本模块 schema 首版冻结值，ASCII 可读）。
pub const SCHEMA_MAGIC: [u8; 8] = *b"VXLTSC01";

/// schema 版本（来源：首版 = 1；修订必须换指纹）。
pub const SCHEMA_VERSION: u32 = 1;

/// FNV-1a 64 位偏移基（来源：FNV 公开规范常数）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数（来源：FNV 公开规范常数）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// —— 编译期断言（仅常量级，非运行期） ——
const _: () = assert!(RING_SAMPLES == 600);
const _: () = assert!(P95_INDEX < RING_SAMPLES);
const _: () = assert!(P99_INDEX < RING_SAMPLES);
const _: () = assert!(P95_INDEX < P99_INDEX);
const _: () = assert!(WATERMARK_WARN_EXIT < WATERMARK_WARN_ENTER);
const _: () = assert!(WATERMARK_CONGEST_EXIT < WATERMARK_CONGEST_ENTER);
const _: () = assert!(WATERMARK_CONGEST_ENTER > WATERMARK_WARN_ENTER);
const _: () = assert!(EWMA_ALPHA_NUM > 0 && EWMA_ALPHA_NUM <= EWMA_ALPHA_DEN);
const _: () = assert!(INGEST_PATH_STEPS_EXPECTED <= MIRROR_T_DOMAIN_INGEST_STEP_BUDGET);
const _: () = assert!(CIRCUIT_CONSECUTIVE_LIMIT >= 2);
const _: () = assert!(THROUGHPUT_BUCKETS >= 2);
const _: () = assert!(MIRROR_T_DOMAIN_EVENT_SEGMENTS == 4);

// ============================================================================
// §1 · 诊断码段 0x1320~0x132F（十六码独占）
// ============================================================================

/// 对接遥测诊断码（0x1320~0x132F，段内互斥；每变体配中文可操作诊断）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkTelError {
    /// 0x1320 序号倒退/重复（(ts,seq) 单调闸）。
    SeqRegression { last_seq: u32, got_seq: u32 },
    /// 0x1321 时间戳倒退（时间单调闸）。
    TimeRegression { last_ts_us: u64, got_ts_us: u64 },
    /// 0x1322 四段守恒破缺（Σ段 ≠ 端到端）。
    ConservationBroken { e2e_us: u32, seg_sum_us: u64 },
    /// 0x1323 空窗（无样本，零分母显性拒绝）。
    EmptyWindow,
    /// 0x1324 帧未开（发布闸：未 begin_frame 即请求发布/闭帧）。
    FrameNotOpen,
    /// 0x1325 帧未闭合（发布闸：begin 后未 finish，拒发布）。
    FrameNotClosed,
    /// 0x1326 schema 字段白名单违规。
    SchemaViolation,
    /// 0x1327 schema 版本未知（版本化闸）。
    UnknownSchemaVersion { got: u32 },
    /// 0x1328 连续异常熔断已开（记账被拒）。
    CircuitBreakerOpen { consecutive: u32 },
    /// 0x1329 路径步数超预算（开销自证闸）。
    BudgetExceeded { counted: u32, budget: u32 },
    /// 0x132A 双跑确定性对拍失配。
    Nondeterministic { fp_a: u64, fp_b: u64 },
    /// 0x132B 拥塞拒收（背压闸：拥塞期且拥塞闸开启时拒非关键批）。
    CongestedReject,
    /// 0x132C 水位配置非法（退出阈 ≥ 进入阈等）。
    WatermarkInvalid {
        warn_enter: u32,
        warn_exit: u32,
        congest_enter: u32,
        congest_exit: u32,
    },
    /// 0x132D 发布被闸拒（gate 指明具体闸名）。
    ExportDenied { gate: &'static str },
    /// 0x132E 体检降级（subsystem 指明未过体检的子系统）。
    HealthDegraded { subsystem: &'static str },
    /// 0x132F 方向字节非法（from_raw 原始事件入口闸）。
    DirectionMismatch { expected_count: u8, got_byte: u8 },
}

impl LinkTelError {
    /// 诊断码（0x1320~0x132F，与变体一一对应，段内互斥）。
    pub fn code(&self) -> u16 {
        match self {
            LinkTelError::SeqRegression { .. } => 0x1320,
            LinkTelError::TimeRegression { .. } => 0x1321,
            LinkTelError::ConservationBroken { .. } => 0x1322,
            LinkTelError::EmptyWindow => 0x1323,
            LinkTelError::FrameNotOpen => 0x1324,
            LinkTelError::FrameNotClosed => 0x1325,
            LinkTelError::SchemaViolation => 0x1326,
            LinkTelError::UnknownSchemaVersion { .. } => 0x1327,
            LinkTelError::CircuitBreakerOpen { .. } => 0x1328,
            LinkTelError::BudgetExceeded { .. } => 0x1329,
            LinkTelError::Nondeterministic { .. } => 0x132A,
            LinkTelError::CongestedReject => 0x132B,
            LinkTelError::WatermarkInvalid { .. } => 0x132C,
            LinkTelError::ExportDenied { .. } => 0x132D,
            LinkTelError::HealthDegraded { .. } => 0x132E,
            LinkTelError::DirectionMismatch { .. } => 0x132F,
        }
    }

    /// 中文可操作诊断（每变体一段，指明"什么坏了、怎么修"）。
    pub fn detail(&self) -> &'static str {
        match self {
            LinkTelError::SeqRegression { .. } => {
                "0x1320 事件序号倒退或重复：(ts,seq) 必须严格 (不降, 递增)。请检查桥两端 \
                 序号生成器是否复位/重放；倒退事件已拒收入账，修正源头后重新发起。"
            }
            LinkTelError::TimeRegression { .. } => {
                "0x1321 事件时间戳倒退：遥测时间基必须单调不降。请检查桥两端时钟同步与 \
                 事件乱序缓冲；倒退事件已拒收，先修时钟再续采。"
            }
            LinkTelError::ConservationBroken { .. } => {
                "0x1322 四段守恒破缺：排队+拷贝+协议+等待 ≠ 端到端。请检查事件填充侧 \
                 四段计时器覆盖是否有重叠或遗漏；破缺事件已拒收并计入守恒违例。"
            }
            LinkTelError::EmptyWindow => {
                "0x1323 空窗：请求的统计在当前窗口无任何样本（零分母显性拒绝，不产伪值）。\
                 请先注入事件或扩大采样周期后再取报告。"
            }
            LinkTelError::FrameNotOpen => {
                "0x1324 帧未开启：发布前必须先 begin_frame。请按 开帧→注入→闭帧→发布 \
                 的闭合顺序调用。"
            }
            LinkTelError::FrameNotClosed => {
                "0x1325 帧未闭合：begin_frame 之后必须 finish_frame 才可发布（闭合纪律）。\
                 请先调用 finish_frame 再重试发布。"
            }
            LinkTelError::SchemaViolation => {
                "0x1326 schema 字段白名单违规：请求的字段不在 LinkSchemaV1::FIELDS 内。\
                 请改用白名单字段，或按版本化流程申请新字段并更新版本指纹。"
            }
            LinkTelError::UnknownSchemaVersion { .. } => {
                "0x1327 schema 版本未知：请求的版本高于本模块冻结版本。请升级本模块或 \
                 降级请求版本；版本指纹以 SCHEMA_MAGIC+FIELDS 为准。"
            }
            LinkTelError::CircuitBreakerOpen { .. } => {
                "0x1328 连续异常熔断已开：异常连续达到熔断阈值，后续异常事件拒收入账。\
                 请先排查链路丢包/重传/超时根因，再调用 clear_breaker 复位。"
            }
            LinkTelError::BudgetExceeded { .. } => {
                "0x1329 路径步数超预算：ingest 实际步数超过 T 域镜像预算。请检查本模块 \
                 是否被改厚；预算口径镜像 T 域预算表，不得放宽。"
            }
            LinkTelError::Nondeterministic { .. } => {
                "0x132A 双跑对拍失配：同事件序列两次处理报告指纹不一致。请检查是否存在 \
                 未初始化状态或依赖环境的分支；确定性是遥测可信前提。"
            }
            LinkTelError::CongestedReject => {
                "0x132B 拥塞拒收：积压水位处于拥塞级且拥塞闸开启，非关键批被拒。\
                 请按水位建议降载/分流，待水位回落后重试。"
            }
            LinkTelError::WatermarkInvalid { .. } => {
                "0x132C 水位配置非法：退出阈必须 < 进入阈，且 拥塞进入 > 警戒进入。\
                 请修正四元组次序后重建观察器。"
            }
            LinkTelError::ExportDenied { .. } => {
                "0x132D 发布被闸拒：发布包未过全部闭合闸。请按 gate 字段定位未过的闸，\
                 修复后重新导出。"
            }
            LinkTelError::HealthDegraded { .. } => {
                "0x132E 体检降级：指定子系统未过体检，发布被体检闸拦下。请按 subsystem \
                 字段排查对应子系统，全绿后再发布。"
            }
            LinkTelError::DirectionMismatch { .. } => {
                "0x132F 方向字节非法：原始事件 direction_byte 只允许 0(host->guest) 或 \
                 1(guest->host)。请修正事件填充侧方向编码。"
            }
        }
    }

    /// 诊断码的十六进制字符串形式（便于日志与测试断言）。
    pub fn code_str(&self) -> String {
        format!("0x{:04X}", self.code())
    }
}

// ============================================================================
// §2 · FNV-1a 64 位指纹与序列化工具
// ============================================================================

/// FNV-1a 64 位增量哈希：以 seed 续喂一段字节。
pub fn fnv1a64(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed;
    let mut i = 0;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u64)).wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV-1a 64 位续喂一个 u64（小端 8 字节展开）。
pub fn fnv1a64_u64(seed: u64, value: u64) -> u64 {
    let b = value.to_le_bytes();
    fnv1a64(seed, &b)
}

/// 把 u64 以小端追加进序列化缓冲（发布包指纹用，规范化字节序）。
fn push_u64_le(out: &mut Vec<u8>, value: u64) {
    let b = value.to_le_bytes();
    let mut i = 0;
    while i < 8 {
        out.push(b[i]);
        i += 1;
    }
}

// ============================================================================
// §3 · 基础枚举：方向 / 延迟段 / 异常种类 / 水位级别
// ============================================================================

/// 桥方向：宿主侧→客侧 / 客侧→宿主（往返=双向合计，单向=按方向分层）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelDirection {
    /// 宿主侧 → 客侧（下行/提交方向）。
    HostToGuest,
    /// 客侧 → 宿主（上行/回执方向）。
    GuestToHost,
}

impl TelDirection {
    /// 方向下标（const fn：仅对原始枚举做穷尽匹配，不涉及 PartialEq 的 ==）。
    pub const fn as_index(self) -> usize {
        match self {
            TelDirection::HostToGuest => 0,
            TelDirection::GuestToHost => 1,
        }
    }

    /// 稳定标签（不携带任何本机标识，仅方向语义，隐私安全）。
    pub fn label(self) -> &'static str {
        match self {
            TelDirection::HostToGuest => "host->guest",
            TelDirection::GuestToHost => "guest->host",
        }
    }
}

/// 延迟段：排队/拷贝/协议/等待四段 + 端到端（守恒：前四段之和 ≡ 端到端）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelSegment {
    /// 排队段（入桥前排队）。
    Queue,
    /// 拷贝段（跨桥数据搬运）。
    Copy,
    /// 协议段（协议编解码/握手）。
    Proto,
    /// 等待段（远端确认等待）。
    Wait,
    /// 端到端（四段之和）。
    EndToEnd,
}

impl TelSegment {
    /// 段下标 0..=3（四段），4 = 端到端。
    pub const fn as_index(self) -> usize {
        match self {
            TelSegment::Queue => 0,
            TelSegment::Copy => 1,
            TelSegment::Proto => 2,
            TelSegment::Wait => 3,
            TelSegment::EndToEnd => 4,
        }
    }

    /// 段标签。
    pub fn label(self) -> &'static str {
        match self {
            TelSegment::Queue => "queue",
            TelSegment::Copy => "copy",
            TelSegment::Proto => "proto",
            TelSegment::Wait => "wait",
            TelSegment::EndToEnd => "e2e",
        }
    }
}

/// 异常三类：丢包 / 重传 / 超时（锚点判据：丢包/重传/超时三类异常计数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyKind {
    /// 丢包。
    Dropped,
    /// 重传。
    Retransmitted,
    /// 超时。
    TimedOut,
}

impl AnomalyKind {
    /// 类别下标（0..2）。
    pub const fn as_index(self) -> usize {
        match self {
            AnomalyKind::Dropped => 0,
            AnomalyKind::Retransmitted => 1,
            AnomalyKind::TimedOut => 2,
        }
    }

    /// 类别标签。
    pub fn label(self) -> &'static str {
        match self {
            AnomalyKind::Dropped => "dropped",
            AnomalyKind::Retransmitted => "retrans",
            AnomalyKind::TimedOut => "timeout",
        }
    }
}

/// 积压水位三级：正常 / 警戒 / 拥塞（滞回闸控迁移）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TelBacklogLevel {
    /// 正常水位。
    Normal,
    /// 警戒水位（进入阈 4096，退出阈 2867）。
    Warning,
    /// 拥塞水位（进入阈 12288，退出阈 8601）。
    Congested,
}

impl TelBacklogLevel {
    /// 级别下标（0..2）。
    pub const fn as_index(self) -> usize {
        match self {
            TelBacklogLevel::Normal => 0,
            TelBacklogLevel::Warning => 1,
            TelBacklogLevel::Congested => 2,
        }
    }

    /// 级别标签。
    pub fn label(self) -> &'static str {
        match self {
            TelBacklogLevel::Normal => "normal",
            TelBacklogLevel::Warning => "warning",
            TelBacklogLevel::Congested => "congested",
        }
    }
}

// ============================================================================
// §4 · 事件层：LinkTelEventV1（定长、严格 (ts,seq) 单调、四段延迟）
// ============================================================================

/// 事件旗位（u16 位图；来源：本模块事件协议首版冻结）。
pub mod event_flags {
    /// 事件发生于拥塞水位期间（由账本侧补盖，事件侧原样携带则以事件侧为准）。
    pub const FLAG_DURING_CONGEST: u16 = 1 << 0;
    /// 本事件链路发生丢包。
    pub const FLAG_DROPPED: u16 = 1 << 1;
    /// 本事件链路发生重传。
    pub const FLAG_RETRANSMIT: u16 = 1 << 2;
    /// 本事件链路发生超时。
    pub const FLAG_TIMEOUT: u16 = 1 << 3;
    /// 背压提示旗（事件填充侧请求下游降载）。
    pub const FLAG_BACKPRESSURE_HINT: u16 = 1 << 4;
}

/// 对接遥测事件 V1（定长 48 字节级，纯 Copy，无堆字段）。
///
/// 严格 (ts,seq) 单调由 `LinkTelHub::ingest` 把关：
/// * `seq` 必须严格递增（等于或倒退 → 0x1320 拒收）；
/// * `ts_us` 必须单调不降（倒退 → 0x1321 拒收）。
///
/// 四段守恒由 `LatencyDecomposer::ingest` 把关：
/// `queue_us + copy_us + proto_us + wait_us == e2e_us`（饱和加后比较），
/// 破缺 → 0x1322 拒收。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkTelEventV1 {
    /// 事件时间戳（µs，单调不降）。
    pub ts_us: u64,
    /// 事件序号（严格递增）。
    pub seq: u32,
    /// 桥方向。
    pub direction: TelDirection,
    /// 本批字节数（字节口径）。
    pub batch_bytes: u64,
    /// 本批帧数（帧口径）。
    pub frame_count: u32,
    /// 排队段延迟（µs）。
    pub queue_us: u32,
    /// 拷贝段延迟（µs）。
    pub copy_us: u32,
    /// 协议段延迟（µs）。
    pub proto_us: u32,
    /// 等待段延迟（µs）。
    pub wait_us: u32,
    /// 端到端延迟（µs）＝ 四段之和（守恒）。
    pub e2e_us: u32,
    /// 采样时点的积压深度（水位观测输入）。
    pub backlog_depth: u32,
    /// 事件旗位（`event_flags` 位图）。
    pub flags: u16,
}

impl LinkTelEventV1 {
    /// 便捷构造（守恒自动成立：e2e = 四段饱和和）。
    pub fn new(
        ts_us: u64,
        seq: u32,
        direction: TelDirection,
        batch_bytes: u64,
        frame_count: u32,
        queue_us: u32,
        copy_us: u32,
        proto_us: u32,
        wait_us: u32,
        backlog_depth: u32,
        flags: u16,
    ) -> Self {
        let e2e = (queue_us as u64)
            .saturating_add(copy_us as u64)
            .saturating_add(proto_us as u64)
            .saturating_add(wait_us as u64);
        LinkTelEventV1 {
            ts_us,
            seq,
            direction,
            batch_bytes,
            frame_count,
            queue_us,
            copy_us,
            proto_us,
            wait_us,
            e2e_us: e2e as u32,
            backlog_depth,
            flags,
        }
    }

    /// 端到端段总和（饱和）。
    pub fn seg_sum_us(&self) -> u64 {
        (self.queue_us as u64)
            .saturating_add(self.copy_us as u64)
            .saturating_add(self.proto_us as u64)
            .saturating_add(self.wait_us as u64)
    }

    /// 是否携带任一异常旗（丢包/重传/超时）。
    pub fn has_anomaly_flag(&self) -> bool {
        let f = self.flags;
        (f & event_flags::FLAG_DROPPED != 0)
            || (f & event_flags::FLAG_RETRANSMIT != 0)
            || (f & event_flags::FLAG_TIMEOUT != 0)
    }
}

/// 原始事件（填充侧给到的是方向字节，入口闸校验后转正式事件）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkTelEventV1Raw {
    /// 事件时间戳（µs）。
    pub ts_us: u64,
    /// 事件序号。
    pub seq: u32,
    /// 方向字节：0 = host->guest，1 = guest->host，其余非法。
    pub direction_byte: u8,
    /// 本批字节数。
    pub batch_bytes: u64,
    /// 本批帧数。
    pub frame_count: u32,
    /// 排队段（µs）。
    pub queue_us: u32,
    /// 拷贝段（µs）。
    pub copy_us: u32,
    /// 协议段（µs）。
    pub proto_us: u32,
    /// 等待段（µs）。
    pub wait_us: u32,
    /// 端到端（µs）。
    pub e2e_us: u32,
    /// 积压深度。
    pub backlog_depth: u32,
    /// 旗位。
    pub flags: u16,
}

impl LinkTelEventV1 {
    /// 原始事件入口闸：方向字节白名单校验（非法 → 0x132F）。
    pub fn from_raw(r: &LinkTelEventV1Raw) -> Result<Self, LinkTelError> {
        let direction = match r.direction_byte {
            0 => TelDirection::HostToGuest,
            1 => TelDirection::GuestToHost,
            other => {
                return Err(LinkTelError::DirectionMismatch {
                    expected_count: 2,
                    got_byte: other,
                })
            }
        };
        Ok(LinkTelEventV1 {
            ts_us: r.ts_us,
            seq: r.seq,
            direction,
            batch_bytes: r.batch_bytes,
            frame_count: r.frame_count,
            queue_us: r.queue_us,
            copy_us: r.copy_us,
            proto_us: r.proto_us,
            wait_us: r.wait_us,
            e2e_us: r.e2e_us,
            backlog_depth: r.backlog_depth,
            flags: r.flags,
        })
    }
}

// ============================================================================
// §5 · 样本环形 + 确定性排序 + 整数分位
// ============================================================================

/// 确定性升序排序（原地堆排序；无分配、无随机性、同输入同输出——
/// 双跑确定性对拍的前提之一。std sort 的算法随工具链版本可变，
/// 故手写以保证指纹跨版本稳定）。
fn sort_ascending(v: &mut [u32]) {
    let n = v.len();
    if n < 2 {
        return;
    }
    // 建堆：自底向上逐根下沉。
    let mut i = n / 2;
    while i > 0 {
        i -= 1;
        sift_down(v, i, n);
    }
    // 排序：堆顶（当前最大）与末端交换后缩堆。
    let mut end = n;
    while end > 1 {
        end -= 1;
        v.swap(0, end);
        sift_down(v, 0, end);
    }
}

/// 堆下沉（max-heap 语义）。
fn sift_down(v: &mut [u32], root: usize, end: usize) {
    let mut r = root;
    loop {
        let left = r * 2 + 1;
        if left >= end {
            return;
        }
        let mut big = left;
        let right = left + 1;
        if right < end && v[right] > v[left] {
            big = right;
        }
        if v[big] > v[r] {
            v.swap(r, big);
            r = big;
        } else {
            return;
        }
    }
}

/// 环形统计快照（分位/均值/极值；count 为活样本数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingStats {
    /// P95（最近邻秩，整数）。
    pub p95: u32,
    /// P99（最近邻秩，整数）。
    pub p99: u32,
    /// 均值（整数除法向下取整）。
    pub mean: u32,
    /// 最小值。
    pub min: u32,
    /// 最大值。
    pub max: u32,
    /// 活样本数。
    pub count: usize,
}

/// 滚动 600 样本环形（u32 样本；满后滚动覆盖最老样本）。
#[derive(Debug, Clone)]
pub struct SampleRing {
    buf: [u32; RING_SAMPLES],
    /// 下一个写入位（也是满载时最老样本位）。
    head: usize,
    count: usize,
    /// 样本和（u64；600 × u32::MAX ≈ 2.58e12，远小于 u64 上限，
    /// 但仍用饱和算术显性处理）。
    sum: u64,
}

impl SampleRing {
    /// 空环。
    pub fn new() -> Self {
        SampleRing {
            buf: [0u32; RING_SAMPLES],
            head: 0,
            count: 0,
            sum: 0,
        }
    }

    /// 压入样本（满后覆盖最老样本，和账饱和修正）。
    pub fn push(&mut self, x: u32) {
        if self.count < RING_SAMPLES {
            self.buf[self.head] = x;
            self.head = (self.head + 1) % RING_SAMPLES;
            self.count += 1;
            self.sum = self.sum.saturating_add(x as u64);
        } else {
            let old = self.buf[self.head];
            self.buf[self.head] = x;
            self.head = (self.head + 1) % RING_SAMPLES;
            self.sum = self.sum.saturating_sub(old as u64).saturating_add(x as u64);
        }
    }

    /// 活样本数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 样本和（饱和记账值）。
    pub fn sum(&self) -> u64 {
        self.sum
    }

    /// 是否空窗。
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 快照排序副本（空窗返回 None；排序用确定性堆排序）。
    pub fn snapshot_sorted(&self) -> Option<Vec<u32>> {
        if self.count == 0 {
            return None;
        }
        let mut v: Vec<u32> = Vec::with_capacity(self.count);
        // 最老样本位 = (head - count) mod N（head 指向下一写入位）。
        let start = if self.count < RING_SAMPLES {
            (self.head + RING_SAMPLES - self.count) % RING_SAMPLES
        } else {
            self.head
        };
        let mut i = 0;
        while i < self.count {
            v.push(self.buf[(start + i) % RING_SAMPLES]);
            i += 1;
        }
        sort_ascending(&mut v);
        Some(v)
    }

    /// 整数统计（空窗 → 0x1323；分位用最近邻秩法，随活样本数缩放：
    /// rank95 = ceil(count×0.95)，rank99 = ceil(count×0.99)，0 基取值）。
    pub fn stats(&self) -> Result<RingStats, LinkTelError> {
        let sorted = match self.snapshot_sorted() {
            Some(s) => s,
            None => return Err(LinkTelError::EmptyWindow),
        };
        let count = sorted.len();
        // count ≥ 1 保证 rank 计算安全（count=600 时分别落在 569/593）。
        let r95 = ((count * 95 + 99) / 100) - 1;
        let r99 = ((count * 99 + 99) / 100) - 1;
        // r95/r99 ≤ count-1 由秩法构造保证，仍做防御性钳制。
        let i95 = if r95 >= count { count - 1 } else { r95 };
        let i99 = if r99 >= count { count - 1 } else { r99 };
        let mean = (self.sum / count as u64) as u32;
        Ok(RingStats {
            p95: sorted[i95],
            p99: sorted[i99],
            mean,
            min: sorted[0],
            max: sorted[count - 1],
            count,
        })
    }
}

// ============================================================================
// §6 · 延迟四段分解账（LatencyDecomposer）
// ============================================================================

/// 延迟分解账：四段 + 端到端各一条 600 样本环形；逐事件守恒闸
/// （Σ段 ≡ 端到端，饱和和后比较，破缺即报 0x1322 并记违例）。
#[derive(Debug, Clone)]
pub struct LatencyDecomposer {
    queue: SampleRing,
    copy: SampleRing,
    proto: SampleRing,
    wait: SampleRing,
    e2e: SampleRing,
    /// 守恒违例累计（饱和）。
    conservation_violations: u64,
    /// 最近一次违例的 (端到端, Σ段)，供审计报码。
    last_violation: Option<(u32, u64)>,
    /// 守恒通过入账事件数。
    accepted: u64,
}

impl LatencyDecomposer {
    /// 新账（五环全空）。
    pub fn new() -> Self {
        LatencyDecomposer {
            queue: SampleRing::new(),
            copy: SampleRing::new(),
            proto: SampleRing::new(),
            wait: SampleRing::new(),
            e2e: SampleRing::new(),
            conservation_violations: 0,
            last_violation: None,
            accepted: 0,
        }
    }

    /// 逐事件入账：先守恒闸后入环（校验失败不产生任何副作用）。
    pub fn ingest(&mut self, ev: &LinkTelEventV1) -> Result<(), LinkTelError> {
        let seg_sum = ev.seg_sum_us();
        if seg_sum != ev.e2e_us as u64 {
            self.conservation_violations = self.conservation_violations.saturating_add(1);
            self.last_violation = Some((ev.e2e_us, seg_sum));
            return Err(LinkTelError::ConservationBroken {
                e2e_us: ev.e2e_us,
                seg_sum_us: seg_sum,
            });
        }
        self.queue.push(ev.queue_us);
        self.copy.push(ev.copy_us);
        self.proto.push(ev.proto_us);
        self.wait.push(ev.wait_us);
        self.e2e.push(ev.e2e_us);
        self.accepted = self.accepted.saturating_add(1);
        Ok(())
    }

    /// 指定段的环形引用。
    pub fn ring(&self, seg: TelSegment) -> &SampleRing {
        match seg {
            TelSegment::Queue => &self.queue,
            TelSegment::Copy => &self.copy,
            TelSegment::Proto => &self.proto,
            TelSegment::Wait => &self.wait,
            TelSegment::EndToEnd => &self.e2e,
        }
    }

    /// 段级统计（空窗 → 0x1323）。
    pub fn segment_stats(&self, seg: TelSegment) -> Result<RingStats, LinkTelError> {
        self.ring(seg).stats()
    }

    /// 守恒违例累计。
    pub fn conservation_violations(&self) -> u64 {
        self.conservation_violations
    }

    /// 守恒通过入账数。
    pub fn accepted(&self) -> u64 {
        self.accepted
    }

    /// 守恒审计闸（破即报码）：
    /// * 空窗 → 0x1323；
    /// * 有违例 → 0x1322（携带最近一次违例数值）；
    /// * 否则通过。
    pub fn conservation_audit(&self) -> Result<(), LinkTelError> {
        if self.accepted == 0 {
            return Err(LinkTelError::EmptyWindow);
        }
        if let Some((e2e, seg_sum)) = self.last_violation {
            return Err(LinkTelError::ConservationBroken {
                e2e_us: e2e,
                seg_sum_us: seg_sum,
            });
        }
        Ok(())
    }

    /// 五段全量报告（任一段空窗 → 0x1323；五环同步填充，理论同空同满）。
    pub fn latency_report(&self) -> Result<LatencyReport, LinkTelError> {
        Ok(LatencyReport {
            queue: self.segment_stats(TelSegment::Queue)?,
            copy: self.segment_stats(TelSegment::Copy)?,
            proto: self.segment_stats(TelSegment::Proto)?,
            wait: self.segment_stats(TelSegment::Wait)?,
            e2e: self.segment_stats(TelSegment::EndToEnd)?,
            conservation_violations: self.conservation_violations,
            conservation_ok: self.conservation_violations == 0,
        })
    }
}

/// 五段延迟报告（往返/单向由吞吐侧分层，此处为全量事件口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencyReport {
    /// 排队段。
    pub queue: RingStats,
    /// 拷贝段。
    pub copy: RingStats,
    /// 协议段。
    pub proto: RingStats,
    /// 等待段。
    pub wait: RingStats,
    /// 端到端。
    pub e2e: RingStats,
    /// 守恒违例累计。
    pub conservation_violations: u64,
    /// 守恒是否零违例。
    pub conservation_ok: bool,
}

// ============================================================================
// §7 · 吞吐表（ThroughputMeter）：三口径滑动窗 + 整数 EWMA + 峰值
// ============================================================================

/// 毫秒桶（三口径累计）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TpBucket {
    /// 桶内字节累计。
    pub bytes: u64,
    /// 桶内帧累计。
    pub frames: u64,
    /// 桶内批累计。
    pub batches: u64,
}

impl TpBucket {
    const EMPTY: TpBucket = TpBucket {
        bytes: 0,
        frames: 0,
        batches: 0,
    };
}

/// 吞吐表：64 桶 × 1ms 滑动窗（字节/帧/批三口径），
/// 桶滚动时结算瞬时吞吐并更新整数 EWMA 与峰值；方向维度独立记账
/// （往返 = 双向合计，单向 = 按方向分层）。
#[derive(Debug, Clone)]
pub struct ThroughputMeter {
    buckets: [TpBucket; THROUGHPUT_BUCKETS],
    cursor: usize,
    /// 当前桶的毫秒纪元（ts_us / 1000）。
    current_bt: u64,
    started: bool,
    /// 窗内已触及桶数（0..=64）。
    filled: usize,
    /// EWMA 字节/毫秒（整数定点，乘子 273/1024）。
    ewma_bpm: u64,
    /// 峰值字节/毫秒。
    peak_bpm: u64,
    /// 方向维度累计：[host->guest, guest->host]。
    dir_bytes: [u64; 2],
    dir_frames: [u64; 2],
    dir_batches: [u64; 2],
    /// 饱和截断自证计数（任何一次累计发生饱和即 +1）。
    clamp_events: u64,
}

impl ThroughputMeter {
    /// 新表。
    pub fn new() -> Self {
        ThroughputMeter {
            buckets: [TpBucket::EMPTY; THROUGHPUT_BUCKETS],
            cursor: 0,
            current_bt: 0,
            started: false,
            filled: 0,
            ewma_bpm: 0,
            peak_bpm: 0,
            dir_bytes: [0; 2],
            dir_frames: [0; 2],
            dir_batches: [0; 2],
            clamp_events: 0,
        }
    }

    /// 毫秒纪元 → 桶下标。
    fn bucket_index(ts_us: u64) -> usize {
        ((ts_us / BUCKET_WIDTH_US) % (THROUGHPUT_BUCKETS as u64)) as usize
    }

    /// 记一笔事件（三口径 + 方向维度；溢出饱和并留自证计数）。
    pub fn record(&mut self, ts_us: u64, batch_bytes: u64, frame_count: u32, direction: TelDirection) {
        let bt = ts_us / BUCKET_WIDTH_US;
        if !self.started {
            self.started = true;
            self.cursor = Self::bucket_index(ts_us);
            self.current_bt = bt;
            self.buckets[self.cursor] = TpBucket::EMPTY;
            self.filled = 1;
        } else if bt != self.current_bt {
            self.advance_window(bt);
        }
        // 三口径入桶（显性饱和检测）。
        let b = &mut self.buckets[self.cursor];
        if batch_bytes > u64::MAX - b.bytes {
            self.clamp_events = self.clamp_events.saturating_add(1);
        }
        if frame_count as u64 > u64::MAX - b.frames {
            self.clamp_events = self.clamp_events.saturating_add(1);
        }
        b.bytes = b.bytes.saturating_add(batch_bytes);
        b.frames = b.frames.saturating_add(frame_count as u64);
        b.batches = b.batches.saturating_add(1);
        // 方向维度累计。
        let di = direction.as_index();
        if batch_bytes > u64::MAX - self.dir_bytes[di] {
            self.clamp_events = self.clamp_events.saturating_add(1);
        }
        self.dir_bytes[di] = self.dir_bytes[di].saturating_add(batch_bytes);
        self.dir_frames[di] = self.dir_frames[di].saturating_add(frame_count as u64);
        self.dir_batches[di] = self.dir_batches[di].saturating_add(1);
    }

    /// 窗口滚动：结算当前桶（EWMA/峰值），跳到新纪元并沿途清桶。
    /// 大间隔（≥64 桶）视为全窗过期，只滚一整周。
    fn advance_window(&mut self, new_bt: u64) {
        self.finalize_bucket();
        let raw_gap = new_bt.saturating_sub(self.current_bt);
        if raw_gap >= THROUGHPUT_BUCKETS as u64 {
            // 全窗过期：旧当前桶的数据也已出窗，必须随行清空
            //（滚动 63 步只会清掉沿途 63 桶，旧当前桶需单独处理）。
            self.buckets[self.cursor] = TpBucket::EMPTY;
        }
        let mut gap = raw_gap;
        if gap >= THROUGHPUT_BUCKETS as u64 {
            gap = (THROUGHPUT_BUCKETS as u64) - 1;
        }
        let mut i: u64 = 0;
        while i < gap {
            self.cursor = (self.cursor + 1) % THROUGHPUT_BUCKETS;
            self.buckets[self.cursor] = TpBucket::EMPTY;
            if self.filled < THROUGHPUT_BUCKETS {
                self.filled += 1;
            }
            i += 1;
        }
        self.current_bt = new_bt;
    }

    /// 结算当前桶：瞬时吞吐（字节/ms）→ EWMA 定点更新 + 峰值刷新。
    fn finalize_bucket(&mut self) {
        // 桶宽恰为 1ms（BUCKET_WIDTH_US = 1000µs）：桶内字节累计本身就是
        // "字节/毫秒"瞬时吞吐，无需再除（除以 1000 是常见笔误，已避开）。
        let inst_bpm = self.buckets[self.cursor].bytes;
        // EWMA 整数定点：ewma += (inst − ewma) × 273 / 1024（方向自适应，
        // 乘法前先取差值避免负数；两路均饱和防溢出）。
        if inst_bpm > self.ewma_bpm {
            let delta = inst_bpm - self.ewma_bpm;
            let step = delta.saturating_mul(EWMA_ALPHA_NUM) / EWMA_ALPHA_DEN;
            self.ewma_bpm = self.ewma_bpm.saturating_add(step);
        } else {
            let delta = self.ewma_bpm - inst_bpm;
            let step = delta.saturating_mul(EWMA_ALPHA_NUM) / EWMA_ALPHA_DEN;
            self.ewma_bpm = self.ewma_bpm.saturating_sub(step);
        }
        if inst_bpm > self.peak_bpm {
            self.peak_bpm = inst_bpm;
        }
    }

    /// 窗口报告（空窗 → 0x1323 零分母显性拒绝）。
    pub fn report(&self) -> Result<ThroughputReport, LinkTelError> {
        if !self.started || self.filled == 0 {
            return Err(LinkTelError::EmptyWindow);
        }
        let mut total_bytes: u64 = 0;
        let mut total_frames: u64 = 0;
        let mut total_batches: u64 = 0;
        let mut i = 0;
        while i < THROUGHPUT_BUCKETS {
            total_bytes = total_bytes.saturating_add(self.buckets[i].bytes);
            total_frames = total_frames.saturating_add(self.buckets[i].frames);
            total_batches = total_batches.saturating_add(self.buckets[i].batches);
            i += 1;
        }
        // 分母 = filled ≥ 1，已由空窗闸保证；均值 = 每桶均摊字节。
        let mean_bpm = total_bytes / (self.filled as u64);
        Ok(ThroughputReport {
            window_bytes: total_bytes,
            window_frames: total_frames,
            window_batches: total_batches,
            mean_bytes_per_ms: mean_bpm,
            ewma_bytes_per_ms: self.ewma_bpm,
            peak_bytes_per_ms: self.peak_bpm,
            host_to_guest_bytes: self.dir_bytes[0],
            guest_to_host_bytes: self.dir_bytes[1],
            host_to_guest_batches: self.dir_batches[0],
            guest_to_host_batches: self.dir_batches[1],
            host_to_guest_frames: self.dir_frames[0],
            guest_to_host_frames: self.dir_frames[1],
            buckets_filled: self.filled,
            clamp_events: self.clamp_events,
        })
    }

    /// 当前 EWMA（未结算的当前桶不计入）。
    pub fn ewma_bpm(&self) -> u64 {
        self.ewma_bpm
    }

    /// 峰值。
    pub fn peak_bpm(&self) -> u64 {
        self.peak_bpm
    }

    /// 饱和自证计数。
    pub fn clamp_events(&self) -> u64 {
        self.clamp_events
    }
}

/// 吞吐窗口报告（往返 = 双向合计；单向 = 按方向字段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThroughputReport {
    /// 窗口字节（往返）。
    pub window_bytes: u64,
    /// 窗口帧（往返）。
    pub window_frames: u64,
    /// 窗口批（往返）。
    pub window_batches: u64,
    /// 每桶均摊字节/ms。
    pub mean_bytes_per_ms: u64,
    /// EWMA 字节/ms（整数定点）。
    pub ewma_bytes_per_ms: u64,
    /// 峰值字节/ms。
    pub peak_bytes_per_ms: u64,
    /// 单向：host->guest 字节。
    pub host_to_guest_bytes: u64,
    /// 单向：guest->host 字节。
    pub guest_to_host_bytes: u64,
    /// 单向：host->guest 批。
    pub host_to_guest_batches: u64,
    /// 单向：guest->host 批。
    pub guest_to_host_batches: u64,
    /// 单向：host->guest 帧。
    pub host_to_guest_frames: u64,
    /// 单向：guest->host 帧。
    pub guest_to_host_frames: u64,
    /// 窗内已触及桶数。
    pub buckets_filled: usize,
    /// 饱和截断自证计数。
    pub clamp_events: u64,
}

// ============================================================================
// §8 · 积压水位观察器（BackpressureWatch）：三级滞回 + 旗传播建议
// ============================================================================

/// 积压水位观察器：三级滞回闸。
///
/// 迁移规则（滞回：退出阈 < 进入阈，防同深抖动）：
/// * Normal → Warning：depth ≥ 4096；Warning → Congested：depth ≥ 12288；
/// * Warning → Normal：depth ≤ 2867；
/// * Congested → Warning：depth ≤ 8601（不直接回 Normal，须经警戒级）。
#[derive(Debug, Clone)]
pub struct BackpressureWatch {
    warn_enter: u32,
    warn_exit: u32,
    congest_enter: u32,
    congest_exit: u32,
    level: TelBacklogLevel,
    last_depth: u32,
    /// 级迁移次数（含双向）。
    transitions: u64,
    /// 拥塞级期间收到的事件数（背压旗记账）。
    congested_events: u64,
    /// 进入拥塞级的次数。
    congested_spans: u64,
    /// 拥塞级期间事件端到端延迟累计（µs，饱和）。
    congested_e2e_us: u64,
}

impl BackpressureWatch {
    /// 默认观察器（水位取 §0 常量，序次已由编译期断言保证）。
    pub fn new() -> Self {
        BackpressureWatch {
            warn_enter: WATERMARK_WARN_ENTER,
            warn_exit: WATERMARK_WARN_EXIT,
            congest_enter: WATERMARK_CONGEST_ENTER,
            congest_exit: WATERMARK_CONGEST_EXIT,
            level: TelBacklogLevel::Normal,
            last_depth: 0,
            transitions: 0,
            congested_events: 0,
            congested_spans: 0,
            congested_e2e_us: 0,
        }
    }

    /// 自定义水位（非法序次 → 0x132C；供策略调参与测试注入）。
    pub fn try_with_watermarks(
        warn_enter: u32,
        warn_exit: u32,
        congest_enter: u32,
        congest_exit: u32,
    ) -> Result<Self, LinkTelError> {
        if !(warn_exit < warn_enter && congest_exit < congest_enter && congest_enter > warn_enter) {
            return Err(LinkTelError::WatermarkInvalid {
                warn_enter,
                warn_exit,
                congest_enter,
                congest_exit,
            });
        }
        Ok(BackpressureWatch {
            warn_enter,
            warn_exit,
            congest_enter,
            congest_exit,
            level: TelBacklogLevel::Normal,
            last_depth: 0,
            transitions: 0,
            congested_events: 0,
            congested_spans: 0,
            congested_e2e_us: 0,
        })
    }

    /// 观测一次积压深度（滞回迁移 + 拥塞期旗记账）。
    pub fn observe(&mut self, depth: u32, e2e_us: u32) -> TelBacklogLevel {
        let prev = self.level;
        let next = match prev {
            TelBacklogLevel::Normal => {
                if depth >= self.congest_enter {
                    TelBacklogLevel::Congested
                } else if depth >= self.warn_enter {
                    TelBacklogLevel::Warning
                } else {
                    TelBacklogLevel::Normal
                }
            }
            TelBacklogLevel::Warning => {
                if depth >= self.congest_enter {
                    TelBacklogLevel::Congested
                } else if depth <= self.warn_exit {
                    TelBacklogLevel::Normal
                } else {
                    TelBacklogLevel::Warning
                }
            }
            TelBacklogLevel::Congested => {
                if depth <= self.congest_exit {
                    TelBacklogLevel::Warning
                } else {
                    TelBacklogLevel::Congested
                }
            }
        };
        if next != prev {
            self.transitions = self.transitions.saturating_add(1);
            if next == TelBacklogLevel::Congested {
                self.congested_spans = self.congested_spans.saturating_add(1);
            }
        }
        self.level = next;
        self.last_depth = depth;
        if next == TelBacklogLevel::Congested {
            self.congested_events = self.congested_events.saturating_add(1);
            self.congested_e2e_us = self
                .congested_e2e_us
                .saturating_add(e2e_us as u64);
        }
        next
    }

    /// 当前水位级。
    pub fn level(&self) -> TelBacklogLevel {
        self.level
    }

    /// 最近观测深度。
    pub fn last_depth(&self) -> u32 {
        self.last_depth
    }

    /// 背压遥测旗：非正常水位即置起（背压触发的遥测旗）。
    pub fn flag(&self) -> bool {
        self.level != TelBacklogLevel::Normal
    }

    /// 级迁移次数。
    pub fn transitions(&self) -> u64 {
        self.transitions
    }

    /// 拥塞期事件数（旗记账）。
    pub fn congested_events(&self) -> u64 {
        self.congested_events
    }

    /// 进入拥塞次数。
    pub fn congested_spans(&self) -> u64 {
        self.congested_spans
    }

    /// 拥塞期事件端到端延迟累计（µs）。
    pub fn congested_e2e_us(&self) -> u64 {
        self.congested_e2e_us
    }

    /// 旗传播建议（按当前水位给出下游动作文本；拥塞期为强建议）。
    pub fn advice(&self) -> &'static str {
        match self.level {
            TelBacklogLevel::Normal => "水位正常：无需传播背压旗。",
            TelBacklogLevel::Warning => {
                "水位警戒：建议向提交侧传播降速旗，收窄批次并观察积压走向。"
            }
            TelBacklogLevel::Congested => {
                "水位拥塞：建议立即传播背压旗——暂停非关键批、分流到备用桥、\
                 并把拥塞期事件旗随发布包送下游决策。"
            }
        }
    }

    /// 配置自审计（序次合法即通过；供 health 闸）。
    pub fn config_valid(&self) -> bool {
        self.warn_exit < self.warn_enter
            && self.congest_exit < self.congest_enter
            && self.congest_enter > self.warn_enter
    }
}

// ============================================================================
// §9 · 异常账（AnomalyLedger）：三类计数 + ppm + 连续熔断
// ============================================================================

/// 异常账：丢包/重传/超时三类独立计数；异常率 ppm（零分母显性拒绝）；
/// 连续异常熔断（连续达限即开闸，开闸期间异常事件拒收入账）。
#[derive(Debug, Clone)]
pub struct AnomalyLedger {
    dropped: u64,
    retrans: u64,
    timeouts: u64,
    /// 已见事件数（异常 + 干净，分母口径）。
    events_seen: u64,
    /// 异常事件总数（三类合计）。
    anomalies_total: u64,
    /// 当前连续异常计数。
    consecutive: u32,
    breaker_open: bool,
    /// 熔断次数。
    breaker_trips: u64,
    /// 熔断复位次数。
    breaker_clears: u64,
}

impl AnomalyLedger {
    /// 新账。
    pub fn new() -> Self {
        AnomalyLedger {
            dropped: 0,
            retrans: 0,
            timeouts: 0,
            events_seen: 0,
            anomalies_total: 0,
            consecutive: 0,
            breaker_open: false,
            breaker_trips: 0,
            breaker_clears: 0,
        }
    }

    /// 记一笔异常事件（分类计数 + 连续计数 + 熔断判定）。
    /// 熔断已开时仍计数但返回 0x1328（调用方按需丢弃或旁路）。
    pub fn record_anomaly(&mut self, kind: AnomalyKind) -> Result<(), LinkTelError> {
        self.events_seen = self.events_seen.saturating_add(1);
        match kind {
            AnomalyKind::Dropped => self.dropped = self.dropped.saturating_add(1),
            AnomalyKind::Retransmitted => self.retrans = self.retrans.saturating_add(1),
            AnomalyKind::TimedOut => self.timeouts = self.timeouts.saturating_add(1),
        }
        self.anomalies_total = self.anomalies_total.saturating_add(1);
        self.consecutive = self.consecutive.saturating_add(1);
        if !self.breaker_open && self.consecutive >= CIRCUIT_CONSECUTIVE_LIMIT {
            self.breaker_open = true;
            self.breaker_trips = self.breaker_trips.saturating_add(1);
        }
        if self.breaker_open {
            return Err(LinkTelError::CircuitBreakerOpen {
                consecutive: self.consecutive,
            });
        }
        Ok(())
    }

    /// 记一笔干净事件（连续计数清零；分母照加）。
    pub fn record_clean(&mut self) {
        self.events_seen = self.events_seen.saturating_add(1);
        self.consecutive = 0;
    }

    /// 复位熔断（根因修复后由显式调用触发；复位计入账）。
    pub fn clear_breaker(&mut self) {
        if self.breaker_open {
            self.breaker_open = false;
            self.breaker_clears = self.breaker_clears.saturating_add(1);
        }
        self.consecutive = 0;
    }

    /// 熔断是否开着。
    pub fn breaker_open(&self) -> bool {
        self.breaker_open
    }

    /// 当前连续异常数。
    pub fn consecutive(&self) -> u32 {
        self.consecutive
    }

    /// 异常率 ppm（零分母显性：events_seen == 0 → 0x1323，不产伪值）。
    pub fn anomaly_ppm(&self) -> Result<u64, LinkTelError> {
        if self.events_seen == 0 {
            return Err(LinkTelError::EmptyWindow);
        }
        Ok(self.anomalies_total.saturating_mul(1_000_000) / self.events_seen)
    }

    /// 三类计数只读快照。
    pub fn counts(&self) -> (u64, u64, u64) {
        (self.dropped, self.retrans, self.timeouts)
    }

    /// 已见事件数。
    pub fn events_seen(&self) -> u64 {
        self.events_seen
    }

    /// 异常总数。
    pub fn anomalies_total(&self) -> u64 {
        self.anomalies_total
    }

    /// 熔断/复位账。
    pub fn breaker_book(&self) -> (bool, u64, u64) {
        (self.breaker_open, self.breaker_trips, self.breaker_clears)
    }
}

// ============================================================================
// §10 · schema 版本化 + 发布包（LinkSchemaV1 / LinkReleasePackV1）
// ============================================================================

/// schema V1：字段白名单 + 版本指纹 + 发布闭合闸。
pub struct LinkSchemaV1;

impl LinkSchemaV1 {
    /// schema 魔数。
    pub const MAGIC: [u8; 8] = SCHEMA_MAGIC;
    /// schema 版本。
    pub const VERSION: u32 = SCHEMA_VERSION;

    /// 字段白名单（来源：事件 V1 协议首版冻结字段集；
    /// 发布包只允许引用这些字段，白名单外一律 0x1326）。
    pub const FIELDS: [&'static str; 12] = [
        "ts_us",
        "seq",
        "direction",
        "batch_bytes",
        "frame_count",
        "queue_us",
        "copy_us",
        "proto_us",
        "wait_us",
        "e2e_us",
        "backlog_depth",
        "flags",
    ];

    /// 版本指纹（FNV-1a over 魔数 + 版本 + 白名单依序拼接；
    /// schema 任何冻结字段变动都会改变指纹，版本化纪律的锚）。
    pub fn fingerprint() -> u64 {
        let mut h = fnv1a64(FNV_OFFSET, &Self::MAGIC);
        h = fnv1a64_u64(h, Self::VERSION as u64);
        let mut i = 0;
        while i < Self::FIELDS.len() {
            h = fnv1a64(h, Self::FIELDS[i].as_bytes());
            i += 1;
        }
        h
    }

    /// 字段白名单校验（白名单外 → 0x1326）。
    pub fn is_field_allowed(name: &str) -> bool {
        let mut i = 0;
        while i < Self::FIELDS.len() {
            if Self::FIELDS[i] == name {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 版本化闸（版本未知 → 0x1327）。
    pub fn validate_header(version: u32) -> Result<(), LinkTelError> {
        if version == Self::VERSION {
            Ok(())
        }
        else {
            Err(LinkTelError::UnknownSchemaVersion { got: version })
        }
    }

    /// 发布闸 + 发布包导出（闭合纪律）。
    ///
    /// 闸序（逐闸放行，任一不过即拒）：
    /// 1. 版本闸：schema 版本必须为本模块冻结版本；
    /// 2. 帧闸：必须已 begin_frame（否则 0x1324）；
    /// 3. 闭合闸：必须已 finish_frame，未闭合拒发布（0x1325）；
    /// 4. 空窗闸：帧内零事件 → 0x1323（空窗诚实，不产空包）；
    /// 5. 守恒闸：延迟分解账守恒审计必须通过（0x1322）；
    /// 6. 熔断闸：异常熔断开着不得发布（0x1328）；
    /// 7. 体检闸：hub 体检必须全绿（0x132E）；
    /// 8. 步数闸：路径步数不得超 T 域镜像预算（0x1329）。
    pub fn export_release_pack(hub: &LinkTelHub) -> Result<LinkReleasePackV1, LinkTelError> {
        // 1. 版本闸。
        Self::validate_header(Self::VERSION)?;
        // 2. 帧闸。
        if !hub.frame_open() {
            return Err(LinkTelError::FrameNotOpen);
        }
        // 3. 闭合闸。
        if !hub.frame_closed() {
            return Err(LinkTelError::FrameNotClosed);
        }
        // 4. 空窗闸。
        if hub.ingests_ok() == 0 {
            return Err(LinkTelError::EmptyWindow);
        }
        // 5. 守恒闸。
        hub.decomposer().conservation_audit()?;
        // 6. 熔断闸。
        if hub.ledger().breaker_open() {
            return Err(LinkTelError::CircuitBreakerOpen {
                consecutive: hub.ledger().consecutive(),
            });
        }
        // 7. 体检闸。
        let health = hub.health();
        if !health.overall_ok {
            return Err(LinkTelError::HealthDegraded {
                subsystem: health.first_failed_or_empty(),
            });
        }
        // 8. 步数闸。
        if hub.path_steps_peak() > MIRROR_T_DOMAIN_INGEST_STEP_BUDGET {
            return Err(LinkTelError::BudgetExceeded {
                counted: hub.path_steps_peak(),
                budget: MIRROR_T_DOMAIN_INGEST_STEP_BUDGET,
            });
        }
        // 全闸放行 → 组包。
        let thr = hub.meter().report()?;
        let lat = hub.decomposer().latency_report()?;
        let backlog = hub.backlog_report();
        let anomalies = hub.anomaly_report();
        let mut pack = LinkReleasePackV1 {
            schema_version: Self::VERSION,
            schema_fingerprint: Self::fingerprint(),
            magic: Self::MAGIC,
            frame_closed: true,
            events_in_frame: hub.events_in_frame(),
            ingests_ok: hub.ingests_ok(),
            ingests_rejected: hub.ingests_rejected(),
            throughput: thr,
            latency: lat,
            backlog,
            anomalies,
            path_steps_last: hub.path_steps_last(),
            path_steps_peak: hub.path_steps_peak(),
            step_drift: hub.step_drift(),
            host_label_hash: 0,
            pack_fingerprint: 0,
        };
        // 隐私本地闭环：仅本地标签哈希，无原始标识出域。
        pack.host_label_hash = hub.local_label_hash();
        pack.pack_fingerprint = pack.fingerprint_self();
        Ok(pack)
    }
}

/// 发布包 V1（纯聚合计数 + 哈希；无任何原始标识字符串——隐私本地闭环）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkReleasePackV1 {
    /// schema 版本。
    pub schema_version: u32,
    /// schema 指纹。
    pub schema_fingerprint: u64,
    /// schema 魔数。
    pub magic: [u8; 8],
    /// 帧已闭合（闭合纪律的证据位）。
    pub frame_closed: bool,
    /// 帧内事件数。
    pub events_in_frame: u64,
    /// 通过入账事件数。
    pub ingests_ok: u64,
    /// 拒收入账事件数。
    pub ingests_rejected: u64,
    /// 吞吐窗口报告（三口径 + 双向分层）。
    pub throughput: ThroughputReport,
    /// 延迟五段报告。
    pub latency: LatencyReport,
    /// 积压水位报告。
    pub backlog: BacklogReport,
    /// 异常报告。
    pub anomalies: AnomalyReport,
    /// 最近一次 ingest 路径步数。
    pub path_steps_last: u32,
    /// 路径步数峰值。
    pub path_steps_peak: u32,
    /// 步数漂移计数（实际 ≠ 设计基线的次数）。
    pub step_drift: u64,
    /// 本地标签哈希（隐私本地闭环：仅哈希出域，无明文）。
    pub host_label_hash: u64,
    /// 包指纹（对全部字段规范化序列化的 FNV-1a）。
    pub pack_fingerprint: u64,
}

impl LinkReleasePackV1 {
    /// 包指纹自算（规范化小端序列化后 FNV-1a；双跑对拍比对对象）。
    pub fn fingerprint_self(&self) -> u64 {
        let mut b: Vec<u8> = Vec::with_capacity(160);
        push_u64_le(&mut b, self.schema_version as u64);
        push_u64_le(&mut b, self.schema_fingerprint);
        push_u64_le(&mut b, u64::from_le_bytes(self.magic));
        push_u64_le(&mut b, self.frame_closed as u64);
        push_u64_le(&mut b, self.events_in_frame);
        push_u64_le(&mut b, self.ingests_ok);
        push_u64_le(&mut b, self.ingests_rejected);
        push_u64_le(&mut b, self.throughput.window_bytes);
        push_u64_le(&mut b, self.throughput.window_frames);
        push_u64_le(&mut b, self.throughput.window_batches);
        push_u64_le(&mut b, self.throughput.mean_bytes_per_ms);
        push_u64_le(&mut b, self.throughput.ewma_bytes_per_ms);
        push_u64_le(&mut b, self.throughput.peak_bytes_per_ms);
        push_u64_le(&mut b, self.throughput.host_to_guest_bytes);
        push_u64_le(&mut b, self.throughput.guest_to_host_bytes);
        push_u64_le(&mut b, self.throughput.host_to_guest_batches);
        push_u64_le(&mut b, self.throughput.guest_to_host_batches);
        push_u64_le(&mut b, self.throughput.host_to_guest_frames);
        push_u64_le(&mut b, self.throughput.guest_to_host_frames);
        push_u64_le(&mut b, self.throughput.buckets_filled as u64);
        push_u64_le(&mut b, self.throughput.clamp_events);
        push_u64_le(&mut b, self.latency.queue.p95 as u64);
        push_u64_le(&mut b, self.latency.copy.p95 as u64);
        push_u64_le(&mut b, self.latency.proto.p95 as u64);
        push_u64_le(&mut b, self.latency.wait.p95 as u64);
        push_u64_le(&mut b, self.latency.e2e.p95 as u64);
        push_u64_le(&mut b, self.latency.e2e.p99 as u64);
        push_u64_le(&mut b, self.latency.conservation_violations);
        push_u64_le(&mut b, self.backlog.level.as_index() as u64);
        push_u64_le(&mut b, self.backlog.last_depth as u64);
        push_u64_le(&mut b, self.backlog.transitions);
        push_u64_le(&mut b, self.backlog.congested_events);
        push_u64_le(&mut b, self.backlog.congested_spans);
        push_u64_le(&mut b, self.backlog.congested_e2e_us);
        push_u64_le(&mut b, self.anomalies.dropped);
        push_u64_le(&mut b, self.anomalies.retransmitted);
        push_u64_le(&mut b, self.anomalies.timeouts);
        push_u64_le(&mut b, self.anomalies.anomalies_total);
        push_u64_le(&mut b, self.anomalies.ppm);
        push_u64_le(&mut b, self.anomalies.ppm_valid as u64);
        push_u64_le(&mut b, self.anomalies.consecutive as u64);
        push_u64_le(&mut b, self.anomalies.breaker_open as u64);
        push_u64_le(&mut b, self.anomalies.breaker_trips);
        push_u64_le(&mut b, self.anomalies.breaker_clears);
        push_u64_le(&mut b, self.path_steps_last as u64);
        push_u64_le(&mut b, self.path_steps_peak as u64);
        push_u64_le(&mut b, self.step_drift);
        push_u64_le(&mut b, self.host_label_hash);
        fnv1a64(FNV_OFFSET, &b)
    }
}

/// 积压水位报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BacklogReport {
    /// 当前水位级。
    pub level: TelBacklogLevel,
    /// 最近观测深度。
    pub last_depth: u32,
    /// 级迁移次数。
    pub transitions: u64,
    /// 拥塞期事件数。
    pub congested_events: u64,
    /// 进入拥塞次数。
    pub congested_spans: u64,
    /// 拥塞期端到端累计（µs）。
    pub congested_e2e_us: u64,
    /// 背压旗。
    pub flag: bool,
    /// 旗传播建议（静态文本）。
    pub advice: &'static str,
}

/// 异常报告（ppm 零分母时 ppm_valid = false，显性不产伪值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnomalyReport {
    /// 丢包数。
    pub dropped: u64,
    /// 重传数。
    pub retransmitted: u64,
    /// 超时数。
    pub timeouts: u64,
    /// 异常总数。
    pub anomalies_total: u64,
    /// 异常率 ppm（仅 ppm_valid 时可信）。
    pub ppm: u64,
    /// ppm 有效性（零分母显性旗）。
    pub ppm_valid: bool,
    /// 当前连续异常数。
    pub consecutive: u32,
    /// 熔断是否开着。
    pub breaker_open: bool,
    /// 熔断次数。
    pub breaker_trips: u64,
    /// 熔断复位次数。
    pub breaker_clears: u64,
}

// ============================================================================
// §11 · 对接遥测总账（LinkTelHub）：汇流 + 闸序 + 体检 + 开销自证
// ============================================================================

/// 体检报告（per-subsystem 全绿判据 + 总闸 + 指纹）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkHealthReport {
    /// 延迟分解账体检（有样本且守恒零违例）。
    pub decomposer_ok: bool,
    /// 吞吐表体检（有窗口且 EWMA ≤ 峰值且窗口非零）。
    pub throughput_ok: bool,
    /// 水位观察器体检（配置序次合法）。
    pub backlog_ok: bool,
    /// 异常账体检（熔断未开）。
    pub anomaly_ok: bool,
    /// 帧体检（帧已开启）。
    pub frame_ok: bool,
    /// 开销体检（路径步数峰值 ≤ T 域镜像预算）。
    pub steps_ok: bool,
    /// 总闸：以上全绿。
    pub overall_ok: bool,
    /// 体检指纹（对判定与关键计数规范化哈希）。
    pub fingerprint: u64,
}

impl LinkHealthReport {
    /// 第一个未过的子系统名（全绿返回空串）。
    pub fn first_failed_or_empty(&self) -> &'static str {
        if !self.decomposer_ok {
            "decomposer"
        } else if !self.throughput_ok {
            "throughput"
        } else if !self.backlog_ok {
            "backlog"
        } else if !self.anomaly_ok {
            "anomaly"
        } else if !self.frame_ok {
            "frame"
        } else if !self.steps_ok {
            "steps"
        } else {
            ""
        }
    }
}

/// 对接遥测总账：事件唯一入口 `ingest`，闸序：
/// 步数入口闸 → (ts,seq) 单调闸 → 四段守恒闸 → 三口径吞吐 → 水位滞回
/// → 异常旗分流 → 账本收口 → 步数自证审计。
#[derive(Debug, Clone)]
pub struct LinkTelHub {
    decomposer: LatencyDecomposer,
    meter: ThroughputMeter,
    watch: BackpressureWatch,
    ledger: AnomalyLedger,
    /// 最近收账事件 (ts, seq)（单调闸基准）。
    last_ts_us: u64,
    last_seq: u32,
    has_last: bool,
    frame_open: bool,
    frame_closed: bool,
    events_in_frame: u64,
    ingests_ok: u64,
    ingests_rejected: u64,
    /// 拒收分类账（可归因）。
    rejects_seq: u64,
    rejects_time: u64,
    rejects_conservation: u64,
    rejects_direction: u64,
    rejects_other: u64,
    path_steps_last: u32,
    path_steps_peak: u32,
    /// 步数漂移计数（实际步数 ≠ 设计基线的次数）。
    step_drift: u64,
    /// 拥塞拒收闸开关（默认关：遥测自身照收，闸供桥载荷侧问询）。
    congest_gate: bool,
    /// 本地标签哈希（隐私本地闭环：set 后随包出域，无明文）。
    local_label_hash: u64,
    /// 背压提示旗事件计数。
    hint_events: u64,
}

impl LinkTelHub {
    /// 新总账（全子系统就位，帧未开，标签哈希为 FNV("")）。
    pub fn new() -> Self {
        LinkTelHub {
            decomposer: LatencyDecomposer::new(),
            meter: ThroughputMeter::new(),
            watch: BackpressureWatch::new(),
            ledger: AnomalyLedger::new(),
            last_ts_us: 0,
            last_seq: 0,
            has_last: false,
            frame_open: false,
            frame_closed: false,
            events_in_frame: 0,
            ingests_ok: 0,
            ingests_rejected: 0,
            rejects_seq: 0,
            rejects_time: 0,
            rejects_conservation: 0,
            rejects_direction: 0,
            rejects_other: 0,
            path_steps_last: 0,
            path_steps_peak: 0,
            step_drift: 0,
            congest_gate: false,
            local_label_hash: fnv1a64(FNV_OFFSET, b""),
            hint_events: 0,
        }
    }

    /// 设置本地标签哈希（隐私：调用方先把明文换成哈希再入账）。
    pub fn set_local_label_hash(&mut self, hash: u64) {
        self.local_label_hash = hash;
    }

    /// 本地标签哈希。
    pub fn local_label_hash(&self) -> u64 {
        self.local_label_hash
    }

    /// 开关拥塞拒收闸（供桥载荷侧问询 `congest_gate_check`）。
    pub fn set_congest_gate(&mut self, on: bool) {
        self.congest_gate = on;
    }

    /// 拥塞期问询：闸开且水位拥塞 → 0x132B（非关键批应让路）。
    pub fn congest_gate_check(&self) -> Result<(), LinkTelError> {
        if self.congest_gate && self.watch.level() == TelBacklogLevel::Congested {
            Err(LinkTelError::CongestedReject)
        } else {
            Ok(())
        }
    }

    /// 开帧（重复开帧幂等；开帧重置闭合位与帧内计数）。
    pub fn begin_frame(&mut self) {
        self.frame_open = true;
        self.frame_closed = false;
        self.events_in_frame = 0;
    }

    /// 闭帧（未开帧 → 0x1324；闭合是发布前置条件）。
    pub fn finish_frame(&mut self) -> Result<(), LinkTelError> {
        if !self.frame_open {
            return Err(LinkTelError::FrameNotOpen);
        }
        self.frame_closed = true;
        Ok(())
    }

    /// 事件唯一入口（闸序见类型文档；错误路径零副作用承诺：
    /// 单调/守恒闸在任何状态变更前完成；熔断拒收时异常计数照记、
    /// 其余入账回滚语义见 §9）。
    pub fn ingest(&mut self, ev: &LinkTelEventV1) -> Result<(), LinkTelError> {
        // —— 开销自证：路径步数记账（设计步数 14，见常量基线） ——
        let mut steps: u32 = 0;
        // 闸 1：(ts,seq) 单调闸。
        steps += 1;
        if self.has_last {
            if ev.seq <= self.last_seq {
                self.rejects_seq = self.rejects_seq.saturating_add(1);
                self.ingests_rejected = self.ingests_rejected.saturating_add(1);
                return Err(LinkTelError::SeqRegression {
                    last_seq: self.last_seq,
                    got_seq: ev.seq,
                });
            }
            if ev.ts_us < self.last_ts_us {
                self.rejects_time = self.rejects_time.saturating_add(1);
                self.ingests_rejected = self.ingests_rejected.saturating_add(1);
                return Err(LinkTelError::TimeRegression {
                    last_ts_us: self.last_ts_us,
                    got_ts_us: ev.ts_us,
                });
            }
        }
        // 闸 2：四段守恒闸（分解账先验后写，Err 即零副作用）。
        steps += 1;
        if let Err(e) = self.decomposer.ingest(ev) {
            if let LinkTelError::ConservationBroken { .. } = e {
                self.rejects_conservation = self.rejects_conservation.saturating_add(1);
            } else {
                self.rejects_other = self.rejects_other.saturating_add(1);
            }
            self.ingests_rejected = self.ingests_rejected.saturating_add(1);
            return Err(e);
        }
        // 闸 3：三口径吞吐（字节/帧/批 + 方向维度）。
        steps += 1;
        self.meter
            .record(ev.ts_us, ev.batch_bytes, ev.frame_count, ev.direction);
        // 闸 4：水位滞回观测（拥塞期旗记账随观察器进行）。
        steps += 1;
        let _lvl = self.watch.observe(ev.backlog_depth, ev.e2e_us);
        // 闸 5：异常旗分流（三类计数 + 干净清零 + 熔断判定）。
        steps += 1;
        let f = ev.flags;
        if f & event_flags::FLAG_DROPPED != 0 {
            if let Err(e) = self.ledger.record_anomaly(AnomalyKind::Dropped) {
                self.ingests_rejected = self.ingests_rejected.saturating_add(1);
                return Err(e);
            }
        } else if f & event_flags::FLAG_RETRANSMIT != 0 {
            if let Err(e) = self.ledger.record_anomaly(AnomalyKind::Retransmitted) {
                self.ingests_rejected = self.ingests_rejected.saturating_add(1);
                return Err(e);
            }
        } else if f & event_flags::FLAG_TIMEOUT != 0 {
            if let Err(e) = self.ledger.record_anomaly(AnomalyKind::TimedOut) {
                self.ingests_rejected = self.ingests_rejected.saturating_add(1);
                return Err(e);
            }
        } else {
            self.ledger.record_clean();
        }
        // 闸 6：背压提示旗记账。
        steps += 1;
        if f & event_flags::FLAG_BACKPRESSURE_HINT != 0 {
            self.hint_events = self.hint_events.saturating_add(1);
        }
        // 闸 7：账本收口（单调基准 + 计数 + 帧内账）。
        steps += 1;
        self.last_ts_us = ev.ts_us;
        self.last_seq = ev.seq;
        self.has_last = true;
        self.ingests_ok = self.ingests_ok.saturating_add(1);
        self.events_in_frame = self.events_in_frame.saturating_add(1);
        // 闸 8：步数自证审计（记录 + 峰值 + 漂移检测）。
        steps += 1;
        self.path_steps_last = steps;
        if steps > self.path_steps_peak {
            self.path_steps_peak = steps;
        }
        if steps != INGEST_PATH_STEPS_EXPECTED {
            self.step_drift = self.step_drift.saturating_add(1);
        }
        if steps > MIRROR_T_DOMAIN_INGEST_STEP_BUDGET {
            self.ingests_rejected = self.ingests_rejected.saturating_add(1);
            return Err(LinkTelError::BudgetExceeded {
                counted: steps,
                budget: MIRROR_T_DOMAIN_INGEST_STEP_BUDGET,
            });
        }
        Ok(())
    }

    // —— 只读访问器（供 schema 发布闸与体检闸使用） ——

    /// 延迟分解账。
    pub fn decomposer(&self) -> &LatencyDecomposer {
        &self.decomposer
    }

    /// 吞吐表。
    pub fn meter(&self) -> &ThroughputMeter {
        &self.meter
    }

    /// 水位观察器。
    pub fn watch(&self) -> &BackpressureWatch {
        &self.watch
    }

    /// 异常账。
    pub fn ledger(&self) -> &AnomalyLedger {
        &self.ledger
    }

    /// 异常账（可变；供显式熔断复位等运维操作）。
    pub fn ledger_mut(&mut self) -> &mut AnomalyLedger {
        &mut self.ledger
    }

    /// 帧是否已开。
    pub fn frame_open(&self) -> bool {
        self.frame_open
    }

    /// 帧是否已闭合。
    pub fn frame_closed(&self) -> bool {
        self.frame_closed
    }

    /// 帧内事件数。
    pub fn events_in_frame(&self) -> u64 {
        self.events_in_frame
    }

    /// 通过入账数。
    pub fn ingests_ok(&self) -> u64 {
        self.ingests_ok
    }

    /// 拒收总数。
    pub fn ingests_rejected(&self) -> u64 {
        self.ingests_rejected
    }

    /// 拒收分类账（seq/time/conservation/direction/other）。
    pub fn reject_breakdown(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.rejects_seq,
            self.rejects_time,
            self.rejects_conservation,
            self.rejects_direction,
            self.rejects_other,
        )
    }

    /// 最近路径步数。
    pub fn path_steps_last(&self) -> u32 {
        self.path_steps_last
    }

    /// 路径步数峰值。
    pub fn path_steps_peak(&self) -> u32 {
        self.path_steps_peak
    }

    /// 步数漂移计数。
    pub fn step_drift(&self) -> u64 {
        self.step_drift
    }

    /// 背压提示旗事件数。
    pub fn hint_events(&self) -> u64 {
        self.hint_events
    }

    /// 背压遥测旗（非正常水位即起旗）。
    pub fn backpressure_flag(&self) -> bool {
        self.watch.flag()
    }

    /// 水位报告。
    pub fn backlog_report(&self) -> BacklogReport {
        BacklogReport {
            level: self.watch.level(),
            last_depth: self.watch.last_depth(),
            transitions: self.watch.transitions(),
            congested_events: self.watch.congested_events(),
            congested_spans: self.watch.congested_spans(),
            congested_e2e_us: self.watch.congested_e2e_us(),
            flag: self.watch.flag(),
            advice: self.watch.advice(),
        }
    }

    /// 拥塞期端到端累计（µs）。
    pub fn congested_e2e_us(&self) -> u64 {
        self.watch.congested_e2e_us()
    }

    /// 异常报告（ppm 零分母 → ppm_valid = false 显性化，不产伪值）。
    pub fn anomaly_report(&self) -> AnomalyReport {
        let (dropped, retrans, timeouts) = self.ledger.counts();
        let (breaker_open, trips, clears) = self.ledger.breaker_book();
        let (ppm, ppm_valid) = match self.ledger.anomaly_ppm() {
            Ok(p) => (p, true),
            Err(_) => (0, false),
        };
        AnomalyReport {
            dropped,
            retransmitted: retrans,
            timeouts,
            anomalies_total: self.ledger.anomalies_total(),
            ppm,
            ppm_valid,
            consecutive: self.ledger.consecutive(),
            breaker_open,
            breaker_trips: trips,
            breaker_clears: clears,
        }
    }

    /// 体检（逐子系统判据 + 总闸 + 指纹；空窗按诚实纪律判不通过）。
    pub fn health(&self) -> LinkHealthReport {
        let decomposer_ok = self.decomposer.accepted() > 0
            && self.decomposer.conservation_violations() == 0;
        let throughput_ok = match self.meter.report() {
            Ok(r) => {
                r.window_bytes > 0
                    && r.ewma_bytes_per_ms <= r.peak_bytes_per_ms
                    && r.buckets_filled > 0
            }
            Err(_) => false,
        };
        let backlog_ok = self.watch.config_valid();
        let anomaly_ok = !self.ledger.breaker_open();
        let frame_ok = self.frame_open;
        let steps_ok = self.path_steps_peak <= MIRROR_T_DOMAIN_INGEST_STEP_BUDGET;
        let overall_ok = decomposer_ok
            && throughput_ok
            && backlog_ok
            && anomaly_ok
            && frame_ok
            && steps_ok;
        // 指纹：判定位 + 关键计数规范化哈希。
        let mut fb: Vec<u8> = Vec::with_capacity(64);
        fb.push(decomposer_ok as u8);
        fb.push(throughput_ok as u8);
        fb.push(backlog_ok as u8);
        fb.push(anomaly_ok as u8);
        fb.push(frame_ok as u8);
        fb.push(steps_ok as u8);
        push_u64_le(&mut fb, self.ingests_ok);
        push_u64_le(&mut fb, self.ingests_rejected);
        push_u64_le(&mut fb, self.decomposer.conservation_violations());
        push_u64_le(&mut fb, self.path_steps_peak as u64);
        push_u64_le(&mut fb, self.watch.level().as_index() as u64);
        let fingerprint = fnv1a64(FNV_OFFSET, &fb);
        LinkHealthReport {
            decomposer_ok,
            throughput_ok,
            backlog_ok,
            anomaly_ok,
            frame_ok,
            steps_ok,
            overall_ok,
            fingerprint,
        }
    }
}

// ============================================================================
// §12 · 双跑确定性对拍 + 隐私本地闭环工具
// ============================================================================

/// 同一事件序列喂两个独立总账，闸到闸、账到账对拍：
/// * 每个 ingest 的结果码必须逐事件一致（0 = Ok）；
/// * 闭帧结果必须一致；
/// * 发布包指纹必须逐位一致。
/// 全对齐 → Ok(包指纹)；任何失配 → 0x132A；两边同码失败 → 原样传播该码。
pub fn determinism_double_run(events: &[LinkTelEventV1]) -> Result<u64, LinkTelError> {
    let mut a = LinkTelHub::new();
    let mut b = LinkTelHub::new();
    a.begin_frame();
    b.begin_frame();
    let mut i = 0;
    while i < events.len() {
        let ra = a.ingest(&events[i]);
        let rb = b.ingest(&events[i]);
        let ca: u16 = match ra {
            Ok(()) => 0,
            Err(e) => e.code(),
        };
        let cb: u16 = match rb {
            Ok(()) => 0,
            Err(e) => e.code(),
        };
        if ca != cb {
            return Err(LinkTelError::Nondeterministic {
                fp_a: ca as u64,
                fp_b: cb as u64,
            });
        }
        i += 1;
    }
    let fa = a.finish_frame();
    let fb = b.finish_frame();
    let cfa: u16 = match fa {
        Ok(()) => 0,
        Err(e) => e.code(),
    };
    let cfb: u16 = match fb {
        Ok(()) => 0,
        Err(e) => e.code(),
    };
    if cfa != cfb {
        return Err(LinkTelError::Nondeterministic {
            fp_a: cfa as u64,
            fp_b: cfb as u64,
        });
    }
    let pa = LinkSchemaV1::export_release_pack(&a);
    let pb = LinkSchemaV1::export_release_pack(&b);
    match (pa, pb) {
        (Ok(x), Ok(y)) => {
            if x.pack_fingerprint == y.pack_fingerprint {
                Ok(x.pack_fingerprint)
            } else {
                Err(LinkTelError::Nondeterministic {
                    fp_a: x.pack_fingerprint,
                    fp_b: y.pack_fingerprint,
                })
            }
        }
        (Err(e1), Err(e2)) if e1.code() == e2.code() => Err(e1),
        (Ok(x), Err(_)) => Err(LinkTelError::Nondeterministic {
            fp_a: x.pack_fingerprint,
            fp_b: u64::MAX,
        }),
        (Err(_), Ok(y)) => Err(LinkTelError::Nondeterministic {
            fp_a: u64::MAX,
            fp_b: y.pack_fingerprint,
        }),
        (Err(e1), Err(_)) => Err(LinkTelError::Nondeterministic {
            fp_a: e1.code() as u64,
            fp_b: u64::MAX,
        }),
    }
}

/// 隐私本地闭环工具：把本地标识明文换成本地 FNV 哈希。
/// 明文永不入账、永不入包；哈希仅用于本地对账，不出域语义由调用方保证。
pub fn local_loop_hash(label: &str) -> u64 {
    fnv1a64(FNV_OFFSET, label.as_bytes())
}

/// 发布包人类可读摘要（仅聚合数字与旗位，不含任何原始标识——隐私纪律）。
pub fn render_release_summary(pack: &LinkReleasePackV1) -> String {
    format!(
        "F3047 发布包 v{} fp={:016X} 帧{} 事件{} 通过{} 拒收{}\n\
         吞吐: 往返字节={} 帧={} 批={} EWMA={}B/ms 峰值={}B/ms\n\
         单向: h2g字节={} g2h字节={} h2g批={} g2h批={}\n\
         延迟: queue P95={} copy P95={} proto P95={} wait P95={} e2e P95/P99={}/{}\n\
         水位: {:?} 深{} 迁移{} 拥塞事件{} 旗{}\n\
         异常: 丢{} 重传{} 超时{} ppm({}) 连续{} 熔断{}\n\
         开销: 步数{} 峰值{} 漂移{}",
        pack.schema_version,
        pack.pack_fingerprint,
        pack.frame_closed as u8,
        pack.events_in_frame,
        pack.ingests_ok,
        pack.ingests_rejected,
        pack.throughput.window_bytes,
        pack.throughput.window_frames,
        pack.throughput.window_batches,
        pack.throughput.ewma_bytes_per_ms,
        pack.throughput.peak_bytes_per_ms,
        pack.throughput.host_to_guest_bytes,
        pack.throughput.guest_to_host_bytes,
        pack.throughput.host_to_guest_batches,
        pack.throughput.guest_to_host_batches,
        pack.latency.queue.p95,
        pack.latency.copy.p95,
        pack.latency.proto.p95,
        pack.latency.wait.p95,
        pack.latency.e2e.p95,
        pack.latency.e2e.p99,
        pack.backlog.level,
        pack.backlog.last_depth,
        pack.backlog.transitions,
        pack.backlog.congested_events,
        pack.backlog.flag as u8,
        pack.anomalies.dropped,
        pack.anomalies.retransmitted,
        pack.anomalies.timeouts,
        pack.anomalies.ppm,
        pack.anomalies.consecutive,
        pack.anomalies.breaker_open as u8,
        pack.path_steps_last,
        pack.path_steps_peak,
        pack.step_drift,
    )
}

// ============================================================================
// §13 · 测试（注入面：守恒/单调/滞回/发布闭合/分位/熔断/确定性/隐私/开销）
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试辅助：期望 Ok，否则以诊断码失败（内核路径禁 unwrap，
    /// 测试面用显式 match 保持同纪律）。
    fn must_ok<T>(r: Result<T, LinkTelError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!(
                "expected Ok, got {}: {}",
                e.code_str(),
                e.detail()
            ),
        }
    }

    /// 测试辅助：期望指定诊断码。
    fn must_err<T>(r: Result<T, LinkTelError>, code: u16) -> LinkTelError {
        match r {
            Ok(_) => panic!("expected err 0x{:04X}, got Ok", code),
            Err(e) => {
                assert_eq!(e.code(), code, "wrong code: {}", e.code_str());
                e
            }
        }
    }

    /// 造一个守恒合法的事件。
    #[allow(clippy::too_many_arguments)]
    fn ev(
        ts_us: u64,
        seq: u32,
        dir: TelDirection,
        bytes: u64,
        frames: u32,
        q: u32,
        c: u32,
        p: u32,
        w: u32,
        depth: u32,
        flags: u16,
    ) -> LinkTelEventV1 {
        LinkTelEventV1::new(ts_us, seq, dir, bytes, frames, q, c, p, w, depth, flags)
    }

    // ── 事件入口闸 ──

    #[test]
    fn raw_direction_gate_rejects_unknown_byte() {
        let raw = LinkTelEventV1Raw {
            ts_us: 1,
            seq: 1,
            direction_byte: 7,
            batch_bytes: 0,
            frame_count: 0,
            queue_us: 1,
            copy_us: 1,
            proto_us: 1,
            wait_us: 1,
            e2e_us: 4,
            backlog_depth: 0,
            flags: 0,
        };
        let e = must_err(LinkTelEventV1::from_raw(&raw), 0x132F);
        assert!(e.detail().contains("方向字节"));
    }

    #[test]
    fn raw_direction_gate_accepts_both_directions() {
        let mut raw = LinkTelEventV1Raw {
            ts_us: 1,
            seq: 1,
            direction_byte: 0,
            batch_bytes: 8,
            frame_count: 1,
            queue_us: 10,
            copy_us: 20,
            proto_us: 30,
            wait_us: 40,
            e2e_us: 100,
            backlog_depth: 0,
            flags: 0,
        };
        let e0 = must_ok(LinkTelEventV1::from_raw(&raw));
        assert_eq!(e0.direction, TelDirection::HostToGuest);
        raw.direction_byte = 1;
        let e1 = must_ok(LinkTelEventV1::from_raw(&raw));
        assert_eq!(e1.direction, TelDirection::GuestToHost);
        assert_eq!(e1.seg_sum_us(), 100);
    }

    // ── 四段守恒 ──

    #[test]
    fn conservation_gate_accepts_and_rejects() {
        let mut hub = LinkTelHub::new();
        let good = ev(100, 1, TelDirection::HostToGuest, 512, 2, 10, 20, 30, 40, 0, 0);
        must_ok(hub.ingest(&good));
        assert_eq!(hub.decomposer().accepted(), 1);
        assert_eq!(hub.decomposer().conservation_violations(), 0);
        // 破缺事件：e2e ≠ Σ段。
        let bad = LinkTelEventV1 {
            ts_us: 101,
            seq: 2,
            direction: TelDirection::HostToGuest,
            batch_bytes: 64,
            frame_count: 1,
            queue_us: 10,
            copy_us: 20,
            proto_us: 30,
            wait_us: 40,
            e2e_us: 999, // Σ段 = 100
            backlog_depth: 0,
            flags: 0,
        };
        let e = must_err(hub.ingest(&bad), 0x1322);
        match e {
            LinkTelError::ConservationBroken {
                e2e_us,
                seg_sum_us,
            } => {
                assert_eq!(e2e_us, 999);
                assert_eq!(seg_sum_us, 100);
            }
            _ => panic!("wrong variant"),
        }
        assert_eq!(hub.decomposer().conservation_violations(), 1);
        assert_eq!(hub.ingests_ok(), 1);
        assert_eq!(hub.ingests_rejected(), 1);
        // 守恒审计闸：有违例即报码。
        must_err(hub.decomposer().conservation_audit(), 0x1322);
    }

    #[test]
    fn conservation_gate_rejects_saturated_overflow() {
        let mut hub = LinkTelHub::new();
        // 四段之和饱和溢出后与 e2e 不等 → 守恒破缺（显性，无 panic）。
        let bad = LinkTelEventV1 {
            ts_us: 1,
            seq: 1,
            direction: TelDirection::GuestToHost,
            batch_bytes: 1,
            frame_count: 1,
            queue_us: u32::MAX,
            copy_us: u32::MAX,
            proto_us: u32::MAX,
            wait_us: u32::MAX,
            e2e_us: u32::MAX,
            backlog_depth: 0,
            flags: 0,
        };
        must_err(hub.ingest(&bad), 0x1322);
    }

    // ── (ts,seq) 单调闸 ──

    #[test]
    fn monotonic_gate_rejects_seq_regression() {
        let mut hub = LinkTelHub::new();
        must_ok(hub.ingest(&ev(100, 5, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, 0)));
        // 重复序号。
        let e = must_err(
            hub.ingest(&ev(101, 5, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, 0)),
            0x1320,
        );
        match e {
            LinkTelError::SeqRegression { last_seq, got_seq } => {
                assert_eq!(last_seq, 5);
                assert_eq!(got_seq, 5);
            }
            _ => panic!("wrong variant"),
        }
        // 倒退序号。
        must_err(
            hub.ingest(&ev(102, 4, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, 0)),
            0x1320,
        );
        let (seq_r, time_r, cons_r, _, _) = hub.reject_breakdown();
        assert_eq!(seq_r, 2);
        assert_eq!(time_r, 0);
        assert_eq!(cons_r, 0);
    }

    #[test]
    fn monotonic_gate_rejects_time_regression() {
        let mut hub = LinkTelHub::new();
        must_ok(hub.ingest(&ev(1000, 1, TelDirection::GuestToHost, 1, 1, 1, 1, 1, 1, 0, 0)));
        let e = must_err(
            hub.ingest(&ev(999, 2, TelDirection::GuestToHost, 1, 1, 1, 1, 1, 1, 0, 0)),
            0x1321,
        );
        match e {
            LinkTelError::TimeRegression {
                last_ts_us,
                got_ts_us,
            } => {
                assert_eq!(last_ts_us, 1000);
                assert_eq!(got_ts_us, 999);
            }
            _ => panic!("wrong variant"),
        }
        // ts 相等但 seq 递增：合法（时间单调"不降"）。
        must_ok(hub.ingest(&ev(1000, 3, TelDirection::GuestToHost, 1, 1, 1, 1, 1, 1, 0, 0)));
        assert_eq!(hub.ingests_ok(), 2);
    }

    // ── 分位与环形 ──

    #[test]
    fn ring_percentile_nearest_rank() {
        let mut ring = SampleRing::new();
        // 注入 600 个值 1..=600（乱序注入，检验确定性排序）。
        let mut i = 0;
        while i < RING_SAMPLES {
            let v = (((i * 37) % RING_SAMPLES) + 1) as u32; // 1..=600 的一个双射
            ring.push(v);
            i += 1;
        }
        let s = must_ok(ring.stats());
        assert_eq!(s.count, RING_SAMPLES);
        assert_eq!(s.min, 1);
        assert_eq!(s.max, 600);
        // 最近邻秩：P95 = 第 570 名 = 570；P99 = 第 594 名 = 594。
        assert_eq!(s.p95, 570);
        assert_eq!(s.p99, 594);
        assert_eq!(s.mean, 300); // (1+600)*600/2/600 = 300.5 → 300（整数向下）
    }

    #[test]
    fn ring_partial_fill_scales_rank() {
        let mut ring = SampleRing::new();
        let mut i = 1;
        while i <= 100 {
            ring.push(i as u32);
            i += 1;
        }
        let s = must_ok(ring.stats());
        assert_eq!(s.count, 100);
        // rank95 = ceil(100×0.95) = 95 → 值 95；rank99 = 99 → 值 99。
        assert_eq!(s.p95, 95);
        assert_eq!(s.p99, 99);
    }

    #[test]
    fn ring_empty_is_honest_empty() {
        let ring = SampleRing::new();
        must_err(ring.stats(), 0x1323);
    }

    #[test]
    fn ring_overwrite_keeps_newest_600() {
        let mut ring = SampleRing::new();
        let mut i = 0;
        while i < RING_SAMPLES + 50 {
            ring.push(i as u32);
            i += 1;
        }
        let s = must_ok(ring.stats());
        assert_eq!(s.count, RING_SAMPLES);
        assert_eq!(s.min, 50); // 前 50 个已被覆盖
        assert_eq!(s.max, 649);
    }

    #[test]
    fn latency_report_five_segments() {
        let mut hub = LinkTelHub::new();
        must_ok(hub.ingest(&ev(1, 1, TelDirection::HostToGuest, 8, 1, 10, 20, 30, 40, 0, 0)));
        must_ok(hub.ingest(&ev(2, 2, TelDirection::HostToGuest, 8, 1, 20, 40, 60, 80, 0, 0)));
        let r = must_ok(hub.decomposer().latency_report());
        assert_eq!(r.queue.mean, 15); // (10+20)/2
        assert_eq!(r.copy.mean, 30);
        assert_eq!(r.proto.mean, 45);
        assert_eq!(r.wait.mean, 60);
        assert_eq!(r.e2e.mean, 150);
        assert!(r.conservation_ok);
        assert_eq!(r.e2e.max, 200);
    }

    // ── 三口径吞吐 + EWMA + 方向分层 ──

    #[test]
    fn throughput_three_calibers_and_directions() {
        let mut hub = LinkTelHub::new();
        // 同一毫秒桶内 3 批：h2g 两批、g2h 一批。
        must_ok(hub.ingest(&ev(1000, 1, TelDirection::HostToGuest, 1000, 2, 1, 1, 1, 1, 0, 0)));
        must_ok(hub.ingest(&ev(1500, 2, TelDirection::HostToGuest, 500, 1, 1, 1, 1, 1, 0, 0)));
        must_ok(hub.ingest(&ev(1900, 3, TelDirection::GuestToHost, 250, 1, 1, 1, 1, 1, 0, 0)));
        let r = must_ok(hub.meter().report());
        assert_eq!(r.window_bytes, 1750);
        assert_eq!(r.window_frames, 4);
        assert_eq!(r.window_batches, 3);
        assert_eq!(r.host_to_guest_bytes, 1500);
        assert_eq!(r.guest_to_host_bytes, 250);
        assert_eq!(r.host_to_guest_batches, 2);
        assert_eq!(r.guest_to_host_batches, 1);
        assert_eq!(r.host_to_guest_frames, 3);
        assert_eq!(r.guest_to_host_frames, 1);
        assert_eq!(r.buckets_filled, 1);
        assert_eq!(r.clamp_events, 0);
    }

    #[test]
    fn throughput_ewma_updates_within_bounds() {
        let mut hub = LinkTelHub::new();
        // 连续 64+ 个毫秒桶，每桶 4096 字节 → 瞬时 4096 B/ms。
        let mut seq = 1;
        let mut ts = 0;
        let mut i = 0;
        while i < 80 {
            must_ok(hub.ingest(&ev(
                ts,
                seq,
                TelDirection::HostToGuest,
                4096,
                1,
                1,
                1,
                1,
                1,
                0,
                0,
            )));
            ts += 1000;
            seq += 1;
            i += 1;
        }
        let r = must_ok(hub.meter().report());
        assert!(r.ewma_bytes_per_ms > 0);
        assert!(r.ewma_bytes_per_ms <= r.peak_bytes_per_ms);
        assert_eq!(r.peak_bytes_per_ms, 4096);
        // 收敛后 EWMA 接近 4096（乘子 273/1024，约两桶半衰期）。
        assert!(r.ewma_bytes_per_ms >= 4000);
        assert_eq!(r.buckets_filled, THROUGHPUT_BUCKETS);
    }

    #[test]
    fn throughput_window_rolls_and_forgets_stale_buckets() {
        let mut hub = LinkTelHub::new();
        must_ok(hub.ingest(&ev(0, 1, TelDirection::HostToGuest, 6400, 1, 1, 1, 1, 1, 0, 0)));
        // 跳 200ms（> 64 桶窗口）：旧桶全部过期。
        must_ok(hub.ingest(&ev(200_000, 2, TelDirection::HostToGuest, 100, 1, 1, 1, 1, 1, 0, 0)));
        let r = must_ok(hub.meter().report());
        // 全窗过期后窗口内只剩新桶的贡献（旧桶被沿途清零）。
        assert_eq!(r.window_bytes, 100);
        assert_eq!(r.peak_bytes_per_ms, 6400); // 峰值是全历史记账，不清
        assert!(r.ewma_bytes_per_ms < 6400); // EWMA 已向下衰减
    }

    #[test]
    fn throughput_empty_window_is_honest() {
        let hub = LinkTelHub::new();
        must_err(hub.meter().report(), 0x1323);
    }

    #[test]
    fn throughput_saturating_bytes_leaves_clamp_evidence() {
        let mut hub = LinkTelHub::new();
        must_ok(hub.ingest(&ev(
            0,
            1,
            TelDirection::HostToGuest,
            u64::MAX,
            1,
            1,
            1,
            1,
            1,
            0,
            0,
        )));
        must_ok(hub.ingest(&ev(
            100,
            2,
            TelDirection::HostToGuest,
            u64::MAX,
            1,
            1,
            1,
            1,
            1,
            0,
            0,
        )));
        let r = must_ok(hub.meter().report());
        assert_eq!(r.window_bytes, u64::MAX); // 饱和不回绕
        assert!(r.clamp_events >= 1); // 饱和自证留痕
    }

    // ── 水位滞回 ──

    #[test]
    fn watermark_hysteresis_full_cycle() {
        let mut w = BackpressureWatch::new();
        assert_eq!(w.level(), TelBacklogLevel::Normal);
        assert!(!w.flag());
        // 4095 仍正常。
        assert_eq!(w.observe(4095, 10), TelBacklogLevel::Normal);
        // 4096 → 警戒。
        assert_eq!(w.observe(4096, 10), TelBacklogLevel::Warning);
        assert!(w.flag());
        // 3000（> 退出阈 2867）维持警戒（滞回）。
        assert_eq!(w.observe(3000, 10), TelBacklogLevel::Warning);
        // 2867 → 回正常。
        assert_eq!(w.observe(2867, 10), TelBacklogLevel::Normal);
        // 直接深跳 12288 → 拥塞（Normal 可一步到 Congested）。
        assert_eq!(w.observe(12288, 25), TelBacklogLevel::Congested);
        assert_eq!(w.congested_spans(), 1);
        assert_eq!(w.congested_events(), 1);
        assert_eq!(w.congested_e2e_us(), 25);
        // 9000（> 退出阈 8601）维持拥塞。
        assert_eq!(w.observe(9000, 15), TelBacklogLevel::Congested);
        assert_eq!(w.congested_events(), 2);
        // 8601 → 降到警戒（不直落正常）。
        assert_eq!(w.observe(8601, 10), TelBacklogLevel::Warning);
        // 4000 在警戒级维持（低于进入阈 4096，高于退出阈 2867）。
        assert_eq!(w.observe(4000, 10), TelBacklogLevel::Warning);
        // 2867 → 正常。
        assert_eq!(w.observe(2867, 10), TelBacklogLevel::Normal);
        assert_eq!(w.transitions(), 5);
        assert!(w.config_valid());
    }

    #[test]
    fn watermark_config_gate_rejects_bad_order() {
        let e = must_err(
            BackpressureWatch::try_with_watermarks(100, 200, 300, 50),
            0x132C,
        );
        assert!(e.detail().contains("水位配置"));
    }

    #[test]
    fn watermark_advice_texts_differ_by_level() {
        let mut w = BackpressureWatch::new();
        let a0 = w.advice();
        w.observe(WATERMARK_CONGEST_ENTER, 1);
        let a2 = w.advice();
        assert!(a0.contains("正常"));
        assert!(a2.contains("拥塞"));
        assert!(!a0.eq(a2));
    }

    // ── 异常三类 + ppm + 熔断 ──

    #[test]
    fn anomaly_three_kinds_counted() {
        let mut hub = LinkTelHub::new();
        must_ok(hub.ingest(&ev(1, 1, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, event_flags::FLAG_DROPPED)));
        must_ok(hub.ingest(&ev(2, 2, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, event_flags::FLAG_RETRANSMIT)));
        must_ok(hub.ingest(&ev(3, 3, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, event_flags::FLAG_TIMEOUT)));
        must_ok(hub.ingest(&ev(4, 4, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, 0)));
        let ar = hub.anomaly_report();
        assert_eq!(ar.dropped, 1);
        assert_eq!(ar.retransmitted, 1);
        assert_eq!(ar.timeouts, 1);
        assert_eq!(ar.anomalies_total, 3);
        assert!(ar.ppm_valid);
        assert_eq!(ar.ppm, 750_000); // 3/4 = 750000 ppm
        assert!(!ar.breaker_open);
    }

    #[test]
    fn anomaly_ppm_zero_denominator_is_explicit() {
        let ledger = AnomalyLedger::new();
        must_err(ledger.anomaly_ppm(), 0x1323);
    }

    #[test]
    fn circuit_breaker_trips_and_rejects_then_clears() {
        let mut hub = LinkTelHub::new();
        // 连续 16 次异常 → 第 16 次熔断开闸。
        let mut seq = 1;
        let mut ts = 0;
        let mut i = 0;
        while i < CIRCUIT_CONSECUTIVE_LIMIT as usize {
            ts += 1;
            let r = hub.ingest(&ev(ts, seq, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, event_flags::FLAG_DROPPED));
            seq += 1;
            if i + 1 < CIRCUIT_CONSECUTIVE_LIMIT as usize {
                must_ok(r);
            } else {
                // 达限那次：熔断开闸，事件被拒。
                must_err(r, 0x1328);
            }
            i += 1;
        }
        assert!(hub.ledger().breaker_open());
        assert_eq!(hub.anomaly_report().breaker_trips, 1);
        // 开闸期间继续拒收。
        must_err(
            hub.ingest(&ev(ts + 1, seq, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, event_flags::FLAG_DROPPED)),
            0x1328,
        );
        // 复位后恢复正常收账。
        hub.ledger_mut().clear_breaker();
        assert!(!hub.ledger().breaker_open());
        must_ok(hub.ingest(&ev(ts + 2, seq + 1, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, 0)));
        let ar = hub.anomaly_report();
        assert_eq!(ar.breaker_clears, 1);
        assert_eq!(ar.consecutive, 0); // 干净事件清零连续计数
    }

    // ── 发布闭合纪律 ──

    #[test]
    fn publish_requires_begin_then_finish_then_data() {
        let mut hub = LinkTelHub::new();
        // 未开帧 → 0x1324。
        must_err(LinkSchemaV1::export_release_pack(&hub), 0x1324);
        hub.begin_frame();
        // 开而未闭 → 0x1325（未闭合拒发布）。
        must_err(LinkSchemaV1::export_release_pack(&hub), 0x1325);
        must_ok(hub.finish_frame());
        // 闭合但空窗 → 0x1323（空窗诚实）。
        must_err(LinkSchemaV1::export_release_pack(&hub), 0x1323);
        // 有数据 + 重新闭合 → 可发布。
        hub.begin_frame();
        must_ok(hub.ingest(&ev(10, 1, TelDirection::HostToGuest, 64, 1, 5, 5, 5, 5, 0, 0)));
        must_ok(hub.finish_frame());
        let pack = must_ok(LinkSchemaV1::export_release_pack(&hub));
        assert!(pack.frame_closed);
        assert_eq!(pack.events_in_frame, 1);
        assert!(pack.schema_fingerprint != 0);
        assert!(pack.pack_fingerprint != 0);
        // 指纹自洽：重算一致。
        assert_eq!(pack.pack_fingerprint, pack.fingerprint_self());
    }

    #[test]
    fn finish_frame_without_begin_is_rejected() {
        let mut hub = LinkTelHub::new();
        must_err(hub.finish_frame(), 0x1324);
        hub.begin_frame();
        must_ok(hub.finish_frame());
        // 已闭帧后再闭幂等。
        must_ok(hub.finish_frame());
    }

    #[test]
    fn publish_denied_when_breaker_open() {
        let mut hub = LinkTelHub::new();
        hub.begin_frame();
        let mut seq = 1;
        let mut ts = 0;
        let mut i = 0;
        while i < CIRCUIT_CONSECUTIVE_LIMIT as usize {
            ts += 1;
            let _ = hub.ingest(&ev(ts, seq, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, 0, event_flags::FLAG_TIMEOUT));
            seq += 1;
            i += 1;
        }
        hub.finish_frame();
        // 熔断闸拦截发布。
        let e = must_err(LinkSchemaV1::export_release_pack(&hub), 0x1328);
        assert_eq!(e.code_str(), "0x1328");
    }

    // ── schema 白名单 + 版本指纹 ──

    #[test]
    fn schema_whitelist_and_fingerprint_stable() {
        assert!(LinkSchemaV1::is_field_allowed("queue_us"));
        assert!(LinkSchemaV1::is_field_allowed("e2e_us"));
        assert!(!LinkSchemaV1::is_field_allowed("host_raw_name"));
        assert!(!LinkSchemaV1::is_field_allowed(""));
        must_ok(LinkSchemaV1::validate_header(1));
        must_err(LinkSchemaV1::validate_header(2), 0x1327);
        // 指纹稳定：两次调用一致，且非零。
        assert_eq!(LinkSchemaV1::fingerprint(), LinkSchemaV1::fingerprint());
        assert!(LinkSchemaV1::fingerprint() != 0);
        assert_eq!(LinkSchemaV1::FIELDS.len(), 12);
    }

    // ── 体检 ──

    #[test]
    fn health_empty_hub_fails_honestly() {
        let hub = LinkTelHub::new();
        let h = hub.health();
        assert!(!h.overall_ok);
        assert_eq!(h.first_failed_or_empty(), "decomposer"); // 空窗诚实
    }

    #[test]
    fn health_all_green_after_data_and_frame() {
        let mut hub = LinkTelHub::new();
        hub.begin_frame();
        must_ok(hub.ingest(&ev(10, 1, TelDirection::HostToGuest, 64, 1, 5, 5, 5, 5, 0, 0)));
        must_ok(hub.finish_frame());
        let h = hub.health();
        assert!(h.decomposer_ok);
        assert!(h.throughput_ok);
        assert!(h.backlog_ok);
        assert!(h.anomaly_ok);
        assert!(h.frame_ok);
        assert!(h.steps_ok);
        assert!(h.overall_ok);
        assert_eq!(h.first_failed_or_empty(), "");
        assert!(h.fingerprint != 0);
    }

    // ── 开销自证 ──

    #[test]
    fn overhead_steps_match_design_baseline() {
        let mut hub = LinkTelHub::new();
        must_ok(hub.ingest(&ev(1, 1, TelDirection::HostToGuest, 8, 1, 1, 1, 1, 1, 0, 0)));
        assert_eq!(hub.path_steps_last(), INGEST_PATH_STEPS_EXPECTED);
        assert_eq!(hub.step_drift(), 0);
        assert!(hub.path_steps_peak() <= MIRROR_T_DOMAIN_INGEST_STEP_BUDGET);
    }

    // ── 拥塞闸问询 ──

    #[test]
    fn congest_gate_allows_by_default_and_blocks_when_enabled() {
        let mut hub = LinkTelHub::new();
        must_ok(hub.congest_gate_check()); // 默认关
        hub.set_congest_gate(true);
        must_ok(hub.congest_gate_check()); // 水位正常
        // 推到拥塞。
        let mut seq = 1;
        let mut ts = 0;
        let mut i = 0;
        while i < 4 {
            ts += 1;
            must_ok(hub.ingest(&ev(ts, seq, TelDirection::HostToGuest, 1, 1, 1, 1, 1, 1, WATERMARK_CONGEST_ENTER, 0)));
            seq += 1;
            i += 1;
        }
        assert!(hub.backpressure_flag());
        must_err(hub.congest_gate_check(), 0x132B);
        // 旗传播建议为拥塞强建议。
        assert!(hub.watch().advice().contains("背压旗"));
    }

    // ── 双跑确定性对拍 ──

    #[test]
    fn determinism_double_run_identical_sequence() {
        let mut events: Vec<LinkTelEventV1> = Vec::new();
        let mut seq = 1;
        let mut ts = 0;
        let mut i = 0;
        while i < 64 {
            ts += 700;
            let depth = if i % 16 == 0 { WATERMARK_WARN_ENTER } else { 100 };
            let flags = if i % 32 == 0 { event_flags::FLAG_RETRANSMIT } else { 0 };
            events.push(ev(
                ts,
                seq,
                if i % 2 == 0 { TelDirection::HostToGuest } else { TelDirection::GuestToHost },
                (i + 1) as u64 * 128,
                2,
                10 + (i % 7) as u32,
                20,
                5,
                5 + (i % 3) as u32,
                depth,
                flags,
            ));
            seq += 1;
            i += 1;
        }
        let fp = must_ok(determinism_double_run(&events));
        assert!(fp != 0);
    }

    #[test]
    fn determinism_probe_rejects_empty_sequence_honestly() {
        // 空序列：两边同码失败（未开帧已修复为开帧后空窗），原样传播而非误报失配。
        let events: Vec<LinkTelEventV1> = Vec::new();
        must_err(determinism_double_run(&events), 0x1323); // 空窗同码
    }

    // ── 隐私本地闭环 ──

    #[test]
    fn privacy_local_hash_stable_and_pack_has_no_raw_label() {
        let h1 = local_loop_hash("host-alpha-01");
        let h2 = local_loop_hash("host-alpha-01");
        let h3 = local_loop_hash("host-alpha-02");
        assert_eq!(h1, h2);
        assert!(h1 != h3);
        let mut hub = LinkTelHub::new();
        hub.set_local_label_hash(h1);
        hub.begin_frame();
        must_ok(hub.ingest(&ev(1, 1, TelDirection::HostToGuest, 8, 1, 1, 1, 1, 1, 0, 0)));
        must_ok(hub.finish_frame());
        let pack = must_ok(LinkSchemaV1::export_release_pack(&hub));
        assert_eq!(pack.host_label_hash, h1);
        // 摘要文本只含聚合与哈希，无明文标签。
        let s = render_release_summary(&pack);
        assert!(s.contains("F3047"));
        assert!(!s.contains("host-alpha-01"));
    }

    // ── 端到端集成：全链路走一遍 ──

    #[test]
    fn end_to_end_pipeline_smoke() {
        let mut hub = LinkTelHub::new();
        hub.set_local_label_hash(local_loop_hash("bridge-7"));
        hub.begin_frame();
        // 一条混合负载：双向、水位起伏、偶发异常、守恒合法。
        let mut seq = 1;
        let mut ts = 100;
        let mut i = 0;
        while i < 128 {
            ts += 250;
            let depth = match i % 32 {
                0..=15 => 200,
                16..=23 => WATERMARK_WARN_ENTER + 10,
                24..=30 => 3000,
                _ => WATERMARK_CONGEST_ENTER,
            };
            let flags = if i % 64 == 0 { event_flags::FLAG_TIMEOUT } else { 0 };
            let q = (i % 11) as u32 + 1;
            let c = (i % 5) as u32 + 2;
            let p = 3;
            let w = (i % 3) as u32 + 1;
            must_ok(hub.ingest(&ev(
                ts,
                seq,
                if i % 3 == 0 { TelDirection::GuestToHost } else { TelDirection::HostToGuest },
                (i + 1) as u64 * 64,
                (i % 4 + 1) as u32,
                q,
                c,
                p,
                w,
                depth,
                flags,
            )));
            seq += 1;
            i += 1;
        }
        must_ok(hub.finish_frame());
        // 体检全绿。
        let h = hub.health();
        assert!(h.overall_ok, "failed at: {}", h.first_failed_or_empty());
        // 发布。
        let pack = must_ok(LinkSchemaV1::export_release_pack(&hub));
        assert_eq!(pack.ingests_ok, 128);
        assert_eq!(pack.throughput.window_batches, 128);
        assert!(pack.throughput.host_to_guest_bytes > 0);
        assert!(pack.throughput.guest_to_host_bytes > 0);
        assert!(pack.latency.conservation_ok);
        assert!(pack.anomalies.ppm_valid);
        assert_eq!(pack.anomalies.timeouts, 2); // i = 0 与 64
        assert!(pack.backlog.flag); // 窗口末期仍在拥塞/警戒级
        assert!(pack.backlog.congested_events > 0);
        // 双跑对拍复现。
        let mut replay: Vec<LinkTelEventV1> = Vec::new();
        let mut j = 0;
        let mut ts2 = 100;
        while j < 128 {
            ts2 += 250;
            let depth = match j % 32 {
                0..=15 => 200,
                16..=23 => WATERMARK_WARN_ENTER + 10,
                24..=30 => 3000,
                _ => WATERMARK_CONGEST_ENTER,
            };
            let flags = if j % 64 == 0 { event_flags::FLAG_TIMEOUT } else { 0 };
            let q = (j % 11) as u32 + 1;
            let c = (j % 5) as u32 + 2;
            let p = 3;
            let w = (j % 3) as u32 + 1;
            replay.push(LinkTelEventV1::new(
                ts2,
                (j + 1) as u32,
                if j % 3 == 0 { TelDirection::GuestToHost } else { TelDirection::HostToGuest },
                (j + 1) as u64 * 64,
                (j % 4 + 1) as u32,
                q,
                c,
                p,
                w,
                depth,
                flags,
            ));
            j += 1;
        }
        let fp = must_ok(determinism_double_run(&replay));
        assert!(fp != 0); // 对拍双跑自身指纹一致（默认标签哈希口径）
        // 独立重放的发布指纹应与原包一致（同序列同报告，同标签哈希）。
        let mut hub2 = LinkTelHub::new();
        hub2.set_local_label_hash(local_loop_hash("bridge-7"));
        hub2.begin_frame();
        let mut k = 0;
        while k < replay.len() {
            must_ok(hub2.ingest(&replay[k]));
            k += 1;
        }
        must_ok(hub2.finish_frame());
        let pack2 = must_ok(LinkSchemaV1::export_release_pack(&hub2));
        assert_eq!(pack.pack_fingerprint, pack2.pack_fingerprint);
    }
}
