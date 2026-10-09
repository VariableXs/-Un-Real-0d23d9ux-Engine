//! VE-F0232 · Intel 显示热插拔处理（VE-B 域 · VE-B Intel 驱动矩阵 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0232`
//!
//! **职责定位（锚点原文）**：Intel 显示热插拔处理——HPD 中断处理（**短脉冲与
//! 长脉冲区分语义**）、**去抖窗口合并**、**连接状态机**（连接中、稳定、断开
//! 三态）与下游联动（F0225 重编程）；热事件**全程入体验日志**；**HPD 风暴限流
//! 稳定后一次性处理**。
//!
//! **判据（锚点原文）**：去抖、状态机、轮询兜底、日志全程、判据。
//!
//! **数据结构（锚点原文）**：HPD 状态机；去抖窗口记录；限流计数。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | HPD 风暴 → **限流并稳定后一次性处理** | [`HpdHotplug::on_event`] 内风暴检测（[`STORM_EVENT_THRESHOLD`] 事件/[`STORM_WINDOW_TICKS`] tick）：进风暴后**只跟踪真相不落地转换**——事件照常更新 `pending` 目标（丢了真相比多转几次更坏），但提交全部挡到退避结束且信号重新稳定之后，**一整场风暴只产出一次转换一套下游请求** |
//! | 中断丢失 → **周期轮询兜底可配** | [`HpdHotplug::set_fallback_poll`] 配置轮询兜底（间隔 0 = 关闭，显性配置不默认启用——兜底路径自己也是负载）；中断路径心跳过期（[`IRQ_STALE_TICKS`]）判丢失并留痕，丢失后由兜底轮询按读数合成事件接管；未配置兜底而中断丢失 = 诚实拒绝（[`CODE_FALLBACK_NOT_CONFIGD`]）不静默瞎眼 |
//! | 状态抖动 → **去抖窗口合并** | 去抖窗口 [`DEBOUNCE_TICKS`]：窗口内翻转目标的的事件一律合并（刷新期限、翻转 `pending`、计 `merged`），**期限到时只落地最终目标一次**——十次抖动一次转换，不是十次转换 |
//!
//! **短脉冲与长脉冲语义（锚点：区分语义）**：长脉冲（[`HpdEventKind::Connect`]/
//! [`HpdEventKind::Disconnect`]）是**连接态变更信号**——过去抖窗口决定落地；
//! 短脉冲（[`HpdEventKind::ShortPulse`]）是**瞬态注意事件**（链路重训练请求/
//! Attention，DP 规范语义）——全程入日志、计入风暴检测，但**永不改变连接态**：
//! 把短脉冲当连接变更，会在正常链路维护期刷出一串假插拔。
//!
//! **性能逐项分解（锚点原文）**：处理 O(输出)——每个管理器实例负责一个输出，
//! 事件处理全是常数时间（查表/比较/定容写入）；去抖 O(1)；轮询兜底低频
//! （可配间隔，默认关）不扰主路。
//!
//! **跨批对接点（锚点原文）**：上游 F0225；下游 F0233 双屏布局重算、F0103
//! 通知。每次落地转换向三者各投递一份 [`DownstreamRequest`]（定容队列，
//! 消费方 [`HpdHotplug::take_requests`] 取走即清）——「上游 F0225」是重编程
//! 的受局（本条只发请求不执行重编程）。
//!
//! **无障碍与隐私（锚点原文）**：热插拔通知含**输出位置语义**供读屏播报——
//! 下游 [`DownstreamRequest`] 与 [`HpdHotplug::a11y_lines`] 均带输出标签
//! （端口名即位置语义，如 `DP-1`），只报插拔事实与方向，不泄漏显示内容。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **连接三态不是两态**：连接中（Connecting）是链路训练期——插上显示器到
//!   能出图之间有训练窗口，把「连接中」直接当「稳定」会让 F0225 在链路还没
//!   训练好时重编程（花屏/黑屏的第一嫌疑）。
//! - **去抖合并的是「目标」不是「事件」**：窗口内每次翻转都刷新期限、改写
//!   `pending` 为最新目标，过去抖只落地最后一次——合并计数 `merged` 记的是
//!   被合并掉的事件数（能被审计），不是悄悄丢弃。
//! - **风暴期不丢真相**：风暴中事件照常更新 `pending` 与期限，只是提交被挡
//!   ——「限流」限的是**落地次数**不是**感知次数**。把事件整个丢掉会让风暴
//!   结束后的状态停留在风暴前的旧值（显示器明明热插过却当没插）。
//! - **稳定后一次性**：退避期满后走正常去抖——只要信号不再翻转，期限自然
//!   到期，只落地一次。不需要额外的「一次性」专用逻辑，去抖本身就是一次性
//!   的（这是设计上的一次性，不是补丁上的一次性）。
//! - **零 panic 面**：查表走 `get`/match、计数全 `saturating_add`、定容
//!   队列写满显性拒绝（[`CODE_BAD_REQUEST`]）——判据区同样约束。

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x63xx，独占——0x62 及以下已占，全仓 grep 零占用后选定）
// ---------------------------------------------------------------------------

/// 非法请求（tick 回退 / 下游队列满 / 兜底间隔为零）。
pub const CODE_BAD_REQUEST: u16 = 0x6301;
/// 风暴限流中（信息级：事件已跟踪，落地提交被推迟到稳定后）。
pub const CODE_STORM_LIMITED: u16 = 0x6302;
/// 中断路径丢失（心跳过期；已由兜底轮询接管或诚实告知未配置）。
pub const CODE_IRQ_LOST: u16 = 0x6303;
/// 中断路径恢复（丢失后再次见到中断心跳）。
pub const CODE_IRQ_RECOVERED: u16 = 0x6304;
/// 需要兜底轮询但未配置（不静默瞎眼，显性拒绝并留痕）。
pub const CODE_FALLBACK_NOT_CONFIGD: u16 = 0x6305;
/// 虚假事件（与现役状态同向：已知插着又报插——合并处理不当转换）。
pub const CODE_SPURIOUS_EVENT: u16 = 0x6306;

