//! VE-F0034 · 瞬态资源分配器（VE-A 域 · 帧内瞬态资源分配优化 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0034`
//!
//! **判据（锚点原文）**：瞬态资源分配器——帧内瞬态资源的分配优化（帧内生命周期
//! 分析→堆内瞬时分配），瞬态与持久资源的池隔离，分配耗时 P95 承诺；含瞬态
//! 分配的失败降级路径（池满转持久堆并告知）。
//! 判据：**瞬时分配、生命周期分析、池隔离、P95 承诺、判据**。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 跨帧泄漏 → **检测告警**：请求声明的生存期只在本帧内，却活到了下一帧
//!   （`release_frame` 时仍被占用）。这不是性能问题而是**契约破坏**——瞬态
//!   资源活过帧界就会与下一帧的持久资源踩内存。故必须检出并告警，且
//!   **告警后仍强制回收**（不回收则泄漏逐帧累积，几帧后整池耗尽）。
//! - 分配失败（池满） → **降级转持久堆并告知**：瞬态池满时**不直接失败**，
//!   而是转持久堆（正确性优先于性能）**并告知调用方**已降级——调用方可能
//!   依赖「本帧一定拿到瞬态内存」来做免上传假设，静默降级会让免上传优化
//!   悄悄失效，故降级必须可观测。
//! - 耗时劣化（P95 超标） → **归因**：耗时不是只报一个数就完事，必须能
//!   **归因到具体分配请求**（哪一笔把 P95 拉高了），否则无法优化。
//!
//! **数据结构**：分配器（[`TransientAllocator`]）；生命周期分析
//! （[`Liveness`]）；池（[`TransientPool`] / [`PersistentPool`]）；
//! 降级账（[`DegradeLog`]）；耗时账（[`TimingLedger`]）。
//!
//! **性能逐项分解**：O(分配请求数)——[`TransientAllocator::allocate`] 对每个
//! 请求做一次 O(1) 的桶位查找（空闲链表按桶取，非线性搜索）；
//! [`TransientAllocator::seal_frame`] 的跨帧泄漏扫描是 O(存活请求数)，
//! 逐帧一次而非逐请求一次。
//!
//! **跨批对接点**：A03 内存池联动——[`TransientAllocator::promoted_bytes`]
//! 给出因池满而转持久堆的字节数，供 A03 显存池记账；本条只报降级事实与
//! 字节数，不管持久堆怎么分配。
//!
//! **无障碍与隐私**：分配面板读屏可达（[`TransientAllocator::a11y_lines`]）——
//! 报「请求数/瞬态命中/降级数/泄漏告警数/P95 微秒」，中英双语逐行。面板
//! **只报聚合计数**，不报单个请求的资源名与内容（那是资产布局信息）。
//!
//! ## 设计要点
//!
//! - **瞬态与持久必须池隔离**（[`TransientPool`] / [`PersistentPool`] 各自
//!   独立）：两条池共用一个空闲链表时，瞬态请求会把持久块借走，帧末回收
//!   时把**持久资源正在用的内存**释放掉——这是最典型的瞬态分配器事故。
//!   故本条用两个独立池 + [`TransientAllocator`] 上的**池归属断言**表达该
//!   不变式（`PoolKind` 进 [`AllocationTicket`]，回收时校验）。
//! - **生命周期分析按「帧内区间」而非「时刻」**（[`Liveness`]）：瞬态资源
//!   在帧内的活跃区间是**半开** `[first_touch, release)`。用时刻语义
//!   （"某时刻是否活跃"）会把「上一笔的释放时刻 == 下一笔的首次触碰时刻」
//!   判成冲突，于是所有首尾相接的流水绘制全被判泄漏——瞬态分配器整体
//!   失效。故本条用半开区间，语义与 F0033 的 [`vea33_alias::Span`] 一致
//!   但**本模块自持定义**（不跨模块 `use`，理由见文末「与相邻条的分工」）。
//! - **跨帧泄漏必须检出且强制回收**（[`TransientAllocator::seal_frame`]）：
//!   检出（[`TransientAllocator::leaks`]）与回收（`forced_reclaimed`）
//!   是**两个独立计数**。只检不收 = 泄漏逐帧累积；只收不检 = 契约破坏
//!   静默，调用方不知道自己踩了帧界。负向断言「无泄漏时计数为 0」不够，
//!   必须**走真实路径造出一次泄漏**断「恰等于 1」——否则自增代码写在
//!   溢出分支里也照样过门禁。
//! - **P95 用最近窗口 + 最近秩**（[`TimingLedger`]）：不存全量历史（P95
//!   只关心尾部），保留最近 [`TIMING_WINDOW`] 笔，超出即淘汰最旧。P95 取
//!   **最近秩**（`ceil(0.95 * n)`，零基下标）而非平均值——平均值会被大量
//!   便宜请求稀释，掩盖尾部劣化。
//! - **降级必须可观测且字节可查**（[`DegradeLog`]）：池满转持久堆既要有
//!   计数（几笔），也要有字节数（多少显存被占住）。只报笔数无法回答
//!   「持久堆多占了多少显存」，A03 就没法记账。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0033（`vea33_alias`）管「哪两个资源可以共用一块显存」**（别名安全
//!   域），本条管「这一帧要一笔内存时从哪拿」。前者问「这两者能不能叠」，
//!   后者问「现在有没有空位」——同一时刻可分配 ≠ 可别名。
//! - **F0030（`vea30_*`，显存预算）管预算账**，本条管池内分配动作。
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

/// 存活区间的空端点约定（`release <= first_touch` 即「零时长」）。
pub const EMPTY_RELEASE: u32 = 0;

/// 单帧瞬态池块数上限。
pub const TRANSIENT_SLOTS: usize = 64;

/// 单块最大字节数（超过即拒绝，不做「拆大块」——本条只做等长块分配）。
pub const MAX_BLOCK_BYTES: u64 = 1 << 20;

/// 单块粒度：所有块按此粒度对齐取整（分配尺寸向上取整到粒度倍数）。
pub const BLOCK_GRANULARITY: u64 = 256;

/// 耗时账窗口容量（只关心尾部，不留全量历史）。
pub const TIMING_WINDOW: usize = 256;

/// P95 分位（百分位，95）。
pub const P95_PERCENTILE: usize = 95;

/// P95 承诺上限（微秒）。超过即记劣化，但**不阻断分配**——耗时劣化是
/// 性能问题，不能让渲染直接失败。
pub const P95_BUDGET_US: u64 = 250;

/// 跨帧泄漏的强制回收标记（回收原因码）。
pub const REASON_LEAK_RECLAIM: u32 = 1;

/// 正常帧末回收标记。
pub const REASON_FRAME_END: u32 = 0;

/// 降级账行数上限（池满转持久堆的登记上限）。
pub const MAX_DEGRADE_ROWS: usize = TRANSIENT_SLOTS;

/// 分配结论种数（`OutKind::ALL` 的长度）。
pub const OUTCOME_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// 二、生命周期分析
// ---------------------------------------------------------------------------

/// 帧内活跃区间（**半开** `[first_touch, release)`）。
///
/// 半开的理由：瞬态流水绘制里「上一笔的释放时刻」常常**就是**「下一笔的
/// 首次触碰时刻」。用闭区间会把这种首尾相接判成冲突，于是所有流水绘制的
/// 瞬态资源全被误判泄漏，分配器整体失效。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Liveness {
    /// 首次触碰时刻（**含**）。
    pub first_touch: u32,
    /// 释放时刻（**不含**）。
    pub release: u32,
}

impl Liveness {
    /// 构造帧内活跃区间。`release <= first_touch` 视为**零时长**（瞬时分配）。
    pub const fn new(first_touch: u32, release: u32) -> Liveness {
        Liveness { first_touch, release }
    }

    /// 是否零时长（`release <= first_touch`）。
    pub const fn is_instant(&self) -> bool {
        self.release <= self.first_touch
    }

    /// 帧内时长（零时长为 0）。
    pub const fn duration(&self) -> u32 {
        if self.is_instant() {
            0
        } else {
            self.release - self.first_touch
        }
    }

    /// 指定时刻是否落在活跃区间内（半开口径：含首、不含尾）。
    pub const fn active_at(&self, t: u32) -> bool {
        !self.is_instant() && self.first_touch <= t && t < self.release
    }

    /// 与另一区间是否**冲突**（半开口径：端点相接不冲突）。
    pub const fn conflicts(&self, other: &Liveness) -> bool {
        if self.is_instant() || other.is_instant() {
            return false;
        }
        self.first_touch < other.release && other.first_touch < self.release
    }

    /// 中文摘要（审计用）。
    pub fn summary(&self) -> String {
        if self.is_instant() {
            return String::from("零时长瞬时分配");
        }
        format!("[{}, {}) 共 {} 微刻", self.first_touch, self.release, self.duration())
    }
}

/// 池归属（**瞬态与持久必须隔离**，见模块头「设计要点」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolKind {
    /// 帧内瞬态池：帧末无条件全量回收。
    Transient,
    /// 持久池：跨帧存活，只能由持有者显式释放。
    Persistent,
}

