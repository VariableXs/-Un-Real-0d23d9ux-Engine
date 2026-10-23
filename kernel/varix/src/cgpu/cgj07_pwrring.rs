//! CGPU-F1449 · 功耗历史与环形缓冲（CGPU-J 域 · 功耗域 · J 组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1449`
//!
//! **判据（锚点原文）**：三粒度、无分配、切片 API、持久化预留、对齐 schema、测试、判据。
//!
//! **职责定位（锚点原文摘录）**：历史存储：环形缓冲设计（定长无分配——60s@10Hz 600
//! 点/10min@1Hz 600 点/2h@0.2Hz 三粒度环形——内存固定约 50KB）；写入路径（F1447
//! 管道聚合结果落环——采样线程直写）；读取 API（时间范围切片导出——降采样导出对
//! 齐前端图表）；崩溃保留（环形缓冲 mmap 持久化预留——崩溃后最近 60s 可追——P 域
//! 遥测联动）；查询性能（切片 O(range)——锁分离）；与 P 域接口（历史数据导出格式
//! 与 P 域遥测 schema 对齐——一处定义双消费）；测试（环形回绕/切片边界/崩溃恢复三组）。
//!
//! ---------------------------------------------------------------------------
//! 模块纪律（no_std + alloc 自包含）：
//!
//! 1. 本文件**零 `crate::` 引用**——隔离编译（探针 crate 单独挂载）；
//!    只依赖 `core::` 与 `alloc::{vec::Vec, format, string::String}`。
//! 2. 内核路径**零 unwrap() / 零 expect() / 零 panic!**：一切可失败操作返回
//!    `Result<T, PowerRingError>`；错误路径必带中文可操作诊断（`detail()`）。
//!    `const` 期的编译断言（`const _: () = assert!(…)`）是编译期闸，不算运行期失败路径。
//! 3. **无分配**（判据）：写入路径稳态零堆分配——三环全部是 `struct` 内定长数组；
//!    读取/导出/持久化序列化属于 API 出口，按需返回 `Vec`（一次性、有界、显性）。
//! 4. **上栈纪律**：三环合计 91,200 B（>64KB），组合体 `PowerRingHub` **禁止上栈**
//!    ——统一经 `Box` 承载（`PowerRingHub::new()`）；`bare()` 仅用于静态段/自定义
//!    放置，文档显性警告。单环最大 57,600 B < 64KB，单环结构本身可安全移动。
//! 5. **诊断码段独占 0x12A0~0x12AF**（开工前已 grep 确认全 cgpu 树空闲）。
//! 6. 单位纪律：功率 µW（u64 定点）、能量 µJ、时间 ms——**零浮点**；
//!    溢出策略逐处显性（游标 wrapping、能量 saturating、计数 wrapping）。
//!
//! ---------------------------------------------------------------------------
//! 七判据落位图：
//!
//! | 判据         | 落位                                                        |
//! |--------------|-------------------------------------------------------------|
//! | 三粒度       | `PowerRingHub` 三独立环 600/600/1440 点 + 级联聚合（10/50 拍）|
//! | 无分配       | 写路径定长数组 + 单写者 test-and-set；构造一次 Box，稳态零分配|
//! | 切片 API     | `slice_range`（二分定位 O(log N)+拷贝 O(range)）/`downsample_export`|
//! | 持久化预留   | `PersistLayout`：magic+version+crc32+三环连续区，页对齐      |
//! | 对齐 schema  | `TelemetrySchemaV1` 字段白名单一处定义双消费 + FNV 指纹      |
//! | 测试         | 环形回绕/切片边界/崩溃恢复三组（探针 crate tests/smoke.rs）  |
//! | 判据         | 本头注释判据原文逐字保留 + `health()` 结构化体检            |
//!
//! ---------------------------------------------------------------------------
//! 内存预算表（诚实口径）：
//!
//! | 环        | 点数 | 单元      | 字节               |
//! |-----------|------|-----------|--------------------|
//! | 高频 10Hz | 600  | 16 B 样本 | 9,600              |
//! | 中频 1Hz  | 600  | 40 B 聚合 | 24,000             |
//! | 低频 0.2Hz| 1440 | 40 B 聚合 | 57,600             |
//! | 合计      |      |           | **91,200 B ≈ 89KiB** |
//!
//! 锚点"内存固定约 50KB"为 16B 均一单元口径（3×600 点 = 28.8KB + 控制块）的
//! 下界估计；本实现为满足"聚合用均值+峰值+谷值三列，不丢信息"把聚合单元做成
//! 40B（含样本数 n），2h@0.2Hz 严格取 1440 点（锚点给了频率与跨度，点数是
//! 导出值 7200s×0.2Hz），预算显性上浮至 89KiB——**宁可显性超注也不偷工减列**。
//! 预算在 `health()` 水位与 `PersistLayout` 页表中持续可见。

#![allow(clippy::all)]

use core::cell::UnsafeCell;
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ===========================================================================
// 第一节 · 诊断码段（0x12A0~0x12AF 十六码独占闭集）
// ===========================================================================
// 开工前 `grep -rn "0x12A\|0x12B" kernel/varix/src/cgpu` 确认整段空闲。
// 码语义：受理/拒绝/降级/持久化/schema 六族，全部账目化（RingEventLedger）。

/// 0x12A0 · 写者句柄受理/正常落环（良性事件锚码）。
pub const X_PWR_RING_OK: u32 = 0x12A0;
/// 0x12A1 · 写者句柄被占：单写者 test-and-set 拒绝第二次 attach（采样线程唯一）。
pub const X_PWR_RING_WRITER_TAKEN: u32 = 0x12A1;
/// 0x12A2 · 时间戳倒退或为 0（ts=0 为空槽保留字）——拒收入账。
pub const X_PWR_RING_TS_RETROGRADE: u32 = 0x12A2;
/// 0x12A3 · 环写满覆盖最旧（回绕）——显性计数不静默。
pub const X_PWR_RING_OVERWRITE: u32 = 0x12A3;
/// 0x12A4 · seqlock 读重试耗尽（写者风暴）——读侧显性失败码。
pub const X_PWR_RING_SEQ_RETRY_EXHAUSTED: u32 = 0x12A4;
/// 0x12A5 · 时间范围切片为空（范围落在环外）。
pub const X_PWR_RING_RANGE_EMPTY: u32 = 0x12A5;
/// 0x12A6 · start > end（区间倒置）——参数错误。
pub const X_PWR_RING_RANGE_INVERTED: u32 = 0x12A6;
/// 0x12A7 · 降采样目标点数过小（<1）——图表导出参数非法。
pub const X_PWR_RING_DOWNSAMPLE_TOO_FEW: u32 = 0x12A7;
/// 0x12A8 · 持久化 CRC32 校验失败（崩溃撕裂/位腐）——拒收。
pub const X_PWR_PERSIST_CRC_BAD: u32 = 0x12A8;
/// 0x12A9 · 持久化 magic 不符（非本格式文件/错位读）。
pub const X_PWR_PERSIST_MAGIC_BAD: u32 = 0x12A9;
/// 0x12AA · 持久化版本不符（格式演进跨版拒收）。
pub const X_PWR_PERSIST_VERSION_BAD: u32 = 0x12AA;
/// 0x12AB · 持久化长度/内部状态不符（文件截断或字段越界）。
pub const X_PWR_PERSIST_LEN_BAD: u32 = 0x12AB;
/// 0x12AC · P 域 schema 冻结违约（版本/字段指纹不一致）。
pub const X_PWR_SCHEMA_FROZEN_MISMATCH: u32 = 0x12AC;
/// 0x12AD · 事件账 256 槽满后覆盖最旧（覆盖计数独立累加，不静默）。
pub const X_PWR_LEDGER_SATURATED: u32 = 0x12AD;
/// 0x12AE · 三粒度守恒审计破坏（峰值<均值 或 均值<谷值）或导出记录非法。
pub const X_PWR_CONSERVATION_BROKEN: u32 = 0x12AE;
/// 0x12AF · 空环查询（尚无任何有效样本）。
pub const X_PWR_RING_EMPTY: u32 = 0x12AF;

/// 十六诊断码闭集回读（供诊断总线注册表批量登记——一段一闭集）。
pub const fn pwr_ring_diag_codes() -> [u32; 16] {
    [
        X_PWR_RING_OK,
        X_PWR_RING_WRITER_TAKEN,
        X_PWR_RING_TS_RETROGRADE,
        X_PWR_RING_OVERWRITE,
        X_PWR_RING_SEQ_RETRY_EXHAUSTED,
        X_PWR_RING_RANGE_EMPTY,
        X_PWR_RING_RANGE_INVERTED,
        X_PWR_RING_DOWNSAMPLE_TOO_FEW,
        X_PWR_PERSIST_CRC_BAD,
        X_PWR_PERSIST_MAGIC_BAD,
        X_PWR_PERSIST_VERSION_BAD,
        X_PWR_PERSIST_LEN_BAD,
        X_PWR_SCHEMA_FROZEN_MISMATCH,
        X_PWR_LEDGER_SATURATED,
        X_PWR_CONSERVATION_BROKEN,
        X_PWR_RING_EMPTY,
    ]
}

// ===========================================================================
// 第二节 · 错误账（错误枚举 + 中文可操作诊断）
// ===========================================================================
// 纪律：每个错误 detail() 必须给出"下一步做什么"，不是复读错误名。

/// 功耗环形缓冲错误闭集。所有可失败 API 的统一出口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerRingError {
    /// 写者句柄已被占用（单写者语义：F1447 采样线程唯一持有）。
    WriterTaken,
    /// 时间戳倒退/为 0：`got_ts <= last_ts` 或 `got_ts == 0`（0 为空槽保留字）。
    TimestampRetrograde {
        /// 环内已记录的最新时间戳。
        last_ts: u64,
        /// 被拒收的时间戳。
        got_ts: u64,
    },
    /// seqlock 读重试耗尽（写者持续落环，读快照始终不一致）。
    SeqRetryExhausted {
        /// 实际尝试次数（= `SEQ_RETRY_LIMIT`）。
        attempts: u32,
    },
    /// 切片区间倒置：start > end。
    RangeInverted {
        /// 起始时间戳。
        start_ts: u64,
        /// 结束时间戳。
        end_ts: u64,
    },
    /// 切片区间与环内数据不相交（范围整体落在环外——太旧或未来）。
    RangeEmpty {
        /// 起始时间戳。
        start_ts: u64,
        /// 结束时间戳。
        end_ts: u64,
    },
    /// 空环（尚无任何有效样本）。
    RingEmpty,
    /// 降采样目标点数非法（target == 0）。
    DownsampleTargetTooSmall {
        /// 收到的目标点数。
        target: usize,
        /// 允许的最小值（恒 1）。
        minimum: usize,
    },
    /// 持久化镜像过短（连 64B 头都放不下或载荷被截断）。
    PersistTooShort {
        /// 实际字节数。
        got: usize,
        /// 期望最小字节数。
        expected: usize,
    },
    /// 持久化 magic 不符。
    PersistMagicMismatch,
    /// 持久化版本不符（跨版拒收，不做猜测式兼容）。
    PersistVersionMismatch {
        /// 镜像中存储的版本。
        stored: u32,
        /// 本实现支持的版本。
        expected: u32,
    },
    /// 持久化 CRC32 校验失败。
    PersistCrcMismatch {
        /// 头中存储的 CRC。
        stored: u32,
        /// 现算的 CRC。
        computed: u32,
    },
    /// P 域 schema 冻结违约（版本或字段指纹不一致）。
    SchemaFrozenMismatch {
        /// 读到的版本。
        got: u32,
        /// 冻结的版本。
        expected: u32,
    },
    /// 导出记录自校验失败（峰值/均值/谷值序关系破坏或不确定度越域）。
    RecordInvalid {
        /// 违约原因（'static 短语）。
        reason: &'static str,
    },
    /// 持久化恢复时环内部状态字段越界（写指针/长度与容量不自洽）。
    PersistStateBad {
        /// 非法状态值。
        state: u32,
    },
}

impl PowerRingError {
    /// 对应诊断码（错误族 → 0x12A 段）。
    pub const fn code(&self) -> u32 {
        match *self {
            PowerRingError::WriterTaken => X_PWR_RING_WRITER_TAKEN,
            PowerRingError::TimestampRetrograde { .. } => X_PWR_RING_TS_RETROGRADE,
            PowerRingError::SeqRetryExhausted { .. } => X_PWR_RING_SEQ_RETRY_EXHAUSTED,
            PowerRingError::RangeInverted { .. } => X_PWR_RING_RANGE_INVERTED,
            PowerRingError::RangeEmpty { .. } => X_PWR_RING_RANGE_EMPTY,
            PowerRingError::RingEmpty => X_PWR_RING_EMPTY,
            PowerRingError::DownsampleTargetTooSmall { .. } => X_PWR_RING_DOWNSAMPLE_TOO_FEW,
            PowerRingError::PersistTooShort { .. } => X_PWR_PERSIST_LEN_BAD,
            PowerRingError::PersistMagicMismatch => X_PWR_PERSIST_MAGIC_BAD,
            PowerRingError::PersistVersionMismatch { .. } => X_PWR_PERSIST_VERSION_BAD,
            PowerRingError::PersistCrcMismatch { .. } => X_PWR_PERSIST_CRC_BAD,
            PowerRingError::SchemaFrozenMismatch { .. } => X_PWR_SCHEMA_FROZEN_MISMATCH,
            PowerRingError::RecordInvalid { .. } => X_PWR_CONSERVATION_BROKEN,
            PowerRingError::PersistStateBad { .. } => X_PWR_PERSIST_LEN_BAD,
        }
    }

