//! m700lock — VARIX-M700 AI-06 同步原语域 (F126~F150)
//!
//! 锁全家福/争用热图/自旋预算官/锁序大典/无锁队列谱/原子序谱/
//! 条件变量律/RCU 微型化/信号量谱系/屏障礼仪/死锁预言机/
//! 睡眠锁持锁自旋检测/优先级继承链/每核局部锁/事务内存实验舱/
//! 争用回归走廊/锁统计分账/假共享猎手/信号礼仪规范/双检锁审计/
//! 同步原语看门狗/无锁栈验证/同步事件流/锁-free 化路线图/同步域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F126 — 锁全家福：全系统同步原语登记
// ===========================================================================

pub const LK_FAMILY_SLOTS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LkLockKind {
    Spin,
    Mutex,
    Rw,
    Seq,
}

/// 锁登记簿：id 去重，容量 12 拒绝溢出。
pub struct LkLockFamily {
    ids: [Option<u32>; LK_FAMILY_SLOTS],
    kinds: [LkLockKind; LK_FAMILY_SLOTS],
    count: usize,
}

impl LkLockFamily {
    pub const fn new() -> LkLockFamily {
        LkLockFamily {
            ids: [const { None }; LK_FAMILY_SLOTS],
            kinds: [LkLockKind::Spin; LK_FAMILY_SLOTS],
            count: 0,
        }
    }

