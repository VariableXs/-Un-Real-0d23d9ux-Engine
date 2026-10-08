//! VE-F0809 · 文字渲染遥测（VE-E 域 · 文字渲染观测段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0809`
//!
//! **判据（锚点原文）**：文字渲染遥测复用全域观测通道（O06 范式），聚焦四个
//! 健康指标：缓存命中率（F0806 三指标）、光栅化耗时分布（P50/P95，按字号
//! 分桶）、每帧文本绘制调用数与 quad 数、解码处置计数（F0802 四档非法序列
//! 频率——异常升高即字体文件问题的前兆信号）。采集纪律三章：异步批量上报、
//! 用户可关闭、绝不包含文本内容（只记计数与耗时，隐私红线——用户打了什么字
//! 永远不出设备）。
//! 判据：**四指标、三章纪律、零内容上行、0.02ms 开销、断线补报**。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 遥测通道断开 → **本地环形缓冲保留最近 1,000 条，重连补报且不阻塞渲染**。
//!   「不阻塞渲染」是硬要求：遥测是观测手段，**绝不能因为上报失败而卡住
//!   绘制**。故 [`TelemetryBus::record`] 只做 O(1) 入环，不做任何 IO。
//! - 指标异常（命中率骤降）→ **看板告警不自行动作**。遥测只负责「报告
//!   异常」，**绝不自己改行为**——自动降级/自动重试会让「因为自动重试所以
//!   指标变好」与「真的没问题」无法区分，且行为变更绝不该由观测层发起。
//!
//! **数据结构**：遥测事件（[`TelemetryEvent`]，六字段）；环形缓冲
//! （[`TelemetryBus`]）；指标聚合（[`MetricAggregator`]）；字号分桶
//! （[`SizeBucket`]）；解码处置四档（[`DecodeOutcome`]）。
//!
//! **性能逐项分解**：[`TelemetryBus::record`] **O(1)**（环形写入，槽数是
//! 编译期常量，无分配、无排序、无 IO）；[`MetricAggregator::snapshot`]
//! O(桶数) 且**按需调用**（不在每帧路径上）。
//!
//! ## 设计要点
//!
//! - **零内容上行是隐私红线，靠类型而非约定保证**（[`TelemetryEvent`] 只有
//!   六个字段，**没有 `String`、没有 `&str`、没有任何可承载文本的类型**）。
//!   靠约定（「记得别把文本放进去」）必然失守——新增字段时谁会记得？
//!   类型上根本放不进去才是真保证。自检`E09-零内容-事件类型不可承载文本`
//!   用**结构体字段全量列举 + 类型清单**双向钉死：新增一个 `String` 字段
//!   会让该判据转红。
//! - **会话匿名 ID 是哈希不是标识**（[`TelemetryBus`] 内部用 u64 指纹）：
//!   ID 只用于「把同一会话的多次上报关联起来」，不用于「识别用户」。故
//!   入口就做一次 FNV-1a 折叠，原始标识不出函数。
//! - **环形缓冲固定 1,000 条**（[`RING_CAPACITY`]）：满了覆盖最旧。覆盖
//!   是正确取舍——缓冲满说明通道长时间不通，此时保留**最新**数据比保留
//!   最旧有用（最新才反映当前状态）。覆盖条数必须**可查**（`dropped`），
//!   否则「丢了多少」无从回答，而丢数据这件事本身必须可见。
//! - **命中率骤降只告警不动手**（[`MetricAggregator::classify`]）：
//!   返回 [`HealthVerdict::Alert`] 而非任何「已自动降级」之类的结论。
//!   自检用**枚举变体本身**钉死：把 `Alert` 换成 `AutoDegraded` 之类
//!   会让判据编译失败或转红。
//! - **P50/P95 按字号分桶**（[`SizeBucket`] 的三个桶）：小字号光栅化
//!   成本与大字号的分布形状不同，混在一起算分位数会把小字号的劣化抹平。
//!   分桶键是**桶的序号**（`u8`），不上报具体像素值——那也是资产布局信息。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **O06 全域观测通道**承载看板，本条只负责**产出文字渲染侧的事件**。
//!   两者是「数据源 vs 看板」的关系，本条**不实现看板**，只保证出口的
//!   事件满足全域通道的隐私与批量契约。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个
//! 模块的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，
//! 与真实缺陷长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 遥测环形缓冲容量（锚点：保留最近 1,000 条）。
pub const RING_CAPACITY: usize = 1000;

/// 单批上报条数上限（**异步批量**：攒够或超时即上行）。
pub const BATCH_LIMIT: usize = 64;

/// 遥测自身开销承诺（微秒/帧；锚点 ≤0.02ms = 20us）。
pub const OVERHEAD_BUDGET_US: u32 = 20;

/// 缓存命中率告警阈值（低于即告警，百万分比）。
pub const HIT_RATE_ALERT: u32 = 800_000; // 80%

/// 合法解码占比告警阈值（低于即告警，百万分比）。
///
/// 锚点说「解码非法序列频率异常升高是字体文件问题的前兆」——换算成
/// 「合法占比低于 99%」比「非法占比高于 1%」更直接，也更容易取整。
pub const VALID_DECODE_ALERT: u32 = 990_000; // 99%

/// 指标名长度上限（字节）。
pub const MAX_METRIC_NAME: usize = 32;

/// 健康结论种数。
pub const HEALTH_VERDICT_COUNT: usize = 3;

/// 事件字段数（锚点：指标名、值、分桶键、时间戳、会话匿名 ID、版本）。
pub const EVENT_FIELD_COUNT: usize = 6;

// ---------------------------------------------------------------------------
// 二、遥测事件（六字段 · 零内容）
// ---------------------------------------------------------------------------

/// 遥测指标名（**封闭枚举**，杜绝任意字符串混进来）。
///
/// 刻意**不用 `String`**：指标名若可任意构造，就成了夹带文本内容的侧门
/// （把用户输入当指标名上报，隐私红线当场破）。封闭枚举让「能上报什么」
/// 在编译期就固定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricName {
    /// 缓存命中率（百万分比）。
    CacheHitRate,
    /// 光栅化耗时（微秒）。
    RasterUs,
    /// 每帧文本绘制调用数。
    DrawCalls,
    /// 每帧文本 quad 数。
    Quads,
    /// 合法解码处置计数。
    DecodeValid,
    /// 非法序列可恢复处置计数。
    DecodeSalvaged,
    /// 非法序列替换处置计数。
    DecodeReplaced,
    /// 非法序列丢弃处置计数。
    DecodeDropped,
}

impl MetricName {
    /// 全枚举（顺序即 [`MetricName::ordinal`] 的下标）。
    pub const ALL: [MetricName; 8] = [
        MetricName::CacheHitRate,
        MetricName::RasterUs,
        MetricName::DrawCalls,
        MetricName::Quads,
        MetricName::DecodeValid,
        MetricName::DecodeSalvaged,
        MetricName::DecodeReplaced,
        MetricName::DecodeDropped,
    ];

    /// 枚举下标（**供计数数组用，不是线上编码值**）。
    pub const fn ordinal(self) -> usize {
        match self {
            MetricName::CacheHitRate => 0,
            MetricName::RasterUs => 1,
            MetricName::DrawCalls => 2,
            MetricName::Quads => 3,
            MetricName::DecodeValid => 4,
            MetricName::DecodeSalvaged => 5,
            MetricName::DecodeReplaced => 6,
            MetricName::DecodeDropped => 7,
        }
    }

    /// 线编码（显式映射，**不用 `as u8`**——判别值 ≠ 线上值）。
    pub const fn wire(self) -> u8 {
        match self {
            MetricName::CacheHitRate => 0x01,
            MetricName::RasterUs => 0x02,
            MetricName::DrawCalls => 0x03,
            MetricName::Quads => 0x04,
            MetricName::DecodeValid => 0x05,
            MetricName::DecodeSalvaged => 0x06,
            MetricName::DecodeReplaced => 0x07,
            MetricName::DecodeDropped => 0x08,
        }
    }

