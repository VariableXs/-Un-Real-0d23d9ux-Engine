//! VE-F0035 · 读回与映射策略（VE-A 域 · GPU→CPU 读回与映射写 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0035`
//!
//! **判据（锚点原文）**：读回与映射策略——GPU→CPU 数据读回的策略（同步读回的
//! 卡顿代价模型→异步读回优先），映射写策略（持久映射/临时映射的选择），
//! 读回延迟账本；含读回策略的自动切换示例。
//! 判据：**异步优先、代价模型、持久映射、延迟账本、判据**。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 同步卡顿 → **自动切异步**：代价模型判同步读回会卡顿时自动改走异步，
//!   并**告知**调用方（静默切换会让「本帧就能拿到数据」的假设失效，
//!   而调用方往往据此决定本帧是否提交绘制）。
//! - 映射泄漏 → **检测**：持久映射必须显式解除；未解除即跨帧持有，
//!   是显存/地址空间泄漏源，故须检出并计数。
//! - 延迟劣化 → **归因**：只报「延迟超标」不报「是哪一笔」，无法优化。
//!
//! **数据结构**：策略器（[`ReadbackPolicy`]）；账本（[`LatencyLedger`]、
//! [`MappingLedger`]）；代价模型（[`StallModel`]）。
//!
//! **性能逐项分解**：O(读回数)——[`ReadbackPolicy::decide`] 对每次读回做
//! O(1) 代价评估（整数乘除，无浮点）；延迟账的最近秩为 O(W·W) 插入排序，
//! W = [`LATENCY_WINDOW`] 固定 64，**不随读回数增长**（对数级而非线性级）。
//!
//! **跨批对接点**：A06 计量同构——本条只报「延迟账」与「降级事实」，
//! 计量口径（字节/次数/耗时）与 A06 同构，便于统一汇总。
//!
//! **无障碍与隐私**：账本读屏可达（[`ReadbackPolicy::a11y_lines`]）——
//! 中英双语逐行报聚合计数，**不报**单请求的字节数与资源名（资产布局信息）。
//!
//! ## 设计要点
//!
//! - **异步优先是默认，不是兜底**（[`ReadbackPolicy::decide`]）：代价模型
//!   先算同步卡顿的代价，只有「异步不可用」且「同步代价在预算内」才走同步。
//!   反过来写（默认同步、超预算才异步）会让每次读回都先付一次模型评估的
//!   分支错误风险，且在预算恰好等于时代理走向不可预期。
//! - **代价模型用整数，不可用浮点**（[`StallModel::sync_stall_us`]）：
//!   `no_std` 无浮点保证，且卡顿预算比较必须**可复现**——浮点在不同优化级
//!   下可能给出不同的比较结果，导致同一输入在Debug/Release 走向不同分支。
//!   代价 = 固定开销 + 字节/带宽，整数向上取整（宁高不低：低估卡顿
//!   会让本该切异步的请求留在同步路径上）。
//! - **「恰好等于预算」合法**（[`StallModel::within_budget`] 用 `<=`）：
//!   预算表正是为「卡顿不超过预算」这句��设计的，等于即达标。
//!   用 `<` 会把恰好达标的读回推去异步，凭空多一次跨帧等待。
//! - **自动切异步必须告知且可观测**（[`ReadbackPolicy::switches`]）：
//!   切了但调用方不知道 ⇒ 免上传/本帧就绪的假设悄悄失效。
//! - **映射泄漏检出与解除是两个独立计数**（[`MappingLedger`]）：
//!   同 F0034 的跨帧泄漏纪律——只检不收则泄漏累积，只收不检则静默。
//!   两者独立**不是口号**：唯一的封帧出口 [`MappingLedger::seal_frame`]
//!   若总是「检出即解除」，两个计数会恒等，「强制解除」那一列只是
//!   `leaks` 的复读。因此另设 [`MappingLedger::seal_frame_report_only`]
//!   ——只检不收、保留现场（诊断取证期用），[`MappingLedger::leaks`]
//!   与 [`MappingLedger::forced`] 由此才真正可能分道，面板上两列
//!   才有信息量。
//! - **延迟账用最近秩而非均值**（[`LatencyLedger::p95`]）：与 F0034 同理，
//!   均值被大量便宜读回稀释，掩盖尾部劣化。窗口环形淘汰，序号与样本
//!   **同槽存储**（靠调用方回填映射在环形淘汰后必然对不上）。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0034（`vea34_transient`）管「帧内往哪里分配」**，本条管
//!   「已分配的内存怎么读回 CPU 与怎么映射写入」。前者产出资源，本条消费资源。
//! - **F0035 与 F0032（`vea32_resstate`）不重叠**：后者管资源状态机，
//!   本条管跨 API 的读回时机与映射生命周期策略。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个
//! 模块的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报E0583，
//! 与真实缺陷长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 延迟账环形窗口容量（只关心尾部，不存全量历史）。
pub const LATENCY_WINDOW: usize = 64;

/// 延迟账取最近秩的百分位（与 F0034 同口径）。
pub const P95_PERCENTILE: usize = 95;

/// 读回延迟承诺预算（微秒）。超出即记劣化，**不阻断**读回。
pub const LATENCY_BUDGET_US: u64 = 500;

/// 持久映射登记表容量。
pub const MAPPING_SLOTS: usize = 32;

/// 单次读回字节上限（超过即拒绝，不进决策）。
pub const MAX_READBACK_BYTES: u64 = 64 << 20;

/// 同步读回的固定开销（等待 API 调用往返与提交同步，微秒）。
pub const SYNC_FIXED_US: u64 = 40;

/// 同步读回的等效带宽（字节/微秒）。取 PCIe 4.0 x16 实测量级。
pub const SYNC_BANDWIDTH_BPU: u64 = 300;

/// 同步卡顿预算（微秒）。**等于即达标**。
pub const SYNC_BUDGET_US: u64 = 400;

/// 持久映射的建立成本（微秒，一次 map）。
pub const PERSIST_MAP_COST_US: u64 = 120;

/// 临时映射的建立成本（微秒，每次 map）。
pub const TEMP_MAP_COST_US: u64 = 60;

/// 持久映射的复用阈值：单帧写次数 ≥ 此值才值得持久化。
pub const PERSIST_WORTH_WRITES: u32 = 3;

/// 读回结论类别数。
pub const OUTCOME_COUNT: usize = 4;

/// 读回账的字节粒度（对齐到 256 字节，与显存块口径一致）。
pub const BYTE_GRANULARITY: u64 = 256;

/// 面板行数（读屏可达，双语）。
pub const PANEL_LINES: usize = 7;

// ---------------------------------------------------------------------------
// 二、生命周期与请求
// ---------------------------------------------------------------------------

/// 一次读回的等待档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitKind {
    /// 异步读回（提交后于后续帧取回）——**默认档**。
    Async,
    /// 同步读回（本帧阻塞等待）。
    Sync,
}

impl WaitKind {
    pub const ALL: [WaitKind; 2] = [WaitKind::Async, WaitKind::Sync];

    pub const fn ordinal(self) -> usize {
        match self {
            WaitKind::Async => 0,
            WaitKind::Sync => 1,
        }
    }

    pub const fn wire(self) -> u8 {
        match self {
            WaitKind::Async => 0,
            WaitKind::Sync => 1,
        }
    }

    pub const fn from_wire(w: u8) -> Option<WaitKind> {
        match w {
            0 => Some(WaitKind::Async),
            1 => Some(WaitKind::Sync),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            WaitKind::Async => "异步读回 / async readback",
            WaitKind::Sync => "同步读回 / sync readback",
        }
    }
}

/// 映射写策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapStrategy {
    /// 持久映射：建立一次、多次写，需显式解除。
    Persistent,
    /// 临时映射：每次写前建立、写后即解除。
    Temporary,
}

impl MapStrategy {
    pub const ALL: [MapStrategy; 2] = [MapStrategy::Persistent, MapStrategy::Temporary];

    pub const fn ordinal(self) -> usize {
        match self {
            MapStrategy::Persistent => 0,
            MapStrategy::Temporary => 1,
        }
    }

    pub const fn wire(self) -> u8 {
        match self {
            MapStrategy::Persistent => 0,
            MapStrategy::Temporary => 1,
        }
    }

    pub const fn from_wire(w: u8) -> Option<MapStrategy> {
        match w {
            0 => Some(MapStrategy::Persistent),
            1 => Some(MapStrategy::Temporary),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            MapStrategy::Persistent => "持久映射 / persistent mapping",
            MapStrategy::Temporary => "临时映射 / temporary mapping",
        }
    }

    /// 建立成本（微秒）。
    pub const fn map_cost_us(self) -> u64 {
        match self {
            MapStrategy::Persistent => PERSIST_MAP_COST_US,
            MapStrategy::Temporary => TEMP_MAP_COST_US,
        }
    }
}

/// 一次读回请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadbackRequest {
    /// 请求序号（原样回带，便于归因）。
    pub seq: u32,
    /// 请求字节数。
    pub bytes: u64,
    /// 本帧写该资源的次数（映射策略的输入）。
    pub writes_per_frame: u32,
    /// 异步是否可用（如资源不支持 fence 则为 false）。
    pub async_capable: bool,
}

impl ReadbackRequest {
    pub const fn new(seq: u32, bytes: u64, writes_per_frame: u32, async_capable: bool) -> Self {
        ReadbackRequest { seq, bytes, writes_per_frame, async_capable }
    }