    pub fn register(&mut self, id: u32, kind: LkLockKind) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.ids[i] == Some(id) {
                return false;
            }
            i += 1;
        }
        if self.count >= LK_FAMILY_SLOTS {
            return false;
        }
        self.ids[self.count] = Some(id);
        self.kinds[self.count] = kind;
        self.count += 1;
        true
    }

    pub fn count_kind(&self, kind: LkLockKind) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.kinds[i] == kind {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F127 — 争用热图：每核争用计数与热点核
// ===========================================================================

pub const LK_HEAT_CORES: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct LkHeatmap {
    pub hits: [u64; LK_HEAT_CORES],
}

impl LkHeatmap {
    pub const fn new() -> LkHeatmap {
        LkHeatmap { hits: [0; LK_HEAT_CORES] }
    }

    pub fn record(&mut self, core: usize) -> bool {
        if core >= LK_HEAT_CORES {
            return false;
        }
        self.hits[core] += 1;
        true
    }

    /// 最热核（并列取最小下标）；全冷返回 None。
    pub fn hottest_core(&self) -> Option<usize> {
        let mut total = 0u64;
        let mut i = 0usize;
        while i < LK_HEAT_CORES {
            total += self.hits[i];
            i += 1;
        }
        if total == 0 {
            return None;
        }
        let mut best = 0usize;
        let mut j = 1usize;
        while j < LK_HEAT_CORES {
            if self.hits[j] > self.hits[best] {
                best = j;
            }
            j += 1;
        }
        Some(best)
    }

    /// 该核争用占比 permille。
    pub fn contention_permille(&self, core: usize) -> u32 {
        let mut total = 0u64;
        let mut i = 0usize;
        while i < LK_HEAT_CORES {
            total += self.hits[i];
            i += 1;
        }
        if core >= LK_HEAT_CORES || total == 0 {
            0
        } else {
            (self.hits[core] * 1000 / total) as u32
        }
    }
}

// ===========================================================================
// F128 — 自旋预算官：预算内自旋，连续耗尽必须入睡
// ===========================================================================

pub const LK_SPIN_BUDGET: u32 = 100;
pub const LK_SPIN_EXHAUST_TO_SLEEP: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct LkSpinBudget {
    pub remaining: u32,
    pub exhaustions: u32,
}

impl LkSpinBudget {
    pub const fn new() -> LkSpinBudget {
        LkSpinBudget { remaining: LK_SPIN_BUDGET, exhaustions: 0 }
    }

    /// 自旋一步；预算耗尽记一次 exhaustion。
    pub fn spin_once(&mut self) -> bool {
        if self.remaining > 0 {
            self.remaining -= 1;
            true
        } else {
            self.exhaustions += 1;
            false
        }
    }

    pub fn must_sleep(&self) -> bool {
        self.exhaustions >= LK_SPIN_EXHAUST_TO_SLEEP
    }

    pub fn refill(&mut self) {
        self.remaining = LK_SPIN_BUDGET;
    }
}

// ===========================================================================
// F129 — 锁序大典：先低地址后高地址，序列严格递增
// ===========================================================================

pub fn lock_order_ok(a: u64, b: u64) -> bool {
    a < b
}

/// 一次获取序列是否满足锁序（严格递增）。
pub fn acquire_sequence_ok(order: &[u64]) -> bool {
    let mut i = 1usize;
    while i < order.len() {
        if order[i] <= order[i - 1] {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F130 — 无锁队列谱：定容环形 FIFO
// ===========================================================================

pub const LK_RING_CAP: usize = 8;

pub struct LkRingQueue {
    slots: [Option<u64>; LK_RING_CAP],
    head: usize,
    len: usize,
}

impl LkRingQueue {
    pub const fn new() -> LkRingQueue {
        LkRingQueue {
            slots: [const { None }; LK_RING_CAP],
            head: 0,
            len: 0,
        }
    }

    pub fn enqueue(&mut self, v: u64) -> bool {
        if self.len >= LK_RING_CAP {
            return false;
        }
        let tail = (self.head + self.len) % LK_RING_CAP;
        self.slots[tail] = Some(v);
        self.len += 1;
        true
    }

    pub fn dequeue(&mut self) -> Option<u64> {
        if self.len == 0 {
            return None;
        }
        let v = self.slots[self.head].take();
        self.head = (self.head + 1) % LK_RING_CAP;
        self.len -= 1;
        v
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ===========================================================================
// F131 — 原子序谱：内存序强度分级
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LkAtomicOrdering {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}

pub fn ordering_rank(o: LkAtomicOrdering) -> u8 {
    match o {
        LkAtomicOrdering::Relaxed => 0,
        LkAtomicOrdering::Acquire => 1,
        LkAtomicOrdering::Release => 1,
        LkAtomicOrdering::AcqRel => 2,
        LkAtomicOrdering::SeqCst => 3,
    }
}

pub fn ordering_at_least(have: LkAtomicOrdering, need: LkAtomicOrdering) -> bool {
    ordering_rank(have) >= ordering_rank(need)
}

// ===========================================================================
// F132 — 条件变量律：持锁等待，唤醒记账
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct LkCondVar {
    pub waiters: u32,
    pub pending_wakes: u32,
}

impl LkCondVar {
    /// 等待必须持锁；无锁等待直接拒绝（丢失唤醒温床）。
    pub fn wait(&mut self, lock_held: bool) -> bool {
        if !lock_held {
            return false;
        }
        self.waiters += 1;
        true
    }

    pub fn signal_one(&mut self) -> bool {
        if self.waiters == 0 {
            return false;
        }
        self.waiters -= 1;
        self.pending_wakes += 1;
        true
    }

    pub fn broadcast(&mut self) -> u32 {
        let n = self.waiters;
        self.waiters = 0;
        self.pending_wakes += n;
        n
    }
}

// ===========================================================================
// F133 — RCU 微型化：读者在场则写者延期，宽限期后发布
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct LkRcu {
    pub readers: u32,
    pub grace_pending: bool,
    pub published: u32,
}

impl LkRcu {
    pub fn read_enter(&mut self) -> bool {
        self.readers += 1;
        true
    }

    pub fn read_exit(&mut self) -> bool {
        if self.readers == 0 {
            return false;
        }
        self.readers -= 1;
        true
    }

    /// 写者更新：有读者在场则挂起宽限期。
    pub fn update(&mut self) -> bool {
        if self.readers > 0 {
            self.grace_pending = true;
            return false;
        }
        self.published += 1;
        true
    }

    /// 宽限期结算：读者全部离场后发布并清挂起位。
    pub fn grace_elapsed(&mut self) -> bool {
        if self.grace_pending && self.readers == 0 {
            self.grace_pending = false;
            self.published += 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F134 — 信号量谱系：计数信号量，下不越零上不越顶
// ===========================================================================

pub const LK_SEM_MAX: u32 = 8;

#[derive(Clone, Copy, Debug)]
pub struct LkSemaphore {
    pub count: u32,
}

impl LkSemaphore {
    pub const fn new(count: u32) -> LkSemaphore {
        LkSemaphore { count }
    }

    /// P 操作：计数为零则阻塞（返回 false）。
    pub fn down(&mut self) -> bool {
        if self.count == 0 {
            return false;
        }
        self.count -= 1;
        true
    }

    /// V 操作：达到上限则拒绝（防溢出）。
    pub fn up(&mut self) -> bool {
        if self.count >= LK_SEM_MAX {
            return false;
        }
        self.count += 1;
        true
    }
}

// ===========================================================================
// F135 — 屏障礼仪：四方到齐才放行，代际可复用
// ===========================================================================

pub const LK_BARRIER_PARTIES: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct LkBarrier {
    pub arrived: u32,
    pub generation: u32,
}

impl LkBarrier {
    pub const fn new() -> LkBarrier {
        LkBarrier { arrived: 0, generation: 0 }
    }

    /// 到达即返回；最后一名返回 true（本代放行）并开启下一代。
    pub fn arrive(&mut self) -> bool {
        self.arrived += 1;
        if self.arrived >= LK_BARRIER_PARTIES {
            self.arrived = 0;
            self.generation += 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F136 — 死锁预言机：等待图环路检测（每节点至多等一锁主）
// ===========================================================================

pub const LK_ORACLE_NODES: usize = 8;

/// 从 start 沿等待边走，回到路径上已见的节点即有环。
pub fn wait_for_cycle(edges: &[Option<usize>; LK_ORACLE_NODES], start: usize) -> bool {
    let mut on_path = [false; LK_ORACLE_NODES];
    let mut cur = start;
    loop {
        if on_path[cur] {
            return true;
        }
        on_path[cur] = true;
        match edges[cur] {
            Some(n) => cur = n,
            None => return false,
        }
    }
}

/// 系统级死锁：任一起点探得环路即成立。
pub fn any_deadlock(edges: &[Option<usize>; LK_ORACLE_NODES]) -> bool {
    let mut s = 0usize;
    while s < LK_ORACLE_NODES {
        if wait_for_cycle(edges, s) {
            return true;
        }
        s += 1;
    }
    false
}

// ===========================================================================
// F137 — 睡眠锁持锁自旋检测：持自旋锁入睡 = 违规
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct LkSleeper {
    pub violations: u32,
    pub clean_sleeps: u32,
}

impl LkSleeper {
    /// 入睡申请：持自旋锁者拒绝并记账。
    pub fn try_sleep(&mut self, holding_spin: bool) -> bool {
        if holding_spin {
            self.violations += 1;
            return false;
        }
        self.clean_sleeps += 1;
        true
    }
}

// ===========================================================================
// F138 — 优先级继承链：沿链传播最高优先级（数值小者优先）
// ===========================================================================

pub const LK_PI_CHAIN_MAX: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct LkPiNode {
    pub owner_prio: u32,
    pub waiter_prio: u32,
}

/// 单级继承：等待者优先级更高（数值更小）则抬升所有者。
pub fn inherit(owner: &mut LkPiNode, waiter_prio: u32) -> bool {
    if waiter_prio < owner.owner_prio {
        owner.owner_prio = waiter_prio;
        true
    } else {
        false
    }
}

/// 链式继承：boost 沿链传播，最多 LK_PI_CHAIN_MAX 跳，返回最终传播值。
pub fn chain_boost(chain: &mut [LkPiNode], base_prio: u32) -> u32 {
    let mut prio = base_prio;
    let mut hops = 0usize;
    while hops < chain.len() && hops < LK_PI_CHAIN_MAX {
        if prio < chain[hops].owner_prio {
            chain[hops].owner_prio = prio;
        }
        prio = chain[hops].owner_prio;
        hops += 1;
    }
    prio
}

// ===========================================================================
// F139 — 每核局部锁：本核无争用，远核触碰记账
// ===========================================================================

pub const LK_CORE_LOCKS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct LkCoreLock {
    pub held: bool,
    pub acquisitions: u64,
    pub remote_hits: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct LkCoreLocks {
    locks: [LkCoreLock; LK_CORE_LOCKS],
}

impl LkCoreLocks {
    pub const fn new() -> LkCoreLocks {
        LkCoreLocks {
            locks: [
                LkCoreLock { held: false, acquisitions: 0, remote_hits: 0 };
                LK_CORE_LOCKS
            ],
        }
    }

    /// 本核获取：已持锁或核号越界拒绝。
    pub fn acquire_local(&mut self, core: usize) -> bool {
        if core >= LK_CORE_LOCKS || self.locks[core].held {
            return false;
        }
        self.locks[core].held = true;
        self.locks[core].acquisitions += 1;
        true
    }

    /// 远核触碰：只记账，不改变持有状态；返回该锁当前是否被持。
    pub fn touch_remote(&mut self, core: usize) -> bool {
        if core >= LK_CORE_LOCKS {
            return false;
        }
        self.locks[core].remote_hits += 1;
        self.locks[core].held
    }

    pub fn release(&mut self, core: usize) -> bool {
        if core >= LK_CORE_LOCKS || !self.locks[core].held {
            return false;
        }
        self.locks[core].held = false;
        true
    }

    pub fn acquisitions_of(&self, core: usize) -> u64 {
        if core >= LK_CORE_LOCKS {
            0
        } else {
            self.locks[core].acquisitions
        }
    }

    pub fn remote_hits_of(&self, core: usize) -> u64 {
        if core >= LK_CORE_LOCKS {
            0
        } else {
            self.locks[core].remote_hits
        }
    }
}

// ===========================================================================
// F140 — 事务内存实验舱：写集冲突检测
// ===========================================================================

pub const LK_TM_WRITES: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct LkTxn {
    addrs: [Option<u64>; LK_TM_WRITES],
    pub active: bool,
}

impl LkTxn {
    pub const fn begin() -> LkTxn {
        LkTxn { addrs: [const { None }; LK_TM_WRITES], active: true }
    }

    /// 记录一次写；同地址覆写允许（末次为准），写集满或事务已提交拒绝。
    pub fn write(&mut self, addr: u64) -> bool {
        if !self.active {
            return false;
        }
        let mut i = 0usize;
        while i < LK_TM_WRITES {
            if self.addrs[i] == Some(addr) {
                return true;
            }
            i += 1;
        }
        i = 0;
        while i < LK_TM_WRITES {
            if self.addrs[i].is_none() {
                self.addrs[i] = Some(addr);
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn commit(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.active = false;
        true
    }

    pub fn touches(&self, addr: u64) -> bool {
        let mut i = 0usize;
        while i < LK_TM_WRITES {
            if self.addrs[i] == Some(addr) {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 两事务写集相交即冲突（提交前必须仲裁）。
pub fn txns_conflict(a: &LkTxn, b: &LkTxn) -> bool {
    let mut i = 0usize;
    while i < LK_TM_WRITES {
        if let Some(addr) = a.addrs[i] {
            if b.touches(addr) {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ===========================================================================
// F141 — 争用回归走廊：争用率不超 300‰ 金线
// ===========================================================================

pub const LK_CONTENTION_TARGET_PERMILLE: u32 = 300;

#[derive(Clone, Copy, Debug, Default)]
pub struct LkContentionSample {
    pub acquisitions: u64,
    pub contended: u64,
}

pub fn sample_contended_permille(s: &LkContentionSample) -> u32 {
    if s.acquisitions == 0 {
        0
    } else {
        (s.contended * 1000 / s.acquisitions) as u32
    }
}

pub fn regression_ok(s: &LkContentionSample) -> bool {
    sample_contended_permille(s) <= LK_CONTENTION_TARGET_PERMILLE
}

// ===========================================================================
// F142 — 锁统计分账：按锁分账，汇总对账
// ===========================================================================

pub const LK_STAT_LOCKS: usize = 4;

#[derive(Clone, Copy, Debug, Default)]
pub struct LkLockStat {
    pub acquisitions: u64,
    pub contended: u64,
    pub hold_ticks: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct LkStatsBook {
    pub locks: [LkLockStat; LK_STAT_LOCKS],
}

impl LkStatsBook {
    pub const fn new() -> LkStatsBook {
        LkStatsBook {
            locks: [LkLockStat { acquisitions: 0, contended: 0, hold_ticks: 0 }; LK_STAT_LOCKS],
        }
    }

    pub fn total_acquisitions(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < LK_STAT_LOCKS {
            n += self.locks[i].acquisitions;
            i += 1;
        }
        n
    }

    pub fn total_contended(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < LK_STAT_LOCKS {
            n += self.locks[i].contended;
            i += 1;
        }
        n
    }

    pub fn total_hold_ticks(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < LK_STAT_LOCKS {
            n += self.locks[i].hold_ticks;
            i += 1;
        }
        n
    }

    pub fn contended_permille(&self) -> u32 {
        let total = self.total_acquisitions();
        if total == 0 {
            0
        } else {
            (self.total_contended() * 1000 / total) as u32
        }
    }
}

// ===========================================================================
// F143 — 假共享猎手：同缓存行的热变量必须隔行
// ===========================================================================

pub const LK_CACHE_LINE_BYTES: u64 = 64;

pub fn same_cache_line(a: u64, b: u64) -> bool {
    a / LK_CACHE_LINE_BYTES == b / LK_CACHE_LINE_BYTES
}

/// 距下一行边界的字节数（已对齐返回 0）。
pub fn bytes_to_next_line(addr: u64) -> u64 {
    let r = addr % LK_CACHE_LINE_BYTES;
    if r == 0 {
        0
    } else {
        LK_CACHE_LINE_BYTES - r
    }
}

// ===========================================================================
// F144 — 信号礼仪规范：持锁发信号，裸奔即违规
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct LkSignalLog {
    pub under_lock: u32,
    pub naked: u32,
}

impl LkSignalLog {
    pub fn record_signal(&mut self, lock_held: bool) {
        if lock_held {
            self.under_lock += 1;
        } else {
            self.naked += 1;
        }
    }

    pub fn etiquette_ok(&self) -> bool {
        self.naked == 0
    }
}

// ===========================================================================
// F145 — 双检锁审计：双检 + 单次初始化
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct LkDcl {
    pub initialized: bool,
    pub inits: u32,
}

impl LkDcl {
    pub const fn new() -> LkDcl {
        LkDcl { initialized: false, inits: 0 }
    }

    /// fast_path_init 表示调用方已持有初始化锁。
    /// 双检：先查已初始化，拿锁后再查一次，恰好一次真正初始化。
    pub fn dcl_init(&mut self, fast_path_init: bool) -> bool {
        if self.initialized {
            return false;
        }
        if !fast_path_init {
            return false;
        }
        if self.initialized {
            return false;
        }
        self.initialized = true;
        self.inits += 1;
        true
    }
}

// ===========================================================================
// F146 — 同步原语看门狗：持锁超时清点
// ===========================================================================

pub const LK_HOLD_MAX_TICKS: u64 = 1000;

#[derive(Clone, Copy, Debug)]
pub struct LkHoldRecord {
    pub lock_id: u32,
    pub held_ticks: u64,
}

pub fn hold_expired(r: &LkHoldRecord) -> bool {
    r.held_ticks > LK_HOLD_MAX_TICKS
}

pub fn watchdog_sweep(records: &[LkHoldRecord]) -> usize {
    records.iter().filter(|r| hold_expired(r)).count()
}

// ===========================================================================
// F147 — 无锁栈验证：LIFO 栈 + ABA 代际标签
// ===========================================================================

pub const LK_STACK_CAP: usize = 8;

pub struct LkTreiberStack {
    items: [Option<u64>; LK_STACK_CAP],
    depth: usize,
    pub aba_tag: u64,
}

impl LkTreiberStack {
    pub const fn new() -> LkTreiberStack {
        LkTreiberStack {
            items: [const { None }; LK_STACK_CAP],
            depth: 0,
            aba_tag: 0,
        }
    }

    pub fn push(&mut self, v: u64) -> bool {
        if self.depth >= LK_STACK_CAP {
            return false;
        }
        self.items[self.depth] = Some(v);
        self.depth += 1;
        self.aba_tag += 1;
        true
    }

    pub fn pop(&mut self) -> Option<u64> {
        if self.depth == 0 {
            return None;
        }
        self.depth -= 1;
        let v = self.items[self.depth].take();
        self.aba_tag += 1;
        v
    }

    pub fn depth(&self) -> usize {
        self.depth
    }
}

// ===========================================================================
// F148 — 同步事件流：定容环形事件流，序号随事件推进
// ===========================================================================

pub const LK_EVENT_RING: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LkSyncEventKind {
    Acquire,
    Release,
    Contend,
    Sleep,
}

pub struct LkEventRing {
    kinds: [LkSyncEventKind; LK_EVENT_RING],
    seqs: [u64; LK_EVENT_RING],
    head: usize,
}

impl LkEventRing {
    pub const fn new() -> LkEventRing {
        LkEventRing {
            kinds: [LkSyncEventKind::Acquire; LK_EVENT_RING],
            seqs: [0; LK_EVENT_RING],
            head: 0,
        }
    }

    pub fn push(&mut self, seq: u64, kind: LkSyncEventKind) {
        self.kinds[self.head] = kind;
        self.seqs[self.head] = seq;
        self.head = (self.head + 1) % LK_EVENT_RING;
    }

    pub fn len(&self) -> usize {
        LK_EVENT_RING
    }

    pub fn kind_at(&self, i: usize) -> LkSyncEventKind {
        self.kinds[i % LK_EVENT_RING]
    }

    pub fn seq_at(&self, i: usize) -> u64 {
        self.seqs[i % LK_EVENT_RING]
    }
}

// ===========================================================================
// F149 — 锁-free 化路线图：高争用原语优先改造
// ===========================================================================

pub const LK_CONVERT_MIN_PERMILLE: u32 = 500;

#[derive(Clone, Copy, Debug)]
pub struct LkCandidate {
    pub name_id: u32,
    pub contention_permille: u32,
}

pub fn conversion_worthy(c: &LkCandidate) -> bool {
    c.contention_permille >= LK_CONVERT_MIN_PERMILLE
}

/// 最该改造的候选（争用最高）；无合格者返回 0。
pub fn top_candidate(cs: &[LkCandidate]) -> u32 {
    let mut best: Option<&LkCandidate> = None;
    let mut i = 0usize;
    while i < cs.len() {
        let c = &cs[i];
        if conversion_worthy(c) {
            match best {
                Some(b) if b.contention_permille >= c.contention_permille => {}
                _ => best = Some(c),
            }
        }
        i += 1;
    }
    match best {
        Some(b) => b.name_id,
        None => 0,
    }
}

// ===========================================================================
// F150 — 同步域年报
// ===========================================================================

pub const LK_REPORT_SECTIONS: [&str; 5] = ["family", "heatmap", "spin", "order", "events"];

pub fn lock_report_complete(filled: u32) -> bool {
    filled >= LK_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700lock_checks() -> CheckSet {
    let mut set = CheckSet::new("m700lock");

    // F126 锁全家福
    let mut fam = LkLockFamily::new();
    let r1 = fam.register(1, LkLockKind::Spin);
    let r2 = fam.register(2, LkLockKind::Mutex);
    let r3 = fam.register(1, LkLockKind::Rw);
    let fam_count = fam.count();
    set.add("F126 family register", r1 && r2 && fam_count == 2, "two locks");
    set.add(
        "F126 family dedup kinds",
        !r3 && fam.count() == 2 && fam.count_kind(LkLockKind::Spin) == 1,
        "id dedup, kind tally",
    );
    let mut ffull = LkLockFamily::new();
    let mut id = 1u32;
    while ffull.count() < LK_FAMILY_SLOTS {
        ffull.register(id, LkLockKind::Seq);
        id += 1;
    }
    let ffull_count = ffull.count();
    set.add(
        "F126 family capacity",
        ffull_count == LK_FAMILY_SLOTS && !ffull.register(999, LkLockKind::Spin),
        "cap 12",
    );

    // F127 争用热图
    let mut heat = LkHeatmap::new();
    heat.record(0);
    heat.record(0);
    heat.record(0);
    heat.record(0);
    heat.record(0);
    heat.record(1);
    heat.record(1);
    heat.record(1);
    heat.record(2);
    heat.record(2);
    heat.record(3);
    heat.record(3);
    let hot = heat.hottest_core();
    let hot_permille = heat.contention_permille(0);
    set.add(
        "F127 heat hottest",
        heat.hits[0] == 5 && heat.hits[1] == 3 && hot == Some(0),
        "core 0 hottest",
    );
    set.add(
        "F127 heat permille",
        hot_permille == 416 && heat.contention_permille(9) == 0 && LkHeatmap::new().hottest_core().is_none(),
        "5/12 = 416, bounds",
    );

    // F128 自旋预算官
    let mut sb = LkSpinBudget::new();
    let mut spins = 0u32;
    while spins < LK_SPIN_BUDGET {
        sb.spin_once();
        spins += 1;
    }
    let drained = sb.remaining;
    let spin_over = sb.spin_once();
    let exhaustion1 = sb.exhaustions;
    set.add(
        "F128 spin budget drain",
        drained == 0 && !spin_over && exhaustion1 == 1,
        "100 spins then exhausted",
    );
    let mut tired = LkSpinBudget { remaining: 0, exhaustions: LK_SPIN_EXHAUST_TO_SLEEP - 1 };
    let over = tired.spin_once();
    let must = tired.must_sleep();
    tired.refill();
    set.add(
        "F128 spin must sleep",
        !over && must && tired.remaining == LK_SPIN_BUDGET,
        "4th exhaustion sleeps, refill resets",
    );

    // F129 锁序大典
    set.add(
        "F129 lock order pair",
        lock_order_ok(1, 2) && !lock_order_ok(2, 1) && !lock_order_ok(2, 2),
        "strict a < b",
    );
    let good_seq = [1u64, 2, 3];
    let bad_seq = [1u64, 3, 2];
    set.add(
        "F129 lock order sequence",
        acquire_sequence_ok(&good_seq) && !acquire_sequence_ok(&bad_seq),
        "strictly increasing",
    );

    // F130 无锁队列谱
    let mut rq = LkRingQueue::new();
    let e1 = rq.enqueue(1);
    let e2 = rq.enqueue(2);
    let e3 = rq.enqueue(3);
    let d1 = rq.dequeue();
    let d2 = rq.dequeue();
    set.add(
        "F130 ring fifo",
        e1 && e2 && e3 && d1 == Some(1) && d2 == Some(2),
        "in order",
    );
    let mut fq = LkRingQueue::new();
    let mut n = 0u64;
    while n < LK_RING_CAP as u64 {
        fq.enqueue(n);
        n += 1;
    }
    let fq_len = fq.len();
    set.add(
        "F130 ring full/empty",
        fq_len == LK_RING_CAP && !fq.enqueue(99) && LkRingQueue::new().dequeue().is_none(),
        "cap 8, empty none",
    );

    // F131 原子序谱
    set.add(
        "F131 ordering rank",
        ordering_rank(LkAtomicOrdering::Acquire) == ordering_rank(LkAtomicOrdering::Release)
            && ordering_rank(LkAtomicOrdering::SeqCst) == 3,
        "acquire == release strength",
    );
    set.add(
        "F131 ordering at least",
        !ordering_at_least(LkAtomicOrdering::Relaxed, LkAtomicOrdering::Release)
            && ordering_at_least(LkAtomicOrdering::SeqCst, LkAtomicOrdering::Acquire)
            && ordering_at_least(LkAtomicOrdering::AcqRel, LkAtomicOrdering::Release),
        "weaker rejected, stronger ok",
    );

    // F132 条件变量律
    let mut cv = LkCondVar::default();
    let nw = cv.wait(false);
    let w1 = cv.wait(true);
    let w2 = cv.wait(true);
    set.add("F132 cond wait needs lock", !nw && w1 && w2 && cv.waiters == 2, "no lock no wait");
    let sg = cv.signal_one();
    let waiters_after_signal = cv.waiters;
    let wakes_after_signal = cv.pending_wakes;
    let bc = cv.broadcast();
    let waiters_after_bc = cv.waiters;
    let wakes_after_bc = cv.pending_wakes;
    set.add(
        "F132 cond wake ledger",
        sg && waiters_after_signal == 1 && wakes_after_signal == 1,
        "one wake",
    );
    set.add(
        "F132 cond broadcast",
        bc == 1 && waiters_after_bc == 0 && wakes_after_bc == 2,
        "remaining waiter woken",
    );

    // F133 RCU 微型化
    let mut rcu = LkRcu::default();
    rcu.read_enter();
    rcu.read_enter();
    let readers_in = rcu.readers;
    let upd = rcu.update();
    let grace_pending = rcu.grace_pending;
    set.add(
        "F133 rcu deferred update",
        readers_in == 2 && !upd && grace_pending,
        "readers block publish",
    );
    rcu.read_exit();
    rcu.read_exit();
    let readers_gone = rcu.readers;
    let settled = rcu.grace_elapsed();
    let published1 = rcu.published;
    set.add(
        "F133 rcu grace settled",
        readers_gone == 0 && settled && published1 == 1,
        "publish after grace",
    );
    set.add("F133 rcu exit underflow", !rcu.read_exit(), "no reader to exit");

    // F134 信号量谱系
    let mut sem = LkSemaphore::new(2);
    let d1 = sem.down();
    let d2 = sem.down();
    let d3 = sem.down();
    let sem_zero = sem.count;
    set.add("F134 sem down to zero", d1 && d2 && !d3 && sem_zero == 0, "P blocks at 0");
    sem.up();
    sem.up();
    let mut sem_full = LkSemaphore::new(LK_SEM_MAX);
    let up_over = sem_full.up();
    let up_ok = sem.up();
    set.add(
        "F134 sem up capped",
        up_ok && sem.count == 3 && !up_over && sem_full.count == LK_SEM_MAX,
        "V refuses past max",
    );

    // F135 屏障礼仪
    let mut bar = LkBarrier::new();
    let a1 = bar.arrive();
    let a2 = bar.arrive();
    let a3 = bar.arrive();
    let gen_before = bar.generation;
    let a4 = bar.arrive();
    let gen_after = bar.generation;
    set.add(
        "F135 barrier trips",
        !a1 && !a2 && !a3 && a4 && gen_before == 0 && gen_after == 1,
        "4th arrival releases",
    );
    let a5 = bar.arrive();
    let a6 = bar.arrive();
    let a7 = bar.arrive();
    let a8 = bar.arrive();
    let gen_reuse = bar.generation;
    set.add("F135 barrier reuse", !a5 && !a6 && !a7 && a8 && gen_reuse == 2, "next generation");

    // F136 死锁预言机
    let mut cyc: [Option<usize>; LK_ORACLE_NODES] = [const { None }; LK_ORACLE_NODES];
    cyc[0] = Some(1);
    cyc[1] = Some(2);
    cyc[2] = Some(0);
    let mut acy: [Option<usize>; LK_ORACLE_NODES] = [const { None }; LK_ORACLE_NODES];
    acy[0] = Some(1);
    acy[2] = Some(1);
    set.add(
        "F136 oracle cycle",
        wait_for_cycle(&cyc, 0) && any_deadlock(&cyc),
        "0→1→2→0 closed",
    );
    set.add(
        "F136 oracle clean",
        !wait_for_cycle(&acy, 0) && !any_deadlock(&acy),
        "no cycle",
    );

    // F137 睡眠锁持锁自旋检测
    let mut sl = LkSleeper::default();
    let bad_sleep = sl.try_sleep(true);
    let violations1 = sl.violations;
    let good_sleep = sl.try_sleep(false);
    set.add(
        "F137 sleep etiquette",
        !bad_sleep && violations1 == 1 && good_sleep && sl.clean_sleeps == 1,
        "spin-held sleep denied",
    );

    // F138 优先级继承链
    let mut owner = LkPiNode { owner_prio: 10, waiter_prio: 0 };
    let inh1 = inherit(&mut owner, 3);
    let owner_after = owner.owner_prio;
    let inh2 = inherit(&mut owner, 3);
    set.add(
        "F138 pi single",
        inh1 && owner_after == 3 && !inh2 && owner.owner_prio == 3,
        "boost once",
    );
    let mut chain = [
        LkPiNode { owner_prio: 10, waiter_prio: 0 },
        LkPiNode { owner_prio: 8, waiter_prio: 0 },
        LkPiNode { owner_prio: 12, waiter_prio: 0 },
    ];
    let boosted = chain_boost(&mut chain, 5);
    set.add(
        "F138 pi chain",
        boosted == 5
            && chain[0].owner_prio == 5
            && chain[1].owner_prio == 5
            && chain[2].owner_prio == 5,
        "min propagates along chain",
    );
    let high = chain_boost(&mut chain, 1);
    let hop_cap = chain_boost(&mut chain, 200);
    set.add(
        "F138 pi chain monotonic",
        high == 1 && hop_cap == 1,
        "never weakens below best",
    );

    // F139 每核局部锁
    let mut cl = LkCoreLocks::new();
    let ac1 = cl.acquire_local(1);
    let ac2 = cl.acquire_local(1);
    set.add("F139 core local acquire", ac1 && !ac2, "held rejects second");
    let touch_held = cl.touch_remote(1);
    let rel = cl.release(1);
    let remote_after = cl.remote_hits_of(1);
    let reacq = cl.acquire_local(1);
    let acq_total = cl.acquisitions_of(1);
    set.add(
        "F139 core remote bookkeeping",
        touch_held && rel && remote_after == 1,
        "remote touch recorded",
    );
    set.add(
        "F139 core reacquire",
        reacq && acq_total == 2 && !cl.release(0),
        "unheld release denied",
    );

    // F140 事务内存实验舱
    let mut t1 = LkTxn::begin();
    let mut t2 = LkTxn::begin();
    t1.write(100);
    t1.write(200);
    t2.write(200);
    t2.write(300);
    let conflict = txns_conflict(&t1, &t2);
    set.add("F140 txn conflict", t1.touches(100) && t2.touches(300) && conflict, "shared addr");
    let mut t3 = LkTxn::begin();
    t3.write(400);
    set.add("F140 txn disjoint", !txns_conflict(&t1, &t3), "disjoint clean");
    let c1 = t1.commit();
    let c2 = t1.commit();
    let w_after = t1.write(500);
    set.add(
        "F140 txn commit once",
        c1 && !c2 && !w_after && !t1.active,
        "commit is one-shot",
    );

    // F141 争用回归走廊
    let calm = LkContentionSample { acquisitions: 20, contended: 4 };
    let stormy = LkContentionSample { acquisitions: 10, contended: 5 };
    set.add(
        "F141 contention permille",
        sample_contended_permille(&calm) == 200 && sample_contended_permille(&stormy) == 500,
        "ratio",
    );
    set.add(
        "F141 contention gate",
        regression_ok(&calm) && !regression_ok(&stormy) && regression_ok(&LkContentionSample::default()),
        "300 permille golden line",
    );

    // F142 锁统计分账
    let mut book = LkStatsBook::new();
    book.locks[0].acquisitions = 5;
    book.locks[0].contended = 1;
    book.locks[2].hold_ticks = 7;
    let t_acq = book.total_acquisitions();
    let t_con = book.total_contended();
    let t_hold = book.total_hold_ticks();
    let con_pm = book.contended_permille();
    set.add(
        "F142 stats ledger",
        t_acq == 5 && t_con == 1 && t_hold == 7,
        "sums across locks",
    );
    set.add("F142 stats permille", con_pm == 200, "1/5 contended");

    // F143 假共享猎手
    set.add(
        "F143 same line",
        same_cache_line(0, 63) && !same_cache_line(63, 64),
        "line index differs",
    );
    set.add(
        "F143 pad to line",
        bytes_to_next_line(60) == 4 && bytes_to_next_line(64) == 0,
        "align boundary",
    );

    // F144 信号礼仪规范
    let mut slog = LkSignalLog::default();
    slog.record_signal(true);
    slog.record_signal(true);
    slog.record_signal(false);
    set.add(
        "F144 signal ledger",
        slog.under_lock == 2 && slog.naked == 1 && !slog.etiquette_ok(),
        "naked signal flagged",
    );
    let mut clean = LkSignalLog::default();
    clean.record_signal(true);
    set.add("F144 signal clean", clean.etiquette_ok() && clean.naked == 0, "all held");

    // F145 双检锁审计
    let mut dcl = LkDcl::new();
    let i1 = dcl.dcl_init(true);
    let i2 = dcl.dcl_init(true);
    set.add(
        "F145 dcl once",
        i1 && dcl.inits == 1 && !i2 && dcl.initialized,
        "exactly one init",
    );
    let mut nd = LkDcl::new();
    let i3 = nd.dcl_init(false);
    set.add(
        "F145 dcl no lock no init",
        !i3 && !nd.initialized && nd.inits == 0,
        "fast path guarded",
    );

    // F146 同步原语看门狗
    let holds = [
        LkHoldRecord { lock_id: 1, held_ticks: 1000 },
        LkHoldRecord { lock_id: 2, held_ticks: 1001 },
        LkHoldRecord { lock_id: 3, held_ticks: 2000 },
    ];
    set.add(
        "F146 watchdog boundary",
        !hold_expired(&holds[0]) && hold_expired(&holds[1]),
        "strict > 1000",
    );
    set.add("F146 watchdog sweep", watchdog_sweep(&holds) == 2, "two expired");

    // F147 无锁栈验证
    let mut st = LkTreiberStack::new();
    let p1 = st.push(1);
    let p2 = st.push(2);
    let p3 = st.push(3);
    let o1 = st.pop();
    let o2 = st.pop();
    let o3 = st.pop();
    set.add(
        "F147 stack lifo",
        p1 && p2 && p3 && o1 == Some(3) && o2 == Some(2) && o3 == Some(1),
        "last in first out",
    );
    let mut fst = LkTreiberStack::new();
    let mut m = 0u64;
    while m < LK_STACK_CAP as u64 {
        fst.push(m);
        m += 1;
    }
    let fst_depth = fst.depth();
    let ops_before = fst.aba_tag;
    let push_over = fst.push(99);
    let pop1 = fst.pop();
    let ops_after = fst.aba_tag;
    set.add(
        "F147 stack cap/aba",
        fst_depth == LK_STACK_CAP && !push_over,
        "cap 8",
    );
    set.add(
        "F147 stack aba tag",
        pop1.is_some() && ops_before == LK_STACK_CAP as u64 && ops_after == ops_before + 1,
        "tag per op",
    );

    // F148 同步事件流
    let mut ring = LkEventRing::new();
    let mut s = 0u64;
    while s < 10 {
        ring.push(s, if s % 2 == 0 { LkSyncEventKind::Acquire } else { LkSyncEventKind::Release });
        s += 1;
    }
    let last_kind = ring.kind_at(9);
    let first_kind = ring.kind_at(0);
    set.add(
        "F148 event ring wrap",
        ring.len() == 8 && last_kind == LkSyncEventKind::Release && first_kind == LkSyncEventKind::Acquire,
        "ring holds 8",
    );
    set.add("F148 event ring seq", ring.seq_at(9) == 9 && ring.seq_at(2) == 2, "seq tracked");

    // F149 锁-free 化路线图
    let cands = [
        LkCandidate { name_id: 7, contention_permille: 800 },
        LkCandidate { name_id: 9, contention_permille: 300 },
        LkCandidate { name_id: 11, contention_permille: 500 },
    ];
    set.add(
        "F149 roadmap worthy",
        conversion_worthy(&cands[0]) && !conversion_worthy(&cands[1]) && conversion_worthy(&cands[2]),
        "500 permille line",
    );
    let top = top_candidate(&cands);
    let none = top_candidate(&[LkCandidate { name_id: 9, contention_permille: 300 }]);
    set.add("F149 roadmap top", top == 7 && none == 0, "hottest first, quiet none");

    // F150 同步域年报
    set.add("F150 report sections", LK_REPORT_SECTIONS.len() == 5, "five sections");
    set.add("F150 report complete", lock_report_complete(5) && !lock_report_complete(4), "completeness");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f128_spin_budget_never_negative() {
        let mut sb = LkSpinBudget::new();
        for _ in 0..LK_SPIN_BUDGET {
            assert!(sb.spin_once());
        }
        assert!(!sb.spin_once());
        assert_eq!(sb.remaining, 0);
        assert_eq!(sb.exhaustions, 1);
        sb.refill();
        assert!(sb.spin_once());
    }

    #[test]
    fn f130_ring_wraparound() {
        let mut rq = LkRingQueue::new();
        for v in 0..LK_RING_CAP as u64 {
            assert!(rq.enqueue(v));
        }
        assert!(!rq.enqueue(99));
        assert_eq!(rq.dequeue(), Some(0));
        assert!(rq.enqueue(100));
        let mut expect = 1u64;
        while expect < LK_RING_CAP as u64 {
            assert_eq!(rq.dequeue(), Some(expect));
            expect += 1;
        }
        assert_eq!(rq.dequeue(), Some(100));
        assert!(rq.dequeue().is_none());
    }

    #[test]
    fn f134_semaphore_bounds() {
        let mut sem = LkSemaphore::new(0);
        assert!(!sem.down());
        assert!(sem.up());
        assert!(sem.down());
        assert_eq!(sem.count, 0);
        let mut full = LkSemaphore::new(LK_SEM_MAX);
        assert!(!full.up());
        assert_eq!(full.count, LK_SEM_MAX);
    }

    #[test]
    fn f136_oracle_self_loop() {
        let mut edges: [Option<usize>; LK_ORACLE_NODES] = [const { None }; LK_ORACLE_NODES];
        edges[4] = Some(4); // 自环：自己等自己
        assert!(wait_for_cycle(&edges, 4));
        assert!(any_deadlock(&edges));
        edges[4] = None;
        assert!(!any_deadlock(&edges));
    }

    #[test]
    fn f138_priority_inheritance_chain() {
        let mut chain = [
            LkPiNode { owner_prio: 20, waiter_prio: 0 },
            LkPiNode { owner_prio: 30, waiter_prio: 0 },
        ];
        // 链首已有更高优先级（20 < 25），传播值取沿途最小 = 20
        assert_eq!(chain_boost(&mut chain, 25), 20);
        assert_eq!(chain[0].owner_prio, 20);
        assert_eq!(chain[1].owner_prio, 20);
        // 已有更高优先级（数值更小）不被削弱
        let mut strong = [LkPiNode { owner_prio: 2, waiter_prio: 0 }];
        assert_eq!(chain_boost(&mut strong, 9), 2);
        assert_eq!(strong[0].owner_prio, 2);
    }

    #[test]
    fn f145_double_checked_locking() {
        let mut dcl = LkDcl::new();
        assert!(!dcl.dcl_init(false));
        assert_eq!(dcl.inits, 0);
        assert!(dcl.dcl_init(true));
        assert!(!dcl.dcl_init(true));
        assert_eq!(dcl.inits, 1);
    }

    #[test]
    fn f150_lock_selfcheck_all_pass() {
        let set = run_m700lock_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
        assert!(!set.truncated());
    }
}

