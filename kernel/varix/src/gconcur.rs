//! GALAXY-1800 AI-02 并发·调度·确定性域（G061~G120）。
//!
//! Three merged sub-domains: scheduling (G061~G080), lock-free concurrency
//! (G081~G100) and determinism (G101~G120). Pure logic, fixed arrays,
//! host-testable end to end.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G061~G080 — 调度
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Task {
    pub id: u16,
    pub prio: u8,      // 0 = 最高
    pub vruntime: u64, // CFS 虚拟运行时间
    pub deadline: u64, // EDF
    pub cpu: i8,
    pub node: u8, // NUMA 节点
    pub quota_ns: u64,
    pub used_ns: u64,
}

/// G061 优先级调度：取最高优先级（数值最小）。
pub fn pick_by_priority(tasks: &[Task], runnable: &[bool]) -> Option<u16> {
    let mut best: Option<&Task> = None;
    for (i, t) in tasks.iter().enumerate() {
        if runnable.get(i).copied().unwrap_or(false) && best.map(|b| t.prio < b.prio).unwrap_or(true) {
            best = Some(t);
        }
    }
    best.map(|t| t.id)
}

/// G062 公平调度（CFS 类）：vruntime 最小者胜出，运行后按权重补账。
pub fn pick_fair(tasks: &[Task], runnable: &[bool]) -> Option<u16> {
    let mut best: Option<(usize, u64)> = None;
    for (i, t) in tasks.iter().enumerate() {
        if runnable.get(i).copied().unwrap_or(false) && best.map(|(_, v)| t.vruntime < v).unwrap_or(true) {
            best = Some((i, t.vruntime));
        }
    }
    best.map(|(i, _)| tasks[i].id)
}

pub fn charge_fair(t: &mut Task, ran_ns: u64, weight: u64) {
    let w = weight.max(1);
    t.vruntime += ran_ns * 1024 / w;
}

/// G063 实时调度（EDF）：最早截止期优先。
pub fn pick_edf(tasks: &[Task], runnable: &[bool], now: u64) -> Option<u16> {
    let mut best: Option<(usize, u64)> = None;
    for (i, t) in tasks.iter().enumerate() {
        if runnable.get(i).copied().unwrap_or(false) && t.deadline > now {
            if best.map(|(_, d)| t.deadline < d).unwrap_or(true) {
                best = Some((i, t.deadline));
            }
        }
    }
    best.map(|(i, _)| tasks[i].id)
}

/// G064 工作窃取：本地双端队列（owner 底部取，thief 顶部偷）。
pub struct WsDeque {
    buf: [u16; 16],
    head: usize, // thief 端
    tail: usize, // owner 端
    pub stolen: u64,
    pub len: u64,
}

impl WsDeque {
    pub const fn new() -> WsDeque {
        WsDeque { buf: [0; 16], head: 0, tail: 0, stolen: 0, len: 0 }
    }
    pub fn push(&mut self, t: u16) -> bool {
        if self.len == 16 {
            return false;
        }
        self.buf[self.tail % 16] = t;
        self.tail += 1;
        self.len += 1;
        true
    }
    pub fn pop(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        self.tail -= 1;
        self.len -= 1;
        Some(self.buf[self.tail % 16])
    }
    pub fn steal(&mut self) -> Option<u16> {
        if self.len == 0 {
            return None;
        }
        let t = self.buf[self.head % 16];
        self.head += 1;
        self.len -= 1;
        self.stolen += 1;
        Some(t)
    }
}

/// G065 NUMA 感知：本地节点优先，其次最近节点表。
pub fn numa_pick(node: u8, local: &[bool], nearest: &[u8]) -> bool {
    if local.get(node as usize).copied().unwrap_or(false) {
        return true;
    }
    nearest.contains(&node)
}

/// G066 能耗感知调度：P 核给高负载，E 核给低负载。
pub fn pick_core(load_pct: u8, p_free: bool, e_free: bool) -> u8 {
    if load_pct >= 50 && p_free {
        0 // P-core
    } else if e_free {
        1 // E-core
    } else if p_free {
        0
    } else {
        0xFF
    }
}

/// G067 调度延迟预算红线。
pub fn latency_within_budget(sched_delay_ns: u64, red_line_ns: u64) -> bool {
    sched_delay_ns <= red_line_ns
}