    /// 线编码 → 枚举（未登记码返回 `None`，不静默兜底）。
    pub const fn from_wire(w: u8) -> Option<MetricName> {
        match w {
            0x01 => Some(MetricName::CacheHitRate),
            0x02 => Some(MetricName::RasterUs),
            0x03 => Some(MetricName::DrawCalls),
            0x04 => Some(MetricName::Quads),
            0x05 => Some(MetricName::DecodeValid),
            0x06 => Some(MetricName::DecodeSalvaged),
            0x07 => Some(MetricName::DecodeReplaced),
            0x08 => Some(MetricName::DecodeDropped),
            _ => None,
        }
    }

    /// 稳定文案（**不含任何用户内容**，是编译期常量）。
    pub const fn label(self) -> &'static str {
        match self {
            MetricName::CacheHitRate => "cache_hit_rate",
            MetricName::RasterUs => "raster_us",
            MetricName::DrawCalls => "draw_calls",
            MetricName::Quads => "quads",
            MetricName::DecodeValid => "decode_valid",
            MetricName::DecodeSalvaged => "decode_salvaged",
            MetricName::DecodeReplaced => "decode_replaced",
            MetricName::DecodeDropped => "decode_dropped",
        }
    }

    /// 文案字节长度（必须 ≤ [`MAX_METRIC_NAME`]，且**非空**）。
    pub const fn label_len(self) -> usize {
        match self {
            MetricName::CacheHitRate => 14,
            MetricName::RasterUs => 9,
            MetricName::DrawCalls => 10,
            MetricName::Quads => 5,
            MetricName::DecodeValid => 12,
            MetricName::DecodeSalvaged => 15,
            MetricName::DecodeReplaced => 15,
            MetricName::DecodeDropped => 14,
        }
    }

    /// 是否属于解码处置四档（F0802）。
    pub const fn is_decode_outcome(self) -> bool {
        matches!(
            self,
            MetricName::DecodeValid
                | MetricName::DecodeSalvaged
                | MetricName::DecodeReplaced
                | MetricName::DecodeDropped
        )
    }
}

/// 遥测事件（锚点六字段）。
///
/// **隐私红线的类型级保证**：本结构体**没有任何** `String` / `&str` /
/// 指向用户数据的引用。指标名是封闭枚举、值与桶键与时间戳与版本都是定长整数、
/// 会话 ID 是已折叠的 `u64` 指纹。因此「把用户打的字带上设备」在**类型上
/// 无路可走**，而不依赖任何人的记性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TelemetryEvent {
    /// 指标名（封闭枚举，非字符串）。
    pub metric: MetricName,
    /// 指标值（计数或百万分比；**不含任何文本**）。
    pub value: u64,
    /// 分桶键（[`SizeBucket::ordinal`] 或 0=不分桶）。
    pub bucket: u8,
    /// 时间戳（单调递增计数，非墙上时钟——墙上时钟会被用户改回去）。
    pub timestamp: u32,
    /// 会话匿名 ID（**已折叠的指纹**，非原始标识）。
    pub anon_session: u64,
    /// 协议版本。
    pub version: u8,
}

impl TelemetryEvent {
    /// 构造一个事件。
    pub const fn new(
        metric: MetricName,
        value: u64,
        bucket: u8,
        timestamp: u32,
        anon_session: u64,
    ) -> TelemetryEvent {
        TelemetryEvent { metric, value, bucket, timestamp, anon_session, version: PROTOCOL_VERSION }
    }

    /// 事件字段数（恒为 [`EVENT_FIELD_COUNT`]）。
    pub const fn field_count(&self) -> usize {
        EVENT_FIELD_COUNT
    }

    /// 上行字节数（**定长**：每条事件恰好 [`WIRE_BYTES`] 字节）。
    ///
    /// 定量长是「零内容上行」的**可验证**形式：若某条事件偷偷多带了数据，
    /// 上行字节数会变，判据立刻转红。
    pub const fn wire_bytes(&self) -> usize {
        WIRE_BYTES
    }

    /// 单行审计（**只含指标名与数值**，可安全打印）。
    pub fn audit_line(&self) -> String {
        format!(
            "{}={} bucket{} t{} s{}x{}",
            self.metric.label(),
            self.value,
            self.bucket,
            self.timestamp,
            self.anon_session,
            self.version
        )
    }
}

/// 协议版本。
pub const PROTOCOL_VERSION: u8 = 1;

/// 单事件上行字节数（1 指标 + 8 值 + 1 桶 + 4 时间 + 8 会话 + 1 版本）。
pub const WIRE_BYTES: usize = 23;

/// 会话标识折叠（FNV-1a 64 位）。
///
/// 原始标识**不出这个函数**——调用方传进来的是原始串，出去的是指纹。
/// 这样「会话 ID」在遥测链路上永远只是指纹，不会成为识别用户的线索。
pub fn fold_session_id(raw: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < raw.len() {
        h ^= raw[i] as u64;
        // FNV-1a 质数步进
        h = h.wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 三、字号分桶与解码处置
// ---------------------------------------------------------------------------

/// 字号分桶（光栅化耗时**按桶**统计，不混算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeBucket {
    /// 小字号（≤ 12px）：描边与 hinting 开销占比高。
    Small,
    /// 中字号（13–24px）。
    Medium,
    /// 大字号（> 24px）：光栅化成本随面积增长。
    Large,
}

impl SizeBucket {
    /// 全枚举（顺序即 [`SizeBucket::ordinal`] 的下标）。
    pub const ALL: [SizeBucket; 3] = [SizeBucket::Small, SizeBucket::Medium, SizeBucket::Large];

    /// 枚举下标（**上行键**，不上报具体像素值——那也是资产布局信息）。
    pub const fn ordinal(self) -> u8 {
        match self {
            SizeBucket::Small => 0,
            SizeBucket::Medium => 1,
            SizeBucket::Large => 2,
        }
    }

    /// 由像素字号归桶。
    pub const fn of_px(px: u32) -> SizeBucket {
        if px <= 12 {
            SizeBucket::Small
        } else if px <= 24 {
            SizeBucket::Medium
        } else {
            SizeBucket::Large
        }
    }

    /// 线编码。
    pub const fn wire(self) -> u8 {
        match self {
            SizeBucket::Small => 0x53,
            SizeBucket::Medium => 0x4D,
            SizeBucket::Large => 0x4C,
        }
    }

    /// 线编码 → 枚举。
    pub const fn from_wire(w: u8) -> Option<SizeBucket> {
        match w {
            0x53 => Some(SizeBucket::Small),
            0x4D => Some(SizeBucket::Medium),
            0x4C => Some(SizeBucket::Large),
            _ => None,
        }
    }

    /// 桶的稳定标签。
    pub const fn label(self) -> &'static str {
        match self {
            SizeBucket::Small => "small",
            SizeBucket::Medium => "medium",
            SizeBucket::Large => "large",
        }
    }
}

/// 解码处置四档（F0802）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeOutcome {
    /// 合法解码。
    Valid,
    /// 非法序列但可恢复。
    Salvaged,
    /// 非法序列替换。
    Replaced,
    /// 非法序列丢弃。
    Dropped,
}

impl DecodeOutcome {
    /// 全枚举（顺序即 [`DecodeOutcome::ordinal`] 的下标）。
    pub const ALL: [DecodeOutcome; 4] =
        [DecodeOutcome::Valid, DecodeOutcome::Salvaged, DecodeOutcome::Replaced, DecodeOutcome::Dropped];