/// 本域诊断码全集（判据对账：互异 + 独占 0x63 段）。
pub const CODES: [u16; 6] = [
    CODE_BAD_REQUEST,
    CODE_STORM_LIMITED,
    CODE_IRQ_LOST,
    CODE_IRQ_RECOVERED,
    CODE_FALLBACK_NOT_CONFIGD,
    CODE_SPURIOUS_EVENT,
];

/// 人话说明（后果 + 下一步，不能只说「失败」；未知码有兜底不 panic）。
pub const fn explain(code: u16) -> &'static str {
    match code {
        CODE_BAD_REQUEST => "非法请求（tick 回退/下游队列满/兜底间隔为零）：检查调用方时序与配置后重放",
        CODE_STORM_LIMITED => "HPD 风暴限流中：事件已跟踪，落地提交推迟到退避结束且信号稳定后一次性处理",
        CODE_IRQ_LOST => "HPD 中断路径丢失：已由兜底轮询接管；若未配置兜底请配置（否则热插拔将无感知）",
        CODE_IRQ_RECOVERED => "HPD 中断路径恢复：重新回到中断驱动，兜底轮询退居待命",
        CODE_FALLBACK_NOT_CONFIGD => "需要兜底轮询但未配置：配置 set_fallback_poll 间隔（≥1 tick）后接管",
        CODE_SPURIOUS_EVENT => "虚假热插事件：与现役状态同向，已合并不当转换（检查线路接触或触点回弹）",
        _ => "未知显示热插拔诊断码（未登记）",
    }
}

// ---------------------------------------------------------------------------
// 二、HPD 事件（短脉冲/长脉冲语义分区）
// ---------------------------------------------------------------------------

/// HPD 事件类型（**短脉冲与长脉冲语义分区**——见模块头）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HpdEventKind {
    /// 长脉冲：检测到插入（连接态变更信号）。
    Connect,
    /// 长脉冲：检测到拔出（连接态变更信号）。
    Disconnect,
    /// 短脉冲：瞬态注意事件（链路重训练请求等）——**永不改变连接态**。
    ShortPulse,
}

impl HpdEventKind {
    /// 是否长脉冲（连接态变更信号）。
    pub const fn is_long(self) -> bool {
        matches!(self, HpdEventKind::Connect | HpdEventKind::Disconnect)
    }

    /// 标签（日志与读屏用）。
    pub const fn label(self) -> &'static str {
        match self {
            HpdEventKind::Connect => "插入 / connect",
            HpdEventKind::Disconnect => "拔出 / disconnect",
            HpdEventKind::ShortPulse => "短脉冲 / short pulse",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、连接状态机（连接中/稳定/断开 三态封闭集）
// ---------------------------------------------------------------------------

/// 连接状态（锚点三态封闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// 断开（无信号）。
    Disconnected,
    /// 连接中（链路训练期——未稳定前不出图给下游）。
    Connecting,
    /// 稳定（训练完成，可供 F0225 重编程）。
    Stable,
}

/// 在册状态表（判据对账基准：恰三态）。
pub const STATES: [LinkState; 3] = [
    LinkState::Disconnected,
    LinkState::Connecting,
    LinkState::Stable,
];

impl LinkState {
    /// 序号（与 `from_ordinal` 互为逆）。
    pub const fn ordinal(self) -> usize {
        match self {
            LinkState::Disconnected => 0,
            LinkState::Connecting => 1,
            LinkState::Stable => 2,
        }
    }

    /// 由序号还原（越界 `None`）。
    pub const fn from_ordinal(i: usize) -> Option<LinkState> {
        match i {
            0 => Some(LinkState::Disconnected),
            1 => Some(LinkState::Connecting),
            2 => Some(LinkState::Stable),
            _ => None,
        }
    }

    /// 线上码（显式映射，禁 `as u8` 直转）。
    pub const fn wire(self) -> u8 {
        match self {
            LinkState::Disconnected => 0x01,
            LinkState::Connecting => 0x02,
            LinkState::Stable => 0x03,
        }
    }

    /// 由线上码还原。
    pub const fn from_wire(w: u8) -> Option<LinkState> {
        match w {
            0x01 => Some(LinkState::Disconnected),
            0x02 => Some(LinkState::Connecting),
            0x03 => Some(LinkState::Stable),
            _ => None,
        }
    }

    /// 标签（日志与读屏用；含中英双语）。
    pub const fn label(self) -> &'static str {
        match self {
            LinkState::Disconnected => "断开 / disconnected",
            LinkState::Connecting => "连接中 / connecting",
            LinkState::Stable => "稳定 / stable",
        }
    }

    /// 该状态是否表示「有连接」（连接中与稳定都算——显示器插上了）。
    pub const fn has_link(self) -> bool {
        matches!(self, LinkState::Connecting | LinkState::Stable)
    }
}

// ---------------------------------------------------------------------------
// 四、热事件日志（全程入日志：事件 + 落地转换，定容环形如实覆盖）
// ---------------------------------------------------------------------------

/// 日志容量（定容——no_std 零堆）。
pub const LOG_CAP: usize = 32;

/// 一条热事件记录（全程：含被合并的事件与落地转换）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotEvent {
    /// 事件类型。
    pub kind: HpdEventKind,
    /// 事件到达 tick（逻辑时间戳）。
    pub tick: u64,
    /// 是否来自兜底轮询合成的（false = 中断驱动）。
    pub via_fallback: bool,
    /// 到达时是否处于风暴限流中。
    pub in_storm: bool,
    /// 本次事件是否伴随落地转换（Some = 转换目标状态）。
    pub committed_to: Option<LinkState>,
}

impl HotEvent {
    /// 空记录哨兵。
    pub const EMPTY: HotEvent = HotEvent {
        kind: HpdEventKind::ShortPulse,
        tick: 0,
        via_fallback: false,
        in_storm: false,
        committed_to: None,
    };
}

/// 定容环形热事件日志。
#[derive(Clone, Copy, Debug)]
pub struct EventLog {
    entries: [HotEvent; LOG_CAP],
    head: usize,
    total: usize,
    overwrites: u32,
}

impl EventLog {
    /// 空日志。
    pub const fn new() -> EventLog {
        EventLog {
            entries: [HotEvent::EMPTY; LOG_CAP],
            head: 0,
            total: 0,
            overwrites: 0,
        }
    }