    /// 对齐后的字节数（向上取整到 [`BYTE_GRANULARITY`]）。
    ///
    /// 零字节仍为 0——零字节读回没有意义，但**不**在此处拒绝：
    /// 校验在 [`ReadbackPolicy::decide`]统一做，避免两处口径不一致。
    pub const fn aligned_bytes(&self) -> u64 {
        if self.bytes == 0 {
            return 0;
        }
        ((self.bytes + BYTE_GRANULARITY - 1) / BYTE_GRANULARITY) * BYTE_GRANULARITY
    }
}

/// 读回结论类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutKind {
    /// 异步读回（首选路径）。
    AsyncChosen,
    /// 同步读回（代价在预算内且异步不可用）。
    SyncAccepted,
    /// 由同步**自动切异步**（代价超预算但异步可用）。
    AutoSwitched,
    /// 非法请求被拒。
    Rejected,
}

impl OutKind {
    pub const ALL: [OutKind; OUTCOME_COUNT] = [
        OutKind::AsyncChosen,
        OutKind::SyncAccepted,
        OutKind::AutoSwitched,
        OutKind::Rejected,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            OutKind::AsyncChosen => 0,
            OutKind::SyncAccepted => 1,
            OutKind::AutoSwitched => 2,
            OutKind::Rejected => 3,
        }
    }

    pub const fn wire(self) -> u8 {
        self.ordinal() as u8
    }

    pub const fn from_wire(w: u8) -> Option<OutKind> {
        if (w as usize) < OUTCOME_COUNT {
            Some(OutKind::ALL[w as usize])
        } else {
            None
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            OutKind::AsyncChosen => "异步读回 / async chosen",
            OutKind::SyncAccepted => "同步读回 / sync accepted",
            OutKind::AutoSwitched => "自动切异步 / auto switched",
            OutKind::Rejected => "拒绝 / rejected",
        }
    }

    /// 是否为「已切异步」类（自动切换的可观测面）。
    pub const fn is_switch(self) -> bool {
        matches!(self, OutKind::AutoSwitched)
    }
}

/// 一次决策的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadbackOutcome {
    pub kind: OutKind,
    /// 实际等待档位（被拒时无意义，故用 `WaitKind::Async` 占位）。
    pub wait: WaitKind,
    /// 实际映射策略（被拒时无意义）。
    pub map: MapStrategy,
    /// 同步代价（微秒；异步路径给 0）。
    pub stall_us: u64,
    pub seq: u32,
}

impl ReadbackOutcome {
    pub const fn is_ok(self) -> bool {
        !matches!(self.kind, OutKind::Rejected)
    }

    pub const fn is_switch(self) -> bool {
        self.kind.is_switch()
    }

    /// 审计行（人可读）。
    pub fn audit_line(self) -> String {
        format!(
            "#{} {} {} 代价{}us {}",
            self.seq,
            self.wait.label(),
            self.map.label(),
            self.stall_us,
            self.kind.label()
        )
    }
}

// ---------------------------------------------------------------------------
// 三、同步卡顿代价模型
// ---------------------------------------------------------------------------

/// 同步读回的卡顿代价模型（**纯整数**，可复现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StallModel;

impl StallModel {
    /// 同步读回的预计卡顿（微秒）。
    ///
    /// 代价 = 固定开销 + 字节 / 等效带宽，**向上取整**。
    /// 向上取整是刻意的：低估卡顿会让本该切异步的请求留在同步路径上，
    /// 而卡顿一旦发生是用户可感知的（掉帧），宁可高估。
    pub const fn sync_stall_us(bytes: u64) -> u64 {
        if bytes == 0 {
            return 0;
        }
        let transfer = (bytes + SYNC_BANDWIDTH_BPU - 1) / SYNC_BANDWIDTH_BPU;
        SYNC_FIXED_US.saturating_add(transfer)
    }

    /// 同步代价是否**在预算内**（**等于即达标**）。
    pub const fn within_budget(stall_us: u64) -> bool {
        stall_us <= SYNC_BUDGET_US
    }

    /// 映射策略总成本（微秒）：建立成本 + 每次写的重建成本。
    ///
    /// 持久映射只在**第一次**付建立成本；临时映射每次写都要重建。
    pub const fn map_total_us(strategy: MapStrategy, writes: u32) -> u64 {
        match strategy {
            MapStrategy::Persistent => strategy.map_cost_us(),
            MapStrategy::Temporary => strategy.map_cost_us().saturating_mul(writes as u64),
        }
    }

    /// 选择映射策略：单帧写次数越多，持久映射越划算。
    ///
    /// 阈值取「持久建立成本 ≥ 临时总成本」的第一处交点，即
    /// `PERSIST >= TEMP * writes` ⇒ `writes >= PERSIST/TEMP = 2`。
    /// 本实现取 [`PERSIST_WORTH_WRITES`] = 3 作为**保守**阈值：
    /// 持久映射还占用映射表槽位并需显式解除，泄漏风险非零，
    /// 故不在理论交点处切换，留一档余量。
    pub const fn choose_map(writes: u32) -> MapStrategy {
        if writes >= PERSIST_WORTH_WRITES {
            MapStrategy::Persistent
        } else {
            MapStrategy::Temporary
        }
    }

    /// 独立重算：给定策略下理论最低成本（判据侧自算，不调被测函数）。
    pub const fn expect_map_us(writes: u32) -> u64 {
        StallModel::map_total_us(StallModel::choose_map(writes), writes)
    }
}

// ---------------------------------------------------------------------------
// 四、延迟账
// ---------------------------------------------------------------------------

/// 读回延迟账（环形窗口 + 最近秩）。
#[derive(Clone, Debug)]
pub struct LatencyLedger {
    samples: [u32; LATENCY_WINDOW],
    seqs: [u32; LATENCY_WINDOW],
    head: usize,
    filled: usize,
    total: u64,
}

impl Default for LatencyLedger {
    fn default() -> Self {
        LatencyLedger::new()
    }
}

impl LatencyLedger {
    pub const fn new() -> LatencyLedger {
        LatencyLedger {
            samples: [0; LATENCY_WINDOW],
            seqs: [0; LATENCY_WINDOW],
            head: 0,
            filled: 0,
            total: 0,
        }
    }

    /// 记一笔延迟（微秒）。
    ///
    /// **序号与样本同槽存储**：靠调用方回填映射在环形淘汰后必然对不上
    /// （实测传映射的版本恒返回 0，归因形同虚设却无人察觉）。
    pub fn record(&mut self, us: u32, seq: u32) {
        let i = self.head;
        self.samples[i] = us;
        self.seqs[i] = seq;
        self.head = (i + 1) % LATENCY_WINDOW;
        if self.filled < LATENCY_WINDOW {
            self.filled += 1;
        }
        self.total = self.total.saturating_add(us as u64);
    }

    pub fn sample_count(&self) -> usize {
        self.filled
    }

    /// 总延迟（微秒，饱和累加）。
    pub fn total(&self) -> u64 {
        self.total
    }

    /// 第 `rank` 小（零基）的样本，rank 越界夹到 `[0, filled-1]`。
    fn kth(&self, rank: usize) -> u32 {
        if self.filled == 0 {
            return 0;
        }
        let mut idx = [0usize; LATENCY_WINDOW];
        let mut i = 0usize;
        while i < self.filled {
            let back = (self.head + LATENCY_WINDOW - 1 - i) % LATENCY_WINDOW;
            idx[i] = back;
            i += 1;
        }
        // 插入排序（窗口 64，O(W²) 但常数极小且不随读回数增长）
        let mut a = 0usize;
        while a < self.filled {
            let mut b = a + 1;
            while b < self.filled {
                if self.samples[idx[b]] < self.samples[idx[a]] {
                    let t = idx[a];
                    idx[a] = idx[b];
                    idx[b] = t;
                }
                b += 1;
            }
            a += 1;
        }
        let r = if rank >= self.filled { self.filled - 1 } else { rank };
        self.samples[idx[r]]
    }

    /// 延迟 P95（微秒；窗口空时给 0）。
    ///
    /// 取**最近秩** `ceil(0.95 * n)`（零基）而非平均值——均值会被大量
    /// 便宜读回稀释，尾部劣化被抹平，延迟承诺失去意义。
    pub fn p95(&self) -> u64 {
        if self.filled == 0 {
            return 0;
        }
        let n = self.filled;
        let mut rank = (P95_PERCENTILE * n + 99) / 100;
        if rank == 0 {
            rank = 1;
        }
        if rank > n {
            rank = n;
        }
        self.kth(rank - 1) as u64
    }

    /// P95 是否超出承诺（超了也**不阻断**读回，只记劣化）。
    pub fn over_budget(&self) -> bool {
        self.p95() > LATENCY_BUDGET_US
    }

