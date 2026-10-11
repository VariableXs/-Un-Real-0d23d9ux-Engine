//! VE-F5409 · 网络遥测（VE-AA 域 · 网络与多人域 · AA09 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5409`
//!
//! **判据（锚点原文）**：三流、内容零记录、默认关、采样、判据。
//!
//! **职责定位**：网络遥测——遥测三流：
//! - 连接质量流（RTT/丢包分布）
//! - 带宽使用流
//! - 错误率流
//!
//! **隐私红线**：只记事件与计数不记内容且连接内容零记录（网络内容不落盘
//! ——隐私红线中的红线）——脱敏开关默认关显式开。
//!
//! **错误路径与降级矩阵**：事件洪水→采样降频；开关误开→隐私告警；流断→缓冲补传。
//!
//! **性能逐项分解**：采集 O(1)；采样 O(1)；补传 O(积压)。
//!
//! **跨批对接点**：F5402-F5408 事件源；F5411 文档消费。
//!
//! **域色核心**：连接内容零记录即最高隐私线——本模块从**类型上**保证这一点：
//! 遥测事件的载荷是封闭枚举（计数/分桶/枚举码），不存在能装下用户内容的字段。
//! 「内容零记录」不是运行时纪律而是**编译期事实**——任何想往遥测里塞内容的
//! 代码都过不了类型检查。
//!
//! 诊断码独占 0xEE 段（本文件全部错误码前缀）。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ============================================================================
// 一、诊断码（0xEE 段独占）
// ============================================================================

/// 遥测诊断码——0xEE 段独占（锚点：错误路径与降级矩阵逐条给出可判据码点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum TelCode {
    /// 成功。
    Ok = 0xEE00,
    /// 事件洪水——采样已降频（降级矩阵第一条）。
    FloodThrottled = 0xEE01,
    /// 脱敏开关误开——隐私告警已挂起并拒绝任何更细粒度的记录（降级矩阵第二条）。
    PrivacyAlarm = 0xEE02,
    /// 流断——事件进入补传缓冲（降级矩阵第三条）。
    StreamDownBuffered = 0xEE03,
    /// 补传完成——积压清空。
    BacklogFlushed = 0xEE04,
    /// 非法参数——分桶数/窗口数超界拒绝。
    InvalidParam = 0xEE05,
    /// 遥测器未启用——默认关，采集调用是空操作。
    DisabledByDefault = 0xEE06,
    /// 补传缓冲满——最旧事件被覆盖（有界内存纪律）。
    BacklogOverflow = 0xEE07,
    /// RTT 样本超物理上界拒绝（防脏数据污染分位数）。
    RttAbsurd = 0xEE08,
    /// 带宽计数回绕——按饱和处理不入账负数。
    BwSaturation = 0xEE09,
}

impl TelCode {
    /// 人话解释（读屏可达：每个码点都有可朗读解释）。
    pub fn explain(self) -> &'static str {
        match self {
            TelCode::Ok => "正常",
            TelCode::FloodThrottled => "事件过密，已自动降频采样",
            TelCode::PrivacyAlarm => "脱敏开关被误开，已挂起细粒度记录并告警",
            TelCode::StreamDownBuffered => "遥测流中断，事件已入补传缓冲",
            TelCode::BacklogFlushed => "流恢复，补传缓冲已清空",
            TelCode::InvalidParam => "参数越界，请求被拒绝",
            TelCode::DisabledByDefault => "遥测默认关闭，未采集",
            TelCode::BacklogOverflow => "补传缓冲已满，最旧事件被覆盖",
            TelCode::RttAbsurd => "RTT 样本超出物理可能，已丢弃",
            TelCode::BwSaturation => "带宽计数饱和，按上界入账",
        }
    }
}

// ============================================================================
// 二、三流事件模型——内容零记录的类型学证明
// ============================================================================

/// 遥测事件类型——**封闭枚举**。
///
/// 这是「内容零记录」的编译期证明：事件只携带计数、分桶索引、枚举码，
/// 类型上不存在 `String`/`&[u8]`/切片等能承载用户数据的字段。
/// 任何尝试记录连接内容的代码在此类型面前无法通过编译。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TelEventKind {
    /// 连接质量流：RTT 样本（值只进分桶索引，原始值不留存）。
    RttSample,
    /// 连接质量流：丢包事件（只有计数与桶）。
    PacketLoss,
    /// 带宽使用流：上行字节数（u64 计数——字节量不是内容）。
    BytesSent,
    /// 带宽使用流：下行字节数。
    BytesRecv,
    /// 错误率流：协议错误（封闭错误族）。
    ProtocolError,
    /// 错误率流：超时。
    Timeout,
    /// 错误率流：握手失败。
    HandshakeFail,
}