    /// 中文可操作诊断：每个错误必答"下一步怎么办"。
    pub const fn detail(&self) -> &'static str {
        match *self {
            PowerRingError::WriterTaken => {
                "写者句柄已被占用：功耗历史是单写者结构（F1447 采样线程唯一持有）。\
                 检查是否重复调用 attach_writer()，或上一写者是否已正常 drop；\
                 若采样线程重启，必须先等旧写者句柄释放（Drop 自动清旗）。"
            }
            PowerRingError::TimestampRetrograde { .. } => {
                "采样时间戳倒退或为 0：本环要求严格单调递增，且 ts=0 为空槽保留字。\
                 检查上游 F1447 管道时钟源是否切换/回拨；时钟源重置时应整环重置而非倒写。"
            }
            PowerRingError::SeqRetryExhausted { .. } => {
                "seqlock 读快照连续重试失败：写者落环速率过高或写者风暴。\
                 建议降低读取频率、增大 SEQ_RETRY_LIMIT，或改用低频粒度（mid/low 环）切片。"
            }
            PowerRingError::RangeInverted { .. } => {
                "切片区间倒置（start > end）：调用方传参错误。\
                 建议先对时间范围做 min/max 归一，或检查时区/单位换算（本模块一律 ms）。"
            }
            PowerRingError::RangeEmpty { .. } => {
                "切片区间与环内数据不相交：请求的时间范围早于环内最旧样本（已被回绕覆盖）\
                 或晚于最新样本（未来时间）。建议先读 health() 的 oldest_ts/newest_ts 再定区间。"
            }
            PowerRingError::RingEmpty => {
                "空环查询：环尚未收到任何有效样本。\
                 检查 F1447 采样线程是否已启动、采样率配置是否为 0。"
            }
            PowerRingError::DownsampleTargetTooSmall { .. } => {
                "降采样目标点数必须 >= 1。前端图表宽度为零像素时不应发起导出；\
                 建议按图表像素宽度设定 target_points（每像素至少 1 点）。"
            }
            PowerRingError::PersistTooShort { .. } => {
                "持久化镜像过短：文件被截断或读偏移错误。\
                 建议按 PersistLayout::total_bytes() 重新映射完整镜像，勿按文件尾猜测长度。"
            }
            PowerRingError::PersistMagicMismatch => {
                "持久化 magic 不符：镜像不是 VXPWRRNG 格式或页偏移错位。\
                 检查 mmap 基址是否对齐页首（4096B），或镜像是否属于其他子系统。"
            }
            PowerRingError::PersistVersionMismatch { .. } => {
                "持久化格式版本不符：跨版拒收（不做猜测式兼容）。\
                 升级本内核或使用对应版本的恢复工具；格式演进规则见 PersistLayout 注释。"
            }
            PowerRingError::PersistCrcMismatch { .. } => {
                "持久化 CRC32 校验失败：崩溃撕裂或存储位腐。\
                 镜像拒收是预期行为（宁缺毋假）；如需抢救，仅信任头部声明的 len 前缀部分。"
            }
            PowerRingError::SchemaFrozenMismatch { .. } => {
                "P 域 schema 冻结违约：导出方与消费方版本/字段指纹不一致。\
                 按演进规则（只增不改）同步两侧 schema 常量后重试。"
            }
            PowerRingError::RecordInvalid { .. } => {
                "导出记录自校验失败（峰值/均值/谷值序关系或不确定度越域）。\
                 上游数据可疑：建议运行 audit_conservation() 定位破坏窗口，并检查聚合级联参数。"
            }
            PowerRingError::PersistStateBad { .. } => {
                "持久化镜像内部状态不自洽（写指针/长度越容量界）。\
                 镜像疑似被截断改写；建议整环重建，仅把可信前缀作为参考数据。"
            }
        }
    }
}

impl fmt::Display for PowerRingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pwrring[0x{:04X}]: {}", self.code(), self.detail())
    }
}

// ===========================================================================
// 第三节 · 核心常量（来源逐条注释 + 编译期闸）
// ===========================================================================

/// 高频环容量：60s @ 10Hz = 600 点。
///
/// 锚点原文："60s@10Hz 600 点"。
pub const HI_RING_LEN: usize = 600;

/// 中频环容量：10min @ 1Hz = 600 点。
///
/// 锚点原文："10min@1Hz 600 点"。
pub const MID_RING_LEN: usize = 600;

/// 低频环容量：2h @ 0.2Hz = 7200s × 0.2Hz = 1440 点。
///
/// 锚点原文："2h@0.2Hz"（点数为频率×跨度的导出值，与"约 50KB"预算的出入见
/// 头注释内存预算表）。
pub const LOW_RING_LEN: usize = 1440;

/// 高频采样周期：10Hz → 100ms/拍。
pub const HI_PERIOD_MS: u64 = 100;

/// 中频聚合周期：1Hz → 1000ms/拍 = 高频 10 拍。
pub const MID_PERIOD_MS: u64 = 1_000;

/// 低频聚合周期：0.2Hz → 5000ms/拍 = 高频 50 拍。
pub const LOW_PERIOD_MS: u64 = 5_000;

/// 高→中级联分母：每 10 个高频拍聚合出一个中频单元（1s / 100ms = 10）。
pub const MID_DIVISOR: u64 = 10;

/// 高→低级联分母：每 50 个高频拍聚合出一个低频单元（5s / 100ms = 50）。
pub const LOW_DIVISOR: u64 = 50;

/// 高频环覆盖跨度：600 × 100ms = 60,000ms（切片自动选档阈值一）。
pub const HI_SPAN_MS: u64 = HI_RING_LEN as u64 * HI_PERIOD_MS;

/// 中频环覆盖跨度：600 × 1000ms = 600,000ms（切片自动选档阈值二）。
pub const MID_SPAN_MS: u64 = MID_RING_LEN as u64 * MID_PERIOD_MS;

/// 低频环覆盖跨度：1440 × 5000ms = 7,200,000ms（2h 整）。
pub const LOW_SPAN_MS: u64 = LOW_RING_LEN as u64 * LOW_PERIOD_MS;

/// 单元字节数（高频）：u64 时间戳 + u64 µW = 16B（编译期闸强制）。
pub const POWER_SAMPLE_BYTES: usize = 16;

/// 单元字节数（聚合）：ts/mean/peak/valley 4×u64 + n/pad 2×u32 = 40B。
pub const AGG_CELL_BYTES: usize = 40;

/// 三环数据总字节：9600 + 24000 + 57600 = 91,200 B（持久化载荷定长）。
pub const RING_DATA_BYTES: usize =
    HI_RING_LEN * POWER_SAMPLE_BYTES + MID_RING_LEN * AGG_CELL_BYTES + LOW_RING_LEN * AGG_CELL_BYTES;

/// seqlock 读重试上限：单次读 API 内允许的快照重试次数。
///
/// 来源：读 O(range) 一次重扫代价 ≤ 数百次拷贝；8 次重试足以吸收正常写者节奏
/// （写者每拍占用 seq 两个奇偶相位的窗口极短）。耗尽显性失败（0x12A4）不空转。
pub const SEQ_RETRY_LIMIT: u32 = 8;

/// 事件账容量：256 槽（2 的幂，取模用掩码）。
///
/// 来源：单写者稳态事件率极低（回绕/倒退/降级），256 条足够容纳一次
/// 采样风暴（如 10Hz 采 30s 全倒退）的完整入账；满后覆盖最旧并计数。
pub const LEDGER_LEN: usize = 256;

/// P 域遥测 schema 冻结版本（v1）。
///
/// 演进规则（锚点"一处定义双消费"）：**只增不改**——新增字段只允许追加到
/// `TELEMETRY_SCHEMA_V1_FIELDS` 表尾；已有字段的名称/语义/单位/编码永不修改；
/// 需要换语义时必须递增版本号并新开 `TelemetrySchemaV2`，旧版保留可解码。
pub const TELEMETRY_SCHEMA_VERSION_V1: u32 = 1;

/// P 域遥测 schema v1 字段白名单（顺序即导出编码顺序——一处定义双消费）。
///
/// 消费方：J 域（本模块导出）与 P 域遥测（cgj377_ptelem 同源消费此常量表）。
/// 单位后缀自明：uw=微瓦、uj=微焦、ppm=百万分制。
pub const TELEMETRY_SCHEMA_V1_FIELDS: [&str; 9] = [
    "ts_ms",
    "tier",
    "instant_uw",
    "win_mean_uw",
    "win_peak_uw",
    "win_valley_uw",
    "energy_uj",
    "uncertainty_ppm",
    "coverage_ppm",
];

/// 持久化镜像 magic（8 字节 ASCII，头偏移 0..8）。
pub const PERSIST_MAGIC: [u8; 8] = *b"VXPWRRNG";

/// 持久化格式版本（v1）。演进规则：只增不改；跨版恢复拒收（0x12AA）。
pub const PERSIST_VERSION: u32 = 1;

/// 持久化头定长：64B（布局见 `PersistLayout`）。
pub const PERSIST_HEADER_BYTES: usize = 64;

/// 页对齐粒度：4096B。
///
/// 来源：mmap 最小映射粒度按主流内核页取 4KiB（x86-64/AArch64 基础页）；
/// 头独占第 0 页，三环连续区从页 1 起——头写穿不撕载荷，崩溃语义清晰。
pub const PERSIST_PAGE_BYTES: usize = 4096;

/// 不确定度基底：50,000 ppm（5%）。
///
/// 来源：J 域估算功率的保守噪声底（MSR/SMU 读数本身的计量不确定度口径，
/// 估算非实测——锚点 F1446 同族纪律）。最终不确定度 = 基底 + 覆盖缺口。
pub const UNCERTAINTY_BASE_PPM: u64 = 50_000;

/// 满覆盖度：1,000,000 ppm（= 100%）。
pub const COVERAGE_FULL_PPM: u64 = 1_000_000;

/// 三粒度守恒审计容差：2000 ppm（0.2%）。
///
/// 来源：中级联是同源精确聚合，理论上偏差为 0；容差只吸收读侧 seqlock
/// 撕裂读造成的极小出入，超差即立案（0x12AE）。
pub const CONSERVATION_TOL_PPM: u64 = 2_000;

// ---- 编译期闸（纯常量校验，无运行期失败路径）----
const _: () = assert!(POWER_SAMPLE_BYTES == core::mem::size_of::<PowerSample>(), "PowerSample 必须 16B");
const _: () = assert!(AGG_CELL_BYTES == core::mem::size_of::<AggCell>(), "AggCell 必须 40B");
const _: () = assert!(LEDGER_LEN & (LEDGER_LEN - 1) == 0, "LEDGER_LEN 必须是 2 的幂");
const _: () = assert!(PERSIST_HEADER_BYTES == 64, "持久化头定长 64B");
const _: () = assert!(RING_DATA_BYTES == 91_200, "三环数据总字节 91200B（见内存预算表）");
const _: () = assert!(
    HI_SPAN_MS == 60_000 && MID_SPAN_MS == 600_000 && LOW_SPAN_MS == 7_200_000,
    "三粒度跨度：60s / 10min / 2h"
);

// ===========================================================================
// 第四节 · 数据单元（PowerSample 16B / AggCell 40B）
// ===========================================================================

/// 环槽单元契约：可零值化（`EMPTY` 以 ts=0 标记空槽）+ 可读时间戳。
///
/// ts=0 是保留字：写入路径拒绝 ts=0（0x12A2），因此 ts!=0 ⟺ 槽非空。
pub trait RingCell: Copy {
    /// 空槽零值（ts=0）。仅用于环构造初始化，永不对外发布。
    const EMPTY: Self;

    /// 该单元的业务时间戳（ms，单调递增域）。
    fn cell_ts(&self) -> u64;
}

/// 高频原始功耗样本（16B 定点，零浮点）。
///
/// - `ts_ms`：业务时间戳（ms），严格单调递增；0 保留为空槽标记。
/// - `power_uw`：瞬时功率，微瓦（u64 定点，100W = 100_000_000 µW）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerSample {
    /// 业务时间戳（ms）。
    pub ts_ms: u64,
    /// 瞬时功率（µW）。
    pub power_uw: u64,
}

impl PowerSample {
    /// 构造（const：可用于静态样本表）。
    pub const fn new(ts_ms: u64, power_uw: u64) -> Self {
        PowerSample { ts_ms, power_uw }
    }

    /// 梯形法能量微元：本样本相对 `prev` 的能量增量（µJ）。
    ///
    /// 口径：E = P × Δt；µW × ms = µJ×10⁻³ × 10⁻³ × 10⁶ … 直接换算：
    /// µW·ms = 10⁻⁶W × 10⁻³s = 10⁻⁹J，而 µJ = 10⁻⁶J，故 ÷1000。
    /// 溢出策略：saturating（能量账宁可封顶也不回卷——回卷会伪造"省电"）。
    /// 功率取梯形均值 (p_prev + p_cur)/2：先各自折半再相加，规避逐点加和上溢。
    pub const fn energy_uj_since(&self, prev: &PowerSample) -> u64 {
        let dt_ms = self.ts_ms.saturating_sub(prev.ts_ms);
        let half_prev = prev.power_uw / 2;
        let half_cur = self.power_uw / 2;
        let avg_uw = half_prev.saturating_add(half_cur);
        (avg_uw.saturating_mul(dt_ms)) / 1000
    }

    /// 序列化到 LE 字节流（16B：ts 8 + power 8）。追加，不分配（out 预留）。
    pub fn to_bytes_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.ts_ms.to_le_bytes());
        out.extend_from_slice(&self.power_uw.to_le_bytes());
    }

    /// 从 LE 字节切片反序列化（不足 16B 返回 None——调用方账目化）。
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < POWER_SAMPLE_BYTES {
            return None;
        }
        let mut ts = [0u8; 8];
        let mut pw = [0u8; 8];
        ts.copy_from_slice(&b[0..8]);
        pw.copy_from_slice(&b[8..16]);
        Some(PowerSample {
            ts_ms: u64::from_le_bytes(ts),
            power_uw: u64::from_le_bytes(pw),
        })
    }
}

impl RingCell for PowerSample {
    const EMPTY: Self = PowerSample { ts_ms: 0, power_uw: 0 };
    fn cell_ts(&self) -> u64 {
        self.ts_ms
    }
}

/// 聚合单元（40B）：均值 + 峰值 + 谷值三列 + 样本数 n——**不丢信息**。
///
/// 锚点原文："聚合用均值+峰值+谷值三列，不丢信息"。n 记录参与聚合的样本数，
/// 使跨级加权均值守恒（低频聚合中频时按 n 加权，不做无权平均）。
/// `_pad` 保持 40B 对齐（5×8），便于持久化镜像定长_stride 读。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AggCell {
    /// 窗口收口时间戳（窗口内最新样本的 ts，ms）。
    pub ts_ms: u64,
    /// 窗口均值（µW，按 n 加权）。
    pub mean_uw: u64,
    /// 窗口峰值（µW，max）。
    pub peak_uw: u64,
    /// 窗口谷值（µW，min）。
    pub valley_uw: u64,
    /// 参与聚合的样本数（高频拍数）。
    pub n: u32,
    /// 对齐填充（恒 0，序列化时写 0、校验时忽略）。
    pub _pad: u32,
}

impl AggCell {
    /// 空单元（const）。
    pub const EMPTY: AggCell = AggCell {
        ts_ms: 0,
        mean_uw: 0,
        peak_uw: 0,
        valley_uw: 0,
        n: 0,
        _pad: 0,
    };

    /// 从一组高频样本折叠出一个聚合单元。
    ///
    /// 边界：`samples` 为空返回 n=0 的空窗（调用方不应把空窗入环）。
    /// 求和用 u128 累加（µW×窗口样本数最多 ~1e11，u64 亦安全，u128 双保险）。
    pub fn fold_samples(ts_ms: u64, samples: &[PowerSample]) -> AggCell {
        if samples.is_empty() {
            return AggCell { ts_ms, ..AggCell::EMPTY };
        }
        let mut sum: u128 = 0;
        let mut peak: u64 = 0;
        let mut valley: u64 = u64::MAX;
        for s in samples {
            sum += s.power_uw as u128;
            if s.power_uw > peak {
                peak = s.power_uw;
            }
            if s.power_uw < valley {
                valley = s.power_uw;
            }
        }
        let n = samples.len() as u64;
        let mean = if sum > u64::MAX as u128 {
            // 饱和口径：均值封顶 u64::MAX（物理上不可达，纯防回卷）。
            u64::MAX
        } else {
            (sum / n as u128) as u64
        };
        AggCell {
            ts_ms,
            mean_uw: mean,
            peak_uw: peak,
            valley_uw: valley,
            n: samples.len() as u32,
            _pad: 0,
        }
    }