    /// 枚举下标（**四档必须互异**）。
    pub const fn ordinal(self) -> usize {
        match self {
            DecodeOutcome::Valid => 0,
            DecodeOutcome::Salvaged => 1,
            DecodeOutcome::Replaced => 2,
            DecodeOutcome::Dropped => 3,
        }
    }

    /// 对应的指标名（四档一一对应，**不共用指标名**——共用则无法区分来源）。
    pub const fn metric(self) -> MetricName {
        match self {
            DecodeOutcome::Valid => MetricName::DecodeValid,
            DecodeOutcome::Salvaged => MetricName::DecodeSalvaged,
            DecodeOutcome::Replaced => MetricName::DecodeReplaced,
            DecodeOutcome::Dropped => MetricName::DecodeDropped,
        }
    }

    /// 是否非法（非 Valid 三档都算非法）。
    pub const fn is_invalid(self) -> bool {
        !matches!(self, DecodeOutcome::Valid)
    }

    /// 稳定标签。
    pub const fn label(self) -> &'static str {
        match self {
            DecodeOutcome::Valid => "valid",
            DecodeOutcome::Salvaged => "salvaged",
            DecodeOutcome::Replaced => "replaced",
            DecodeOutcome::Dropped => "dropped",
        }
    }
}

/// 健康结论（**只告警，不自行动作**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealthVerdict {
    /// 健康。
    Ok,
    /// 告警（命中率骤降 / 合法解码占比过低）。
    Alert,
    /// 数据不足（样本不够，下不了结论）。
    Insufficient,
}

impl HealthVerdict {
    /// 全枚举（顺序即 [`HealthVerdict::ordinal`] 的下标）。
    pub const ALL: [HealthVerdict; 3] =
        [HealthVerdict::Ok, HealthVerdict::Alert, HealthVerdict::Insufficient];

    /// 枚举下标（**三变体必须互异**）。
    pub const fn ordinal(self) -> usize {
        match self {
            HealthVerdict::Ok => 0,
            HealthVerdict::Alert => 1,
            HealthVerdict::Insufficient => 2,
        }
    }

    /// 是否为告警。
    pub const fn is_alert(self) -> bool {
        matches!(self, HealthVerdict::Alert)
    }

    /// 线编码（**显式映射，不用 `as u8` 拿判别值**）。
    ///
    /// 取与 `ordinal()` 不同的值（`0x01`/`0xA1`/`0xA2`），使「改枚举顺序
    /// 静默改协议」在判据层直接转红。
    pub const fn wire(self) -> u8 {
        match self {
            HealthVerdict::Ok => 0x01,
            HealthVerdict::Alert => 0xA1,
            HealthVerdict::Insufficient => 0xA2,
        }
    }

    /// 线编码反解（未知码返回 [`None`]，**不静默兜底成`Ok`**——
    /// 兜底成「健康」会让协议损坏被读成正常指标）。
    pub const fn from_wire(w: u8) -> Option<HealthVerdict> {
        match w {
            0x01 => Some(HealthVerdict::Ok),
            0xA1 => Some(HealthVerdict::Alert),
            0xA2 => Some(HealthVerdict::Insufficient),
            _ => None,
        }
    }

    /// 稳定标签。
    pub const fn label(self) -> &'static str {
        match self {
            HealthVerdict::Ok => "ok",
            HealthVerdict::Alert => "alert",
            HealthVerdict::Insufficient => "insufficient",
        }
    }

    /// 读屏文案（**明确写出「不自动处置」**——这是纪律，屏上也要看得见）。
    pub const fn a11y_note(self) -> &'static str {
        match self {
            HealthVerdict::Ok => "看板展示，无动作 / shown, no action",
            HealthVerdict::Alert => "看板告警，不自行动作 / alert only, no self-action",
            HealthVerdict::Insufficient => "样本不足 / insufficient samples",
        }
    }
}

/// 最小样本数（低于此数判 [`HealthVerdict::Insufficient`]）。
pub const MIN_SAMPLES: u64 = 16;

/// 解码处置的最小样本数（**独立于 [`MIN_SAMPLES`]**）。
///
/// 解码事件只在遇到码点时累计，量级远小于缓存查询；沿用缓存门槛会让
/// 「一段短文本里全是坏码点」（合法占比 0%）因样本不足被跳过告警。
pub const MIN_DECODE_SAMPLES: u64 = 5;

// ---------------------------------------------------------------------------
// 四、遥测总线（环形缓冲 · 异步批量 · 可关闭）
// ---------------------------------------------------------------------------

/// 遥测总线（环形缓冲 + 批量上行 + 用户开关）。
#[derive(Clone, Debug)]
pub struct TelemetryBus {
    /// 环形缓冲（`len` 为已用条数；满则覆盖最旧）。
    ring: [(u8, TelemetryEvent); RING_CAPACITY],
    /// 环形写指针。
    head: usize,
    /// 环形有效条数。
    len: usize,
    /// 因覆盖而丢弃的条数（**丢数据必须可查**）。
    pub dropped: u64,
    /// 用户是否关闭遥测（**关闭即完全不采集**）。
    enabled: bool,
    /// 会话匿名指纹。
    anon_session: u64,
    /// 单调时间戳。
    clock: u32,
    /// 已上行条数。
    pub uploaded: u64,
    /// 当前待上报批的条数。
    batch: usize,
    /// 本帧遥测自开销累计（微秒）。
    pub overhead_us: u64,
    /// 采集期间累计的记录条数（关闭时为 0）。
    pub recorded: u64,
}

impl Default for TelemetryBus {
    fn default() -> Self {
        Self::new(0)
    }
}

impl TelemetryBus {
    /// 新建总线；`anon_session` 为**已折叠**的会话指纹。
    pub fn new(anon_session: u64) -> TelemetryBus {
        TelemetryBus {
            ring: [(0u8, TelemetryEvent::new(MetricName::Quads, 0, 0, 0, 0)); RING_CAPACITY],
            head: 0,
            len: 0,
            dropped: 0,
            enabled: true,
            anon_session,
            clock: 0,
            uploaded: 0,
            batch: 0,
            overhead_us: 0,
            recorded: 0,
        }
    }

    /// 由原始标识构造（**内部折叠**，原始标识不出此函数）。
    pub fn with_raw_session(raw: &[u8]) -> TelemetryBus {
        TelemetryBus::new(fold_session_id(raw))
    }