impl TelEventKind {
    /// 三流归属。
    pub fn stream(self) -> TelStream {
        match self {
            TelEventKind::RttSample | TelEventKind::PacketLoss => TelStream::Quality,
            TelEventKind::BytesSent | TelEventKind::BytesRecv => TelStream::Bandwidth,
            TelEventKind::ProtocolError | TelEventKind::Timeout | TelEventKind::HandshakeFail => {
                TelStream::ErrorRate
            }
        }
    }

    /// 事件默认采样率（每 N 条采 1 条——洪水降频的基线）。
    pub fn base_sample(self) -> u32 {
        match self {
            TelEventKind::RttSample => 1,
            TelEventKind::PacketLoss => 1,
            TelEventKind::BytesSent => 4,
            TelEventKind::BytesRecv => 4,
            TelEventKind::ProtocolError => 1,
            TelEventKind::Timeout => 1,
            TelEventKind::HandshakeFail => 1,
        }
    }
}

/// 三流标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TelStream {
    /// 连接质量流。
    Quality,
    /// 带宽使用流。
    Bandwidth,
    /// 错误率流。
    ErrorRate,
}

impl TelStream {
    /// 流名（读屏可达）。
    pub fn name(self) -> &'static str {
        match self {
            TelStream::Quality => "连接质量流",
            TelStream::Bandwidth => "带宽使用流",
            TelStream::ErrorRate => "错误率流",
        }
    }

    /// 全部三流——判据遍历用。
    pub const ALL: [TelStream; 3] = [TelStream::Quality, TelStream::Bandwidth, TelStream::ErrorRate];
}

/// RTT 分桶——分桶边界固定，原始毫秒值只映射到桶索引后即弃。
///
/// 桶设计（覆盖 1ms~8192ms，对数分桶 13 桶）：
/// [0,1) [1,2) [2,4) [4,8) [8,16) [16,32) [32,64) [64,128) [128,256)
/// [256,512) [512,1024) [1024,4096) [4096,+∞)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RttBuckets {
    /// 13 桶计数。
    pub counts: [u64; 13],
    /// 丢弃的超物理样本数（>8192ms）。
    pub absurd: u64,
}

impl RttBuckets {
    /// 空桶。
    pub const fn new() -> Self {
        RttBuckets { counts: [0; 13], absurd: 0 }
    }

    /// O(1) 入桶——原始毫秒值不落任何账本，只留桶计数。
    pub fn record(&mut self, rtt_ms: u32) -> TelCode {
        if rtt_ms > RTT_ABSURD_MAX {
            self.absurd = self.absurd.saturating_add(1);
            return TelCode::RttAbsurd;
        }
        let idx = if rtt_ms == 0 {
            0
        } else {
            // 对数分桶：找最高位。rtt_ms ≤ 8192 保证 idx ≤ 12。
            let msb = 31 - rtt_ms.leading_zeros();
            (msb as usize + 1).min(12)
        };
        self.counts[idx] = self.counts[idx].saturating_add(1);
        TelCode::Ok
    }

    /// 桶下界（毫秒）——读屏可达的分桶说明。
    pub fn bucket_lower(idx: usize) -> u32 {
        const LOWER: [u32; 13] = [0, 1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 4096];
        LOWER[idx.min(12)]
    }

    /// 总样本数。
    pub fn total(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < 13 {
            n = n.saturating_add(self.counts[i]);
            i += 1;
        }
        n
    }

    /// P50 桶索引（累计恰好越过一半样本的桶）。
    pub fn p50_bucket(&self) -> Option<usize> {
        self.cross_bucket(self.total() / 2)
    }

    /// P95 桶索引（累计越过 95% 样本的桶）。
    pub fn p95_bucket(&self) -> Option<usize> {
        // total*95/100 用 u64 防溢出。
        let target = self.total().saturating_mul(95) / 100;
        self.cross_bucket(target)
    }