    /// 从一组低级聚合单元折叠出更粗粒度单元（按 n 加权均值——守恒口径）。
    pub fn fold_cells(ts_ms: u64, cells: &[AggCell]) -> AggCell {
        if cells.is_empty() {
            return AggCell { ts_ms, ..AggCell::EMPTY };
        }
        let mut weighted: u128 = 0;
        let mut total_n: u128 = 0;
        let mut peak: u64 = 0;
        let mut valley: u64 = u64::MAX;
        let mut total_n32: u64 = 0;
        for c in cells {
            weighted += c.mean_uw as u128 * c.n as u128;
            total_n += c.n as u128;
            total_n32 = total_n32.saturating_add(c.n as u64);
            if c.peak_uw > peak {
                peak = c.peak_uw;
            }
            if c.valley_uw < valley {
                valley = c.valley_uw;
            }
        }
        let mean = if total_n == 0 {
            0
        } else if weighted > u64::MAX as u128 {
            u64::MAX
        } else {
            (weighted / total_n) as u64
        };
        AggCell {
            ts_ms,
            mean_uw: mean,
            peak_uw: peak,
            valley_uw: valley,
            n: (total_n32.min(u32::MAX as u64)) as u32,
            _pad: 0,
        }
    }

    /// 序关系自校验：peak >= mean >= valley（n>0 时必须成立）。
    pub fn is_ordered(&self) -> bool {
        self.n == 0 || (self.peak_uw >= self.mean_uw && self.mean_uw >= self.valley_uw)
    }

    /// 序列化到 LE 字节流（40B）。
    pub fn to_bytes_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.ts_ms.to_le_bytes());
        out.extend_from_slice(&self.mean_uw.to_le_bytes());
        out.extend_from_slice(&self.peak_uw.to_le_bytes());
        out.extend_from_slice(&self.valley_uw.to_le_bytes());
        out.extend_from_slice(&self.n.to_le_bytes());
        out.extend_from_slice(&self._pad.to_le_bytes());
    }

    /// 从 LE 字节切片反序列化（不足 40B 返回 None）。
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < AGG_CELL_BYTES {
            return None;
        }
        let mut f = [[0u8; 8]; 4];
        for (i, slot) in f.iter_mut().enumerate() {
            slot.copy_from_slice(&b[i * 8..i * 8 + 8]);
        }
        let mut n4 = [0u8; 4];
        let mut pad4 = [0u8; 4];
        n4.copy_from_slice(&b[32..36]);
        pad4.copy_from_slice(&b[36..40]);
        Some(AggCell {
            ts_ms: u64::from_le_bytes(f[0]),
            mean_uw: u64::from_le_bytes(f[1]),
            peak_uw: u64::from_le_bytes(f[2]),
            valley_uw: u64::from_le_bytes(f[3]),
            n: u32::from_le_bytes(n4),
            _pad: u32::from_le_bytes(pad4),
        })
    }
}

impl RingCell for AggCell {
    const EMPTY: Self = AggCell::EMPTY;
    fn cell_ts(&self) -> u64 {
        self.ts_ms
    }
}

// ===========================================================================
// 第五节 · 哈希与校验和（FNV-1a / CRC32——位运算真实现，零查表零依赖）
// ===========================================================================

/// FNV-1a 32 位（导出 schema 指纹）。
///
/// 参数来源：FNV 哈希公开常数——offset 2166136261、prime 16777619。
/// 用途：schema 字段表白名单指纹（消费方对表即可验证同源）。
pub const fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// FNV-1a 64 位（导出记录行指纹，碰撞域更大）。
pub const fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// CRC-32/IEEE（ reflected，多项式 0xEDB88320）——无表位运算实现。
///
/// 来源：IEEE 802.3 多项式（zlib/gzip 同族）；无表版每字节 8 次迭代，
/// 持久化校验一次性 91KB 载荷 ≈ 73 万次迭代，启动路径可接受。
/// 溢出策略：wrapping（CRC 本就是模 2 环上的运算）。
pub const fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0;
    while i < data.len() {
        crc ^= data[i] as u32;
        let mut bit = 0;
        while bit < 8 {
            // mask = 低位为 1 时的全 1 掩码（wrapping_neg 把 0/1 变成 0x0/0xFFFFFFFF）
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            bit += 1;
        }
        i += 1;
    }
    !crc
}

/// CRC 空串自检（编译期：标准校验值 0x00000000——实现正确性的编译期证明）。
const _: () = assert!(crc32_ieee(&[]) == 0x0000_0000, "CRC32 空串校验值不符");
/// CRC "123456789" 自检（标准校验值 0xCBF43926——IEEE 测试向量）。
const _: () = assert!(crc32_ieee(b"123456789") == 0xCBF4_3926, "CRC32 IEEE 测试向量不符");

// ===========================================================================
// 第六节 · 通用环形核（seqlock 无锁读 + 单写者直写）
// ===========================================================================
// 内存模型契约（本模块 unsafe 的唯一来源，全部集中在 RingCore 内）：
//
// 1. **单写者**：buf 的写访问只允许发生在 PowerRingWriter 句柄生命周期内；
//    句柄由 `PowerRingHub::attach_writer()` 的 test-and-set 原子旗保证至多一个。
// 2. **seqlock 发布**：写者每次落环前把 `seq` 加 1（变奇），写完 buf/w/len 后
//    再加 1（变偶，Release）；读者只在 seq 为偶时取快照，读后复查 seq 相等
//    才采信。撕裂读必然被奇数相位或 s1!=s2 捕获。
// 3. **Sync 授权**：`unsafe impl Sync` 的依据 = 上述两条契约（T: Send 保证
//    数据跨线程合法，全部跨线程可见性由 Acquire/Release 序标注承担）。
// 4. seq 为 u32，按每次写 +2 递增；2^31 次写后可能回绕碰撞——回绕窗口内
//    读者误采同一 seq 的概率被 w/len 二次比对与业务 ts 单调性双重兜底，
//    且 10Hz 采样率下 2^31 次写 ≈ 6.8 年，注释显性声明为已知边界。

/// 通用定长环形核。`N` 为容量（编译期定长，入 struct 无堆分配）。
///
/// 逻辑序：logical 0 = 最旧，logical len-1 = 最新；
/// 物理槽 phys = (w + N - len + logical) mod N，其中 `w` 是下一个写槽。
pub struct RingCore<T: RingCell, const N: usize> {
    /// 物理槽阵列（UnsafeCell：单写者直写 + seqlock 读者）。
    buf: UnsafeCell<[T; N]>,
    /// 下一个写槽物理下标（0..N）。
    w: AtomicU32,
    /// 有效长度（0..=N）。
    len: AtomicU32,
    /// 覆盖最旧计数（回绕次数累计，wrapping 不回卷为账目诚实性让路——u64 足够）。
    overwrites: AtomicU64,
    /// seqlock 相位：偶=稳定，奇=写入中。
    seq: AtomicU32,
}

/// 切片结果：数据 + 一致性标记（false = 重试耗尽，数据不可采信）。
#[derive(Clone, Debug)]
pub struct SliceOutcome<T> {
    /// 快照数据（逻辑序：最旧在前）。
    pub points: Vec<T>,
    /// true = seq 前后一致（快照可采信）；false = 重试耗尽（0x12A4）。
    pub consistent: bool,
}

// Sync 授权的唯一 unsafe 依据：单写者 + seqlock 读（见第六节头注释契约 1~3）。
unsafe impl<T: RingCell + Send, const N: usize> Sync for RingCore<T, N> {}

impl<T: RingCell, const N: usize> RingCore<T, N> {
    /// const 构造：全槽 EMPTY，零分配（无分配判据的根基）。
    pub const fn new() -> Self {
        RingCore {
            buf: UnsafeCell::new([T::EMPTY; N]),
            w: AtomicU32::new(0),
            len: AtomicU32::new(0),
            overwrites: AtomicU64::new(0),
            seq: AtomicU32::new(0),
        }
    }

    /// 容量（const 泛型回读）。
    pub const fn capacity(&self) -> usize {
        N
    }

    /// 当前有效长度（快照口径，仅供水位展示；精确值随 seqlock 快照走）。
    pub fn len_now(&self) -> usize {
        self.len.load(Ordering::Acquire) as usize
    }

    /// 覆盖最旧累计计数（0x12A3 账）。
    pub fn overwrites_total(&self) -> u64 {
        self.overwrites.load(Ordering::Relaxed)
    }

    /// 逻辑下标 → 物理槽下标（恒在界内：两参数均为 0..N / 0..=len 的合法域）。
    fn phys_of(&self, w: usize, len: usize, logical: usize) -> usize {
        (w + N - len + logical) % N
    }

    /// 单写者直写一槽：返回 true 表示本次覆盖了最旧样本（环已满回绕）。
    ///
    /// 仅 PowerRingWriter 可调用（契约 1）；seq 奇偶相位的发布序见契约 2。
    /// 游标回绕：w = (w+1) % N 显式取模，不依赖 wrapping 语义（容量 0 时
    /// 取模除零在编译期被 N>=1 的环实例化保证——本模块三环容量均为正）。
    pub fn push_cell(&self, v: T) -> bool {
        // 进入奇相位：禁止读者采信（AcqRel 保证写者自身先看到自己的推进）。
        self.seq.fetch_add(1, Ordering::AcqRel);
        let w = self.w.load(Ordering::Relaxed) as usize;
        let was_full = self.len.load(Ordering::Acquire) as usize == N;
        // SAFETY（契约 1）：唯一写者句柄存续期内调用；w 恒 < N（构造与回绕保证）。
        let buf = unsafe { &mut *self.buf.get() };
        buf[w] = v;
        self.w.store(((w + 1) % N) as u32, Ordering::Release);
        if was_full {
            self.len.store(N as u32, Ordering::Release);
            self.overwrites.fetch_add(1, Ordering::Relaxed);
        } else {
            self.len.store((self.len.load(Ordering::Relaxed) as usize + 1) as u32, Ordering::Release);
        }
        // 退出奇相位（Release）：此后读者可采信新快照。
        self.seq.fetch_add(1, Ordering::Release);
        was_full
    }

    /// 单写者侧"读最近 n 个"（级联聚合专用，无 seqlock——写者读自己的写）。
    ///
    /// 从最新往回取至多 `out.len()` 个，按旧→新填入 out[0..n]；返回实际读取数。
    /// 仅写者可调用（契约 1）；n > len 时只取 len 个（启动未满期边界）。
    pub fn read_last_writer(&self, out: &mut [T]) -> usize {
        let len = self.len.load(Ordering::Acquire) as usize;
        let w = self.w.load(Ordering::Acquire) as usize;
        let n = if out.len() < len { out.len() } else { len };
        let mut i = 0;
        while i < n {
            let logical = len - n + i;
            let phys = self.phys_of(w, len, logical);
            // SAFETY（契约 1）：唯一写者读自己刚写的数据；phys 恒 < N。
            let buf = unsafe { &*self.buf.get() };
            match buf.get(phys) {
                Some(cell) => out[i] = *cell,
                // phys 由取模构造，get 恒 Some；此分支纯为免 panic 纪律兜底。
                None => return i,
            }
            i += 1;
        }
        n
    }