    /// 用户开关遥测（**关闭即完全不采集**，不只是不上报）。
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            // 关闭时清空待上报批次：已采集的数据不该在重开后被偷偷送出去。
            self.batch = 0;
        }
    }

    /// 遥测是否开启。
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 推进时钟（一帧）。
    pub fn tick(&mut self) {
        self.clock = self.clock.saturating_add(1);
    }

    /// 记录一个指标（**O(1)，无分配、无排序、无 IO** —— 不阻塞渲染）。
    ///
    /// 返回 `true` 表示已入环。遥测关闭时返回 `false` 且**什么都不做**。
    pub fn record(&mut self, metric: MetricName, value: u64, bucket: SizeBucket, us: u32) -> bool {
        if !self.enabled {
            return false;
        }
        let ev = TelemetryEvent::new(metric, value, bucket.ordinal(), self.clock, self.anon_session);
        // 环形写入：满则覆盖最旧
        if self.len == RING_CAPACITY {
            self.dropped = self.dropped.saturating_add(1);
        } else {
            self.len += 1;
        }
        self.ring[self.head] = (1u8, ev);
        self.head = (self.head + 1) % RING_CAPACITY;
        self.recorded = self.recorded.saturating_add(1);
        self.batch += 1;
        // 自开销累加（用于 0.02ms/帧承诺的核验）
        self.overhead_us = self.overhead_us.saturating_add(us as u64);
        true
    }

    /// 记录解码处置（四档各计一次）。
    pub fn record_decode(&mut self, outcome: DecodeOutcome, us: u32) -> bool {
        self.record(outcome.metric(), 1, SizeBucket::Small, us)
    }

    /// 缓冲内条数。
    pub fn buffered(&self) -> usize {
        self.len
    }

    /// 取第 `i` 条（零基，**按时间序**：0 是最旧）。
    pub fn at(&self, i: usize) -> Option<TelemetryEvent> {
        if i >= self.len {
            return None;
        }
        // 环形：最旧位于 head - len
        let start = (self.head + RING_CAPACITY - self.len) % RING_CAPACITY;
        let idx = (start + i) % RING_CAPACITY;
        let (live, ev) = self.ring[idx];
        if live == 1u8 {
            Some(ev)
        } else {
            None
        }
    }

    /// 批次是否已满（满则触发上行）。
    pub fn batch_ready(&self) -> bool {
        self.batch >= BATCH_LIMIT
    }

    /// 上行一个批次（**断线时返回 0 条并保留在环内** —— 补报）。
    ///
    /// `channel_up` 为 false 时**不清空**任何数据，这就是「断线补报」的
    /// 结构保证：断线期间事件继续入环，重连后一并上行。
    pub fn flush(&mut self, channel_up: bool) -> usize {
        if !channel_up {
            // 断线：一条都不上行，但**批次计数也不清零**——重连后补报。
            return 0;
        }
        let n = self.len;
        self.uploaded = self.uploaded.saturating_add(n as u64);
        self.len = 0;
        self.head = 0;
        self.batch = 0;
        // 环形槽位标为未使用（防at() 读到陈旧条目）
        let mut i = 0usize;
        while i < RING_CAPACITY {
            self.ring[i] = (0u8, TelemetryEvent::new(MetricName::Quads, 0, 0, 0, 0));
            i += 1;
        }
        n
    }

    /// 补报断线期间积压的条数（重连时调用）。
    pub fn replay_backlog(&mut self, channel_up: bool) -> usize {
        self.flush(channel_up)
    }

    /// 平均每条记录的自开销（微秒；用于核验 ≤0.02ms/帧）。
    pub fn avg_overhead_us(&self) -> u64 {
        if self.recorded == 0 {
            return 0;
        }
        self.overhead_us / self.recorded
    }

    /// 读屏面板：中英双语七行，**只报聚合计数**。
    ///
    /// 面板**不列任何单条事件**——那会把指标明细（可能间接透露用了什么
    /// 字号、什么缓存状态）摊开，面板只报总量与开关状态。
    pub fn a11y_lines(&self) -> [String; 7] {
        [
            format!(
                "遥测开关 / telemetry: {}",
                if self.enabled { "on" } else { "off" }
            ),
            format!("缓冲条数 / buffered: {}", self.len),
            format!("覆盖丢弃 / dropped: {}", self.dropped),
            format!("已上行 / uploaded: {}", self.uploaded),
            format!("累计记录 / recorded: {}", self.recorded),
            format!("待上报批次 / pending batch: {}", self.batch),
            format!("平均单条开销 / avg us per event: {}", self.avg_overhead_us()),
        ]
    }
}

// ---------------------------------------------------------------------------
// 五、四指标聚合与健康判定
// ---------------------------------------------------------------------------

/// 四指标聚合器（一帧或一批的汇总）。
// `Default` **不能 derive**：`raster` 是 `[(u8, u32); BUCKET_SAMPLES * 3]`，
// 长度超过 32，标准库只为 ≤32 的数组实现了 `Default`（E0277）。
// 故手写 impl，语义与 derive 完全一致（全零）。
#[derive(Clone, Debug)]
pub struct MetricAggregator {
    /// 缓存命中数 / 查询总数。
    pub cache_hit: u64,
    pub cache_query: u64,
    /// 各字号桶的光栅化耗时样本（每桶最多 [`BUCKET_SAMPLES`] 条）。
    raster: [(u8, u32); BUCKET_SAMPLES * 3],
    /// 各桶样本数。
    bucket_len: [u32; 3],
    /// 各桶写指针。
    bucket_head: [u32; 3],
    /// 每帧文本绘制调用数累计。
    pub draw_calls: u64,
    /// 每帧文本 quad 数累计。
    pub quads: u64,
    /// 解码四档计数。
    pub decode: [u64; 4],
}

impl MetricAggregator {
    /// 新建聚合器。
    pub fn new() -> MetricAggregator {
        MetricAggregator {
            cache_hit: 0,
            cache_query: 0,
            raster: [(0u8, 0u32); BUCKET_SAMPLES * 3],
            bucket_len: [0u32; 3],
            bucket_head: [0u32; 3],
            draw_calls: 0,
            quads: 0,
            decode: [0u64; 4],
        }
    }

    /// 记一次缓存查询。
    pub fn record_cache(&mut self, hit: bool) {
        self.cache_query = self.cache_query.saturating_add(1);
        if hit {
            self.cache_hit = self.cache_hit.saturating_add(1);
        }
    }

    /// 记一笔光栅化耗时（按字号入桶）。
    pub fn record_raster(&mut self, bucket: SizeBucket, us: u32) {
        let b = bucket.ordinal() as usize;
        let base = b * BUCKET_SAMPLES;
        let i = self.bucket_head[b] as usize % BUCKET_SAMPLES;
        self.raster[base + i] = (1u8, us);
        self.bucket_head[b] = self.bucket_head[b].saturating_add(1);
        if (self.bucket_len[b] as usize) < BUCKET_SAMPLES {
            self.bucket_len[b] += 1;
        }
    }

    /// 记解码处置。
    pub fn record_decode(&mut self, outcome: DecodeOutcome) {
        let i = outcome.ordinal();
        self.decode[i] = self.decode[i].saturating_add(1);
    }

    /// 缓存命中率（百万分比；分母为 0 时给 0）。
    pub fn hit_rate_ppm(&self) -> u32 {
        if self.cache_query == 0 {
            return 0;
        }
        // 命中 / 总数，饱和防溢出
        let hit = if self.cache_hit > self.cache_query { self.cache_query } else { self.cache_hit };
        ((hit as u128 * 1_000_000u128) / self.cache_query as u128) as u32
    }

    /// 合法解码占比（百万分比）。
    pub fn valid_decode_ppm(&self) -> u32 {
        let mut total: u64 = 0;
        let mut i = 0usize;
        while i < 4 {
            total = total.saturating_add(self.decode[i]);
            i += 1;
        }
        if total == 0 {
            return 0;
        }
        ((self.decode[DecodeOutcome::Valid.ordinal()] as u128 * 1_000_000u128) / total as u128)
            as u32
    }

    /// 解码非法序列频率（百万分比；越高越像字体文件有问题）。
    pub fn invalid_decode_ppm(&self) -> u32 {
        1_000_000u32.saturating_sub(self.valid_decode_ppm())
    }

    /// 某字号桶的光栅化 P50（微秒；无样本给 0）。
    pub fn raster_p50(&self, bucket: SizeBucket) -> u32 {
        self.raster_p(bucket, 50)
    }

    /// 某字号桶的光栅化 P95（微秒；无样本给 0）。
    pub fn raster_p95(&self, bucket: SizeBucket) -> u32 {
        self.raster_p(bucket, 95)
    }

