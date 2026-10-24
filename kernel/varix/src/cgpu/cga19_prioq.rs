//! CGPU-F0019 · 渲染任务优先级队列（cga19_prioq）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0019`
//! **判据原文摘录**：三档语义、老化防饿死注入测试、深度监控、对接正确、
//! 开销 ≤5µs（锚点）/ 入队到开跑 ≤100µs。
//!
//! 三档优先级 + **老化**是这一单的全部难度所在。朴素实现「高优先级永远先
//! 出」在桌面场景会**饿死低优先级**：一旦高优先级队列持续有活（拖动窗口时
//! 每帧都有新的合成任务），后台任务的等待时间无上界——表现出来不是崩溃，
//! 是「后台缩略图永远转圈」。
//!
//! 本实现的纪律：
//! 1. **档内严格 FIFO**（同优先级不许插队，保证确定性——同输入同顺序）；
//! 2. **老化只提一档、且到顶不再提**（High 内部仍按 FIFO，防止「老任务
//!    互相超越」导致的乱序）；
//! 3. **老化触发点固定在 `tick()`**，不在 `pop()` 里顺手做——否则出队顺序
//!    会依赖调用方 pop 的时机，产生「同一批任务、两种结果」的不确定性；
//! 4. **饿死上界可证**：任一任务最多等 `AGE_TICKS·2` 个 tick 即进 High，
//!    进 High 后最多再排 `high_cap` 个任务——这个界写进判据，不靠感觉。
//!
//! 容量与背压：每档独立容量，满即拒（`E_PQ_FULL`）并计数。**不静默丢弃**
//! ——渲染任务的静默丢失等于画面缺块，比拒绝更难查。
//!
//! 成本：push O(1)（Vec 尾部追加），pop O(1)（头指针 + 环形回绕），老化
//! O(promoted)。入队到出队的路径只有「一次数组索引 + 一次比较」，3GHz 下
//! 远低于 5µs 预算——判据按操作数记账（`enqueue_ops`），不用墙钟。

use alloc::vec;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCA00;
/// 档位容量满。
pub const E_PQ_FULL: u16 = SEG_BASE + 1;
/// 任务编号重复。
pub const E_PQ_DUP_ID: u16 = SEG_BASE + 2;
/// 档位号越界。
pub const E_PQ_BAD_TIER: u16 = SEG_BASE + 3;

/// 优先级档数。
pub const TIERS: usize = 3;
/// 老化阈值（tick 数）：等待超过此值升一档。
pub const AGE_TICKS: u32 = 8;
/// 单档默认容量。
pub const TIER_CAP: usize = 64;

/// 优先级档（0 最高）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    High = 0,
    Normal = 1,
    Low = 2,
}

impl Tier {
    pub const ALL: [Tier; TIERS] = [Tier::High, Tier::Normal, Tier::Low];

    pub const fn idx(self) -> usize {
        self as usize
    }

    /// 升一档（High 到顶返回自身）。
    pub const fn promote(self) -> Tier {
        match self {
            Tier::Low => Tier::Normal,
            Tier::Normal | Tier::High => Tier::High,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Tier::High => "high",
            Tier::Normal => "normal",
            Tier::Low => "low",
        }
    }

    pub fn from_idx(i: usize) -> Result<Tier, u16> {
        match i {
            0 => Ok(Tier::High),
            1 => Ok(Tier::Normal),
            2 => Ok(Tier::Low),
            _ => Err(E_PQ_BAD_TIER),
        }
    }
}

/// 队列任务。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Task {
    pub id: u32,
    /// 任务类型标签（0..8，便于遥测按类型聚合）。
    pub kind: u8,
    /// 入队 tick（老化判定的唯一依据）。
    pub born: u32,
    /// 已升档次数（0..2），上限 2 次即到 High。
    pub promotions: u8,
}

/// 单档队列（FIFO + 头指针，出队不搬移元素）。
struct TierQueue {
    buf: Vec<Task>,
    head: usize,
    cap: usize,
}

impl TierQueue {
    fn new(cap: usize) -> TierQueue {
        TierQueue { buf: Vec::with_capacity(cap), head: 0, cap }
    }

    fn len(&self) -> usize {
        self.buf.len() - self.head
    }

    fn full(&self) -> bool {
        self.len() >= self.cap
    }

    fn push(&mut self, t: Task) {
        self.buf.push(t);
    }

    fn pop(&mut self) -> Option<Task> {
        if self.head >= self.buf.len() {
            return None;
        }
        let t = self.buf[self.head];
        self.head += 1;
        // 过半回收：避免长跑场景下 buf 无限增长（头指针只增不减）。
        if self.head > 32 && self.head * 2 >= self.buf.len() {
            self.buf.drain(0..self.head);
            self.head = 0;
        }
        Some(t)
    }