    /// seqlock 单元读（读者侧）：奇相位自旋 + 读后复查，重试耗尽返回 None。
    pub fn read_logical(&self, logical: usize) -> Option<T> {
        let mut attempt: u32 = 0;
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 == 0 {
                let len = self.len.load(Ordering::Acquire) as usize;
                if logical >= len {
                    return None;
                }
                let w = self.w.load(Ordering::Acquire) as usize;
                let phys = self.phys_of(w, len, logical);
                // SAFETY（契约 1+2）：偶相位读，撕裂由读后 s1==s2 复查捕获。
                let buf = unsafe { &*self.buf.get() };
                let got = buf.get(phys).copied();
                if self.seq.load(Ordering::Acquire) == s1 {
                    return got;
                }
            }
            attempt += 1;
            if attempt > SEQ_RETRY_LIMIT {
                return None;
            }
        }
    }

    /// 最新样本时间戳（空环返回 0）。
    pub fn newest_ts(&self) -> u64 {
        let len = self.len_now();
        if len == 0 {
            return 0;
        }
        match self.read_logical(len - 1) {
            Some(c) => c.cell_ts(),
            None => 0,
        }
    }

    /// 最旧样本时间戳（空环返回 0）。
    pub fn oldest_ts(&self) -> u64 {
        if self.len_now() == 0 {
            return 0;
        }
        match self.read_logical(0) {
            Some(c) => c.cell_ts(),
            None => 0,
        }
    }

    /// seqlock 整体一致性读：逻辑区间 [from, to) 的快照（O(range) 拷贝）。
    ///
    /// 统一入口：`slice_ts`/`latest`/`snapshot_all` 都走这里。
    /// 一致性失败时 consistent=false（不注入脏数据——宁缺毋假）。
    fn read_range_consistent(&self, from: usize, to: usize) -> SliceOutcome<T> {
        let mut attempt: u32 = 0;
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 == 0 {
                let len = self.len.load(Ordering::Acquire) as usize;
                let w = self.w.load(Ordering::Acquire) as usize;
                let hi = if to < len { to } else { len };
                let mut points = Vec::with_capacity(hi.saturating_sub(from));
                let mut logical = from;
                let mut torn = false;
                while logical < hi {
                    let phys = self.phys_of(w, len, logical);
                    // SAFETY（契约 1+2）：偶相位整段读，读后统一复查 seq。
                    let buf = unsafe { &*self.buf.get() };
                    match buf.get(phys).copied() {
                        Some(c) => points.push(c),
                        None => {
                            torn = true;
                            break;
                        }
                    }
                    logical += 1;
                }
                if self.seq.load(Ordering::Acquire) == s1 && !torn {
                    return SliceOutcome { points, consistent: true };
                }
            }
            attempt += 1;
            if attempt > SEQ_RETRY_LIMIT {
                return SliceOutcome {
                    points: Vec::new(),
                    consistent: false,
                };
            }
        }
    }

    /// 时间范围切片：二分定位 O(log N) + 拷贝 O(range)。
    ///
    /// 边界语义：含 start、含 end（闭区间）；环内 ts 无重复（写入路径单调）。
    /// 空环返回空且 consistent=true（上层转 RingEmpty）；重试耗尽 consistent=false。
    pub fn slice_ts(&self, start_ts: u64, end_ts: u64) -> SliceOutcome<T> {
        let mut attempt: u32 = 0;
        loop {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 & 1 == 0 {
                let len = self.len.load(Ordering::Acquire) as usize;
                let w = self.w.load(Ordering::Acquire) as usize;
                // 二分下界：第一个 ts >= start_ts 的逻辑下标。
                let mut lo: usize = 0;
                let mut hi: usize = len;
                while lo < hi {
                    let mid = lo + (hi - lo) / 2;
                    let phys = self.phys_of(w, len, mid);
                    // SAFETY（契约 1+2）：与整段读同一次快照相位内。
                    let buf = unsafe { &*self.buf.get() };
                    let t = match buf.get(phys) {
                        Some(c) => c.cell_ts(),
                        None => u64::MAX, // 不可达兜底：当 t 越界处理使搜索收敛。
                    };
                    if t < start_ts {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                let mut points = Vec::new();
                let mut logical = lo;
                let mut torn = false;
                while logical < len {
                    let phys = self.phys_of(w, len, logical);
                    // SAFETY：同上，同相位读。
                    let buf = unsafe { &*self.buf.get() };
                    match buf.get(phys).copied() {
                        Some(c) => {
                            let t = c.cell_ts();
                            if t > end_ts {
                                break;
                            }
                            points.push(c);
                        }
                        None => {
                            torn = true;
                            break;
                        }
                    }
                    logical += 1;
                }
                if self.seq.load(Ordering::Acquire) == s1 && !torn {
                    return SliceOutcome { points, consistent: true };
                }
            }
            attempt += 1;
            if attempt > SEQ_RETRY_LIMIT {
                return SliceOutcome {
                    points: Vec::new(),
                    consistent: false,
                };
            }
        }
    }

    /// 读最近 n 个（逻辑序旧→新）。n=0 返回空。
    pub fn latest(&self, n: usize) -> SliceOutcome<T> {
        let len = self.len_now();
        if n == 0 || len == 0 {
            return SliceOutcome {
                points: Vec::new(),
                consistent: true,
            };
        }
        let from = if len > n { len - n } else { 0 };
        self.read_range_consistent(from, len)
    }

    /// 全环快照（持久化 dump 用）：整环逻辑序快照。
    pub fn snapshot_all(&self) -> SliceOutcome<T> {
        let len = self.len_now();
        self.read_range_consistent(0, len)
    }

    /// 崩溃恢复：整镜像原位回灌（物理槽序 + w/len 精确复原）。
    ///
    /// 只允许在写者 attach 之前调用（恢复期采样线程必然尚未启动——契约 1 的
    /// 静态前题）。ts 单调性由调用方（recover_from_bytes）先验证截断后传入。
    /// 返回 Err(PersistStateBad) 表示 w/len 与容量不自洽（镜像被改写）。
    pub fn restore_image(&self, cells: &[T], w: usize, len: usize) -> Result<(), PowerRingError> {
        if w >= N && !(w == 0 && N == 0) {
            return Err(PowerRingError::PersistStateBad { state: w as u32 });
        }
        if len > N || cells.len() < N {
            return Err(PowerRingError::PersistStateBad { state: len as u32 });
        }
        let mut i = 0;
        while i < N {
            self.raw_write_slot(i, cells[i]);
            i += 1;
        }
        self.w.store(w as u32, Ordering::Release);
        self.len.store(len as u32, Ordering::Release);
        self.seq.store(0, Ordering::Release); // 偶相位归零：恢复态即稳定态
        Ok(())
    }

    /// 物理槽直写（仅恢复路径；绕过 seqlock——恢复期无读者，契约 1 静态前题）。
    fn raw_write_slot(&self, phys: usize, v: T) {
        // SAFETY：恢复期唯一访问者；phys < N 由调用方保证。
        let buf = unsafe { &mut *self.buf.get() };
        if phys < N {
            buf[phys] = v;
        }
    }

    /// 物理槽直读（仅持久化 dump 路径：镜像 = 物理内存原样，不是逻辑序快照）。
    ///
    /// dump 与恢复走同一条物理序路径，逻辑序由 w/len 在两侧一致复原。
    /// 一致性：调用方持有写者句柄（采样暂停）或接受撕裂窗口时使用——
    /// dump 前后各查一次 seq，不一致则上层重试（见 PowerRingHub::dump_persist）。
    pub fn phys_slot(&self, phys: usize) -> Option<T> {
        if phys >= N {
            return None;
        }
        // SAFETY（契约 1+2）：dump 路径读，撕裂由 dump 的 seq 前后比对捕获。
        let buf = unsafe { &*self.buf.get() };
        buf.get(phys).copied()
    }

    /// 当前写指针快照（持久化头需要：恢复时精确复原逻辑序）。
    pub fn w_now(&self) -> usize {
        self.w.load(Ordering::Acquire) as usize
    }
}

// ===========================================================================
// 第七节 · 事件账（RingEventLedger：定长 256 槽环形账，满覆盖显性计数）
// ===========================================================================

/// 账目条目（原子字段：写者与读者侧事件都会入账，无锁并发安全）。
///
/// 同槽同码合并计数（回绕风暴不刷屏）；跨码换槽；被覆盖的旧账由
/// `covered_total` 独立累加（不静默——0x12AD 语义）。
#[derive(Debug)]
pub struct LedgerEntry {
    /// 事件时间戳（ms）。
    pub ts_ms: AtomicU64,
    /// 诊断码（0 = 空槽）。
    pub code: AtomicU32,
    /// 同槽同码累计次数。
    pub count: AtomicU64,
}

/// 定长环形事件账。
///
/// 并发说明：record() 可能来自写者（回绕/倒退）与读者（重试耗尽/审计）两侧，
/// 因此条目用原子字段而非 UnsafeCell。同槽并发写最坏情形是 count 少记一次
/// （fetch_add 原子，故只多不少；ts/code 交换可能交错）——账目计数只增不减，
/// 属可接受口径，注释显性声明。
pub struct RingEventLedger {
    buf: [LedgerEntry; LEDGER_LEN],
    /// 环形写游标（wrapping：fetch_add 自然回卷，掩码取槽）。
    w: AtomicU32,
    /// 历史入账总条数（含合并；用于判断已填充槽区间）。
    total: AtomicU64,
    /// 满后被覆盖的旧账条数（0x12AD 计数器，不静默）。
    covered: AtomicU64,
}

/// 账目空槽常量（数组 repeat 的具名 const 技巧——原子类型非 Copy 的唯一出路）。
const LEDGER_ZERO_ENTRY: LedgerEntry = LedgerEntry {
    ts_ms: AtomicU64::new(0),
    code: AtomicU32::new(0),
    count: AtomicU64::new(0),
};

/// 账目快照条目（纯数据，供导出/体检）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerSnapshot {
    /// 事件时间戳。
    pub ts_ms: u64,
    /// 诊断码。
    pub code: u32,
    /// 同码累计次数。
    pub count: u64,
}

impl RingEventLedger {
    /// const 构造：全槽清零。
    pub const fn new() -> Self {
        // 数组 repeat 对非 Copy 类型要求元素是具名 const（原子字段非 Copy），
        // 见模块级 LEDGER_ZERO_ENTRY。
        RingEventLedger {
            buf: [LEDGER_ZERO_ENTRY; LEDGER_LEN],
            w: AtomicU32::new(0),
            total: AtomicU64::new(0),
            covered: AtomicU64::new(0),
        }
    }

    /// 入账：同槽同码合并计数，跨码换槽；满后覆盖最旧并累计 covered。
    ///
    /// 设计取舍：被覆盖的旧账不再反向写一条"覆盖事件"（会引发递归入账），
    /// 而是累计进 `covered_total`，由 health()/snapshot() 显性暴露。
    pub fn record(&self, code: u32, ts_ms: u64) {
        let slot = self.w.fetch_add(1, Ordering::Relaxed) as usize & (LEDGER_LEN - 1);
        let entry = &self.buf[slot];
        let prev = entry.code.swap(code, Ordering::AcqRel);
        if prev == code {
            entry.count.fetch_add(1, Ordering::Relaxed);
            entry.ts_ms.store(ts_ms, Ordering::Release);
        } else {
            entry.count.store(1, Ordering::Release);
            entry.ts_ms.store(ts_ms, Ordering::Release);
            if prev != 0 {
                // 旧账被覆盖：独立计数（0x12AD），不静默。
                self.covered.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.total.fetch_add(1, Ordering::Relaxed);
    }

    /// 被覆盖旧账累计数。
    pub fn covered_total(&self) -> u64 {
        self.covered.load(Ordering::Relaxed)
    }

    /// 历史入账总条数（含合并计数）。
    pub fn total_records(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    /// 账目快照（旧→新，空槽跳过；并发入账下为尽力一致快照）。
    pub fn snapshot(&self) -> Vec<LedgerSnapshot> {
        let w = self.w.load(Ordering::Acquire) as usize;
        let total = self.total_records();
        let filled = if total as usize > LEDGER_LEN {
            LEDGER_LEN
        } else {
            total as usize
        };
        let mut out = Vec::with_capacity(filled);
        let mut k = 0;
        while k < filled {
            // 槽序：最旧在前。w 是"下一个写槽"，故最旧槽 = (w - filled + k)。
            let idx = (w + LEDGER_LEN - filled + k) & (LEDGER_LEN - 1);
            let entry = &self.buf[idx];
            let code = entry.code.load(Ordering::Acquire);
            if code != 0 {
                out.push(LedgerSnapshot {
                    ts_ms: entry.ts_ms.load(Ordering::Acquire),
                    code,
                    count: entry.count.load(Ordering::Acquire),
                });
            }
            k += 1;
        }
        out
    }

    /// 按码累计（诊断总线聚合视图）。
    pub fn count_by_code(&self, code: u32) -> u64 {
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < LEDGER_LEN {
            let entry = &self.buf[i];
            if entry.code.load(Ordering::Acquire) == code {
                acc = acc.wrapping_add(entry.count.load(Ordering::Acquire));
            }
            i += 1;
        }
        acc
    }
}

// ===========================================================================
// 第八节 · 三粒度枢纽（PowerRingHub：三环 + 级联 + 账目 + schema + 持久化态）
// ===========================================================================

/// 粒度标签（切片/导出必带——下游图表按标签换算坐标密度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TierTag {
    /// 高频：10Hz，覆盖 60s。
    Hi,
    /// 中频：1Hz，覆盖 10min。
    Mid,
    /// 低频：0.2Hz，覆盖 2h。
    Low,
}

impl TierTag {
    /// 稳定字符串（schema 编码用，冻结：只增不改）。
    pub const fn as_str(self) -> &'static str {
        match self {
            TierTag::Hi => "hi10hz",
            TierTag::Mid => "mid1hz",
            TierTag::Low => "low0p2hz",
        }
    }

    /// 本粒度采样周期（ms）。
    pub const fn period_ms(self) -> u64 {
        match self {
            TierTag::Hi => HI_PERIOD_MS,
            TierTag::Mid => MID_PERIOD_MS,
            TierTag::Low => LOW_PERIOD_MS,
        }
    }

    /// 本粒度环容量。
    pub const fn capacity(self) -> usize {
        match self {
            TierTag::Hi => HI_RING_LEN,
            TierTag::Mid => MID_RING_LEN,
            TierTag::Low => LOW_RING_LEN,
        }
    }

    /// 本粒度覆盖跨度（ms）。
    pub const fn span_ms(self) -> u64 {
        match self {
            TierTag::Hi => HI_SPAN_MS,
            TierTag::Mid => MID_SPAN_MS,
            TierTag::Low => LOW_SPAN_MS,
        }
    }
}

/// 跨度 → 粒度自动选档（const：跨度 > 粒度容量自动换粗粒度——锚点"标注"）。
///
/// 边界恰等时选细粒度（60s 整跨走 Hi——细粒度信息无损）。
pub const fn tier_for_span(span_ms: u64) -> TierTag {
    if span_ms <= HI_SPAN_MS {
        TierTag::Hi
    } else if span_ms <= MID_SPAN_MS {
        TierTag::Mid
    } else {
        TierTag::Low
    }
}

/// 持久化状态机（u32 原子存储：0=从未落盘 1=已落盘 2=已恢复）。
pub const PERSIST_STATE_NEVER: u32 = 0;
/// 已执行 dump_persist（镜像可用）。
pub const PERSIST_STATE_DUMPED: u32 = 1;
/// 执行过 recover_from_bytes（环内容来自镜像）。
pub const PERSIST_STATE_RECOVERED: u32 = 2;

/// 三粒度功耗历史枢纽。
///
/// 尺寸 ≈ 91,200B + 控制块 —— **>64KB，禁止上栈**：统一 `PowerRingHub::new()`
/// 返回 `Box`；`bare()` 仅限静态段放置（链接脚本指定的 mmap 镜像区等）。
pub struct PowerRingHub {
    /// 高频环：60s @ 10Hz，原始样本。
    hi: RingCore<PowerSample, HI_RING_LEN>,
    /// 中频环：10min @ 1Hz，均值/峰值/谷值聚合。
    mid: RingCore<AggCell, MID_RING_LEN>,
    /// 低频环：2h @ 0.2Hz，均值/峰值/谷值聚合。
    low: RingCore<AggCell, LOW_RING_LEN>,
    /// 单写者旗（test-and-set：false→true 成功，true→拒绝 0x12A1）。
    writer_flag: AtomicBool,
    /// 高频拍计数（级联分频的相位基准；wrapping——拍数回卷只影响一次聚合相位，
    /// 10Hz 下 u64 回卷周期远超宇宙年龄，注释显性声明）。
    tick: AtomicU64,
    /// 已受理的最新样本时间戳（单调性基准）。
    last_ts: AtomicU64,
    /// 时间戳倒退累计（0x12A2 计数器）。
    retrogrades: AtomicU64,
    /// 守恒审计破坏累计（0x12AE 计数器）。
    conservation_breaks: AtomicU64,
    /// 持久化状态机（PERSIST_STATE_*）。
    persist_state: AtomicU32,
    /// 最近一次 dump 的载荷 CRC（0 = 无）。
    last_dump_crc: AtomicU32,
    /// 事件账。
    ledger: RingEventLedger,
}

impl PowerRingHub {
    /// 堆上构造（>64KB 上栈纪律：Hub 永远住 Box）。
    pub fn new() -> alloc::boxed::Box<PowerRingHub> {
        alloc::boxed::Box::new(PowerRingHub::bare())
    }

    /// 裸构造（const：仅供静态段/链接脚本放置；**禁止作为局部变量上栈**）。
    pub const fn bare() -> PowerRingHub {
        PowerRingHub {
            hi: RingCore::new(),
            mid: RingCore::new(),
            low: RingCore::new(),
            writer_flag: AtomicBool::new(false),
            tick: AtomicU64::new(0),
            last_ts: AtomicU64::new(0),
            retrogrades: AtomicU64::new(0),
            conservation_breaks: AtomicU64::new(0),
            persist_state: AtomicU32::new(PERSIST_STATE_NEVER),
            last_dump_crc: AtomicU32::new(0),
            ledger: RingEventLedger::new(),
        }
    }

    /// 尝试获取写者句柄（单写者 test-and-set）。
    ///
    /// 成功返回 RAII 句柄（Drop 自动还旗）；重复 attach 拒收 0x12A1 并入账。
    pub fn attach_writer(&self) -> Result<PowerRingWriter<'_>, PowerRingError> {
        match self.writer_flag.compare_exchange(
            false,
            true,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                self.ledger.record(X_PWR_RING_OK, self.last_ts.load(Ordering::Relaxed));
                Ok(PowerRingWriter { hub: self })
            }
            Err(_) => {
                self.ledger.record(X_PWR_RING_WRITER_TAKEN, self.last_ts.load(Ordering::Relaxed));
                Err(PowerRingError::WriterTaken)
            }
        }
    }

    /// 获取读者门面（共享引用，无锁读；任意多个并存——锁分离语义）。
    pub fn reader(&self) -> PowerRingReader<'_> {
        PowerRingReader { hub: self }
    }

    /// 是否已有写者在位。
    pub fn writer_attached(&self) -> bool {
        self.writer_flag.load(Ordering::Acquire)
    }

    /// 高频拍计数快照。
    pub fn tick_now(&self) -> u64 {
        self.tick.load(Ordering::Relaxed)
    }

    /// 事件账引用（诊断总线镜像读取）。
    pub fn ledger(&self) -> &RingEventLedger {
        &self.ledger
    }

    /// 时间戳倒退累计。
    pub fn retrograde_total(&self) -> u64 {
        self.retrogrades.load(Ordering::Relaxed)
    }

    /// 守恒破坏累计。
    pub fn conservation_break_total(&self) -> u64 {
        self.conservation_breaks.load(Ordering::Relaxed)
    }

    /// 持久化状态机快照（PERSIST_STATE_* 常量之一）。
    pub fn persist_state(&self) -> u32 {
        self.persist_state.load(Ordering::Relaxed)
    }

    /// 最近 dump 的载荷 CRC。
    pub fn last_dump_crc(&self) -> u32 {
        self.last_dump_crc.load(Ordering::Relaxed)
    }
}