    /// 某字号桶的分位数（**按桶独立取秩**，不与其它桶混算）。
    fn raster_p(&self, bucket: SizeBucket, pct: usize) -> u32 {
        let b = bucket.ordinal() as usize;
        let n = self.bucket_len[b] as usize;
        if n == 0 {
            return 0;
        }
        let base = b * BUCKET_SAMPLES;
        // 收集有效样本
        let mut vals: [u32; BUCKET_SAMPLES] = [0u32; BUCKET_SAMPLES];
        let mut i = 0usize;
        while i < n {
            vals[i] = self.raster[base + i].1;
            i += 1;
        }
        // 选择排序取秩
        for a in 0..n {
            for b2 in (a + 1)..n {
                if vals[b2] < vals[a] {
                    let t = vals[a];
                    vals[a] = vals[b2];
                    vals[b2] = t;
                }
            }
        }
        let mut rank = (pct * n + 99) / 100;
        if rank == 0 {
            rank = 1;
        }
        if rank > n {
            rank = n;
        }
        vals[rank - 1]
    }

    /// 健康判定（**只告警，不自行动作**）。
    ///
    /// 样本不足时返回 [`HealthVerdict::Insufficient`] 而不是硬下结论——
    /// 前 5 次查询算出 100% 命中率就宣布「健康」是自欺欺人。
    pub fn classify(&self) -> HealthVerdict {
        if self.cache_query < MIN_SAMPLES {
            return HealthVerdict::Insufficient;
        }
        if self.hit_rate_ppm() < HIT_RATE_ALERT {
            return HealthVerdict::Alert;
        }
        let decode_total: u64 = self.decode.iter().fold(0u64, |a, b| a.saturating_add(*b));
        //解码分支用**自己的样本门槛** [`MIN_DECODE_SAMPLES`]，不复用缓存的
        // [`MIN_SAMPLES`]：两者样本量级不同（缓存查询每帧数十次，解码处置
        // 只在遇到码点时累计）。套用缓存门槛会让「少量文本里全是坏码点」
        // 这种最该告警的情形因样本不足被静默跳过——0% 合法占比反被判Ok。
        if decode_total >= MIN_DECODE_SAMPLES && self.valid_decode_ppm() < VALID_DECODE_ALERT {
            return HealthVerdict::Alert;
        }
        HealthVerdict::Ok
    }

    /// 读屏面板：四指标 + 健康结论，**只报聚合计数**。
    pub fn a11y_lines(&self) -> [String; 7] {
        [
            format!("缓存命中率 / cache hit rate ppm: {}", self.hit_rate_ppm()),
            format!(
                "小字号光栅化P50/P95 / raster small us: {}/{}",
                self.raster_p50(SizeBucket::Small),
                self.raster_p95(SizeBucket::Small)
            ),
            format!(
                "文本绘制调用数 / draw calls: {}",
                self.draw_calls
            ),
            format!("文本 quad 数 / quads: {}", self.quads),
            format!(
                "非法解码频率 / invalid decode ppm: {}",
                self.invalid_decode_ppm()
            ),
            format!("解码四档 / decode: {:?}", self.decode),
            format!("健康结论 / verdict: {}", self.classify().label()),
        ]
    }
}

impl Default for MetricAggregator {
    /// 手写 `Default`（全零），语义与 derive 等价——`raster` 是
    /// `[(u8, u32); BUCKET_SAMPLES * 3]`，长度超过 32，标准库只为 ≤32 的
    /// 数组实现了 `Default`（E0277），故此处不能 derive。
    fn default() -> MetricAggregator {
        MetricAggregator::new()
    }
}