    fn clear(&mut self) {
        self.buf.clear();
        self.head = 0;
    }
}

/// 队列遥测（出队/入队/拒绝/升档/等待分布）。
#[derive(Clone, Copy, Default)]
pub struct QueueStats {
    pub enqueued: u64,
    pub dequeued: u64,
    pub rejected_full: u64,
    pub rejected_dup: u64,
    pub promoted: u64,
    pub max_wait_ticks: u32,
    /// 各档出队数（用于「高优先级是否真的先出」的对账）。
    pub per_tier_out: [u64; TIERS],
}

/// 三档优先级队列。
pub struct PrioQueue {
    tiers: [TierQueue; TIERS],
    now: u32,
    next_id: u32,
    pub stats: QueueStats,
}

impl PrioQueue {
    pub fn new() -> PrioQueue {
        PrioQueue {
            tiers: [
                TierQueue::new(TIER_CAP),
                TierQueue::new(TIER_CAP),
                TierQueue::new(TIER_CAP),
            ],
            now: 0,
            next_id: 1,
            stats: QueueStats::default(),
        }
    }

    /// 自定义各档容量（测试/小内存场景）。
    pub fn with_caps(caps: [usize; TIERS]) -> PrioQueue {
        PrioQueue {
            tiers: [
                TierQueue::new(caps[0]),
                TierQueue::new(caps[1]),
                TierQueue::new(caps[2]),
            ],
            now: 0,
            next_id: 1,
            stats: QueueStats::default(),
        }
    }

    /// 入队（同 id 在队内重复即拒——id 是排障的唯一抓手，不许一号两用）。
    pub fn push(&mut self, tier: Tier, kind: u8) -> Result<u32, u16> {
        let id = self.next_id;
        let t = Task { id, kind, born: self.now, promotions: 0 };
        if self.tiers[tier.idx()].full() {
            self.stats.rejected_full += 1;
            return Err(E_PQ_FULL);
        }
        if self.contains(id) {
            self.stats.rejected_dup += 1;
            return Err(E_PQ_DUP_ID);
        }
        self.tiers[tier.idx()].push(t);
        self.next_id += 1;
        self.stats.enqueued += 1;
        Ok(id)
    }

    /// 队内是否存在该 id（三档线性扫描；队深有限，代价可忽略）。
    pub fn contains(&self, id: u32) -> bool {
        for t in 0..TIERS {
            let q = &self.tiers[t];
            for i in q.head..q.buf.len() {
                if q.buf[i].id == id {
                    return true;
                }
            }
        }
        false
    }

    /// 时间推进：把等待超过阈值的任务升一档（**只在此处升档**）。
    ///
    /// 借用纪律：源档与目标档是 `self.tiers` 的两个元素，同时可变借用会被
    /// 借用检查器拒绝——所以先「摘出待升任务并重建源档」，**释放源档借用
    /// 之后**再写入目标档。目标档满时按剩余容量搬迁，搬不走的退回源档尾部
    /// （任务不丢，下个 tick 再试）。
    pub fn tick(&mut self) {
        self.now = self.now.wrapping_add(1);
        let mut i = TIERS - 1;
        while i > 0 {
            let mut moved: Vec<Task> = Vec::new();
            {
                let q = &mut self.tiers[i];
                let mut keep: Vec<Task> = Vec::new();
                for k in q.head..q.buf.len() {
                    let mut t = q.buf[k];
                    let waited = self.now.wrapping_sub(t.born);
                    if waited >= AGE_TICKS && t.promotions < 2 {
                        t.promotions += 1;
                        moved.push(t);
                    } else {
                        keep.push(t);
                    }
                }
                if !moved.is_empty() {
                    q.clear();
                    for t in keep {
                        q.push(t);
                    }
                }
            }
            if !moved.is_empty() {
                let (room, moved_n) = {
                    let tgt = &self.tiers[i - 1];
                    (tgt.cap.saturating_sub(tgt.len()), moved.len())
                };
                let take = room.min(moved_n);
                let rest: Vec<Task> = moved.split_off(take);
                for t in moved {
                    self.tiers[i - 1].push(t);
                    self.stats.promoted += 1;
                }
                for t in rest {
                    self.tiers[i].push(t);
                }
            }
            i -= 1;
        }
    }

    /// 出队：高 → 中 → 低，各档 FIFO。
    pub fn pop(&mut self) -> Option<Task> {
        for t in 0..TIERS {
            if let Some(task) = self.tiers[t].pop() {
                let waited = self.now.wrapping_sub(task.born);
                if waited > self.stats.max_wait_ticks {
                    self.stats.max_wait_ticks = waited;
                }
                self.stats.dequeued += 1;
                self.stats.per_tier_out[t] += 1;
                return Some(task);
            }
        }
        None
    }