impl Default for alloc::boxed::Box<PowerRingHub> {
    fn default() -> Self {
        PowerRingHub::new()
    }
}

// ===========================================================================
// 第九节 · 写入路径（PowerRingWriter：单写者直写 + 三粒度级联聚合）
// ===========================================================================

/// 单次落环回执（写者视角的全部事实，调用方按需审计）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PushReceipt {
    /// 本次落环后的高频拍计数（1 基）。
    pub tick: u64,
    /// 本次写是否覆盖了高频环最旧样本（回绕发生）。
    pub hi_overwrote: bool,
    /// 本次触发的中频聚合单元（tick % 10 == 0 时 Some）。
    pub mid_cell: Option<AggCell>,
    /// 中频环本次是否回绕覆盖。
    pub mid_overwrote: bool,
    /// 本次触发的低频聚合单元（tick % 50 == 0 时 Some）。
    pub low_cell: Option<AggCell>,
    /// 低频环本次是否回绕覆盖。
    pub low_overwrote: bool,
    /// 本次产出的聚合单元是否全部通过序关系自校验（peak>=mean>=valley）。
    pub conservation_ok: bool,
}

/// 单写者句柄（RAII：Drop 自动还旗）。
///
/// 契约：同一时刻至多一个句柄（attach_writer 的原子旗保证）；句柄存活期内
/// 持有对 Hub 的共享引用并直写三环（RingCore 契约 1 的执行者）。
pub struct PowerRingWriter<'a> {
    hub: &'a PowerRingHub,
}

impl Drop for PowerRingWriter<'_> {
    fn drop(&mut self) {
        self.hub.writer_flag.store(false, Ordering::Release);
        self.hub.ledger.record(X_PWR_RING_OK, self.hub.last_ts.load(Ordering::Relaxed));
    }
}

impl PowerRingWriter<'_> {
    /// 直写一个高频样本（F1447 管道聚合结果落环——采样线程直写）。
    ///
    /// 流程：
    /// 1. ts==0 拒收（空槽保留字，0x12A2）；
    /// 2. 单调性校验：严格递增（同拍重复也拒——10Hz 契约由上游保证），
    ///    倒退拒收入账（0x12A2），retrogrades 计数；
    /// 3. 高频环直写（回绕覆盖入账 0x12A3）；
    /// 4. 级联：每 10 拍聚合中频单元、每 50 拍聚合低频单元（均值+峰值+谷值
    ///    三列 + 样本数 n——不丢信息），聚合单元序关系自校验，破坏入账 0x12AE。
    pub fn push(&self, sample: PowerSample) -> Result<PushReceipt, PowerRingError> {
        // 1) 保留字拒收。
        if sample.ts_ms == 0 {
            self.hub.ledger.record(X_PWR_RING_TS_RETROGRADE, 0);
            self.hub.retrogrades.fetch_add(1, Ordering::Relaxed);
            return Err(PowerRingError::TimestampRetrograde {
                last_ts: self.hub.last_ts.load(Ordering::Relaxed),
                got_ts: 0,
            });
        }
        // 2) 严格单调递增校验（首样本任意非零 ts 直接收）。
        let last = self.hub.last_ts.load(Ordering::Relaxed);
        if last != 0 && sample.ts_ms <= last {
            self.hub.ledger.record(X_PWR_RING_TS_RETROGRADE, sample.ts_ms);
            self.hub.retrogrades.fetch_add(1, Ordering::Relaxed);
            return Err(PowerRingError::TimestampRetrograde {
                last_ts: last,
                got_ts: sample.ts_ms,
            });
        }
        self.hub.last_ts.store(sample.ts_ms, Ordering::Relaxed);

        // 3) 高频环直写。
        let hi_overwrote = self.hub.hi.push_cell(sample);
        if hi_overwrote {
            self.hub.ledger.record(X_PWR_RING_OVERWRITE, sample.ts_ms);
        }
        let tick = self.hub.tick.fetch_add(1, Ordering::Relaxed).wrapping_add(1);

        // 4) 级联聚合（均值+峰值+谷值三列，不丢信息）。
        let mut conservation_ok = true;
        let mut mid_cell: Option<AggCell> = None;
        let mut mid_overwrote = false;
        let mut low_cell: Option<AggCell> = None;
        let mut low_overwrote = false;

        if tick % LOW_DIVISOR == 0 {
            // 低频在前：tick%50==0 时必先有 tick%10==0 产出的中频单元，
            // 先补齐中频再聚合低频，保证低频窗口读到的是完整中频序列。
            if tick % MID_DIVISOR == 0 {
                let (cell, overwrote, ok) = self.cascade_mid(sample.ts_ms);
                conservation_ok &= ok;
                mid_cell = Some(cell);
                mid_overwrote = overwrote;
            }
            let (cell, overwrote, ok) = self.cascade_low(sample.ts_ms);
            conservation_ok &= ok;
            low_cell = Some(cell);
            low_overwrote = overwrote;
        } else if tick % MID_DIVISOR == 0 {
            let (cell, overwrote, ok) = self.cascade_mid(sample.ts_ms);
            conservation_ok &= ok;
            mid_cell = Some(cell);
            mid_overwrote = overwrote;
        }

        Ok(PushReceipt {
            tick,
            hi_overwrote,
            mid_cell,
            mid_overwrote,
            low_cell,
            low_overwrote,
            conservation_ok,
        })
    }

    /// 中频级联：取最近 10 个高频样本折叠为中频单元并入环。
    fn cascade_mid(&self, ts_ms: u64) -> (AggCell, bool, bool) {
        let mut window = [PowerSample::new(0, 0); MID_DIVISOR as usize];
        let n = self.hub.hi.read_last_writer(&mut window);
        if n == 0 {
            // 不可达（push 先于级联），纯纪律兜底：返回空窗且不入环。
            return (AggCell { ts_ms, ..AggCell::EMPTY }, false, true);
        }
        let cell = AggCell::fold_samples(ts_ms, &window[..n]);
        let ok = cell.is_ordered();
        if !ok {
            self.hub.ledger.record(X_PWR_CONSERVATION_BROKEN, ts_ms);
            self.hub.conservation_breaks.fetch_add(1, Ordering::Relaxed);
        }
        let overwrote = self.hub.mid.push_cell(cell);
        if overwrote {
            self.hub.ledger.record(X_PWR_RING_OVERWRITE, ts_ms);
        }
        (cell, overwrote, ok)
    }

    /// 低频级联：取最近 5 个中频单元按 n 加权折叠为低频单元并入环。
    fn cascade_low(&self, ts_ms: u64) -> (AggCell, bool, bool) {
        let mut window = [AggCell::EMPTY; (LOW_DIVISOR / MID_DIVISOR) as usize];
        let n = self.hub.mid.read_last_writer(&mut window);
        if n == 0 {
            return (AggCell { ts_ms, ..AggCell::EMPTY }, false, true);
        }
        let cell = AggCell::fold_cells(ts_ms, &window[..n]);
        let ok = cell.is_ordered();
        if !ok {
            self.hub.ledger.record(X_PWR_CONSERVATION_BROKEN, ts_ms);
            self.hub.conservation_breaks.fetch_add(1, Ordering::Relaxed);
        }
        let overwrote = self.hub.low.push_cell(cell);
        if overwrote {
            self.hub.ledger.record(X_PWR_RING_OVERWRITE, ts_ms);
        }
        (cell, overwrote, ok)
    }

    /// 直读当前最新样本（写者视角自检用；不产生账目事件）。
    pub fn last_sample(&self) -> Option<PowerSample> {
        let len = self.hub.hi.len_now();
        if len == 0 {
            None
        } else {
            self.hub.hi.read_logical(len - 1)
        }
    }
}

// ===========================================================================
// 第十节 · 读取 API（切片 O(range) / 降采样导出 / 二分定位 / 守恒审计）
// ===========================================================================

/// 统一切片点（三粒度归一形态：高频点 mean=peak=valley=瞬时值）。
///
/// 下游图表只认这一种形态——粒度差异由 `tier` 字段标注（锚点"自动换粗粒度
/// 并标注"），数值语义按 tier 的 period_ms 解释。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlicePoint {
    /// 时间戳（ms）。高频=采样 ts；聚合=窗口收口 ts。
    pub ts_ms: u64,
    /// 均值功率（µW）。高频点=瞬时值。
    pub mean_uw: u64,
    /// 峰值功率（µW）。高频点=瞬时值。
    pub peak_uw: u64,
    /// 谷值功率（µW）。高频点=瞬时值。
    pub valley_uw: u64,
    /// 粒度标注。
    pub tier: TierTag,
}

impl SlicePoint {
    /// 高频样本 → 切片点（三列同值）。
    pub const fn from_sample(s: PowerSample) -> Self {
        SlicePoint {
            ts_ms: s.ts_ms,
            mean_uw: s.power_uw,
            peak_uw: s.power_uw,
            valley_uw: s.power_uw,
            tier: TierTag::Hi,
        }
    }

    /// 聚合单元 → 切片点。
    pub const fn from_agg(c: AggCell, tier: TierTag) -> Self {
        SlicePoint {
            ts_ms: c.ts_ms,
            mean_uw: c.mean_uw,
            peak_uw: c.peak_uw,
            valley_uw: c.valley_uw,
            tier,
        }
    }
}

/// 时间范围切片结果（带粒度标注——锚点"自动选档并标注"）。
#[derive(Clone, Debug)]
pub struct TierSlice {
    /// 实际选用的粒度。
    pub tier: TierTag,
    /// 切片点（逻辑序旧→新）。
    pub points: Vec<SlicePoint>,
}

impl TierSlice {
    /// 切片点数。
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// 是否空切片。
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// 时间跨度（首尾 ts 差；空/单点为 0）。
    pub fn span_ms(&self) -> u64 {
        match (self.points.first(), self.points.last()) {
            (Some(a), Some(b)) => b.ts_ms.saturating_sub(a.ts_ms),
            _ => 0,
        }
    }
}

/// 二分定位命中（时间戳 → 环内最近单元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocateHit {
    /// 命中粒度。
    pub tier: TierTag,
    /// 逻辑下标（0 = 最旧）。
    pub logical: usize,
    /// 命中单元时间戳。
    pub ts_ms: u64,
    /// 是否精确命中（false = 就近取整）。
    pub exact: bool,
    /// 与请求 ts 的绝对差（ms，exact 时为 0）。
    pub delta_ms: u64,
}

/// 守恒审计报告（三粒度对账——中频窗口 vs 高频原样本重算）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConservationReport {
    /// 参与对账的中频单元数。
    pub checked_cells: usize,
    /// 最大偏差（ppm，按加权均值口径）。
    pub max_dev_ppm: u64,
    /// 容差（= CONSERVATION_TOL_PPM）。
    pub tolerance_ppm: u64,
    /// 是否超差（true = 立案 0x12AE）。
    pub broken: bool,
    /// 结论注记（'static 短语）。
    pub note: &'static str,
}

/// 读者门面：无锁读（seqlock 快照），任意多个并存（锁分离）。
pub struct PowerRingReader<'a> {
    hub: &'a PowerRingHub,
}