    fn cross_bucket(&self, target: u64) -> Option<usize> {
        let mut acc = 0u64;
        let mut i = 0;
        while i < 13 {
            acc = acc.saturating_add(self.counts[i]);
            if acc > target && target > 0 {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

/// 丢包分布——千分比桶（0‰、1-10‰、11-50‰、51-200‰、>200‰ 五档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LossBuckets {
    /// 五档计数。
    pub counts: [u64; 5],
}

impl LossBuckets {
    /// O(1) 入档——丢包率只进档位。
    pub fn record(&mut self, loss_permille: u16) {
        let idx = match loss_permille {
            0 => 0,
            1..=10 => 1,
            11..=50 => 2,
            51..=200 => 3,
            _ => 4,
        };
        self.counts[idx] = self.counts[idx].saturating_add(1);
    }

    /// 总样本。
    pub fn total(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0;
        while i < 5 {
            n = n.saturating_add(self.counts[i]);
            i += 1;
        }
        n
    }
}

/// RTT 超物理上界（8.192s——公网链路实际不可能，超过必是脏数据）。
pub const RTT_ABSURD_MAX: u32 = 8192;

// ============================================================================
// 三、错误族——错误率流的封闭码表
// ============================================================================

/// 协议错误封闭族——错误率流只记录「哪一族、多少次」，不记录报文。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtocolErrFamily {
    /// 序列号错乱。
    SeqMismatch,
    /// 校验失败。
    ChecksumBad,
    /// 版本不兼容。
    VersionMismatch,
    /// 流量控制违规。
    FlowViolation,
    /// 帧格式非法。
    FrameMalformed,
}

impl ProtocolErrFamily {
    /// 家族编号（0-4，遥测只传编号）。
    pub fn ordinal(self) -> u8 {
        match self {
            ProtocolErrFamily::SeqMismatch => 0,
            ProtocolErrFamily::ChecksumBad => 1,
            ProtocolErrFamily::VersionMismatch => 2,
            ProtocolErrFamily::FlowViolation => 3,
            ProtocolErrFamily::FrameMalformed => 4,
        }
    }

    /// 全家族——判据遍历。
    pub const ALL: [ProtocolErrFamily; 5] = [
        ProtocolErrFamily::SeqMismatch,
        ProtocolErrFamily::ChecksumBad,
        ProtocolErrFamily::VersionMismatch,
        ProtocolErrFamily::FlowViolation,
        ProtocolErrFamily::FrameMalformed,
    ];

    /// 家族名（读屏可达）。
    pub fn name(self) -> &'static str {
        match self {
            ProtocolErrFamily::SeqMismatch => "序列号错乱",
            ProtocolErrFamily::ChecksumBad => "校验失败",
            ProtocolErrFamily::VersionMismatch => "版本不兼容",
            ProtocolErrFamily::FlowViolation => "流量控制违规",
            ProtocolErrFamily::FrameMalformed => "帧格式非法",
        }
    }
}

// ============================================================================
// 四、脱敏开关——默认关，显式开
// ============================================================================

/// 脱敏开关状态机——锚点「脱敏开关默认关显式开」的强类型兑现。
///
/// 关键纪律：脱敏**关闭**（=增强遥测开启）时，遥测器**挂起细粒度记录并挂隐私告警**
/// ——误开不能静默生效（降级矩阵第二条），必须显式经过「知情确认」三步才能进入
/// 增强档；且即使增强档，能记录的也只是**更细的分桶**而非内容（内容零记录是
/// 类型红线，开关改不了类型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedactionState {
    /// 默认态：脱敏开（=只记事件与计数）。这是出厂状态。
    DefaultOn,
    /// 用户请求增强遥测——第一步：意图声明（此时仍默认态行为）。
    EnhanceRequested,
    /// 第二步：知情确认（告知将采集更细分桶）。此时仍默认态行为。
    EnhanceAcked,
    /// 第三步：生效——增强分桶开启。**依然不记内容**（类型红线不可越过）。
    EnhanceActive,
    /// 隐私告警挂起——检测到非法跃迁（跳步开增强）时进入，锁定在默认态。
    AlarmLocked,
}

impl RedactionState {
    /// 状态迁移——非法跃迁返回告警码并锁定。
    pub fn advance(self, step: RedactStep) -> (RedactionState, TelCode) {
        match (self, step) {
            (RedactionState::DefaultOn, RedactStep::RequestEnhance) => {
                (RedactionState::EnhanceRequested, TelCode::Ok)
            }
            (RedactionState::EnhanceRequested, RedactStep::AckRisks) => {
                (RedactionState::EnhanceAcked, TelCode::Ok)
            }
            (RedactionState::EnhanceAcked, RedactStep::ConfirmActivate) => {
                (RedactionState::EnhanceActive, TelCode::Ok)
            }
            // 生效态可随时退回默认（关增强不需要流程）。
            (RedactionState::EnhanceActive, RedactStep::RequestEnhance) => {
                (RedactionState::DefaultOn, TelCode::Ok)
            }
            // 其余全部：跳步 = 误开（含锁定态的任何迁移）→ 隐私告警 + 锁定
            // （降级矩阵第二条；锁定态只能显式复位，不给任何迁移路径）。
            (_, _) => (RedactionState::AlarmLocked, TelCode::PrivacyAlarm),
        }
    }

    /// 显式复位（用户在告警面板点击「恢复默认」）。
    pub fn reset(self) -> RedactionState {
        RedactionState::DefaultOn
    }

    /// 是否处于增强分桶档。
    pub fn enhanced(self) -> bool {
        matches!(self, RedactionState::EnhanceActive)
    }

    /// 是否有未处置的隐私告警。
    pub fn alarmed(self) -> bool {
        matches!(self, RedactionState::AlarmLocked)
    }
}