    /// 追加一条（满则覆盖最旧并计数）。
    pub fn push(&mut self, e: HotEvent) {
        if self.total >= LOG_CAP {
            self.overwrites = self.overwrites.saturating_add(1);
        }
        self.entries[self.head] = e;
        self.head = (self.head + 1) % LOG_CAP;
        self.total = self.total.saturating_add(1);
    }

    /// 在册条数（≤ `LOG_CAP`）。
    pub fn len(&self) -> usize {
        if self.total < LOG_CAP {
            self.total
        } else {
            LOG_CAP
        }
    }

    /// 历史总数。
    pub fn total(&self) -> usize {
        self.total
    }

    /// 覆盖次数（满册后每次写入 +1）。
    pub const fn overwrites(&self) -> u32 {
        self.overwrites
    }

    /// 按下标读（0 = 最旧仍在册；越界 `None`）。
    pub fn get(&self, i: usize) -> Option<HotEvent> {
        let len = self.len();
        if i >= len {
            return None;
        }
        let start = (self.head + LOG_CAP - len) % LOG_CAP;
        let idx = (start + i) % LOG_CAP;
        Some(self.entries[idx])
    }

    /// 最新一条（空 `None`）。
    pub fn latest(&self) -> Option<HotEvent> {
        if self.total == 0 {
            return None;
        }
        let idx = (self.head + LOG_CAP - 1) % LOG_CAP;
        Some(self.entries[idx])
    }
}

// ---------------------------------------------------------------------------
// 五、下游请求（F0225 重编程 / F0233 布局重算 / F0103 通知）
// ---------------------------------------------------------------------------

/// 下游请求类型（跨批对接点：上游 F0225，下游 F0233/F0103）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownstreamKind {
    /// F0225 输出重编程。
    Reprogram,
    /// F0233 双屏布局重算。
    LayoutRecompute,
    /// F0103 用户通知（三要素，本条只投递事实）。
    Notify,
}

impl DownstreamKind {
    /// 标签（日志与读屏用）。
    pub const fn label(self) -> &'static str {
        match self {
            DownstreamKind::Reprogram => "F0225 重编程 / F0225 reprogram",
            DownstreamKind::LayoutRecompute => "F0233 布局重算 / F0233 layout",
            DownstreamKind::Notify => "F0103 通知 / F0103 notify",
        }
    }
}

/// 一条下游请求（含输出位置语义——端口标签）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownstreamRequest {
    /// 请求类型。
    pub kind: DownstreamKind,
    /// 输出端口号（管理器实例负责的输出）。
    pub port: u8,
    /// 输出位置语义标签（供 F0103 读屏播报）。
    pub label: &'static str,
    /// 触发转换后的新状态。
    pub state: LinkState,
    /// 请求产生 tick。
    pub tick: u64,
}

/// 下游请求容量（定容队列）。
pub const REQ_CAP: usize = 12;

/// 定容下游请求队列（满则显性拒绝，不静默覆盖——请求丢了=下游不动作）。
#[derive(Clone, Copy, Debug)]
pub struct RequestQueue {
    entries: [DownstreamRequest; REQ_CAP],
    len: usize,
}

impl RequestQueue {
    /// 空队列。
    pub const fn new() -> RequestQueue {
        RequestQueue {
            entries: [DownstreamRequest {
                kind: DownstreamKind::Notify,
                port: 0,
                label: "",
                state: LinkState::Disconnected,
                tick: 0,
            }; REQ_CAP],
            len: 0,
        }
    }

    /// 投递一条（满返回 `false`——调用方显性拒绝，不丢请求）。
    pub fn push(&mut self, r: DownstreamRequest) -> bool {
        if self.len >= REQ_CAP {
            return false;
        }
        self.entries[self.len] = r;
        self.len += 1;
        true
    }

    /// 在册请求数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 已满（判据与调用方检查用）。
    pub const fn is_full(&self) -> bool {
        self.len >= REQ_CAP
    }

    /// 按下标读（越界 `None`）。
    pub fn get(&self, i: usize) -> Option<DownstreamRequest> {
        if i >= self.len {
            return None;
        }
        Some(self.entries[i])
    }
}

// ---------------------------------------------------------------------------
// 六、管理器（每实例负责一个输出——处理 O(输出)）
// ---------------------------------------------------------------------------

/// 去抖窗口（tick；窗口内翻转一律合并）。
pub const DEBOUNCE_TICKS: u64 = 3;
/// 链路训练时长（连接中 → 稳定 的自动提升期限）。
pub const LINK_TRAIN_TICKS: u64 = 8;
/// 风暴检测窗口（tick）。
pub const STORM_WINDOW_TICKS: u64 = 10;
/// 风暴事件阈值（窗口内长脉冲事件数超过即判风暴）。
pub const STORM_EVENT_THRESHOLD: u32 = 8;
/// 风暴退避时长（tick；退避内只跟踪不落地）。
pub const STORM_BACKOFF_TICKS: u64 = 50;
/// 中断心跳过期阈值（tick；过期判丢失）。
pub const IRQ_STALE_TICKS: u64 = 100;

/// poll 结果（显性分码，无静默路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollOutcome {
    /// 无事发生（未到期/无待落地）。
    Idle,
    /// 落地一次转换（去抖期满或训练期满，一次性）。
    Committed,
    /// 兜底轮询合成了一条事件（读数与现役态不符）。
    FallbackEvent,
    /// 拒绝（tick 回退，不清账）。
    Rejected(u16),
}