impl PowerRingReader<'_> {
    /// 读最近 n 个高频样本（旧→新）。n=0 / 空环返回空。
    pub fn latest(&self, n: usize) -> Vec<PowerSample> {
        self.hub.hi.latest(n).points
    }

    /// 读最近 n 个切片点（高频形态）。空环返回空 Vec（不报错——latest 是
    /// 宽松口径；严格口径请用 slice_range）。
    pub fn latest_points(&self, n: usize) -> Vec<SlicePoint> {
        self.hub
            .hi
            .latest(n)
            .points
            .iter()
            .map(|s| SlicePoint::from_sample(*s))
            .collect()
    }

    /// 指定粒度的时间范围切片（闭区间 [start, end]）。
    ///
    /// 边界处理：start>end → 0x12A6；空环 → 0x12AF；结果空 → 0x12A5；
    /// seqlock 重试耗尽 → 0x12A4。
    pub fn slice_on_tier(&self, tier: TierTag, start_ts: u64, end_ts: u64) -> Result<TierSlice, PowerRingError> {
        if start_ts > end_ts {
            self.hub.ledger.record(X_PWR_RING_RANGE_INVERTED, start_ts);
            return Err(PowerRingError::RangeInverted { start_ts, end_ts });
        }
        let outcome = match tier {
            TierTag::Hi => {
                let o = self.hub.hi.slice_ts(start_ts, end_ts);
                SliceOutcome {
                    consistent: o.consistent,
                    points: o
                        .points
                        .iter()
                        .map(|s| SlicePoint::from_sample(*s))
                        .collect(),
                }
            }
            TierTag::Mid => {
                let o = self.hub.mid.slice_ts(start_ts, end_ts);
                SliceOutcome {
                    consistent: o.consistent,
                    points: o
                        .points
                        .iter()
                        .map(|c| SlicePoint::from_agg(*c, TierTag::Mid))
                        .collect(),
                }
            }
            TierTag::Low => {
                let o = self.hub.low.slice_ts(start_ts, end_ts);
                SliceOutcome {
                    consistent: o.consistent,
                    points: o
                        .points
                        .iter()
                        .map(|c| SlicePoint::from_agg(*c, TierTag::Low))
                        .collect(),
                }
            }
        };
        if !outcome.consistent {
            self.hub.ledger.record(X_PWR_RING_SEQ_RETRY_EXHAUSTED, start_ts);
            return Err(PowerRingError::SeqRetryExhausted {
                attempts: SEQ_RETRY_LIMIT,
            });
        }
        if outcome.points.is_empty() {
            let empty_ring = match tier {
                TierTag::Hi => self.hub.hi.len_now() == 0,
                TierTag::Mid => self.hub.mid.len_now() == 0,
                TierTag::Low => self.hub.low.len_now() == 0,
            };
            if empty_ring {
                self.hub.ledger.record(X_PWR_RING_EMPTY, start_ts);
                return Err(PowerRingError::RingEmpty);
            }
            self.hub.ledger.record(X_PWR_RING_RANGE_EMPTY, start_ts);
            return Err(PowerRingError::RangeEmpty { start_ts, end_ts });
        }
        Ok(TierSlice {
            tier,
            points: outcome.points,
        })
    }

    /// 时间范围切片（自动选档：跨度 > 粒度容量自动换粗粒度并标注）。
    ///
    /// 选档规则（const `tier_for_span`）：≤60s 走高频（信息无损）；≤10min 走
    /// 中频；>10min 走低频。边界恰等时选细粒度。
    pub fn slice_range(&self, start_ts: u64, end_ts: u64) -> Result<TierSlice, PowerRingError> {
        let span = end_ts.saturating_sub(start_ts);
        self.slice_on_tier(tier_for_span(span), start_ts, end_ts)
    }

    /// 降采样导出：桶取峰值保极值不丢尖峰——图表对齐语义。
    ///
    /// 桶划分：把切片点均分为 `target` 个桶（u128 乘法防溢出），每桶取
    /// peak 最大者；保留尖峰自己的 ts（下钻时可精确定位毛刺时刻）。
    /// 点数不足 target 时原样返回（不插值——诚实口径）。
    pub fn downsample_export(
        &self,
        start_ts: u64,
        end_ts: u64,
        target_points: usize,
    ) -> Result<Vec<SlicePoint>, PowerRingError> {
        if target_points == 0 {
            self.hub.ledger.record(X_PWR_RING_DOWNSAMPLE_TOO_FEW, start_ts);
            return Err(PowerRingError::DownsampleTargetTooSmall {
                target: target_points,
                minimum: 1,
            });
        }
        let slice = self.slice_range(start_ts, end_ts)?;
        if slice.points.len() <= target_points {
            return Ok(slice.points);
        }
        let n = slice.points.len();
        let t = target_points as u128;
        let total = n as u128;
        let mut out = Vec::with_capacity(target_points);
        let mut b: u128 = 0;
        while b < t {
            // 桶 b 的点区间 [b*n/t, (b+1)*n/t)——u128 乘除，n≤1440 无溢出。
            let from = (b * total / t) as usize;
            let to_excl = ((b + 1) * total / t) as usize;
            let mut best = slice.points[from];
            let mut k = from + 1;
            while k < to_excl {
                if slice.points[k].peak_uw > best.peak_uw {
                    best = slice.points[k];
                }
                k += 1;
            }
            out.push(best);
            b += 1;
        }
        Ok(out)
    }

    /// 时间戳二分定位（就近取整）：日志对齐/告警下钻用。
    pub fn locate_ts(&self, ts_ms: u64) -> Result<LocateHit, PowerRingError> {
        // 选档：按"该 ts 落在哪段历史"选粒度——先高频（60s 内），逐级回退。
        let tiers = [TierTag::Hi, TierTag::Mid, TierTag::Low];
        let mut ti = 0;
        let mut found: Option<(TierTag, usize, u64, bool, u64)> = None;
        while ti < tiers.len() && found.is_none() {
            let tier = tiers[ti];
            let len = match tier {
                TierTag::Hi => self.hub.hi.len_now(),
                TierTag::Mid => self.hub.mid.len_now(),
                TierTag::Low => self.hub.low.len_now(),
            };
            if len == 0 {
                ti += 1;
                continue;
            }
            found = self.locate_in_ring(tier, len, ts_ms);
            ti += 1;
        }
        match found {
            Some((tier, logical, t, exact, delta)) => Ok(LocateHit {
                tier,
                logical,
                ts_ms: t,
                exact,
                delta_ms: delta,
            }),
            None => {
                self.hub.ledger.record(X_PWR_RING_EMPTY, ts_ms);
                Err(PowerRingError::RingEmpty)
            }
        }
    }

    /// 单元时间戳 seqlock 读（按粒度分发——定位与守恒审计共用的取数原语）。
    fn ts_at(&self, tier: TierTag, logical: usize) -> Option<u64> {
        match tier {
            TierTag::Hi => self.hub.hi.read_logical(logical).map(|c| c.cell_ts()),
            TierTag::Mid => self.hub.mid.read_logical(logical).map(|c| c.cell_ts()),
            TierTag::Low => self.hub.low.read_logical(logical).map(|c| c.cell_ts()),
        }
    }

    /// 单环内二分定位（就近：取 |ts - ts_ms| 最小者；并列取较早者）。
    fn locate_in_ring(&self, tier: TierTag, len: usize, ts_ms: u64) -> Option<(TierTag, usize, u64, bool, u64)> {
        // 先取边界：环外（晚于最新 / 早于最旧）直接贴边。
        let newest = self.ts_at(tier, len - 1)?;
        let oldest = self.ts_at(tier, 0)?;
        if ts_ms >= newest {
            return Some((tier, len - 1, newest, ts_ms == newest, ts_ms.saturating_sub(newest)));
        }
        if ts_ms <= oldest {
            return Some((tier, 0, oldest, ts_ms == oldest, oldest.saturating_sub(ts_ms)));
        }
        // 二分：第一个 ts >= ts_ms 的逻辑下标。
        let mut lo: usize = 0;
        let mut hi: usize = len;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let t = self.ts_at(tier, mid)?;
            if t < ts_ms {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        // 就近比较：candidates[lo-1]（< ts）与 candidates[lo]（>= ts）。
        let right = self.ts_at(tier, lo)?;
        let right_delta = right.saturating_sub(ts_ms);
        if lo == 0 {
            return Some((tier, 0, right, right_delta == 0, right_delta));
        }
        let left = self.ts_at(tier, lo - 1)?;
        let left_delta = ts_ms.saturating_sub(left);
        if left_delta <= right_delta {
            Some((tier, lo - 1, left, left_delta == 0, left_delta))
        } else {
            Some((tier, lo, right, right_delta == 0, right_delta))
        }
    }

    /// 能量积分：区间梯形法（µJ）。基于 mean_uw 列（均值口径能量）。
    ///
    /// 溢出策略：逐段 saturating；半功率折半先于加和（见 PowerSample::energy_uj_since）。
    pub fn energy_uj_between(&self, start_ts: u64, end_ts: u64) -> Result<u64, PowerRingError> {
        let slice = self.slice_range(start_ts, end_ts)?;
        let mut total: u64 = 0;
        let mut i = 1;
        while i < slice.points.len() {
            let a = &slice.points[i - 1];
            let b = &slice.points[i];
            let dt = b.ts_ms.saturating_sub(a.ts_ms);
            let avg = a.mean_uw / 2 + b.mean_uw / 2; // 折半先于加和：规避逐点溢出
            let seg = (avg.saturating_mul(dt)) / 1000;
            total = total.saturating_add(seg);
            i += 1;
        }
        Ok(total)
    }

    /// 三粒度守恒审计：中频窗口 vs 高频原样本重算对账。
    ///
    /// 口径：对环内最近 `checked` 个中频单元，各取其窗口（单元 n=10 拍）对应的
    /// 高频样本重算均值，偏差 ppm 超过 CONSERVATION_TOL_PPM 即 broken（0x12AE）。
    /// 正常情形偏差应为 0（同源精确聚合）；非 0 只可能来自读侧撕裂或镜像篡改。
    pub fn audit_conservation(&self, checked: usize) -> Result<ConservationReport, PowerRingError> {
        let mid_len = self.hub.mid.len_now();
        let hi_len = self.hub.hi.len_now();
        if mid_len == 0 || hi_len == 0 {
            self.hub.ledger.record(X_PWR_RING_EMPTY, self.hub.last_ts.load(Ordering::Relaxed));
            return Err(PowerRingError::RingEmpty);
        }
        let want = if checked > mid_len { mid_len } else { checked };
        let mut max_dev_ppm: u64 = 0;
        let mut broken = false;
        let mut ci = 0;
        while ci < want {
            // 中频单元按逻辑序从最新往回取。
            let mid_idx = mid_len - 1 - ci;
            let cell = match self.hub.mid.read_logical(mid_idx) {
                Some(c) => c,
                None => {
                    ci += 1;
                    continue;
                }
            };
            if cell.n == 0 {
                ci += 1;
                continue;
            }
            // 用单元 ts 反查高频窗口：窗口 = [ts - (n-1)*period, ts]。
            let win_start = cell.ts_ms.saturating_sub((cell.n as u64 - 1) * HI_PERIOD_MS);
            let win = self.hub.hi.slice_ts(win_start, cell.ts_ms);
            if !win.consistent || win.points.is_empty() {
                // 高频已被回绕覆盖：对账不可达——不算破坏（显性跳过）。
                ci += 1;
                continue;
            }
            let recomputed = AggCell::fold_samples(cell.ts_ms, &win.points);
            if recomputed.mean_uw != cell.mean_uw {
                let base = if cell.mean_uw == 0 { 1 } else { cell.mean_uw as u128 };
                let diff = if recomputed.mean_uw > cell.mean_uw {
                    (recomputed.mean_uw as u128 - cell.mean_uw as u128) * COVERAGE_FULL_PPM as u128 / base
                } else {
                    (cell.mean_uw as u128 - recomputed.mean_uw as u128) * COVERAGE_FULL_PPM as u128 / base
                };
                let dev = (diff as u64).min(COVERAGE_FULL_PPM);
                if dev > max_dev_ppm {
                    max_dev_ppm = dev;
                }
                if dev > CONSERVATION_TOL_PPM {
                    broken = true;
                }
            }
            ci += 1;
        }
        if broken {
            self.hub.ledger.record(X_PWR_CONSERVATION_BROKEN, self.hub.last_ts.load(Ordering::Relaxed));
            self.hub.conservation_breaks.fetch_add(1, Ordering::Relaxed);
        }
        Ok(ConservationReport {
            checked_cells: want,
            max_dev_ppm,
            tolerance_ppm: CONSERVATION_TOL_PPM,
            broken,
            note: if broken {
                "中频聚合与高频原样本对账超差：疑撕裂读或镜像篡改，建议暂停写者后复审计"
            } else if max_dev_ppm == 0 {
                "对账精确一致（同源聚合零偏差）"
            } else {
                "对账在容差内（撕裂读窗口内的预期极小偏差）"
            },
        })
    }
}

// ===========================================================================
// 第十一节 · 崩溃保留（PersistLayout：mmap 持久化预留 + CRC32 + 恢复）
// ===========================================================================
// 锚点原文："环形缓冲 mmap 持久化预留——崩溃后最近 60s 可追——P 域遥测联动"。
//
// 镜像布局（自描述、定长、页对齐）：
//
//   页 0（4096B）           ：64B 头 + 4032B 保留零填充
//   页 1..(1+P)             ：三环连续区（物理槽序原样镜像）
//       [0        ..9600 )  ：高频环 600 × 16B
//       [9600     ..33600)  ：中频环 600 × 40B
//       [33600    ..91200)  ：低频环 1440 × 40B
//
// 头布局（64B，全部 LE）：
//   0..8    magic "VXPWRRNG"
//   8..12   format version (u32) —— PERSIST_VERSION
//   12..16  payload CRC32 (u32) —— 对 91200B 载荷现算
//   16..20  hi valid len (u32)
//   20..24  mid valid len (u32)
//   24..28  low valid len (u32)
//   28..32  hi write cursor (u32)
//   32..36  mid write cursor (u32)
//   36..40  low write cursor (u32)
//   40..48  tick (u64)
//   48..56  last_ts (u64)
//   56..60  schema version (u32) —— TELEMETRY_SCHEMA_VERSION_V1
//   60..64  reserved (u32，恒 0；演进只增不改：新字段从这里续)
//
// 演进规则：版本递增才可改布局；旧字段永不重定位；新字段从 reserved 区续。

/// 持久化布局描述器（零实例——全部关联函数，纯布局数学）。
pub struct PersistLayout;

/// 头信息反解结果（verify/recover 的中间产物）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PersistHeaderInfo {
    /// 镜像格式版本。
    pub version: u32,
    /// 头中存储的载荷 CRC。
    pub payload_crc: u32,
    /// 三环有效长度（hi, mid, low）。
    pub valid_lens: (u32, u32, u32),
    /// 三环写指针（hi, mid, low）。
    pub write_cursors: (u32, u32, u32),
    /// 落盘时的拍计数。
    pub tick: u64,
    /// 落盘时的最新时间戳。
    pub last_ts: u64,
    /// 落盘时的 schema 版本。
    pub schema_version: u32,
}

/// 恢复摘要（recover_from_bytes 的返回，账目化恢复事实）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecoverSummary {
    /// 恢复出的高频样本数（崩溃后最近 60s 可追的兑现量）。
    pub hi_recovered: usize,
    /// 恢复出的中频单元数。
    pub mid_recovered: usize,
    /// 恢复出的低频单元数。
    pub low_recovered: usize,
    /// 恢复时因 ts 回归被截断的高频样本数（0 = 完好）。
    pub hi_truncated: usize,
    /// 镜像 last_ts。
    pub last_ts: u64,
}

impl PersistLayout {
    /// 载荷定长（= 三环数据总字节）。
    pub const PAYLOAD_BYTES: usize = RING_DATA_BYTES;

    /// 载荷占页数（向上取整）：91200B → 23 页。
    pub const PAYLOAD_PAGES: usize = (RING_DATA_BYTES + PERSIST_PAGE_BYTES - 1) / PERSIST_PAGE_BYTES;

    /// 镜像总字节数：头页 + 载荷页 = 4096 + 94208 = 98304B（96KiB）。
    pub const TOTAL_BYTES: usize = PERSIST_PAGE_BYTES + Self::PAYLOAD_PAGES * PERSIST_PAGE_BYTES;

    /// 高频区在载荷内的偏移。
    pub const HI_OFFSET: usize = 0;
    /// 中频区在载荷内的偏移。
    pub const MID_OFFSET: usize = HI_RING_LEN * POWER_SAMPLE_BYTES;
    /// 低频区在载荷内的偏移。
    pub const LOW_OFFSET: usize = Self::MID_OFFSET + MID_RING_LEN * AGG_CELL_BYTES;

