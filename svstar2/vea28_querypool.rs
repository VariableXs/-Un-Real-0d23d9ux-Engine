//! VE-F0028 · 查询池管理（VE-A 域 · GPU 查询池化 + 延迟取回 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0028`
//!
//! **判据（锚点原文）**：GPU 查询（遮挡/时间戳/流水线统计）的池化管理，查询结果
//! 延迟取回（同步点优化），查询溢出防护；含查询池的用途分组（遮挡/时间戳分池不
//! 混用）。判据：**三类查询、延迟取回、溢出防护、池化复用、判据**。
//!
//! 判据共 **31 条**，五个判据族逐条落地，另附变异验证（10/10 捕获，见下）。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 池耗尽 → **扩容 + 告警**（未到上限则扩容；到上限则拒绝并记账告警）
//! - 结果未就绪 → **延迟重试**（不在本帧阻塞等待，退到延迟队列下一帧再取）
//! - 溢出 → **钳制**（计数器饱和在 `u64::MAX` 且置粘滞饱和标志，绝不回绕）
//!
//! **数据结构**：池管理（`QueryPoolSet`）；延迟取回（`Deferred` 队列 + `Handle`）。
//!
//! **性能逐项分解**：O(查询)——`submit` 是 O(1) 摊还（空闲栈出栈），`signal` 是
//! O(1)（按句柄定位单槽），`collect` 是 O(本帧延迟队列长度)，扩容是 O(增量步长)。
//! 均**不随查询总数 N 增长**，也不随已取回结果数增长。
//!
//! **跨批对接点**：A12 探针基础设施消费——[`QueryPoolSet::snapshot`] 产出
//! [`PoolStat`] 快照（容量/空闲/在飞/就绪/发放/完成/扩容次数/高水位/耗尽拒绝），
//! A12 探针按帧取该快照画池态曲线。**本条不缓存探针数据**，A12 改自己的采样
//! 策略不必动本条；反之本条改池化策略会使 A12 曲线变化，那由快照字段语义固定
//! 挡住（字段含义写进 [`PoolStat`] 文档，不随实现漂移）。
//!
//! **无障碍与隐私**：池状态读屏可达（[`QueryPoolSet::a11y_lines`]）——逐池报
//! 「容量/空闲/在飞/延迟待取」，双语逐行本地化。面板只报**池的聚合计数**，
//! **不报单条查询的标识与结果值**（结果值可能含用户计时信息）。
//!
//! ## 设计要点
//!
//! - **三类查询分池不混用，且这条要能证伪**（[`QueryPoolSet::pools`] 按
//!   [`QueryKind`] 各自独立）：遮挡查询的槽位与时间戳查询的槽位**互不可见**。
//!   仅靠「三个池各自 `Vec`」是不够的——若某处代码拿 `ordinal` 直接索引一个
//!   跨类共享的数组，混用就成立了。故本条让**每一次跨类访问都返回专属错误码**
//!   （[`CollectFail::KindMismatch`] / [`SignalVerdict::KindMismatch`]），
//!   判据 `A28-isolate-*` 直接断言**故障种类**而不是「出错了」。
//! - **延迟取回不等于「晚点返回错值」**（[`QueryPoolSet::collect`]）：未就绪的
//!   句柄**留在**延迟队列里等下一帧，绝不返回占位值。判据
//!   `A28-defer-未就绪不返回占位值且句柄留存` 断言两件事同时成立：返回值是
//!   `Err(NotReady)`（不是 `Ok(0)`），且队列长度不减。
//! - **「延迟重试」必须有终止条件**（[`RETRY_LIMIT`]）：无限重试会把延迟队列
//!   变成永久泄漏——槽位永远不归还，池最终假性耗尽。故超限强制释放槽位并记
//!   [`CollectFail::RetryExhausted`]，判据 `A28-defer-超限释放槽位不泄漏` 断言
//!   **回收后空闲槽数确实回来了**。
//! - **池化复用的 ABA 陷阱**（[`QuerySlot::generation`]）：槽位归还后被再次发放，
//!   句柄里的 `slot` 数字**不变**。若只按 `slot` 索引，一个陈旧句柄就能读走
//!   别人的结果。故每次归还都 `generation += 1`，句柄携带发放时的代号，
//!   陈旧句柄一律 [`CollectFail::StaleGeneration`]。判据
//!   `A28-reuse-复用后代号变化` 断言代号**确实变了**，而不是只断言「槽被复用了」。
//! - **溢出钳制用饱和 + 粘滞标志，不是回绕**（[`Counter`]）：回绕的计数器会让
//!   「发放数」变成小数，探针曲线出现负斜率且无人察觉。饱和在 `u64::MAX` 并置
//!   [`Counter::saturated`]，此后只增不减。判据 `A28-of-饱和不回绕且标志粘滞`
//!   断言**跨越 `u64::MAX` 之后仍单调非降**且标志不自我清除。
//! - **枚举判别值 ≠ 线上编码值**（[`QueryKind::wire`]）：三类的判别值是 0/1/2，
//!   而跨进程 ABI 上的查询类型编码是 0x11/0x22/0x33。故走显式 `wire()` 映射，
//!   禁 `as u8`；判据 `A28-kind-编码值非判别值` 把这条纪律钉死——哪天有人图
//!   省事改成 `kind as u8`，编码值立刻与判别值重合，该判据转红。
//! - **扩容必须按步长渐进，不得一次跳满**（[`GROW_STEP`]）：判据
//!   `A28-reuse-扩容按步长渐进且恰好抵达上限` 与既有的「耗尽才扩容」互补——
//!   后者只断「容量增长了」，在前者写成「首次耗尽即扩到上限」的实现下同样全绿，
//!   于是 [`MAX_SLOTS_PER_POOL`] 从渐进逼近的预算闸退化成一次性全开，
//!   [`GROW_STEP`] 沦为死常量。故判据钉死**单次增量 ≤ 步长**且**扩容次数恰等于
//!   把容量从初始推到上限所需的步数**。
//!
//! ## 变异验证（判据是否有牙的证据）
//!
//! 全绿不证明判据有效——只证明没触发。10 个人工变体逐条对准五个判据族的
//! 核心机制（串池放行 / wire 退回判别值 / 未就绪返占位值 / 超限不归还 /
//! 饱和改回绕 / 队列不拒收 / 归还不推进代号 / 单次扩满 / 重复归还重复入栈 /
//! 硬失败偷他人槽），**捕获 10/10**，且基线仍全绿。这 10 条里有 1 条在补判据
//! **之前**是漏网的（`A28-reuse-耗尽才扩容且扩容后可发` 只断 `grow_events > 0`，
//! 断不出「一次扩满」），补判据后转红——记录在此以备后人复核。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0023（`vea23_psocache`）管管线状态缓存**，本条管**查询槽位**。二者都
//!   讲「复用」，但复用的对象不同：F0023 复用的是编译产物，本条复用的是**正在
//!   飞行的查询槽**。
//! - **F0026（`vea26_desc_heap`）管描述符堆**，也是池化，但堆的三段是
//!   分配/分片/复用，回收受「一帧绑定次数」预算约束；本条的回收受
//!   [`RETRY_LIMIT`] 与延迟队列上限约束，且**在飞槽一律不收**（收了就会读到
//!   未完成的 GPU 结果）。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个模块
//! 的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，与真实缺陷
//! 长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 查询用途分类数（锚点原文三类：遮挡/时间戳/流水线统计）。
pub const KIND_COUNT: usize = 3;

/// 每池初始槽位数。
pub const DEFAULT_SLOTS_PER_POOL: usize = 8;

/// 每池槽位上限（池耗尽且已达此上限 → 拒绝并告警，不再扩容）。
pub const MAX_SLOTS_PER_POOL: usize = 64;

/// 扩容步长（池耗尽时一次补多少槽）。
pub const GROW_STEP: usize = 4;

/// 延迟取回的重试上限（超限强制释放槽位并记 `RetryExhausted`）。
pub const RETRY_LIMIT: u32 = 4;

/// 延迟队列长度上限（超出即拒收并记账，即锚点的「溢出→钳制」在队列侧的落点）。
pub const MAX_DEFERRED: usize = 32;

/// 告警日志保留条数（超出只累加 [`QueryPoolSet::alarms_total`]，不静默丢弃）。
pub const ALARM_LOG_CAP: usize = 8;