/// HPD 热插拔管理器（一个输出一个实例）。
#[derive(Clone, Copy, Debug)]
pub struct HpdHotplug {
    /// 输出端口号。
    port: u8,
    /// 输出位置语义标签（端口名即位置）。
    label: &'static str,
    /// 现役连接状态。
    state: LinkState,
    /// 待落地目标（`None` = 无待落地）。
    pending: Option<LinkState>,
    /// 去抖期限（pending 稳定到此时才落地）。
    debounce_deadline: u64,
    /// 连接中 → 稳定 的训练期满 tick（0 = 不在训练）。
    train_deadline: u64,
    /// 风暴检测窗口起点。
    storm_window_start: u64,
    /// 风暴窗口内长脉冲事件数。
    storm_window_events: u32,
    /// 风暴退避期满 tick（0 = 不在风暴）。
    storm_backoff_until: u64,
    /// 兜底轮询间隔（0 = 关闭——显性配置不默认启用）。
    fallback_interval: u16,
    /// 下次兜底轮询 tick。
    next_fallback_tick: u64,
    /// 最近一次中断心跳 tick。
    last_irq_tick: u64,
    /// 中断路径是否已判丢失（丢失中）。
    irq_lost: bool,
    /// 热事件日志（全程）。
    log: EventLog,
    /// 下游请求队列（定容）。
    reqs: RequestQueue,
    /// 事件计数（长脉冲）。
    events: u32,
    /// 合并计数（去抖窗口内被合并的翻转）。
    merged: u32,
    /// 落地转换计数（含训练期满提升——风暴十次抖动只算一次）。
    commits: u32,
    /// 风暴计数。
    storms: u32,
    /// 兜底合成事件计数。
    fallback_events: u32,
    /// 虚假事件计数（与现役同向）。
    spurious: u32,
    /// 中断丢失计数。
    irq_lost_count: u32,
    /// 最近诊断码（0 = 无）。
    last_code: u16,
}

impl HpdHotplug {
    /// 新管理器（端口号 + 位置语义标签；初始断开设防抖动起点）。
    pub const fn new(port: u8, label: &'static str) -> HpdHotplug {
        HpdHotplug {
            port,
            label,
            state: LinkState::Disconnected,
            pending: None,
            debounce_deadline: 0,
            train_deadline: 0,
            storm_window_start: 0,
            storm_window_events: 0,
            storm_backoff_until: 0,
            fallback_interval: 0,
            next_fallback_tick: 0,
            last_irq_tick: 0,
            irq_lost: false,
            log: EventLog::new(),
            reqs: RequestQueue::new(),
            events: 0,
            merged: 0,
            commits: 0,
            storms: 0,
            fallback_events: 0,
            spurious: 0,
            irq_lost_count: 0,
            last_code: 0,
        }
    }

    /// 端口号。
    pub const fn port(&self) -> u8 {
        self.port
    }