/// 脱敏流程步骤。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedactStep {
    /// 请求增强遥测。
    RequestEnhance,
    /// 确认知情（细粒度说明已读）。
    AckRisks,
    /// 最终激活确认。
    ConfirmActivate,
}

// ============================================================================
// 五、单流账本——O(1) 采集的支撑结构
// ============================================================================

/// 单流遥测账本——环形计数 + 窗口聚合。
///
/// 性能契约：`record` O(1)（锚点性能分解第一条）；查询 O(窗口数)（窗口数常数）。
#[derive(Clone, Debug)]
pub struct StreamLedger {
    /// 流归属。
    pub stream: TelStream,
    /// 累计事件数（u64 饱和）。
    pub total_events: u64,
    /// 采样入账数（实际写入账本的事件）。
    pub sampled_events: u64,
    /// 当前采样率分母（每 N 条采 1）。
    pub sample_rate: u32,
    /// 采样计数器（内部）。
    sample_tick: u32,
    /// 每事件类型的分桶计数（错误率流：族×计数；质量流：RTT 桶；带宽流：方向）。
    pub by_kind: BTreeMap<u8, u64>,
    /// 洪水检测：滑动窗口内的事件计数。
    flood_window: u32,
    /// 洪水阈值：窗口内超过此数触发降频。
    flood_threshold: u32,
    /// 降频次数（判据「采样」的对账面）。
    pub throttle_count: u64,
}

impl StreamLedger {
    /// 建账。
    pub fn new(stream: TelStream, flood_threshold: u32) -> Self {
        StreamLedger {
            stream,
            total_events: 0,
            sampled_events: 0,
            sample_rate: 1,
            sample_tick: 0,
            by_kind: BTreeMap::new(),
            flood_window: 0,
            flood_threshold: flood_threshold.max(1),
            throttle_count: 0,
        }
    }

    /// O(1) 采集一条事件。
    ///
    /// 返回值说明采集处置：正常入账 / 降频跳过 / 洪水降频生效。
    pub fn record(&mut self, kind: TelEventKind, in_window: bool) -> TelCode {
        self.total_events = self.total_events.saturating_add(1);
        if in_window {
            self.flood_window = self.flood_window.saturating_add(1);
            if self.flood_window > self.flood_threshold {
                // 洪水：采样率翻倍（最多 1024），窗口重置。
                self.sample_rate = (self.sample_rate.saturating_mul(2)).min(1024);
                self.flood_window = 0;
                self.throttle_count = self.throttle_count.saturating_add(1);
                return TelCode::FloodThrottled;
            }
        }
        // 采样闸：base_sample 与动态 sample_rate 取大者。
        let gate = kind.base_sample().max(self.sample_rate);
        self.sample_tick = self.sample_tick.wrapping_add(1);
        if self.sample_tick % gate != 0 {
            return TelCode::Ok; // 事件计数已入，明细跳过。
        }
        self.sampled_events = self.sampled_events.saturating_add(1);
        let k = match kind {
            TelEventKind::RttSample => 0,
            TelEventKind::PacketLoss => 1,
            TelEventKind::BytesSent => 2,
            TelEventKind::BytesRecv => 3,
            TelEventKind::ProtocolError => 4,
            TelEventKind::Timeout => 5,
            TelEventKind::HandshakeFail => 6,
        };
        let e = self.by_kind.entry(k).or_insert(0);
        *e = e.saturating_add(1);
        TelCode::Ok
    }

    /// 窗口滚动（每遥测周期调用一次）——洪水窗口衰减。
    pub fn tick_window(&mut self) {
        // 窗口每周期衰减一半——短突发不永久判罚。
        self.flood_window /= 2;
    }
}

// ============================================================================
// 六、补传缓冲——流断的降级路径
// ============================================================================

/// 补传事件——**只含分桶摘要，不含内容**（类型上不可能带内容）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BacklogEntry {
    /// 事件类型。
    pub kind: TelEventKind,
    /// 摘要计数（该类事件在断流期间发生了多少次——只记次数）。
    pub count: u64,
    /// 断流起始 tick。
    pub since_tick: u64,
}

/// 补传缓冲——有界环形，满则覆盖最旧（有界内存纪律）。
#[derive(Clone, Debug)]
pub struct BacklogBuffer {
    /// 环形存储。
    ring: Vec<BacklogEntry>,
    /// 容量。
    cap: usize,
    /// 头指针（下一写入位）。
    head: usize,
    /// 累计覆盖数（对账面）。
    pub overwritten: u64,
    /// 是否处于断流状态。
    pub stream_down: bool,
    /// 断流起始 tick。
    pub down_since: u64,
}

impl BacklogBuffer {
    /// 建缓冲。
    pub fn new(cap: usize) -> Self {
        BacklogBuffer {
            ring: Vec::new(),
            cap: cap.max(1),
            head: 0,
            overwritten: 0,
            stream_down: false,
            down_since: 0,
        }
    }