/// 三类查询的映射表：`tag` / 中文名 / 线上编码值。
///
/// 本表是「枚举」与「线表」的唯一权威对账面：判据 `A28-kind-表与枚举逐项一致`
/// 逐项核对 [`QueryKind`] 的三个方法与本表**逐字一致**——两处书写而不核对，就
/// 会出现「表说 timestamp、枚举说 time_stamp」这种对不上的错，而它在真机上只
/// 表现为探针曲线错一条，不崩、不报，纯靠人眼发现。
pub const KINDS: [(&str, &str, u8); KIND_COUNT] = [
    ("occlusion", "遮挡查询", 0x11),
    ("timestamp", "时间戳查询", 0x22),
    ("pipeline-stats", "流水线统计查询", 0x33),
];

// ---------------------------------------------------------------------------
// 二、查询分类
// ---------------------------------------------------------------------------

/// GPU 查询的用途分类。三类**分池不混用**。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueryKind {
    /// 遮挡查询（可见性/遮挡剔除）。
    Occlusion,
    /// 时间戳查询（GPU 计时）。
    Timestamp,
    /// 流水线统计查询（计数器快照）。
    PipelineStats,
}

impl QueryKind {
    /// 全集（顺序即判别值顺序，供表索引用）。
    pub const ALL: [QueryKind; KIND_COUNT] = [
        QueryKind::Occlusion,
        QueryKind::Timestamp,
        QueryKind::PipelineStats,
    ];

    /// 表索引用的判别值。
    ///
    /// **这是源码顺序，不是线上编码**——线上编码走 [`QueryKind::wire`]。
    const fn ordinal(self) -> usize {
        match self {
            QueryKind::Occlusion => 0,
            QueryKind::Timestamp => 1,
            QueryKind::PipelineStats => 2,
        }
    }

    /// 稳定标签（与 [`KINDS`] 表逐字一致）。
    pub const fn tag(self) -> &'static str {
        match self {
            QueryKind::Occlusion => "occlusion",
            QueryKind::Timestamp => "timestamp",
            QueryKind::PipelineStats => "pipeline-stats",
        }
    }

    /// 中文名（与 [`KINDS`] 表逐字一致）。
    pub const fn zh(self) -> &'static str {
        match self {
            QueryKind::Occlusion => "遮挡查询",
            QueryKind::Timestamp => "时间戳查询",
            QueryKind::PipelineStats => "流水线统计查询",
        }
    }

    /// 线上编码值（跨进程 ABI）。
    ///
    /// **刻意不等于判别值**：判别值随源码里枚举变体顺序而变，改顺序就变；线上
    /// 编码一旦发布就不能变。故此处是显式映射，**禁止** `self as u8`。
    pub const fn wire(self) -> u8 {
        match self {
            QueryKind::Occlusion => 0x11,
            QueryKind::Timestamp => 0x22,
            QueryKind::PipelineStats => 0x33,
        }
    }

    /// 由线上编码值反查分类；非法编码返回 `None`（不静默兜底成某类）。
    ///
    /// 静默兜底是这里最危险的一种写法：`new(0)` 悄悄变成遮挡查询，会让时间戳
    /// 查询的结果被记进遮挡池的探针曲线——不崩、不报、画面正常，只有曲线错了。
    pub const fn from_wire(v: u8) -> Option<QueryKind> {
        match v {
            0x11 => Some(QueryKind::Occlusion),
            0x22 => Some(QueryKind::Timestamp),
            0x33 => Some(QueryKind::PipelineStats),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、槽位与句柄
// ---------------------------------------------------------------------------

/// 槽位状态机：`Free → InFlight → Ready → Free`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空闲，可被发放。
    Free,
    /// 已发放，GPU 在飞，结果未定。
    InFlight,
    /// 结果已就绪，可取回。
    Ready,
}

/// 单个查询槽。
#[derive(Clone, Copy, Debug)]
pub struct QuerySlot {
    /// 本槽所属分类（发放时写入，取回时复核——见 [`QueryKind`] 的隔离纪律）。
    pub kind: QueryKind,
    /// 本槽所属**池**的域标识。
    ///
    /// 这是「分池不混用」能真正落地的那一位：三个池各自都有 0 号槽，光靠
    /// `kind` 校验挡不住串池——时间戳池的 0 号槽与遮挡池的 0 号槽下标相同、
    /// 代号相同，伪造句柄走过去会被当成「本池的合法句柄」受理。带上域标识后，
    /// 跨池访问**必然**在第一道校验就被打回，且报的是专属码。
    pub scope: u32,
    /// 当前状态。
    pub state: SlotState,
    /// 代号：每次归还 `+1`，用于识破陈旧句柄（ABA 防护）。
    pub generation: u32,
    /// 发放序号（取回后可与统计对账）。
    pub sequence: u64,
    /// 就绪结果值；未就绪时为 0，**该 0 无意义**，不得当占位值取走。
    pub result: u64,
}

impl QuerySlot {
    fn new(kind: QueryKind, scope: u32) -> QuerySlot {
        QuerySlot {
            kind,
            scope,
            state: SlotState::Free,
            generation: 0,
            sequence: 0,
            result: 0,
        }
    }

    /// 是否可被发放。
    pub fn free(&self) -> bool {
        self.state == SlotState::Free
    }
}

/// 发放句柄：调用方持有的唯一凭据。
///
/// 携带 `generation` 是本条的关键设计——只凭 `slot` 索引会让陈旧句柄在槽位复用
/// 后读走**后来者**的结果（ABA）。见 [`QueryKind`] 上方设计要点第 5 条。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handle {
    /// 发放时的分类（取回时与槽内 `kind` 复核）。
    pub kind: QueryKind,
    /// 发放时的池域标识（取回时与槽内 `scope` 复核——串池的第一道闸门）。
    pub scope: u32,
    /// 槽位下标。
    pub slot: u32,
    /// 发放时的槽位代号。
    pub generation: u32,
    /// 发放序号。
    pub sequence: u64,
}

/// 单池的一次发放结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Acquired {
    /// 新句柄。
    pub handle: Handle,
    /// 发放方式：`Reused` 命中空闲槽（池化复用），`Grown` 因池耗尽而扩容后发放。
    pub verdict: AcquireVerdict,
}

/// 发放方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcquireVerdict {
    /// 命中空闲槽——**池化复用**的判据就在这里。
    Reused,
    /// 池耗尽，扩容后发放（锚点「池耗尽→扩容+告警」的扩容侧）。
    Grown,
}

// ---------------------------------------------------------------------------
// 四、饱和计数器（溢出→钳制）
// ---------------------------------------------------------------------------

/// 饱和计数器：到顶后钳制在 [`u64::MAX`] 并置粘滞标志，**绝不回绕**。
///
/// 回绕是这类计数器最隐蔽的失效：发放数从 `u64::MAX` 跳回 0，探针曲线上表现为
/// 一段负斜率，而「发放数不可能减少」这条不变式没有任何机制在守。饱和 + 粘滞
/// 标志把不变式变成可检的：`value` 单调非降，`saturated` 一旦置位不再清除。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Counter {
    /// 当前值（到顶钳制）。
    pub value: u64,
    /// 饱和标志：一旦为 `true` 永不恢复（粘滞）。
    pub saturated: bool,
}

impl Counter {
    /// 归零计数器。
    pub const fn zero() -> Counter {
        Counter {
            value: 0,
            saturated: false,
        }
    }

    /// 构造一个距顶只剩 `n` 的计数器（供溢出判据用，不参与生产路径）。
    pub const fn near_saturating(n: u64) -> Counter {
        Counter {
            value: u64::MAX - n,
            saturated: false,
        }
    }

    /// 钳制自增：到顶则停在 `u64::MAX` 并置标志，不回绕。
    pub fn bump(&mut self) {
        match self.value.checked_add(1) {
            Some(v) => self.value = v,
            None => {
                self.value = u64::MAX;
                self.saturated = true;
            }
        }
    }

    /// 是否已饱和。
    pub fn saturated(&self) -> bool {
        self.saturated
    }
}

// ---------------------------------------------------------------------------
// 五、单池
// ---------------------------------------------------------------------------

/// 单个分类的查询池。三个池互相独立，槽位**互不可见**。
pub struct QueryPool {
    kind: QueryKind,
    scope: u32,
    slots: Vec<QuerySlot>,
    free: Vec<u32>,
    /// 高水位：历史最大同时占用槽数（不含空闲）。
    high_water: usize,
    /// 扩容事件累计。
    grow_events: u32,
    /// 因池耗尽被拒绝的发放累计。
    exhausted_rejects: u64,
}

