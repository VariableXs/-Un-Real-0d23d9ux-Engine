//! VE-F3205 · 加载请求与优先级（Q 域 · 资源请求段 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3205`
//!
//! **判据（锚点原文）**：五级语义、级内 FIFO、请求合并、语义通胀、前置拦截、判据。
//!
//! **职责定位**：本条是资源加载的**请求侧**——把「谁要什么、多急、能等多久」
//! 表达成可裁决的数据结构，并把脏请求挡在队列之外。它**不碰 IO**：
//! 真正的取数在 F 域解码器，调度消费在 F3207，去重在 F3210，带宽联动在 F3222。
//! 三者都以本条的 [`LoadRequest`] 与 [`Priority`] 为输入契约，故本条的**类型**
//! 一旦定错，下游三处会一起错。
//!
//! ## 锚点原文要点 → 本文件章节映射
//!
//! - 请求体系（URI/类型/优先级/回调/超时**五元**） → §三 [`LoadRequest`]
//! - 优先级枚举（critical/high/normal/low/idle **五级**）+ 语义表 → §二 [`Priority`]
//! - 优先级队列（五级多队列 + **级内 FIFO**） → §四 [`PriorityQueues`]
//! - 请求合并（同 URI 同参并发 → **单请求多订阅**） → §五 [`MergeTable`]
//! - 请求验证（URI 合法性/类型匹配**前置校验**） → §六 [`validate_request`]
//! - 错误路径与降级矩阵（四条） → §七
//! - 性能逐项分解 → §八 [`PERF_BUDGET`]
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! 1. **非法 URI → 前置拒绝三要素（不进队列——脏请求红线）**。
//!    「三要素」= 原因（cause）+ 建议动作（advice）+ 可读串（readable）。
//!    只说「非法」而不说「为什么、怎么办」的错误，等于把排查成本转嫁给调用方。
//!    **不进队列**是硬要求：脏请求一旦入队，就会被调度器消费、占用带宽、
//!    触发回调——把「一个错的请求」放大成「一次资源事故」。
//! 2. **优先级越级滥用（全 critical → 降级为 FIFO + 告警）**。
//!    锚点称之为「语义通胀红线：都急=都不急」。本条把它做成**可判定的**：
//!    一个批次里全部请求都是 `critical` 时，判定为语义通胀，整批**降级为
//!    `normal` 的 FIFO**并告警。理由：优先级只有相对意义，全都最高级时
//!    优先级退化成随机序（取决于入队顺序），比显式的 FIFO 更容易造成饿死。
//! 3. **合并表泄漏 → 订阅计数回收（泄漏闸）**。合并表的条目靠订阅计数
//!    存活；最后一个订阅者退订时必须回收条目，否则表会单调增长，
//!    最终把内存吃光。故 [`MergeTable::unsubscribe`] 是**必须实现**的闸门，
//!    且判据要验证「退订后条目真的消失」而不只是「计数归零」。
//! 4. **超时未配 → 默认超时 + 告警**。五元里的超时有合法区间，
//!    零值表示「调用方没配」——此时套用默认超时并**告警**，而不是当作
//!    「无限等待」。当作无限等待会让一个忘配超时的请求永久占住队列槽位。
//!
//! **数据结构**：请求五元（[`LoadRequest`]）；五级优先级（[`Priority`]）；
//! 五级队列（[`PriorityQueues`]）；合并表（[`MergeTable`]）；
//! 前置校验器（[`validate_request`]）。
//!
//! **性能逐项分解**：入队 **O(1)**（尾插进环形槽）；合并 **O(1)**（线性小表
//! 查表，表长上限 [`MERGE_SLOTS`] 固定）；校验 **O(URI 长度)**；告警 **O(1)**。
//!
//! **对接点**：F3207 调度消费（[`PriorityQueues::pop`] 是它的输入面）；
//! F3210 去重（在合并表之上做内容级去重）；F3222 带宽（优先级联动前向——
//! 高优先级可预取更多，但**带宽分配不是本条的决定权**，本条只提供优先级）；
//! 八个消费域的请求侧。
//!
//! ## 设计要点
//!
//! - **优先级五级是闭集不是整数**（[`Priority`]）：用 `u8` 裸值会让
//!   「传了 7」既不报错也不拒绝，只是排在最末——调用方以为自己是最高优先级。
//!   闭集 + [`Priority::from_wire`] 遇未知码返回 [`None`]，让错误在入口暴露。
//! - **五级多队列而非单队列内排序**（[`PriorityQueues`]）：单队列按优先级
//!   排序每次入队要 O(n log n)，而资源加载的入队频率高到能吃掉整个帧预算。
//!   五级各一条 FIFO 槽，入队 O(1)、出队 O(1)，代价是**级内不排序**——
//!   这正是锚点要的语义（「级内 FIFO」）。
//! - **合并只认「同 URI + 同类型 + 同优先级」**（[`MergeKey`]）：少了类型，
//!   同一 URI 的纹理与缓冲会被合成一个请求；少了优先级，两个不同紧急度的
//!   订阅者会共享到「先到者的优先级」，后到的高优先级请求被降级。
//!   故合并键是**三元**，缺一即错误合并。
//! - **合并是「单请求多订阅」不是「丢弃后到者」**（[`MergeTable`]）：
//!   后到者拿到**自己的订阅句柄**，回调各自触发。丢弃后到者的回调是
//!   最常见的实现偷懒，也是最难查的一类丢失。
//! - **语义通胀按「批次内全critical」判定，不按比例**（[`detect_inflation`]）：
//!   比例阈值（如 critical 占比 >80%）在「一个批次只有 1 个请求」时会误报——
//!   1/1 = 100% 但那不是通胀。全 critical 是**无论批次大小都成立**的
//!   退化情形，是唯一可判定的通胀形态。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F3207 调度消费**消费本条的队列；**F3210 去重**在本条合并之上做
//!   内容级去重（本条只认「键相同」，F3210 认「内容相同」）。
//! - 本条**不做带宽决策**：优先级是输入，带宽分配由 F3222 决定。
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

/// 优先级级数（锚点明文五级）。
pub const PRIORITY_LEVELS: usize = 5;

/// 每级队列槽位上限（每级一条环形FIFO）。
pub const QUEUE_SLOTS: usize = 64;

/// 合并表槽位上限。
pub const MERGE_SLOTS: usize = 64;

/// 请求 URI 最大长度（字节）。
pub const URI_MAX_LEN: usize = 512;

/// 默认超时（毫秒）——**超时未配时套用并告警**，不当作无限等待。
pub const DEFAULT_TIMEOUT_MS: u32 = 30_000;

/// 最小超时（毫秒）：低于此值请求必被前置拒绝。
pub const MIN_TIMEOUT_MS: u32 = 16;

/// 最大超时（毫秒）。
pub const MAX_TIMEOUT_MS: u32 = 600_000;

/// 负载类型种数（闭合集，杜绝任意字符串混进类型位）。
pub const LOAD_TYPE_COUNT: usize = 4;

/// 前置拒绝变体种数。
pub const REJECT_COUNT: usize = 4;

/// 语义通胀判定的唯一形态：**批内全为critical**。
pub const INFLATION_LEVEL: u8 = 0;

// ---------------------------------------------------------------------------
// 二、优先级（五级闭集+ 语义显性）
// ---------------------------------------------------------------------------

/// 加载优先级（**闭集**，锚点明文五级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// 当前帧必需。
    Critical,
    /// 本帧内尽快。
    High,
    /// 常规。
    Normal,
    /// 可延后。
    Low,
    /// 空闲时才做。
    Idle,
}

impl Priority {
    /// 全枚举（**顺序即 [`Priority::ordinal`] 的下标**，从最急到最闲）。
    pub const ALL: [Priority; 5] = [
        Priority::Critical,
        Priority::High,
        Priority::Normal,
        Priority::Low,
        Priority::Idle,
    ];

    /// 枚举下标（0 = 最急）。
    pub const fn ordinal(self) -> usize {
        match self {
            Priority::Critical => 0,
            Priority::High => 1,
            Priority::Normal => 2,
            Priority::Low => 3,
            Priority::Idle => 4,
        }
    }

    /// 由下标反解（**越界返回 [`None`]**，不静默夹到末级——
    /// 夹到 `Idle` 会让一个错误的 critical 请求悄悄排到最后）。
    pub const fn from_ordinal(o: usize) -> Option<Priority> {
        match o {
            0 => Some(Priority::Critical),
            1 => Some(Priority::High),
            2 => Some(Priority::Normal),
            3 => Some(Priority::Low),
            4 => Some(Priority::Idle),
            _ => None,
        }
    }

    /// 线编码（**显式映射，不用 `as u8` 拿判别值**）。
    ///
    /// 取 `0xC0 + ordinal` 形态，与判别值错开，使「改枚举顺序静默改协议」
    /// 在判据层直接转红。
    pub const fn wire(self) -> u8 {
        0xC0 + self.ordinal() as u8
    }

    /// 线编码反解（未知码 [`None`]）。
    pub const fn from_wire(w: u8) -> Option<Priority> {
        if w < 0xC0 {
            return None;
        }
        Priority::from_ordinal((w - 0xC0) as usize)
    }

    /// 语义说明（**语义显性**：每级一句话，进读屏面板）。
    pub const fn semantics(self) -> &'static str {
        match self {
            Priority::Critical => "当前帧必需 / required this frame",
            Priority::High => "本帧内尽快 / soon within this frame",
            Priority::Normal => "常规无时限 / normal, no deadline",
            Priority::Low => "可延后到后续帧 / may defer to later frames",
            Priority::Idle => "空闲时才做 / only when idle",
        }
    }

    /// 是否为最高级（语义通胀判定的唯一依据）。
    pub const fn is_critical(self) -> bool {
        matches!(self, Priority::Critical)
    }
}

// ---------------------------------------------------------------------------
// 三、请求五元（URI / 类型 / 优先级 / 回调 / 超时）
// ---------------------------------------------------------------------------

/// 负载类型（**闭合集**，杜绝任意字符串混进类型位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadType {
    /// 纹理。
    Texture,
    /// 缓冲。
    Buffer,
    /// 着色器。
    Shader,
    /// 几何网格。
    Mesh,
}