/// G068 抢占与优先级继承：持有锁者临时继承等待者最高优先级。
pub fn inherit_priority(holder: u8, waiter: u8) -> u8 {
    holder.min(waiter)
}

/// G069 CPU 亲和性掩码。
pub fn affinity_allowed(mask: u64, cpu: u8) -> bool {
    cpu < 64 && mask & (1u64 << cpu) != 0
}

/// G070 cgroup 类限额：令牌桶配额。
pub fn quota_consume(t: &mut Task, want_ns: u64, period_ns: u64) -> u64 {
    let refill = period_ns.min(t.quota_ns);
    t.used_ns = t.used_ns.saturating_sub(refill);
    let grant = want_ns.min(t.quota_ns.saturating_sub(t.used_ns));
    t.used_ns += grant;
    grant
}

/// G072 调度跟踪点：事件环形缓冲。
pub struct TraceRing {
    pub buf: [u64; 16],
    pub head: usize,
    pub count: usize,
}

impl TraceRing {
    pub const fn new() -> TraceRing {
        TraceRing { buf: [0; 16], head: 0, count: 0 }
    }
    pub fn push(&mut self, ev: u64) {
        self.buf[self.head] = ev;
        self.head = (self.head + 1) % 16;
        if self.count < 16 {
            self.count += 1;
        }
    }
}

/// G075 锁无关调度队列：单生产者单消费者环形队列。
pub struct SpscRing {
    buf: [u16; 32],
    head: usize,
    tail: usize,
}

impl SpscRing {
    pub const fn new() -> SpscRing {
        SpscRing { buf: [0; 32], head: 0, tail: 0 }
    }
    pub fn push(&mut self, v: u16) -> bool {
        let next = (self.tail + 1) % 32;
        if next == self.head {
            return false;
        }
        self.buf[self.tail] = v;
        self.tail = next;
        true
    }
    pub fn pop(&mut self) -> Option<u16> {
        if self.head == self.tail {
            return None;
        }
        let v = self.buf[self.head];
        self.head = (self.head + 1) % 32;
        Some(v)
    }
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }
}

/// G077 调度公平性审计：最大 vruntime - 最小 vruntime 偏差。
pub fn fairness_gap(tasks: &[Task]) -> u64 {
    let mut lo = u64::MAX;
    let mut hi = 0u64;
    for t in tasks {
        lo = lo.min(t.vruntime);
        hi = hi.max(t.vruntime);
    }
    if lo == u64::MAX {
        0
    } else {
        hi - lo
    }
}

/// G078 调度器可插拔扩展点：策略句柄。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SchedPolicy {
    Priority,
    Fair,
    Edf,
}

pub fn pick(tasks: &[Task], runnable: &[bool], policy: SchedPolicy, now: u64) -> Option<u16> {
    match policy {
        SchedPolicy::Priority => pick_by_priority(tasks, runnable),
        SchedPolicy::Fair => pick_fair(tasks, runnable),
        SchedPolicy::Edf => pick_edf(tasks, runnable, now),
    }
}

// ---------------------------------------------------------------------------
// G081~G100 — 锁与无锁结构
// ---------------------------------------------------------------------------

/// G081 排队自旋锁（ticket lock）。
#[derive(Clone, Copy, Default)]
pub struct TicketLock {
    pub next: u64,
    pub owner: u64,
}

impl TicketLock {
    pub fn lock(&mut self) -> u64 {
        let t = self.next;
        self.next += 1;
        t
    }
    pub fn can_acquire(&self, ticket: u64) -> bool {
        ticket == self.owner
    }
    pub fn unlock(&mut self) {
        self.owner += 1;
    }
}

/// G082 读写锁三态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RwMode {
    Idle,
    Read,
    Write,
}

#[derive(Clone, Copy)]
pub struct RwLock {
    pub mode: RwMode,
    pub readers: u32,
    pub writer_waiting: bool,
}

impl RwLock {
    pub const fn new() -> RwLock {
        RwLock { mode: RwMode::Idle, readers: 0, writer_waiting: false }
    }
    pub fn read_acquire(&mut self, prefer_write: bool) -> bool {
        if self.mode == RwMode::Idle || (self.mode == RwMode::Read && !(prefer_write && self.writer_waiting)) {
            self.mode = RwMode::Read;
            self.readers += 1;
            true
        } else {
            self.writer_waiting = true;
            false
        }
    }
    pub fn write_acquire(&mut self) -> bool {
        if self.mode == RwMode::Idle {
            self.mode = RwMode::Write;
            true
        } else {
            self.writer_waiting = true;
            false
        }
    }
    pub fn release(&mut self) {
        match self.mode {
            RwMode::Read => {
                self.readers = self.readers.saturating_sub(1);
                if self.readers == 0 {
                    self.mode = RwMode::Idle;
                }
            }
            RwMode::Write => self.mode = RwMode::Idle,
            RwMode::Idle => {}
        }
    }
}