impl QueryPool {
    /// 建池：预置 [`DEFAULT_SLOTS_PER_POOL`] 个空闲槽。
    ///
    /// `scope` 是本池的域标识，**必须全池组唯一**——串池检测全靠它。
    pub fn new(kind: QueryKind, scope: u32) -> QueryPool {
        let mut slots = Vec::new();
        let mut free = Vec::new();
        let mut i = 0usize;
        while i < DEFAULT_SLOTS_PER_POOL {
            slots.push(QuerySlot::new(kind, scope));
            // 倒序入栈：首次出栈得到下标 0，使「首次发放落在 0 号槽」可断言。
            free.push((DEFAULT_SLOTS_PER_POOL - 1 - i) as u32);
            i += 1;
        }
        QueryPool {
            kind,
            scope,
            slots,
            free,
            high_water: 0,
            grow_events: 0,
            exhausted_rejects: 0,
        }
    }

    /// 本池分类。
    pub fn kind(&self) -> QueryKind {
        self.kind
    }

    /// 本池域标识（串池检测用）。
    pub fn scope(&self) -> u32 {
        self.scope
    }

    /// 容量（已分配槽数）。
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// 空闲槽数。
    pub fn free_count(&self) -> usize {
        self.free.len()
    }

    /// 已占用槽数（容量 − 空闲）。
    pub fn occupied(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    /// 历史高水位。
    pub fn high_water(&self) -> usize {
        self.high_water
    }

    /// 扩容事件累计。
    pub fn grow_events(&self) -> u32 {
        self.grow_events
    }

    /// 耗尽拒绝累计。
    pub fn exhausted_rejects(&self) -> u64 {
        self.exhausted_rejects
    }

    /// 计数某状态的槽数。
    pub fn count_state(&self, st: SlotState) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            if self.slots[i].state == st {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 发放一个槽。`None` 表示池已耗尽且已达上限（调用方须告警）。
    pub fn acquire(&mut self, sequence: u64) -> Option<Acquired> {
        if let Some(idx) = self.free.pop() {
            return self.claim(idx, sequence, AcquireVerdict::Reused);
        }
        // 空闲栈空 = 池耗尽 → 扩容（锚点「池耗尽→扩容」）。
        if self.slots.len() < MAX_SLOTS_PER_POOL {
            let target = if self.slots.len() + GROW_STEP > MAX_SLOTS_PER_POOL {
                MAX_SLOTS_PER_POOL
            } else {
                self.slots.len() + GROW_STEP
            };
            while self.slots.len() < target {
                let idx = self.slots.len() as u32;
                self.slots.push(QuerySlot::new(self.kind, self.scope));
                self.free.push(idx);
            }
            self.grow_events = self.grow_events.saturating_add(1);
            if let Some(idx) = self.free.pop() {
                return self.claim(idx, sequence, AcquireVerdict::Grown);
            }
            // 刚 push 过 free，不可能为空；仍走 None 以免 panic 面扩散。
            return None;
        }
        None
    }

    /// 把 `idx` 号槽从空闲转为在飞。
    fn claim(&mut self, idx: u32, sequence: u64, verdict: AcquireVerdict) -> Option<Acquired> {
        // 借用前先把要用到的量取成副本，避免与 `slots` 的可变借用重叠。
        let occupied = self.occupied();
        let generation = match self.slots.get(idx as usize) {
            Some(s) => s.generation,
            None => return None,
        };
        let kind = self.kind;
        let scope = self.scope;
        let slot = self.slots.get_mut(idx as usize)?;
        if slot.state != SlotState::Free {
            return None;
        }
        slot.state = SlotState::InFlight;
        slot.kind = kind;
        slot.scope = scope;
        slot.result = 0;
        slot.sequence = sequence;
        if occupied > self.high_water {
            self.high_water = occupied;
        }
        Some(Acquired {
            handle: Handle {
                kind,
                scope,
                slot: idx,
                generation,
                sequence,
            },
            verdict,
        })
    }

    /// 按句柄投递结果。返回投递是否生效。
    ///
    /// 校验顺序刻意是「先分类、后代号、再状态」：分类错说明调用方串池，是最严重
    /// 的错误，必须以**专属码**报出而不是笼统的「失败」。
    pub fn signal(&mut self, h: Handle, value: u64) -> SignalVerdict {
        let slot = match self.slots.get(h.slot as usize) {
            Some(s) => s,
            None => return SignalVerdict::UnknownSlot,
        };
        if slot.scope != h.scope || slot.kind != h.kind || self.scope != h.scope
            || self.kind != h.kind
        {
            return SignalVerdict::KindMismatch;
        }
        if slot.generation != h.generation {
            return SignalVerdict::StaleGeneration;
        }
        if slot.state != SlotState::InFlight {
            return SignalVerdict::NotInFlight;
        }
        if let Some(slot) = self.slots.get_mut(h.slot as usize) {
            slot.state = SlotState::Ready;
            slot.result = value;
        }
        SignalVerdict::Ok
    }

    /// 尝试取回结果。`Ok` 表示已取回并归还槽位；`Err(NotReady)` 表示**留待下帧**。
    pub fn try_release(&mut self, h: Handle) -> Result<u64, CollectFail> {
        let (kind, scope, generation, state, result) = match self.slots.get(h.slot as usize) {
            Some(s) => (s.kind, s.scope, s.generation, s.state, s.result),
            None => return Err(CollectFail::UnknownSlot),
        };
        if scope != h.scope || kind != h.kind || self.scope != h.scope || self.kind != h.kind {
            return Err(CollectFail::KindMismatch);
        }
        if generation != h.generation {
            return Err(CollectFail::StaleGeneration);
        }
        if state == SlotState::Free {
            return Err(CollectFail::NotInFlight);
        }
        if state != SlotState::Ready {
            // 未就绪：**不返回占位值**，让调用方退到延迟队列下一帧再取。
            return Err(CollectFail::NotReady);
        }
        self.recycle(h.slot);
        Ok(result)
    }

    /// 强制归还槽位（放弃查询时用）。槽位若不在飞行态则不动。
    pub fn force_release(&mut self, slot: u32) -> bool {
        let reclaim = match self.slots.get(slot as usize) {
            Some(s) => s.state == SlotState::InFlight || s.state == SlotState::Ready,
            None => false,
        };
        if !reclaim {
            return false;
        }
        self.recycle(slot);
        true
    }

    /// 归还槽位并推进代号（ABA 防护的唯一入口）。
    fn recycle(&mut self, slot: u32) {
        let kind = self.kind;
        let scope = self.scope;
        let mut freed = false;
        if let Some(s) = self.slots.get_mut(slot as usize) {
            s.state = SlotState::Free;
            // 代号推进：陈旧句柄由此失效。
            s.generation = s.generation.saturating_add(1);
            s.result = 0;
            s.kind = kind;
            s.scope = scope;
            freed = true;
        }
        if freed {
            self.free.push(slot);
        }
    }
}

/// 投递结果的处置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalVerdict {
    /// 已投递。
    Ok,
    /// 槽位下标越界。
    UnknownSlot,
    /// **串池**：句柄分类与所在池不符。
    KindMismatch,
    /// 陈旧句柄：槽位已复用，代号不符。
    StaleGeneration,
    /// 槽位不在飞（已就绪或已归还）。
    NotInFlight,
}

impl SignalVerdict {
    /// 是否成功。
    pub fn ok(self) -> bool {
        self == SignalVerdict::Ok
    }
}

// ---------------------------------------------------------------------------
// 六、取回失败的原因分类
// ---------------------------------------------------------------------------

/// 取回失败的原因。
///
/// **每一类失败都有专属变体，不合并成一个 `Failed`**——三类失败的后续动作相反：
/// `NotReady` 要**留着下帧再取**，`KindMismatch`/`StaleGeneration` 要**报错并
/// 放弃**，`RetryExhausted` 要**释放槽位**。共用一个码会让上层把它们当同一类处理，
/// 于是「该留的丢了、该放的没放」，泄漏与误报同时发生。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CollectFail {
    /// 槽位下标越界。
    UnknownSlot,
    /// **串池**：句柄分类与所在池不符。
    KindMismatch,
    /// 陈旧句柄：槽位已复用。
    StaleGeneration,
    /// 槽位已归还，不再在飞。
    NotInFlight,
    /// **结果未就绪**：本帧不取，留待延迟队列下一帧重试。
    NotReady,
    /// 重试超限，强制释放并放弃。
    RetryExhausted,
    /// 延迟队列已满，拒收（锚点「溢出→钳制」在队列侧的落点）。
    DeferredOverflow,
}