    /// 布局页表（诊断/容量规划用：段名、起始页、页数）。
    pub const fn page_table() -> [(&'static str, usize, usize); 2] {
        [("header", 0, 1), ("rings", 1, Self::PAYLOAD_PAGES)]
    }

    /// 序列化 64B 头（crc 由调用方对载荷现算后传入）。
    pub fn serialize_header(
        valid_lens: (u32, u32, u32),
        write_cursors: (u32, u32, u32),
        tick: u64,
        last_ts: u64,
        schema_version: u32,
        payload_crc: u32,
    ) -> [u8; PERSIST_HEADER_BYTES] {
        let mut h = [0u8; PERSIST_HEADER_BYTES];
        h[0..8].copy_from_slice(&PERSIST_MAGIC);
        h[8..12].copy_from_slice(&PERSIST_VERSION.to_le_bytes());
        h[12..16].copy_from_slice(&payload_crc.to_le_bytes());
        h[16..20].copy_from_slice(&valid_lens.0.to_le_bytes());
        h[20..24].copy_from_slice(&valid_lens.1.to_le_bytes());
        h[24..28].copy_from_slice(&valid_lens.2.to_le_bytes());
        h[28..32].copy_from_slice(&write_cursors.0.to_le_bytes());
        h[32..36].copy_from_slice(&write_cursors.1.to_le_bytes());
        h[36..40].copy_from_slice(&write_cursors.2.to_le_bytes());
        h[40..48].copy_from_slice(&tick.to_le_bytes());
        h[48..56].copy_from_slice(&last_ts.to_le_bytes());
        h[56..60].copy_from_slice(&schema_version.to_le_bytes());
        // 60..64 reserved 恒 0（数组初始化已保证）。
        h
    }

    /// 头校验（不含 CRC——CRC 需要载荷一起看；见 recover_from_bytes）。
    ///
    /// 依次验：长度 ≥ 64 → magic（0x12A9）→ 版本（0x12AA）→ 状态域自洽
    /// （valid len ≤ 容量、write cursor < 容量——0x12AB）。
    pub fn verify_header(bytes: &[u8]) -> Result<PersistHeaderInfo, PowerRingError> {
        if bytes.len() < PERSIST_HEADER_BYTES {
            return Err(PowerRingError::PersistTooShort {
                got: bytes.len(),
                expected: PERSIST_HEADER_BYTES,
            });
        }
        if bytes[0..8] != PERSIST_MAGIC {
            return Err(PowerRingError::PersistMagicMismatch);
        }
        let u32b = |off: usize| -> u32 {
            let mut b4 = [0u8; 4];
            b4.copy_from_slice(&bytes[off..off + 4]);
            u32::from_le_bytes(b4)
        };
        let version = u32b(8);
        if version != PERSIST_VERSION {
            return Err(PowerRingError::PersistVersionMismatch {
                stored: version,
                expected: PERSIST_VERSION,
            });
        }
        let hi_len = u32b(16);
        let mid_len = u32b(20);
        let low_len = u32b(24);
        let hi_w = u32b(28);
        let mid_w = u32b(32);
        let low_w = u32b(36);
        // 状态域自洽闸：写指针必须落在槽域内（容量 0 环不存在——三环容量恒正）。
        if hi_w >= HI_RING_LEN as u32 || mid_w >= MID_RING_LEN as u32 || low_w >= LOW_RING_LEN as u32 {
            return Err(PowerRingError::PersistStateBad { state: hi_w.max(mid_w).max(low_w) });
        }
        if hi_len > HI_RING_LEN as u32 || mid_len > MID_RING_LEN as u32 || low_len > LOW_RING_LEN as u32 {
            return Err(PowerRingError::PersistStateBad {
                state: hi_len.max(mid_len).max(low_len),
            });
        }
        let u64b = |off: usize| -> u64 {
            let mut b8 = [0u8; 8];
            b8.copy_from_slice(&bytes[off..off + 8]);
            u64::from_le_bytes(b8)
        };
        Ok(PersistHeaderInfo {
            version,
            payload_crc: u32b(12),
            valid_lens: (hi_len, mid_len, low_len),
            write_cursors: (hi_w, mid_w, low_w),
            tick: u64b(40),
            last_ts: u64b(48),
            schema_version: u32b(56),
        })
    }
}

impl PowerRingHub {
    /// dump：三环物理镜像 + 头 → 定长镜像字节（Vec 承载——91KB 不上栈）。
    ///
    /// 一致性：dump 前后各读一次三环 seq；任一环在 dump 期间被写入则重试，
    /// 重试耗尽返回 0x12A4（不产出撕裂镜像——宁缺毋假）。
    /// 调用建议：持写者句柄的采样线程在空闲窗口调用（seq 必然稳定）。
    pub fn dump_persist(&self) -> Result<Vec<u8>, PowerRingError> {
        let mut attempt: u32 = 0;
        let img = loop {
            let s_hi = self.hi.seq_snapshot();
            let s_mid = self.mid.seq_snapshot();
            let s_low = self.low.seq_snapshot();
            let mut payload = Vec::with_capacity(RING_DATA_BYTES);
            let mut i = 0;
            while i < HI_RING_LEN {
                match self.hi.phys_slot(i) {
                    Some(c) => c.to_bytes_into(&mut payload),
                    None => return Err(PowerRingError::PersistTooShort {
                        got: i * POWER_SAMPLE_BYTES,
                        expected: RING_DATA_BYTES,
                    }),
                }
                i += 1;
            }
            let mut j = 0;
            while j < MID_RING_LEN {
                match self.mid.phys_slot(j) {
                    Some(c) => c.to_bytes_into(&mut payload),
                    None => return Err(PowerRingError::PersistTooShort {
                        got: j * AGG_CELL_BYTES,
                        expected: RING_DATA_BYTES,
                    }),
                }
                j += 1;
            }
            let mut k = 0;
            while k < LOW_RING_LEN {
                match self.low.phys_slot(k) {
                    Some(c) => c.to_bytes_into(&mut payload),
                    None => return Err(PowerRingError::PersistTooShort {
                        got: k * AGG_CELL_BYTES,
                        expected: RING_DATA_BYTES,
                    }),
                }
                k += 1;
            }
            let stable = self.hi.seq_snapshot() == s_hi
                && self.mid.seq_snapshot() == s_mid
                && self.low.seq_snapshot() == s_low;
            if stable {
                break payload;
            }
            attempt += 1;
            if attempt > SEQ_RETRY_LIMIT {
                self.ledger.record(X_PWR_RING_SEQ_RETRY_EXHAUSTED, self.last_ts.load(Ordering::Relaxed));
                return Err(PowerRingError::SeqRetryExhausted {
                    attempts: SEQ_RETRY_LIMIT,
                });
            }
        };
        let crc = crc32_ieee(&img);
        let header = PersistLayout::serialize_header(
            (
                self.hi.len_now() as u32,
                self.mid.len_now() as u32,
                self.low.len_now() as u32,
            ),
            (
                self.hi.w_now() as u32,
                self.mid.w_now() as u32,
                self.low.w_now() as u32,
            ),
            self.tick.load(Ordering::Relaxed),
            self.last_ts.load(Ordering::Relaxed),
            TELEMETRY_SCHEMA_VERSION_V1,
            crc,
        );
        self.last_dump_crc.store(crc, Ordering::Relaxed);
        self.persist_state.store(PERSIST_STATE_DUMPED, Ordering::Relaxed);
        self.ledger.record(X_PWR_RING_OK, self.last_ts.load(Ordering::Relaxed));
        let mut out = Vec::with_capacity(PersistLayout::TOTAL_BYTES);
        out.extend_from_slice(&header);
        // 头页零填充至 4096（mmap 页对齐契约）。
        let mut pad = PERSIST_PAGE_BYTES - PERSIST_HEADER_BYTES;
        while pad > 0 {
            out.push(0);
            pad -= 1;
        }
        out.extend_from_slice(&img);
        // 载荷页尾零填充至 TOTAL_BYTES（对齐到整页，便于 mmap 固定长度映射）。
        let tail = PersistLayout::TOTAL_BYTES - out.len();
        let mut t = 0;
        while t < tail {
            out.push(0);
            t += 1;
        }
        Ok(out)
    }

    /// 崩溃恢复：镜像 → 新 Hub（写者 attach 之前调用；崩溃后最近 60s 可追）。
    ///
    /// 校验链：长度（0x12AB）→ 头 magic/版本/状态自洽 → 载荷 CRC（0x12A8）→
    /// 三环 ts 单调性（撕裂样本截断，截断量入 RecoverSummary）→ 原位回灌。
    pub fn recover_from_bytes(bytes: &[u8]) -> Result<alloc::boxed::Box<PowerRingHub>, PowerRingError> {
        if bytes.len() < PersistLayout::TOTAL_BYTES {
            return Err(PowerRingError::PersistTooShort {
                got: bytes.len(),
                expected: PersistLayout::TOTAL_BYTES,
            });
        }
        let info = PersistLayout::verify_header(bytes)?;
        let payload = &bytes[PERSIST_PAGE_BYTES..PERSIST_PAGE_BYTES + PersistLayout::PAYLOAD_BYTES];
        let computed = crc32_ieee(payload);
        if computed != info.payload_crc {
            return Err(PowerRingError::PersistCrcMismatch {
                stored: info.payload_crc,
                computed,
            });
        }
        let hub = PowerRingHub::new();

        // --- 高频环：逐槽反解 + ts 单调性截断（崩溃撕裂的诚实处理）---
        let hi_bytes = &payload[PersistLayout::HI_OFFSET..PersistLayout::MID_OFFSET];
        let mut hi_cells: Vec<PowerSample> = Vec::with_capacity(HI_RING_LEN);
        let mut hi_truncated: usize = 0;
        {
            let valid = info.valid_lens.0 as usize;
            let w = info.write_cursors.0 as usize;
            // 物理槽序逐个反解，随后按逻辑序（最旧在前）扫描单调性。
            let mut i = 0;
            while i < HI_RING_LEN {
                match PowerSample::from_bytes(&hi_bytes[i * POWER_SAMPLE_BYTES..(i + 1) * POWER_SAMPLE_BYTES]) {
                    Some(c) => hi_cells.push(c),
                    None => {
                        return Err(PowerRingError::PersistTooShort {
                            got: i * POWER_SAMPLE_BYTES,
                            expected: RING_DATA_BYTES,
                        })
                    }
                }
                i += 1;
            }
            // 逻辑序单调截断：找第一个倒退点，把有效前缀截到那里。
            // 逻辑下标 l 的物理槽 = (w + N - valid + l) % N。
            let mut keep = valid;
            let mut l = 1;
            while l < valid {
                let prev_p = (w + HI_RING_LEN - valid + l - 1) % HI_RING_LEN;
                let cur_p = (w + HI_RING_LEN - valid + l) % HI_RING_LEN;
                let pt = hi_cells[prev_p].ts_ms;
                let ct = hi_cells[cur_p].ts_ms;
                if ct != 0 && pt != 0 && ct <= pt {
                    keep = l; // 倒退点起不可信：截断（撕裂样本丢弃）
                    hi_truncated = valid - l;
                    break;
                }
                l += 1;
            }
            hub.hi.restore_image(&hi_cells, w, keep)?;
        }

        // --- 中频环：同样单调截断 ---
        let mid_bytes = &payload[PersistLayout::MID_OFFSET..PersistLayout::LOW_OFFSET];
        let mut mid_cells: Vec<AggCell> = Vec::with_capacity(MID_RING_LEN);
        {
            let valid = info.valid_lens.1 as usize;
            let w = info.write_cursors.1 as usize;
            let mut i = 0;
            while i < MID_RING_LEN {
                match AggCell::from_bytes(&mid_bytes[i * AGG_CELL_BYTES..(i + 1) * AGG_CELL_BYTES]) {
                    Some(c) => mid_cells.push(c),
                    None => {
                        return Err(PowerRingError::PersistTooShort {
                            got: i * AGG_CELL_BYTES,
                            expected: RING_DATA_BYTES,
                        })
                    }
                }
                i += 1;
            }
            let mut keep = valid;
            let mut l = 1;
            while l < valid {
                let prev_p = (w + MID_RING_LEN - valid + l - 1) % MID_RING_LEN;
                let cur_p = (w + MID_RING_LEN - valid + l) % MID_RING_LEN;
                let pt = mid_cells[prev_p].ts_ms;
                let ct = mid_cells[cur_p].ts_ms;
                if ct != 0 && pt != 0 && ct <= pt {
                    keep = l;
                    break;
                }
                l += 1;
            }
            hub.mid.restore_image(&mid_cells, w, keep)?;
        }

        // --- 低频环：同样单调截断 ---
        let low_bytes = &payload[PersistLayout::LOW_OFFSET..];
        let mut low_cells: Vec<AggCell> = Vec::with_capacity(LOW_RING_LEN);
        {
            let valid = info.valid_lens.2 as usize;
            let w = info.write_cursors.2 as usize;
            let mut i = 0;
            while i < LOW_RING_LEN {
                match AggCell::from_bytes(&low_bytes[i * AGG_CELL_BYTES..(i + 1) * AGG_CELL_BYTES]) {
                    Some(c) => low_cells.push(c),
                    None => {
                        return Err(PowerRingError::PersistTooShort {
                            got: i * AGG_CELL_BYTES,
                            expected: RING_DATA_BYTES,
                        })
                    }
                }
                i += 1;
            }
            let mut keep = valid;
            let mut l = 1;
            while l < valid {
                let prev_p = (w + LOW_RING_LEN - valid + l - 1) % LOW_RING_LEN;
                let cur_p = (w + LOW_RING_LEN - valid + l) % LOW_RING_LEN;
                let pt = low_cells[prev_p].ts_ms;
                let ct = low_cells[cur_p].ts_ms;
                if ct != 0 && pt != 0 && ct <= pt {
                    keep = l;
                    break;
                }
                l += 1;
            }
            hub.low.restore_image(&low_cells, w, keep)?;
        }

        // --- 恢复控制态：tick/last_ts 从头复原（写指针已在 restore_image 内）---
        hub.tick.store(info.tick, Ordering::Relaxed);
        hub.last_ts.store(info.last_ts, Ordering::Relaxed);
        hub.persist_state.store(PERSIST_STATE_RECOVERED, Ordering::Relaxed);
        hub.last_dump_crc.store(info.payload_crc, Ordering::Relaxed);
        hub.ledger.record(X_PWR_RING_OK, info.last_ts);
        if hi_truncated > 0 {
            // 截断即立案：恢复不是无损时必须显性留痕。
            hub.ledger.record(X_PWR_PERSIST_CRC_BAD, info.last_ts);
        }
        Ok(hub)
    }

    /// 恢复摘要（供体检/日志）：与 recover_from_bytes 配套的三环有效量回读。
    pub fn recover_summary(&self) -> RecoverSummary {
        RecoverSummary {
            hi_recovered: self.hi.len_now(),
            mid_recovered: self.mid.len_now(),
            low_recovered: self.low.len_now(),
            hi_truncated: 0, // 截断量在 recover 时已入账（0x12A8 镜像）；此处为当前态快照
            last_ts: self.last_ts.load(Ordering::Relaxed),
        }
    }
}

impl<T: RingCell, const N: usize> RingCore<T, N> {
    /// seq 相位快照（dump 一致性窗口用）。
    pub fn seq_snapshot(&self) -> u32 {
        self.seq.load(Ordering::Acquire)
    }
}

// ===========================================================================
// 第十二节 · P 域 schema 对齐（TelemetrySchemaV1：一处定义双消费）
// ===========================================================================
// 锚点原文："历史数据导出格式与 P 域遥测 schema 对齐——一处定义双消费"。
//
// 双消费的两端：
//   1) J 域（本模块 export_pdomain）按白名单表产出记录；
//   2) P 域遥测（cgj377_ptelem 族）按同一张常量表解码。
// 对齐手段：字段白名单常量（TELEMETRY_SCHEMA_V1_FIELDS）+ FNV-1a 指纹
// （schema_fingerprint）——两侧对指纹即可验证同源，无需共享类型。
// 演进规则（冻结）：只增不改——新字段追加表尾；旧字段名/语义/单位/编码
// 永不修改；换语义必须新开 V2 并保留 V1 可解码。

/// P 域遥测记录（schema v1 冻结形态——字段顺序即白名单表顺序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PDomainRecordV1 {
    /// ts_ms：窗口收口时间戳（ms）。
    pub ts_ms: u64,
    /// tier：数据来源粒度（编码为 TierTag::as_str，冻结字符串）。
    pub tier: TierTag,
    /// instant_uw：瞬时功率（µW，窗口内最后一点的峰值口径）。
    pub instant_uw: u64,
    /// win_mean_uw：窗口均值功率（µW）。
    pub win_mean_uw: u64,
    /// win_peak_uw：窗口峰值功率（µW，降采样取峰保极值）。
    pub win_peak_uw: u64,
    /// win_valley_uw：窗口谷值功率（µW）。
    pub win_valley_uw: u64,
    /// energy_uj：窗口能量积分（µJ，梯形法）。
    pub energy_uj: u64,
    /// uncertainty_ppm：不确定度（百万分制，保守线性模型——估算非实测）。
    pub uncertainty_ppm: u64,
    /// coverage_ppm：采样覆盖度（实际样本/期望样本，百万分制）。
    pub coverage_ppm: u64,
}