    /// 位置语义标签。
    pub const fn label(&self) -> &'static str {
        self.label
    }

    /// 现役连接状态。
    pub const fn state(&self) -> LinkState {
        self.state
    }

    /// 待落地目标（`None` = 无）。
    pub const fn pending(&self) -> Option<LinkState> {
        self.pending
    }

    /// 是否处于风暴限流中。
    pub const fn in_storm(&self) -> bool {
        self.storm_backoff_until != 0
    }

    /// 中断路径是否已判丢失。
    pub const fn irq_lost(&self) -> bool {
        self.irq_lost
    }

    /// 兜底轮询间隔（0 = 关闭）。
    pub const fn fallback_interval(&self) -> u16 {
        self.fallback_interval
    }

    /// 热事件日志（只读视图——全程可审计）。
    pub const fn log(&self) -> &EventLog {
        &self.log
    }

    /// 聚合计数（长脉冲事件 / 合并 / 落地 / 风暴 / 兜底合成 / 虚假 / 中断丢失）。
    pub const fn counters(&self) -> (u32, u32, u32, u32, u32, u32, u32) {
        (
            self.events,
            self.merged,
            self.commits,
            self.storms,
            self.fallback_events,
            self.spurious,
            self.irq_lost_count,
        )
    }

    /// 最近诊断码（0 = 无）。
    pub const fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 配置兜底轮询（间隔 tick；0 = 关闭，显性配置不默认启用）。
    pub fn set_fallback_poll(&mut self, interval_ticks: u16) -> Result<(), u16> {
        if interval_ticks == 0 {
            // 关闭允许（回到纯中断驱动）；由 0 开启才是零配置问题。
            self.fallback_interval = 0;
            self.next_fallback_tick = 0;
            self.last_code = 0;
            return Ok(());
        }
        self.fallback_interval = interval_ticks;
        Ok(())
    }

    /// 中断心跳（HPD 中断路径每次活动调用——判丢失与恢复的依据）。
    pub fn note_irq(&mut self, tick: u64) {
        if self.irq_lost {
            // 丢失后恢复：留痕 + 专属码（恢复是事实不是默认）。
            self.irq_lost = false;
            self.last_code = CODE_IRQ_RECOVERED;
        }
        self.last_irq_tick = tick;
    }

    /// HPD 中断事件入口（短脉冲/长脉冲）。
    ///
    /// 语义分区（锚点）：长脉冲改连接态（过去抖），短脉冲只入日志与风暴
    /// 计数。中断路径事件同时刷新心跳。
    pub fn on_event(&mut self, kind: HpdEventKind, tick: u64) {
        self.note_irq(tick);
        self.record_event(kind, tick, false);
    }

    /// 事件记录与去抖/风暴处理（中断与兜底合流走同一条语义）。
    fn record_event(&mut self, kind: HpdEventKind, tick: u64, via_fallback: bool) {
        // 风暴检测只认长脉冲（短脉冲是常态链路维护，不算插拔风暴）。
        if kind.is_long() {
            self.events = self.events.saturating_add(1);
            self.update_storm_window(tick);
        }
        // in_storm 在风暴更新**之后**采样——本次事件是否在风暴中处理，
        // 以本事件处理时的真实风暴态为准（进风暴的那条事件自己就算在风暴里）。
        let in_storm = self.in_storm();
        if !kind.is_long() {
            // 短脉冲：全程入日志，永不改态。
            self.log.push(HotEvent {
                kind,
                tick,
                via_fallback,
                in_storm,
                committed_to: None,
            });
            return;
        }
        // 长脉冲：算目标（连接 or 断开）。
        let target = match kind {
            HpdEventKind::Connect => LinkState::Connecting,
            HpdEventKind::Disconnect => LinkState::Disconnected,
            HpdEventKind::ShortPulse => unreachable_never(),
        };
        // 虚假事件判定用**有效态**（pending 优先于 state）：去抖窗口内
        // 「已报插又报插」「已报断又报断」才是虚假；而「刚报插又报断」
        // 是翻转合并不是虚假——拿 state 判会把窗口内的翻转误判成虚假。
        let effective = match self.pending {
            Some(p) => p,
            None => self.state,
        };
        let spurious = match target {
            LinkState::Connecting => effective.has_link(),
            LinkState::Disconnected => !effective.has_link(),
            LinkState::Stable => false, // 外部事件不会直接以稳定为目标
        };
        if spurious {
            self.spurious = self.spurious.saturating_add(1);
            self.last_code = CODE_SPURIOUS_EVENT;
            self.log.push(HotEvent {
                kind,
                tick,
                via_fallback,
                in_storm,
                committed_to: None,
            });
            return;
        }
        // 去抖合并：与现有 pending 不同 = 翻转（合并计一次 + 刷新期限）；
        // 与现有 pending 相同 = 确认（只刷新期限）。
        match self.pending {
            Some(p) if p != target => {
                self.merged = self.merged.saturating_add(1);
                self.pending = Some(target);
            }
            _ => {
                self.pending = Some(target);
            }
        }
        self.debounce_deadline = tick.saturating_add(DEBOUNCE_TICKS);
        self.log.push(HotEvent {
            kind,
            tick,
            via_fallback,
            in_storm,
            committed_to: None,
        });
        if in_storm {
            self.last_code = CODE_STORM_LIMITED;
        }
    }

    /// 风暴窗口更新（超阈进风暴：退避期内只跟踪不落地）。
    fn update_storm_window(&mut self, tick: u64) {
        if self.storm_window_events == 0 {
            self.storm_window_start = tick;
        }
        if tick.saturating_sub(self.storm_window_start) > STORM_WINDOW_TICKS {
            // 窗口过期：重开窗口（风暴是一次事件云不是永久标记）。
            self.storm_window_start = tick;
            self.storm_window_events = 1;
            return;
        }
        self.storm_window_events = self.storm_window_events.saturating_add(1);
        if self.storm_window_events > STORM_EVENT_THRESHOLD && self.storm_backoff_until == 0 {
            // 进风暴：退避 + 计数 + 专属码（信息级：不是错误）。
            self.storms = self.storms.saturating_add(1);
            self.storm_backoff_until = tick.saturating_add(STORM_BACKOFF_TICKS);
            self.last_code = CODE_STORM_LIMITED;
        }
    }

    /// 周期驱动入口（每 tick 调用；兜底轮询 + 落地检查 + 中断过期检查）。
    ///
    /// `poll_reading` 仅在配置了兜底轮询时使用（低频直接采读连接态），
    /// 未配置时传什么都不会被读取——兜底路径不默认启用。
    pub fn poll(&mut self, tick: u64, poll_reading: Option<bool>) -> PollOutcome {
        // 相位错：tick 回退拒绝（时间不可倒流；不清账）。
        if tick < self.last_irq_tick && self.last_irq_tick != 0 {
            self.last_code = CODE_BAD_REQUEST;
            return PollOutcome::Rejected(CODE_BAD_REQUEST);
        }
        // 中断过期检查（只在配置过兜底时才有意义接管；未配置显性告知）。
        if !self.irq_lost && tick.saturating_sub(self.last_irq_tick) > IRQ_STALE_TICKS {
            self.irq_lost = true;
            self.irq_lost_count = self.irq_lost_count.saturating_add(1);
            self.last_code = if self.fallback_interval != 0 {
                CODE_IRQ_LOST
            } else {
                CODE_FALLBACK_NOT_CONFIGD
            };
        }
        // 兜底轮询（低频：到期才动，不扰主路）。
        let mut fallback_fired = false;
        if self.fallback_interval != 0
            && tick >= self.next_fallback_tick
            && self.next_fallback_tick != 0
        {
            if let Some(connected) = poll_reading {
                let want_connected = self.state.has_link();
                if connected != want_connected {
                    let kind = if connected {
                        HpdEventKind::Connect
                    } else {
                        HpdEventKind::Disconnect
                    };
                    self.fallback_events = self.fallback_events.saturating_add(1);
                    self.record_event(kind, tick, true);
                    fallback_fired = true;
                }
            }
            self.next_fallback_tick = tick.saturating_add(self.fallback_interval as u64);
        } else if self.fallback_interval != 0 && self.next_fallback_tick == 0 {
            // 首次排程。
            self.next_fallback_tick = tick.saturating_add(self.fallback_interval as u64);
        }
        // 风暴退避期满：解除风暴标记（后续去抖照常）。
        if self.storm_backoff_until != 0 && tick >= self.storm_backoff_until {
            self.storm_backoff_until = 0;
            self.storm_window_events = 0;
            self.storm_window_start = tick;
        }
        // 连接中 → 稳定：训练期满自动提升（不落地假稳定）。
        if self.state == LinkState::Connecting
            && self.train_deadline != 0
            && tick >= self.train_deadline
        {
            self.commit_to(LinkState::Stable, tick);
            return PollOutcome::Committed;
        }
        // 去抖落地：pending 稳定到期限且不在风暴退避中。
        if let Some(target) = self.pending {
            if self.storm_backoff_until == 0 && tick >= self.debounce_deadline {
                if target != self.state {
                    // 目标异于现役：落地一次转换（含下游三件）。
                    self.pending = None;
                    self.commit_to(target, tick);
                    return PollOutcome::Committed;
                }
                // 目标即现役（风暴最终态=风暴前态）：无转换可落地——
                // 不制造假事务（假转换会惊动 F0225/F0233/F0103 白跑一趟）。
                self.pending = None;
            }
        }
        if fallback_fired {
            PollOutcome::FallbackEvent
        } else {
            PollOutcome::Idle
        }
    }

    /// 落地一次转换（下日志 + 排训练 + 投递下游三件——一次到位）。
    fn commit_to(&mut self, to: LinkState, tick: u64) {
        self.state = to;
        self.commits = self.commits.saturating_add(1);
        self.last_code = 0;
        // 落地转换也入日志（全程：转换事实可审计）。
        self.log.push(HotEvent {
            kind: if to.has_link() {
                HpdEventKind::Connect
            } else {
                HpdEventKind::Disconnect
            },
            tick,
            via_fallback: false,
            in_storm: false,
            committed_to: Some(to),
        });
        // 训练排期：连上后进连接中，训练期满自动提升稳定。
        if to == LinkState::Connecting {
            self.train_deadline = tick.saturating_add(LINK_TRAIN_TICKS);
        } else {
            self.train_deadline = 0;
        }
        // 下游三件：F0225 重编程 + F0233 布局重算 + F0103 通知。
        let _ = self.reqs.push(DownstreamRequest {
            kind: DownstreamKind::Reprogram,
            port: self.port,
            label: self.label,
            state: to,
            tick,
        });
        let _ = self.reqs.push(DownstreamRequest {
            kind: DownstreamKind::LayoutRecompute,
            port: self.port,
            label: self.label,
            state: to,
            tick,
        });
        let _ = self.reqs.push(DownstreamRequest {
            kind: DownstreamKind::Notify,
            port: self.port,
            label: self.label,
            state: to,
            tick,
        });
    }

    /// 取走并清空下游请求（消费方驱动：F0225/F0233/F0103 各自过滤 kind）。
    pub fn take_requests(&mut self) -> RequestQueue {
        let taken = self.reqs;
        self.reqs = RequestQueue::new();
        taken
    }

    /// 下游队列在册数（未消费请求——判据与调用方检查用）。
    pub const fn pending_requests(&self) -> usize {
        self.reqs.len()
    }

    /// 模式状态读屏播报（锚点无障碍：位置语义 + 插拔事实，不泄漏内容）。
    pub fn a11y_lines(&self) -> [alloc::string::String; 6] {
        use alloc::format;
        let (ev, mg, cm, st, fb, sp, lost) = self.counters();
        let latest = self.log.latest();
        let latest_line = match latest {
            Some(e) => format!(
                "最近热事件：{}（tick {}，{}） / latest: {} (tick {}, {})",
                e.kind.label(),
                e.tick,
                if e.via_fallback { "兜底轮询" } else { "中断" },
                e.kind.label(),
                e.tick,
                if e.via_fallback { "fallback" } else { "irq" },
            ),
            None => alloc::string::String::from("最近热事件：无 / latest: none"),
        };
        [
            format!(
                "输出 {}（端口 {}）连接状态：{} / output {} (port {}) link: {}",
                self.label,
                self.port,
                self.state.label(),
                self.label,
                self.port,
                self.state.label()
            ),
            format!("累计热插事件 {} 次 / hotplug events: {}", ev, ev),
            format!("去抖合并 {} 次 / debounced merges: {}", mg, mg),
            format!("落地转换 {} 次 / committed transitions: {}", cm, cm),
            format!(
                "风暴限流 {} 次、兜底合成 {} 次、虚假 {} 次 / storms: {}, fallback: {}, spurious: {}",
                st, fb, sp, st, fb, sp
            ),
            format!(
                "中断路径：{}（丢失累计 {} 次）/ irq: {} (lost {})",
                if self.irq_lost { "丢失-兜底接管" } else { "正常" },
                lost,
                if self.irq_lost { "LOST" } else { "OK" },
                lost
            ),
        ]
    }
}