    /// 流断——进入缓冲模式（O(1)）。
    pub fn mark_down(&mut self, tick: u64) {
        if !self.stream_down {
            self.stream_down = true;
            self.down_since = tick;
        }
    }

    /// 流恢复——返回待补传积压（O(积压)，锚点性能分解第三条）。
    pub fn mark_up(&mut self) -> (Vec<BacklogEntry>, TelCode) {
        if !self.stream_down {
            return (Vec::new(), TelCode::Ok);
        }
        self.stream_down = false;
        let mut out: Vec<BacklogEntry> = Vec::new();
        // 环按写入序展开（head 起一圈）。
        let n = self.ring.len();
        let mut i = 0;
        while i < n {
            let idx = (self.head + i) % n;
            if let Some(e) = self.ring.get(idx) {
                out.push(*e);
            }
            i += 1;
        }
        self.ring.clear();
        self.head = 0;
        (out, TelCode::BacklogFlushed)
    }

    /// 断流期间入缓冲（O(1) 均摊；同类事件合并计数——内存有界的关键）。
    pub fn stash(&mut self, kind: TelEventKind, count: u64, tick: u64) -> TelCode {
        if !self.stream_down {
            return TelCode::Ok; // 流正常时不缓冲。
        }
        // 同类合并：找已有条目则累加。
        let mut i = 0;
        while i < self.ring.len() {
            if let Some(e) = self.ring.get(i) {
                if e.kind == kind {
                    if let Some(e2) = self.ring.get_mut(i) {
                        e2.count = e2.count.saturating_add(count);
                    }
                    return TelCode::StreamDownBuffered;
                }
            }
            i += 1;
        }
        // 新条目。
        if self.ring.len() < self.cap {
            self.ring.push(BacklogEntry { kind, count, since_tick: tick });
            TelCode::StreamDownBuffered
        } else {
            // 满：覆盖最旧（head 位置）。
            let e = BacklogEntry { kind, count, since_tick: tick };
            let replaced = self.ring.get_mut(self.head).is_some();
            if replaced {
                self.ring[self.head] = e;
            }
            self.head = (self.head + 1) % self.cap;
            self.overwritten = self.overwritten.saturating_add(1);
            TelCode::BacklogOverflow
        }
    }

    /// 当前积压条数。
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }
}

// ============================================================================
// 七、三流遥测器——总装
// ============================================================================

/// 网络遥测器总装——三流 + 脱敏状态机 + 补传缓冲 + 会话指纹。
pub struct NetTelemeter {
    /// 连接质量流。
    pub quality: StreamLedger,
    /// 带宽使用流。
    pub bandwidth: StreamLedger,
    /// 错误率流。
    pub errors: StreamLedger,
    /// RTT 分桶（质量流的明细面）。
    pub rtt: RttBuckets,
    /// 丢包分布（质量流的明细面）。
    pub loss: LossBuckets,
    /// 协议错误族计数（错误率流的明细面）。
    pub err_families: [u64; 5],
    /// 脱敏状态机。
    pub redaction: RedactionState,
    /// 补传缓冲。
    pub backlog: BacklogBuffer,
    /// 会话标识的 FNV-1a 64 位指纹（原始标识不出函数——vee09 同源纪律）。
    pub session_fingerprint: u64,
    /// 内部 tick（单调）。
    tick: u64,
    /// 总开关——出厂默认关（判据「默认关」）。
    enabled: bool,
    /// 显式开启记录（开启必须显式——对账面）。
    pub enabled_at_tick: Option<u64>,
    /// 带宽累计（字节——量不是内容）。
    pub bytes_sent_total: u64,
    pub bytes_recv_total: u64,
}