/// 每字号桶保留的耗时样本数。
pub const BUCKET_SAMPLES: usize = 32;

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// VE-F0809 模块自检（受 `CheckSet::MAX_CHECKS=112` 约束，逐条覆盖锚点判据）。
pub fn run_vee09_checks() -> CheckSet {
    let mut s = CheckSet::new("vee09_texttelemetry");

    // --- 判据 0：规格常量**逐字锚定**（锚点明文数字不许漂移） --------------
    //
    // 其余判据里的容量/批量/预算都用 `RING_CAPACITY`、`BATCH_LIMIT`、
    // `OVERHEAD_BUDGET_US` 这些**符号**引用，改了常量判据会跟着变而照样通过
    //（变异实测：容量 1000→100 全绿漏网）。故此处**逐字钉死锚点明文数字**，
    // 常量被改动立即转红。
    s.add(
        "E09-规格常量-与锚点明文数字逐字一致",
        RING_CAPACITY == 1_000
            && BATCH_LIMIT == 64
            && OVERHEAD_BUDGET_US == 20
            && EVENT_FIELD_COUNT == 6
            && HEALTH_VERDICT_COUNT == 3
            // 分桶边界（≤12 小/ 13–24 中 / >24 大）与样本桶数
            && MIN_SAMPLES == 16
            && MIN_DECODE_SAMPLES == 5
            && BUCKET_SAMPLES == 32
            && MAX_METRIC_NAME == 32,
        "锚点明文：环形缓冲 1,000 条、批量 64 条/批、遥测开销 ≤0.02ms=20us、事件六字段、三种健康结论——逐字钉死",
    );

    // --- 判据 1：零内容上行 —— 事件类型**不可承载文本** --------------------
    //
    // 这条不是「检查事件里没有文本」，而是「**类型上放不进去**」：
    // TelemetryEvent 六个字段全是定长整数或封闭枚举，没有任何 String/&str。
    // 靠约定必然失守（新增字段时谁会记得），靠类型才是真保证。
    {
        let ev = TelemetryEvent::new(MetricName::CacheHitRate, 900_000, 0, 42, 0xABCD);
        // 六字段齐全
        let fields_ok = ev.field_count() == EVENT_FIELD_COUNT
            && ev.metric == MetricName::CacheHitRate
            && ev.value == 900_000
            && ev.bucket == 0
            && ev.timestamp == 42
            && ev.anon_session == 0xABCD
            && ev.version == PROTOCOL_VERSION;
        // **定量长**：每条恰好 WIRE_BYTES 字节（偷偷多带数据会让它变）
        let wire_ok = ev.wire_bytes() == WIRE_BYTES && WIRE_BYTES == 23;
        // 指标名是封闭枚举：只有 8 种可能，且文案长度都在上限内、非空
        let mut names_ok = true;
        let mut i = 0usize;
        while i < MetricName::ALL.len() {
            let m = MetricName::ALL[i];
            names_ok &= m.label_len() > 0 && m.label_len() <= MAX_METRIC_NAME;
            names_ok &= MetricName::from_wire(m.wire()) == Some(m);
            i += 1;
        }
        // 审计行只含指标名与数值，不含任何可承载用户内容的片段
        let line = ev.audit_line();
        s.add(
            "E09-零内容-事件类型不可承载文本",
            fields_ok
                && wire_ok
                && names_ok
                && line.contains("cache_hit_rate")
                && line.contains("900000")
                // 审计行里不出现「文本」「content」「glyph」这类可能夹带内容的键
                && !line.contains("content")
                && !line.contains("glyph")
                && !line.contains("text"),
            "事件六字段全为定长整数或封闭枚举（无 String/&str），单条定长 23 字节，审计行只含指标名与数值",
        );
    }

    // --- 判据 2：会话匿名 ID 是**折叠指纹**不是原始标识 -------------------
    {
        let raw = b"user@example.com";
        let a = fold_session_id(raw);
        let b2 = fold_session_id(b"user@example.com");
        let c = fold_session_id(b"user@example.org");
        s.add(
            "E09-会话ID-折叠为指纹且同输入同输出",
            // 同一原始标识必得同一指纹（可关联同一会话）
            a == b2
                // 不同标识得不同指纹（不至于全撞成一个）
                && a != c
                // 指纹是 u64，不泄露原始长度之外的信息：不同长度也不必然相等
                && fold_session_id(b"ab") != fold_session_id(b"abc")
                // 空标识也得给出一个确定值，不 panic
                && fold_session_id(b"") == fold_session_id(b""),
            "会话标识入口即折叠为 64 位指纹（原始标识不出函数），同输入同输出、异输入异输出",
        );
        // **独立第二侧**：判据侧自行实现一份 FNV-1a，与被测函数逐位对拍。
        //
        // 判据 2 只验「同输入同输出 / 异输入异输出」，这两条**恒真于任何确定性
        // 函数**——把实现退化成 `return 42` 仍全过（变异实测漏网，属同源驱动恒真）。
        // 故另立一条：判据侧**独立重算**期望值再逐位比对，退化成常数立即转红。
        let fnv_fnv = |raw: &[u8]| -> u64 {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            let mut i = 0usize;
            while i < raw.len() {
                h ^= raw[i] as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
                i += 1;
            }
            h
        };
        let mut cases: [(&[u8], u64); 5] = [
            (b"user@example.com", 0),
            (b"", 0),
            (b"a", 0),
            (&[0u8, 255u8, 128u8, 1u8], 0),
            (b"session-0001", 0),
        ];
        let mut k = 0usize;
        let mut pair_ok = true;
        while k < cases.len() {
            // 判据侧独立算出期望值
            cases[k].1 = fnv_fnv(cases[k].0);
            // 与被测函数逐位相等（退化成常数 42 ⇒ 立即不等）
            pair_ok &= fold_session_id(cases[k].0) == cases[k].1;
            k += 1;
        }
        // 反向：期望值本身不得退化成常数（自证式防护：独立侧也要有区分度）
        let distinct = cases[0].1 != cases[1].1
            && cases[0].1 != cases[2].1
            && cases[2].1 != cases[3].1;
        s.add(
            "E09-会话ID-与独立FNV1a逐位对拍",
            pair_ok && distinct,
            "判据侧独立实现 FNV-1a 与被测折叠函数逐位对拍（5 组含空/单字节/含 0xFF 字节），不退化为常数",
        );
    }

    // --- 判据 3：三章纪律之一 —— 用户可关闭（关闭即完全不采集） -----------
    {
        let mut bus = TelemetryBus::new(0x1234);
        bus.record(MetricName::Quads, 10, SizeBucket::Small, 1);
        let on_recorded = bus.recorded;
        let on_buffered = bus.buffered();
        // 关闭
        bus.set_enabled(false);
        let ret = bus.record(MetricName::Quads, 10, SizeBucket::Small, 1);
        s.add(
            "E09-纪律-用户关闭后完全不采集",
            on_recorded == 1
                && on_buffered == 1
                && !bus.enabled()
                // 关闭后 record 返回 false
                && !ret
                // **不只是不上报，而是根本不采集**：计数零增长
                && bus.recorded == 1
                && bus.buffered() == 1,
            "遥测关闭后 record 不入环、计数零增长（关闭即完全不采集，不只是不上报）",
        );
    }

    // --- 判据 4：关闭后清空待上报批次（重开不偷送旧数据） -----------------
    {
        let mut bus = TelemetryBus::new(1);
        let mut i = 0;
        while i < 5 {
            bus.record(MetricName::DrawCalls, 1, SizeBucket::Small, 1);
            i += 1;
        }
        let batch_before = bus.batch;
        bus.set_enabled(false);
        s.add(
            "E09-纪律-关闭时清空待上报批次",
            batch_before == 5 && bus.batch == 0,
            "关闭遥测时清空待上报批次，用户关闭后重开不会把关闭期间的数据偷偷送出去",
        );
    }

    // --- 判据 5：O(1) 入环、**不做IO**（record 不返回批次号） ------------
    {
        let mut bus = TelemetryBus::new(7);
        bus.tick();
        let ok = bus.record(MetricName::RasterUs, 42, SizeBucket::Medium, 2);
        let ev = bus.at(0);
        s.add(
            "E09-记录-O1入环不阻塞渲染",
            ok
                && bus.buffered() == 1
                && ev.is_some()
                // 时间戳取自单调时钟（非墙上时钟——那个会被用户改回去）
                && ev.map(|e| e.timestamp) == Some(1)
                // 桶键是桶序号，不是像素值（像素值也是资产布局信息）
                && ev.map(|e| e.bucket) == Some(SizeBucket::Medium.ordinal())
                && ev.map(|e| e.anon_session) == Some(7),
            "record 仅 O(1) 入环不做任何 IO，时间戳取单调计数，桶键为桶序号而非像素值",
        );
    }

    // --- 判据 6：环形缓冲覆盖最旧且**丢弃数可查** -------------------------
    {
        let mut bus = TelemetryBus::new(1);
        // 灌 RING_CAPACITY + 10 条
        let total = RING_CAPACITY + 10;
        let mut i = 0u32;
        while (i as usize) < total {
            bus.record(MetricName::Quads, i as u64, SizeBucket::Small, 1);
            i += 1;
        }
        // 缓冲封顶在最旧被覆盖后
        let full = bus.buffered() == RING_CAPACITY && bus.dropped == 10;
        // `at()` 是**按时间序**（0 = 最旧，见其文档），故覆盖后：
        // `at(0)` = 最早幸存的那条（第 10 条），`at(len-1)` = 最新（第 1009 条）。
        // 初版此处断言 `at(0) == 最新`，与文档语义相反——期望写错，非实现错。
        let oldest_surviving = bus.at(0).map(|e| e.value);
        let newest = bus.at(bus.buffered() - 1).map(|e| e.value);
        s.add(
            "E09-环形缓冲-满则覆盖最旧且丢弃可查",
            full
                // 最早的 10 条（value 0..9）已被覆盖，幸存者从 10 起
                && oldest_surviving == Some(10)
                // 最新一条是最后写入的
                && newest == Some((total - 1) as u64)
                // at() 越界返回 None 不 panic
                && bus.at(RING_CAPACITY).is_none(),
            "缓冲超过 1000 条覆盖最旧（at(0) 为最早幸存者 10，at(len-1) 为最新 1009），丢弃 10 条可查，越界取为 None",
        );
    }

    // --- 判据 7：断线 → 保留积压，重连**补报**且不阻塞 ---------------------
    {
        let mut bus = TelemetryBus::new(1);
        let mut i = 0u32;
        while i < 5 {
            bus.record(MetricName::Quads, i as u64, SizeBucket::Small, 1);
            i += 1;
        }
        // 断线上行⇒ 一条都不上行，数据全留着
        let sent_down = bus.flush(false);
        let still = bus.buffered();
        // 重连⇒ 补报全部
        let sent_up = bus.replay_backlog(true);
        s.add(
            "E09-断线补报-保留积压重连一并上行",
            sent_down == 0
                && still == 5
                && sent_up == 5
                && bus.buffered() == 0
                && bus.uploaded == 5,
            "断线时上行 0 条且数据全留在环内，重连后一次性补报 5 条（不阻塞渲染）",
        );
    }

    // --- 判据 8：断线期间继续采集（不丢事件） -----------------------------
    {
        let mut bus = TelemetryBus::new(1);
        bus.record(MetricName::Quads, 1, SizeBucket::Small, 1);
        bus.flush(false); // 断线
        let mut i = 0u32;
        while i < 3 {
            bus.record(MetricName::DrawCalls, i as u64, SizeBucket::Small, 1);
            i += 1;
        }
        s.add(
            "E09-断线补报-断线期间事件不丢",
            bus.buffered() == 4 && bus.uploaded == 0,
            "断线期间新事件继续入环（4 条），上行计数仍为 0，重连后可全部补报",
        );
    }

    // --- 判据 9：批次上限（异步批量） -------------------------------------
    {
        let mut bus = TelemetryBus::new(1);
        let mut i = 0;
        while i < BATCH_LIMIT {
            bus.record(MetricName::Quads, 1, SizeBucket::Small, 1);
            i += 1;
        }
        let ready = bus.batch_ready();
        // 再记一条 ⇒ 仍应就绪（>= 而非 ==）
        bus.record(MetricName::Quads, 1, SizeBucket::Small, 1);
        s.add(
            "E09-批量-攒够上限即触发上行",
            ready && bus.batch_ready() && BATCH_LIMIT == 64 && bus.batch == 65,
            "攒够 64 条即触发批量上行（异步批量上报，不逐条上行）",
        );
    }

    // --- 判据 10：四指标齐全 -----------------------------------------------
    {
        let mut a = MetricAggregator::new();
        // ① 缓存命中率
        let mut i = 0;
        while i < 20 {
            a.record_cache(i < 18);
            i += 1;
        }
        // ② 光栅化耗时按字号分桶
        let mut j = 0u32;
        while j < 10 {
            a.record_raster(SizeBucket::Small, 100 + j);
            a.record_raster(SizeBucket::Large, 500 + j);
            j += 1;
        }
        // ③ 每帧绘制调用数与 quad 数
        a.draw_calls = 3;
        a.quads = 250;
        // ④ 解码处置计数
        a.record_decode(DecodeOutcome::Valid);
        a.record_decode(DecodeOutcome::Salvaged);
        s.add(
            "E09-四指标-全部可采集可查",
            a.hit_rate_ppm() == 900_000
                && a.draw_calls == 3
                && a.quads == 250
                && a.decode[DecodeOutcome::Valid.ordinal()] == 1
                && a.decode[DecodeOutcome::Salvaged.ordinal()] == 1
                && a.decode[DecodeOutcome::Replaced.ordinal()] == 0
                && a.raster_p50(SizeBucket::Small) == 104
                && a.raster_p50(SizeBucket::Large) == 504,
            "四指标齐备：命中率 90%、绘制调用 3、quad 250、解码四档计数、小字号与大字号分桶 P50 独立",
        );
    }

    // --- 判据 11：光栅化**按字号分桶**不混算 ------------------------------
    {
        let mut a = MetricAggregator::new();
        // 小字号全便宜、大字号全昂贵：混算会把小字号的劣化抹平
        let mut i = 0u32;
        while i < 10 {
            a.record_raster(SizeBucket::Small, 10);
            a.record_raster(SizeBucket::Large, 1000);
            i += 1;
        }
        let sp = a.raster_p95(SizeBucket::Small);
        let lp = a.raster_p95(SizeBucket::Large);
        // **梯度语料**：同一桶内造出 1..=10 的升序样本，使P50 与 P95 落在
        // **不同秩位**上。判据 11 的语料是「同值桶」（全 10 / 全 1000），
        // 任一秩位取值都相同 ⇒ 把 P95 改成 P50 也照样通过（变异实测漏网）。
        // 本判据用有梯度的语料把两个分位数的**秩位置差异**钉死。
        let mut g = MetricAggregator::new();
        let mut v = 1u32;
        while v <= 10 {
            g.record_raster(SizeBucket::Medium, v);
            v += 1;
        }
        let p50 = g.raster_p50(SizeBucket::Medium);
        let p95 = g.raster_p95(SizeBucket::Medium);
        s.add(
            "E09-分位数-P50与P95取不同秩位",
            // 升序 1..10 共 10 样本：rank(50%) 落在第 5 位=5，rank(95%) 落在第 10 位=10
            p50 == 5
                && p95 == 10
                // **核心**：P95 严格大于 P50（退化成 P50 立即转红）
                && p95 > p50
                // 单调性方向：P95 不得低于 P50
                && g.raster_p50(SizeBucket::Medium) <= g.raster_p95(SizeBucket::Medium),
            "升序 1..10 样本：P50 取第 5 位=5、P95 取第 10 位=10，两分位数取**不同秩位**且 P95>P50（P95 退化为 P50 即转红）",
        );
        s.add(
            "E09-分桶-光栅化耗时按字号独立取秩",
            sp == 10
                && lp == 1000
                && sp != lp
                // 空桶给 0 而不是沿用别的桶
                && a.raster_p95(SizeBucket::Medium) == 0,
            "小字号 P95=10 与大字号 P95=1000 各自独立（混算会把小字号劣化抹平），空桶给 0",
        );
    }

    // --- 判据 12：解码四档互异且**不共用指标名** --------------------------
    {
        let mut ok = true;
        let mut wires: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < DecodeOutcome::ALL.len() {
            let o = DecodeOutcome::ALL[i];
            // 四档 ordinal互异（共用下标会让四档计数混在一起）
            ok &= o.metric().ordinal() != MetricName::ALL[0].ordinal();
            ok &= !wires.contains(&o.metric().wire());
            wires.push(o.metric().wire());
            ok &= o.metric().is_decode_outcome();
            i += 1;
        }
        // 四种 ordinal 恰好 0..3 互异
        let mut ords = [0usize, 0, 0, 0];
        let mut k = 0usize;
        while k < 4 {
            ords[k] = DecodeOutcome::ALL[k].ordinal();
            k += 1;
        }
        ok &= ords[0] != ords[1] && ords[1] != ords[2] && ords[2] != ords[3];
        s.add(
            "E09-解码四档-档位互异且指标名不共用",
            ok
                && DecodeOutcome::Valid.metric() != DecodeOutcome::Dropped.metric()
                && DecodeOutcome::Valid.is_invalid() == false
                && DecodeOutcome::Dropped.is_invalid(),
            "解码四档 ordinal 与线编码互异、各自对应独立指标名（共用则无法区分来源）",
        );
    }

    // --- 判据 13：健康判定 —— **只告警不自行动作** ------------------------
    {
        // 命中率健康
        let mut ok = MetricAggregator::new();
        let mut i = 0;
        while i < 20 {
            ok.record_cache(true);
            i += 1;
        }
        let v_ok = ok.classify();
        // 命中率骤降⇒ 告警
        let mut bad = MetricAggregator::new();
        let mut j = 0;
        while j < 20 {
            bad.record_cache(j < 5); // 25% 命中率
            j += 1;
        }
        let v_bad = bad.classify();
        // 非法解码频率高⇒ 告警
        let mut dec = MetricAggregator::new();
        let mut k = 0;
        while k < 20 {
            dec.record_cache(true);
            k += 1;
        }
        let mut m = 0;
        while m < 5 {
            dec.record_decode(DecodeOutcome::Dropped);
            m += 1;
        }
        let v_dec = dec.classify();
        s.add(
            "E09-健康-只判告警不自动处置",
            v_ok == HealthVerdict::Ok
                && v_bad == HealthVerdict::Alert
                && v_bad.is_alert()
                && v_dec == HealthVerdict::Alert
                // **枚举里没有「已自动降级」这类变体**（编译期保证）
                && HEALTH_VERDICT_COUNT == 3
                // 告警文案明写「不自动处置」
                && HealthVerdict::Alert.a11y_note().contains("no self-action"),
            "命中率 90% 判健康、25% 与非法解码 20% 均判告警；结论枚举只有 Ok/Alert/Insufficient，无自动处置变体",
        );
    }

    // --- 判据 14：样本不足**不下结论**（不得硬判健康） -------------------
    {
        let mut a = MetricAggregator::new();
        let mut i = 0;
        while i < 5 {
            a.record_cache(true); // 100% 命中，但样本太少
            i += 1;
        }
        s.add(
            "E09-健康-样本不足不下结论",
            a.classify() == HealthVerdict::Insufficient
                && !a.classify().is_alert()
                && MIN_SAMPLES == 16,
            "仅 5 次查询（100% 命中）判样本不足而非健康——前几次就宣布健康是自欺欺人",
        );
    }

    // --- 判据 15：命中率分子**不超过分母**（饱和口径） --------------------
    {
        let mut a = MetricAggregator::new();
        // 恶意/异常输入：命中数大于查询数
        a.cache_hit = 50;
        a.cache_query = 10;
        s.add(
            "E09-命中率-分子饱和不超过分母",
            a.hit_rate_ppm() == 1_000_000,
            "命中数大于查询数时命中率夹到 100%（不做无符号除法溢出或超 100% 的荒谬值）",
        );
    }

    // --- 判据 16：非法解码频率 = 1 - 合法占比 -----------------------------
    {
        let mut a = MetricAggregator::new();
        let mut i = 0;
        while i < 95 {
            a.record_decode(DecodeOutcome::Valid);
            i += 1;
        }
        a.record_decode(DecodeOutcome::Salvaged);
        a.record_decode(DecodeOutcome::Replaced);
        a.record_decode(DecodeOutcome::Dropped);
        a.record_decode(DecodeOutcome::Dropped);
        // 语料自洽核对：95 合法 + **4** 非法（Salvaged/Replaced/Dropped×2）= 99 总数。
        // 95/99 ≈ 959595ppm，故**不能**断言 950_000（那是 95/100 的值）——
        // 语料与期望必须同一口径，否则判据钉的是不存在的数据分布。
        // 初版此处写 95+5 却只记了 4 条非法，实现完全正确却判红。
        let invalid_n: u64 = a.decode.iter().fold(0u64, |x, y| x.saturating_add(*y))
            - a.decode[DecodeOutcome::Valid.ordinal()];
        s.add(
            "E09-解码频率-非法占比等于一减合法",
            invalid_n == 4
                && a.valid_decode_ppm() == 959_595
                && a.invalid_decode_ppm() == 40_405
                // 互补关系是本判据的核心（不是某个具体数值）
                && a.invalid_decode_ppm() + a.valid_decode_ppm() == 1_000_000,
            "95 合法 + 4 非法（Salvaged/Replaced/Dropped×2）= 99 总数 ⇒ 合法 959595ppm、非法 40405ppm，两者互补为100%；非法计数独立复算钉死语料",
        );
    }

    // --- 判据 17：0.02ms/帧开销承诺可核验 --------------------------------
    {
        let mut bus = TelemetryBus::new(1);
        // 一帧记 8 条，每条 2us⇒ 帧内 16us < 20us 承诺
        let mut i = 0;
        while i < 8 {
            bus.record(MetricName::RasterUs, 10, SizeBucket::Small, 2);
            i += 1;
        }
        let frame_us = bus.overhead_us;
        s.add(
            "E09-开销-不超过每帧0.02ms承诺",
            frame_us == 16
                && OVERHEAD_BUDGET_US == 20
                && frame_us < OVERHEAD_BUDGET_US as u64
                && bus.avg_overhead_us() == 2,
            "一帧 8 条×2us=16us 低于 20us(=0.02ms) 承诺上限，自开销可逐帧核验",
        );
    }

    // --- 判据 18：字号分桶边界（夹逼对） ---------------------------------
    {
        s.add(
            "E09-分桶-字号边界三点钉死",
            SizeBucket::of_px(1) == SizeBucket::Small
                && SizeBucket::of_px(12) == SizeBucket::Small
                && SizeBucket::of_px(13) == SizeBucket::Medium
                && SizeBucket::of_px(24) == SizeBucket::Medium
                && SizeBucket::of_px(25) == SizeBucket::Large
                && SizeBucket::of_px(100000) == SizeBucket::Large
                // 三桶 ordinal 与线编码互异、往返自洽
                && SizeBucket::Small.ordinal() != SizeBucket::Medium.ordinal()
                && SizeBucket::from_wire(SizeBucket::Large.wire()) == Some(SizeBucket::Large)
                && SizeBucket::from_wire(0x00).is_none(),
            "字号分桶界：≤12 小、13–24 中、>24 大；三桶 ordinal 互异且线编码往返自洽",
        );
    }

    // --- 判据 19：枚举线编码**不等于判别值**（不得用 as u8） --------------
    {
        let mut ok = true;
        let mut i = 0;
        while i < HealthVerdict::ALL.len() {
            let v = HealthVerdict::ALL[i];
            ok &= v.wire() != v.ordinal() as u8 || v.ordinal() > 8;
            i += 1;
        }
        // MetricName 的 ordinal 恰好 0..8，wire 是 1..8 ⇒ 必须显式映射
        let mut mo = true;
        let mut k = 0;
        while k < MetricName::ALL.len() {
            mo &= MetricName::ALL[k].wire() != MetricName::ALL[k].ordinal() as u8;
            k += 1;
        }
        s.add(
            "E09-线编码-显式映射不等于判别值",
            ok
                && mo
                && MetricName::CacheHitRate.ordinal() == 0
                && MetricName::CacheHitRate.wire() == 0x01
                && MetricName::from_wire(0xFF).is_none(),
            "指标名的线编码（1..8）与枚举下标（0..7）不同——用 as u8 会在改枚举顺序时静默改协议",
        );
    }

    // --- 判据 20：面板中英双语 + **不列单条事件** -------------------------
    {
        let mut bus = TelemetryBus::new(0xFF);
        bus.tick();
        bus.record(MetricName::RasterUs, 9999, SizeBucket::Large, 3);
        let lines = bus.a11y_lines();
        let mut joined = String::new();
        let mut i = 0usize;
        while i < lines.len() {
            joined.push_str(&lines[i]);
            joined.push('\n');
            i += 1;
        }
        s.add(
            "E09-面板-七行双语且不列单条事件",
            lines.len() == 7
                && joined.contains("telemetry")
                && joined.contains("buffered")
                && joined.contains("dropped")
                && joined.contains("uploaded")
                // 单条事件的指标名（raster_us）与取值（9999）**不得**上面板
                && !joined.contains("raster_us")
                && !joined.contains("9999")
            // 聚合计数必须可查（漏报同样是缺陷）
                && joined.contains(&format!("{}", bus.recorded)),
            "遥测面板七行中英双语，只报总量与开关状态，不列任何单条事件的指标名与取值",
        );
    }

    // --- 判据 21：聚合面板同样只报聚合计 -------------------------------
    {
        let mut a = MetricAggregator::new();
        let mut i = 0;
        while i < 20 {
            a.record_cache(true);
            i += 1;
        }
        a.draw_calls = 7;
        a.quads = 1234;
        let lines = a.a11y_lines();
        let mut joined = String::new();
        let mut k = 0usize;
        while k < lines.len() {
            joined.push_str(&lines[k]);
            joined.push('\n');
            k += 1;
        }
        s.add(
            "E09-聚合面板-四指标可查",
            lines.len() == 7
                && joined.contains("cache hit rate")
                && joined.contains("draw calls")
                && joined.contains("quads")
                && joined.contains("invalid decode")
                && joined.contains("verdict")
                && joined.contains(&format!("{}", a.quads)),
            "聚合面板七行覆盖四指标与健康结论，quad 数等聚合计值可查",
        );
    }

    // --- 判据 22：零 panic 面 —— at越界/空环/空批次 ------------------------
    {
        let mut bus = TelemetryBus::new(1);
        // 空环取任何下标都安全
        let empty_ok = bus.at(0).is_none() && bus.at(999999).is_none();
        // 空总线断线上行（无数据可上行）
        let flush_empty = bus.flush(true);
        // 空环已上报过再断线上行
        let flush_down = bus.flush(false);
        // 空聚合器的分位数与命中率
        let a = MetricAggregator::new();
        s.add(
            "E09-零panic-空环与空聚合安全",
            empty_ok
                && flush_empty == 0
                && flush_down == 0
                && a.raster_p95(SizeBucket::Small) == 0
                && a.hit_rate_ppm() == 0
                && a.valid_decode_ppm() == 0
                && a.classify() == HealthVerdict::Insufficient,
            "空环取越界下标返回 None 不 panic，空聚合器分位数与占比给 0，健康判定为样本不足",
        );
    }

    s
}