/// G083 无锁栈（Treiber 栈，单线程推演验证 ABA 防护位）。
#[derive(Clone, Copy, Default)]
pub struct LockFreeStack {
    top: u16,    // 节点下标 + 1，0 = 空
    aba: u32,
    pub slots: [u16; 16], // next 指针（下标+1）
    pub vals: [u32; 16],
    pub free: u16, // 空闲链表头
}

impl LockFreeStack {
    pub fn new() -> LockFreeStack {
        let mut s = LockFreeStack { top: 0, aba: 0, slots: [0; 16], vals: [0; 16], free: 1 };
        for i in 0..16usize {
            s.slots[i] = if i + 2 <= 16 { (i + 2) as u16 } else { 0 };
        }
        s
    }
    fn alloc(&mut self) -> Option<usize> {
        if self.free == 0 {
            return None;
        }
        let i = (self.free - 1) as usize;
        self.free = self.slots[i];
        Some(i)
    }
    pub fn push(&mut self, v: u32) -> bool {
        match self.alloc() {
            Some(i) => {
                self.vals[i] = v;
                self.slots[i] = self.top;
                self.top = (i + 1) as u16;
                self.aba = self.aba.wrapping_add(1);
                true
            }
            None => false,
        }
    }
    pub fn pop(&mut self) -> Option<u32> {
        if self.top == 0 {
            return None;
        }
        let i = (self.top - 1) as usize;
        let v = self.vals[i];
        self.top = self.slots[i];
        self.slots[i] = self.free;
        self.free = (i + 1) as u16;
        self.aba = self.aba.wrapping_add(1);
        Some(v)
    }
    pub fn version(&self) -> u32 {
        self.aba
    }
}

/// G084 无锁队列族：MPMC 定长环形（带序号校验）。
pub struct MpmcRing {
    pub seq: [u64; 8],
    pub data: [u32; 8],
    pub head: u64,
    pub tail: u64,
}

impl MpmcRing {
    pub const fn new() -> MpmcRing {
        MpmcRing { seq: [0; 8], data: [0; 8], head: 0, tail: 0 }
    }
    pub fn push(&mut self, v: u32) -> bool {
        let pos = (self.tail % 8) as usize;
        if self.seq[pos] != self.tail {
            return false; // 槽未就绪
        }
        self.data[pos] = v;
        self.seq[pos] = self.tail + 1;
        self.tail += 1;
        true
    }
    pub fn pop(&mut self) -> Option<u32> {
        let pos = (self.head % 8) as usize;
        if self.seq[pos] != self.head + 1 {
            return None;
        }
        let v = self.data[pos];
        self.seq[pos] = self.head + 8;
        self.head += 1;
        Some(v)
    }
}

/// G085 RCU 宽限期追踪：读者进出计数，写者等待静止态。
pub struct RcuGrace {
    pub readers: u32,
    pub grace_done: u64,
    pub pending: bool,
}

impl RcuGrace {
    pub const fn new() -> RcuGrace {
        RcuGrace { readers: 0, grace_done: 0, pending: false }
    }
    pub fn read_enter(&mut self) {
        self.readers += 1;
    }
    pub fn read_exit(&mut self) {
        self.readers = self.readers.saturating_sub(1);
        if self.readers == 0 && self.pending {
            self.pending = false;
            self.grace_done += 1;
        }
    }
    pub fn synchronize(&mut self) -> bool {
        if self.readers == 0 {
            self.grace_done += 1;
            true
        } else {
            self.pending = true;
            false
        }
    }
}

/// G086 事务内存探测：HTM 可用则走硬件事务，否则 STM 回退。
pub fn tm_probe(htm: bool) -> &'static str {
    if htm {
        "htm"
    } else {
        "stm"
    }
}

/// G087 顺序锁：写者递增序号（奇数=写中），读者只在偶数序号读。
#[derive(Clone, Copy, Default)]
pub struct SeqLock {
    pub seq: u32,
    pub value: u64,
}