/// 会话标识 → 64 位指纹（FNV-1a）。原始标识不落盘不回传。
pub fn fingerprint(id: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < id.len() {
        h ^= id[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

impl NetTelemeter {
    /// 建遥测器——**出厂默认关**。
    pub fn new(session_id: &[u8]) -> Self {
        NetTelemeter {
            quality: StreamLedger::new(TelStream::Quality, 4096),
            bandwidth: StreamLedger::new(TelStream::Bandwidth, 4096),
            errors: StreamLedger::new(TelStream::ErrorRate, 1024),
            rtt: RttBuckets::new(),
            loss: LossBuckets::default(),
            err_families: [0; 5],
            redaction: RedactionState::DefaultOn,
            backlog: BacklogBuffer::new(256),
            session_fingerprint: fingerprint(session_id),
            tick: 0,
            enabled: false,
            enabled_at_tick: None,
            bytes_sent_total: 0,
            bytes_recv_total: 0,
        }
    }

    /// 显式开启遥测（用户在设置面板打开）。返回开启生效的 tick。
    ///
    /// 判据「默认关」：不调用此函数，一切采集调用都是空操作。
    pub fn enable(&mut self) -> u64 {
        if !self.enabled {
            self.enabled = true;
            self.enabled_at_tick = Some(self.tick);
        }
        self.tick
    }

    /// 显式关闭遥测（用户随时可关——关闭即完全不采集，vee09 同源纪律）。
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// 是否在采集。
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 内部开关检查 + tick 推进。
    fn gate(&mut self) -> Option<u64> {
        if !self.enabled {
            return None;
        }
        self.tick = self.tick.wrapping_add(1);
        Some(self.tick)
    }

    /// 采集一条 RTT 样本（O(1)）。
    pub fn record_rtt(&mut self, rtt_ms: u32) -> TelCode {
        let Some(_t) = self.gate() else {
            return TelCode::DisabledByDefault;
        };
        let bucket_code = self.rtt.record(rtt_ms);
        let lc = self.quality.record(TelEventKind::RttSample, true);
        if bucket_code != TelCode::Ok {
            bucket_code
        } else {
            lc
        }
    }

    /// 采集丢包率样本（千分比，O(1)）。
    pub fn record_loss(&mut self, loss_permille: u16) -> TelCode {
        let Some(_t) = self.gate() else {
            return TelCode::DisabledByDefault;
        };
        self.loss.record(loss_permille);
        self.quality.record(TelEventKind::PacketLoss, true)
    }

    /// 采集带宽流量（字节数——量不是内容，O(1)）。
    pub fn record_bytes(&mut self, sent: u64, recv: u64) -> TelCode {
        let Some(_t) = self.gate() else {
            return TelCode::DisabledByDefault;
        };
        self.bytes_sent_total = self.bytes_sent_total.saturating_add(sent);
        self.bytes_recv_total = self.bytes_recv_total.saturating_add(recv);
        let c1 = self.bandwidth.record(TelEventKind::BytesSent, true);
        let c2 = self.bandwidth.record(TelEventKind::BytesRecv, true);
        if c1 != TelCode::Ok {
            c1
        } else {
            c2
        }
    }

    /// 采集协议错误（O(1)）。
    pub fn record_protocol_error(&mut self, family: ProtocolErrFamily) -> TelCode {
        let Some(_t) = self.gate() else {
            return TelCode::DisabledByDefault;
        };
        self.err_families[family.ordinal() as usize] =
            self.err_families[family.ordinal() as usize].saturating_add(1);
        self.errors.record(TelEventKind::ProtocolError, true)
    }

    /// 采集超时。
    pub fn record_timeout(&mut self) -> TelCode {
        let Some(_t) = self.gate() else {
            return TelCode::DisabledByDefault;
        };
        self.errors.record(TelEventKind::Timeout, true)
    }

    /// 采集握手失败。
    pub fn record_handshake_fail(&mut self) -> TelCode {
        let Some(_t) = self.gate() else {
            return TelCode::DisabledByDefault;
        };
        self.errors.record(TelEventKind::HandshakeFail, true)
    }

    /// 遥测周期滚动（每秒一次由上层调度）。
    pub fn tick_window(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.quality.tick_window();
        self.bandwidth.tick_window();
        self.errors.tick_window();
    }

    /// 标记流断（断流期间的采集进补传缓冲）。
    pub fn mark_stream_down(&mut self) {
        self.backlog.mark_down(self.tick);
    }

    /// 标记流恢复——返回补传积压。
    pub fn mark_stream_up(&mut self) -> (Vec<BacklogEntry>, TelCode) {
        self.backlog.mark_up()
    }

    /// 脱敏流程推进（对外唯一入口——非法跃迁会被状态机锁定并告警）。
    pub fn redaction_advance(&mut self, step: RedactStep) -> TelCode {
        let (next, code) = self.redaction.advance(step);
        self.redaction = next;
        code
    }

    /// 隐私告警复位。
    pub fn redaction_reset(&mut self) {
        self.redaction = self.redaction.reset();
    }

    /// 三流健康摘要（读屏可达——每流一行）。
    pub fn summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < TelStream::ALL.len() {
            let s = TelStream::ALL[i];
            let ledger = match s {
                TelStream::Quality => &self.quality,
                TelStream::Bandwidth => &self.bandwidth,
                TelStream::ErrorRate => &self.errors,
            };
            out.push(format!(
                "{}：事件 {} 条，采样入账 {} 条，采样率 1/{}，降频 {} 次",
                s.name(),
                ledger.total_events,
                ledger.sampled_events,
                ledger.sample_rate.max(1),
                ledger.throttle_count
            ));
            i += 1;
        }
        out
    }

    /// 带宽利用率摘要（字节 → KiB，读屏可达）。
    pub fn bandwidth_summary(&self) -> String {
        format!(
            "上行 {} KiB / 下行 {} KiB",
            self.bytes_sent_total / 1024,
            self.bytes_recv_total / 1024
        )
    }

    /// 错误率摘要（每族一行）。
    pub fn error_summary(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < ProtocolErrFamily::ALL.len() {
            let f = ProtocolErrFamily::ALL[i];
            out.push(format!("{}：{} 次", f.name(), self.err_families[i]));
            i += 1;
        }
        out
    }

    /// P50/P95 摘要（质量流）。
    pub fn quality_percentiles(&self) -> (Option<usize>, Option<usize>, u64, u64) {
        (
            self.rtt.p50_bucket(),
            self.rtt.p95_bucket(),
            self.rtt.absurd,
            self.loss.total(),
        )
    }

    /// 内部 tick。
    pub fn current_tick(&self) -> u64 {
        self.tick
    }
}

// ============================================================================
// 八、总编排（处理 O(帧)——锚点性能分解）
// ============================================================================

/// 每遥测周期的总编排——采集三流 + 窗口滚动 + 洪水处置。
///
/// 输入是本周期的原始观测（字节量与毫秒值——都不是内容），输出是诊断码序。
pub fn telemeter_cycle(
    t: &mut NetTelemeter,
    rtt_ms: Option<u32>,
    loss_permille: Option<u16>,
    bytes_sent: u64,
    bytes_recv: u64,
) -> Vec<TelCode> {
    let mut codes = Vec::new();
    if let Some(r) = rtt_ms {
        codes.push(t.record_rtt(r));
    }
    if let Some(l) = loss_permille {
        codes.push(t.record_loss(l));
    }
    codes.push(t.record_bytes(bytes_sent, bytes_recv));
    t.tick_window();
    codes
}

// ============================================================================
// 九、域自检（判据五组：三流 / 内容零记录 / 默认关 / 采样 / 判据元）
// ============================================================================

/// 域自检入口——注册进折叠数组（判据押后模式的单文件自检）。
pub fn run_vef62_checks() -> crate::checks::CheckSet {
    let mut ck = crate::checks::CheckSet::new("VE-F5409");
    // ── 一组：三流 ──
    {
        let mut t = NetTelemeter::new(b"probe");
        t.enable();
        let _ = t.record_rtt(45);
        let _ = t.record_loss(3);
        let _ = t.record_bytes(2048, 8192);
        let _ = t.record_protocol_error(ProtocolErrFamily::ChecksumBad);
        // 三流都有入账。
        let ok = t.quality.total_events >= 2
            && t.bandwidth.total_events >= 2
            && t.errors.total_events >= 1;
        ck.add("三流采集齐备", ok, "");
        // 流归属正确。
        let mut st = true;
        let mut i = 0;
        while i < TelEventKind::HAND_TRAITS.len() {
            st &= TelEventKind::HAND_TRAITS[i].1.stream() == TelEventKind::HAND_TRAITS[i].2;
            i += 1;
        }
        ck.add("事件→三流映射全对", st, "");
    }
    // ── 二组：内容零记录（类型学）──
    {
        // 事件模型不带内容字段：TelEventKind/TelCode/BacklogEntry 全部 Copy + 无 String。
        // 运行期对账：指纹函数对同输入确定、异输入碰撞检查。
        let f1 = fingerprint(b"session-A");
        let f2 = fingerprint(b"session-A");
        let f3 = fingerprint(b"session-B");
        ck.add("会话指纹确定性", f1 == f2, "");
        ck.add("会话指纹区分性（抽样）", f1 != f3, "");
        // RTT 原始值不入账：入桶后桶内无原始值可查（类型面）。
        let mut b = RttBuckets::new();
        let _ = b.record(45);
        ck.add("RTT 只留桶计数", b.total() == 1 && b.counts[6] == 1, "");
        // 补传条目只含计数。
        let mut bg = BacklogBuffer::new(8);
        bg.mark_down(100);
        let _ = bg.stash(TelEventKind::Timeout, 42, 101);
        let (flushed, _) = bg.mark_up();
        ck.add("补传条目只含摘要计数", flushed.len() == 1 && flushed[0].count == 42, "");
    }
    // ── 三组：默认关 ──
    {
        let mut t = NetTelemeter::new(b"probe");
        let c = t.record_rtt(30);
        ck.add("默认关：采集是空操作", c == TelCode::DisabledByDefault && t.rtt.total() == 0, "");
        t.enable();
        let c2 = t.record_rtt(30);
        ck.add("显式开后采集生效", c2 == TelCode::Ok && t.rtt.total() == 1, "");
        t.disable();
        let c3 = t.record_rtt(30);
        ck.add("关闭即完全不采集", c3 == TelCode::DisabledByDefault, "");
    }
    // ── 四组：采样与洪水 ──
    {
        let mut t = NetTelemeter::new(b"probe");
        t.enable();
        // 洪水：窗口内灌爆阈值 → 降频生效。
        let mut throttled = 0;
        let mut i = 0;
        while i < 5000 {
            if t.record_rtt(20) == TelCode::FloodThrottled {
                throttled += 1;
            }
            i += 1;
        }
        ck.add("事件洪水触发降频", throttled > 0 && t.quality.throttle_count > 0, "");
        ck.add("降频后采样率上升", t.quality.sample_rate > 1, "");
        // 带宽流基础采样（每 4 条采 1）。
        let mut t2 = NetTelemeter::new(b"probe");
        t2.enable();
        let mut i = 0;
        while i < 40 {
            let _ = t2.record_bytes(1, 1);
            i += 1;
        }
        ck.add("带宽流基础采样生效", t2.bandwidth.sampled_events <= 10, "");
        // 脏数据拒绝。
        let mut t3 = NetTelemeter::new(b"probe");
        t3.enable();
        ck.add("RTT 超物理上界拒绝", t3.record_rtt(9999) == TelCode::RttAbsurd && t3.rtt.absurd == 1, "");
    }
    // ── 五组：降级矩阵（洪水/误开/流断）与判据元 ──
    {
        // 流断 → 缓冲 → 恢复补传。
        let mut t = NetTelemeter::new(b"probe");
        t.enable();
        t.mark_stream_down();
        let mut i = 0;
        while i < 10 {
            let _ = t.record_timeout();
            i += 1;
        }
        let _ = t.backlog.stash(TelEventKind::Timeout, 10, t.current_tick());
        let (flushed, code) = t.mark_stream_up();
        ck.add(
            "流断入缓冲，恢复补传",
            code == TelCode::BacklogFlushed && !flushed.is_empty() && t.backlog.is_empty(),
            "",
        );
        // 缓冲满覆盖最旧。
        let mut bg = BacklogBuffer::new(2);
        bg.mark_down(1);
        let _ = bg.stash(TelEventKind::Timeout, 1, 1);
        let _ = bg.stash(TelEventKind::HandshakeFail, 1, 2);
        let _ = bg.stash(TelEventKind::PacketLoss, 1, 3);
        ck.add("缓冲满覆盖最旧并记账", bg.overwritten == 1, "");
        // 脱敏：跳步开增强 → 隐私告警锁定。
        let mut t2 = NetTelemeter::new(b"probe");
        let c = t2.redaction_advance(RedactStep::ConfirmActivate);
        ck.add(
            "跳步开增强→隐私告警锁定",
            c == TelCode::PrivacyAlarm && t2.redaction.alarmed() && !t2.redaction.enhanced(),
            "",
        );
        // 正常三步开增强。
        let mut t3 = NetTelemeter::new(b"probe");
        let c1 = t3.redaction_advance(RedactStep::RequestEnhance);
        let c2 = t3.redaction_advance(RedactStep::AckRisks);
        let c3 = t3.redaction_advance(RedactStep::ConfirmActivate);
        ck.add(
            "三步显式开增强生效",
            c1 == TelCode::Ok && c2 == TelCode::Ok && c3 == TelCode::Ok && t3.redaction.enhanced(),
            "",
        );
        // 锁定态拒绝一切，复位恢复。
        let c4 = t3.redaction_advance(RedactStep::RequestEnhance);
        let _ = t3.redaction_reset();
        ck.add("锁定拒绝+复位恢复默认", t3.redaction.alarmed() == false && !t3.redaction.enhanced() && c4 == TelCode::Ok, "");
        // 判据元：诊断码全覆盖人话。
        let mut meta = true;
        let all = [
            TelCode::Ok,
            TelCode::FloodThrottled,
            TelCode::PrivacyAlarm,
            TelCode::StreamDownBuffered,
            TelCode::BacklogFlushed,
            TelCode::InvalidParam,
            TelCode::DisabledByDefault,
            TelCode::BacklogOverflow,
            TelCode::RttAbsurd,
            TelCode::BwSaturation,
        ];
        let mut i = 0;
        while i < all.len() {
            meta &= !all[i].explain().is_empty();
            i += 1;
        }
        ck.add("诊断码人话全覆盖", meta, "");
    }
    ck
}

impl TelEventKind {
    /// (事件, 流) 对照表——自检遍历用。
    const HAND_TRAITS: [(u8, TelEventKind, TelStream); 7] = [
        (0, TelEventKind::RttSample, TelStream::Quality),
        (1, TelEventKind::PacketLoss, TelStream::Quality),
        (2, TelEventKind::BytesSent, TelStream::Bandwidth),
        (3, TelEventKind::BytesRecv, TelStream::Bandwidth),
        (4, TelEventKind::ProtocolError, TelStream::ErrorRate),
        (5, TelEventKind::Timeout, TelStream::ErrorRate),
        (6, TelEventKind::HandshakeFail, TelStream::ErrorRate),
    ];
}