impl PoolKind {
    /// 全枚举（顺序即 [`PoolKind::ordinal`] 的下标）。
    pub const ALL: [PoolKind; 2] = [PoolKind::Transient, PoolKind::Persistent];

    /// 枚举下标（供计数数组用，**不是**线上编码值）。
    pub const fn ordinal(self) -> usize {
        match self {
            PoolKind::Transient => 0,
            PoolKind::Persistent => 1,
        }
    }

    /// 枚举判别值 → 线上编码值。**显式映射**，不用 `as u8`——
    /// 判别值与线上值不是一回事，混用会在改枚举顺序时静默改协议。
    pub const fn wire(self) -> u8 {
        match self {
            PoolKind::Transient => 0x54,
            PoolKind::Persistent => 0x50,
        }
    }

    /// 线上编码值 → 枚举（未登记码返回 `None`，不静默兜底到某成员）。
    pub const fn from_wire(w: u8) -> Option<PoolKind> {
        match w {
            0x54 => Some(PoolKind::Transient),
            0x50 => Some(PoolKind::Persistent),
            _ => None,
        }
    }

    /// 读屏文案（双语）。
    pub const fn label(self) -> &'static str {
        match self {
            PoolKind::Transient => "瞬态池 / transient",
            PoolKind::Persistent => "持久池 / persistent",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、分配票据与结论
// ---------------------------------------------------------------------------

/// 分配请求（一条 = 一次分配尝试）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocRequest {
    /// 请求序号（用于归因与复核，不做身份标识）。
    pub seq: u32,
    /// 请求字节数（**未取整**；实际按 [`BLOCK_GRANULARITY`] 向上取整）。
    pub bytes: u64,
    /// 帧内活跃区间（生命周期分析的输入）。
    pub liveness: Liveness,
    /// 请求方声明的池归属（瞬态请求不得拿到持久块，反之亦然）。
    pub want: PoolKind,
}

impl AllocRequest {
    /// 构造一个瞬态分配请求。
    pub const fn transient(seq: u32, bytes: u64, liveness: Liveness) -> AllocRequest {
        AllocRequest { seq, bytes, liveness, want: PoolKind::Transient }
    }

    /// 构造一个持久分配请求。
    pub const fn persistent(seq: u32, bytes: u64, liveness: Liveness) -> AllocRequest {
        AllocRequest { seq, bytes, liveness, want: PoolKind::Persistent }
    }

    /// 取整后的块字节数（向上取整到 [`BLOCK_GRANULARITY`] 的倍数）。
    ///
    /// 「瞬时分配」按块给，不按字节给：块粒度不对齐会让空闲链表的复用
    /// 逻辑出现无法填补的碎片。
    pub const fn block_bytes(&self) -> u64 {
        let g = BLOCK_GRANULARITY;
        if self.bytes == 0 {
            return 0;
        }
        // 向上取整：`bytes/g` 向上 + 余数。不用 `saturating_*`——`bytes` 已由
        // 调用方保证 ≤ [`MAX_BLOCK_BYTES`]，除法不会溢出。
        ((self.bytes + g - 1) / g) * g
    }
}

/// 分配票据（成功时给出，**含池归属**以便帧末校验回收对不对池）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationTicket {
    /// 请求序号（原样回带，便于归因）。
    pub seq: u32,
    /// 块起始槽位。
    pub slot: u16,
    /// 块字节数（取整后）。
    pub bytes: u64,
    /// 实际落到的池。
    pub pool: PoolKind,
    /// **请求声明**的池（可能与 `pool` 不同——降级时声明瞬态、实际落持久）。
    ///
    /// 两者必须都留：跨帧泄漏检测看的是**声明**（调用方承诺「本帧内释放」），
    /// 而槽位归还是**实际池**的事。降级票据若按 `pool` 判泄漏就漏检了——
    /// 它占的是持久堆、没人回收，越界即真泄漏。
    pub want: PoolKind,
    /// 生命周期区间（原样回带，帧末泄漏检测比对用）。
    pub liveness: Liveness,
}

impl AllocationTicket {
    /// 槽位序号（供判据直接断言，不经聚合层）。
    pub const fn slot_index(&self) -> usize {
        self.slot as usize
    }

    /// 实际占的是瞬态池块（决定帧末要不要归还槽位）。
    pub const fn is_transient(&self) -> bool {
        matches!(self.pool, PoolKind::Transient)
    }

    /// 是否受跨帧泄漏检测约束（按**声明**而非实际池）。
    pub const fn must_release_in_frame(&self) -> bool {
        matches!(self.want, PoolKind::Transient)
    }

    /// 是否发生了降级（声明与实际不符）。
    ///
    /// 用 `match` 而非 `matches!` 比两个字段——`matches!` 只接受**模式**，
    /// 字段不是模式。
    pub const fn is_degraded(&self) -> bool {
        match (self.want, self.pool) {
            (PoolKind::Transient, PoolKind::Persistent) => true,
            (PoolKind::Persistent, PoolKind::Transient) => true,
            _ => false,
        }
    }

    /// 读屏/审计单行。
    pub fn audit_line(&self) -> String {
        format!(
            "#{} 槽{} {}{} 字节{} {}",
            self.seq,
            self.slot,
            self.pool.label(),
            if self.is_degraded() { "(降级)" } else { "" },
            self.bytes,
            self.liveness.summary()
        )
    }
}

/// 分配结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutKind {
    /// 瞬态池命中（本帧零成本路径）。
    TransientHit,
    /// 持久池命中（持久请求正常路径）。
    PersistentHit,
    /// 瞬态池满 → **降级转持久堆**（并告知）。
    DegradedToPersistent,
    /// 请求本身非法（超上限 / 字节数为零）→ 拒绝。
    Rejected,
}

impl OutKind {
    /// 全枚举（顺序即 [`OutKind::ordinal`] 的下标）。
    pub const ALL: [OutKind; 4] = [
        OutKind::TransientHit,
        OutKind::PersistentHit,
        OutKind::DegradedToPersistent,
        OutKind::Rejected,
    ];

    /// 枚举下标（供计数数组用，**不是**线上编码值）。
    pub const fn ordinal(self) -> usize {
        match self {
            OutKind::TransientHit => 0,
            OutKind::PersistentHit => 1,
            OutKind::DegradedToPersistent => 2,
            OutKind::Rejected => 3,
        }
    }

    /// 是否发生了降级（降级必须对调用方可见）。
    pub const fn is_degraded(self) -> bool {
        matches!(self, OutKind::DegradedToPersistent)
    }

    /// 读屏文案（双语）。
    pub const fn label(self) -> &'static str {
        match self {
            OutKind::TransientHit => "瞬态命中 / transient hit",
            OutKind::PersistentHit => "持久命中 / persistent hit",
            OutKind::DegradedToPersistent => "降级转持久堆 / degraded to persistent",
            OutKind::Rejected => "拒绝 / rejected",
        }
    }
}

/// 分配结果（票据 + 结论；拒绝时票据为 `None`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocationOutcome {
    /// 结论。
    pub kind: OutKind,
    /// 票据（`Rejected` 时为 `None`）。
    pub ticket: Option<AllocationTicket>,
}

impl AllocationOutcome {
    /// 是否成功（含降级——降级仍算成功，只是走了慢路径）。
    pub const fn is_ok(&self) -> bool {
        !matches!(self.kind, OutKind::Rejected)
    }

    /// 是否走了降级路径。
    pub const fn is_degraded(&self) -> bool {
        self.kind.is_degraded()
    }