impl LoadType {
    /// 全枚举。
    pub const ALL: [LoadType; 4] =
        [LoadType::Texture, LoadType::Buffer, LoadType::Shader, LoadType::Mesh];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            LoadType::Texture => 0,
            LoadType::Buffer => 1,
            LoadType::Shader => 2,
            LoadType::Mesh => 3,
        }
    }

    /// 线编码（显式映射）。
    pub const fn wire(self) -> u8 {
        match self {
            LoadType::Texture => 0x70,
            LoadType::Buffer => 0x71,
            LoadType::Shader => 0x72,
            LoadType::Mesh => 0x73,
        }
    }

    /// 线编码反解（未知码 [`None`]）。
    pub const fn from_wire(w: u8) -> Option<LoadType> {
        match w {
            0x70 => Some(LoadType::Texture),
            0x71 => Some(LoadType::Buffer),
            0x72 => Some(LoadType::Shader),
            0x73 => Some(LoadType::Mesh),
            _ => None,
        }
    }

    /// 该类型要求的**最小超时**（毫秒）——着色器编译最慢，纹理可快些。
    /// 类型匹配前置校验用它：给着色器 1ms 超时必然超时。
    pub const fn min_timeout_ms(self) -> u32 {
        match self {
            LoadType::Texture => MIN_TIMEOUT_MS,
            LoadType::Buffer => MIN_TIMEOUT_MS,
            LoadType::Shader => MIN_TIMEOUT_MS * 8,
            LoadType::Mesh => MIN_TIMEOUT_MS * 4,
        }
    }
}

/// 回调订阅句柄（**每个订阅者一个**，合并时各自触发）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackHandle {
    /// 订阅者 ID（**每个订阅者独立**，合并不共享）。
    pub subscriber: u32,
}

/// 加载请求五元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadRequest {
    /// URI（**最长的元**，故 O(URI 长度) 校验）。
    pub uri: Vec<u8>,
    /// 负载类型。
    pub ty: LoadType,
    /// 优先级。
    pub priority: Priority,
    /// 回调订阅句柄。
    pub cb: CallbackHandle,
    /// 超时（毫秒；`0` = 未配 ⇒ 套默认并告警）。
    pub timeout_ms: u32,
}

impl LoadRequest {
    /// 构造一个合规请求（便于判据造语料）。
    pub fn new(
        uri: &[u8],
        ty: LoadType,
        priority: Priority,
        subscriber: u32,
        timeout_ms: u32,
    ) -> LoadRequest {
        LoadRequest {
            uri: uri.to_vec(),
            ty,
            priority,
            cb: CallbackHandle { subscriber },
            timeout_ms,
        }
    }

    /// 五元字段数（**锚点明文五元**：URI/类型/优先级/回调/超时）。
    pub const fn field_count() -> usize {
        5
    }
}

/// 合并键（**三元**：URI + 类型 + 优先级）。
///
/// 少一维即错误合并：少了类型 → 同 URI 的纹理与缓冲合成一个；
/// 少了优先级 → 后到的高优先级订阅被降级成先到者的优先级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeKey {
    /// URI 指纹（**不存字符串**，只存 64 位指纹 ⇒ 合并键定长可比）。
    pub uri_hash: u64,
    /// 类型下标。
    pub ty: u8,
    /// 优先级下标。
    pub priority: u8,
}

impl MergeKey {
    /// 由请求派生合并键（URI 走指纹 ⇒ 定长可比，O(URI 长度)）。
    pub fn of(req: &LoadRequest) -> MergeKey {
        MergeKey {
            uri_hash: uri_hash(&req.uri),
            ty: req.ty.ordinal() as u8,
            priority: req.priority.ordinal() as u8,
        }
    }
}