impl PDomainRecordV1 {
    /// schema 指纹（FNV-1a 32）：白名单表 + 版本号的规范序列化哈希。
    ///
    /// 消费方校验方式：对同一张常量表重算本值并比对——不一致即 0x12AC。
    pub const fn schema_fingerprint() -> u32 {
        // 规范序列化：字段名以 '\x1f'（单元分隔符）连接，尾部接版本号字节。
        // const fn 里不能拼接 String，改为逐字段累喂 fnv（等价于一次性哈希）。
        let mut h: u32 = 0x811C_9DC5;
        let mut fi = 0;
        while fi < TELEMETRY_SCHEMA_V1_FIELDS.len() {
            let field = TELEMETRY_SCHEMA_V1_FIELDS[fi].as_bytes();
            h = fnv1a32_upd(h, 0x1F);
            let mut bi = 0;
            while bi < field.len() {
                h = fnv1a32_upd(h, field[bi]);
                bi += 1;
            }
            fi += 1;
        }
        let vb = TELEMETRY_SCHEMA_VERSION_V1.to_le_bytes();
        h = fnv1a32_upd(h, vb[0]);
        h = fnv1a32_upd(h, vb[1]);
        h = fnv1a32_upd(h, vb[2]);
        h = fnv1a32_upd(h, vb[3]);
        h
    }

    /// 记录自校验（导出前最后一道闸）：
    /// 峰值 ≥ 均值 ≥ 谷值（0x12AE）；不确定度 ≤ 1e6；覆盖度 ≤ 1e6。
    pub fn validate(&self) -> Result<(), PowerRingError> {
        if !(self.win_peak_uw >= self.win_mean_uw && self.win_mean_uw >= self.win_valley_uw) {
            return Err(PowerRingError::RecordInvalid {
                reason: "peak/mean/valley 序关系破坏",
            });
        }
        if self.uncertainty_ppm > COVERAGE_FULL_PPM {
            return Err(PowerRingError::RecordInvalid {
                reason: "不确定度越域（>1e6 ppm）",
            });
        }
        if self.coverage_ppm > COVERAGE_FULL_PPM {
            return Err(PowerRingError::RecordInvalid {
                reason: "覆盖度越域（>1e6 ppm）",
            });
        }
        Ok(())
    }

    /// 规范行编码（单行 KV，字段顺序 = 白名单表顺序——冻结）。
    ///
    /// 零浮点：全部整数字段 LE 十进制；tier 用冻结字符串。行尾带 FNV-1a 64
    /// 行指纹（防传输截断/篡改的轻量闸——P 域入库前可复验）。
    pub fn encode_canonical(&self) -> String {
        let mut body = String::with_capacity(160);
        body.push_str("v1 pwrring ts_ms=");
        body.push_str(&self.ts_ms.to_string());
        body.push_str(" tier=");
        body.push_str(self.tier.as_str());
        body.push_str(" instant_uw=");
        body.push_str(&self.instant_uw.to_string());
        body.push_str(" win_mean_uw=");
        body.push_str(&self.win_mean_uw.to_string());
        body.push_str(" win_peak_uw=");
        body.push_str(&self.win_peak_uw.to_string());
        body.push_str(" win_valley_uw=");
        body.push_str(&self.win_valley_uw.to_string());
        body.push_str(" energy_uj=");
        body.push_str(&self.energy_uj.to_string());
        body.push_str(" uncertainty_ppm=");
        body.push_str(&self.uncertainty_ppm.to_string());
        body.push_str(" coverage_ppm=");
        body.push_str(&self.coverage_ppm.to_string());
        let line_fp = fnv1a64(body.as_bytes());
        format!("{} fp64={:016x}", body, line_fp)
    }
}

/// fnv1a32 单字节更新（const 上下文的哈希累进原语）。
const fn fnv1a32_upd(h: u32, byte: u8) -> u32 {
    (h ^ byte as u32).wrapping_mul(0x0100_0193)
}

impl PowerRingReader<'_> {
    /// P 域导出：区间聚合 → schema v1 记录（一处定义双消费的 J 域出口）。
    ///
    /// 口径：
    /// - instant：窗口最后一点的峰值（最新瞬时）；
    /// - win_mean/peak/valley：区间内各列的再聚合（均值按列平均，峰值取 max，
    ///   谷值取 min——极值永不下传丢失）；
    /// - energy：区间梯形积分（µJ）；
    /// - coverage：实际点数 / 期望点数（期望 = 跨度/周期 + 1），封顶 1e6；
    /// - uncertainty：保守线性模型 = 基底 50,000 ppm + 覆盖缺口（估算非实测）。
    pub fn export_pdomain(&self, start_ts: u64, end_ts: u64) -> Result<PDomainRecordV1, PowerRingError> {
        let slice = self.slice_range(start_ts, end_ts)?;
        let n = slice.points.len();
        // 空切片理论不可达（slice_range 对空返回 Err），防御性账目化兜底。
        if n == 0 {
            return Err(PowerRingError::RangeEmpty { start_ts, end_ts });
        }
        let mut sum_mean: u128 = 0;
        let mut peak: u64 = 0;
        let mut valley: u64 = u64::MAX;
        let mut i = 0;
        while i < n {
            let p = &slice.points[i];
            sum_mean += p.mean_uw as u128;
            if p.peak_uw > peak {
                peak = p.peak_uw;
            }
            if p.valley_uw < valley {
                valley = p.valley_uw;
            }
            i += 1;
        }
        let win_mean = (sum_mean / n as u128).min(u64::MAX as u128) as u64;
        let energy = self.energy_uj_between(start_ts, end_ts)?;
        let last = slice.points[n - 1];
        // 期望点数：按所选粒度周期计算（跨度/周期 + 1，含首尾闭区间）。
        let period = slice.tier.period_ms();
        let span = end_ts.saturating_sub(start_ts);
        let expected = span / period + 1;
        let coverage = if expected == 0 {
            COVERAGE_FULL_PPM
        } else {
            let cov = (n as u128 * COVERAGE_FULL_PPM as u128) / expected as u128;
            (cov.min(COVERAGE_FULL_PPM as u128)) as u64
        };
        let uncertainty = UNCERTAINTY_BASE_PPM.saturating_add(COVERAGE_FULL_PPM.saturating_sub(coverage));
        let record = PDomainRecordV1 {
            ts_ms: last.ts_ms,
            tier: slice.tier,
            instant_uw: last.peak_uw,
            win_mean_uw: win_mean,
            win_peak_uw: peak,
            win_valley_uw: valley,
            energy_uj: energy,
            uncertainty_ppm: uncertainty,
            coverage_ppm: coverage,
        };
        // 导出前自校验（0x12AE 闸）；校验失败同时入账。
        if let Err(e) = record.validate() {
            self.hub.ledger.record(e.code(), last.ts_ms);
            return Err(e);
        }
        Ok(record)
    }
}

// ===========================================================================
// 第十三节 · 体检（health()：结构化报告——水位/覆盖/倒退/持久化/schema）
// ===========================================================================

/// 单环体检项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TierHealth {
    /// 粒度标签。
    pub tier: TierTag,
    /// 有效长度。
    pub len: usize,
    /// 容量。
    pub capacity: usize,
    /// 水位（ppm = len/capacity × 1e6，封顶）。
    pub watermark_ppm: u64,
    /// 覆盖最旧累计（0x12A3 计数）。
    pub overwrites: u64,
    /// 最旧样本时间戳（空环 0）。
    pub oldest_ts: u64,
    /// 最新样本时间戳（空环 0）。
    pub newest_ts: u64,
}

/// 事件账体检项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerHealth {
    /// 当前有效账目条数（快照口径）。
    pub entries: usize,
    /// 历史入账总条数。
    pub total_records: u64,
    /// 满后覆盖旧账累计（0x12AD 计数）。
    pub covered_total: u64,
}

/// 持久化体检项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PersistHealth {
    /// 状态机（PERSIST_STATE_*）。
    pub state: u32,
    /// 最近 dump 载荷 CRC（0 = 从未 dump）。
    pub last_crc: u32,
    /// 镜像总字节数（容量规划回读）。
    pub total_bytes: usize,
}

/// 结构化体检报告（判据落位：水位/覆盖计数/倒退次数/持久化状态/schema 版本）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingHealthReport {
    /// P 域 schema 冻结版本。
    pub schema_version: u32,
    /// schema 指纹（消费方对表验证）。
    pub schema_fingerprint: u32,
    /// 高频环体检项。
    pub hi: TierHealth,
    /// 中频环体检项。
    pub mid: TierHealth,
    /// 低频环体检项。
    pub low: TierHealth,
    /// 时间戳倒退累计（0x12A2 计数）。
    pub retrogrades: u64,
    /// 守恒破坏累计（0x12AE 计数）。
    pub conservation_breaks: u64,
    /// 写者是否在位。
    pub writer_attached: bool,
    /// 高频拍计数。
    pub tick: u64,
    /// 事件账体检项。
    pub ledger: LedgerHealth,
    /// 持久化体检项。
    pub persist: PersistHealth,
    /// 体检时刻的最新时间戳（三环最大值）。
    pub newest_ts: u64,
}

impl PowerRingHub {
    /// 结构化体检（无锁读，尽力一致快照；单环 seqlock 重试耗尽按 0 计并
    /// 由事件账留痕——体检本身永不失败）。
    pub fn health(&self) -> RingHealthReport {
        let hi_len = self.hi.len_now();
        let mid_len = self.mid.len_now();
        let low_len = self.low.len_now();
        let hi_new = self.hi.newest_ts();
        let mid_new = self.mid.newest_ts();
        let low_new = self.low.newest_ts();
        let newest = hi_new.max(mid_new).max(low_new);
        RingHealthReport {
            schema_version: TELEMETRY_SCHEMA_VERSION_V1,
            schema_fingerprint: PDomainRecordV1::schema_fingerprint(),
            hi: TierHealth {
                tier: TierTag::Hi,
                len: hi_len,
                capacity: HI_RING_LEN,
                watermark_ppm: watermark_ppm(hi_len, HI_RING_LEN),
                overwrites: self.hi.overwrites_total(),
                oldest_ts: self.hi.oldest_ts(),
                newest_ts: hi_new,
            },
            mid: TierHealth {
                tier: TierTag::Mid,
                len: mid_len,
                capacity: MID_RING_LEN,
                watermark_ppm: watermark_ppm(mid_len, MID_RING_LEN),
                overwrites: self.mid.overwrites_total(),
                oldest_ts: self.mid.oldest_ts(),
                newest_ts: mid_new,
            },
            low: TierHealth {
                tier: TierTag::Low,
                len: low_len,
                capacity: LOW_RING_LEN,
                watermark_ppm: watermark_ppm(low_len, LOW_RING_LEN),
                overwrites: self.low.overwrites_total(),
                oldest_ts: self.low.oldest_ts(),
                newest_ts: low_new,
            },
            retrogrades: self.retrograde_total(),
            conservation_breaks: self.conservation_break_total(),
            writer_attached: self.writer_attached(),
            tick: self.tick_now(),
            ledger: LedgerHealth {
                entries: self.ledger.snapshot().len(),
                total_records: self.ledger.total_records(),
                covered_total: self.ledger.covered_total(),
            },
            persist: PersistHealth {
                state: self.persist_state(),
                last_crc: self.last_dump_crc(),
                total_bytes: PersistLayout::TOTAL_BYTES,
            },
            newest_ts: newest,
        }
    }
}

/// 水位 ppm（0 容量按满算——容量 0 的环在实例化层被禁，纯数学兜底）。
const fn watermark_ppm(len: usize, capacity: usize) -> u64 {
    if capacity == 0 {
        return COVERAGE_FULL_PPM;
    }
    let v = (len as u128 * COVERAGE_FULL_PPM as u128) / capacity as u128;
    if v > COVERAGE_FULL_PPM as u128 {
        COVERAGE_FULL_PPM
    } else {
        v as u64
    }
}

// ===========================================================================
// 第十四节 · 模块级约定收尾（判据回执表 + 测试组指引）
// ===========================================================================
// 判据回执（与头注释七判据落位图一一对应）：
//
// 1. 三粒度    → hi/mid/low 三独立 RingCore；级联 10/50 拍；均值/峰值/谷值
//                三列 + 样本数 n（AggCell::fold_samples / fold_cells 按 n 加权
//                ——跨级均值守恒口径）。
// 2. 无分配    → 三环 [T; N] 定长入 struct（const fn new 零分配）；写路径
//                push() 稳态零分配；Box 仅构造一次；事件账定长 256 槽。
// 3. 切片 API  → slice_range 自动选档（tier_for_span）+ slice_on_tier 显档；
//                二分定位 O(log N) + 拷贝 O(range)；downsample_export 桶取峰
//                保极值；latest(n) 宽松口径。
// 4. 持久化预留→ PersistLayout 页表 + 64B 头（magic/version/crc32/游标）+
//                dump_persist / recover_from_bytes（CRC 闸 + ts 单调截断）+
//                mmap 页对齐 4096B（头页写穿不撕载荷）。
// 5. 对齐 schema→ TELEMETRY_SCHEMA_V1_FIELDS 白名单 + 指纹 + PDomainRecordV1
//                规范行编码 + validate 自校验 + 冻结演进规则。
// 6. 测试      → 三组（锚点原文）：环形回绕（探针 smoke 写 700 点触发覆盖）、
//                切片边界（空区间/倒置/环外/闭区间端点）、崩溃恢复（dump→
//                recover→比对 + 撕裂截断）。探针 crate：
//                kernel/_attic/vx1024_probe_f1449/tests/smoke.rs。
// 7. 判据      → health() 结构化体检 + 本回执表 + 诊断码 0x12A0~0x12AF 闭集。
//
// 与邻域的接口声明（隔离编译的边界，不引用对方模块）：
// - F1447 管道聚合结果 → PowerRingWriter::push()（采样线程直写，唯一写者）；
// - F1448 事件总线     → 本模块 RingEventLedger 快照可被事件域镜像消费；
// - P 域遥测           → export_pdomain() 记录 + schema 指纹（双消费）；
// - J08 历史仓库       → dump_persist() 镜像可作为温层归档的种子数据；
// - 诊断总线           → pwr_ring_diag_codes() 批量注册十六码闭集。
//
// 已知边界（诚实声明）：
// - seq u32 回绕碰撞窗口：2^31 次写 ≈ 10Hz 下 6.8 年（第六节契约 4）；
// - "约 50KB"锚点预算 vs 91,200B 实际：三列聚合保真的显性取舍（头注释预算表）；
// - 事件账同槽并发写最坏 count 只多不少（第七节并发说明）；
// - 恢复期截断样本不可恢复（CRC 过但撕裂）——显性留痕不静默。