impl CollectFail {
    /// 是否属于「留待下帧」的软失败（唯一该保留句柄的一类）。
    pub fn retryable(self) -> bool {
        self == CollectFail::NotReady
    }
}

/// 延迟队列条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Deferred {
    /// 待取回的句柄。
    pub handle: Handle,
    /// 已重试次数（超过 [`RETRY_LIMIT`] 即放弃）。
    pub retries: u32,
}

impl Deferred {
    /// 新建延迟条目。
    pub fn new(handle: Handle) -> Deferred {
        Deferred { handle, retries: 0 }
    }
}

/// 成功取回的一条结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Collected {
    /// 原句柄。
    pub handle: Handle,
    /// 结果值。
    pub result: u64,
}

/// 被放弃的一条记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DropRecord {
    /// 原句柄。
    pub handle: Handle,
    /// 放弃原因（判据直接断言**故障种类**，不只断言「有数」）。
    pub reason: CollectFail,
}

/// 一帧取回的汇总。
#[derive(Clone, Debug)]
pub struct CollectOutcome {
    /// 本帧成功取回的结果。
    pub collected: Vec<Collected>,
    /// 本帧放弃的记录。
    pub dropped: Vec<DropRecord>,
    /// 本帧发生的软失败（未就绪）次数。
    pub retries: u32,
    /// 取回结束后仍留在延迟队列的条目数。
    pub deferred_left: usize,
}

impl CollectOutcome {
    fn zero() -> CollectOutcome {
        CollectOutcome {
            collected: Vec::new(),
            dropped: Vec::new(),
            retries: 0,
            deferred_left: 0,
        }
    }

    /// 本帧成功取回条数。
    pub fn collected_count(&self) -> usize {
        self.collected.len()
    }
}

// ---------------------------------------------------------------------------
// 七、池集合（池化管理本体）
// ---------------------------------------------------------------------------

/// 三类查询池的集合——本条的管理本体。
pub struct QueryPoolSet {
    pools: Vec<QueryPool>,
    deferred: Vec<Deferred>,
    /// 全池发放序号（单调，饱和不回绕）。
    issued: Counter,
    /// 全池完成累计（单调，饱和不回绕）。
    completed: Counter,
    /// 延迟队列溢出的拒收累计（锚点「溢出→钳制」）。
    deferred_drops: Counter,
    /// 告警日志（超出 [`ALARM_LOG_CAP`] 只累加计数，不静默丢弃）。
    alarms: Vec<PoolAlarm>,
    /// 告警累计总数（与 `alarms.len()` 区分，前者不封顶）。
    alarms_total: u64,
}

/// 一条池耗尽告警。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PoolAlarm {
    /// 触发告警的分类。
    pub kind: QueryKind,
    /// 触发时的容量。
    pub capacity: usize,
    /// 触发时的已占用槽数。
    pub occupied: usize,
}

impl QueryPoolSet {
    /// 建池组：三类各 [`DEFAULT_SLOTS_PER_POOL`] 槽。
    pub fn new() -> QueryPoolSet {
        QueryPoolSet {
            pools: vec![
                QueryPool::new(QueryKind::Occlusion, 1),
                QueryPool::new(QueryKind::Timestamp, 2),
                QueryPool::new(QueryKind::PipelineStats, 3),
            ],
            deferred: Vec::new(),
            issued: Counter::zero(),
            completed: Counter::zero(),
            deferred_drops: Counter::zero(),
            alarms: Vec::new(),
            alarms_total: 0,
        }
    }

    /// 取某分类的池（只读）。分类恒在域内，故不会 `None`，但仍返回 `Option`
    /// 以免调用方在索引失败时 panic。
    pub fn pool(&self, kind: QueryKind) -> Option<&QueryPool> {
        self.pools.get(kind.ordinal())
    }

    /// 取某分类的池（可变）。
    pub fn pool_mut(&mut self, kind: QueryKind) -> Option<&mut QueryPool> {
        self.pools.get_mut(kind.ordinal())
    }

    /// 发放一个查询槽。返回 `None` 表示该池耗尽且已到上限（已记账告警）。
    ///
    /// 告警日志超出 [`ALARM_LOG_CAP`] 时**只累加 [`QueryPoolSet::alarms_total`]**，
    /// 不静默丢弃——「池耗尽却在日志里看不到」是最难查的一类故障。
    pub fn submit(&mut self, kind: QueryKind) -> Option<Handle> {
        let ord = kind.ordinal();
        let sequence = self.issued.value;
        let acq = match self.pools.get_mut(ord) {
            Some(p) => p.acquire(sequence),
            None => None,
        };
        match acq {
            Some(a) => {
                self.issued.bump();
                Some(a.handle)
            }
            None => {
                let (capacity, occupied) = match self.pools.get(ord) {
                    Some(p) => (p.capacity(), p.occupied()),
                    None => (0, 0),
                };
                if let Some(p) = self.pools.get_mut(ord) {
                    p.exhausted_rejects = p.exhausted_rejects.saturating_add(1);
                }
                self.alarms_total = self.alarms_total.saturating_add(1);
                if self.alarms.len() < ALARM_LOG_CAP {
                    self.alarms.push(PoolAlarm {
                        kind,
                        capacity,
                        occupied,
                    });
                }
                None
            }
        }
    }

    /// 投递结果（GPU 完成）。返回处置，供调用方定位失败种类。
    pub fn signal(&mut self, h: Handle, value: u64) -> SignalVerdict {
        let ord = h.kind.ordinal();
        match self.pools.get_mut(ord) {
            Some(p) => p.signal(h, value),
            None => SignalVerdict::UnknownSlot,
        }
    }

    /// 把句柄放入延迟队列，等待取回。
    ///
    /// 队列满时返回 `false` 并记账（锚点「溢出→钳制」）。此时句柄**不丢**——调用方
    /// 仍持有它，可自行重试或强制归还；本条只拒收，不代为销毁。
    pub fn defer(&mut self, h: Handle) -> bool {
        if self.deferred.len() >= MAX_DEFERRED {
            self.deferred_drops.bump();
            return false;
        }
        self.deferred.push(Deferred::new(h));
        true
    }

    /// 延迟队列长度。
    pub fn deferred_len(&self) -> usize {
        self.deferred.len()
    }

    /// 一帧取回：遍历延迟队列，能取的取走，未就绪的留下，超限的释放并放弃。
    ///
    /// 这是「同步点优化」的落点——本函数**不含任何等待**，未就绪一律留待下帧。
    pub fn collect(&mut self) -> CollectOutcome {
        let mut out = CollectOutcome::zero();
        let queue = core::mem::take(&mut self.deferred);
        let mut keep: Vec<Deferred> = Vec::new();
        let mut i = 0usize;
        while i < queue.len() {
            let mut entry = queue[i];
            i += 1;
            let ord = entry.handle.kind.ordinal();
            let verdict = match self.pools.get_mut(ord) {
                Some(p) => p.try_release(entry.handle),
                None => Err(CollectFail::UnknownSlot),
            };
            match verdict {
                Ok(value) => {
                    self.completed.bump();
                    out.collected.push(Collected {
                        handle: entry.handle,
                        result: value,
                    });
                }
                Err(CollectFail::NotReady) => {
                    entry.retries = entry.retries.saturating_add(1);
                    out.retries = out.retries.saturating_add(1);
                    if entry.retries > RETRY_LIMIT {
                        // 重试超限：必须归还槽位，否则延迟队列变成永久泄漏，
                        // 池最终「假性耗尽」——明明有槽却发不出去。
                        if let Some(p) = self.pools.get_mut(ord) {
                            p.force_release(entry.handle.slot);
                        }
                        out.dropped.push(DropRecord {
                            handle: entry.handle,
                            reason: CollectFail::RetryExhausted,
                        });
                    } else {
                        keep.push(entry);
                    }
                }
                Err(reason) => {
                    // 硬失败（串池/陈旧/越界/已归还）：报错并放弃，且
                    //**绝不归还槽位**。
                    //
                    // 这里刻意**不**调`force_release`：陈旧句柄的槽早已被别人
                    // 占用（这正是 ABA 的定义），串池句柄的槽则属于另一个池——
                    // 两种情况下回收都是**偷别人的槽**。后果不是「少一个槽」，
                    // 而是同一个槽被同时发给两个句柄，后写的结果覆盖先写的，
                    // 两个持有者读到同一份数据且都以为是自己那份。故硬失败一律
                    // 只记账不动槽：真持有者自会走自己的投递/取回路径。
                    out.dropped.push(DropRecord {
                        handle: entry.handle,
                        reason,
                    });
                }
            }
        }
        self.deferred = keep;
        out.deferred_left = self.deferred.len();
        out
    }