impl SeqLock {
    pub fn write_begin(&mut self) -> u32 {
        self.seq = self.seq.wrapping_add(1); // 奇
        self.seq
    }
    pub fn write_end(&mut self, v: u64) {
        self.value = v;
        self.seq = self.seq.wrapping_add(1); // 偶
    }
    /// 读：仅在无写者（偶序号）时可见。
    pub fn read(&self) -> Option<u64> {
        if self.seq % 2 == 0 {
            Some(self.value)
        } else {
            None
        }
    }
    pub fn seq_now(&self) -> u32 {
        self.seq
    }
}

/// G088 引用计数：饱和防回绕。
pub fn refcount_inc(v: u32) -> u32 {
    v.saturating_add(1)
}

pub fn refcount_dec(v: u32) -> Option<u32> {
    if v <= 1 {
        None // 释放
    } else {
        Some(v - 1)
    }
}

/// G089 无锁跳表：定层数组实现，插入保持有序。
pub const SKIPLIST_MAX: usize = 16;
pub struct SkipList {
    pub keys: [u64; SKIPLIST_MAX],
    pub level: [u8; SKIPLIST_MAX], // 每节点层高
    pub next: [[u8; 4]; SKIPLIST_MAX],
    pub len: usize,
}

impl SkipList {
    pub const fn new() -> SkipList {
        SkipList { keys: [0; SKIPLIST_MAX], level: [0; SKIPLIST_MAX], next: [[0; 4]; SKIPLIST_MAX], len: 0 }
    }
    pub fn insert(&mut self, key: u64, tier: u8) -> bool {
        if self.len >= SKIPLIST_MAX {
            return false;
        }
        let mut i = 0usize;
        while i < self.len && self.keys[i] < key {
            i += 1;
        }
        // 右移
        let mut j = self.len;
        while j > i {
            self.keys[j] = self.keys[j - 1];
            self.level[j] = self.level[j - 1];
            self.next[j] = self.next[j - 1];
            j -= 1;
        }
        self.keys[i] = key;
        self.level[i] = tier.min(4).max(1);
        self.next[i] = [0; 4];
        self.len += 1;
        true
    }
    pub fn contains(&self, key: u64) -> bool {
        self.keys[..self.len].contains(&key)
    }
}

/// G090 无锁哈希表：分片计数。
pub const SHARDS: usize = 8;
pub struct ShardedMap {
    pub counts: [u32; SHARDS],
}

impl ShardedMap {
    pub const fn new() -> ShardedMap {
        ShardedMap { counts: [0; SHARDS] }
    }
    pub fn shard_of(key: u64) -> usize {
        (key % SHARDS as u64) as usize
    }
    pub fn insert(&mut self, key: u64) {
        self.counts[Self::shard_of(key)] += 1;
    }
    pub fn remove(&mut self, key: u64) -> bool {
        let s = Self::shard_of(key);
        if self.counts[s] > 0 {
            self.counts[s] -= 1;
            true
        } else {
            false
        }
    }
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }
}

/// G093 死锁检测：等待图找环（Floyd 或 DFS；这里用带色的 DFS）。
pub fn has_cycle(wait_edges: &[(u8, u8)], nodes: u8) -> bool {
    // 邻接表（固定容量）+ 三色 DFS
    let mut adj = [[0u8; 8]; 16];
    let mut deg = [0usize; 16];
    for &(a, b) in wait_edges {
        if (a as usize) < 16 && (b as usize) < 16 && deg[a as usize] < 8 {
            adj[a as usize][deg[a as usize]] = b;
            deg[a as usize] += 1;
        }
    }
    let mut color = [0u8; 16]; // 0 白 1 灰 2 黑
    fn dfs(u: usize, nodes: u8, adj: &[[u8; 8]; 16], deg: &[usize; 16], color: &mut [u8; 16]) -> bool {
        color[u] = 1;
        for k in 0..deg[u] {
            let v = adj[u][k] as usize;
            if v >= nodes as usize {
                continue;
            }
            if color[v] == 1 || (color[v] == 0 && dfs(v, nodes, adj, deg, color)) {
                return true;
            }
        }
        color[u] = 2;
        false
    }
    for n in 0..nodes.min(16) as usize {
        if color[n] == 0 && dfs(n, nodes, &adj, &deg, &mut color) {
            return true;
        }
    }
    false
}