/// URI 指纹（FNV-1a，与全域哈希口径一致）。
pub fn uri_hash(uri: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < uri.len() {
        h ^= uri[i] as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 四、五级队列（级内 FIFO）
// ---------------------------------------------------------------------------

/// 单级 FIFO 环形队列（**定容、无分配**）。
#[derive(Clone, Debug)]
pub struct FifoQueue {
    slots: [u32; QUEUE_SLOTS],
    /// 写指针。
    head: usize,
    /// 有效条数。
    len: usize,
    /// 累计入队数（**FIFO 判据的计数侧**：出队次序必须等于入队次序）。
    pub enqueued: u64,
    /// 累计出队数。
    pub dequeued: u64,
    /// 满时丢弃数（**不静默丢弃**：满了必须可查）。
    pub dropped: u64,
}

impl FifoQueue {
    /// 空队列。
    pub const fn new() -> FifoQueue {
        FifoQueue {
            slots: [0u32; QUEUE_SLOTS],
            head: 0,
            len: 0,
            enqueued: 0,
            dequeued: 0,
            dropped: 0,
        }
    }

    /// 入队（**O(1)** 尾插；满则丢最旧并计数）。
    pub fn push(&mut self, req_id: u32) -> bool {
        if self.len == QUEUE_SLOTS {
            // 满：丢最旧（覆盖），并计数——覆盖比拒绝更符合「尽快加载」语义，
            // 但**必须可查**，否则丢了几条无从回答。
            self.slots[self.head] = req_id;
            self.head = (self.head + 1) % QUEUE_SLOTS;
            self.dropped = self.dropped.saturating_add(1);
        } else {
            let idx = (self.head + self.len) % QUEUE_SLOTS;
            self.slots[idx] = req_id;
            self.len += 1;
        }
        self.enqueued = self.enqueued.saturating_add(1);
        true
    }

    /// 出队（**O(1)** 头取；空则 [`None`]）。
    pub fn pop(&mut self) -> Option<u32> {
        if self.len == 0 {
            return None;
        }
        let v = self.slots[self.head];
        self.head = (self.head + 1) % QUEUE_SLOTS;
        self.len -= 1;
        self.dequeued = self.dequeued.saturating_add(1);
        Some(v)
    }

    /// 当前条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// 五级优先级队列（**每级一条 FIFO，入队 O(1)**）。
#[derive(Clone, Debug)]
pub struct PriorityQueues {
    queues: [FifoQueue; PRIORITY_LEVELS],
}

impl PriorityQueues {
    /// 五级全空。
    pub fn new() -> PriorityQueues {
        PriorityQueues {
            queues: [
                FifoQueue::new(),
                FifoQueue::new(),
                FifoQueue::new(),
                FifoQueue::new(),
                FifoQueue::new(),
            ],
        }
    }

    /// 入队（**O(1)**，按优先级落到对应级）。
    pub fn push(&mut self, p: Priority, req_id: u32) {
        self.queues[p.ordinal()].push(req_id);
    }

    /// 出队（**按最急级优先取**；同级内 FIFO）。返回 `(优先级, 请求 ID)`。
    ///
    /// 遍历方向固定为 critical→idle（**从最急开始**），保证「critical 先于
    /// high」在跨级场景下也成立；级内顺序由各自的 FIFO 保证。
    ///
    /// 修复记录（VE-F3205 打磨）：初版写作「从末级倒扣到零级」且循环体尾部
    /// 多一句 `i += 1`——于是 (a) Idle 级有货时**先返最闲级**（优先级倒置），
    /// (b) Idle 级为空时下标在末级与越界值之间往复，**无限自旋挂死**
    /// （F3207 的消费入口直接卡住整条加载管线）。本条按锚点「最急级先出」
    /// 改为从零级（critical）正向扫到末级（idle），见判据 Q05-跨级复测。
    pub fn pop(&mut self) -> Option<(Priority, u32)> {
        let mut i = 0usize;
        while i < PRIORITY_LEVELS {
            if let Some(id) = self.queues[i].pop() {
                // `from_ordinal` 对 `i < PRIORITY_LEVELS` 恒 Some，不做 unwrap。
                return match Priority::from_ordinal(i) {
                    Some(p) => Some((p, id)),
                    None => None,
                };
            }
            i += 1;
        }
        None
    }

    /// 某级条数。
    pub fn level_len(&self, p: Priority) -> usize {
        self.queues[p.ordinal()].len()
    }

    /// 某级累计入队数（FIFO 判据的计数侧）。
    pub fn level_enqueued(&self, p: Priority) -> u64 {
        self.queues[p.ordinal()].enqueued
    }

    /// 某级累计出队数。
    pub fn level_dequeued(&self, p: Priority) -> u64 {
        self.queues[p.ordinal()].dequeued
    }

    /// 某级丢弃数（满时覆盖最旧的条数）。
    pub fn level_dropped(&self, p: Priority) -> u64 {
        self.queues[p.ordinal()].dropped
    }

    /// 总条数。
    pub fn total_len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < PRIORITY_LEVELS {
            n = n.saturating_add(self.queues[i].len());
            i += 1;
        }
        n
    }

    /// 结构性容量（五级 × 每级定容槽位）——**恒定值，不随入队条数增长**。
    ///
    /// 这是「入队 O(1) 且不增长内存」的**可观测证据**：判据在灌入 100 条之后
    /// 再问容量，仍须等于这个常量。断容量而非断某个自报的操作数，
    /// 才不会退化成「常量等于自身」的恒真门禁。
    pub fn capacity(&self) -> usize {
        PRIORITY_LEVELS * QUEUE_SLOTS
    }
}

/// 语义通胀检测结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InflationVerdict {
    /// 无通胀。
    None,
    /// 语义通胀：批内全为 critical ⇒ **降级为 normal 的 FIFO** 并告警。
    Inflated,
}

/// 语义通胀检测（**唯一可判定形态：批内全为 critical**）。
///
/// 「都急=都不急」：全 critical 时优先级退化成入队序（随机），
/// 比显式 FIFO 更容易造成饿死，故整批降级为 `normal` 并告警。
///
/// **不按比例**：比例阈值在「批次只有 1 个请求」时必然误报（1/1=100%），
/// 而单请求批次恰恰是常态（启动时逐个加载）。
///
/// **且批内须多于一条**：只有一条请求时，它「既是 critical 也是全部」是
/// 平凡事实而非通胀——把它降级等于让启动阶段的第一个请求凭空降到 normal。
/// 「退化」的前提是**多个互相矛盾的声明**，一条声明不构成退化。
pub fn detect_inflation(q: &PriorityQueues) -> InflationVerdict {
    let total = q.total_len();
    if total < 2 {
        // 单条（或空）不构成通胀
        return InflationVerdict::None;
    }
    let critical = q.level_len(Priority::Critical);
    if critical == total {
        return InflationVerdict::Inflated;
    }
    InflationVerdict::None
}

/// 语义通胀降级：把整批 critical 降为 normal（**返回降级条数**）。
pub fn degrade_inflation(q: &mut PriorityQueues) -> usize {
    // 走 `Priority::ordinal()` 而非硬编码 0/2：优先级是闭集，
    // 两个槽位与两个级别名**双向绑定**（判据 G6 钉死），
    // 日后调整枚举顺序时此处不会静默降错级。
    let from = Priority::Critical.ordinal();
    let to = Priority::Normal.ordinal();
    let n = q.level_len(Priority::Critical);
    let mut i = 0u32;
    while (i as usize) < n {
        // 逐条从 critical 移到 normal：保持**级内原次序**（FIFO 语义不破）。
        if let Some(id) = q.queues[from].pop() {
            q.queues[to].push(id);
        }
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 五、请求合并（同键→ 单请求多订阅）
// ---------------------------------------------------------------------------

/// 合并表条目。
#[derive(Clone, Copy, Debug)]
pub struct MergeEntry {
    /// 合并键。
    pub key: MergeKey,
    /// 首个请求 ID（**实际加载的是它**）。
    pub primary: u32,
    /// 订阅者计数（含首个）。
    pub subscribers: u32,
}

/// 合并表（**线性小表，查表 O(1)**——表长上限固定 64）。
#[derive(Clone, Debug)]
pub struct MergeTable {
    entries: [Option<MergeEntry>; MERGE_SLOTS],
    /// 表长（**单调增长的入口就是泄漏**，判据直接断这个）。
    pub len: usize,
    /// 命中次数（合并生效的次数，可查）。
    pub hits: u64,
    /// 泄漏闸触发次数（订阅归零却未回收的次数，**必须为 0**）。
    pub leak_alarms: u64,
}

impl MergeTable {
    /// 空表。
    pub fn new() -> MergeTable {
        MergeTable {
            entries: [None; MERGE_SLOTS],
            len: 0,
            hits: 0,
            leak_alarms: 0,
        }
    }

    /// 查找条目下标（线性小表，**O(1)** 因表长上限固定）。
    fn find(&self, key: &MergeKey) -> Option<usize> {
        let mut i = 0usize;
        while i < MERGE_SLOTS {
            if let Some(e) = self.entries[i] {
                if e.key == *key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 首空闲槽（表满返回 [`None`]）。
    fn free_slot(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < MERGE_SLOTS {
            if self.entries[i].is_none() {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 订阅一个请求。
    ///
    /// 返回 `(是否合并到已有请求, 该键的主请求 ID)`：
    /// - `(false, id)`：新建条目，`id` 是主请求；
    /// - `(true, primary)`：并入已有请求，**订阅者计数加一**，
    ///   调用方**仍须登记自己的回调**——丢弃后到者的回调是最常见的实现偷懒。
    pub fn subscribe(&mut self, key: &MergeKey, req_id: u32) -> (bool, u32) {
        if let Some(idx) = self.find(key) {
            if let Some(e) = self.entries[idx].as_mut() {
                e.subscribers = e.subscribers.saturating_add(1);
            }
            self.hits = self.hits.saturating_add(1);
            let primary = match self.entries[idx] {
                Some(e) => e.primary,
                None => 0,
            };
            return (true, primary);
        }
        // 新建
        match self.free_slot() {
            Some(slot) => {
                self.entries[slot] = Some(MergeEntry {
                    key: *key,
                    primary: req_id,
                    subscribers: 1,
                });
                self.len += 1;
                (false, req_id)
            }
            None => {
                // 表满：**不静默丢弃**，按「新建不入表」处理并让调用方照常加载。
                // 合并是优化，不是正确性依赖——表满退化成「不合并」仍须正确。
                (false, req_id)
            }
        }
    }

    /// 退订并**回收**条目（**泄漏闸**）。
    ///
    /// 订阅计数归零时条目**必须消失**——否则合并表单调增长吃光内存。
    ///
    /// **超额退订（对已回收的键再退）必须可观测**：调用方多退一次意味着它的
    /// 生命周期记账与本表不一致；若静默忽略，该漂移永远查不出来。由于计数归零
    /// 的条目**当场被删**，再次退订必然走「键不存在」这条路径，故告警计数
    /// [`MergeTable::leak_alarms`] 在此**可达**——初版把递增写进了不可达分支，
    /// 等于把泄漏闸焊成恒真门禁（判据断`== 0` 于是恒绿，闸门形同虚设）。
    pub fn unsubscribe(&mut self, key: &MergeKey) -> bool {
        let idx = match self.find(key) {
            Some(i) => i,
            // 表里已无此键：只可能是超额退订（键已随计数归零被回收）。
            None => {
                self.leak_alarms = self.leak_alarms.saturating_add(1);
                return false;
            }
        };
        if let Some(e) = self.entries[idx].as_mut() {
            if e.subscribers > 0 {
                e.subscribers -= 1;
            }
        }
        // 归零 ⇒ 当场回收（**不留僵尸槽**，表长随之回落）。
        let recycle = match self.entries[idx] {
            Some(e) => e.subscribers == 0,
            None => false,
        };
        if recycle {
            self.entries[idx] = None;
            self.len -= 1;
        }
        true
    }

    /// 某键的订阅者计数（`0` 表示无条目）。
    pub fn subscribers_of(&self, key: &MergeKey) -> u32 {
        match self.find(key) {
            Some(i) => match self.entries[i] {
                Some(e) => e.subscribers,
                None => 0,
            },
            None => 0,
        }
    }

    /// 某键是否存在条目。
    pub fn contains(&self, key: &MergeKey) -> bool {
        self.find(key).is_some()
    }
}

// ---------------------------------------------------------------------------
// 六、前置校验（拦截脏请求·三要素）
// ---------------------------------------------------------------------------

/// 前置拒绝变体（**各带原因与建议动作**，锚点「三要素」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectKind {
    /// URI 为空。
    EmptyUri,
    /// URI 超长。
    UriTooLong,
    /// URI 含非法字符（控制符/ 空格 /反斜杠）。
    IllegalChar,
    /// 超时越界（含「未配」之外的真实越界）。
    TimeoutOutOfRange,
}

impl RejectKind {
    /// 全枚举。
    pub const ALL: [RejectKind; 4] = [
        RejectKind::EmptyUri,
        RejectKind::UriTooLong,
        RejectKind::IllegalChar,
        RejectKind::TimeoutOutOfRange,
    ];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            RejectKind::EmptyUri => 0,
            RejectKind::UriTooLong => 1,
            RejectKind::IllegalChar => 2,
            RejectKind::TimeoutOutOfRange => 3,
        }
    }

    /// 线编码（显式映射）。
    pub const fn wire(self) -> u8 {
        match self {
            RejectKind::EmptyUri => 0xD0,
            RejectKind::UriTooLong => 0xD1,
            RejectKind::IllegalChar => 0xD2,
            RejectKind::TimeoutOutOfRange => 0xD3,
        }
    }

    /// 原因（**三要素之一**）。
    pub const fn cause(self) -> &'static str {
        match self {
            RejectKind::EmptyUri => "URI 为空 / empty URI",
            RejectKind::UriTooLong => "URI 超长 / URI too long",
            RejectKind::IllegalChar => "URI 含非法字符 / illegal char in URI",
            RejectKind::TimeoutOutOfRange => "超时越界 / timeout out of range",
        }
    }

    /// 建议动作（**三要素之二**）。
    pub const fn advice(self) -> &'static str {
        match self {
            RejectKind::EmptyUri => "补齐资源路径 / supply the resource path",
            RejectKind::UriTooLong => "改用更短的资源标识 / shorten the resource id",
            RejectKind::IllegalChar => "去掉控制符、空格与反斜杠 / strip control chars, space, backslash",
            RejectKind::TimeoutOutOfRange => "把超时设到 16..600000ms 区间 / set timeout within 16..600000ms",
        }
    }

    /// 可读串（**三要素之三**：原因 + 建议合成可读串）。
    pub fn readable(self) -> String {
        format!("{} -> {}", self.cause(), self.advice())
    }
}

/// 前置校验结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidateVerdict {
    /// 通过（**已套用默认超时则标记 used_default**）。
    Ok { used_default: bool },
    /// 拒绝（**不进队列**）。
    Rejected(RejectKind),
}

/// 请求前置校验（**O(URI 长度)**，锚点「前置拦截」）。
///
/// 顺序：空→ 超长 → 非法字符 → 超时。**先判 URI**是因为它最长最便宜，
/// 且URI 非法时谈超时没有意义。
pub fn validate_request(req: &LoadRequest) -> ValidateVerdict {
    // 1) 空 URI
    if req.uri.is_empty() {
        return ValidateVerdict::Rejected(RejectKind::EmptyUri);
    }
    // 2) 超长
    if req.uri.len() > URI_MAX_LEN {
        return ValidateVerdict::Rejected(RejectKind::UriTooLong);
    }
    // 3) 非法字符：控制符( <0x20 或 0x7F)、空格、反斜杠。
    let mut i = 0usize;
    while i < req.uri.len() {
        let b = req.uri[i];
        if b < 0x20 || b == 0x7F || b == b' ' || b == b'\\' {
            return ValidateVerdict::Rejected(RejectKind::IllegalChar);
        }
        i += 1;
    }
    // 4) 超时：`0` 视为未配 ⇒ 套默认并告警（**不当作无限等待**）；
    //    非0 但越界 ⇒ 拒绝。
    if req.timeout_ms == 0 {
        return ValidateVerdict::Ok { used_default: true };
    }
    if req.timeout_ms < MIN_TIMEOUT_MS || req.timeout_ms > MAX_TIMEOUT_MS {
        return ValidateVerdict::Rejected(RejectKind::TimeoutOutOfRange);
    }
    // 类型相关的最小超时（着色器 1ms 必超时 —— 前置拦住，不让它进队列白占槽）
    if req.timeout_ms < req.ty.min_timeout_ms() {
        return ValidateVerdict::Rejected(RejectKind::TimeoutOutOfRange);
    }
    ValidateVerdict::Ok { used_default: false }
}

// ---------------------------------------------------------------------------
// 七、装载门面（校验 → 合并 → 入队，含通胀降级与超时告警）
// ---------------------------------------------------------------------------

/// 装载计数（各口径分开，便于判据逐项钉死）。
#[derive(Clone, Debug, Default)]
pub struct LoadStats {
    /// 前置拒绝次数。
    pub rejected: u64,
    /// 合并命中次数。
    pub merged: u64,
    /// 入队次数。
    pub enqueued: u64,
    /// 语义通胀告警次数。
    pub inflation_alarms: u64,
    /// 超时未配置告警次数。
    pub default_timeout_alarms: u64,
    /// 合并表满 ⇒ 键未入表（降级为不合并）的次数。
    ///
    /// 没有这个计数，表满路径之后 `release` 会把「本就没入表的键」当成
    /// 记账漂移打泄漏告警——**降级被误报成泄漏**，闸门信号从此不可信。
    /// 有了它，泄漏告警可与 `not_tabled` 逐条对账（见判据 30）。
    pub not_tabled: u64,
}

/// 装载结果（**合并时必须区分：谁是主请求**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admit {
    /// 新建并入队（携带最终生效的优先级与超时）。
    Enqueued { priority: Priority, timeout_ms: u32 },
    /// 合并到已有请求（**本请求不占队列槽**，但回调仍须单独登记）。
    Merged { primary: u32 },
    /// 前置拒绝（**未入队**）。
    Rejected(RejectKind),
}

/// 资源加载请求侧门面。
#[derive(Clone, Debug)]
pub struct LoadGate {
    queues: PriorityQueues,
    merge: MergeTable,
    /// 请求 ID 分配器（单调递增）。
    next_id: u32,
    /// 统计。
    pub stats: LoadStats,
}

impl LoadGate {
    /// 新建门面。
    pub fn new() -> LoadGate {
        LoadGate {
            queues: PriorityQueues::new(),
            merge: MergeTable::new(),
            next_id: 1,
            stats: LoadStats::default(),
        }
    }

    /// 提交一个请求（**完整流水线**）。
    ///
    /// 顺序：前置校验 →（超时未配则套默认并告警）→ 合并 → 入队。
    /// **拒绝的请求绝不进队列**（脏请求红线）。
    pub fn admit(&mut self, req: &LoadRequest) -> Admit {
        // 1) 前置校验
        let mut timeout = req.timeout_ms;
        match validate_request(req) {
            ValidateVerdict::Rejected(k) => {
                self.stats.rejected = self.stats.rejected.saturating_add(1);
                return Admit::Rejected(k);
            }
            ValidateVerdict::Ok { used_default: true } => {
                timeout = DEFAULT_TIMEOUT_MS;
                self.stats.default_timeout_alarms =
                    self.stats.default_timeout_alarms.saturating_add(1);
            }
            ValidateVerdict::Ok { used_default: false } => {}
        }
        // 2) 合并（键含类型与优先级 ⇒ 不会把不同诉求合成一个）
        let key = MergeKey::of(req);
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let (merged, primary) = self.merge.subscribe(&key, id);
        if merged {
            self.stats.merged = self.stats.merged.saturating_add(1);
            return Admit::Merged { primary };
        }
        if !self.merge.contains(&key) {
            // subscribe 返回「新建」但表里查不到此键 ⇒ 合并表满，
            // 本请求降级为不合并（仍须正常入队加载）。记 not_tabled：
            // 事后 release 的泄漏告警靠它逐条对账（降级≠泄漏）。
            self.stats.not_tabled = self.stats.not_tabled.saturating_add(1);
        }
        // 3) 入队
        self.queues.push(req.priority, id);
        self.stats.enqueued = self.stats.enqueued.saturating_add(1);
        Admit::Enqueued {
            priority: req.priority,
            timeout_ms: timeout,
        }
    }

    /// 退订并回收合并表条目（**泄漏闸**）。
    pub fn release(&mut self, req: &LoadRequest) -> bool {
        let key = MergeKey::of(req);
        self.merge.unsubscribe(&key)
    }

    /// 弹出下一个待处理请求（**F3207 的消费入口**）。
    pub fn pop(&mut self) -> Option<(Priority, u32)> {
        self.queues.pop()
    }

    /// 语义通胀检测 + 降级（**降级为 normal 的 FIFO 并告警**）。
    ///
    /// 返回是否发生了通胀（调用方据此告警上报）。
    pub fn apply_inflation_guard(&mut self) -> bool {
        match detect_inflation(&self.queues) {
            InflationVerdict::None => false,
            InflationVerdict::Inflated => {
                degrade_inflation(&mut self.queues);
                self.stats.inflation_alarms = self.stats.inflation_alarms.saturating_add(1);
                true
            }
        }
    }

    /// 合并表条目数（**泄漏判据直接断这个**）。
    pub fn merge_len(&self) -> usize {
        self.merge.len
    }

    /// 合并表泄漏告警数（**必须恒为 0**）。
    pub fn leak_alarms(&self) -> u64 {
        self.merge.leak_alarms
    }

    /// 合并命中次数。
    pub fn merge_hits(&self) -> u64 {
        self.merge.hits
    }

    /// 某键的订阅者计数（**合并后每个订阅者都应可查**）。
    pub fn subscribers_of(&self, req: &LoadRequest) -> u32 {
        self.merge.subscribers_of(&MergeKey::of(req))
    }

    /// 某键的条目是否仍在合并表中（**回收的证据不能只看计数**——
    /// 计数归零而条目仍在，正是泄漏闸失守的形态）。
    pub fn merge_contains(&self, req: &LoadRequest) -> bool {
        self.merge.contains(&MergeKey::of(req))
    }

    /// 总队列条数。
    pub fn queued(&self) -> usize {
        self.queues.total_len()
    }

    /// 队列结构性容量（**不随入队增长**，见 [`PriorityQueues::capacity`]）。
    pub fn queue_capacity(&self) -> usize {
        self.queues.capacity()
    }

    /// 某级条数。
    pub fn level_len(&self, p: Priority) -> usize {
        self.queues.level_len(p)
    }

    /// 某级累计入队数。
    pub fn level_enqueued(&self, p: Priority) -> u64 {
        self.queues.level_enqueued(p)
    }

    /// 某级丢弃数（**满时覆盖最旧的条数，必须可查**）。
    pub fn level_dropped(&self, p: Priority) -> u64 {
        self.queues.level_dropped(p)
    }

    /// 读屏面板（**只报聚合计数**）。
    pub fn a11y_lines(&self) -> [String; 7] {
        [
            format!("拒绝 / rejected: {}", self.stats.rejected),
            format!("合并命中 / merged: {}", self.stats.merged),
            format!("入队 / enqueued: {}", self.stats.enqueued),
            format!("语义通胀告警 / inflation alarms: {}", self.stats.inflation_alarms),
            format!(
                "超时默认告警 / default-timeout alarms: {}",
                self.stats.default_timeout_alarms
            ),
            format!(
                "合并表条目 / merge entries: {} (泄漏闸 {} )",
                self.merge.len, self.merge.leak_alarms
            ),
            format!("合并表满未入表 / not tabled: {}", self.stats.not_tabled),
        ]
    }
}

// ---------------------------------------------------------------------------
// 八、性能逐项分解
// ---------------------------------------------------------------------------

/// 性能预算（锚点「性能逐项分解」：入队 O(1)、合并 O(1) 查表、校验 O(URI 长)、
/// 告警 O(1)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfBudget {
    /// 入队 O(1)。
    pub enqueue_ops: u32,
    /// 合并查表 O(1)。
    pub merge_ops: u32,
    /// 校验按 URI 长度线性。
    pub validate_uri_scan: u32,
    /// 告警 O(1)。
    pub alarm_ops: u32,
}

impl PerfBudget {
    /// 锚点声明的复杂度（**用于判据钉死「哪步是 O(1)」**）。
    pub const fn declared() -> PerfBudget {
        PerfBudget {
            enqueue_ops: 1,
            merge_ops: 1,
            validate_uri_scan: 1, // 按 URI 长度线性 ⇒ 单位操作记 1
            alarm_ops: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// 九、判据
// ---------------------------------------------------------------------------

/// VE-F3205 模块自检（逐条覆盖锚点六项判据）。
pub fn run_veq05_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F3205 加载请求与优先级");

    // --- 判据 1：规格常量逐字锚定 ---------------------------------------
    s.add(
        "Q05-规格-五级队列与常量逐字一致",
        PRIORITY_LEVELS == 5
            && QUEUE_SLOTS == 64
            && MERGE_SLOTS == 64
            && URI_MAX_LEN == 512
            && DEFAULT_TIMEOUT_MS == 30_000
            && MIN_TIMEOUT_MS == 16
            && MAX_TIMEOUT_MS == 600_000
            && LOAD_TYPE_COUNT == 4
            && REJECT_COUNT == 4
            && LoadRequest::field_count() == 5,
        "五级优先级、五元请求、64 槽队列与合并表、URI 512 上限、超时 16..600000ms 默认 30000ms",
    );

    // --- 判据 2：**五级语义显性** ------------------------------------------
    {
        let mut ok = true;
        let mut wires: Vec<u8> = Vec::new();
        let mut ords: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < Priority::ALL.len() {
            let p = Priority::ALL[i];
            // 语义**每级都有且互不相同**（语义显性：可答「这级什么意思」）
            ok &= !p.semantics().is_empty();
            ok &= !wires.contains(&p.wire());
            wires.push(p.wire());
            ok &= !ords.contains(&p.ordinal());
            ords.push(p.ordinal());
            // 线编码往返自洽、且**不等于判别值**（改枚举顺序不静默改协议）
            ok &= Priority::from_wire(p.wire()) == Some(p);
            ok &= p.wire() != p.ordinal() as u8;
            i += 1;
        }
        // 语义互异：五级不能共用同一句话（否则「语义显性」是空话）
        let sems_differ = Priority::Critical.semantics() != Priority::High.semantics()
            && Priority::High.semantics() != Priority::Normal.semantics()
            && Priority::Normal.semantics() != Priority::Low.semantics()
            && Priority::Low.semantics() != Priority::Idle.semantics();
        s.add(
            "Q05-五级-闭集五级语义显性且线编码自洽",
            ok
                && sems_differ
                && Priority::ALL.len() == 5
                // 未知码返回 None（不静默夹到末级）
                && Priority::from_wire(0x00).is_none()
                && Priority::from_ordinal(5).is_none()
                && Priority::from_ordinal(99).is_none()
                // 序关系：critical 最急、idle 最闲
                && Priority::Critical < Priority::Idle
                && Priority::Idle.is_critical() == false
                && Priority::Critical.is_critical(),
            "critical/high/normal/low/idle 五级语义互异可查，线编码往返自洽且不等于判别值，未知码与越界下标返回 None，序关系 critical<idle",
        );
    }

    // --- 判据 3：**级内 FIFO**（出队次序 == 入队次序） -------------------
    {
        let mut q = PriorityQueues::new();
        //同一级按 0..8 顺序入队
        let mut i = 0u32;
        while i < 8 {
            q.push(Priority::Normal, i);
            i += 1;
        }
        // 出队必须严格 0,1,2,...,7
        let mut order_ok = true;
        let mut k = 0u32;
        while k < 8 {
            match q.pop() {
                Some((p, id)) => {
                    order_ok &= id == k && p == Priority::Normal;
                }
                None => order_ok = false,
            }
            k += 1;
        }
        // 反向对照：空队列 pop 给 None（不 panic、不给脏数据）
        let empty_ok = q.pop().is_none();
        s.add(
            "Q05-级内FIFO-出队次序等于入队次序",
            order_ok
                && empty_ok
                && q.level_enqueued(Priority::Normal) == 8
                && q.level_dequeued(Priority::Normal) == 8
                && q.total_len() == 0,
            "同一级按 0..8 顺序入队 0..8 出队严格同序（级内 FIFO 不排序）；空队列 pop 返回 None；入队/出队计数各恰为 8",
        );
    }

    // --- 判据 4：跨级优先（critical 先于 high，级内仍FIFO） --------------
    {
        let mut q = PriorityQueues::new();
        // 故意**逆优先级**入队：先 high 再 critical
        q.push(Priority::High, 1);
        q.push(Priority::Critical, 2);
        q.push(Priority::Idle, 3);
        q.push(Priority::Normal, 4);
        let mut got = [0u8; 4];
        let mut i = 0usize;
        while i < 4 {
            if let Some((_, id)) = q.pop() {
                got[i] = id as u8;
            }
            i += 1;
        }
        s.add(
            "Q05-跨级-最急级先出且与入队序无关",
            got[0] == 2
                && got[1] == 1
                && got[2] == 4
                && got[3] == 3
                && q.total_len() == 0,
            "入队序 high,critical,idle,normal ⇒ 出队序 critical(2),high(1),normal(4),idle(3)：跨级按优先级、级内按 FIFO",
        );
    }

    // --- 判据 5：**请求合并**（同 URI 同参 → 单请求多订阅） --------------
    {
        let mut g = LoadGate::new();
        // 同 URI + 同类型 + 同优先级 ⇒应合并
        let a = LoadRequest::new(b"tex/a.png", LoadType::Texture, Priority::Normal, 1, 1000);
        let b = LoadRequest::new(b"tex/a.png", LoadType::Texture, Priority::Normal, 2, 1000);
        let r1 = g.admit(&a);
        let r2 = g.admit(&b);
        // 订阅者计数应为 2，且队列只占1 槽
        let subs = g.subscribers_of(&a);
        s.add(
            "Q05-合并-同键合并为单请求多订阅",
            r1 == Admit::Enqueued { priority: Priority::Normal, timeout_ms: 1000 }
                && r2 == Admit::Merged { primary: 1 }
                // 两个订阅者各自可查（**后到者的回调不被丢弃**）
                && subs == 2
                // 只占一个队列槽
                && g.queued() == 1
                && g.merge_len() == 1
                && g.merge_hits() == 1
                && g.level_enqueued(Priority::Normal) == 1,
            "同 URI 同类型同优先级的第二个请求合并进已有请求（primary=1），订阅者计数为 2、队列只占 1 槽、合并命中 1 次",
        );
    }

    // --- 判据 6：合并键**三维缺一即错误合并** ---------------------------
    {
        let mut g = LoadGate::new();
        let base = LoadRequest::new(b"res/x", LoadType::Texture, Priority::Normal, 1, 1000);
        g.admit(&base);
        // 仅类型不同 ⇒ 不合并
        let diff_ty = LoadRequest::new(b"res/x", LoadType::Buffer, Priority::Normal, 2, 1000);
        // 仅优先级不同 ⇒ 不合并
        let diff_pri = LoadRequest::new(b"res/x", LoadType::Texture, Priority::High, 3, 1000);
        let r_ty = g.admit(&diff_ty);
        let r_pri = g.admit(&diff_pri);
        s.add(
            "Q05-合并键-类型或优先级不同即不合并",
            r_ty == Admit::Enqueued { priority: Priority::Normal, timeout_ms: 1000 }
                && r_pri == Admit::Enqueued { priority: Priority::High, timeout_ms: 1000 }
                // 三条独立条目（三键各自不同）
                && g.merge_len() == 3
                && g.queued() == 3,
            "同 URI 但类型不同、优先级不同的请求各自独立入队（三键缺一即错误合并：少类型会把纹理与缓冲合成一个，少优先级会把后到的critical 降级）",
        );
    }

    // --- 判据 7：合并表**泄漏闸**（订阅归零必须回收条目） ---------------
    {
        let mut g = LoadGate::new();
        let a = LoadRequest::new(b"res/leak", LoadType::Texture, Priority::Normal, 1, 1000);
        let b = LoadRequest::new(b"res/leak", LoadType::Texture, Priority::Normal, 2, 1000);
        g.admit(&a);
        g.admit(&b);
        let before = g.merge_len();
        // 只退订一次 ⇒ 订阅者还有 1 个，条目**仍在**
        let r1 = g.release(&a);
        let mid = g.merge_len();
        let subs_mid = g.subscribers_of(&a);
        // 退订第二次 ⇒ 归零，条目**必须消失**
        let r2 = g.release(&b);
        let after = g.merge_len();
        s.add(
            "Q05-泄漏闸-订阅归零即回收条目",
            before == 1
                && r1 && r2
                && mid == 1
                && subs_mid == 1
                // **核心**：归零后条目真的消失，而不只是计数归零。
                // 注意此处必须写 `== 0` 而非 `!x > 0`：`!u32` 得到的是
                // 按位取反（`!0u32 == u32::MAX`，恒 `> 0`），那样写等于
                // 断了一个恒真式子，订阅数是1 还是 99 都照样绿。
                && after == 0
                && g.subscribers_of(&a) == 0
                && !g.merge_contains(&a)
                // 正常用法下泄漏告警恒为 0（可达性由判据 23 反向证明）
                && g.leak_alarms() == 0,
            "两个订阅者退订一个后条目仍在（订阅数 1），退订第二个后条目被回收、表长归零、泄漏告警为 0",
        );
    }

    // --- 判据 8：**语义通胀**（全critical → 降级 + 告警） ---------------
    {
        let mut g = LoadGate::new();
        // 4 个请求全critical。
        // URI 必须**各不相同**——同 URI 会走合并只入队 1 条，
        // 那样测的就不是「批内四条都是 critical」而是「一条 critical」。
        let mut i = 0u32;
        while i < 4 {
            let uri = match i {
                0 => &b"res/inf0"[..],
                1 => &b"res/inf1"[..],
                2 => &b"res/inf2"[..],
                _ => &b"res/inf3"[..],
            };
            g.admit(&LoadRequest::new(
                uri,
                LoadType::Texture,
                Priority::Critical,
                i + 1,
                1000,
            ));
            i += 1;
        }
        let crit_before = g.level_len(Priority::Critical);
        let fired = g.apply_inflation_guard();
        s.add(
            "Q05-语义通胀-全critical降级为FIFO并告警",
            crit_before == 4
                && fired
                // 降级后 critical 清空、normal 拿到全部 4 条
                && g.level_len(Priority::Critical) == 0
                && g.level_len(Priority::Normal) == 4
                && g.stats.inflation_alarms == 1,
            "批内4 个全 critical 触发语义通胀：整批降级为 normal 的 FIFO，critical 清空、告警 1 次（都急=都不急）",
        );
    }

    // --- 判据 9：通胀降级**保序**（降级不破级内 FIFO） -------------------
    {
        let mut q = PriorityQueues::new();
        q.push(Priority::Critical, 10);
        q.push(Priority::Critical, 11);
        q.push(Priority::Critical, 12);
        degrade_inflation(&mut q);
        // 降级后 normal 级内必须仍 10,11,12
        let mut ids = [0u32; 3];
        let mut i = 0usize;
        while i < 3 {
            if let Some((_, id)) = q.pop() {
                ids[i] = id;
            }
            i += 1;
        }
        s.add(
            "Q05-通胀降级-保序不破级内FIFO",
            ids[0] == 10 && ids[1] == 11 && ids[2] == 12,
            "三个 critical 降级到 normal 后出队仍是 10,11,12：降级只换级不重排，级内 FIFO 语义不破",
        );
    }

    // --- 判据 10：**反向对照**——非全critical 不触发通胀 ------------------
    {
        let mut g = LoadGate::new();
        // 只有 1 个critical（1/1 = 100%，但**不是**通胀）
        g.admit(&LoadRequest::new(b"res/one", LoadType::Texture, Priority::Critical, 1, 1000));
        let fired1 = g.apply_inflation_guard();
        // 再加一个 normal ⇒ 混排，仍不触发
        g.admit(&LoadRequest::new(b"res/two", LoadType::Texture, Priority::Normal, 2, 1000));
        let fired2 = g.apply_inflation_guard();
        s.add(
            "Q05-通胀判定-混排与单请求不误报",
            fired1 == false
                && fired2 == false
                && g.stats.inflation_alarms == 0
                && g.level_len(Priority::Critical) == 1,
            "单请求批次（1/1=100%）与 critical+normal 混排均不判通胀——比例阈值在此必然误报，故按「全critical」这一唯一可判定形态判",
        );
    }

    // --- 判据 11：**前置拦截**（非法 URI 不进队列 + 三要素） ------------
    {
        let mut g = LoadGate::new();
        // 空 URI
        let empty = LoadRequest::new(b"", LoadType::Texture, Priority::Normal, 1, 1000);
        // 含空格
        let spaced = LoadRequest::new(b"tex/a b.png", LoadType::Texture, Priority::Normal, 2, 1000);
        // 含反斜杠
        let bslash = LoadRequest::new(b"tex\\a.png", LoadType::Texture, Priority::Normal, 3, 1000);
        // 超长 URI
        let mut long = [b'a'; 600];
        long[0] = b't';
        let toolong = LoadRequest::new(&long, LoadType::Texture, Priority::Normal, 4, 1000);
        let r1 = g.admit(&empty);
        let r2 = g.admit(&spaced);
        let r3 = g.admit(&bslash);
        let r4 = g.admit(&toolong);
        s.add(
            "Q05-前置拦截-脏请求拒绝且不入队列",
            r1 == Admit::Rejected(RejectKind::EmptyUri)
                && r2 == Admit::Rejected(RejectKind::IllegalChar)
                && r3 == Admit::Rejected(RejectKind::IllegalChar)
                && r4 == Admit::Rejected(RejectKind::UriTooLong)
                // **核心**：一个都没进队列（脏请求红线）
                && g.queued() == 0
                && g.merge_len() == 0
                && g.stats.rejected == 4,
            "空URI/含空格/含反斜杠/超长四类脏请求全部前置拒绝，队列与合并表均为空（不进队列——脏请求一旦入队会被消费、占带宽、触发回调）",
        );
    }

    // --- 判据 12：拒绝**三要素**（原因 + 建议 + 可读串） ----------------
    {
        let mut ok = true;
        let mut causes: Vec<&str> = Vec::new();
        let mut advices: Vec<&str> = Vec::new();
        let mut i = 0usize;
        while i < RejectKind::ALL.len() {
            let k = RejectKind::ALL[i];
            ok &= !k.cause().is_empty() && !k.advice().is_empty();
            let mut j = 0usize;
            while j < causes.len() {
                // 原因与建议都不得复用（复用会让「三要素」变成同一句话）
                ok &= causes[j] != k.cause() && advices[j] != k.advice();
                j += 1;
            }
            causes.push(k.cause());
            advices.push(k.advice());
            // 线编码互异且不等于判别值
            ok &= k.wire() != k.ordinal() as u8;
            i += 1;
        }
        let readable = RejectKind::EmptyUri.readable();
        s.add(
            "Q05-拒绝三要素-原因与建议互异且可读串齐备",
            ok
                && RejectKind::ALL.len() == REJECT_COUNT
                && readable.contains("->")
                && readable.contains(RejectKind::EmptyUri.advice())
                && readable.contains(RejectKind::EmptyUri.cause()),
            "四个拒绝变体的原因与建议两两互异，可读串含『原因 -> 建议』两段（只说失败不说怎么办等于把排查成本转嫁给调用方）",
        );
    }

    // --- 判据 13：校验顺序（URI 非法时不看超时） ------------------------
    {
        // URI 非法 **且** 超时也非法 ⇒ 报URI 的问题
        let bad_both = LoadRequest::new(b"", LoadType::Shader, Priority::Normal, 1, 1);
        let v = validate_request(&bad_both);
        s.add(
            "Q05-校验顺序-URI非法优先于超时",
            v == ValidateVerdict::Rejected(RejectKind::EmptyUri),
            "URI 为空且超时 1ms（对Shader 必然越界）时仍报『URI 为空』——先判URI（最长最便宜），URI 非法时谈超时没意义",
        );
    }

    // --- 判据 14：超时**未配** ⇒ 默认 + 告警（非无限等待） -------------
    {
        let mut g = LoadGate::new();
        // 超时 0 = 未配
        let no_timeout = LoadRequest::new(b"res/noto", LoadType::Texture, Priority::Normal, 1, 0);
        let v = validate_request(&no_timeout);
        let r = g.admit(&no_timeout);
        s.add(
            "Q05-超时未配-套默认并告警而非无限等待",
            v == ValidateVerdict::Ok { used_default: true }
                && r == Admit::Enqueued { priority: Priority::Normal, timeout_ms: DEFAULT_TIMEOUT_MS }
                && g.stats.default_timeout_alarms == 1
                // 反向：配了合法超时则**不告警**
                && {
                    let mut g2 = LoadGate::new();
                    let ok_to = LoadRequest::new(b"res/ok", LoadType::Texture, Priority::Normal, 1, 1000);
                    g2.admit(&ok_to);
                    g2.stats.default_timeout_alarms == 0
                },
            "超时 0 视作未配：套默认 30000ms 并告警 1 次（当作无限等待会让忘配的请求永久占住队列槽）；合法超时则不告警",
        );
    }

    // --- 判据 15：超时**越界**双向（过小/过大皆拒，边界可过） -----------
    {
        let mut g = LoadGate::new();
        let too_small = LoadRequest::new(b"res/ts", LoadType::Texture, Priority::Normal, 1, 5);
        let too_big = LoadRequest::new(b"res/tb", LoadType::Texture, Priority::Normal, 2, 999_999);
        let r1 = g.admit(&too_small);
        let r2 = g.admit(&too_big);
        // 反向：恰好等于下界（对 Texture 而言 min=16）应通过
        let edge = LoadRequest::new(b"res/te", LoadType::Texture, Priority::Normal, 3, MIN_TIMEOUT_MS);
        let r3 = g.admit(&edge);
        s.add(
            "Q05-超时区间-双边夹逼且边界可过",
            r1 == Admit::Rejected(RejectKind::TimeoutOutOfRange)
                && r2 == Admit::Rejected(RejectKind::TimeoutOutOfRange)
                // **反向**：恰等于下界必须可过（夹逼对钉死界位置）
                && r3 == Admit::Enqueued { priority: Priority::Normal, timeout_ms: MIN_TIMEOUT_MS }
                && g.queued() == 1,
            "超时 5ms 与 999999ms 均拒；恰为下界 16ms 通过（夹逼对：不误杀边界值）",
        );
    }

    // --- 判据 16：类型相关最小超时（前置拦住必超时的请求） ------------
    {
        // Shader 要求 ≥128ms，给 20ms 必超时 ⇒ 前置就拦
        let shader_too_fast = LoadRequest::new(b"res/s", LoadType::Shader, Priority::Normal, 1, 20);
        let v = validate_request(&shader_too_fast);
        // Texture 同样 20ms 则可过（min=16）
        let tex_ok = LoadRequest::new(b"res/t", LoadType::Texture, Priority::Normal, 2, 20);
        let v2 = validate_request(&tex_ok);
        s.add(
            "Q05-类型最小超时-按类型差异拦截",
            v == ValidateVerdict::Rejected(RejectKind::TimeoutOutOfRange)
                && v2 == ValidateVerdict::Ok { used_default: false }
                && LoadType::Shader.min_timeout_ms() > LoadType::Texture.min_timeout_ms(),
            "同一 20ms 超时：Shader 被拒（编译慢，min=128ms）、Texture 通过（min=16ms）——类型相关的最小超时前置拦住必超时的请求，不让它白占队列槽",
        );
    }

    // --- 判据 17：五元齐备（请求五字段可逐项核） ----------------------
    {
        let r = LoadRequest::new(b"res/five", LoadType::Mesh, Priority::High, 77, 2000);
        s.add(
            "Q05-请求五元-URI类型优先级回调超时齐备",
            r.uri == b"res/five".to_vec()
                && r.ty == LoadType::Mesh
                && r.priority == Priority::High
                && r.cb.subscriber == 77
                && r.timeout_ms == 2000
                && LoadRequest::field_count() == 5,
            "请求五元逐项可核：URI 字节、负载类型、优先级、回调订阅者 ID、超时毫秒",
        );
    }

    // --- 判据 18：负载类型闭集（线编码自洽+ 未知码 None） --------------
    {
        let mut ok = true;
        let mut wires: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < LoadType::ALL.len() {
            let t = LoadType::ALL[i];
            ok &= LoadType::from_wire(t.wire()) == Some(t);
            ok &= !wires.contains(&t.wire());
            wires.push(t.wire());
            ok &= t.wire() != t.ordinal() as u8;
            i += 1;
        }
        s.add(
            "Q05-负载类型-四类闭集线编码自洽",
            ok
                && LoadType::ALL.len() == LOAD_TYPE_COUNT
                && LoadType::from_wire(0x00).is_none()
                && LoadType::from_wire(0xFF).is_none(),
            "纹理/缓冲/着色器/网格四类闭集，线编码往返自洽互异且不等于判别值，未知码返回 None",
        );
    }

    // --- 判据 19：合并键 URI 指纹（**定长可比**，不含字符串） ---------
    {
        let a = LoadRequest::new(b"res/k", LoadType::Texture, Priority::Normal, 1, 1000);
        let b = LoadRequest::new(b"res/k", LoadType::Texture, Priority::Normal, 2, 1000);
        let ka = MergeKey::of(&a);
        let kb = MergeKey::of(&b);
        // 判据侧**独立重算** URI 指纹（不向被测函数问答案）
        let indep = uri_hash(b"res/k");
        s.add(
            "Q05-合并键-URI指纹独立重算对拍",
            ka == kb
                && ka.uri_hash == indep
                // 不同 URI ⇒ 不同指纹
                && MergeKey::of(&LoadRequest::new(b"res/other", LoadType::Texture, Priority::Normal, 3, 1000)).uri_hash != indep
                // 类型与优先级各占一字节
                && ka.ty == LoadType::Texture.ordinal() as u8
                && ka.priority == Priority::Normal.ordinal() as u8,
            "合并键存URI 的 64 位指纹（定长可比、不含字符串），判据侧独立重算对拍；类型与优先级各占一字节",
        );
    }

    // --- 判据 20：队列满**覆盖最旧且可查** ----------------------------
    {
        let mut q = FifoQueue::new();
        let mut i = 0u32;
        while i < QUEUE_SLOTS as u32 {
            q.push(i);
            i += 1;
        }
        let full = q.len() == QUEUE_SLOTS;
        // 再塞一条 ⇒ 覆盖最旧，条数不增、丢弃计数 +1。
        // **「条数不增」必须在 pop 之前取样**：pop 会消耗一条，
        // 之后再问len() 必然少 1。初版把取样放在 pop 之后，
        // 实现完全正确却判红——判据自身的时序错。
        q.push(9999);
        let still_full = q.len() == QUEUE_SLOTS;
        // 首个出队应是 1（0 已被覆盖）
        let first = q.pop();
        s.add(
            "Q05-队列满-覆盖最旧且丢弃可查",
            full
                && still_full
                && q.dropped == 1
                && first == Some(1)
                // 总共恰好排空 64 条。注意上一步的 pop **已消耗一条**，
                // 故这里只能再排 63 条——初版误按 64 次排，是判据自身算术错。
                && {
                    let mut drained = 1u32;
                    let mut k = 0u32;
                    while k < QUEUE_SLOTS as u32 - 1 {
                        if q.pop().is_some() {
                            drained += 1;
                        }
                        k += 1;
                    }
                    drained == QUEUE_SLOTS as u32 && q.pop().is_none()
                },
            "队列灌满 64 条后再入⇒ 覆盖最旧（首条出队为 1 而非 0）、丢弃计数为 1、恰好出 64 次后空",
        );
    }

    // --- 判据 21：性能逐项分解（入队/合并/告警均 O(1)） ---------------
    {
        let pb = PerfBudget::declared();
        let mut g = LoadGate::new();
        // 入队 100 次：计数应严格 +100（每次一条，无合并无拒绝）
        let before = g.stats.enqueued;
        let mut i = 0u32;
        while i < 100 {
            // 每个 URI 不同 ⇒ 不合并。
            // URI 必须**全部是合法可见 ASCII**：`to_le_bytes()` 会写入
            // 0x00 等控制符，而前置校验会拒（这正是判据 11 的设计意图），
            // 于是 100 次全被拒、enqueued 恒0 —— 初版正是这么错的。
            // 这里用十六进制可见字符拼编号，全程可见。
            let mut uri = [b'u'; 16];
            uri[7] = b'/';
            let hex = b"0123456789abcdef";
            uri[8] = hex[((i >> 12) & 0xF) as usize];
            uri[9] = hex[((i >> 8) & 0xF) as usize];
            uri[10] = hex[((i >> 4) & 0xF) as usize];
            uri[11] = hex[(i & 0xF) as usize];
            g.admit(&LoadRequest::new(&uri, LoadType::Texture, Priority::Normal, i, 1000));
            i += 1;
        }
        s.add(
            "Q05-性能-入队合并告警均O1且计数精确",
            pb.enqueue_ops == 1
                && pb.merge_ops == 1
                && pb.alarm_ops == 1
                // 校验按 URI 长度线性（单位操作记 1）
                && pb.validate_uri_scan == 1
                // 100 次提交 ⇒ 入队恰 100（无拒绝无合并，计数须精确）
                && g.stats.enqueued == before + 100
                && g.stats.rejected == 0
                && g.stats.merged == 0
                // **O(1) 且不增长内存的可观测证据**：结构性容量恒为常量，
                // 且单级队列被定容钳在 QUEUE_SLOTS——100 条提交后条数**饱和**
                // 在 64 而非增长到 100，落后的36 条以覆盖最旧的方式被丢弃
                // 且**可查**（覆盖不静默）。
                // 只断 `PerfBudget::declared()` 的字段等于 1 是**自证式恒真**
                // ——那个结构体与本实现零关联，改它自己就红不到真问题上。
                && g.queue_capacity() == PRIORITY_LEVELS * QUEUE_SLOTS
                && g.queued() == QUEUE_SLOTS
                && g.level_dropped(Priority::Normal) == 100 - QUEUE_SLOTS as u64,
            "入队/合并/告警三步均 O(1)、校验按 URI 长线性；100 次不同 URI 提交入队恰 100、拒绝 0、合并 0（计数精确无漂移）；灌入后结构性容量仍为 5×64 不增长",
        );
    }

    // --- 判据 22：面板只报聚合计数 + 零panic 面 ----------------------
    {
        let mut g = LoadGate::new();
        g.admit(&LoadRequest::new(b"res/p", LoadType::Texture, Priority::Normal, 1, 1000));
        g.admit(&LoadRequest::new(b"", LoadType::Texture, Priority::Normal, 2, 1000));
        let lines = g.a11y_lines();
        let mut joined = String::new();
        let mut i = 0;
        while i < lines.len() {
            joined.push_str(&lines[i]);
            joined.push('\n');
            i += 1;
        }
        // 零 panic：空队列连pop、空 URI 校验、越界下标反解
        let mut empty_gate = LoadGate::new();
        let pop_empty = empty_gate.pop().is_none();
        let empty_uri = validate_request(&LoadRequest::new(b"", LoadType::Texture, Priority::Normal, 0, 0));
        s.add(
            "Q05-面板与零panic-聚合计数可查且空态安全",
            lines.len() == 7
                && joined.contains("rejected")
                && joined.contains("merged")
                && joined.contains("inflation")
                && joined.contains("not tabled")
                // **整行匹配**而非 `contains("1")`：单字符子串在别处也会命中
                // （"rejected: 1" 与 "inflation alarms: 0" 里都有 "1" 的邻居），
                // 那样这条断的是"某个数字出现过"，不是"这个计数被正确报出"。
                && lines[0] == format!("拒绝 / rejected: {}", g.stats.rejected)
                && lines[1] == format!("合并命中 / merged: {}", g.stats.merged)
                && lines[2] == format!("入队 / enqueued: {}", g.stats.enqueued)
                && lines[6] == format!("合并表满未入表 / not tabled: {}", g.stats.not_tabled)
                // 私有形态不得泄漏：具体 URI
                && !joined.contains("res/p")
                && pop_empty
                && empty_uri == ValidateVerdict::Rejected(RejectKind::EmptyUri)
                && Priority::from_ordinal(usize::MAX).is_none(),
            "面板七行双语只报聚合计数且不含具体 URI；空队列 pop 返回 None、空 URI 被拒、越界下标反解返回 None（零 panic 面）",
        );
    }

    // --- 判据 23：**泄漏闸可达性**（反向证明闸门不是恒真） ---------------
    //
    // 判据 7 断「正常用法 leak_alarms == 0」。若该计数器是死代码（永不递增），
    // 判据 7 就是恒真门禁。此条**故意制造超额退订**并要求告警**必须递增**——
    // 于是「闸门焊死」这种实现退化会在这里转红。
    {
        let mut g = LoadGate::new();
        let a = LoadRequest::new(b"res/reach", LoadType::Texture, Priority::Normal, 1, 1000);
        g.admit(&a);
        // 正常退订一次 ⇒ 恰好归零回收
        let ok1 = g.release(&a);
        let quiet = g.leak_alarms() == 0 && g.merge_len() == 0;
        // **反向**：对已回收的键再退一次 ⇒ 记账漂移，必须被记录
        let ok2 = g.release(&a);
        let alarmed = g.leak_alarms() == 1;
        // 表长不得被这次漂移退订搞成下溢/错乱
        let stable = g.merge_len() == 0;
        // 对一个从未订阅过的键退订，同样必须记漂移
        let never = LoadRequest::new(b"res/never", LoadType::Texture, Priority::Normal, 9, 1000);
        let ok3 = g.release(&never);
        let drifted = g.leak_alarms() == 2;
        s.add(
            "Q05-泄漏闸-告警计数可达且反向可证",
            ok1 && quiet && !ok2 && alarmed && stable && !ok3 && drifted,
            "正常退订后告警为 0；对已回收键或从未订阅的键再退订，告警必须递增到 1/2——\
             若告警计数永不递增，判据 7 的『恒为 0』就是恒真门禁，泄漏闸形同虚设",
        );
    }

    // --- 判据 24：**非法字符语料补全**（控制符 / 0x7F） -------------------
    //
    // 判据 11 只喂了空格与反斜杠，而实现另判了控制符与 DEL。删掉那两行
    // 控制符条件，判据 11 仍全绿——覆盖漏洞。此条把语料补齐。
    {
        let mut g = LoadGate::new();
        // 0x01 控制符
        let ctl = LoadRequest::new(b"res/a\x01b", LoadType::Texture, Priority::Normal, 1, 1000);
        // 0x7F DEL
        let del = LoadRequest::new(b"res/a\x7fb", LoadType::Texture, Priority::Normal, 2, 1000);
        // 裸0x1F（控制符上界）
        let ctl_max = LoadRequest::new(b"res/a\x1fb", LoadType::Texture, Priority::Normal, 3, 1000);
        let r1 = g.admit(&ctl);
        let r2 = g.admit(&del);
        let r3 = g.admit(&ctl_max);
        // 边界对照：0x20 是空格（已拒），0x21 '!' 必须**放行**（夹逼对钉死界位置）
        let just_ok = LoadRequest::new(b"res/a!b", LoadType::Texture, Priority::Normal, 4, 1000);
        let r4 = g.admit(&just_ok);
        s.add(
            "Q05-非法字符-控制符与DEL拒且0x21放行",
            r1 == Admit::Rejected(RejectKind::IllegalChar)
                && r2 == Admit::Rejected(RejectKind::IllegalChar)
                && r3 == Admit::Rejected(RejectKind::IllegalChar)
                // 反向：紧邻控制符上界的可见字符必须可过（不误杀）
                && r4 == Admit::Enqueued { priority: Priority::Normal, timeout_ms: 1000 }
                && g.queued() == 1
                && g.stats.rejected == 3,
            "0x01/0x1F/0x7F 三类控制字符均按非法字符拒；紧邻的 0x21 '!' 放行——\
             控制符判据被删掉时本条必须转红（初版语料只含空格与反斜杠，删了仍全绿）",
        );
    }

    // --- 判据 25：**URI 长度边界夹逼**（512 过 / 513 拒） ------------------
    {
        let mut g = LoadGate::new();
        let mut exact = [b'a'; URI_MAX_LEN];
        exact[0] = b'r';
        let at_max = LoadRequest::new(&exact, LoadType::Texture, Priority::Normal, 1, 1000);
        let r1 = g.admit(&at_max);
        let over = LoadRequest::new(
            &[b'a'; URI_MAX_LEN + 1],
            LoadType::Texture,
            Priority::Normal,
            2,
            1000,
        );
        let r2 = g.admit(&over);
        s.add(
            "Q05-URI长度-上界夹逼512过513拒",
            r1 == Admit::Enqueued { priority: Priority::Normal, timeout_ms: 1000 }
                && r2 == Admit::Rejected(RejectKind::UriTooLong)
                // 判据侧独立重算长度，不问被测函数
                && exact.len() == URI_MAX_LEN
                && g.queued() == 1,
            "URI 恰为上限 512 通过、513 拒绝：把实现里的 `>` 改成 `>=` 时本条必须转红\
             （初版只用远界 600 测超长，界位置无人管）",
        );
    }

    // --- 判据 26：**优先级↔槽位绑定**（降级真落到Normal 那条槽） ---------
    //
    // degrade_inflation 走 Priority::ordinal() 取槽位。此条把「级别名 ↔ 槽位下标」
    // 的绑定钉死，避免日后调整枚举顺序时降级静默落错级。
    {
        let bound = Priority::Critical.ordinal() == 0 && Priority::Normal.ordinal() == 2;
        let mut q = PriorityQueues::new();
        q.push(Priority::Critical, 7);
        q.push(Priority::High, 8);
        let moved = degrade_inflation(&mut q);
        // 降级只动critical，high 不受影响
        let untouched = q.level_len(Priority::High) == 1 && q.level_len(Priority::Idle) == 0;
        let drained = q.level_len(Priority::Critical) == 0 && q.level_len(Priority::Normal) == 1;
        s.add(
            "Q05-槽位绑定-降级只动Critical不误伤他级",
            bound && moved == 1 && untouched && drained,
            "Critical.ordinal()==0、Normal.ordinal()==2 与降级落点绑定；\
             一个 critical 混一个 high 触发降级时，只有它落到 normal，high 不被牵连",
        );
    }

    // --- 判据 27：**降级与合并键的耦合**（降级不破坏去重） -----------------
    //
    // 降级把 critical 队列里的条目搬到 normal，但合并键里仍存着「请求自报的
    // critical」。若调用方在降级后再次提交同键请求，它仍会合并到同一主请求——
    // 这是正确行为（不会双载同一资源）。此条把该耦合钉成显式契约。
    {
        let mut g = LoadGate::new();
        let uri = b"res/coup";
        let r1 = g.admit(&LoadRequest::new(uri, LoadType::Texture, Priority::Critical, 1, 1000));
        let fired = g.apply_inflation_guard();
        let _ = fired;
        //降级后队列里已是 normal 槽，但合并键仍按请求自报优先级
        let r2 = g.admit(&LoadRequest::new(uri, LoadType::Texture, Priority::Critical, 2, 1000));
        s.add(
            "Q05-降级合并耦合-降级后同键仍合并不双载",
            r1 == Admit::Enqueued { priority: Priority::Critical, timeout_ms: 1000 }
                // 第二份合入同一主请求（primary=1），不新增队列槽
                && r2 == Admit::Merged { primary: 1 }
                && g.queued() == 1
                && g.merge_len() == 1
                && g.subscribers_of(&LoadRequest::new(
                    uri,
                    LoadType::Texture,
                    Priority::Critical,
                    9,
                    1000,
                )) == 2,
            "降级后再次提交同 URI 同类型同优先级的请求仍合并到原主请求（primary=1）：\
             降级改的是队列落点而非请求自报优先级，故不会把同一资源双载",
        );
    }

    // --- 判据 28：**降级可重复且幂等**（二次触发不重复搬运） --------------
    {
        let mut q = PriorityQueues::new();
        q.push(Priority::Critical, 1);
        let first = degrade_inflation(&mut q);
        // 已无 critical ⇒ 二次降级搬0 条（幂等，不制造幽灵条目）
        let second = degrade_inflation(&mut q);
        s.add(
            "Q05-降级幂等-二次降级搬运0条",
            first == 1 && second == 0 && q.total_len() == 1 && q.level_len(Priority::Normal) == 1,
            "一个 critical 首次降级搬 1 条，再次降级搬 0 条且总条数不变——\
             降级函数被重复调用不会凭空多出条目或丢条",
        );
    }

    // --- 判据 29：**五级齐全员队**（出队严格 critical→idle 不越级） -----
    //
    // 判据 4 只压了四级。本条把五级一次压满，出队必须严格按
    // critical→high→normal→low→idle 全序返回——遍历方向若写成从末级
    // 倒扣（idle 先出）或漏扫某一级，本条必红。
    {
        let mut q = PriorityQueues::new();
        // 逆序压入：先 idle 后 critical（入队序故意与优先级反向）
        q.push(Priority::Idle, 50);
        q.push(Priority::Low, 40);
        q.push(Priority::Normal, 30);
        q.push(Priority::High, 20);
        q.push(Priority::Critical, 10);
        let mut order = [0u32; 5];
        let mut i = 0usize;
        while i < 5 {
            if let Some((_, id)) = q.pop() {
                order[i] = id;
            }
            i += 1;
        }
        let drained = q.pop().is_none() && q.total_len() == 0;
        s.add(
            "Q05-五级全序-出队严格critical到idle且排空即None",
            order == [10, 20, 30, 40, 50]
                && drained
                // 各恰一条：五级都真实参与了优先级裁决而不是某级恒空
                && q.level_dequeued(Priority::Critical) == 1
                && q.level_dequeued(Priority::High) == 1
                && q.level_dequeued(Priority::Normal) == 1
                && q.level_dequeued(Priority::Low) == 1
                && q.level_dequeued(Priority::Idle) == 1,
            "五级各压一条后逆优先级出队严格为 10,20,30,40,50（critical→high→normal→low→idle），\
             排空后再 pop 返回 None：遍历方向与越级扫视在此被钉死",
        );
    }

    // --- 判据 30：**合并表满降级路径**（不入表仍入队 + 告警可对账） -----
    //
    // 合并表灌满 64 条不同键后，第 65 个新键进不了表：请求必须**照常入队
    // 加载**（合并是优化不是正确性依赖），同时 not_tabled 记 1。
    // 随后对该键 release：键从不在表里，泄漏闸必然告警——这不是记账漂移
    // 而是已知降级，故泄漏告警数必须与 not_tabled **逐条对账得上**。
    // 若没有 not_tabled，降级路径会把闸门打成假阳，泄漏信号从此不可信。
    {
        let mut g = LoadGate::new();
        let mut i = 0u32;
        while i < MERGE_SLOTS as u32 {
            // 64 个互不相同的合法 URI 灌满合并表（每条都真实入表）
            let mut uri = [b'k'; 16];
            uri[7] = b'/';
            let hex = b"0123456789abcdef";
            uri[8] = hex[((i >> 12) & 0xF) as usize];
            uri[9] = hex[((i >> 8) & 0xF) as usize];
            uri[10] = hex[((i >> 4) & 0xF) as usize];
            uri[11] = hex[(i & 0xF) as usize];
            g.admit(&LoadRequest::new(&uri, LoadType::Texture, Priority::Normal, i, 1000));
            i += 1;
        }
        let filled = g.merge_len() == MERGE_SLOTS;
        // 第 65 个新键：表满 ⇒ 不入表但仍入队
        let extra = LoadRequest::new(b"zz/overflow", LoadType::Texture, Priority::Normal, 99, 1000);
        let r = g.admit(&extra);
        let not_tabled = g.stats.not_tabled;
        // 常态路径（未满时）不得记 not_tabled
        let clean_gate = {
            let mut g2 = LoadGate::new();
            g2.admit(&LoadRequest::new(b"clean/one", LoadType::Texture, Priority::Normal, 1, 1000));
            g2.stats.not_tabled == 0
        };
        // 对入不了表的键 release ⇒ 泄漏告警 1 次，与 not_tabled 对账得上
        let rel = g.release(&extra);
        let reconciled = g.leak_alarms() == not_tabled && not_tabled == 1;
        s.add(
            "Q05-合并表满-降级不入表仍入队且告警可对账",
            filled
                && r == Admit::Enqueued { priority: Priority::Normal, timeout_ms: 1000 }
                && clean_gate
                && !rel
                && reconciled
                // 表长未被这次release 破坏
                && g.merge_len() == MERGE_SLOTS,
            "合并表灌满 64 键后第 65 键照常入队（合并是优化不是正确性依赖）并记 not_tabled=1；\
             对该键 release 的泄漏告警恰为 1 且与 not_tabled 逐条对账——降级不被误报成泄漏",
        );
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全判据必须全绿且不截断（**判据驱动**：没有这个测试，
    /// `run_veq05_checks` 只是一段没人调用的死代码——初版的
    /// `PriorityQueues::pop` 倒置+自旋 bug 正是这样绕过所有验证的）。
    #[test]
    fn q05_all_criteria_pass() {
        let s = run_veq05_checks();
        let (_p, f) = s.tally();
        assert_eq!(f, 0, "VE-F3205 判据存在红项");
        assert!(!s.truncated(), "判据集不应被截断");
    }

    /// 出队方向回归：五级齐全员队必须 critical 先出。
    #[test]
    fn q05_pop_serves_most_urgent_first() {
        let mut q = PriorityQueues::new();
        q.push(Priority::Idle, 5);
        q.push(Priority::Normal, 3);
        q.push(Priority::Critical, 1);
        assert_eq!(q.pop(), Some((Priority::Critical, 1)), "最急级必须先出");
        assert_eq!(q.pop(), Some((Priority::Normal, 3)));
        assert_eq!(q.pop(), Some((Priority::Idle, 5)));
        assert_eq!(q.pop(), None, "排空即 None");
    }

    /// Idle 级为空时 pop 必须终止（初版在末级空槽上无限自旋）。
    #[test]
    fn q05_pop_terminates_without_idle_traffic() {
        let mut q = PriorityQueues::new();
        q.push(Priority::Critical, 7);
        assert_eq!(q.pop(), Some((Priority::Critical, 7)));
        assert_eq!(q.pop(), None, "末级空槽上不得自旋");
    }
}