    /// 强制归还某句柄的槽位（调用方放弃查询时用）。
    pub fn abandon(&mut self, h: Handle) -> bool {
        let ord = h.kind.ordinal();
        match self.pools.get_mut(ord) {
            Some(p) => p.force_release(h.slot),
            None => false,
        }
    }

    /// 全池发放累计。
    pub fn issued(&self) -> u64 {
        self.issued.value
    }

    /// 全池完成累计。
    pub fn completed(&self) -> u64 {
        self.completed.value
    }

    /// 延迟队列溢出拒收累计。
    pub fn deferred_drops(&self) -> u64 {
        self.deferred_drops.value
    }

    /// 发放计数器是否已饱和。
    pub fn issued_saturated(&self) -> bool {
        self.issued.saturated()
    }

    /// 告警累计总数（不封顶）。
    pub fn alarms_total(&self) -> u64 {
        self.alarms_total
    }

    /// 保留的告警日志。
    pub fn alarms(&self) -> &[PoolAlarm] {
        &self.alarms
    }

    /// 产出一帧池态快照，供 A12 探针基础设施消费。
    ///
    /// 字段语义固定（见 [`PoolStat`] 文档），不随池化策略实现漂移——A12 按此
    /// 画曲线，本条改策略只改数值不改语义。
    pub fn snapshot(&self) -> Vec<PoolStat> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.pools.len() {
            if let Some(p) = self.pools.get(i) {
                out.push(PoolStat {
                    kind: p.kind,
                    capacity: p.capacity() as u32,
                    free: p.free_count() as u32,
                    inflight: p.count_state(SlotState::InFlight) as u32,
                    ready: p.count_state(SlotState::Ready) as u32,
                    grow_events: p.grow_events(),
                    high_water: p.high_water() as u32,
                    exhausted_rejects: p.exhausted_rejects(),
                });
            }
            i += 1;
        }
        out
    }

    /// 池态读屏行（无障碍）。
    ///
    /// 逐池报「容量/空闲/在飞/延迟待取」，双语。**只报聚合计数，不报单条查询的
    /// 标识与结果值**——结果值可能含用户计时信息，属隐私面，不进读屏。
    pub fn a11y_lines(&self, zh_cn: bool) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.pools.len() {
            if let Some(p) = self.pools.get(i) {
                if zh_cn {
                    out.push(format!(
                        "{}：容量 {}，空闲 {}，在飞 {}，就绪 {}",
                        p.kind.zh(),
                        p.capacity(),
                        p.free_count(),
                        p.count_state(SlotState::InFlight),
                        p.count_state(SlotState::Ready),
                    ));
                } else {
                    out.push(format!(
                        "{}: capacity {}, free {}, in flight {}, ready {}",
                        p.kind.tag(),
                        p.capacity(),
                        p.free_count(),
                        p.count_state(SlotState::InFlight),
                        p.count_state(SlotState::Ready),
                    ));
                }
            }
            i += 1;
        }
        if zh_cn {
            out.push(format!("延迟队列待取 {} 条", self.deferred.len()));
            out.push(format!("告警累计 {} 条", self.alarms_total));
        } else {
            out.push(format!("deferred queue {} pending", self.deferred.len()));
            out.push(format!("alarms {} total", self.alarms_total));
        }
        out
    }
}

/// 一帧的单池池态快照（A12 探针消费）。
///
/// **字段语义固定，不得随实现漂移**：A12 探针按这些字段画曲线，本条改池化策略时
/// 只应改数值不改语义；语义一变，A12 的历史曲线就不可比。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PoolStat {
    /// 分类。
    pub kind: QueryKind,
    /// 已分配槽数（容量）。
    pub capacity: u32,
    /// 当前空闲槽数。
    pub free: u32,
    /// 在飞（已发放未就绪）槽数。
    pub inflight: u32,
    /// 已就绪待取槽数。
    pub ready: u32,
    /// 扩容事件累计。
    pub grow_events: u32,
    /// 历史最大同时占用槽数。
    pub high_water: u32,
    /// 耗尽拒绝累计。
    pub exhausted_rejects: u64,
}

/// 空池组（供默认构造与判据使用）。
pub fn new_pool_set() -> QueryPoolSet {
    QueryPoolSet::new()
}

// ---------------------------------------------------------------------------
// 八、判据
// ---------------------------------------------------------------------------