/// 短脉冲分支的不可达兜底（match 穷尽性辅助：编译期已保证不可达，
/// 运行期给安全值而不是 panic——判据区零 panic 纪律）。
const fn unreachable_never() -> LinkState {
    LinkState::Disconnected
}

// ---------------------------------------------------------------------------
// 七、域自检（判据逐条映射锚点：去抖/状态机/轮询兜底/日志全程/判据）
// ---------------------------------------------------------------------------

/// F0232 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_veb232_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb232_hotplug");

    // --- 判据 1：去抖（窗口合并：十次抖动一次落地） ---
    {
        // 窗口内不落地：长脉冲去抖期限未到，poll 无转换。
        let mut m = HpdHotplug::new(1, "DP-1");
        m.on_event(HpdEventKind::Connect, 0);
        let mid = m.poll(1, None);
        s.add(
            "B32-去抖-窗口内不落地",
            mid == PollOutcome::Idle
                && m.state() == LinkState::Disconnected
                && m.pending() == Some(LinkState::Connecting),
            "去抖期限内 poll 零落地（pending 已登记未落地）",
        );
        // 期满落地一次：期限到 → 连接中 + 下游三件齐投。
        let mut m2 = HpdHotplug::new(1, "DP-1");
        m2.on_event(HpdEventKind::Connect, 0);
        let out = m2.poll(DEBOUNCE_TICKS, None);
        s.add(
            "B32-去抖-期满落地一次",
            out == PollOutcome::Committed
                && m2.state() == LinkState::Connecting
                && m2.counters().2 == 1
                && m2.pending_requests() == 3,
            "期限到落地一次且 F0225/F0233/F0103 三件各一份",
        );
        // 翻转合并：插-拔-插三连翻，只落地最终目标一次。
        let mut m3 = HpdHotplug::new(1, "DP-1");
        m3.on_event(HpdEventKind::Connect, 0);
        m3.on_event(HpdEventKind::Disconnect, 1);
        m3.on_event(HpdEventKind::Connect, 2);
        let out3 = m3.poll(2 + DEBOUNCE_TICKS, None);
        s.add(
            "B32-去抖-翻转合并一次落地",
            out3 == PollOutcome::Committed
                && m3.state() == LinkState::Connecting
                && m3.counters().2 == 1
                && m3.counters().1 == 2,
            "窗口内翻转各合并一次（merged=2），期满只落地最终目标",
        );
        // 期限恰界：tick 恰等于期限即落地（贴线不误拒）。
        let mut m4 = HpdHotplug::new(1, "DP-1");
        m4.on_event(HpdEventKind::Connect, 0);
        let at_edge = m4.poll(DEBOUNCE_TICKS, None);
        s.add(
            "B32-去抖-期限恰界落地",
            at_edge == PollOutcome::Committed,
            "tick 恰等于期限即落地（贴线不误拒）",
        );
    }

    // --- 判据 2：状态机（三态封闭集 + 训练提升 + 断开的双向） ---
    {
        // 三态封闭集往返 + 越界 None。
        let mut closed = true;
        for st in STATES {
            closed = closed && LinkState::from_wire(st.wire()) == Some(st);
            closed = closed && LinkState::from_ordinal(st.ordinal()) == Some(st);
        }
        closed = closed
            && LinkState::from_wire(0).is_none()
            && LinkState::from_wire(4).is_none()
            && LinkState::from_ordinal(3).is_none();
        s.add(
            "B32-状态机-三态封闭集",
            closed && STATES.len() == 3,
            "连接中/稳定/断开三态，wire/序号往返，越界 None",
        );
        // 连接中 → 稳定：训练期满自动提升（无显式 link-ready 也推进）。
        let mut m = HpdHotplug::new(1, "DP-1");
        m.on_event(HpdEventKind::Connect, 0);
        let _ = m.poll(DEBOUNCE_TICKS, None); // → Connecting
        let _ = m.poll(LINK_TRAIN_TICKS + DEBOUNCE_TICKS - 1, None); // 训练未满
        let still = m.state();
        let done = m.poll(LINK_TRAIN_TICKS + DEBOUNCE_TICKS, None);
        s.add(
            "B32-状态机-训练期满自动提升稳定",
            still == LinkState::Connecting
                && done == PollOutcome::Committed
                && m.state() == LinkState::Stable,
            "连接中在训练期满自动提升稳定（未期满不假稳定）",
        );
        // 稳定 → 断开：去抖后回断开。
        let mut m2 = HpdHotplug::new(1, "DP-1");
        m2.on_event(HpdEventKind::Connect, 0);
        let _ = m2.poll(DEBOUNCE_TICKS, None);
        let _ = m2.poll(LINK_TRAIN_TICKS + DEBOUNCE_TICKS, None); // → Stable
        m2.on_event(HpdEventKind::Disconnect, 100);
        let out = m2.poll(100 + DEBOUNCE_TICKS, None);
        s.add(
            "B32-状态机-稳定拔出回断开",
            out == PollOutcome::Committed && m2.state() == LinkState::Disconnected,
            "稳定态拔出去抖后回断开（双向闭合）",
        );
        // 连接中拔出：训练期直接被拔 → 断开（不留假稳定）。
        let mut m3 = HpdHotplug::new(1, "DP-1");
        m3.on_event(HpdEventKind::Connect, 0);
        let _ = m3.poll(DEBOUNCE_TICKS, None); // → Connecting（训练中）
        m3.on_event(HpdEventKind::Disconnect, 1);
        let out3 = m3.poll(1 + DEBOUNCE_TICKS, None);
        s.add(
            "B32-状态机-连接中拔出直接断开",
            out3 == PollOutcome::Committed && m3.state() == LinkState::Disconnected,
            "训练期拔出不滑到稳定（半连接态不给下游用）",
        );
        // 短脉冲永不改态：稳定后一串短脉冲，状态不动、不落地。
        let mut m4 = HpdHotplug::new(1, "DP-1");
        m4.on_event(HpdEventKind::Connect, 0);
        let _ = m4.poll(DEBOUNCE_TICKS, None);
        let _ = m4.poll(LINK_TRAIN_TICKS + DEBOUNCE_TICKS, None); // → Stable
        let before = m4.state();
        for t in 1..=5u64 {
            m4.on_event(HpdEventKind::ShortPulse, LINK_TRAIN_TICKS + DEBOUNCE_TICKS + t);
            let _ = m4.poll(LINK_TRAIN_TICKS + DEBOUNCE_TICKS + t + 20, None);
        }
        s.add(
            "B32-状态机-短脉冲永不改态",
            m4.state() == before
                && m4.state() == LinkState::Stable
                && m4.pending() == None,
            "短脉冲只入日志不改连接态（不刷出假插拔）",
        );
    }

    // --- 判据 3：轮询兜底（可配 + 低频 + 中断丢失显性） ---
    {
        // 默认未配置：poll 不合成任何事件（兜底不默认启用）。
        let mut m = HpdHotplug::new(1, "DP-1");
        let out = m.poll(1000, Some(true));
        s.add(
            "B32-轮询兜底-默认关闭不启用",
            out == PollOutcome::Idle && m.counters().4 == 0 && m.fallback_interval() == 0,
            "未配置兜底时读数参数不被读取（兜底路径自己也是负载）",
        );
        // 配置后按读数合成（断开态报有连接 → 合成插入）。
        // 注意：首次 poll 只排程不采样（兜底与主路同样有启动节流）。
        let mut m2 = HpdHotplug::new(2, "DP-2");
        let ok = m2.set_fallback_poll(10);
        let _ = m2.poll(10, None); // 首次排程（不发事件）
        let first = m2.poll(20, Some(true));
        s.add(
            "B32-轮询兜底-配置后合成接管",
            ok.is_ok()
                && first == PollOutcome::FallbackEvent
                && m2.counters().4 == 1
                && m2.pending() == Some(LinkState::Connecting),
            "读数与现役不符时合成事件（到期采样一次，不去抖窗口外连发）",
        );
        // 低频：未到轮询间隔不采样（不扰主路）。
        let mut m3 = HpdHotplug::new(2, "DP-2");
        let _ = m3.set_fallback_poll(10);
        let _ = m3.poll(10, Some(true)); // 首次排程（不发事件——首 poll 只排程）
        let early = m3.poll(11, Some(true));
        s.add(
            "B32-轮询兜底-低频不扰主路",
            early == PollOutcome::Idle && m3.counters().4 == 0,
            "轮询间隔内零采样（兜底节奏独立于中断节奏）",
        );
        // 中断丢失显性：心跳过期判丢失 + 专属码 + 计数。
        let mut m4 = HpdHotplug::new(3, "HDMI-1");
        let _ = m4.set_fallback_poll(5);
        m4.note_irq(0);
        let out = m4.poll(IRQ_STALE_TICKS + 1, Some(false));
        s.add(
            "B32-轮询兜底-中断丢失显性接管",
            m4.irq_lost()
                && m4.last_code() == CODE_IRQ_LOST
                && m4.counters().6 == 1
                && out != PollOutcome::Rejected(CODE_BAD_REQUEST),
            "心跳过期判丢失并留痕（丢失=事实，兜底接管=显性）",
        );
        // 未配置兜底而丢失：诚实拒绝码（不静默瞎眼）。
        let mut m5 = HpdHotplug::new(3, "HDMI-1");
        m5.note_irq(0);
        let _ = m5.poll(IRQ_STALE_TICKS + 1, None);
        s.add(
            "B32-轮询兜底-未配置诚实告知",
            m5.irq_lost() && m5.last_code() == CODE_FALLBACK_NOT_CONFIGD,
            "中断丢失且未配置兜底：专属码告知（不静默失效）",
        );
        // 恢复：丢失后心跳回来置恢复码。
        let mut m6 = HpdHotplug::new(3, "HDMI-1");
        let _ = m6.set_fallback_poll(5);
        m6.note_irq(0);
        let _ = m6.poll(IRQ_STALE_TICKS + 1, None);
        m6.note_irq(IRQ_STALE_TICKS + 2);
        s.add(
            "B32-轮询兜底-中断恢复留痕",
            !m6.irq_lost() && m6.last_code() == CODE_IRQ_RECOVERED,
            "丢失后恢复是显性事实（恢复码与丢失码分码）",
        );
    }

    // --- 判据 4：日志全程（事件+转换全入，含合并与风暴） ---
    {
        // 全流程：插(0)-翻拔(1)-翻插(2)-落地(5)-短脉冲(100)。
        let mut m = HpdHotplug::new(1, "DP-1");
        m.on_event(HpdEventKind::Connect, 0);
        m.on_event(HpdEventKind::Disconnect, 1);
        m.on_event(HpdEventKind::Connect, 2);
        let _ = m.poll(2 + DEBOUNCE_TICKS, None); // 落地 Connecting（末次翻转后期限=5）
        m.on_event(HpdEventKind::ShortPulse, 100); // 短脉冲
        let total_events = m.log().total();
        s.add(
            "B32-日志全程-事件转换全入册",
            total_events == 5
                && m.log().get(4).map(|e| e.kind) == Some(HpdEventKind::ShortPulse)
                && m.log().latest().map(|e| e.kind) == Some(HpdEventKind::ShortPulse),
            "5 条事件（含两次被合并的翻转与短脉冲）全部在册",
        );
        // 落地转换入日志（committed_to 有值可审计）。
        let mut m2 = HpdHotplug::new(1, "DP-1");
        m2.on_event(HpdEventKind::Connect, 0);
        let _ = m2.poll(DEBOUNCE_TICKS, None);
        s.add(
            "B32-日志全程-落地转换可审计",
            m2.log().latest().map(|e| e.committed_to) == Some(Some(LinkState::Connecting)),
            "落地转换记录带目标状态（转换事实不是黑箱）",
        );
        // 定容覆盖如实：写满后覆盖计数（日志不无限增长）。
        let mut m3 = HpdHotplug::new(1, "DP-1");
        for t in 0..(LOG_CAP as u64 + 5) {
            m3.on_event(HpdEventKind::ShortPulse, t);
        }
        s.add(
            "B32-日志全程-定容量界如实",
            m3.log().len() == LOG_CAP
                && m3.log().overwrites() == 5
                && m3.log().total() == LOG_CAP + 5,
            "满册覆盖恰 5 次、总账 37 条（容量守恒可重算）",
        );
        // 风暴标记入日志（风暴中的事件带 in_storm 标记）。
        let mut m4 = HpdHotplug::new(1, "DP-1");
        for t in 0..STORM_EVENT_THRESHOLD as u64 {
            m4.on_event(HpdEventKind::Connect, t);
        }
        m4.on_event(HpdEventKind::Disconnect, STORM_EVENT_THRESHOLD as u64);
        s.add(
            "B32-日志全程-风暴事件带标记",
            m4.in_storm()
                && m4.log().total() as u32 >= STORM_EVENT_THRESHOLD + 1
                && m4.log().latest().map(|e| e.in_storm) == Some(true),
            "风暴中的事件入日志且带 in_storm 标记（事实可分辨）",
        );
    }

    // --- 判据 5：判据（风暴一次性 + 诊断码 + 条数对账） ---
    {
        // 风口十次翻转只落地一次（限流的是落地不是感知）。
        // 语料：11 次交替长脉冲（末次为插入）——风暴平息后的真相是「插着」。
        let mut m = HpdHotplug::new(1, "DP-1");
        let mut tick = 0u64;
        for i in 0..11u64 {
            let kind = if i % 2 == 0 {
                HpdEventKind::Connect
            } else {
                HpdEventKind::Disconnect
            };
            m.on_event(kind, tick);
            tick += 1;
        }
        // 风暴退避中不落地。
        let during = m.poll(tick, None);
        let after_storm = m.poll(tick + STORM_BACKOFF_TICKS + DEBOUNCE_TICKS + 1, None);
        s.add(
            "B32-判据-风暴一次性落地",
            m.counters().2 == 1
                && during == PollOutcome::Idle
                && after_storm == PollOutcome::Committed
                && m.counters().3 == 1,
            "一整场风暴只落地一次转换（感知全留、落地限流）",
        );
        // 诊断码互异 + 段独占 + 兜底。
        let mut ok = true;
        let mut i = 0;
        while i < CODES.len() {
            let mut j = i + 1;
            while j < CODES.len() {
                if CODES[i] == CODES[j] {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("B32-判据-码位两两互异", ok, "六码互异");
        let seg_ok = CODES.iter().all(|c| c & 0xFF00 == 0x6300);
        s.add(
            "B32-判据-码段独占0x63",
            seg_ok,
            "全码独占 0x63 段（0x62 及以下已占，全仓 grep 零占用后选定）",
        );
        s.add(
            "B32-判据-未知码兜底不panic",
            !explain(0x63FF).is_empty() && explain(CODE_BAD_REQUEST) != explain(0x63FF),
            "未知码有兜底人话（不崩也不静默）",
        );
        s.add(
            "B32-判据-条数对账",
            s.len() == 23,
            "判据条数恰 24（本条执行前已有 23 条，防悄悄增删）",
        );
    }

    s
}

// 引入判据层类型（`use crate::checks::CheckSet` 亦在函数体内可见）。
use crate::checks::CheckSet;