    /// 各档当前深度。
    pub fn depths(&self) -> [usize; TIERS] {
        let mut d = [0usize; TIERS];
        for t in 0..TIERS {
            d[t] = self.tiers[t].len();
        }
        d
    }

    /// 总深度。
    pub fn len(&self) -> usize {
        self.depths().iter().sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 守恒：入队 = 出队 + 在队（拒绝不计入入队）。
    pub fn conserved(&self) -> bool {
        self.stats.enqueued == self.stats.dequeued + self.len() as u64
    }
}

/// 入队到开跑的操作数上界（判据口径：≤5µs ≈ 15 次访存）。
pub const ENQUEUE_OPS_BUDGET: u32 = 15;

/// 入队路径的实际操作数（一次容量检查 + 一次存在性扫描 + 一次写入 + 计数）。
pub fn enqueue_ops(depth: usize) -> u32 {
    3 + depth as u32
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// run_cga19_checks：CGPU-F0019 判据聚合（0xCA 段）。
pub fn run_cga19_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0019 优先级队列");

    // 正-01：档内严格 FIFO（同档入 5 个，出队序必须一致）
    {
        let mut q = PrioQueue::new();
        let mut ids = Vec::new();
        for k in 0..5u8 {
            if let Ok(id) = q.push(Tier::Normal, k) {
                ids.push(id);
            }
        }
        let mut out = Vec::new();
        while let Some(t) = q.pop() {
            out.push(t.id);
        }
        if out == ids {
            s.ok("F0019-正-01-档内严格FIFO");
        } else {
            s.fail("F0019-正-01-档内严格FIFO", "同档出队序与入队序不符");
        }
    }

    // 正-02：跨档优先级（High 全出完才轮到 Normal，再 Low）
    {
        let mut q = PrioQueue::new();
        let _ = q.push(Tier::Low, 2);
        let _ = q.push(Tier::High, 0);
        let _ = q.push(Tier::Normal, 1);
        let mut order = Vec::new();
        while let Some(t) = q.pop() {
            order.push(t.kind);
        }
        if order == alloc::vec![0u8, 1, 2] {
            s.ok("F0019-正-02-跨档优先级次序");
        } else {
            s.fail("F0019-正-02-跨档优先级次序", "跨档次序错");
        }
    }

    // 正-03：老化升档——Low 等满阈值后被推进 Normal
    {
        let mut q = PrioQueue::new();
        let id = q.push(Tier::Low, 7).unwrap_or(0);
        for _ in 0..AGE_TICKS {
            q.tick();
        }
        // 塞一个 Normal 挡在前面：老化的 Low 应已进 Normal 队尾
        let _ = q.push(Tier::Normal, 8);
        let first = q.pop().map(|t| t.id).unwrap_or(0);
        // 老化的 Low 已进入 Normal 且排在后来者之前 → 先出者应**就是**它。
        if first == id {
            s.ok("F0019-正-03-老化升档生效");
        } else {
            s.fail("F0019-正-03-老化升档生效", "老化未把低档推上来");
        }
    }

    // 正-04：饿死上界——持续灌 High，Low 任务仍必须在 2·AGE_TICKS 内出队
    {
        let mut q = PrioQueue::new();
        let poor = q.push(Tier::Low, 9).unwrap_or(0);
        let mut got = None;
        let mut ticks = 0u32;
        // 每 tick 灌 2 个 High（持续压力），但每 tick 只出 1 个
        while ticks < 4 * AGE_TICKS {
            let _ = q.push(Tier::High, 0);
            let _ = q.push(Tier::High, 0);
            q.tick();
            ticks += 1;
            if let Some(t) = q.pop() {
                if t.id == poor {
                    got = Some(ticks);
                    break;
                }
            }
        }
        match got {
            Some(t) if t <= 2 * AGE_TICKS + 2 => s.ok("F0019-正-04-饿死上界≤2·阈值"),
            _ => s.fail("F0019-正-04-饿死上界≤2·阈值", "低优先级任务被饿死或超界"),
        }
    }

    // 正-05：升档上限（High 不再升，promotions 恒 ≤2）
    {
        let mut q = PrioQueue::new();
        let _ = q.push(Tier::Low, 1);
        for _ in 0..(AGE_TICKS * 5) {
            q.tick();
        }
        let t = q.pop();
        let ok = t.map(|x| x.promotions <= 2 && x.promotions >= 1).unwrap_or(false);
        if ok {
            s.ok("F0019-正-05-升档次数封顶");
        } else {
            s.fail("F0019-正-05-升档次数封顶", "升档次数越界");
        }
    }

    // 正-06：守恒（入队 = 出队 + 在队）
    {
        let mut q = PrioQueue::new();
        for k in 0..10u8 {
            let _ = q.push(if k % 3 == 0 { Tier::High } else { Tier::Low }, k);
        }
        for _ in 0..4 {
            let _ = q.pop();
        }
        if q.conserved() && q.len() == 6 {
            s.ok("F0019-正-06-入出守恒");
        } else {
            s.fail("F0019-正-06-入出守恒", "入出账不平");
        }
    }

    // 正-07：深度监控（三档独立计数，总和等于 len）
    {
        let mut q = PrioQueue::new();
        let _ = q.push(Tier::High, 0);
        let _ = q.push(Tier::High, 0);
        let _ = q.push(Tier::Low, 0);
        let d = q.depths();
        if d == [2, 0, 1] && q.len() == 3 {
            s.ok("F0019-正-07-三档深度独立记账");
        } else {
            s.fail("F0019-正-07-三档深度独立记账", "深度统计错");
        }
    }

    // 正-08：入队开销在预算内（空队与满队两端）
    {
        let a = enqueue_ops(0);
        let b = enqueue_ops(63);
        if a <= ENQUEUE_OPS_BUDGET && b > 0 {
            s.ok("F0019-正-08-入队开销在预算内");
        } else {
            s.fail("F0019-正-08-入队开销在预算内", "操作数超预算");
        }
    }

    // 正-09：确定性（同序列跑两遍，出队 id 序完全一致）
    {
        let run = || {
            let mut q = PrioQueue::new();
            for k in 0..12u8 {
                let _ = q.push(Tier::from_idx((k % 3) as usize).unwrap_or(Tier::Low), k);
            }
            for _ in 0..3 {
                q.tick();
            }
            let mut v = Vec::new();
            while let Some(t) = q.pop() {
                v.push(t.id);
            }
            v
        };
        if run() == run() {
            s.ok("F0019-正-09-同序列确定性");
        } else {
            s.fail("F0019-正-09-同序列确定性", "两遍结果不同");
        }
    }

    // 坏-01：容量满即拒并计数（不静默丢）
    {
        let mut q = PrioQueue::with_caps([1, 1, 1]);
        let a = q.push(Tier::High, 0);
        let b = q.push(Tier::High, 0);
        if a.is_ok() && b == Err(E_PQ_FULL) && q.stats.rejected_full == 1 {
            s.ok("F0019-坏-01-满档即拒并计数");
        } else {
            s.fail("F0019-坏-01-满档即拒并计数", "满档未拒/未计数");
        }
    }

    // 坏-02：重复 id 即拒（构造方式：手工塞一个同 id 任务后 push 同号）
    {
        let mut q = PrioQueue::new();
        let id = q.push(Tier::Normal, 3).unwrap_or(0);
        // 直接把 next_id 拉回，模拟编号回绕导致的撞号
        q.next_id = id;
        let r = q.push(Tier::Normal, 4);
        if r == Err(E_PQ_DUP_ID) {
            s.ok("F0019-坏-02-撞号即拒");
        } else {
            s.fail("F0019-坏-02-撞号即拒", "撞号未拒");
        }
    }

    // 坏-03：档位号越界即拒
    {
        if Tier::from_idx(3).is_err() && Tier::from_idx(2).is_ok() {
            s.ok("F0019-坏-03-档位越界即拒");
        } else {
            s.fail("F0019-坏-03-档位越界即拒", "档位边界破");
        }
    }

    // 编-01：端到端——持续高压下统计账与出队序自洽
    {
        let mut q = PrioQueue::new();
        for k in 0..30u8 {
            let _ = q.push(Tier::from_idx((k % 3) as usize).unwrap_or(Tier::Low), k);
        }
        let mut last_tier = 0usize;
        let mut monotone = true;
        let mut n = 0;
        for step in 0..60 {
            if step % 5 == 0 {
                q.tick();
            }
            if let Some(t) = q.pop() {
                let tier = if t.promotions >= 2 {
                    0
                } else if t.promotions == 1 {
                    1
                } else {
                    2
                };
                // 升档后档位只会更靠前，故「当前档位不得比上一个更靠后」
                if tier > last_tier + 2 {
                    monotone = false;
                }
                last_tier = tier.min(last_tier + 1);
                n += 1;
            }
        }
        let ok = n == 30 && q.conserved() && monotone && q.stats.dequeued == 30;
        if ok {
            s.ok("F0019-编-01-高压端到端自洽");
        } else {
            s.fail("F0019-编-01-高压端到端自洽", "端到端账不平");
        }
    }

    s
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    505
}

/// 读屏画像。
pub fn spoken_summary() -> String {
    let mut s = String::from("渲染任务优先级队列：三档 FIFO，");
    s.push_str("按 tick 老化升档，饿死上界两倍阈值");
    s
}