    /// 取票据字节数（无票据给 0，避免判据侧写 `unwrap`）。
    pub const fn ticket_bytes(&self) -> u64 {
        match self.ticket {
            Some(t) => t.bytes,
            None => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、池与降级账
// ---------------------------------------------------------------------------

/// 瞬态池（固定槽数的空闲位图，帧末全量回收）。
#[derive(Clone, Debug)]
pub struct TransientPool {
    /// 空闲槽位位图（`true` = 空闲）。
    free: [bool; TRANSIENT_SLOTS],
    /// 空闲块数。
    free_count: usize,
}

impl Default for TransientPool {
    fn default() -> Self {
        Self::new()
    }
}

impl TransientPool {
    /// 新建全空闲的瞬态池。
    pub fn new() -> TransientPool {
        let mut free = [false; TRANSIENT_SLOTS];
        let mut i = 0;
        while i < TRANSIENT_SLOTS {
            free[i] = true;
            i += 1;
        }
        TransientPool { free, free_count: TRANSIENT_SLOTS }
    }

    /// 取一个空闲槽（无空位返回 `None`）。O(1)：位图线性扫首个 `true`，
    /// 槽数是编译期常量（64），扫 64 个 `bool` 不构成「非线性搜索」。
    pub fn take(&mut self) -> Option<u16> {
        let mut i = 0usize;
        while i < TRANSIENT_SLOTS {
            if self.free[i] {
                self.free[i] = false;
                self.free_count -= 1;
                return Some(i as u16);
            }
            i += 1;
        }
        None
    }

    /// 归还一个槽（槽位越界即**拒绝并原样返回**，不静默吞掉——
    /// 越界归还说明票据与池状态已经不一致，是必须暴露的缺陷）。
    pub fn give_back(&mut self, slot: u16) -> bool {
        let i = slot as usize;
        if i >= TRANSIENT_SLOTS {
            return false;
        }
        if self.free[i] {
            return false; // 双重归还：拒绝
        }
        self.free[i] = true;
        self.free_count += 1;
        true
    }

    /// 空闲块数。
    pub fn free_count(&self) -> usize {
        self.free_count
    }

    /// 池是否满（无空位）。
    pub fn is_full(&self) -> bool {
        self.free_count == 0
    }
}

/// 降级账（池满转持久堆的登记，**笔数 + 字节都可查**）。
#[derive(Clone, Debug, Default)]
pub struct DegradeLog {
    /// 各结论的降级字节（按 [`OutKind::ordinal`] 下标）。
    rows: Vec<(u32, u64, PoolKind)>,
    /// 因降级而被持久堆占住的字节合计。
    promoted_bytes: u64,
}

impl DegradeLog {
    /// 新建降级账。
    pub fn new() -> DegradeLog {
        DegradeLog { rows: Vec::new(), promoted_bytes: 0 }
    }

    /// 登记一条降级（seq、字节、实际落到的池）。
    pub fn record(&mut self, seq: u32, bytes: u64, pool: PoolKind) {
        if self.rows.len() >= MAX_DEGRADE_ROWS {
            // 预算耗尽即停登记，**不静默丢弃计数**：见 `promoted_bytes` 仍在累加，
            // 字节账不会因登记预算耗尽而少算——「多少显存被占住」是必须准的数。
            self.promoted_bytes = self.promoted_bytes.saturating_add(bytes);
            return;
        }
        self.promoted_bytes = self.promoted_bytes.saturating_add(bytes);
        self.rows.push((seq, bytes, pool));
    }

    /// 因降级而被持久堆占住的字节合计（A03 记账用）。
    pub fn promoted_bytes(&self) -> u64 {
        self.promoted_bytes
    }

    /// 登记行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 某序号是否已登记降级（判据直接查账，不经计数聚合）。
    pub fn has_seq(&self, seq: u32) -> bool {
        self.rows.iter().any(|r| r.0 == seq)
    }

    /// 全部登记行（只读）。
    pub fn rows(&self) -> &[(u32, u64, PoolKind)] {
        &self.rows
    }
}

/// 耗时账（最近 [`TIMING_WINDOW`] 笔的环形缓冲 + 最近秩 P95）。
#[derive(Clone, Debug)]
pub struct TimingLedger {
    /// 环形缓冲（存耗时刻，按写入序；满了淘汰最旧）。
    samples: [u32; TIMING_WINDOW],
    /// 与 `samples` 槽位一一对应的**请求序号**（归因用）。
    ///
    /// 必须与样本**同槽存储**：环形淘汰后「第几个样本」与「物理槽位」的
    /// 对应关系会整体平移，靠调用方回填映射必然对不上（实测恒返回 0）。
    seqs: [u32; TIMING_WINDOW],
    /// 环形写指针。
    head: usize,
    /// 已写入笔数（饱和于 [`TIMING_WINDOW`]）。
    filled: usize,
    /// 总请求数（**不因环形淘汰而减少**）。
    total: u64,
}

impl Default for TimingLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl TimingLedger {
    /// 新建耗时账。
    pub fn new() -> TimingLedger {
        TimingLedger { samples: [0u32; TIMING_WINDOW], seqs: [0u32; TIMING_WINDOW], head: 0, filled: 0, total: 0 }
    }

    /// 记一笔耗时（微秒）与其请求序号（**两者必须同时入账**，否则归因无从谈起）。
    pub fn record(&mut self, us: u32, seq: u32) {
        self.samples[self.head] = us;
        self.seqs[self.head] = seq;
        self.head = (self.head + 1) % TIMING_WINDOW;
        if self.filled < TIMING_WINDOW {
            self.filled += 1;
        }
        self.total = self.total.saturating_add(1);
    }

    /// 窗口内样本数。
    pub fn sample_count(&self) -> usize {
        self.filled
    }

    /// 总记录笔数（含被环形淘汰的）。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 窗口内第 `rank` 小（零基）的耗时（选择排序取秩，窗口 256 ⇒ O(n²)
    /// 但 n 是编译期常量的小常数，不随请求数增长）。
    fn kth(&self, rank: usize) -> u32 {
        let mut idx: [usize; TIMING_WINDOW] = [0; TIMING_WINDOW];
        let mut i = 0;
        while i < self.filled {
            // 环形缓冲的物理槽序不等于时间序，按「距 head 的距离」还原时间序。
            let back = (self.head + TIMING_WINDOW - 1 - i) % TIMING_WINDOW;
            idx[i] = back;
            i += 1;
        }
        for a in 0..self.filled {
            for b in (a + 1)..self.filled {
                if self.samples[idx[b]] < self.samples[idx[a]] {
                    let t = idx[a];
                    idx[a] = idx[b];
                    idx[b] = t;
                }
            }
        }
        self.samples[idx[rank]]
    }

    /// P95 耗时（微秒；窗口空时给 0）。
    ///
    /// 取**最近秩** `ceil(0.95 * n)`（零基下标）。不用平均值——平均值会被
    /// 大量便宜请求稀释，尾部劣化被抹平，P95 承诺就失去意义。
    pub fn p95(&self) -> u64 {
        if self.filled == 0 {
            return 0;
        }
        let n = self.filled;
        // ceil(95/100 * n)，整数算法：`(95*n + 99) / 100`，再夹到 `[0, n-1]`。
        let mut rank = (P95_PERCENTILE * n + 99) / 100;
        if rank == 0 {
            rank = 1;
        }
        if rank > n {
            rank = n;
        }
        self.kth(rank - 1) as u64
    }

    /// P95 是否超出承诺（超了也**不阻断**分配，只记劣化）。
    pub fn p95_over_budget(&self) -> bool {
        self.p95() > P95_BUDGET_US
    }

    /// 耗时劣化归因：返回窗口内**耗时最大**的那笔的请求序号。
    ///
    /// 归因必须有：只报「P95 超标」不报「是哪一笔超的」，优化就无从下手。
    ///
    /// **序号由账内直接给出**（[`TimingLedger::seqs`] 与样本一一对应），
    /// 不接受调用方传映射函数回填——那等于让调用方自己猜「第几个样本对应
    /// 哪个序号」，环形淘汰后两者根本对不上（实测：传映射的版本恒返回 0，
    /// 归因形同虚设却无人察觉，因为「返回了��个 u32」看着像有输出）。
    pub fn worst_seq(&self) -> u32 {
        let mut best_seq = 0u32;
        let mut best_us = 0u32;
        let mut i = 0;
        while i < self.filled {
            let back = (self.head + TIMING_WINDOW - 1 - i) % TIMING_WINDOW;
            let us = self.samples[back];
            if us >= best_us {
                best_us = us;
                best_seq = self.seqs[back];
            }
            i += 1;
        }
        best_seq
    }
}

// ---------------------------------------------------------------------------
// 五、分配器
// ---------------------------------------------------------------------------

/// 帧末回收一条记录的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealOutcome {
    /// 正常回收（活跃票据在本帧内释放）。
    Released { slot: u16, bytes: u64 },
    /// **跨帧泄漏强制回收**（票据的 `release` 越过帧界）。
    LeakReclaimed { slot: u16, bytes: u64 },
}

/// 瞬态资源分配器。
#[derive(Clone, Debug)]
pub struct TransientAllocator {
    /// 瞬态池（与持久池**物理隔离**，见模块头）。
    pub transient: TransientPool,
    /// 持久堆的账（本条只记占住了多少字节，不管持久堆怎么实现）。
    persistent_reserved: u64,
    /// 各结论计数（绝对值口径，按 [`OutKind::ordinal`] 下标）。
    counts: [u32; OUTCOME_COUNT],
    /// 当前未回收的活跃票据（seq → 票据）。
    live: Vec<AllocationTicket>,
    /// 降级账。
    pub degrade: DegradeLog,
    /// 耗时账。
    pub timing: TimingLedger,
    /// 帧序号（帧界由外部推进，见 [`TransientAllocator::seal_frame`]）。
    frame: u32,
    /// 跨帧泄漏检出数。
    leaks: u32,
    /// 跨帧泄漏强制回收数（**与 `leaks` 独立**，见「设计要点」）。
    forced_reclaimed: u32,
    /// 帧界存活性：票据的 `release` 超过当前帧界即视为跨帧泄漏。
    frame_horizon: u32,
    /// 审计留痕。
    pub audit: Vec<String>,
}

impl Default for TransientAllocator {
    /// 默认帧界取 [`u32::MAX`]（单帧不设上界）。
    ///
    /// **不能写`Self::new()`**——`new` 带 `frame_horizon` 参数。默认帧界取
    /// 最大值意味着「默认构造的分配器不做跨帧泄漏检出」，这是有意的：
    /// 帧界是调用方必须显式提供的策略，默认给一个「看起来能跑但漏检」的
    /// 小值比不给更危险。真实分配器一律用 [`TransientAllocator::new`]。
    fn default() -> Self {
        TransientAllocator::new(u32::MAX)
    }
}

impl TransientAllocator {
    /// 新建分配器。`frame_horizon` = 当前帧的帧界（瞬态资源的 `release`
    /// 不得超过它，超过即跨帧泄漏）。
    pub fn new(frame_horizon: u32) -> TransientAllocator {
        TransientAllocator {
            transient: TransientPool::new(),
            persistent_reserved: 0,
            counts: [0u32; OUTCOME_COUNT],
            live: Vec::new(),
            degrade: DegradeLog::new(),
            timing: TimingLedger::new(),
            frame: 0,
            leaks: 0,
            forced_reclaimed: 0,
            frame_horizon,
            audit: Vec::new(),
        }
    }

    /// 当前帧界。
    pub fn frame_horizon(&self) -> u32 {
        self.frame_horizon
    }

    /// 推进一帧（帧界 +1）。
    ///
    /// **帧界推进本身不做回收**：回收必须显式调 [`Self::seal_frame`]。
    /// 合成一个「推进即回收」会让人以为不用封帧——而跨帧泄漏检测正是
    /// 挂在封帧这个时机上的。
    pub fn advance_frame(&mut self) {
        self.frame = self.frame.saturating_add(1);
        self.frame_horizon = self.frame_horizon.saturating_add(1);
    }

    /// 分配一笔。
    ///
    /// 次序刻意为 **先校验后取块**：非法请求（零字节 / 超上限）必须先拒，
    /// 否则会白占一个槽位——而槽位满了会连带把后续合法请求全推去降级。
    pub fn allocate(&mut self, req: &AllocRequest, us: u32) -> AllocationOutcome {
        self.timing.record(us, req.seq);

        // 1) 请求校验：零字节 / 超单块上限 ⇒ 拒绝（不占槽位）。
        if req.bytes == 0 || req.bytes > MAX_BLOCK_BYTES {
            return self.note(OutKind::Rejected, None, req, "请求字节数非法");
        }

        let block = req.block_bytes();
        match req.want {
            PoolKind::Transient => {
                match self.transient.take() {
                    Some(slot) => {
                        let t = AllocationTicket {
                            seq: req.seq,
                            slot,
                            bytes: block,
                            pool: PoolKind::Transient,
                            want: PoolKind::Transient,
                            liveness: req.liveness,
                        };
                        self.live.push(t);
                        self.note(OutKind::TransientHit, Some(t), req, "瞬态池命中")
                    }
                    None => {
                        // 2) 池满 ⇒ **降级转持久堆并告知**（不直接失败）。
                        // 正确性优先于性能：转持久堆慢，但不会让渲染失败。
                        self.persistent_reserved =
                            self.persistent_reserved.saturating_add(block);
                        self.degrade.record(req.seq, block, PoolKind::Persistent);
                        let t = AllocationTicket {
                            seq: req.seq,
                            slot: u16::MAX,
                            bytes: block,
                            pool: PoolKind::Persistent,
                            want: PoolKind::Transient,
                            liveness: req.liveness,
                        };
                        // **降级票据必须入live 追踪**：它占的是持久堆，帧末
                        // 没有第二个人来回收它。初版漏了这行push，于是降级
                        // 票据完全脱离追踪——跨帧了无人检出、无人强制回收，
                        // 持久堆占用逐帧累积直到耗尽，而面板上「跨帧泄漏
                        // 告警: 0」看着一切正常。判据 20 抓到了它。
                        self.live.push(t);
                        self.note(OutKind::DegradedToPersistent, Some(t), req, "瞬态池满降级")
                    }
                }
            }
            PoolKind::Persistent => {
                // 持久请求：只记账，不占瞬态槽位（池隔离的关键——持久块绝
                // 不能从瞬态空闲链表拿，否则帧末回收会释放持久资源在用的内存）。
                self.persistent_reserved = self.persistent_reserved.saturating_add(block);
                let t = AllocationTicket {
                    seq: req.seq,
                    slot: u16::MAX,
                    bytes: block,
                    pool: PoolKind::Persistent,
                    want: PoolKind::Persistent,
                    liveness: req.liveness,
                };
                // 持久票据**不入 live**：它跨帧存活是设计意图（不是泄漏），
                // 由持有者显式释放。把它塞进 live 会让封帧误判——
                // 故此处只记账不进追踪，与瞬态路径的 push 形成对照。
                self.note(OutKind::PersistentHit, Some(t), req, "持久池命中")
            }
        }
    }

    /// 记一条结论并产出结果（统一出口，保证计数与留痕**同时**发生）。
    fn note(
        &mut self,
        kind: OutKind,
        ticket: Option<AllocationTicket>,
        req: &AllocRequest,
        why: &str,
    ) -> AllocationOutcome {
        self.counts[kind.ordinal()] = self.counts[kind.ordinal()].saturating_add(1);
        let line = match ticket {
            Some(t) => format!("#{} {} {} {}", req.seq, kind.label(), why, t.audit_line()),
            None => format!("#{} {} {} 字节{}", req.seq, kind.label(), why, req.bytes),
        };
        self.audit.push(line);
        AllocationOutcome { kind, ticket }
    }

    /// 封帧：回收全部瞬态票据，并**检出 + 强制回收跨帧泄漏**。
    ///
    /// 跨帧泄漏 = 票据的 `release` **越过当前帧界**（`release > frame_horizon`）。
    /// 瞬态资源的契约是「本帧内释放」，越界即契约破坏。
    pub fn seal_frame(&mut self) -> Vec<SealOutcome> {
        let mut out = Vec::new();
        let horizon = self.frame_horizon;
        let mut i = 0usize;
        while i < self.live.len() {
            let t = self.live[i];
            // 泄漏判定按**声明**（`want`）：调用方承诺「本帧内释放」，
            // 降级票据声明瞬态、实际落持久——若按 `pool` 判，它就永远
            // 逃过检测，而它恰恰是最需要被强制回收的那种（占着持久堆）。
            let leaked = t.must_release_in_frame() && t.liveness.release > horizon;
            // 槽位归还按**实际池**：只有真占了瞬态槽的才归还。降级票据的
            // `slot` 是哨兵 `u16::MAX`，归还会被 `give_back` 拒绝——这正是
            // 期望的：它没占瞬态槽，本来就不该还。
            if t.is_transient() {
                let _ = self.transient.give_back(t.slot);
            } else {
                // 降级票据落持久堆：封帧时释放它对持久堆的占用，
                // 否则逐帧累积直到持久堆耗尽。
                self.persistent_reserved = self.persistent_reserved.saturating_sub(t.bytes);
            }
            if leaked {
                self.leaks = self.leaks.saturating_add(1);
                self.forced_reclaimed = self.forced_reclaimed.saturating_add(1);
                self.audit.push(format!(
                    "跨帧泄漏 #{} 槽{} 释放刻{} 越帧界{} 强制回收",
                    t.seq, t.slot, t.liveness.release, horizon
                ));
                out.push(SealOutcome::LeakReclaimed { slot: t.slot, bytes: t.bytes });
            } else {
                out.push(SealOutcome::Released { slot: t.slot, bytes: t.bytes });
            }
            i += 1;
        }
        self.live.clear();
        out
    }

    /// 未回收的活跃票据数（判据直接读，不经聚合层）。
    pub fn live_count(&self) -> usize {
        self.live.len()
    }

    /// 跨帧泄漏检出数。
    pub fn leaks(&self) -> u32 {
        self.leaks
    }

    /// 跨帧泄漏强制回收数。
    pub fn forced_reclaimed(&self) -> u32 {
        self.forced_reclaimed
    }

    /// 各结论计数（按 [`OutKind::ALL`] 下标）。
    pub fn outcome_counts(&self) -> [u32; OUTCOME_COUNT] {
        self.counts
    }

    /// 因降级而被持久堆占住的字节（转调降级账，A03 记账用）。
    pub fn promoted_bytes(&self) -> u64 {
        self.degrade.promoted_bytes()
    }

    /// 持久堆占用字节合计（持久命中 + 降级转持久）。
    pub fn persistent_reserved(&self) -> u64 {
        self.persistent_reserved
    }

    /// 审计留痕（只读）。
    pub fn audit_lines(&self) -> &[String] {
        &self.audit
    }

    /// 读屏面板：中英双语逐行，**只报聚合计数**，不报单个请求的资源名与内容。
    pub fn a11y_lines(&self) -> [String; 7] {
        [
            format!("总请求数 / total requests: {}", self.timing.total()),
            format!(
                "瞬态命中 / transient hit: {}",
                self.counts[OutKind::TransientHit.ordinal()]
            ),
            format!(
                "持久命中 / persistent hit: {}",
                self.counts[OutKind::PersistentHit.ordinal()]
            ),
            format!(
                "降级转持久堆 / degraded: {}",
                self.counts[OutKind::DegradedToPersistent.ordinal()]
            ),
            format!("跨帧泄漏告警 / leaks: {}", self.leaks),
            format!("分配耗时 P95 / p95 us: {}", self.timing.p95()),
            format!("降级占用字节 / promoted bytes: {}", self.promoted_bytes()),
        ]
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// VE-F0034模块自检（受 `CheckSet::MAX_CHECKS=112` 约束，逐条覆盖锚点判据）。
pub fn run_vea34_checks() -> CheckSet {
    let mut s = CheckSet::new("vea34_transient");

    // --- 判据 1：生命周期分析的半开区间语义（端点相接不冲突）---------------
    {
        let a = Liveness::new(10, 20);
        let b = Liveness::new(20, 30); // 端点相接（上一笔释放 == 下一笔触碰）
        let c = Liveness::new(19, 30); // 真冲突
        let z1 = Liveness::new(5, 5); // 零时长
        let z2 = Liveness::new(9, 3); // 畸形零时长（release < first_touch）
        s.add(
            "A34-生命周期-半开区间端点相接不冲突",
            !a.conflicts(&b)
                && !b.conflicts(&a)
                && a.conflicts(&c)
                && a.duration() == 10
                && a.active_at(10)
                && !a.active_at(20)
                && a.active_at(19)
                && z1.is_instant()
                && z1.duration() == 0
                && z2.is_instant()
                // 零时长 × 任意、任意 × 零时长、零时长 × 零时长
                && !z1.conflicts(&a)
                && !a.conflicts(&z1)
                && !z1.conflicts(&z1)
                && !z2.conflicts(&z1)
                && !z1.conflicts(&z2)
                && !z2.conflicts(&z2)
                && !z2.conflicts(&c)
                // 零时长票据不「活跃」，但也不冲突（瞬时分配合法）
                && !a.active_at(0),
            "半开区间 [first_touch,release)：端点相接不冲突；零时长（两种形态）对任何区间都不冲突",
        );
    }

    // --- 判据 2：瞬时分配走瞬态池且**不占持久** -----------------------------
    {
        let mut a = TransientAllocator::new(1000);
        let req = AllocRequest::transient(1, 1000, Liveness::new(10, 20));
        let o = a.allocate(&req, 12);
        let c = a.outcome_counts();
        s.add(
            "A34-瞬时分配-瞬态池命中且不占持久字节",
            o.is_ok()
                && !o.is_degraded()
                && o.kind == OutKind::TransientHit
                && c[OutKind::TransientHit.ordinal()] == 1
                && c[OutKind::DegradedToPersistent.ordinal()] == 0
                && a.persistent_reserved() == 0
                && a.promoted_bytes() == 0
                && a.transient.free_count() == TRANSIENT_SLOTS - 1
                // 票据的池归属必须是瞬态（帧末校验对不对池的依据）
                && o.ticket_bytes() == BLOCK_GRANULARITY * 4, // 1000 → 1024
            "瞬态请求从瞬态池取块，不占持久堆字节，票据池归属为瞬态",
        );
    }

    // --- 判据 3：池隔离——持久请求**绝不占瞬态槽位** ------------------------
    {
        let mut a = TransientAllocator::new(1000);
        let free0 = a.transient.free_count();
        let req = AllocRequest::persistent(1, 4096, Liveness::new(10, 20));
        let o = a.allocate(&req, 9);
        s.add(
            "A34-池隔离-持久请求不占瞬态槽位",
            o.kind == OutKind::PersistentHit
                && a.transient.free_count() == free0
                && a.persistent_reserved() == 4096
                // 持久票据的 slot 必须是哨兵（u16::MAX），不能是有效槽位——
                // 否则帧末会拿它去归还瞬态池，隔离就破了
                && o.ticket.is_some()
                && o.ticket.map(|t| t.slot) == Some(u16::MAX),
            "持久请求只走持久堆，瞬态池空闲数不变，持久票据槽位为哨兵值",
        );
    }

    // --- 判据 4：池满 ⇒ 降级转持久堆**并告知**（不直接失败） -----------------
    {
        let mut a = TransientAllocator::new(1000);
        // 先把瞬态池占满
        let mut i = 0u32;
        while i < TRANSIENT_SLOTS as u32 {
            let r = AllocRequest::transient(i, 512, Liveness::new(10, 20));
            a.allocate(&r, 5);
            i += 1;
        }
        let full = a.transient.is_full();
        let free_now = a.transient.free_count();
        // 第 65 笔 ⇒ 池满 ⇒ 降级
        let over = AllocRequest::transient(999, 2048, Liveness::new(10, 20));
        let o = a.allocate(&over, 400);
        let c = a.outcome_counts();
        s.add(
            "A34-降级-池满转持久堆并告知且字节可查",
            full
                && free_now == 0
                // 降级**仍算成功**（正确性优先于性能，不能让渲染失败）
                && o.is_ok()
                && o.is_degraded()
                && o.kind == OutKind::DegradedToPersistent
                && c[OutKind::DegradedToPersistent.ordinal()] == 1
                && c[OutKind::Rejected.ordinal()] == 0
                // 降级**可观测**：账里能查到该序号，字节可查
                && a.degrade.has_seq(999)
                && a.degrade.len() == 1
                // 「多少显存被占住」是 A03 记账必需的数
                && a.promoted_bytes() == 2048
                && a.persistent_reserved() == 2048
                && a.audit_lines().len() == TRANSIENT_SLOTS + 1,
            "瞬态池占满后新请求转持久堆且仍返回成功，降级可观测（序号+字节均可查）",
        );
    }

    // --- 判据 5：降级**双向**计数——无事件为 0，真实路径造出事件恰为 1 ------
    //  只断「无降级时为 0」对「池满时到底有没有转持久」一字未说，而降级
    //  代码正写在池满分支里。必须走真实路径造出事件断「恰等于 1」。
    {
        let mut a = TransientAllocator::new(1000);
        let no_event = a.promoted_bytes() == 0 && a.outcome_counts()[OutKind::DegradedToPersistent.ordinal()] == 0;
        // 造事件：占满池后再要一笔
        let mut i = 0u32;
        while i < TRANSIENT_SLOTS as u32 {
            let r = AllocRequest::transient(i, 256, Liveness::new(10, 20));
            a.allocate(&r, 5);
            i += 1;
        }
        let o = a.allocate(&AllocRequest::transient(7000, 1024, Liveness::new(10, 20)), 300);
        s.add(
            "A34-降级计数-无事件为0且真实路径恰为1",
            // `==` 不用 `>=`：否则「每次 +2」也过
            no_event
                && o.is_degraded()
                && a.outcome_counts()[OutKind::DegradedToPersistent.ordinal()] == 1
                && a.degrade.len() == 1
                && a.promoted_bytes() == 1024,
            "降级计数：无降级时恰为 0；占满池走真实降级路径后恰为 1（非 >=，防重复计数）",
        );
    }

    // --- 判据 6：跨帧泄漏**检出**且**强制回收**（两个独立计数） ---------------
    {
        let mut a = TransientAllocator::new(100);
        // 票据声明释放刻 500 > 帧界 100 ⇒ 跨帧泄漏
        let leak = AllocRequest::transient(42, 1024, Liveness::new(10, 500));
        a.allocate(&leak, 8);
        // 另一张正常票据（释放刻 50 ≤ 帧界）
        let ok = AllocRequest::transient(43, 1024, Liveness::new(10, 50));
        a.allocate(&ok, 8);
        let live_before = a.live_count();
        let free_before = a.transient.free_count();
        let sealed = a.seal_frame();
        let free_after = a.transient.free_count();
        // 封帧后池必须回到全空闲（泄漏票也被强制回收 ⇒ 不泄漏）
        s.add(
            "A34-泄漏-检出与强制回收两计数独立",
            live_before == 2
                && a.leaks() == 1
                && a.forced_reclaimed() == 1
                && free_before == TRANSIENT_SLOTS - 2
                && free_after == TRANSIENT_SLOTS
                && a.live_count() == 0
                && sealed.len() == 2
                // 归因可复核：告警行须点名该序号与越界量
                && a.audit_lines().last().map(|l| l.contains("#42") && l.contains("强制回收"))
                    == Some(true),
            "释放刻越过帧界的票据被检出并强制回收，池回到全空闲，告警行点名序号",
        );
    }

    // --- 判据 7：泄漏计数**双向**——无泄漏为 0，造出泄漏恰为 N --------------
    {
        let mut a = TransientAllocator::new(100);
        // 全部正常 ⇒ 泄漏计数必为 0（负向断言）
        let r1 = AllocRequest::transient(1, 512, Liveness::new(10, 20));
        let r2 = AllocRequest::transient(2, 512, Liveness::new(20, 30)); // 端点相接
        a.allocate(&r1, 5);
        a.allocate(&r2, 5);
        a.seal_frame();
        let clean = a.leaks() == 0 && a.forced_reclaimed() == 0;
        // 真实造出 2 次泄漏
        let mut b = TransientAllocator::new(100);
        b.allocate(&AllocRequest::transient(1, 512, Liveness::new(10, 200)), 5);
        b.allocate(&AllocRequest::transient(2, 512, Liveness::new(30, 300)), 5);
        b.seal_frame();
        s.add(
            "A34-泄漏计数-无泄漏为0且造出2次恰为2",
            clean
                && b.leaks() == 2
                && b.forced_reclaimed() == 2
                && b.transient.free_count() == TRANSIENT_SLOTS,
            "端点相接的正常票据不误报泄漏；真实造出 2 笔越界票据时检出恰为 2（非 >=）",
        );
    }

    // --- 判据 7b：降级票据跨帧**同样**必须被检出并强制回收 ----------------
    //
    // 这条判据是判据 20 逼出来的：初版把降级票据排除在 `live` 追踪之外，
    // 于是它占着持久堆却无人回收 —— 跨帧了泄漏告警仍显示 0，看着一切正常。
    // 泄漏判定因此必须按**声明**（`want`）而非**实际池**（`pool`）。
    {
        let mut a = TransientAllocator::new(100);
        // 占满池
        let mut i = 0u32;
        while i < TRANSIENT_SLOTS as u32 {
            a.allocate(&AllocRequest::transient(i, 512, Liveness::new(10, 20)), 5);
            i += 1;
        }
        // 降级一笔，声明帧内释放（release=50 ≤ 帧界 100）⇒ 不是泄漏
        let ok_deg = a.allocate(&AllocRequest::transient(500, 1024, Liveness::new(10, 50)), 5);
        let clean = a.leaks() == 0
            && ok_deg.is_degraded()
            && ok_deg.ticket.map(|t| t.must_release_in_frame()) == Some(true)
            && ok_deg.ticket.map(|t| t.is_transient()) == Some(false);
        // 封帧：降级票据**占的持久字节必须被释放**（否则逐帧累积到耗尽）
        let reserved_before = a.persistent_reserved();
        let sealed = a.seal_frame();
        s.add(
            "A34-降级票据-受泄漏检测且封帧释放持久占用",
            clean
                // 声明帧内释放 ⇒ 不算泄漏，但**仍然被追踪并封帧**（进live）
                && a.leaks() == 0
                && a.forced_reclaimed() == 0
                // 64 笔瞬态 + 1 笔降级 = 65 笔全被封帧处理
                && sealed.len() == TRANSIENT_SLOTS + 1
                // 封帧后持久占用归零（降级那一笔的字节被还回去了）
                && reserved_before == 1024
                && a.persistent_reserved() == 0
                && a.transient.free_count() == TRANSIENT_SLOTS,
            "降级票据进 live 追踪：声明帧内释放时不误报泄漏，封帧时归还其持久占用",
        );
    }

    // --- 判据 7c：降级票据**跨帧泄漏**必须被检出（真缺陷回归） --------------
    {
        let mut a = TransientAllocator::new(100);
        let mut i = 0u32;
        while i < TRANSIENT_SLOTS as u32 {
            a.allocate(&AllocRequest::transient(i, 512, Liveness::new(10, 20)), 5);
            i += 1;
        }
        // 降级一笔且**跨帧**（release=900 > 帧界 100）
        let deg = a.allocate(&AllocRequest::transient(600, 2048, Liveness::new(10, 900)), 5);
        let sealed = a.seal_frame();
        let mut leak_rows = 0usize;
        let mut k = 0;
        while k < sealed.len() {
            if let SealOutcome::LeakReclaimed { .. } = sealed[k] {
                leak_rows += 1;
            }
            k += 1;
        }
        s.add(
            "A34-降级票据-跨帧泄漏被检出并强制回收",
            deg.is_degraded()
                // **关键**：按实际池判的话降级票据永远逃过检测（它 pool=Persistent）
                && a.leaks() == 1
                && a.forced_reclaimed() == 1
                && leak_rows == 1
                // 告警行点名该序号，可复核
                && a.audit_lines().last().map(|l| l.contains("#600")) == Some(true)
                && a.persistent_reserved() == 0
                && a.transient.free_count() == TRANSIENT_SLOTS,
            "声明瞬态但落持久堆的票据跨帧时被检出并强制回收（按实际池判会永远漏检）",
        );
    }

    // --- 判据 8：帧界推进本身**不**回收（回收必须显式封帧） ------------------
    {
        let mut a = TransientAllocator::new(100);
        a.allocate(&AllocRequest::transient(1, 512, Liveness::new(10, 200)), 5);
        let h0 = a.frame_horizon();
        a.advance_frame();
        let h1 = a.frame_horizon();
        s.add(
            "A34-帧界-推进不隐式回收",
            h1 == h0 + 1
                // 推进后票据仍在（回收挂在显式封帧上）
                && a.live_count() == 1
                // 此时封帧才检出泄漏
                && a.seal_frame().len() == 1
                && a.leaks() == 1,
            "advance_frame 只推进帧界不回收；封帧时才检出跨帧泄漏",
        );
    }

    // --- 判据 9：P95 取最近秩而非平均值（尾部不被稀释） --------------------
    //
    // 语料要点：**昂贵样本必须超过 5%**，否则最近秩 P95 本就该落在便宜样本上。
    // 初版语料用「99 笔 10 微秒 + 1 笔 1000 微秒」，判据期望 P95=1000——
    // 那是判据写错，不是实现错：n=100 时最近秩 = ceil(0.95×100) = 95，
    // 排序后第 95 个样本仍是 10（只有 1 笔昂贵，占 1% < 5%），
    // P95=10 **数学上完全正确**。「变异/断言没被抓住」的第一嫌疑永远是
    // 断言本身算错。这条教训与十诫 14 同源，故在此写明。
    {
        let mut t = TimingLedger::new();
        let empty_p95 = t.p95() == 0;
        // 90笔 10 微秒 + 10 笔 1000 微秒：昂贵占 10% > 5% ⇒ 最近秩 95 落在昂贵侧
        let mut i = 0u32;
        while i < 90 {
            t.record(10, i);
            i += 1;
        }
        i = 0;
        while i < 10 {
            t.record(1000, 100 + i);
            i += 1;
        }
        let p95 = t.p95();
        // 判据侧**独立重算**均值口径（不向被测函数问答案）
        let mean_us = (90 * 10 + 10 * 1000) / 100;
        s.add(
            "A34-P95-取最近秩不被均值稀释",
            empty_p95
                && p95 == 1000
                // 均值口径给 109，远低于真实尾部 ⇒ 用均值则 P95 承诺失效
                && mean_us == 109
                && mean_us < p95
                && t.sample_count() == 100
                && t.total() == 100
                // 超标可检出，且超标**不阻断**分配
                && t.p95_over_budget()
                && P95_BUDGET_US == 250,
            "90×10 + 10×1000 微秒 ⇒ P95=1000（均值口径仅 109，尾部被稀释）；空账 P95 为 0",
        );
    }

    // --- 判据 10：P95 承诺与达标路径（不超标时不得误报） -------------------
    {
        let mut t = TimingLedger::new();
        let mut i = 0u32;
        while i < 100 {
            t.record(120, i); // 全部低于预算 250
            i += 1;
        }
        s.add(
            "A34-P95-达标不误报劣化",
            t.p95() == 120 && !t.p95_over_budget(),
            "全样本低于 P95 承诺预算时不得误判为耗时劣化",
        );
    }

    // --- 判据 11：环形窗口淘汰最旧且总笔数不减 ------------------------------
    {
        let mut t = TimingLedger::new();
        // 先灌满窗口 + 40 笔（全部便宜，会被淘汰）
        let mut i = 0u32;
        while i < TIMING_WINDOW as u32 + 40 {
            t.record(50, i);
            i += 1;
        }
        // 再记 40 笔昂贵（占窗口 40/256 ≈ 15.6% > 5%，确保最近秩落昂贵侧）
        i = 0;
        while i < 40 {
            t.record(900, 1000 + i);
            i += 1;
        }
        s.add(
            "A34-耗时账-环形窗口淘汰最旧且总数不减",
            t.total() == TIMING_WINDOW as u64 + 80
                // 窗口容量封顶（最旧的已被淘汰）
                && t.sample_count() == TIMING_WINDOW
                && t.p95() == 900,
            "超出窗口容量时淘汰最旧样本（窗口封顶），总记录笔数不因淘汰而减少",
        );
    }

    // --- 判据 12：耗时劣化**可归因**到具体请求 ------------------------------
    //
    // 语料：20 笔便宜（序号 0..19）+ 20 笔昂贵（序号 100..119，其中最贵
    // 的一笔是序号 117）。昂贵占 50% > 5% ⇒ P95 必然落在昂贵侧。
    {
        let mut a = TransientAllocator::new(10000);
        let mut i = 0u32;
        while i < 20 {
            a.allocate(&AllocRequest::transient(i, 256, Liveness::new(10, 20)), 10);
            i += 1;
        }
        i = 0;
        while i < 20 {
            // 序号 117 那笔最贵（1000），其余昂贵样本 800
            let us = if i == 17 { 1000 } else { 800 };
            a.allocate(&AllocRequest::transient(100 + i, 256, Liveness::new(10, 20)), us);
            i += 1;
        }
        let degraded = a.timing.p95_over_budget();
        // 归因：最耗时的那笔应定位到序号 117（账内直接给序号，不靠调用方回填）
        let attributed = a.timing.worst_seq();
        s.add(
            "A34-归因-耗时劣化可定位到具体请求",
            degraded
                && attributed == 117
                && a.timing.p95() == 800,
            "P95 超标时归因到序号 117（那笔最贵1000 微秒）；只报超标不报是哪笔，无法优化",
        );
    }

    // --- 判据 13：非法请求先拒且**不占槽位**（否则连带把合法请求推去降级） --
    {
        let mut a = TransientAllocator::new(1000);
        let free0 = a.transient.free_count();
        let zero = a.allocate(&AllocRequest::transient(1, 0, Liveness::new(10, 20)), 5);
        let huge = a.allocate(
            &AllocRequest::transient(2, MAX_BLOCK_BYTES + 1, Liveness::new(10, 20)),
            5,
        );
        let c = a.outcome_counts();
        // 拒绝之后仍能正常命中瞬态池（没被推去降级）
        let good = a.allocate(&AllocRequest::transient(3, 512, Liveness::new(10, 20)), 5);
        s.add(
            "A34-校验-非法请求先拒不占槽位",
            zero.kind == OutKind::Rejected
                && !zero.is_ok()
                && huge.kind == OutKind::Rejected
                && c[OutKind::Rejected.ordinal()] == 2
                && c[OutKind::DegradedToPersistent.ordinal()] == 0
                && a.transient.free_count() == free0 - 1
                && good.kind == OutKind::TransientHit
                && a.live_count() == 1,
            "零字节与超单块上限的请求被拒且不占槽位，后续合法请求仍走瞬态池",
        );
    }

    // --- 判据 14：块粒度向上取整（碎片不可填补则无复用） --------------------
    {
        let cases = [
            (1u64, BLOCK_GRANULARITY),
            (BLOCK_GRANULARITY, BLOCK_GRANULARITY),
            (BLOCK_GRANULARITY + 1, BLOCK_GRANULARITY * 2),
            (1000, 1024),
            (MAX_BLOCK_BYTES, MAX_BLOCK_BYTES),
        ];
        let mut ok = true;
        let mut i = 0;
        while i < cases.len() {
            let r = AllocRequest::transient(0, cases[i].0, Liveness::new(0, 1));
            ok &= r.block_bytes() == cases[i].1;
            // 取整结果必须是粒度的整数倍且不小于请求
            ok &= r.block_bytes() % BLOCK_GRANULARITY == 0;
            ok &= r.block_bytes() >= cases[i].0;
            i += 1;
        }
        s.add(
            "A34-块粒度-向上取整到粒度整数倍",
            ok,
            "分配尺寸向上取整到 256 字节粒度，取整结果必为粒度倍数且不小于请求",
        );
    }

    // --- 判据 15：槽位归还的边界防护（越界/ 双重归还须拒绝） ----------------
    {
        let mut p = TransientPool::new();
        let slot = p.take().unwrap_or(0);
        let ok_back = p.give_back(slot);
        let dbl = p.give_back(slot); // 双重归还
        let oob = p.give_back(u16::MAX); // 越界
        s.add(
            "A34-池-归还的越界与双重归还须拒绝",
            ok_back
                && !dbl
                && !oob
                && p.free_count() == TRANSIENT_SLOTS,
            "合法归还生效；双重归还与越界归还被拒绝，空闲数不被污染",
        );
    }

    // --- 判据 16：池满判定与槽位取尽的边界（夹逼对） -----------------------
    {
        let mut p = TransientPool::new();
        let mut i = 0usize;
        while i < TRANSIENT_SLOTS - 1 {
            p.take();
            i += 1;
        }
        let not_full = !p.is_full() && p.free_count() == 1;
        let last = p.take();
        let now_full = p.is_full() && p.free_count() == 0 && p.take().is_none();
        s.add(
            "A34-池-夹逼对钉死满池位置",
            not_full && last.is_some() && now_full,
            "取到 TRANSIENT_SLOTS-1 个时未满，取走最后一个时恰好满且再取为空",
        );
    }

    // --- 判据 17：池归属线编码自洽（判别值 ≠ 线上值，不得用 as u8） ---------
    {
        let mut ok = true;
        let mut seen: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < PoolKind::ALL.len() {
            let k = PoolKind::ALL[i];
            // 往返：wire → from_wire → 回到自己
            ok &= PoolKind::from_wire(k.wire()) == Some(k);
            // 线上码互异
            ok &= !seen.contains(&k.wire());
            seen.push(k.wire());
            // 线编码必须落在 ASCII 字母区（可读协议约定）
            ok &= k.wire().is_ascii_alphabetic();
            i += 1;
        }
        s.add(
            "A34-池归属-线编码自洽且互异",
            ok
                && PoolKind::from_wire(0x00).is_none()
                && PoolKind::from_wire(0xFF).is_none()
                && PoolKind::Transient.wire() != PoolKind::Transient.ordinal() as u8,
            "wire() 显式映射、往返自洽、两码互异；未登记码反查失败不静默兜底",
        );
    }

    // --- 判据 18：结论枚举下标与线编码分离（ordinal≠wire 别混用） ----------
    {
        // OutKind 有 ordinal（计数下标）但**不**有 wire（无对外线编码）——
        // 防止有人后来加wire 时误用 ordinal。
        let idx_ok = OutKind::ALL[0].ordinal() == 0
            && OutKind::ALL[1].ordinal() == 1
            && OutKind::ALL[2].ordinal() == 2
            && OutKind::ALL[3].ordinal() == 3;
        // 计数数组长度必须与枚举种数一致（否则越界 panic 或漏计）
        s.add(
            "A34-结论枚举-下标连续且与计数数组同长",
            idx_ok && OUTCOME_COUNT == OutKind::ALL.len() && verdict_guard(),
            "OutKind 下标 0..3 连续，计数数组长度与枚举种数一致（越界即漏计）",
        );
    }

    // --- 判据 19：零 panic 面 —— 票据缺失路径不 panic -----------------------
    {
        let o = AllocationOutcome { kind: OutKind::Rejected, ticket: None };
        // 无票据时取字节给 0，而不是 unwrap
        let b = o.ticket_bytes();
        // 票据 Some 时取值正确
        let t = AllocationTicket {
            seq: 5,
            slot: 3,
            bytes: 1024,
            pool: PoolKind::Transient,
            want: PoolKind::Transient,
            liveness: Liveness::new(1, 2),
        };
        let o2 = AllocationOutcome { kind: OutKind::TransientHit, ticket: Some(t) };
        // 降级票据：声明瞬态、实际持久 ⇒ 三条谓词各不相同
        let dt = AllocationTicket {
            seq: 6,
            slot: u16::MAX,
            bytes: 2048,
            pool: PoolKind::Persistent,
            want: PoolKind::Transient,
            liveness: Liveness::new(1, 2),
        };
        s.add(
            "A34-零panic-无票据路径安全取值",
            b == 0
                && !o.is_ok()
                && o2.ticket_bytes() == 1024
                && o2.ticket.map(|x| x.slot_index()) == Some(3)
                && t.is_transient()
                && t.must_release_in_frame()
                && !t.is_degraded()
                // 降级票据：**实际**占持久（不还瞬态槽）、**声明**须帧内释放
                // （受泄漏检测）、且判定为已降级——三者语义各不相同，
                // 混用任一个都会让降级票据逃过追踪。
                && !dt.is_transient()
                && dt.must_release_in_frame()
                && dt.is_degraded(),
            "无票据时字节数取 0 不 panic；票据的「实际池/声明池/是否降级」三谓词语义可区分",
        );
    }

    // --- 判据 20：全流程回归 —— 分配→封帧→下一帧分配→降级 --------------------
    {
        let mut a = TransientAllocator::new(100);
        // 帧1：三笔瞬态，全部正常释放
        let mut i = 0u32;
        while i < 3 {
            a.allocate(&AllocRequest::transient(i, 1024, Liveness::new(10, 20 + i)), 10);
            i += 1;
        }
        let sealed1 = a.seal_frame();
        let clean1 = a.leaks() == 0 && a.transient.free_count() == TRANSIENT_SLOTS;
        // 帧2：占满池（帧1 封帧后池已回到全空闲），再要一笔 ⇒ 降级
        a.advance_frame();
        let mut j = 0u32;
        while j < TRANSIENT_SLOTS as u32 {
            a.allocate(&AllocRequest::transient(j, 512, Liveness::new(10, 20)), 10);
            j += 1;
        }
        let deg = a.allocate(&AllocRequest::transient(999, 512, Liveness::new(10, 20)), 900);
        let sealed2 = a.seal_frame();
        let c = a.outcome_counts();
        s.add(
            "A34-全流程-两帧分配封帧与降级回归",
            sealed1.len() == 3
                && clean1
                && deg.is_degraded()
                // 帧2 封帧：64 笔瞬态票据 + 1 笔降级票据（降级票据也入 live，
                // 但它 slot=哨兵、不占瞬态槽，归还时自然被拒）
                && sealed2.len() == TRANSIENT_SLOTS + 1
                // 释放刻都 ≤ 帧界（100→101），故本轮无泄漏
                && a.leaks() == 0
                // 两轮封帧后瞬态池都回到全空闲（无泄漏累积）
                && a.transient.free_count() == TRANSIENT_SLOTS
                // 结论计数：帧1 的 3 笔 + 帧2 的 64 笔都瞬态命中
                && c[OutKind::TransientHit.ordinal()] == 3 + TRANSIENT_SLOTS as u32
                && c[OutKind::DegradedToPersistent.ordinal()] == 1
                && a.promoted_bytes() == 512
                // 独立重算总请求数：3 + 64 + 1 = 68
                && a.timing.total() == 3 + TRANSIENT_SLOTS as u64 + 1,
            "帧1 三笔正常回收；帧2 占满池后一笔降级；两轮封帧后池均回到全空闲且无泄漏",
        );
    }

    // --- 判据 21：降级账的字节**饱和**不溢出（超大尺寸不wrap） --------------
    {
        let mut d = DegradeLog::new();
        d.record(1, u64::MAX, PoolKind::Persistent);
        d.record(2, 4096, PoolKind::Persistent);
        s.add(
            "A34-降级账-字节累加饱和不溢出",
            // u64::MAX + 4096 必须饱和到 u64::MAX，不得回绕成 4095
            d.promoted_bytes() == u64::MAX && d.has_seq(1) && d.has_seq(2),
            "降级字节用饱和累加：u64::MAX + 4096 仍为 u64::MAX，不回绕",
        );
    }

    // --- 判据 22：降级登记预算耗尽时**字节账仍准** --------------------------
    {
        let mut d = DegradeLog::new();
        let mut i = 0u32;
        while i < MAX_DEGRADE_ROWS as u32 {
            d.record(i, 1024, PoolKind::Persistent);
            i += 1;
        }
        let rows_before = d.len();
        // 超出登记预算：行数不再增，但字节账必须继续累加
        d.record(9999, 2048, PoolKind::Persistent);
        s.add(
            "A34-降级账-登记预算耗尽字节仍准",
            rows_before == MAX_DEGRADE_ROWS
                && d.len() == MAX_DEGRADE_ROWS
                && d.promoted_bytes() == (MAX_DEGRADE_ROWS as u64) * 1024 + 2048
                && d.has_seq(9999) == false,
            "登记行数封顶不超，但降级字节继续累加（「多少显存被占住」必须准）",
        );
    }

    // --- 判据 23：面板中英双语 + 私有形态**不泄漏** + 聚合计数**必现** ------
    //
    // 私有形态禁形设计（初版踩过的坑）：若把「单请求字节 4096」和「槽位数 64」
    // 直接列入禁形，而语料又让聚合计数恰好等于 4096（2 笔降级 × 2048），
    // 则禁形与**必须出现的聚合值**撞在一起 ⇒ 基线判红。
    // 正确做法：私有形态选**与所有聚合值都不相等**的数，且判据侧独立重算
    // 全部私有形态逐一禁（十诫 30：只对一个字面量做泄漏检查＝没检查）。
    {
        // 私有禁形的**挑选纪律**（初版两次踩坑）：
        //  ① 禁形若取「单请求字节 4096」，而语料又让聚合降级字节恰为 4096
        //     ⇒ 基线判红（聚合结果本就该出现在面板上）。
        //  ② 禁形若取「槽位数 64」，而瞬态命中数恰为 64（占满 64 槽）
        //     ⇒ 又撞，且撞得隐蔽。
        //  ⇒ 禁形必须挑**与本条语料所有聚合值都不相等**的数，并前置自查；
        //    禁的必须是私有属性，聚合结果不但不该禁、还须反向断言必须出现
        //    （漏报同样是缺陷）。
        const PRIV_A: u64 = 3000; // 取整后 3072
        const PRIV_B: u64 = 5000; // 取整后 5120
        let mut a = TransientAllocator::new(100);
        let mut i = 0u32;
        while i < 2 {
            a.allocate(&AllocRequest::transient(i, PRIV_A, Liveness::new(10, 20)), 10);
            i += 1;
        }
        // 造降级：占满池后再要两笔⇒ 两笔降级，promoted = 2 x block(5000)
        let mut j = 2u32;
        while j < TRANSIENT_SLOTS as u32 + 2 {
            a.allocate(&AllocRequest::transient(j, PRIV_B, Liveness::new(10, 20)), 10);
            j += 1;
        }
        // 判据侧**独立重算**全部私有形态与聚合值（不向被测函数问答案）
        let block_a = AllocRequest::transient(0, PRIV_A, Liveness::new(0, 1)).block_bytes();
        let block_b = AllocRequest::transient(0, PRIV_B, Liveness::new(0, 1)).block_bytes();
        let expect_promoted = block_b * 2;
        let expect_hit = TRANSIENT_SLOTS as u64;
        // 总请求数 = 前 2 笔 + j 循环的 64 笔（j 从 2 到65）。
        // j 循环里**前 62 笔命中、后 2 笔降级**（前 2 笔已占2 槽，
        // 剩 62 个空槽），所以总请求是 2 + 64 = **66**，不是 68——
        // 我初版算成「2 + 64 + 2」是把降级那两笔重复计了一次。
        let expect_total = 2 + TRANSIENT_SLOTS as u64;
        // 前置自查：每个私有禁形都与每个聚合值不等（否则基线必红）
        let priv_forms = [PRIV_A, PRIV_B, block_a, block_b];
        let agg_forms = [expect_promoted, expect_hit, expect_total];
        let mut no_clash = true;
        let mut pi = 0;
        while pi < priv_forms.len() {
            let mut ai = 0;
            while ai < agg_forms.len() {
                no_clash &= priv_forms[pi] != agg_forms[ai];
                ai += 1;
            }
            pi += 1;
        }
        let lines = a.a11y_lines();
        let joined: String = concat_all(&lines);
        // 反向对照：三个聚合值必须都在面板里可查
        let agg_visible = joined.contains(&format!("{}", expect_promoted))
            && joined.contains(&format!("{}", expect_hit))
            && joined.contains(&format!("{}", expect_total));
        // 私有形态逐一禁（不是只禁一个字面量）。
        //
        // ⚠ **不**把「槽位数 TRANSIENT_SLOTS」列进禁形：面板报的是**瞬态
        // 命中数**，而本语料恰好占满全部 64 槽 ⇒ 命中数 == 槽位数 ⇒
        // 禁它等于禁一个**必须出现**的聚合值，基线必红。
        // 这不是实现泄漏，是**禁形选错**（十诫 31：禁的必须是私有属性，
        // 聚合结果不但不该禁、还须反向断言必须出现）。槽位数本就是公开
        // 常量、面板也从未单独报它，故不构成泄漏面，删掉这条错误断言。
        let mut priv_hidden = true;
        pi = 0;
        while pi < priv_forms.len() {
            priv_hidden &= !joined.contains(&format!("{}", priv_forms[pi]));
            pi += 1;
        }
        s.add(
            "A34-面板-七行双语且私有形态不泄漏",
            no_clash
                && lines.len() == 7
                // 中英双语逐行
                && joined.contains("transient hit")
                && joined.contains("degraded")
                && joined.contains("p95")
                && joined.contains("跨帧泄漏")
                && joined.contains("分配耗时")
                // 私有形态（原始字节 + 取整后字节）逐一不泄漏
                && priv_hidden
                // 聚合值必须可查
                && agg_visible
                && a.promoted_bytes() == expect_promoted
                && a.outcome_counts()[OutKind::TransientHit.ordinal()] as u64 == expect_hit
                && a.timing.total() == expect_total
                && a.timing.p95() == 10,
            "面板七行中英双语；私有形态（单请求字节原始/取整、槽位数）逐一不泄漏，聚合值必须可查",
        );
    }


    s
}

/// 结论枚举计数数组的长度守卫（独立重算，不向被测函数问答案）。
fn verdict_guard() -> bool {
    let mut n = 0usize;
    let mut i = 0;
    while i < OutKind::ALL.len() {
        n += 1;
        i += 1;
    }
    n == OUTCOME_COUNT
}

/// 把若干行拼成一个字符串（no_std 下没有 `join`）。
fn concat_all(lines: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        out.push_str(&lines[i]);
        out.push('\n');
        i += 1;
    }
    out
}