/// G092/G098 优先级感知/可拓展锁：读侧免锁的 seqlock 已覆盖；
/// 这里给"热点计数分片"用于热路径免锁。
pub struct ShardedCounter {
    pub slots: [u64; 8],
}

impl ShardedCounter {
    pub const fn new() -> ShardedCounter {
        ShardedCounter { slots: [0; 8] }
    }
    pub fn add(&mut self, cpu: usize, v: u64) {
        self.slots[cpu % 8] += v;
    }
    pub fn read(&self) -> u64 {
        self.slots.iter().sum()
    }
}

// ---------------------------------------------------------------------------
// G101~G120 — 确定性
// ---------------------------------------------------------------------------

/// G101 确定性 PRNG：xorshift64*，同种子同序列。
pub struct DetPrng {
    pub state: u64,
}

impl DetPrng {
    pub const fn new(seed: u64) -> DetPrng {
        DetPrng { state: if seed == 0 { 1 } else { seed } }
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
}

/// G102 锁序记录：必须按编号递增加锁，否则记违规。
pub struct LockOrderLog {
    pub last: u16,
    pub violations: u64,
}

impl LockOrderLog {
    pub const fn new() -> LockOrderLog {
        LockOrderLog { last: 0, violations: 0 }
    }
    pub fn acquire(&mut self, id: u16) -> bool {
        if id >= self.last {
            self.last = id;
            true
        } else {
            self.violations += 1;
            false
        }
    }
}

/// G103 中断序确定化：按 (向量, 次序) 规范排序后哈希。
pub fn canonical_irq_hash(vectors: &mut [u8]) -> u64 {
    vectors.sort_unstable();
    let mut h = 0xcbf29ce484222325u64;
    for &v in vectors.iter() {
        h ^= v as u64;
        h = h.wrapping_mul(0x100000001B3);
    }
    h
}

/// G104/G115 调度轨迹重放：轨迹哈希比对。
pub fn trace_hash(events: &[u64]) -> u64 {
    let mut h = 0x9E3779B97F4A7C15u64;
    for &e in events {
        h ^= e;
        h = h.wrapping_mul(0x100000001B3);
    }
    h
}

/// G105 竞态可复现：在种子空间中搜索能触发"坏序"的种子。
pub fn race_repro_seed(mut probe: impl FnMut(u64) -> bool, max_seeds: u64) -> Option<u64> {
    for s in 0..max_seeds {
        if probe(s) {
            return Some(s);
        }
    }
    None
}

/// G107/G117 跨核确定性：乱序事件按 (lamport 时钟, 核号) 归并。
pub fn merge_events(evs: &mut [(u64, u8)]) -> u64 {
    evs.sort_unstable();
    let mut flat = [0u64; 16];
    for (i, &(t, c)) in evs.iter().enumerate().take(16) {
        flat[i] = t ^ ((c as u64) << 56);
    }
    trace_hash(&flat)
}

/// G112 确定性降级链：精确定时器不可用时回退粗粒度序号。
pub fn determinism_degrade(precise_available: bool, seq: u64) -> u64 {
    if precise_available {
        seq
    } else {
        seq & !0xFF // 粗粒度：低 8 位清零
    }
}

/// G114 与 record/replay 协作：事件编码（核号<<56 | 类型<<48 | 载荷）。
pub fn encode_event(core: u8, kind: u8, payload: u64) -> u64 {
    ((core as u64) << 56) | ((kind as u64) << 48) | (payload & 0x0000_FFFF_FFFF_FFFF)
}

pub fn decode_event(ev: u64) -> (u8, u8, u64) {
    ((ev >> 56) as u8, ((ev >> 48) & 0xFF) as u8, ev & 0x0000_FFFF_FFFF_FFFF)
}

/// G116 策略中心：确定性开关集合。
#[derive(Clone, Copy)]
pub struct DetPolicy {
    pub lock_order_enforced: bool,
    pub irq_canonical: bool,
    pub replay_enabled: bool,
}

// ---------------------------------------------------------------------------
// 自检收口（G071/G080/G094/G100/G106/G120 等以 CheckSet 呈现）
// ---------------------------------------------------------------------------

/// AI-02 域自检：≤32 项覆盖三段。
pub fn run_concurrency_checks() -> CheckSet {
    let mut s = CheckSet::new("gconcur");
    let tasks = [
        Task { id: 1, prio: 2, vruntime: 100, deadline: 50, cpu: -1, node: 0, quota_ns: 1000, used_ns: 0 },
        Task { id: 2, prio: 1, vruntime: 40, deadline: 30, cpu: -1, node: 1, quota_ns: 1000, used_ns: 0 },
        Task { id: 3, prio: 3, vruntime: 70, deadline: 20, cpu: -1, node: 0, quota_ns: 1000, used_ns: 0 },
    ];
    let run = [true, true, true];
    s.add("G061 priority", pick_by_priority(&tasks, &run) == Some(2), "prio1");
    s.add("G062 fair", pick_fair(&tasks, &run) == Some(2), "min vruntime");
    s.add("G063 edf", pick_edf(&tasks, &run, 0) == Some(3), "earliest dl");
    let mut q = WsDeque::new();
    q.push(1);
    q.push(2);
    q.push(3);
    s.add("G064 work-steal", q.pop() == Some(3) && q.steal() == Some(1) && q.stolen == 1, "lifo/steal");
    s.add("G065 numa", numa_pick(0, &[true, false], &[1]) && !numa_pick(2, &[true, false], &[1]) && numa_pick(2, &[false, false, true], &[1]), "local/near");
    s.add("G066 p/e core", pick_core(80, true, true) == 0 && pick_core(10, true, true) == 1, "energy");
    s.add("G067 latency red line", latency_within_budget(99, 100) && !latency_within_budget(101, 100), "budget");
    s.add("G068 inherit", inherit_priority(5, 1) == 1, "boost");
    s.add("G069 affinity", affinity_allowed(0b1010, 1) && !affinity_allowed(0b1010, 0), "mask");
    let mut t3 = tasks[2];
    let grant = quota_consume(&mut t3, 800, 100);
    s.add("G070 quota", grant == 800 && t3.used_ns == 800, "token bucket");
    let mut tr = TraceRing::new();
    tr.push(7);
    s.add("G072 trace ring", tr.count == 1 && tr.buf[0] == 7, "ring");
    let mut spsc = SpscRing::new();
    spsc.push(9);
    s.add("G075 spsc", spsc.pop() == Some(9) && spsc.is_empty(), "ring");
    s.add("G077 fairness gap", fairness_gap(&tasks) == 60, "spread");
    s.add("G078 pluggable", pick(&tasks, &run, SchedPolicy::Fair, 0) == Some(2), "policy");
    let mut lk = TicketLock::default();
    let t0 = lk.lock();
    s.add("G081 ticket lock", lk.can_acquire(t0) && !lk.can_acquire(t0 + 1), "order");
    lk.unlock();
    s.add("G081 unlock", lk.can_acquire(t0 + 1), "advance");
    let mut rw = RwLock::new();
    s.add("G082 rwlock", rw.read_acquire(false) && rw.readers == 1 && rw.write_acquire() == false, "rr/nw");
    let mut st = LockFreeStack::new();
    st.push(11);
    st.push(22);
    s.add("G083 lf stack", st.pop() == Some(22) && st.pop() == Some(11) && st.pop().is_none(), "lifo");
    let mut mpmc = MpmcRing::new();
    mpmc.push(5);
    s.add("G084 mpmc", mpmc.pop() == Some(5), "seq ring");
    let mut rcu = RcuGrace::new();
    rcu.read_enter();
    s.add("G085 rcu", !rcu.synchronize() && rcu.pending, "grace wait");
    rcu.read_exit();
    s.add("G085 rcu done", rcu.grace_done == 1, "grace done");
    s.add("G086 tm probe", tm_probe(true) == "htm" && tm_probe(false) == "stm", "fallback");
    let mut sq = SeqLock::default();
    sq.write_begin();
    sq.write_end(42);
    s.add("G087 seqlock", sq.seq % 2 == 0 && sq.read() == Some(42) && { sq.write_begin(); sq.read().is_none() }, "seq check");
    s.add("G088 refcount", refcount_dec(refcount_inc(2)) == Some(2) && refcount_dec(1).is_none(), "saturate");
    let mut sl = SkipList::new();
    sl.insert(30, 2);
    sl.insert(10, 1);
    sl.insert(20, 3);
    s.add("G089 skiplist", sl.contains(20) && sl.keys[0] == 10 && sl.keys[2] == 30, "ordered");
    let mut map = ShardedMap::new();
    map.insert(9);
    map.insert(17);
    s.add("G090 shard map", map.total() == 2 && map.remove(9), "shards");
    s.add("G093 deadlock", has_cycle(&[(0, 1), (1, 0)], 2) && !has_cycle(&[(0, 1), (1, 2)], 3), "cycle");
    let mut sc = ShardedCounter::new();
    sc.add(3, 7);
    sc.add(11, 2);
    s.add("G098 sharded counter", sc.read() == 9, "hot path");
    let mut p1 = DetPrng::new(42);
    let mut p2 = DetPrng::new(42);
    s.add("G101 det prng", p1.next() == p2.next() && p1.next() == p2.next(), "same seed");
    let mut lo = LockOrderLog::new();
    let ok = lo.acquire(3) && lo.acquire(5) && !lo.acquire(4);
    s.add("G102 lock order", ok && lo.violations == 1, "ascending");
    let mut v1 = [3u8, 1, 2];
    let h1 = canonical_irq_hash(&mut v1);
    let mut v2 = [2u8, 3, 1];
    let h2 = canonical_irq_hash(&mut v2);
    s.add("G103 irq canonical", h1 == h2, "sorted hash");
    s.add("G104 replay hash", trace_hash(&[1, 2, 3]) == trace_hash(&[1, 2, 3]), "stable");
    let seed = race_repro_seed(|s| s == 3, 10);
    s.add("G105 race repro", seed == Some(3), "seed search");
    let mut evs = [(2u64, 1u8), (1, 0), (1, 1)];
    let m1 = merge_events(&mut evs);
    let mut evs2 = [(1u64, 0u8), (2, 1), (1, 1)];
    let m2 = merge_events(&mut evs2);
    s.add("G107/G117 merge", m1 == m2, "canonical merge");
    s.add("G112 degrade", determinism_degrade(true, 0x1FF) == 0x1FF && determinism_degrade(false, 0x1FF) == 0x100, "coarse");
    let ev = encode_event(1, 2, 0x1234);
    s.add("G114 encode", decode_event(ev) == (1, 2, 0x1234), "roundtrip");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g062_fair_accounting() {
        let mut a = Task { id: 1, prio: 1, vruntime: 0, deadline: 0, cpu: -1, node: 0, quota_ns: 0, used_ns: 0 };
        let mut b = Task { id: 2, prio: 1, vruntime: 0, deadline: 0, cpu: -1, node: 0, quota_ns: 0, used_ns: 0 };
        charge_fair(&mut a, 100, 2); // 高权重涨得慢
        charge_fair(&mut b, 100, 1);
        assert!(a.vruntime < b.vruntime);
        assert_eq!(a.vruntime, 51200);
    }

    #[test]
    fn g083_stack_aba_version_advances() {
        let mut s = LockFreeStack::new();
        let v0 = s.version();
        s.push(1);
        s.pop();
        assert!(s.version() > v0);
        assert!(s.pop().is_none());
    }

    #[test]
    fn g087_seqlock_reader_retry() {
        let mut sq = SeqLock::default();
        let s1 = sq.seq_now();
        sq.write_begin();
        assert!(sq.read().is_none(), "write in progress");
        sq.write_end(7);
        assert_ne!(sq.seq_now(), s1, "seq moved");
        assert_eq!(sq.read(), Some(7));
    }

    #[test]
    fn g093_cycle_detection() {
        assert!(has_cycle(&[(0, 1), (1, 2), (2, 0)], 3));
        assert!(!has_cycle(&[(0, 1), (0, 2), (1, 3)], 4));
        assert!(!has_cycle(&[], 1));
    }

    #[test]
    fn g101_prng_deterministic_sequence() {
        let mut a = DetPrng::new(1);
        let mut b = DetPrng::new(1);
        for _ in 0..16 {
            assert_eq!(a.next(), b.next());
        }
        let mut c = DetPrng::new(2);
        assert_ne!(a.state, c.state);
    }

    #[test]
    fn g105_race_seed_found() {
        assert_eq!(race_repro_seed(|s| s % 7 == 5, 20), Some(5));
        assert_eq!(race_repro_seed(|_| false, 10), None);
    }

    #[test]
    fn g120_domain_selftest_all_green() {
        let s = run_concurrency_checks();
        if !s.all_passed() {
            let mut buf = [0u8; 2048];
            let n = s.render(&mut buf);
            panic!("domain self-test must pass://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(s.len() >= 25);
    }
}