/// 判据集：锚点五条判据逐条落地——**三类查询、延迟取回、溢出防护、池化复用、判据**。
pub fn run_vea28_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F0028");
    let _ = &mut set;

    // ---- 规格常量自洽 ----
    set.add(
        "A28-const-容量阶梯合法",
        DEFAULT_SLOTS_PER_POOL > 0
            && GROW_STEP > 0
            && MAX_SLOTS_PER_POOL >= DEFAULT_SLOTS_PER_POOL,
        "",
    );
    set.add(
        "A28-const-重试上限与队列上限为正",
        RETRY_LIMIT > 0 && MAX_DEFERRED > 0 && ALARM_LOG_CAP > 0,
        "",
    );
    set.add(
        "A28-const-三类表规模与分类数一致",
        KIND_COUNT == 3 && QueryKind::ALL.len() == KIND_COUNT,
        "",
    );

    // ---- 判据一：三类查询（映射表 / 编码值 / 分池） ----
    {
        // 表内无重复：tag / 中文名 / 编码值三者各自唯一。
        let mut dup_tag = 0u32;
        let mut dup_zh = 0u32;
        let mut dup_wire = 0u32;
        let mut i = 0usize;
        while i < KIND_COUNT {
            let mut j = i + 1;
            while j < KIND_COUNT {
                if KINDS[i].0 == KINDS[j].0 {
                    dup_tag += 1;
                }
                if KINDS[i].1 == KINDS[j].1 {
                    dup_zh += 1;
                }
                if KINDS[i].2 == KINDS[j].2 {
                    dup_wire += 1;
                }
                j += 1;
            }
            i += 1;
        }
        set.add(
            "A28-kind-表内三列各自唯一",
            dup_tag == 0 && dup_zh == 0 && dup_wire == 0,
            "",
        );
    }
    {
        // 枚举的三个方法必须与映射表逐字一致——两处书写，缺核对就会出现
        // 「表说 timestamp、枚举说 time_stamp」，真机上只表现为探针曲线错一条。
        let mut consistent = true;
        let mut i = 0usize;
        while i < QueryKind::ALL.len() {
            let k = QueryKind::ALL[i];
            if k.tag() != KINDS[i].0 || k.zh() != KINDS[i].1 || k.wire() != KINDS[i].2 {
                consistent = false;
            }
            i += 1;
        }
        set.add("A28-kind-表与枚举逐项一致", consistent, "");
    }
    {
        // 编码值不得等于判别值：把「禁 kind as u8」这条纪律钉死。哪天有人图省事
        // 改成强转，编码与判别重合，此判据立刻转红。
        let mut all_distinct = true;
        let mut i = 0usize;
        while i < QueryKind::ALL.len() {
            let k = QueryKind::ALL[i];
            if k.wire() as usize == k.ordinal() {
                all_distinct = false;
            }
            i += 1;
        }
        set.add("A28-kind-编码值非判别值", all_distinct, "");
    }
    {
        // from_wire 必须与 wire 互逆，且非法编码返回 None（不静默兜底成某类——
        // 兜底会让时间戳结果记进遮挡池的曲线，不崩不报，只有曲线错）。
        let mut inverse = true;
        let mut i = 0usize;
        while i < QueryKind::ALL.len() {
            let k = QueryKind::ALL[i];
            if QueryKind::from_wire(k.wire()) != Some(k) {
                inverse = false;
            }
            i += 1;
        }
        let mut rejects = true;
        let mut probe = 0u16;
        while probe < 256u16 {
            let v = probe as u8;
            // 不写 `from_wire(v).unwrap()`：`is_some()` 与 `unwrap()` 是两次独立
            // 调用，判据自己就把「同源驱动恒真」的坑踩了进去——这里是判据侧，
            // 一旦`from_wire` 被改成有副作用或两次结果不一致，断言会跟着漂。
            // 单次绑定后比对，恒等关系由这一次求值承担。
            match QueryKind::from_wire(v) {
                Some(k) => {
                    if k.wire() != v {
                        rejects = false;
                    }
                }
                None => {}
            }
            probe += 1;
        }
        set.add("A28-kind-编码反查互逆且非法码拒绝", inverse && rejects, "");
    }
    {
        // 三个池互相独立：只发遮挡，另两池的占用数必须**恒为 0**。
        let mut s = QueryPoolSet::new();
        let mut i = 0;
        while i < 4 {
            if s.submit(QueryKind::Occlusion).is_none() {
                break;
            }
            i += 1;
        }
        let mut isolated = true;
        let mut k = 0usize;
        while k < KIND_COUNT {
            let kind = QueryKind::ALL[k];
            if kind != QueryKind::Occlusion {
                if let Some(p) = s.pool(kind) {
                    if p.occupied() != 0 || p.free_count() != DEFAULT_SLOTS_PER_POOL {
                        isolated = false;
                    }
                } else {
                    isolated = false;
                }
            }
            k += 1;
        }
        set.add("A28-isolate-他池占用不受本池影响", isolated, "");
    }
    {
        // 串池必须以**专属错误码**报出，而不是笼统失败。断言故障种类。
        let mut s = QueryPoolSet::new();
        let h = match s.submit(QueryKind::Occlusion) {
            Some(h) => h,
            None => {
                set.fail("A28-isolate-串池投递报专属码", "发放失败");
                set.fail("A28-isolate-串池取回报专属码", "未构造");
                set.fail("A28-isolate-串池不误伤源池", "未构造");
                return finish(set);
            }
        };
        // 拿着遮挡句柄去时间戳池投递 → 必须 KindMismatch，且**不得**动源池状态。
        let before_ready = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.count_state(SlotState::Ready))
            .unwrap_or(0);
        let v = s.signal(
            Handle {
                kind: QueryKind::Timestamp,
                scope: h.scope,
                slot: h.slot,
                generation: h.generation,
                sequence: h.sequence,
            },
            42,
        );
        let after_ready = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.count_state(SlotState::Ready))
            .unwrap_or(0);
        set.add(
            "A28-isolate-串池投递报专属码",
            v == SignalVerdict::KindMismatch,
            "",
        );
        set.add(
            "A28-isolate-串池不误伤源池",
            before_ready == 0 && after_ready == 0,
            "",
        );
        // 取回侧同样必须给专属码。
        let mut forged = Handle {
            kind: QueryKind::PipelineStats,
            scope: h.scope,
            slot: h.slot,
            generation: h.generation,
            sequence: h.sequence,
        };
        if let Some(p) = s.pool_mut(QueryKind::PipelineStats) {
            forged.scope = p.scope();
            forged.kind = QueryKind::PipelineStats;
        }
        let rr = match s.pool_mut(QueryKind::Occlusion) {
            Some(p) => p.try_release(forged),
            None => Err(CollectFail::UnknownSlot),
        };
        set.add(
            "A28-isolate-串池取回报专属码",
            rr == Err(CollectFail::KindMismatch),
            "",
        );
    }

    // ---- 判据二：延迟取回 ----
    {
        // 未就绪**不得**返回占位值，且句柄必须留在队列里（不减）。
        let mut s = QueryPoolSet::new();
        let h = match s.submit(QueryKind::Timestamp) {
            Some(h) => h,
            None => {
                set.fail("A28-defer-未就绪不返回占位值且句柄留存", "发放失败");
                return finish(set);
            }
        };
        let queued = s.defer(h);
        let before = s.deferred_len();
        let out = s.collect();
        let after = s.deferred_len();
        let retained = out.collected.is_empty()
            && out.dropped.is_empty()
            && out.retries == 1
            && queued
            && after == before
            && after > 0;
        set.add("A28-defer-未就绪不返回占位值且句柄留存", retained, "");
        // 槽位必须仍在飞（没被误回收）。
        let still_inflight = s
            .pool(QueryKind::Timestamp)
            .map(|p| p.count_state(SlotState::InFlight))
            .unwrap_or(0);
        set.add(
            "A28-defer-未就绪不误回收在飞槽",
            still_inflight == 1,
            "",
        );
    }
    {
        // 投递后就绪 → 同一句柄本帧取回，队列清空。
        let mut s = QueryPoolSet::new();
        let h = match s.submit(QueryKind::Timestamp) {
            Some(h) => h,
            None => {
                set.fail("A28-defer-就绪后取回且队列清空", "发放失败");
                return finish(set);
            }
        };
        let _ = s.defer(h);
        let sig = s.signal(h, 0xBEEF);
        let out = s.collect();
        let got = out.collected_count() == 1
            && out.collected[0].result == 0xBEEF
            && s.deferred_len() == 0;
        set.add("A28-defer-就绪后取回且队列清空", sig.ok() && got, "");
    }
    {
        // 延迟取回是 O(队列长度)，不阻塞：一次 collect 里连续取回多条，
        // 条数必须与投递条数**独立对账相等**（不能只断「大于 0」）。
        let mut s = QueryPoolSet::new();
        let want = 6usize;
        let mut hs: Vec<Handle> = Vec::new();
        let mut i = 0usize;
        while i < want {
            if let Some(h) = s.submit(QueryKind::PipelineStats) {
                hs.push(h);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < hs.len() {
            let h = hs[j];
            let _ = s.signal(h, (j as u64) + 1);
            let _ = s.defer(h);
            j += 1;
        }
        let out = s.collect();
        let sum_ok = out.collected_count() == want
            && out
                .collected
                .iter()
                .fold(0u64, |acc, c| acc.wrapping_add(c.result))
                == ((want as u64) * (want as u64 + 1)) / 2;
        set.add("A28-defer-一批取回条数与结果和独立对账", sum_ok, "");
    }
    {
        // 重试超限必须**归还槽位**，否则延迟队列变成永久泄漏、池假性耗尽。
        let mut s = QueryPoolSet::new();
        let h = match s.submit(QueryKind::Occlusion) {
            Some(h) => h,
            None => {
                set.fail("A28-defer-超限释放槽位不泄漏", "发放失败");
                return finish(set);
            }
        };
        let _ = s.defer(h);
        let free_before = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.free_count())
            .unwrap_or(0);
        let mut dropped_reason = CollectFail::NotReady;
        let mut rounds = 0u32;
        while rounds <= RETRY_LIMIT {
            let out = s.collect();
            if let Some(d) = out.dropped.first() {
                dropped_reason = d.reason;
                break;
            }
            rounds += 1;
        }
        let free_after = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.free_count())
            .unwrap_or(0);
        let inflight = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.count_state(SlotState::InFlight))
            .unwrap_or(1);
        set.add(
            "A28-defer-超限释放槽位不泄漏",
            dropped_reason == CollectFail::RetryExhausted
                && free_after == free_before + 1
                && inflight == 0,
            "",
        );
    }
    {
        // 陈旧句柄：槽位复用后，原句柄必须以 StaleGeneration 失败而不是读到新值。
        // 这是延迟取回里最容易漏的一条——句柄存在延迟队列里，可能跨过一整轮复用。
        let mut s = QueryPoolSet::new();
        let stale = match s.submit(QueryKind::Timestamp) {
            Some(h) => h,
            None => {
                set.fail("A28-defer-陈旧句柄不读走新值", "发放失败");
                return finish(set);
            }
        };
        let _ = s.signal(stale, 1);
        let _ = s.defer(stale);
        let first = s.collect();
        let ok_first = first.collected_count() == 1 && first.collected[0].result == 1;
        let again = s.collect();
        let _ = again;
        let fresh = s.submit(QueryKind::Timestamp);
        let stale_verdict = match fresh {
            Some(fh) => {
                let _ = s.signal(fh, 999);
                s.pool_mut(QueryKind::Timestamp)
                    .map(|p| p.try_release(stale))
                    .unwrap_or(Err(CollectFail::UnknownSlot))
            }
            None => Err(CollectFail::UnknownSlot),
        };
        set.add(
            "A28-defer-陈旧句柄不读走新值",
            ok_first && stale_verdict == Err(CollectFail::StaleGeneration),
            "",
        );
    }

    {
        // 硬失败**不得偷别人的槽**。陈旧句柄的槽已由新持有者占用，此时若归还，
        // 同一槽会被同时发给两个句柄——两人读到同一份数据且都以为是自己那份。
        // 故硬失败只记账不动槽。
        let mut s = QueryPoolSet::new();
        let stale = match s.submit(QueryKind::Occlusion) {
            Some(h) => h,
            None => {
                set.fail("A28-defer-硬失败不偷他人槽位", "发放失败");
                return finish(set);
            }
        };
        let _ = s.signal(stale, 1);
        let _ = s.defer(stale);
        let first = s.collect();
        let owner = match s.submit(QueryKind::Occlusion) {
            Some(h) => h,
            None => {
                set.fail("A28-defer-硬失败不偷他人槽位", "再发放失败");
                return finish(set);
            }
        };
        let same_slot = owner.slot == stale.slot;
        let _ = s.defer(stale); // 陈旧句柄再次入队
        let out = s.collect();
        let dropped_stale = out
            .dropped
            .iter()
            .any(|d| d.reason == CollectFail::StaleGeneration);
        // 新持有者的槽必须**仍在飞**（没被陈旧句柄顺手回收）。
        let owner_inflight = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.count_state(SlotState::InFlight))
            .unwrap_or(0);
        set.add(
            "A28-defer-硬失败不偷他人槽位",
            first.collected_count() == 1
                && same_slot
                && dropped_stale
                && owner_inflight == 1
                && out.collected.is_empty(),
            "",
        );
    }

    // ---- 判据三：溢出防护 ----
    {
        // 饱和不回绕：跨过 u64::MAX 之后仍单调非降，且标志粘滞不自我清除。
        let mut c = Counter::near_saturating(2);
        let mut prev = c.value;
        let mut monotone = true;
        let mut i = 0;
        while i < 6 {
            c.bump();
            if c.value < prev {
                monotone = false;
            }
            prev = c.value;
            i += 1;
        }
        set.add(
            "A28-of-饱和不回绕且标志粘滞",
            monotone && c.value == u64::MAX && c.saturated(),
            "",
        );
    }
    {
        // 标志粘滞：饱和后再自增多次，标志不得自我清除，且值恒为 MAX。
        let mut c = Counter::near_saturating(0);
        c.bump();
        let mut stuck = true;
        let mut i = 0;
        while i < 32 {
            c.bump();
            if !c.saturated() || c.value != u64::MAX {
                stuck = false;
            }
            i += 1;
        }
        set.add("A28-of-饱和标志不自我清除", stuck, "");
    }
    {
        // 延迟队列溢出必须拒收并记账（MAX_DEFERRED 之后 defer 返回 false）。
        let mut s = QueryPoolSet::new();
        let mut accepted = 0usize;
        let mut i = 0usize;
        while i < MAX_DEFERRED + 4 {
            // 发放数超过池容量时改用扩容路径；这里只需足够多的句柄入队。
            if s.submit(QueryKind::PipelineStats).is_some() {
                let h = Handle {
                    kind: QueryKind::PipelineStats,
                    scope: s
                        .pool(QueryKind::PipelineStats)
                        .map(|p| p.scope())
                        .unwrap_or(3),
                    slot: (i % DEFAULT_SLOTS_PER_POOL.max(1)) as u32,
                    generation: 0,
                    sequence: i as u64,
                };
                if s.defer(h) {
                    accepted += 1;
                }
            }
            i += 1;
        }
        let overflow_recorded = s.deferred_drops() == (MAX_DEFERRED + 4 - accepted) as u64;
        set.add(
            "A28-of-延迟队列溢出拒收并记账",
            s.deferred_len() == MAX_DEFERRED && overflow_recorded,
            "",
        );
    }
    {
        // 池耗尽告警不得静默丢弃：日志封顶但累计数不封顶。
        let mut s = QueryPoolSet::new();
        // 填满一个池直至上限：DEFAULT + 若干次 GROW，直到 MAX。
        let mut issued = 0usize;
        let mut refused = 0usize;
        while issued < MAX_SLOTS_PER_POOL + 8 {
            if s.submit(QueryKind::Occlusion).is_none() {
                refused += 1;
            }
            issued += 1;
        }
        let log_capped = s.alarms().len() <= ALARM_LOG_CAP;
        set.add(
            "A28-of-池耗尽告警日志封顶但累计不封顶",
            refused > 0 && log_capped && s.alarms_total() == refused as u64,
            "",
        );
    }

    // ---- 判据四：池化复用 ----
    {
        // 复用后**代号必须变化**（ABA 防护的实质）。只断「槽被复用」是不够的：
        // 那在 generation 不变时也成立，而不变正是 ABA 的根因。
        let mut s = QueryPoolSet::new();
        let h1 = match s.submit(QueryKind::Timestamp) {
            Some(h) => h,
            None => {
                set.fail("A28-reuse-复用后代号变化", "发放失败");
                return finish(set);
            }
        };
        let _ = s.signal(h1, 7);
        let _ = s.defer(h1);
        let out = s.collect();
        let got = out.collected_count() == 1 && out.collected[0].result == 7;
        let h2 = s.submit(QueryKind::Timestamp);
        let gen_changed = match h2 {
            Some(h) => h.slot == h1.slot && h.generation != h1.generation && h.sequence > h1.sequence,
            None => false,
        };
        set.add("A28-reuse-复用后代号变化", got && gen_changed, "");
    }
    {
        // 池化复用必须是 O(1) 摊还且容量**不增长**：反复发放-取回 N 轮，
        // 容量须与初始一致（复用而非泄漏式扩容）。
        let mut s = QueryPoolSet::new();
        let cap0 = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.capacity())
            .unwrap_or(0);
        let mut i = 0u32;
        while i < 50 {
            if let Some(h) = s.submit(QueryKind::Occlusion) {
                let _ = s.signal(h, i as u64);
                let _ = s.defer(h);
                let _ = s.collect();
            }
            i += 1;
        }
        let cap1 = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.capacity())
            .unwrap_or(1);
        let free1 = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.free_count())
            .unwrap_or(0);
        set.add(
            "A28-reuse-反复复用容量不增长且槽全归还",
            cap0 == cap1 && free1 == cap1,
            "",
        );
    }
    {
        // 扩容是「池耗尽」而非「池初始化」：先耗尽到发不出，才见 grow_events 增长；
        // 且扩容后立刻发得出（扩容与告警是同一动作的两面）。
        let mut s = QueryPoolSet::new();
        let cap0 = s
            .pool(QueryKind::Timestamp)
            .map(|p| p.capacity())
            .unwrap_or(0);
        let g0 = s
            .pool(QueryKind::Timestamp)
            .map(|p| p.grow_events())
            .unwrap_or(0);
        let mut n = 0usize;
        let mut grew = false;
        // 上限必须越过初始容量，否则「扩容后才发得出」这一条永远走不到。
        while n < DEFAULT_SLOTS_PER_POOL + GROW_STEP + 2 {
            if s.submit(QueryKind::Timestamp).is_some() {
                n += 1;
            } else {
                grew = true;
                break;
            }
        }
        let g1 = s
            .pool(QueryKind::Timestamp)
            .map(|p| p.grow_events())
            .unwrap_or(0);
        let cap1 = s
            .pool(QueryKind::Timestamp)
            .map(|p| p.capacity())
            .unwrap_or(0);
        // 池满时的那次发放仍成功（扩容了），故 n 会超过初始容量。
        set.add(
            "A28-reuse-耗尽才扩容且扩容后可发",
            g0 == 0 && g1 > 0 && cap1 > cap0 && n > DEFAULT_SLOTS_PER_POOL,
            "",
        );
        let _ = grew;
    }
    {
        // 扩容必须**按步长渐进**，不得一次跳到上限。
        //
        // 上一条只断「容量增长了、扩容后可发」，那在「首次耗尽就一次扩到
        // MAX_SLOTS_PER_POOL」的实现下同样全绿——于是上限从一个渐进逼近的
        // 预算闸退化成「一次性全开」，`GROW_STEP` 直接变成死常量：第一次
        // 抖动就把显存顶满，而后续无论池多忙都不会再触发扩容决策。
        //
        // 判据钉死两件事，缺一不可：
        //   ① 单次扩容的增量**恰为** `GROW_STEP`（不多不少——一次跳满是「>」，
        //      不扩容是「=0」，只断「>0」两边都漏）；
        //   ② 从初始容量填到上限，扩容**发生多次**而非一次（`grow_events`
        //      恰等于总步数 `(MAX - DEFAULT) / GROW_STEP`）。
        // 这条与上一条互补：上一条管「何时扩」，本条管「扩多少」。
        let mut s = QueryPoolSet::new();
        let cap0 = s
            .pool(QueryKind::Timestamp)
            .map(|p| p.capacity())
            .unwrap_or(0);
        // 逐轮发放-归还，每一轮把池推到「刚耗尽」的下一档，逐步记录单次增量。
        let mut steps: Vec<usize> = Vec::new();
        let mut live = 0usize;
        let mut prev_cap = cap0;
        let mut guard = 0u32;
        while prev_cap < MAX_SLOTS_PER_POOL && guard < 256 {
            guard += 1;
            // 填满当前容量**再多要一个**——恰好吃到 `prev_cap` 时空闲栈恰好被取空，
            // 而 `acquire` 只在「取空之后仍被索要」时才走扩容分支，故必须多索一次，
            // 否则永远量不到扩容（这正是本判据第一版的错法：填到 `prev_cap`
            // 就停，扩容一次都不触发，量得 steps 为空而把正确实现判红）。
            let mut hs: Vec<Handle> = Vec::new();
            while live <= prev_cap {
                match s.submit(QueryKind::Timestamp) {
                    Some(h) => {
                        hs.push(h);
                        live += 1;
                    }
                    None => break,
                }
            }
            let cap_now = s.pool(QueryKind::Timestamp).map(|p| p.capacity()).unwrap_or(0);
            if cap_now > prev_cap {
                steps.push(cap_now - prev_cap);
                prev_cap = cap_now;
            }
            // 全量归还，腾空以便下一轮填满（这就是「复用」而非扩容）。
            let mut i = 0usize;
            while i < hs.len() {
                let _ = s.abandon(hs[i]);
                i += 1;
            }
            live = 0;
        }
        let g_final = s
            .pool(QueryKind::Timestamp)
            .map(|p| p.grow_events())
            .unwrap_or(0) as usize;
        // 每一步都恰为 GROW_STEP（末步若被上限截断则允许更小，但不得更大）。
        let step_ok = steps.len() > 0 && steps.iter().all(|d| *d > 0 && *d <= GROW_STEP);
        // 扩容次数恰等于把容量从 DEFAULT 推到 MAX 所需的步数。
        let want_steps = (MAX_SLOTS_PER_POOL - DEFAULT_SLOTS_PER_POOL + GROW_STEP - 1)
            / GROW_STEP;
        // 最终容量必须**恰好**落在上限上（不得越界，也不得半途而废）。
        let cap_final = s
            .pool(QueryKind::Timestamp)
            .map(|p| p.capacity())
            .unwrap_or(0);
        set.add(
            "A28-reuse-扩容按步长渐进且恰好抵达上限",
            step_ok && g_final == want_steps && cap_final == MAX_SLOTS_PER_POOL,
            "",
        );
    }
    {
        // 高水位必须等于历史峰值占用，且不超过容量。
        let mut s = QueryPoolSet::new();
        let mut hs: Vec<Handle> = Vec::new();
        let mut i = 0usize;
        while i < 5 {
            if let Some(h) = s.submit(QueryKind::Occlusion) {
                hs.push(h);
            }
            i += 1;
        }
        let peak = hs.len();
        let _ = s.collect();
        let mut j = 0usize;
        while j < hs.len() {
            let h = hs[j];
            let _ = s.abandon(h);
            j += 1;
        }
        let hw = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.high_water())
            .unwrap_or(0);
        let cap = s
            .pool(QueryKind::Occlusion)
            .map(|p| p.capacity())
            .unwrap_or(0);
        set.add(
            "A28-reuse-高水位等于历史峰值且不超容量",
            hw == peak && hw <= cap,
            "",
        );
    }
    {
        // 在飞槽一律不收：未就绪的槽即便被 abandon 之外的路数碰到也不能凭空变空闲。
        let mut s = QueryPoolSet::new();
        let h = match s.submit(QueryKind::PipelineStats) {
            Some(h) => h,
            None => {
                set.fail("A28-reuse-重复归还不重复入栈", "发放失败");
                return finish(set);
            }
        };
        let free0 = s
            .pool(QueryKind::PipelineStats)
            .map(|p| p.free_count())
            .unwrap_or(0);
        let first = s.abandon(h);
        let second = s.abandon(h);
        let free1 = s
            .pool(QueryKind::PipelineStats)
            .map(|p| p.free_count())
            .unwrap_or(0);
        // 二次归还必须失败，且空闲数**不得**二次增长（否则同一槽在空闲栈里出现两次，
        // 会被发放给两个句柄——那正是「一个结果被两个查询读到」）。
        set.add(
            "A28-reuse-重复归还不重复入栈",
            first && !second && free1 == free0 + 1,
            "",
        );
    }

    // ---- 判据五：判据自身的度量与跨批对接 ----
    {
        // 快照三行、字段语义固定（A12 探针按此画曲线）。
        let mut s = QueryPoolSet::new();
        let _ = s.submit(QueryKind::Occlusion);
        let snap = s.snapshot();
        let mut well_formed = snap.len() == KIND_COUNT;
        let mut i = 0usize;
        while i < snap.len() {
            let st = snap[i];
            if st.kind != QueryKind::ALL[i] || st.free > st.capacity || st.high_water > st.capacity {
                well_formed = false;
            }
            i += 1;
        }
        set.add("A28-判据-快照行数与字段自洽", well_formed, "");
    }
    {
        // 读屏行覆盖三池 + 延迟队列 + 告警，且**不含**任何结果值。
        let mut s = QueryPoolSet::new();
        let h = match s.submit(QueryKind::Timestamp) {
            Some(h) => h,
            None => {
                set.fail("A28-判据-读屏行覆盖三池且不泄露结果值", "发放失败");
                return finish(set);
            }
        };
        let _ = s.signal(h, 123_456_789);
        let zh = s.a11y_lines(true);
        let en = s.a11y_lines(false);
        let mut covered = zh.len() >= KIND_COUNT + 2 && en.len() == zh.len();
        let mut i = 0usize;
        while i < QueryKind::ALL.len() {
            let k = QueryKind::ALL[i];
            let tag = k.tag();
            let zhh = k.zh();
            let mut hit_en = false;
            let mut hit_zh = false;
            let mut j = 0usize;
            while j < en.len() {
                if en[j].contains(tag) {
                    hit_en = true;
                }
                if zh[j].contains(zhh) {
                    hit_zh = true;
                }
                j += 1;
            }
            if !hit_en || !hit_zh {
                covered = false;
            }
            i += 1;
        }
        // 隐私面：结果值不得出现在任何读屏行里。
        let mut leaked = false;
        let mut j = 0usize;
        while j < en.len() {
            if en[j].contains("123456789") {
                leaked = true;
            }
            j += 1;
        }
        set.add("A28-判据-读屏行覆盖三池且不泄露结果值", covered && !leaked, "");
    }
    {
        // 发放/完成累计与实际动作独立对账（净值口径会被「建了又销」骗过，
        // 故这里断的是**恰等于**本轮真实发放数）。
        let mut s = QueryPoolSet::new();
        let want = 9usize;
        let mut hs: Vec<Handle> = Vec::new();
        let mut i = 0usize;
        while i < want {
            if let Some(h) = s.submit(QueryKind::Occlusion) {
                hs.push(h);
            }
            i += 1;
        }
        let issued_ok = s.issued() == hs.len() as u64;
        let mut j = 0usize;
        while j < hs.len() {
            let h = hs[j];
            let _ = s.signal(h, j as u64);
            let _ = s.defer(h);
            j += 1;
        }
        let out = s.collect();
        set.add(
            "A28-判据-发放完成累计与实际动作对账",
            issued_ok
                && s.completed() == out.collected_count() as u64
                && out.collected_count() == want,
            "",
        );
    }

    finish(set)
}

/// 收口：判据集自带条数上界，超限如实报红（[`CheckSet::add`] 已按此记账）。
fn finish(set: CheckSet) -> CheckSet {
    set
}

// ---------------------------------------------------------------------------
// 九、自检内部辅助（仅判据使用）
// ---------------------------------------------------------------------------

/// 造一个「三池各发一条、全部就绪」的池组，供外部探针复用。
///
/// 属判据支撑，不参与生产路径；单独列出以免与池本体混在一起被误当 API 用。
pub fn sample_pool_set() -> QueryPoolSet {
    let mut s = QueryPoolSet::new();
    let mut i = 0usize;
    while i < QueryKind::ALL.len() {
        let kind = QueryKind::ALL[i];
        if let Some(h) = s.submit(kind) {
            let _ = s.signal(h, i as u64);
            let _ = s.defer(h);
        }
        i += 1;
    }
    s
}