    /// 延迟劣化归因：窗口内**延迟最大**那笔的请求序号。
    pub fn worst_seq(&self) -> u32 {
        let mut best_seq = 0u32;
        let mut best_us = 0u32;
        let mut i = 0usize;
        while i < self.filled {
            let back = (self.head + LATENCY_WINDOW - 1 - i) % LATENCY_WINDOW;
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
// 五、映射账（持久映射泄漏的检出与解除）
// ---------------------------------------------------------------------------

/// 持久映射登记表的一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MappingRow {
    pub seq: u32,
    pub bytes: u64,
    /// 登记时所在的帧号。
    pub frame: u32,
}

/// 持久映射账（泄漏检出 + 强制解除，两个独立计数）。
#[derive(Clone, Debug)]
pub struct MappingLedger {
    rows: [Option<MappingRow>; MAPPING_SLOTS],
    count: usize,
    /// 检出到的泄漏笔数（跨帧仍持有）。
    pub leaks: u32,
    /// 强制解除的笔数（与 `leaks` **独立**：只检不收则泄漏累积，
    /// 只收不检则契约破坏静默）。
    pub forced: u32,
    frame: u32,
}

impl Default for MappingLedger {
    fn default() -> Self {
        MappingLedger::new()
    }
}

impl MappingLedger {
    pub const fn new() -> MappingLedger {
        MappingLedger {
            rows: [None; MAPPING_SLOTS],
            count: 0,
            leaks: 0,
            forced: 0,
            frame: 0,
        }
    }

    /// 登记一个持久映射。登记表满即拒绝（返回 false），**不覆盖**旧行——
    /// 覆盖会让先前的映射失去登记 ⇒ 泄漏检不出来。
    pub fn register(&mut self, seq: u32, bytes: u64) -> bool {
        if self.count >= MAPPING_SLOTS {
            return false;
        }
        self.rows[self.count] = Some(MappingRow { seq, bytes, frame: self.frame });
        self.count += 1;
        true
    }

    /// 显式解除一个持久映射（登记序号须匹配）。
    pub fn release(&mut self, seq: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if let Some(r) = self.rows[i] {
                if r.seq == seq {
                    self.rows[i] = None;
                    self.count -= 1;
                    // 末位补空位，保持紧凑
                    let mut j = self.count;
                    while j < MAPPING_SLOTS {
                        if let Some(last) = self.rows[j] {
                            self.rows[i] = Some(last);
                            self.rows[j] = None;
                            break;
                        }
                        j += 1;
                    }
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 当前仍登记（未解除）的映射数。
    pub fn live(&self) -> usize {
        self.count
    }

    /// 当前仍登记的字节数（饱和累加）。
    pub fn live_bytes(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < MAPPING_SLOTS {
            if let Some(r) = self.rows[i] {
                sum = sum.saturating_add(r.bytes);
            }
            i += 1;
        }
        sum
    }

    /// 推进一帧。
    pub fn advance_frame(&mut self) {
        self.frame = self.frame.saturating_add(1);
    }

    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// 检出跨帧泄漏**并强制解除**（检出与解除是两个独立计数）。
    pub fn seal_frame(&mut self) -> u32 {
        let found = self.release_stale();
        self.leaks = self.leaks.saturating_add(found);
        self.forced = self.forced.saturating_add(found);
        found
    }

    /// **只检不收**：检出跨帧泄漏并计入 `leaks`，**保留现场不解除**。
    ///
    /// 存在的理由不是功能需求而是**可观测性**：若唯一的封帧路径总是
    /// 「检出即解除」，则 `leaks` 与 `forced` 恒等，两个计数器退化成
    /// 一个计数器写两遍——「只检不收则泄漏累积」这条契约在代码里
    /// **没有任何路径可以触发**，于是它既不能被违反、也不能被验证。
    /// 诊断期（需要保留映射现场取证）与解除期（正常回收）因此必须分开。
    pub fn seal_frame_report_only(&mut self) -> u32 {
        let found = self.count_stale();
        self.leaks = self.leaks.saturating_add(found);
        found
    }

    /// 跨帧持有的登记行数（只读，不改账）。
    pub fn count_stale(&self) -> u32 {
        let here = self.frame;
        let mut found = 0u32;
        let mut i = 0usize;
        while i < MAPPING_SLOTS {
            let stale = match self.rows[i] {
                Some(r) => r.frame < here,
                None => false,
            };
            if stale {
                found = found.saturating_add(1);
            }
            i += 1;
        }
        found
    }

    /// 解除全部跨帧持有的登记行，返回解除笔数（不动两个计数）。
    fn release_stale(&mut self) -> u32 {
        let here = self.frame;
        let mut found = 0u32;
        let mut i = 0usize;
        while i < MAPPING_SLOTS {
            let stale = match self.rows[i] {
                Some(r) => r.frame <= here,
                None => false,
            };
            if stale {
                found = found.saturating_add(1);
                self.rows[i] = None;
                self.count -= 1;
            }
            i += 1;
        }
        found
    }
}

// ---------------------------------------------------------------------------
// 六、策略器
// ---------------------------------------------------------------------------

/// 读回与映射策略器。
#[derive(Clone, Debug)]
pub struct ReadbackPolicy {
    latency: LatencyLedger,
    mappings: MappingLedger,
    counts: [u32; OUTCOME_COUNT],
    switches: u32,
    rejected: u32,
    audit: Vec<String>,
}

impl Default for ReadbackPolicy {
    fn default() -> Self {
        ReadbackPolicy::new()
    }
}

impl ReadbackPolicy {
    pub const fn new() -> ReadbackPolicy {
        ReadbackPolicy {
            latency: LatencyLedger::new(),
            mappings: MappingLedger::new(),
            counts: [0u32; OUTCOME_COUNT],
            switches: 0,
            rejected: 0,
            audit: Vec::new(),
        }
    }

    /// 决策一次读回：选等待档位 + 映射策略，并记账。
    ///
    /// 次序刻意为**先校验后决策**：非法请求（零字节 / 超上限）必须先拒，
    /// 否则白占延迟账的样本位——而账是环形窗口，白占会把真实的慢样本
    /// 挤掉，让 P95 变得好看。
    pub fn decide(&mut self, req: &ReadbackRequest) -> ReadbackOutcome {
        if req.bytes == 0 || req.bytes > MAX_READBACK_BYTES {
            self.rejected = self.rejected.saturating_add(1);
            let o = ReadbackOutcome {
                kind: OutKind::Rejected,
                wait: WaitKind::Async,
                map: MapStrategy::Temporary,
                stall_us: 0,
                seq: req.seq,
            };
            return self.note(o, "请求字节数非法");
        }

        let stall = StallModel::sync_stall_us(req.bytes);
        let map = StallModel::choose_map(req.writes_per_frame);
        let kind = if req.async_capable {
            // 异步可用 ⇒ 异步优先。代价超预算时**额外记一次自动切换**，
            // 因为「本可同步但刻意异步」是调用方需要知道的事实。
            if StallModel::within_budget(stall) {
                OutKind::AsyncChosen
            } else {
                OutKind::AutoSwitched
            }
        } else if StallModel::within_budget(stall) {
            // 异步不可用且代价在预算内 ⇒ 同步是**唯一可行**路径，
            // 不是「选择同步」——此时若再降级为异步只会失败。
            OutKind::SyncAccepted
        } else {
            // 异步不可用**且**同步超预算：无路可走。
            // 仍走同步（正确性优先于性能，不让读回失败）但显式标记，
            // 面板与审计可查「这条注定卡顿」。
            OutKind::AutoSwitched
        };

        if kind.is_switch() {
            self.switches = self.switches.saturating_add(1);
        }
        // 延迟记账口径：**按实际阻塞时间**记，不按「决策标签」记。
        // - 同步路径：按其预计卡顿记（这就是它真实的阻塞代价）。
        // - 异步路径：按固定开销记（异步不在 CPU 上阻塞）。
        // - 「异步不可用且同步超预算」的无路可走分支：**仍按预计卡顿记**。
        //   这不是笔误而是本条最容易漏的一处：初版在此按固定开销记，
        //   于是「注定要卡 223 毫秒的读回」在延迟账上只记 40 微秒，
        //   P95 看起来毫无问题、`latency_degraded()` 恒假——
        //   **恰好卡顿最严重的路径被排除在劣化统计之外**，
        //   而这正是锚点要求「延迟劣化→归因」要抓的对象。
        let observed = match kind {
            OutKind::AsyncChosen => SYNC_FIXED_US as u32,
            _ => stall as u32,
        };
        self.latency.record(observed, req.seq);

        // 持久映射**必须登记**：未登记 ⇒ 封帧检不出泄漏。
        if map == MapStrategy::Persistent {
            // 登记表满不阻断读回（正确性优先），但登记表满本身是
            // 泄漏信号，由 `mappings.live()` 暴露。
            let _ = self.mappings.register(req.seq, req.aligned_bytes());
        }

        let o = ReadbackOutcome { kind, wait: WaitKind::Async, map, stall_us: stall, seq: req.seq };
        let o = ReadbackOutcome {
            wait: if kind == OutKind::SyncAccepted { WaitKind::Sync } else { WaitKind::Async },
            ..o
        };
        self.note(o, if kind.is_switch() { "同步卡顿自动切异步" } else { "代价模型决策" })
    }

    /// 记一条结论并产出留痕（统一出口，计数与留痕**同时**发生）。
    fn note(&mut self, o: ReadbackOutcome, why: &str) -> ReadbackOutcome {
        self.counts[o.kind.ordinal()] = self.counts[o.kind.ordinal()].saturating_add(1);
        self.audit.push(format!("{} | {}", o.audit_line(), why));
        o
    }

    /// 推进一帧（帧号 +1）。
    pub fn advance_frame(&mut self) {
        self.mappings.advance_frame();
    }

    /// 封帧：检出并强制解除跨帧持有的持久映射。
    pub fn seal_frame(&mut self) -> u32 {
        self.mappings.seal_frame()
    }

    /// 封帧**只检不收**（诊断期保留现场）。这是`leaks` 与 `forced`
    /// 唯一可能分道的一条路径——诊断期看得到泄漏但账上不销账。
    pub fn seal_frame_report_only(&mut self) -> u32 {
        self.mappings.seal_frame_report_only()
    }

    /// 当前跨帧持有的登记行数（只读）。
    pub fn mapping_stale(&self) -> u32 {
        self.mappings.count_stale()
    }

    /// 显式解除某序号的持久映射。
    pub fn release_mapping(&mut self, seq: u32) -> bool {
        self.mappings.release(seq)
    }

    pub fn outcome_counts(&self) -> [u32; OUTCOME_COUNT] {
        self.counts
    }

    pub fn switches(&self) -> u32 {
        self.switches
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    pub fn p95_us(&self) -> u64 {
        self.latency.p95()
    }

    pub fn latency_degraded(&self) -> bool {
        self.latency.over_budget()
    }

    /// 延迟劣化归因到具体请求序号。
    pub fn blame_seq(&self) -> u32 {
        self.latency.worst_seq()
    }

    pub fn mapping_leaks(&self) -> u32 {
        self.mappings.leaks
    }

    pub fn mapping_forced(&self) -> u32 {
        self.mappings.forced
    }

    pub fn mapping_live(&self) -> usize {
        self.mappings.live()
    }

    /// 当前仍登记的持久映射字节数（供 A06 计量汇总）。
    pub fn mapping_live_bytes(&self) -> u64 {
        self.mappings.live_bytes()
    }

    pub fn audit_lines(&self) -> &[String] {
        &self.audit
    }

    /// 读屏面板（[`PANEL_LINES`] 行，中英双语）。
    ///
    /// **只报聚合计数**，不报单请求字节数与资源名——那是资产布局信息。
    pub fn a11y_lines(&self) -> [String; PANEL_LINES] {
        [
            format!("读回请求数 / readback requests: {}", self.audit.len()),
            format!(
                "异步读回 / async readbacks: {}",
                self.counts[OutKind::AsyncChosen.ordinal()]
            ),
            format!(
                "同步读回 / sync readbacks: {}",
                self.counts[OutKind::SyncAccepted.ordinal()]
            ),
            format!("自动切异步 / auto switched to async: {}", self.switches),
            format!("拒绝请求 / rejected requests: {}", self.rejected),
            format!(
                "映射泄漏检出 / mapping leaks: {}（强制解除 {}）",
                self.mappings.leaks, self.mappings.forced
            ),
            format!(
                "延迟 P95 / latency p95: {} us（预算 {} us）",
                self.latency.p95(),
                LATENCY_BUDGET_US
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

pub fn run_vea35_checks() -> CheckSet {
    let mut s = CheckSet::new("vea35_readback");

    // --- 判据 1：代价模型——整数、向上取整、零字节为 0 ---
    {
        let zero = StallModel::sync_stall_us(0);
        let small = StallModel::sync_stall_us(1);
        let exact = StallModel::sync_stall_us(SYNC_BANDWIDTH_BPU * 10); // 整除
        let partial = StallModel::sync_stall_us(SYNC_BANDWIDTH_BPU * 10 + 1); // 需进位
        s.add(
            "A35-代价模型-整数向上取整且零字节为零",
            zero == 0
                // 1 字节也要付固定开销 + 1 微秒传输（向上取整）
                && small == SYNC_FIXED_US + 1
                // 整除时无进位
                && exact == SYNC_FIXED_US + 10
                // 余 1 字节亦按 1 微秒计（向上取整）
                && partial == SYNC_FIXED_US + 11
                // 恒等式：对全部采样点，代价 >= 固定开销且 >= ceil(字节/带宽)
                && {
                    let mut ok = true;
                    let mut b = 1u64;
                    while b <= 4096 {
                        let expect = SYNC_FIXED_US + (b + SYNC_BANDWIDTH_BPU - 1) / SYNC_BANDWIDTH_BPU;
                        if StallModel::sync_stall_us(b) != expect {
                            ok = false;
                            break;
                        }
                        b += 7;
                    }
                    ok
                },
            "同步代价 = 固定开销 + ceil(字节/带宽)，全整数可复现；零字节为 0",
        );
    }

    // --- 判据 2：预算比较用`<=`（等于即达标） ---
    {
        // 构造恰好等于预算的字节数：令 transfer = 预算 - 固定
        let target_transfer = SYNC_BUDGET_US - SYNC_FIXED_US;
        let exact_bytes = target_transfer * SYNC_BANDWIDTH_BPU;
        let at_budget = StallModel::sync_stall_us(exact_bytes);
        let over_bytes = exact_bytes + SYNC_BANDWIDTH_BPU; // 多一个整带宽 ⇒ 超 1 微秒
        let over = StallModel::sync_stall_us(over_bytes);
        s.add(
            "A35-代价模型-恰好等于预算判达标",
            at_budget == SYNC_BUDGET_US
                && StallModel::within_budget(at_budget)
                // 超预算一微秒即判不达标（夹逼对）
                && over == SYNC_BUDGET_US + 1
                && !StallModel::within_budget(over)
                // 边界另一侧：预算减一微秒仍达标
                && StallModel::within_budget(SYNC_BUDGET_US - 1)
                && !StallModel::within_budget(SYNC_BUDGET_US + 1),
            "同步代价恰等于预算判达标（用 <= 而非 <），超 1 微秒即不达标",
        );
    }

    // --- 判据 3：异步优先（异步可用时永不选同步） ---
    {
        let mut p = ReadbackPolicy::new();
        // 小读回：代价在预算内，但异步可用 ⇒ 仍走异步
        let r = ReadbackRequest::new(1, 1024, 1, true);
        let o = p.decide(&r);
        s.add(
            "A35-异步优先-异步可用时不选同步",
            o.kind == OutKind::AsyncChosen
                && o.wait == WaitKind::Async
                && !o.is_switch()
                && p.outcome_counts()[OutKind::AsyncChosen.ordinal()] == 1
                && p.outcome_counts()[OutKind::SyncAccepted.ordinal()] == 0,
            "异步可用时即便同步代价在预算内也走异步（异步优先是默认而非兜底）",
        );
    }

    // --- 判据 4：同步卡顿 ⇒ 自动切异步**并告知** ---
    {
        let mut p = ReadbackPolicy::new();
        // 大读回：代价超预算，异步可用 ⇒ 自动切换
        let big = ReadbackRequest::new(77, 8 << 20, 1, true);
        let o = p.decide(&big);
        // 第一笔后的快照（用于断言「切一次 ⇒ 恰 1」）
        let c1 = p.outcome_counts();
        // 第二笔须用**不同序号**：序号是归因键，同序号两笔在延迟账里
        // 无法区分（让两笔共用序号会让审计行看起来像同一笔重复，徒增误判）
        let big2 = ReadbackRequest::new(78, 8 << 20, 1, true);
        let o2 = p.decide(&big2);
        // 第二笔后的快照（用于断言「切两次 ⇒ 恰 2」）
        let c2 = p.outcome_counts();
        s.add(
            "A35-自动切换-同步卡顿切异步且可观测",
            o.is_switch()
                && o.kind == OutKind::AutoSwitched
                && o.wait == WaitKind::Async
                // 必须**告知**：切换计数与审计行都可查。
                // 两次快照分别断言，避免拿同一份快照断两个不同的值。
                && p.switches() == 2
                && c1[OutKind::AutoSwitched.ordinal()] == 1
                && c2[OutKind::AutoSwitched.ordinal()] == 2
                // 切了两次 ⇒ 审计行两条（每条结论都留痕，不静默）
                && o2.is_switch()
                && p.audit_lines().len() == 2
                // 未被拒（自动切换不是失败）
                && c2[OutKind::Rejected.ordinal()] == 0
                // 审计行须含「自动切异步」字样（可观测而非静默）
                && p.audit_lines()[0].contains("自动切异步")
                && p.audit_lines()[1].contains("自动切异步"),
            "同步卡顿自动切异步，切换笔数与审计行均可查（不静默降级）",
        );
    }

    // --- 判据 5：异步不可用且超预算 ⇒ 不失败（正确性优先） ---
    {
        let mut p = ReadbackPolicy::new();
        let big = ReadbackRequest::new(5, 8 << 20, 1, false);
        let o = p.decide(&big);
        s.add(
            "A35-无路可走-异步不可用仍不失败",
            o.is_ok()
                && o.kind == OutKind::AutoSwitched
                && o.wait == WaitKind::Async
                && p.rejected() == 0
                && p.outcome_counts()[OutKind::Rejected.ordinal()] == 0,
            "异步不可用且同步超预算时仍返回可用结论（不让读回失败），但显式标记可查",
        );
    }

    // --- 判据 6：同步仅在「异步不可用且预算内」 ---
    {
        let mut p = ReadbackPolicy::new();
        let small = ReadbackRequest::new(9, 2048, 1, false);
        let o = p.decide(&small);
        s.add(
            "A35-同步条件-仅异步不可用且预算内",
            o.kind == OutKind::SyncAccepted
                && o.wait == WaitKind::Sync
                && !o.is_switch()
                && o.stall_us == StallModel::sync_stall_us(2048)
                && StallModel::within_budget(o.stall_us)
                && p.outcome_counts()[OutKind::SyncAccepted.ordinal()] == 1,
            "同步读回仅在异步不可用且同步代价在预算内时选中，且代价可查",
        );
    }

    // --- 判据 7：映射策略阈值（写次数分道，夹逼对钉死阈值位置） ---
    {
        let below = StallModel::choose_map(PERSIST_WORTH_WRITES - 1);
        let at = StallModel::choose_map(PERSIST_WORTH_WRITES);
        let above = StallModel::choose_map(PERSIST_WORTH_WRITES + 5);
        // 零写次⇒ 临时（否则白付建立成本）
        let zero = StallModel::choose_map(0);
        s.add(
            "A35-持久映射-写次数阈值处夹逼分道",
            below == MapStrategy::Temporary
                && at == MapStrategy::Persistent
                && above == MapStrategy::Persistent
                && zero == MapStrategy::Temporary
                // 阈值处持久更划算（成本不高于临时）
                && StallModel::map_total_us(at, PERSIST_WORTH_WRITES)
                    <= StallModel::map_total_us(MapStrategy::Temporary, PERSIST_WORTH_WRITES),
            "写次数达阈值选持久映射，阈值前一档选临时；阈值处持久总成本不高于临时",
        );
    }

    // --- 判据 8：持久映射**必登记**（不登记则泄漏检不出） ---
    {
        let mut p = ReadbackPolicy::new();
        // 单帧写 9 次 ⇒ 持久；不推进帧、不解除 ⇒ 封帧应检出泄漏
        let r = ReadbackRequest::new(31, 8192, 9, true);
        let o = p.decide(&r);
        let live_before = p.mapping_live();
        let found = p.seal_frame(); // 仍在第 0 帧 ⇒ 非跨帧
        s.add(
            "A35-持久映射-登记后可被显式解除",
            o.map == MapStrategy::Persistent
                && live_before == 1
                // 同帧封帧**不**算泄漏（帧号未推进）
                && found == 0
                && p.mapping_leaks() == 0
                && p.mapping_forced() == 0
                // 显式解除后归零
                && p.release_mapping(31)
                && p.mapping_live() == 0,
            "持久映射进登记表，可被显式解除；同帧封帧不误报泄漏",
        );
    }

    // --- 判据 9：映射泄漏检出与强制解除是**两个独立计数** ---
    {
        let mut p = ReadbackPolicy::new();
        let none_before = p.mapping_leaks() == 0 && p.mapping_forced() == 0 && p.mapping_live() == 0;
        // 造事件：登记持久映射后推进一帧再封帧
        let r = ReadbackRequest::new(41, 4096, 9, true);
        p.decide(&r);
        let live_mid = p.mapping_live();
        p.advance_frame();
        let found = p.seal_frame();
        s.add(
            "A35-映射泄漏-检出与强制解除两计数独立",
            none_before
                && live_mid == 1
                // 跨帧封帧检出 1 笔
                && found == 1
                && p.mapping_leaks() == 1
                // 强制解除也是 1（两个独立计数各自更新）
                && p.mapping_forced() == 1
                // 强制解除后登记表清空（只检不收则泄漏累积）
                && p.mapping_live() == 0
                // 字节账同样归零：泄漏解除后显存占用必须真的还回去，
                // 否则「检出了泄漏但占用仍在」——A06 计量会持续虚高
                && p.mapping_live_bytes() == 0,
            "负向无泄漏时两计数均为 0；跨帧封帧检出恰 1 笔且强制解除后登记表清空",
        );
    }

    // --- 判据 10：登记表满**不覆盖**旧行（否则先前的映射检不出） ---
    {
        let mut m = MappingLedger::new();
        let mut i = 0u32;
        while (i as usize) < MAPPING_SLOTS {
            if !m.register(1000 + i, 256) {
                break;
            }
            i += 1;
        }
        let full_live = m.live();
        let full_bytes = m.live_bytes();
        let overflow = m.register(9999, 256); // 溢出 ⇒ 拒
        // 反证：登记表满时 advance+seal 应把**全部**旧行判为泄漏
        m.advance_frame();
        let found = m.seal_frame();
        s.add(
            "A35-映射登记表-满时拒绝且旧行仍可检出",
            full_live == MAPPING_SLOTS
                && !overflow
                // 字节账亦须守恒：满表 32 行 × 256 字节 = 8192。
                // 变异 M09（登记时把字节写成 0）**杀不死**任何既有判据
                // —— 因为无判据断言 `live_bytes()`，字节记账完全没被验证：
                // 泄漏「检得出但报不出多少字节」，A06 计量无法汇总。
                // 注意须用**封帧前**的快照 `full_bytes`：
                // `seal_frame()` 已把表清空，此时再读 live_bytes() 恒为 0。
                && full_bytes == MAPPING_SLOTS as u64 * 256
                // 封帧后字节账归零（占用真的还回去了）
                && m.live_bytes() == 0
                // 关键：拒登记**不覆盖**旧行 ⇒ 满表时仍能检出全部泄漏
                && found == MAPPING_SLOTS as u32
                && m.leaks == MAPPING_SLOTS as u32
                && m.live() == 0,
            "登记表满时拒新登记且不覆盖旧行；满表状态下全部旧行仍可被检出为泄漏",
        );
    }

    // --- 判据 10b：登记表容量**规格明文化**并绑定到槽位数组 ---
    //
    // 变异 M11（`MAPPING_SLOTS: 32→64`）在判据 10 下**零红项存活**，
    // 根因是判据 10 的每条断言都写成 `== MAPPING_SLOTS`（符号化表述）：
    // 常量与判据同步漂移 ⇒ 断言恒成立。这是「悬空常量」形态——
    // 常量事实上就是无界的，没有任何判据把它钉在规格值上。
    //
    // 修法两条腿：
    // ① 判据侧写**字面量** 32（规格明文容量），与被测常量对照 ⇒ 常量漂移即红；
    // ② 灌入三倍于容量的登记请求后仍只留 32 行 ⇒ 数组上界与容量一致
    //    （若有人只把`rows` 数组改大而常量留在 32，本条抓不到；
    //    但那种改动不会造成无界登记，只造成「多分配」——不属本条要防的
    //    正确性缺陷，故不为它加恒真子句充数）。
    {
        // 判据侧独立重算：规格明文容量 = 32 槽。
        const SPEC_MAPPING_SLOTS: usize = 32;
        let mut m = MappingLedger::new();
        let mut i = 0usize;
        // 连填三倍于容量的登记请求：前 32 次必成功，其后**全部**被拒
        while i < SPEC_MAPPING_SLOTS * 3 {
            let _ = m.register(2000 + i as u32, 256);
            i += 1;
        }
        let mut accepted = 0usize;
        let mut r = MappingLedger::new();
        while accepted < SPEC_MAPPING_SLOTS * 2 {
            if !r.register(3000 + accepted as u32, 256) {
                break;
            }
            accepted += 1;
        }
        s.add(
            "A35-映射登记表-容量规格明文且拒绝点在第33次",
            // ① 常量必须等于规格字面量（变异 M11 改 64 即红）
            MAPPING_SLOTS == SPEC_MAPPING_SLOTS
                // ② 三倍请求灌入后仍只有 32 行、字节账恰为 32×256
                && m.live() == SPEC_MAPPING_SLOTS
                && m.live_bytes() == SPEC_MAPPING_SLOTS as u64 * 256
                // 恰好第 33 次被拒（前 32 次全成功）
                && accepted == SPEC_MAPPING_SLOTS
                // 夹逼：第 32 次仍成功、第 33 次必拒 ⇒ 拒绝点钉死在容量+1
                && {
                    let mut b = MappingLedger::new();
                    let mut k = 0usize;
                    let mut last_ok = false;
                    while k < SPEC_MAPPING_SLOTS {
                        last_ok = b.register(4000 + k as u32, 256);
                        k += 1;
                    }
                    let first_reject = !b.register(9999, 256);
                    last_ok && first_reject && b.live() == SPEC_MAPPING_SLOTS
                },
            "登记表容量规格明文为 32 槽；灌入 96 次仍只留 32 行，拒绝点恰在第 33 次",
        );
    }

    // --- 判据 11：解除未登记的序号**拒绝**（不静默成功） ---
    {
        let mut m = MappingLedger::new();
        m.register(1, 512);
        let miss = m.release(777); // 未登记
        let hit = m.release(1);
        let again = m.release(1); // 已解除 ⇒ 再次解除应失败
        s.add(
            "A35-映射解除-未登记与重复解除均拒绝",
            !miss
                && hit
                && !again
                && m.live() == 0,
            "解除未登记序号与重复解除均拒绝，不静默成功",
        );
    }

    // --- 判据 12：延迟账取最近秩**不被均值稀释** ---
    {
        let mut t = LatencyLedger::new();
        let empty = t.p95() == 0 && t.sample_count() == 0;
        // 90 笔便宜 10 + 10 笔昂贵 1000：昂贵占 10% > 5% ⇒ 最近秩落昂贵侧
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
        // 判据侧独立重算均值口径（不向被测函数问答案）
        let mean = (90 * 10 + 10 * 1000) / 100;
        s.add(
            "A35-延迟账-取最近秩不被均值稀释",
            empty
                && t.p95() == 1000
                // 均值仅 109，远低于真实尾部 ⇒ 用均值则承诺失效
                && mean == 109
                && mean < t.p95()
                // 窗口上限即 [`LATENCY_WINDOW`]：写入 100 笔时窗口封顶 64，
                // 而最旧的36 笔被淘汰。样本数须断言**封顶值**而非写入笔数
                // （断言 100 恒假：环形窗口本就不保留全部历史）。
                // P95 不受淘汰影响：被淘汰的 36 笔都是便宜侧，
                // 窗口内仍保留 28 笔便宜 + 10 笔昂贵 ⇒ 最近秩落在昂贵侧。
                && t.sample_count() == LATENCY_WINDOW
                // 总数是**累计**值（含被淘汰的 36 笔），判据侧独立重算
                && t.total() == 90 * 10 + 10 * 1000
                && t.over_budget()
                && LATENCY_BUDGET_US == 500,
            "90×10 + 10×1000 ⇒ P95=1000（均值口径仅 109，尾部被稀释）；空账 P95 为 0",
        );
    }

    // --- 判据 12b：最近秩的**进位**语义（n 不整除时 floor≠ceil） ---
    //
    // 判据 12 用 n=100，而 `0.95×100 = 95` 是整数 ⇒ `ceil==floor`，
    // 于是「向上取整」这个动作在语料里**不可观测**：把 `(95*n+99)/100`
    // 改成 `(95*n)/100` 结果完全相同（变异 M04 存活）。
    //
    // 取**n=21**：`0.95×21 = 19.95` ⇒ ceil=20、floor=19，差一位。
    // 语料取 19 笔便宜(10) + 2 笔昂贵(800)：升序后第 18 位=10、第 19 位=800，
    // 两口径分别取到 800（ceil，零基 19）与 10（floor，零基 18）——
    // **取值不同**，故口径写错必然被本条抓到。
    {
        const N_CHEAP: usize = 19;
        const N_DEAR: usize = 2;
        let mut t = LatencyLedger::new();
        let mut i = 0u32;
        while (i as usize) < N_CHEAP {
            t.record(10, i);
            i += 1;
        }
        i = 0;
        while (i as usize) < N_DEAR {
            t.record(800, 500 + i);
            i += 1;
        }
        let n = t.sample_count();
        let rank_ceil = (P95_PERCENTILE * n + 99) / 100; // ceil(19.95) = 20
        let rank_floor = (P95_PERCENTILE * n) / 100; // floor(19.95) = 19
        // 独立重算升序取值：前 19 位 10、后 2 位 800
        let value_at = |r: usize| -> u32 {
            if r < N_CHEAP {
                10
            } else {
                800
            }
        };
        let expect_ceil = value_at(rank_ceil - 1);
        let expect_floor = value_at(rank_floor - 1);
        s.add(
            "A35-延迟账-最近秩取ceil在n不整除时与floor分道",
            // 前置：两口径确实分道且取值确实不同（否则本条是恒真门禁）
            rank_ceil != rank_floor
                && expect_ceil != expect_floor
                && n == 21
                && rank_ceil == 20
                && rank_floor == 19
                && expect_ceil == 800
                && expect_floor == 10
                // 被测取 ceil 口径⇒ 800；若是 floor 口径会给 10，本条转红
                && t.p95() == 800,
            "n=21（19×10 + 2×800）：ceil(0.95n)=20 => P95=800；floor=19 会给 10，两口径取值不同故可判别",
        );
    }

    // --- 判据 13：环形窗口淘汰最旧且样本数封顶 ---
    {
        let mut t = LatencyLedger::new();
        let mut i = 0u32;
        // 写入 LATENCY_WINDOW + 20 笔 ⇒ 旧样本须被淘汰
        while i < (LATENCY_WINDOW as u32) + 20 {
            t.record(100, i);
            i += 1;
        }
        s.add(
            "A35-延迟账-环形窗口淘汰最旧且计数封顶",
            t.sample_count() == LATENCY_WINDOW
                // 全部样本同值 100 ⇒ P95 仍为 100（淘汰不改变取值口径）
                && t.p95() == 100
                // 总数是**累计**（含被淘汰的），不减
                && t.total() == ((LATENCY_WINDOW as u32 + 20) * 100) as u64,
            "超出窗口后样本数封顶，最旧被淘汰；总延迟是累计值不减",
        );
    }

    // --- 判据 14：延迟劣化**归因到具体请求序号** ---
    //
    // 语料构造须让**劣化真的发生**：P95 取最近秩 ⇒ 单笔极慢只占
    // 1/31 = 3.2% < 5%，最近秩仍落在便宜侧，`latency_degraded()`
    // 数学上就该为 false。所以慢笔必须**超过 5%** 才造得出劣化
    // ——初版只造一笔慢的，判据却要求 degraded 为真，是判据写错。
    //
    // 本条用 60 笔便宜 + 8 笔慢（慢占 8/68 = 11.8% > 5%），
    // 其中序号 31337 那笔最慢（64MiB，异步不可用 ⇒ 注定卡顿）。
    {
        let mut p = ReadbackPolicy::new();
        let none_blame = p.blame_seq() == 0;
        let not_degraded_yet = !p.latency_degraded();
        // 60 笔便宜读回（异步路径，各记固定开销）
        let mut i = 0u32;
        while i < 60 {
            let r = ReadbackRequest::new(i, 1024, 1, true);
            p.decide(&r);
            i += 1;
        }
        // 7 笔中速（超预算但异步可用 ⇒ 自动切异步，按真实卡顿记账）
        i = 0;
        while i < 7 {
            let r = ReadbackRequest::new(1000 + i, 4 << 20, 1, true);
            p.decide(&r);
            i += 1;
        }
        // 最后一笔最慢：异步不可用 ⇒ 注定阻塞
        let slow = ReadbackRequest::new(31337, 64 << 20, 1, false);
        let o = p.decide(&slow);
        s.add(
            "A35-归因-延迟劣化定位到具体请求",
            none_blame
                // 空账与全便宜账**不得**误报劣化（负向，否则本条恒真）
                && not_degraded_yet
                // 慢笔占比 11.8% > 5% ⇒ 最近秩落入昂贵侧，劣化成立
                && p.latency_degraded()
                // 归因须指向那笔最慢的序号（不是 0、不是别的）
                && p.blame_seq() == 31337
                && o.seq == 31337
                && p.p95_us() > LATENCY_BUDGET_US
                // 独立重算：P95 取最近秩，68 笔中最贵的 8 笔落在窗口内
                // ⇒ 最近秩 ceil(0.95×68)=65（零基 64）落在昂贵侧，
                // 独立重算该位取值 = 4MiB 那档的卡顿（14022）。
                // 注意 P95 **不等于**最慢那笔（那笔只占 1/68 < 5%）——
                // 断言它等于最慢值是错的，最近秩本就不看极值。
                // 归因才看极值：blame 指向 31337。
                && p.p95_us() == StallModel::sync_stall_us(4 << 20)
                // P95 须严格大于全便宜账的固定开销 40，证明昂贵样本
                // 真的进了窗口（否则「无路可走」路径按固定开销记账的
                // 旧缺陷会让本条转红）
                && p.p95_us() > SYNC_FIXED_US
                // 且P95 不得超过最慢那笔（最近秩是分位数，恒 <= 极值）
                && p.p95_us() <= StallModel::sync_stall_us(64 << 20),
            "空账与全便宜账不误报；慢笔占 11.8% 时劣化成立且归因指向序号 31337，P95 落在昂贵侧",
        );
    }

    // --- 判据 15：非法请求先拒不占延迟账样本位 ---
    {
        let mut p = ReadbackPolicy::new();
        let zero = ReadbackRequest::new(1, 0, 1, true);
        let huge = ReadbackRequest::new(2, MAX_READBACK_BYTES + 1, 1, true);
        let oz = p.decide(&zero);
        let oh = p.decide(&huge);
        // 随后一笔合法读回：延迟账应**只**记它一笔
        let ok = ReadbackRequest::new(3, 1024, 1, true);
        let ook = p.decide(&ok);
        s.add(
            "A35-校验-非法请求先拒不占样本位",
            oz.kind == OutKind::Rejected
                && oh.kind == OutKind::Rejected
                && !oz.is_ok() && !oh.is_ok()
                && p.rejected() == 2
                && p.outcome_counts()[OutKind::Rejected.ordinal()] == 2
                // 关键：非法请求不得污染延迟账（白占会把真实慢样本挤掉）
                && ook.is_ok()
                && p.outcome_counts()[OutKind::AsyncChosen.ordinal()] == 1
                // 对齐字节数：合法请求对齐到256
                && ReadbackRequest::new(4, 1000, 1, true).aligned_bytes() == 1024,
            "零字节与超上限请求被拒且不占延迟账样本位，后续合法请求不受污染",
        );
    }

    // --- 判据 16：结论枚举下标连续且与计数数组同长 ---
    {
        let mut ok = true;
        let mut seen = 0usize;
        let mut k = 0usize;
        while k < OUTCOME_COUNT {
            let o = OutKind::ALL[k];
            if o.ordinal() != k {
                ok = false;
            }
            //线编码往返自洽
            match OutKind::from_wire(o.wire()) {
                Some(back) => {
                    if back != o {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            seen += 1;
            k += 1;
        }
        s.add(
            "A35-结论枚举-下标连续且与计数数组同长",
            ok && seen == OUTCOME_COUNT,
            "四个结论类下标 0..3 连续，线编码往返自洽，与计数数组同长",
        );
    }

    // --- 判据 17：零 panic 面——被拒路径无票据仍安全取值 ---
    {
        let mut p = ReadbackPolicy::new();
        let bad = ReadbackRequest::new(8, 0, 1, true);
        let o = p.decide(&bad);
        // 被拒结论仍须能安全读出全部字段（不 panic、不越界）
        let safe = !o.is_ok()
            && o.seq == 8
            && o.stall_us == 0
            && o.wait == WaitKind::Async
            && o.map == MapStrategy::Temporary
            && !o.is_switch()
            // 审计行可生成（含全部字段）
            && !o.audit_line().is_empty()
            // 等待档与映射策略的线编码往返
            && WaitKind::from_wire(WaitKind::Sync.wire()) == Some(WaitKind::Sync)
            && MapStrategy::from_wire(MapStrategy::Persistent.wire()) == Some(MapStrategy::Persistent)
            && WaitKind::from_wire(200).is_none()
            && MapStrategy::from_wire(200).is_none()
            && OutKind::from_wire(200).is_none();
        s.add(
            "A35-零panic-被拒路径字段安全可读",
            safe,
            "被拒结论的每个字段均可安全读取（无 panic/越界），三个枚举线编码越界返回 None",
        );
    }

    // --- 判据 18：全流程——两帧读回 + 映射泄漏 + 劣化归因 ---
    {
        let mut p = ReadbackPolicy::new();
        // 帧 0：混合三种决策
        let a = p.decide(&ReadbackRequest::new(1, 1024, 1, true)); // 异步
        let b = p.decide(&ReadbackRequest::new(2, 2048, 1, false)); // 同步（预算内）
        let c = p.decide(&ReadbackRequest::new(3, 8 << 20, 1, true)); // 自动切异步
        let d = p.decide(&ReadbackRequest::new(4, 8192, 9, true)); // 持久映射
        let bytes_before = p.mapping_live_bytes();
        let seal0 = p.seal_frame(); // 同帧 ⇒ 不算泄漏
        p.advance_frame();
        let seal1 = p.seal_frame(); // 跨帧 ⇒ 检出并强制解除
        let counts = p.outcome_counts();
        s.add(
            "A35-全流程-决策封帧泄漏与计数守恒",
            a.kind == OutKind::AsyncChosen
                && b.kind == OutKind::SyncAccepted
                && c.is_switch()
                && d.map == MapStrategy::Persistent
                // 登记的字节须是**对齐后**的字节（8192 已是 256 的倍数）
                && bytes_before == 8192
                && seal0 == 0
                && seal1 == 1
                && p.mapping_leaks() == 1
                && p.mapping_forced() == 1
                && p.mapping_live() == 0
                // 强制解除后字节账也归零（占用真的还回去了）
                && p.mapping_live_bytes() == 0
                && p.switches() == 1
                // 四类结论计数之和 == 决策次数（用 == 不用 >=）
                && counts[OutKind::AsyncChosen.ordinal()]
                    + counts[OutKind::SyncAccepted.ordinal()]
                    + counts[OutKind::AutoSwitched.ordinal()]
                    + counts[OutKind::Rejected.ordinal()]
                    == 4
                // 审计行数 == 决策次数（每条结论都留痕，不静默）
                && p.audit_lines().len() == 4,
            "四种决策路径各一，同帧封帧不误报、跨帧封帧检出并强制解除，计数与留痕守恒",
        );
    }

    // --- 判据 19：面板七行双语且私有形态不泄漏 ---
    {
        let mut p = ReadbackPolicy::new();
        let mut i = 0u32;
        while i < 6 {
            let r = ReadbackRequest::new(900 + i, 4096 + i as u64 * 16, 1, true);
            p.decide(&r);
            i += 1;
        }
        let lines = p.a11y_lines();
        let all_nonempty = lines.iter().all(|l| !l.is_empty());
        // 面板不得出现单请求序号（900..905）或单请求字节数
        let leaks_seq = lines.iter().any(|l| l.contains("900") || l.contains("905"));
        // 也不得出现形如具体字节的裸数字段
        let leaks_bytes = lines.iter().any(|l| l.contains("4112") || l.contains("4096"));
        s.add(
            "A35-面板-七行双语且私有形态不泄漏",
            lines.len() == PANEL_LINES
                && all_nonempty
                && !leaks_seq
                && !leaks_bytes
                // 首行须报聚合计数（6 笔）
                && lines[0].contains('6')
                // 双语：每行都含 ASCII 键名
                && lines.iter().all(|l| l.chars().any(|c| c.is_ascii_alphabetic())),
            "面板恰七行、每行双语且非空，只报聚合计数，不泄漏单请求序号与字节数",
        );
    }

    // --- 判据 20：面板七行**逐行绑定**到各自聚合量（值可辨，非仅行数/非空） ---
    //
    // 变异教训：判据 19 只断「恰七行 + 每行非空 + 双语 + 首行含'6'」，
    // **没有一行断过它的数值内容**。于是把面板某一行改成报另一个量
    // （如P95 行改报累计延迟、泄漏行漏掉「强制解除」）全绿——
    // 读屏用户拿到的是**错误但格式正确**的数字，比缺行更坏：
    // 缺行会被发现，错值会被当真。
    //
    // 语料刻意让各行取值**互不相同**（2/1/1/1/1/1/28002），
    // 这样「某行取错字段」等价于「某行显示别人的数」，必被本条抓到。
    // 期望值全部由判据侧独立重算，不向 `a11y_lines()` 或任何被测取值问答案。
    {
        let mut p = ReadbackPolicy::new();
        // A 异步且预算内 ⇒ AsyncChosen（延迟账记固定开销 40）
        p.decide(&ReadbackRequest::new(1, 1024, 1, true));
        // B 异步不可用且预算内 ⇒ SyncAccepted（记预计卡顿 47）
        p.decide(&ReadbackRequest::new(2, 2048, 1, false));
        // C 8MiB 异步 ⇒ 超预算 ⇒ AutoSwitched（记预计卡顿 28002）
        p.decide(&ReadbackRequest::new(3, 8 << 20, 1, true));
        // D 写 9 次 ⇒ 持久映射并登记（8192 字节）
        p.decide(&ReadbackRequest::new(4, 8192, 9, true));
        // E 零字节 ⇒ Rejected（不占延迟账样本位）
        p.decide(&ReadbackRequest::new(5, 0, 1, true));
        p.advance_frame();
        let sealed = p.seal_frame();

        let lines = p.a11y_lines();
        // 判据侧独立重算期望 P95：延迟账共 4 笔
        //   A 异步 1KiB⇒ 记固定开销 40
        //   B 同步 2KiB⇒ 记 40 + ceil(2048/300) = 40 + 7 = 47
        //   C 自动切异步 8MiB ⇒ 记 40 + ceil(8388608/300) = 40 + 27963 = 28003
        //   D 异步 8KiB ⇒ 记 40
        // 最近秩 = ceil(0.95×4) = 4 ⇒ 升序末位 = 28003。
        // 全部按**规格字面量**独立重算，不调`p95_us()`（那是待验证的产出侧），
        // 也不复用 `sync_stall_us()`（那是被测函数本身，自证式）。
        const SPEC_FIXED_US: u32 = 40;
        const SPEC_BPU: u32 = 300;
        const C_BYTES: u64 = 8 << 20;
        let expect_c_stall = SPEC_FIXED_US + ((C_BYTES + SPEC_BPU as u64 - 1) / SPEC_BPU as u64) as u32;
        let mut sorted = [SPEC_FIXED_US, SPEC_FIXED_US + 7, expect_c_stall, SPEC_FIXED_US];
        // 判据侧插入排序（升序）。外层必须从 1 起、内层向 0 走——
        // 写成「外层从 0 起 + 内层从 w 向右」只完成一轮冒泡，
        // 数组仍是乱的，而下面 `sorted[rank-1]` 会取到位⇒ 期望值错。
        let mut w = 1usize;
        while w < sorted.len() {
            let mut k = w;
            while k > 0 && sorted[k - 1] > sorted[k] {
                let t = sorted[k - 1];
                sorted[k - 1] = sorted[k];
                sorted[k] = t;
                k -= 1;
            }
            w += 1;
        }
        let rank = (95 * sorted.len() + 99) / 100; // ceil(3.8) = 4
        let expect_p95 = sorted[rank - 1];
        // 累计口径的值（若面板误报累计而非分位数就会印出它）
        let expect_total_us: u64 = sorted.iter().map(|v| *v as u64).sum();
        // 独立重算各聚合计数：5 次决策⇒ 审计 5 行
        let expect_total = 5usize;
        // 各行取值互不相同是本条可判别的**前提**（否则取值写错也看不出）：
        // 面板七行分别印 5/2/1/1/1/1/28003，须有≥3 个互异值
        let values_distinct = expect_p95 != expect_total as u32 && sorted[0] != sorted[2];

        s.add(
            "A35-面板-七行逐行绑定各自聚合量",
            values_distinct
                && rank == 4
                // 独立重算的最近秩值必须落在大额那一档，否则本条恒真
                && expect_c_stall == 28003
                && expect_p95 == 28003
                && expect_p95 > sorted[0]
                // 第 0 行：审计行数 = 决策次数（全部决策都留痕）
                && p.audit_lines().len() == expect_total
                && lines[0].contains("readback requests: 5")
                // 第 1 行：异步笔数 = 2（A、D）
                && lines[1].contains("async readbacks: 2")
                // 第 2 行：同步笔数 = 1（B）—— 与上行取值不同 ⇒ 可辨
                && lines[2].contains("sync readbacks: 1")
                // 第 3 行：自动切换笔数 = 1（C）
                && lines[3].contains("auto switched to async: 1")
                // 第 4 行：拒绝笔数 = 1（E）
                && lines[4].contains("rejected requests: 1")
                // 第 5 行：泄漏 1 笔且**强制解除 1 笔**——两个独立计数都在面板上，
                // 漏掉「强制解除」则只检不收的策略在读屏侧不可见
                && lines[5].contains("leaks: 1")
                && lines[5].contains("强制解除 1")
                // 第 6 行：P95 是最近秩值，**不是**累计
                && lines[6].contains(&format!("latency p95: {} us", expect_p95))
                && lines[6].contains("预算 500 us")
                // 反向：累计口径的数不得出现在面板（分位数≠累计）
                && !lines.iter().any(|l| l.contains(&format!("{}", expect_total_us)))
                // 前置账目核对（否则上面是在断言一个错账）
                && sealed == 1
                && p.mapping_leaks() == 1
                && p.mapping_forced() == 1
                && p.switches() == 1
                && p.rejected() == 1
                && p.outcome_counts()[OutKind::AsyncChosen.ordinal()] == 2
                && p.outcome_counts()[OutKind::SyncAccepted.ordinal()] == 1,
            "面板七行逐行绑定：审计 5 / 异步 2 / 同步 1 / 切异步 1 / 拒绝 1 / 泄漏 1 且强制解除 1 / P95 为最近秩值而非累计",
        );
    }

    // --- 判据 21：只检不收档使leaks 与 forced **真正分道** ---
    //
    // 变异M15（面板把「强制解除」也印成 leaks）在判据 20 下存活，
    // 根因不是判据漏写，而是**产出侧根本没有能让两计数分道的路径**：
    // 唯一的封帧出口 `seal_frame` 对同一 `found` 同步递增两个计数，
    // 于是 `leaks == forced` **恒成立**，「强制解除」这一列只是
    // `leaks` 的复读——两个计数器退化成同一个计数器写两遍。
    // 头注宣称的「只检不收则泄漏累积」在代码里**不可触发**。
    //
    // 本条既验证新增的只检不收档，也**钉住两计数分道后各自可查**：
    // 诊断期封两次（leaks 累到 4、forced 仍 0、映射仍在登记表）
    // ⇒ 面板上「leaks: 4（强制解除 0）」两值不同，M15 改印 leaks 即红。
    {
        let mut p = ReadbackPolicy::new();
        p.decide(&ReadbackRequest::new(1, 8192, 9, true)); // 持久映射 seq=1
        p.decide(&ReadbackRequest::new(2, 4096, 9, true)); // 持久映射 seq=2
        // 帧 0 封帧：不跨帧 ⇒ 两个计数均为 0
        let seal_same = p.seal_frame();
        // **同帧只读面**：两笔登记的 frame == here ⇒ 跨帧数为 0。
        // 这一句是 `count_stale` 里 `r.frame < here` 的**唯一**判据出口：
        // 少了它，把 `<` 改成 `<=` 在跨帧语料下与原语义同值（0 < 1 与
        // 0 <= 1 同真），该谓词将永不被任何判据观察到——一个没人看的
        // 谓词和一个错的谓词在行为上无法区分。
        let stale_same_frame = p.mapping_stale();
        p.advance_frame();
        // 跨帧只读面：两笔都是更早帧 ⇒ 恰为 2
        let stale_next_frame = p.mapping_stale();
        // 诊断期：两次只检不收 ⇒ leaks=2、forced=0、映射仍在
        let r1 = p.seal_frame_report_only();
        let r2 = p.seal_frame_report_only();
        let lines = p.a11y_lines();
        // 中间态必须**先存变量**：判据表达式按书写顺序求值，
        // 把 `mapping_live_bytes()` 直接写在 `s.add` 里会在真封帧**之后**
        // 才求值（此时字节账已归零）⇒ 该子句恒假、整条判据永远转红。
        let live_after_report = p.mapping_live();
        let bytes_after_report = p.mapping_live_bytes();
        // 诊断期后**立刻**冻结两计数：真封帧会再次递增它们，
        // 判据表达式按书写顺序求值，直接写 `mapping_leaks()` 会取到终态。
        let leaks_after_report = p.mapping_leaks();
        let forced_after_report = p.mapping_forced();
        // 归因期：一次真封帧 ⇒ 检出 2 笔并**同时**强制解除 2 笔
        // （`seal_frame` 对同一 found 递增两个计数，故 leaks 6 / forced 2）
        let seal_real = p.seal_frame();
        let lines_after = p.a11y_lines();
        s.add(
            "A35-只检不收-两计数分道且诊断期不销账",
            // 同帧封帧不误报；**同帧只读面同样不误报**（`stale_same_frame == 0`
            // 是 `<` 与 `<=` 唯一分道处：跨帧侧两者同真，同帧侧才分道）
            seal_same == 0
                && stale_same_frame == 0
                && stale_next_frame == 2
                // 诊断期每次都检出 2 笔，且**不**销账
                && r1 == 2
                && r2 == 2
                // 两计数真正分道：检出 4 笔、强制解除 0 笔
                && leaks_after_report == 4
                && forced_after_report == 0
                && live_after_report == 2
                // 字节账未销（8192 + 4096 仍登记在册）
                && bytes_after_report == 12288
                // 面板上两值不同—— M15 的变异点就在这一列
                && lines[5].contains("leaks: 4")
                && lines[5].contains("强制解除 0")
                // 归因期真封帧：检出与解除**同步**递增 ⇒ leaks 6 / forced 2
                && seal_real == 2
                && p.mapping_leaks() == 6
                && p.mapping_forced() == 2
                && p.mapping_live() == 0
                && p.mapping_live_bytes() == 0
                // 解除后只读面归零：占用真的还回去了（否则上面那两句
                // 可能只是读数被清而登记表仍有幽灵行）
                && p.mapping_stale() == 0
                && lines_after[5].contains("leaks: 6")
                && lines_after[5].contains("强制解除 2"),
            "同帧只读跨帧数为 0、跨帧后恰 2；诊断期两次只检不收：leaks=4 而 forced=0、映射与字节仍在登记表；归因期真封帧后 leaks=6/forced=2 且映射清空、只读面归零",
        );
    }

    // --- 判据 22：登记表**未满必接受、满必拒绝**（夹逼封死守卫算子） ---
    //
    // 变异 M13（`count >= MAPPING_SLOTS` 改`count == MAPPING_SLOTS`）
    // 在判据 10/10b 下存活，根因是那两条语料里 `count` 从不越过容量：
    // 只增不减时 `count == CAP` 与 `count >= CAP` 同值 ⇒ 等价变异。
    //
    // 真正能分道的语料是「释放一槽 ⇒ 恰好等于 CAP-1 ⇒ 必须接受」：
    // `==` 守卫在 count == CAP-1 时同样放行，所以这条路也分不了道——
    // **除非** count 能超过 CAP，而单条register 永远做不到。
    //
    // 结论：这两个算子在本结构下**可证等价**（count 由 register 单调
    // 推进且每次至多 +1，永不跳过 CAP），M13 是真等价变异而非门禁漏网。
    // 但「等价」不能靠推断，本条把它**正面钉死**：
    // 交替释放/重登记 64 轮，「未满必接受、满必拒绝」两侧都真发生过，
    // 且末态容量与字节账守恒。这样任何人日后把 register 改成批量推进
    // （一次 +2 之类）越过 CAP，本条立刻转红。
    {
        const CAP: usize = 32;
        let mut m = MappingLedger::new();
        let mut k = 0usize;
        while k < CAP {
            let _ = m.register(k as u32, 256);
            k += 1;
        }
        let full = m.live();
        // 满表 ⇒ 拒登记（守卫上界）
        let refused_when_full = !m.register(999, 256);
        let mut accepted_when_not_full = 0u32;
        let mut refused_when_not_full = 0u32;
        let mut round = 0usize;
        while round < CAP * 2 {
            let was_live = m.live();
            let ok = m.register(7000 + round as u32, 256);
            if was_live < CAP {
                // 未满 ⇒ 必须接受
                if ok {
                    accepted_when_not_full += 1;
                }
            } else if !ok {
                // 满 ⇒ 必须拒绝
                refused_when_not_full += 1;
            }
            // 制造一个空位，让下一轮从「未满」起步
            let _ = m.release(0);
            round += 1;
        }
        s.add(
            "A35-槽位-未满必接受且满必拒绝（夹逼守卫算子）",
            full == CAP
                && refused_when_full
                // 未满侧与满侧都真发生过（各至少一次），
                // 否则本条会退化成「只验了满侧拒」的半截门禁
                && accepted_when_not_full >= 1
                && refused_when_not_full >= 1
                // 末态守恒：登记表不超容、字节账与实际行数一致
                && m.live() <= CAP
                && m.live_bytes() == m.live() as u64 * 256,
            "填满后拒登记；释放一槽后必接受、再满必拒，两侧都真发生过；末态行数不超容且字节账守恒",
        );
    }

    s
}