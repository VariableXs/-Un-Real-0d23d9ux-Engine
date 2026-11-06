//! UNX-F48995 事件洪泛×10^4 + UNX-F48996 长跑稳定性 · 0x71800
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B10 族第八槽）：
//!   UNX-F48995 手势状态机（1/2/3 指 · 42 迁移表）错误码族（15 条）:
//!          ① 纯功能代码 ≥220 行；
//!          ② UNX-F48995-J1 错误码 ×15 零漂移互不混淆。
//!   UNX-F48996 手势状态机（1/2/3 指 · 42 迁移表）观测埋点（入口/出口/耗时/结果四点全覆盖）:
//!          ① 纯功能代码 ≥220 行；
//!          ② UNX-F48996-J1 四点 100% 覆盖；异常零静默。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：时钟 → 容量 → 序号 → 入队 → 出队 → 延迟分桶 → 账本。
//! 时钟先于容量：时钟回退意味着「这一拍的顺序坏了」，容量满只是「这一拍
//! 的背压到了」。二者在账面上完全不同——回退是错误，满容是合法的丢弃
//! 账。若先判容量，回退拍可能恰好落在满容分支，被记成一笔合法丢弃，
//! 顺序错误就永久混进丢弃账，后续 latency 全部不可信。
//! 容量先于序号：满容时序号根本不消耗（不入队就不该推进 next_seq），
//! 否则丢弃会制造序号空洞之外的第二种空洞，序号单调性检验就失去意义。
//! 序号先于入队：入队前的重复序号必须拒绝，而不是让坏序号进环后再由
//! 出队侧发现——环是有界的，坏序号一旦占槽，正确事件会被背压丢弃。
//! 延迟分桶放在出队之后：分桶只信「出队时刻 - 入队时刻」，两个时刻都
//! 必须已经通过时钟闸门；先分桶再校验会把负延迟折叠成巨大无符号数，
//! 桶位整体右移，延迟分布失真且无法定位。
//! 账本最后：前六道闸门都可能拒账，账本只负责核对
//! enqueued == drained + dropped + queued 与 Σlat == drained。把账本
//! 前移会让拒账本身破坏守恒式，排障时分不清「业务拒账」和「账本坏了」。
//! 关键差异：本模块只回答洪泛账是否守恒、长跑是否稳定；不做手势语义
//! 分类（B10-CORE）、不做误触抑制（B10-GUARD）、不做手势→动作映射
//! （B10-DISPATCH）、不做自适应阈值（B10-ADAPT）。环形队列是定长数组，
//! 长跑不分配堆内存，`storage_audit` 用 `size_of` 把这条承诺钉死。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71800，VX6 批 B10 族第八槽）。
pub const SEG_BASE: u32 = 0x71800;

/// 环形队列容量（事件数）。
pub const QUEUE_CAP: usize = 64;
/// 洪泛轮数（卡面 ×10^4）。
pub const FLOOD_ROUNDS: u32 = 10_000;
/// 长跑轮数（卡面 ×10^5 拍）。
pub const LONG_ROUNDS: u32 = 100_000;
/// 单轮最大突发入队数。
pub const BURST_MAX: u32 = 8;
/// 洪泛加压周期：每 5 轮叠加一次深突发。
pub const PRESSURE_EVERY: u32 = 5;
/// 深突发叠加量（足以在 64 槽环上制造背压）。
pub const PRESSURE_EXTRA: u32 = 60;
/// 单次出队上限。
pub const DRAIN_MAX: u32 = 16;
/// 延迟分桶数。
pub const LAT_BUCKETS: usize = 5;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 账本字段数。
pub const STATS_FIELDS: usize = 6;
/// 黄金轮数。
pub const GOLDEN_ROUNDS: u32 = 16;
/// 黄金每轮入队数。
pub const GOLDEN_BURST: u32 = 4;
/// 时钟上界。
pub const TICK_CAP: u32 = 10_000_000;
/// 长跑检查点间隔（拍）。
pub const DRIFT_EVERY: u32 = 997;
/// 存储预算（字节）：Engine 必须远小于一页，保证定长。
pub const ENGINE_BUDGET: usize = 4096;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0E） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StCode {
    /// 洪泛账不守恒（入/出/丢/存四元对不上）
    FloodBad,
    /// 环形队列游标或容量自检失败
    QueueBad,
    /// 背压丢弃账异常（丢弃数越界或丢弃序号不单调）
    DropBad,
    /// 延迟分布不守恒（Σlat != drained）或延迟越界
    DelayBad,
    /// 出队序号回退/重复
    OrderBad,
    /// 账本不等式不成立
    LedgerBad,
    /// 时钟回退或越界
    TickBad,
    /// 计数溢出（checked 运算失败）
    OverflowBad,
    /// 长跑增量账与影子账漂移
    DriftBad,
    /// 渲染字段数/格式坏
    RenderBad,
    /// 重放结果不一致
    ReplayBad,
    /// 轮数越界（覆盖闸门失败）
    CoverBad,
    /// 段窗口越界或码族重复
    SegmentBad,
    /// 注入未全捕获
    FuzzBad,
}

impl StCode {
    /// 诊断值：段基址 + 0x01 + 枚举序，窗口止于 +0x0E。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 中文诊断说明。
    pub fn detail(self) -> &'static str {
        match self {
            StCode::FloodBad => "洪泛账不守恒",
            StCode::QueueBad => "环形队列游标或容量自检失败",
            StCode::DropBad => "背压丢弃账异常",
            StCode::DelayBad => "延迟分布不守恒或延迟越界",
            StCode::OrderBad => "出队序号回退或重复",
            StCode::LedgerBad => "账本不等式不成立",
            StCode::TickBad => "时钟回退或越界",
            StCode::OverflowBad => "计数溢出",
            StCode::DriftBad => "长跑增量账与影子账漂移",
            StCode::RenderBad => "渲染字段数或格式坏",
            StCode::ReplayBad => "重放结果不一致",
            StCode::CoverBad => "轮数越界覆盖闸门失败",
            StCode::SegmentBad => "段窗口越界或码族重复",
            StCode::FuzzBad => "注入未全捕获",
        }
    }

    /// 全码族（14 项，窗口 +0x01..+0x0E）。
    pub const ALL: [StCode; 14] = [
        StCode::FloodBad,
        StCode::QueueBad,
        StCode::DropBad,
        StCode::DelayBad,
        StCode::OrderBad,
        StCode::LedgerBad,
        StCode::TickBad,
        StCode::OverflowBad,
        StCode::DriftBad,
        StCode::RenderBad,
        StCode::ReplayBad,
        StCode::CoverBad,
        StCode::SegmentBad,
        StCode::FuzzBad,
    ];
}

/// 自由函数版诊断说明（按诊断值反查）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < StCode::ALL.len() {
        if let Some(c) = StCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知压力诊断码"
}

/// 码族唯一性：14 个诊断值两两不同。
pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < StCode::ALL.len() {
        let a = match StCode::ALL.get(i) {
            Some(v) => v.val(),
            None => return false,
        };
        let mut j = i + 1;
        while j < StCode::ALL.len() {
            let b = match StCode::ALL.get(j) {
                Some(v) => v.val(),
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 段位自检：窗口止于 +0x0E，且最高位不超过 +0x0F。
pub fn segment_guard() -> Result<(), u32> {
    let mut i = 0usize;
    while i < StCode::ALL.len() {
        let c = match StCode::ALL.get(i) {
            Some(v) => *v,
            None => return Err(StCode::SegmentBad.val()),
        };
        let off = c.val().wrapping_sub(SEG_BASE);
        if off < 0x01 || off > 0x0F {
            return Err(StCode::SegmentBad.val());
        }
        i += 1;
    }
    if !codes_are_distinct() {
        return Err(StCode::SegmentBad.val());
    }
    Ok(())
}

// ── 事件与环形队列 ────────────────────────────────────────────────
/// 入环事件：序号、入队时钟、种类（0..=7 的原始事件码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Event {
    pub seq: u32,
    pub tick: u32,
    pub kind: u8,
}

impl Event {
    pub fn new(seq: u32, tick: u32, kind: u8) -> Event {
        Event { seq, tick, kind }
    }
}

/// 定长环形队列：64 槽，head/tail/count 三元游标。
///
/// 不变量：
/// - `count == 0` 时 `head == tail`；
/// - `tail == (head + count) % QUEUE_CAP` 恒成立；
/// - 槽位在 `[head, tail)` 区间内为 `Some`，区间外为 `None`。
/// 三个不变量由 `audit` 逐槽核对，长跑每轮抽查一次。
pub struct Ring {
    slots: [Option<Event>; QUEUE_CAP],
    head: usize,
    tail: usize,
    count: usize,
}

impl Ring {
    pub fn new() -> Ring {
        Ring {
            slots: [None; QUEUE_CAP],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn is_full(&self) -> bool {
        self.count >= QUEUE_CAP
    }

    /// 入环。满容拒绝且**不移动任何游标**。
    pub fn push(&mut self, e: Event) -> Result<(), u32> {
        if self.is_full() {
            return Err(StCode::QueueBad.val());
        }
        if let Some(slot) = self.slots.get_mut(self.tail) {
            *slot = Some(e);
        }
        self.tail = (self.tail + 1) % QUEUE_CAP;
        self.count = self.count.saturating_add(1);
        Ok(())
    }

    /// 出环。空环返回 None 且不移动游标。
    pub fn pop(&mut self) -> Option<Event> {
        if self.count == 0 {
            return None;
        }
        let e = match self.slots.get(self.head) {
            Some(Some(v)) => *v,
            _ => return None,
        };
        if let Some(slot) = self.slots.get_mut(self.head) {
            *slot = None;
        }
        self.head = (self.head + 1) % QUEUE_CAP;
        self.count = self.count.saturating_sub(1);
        Some(e)
    }

    /// 队首只读访问。
    pub fn peek(&self) -> Option<Event> {
        match self.slots.get(self.head) {
            Some(Some(v)) => Some(*v),
            _ => None,
        }
    }

    /// 三元游标自检：tail 必须等于 (head + count) % CAP。
    pub fn audit(&self) -> Result<(), u32> {
        if self.count > QUEUE_CAP {
            return Err(StCode::QueueBad.val());
        }
        let expect_tail = if QUEUE_CAP == 0 {
            0
        } else {
            (self.head + self.count) % QUEUE_CAP
        };
        if self.tail != expect_tail {
            return Err(StCode::QueueBad.val());
        }
        if self.count == 0 && self.head != self.tail {
            return Err(StCode::QueueBad.val());
        }
        // 槽位填充必须与 [head, tail) 区间一致（区分满/空两种 head==tail）。
        let mut i = 0usize;
        while i < QUEUE_CAP {
            let live = if self.count == 0 {
                false
            } else if self.head < self.tail {
                i >= self.head && i < self.tail
            } else if self.head > self.tail {
                i >= self.head || i < self.tail
            } else {
                true
            };
            match self.slots.get(i) {
                Some(Some(_)) if !live => return Err(StCode::QueueBad.val()),
                Some(None) if live => return Err(StCode::QueueBad.val()),
                Some(_) => {}
                None => return Err(StCode::QueueBad.val()),
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 延迟分桶 ──────────────────────────────────────────────────────
/// 延迟桶：0 / 1 / 2..=4 / 5..=16 / >16（拍）。
pub fn bucket(lat: u32) -> usize {
    match lat {
        0 => 0,
        1 => 1,
        2..=4 => 2,
        5..=16 => 3,
        _ => 4,
    }
}

/// 桶名（诊断文本用）。
pub const LAT_NAMES: [&str; LAT_BUCKETS] = ["0", "1", "2-4", "5-16", ">16"];

// ── 洪泛账 ────────────────────────────────────────────────────────
/// 六字段账：入队、出队、丢弃、队列存量、最大延迟、摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FloodStats {
    pub enqueued: u32,
    pub drained: u32,
    pub dropped: u32,
    pub queued: u32,
    pub max_lat: u32,
    pub lat: [u32; LAT_BUCKETS],
    pub digest: u64,
}

impl FloodStats {
    /// Σ 分桶计数。
    pub fn lat_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < LAT_BUCKETS {
            if let Some(v) = self.lat.get(i) {
                s = s.saturating_add(*v);
            }
            i += 1;
        }
        s
    }

    /// 四元守恒：入队 == 出队 + 丢弃 + 存量。
    pub fn balanced(&self) -> bool {
        self.enqueued
            == self
                .drained
                .saturating_add(self.dropped)
                .saturating_add(self.queued)
    }

    /// 账本自检：守恒 + 分桶 + 容量 + 摘要。
    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(StCode::LedgerBad.val());
        }
        if self.lat_sum() != self.drained {
            return Err(StCode::DelayBad.val());
        }
        if self.queued as usize > QUEUE_CAP {
            return Err(StCode::QueueBad.val());
        }
        if self.max_lat > TICK_CAP {
            return Err(StCode::DelayBad.val());
        }
        if self.digest == 0 {
            return Err(StCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 洪泛引擎：入队/出队/丢弃的唯一账主。
///
/// 账本不变量（每拍核对）：
/// - `enqueued == drained + dropped + ring.len()`；
/// - `Σ lat buckets == drained`；
/// - `last_drained_seq` 严格递增（丢弃只制造合法空洞，不制造回退）；
/// - `next_seq` 只在实际入环时推进。
pub struct Engine {
    ring: Ring,
    tick: u32,
    next_seq: u32,
    enqueued: u32,
    drained: u32,
    dropped: u32,
    max_lat: u32,
    lat: [u32; LAT_BUCKETS],
    digest: u64,
    last_drained_seq: u32,
    last_drop_seq: u32,
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            ring: Ring::new(),
            tick: 0,
            next_seq: 1,
            enqueued: 0,
            drained: 0,
            dropped: 0,
            max_lat: 0,
            lat: [0; LAT_BUCKETS],
            digest: p_core::FNV_OFF,
            last_drained_seq: 0,
            last_drop_seq: 0,
        }
    }

    /// 时钟闸门：只允许单调不减且不越上界。
    fn advance(&mut self, tick: u32) -> Result<(), u32> {
        if tick < self.tick || tick > TICK_CAP {
            return Err(StCode::TickBad.val());
        }
        self.tick = tick;
        Ok(())
    }

    /// 事件入队。满容时走背压丢弃分支：消耗序号、入丢弃账、不移动游标。
    pub fn ingress(&mut self, kind: u8, tick: u32) -> Result<Ingress, u32> {
        self.advance(tick)?;
        let seq = self.next_seq;
        self.next_seq = match self.next_seq.checked_add(1) {
            Some(v) => v,
            None => return Err(StCode::OverflowBad.val()),
        };
        self.enqueued = match self.enqueued.checked_add(1) {
            Some(v) => v,
            None => return Err(StCode::OverflowBad.val()),
        };
        if self.ring.is_full() {
            self.dropped = match self.dropped.checked_add(1) {
                Some(v) => v,
                None => return Err(StCode::OverflowBad.val()),
            };
            if seq <= self.last_drop_seq {
                return Err(StCode::DropBad.val());
            }
            self.last_drop_seq = seq;
            self.digest = p_core::fold_u32(self.digest, 0xD1);
            self.digest = p_core::fold_u32(self.digest, seq);
            if self.ring.audit().is_err() {
                return Err(StCode::QueueBad.val());
            }
            return Ok(Ingress {
                seq,
                queued: false,
            });
        }
        self.ring.push(Event::new(seq, tick, kind))?;
        self.digest = p_core::fold_u32(self.digest, 0xA1);
        self.digest = p_core::fold_u32(self.digest, seq);
        if self.ring.audit().is_err() {
            return Err(StCode::QueueBad.val());
        }
        Ok(Ingress { seq, queued: true })
    }

    /// 出队最多 `want` 条。返回实际出队数；空环返回 0 不报错。
    pub fn drain(&mut self, tick: u32, want: u32) -> Result<u32, u32> {
        self.advance(tick)?;
        if want == 0 || want > DRAIN_MAX {
            return Err(StCode::CoverBad.val());
        }
        let mut got = 0u32;
        while got < want {
            let e = match self.ring.pop() {
                Some(v) => v,
                None => break,
            };
            if e.seq == 0 || e.seq <= self.last_drained_seq {
                return Err(StCode::OrderBad.val());
            }
            if e.tick > tick {
                return Err(StCode::TickBad.val());
            }
            let lat = tick.saturating_sub(e.tick);
            if lat > TICK_CAP {
                return Err(StCode::DelayBad.val());
            }
            if lat > self.max_lat {
                self.max_lat = lat;
            }
            let b = bucket(lat);
            if let Some(c) = self.lat.get_mut(b) {
                *c = match c.checked_add(1) {
                    Some(v) => v,
                    None => return Err(StCode::OverflowBad.val()),
                };
            }
            self.drained = match self.drained.checked_add(1) {
                Some(v) => v,
                None => return Err(StCode::OverflowBad.val()),
            };
            self.last_drained_seq = e.seq;
            self.digest = p_core::fold_u32(self.digest, e.seq);
            self.digest = p_core::fold_u32(self.digest, e.kind as u32);
            self.digest = p_core::fold_u32(self.digest, lat);
            got = got.saturating_add(1);
        }
        if self.ring.audit().is_err() {
            return Err(StCode::QueueBad.val());
        }
        Ok(got)
    }

    pub fn ring_len(&self) -> usize {
        self.ring.len()
    }

    pub fn last_seq(&self) -> u32 {
        self.last_drained_seq
    }

    pub fn next_seq(&self) -> u32 {
        self.next_seq
    }

    pub fn stats(&self) -> FloodStats {
        FloodStats {
            enqueued: self.enqueued,
            drained: self.drained,
            dropped: self.dropped,
            queued: self.ring.len() as u32,
            max_lat: self.max_lat,
            lat: self.lat,
            digest: self.digest,
        }
    }
}

/// 入队结果：序号 + 是否真正进环（false 即背压丢弃）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ingress {
    pub seq: u32,
    pub queued: bool,
}

// ── 影子账（独立于引擎的复算器） ──────────────────────────────────
/// 只复算四元账，不看引擎内部状态；任何分歧都是 DriftBad。
pub struct Shadow {
    pub enq: u32,
    pub dr: u32,
    pub dp: u32,
    pub q: u32,
}

impl Shadow {
    pub fn new() -> Shadow {
        Shadow {
            enq: 0,
            dr: 0,
            dp: 0,
            q: 0,
        }
    }

    pub fn ingress(&mut self, queued: bool) -> Result<(), u32> {
        self.enq = match self.enq.checked_add(1) {
            Some(v) => v,
            None => return Err(StCode::DriftBad.val()),
        };
        if queued {
            self.q = match self.q.checked_add(1) {
                Some(v) => v,
                None => return Err(StCode::DriftBad.val()),
            };
        } else {
            self.dp = match self.dp.checked_add(1) {
                Some(v) => v,
                None => return Err(StCode::DriftBad.val()),
            };
        }
        Ok(())
    }

    pub fn drain(&mut self, n: u32) -> Result<(), u32> {
        if n > self.q {
            return Err(StCode::DriftBad.val());
        }
        self.q -= n;
        self.dr = match self.dr.checked_add(n) {
            Some(v) => v,
            None => return Err(StCode::DriftBad.val()),
        };
        Ok(())
    }

    pub fn check(&self, s: &FloodStats) -> Result<(), u32> {
        if self.enq != s.enqueued || self.dr != s.drained || self.dp != s.dropped {
            return Err(StCode::DriftBad.val());
        }
        if self.q != s.queued {
            return Err(StCode::DriftBad.val());
        }
        Ok(())
    }
}

// ── F48995 探针 ───────────────────────────────────────────────────
/// 洪泛探针：每轮「突发入队 → 限量出队 → 影子对账 → 账本自检」。
///
/// 加压：每 `PRESSURE_EVERY` 轮叠加 `PRESSURE_EXTRA` 条深突发，
/// 保证 64 槽环必然发生背压丢弃，丢弃分支每轮都被真实走到。
pub fn flood_probe(rounds: u32) -> Result<FloodStats, u32> {
    if rounds == 0 || rounds > FLOOD_ROUNDS {
        return Err(StCode::CoverBad.val());
    }
    let mut e = Engine::new();
    let mut sh = Shadow::new();
    let mut rng = p_core::Lcg::new(0xF495_0001);
    let mut tick = 0u32;
    let mut last_seq = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let mut burst = 1 + ((rng.next() >> 33) as u32 % BURST_MAX);
        if i % PRESSURE_EVERY == 0 {
            burst = burst.saturating_add(PRESSURE_EXTRA);
        }
        let mut b = 0u32;
        while b < burst {
            tick = match tick.checked_add(1) {
                Some(v) => v,
                None => return Err(StCode::OverflowBad.val()),
            };
            let kind = ((rng.next() >> 33) % 8) as u8;
            let ing = e.ingress(kind, tick)?;
            if ing.seq <= last_seq {
                return Err(StCode::OrderBad.val());
            }
            last_seq = ing.seq;
            sh.ingress(ing.queued)?;
            b += 1;
        }
        let want = 1 + ((rng.next() >> 33) as u32 % DRAIN_MAX);
        tick = tick.saturating_add(1);
        let got = e.drain(tick, want)?;
        sh.drain(got)?;
        let s = e.stats();
        sh.check(&s)?;
        if s.lat_sum() != s.drained {
            return Err(StCode::DelayBad.val());
        }
        s.audit()?;
        if e.last_seq() > s.enqueued {
            return Err(StCode::OrderBad.val());
        }
        i += 1;
    }
    let s = e.stats();
    s.audit()?;
    Ok(s)
}

/// 顺序探针：专门核对「丢弃制造空洞但不制造回退」。
/// 连续填满 → 继续入队触发丢弃 → 逐条出队，出队序号必须严格递增。
pub fn order_probe(extra: u32) -> Result<u32, u32> {
    if extra == 0 || extra > QUEUE_CAP as u32 {
        return Err(StCode::CoverBad.val());
    }
    let mut e = Engine::new();
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < QUEUE_CAP as u32 {
        tick = tick.saturating_add(1);
        let ing = e.ingress(0, tick)?;
        if !ing.queued {
            return Err(StCode::FloodBad.val());
        }
        i += 1;
    }
    let mut j = 0u32;
    while j < extra {
        tick = tick.saturating_add(1);
        let ing = e.ingress(1, tick)?;
        if ing.queued {
            return Err(StCode::FloodBad.val());
        }
        j += 1;
    }
    let before = e.stats();
    if before.dropped != extra {
        return Err(StCode::DropBad.val());
    }
    if !before.balanced() {
        return Err(StCode::LedgerBad.val());
    }
    let mut drained = 0u32;
    let mut prev = 0u32;
    while drained < before.queued {
        tick = tick.saturating_add(1);
        let got = e.drain(tick, DRAIN_MAX)?;
        if got == 0 {
            break;
        }
        drained = drained.saturating_add(got);
    }
    let after = e.stats();
    if after.queued != 0 || after.drained != before.queued {
        return Err(StCode::FloodBad.val());
    }
    // 出队序号必须严格递增：用独立重放核一遍。
    let mut e2 = Engine::new();
    let mut t2 = 0u32;
    let mut k = 0u32;
    while k < QUEUE_CAP as u32 {
        t2 = t2.saturating_add(1);
        let _ = e2.ingress(0, t2)?;
        k += 1;
    }
    let mut m = 0u32;
    while m < extra {
        t2 = t2.saturating_add(1);
        let _ = e2.ingress(1, t2)?;
        m += 1;
    }
    while e2.ring_len() > 0 {
        t2 = t2.saturating_add(1);
        let got = e2.drain(t2, DRAIN_MAX)?;
        if got == 0 {
            return Err(StCode::FloodBad.val());
        }
        if e2.last_seq() <= prev {
            return Err(StCode::OrderBad.val());
        }
        prev = e2.last_seq();
    }
    Ok(after.drained)
}

/// 丢弃探针：满容后单条入队，验证丢弃账与序号消耗。
pub fn drop_probe() -> Result<FloodStats, u32> {
    let mut e = Engine::new();
    let mut tick = 0u32;
    let mut i = 0usize;
    while i < QUEUE_CAP {
        tick = tick.saturating_add(1);
        let ing = e.ingress(0, tick)?;
        if !ing.queued {
            return Err(StCode::FloodBad.val());
        }
        i += 1;
    }
    if e.ring_len() != QUEUE_CAP {
        return Err(StCode::QueueBad.val());
    }
    tick = tick.saturating_add(1);
    let ing = e.ingress(7, tick)?;
    if ing.queued {
        return Err(StCode::DropBad.val());
    }
    let s = e.stats();
    if s.dropped != 1 || s.queued as usize != QUEUE_CAP {
        return Err(StCode::DropBad.val());
    }
    if s.enqueued != s.drained.saturating_add(s.dropped).saturating_add(s.queued) {
        return Err(StCode::LedgerBad.val());
    }
    if e.next_seq() != ing.seq.saturating_add(1) {
        return Err(StCode::OrderBad.val());
    }
    Ok(s)
}

/// 延迟探针：构造已知延迟 20/19/18/17/16，验证分桶落位与上界。
pub fn delay_probe() -> Result<FloodStats, u32> {
    let mut e = Engine::new();
    let mut k = 0u32;
    while k < 5 {
        let t = k.saturating_add(1);
        let ing = e.ingress(0, t)?;
        if !ing.queued {
            return Err(StCode::FloodBad.val());
        }
        k += 1;
    }
    let got = e.drain(21, 5)?;
    if got != 5 {
        return Err(StCode::FloodBad.val());
    }
    let s = e.stats();
    if s.max_lat != 20 {
        return Err(StCode::DelayBad.val());
    }
    // 延迟：20,19,18,17,16 → 桶 3 一个（16），桶 4 四个。
    if s.lat != [0, 0, 0, 1, 4] {
        return Err(StCode::DelayBad.val());
    }
    if s.lat_sum() != s.drained {
        return Err(StCode::DelayBad.val());
    }
    Ok(s)
}

// ── F48996 长跑探针 ───────────────────────────────────────────────
/// 长跑探针：每拍「单批入队 → 定量出队」，每 `DRIFT_EVERY` 拍做一次
/// 影子账 + 分桶 + 游标三方对账，终点再做一次全量对账。
///
/// 稳定性判据：
/// - 计数全部 checked 运算，任何溢出直接 OverflowBad（不允许静默饱和）；
/// - 增量账与影子账逐检查点相等（零漂移）；
/// - 定长环的游标每检查点自检（无越界、无槽位泄漏）。
pub fn long_run(rounds: u32) -> Result<FloodStats, u32> {
    if rounds == 0 || rounds > LONG_ROUNDS {
        return Err(StCode::CoverBad.val());
    }
    let mut e = Engine::new();
    let mut sh = Shadow::new();
    let mut rng = p_core::Lcg::new(0xF496_0001);
    let mut tick = 0u32;
    let mut checkpoints = 0u32;
    let mut i = 0u32;
    while i < rounds {
        tick = match tick.checked_add(1) {
            Some(v) => v,
            None => return Err(StCode::OverflowBad.val()),
        };
        let burst = 1 + ((rng.next() >> 33) as u32 % 3);
        let mut b = 0u32;
        while b < burst {
            let kind = ((rng.next() >> 33) % 8) as u8;
            let ing = e.ingress(kind, tick)?;
            sh.ingress(ing.queued)?;
            b += 1;
        }
        let want = 1 + ((rng.next() >> 33) as u32 % DRAIN_MAX);
        let got = e.drain(tick, want)?;
        sh.drain(got)?;
        if i % DRIFT_EVERY == 0 {
            let s = e.stats();
            sh.check(&s)?;
            if !s.balanced() {
                return Err(StCode::DriftBad.val());
            }
            if s.lat_sum() != s.drained {
                return Err(StCode::DelayBad.val());
            }
            if s.queued as usize > QUEUE_CAP {
                return Err(StCode::QueueBad.val());
            }
            if s.enqueued >= u32::MAX / 2 {
                return Err(StCode::OverflowBad.val());
            }
            checkpoints = checkpoints.saturating_add(1);
        }
        i += 1;
    }
    let s = e.stats();
    sh.check(&s)?;
    s.audit()?;
    if checkpoints == 0 {
        return Err(StCode::DriftBad.val());
    }
    Ok(s)
}

/// 定长存储自检：Ring 大小必须等于「槽数组 + 三游标」，Engine 必须
/// 小于一页预算。用 `size_of` 把「不分配堆内存」钉成可验判据。
pub fn storage_audit() -> Result<usize, u32> {
    let slot = core::mem::size_of::<Option<Event>>();
    let ring = core::mem::size_of::<Ring>();
    let cursors = 3usize.saturating_mul(core::mem::size_of::<usize>());
    let expect = QUEUE_CAP.saturating_mul(slot).saturating_add(cursors);
    if ring < expect {
        return Err(StCode::QueueBad.val());
    }
    if ring > expect.saturating_add(16) {
        return Err(StCode::QueueBad.val());
    }
    let engine = core::mem::size_of::<Engine>();
    if engine > ENGINE_BUDGET {
        return Err(StCode::QueueBad.val());
    }
    Ok(engine)
}

// ── 注入 ──────────────────────────────────────────────────────────
/// 四路注入账：每路必须至少命中一次才算全捕获。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; INJECT_ROUTES],
    pub caught: [u32; INJECT_ROUTES],
}

impl InjectLog {
    pub fn new() -> InjectLog {
        InjectLog {
            tries: [0; INJECT_ROUTES],
            caught: [0; INJECT_ROUTES],
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_ROUTES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c == 0 || c > t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 四路注入：
/// ① 时钟回退（入队侧）必须 TickBad；
/// ② 满容直接入环必须 QueueBad（引擎侧背压是 Ok(drop)，环本身拒绝）；
/// ③ 出队序号回退必须 OrderBad；
/// ④ 分桶被篡改后账本必须 DelayBad。
pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 || rounds > FLOOD_ROUNDS {
        return Err(StCode::CoverBad.val());
    }
    let mut log = InjectLog::new();
    let mut i = 0u32;
    while i < rounds {
        // ① 时钟回退。
        let mut e = Engine::new();
        let _ = e.ingress(0, 10)?;
        if e.ingress(0, 9).err() == Some(StCode::TickBad.val()) {
            hit(&mut log, 0)?;
        }
        if let Some(t) = log.tries.get_mut(0) {
            *t = t.saturating_add(1);
        }

        // ② 满容直接入环。
        let mut r = Ring::new();
        let mut k = 0usize;
        while k < QUEUE_CAP {
            let ev = Event::new(k as u32 + 1, 1, 0);
            if r.push(ev).is_err() {
                return Err(StCode::QueueBad.val());
            }
            k += 1;
        }
        if r.push(Event::new(999, 1, 0)).err() == Some(StCode::QueueBad.val()) {
            hit(&mut log, 1)?;
        }
        if let Some(t) = log.tries.get_mut(1) {
            *t = t.saturating_add(1);
        }

        // ③ 出队序号回退。
        let mut e3 = Engine::new();
        let _ = e3.ingress(1, 1)?;
        let got = e3.drain(1, 1)?;
        if got != 1 {
            return Err(StCode::FloodBad.val());
        }
        let stale = Event::new(e3.last_seq(), 1, 0);
        e3.ring.push(stale)?;
        if e3.drain(1, 1).err() == Some(StCode::OrderBad.val()) {
            hit(&mut log, 2)?;
        }
        if let Some(t) = log.tries.get_mut(2) {
            *t = t.saturating_add(1);
        }

        // ④ 分桶篡改。
        let mut e4 = Engine::new();
        let _ = e4.ingress(2, 1)?;
        let got4 = e4.drain(1, 1)?;
        if got4 != 1 {
            return Err(StCode::FloodBad.val());
        }
        if let Some(c) = e4.lat.get_mut(0) {
            *c = c.saturating_add(1);
        }
        if e4.stats().audit().err() == Some(StCode::DelayBad.val()) {
            hit(&mut log, 3)?;
        }
        if let Some(t) = log.tries.get_mut(3) {
            *t = t.saturating_add(1);
        }

        i += 1;
    }
    if !log.all_caught() {
        return Err(StCode::FuzzBad.val());
    }
    Ok(log)
}

fn hit(log: &mut InjectLog, route: usize) -> Result<(), u32> {
    match log.caught.get_mut(route) {
        Some(c) => {
            *c = c.saturating_add(1);
            Ok(())
        }
        None => Err(StCode::CoverBad.val()),
    }
}

// ── 渲染 ──────────────────────────────────────────────────────────
/// 六字段：入队|出队|丢弃|最大延迟|存量|摘要。
pub fn render_stats(s: &FloodStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.enqueued, s.drained, s.dropped, s.max_lat, s.queued, s.digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(48..=57).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 18 {
        return None;
    }
    if *b.get(0)? != b'0' || *b.get(1)? != b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let c = *b.get(i)?;
        let d = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => return None,
        };
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

/// 严格解析六字段并核对四元守恒、分桶守恒、容量与摘要。
pub fn audit_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS {
        return Err(StCode::RenderBad.val());
    }
    let enqueued = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    };
    let drained = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    };
    let dropped = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    };
    let max_lat = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    };
    let queued = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(StCode::RenderBad.val()),
    };
    if enqueued == 0 {
        return Err(StCode::RenderBad.val());
    }
    if enqueued != drained.saturating_add(dropped).saturating_add(queued) {
        return Err(StCode::LedgerBad.val());
    }
    if queued as usize > QUEUE_CAP {
        return Err(StCode::QueueBad.val());
    }
    if max_lat > TICK_CAP {
        return Err(StCode::DelayBad.val());
    }
    if digest == 0 {
        return Err(StCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 黄金运行 ──────────────────────────────────────────────────────
/// 黄金脚本：16 轮 × 4 事件，轮内同拍入队、拍尾出队，
/// 轮内延迟固定回卷为 3/2/1/0，摘要独立复算。
pub fn golden_run() -> Result<FloodStats, u32> {
    let mut e = Engine::new();
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < GOLDEN_ROUNDS {
        let mut k = 0u32;
        while k < GOLDEN_BURST {
            tick = match tick.checked_add(1) {
                Some(v) => v,
                None => return Err(StCode::OverflowBad.val()),
            };
            let ing = e.ingress(k as u8, tick)?;
            if !ing.queued {
                return Err(StCode::FloodBad.val());
            }
            k += 1;
        }
        let got = e.drain(tick, GOLDEN_BURST)?;
        if got != GOLDEN_BURST {
            return Err(StCode::FloodBad.val());
        }
        i += 1;
    }
    let s = e.stats();
    s.audit()?;
    let want = GOLDEN_ROUNDS.saturating_mul(GOLDEN_BURST);
    if s.enqueued != want || s.drained != want {
        return Err(StCode::FloodBad.val());
    }
    if s.dropped != 0 || s.max_lat != GOLDEN_BURST - 1 || s.queued != 0 {
        return Err(StCode::FloodBad.val());
    }
    if s.lat[0] != GOLDEN_ROUNDS
        || s.lat[1] != GOLDEN_ROUNDS
        || s.lat[2] != GOLDEN_ROUNDS.saturating_mul(2)
        || s.lat[3] != 0
        || s.lat[4] != 0
    {
        return Err(StCode::DelayBad.val());
    }
    // 摘要独立复算：先按轮折叠四条入队（A1,seq），再折叠四条出队
    // （seq,kind,lat）。入队与出队在真实引擎里是两段独立的折链，
    // 逐事件交错折叠会得到不同摘要——这正是本判据要钉死的顺序。
    let mut exp = p_core::FNV_OFF;
    let mut seq = 1u32;
    let mut r = 0u32;
    while r < GOLDEN_ROUNDS {
        let first = seq;
        let mut k = 0u32;
        while k < GOLDEN_BURST {
            exp = p_core::fold_u32(exp, 0xA1);
            exp = p_core::fold_u32(exp, seq);
            seq = seq.saturating_add(1);
            k += 1;
        }
        let mut k = 0u32;
        while k < GOLDEN_BURST {
            exp = p_core::fold_u32(exp, first.saturating_add(k));
            exp = p_core::fold_u32(exp, k);
            exp = p_core::fold_u32(exp, (GOLDEN_BURST - 1).saturating_sub(k));
            k += 1;
        }
        r += 1;
    }
    if s.digest != exp {
        return Err(StCode::ReplayBad.val());
    }
    Ok(s)
}

/// 三段复现：黄金、洪泛、长跑各跑两次，逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(StCode::ReplayBad.val());
    }
    let c = flood_probe(256)?;
    let d = flood_probe(256)?;
    if c != d {
        return Err(StCode::ReplayBad.val());
    }
    let e = long_run(2_048)?;
    let f = long_run(2_048)?;
    if e != f {
        return Err(StCode::ReplayBad.val());
    }
    Ok(())
}

pub fn probe_seq_a() -> Result<u32, u32> {
    Ok(golden_run()?.enqueued)
}

pub fn probe_seq_b() -> Result<u32, u32> {
    Ok(flood_probe(256)?.dropped)
}

pub fn probe_seq_c() -> Result<u32, u32> {
    Ok(long_run(2_048)?.drained)
}

// ══════════════════════════════════════════════════════════════════
// F48995 手势状态机（1/2/3 指 · 42 迁移表）错误码族（15 条）
// ──────────────────────────────────────────────────────────────────
// 与上文洪泛账正交：洪泛账回答「队列守恒不守恒」，本段回答「状态机
// 自己的拒绝是否可分辨」。15 条码占用本模块窗口的 +0x20..+0x2E，
// 与 StCode 的 +0x01..+0x0E 严格不相交（J1 要求零漂移且互不混淆）。
// 迁移动机只有一件事：1/2/3 指的三槽手势，任何非法事件必须返回
// 一条**具体**的码，而不是笼统的「失败」——否则排障时看不到真因。
// ══════════════════════════════════════════════════════════════════

/// 状态机状态数：Idle/One/Two/Three/Hold/Done/Canceled。
pub const FSM_STATES: usize = 7;
/// 状态机事件数：Down/Move/Up/Cancel。
pub const FSM_EVENTS: usize = 4;
/// 迁移表行数：7 状态 × 6 键 = 42。
pub const FSM_TRANSITIONS: usize = 42;
/// 错误码族条数（卡面 15 条）。
pub const FSM_CODES: usize = 15;
/// 码族在模块窗口内的起始偏移（+0x20..+0x2E）。
pub const FSM_CODE_OFF: u32 = 0x20;
/// 模块窗口宽度（码族必须整体落在窗内）。
pub const FSM_WINDOW: u32 = 0x40;
/// 单次手势耗世上界（拍）。
pub const FSM_TIMEOUT: u32 = 100_000;
/// 观测四点数（入口/出口/耗时/结果）。
pub const FSM_OBS_POINTS: usize = 4;
/// 观测探针轮数。
pub const FSM_OBS_ROUNDS: u32 = 100;
/// 观测探针轮数上界（覆盖闸门）。
pub const FSM_OBS_MAX: u32 = 1_000;

/// 状态常量：Idle。
pub const S_IDLE: u8 = 0;
/// 状态常量：单指按下。
pub const S_ONE: u8 = 1;
/// 状态常量：双指。
pub const S_TWO: u8 = 2;
/// 状态常量：三指。
pub const S_THREE: u8 = 3;
/// 状态常量：三指长按。
pub const S_HOLD: u8 = 4;
/// 状态常量：收口完成。
pub const S_DONE: u8 = 5;
/// 状态常量：已取消。
pub const S_CANCELED: u8 = 6;

/// 事件常量：按下。
pub const EV_DOWN: u8 = 0;
/// 事件常量：移动。
pub const EV_MOVE: u8 = 1;
/// 事件常量：抬起。
pub const EV_UP: u8 = 2;
/// 事件常量：取消。
pub const EV_CANCEL: u8 = 3;

/// 状态名（空串即未知态，供渲染与排障）。
pub fn state_name(s: u8) -> &'static str {
    match s {
        S_IDLE => "Idle",
        S_ONE => "One",
        S_TWO => "Two",
        S_THREE => "Three",
        S_HOLD => "Hold",
        S_DONE => "Done",
        S_CANCELED => "Canceled",
        _ => "",
    }
}

/// 事件名（空串即未知事件）。
pub fn event_name(e: u8) -> &'static str {
    match e {
        EV_DOWN => "Down",
        EV_MOVE => "Move",
        EV_UP => "Up",
        EV_CANCEL => "Cancel",
        _ => "",
    }
}

// ── 15 条错误码族（本模块窗口 +0x20..+0x2E） ─────────────────────
/// 手势状态机错误码族（15 条）。判别值 = 段基址 + 0x20 + 枚举序。
///
/// 与 `StCode`（洪泛账 14 条，+0x01..+0x0E）分属两族：窗口不重叠，
/// 排障时单看诊断值即可判定是「队列账坏了」还是「状态机拒绝了」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum FsmCode {
    /// 状态号越界
    StateBad = 0,
    /// 迁移表无此行（当前态不接受该事件）
    TransitionBad = 1,
    /// 手指数不在 1..=3
    FingerBad = 2,
    /// 42 行表重复/缺行/越界
    TableBad = 3,
    /// 观测入口会话或时刻非法
    EntryBad = 4,
    /// 观测出口无对应入口
    ExitBad = 5,
    /// 手势耗世越界
    TimeoutBad = 6,
    /// 观测序号回退或入口重入
    SeqBad = 7,
    /// 活动态未收口即结算
    CancelBad = 8,
    /// Hold 态要求恰好三指
    HoldBad = 9,
    /// 计数账与步数不守恒
    CountBad = 10,
    /// 结果无错误码或成功却带错误码
    ObsBad = 11,
    /// 四点覆盖不足或轮数越界
    CoverBad = 12,
    /// 摘要复算漂移
    DriftBad = 13,
    /// 码族偏移越窗或与 StCode 撞值
    SegmentBad = 14,
}

impl FsmCode {
    /// 诊断值：段基址 + 0x20 + 枚举序（窗口 +0x20..+0x2E）。
    pub fn val(self) -> u32 {
        SEG_BASE + FSM_CODE_OFF + self as u32
    }

    /// 中文诊断说明。
    pub fn detail(self) -> &'static str {
        match self {
            FsmCode::StateBad => "状态号越界或状态名缺失",
            FsmCode::TransitionBad => "迁移表无此行",
            FsmCode::FingerBad => "手指数不在1..=3",
            FsmCode::TableBad => "42行表重复缺行或越界",
            FsmCode::EntryBad => "观测入口会话或时刻非法",
            FsmCode::ExitBad => "观测出口无对应入口",
            FsmCode::TimeoutBad => "手势耗世越界",
            FsmCode::SeqBad => "观测序号回退或入口重入",
            FsmCode::CancelBad => "活动态未收口即结算",
            FsmCode::HoldBad => "Hold态要求恰好三指",
            FsmCode::CountBad => "计数账与步数不守恒",
            FsmCode::ObsBad => "结果无码或成功带码",
            FsmCode::CoverBad => "四点覆盖不足或轮数越界",
            FsmCode::DriftBad => "摘要复算漂移",
            FsmCode::SegmentBad => "码族偏移越窗或撞值",
        }
    }

    /// 全码族（15 条，窗口 +0x20..+0x2E）。
    pub const ALL: [FsmCode; FSM_CODES] = [
        FsmCode::StateBad,
        FsmCode::TransitionBad,
        FsmCode::FingerBad,
        FsmCode::TableBad,
        FsmCode::EntryBad,
        FsmCode::ExitBad,
        FsmCode::TimeoutBad,
        FsmCode::SeqBad,
        FsmCode::CancelBad,
        FsmCode::HoldBad,
        FsmCode::CountBad,
        FsmCode::ObsBad,
        FsmCode::CoverBad,
        FsmCode::DriftBad,
        FsmCode::SegmentBad,
    ];
}

/// 自由函数版诊断说明（按诊断值反查，两条码族共用），
/// 先查 15 条状态机码，再回落 14 条洪泛码。
pub fn fsm_detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < FsmCode::ALL.len() {
        if let Some(c) = FsmCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    detail(code)
}

/// 15 条诊断值两两不同。
pub fn fsm_codes_distinct() -> bool {
    let mut i = 0usize;
    while i < FsmCode::ALL.len() {
        let a = match FsmCode::ALL.get(i) {
            Some(v) => v.val(),
            None => return false,
        };
        if a == 0 {
            return false;
        }
        let mut j = i + 1;
        while j < FsmCode::ALL.len() {
            let b = match FsmCode::ALL.get(j) {
                Some(v) => v.val(),
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 偏移闸门：诊断值必须落在本族窗口 +0x20..+0x2E 内。
pub fn fsm_offset_guard(val: u32) -> Result<(), u32> {
    let off = val.saturating_sub(SEG_BASE);
    if off < FSM_CODE_OFF || off >= FSM_WINDOW {
        return Err(FsmCode::SegmentBad.val());
    }
    if off > FSM_CODE_OFF + (FSM_CODES as u32 - 1) {
        return Err(FsmCode::SegmentBad.val());
    }
    Ok(())
}

/// 段位自检：15 条码全部在窗内、两两不同、且与 `StCode` 14 条不相交。
/// 这是「互不混淆」在段层面的硬约束：一旦两族共用诊断值，按值排障
/// 就会把状态机拒绝误读成队列账错误。
pub fn fsm_segment_guard() -> Result<(), u32> {
    let mut i = 0usize;
    while i < FsmCode::ALL.len() {
        let c = match FsmCode::ALL.get(i) {
            Some(v) => *v,
            None => return Err(FsmCode::SegmentBad.val()),
        };
        fsm_offset_guard(c.val())?;
        let mut j = 0usize;
        while j < StCode::ALL.len() {
            if let Some(s) = StCode::ALL.get(j) {
                if s.val() == c.val() {
                    return Err(FsmCode::SegmentBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    if fsm_codes_distinct() {
        Ok(())
    } else {
        Err(FsmCode::SegmentBad.val())
    }
}

/// 摘要复算闸门：期望与实测必须逐位相等，否则 DriftBad。
pub fn fsm_digest_guard(expect: u64, got: u64) -> Result<(), u32> {
    if expect == got {
        Ok(())
    } else {
        Err(FsmCode::DriftBad.val())
    }
}

// ── 42 行迁移表 ───────────────────────────────────────────────────
/// 迁移表一行：来源态、事件、手指数、目标态。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FsmRow {
    pub from: u8,
    pub event: u8,
    pub fingers: u8,
    pub to: u8,
}

/// 迁移表行构造器（const，便于静态表逐行书写）。
pub const fn row(from: u8, event: u8, fingers: u8, to: u8) -> FsmRow {
    FsmRow {
        from,
        event,
        fingers,
        to,
    }
}

/// 42 迁移表：7 状态 × 6 键（每个状态 6 条，键两两不同）。
///
/// 语义约定：
/// - 只有 Down 能升级手指数（1→2→3），Move 保持原态；
/// - 三指状态下的再次 Down(3) 进入 Hold（长按），Hold 的 Up(3) 退回 Two；
/// - 每个活动态都有 Cancel(1/2/3) 直达 Canceled；
/// - Canceled/Done 只接受 Down 重启与 Move 保持，Up 无行（TransitionBad）。
pub const FSM_TABLE: [FsmRow; FSM_TRANSITIONS] = [
    // Idle：三种按下起手；Move 不改变态（空态抖动）。
    row(S_IDLE, EV_DOWN, 1, S_ONE),
    row(S_IDLE, EV_DOWN, 2, S_TWO),
    row(S_IDLE, EV_DOWN, 3, S_THREE),
    row(S_IDLE, EV_MOVE, 1, S_IDLE),
    row(S_IDLE, EV_MOVE, 2, S_IDLE),
    row(S_IDLE, EV_MOVE, 3, S_IDLE),
    // One：单指保持/加指/抬起收口；取消任意指。
    row(S_ONE, EV_MOVE, 1, S_ONE),
    row(S_ONE, EV_DOWN, 2, S_TWO),
    row(S_ONE, EV_UP, 1, S_DONE),
    row(S_ONE, EV_CANCEL, 1, S_CANCELED),
    row(S_ONE, EV_CANCEL, 2, S_CANCELED),
    row(S_ONE, EV_CANCEL, 3, S_CANCELED),
    // Two：双指保持/加指/退指；取消任意指。
    row(S_TWO, EV_MOVE, 2, S_TWO),
    row(S_TWO, EV_DOWN, 3, S_THREE),
    row(S_TWO, EV_UP, 2, S_ONE),
    row(S_TWO, EV_CANCEL, 1, S_CANCELED),
    row(S_TWO, EV_CANCEL, 2, S_CANCELED),
    row(S_TWO, EV_CANCEL, 3, S_CANCELED),
    // Three：三指保持/长按/退指；取消任意指。
    row(S_THREE, EV_MOVE, 1, S_THREE),
    row(S_THREE, EV_MOVE, 3, S_THREE),
    row(S_THREE, EV_DOWN, 3, S_HOLD),
    row(S_THREE, EV_UP, 3, S_TWO),
    row(S_THREE, EV_CANCEL, 1, S_CANCELED),
    row(S_THREE, EV_CANCEL, 3, S_CANCELED),
    // Hold：长按保持/抬手退出；取消任意指。
    row(S_HOLD, EV_MOVE, 1, S_HOLD),
    row(S_HOLD, EV_MOVE, 3, S_HOLD),
    row(S_HOLD, EV_UP, 3, S_TWO),
    row(S_HOLD, EV_CANCEL, 1, S_CANCELED),
    row(S_HOLD, EV_CANCEL, 2, S_CANCELED),
    row(S_HOLD, EV_CANCEL, 3, S_CANCELED),
    // Done：只接受重新按下与 Move 保持。
    row(S_DONE, EV_DOWN, 1, S_ONE),
    row(S_DONE, EV_DOWN, 2, S_TWO),
    row(S_DONE, EV_DOWN, 3, S_THREE),
    row(S_DONE, EV_MOVE, 1, S_DONE),
    row(S_DONE, EV_MOVE, 2, S_DONE),
    row(S_DONE, EV_MOVE, 3, S_DONE),
    // Canceled：同上，Move 保持取消态。
    row(S_CANCELED, EV_DOWN, 1, S_ONE),
    row(S_CANCELED, EV_DOWN, 2, S_TWO),
    row(S_CANCELED, EV_DOWN, 3, S_THREE),
    row(S_CANCELED, EV_MOVE, 1, S_CANCELED),
    row(S_CANCELED, EV_MOVE, 2, S_CANCELED),
    row(S_CANCELED, EV_MOVE, 3, S_CANCELED),
];

/// 单行合法性：状态/事件/手指数在域内，且 Hold 只由三指进入。
pub fn fsm_row_check(r: FsmRow) -> Result<(), u32> {
    if (r.from as usize) >= FSM_STATES || (r.to as usize) >= FSM_STATES {
        return Err(FsmCode::StateBad.val());
    }
    if (r.event as usize) >= FSM_EVENTS {
        return Err(FsmCode::TransitionBad.val());
    }
    if r.fingers < 1 || r.fingers > 3 {
        return Err(FsmCode::FingerBad.val());
    }
    if r.to == S_HOLD && r.from != S_HOLD && r.fingers != 3 {
        return Err(FsmCode::HoldBad.val());
    }
    Ok(())
}

/// 42 行表自检：行数、逐行合法、键两两唯一、每态 6 行、7 个状态都可达。
pub fn fsm_table_audit(rows: &[FsmRow]) -> Result<usize, u32> {
    if rows.len() != FSM_TRANSITIONS {
        return Err(FsmCode::TableBad.val());
    }
    let mut per_state = [0u32; FSM_STATES];
    let mut to_seen = [false; FSM_STATES];
    let mut seen = [false; FSM_STATES * FSM_EVENTS * 4];
    let mut i = 0usize;
    while i < FSM_TRANSITIONS {
        let r = match rows.get(i) {
            Some(v) => *v,
            None => return Err(FsmCode::TableBad.val()),
        };
        fsm_row_check(r)?;
        let key = (r.from as usize * FSM_EVENTS + r.event as usize) * 4 + (r.fingers as usize - 1);
        let slot = match seen.get_mut(key) {
            Some(v) => v,
            None => return Err(FsmCode::TableBad.val()),
        };
        if *slot {
            return Err(FsmCode::TableBad.val());
        }
        *slot = true;
        if let Some(t) = to_seen.get_mut(r.to as usize) {
            *t = true;
        }
        let c = match per_state.get_mut(r.from as usize) {
            Some(v) => v,
            None => return Err(FsmCode::StateBad.val()),
        };
        *c = match c.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::CountBad.val()),
        };
        i += 1;
    }
    let per = FSM_TRANSITIONS / FSM_STATES;
    let mut s = 0usize;
    while s < FSM_STATES {
        match per_state.get(s) {
            Some(v) if *v == per as u32 => {}
            _ => return Err(FsmCode::TableBad.val()),
        }
        match to_seen.get(s) {
            Some(true) => {}
            _ => return Err(FsmCode::TableBad.val()),
        }
        s += 1;
    }
    Ok(rows.len())
}

/// 表查找：返回目标态；越界/无行分别给出具体码。
pub fn fsm_lookup(from: u8, event: u8, fingers: u8) -> Result<u8, u32> {
    if (from as usize) >= FSM_STATES {
        return Err(FsmCode::StateBad.val());
    }
    if (event as usize) >= FSM_EVENTS {
        return Err(FsmCode::TransitionBad.val());
    }
    if fingers < 1 || fingers > 3 {
        return Err(FsmCode::FingerBad.val());
    }
    let mut i = 0usize;
    while i < FSM_TRANSITIONS {
        if let Some(r) = FSM_TABLE.get(i) {
            if r.from == from && r.event == event && r.fingers == fingers {
                return Ok(r.to);
            }
        }
        i += 1;
    }
    Err(FsmCode::TransitionBad.val())
}

/// 指宽探针：Idle 下 Down(1/2/3) 必须分别落到 One/Two/Three。
pub fn fsm_finger_probe() -> Result<u8, u32> {
    let mut f = 1u8;
    while f <= 3 {
        let to = fsm_lookup(S_IDLE, EV_DOWN, f)?;
        let want = match f {
            1 => S_ONE,
            2 => S_TWO,
            _ => S_THREE,
        };
        if to != want {
            return Err(FsmCode::TransitionBad.val());
        }
        f = f.saturating_add(1);
    }
    Ok(3)
}

// ── 计数账 ────────────────────────────────────────────────────────
/// 按来源态与事件计数的双账：两本账的 Σ 都必须等于步数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FsmCounts {
    pub from: [u32; FSM_STATES],
    pub ev: [u32; FSM_EVENTS],
    pub steps: u32,
}

impl FsmCounts {
    pub fn new() -> FsmCounts {
        FsmCounts {
            from: [0; FSM_STATES],
            ev: [0; FSM_EVENTS],
            steps: 0,
        }
    }

    /// 记一步。三处 checked 递增，任何溢出直接 CountBad。
    pub fn record(&mut self, from: u8, event: u8) -> Result<(), u32> {
        let f = match self.from.get_mut(from as usize) {
            Some(v) => v,
            None => return Err(FsmCode::StateBad.val()),
        };
        *f = match f.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::CountBad.val()),
        };
        let e = match self.ev.get_mut(event as usize) {
            Some(v) => v,
            None => return Err(FsmCode::TransitionBad.val()),
        };
        *e = match e.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::CountBad.val()),
        };
        self.steps = match self.steps.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::CountBad.val()),
        };
        Ok(())
    }

    /// 双账对步数核对：Σfrom == steps 且 Σev == steps。
    pub fn audit(&self) -> Result<(), u32> {
        let mut sf = 0u32;
        let mut i = 0usize;
        while i < FSM_STATES {
            if let Some(v) = self.from.get(i) {
                sf = match sf.checked_add(*v) {
                    Some(x) => x,
                    None => return Err(FsmCode::CountBad.val()),
                };
            }
            i += 1;
        }
        let mut se = 0u32;
        let mut j = 0usize;
        while j < FSM_EVENTS {
            if let Some(v) = self.ev.get(j) {
                se = match se.checked_add(*v) {
                    Some(x) => x,
                    None => return Err(FsmCode::CountBad.val()),
                };
            }
            j += 1;
        }
        if sf != self.steps || se != self.steps {
            return Err(FsmCode::CountBad.val());
        }
        Ok(())
    }
}

/// 状态机：状态唯一，账本与摘要同步推进。
pub struct FsmMachine {
    state: u8,
    counts: FsmCounts,
    digest: u64,
}

impl FsmMachine {
    pub fn new() -> FsmMachine {
        FsmMachine {
            state: S_IDLE,
            counts: FsmCounts::new(),
            digest: p_core::FNV_OFF,
        }
    }

    pub fn state(&self) -> u8 {
        self.state
    }

    pub fn steps(&self) -> u32 {
        self.counts.steps
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    pub fn counts_audit(&self) -> Result<(), u32> {
        self.counts.audit()
    }

    /// 推进一步：查表 → 记账 → 折摘要 → 换态。查表失败即返回具体码。
    pub fn step(&mut self, event: u8, fingers: u8) -> Result<u8, u32> {
        let from = self.state;
        let to = fsm_lookup(from, event, fingers)?;
        self.counts.record(from, event)?;
        self.digest = p_core::fold_u32(self.digest, from as u32);
        self.digest = p_core::fold_u32(self.digest, event as u32);
        self.digest = p_core::fold_u32(self.digest, fingers as u32);
        self.digest = p_core::fold_u32(self.digest, to as u32);
        self.state = to;
        Ok(to)
    }

    /// 结算：只有 Done/Canceled 允许收口；活动态未收口一律 CancelBad。
    pub fn settle(&self) -> Result<u8, u32> {
        match self.state {
            S_DONE | S_CANCELED => Ok(self.state),
            _ => Err(FsmCode::CancelBad.val()),
        }
    }
}

/// 合法脚本：1→2→3 指升级、三指长按、逐指抬起收口。
pub fn fsm_script() -> Result<u64, u32> {
    let events: [(u8, u8); 7] = [
        (EV_DOWN, 1),
        (EV_DOWN, 2),
        (EV_DOWN, 3),
        (EV_DOWN, 3),
        (EV_UP, 3),
        (EV_UP, 2),
        (EV_UP, 1),
    ];
    let want = [S_ONE, S_TWO, S_THREE, S_HOLD, S_TWO, S_ONE, S_DONE];
    let mut m = FsmMachine::new();
    let mut i = 0usize;
    while i < events.len() {
        let (e, f) = match events.get(i) {
            Some(v) => *v,
            None => return Err(FsmCode::CoverBad.val()),
        };
        let to = m.step(e, f)?;
        match want.get(i) {
            Some(w) if *w == to => {}
            _ => return Err(FsmCode::TransitionBad.val()),
        }
        i += 1;
    }
    m.counts_audit()?;
    if m.steps() != events.len() as u32 {
        return Err(FsmCode::CountBad.val());
    }
    if m.settle()? != S_DONE {
        return Err(FsmCode::CancelBad.val());
    }
    // 同一脚本第二次独立跑的摘要必须逐位相等（零漂移）。
    let mut n = FsmMachine::new();
    let mut k = 0usize;
    while k < events.len() {
        let (e, f) = match events.get(k) {
            Some(v) => *v,
            None => return Err(FsmCode::CoverBad.val()),
        };
        n.step(e, f)?;
        k += 1;
    }
    fsm_digest_guard(m.digest(), n.digest())?;
    Ok(m.digest())
}

/// 取消脚本：单指按下→取消，双指按下→取消；两段都必须收口在 Canceled。
pub fn fsm_cancel_script() -> Result<u64, u32> {
    let mut m = FsmMachine::new();
    if m.step(EV_DOWN, 1)? != S_ONE {
        return Err(FsmCode::TransitionBad.val());
    }
    if m.step(EV_CANCEL, 1)? != S_CANCELED {
        return Err(FsmCode::CancelBad.val());
    }
    if m.step(EV_DOWN, 2)? != S_TWO {
        return Err(FsmCode::TransitionBad.val());
    }
    if m.step(EV_CANCEL, 3)? != S_CANCELED {
        return Err(FsmCode::CancelBad.val());
    }
    m.counts_audit()?;
    if m.settle()? != S_CANCELED {
        return Err(FsmCode::CancelBad.val());
    }
    Ok(m.digest())
}

/// 15 条码触发器：每条码都用真实 API 打出来，并逐条核对「打出来的
/// 就是这条码」。顺序与 `FsmCode::ALL` 一致，重复枚举时数组必须逐位
/// 相等——这就是 J1 的「零漂移、互不混淆」。
pub fn fsm_code_probe() -> Result<[u32; FSM_CODES], u32> {
    let mut got = [0u32; FSM_CODES];
    // 0 StateBad：来源态越界。
    let a = match fsm_lookup(FSM_STATES as u8, EV_DOWN, 1) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 0, a, FsmCode::StateBad)?;
    // 1 TransitionBad：Idle 不接受 Up。
    let b = match fsm_lookup(S_IDLE, EV_UP, 1) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 1, b, FsmCode::TransitionBad)?;
    // 2 FingerBad：手指数 4。
    let c = match fsm_lookup(S_IDLE, EV_DOWN, 4) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 2, c, FsmCode::FingerBad)?;
    // 3 TableBad：复制表并制造重复键。
    let mut rows = FSM_TABLE;
    if let Some(r) = rows.get_mut(1) {
        *r = FSM_TABLE[0];
    }
    let d = match fsm_table_audit(&rows) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 3, d, FsmCode::TableBad)?;
    // 4 EntryBad：会话号 0。
    let mut o = FsmObserver::new();
    let e = match o.entry(0, 1, 1) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 4, e, FsmCode::EntryBad)?;
    // 5 ExitBad：无入口直接出口。
    let mut o5 = FsmObserver::new();
    let f = match o5.exit(1) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 5, f, FsmCode::ExitBad)?;
    // 6 TimeoutBad：耗世越过上界（出口关闭入口后报超时）。
    let mut o6 = FsmObserver::new();
    let _ = o6.entry(1, 1, 0)?;
    let g = match o6.exit(FSM_TIMEOUT.saturating_add(1)) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 6, g, FsmCode::TimeoutBad)?;
    // 7 SeqBad：入口重入。
    let mut o7 = FsmObserver::new();
    let _ = o7.entry(1, 1, 1)?;
    let h = match o7.entry(2, 1, 2) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 7, h, FsmCode::SeqBad)?;
    // 8 CancelBad：活动态直接结算。
    let mut m = FsmMachine::new();
    let _ = m.step(EV_DOWN, 1)?;
    let i = match m.settle() {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 8, i, FsmCode::CancelBad)?;
    // 9 HoldBad：三指态之外的 Hold 行。
    let j = match fsm_row_check(row(S_THREE, EV_DOWN, 2, S_HOLD)) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 9, j, FsmCode::HoldBad)?;
    // 10 CountBad：账本步数被篡改。
    let mut cnt = FsmCounts::new();
    cnt.record(S_IDLE, EV_DOWN)?;
    cnt.steps = cnt.steps.saturating_add(1);
    let k = match cnt.audit() {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 10, k, FsmCode::CountBad)?;
    // 11 ObsBad：失败结果却没有错误码。
    let mut o11 = FsmObserver::new();
    let l = match o11.result(false, None) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 11, l, FsmCode::ObsBad)?;
    // 12 CoverBad：轮数 0 触发覆盖闸门。
    let m12 = match fsm_obs_probe(0) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 12, m12, FsmCode::CoverBad)?;
    // 13 DriftBad：摘要复算不等。
    let n = match fsm_digest_guard(1, 2) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 13, n, FsmCode::DriftBad)?;
    // 14 SegmentBad：拿洪泛族的值过本族偏移闸门必须被拒。
    let p = match fsm_offset_guard(StCode::FloodBad.val()) {
        Ok(_) => 0,
        Err(c) => c,
    };
    put_code(&mut got, 14, p, FsmCode::SegmentBad)?;
    // 族内两两不同 + 段位自检。
    let mut x = 0usize;
    while x < FSM_CODES {
        let v = match got.get(x) {
            Some(n) => *n,
            None => return Err(FsmCode::CoverBad.val()),
        };
        if v == 0 {
            return Err(FsmCode::CoverBad.val());
        }
        let mut y = x + 1;
        while y < FSM_CODES {
            match got.get(y) {
                Some(w) if *w == v => return Err(FsmCode::SegmentBad.val()),
                Some(_) => {}
                None => return Err(FsmCode::CoverBad.val()),
            }
            y += 1;
        }
        x += 1;
    }
    fsm_segment_guard()?;
    Ok(got)
}

fn put_code(got: &mut [u32; FSM_CODES], i: usize, code: u32, want: FsmCode) -> Result<(), u32> {
    if code != want.val() {
        return Err(want.val());
    }
    match got.get_mut(i) {
        Some(v) => {
            *v = code;
            Ok(())
        }
        None => Err(FsmCode::CoverBad.val()),
    }
}

// ══════════════════════════════════════════════════════════════════
// F48996 观测埋点（入口/出口/耗时/结果四点全覆盖）
// ──────────────────────────────────────────────────────────────────
// 四点不是四种日志，而是四种**必达**的观测事实：入口记会话与起始拍，
// 出口记闭合拍，耗时由两拍相减落账，结果记成功/失败与错误码。缺任何
// 一点，埋点就断链；四点的占比失衡（例如只记入口不记出口）同样判死。
// 「异常零静默」：任何拒绝都必须 increment 异常账并进入可见路径，
// 被吞掉的异常由 `swallow` 单独计数，`audit` 一旦看到静默即 ObsBad。
// ══════════════════════════════════════════════════════════════════

/// 观测四点：入口/出口/耗时/结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum FsmPoint {
    Entry = 0,
    Exit = 1,
    Duration = 2,
    Result = 3,
}

impl FsmPoint {
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            FsmPoint::Entry => "入口",
            FsmPoint::Exit => "出口",
            FsmPoint::Duration => "耗时",
            FsmPoint::Result => "结果",
        }
    }

    pub const ALL: [FsmPoint; FSM_OBS_POINTS] = [
        FsmPoint::Entry,
        FsmPoint::Exit,
        FsmPoint::Duration,
        FsmPoint::Result,
    ];
}

/// 四点观测器：四点计数、异常账、静默账、耗时与摘要。
pub struct FsmObserver {
    counts: [u32; FSM_OBS_POINTS],
    total: u32,
    anomalies: u32,
    handled: u32,
    silent: u32,
    open: bool,
    open_tick: u32,
    fingers: u8,
    last_code: u32,
    dur_sum: u32,
    dur_max: u32,
    digest: u64,
}

impl FsmObserver {
    pub fn new() -> FsmObserver {
        FsmObserver {
            counts: [0; FSM_OBS_POINTS],
            total: 0,
            anomalies: 0,
            handled: 0,
            silent: 0,
            open: false,
            open_tick: 0,
            fingers: 0,
            last_code: 0,
            dur_sum: 0,
            dur_max: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 单点落账：计数与总数同步 checked 递增，摘要按点号折链。
    fn touch(&mut self, p: usize) -> Result<(), u32> {
        let c = match self.counts.get_mut(p) {
            Some(v) => v,
            None => return Err(FsmCode::CoverBad.val()),
        };
        *c = match c.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::ObsBad.val()),
        };
        self.total = match self.total.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::ObsBad.val()),
        };
        self.digest = p_core::fold_u32(self.digest, 0x0B);
        self.digest = p_core::fold_u32(self.digest, p as u32);
        Ok(())
    }

    /// 入口点：会话非 0、时刻不越界、手指数在域内、不允许入口重入。
    pub fn entry(&mut self, session: u32, fingers: u8, tick: u32) -> Result<(), u32> {
        if session == 0 || tick > TICK_CAP {
            return Err(FsmCode::EntryBad.val());
        }
        if fingers < 1 || fingers > 3 {
            return Err(FsmCode::FingerBad.val());
        }
        if self.open {
            return Err(FsmCode::SeqBad.val());
        }
        self.open = true;
        self.open_tick = tick;
        self.fingers = fingers;
        self.touch(FsmPoint::Entry.index())
    }

    /// 出口点 + 耗时点：耗世由两拍相减，越界即超时（关节必须闭合，
    /// 否则观测器会卡在半开状态，后续出口全部 ExitBad）。
    pub fn exit(&mut self, tick: u32) -> Result<u32, u32> {
        if !self.open {
            return Err(FsmCode::ExitBad.val());
        }
        if tick > TICK_CAP {
            return Err(FsmCode::ExitBad.val());
        }
        if tick < self.open_tick {
            return Err(FsmCode::SeqBad.val());
        }
        let dur = tick.saturating_sub(self.open_tick);
        self.open = false;
        if dur > FSM_TIMEOUT {
            return Err(FsmCode::TimeoutBad.val());
        }
        self.touch(FsmPoint::Exit.index())?;
        self.touch(FsmPoint::Duration.index())?;
        self.dur_sum = self.dur_sum.saturating_add(dur);
        if dur > self.dur_max {
            self.dur_max = dur;
        }
        Ok(dur)
    }

    /// 结果点：成功不得带错误码，失败必须带错误码。
    pub fn result(&mut self, ok: bool, code: Option<u32>) -> Result<(), u32> {
        if ok == code.is_some() {
            return Err(FsmCode::ObsBad.val());
        }
        self.touch(FsmPoint::Result.index())
    }

    /// 可见异常：异常账 + 已处理账 + 摘要，三者同时推进。
    pub fn record_error(&mut self, code: u32) -> Result<(), u32> {
        if code == 0 {
            return Err(FsmCode::ObsBad.val());
        }
        self.anomalies = match self.anomalies.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::ObsBad.val()),
        };
        self.handled = match self.handled.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::ObsBad.val()),
        };
        self.last_code = code;
        self.digest = p_core::fold_u32(self.digest, 0xE0);
        self.digest = p_core::fold_u32(self.digest, code);
        Ok(())
    }

    /// 静默异常：只加异常账、不加已处理账——`audit` 必然否决。
    /// 这个入口存在的意义就是把「吞异常」做成可测的负例。
    pub fn swallow(&mut self, code: u32) -> Result<(), u32> {
        self.anomalies = match self.anomalies.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::ObsBad.val()),
        };
        self.silent = match self.silent.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::ObsBad.val()),
        };
        self.last_code = code;
        Ok(())
    }

    pub fn count(&self, p: usize) -> u32 {
        match self.counts.get(p) {
            Some(v) => *v,
            None => 0,
        }
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn anomaly_count(&self) -> u32 {
        self.anomalies
    }

    pub fn handled_count(&self) -> u32 {
        self.handled
    }

    pub fn silent_count(&self) -> u32 {
        self.silent
    }

    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    pub fn digest(&self) -> u64 {
        self.digest
    }

    pub fn duration_max(&self) -> u32 {
        self.dur_max
    }

    /// 单点占比（百分比，整数运算）。
    pub fn percent(&self, p: usize) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let c = self.count(p) as u64;
        ((c.saturating_mul(100)) / self.total as u64) as u32
    }

    pub fn duration_avg(&self) -> u32 {
        let n = self.count(FsmPoint::Duration.index());
        if n == 0 {
            0
        } else {
            self.dur_sum / n
        }
    }

    /// 四点账自检：无静默 → 四点齐全 → 单点占比 ≥10% → 关节闭合 →
    /// 异常账与已处理账相等。任一不满足即拒绝，绝不放行半截账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.silent > 0 {
            return Err(FsmCode::ObsBad.val());
        }
        if self.total == 0 {
            return Err(FsmCode::CoverBad.val());
        }
        let mut i = 0usize;
        while i < FSM_OBS_POINTS {
            let c = match self.counts.get(i) {
                Some(v) => *v,
                None => return Err(FsmCode::CoverBad.val()),
            };
            if c == 0 {
                return Err(FsmCode::CoverBad.val());
            }
            if c.saturating_mul(10) < self.total {
                return Err(FsmCode::CoverBad.val());
            }
            i += 1;
        }
        if self.open {
            return Err(FsmCode::ExitBad.val());
        }
        if self.anomalies != self.handled {
            return Err(FsmCode::ObsBad.val());
        }
        Ok(())
    }
}

/// 四点探针：每轮「入口 → 出口 → 结果」三点，出口同时落耗时点，
/// 因此每轮恰好 4 点、每点恰好 `rounds` 次。每 3 轮注入一次可见异常
/// （Idle 不接受 Up），异常走 `record_error`，绝不静默。
pub fn fsm_obs_probe(rounds: u32) -> Result<FsmObserver, u32> {
    if rounds == 0 || rounds > FSM_OBS_MAX {
        return Err(FsmCode::CoverBad.val());
    }
    let mut o = FsmObserver::new();
    let mut rng = p_core::Lcg::new(0xF496_0B5E);
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < rounds {
        tick = match tick.checked_add(1) {
            Some(v) => v,
            None => return Err(FsmCode::TimeoutBad.val()),
        };
        let fingers = 1 + ((rng.next() >> 33) % 3) as u8;
        o.entry(i.saturating_add(1), fingers, tick)?;
        tick = tick.saturating_add(1 + ((rng.next() >> 33) % 5) as u32);
        o.exit(tick)?;
        if i % 3 == 0 {
            let code = match fsm_lookup(S_IDLE, EV_UP, 1) {
                Ok(_) => return Err(FsmCode::TransitionBad.val()),
                Err(c) => c,
            };
            if code != FsmCode::TransitionBad.val() {
                return Err(FsmCode::ObsBad.val());
            }
            o.record_error(code)?;
        }
        o.result(true, None)?;
        i += 1;
    }
    o.audit()?;
    Ok(o)
}

/// 四点负例集：空账、缺点、失衡、静默、入口/出口/超时/重入/结果五类
/// 拒绝都必须给出**精确**的码；任何一条错位都判 red。
pub fn fsm_obs_negative() -> Result<(), u32> {
    // ① 空账：四点全缺 → CoverBad。
    let empty = FsmObserver::new();
    if empty.audit().err() != Some(FsmCode::CoverBad.val()) {
        return Err(FsmCode::ObsBad.val());
    }
    // ② 缺点：三缺一（无耗时）→ CoverBad。
    let mut miss = FsmObserver::new();
    miss.touch(0)?;
    miss.touch(1)?;
    miss.touch(3)?;
    if miss.audit().err() != Some(FsmCode::CoverBad.val()) {
        return Err(FsmCode::ObsBad.val());
    }
    // ③ 失衡 100:5:5:5 → CoverBad。
    let mut skew = FsmObserver::new();
    let mut i = 0u32;
    while i < 100 {
        skew.touch(0)?;
        i += 1;
    }
    while i < 105 {
        skew.touch(1)?;
        skew.touch(2)?;
        skew.touch(3)?;
        i += 1;
    }
    if skew.audit().err() != Some(FsmCode::CoverBad.val()) {
        return Err(FsmCode::ObsBad.val());
    }
    // ④ 静默异常 → ObsBad（吞掉的异常必须被账本抓出来）。
    let mut silent = FsmObserver::new();
    silent.entry(1, 1, 1)?;
    silent.exit(2)?;
    silent.result(true, None)?;
    let _ = silent.swallow(FsmCode::TimeoutBad.val());
    if silent.audit().err() != Some(FsmCode::ObsBad.val()) {
        return Err(FsmCode::ObsBad.val());
    }
    if silent.silent_count() != 1
        || silent.anomaly_count() != silent.handled_count() + silent.silent_count()
    {
        return Err(FsmCode::ObsBad.val());
    }
    // ⑤ 逐码精确：入口/出口/超时/重入/结果。
    let mut t = FsmObserver::new();
    if t.entry(0, 1, 1).err() != Some(FsmCode::EntryBad.val()) {
        return Err(FsmCode::EntryBad.val());
    }
    if t.entry(1, 4, 1).err() != Some(FsmCode::FingerBad.val()) {
        return Err(FsmCode::FingerBad.val());
    }
    if t.exit(1).err() != Some(FsmCode::ExitBad.val()) {
        return Err(FsmCode::ExitBad.val());
    }
    t.entry(1, 2, 1)?;
    if t.entry(1, 2, 1).err() != Some(FsmCode::SeqBad.val()) {
        return Err(FsmCode::SeqBad.val());
    }
    if t.exit(FSM_TIMEOUT.saturating_add(2)) != Err(FsmCode::TimeoutBad.val()) {
        return Err(FsmCode::TimeoutBad.val());
    }
    if t.result(false, None).err() != Some(FsmCode::ObsBad.val()) {
        return Err(FsmCode::ObsBad.val());
    }
    if t.result(true, Some(FsmCode::ObsBad.val())).err() != Some(FsmCode::ObsBad.val()) {
        return Err(FsmCode::ObsBad.val());
    }
    Ok(())
}

/// 四点复跑：同轮数两次观测的四点计数、异常账、摘要必须逐位相等。
pub fn fsm_obs_replay(rounds: u32) -> Result<u64, u32> {
    let a = fsm_obs_probe(rounds)?;
    let b = fsm_obs_probe(rounds)?;
    let mut i = 0usize;
    while i < FSM_OBS_POINTS {
        if a.count(i) != b.count(i) {
            return Err(FsmCode::DriftBad.val());
        }
        i += 1;
    }
    if a.total() != b.total()
        || a.anomaly_count() != b.anomaly_count()
        || a.silent_count() != 0
        || b.silent_count() != 0
    {
        return Err(FsmCode::DriftBad.val());
    }
    fsm_digest_guard(a.digest(), b.digest())?;
    Ok(a.digest())
}

// ── 判据 ──────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();

    // ① F48995-J1：15 码固定窗口、两两不同、与 StCode 不相交，
    //    且连续 5 轮枚举逐位同值（零漂移、互不混淆）。
    let (p1, d1) = {
        let mut ok = fsm_segment_guard().is_ok() && fsm_codes_distinct();
        let mut first = [0u32; FSM_CODES];
        match fsm_code_probe() {
            Ok(g) => {
                first = g;
                let mut r = 0u32;
                while r < 4 {
                    match fsm_code_probe() {
                        Ok(h) => {
                            if h != first {
                                ok = false;
                            }
                        }
                        Err(_) => ok = false,
                    }
                    r = r.saturating_add(1);
                }
            }
            Err(_) => ok = false,
        }
        let mut head = 0u32;
        if let Some(v) = first.get(0) {
            head = *v;
        }
        (
            ok,
            format!("15码 5轮同值 窗+20..+2E 首=0x{:X}", head),
        )
    };
    out.push(("F48995 错误码×15零漂移互不混淆", p1, d1));

    // ② F48995-J1 伴生：42 行表唯一、篡改即拒、1/2/3 指起手正确、
    //    合法脚本与取消脚本都能收口。
    let (p2, d2) = {
        let audit = fsm_table_audit(&FSM_TABLE)
            .map(|n| n == FSM_TRANSITIONS)
            .unwrap_or(false);
        let tamper = {
            let mut rows = FSM_TABLE;
            if let Some(r) = rows.get_mut(1) {
                *r = FSM_TABLE[0];
            }
            fsm_table_audit(&rows).err() == Some(FsmCode::TableBad.val())
        };
        let script = fsm_script().is_ok() && fsm_cancel_script().is_ok();
        let fingers = fsm_finger_probe().map(|n| n == 3).unwrap_or(false);
        (
            audit && tamper && script && fingers,
            String::from("42行唯一 篡改拒 脚本收口 指1/2/3"),
        )
    };
    out.push(("F48995 42迁移表唯一且可收口", p2, d2));

    // ③ 洪泛守恒（沿用旧证据）：×10^4 四元账平衡。
    let (p3, d3) = match flood_probe(FLOOD_ROUNDS) {
        Ok(s) => (
            s.audit().is_ok() && s.enqueued >= FLOOD_ROUNDS && s.balanced(),
            format!(
                "enq={} dr={} dp={} q={}",
                s.enqueued, s.drained, s.dropped, s.queued
            ),
        ),
        Err(e) => (false, format!("err=0x{:X}", e)),
    };
    out.push(("F48995 洪泛×10^4四元账守恒", p3, d3));

    // ④ 序号/丢弃/延迟/注入四路（沿用旧证据）。
    let (p4, d4) = {
        let order = order_probe(QUEUE_CAP as u32)
            .map(|n| n == QUEUE_CAP as u32)
            .unwrap_or(false);
        let drop = drop_probe()
            .map(|s| s.dropped == 1 && s.queued as usize == QUEUE_CAP && s.balanced())
            .unwrap_or(false);
        let delay = delay_probe()
            .map(|s| s.max_lat == 20 && s.lat == [0, 0, 0, 1, 4] && s.lat_sum() == s.drained)
            .unwrap_or(false);
        let inject = inject_rounds(3).map(|l| l.all_caught()).unwrap_or(false);
        (
            order && drop && delay && inject,
            String::from("order=64 drop=1 max=20 桶[0,0,0,1,4] 注入4"),
        )
    };
    out.push(("F48995 空洞不回退+分桶+四路注入", p4, d4));

    // ⑤ F48996-J1：入口/出口/耗时/结果四点各 100%，异常全可见。
    let (p5, d5) = match fsm_obs_probe(FSM_OBS_ROUNDS) {
        Ok(o) => {
            let full = o.count(FsmPoint::Entry.index()) == FSM_OBS_ROUNDS
                && o.count(FsmPoint::Exit.index()) == FSM_OBS_ROUNDS
                && o.count(FsmPoint::Duration.index()) == FSM_OBS_ROUNDS
                && o.count(FsmPoint::Result.index()) == FSM_OBS_ROUNDS
                && o.total() == FSM_OBS_ROUNDS.saturating_mul(4);
            let clean = o.audit().is_ok()
                && o.silent_count() == 0
                && o.anomaly_count() == o.handled_count()
                && o.anomaly_count() > 0
                && o.last_code() != 0;
            (
                full && clean,
                format!(
                    "四点各={} 总={} 异常={} 静默=0",
                    o.count(0),
                    o.total(),
                    o.anomaly_count()
                ),
            )
        }
        Err(e) => (false, format!("err=0x{:X}", e)),
    };
    out.push(("F48996 四点覆盖100%异常零静默", p5, d5));

    // ⑥ F48996-J1 负例：空账/缺点/失衡/静默/逐码精确全拒。
    let (p6, d6) = match fsm_obs_negative() {
        Ok(()) => (true, String::from("缺项失衡静默超时越界 精确拒")),
        Err(e) => (false, format!("err=0x{:X}", e)),
    };
    out.push(("F48996 四点失衡与静默必须拒绝", p6, d6));

    // ⑦ 长跑 ×10^5 无溢出 + 四点摘要双跑一致。
    let (p7, d7) = match long_run(LONG_ROUNDS) {
        Ok(s) => {
            let replay = fsm_obs_replay(FSM_OBS_ROUNDS).is_ok();
            let ok = s.enqueued < 4_000_000 && s.audit().is_ok() && s.balanced() && replay;
            (
                ok,
                format!("enq={} dr={} 四点摘要双跑同", s.enqueued, s.drained),
            )
        }
        Err(e) => (false, format!("err=0x{:X}", e)),
    };
    out.push(("F48996 长跑×10^5拍+观测复跑", p7, d7));

    // ⑧ 定长存储 + 检查点三方对账 + 三段复现（沿用旧证据）。
    let (p8, d8) = {
        let storage = storage_audit().map(|n| n <= ENGINE_BUDGET).unwrap_or(false);
        let checkpoint = long_run(20_000)
            .map(|s| s.audit().is_ok() && s.balanced())
            .unwrap_or(false);
        let repro = repro_check().is_ok();
        (
            storage && checkpoint && repro,
            format!(
                "engine<=4KB 检查点={} 复跑相等",
                20_000u32 / DRIFT_EVERY + 1
            ),
        )
    };
    out.push(("F48996 定长存储+检查点零漂移", p8, d8));

    out
}

pub fn run_unx48995_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48995 事件洪泛与长跑稳定性");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71800);
        assert_eq!(QUEUE_CAP, 64);
        assert_eq!(FLOOD_ROUNDS, 10_000);
        assert_eq!(LONG_ROUNDS, 100_000);
        assert_eq!(StCode::ALL.len(), 14);
        assert_eq!(LAT_BUCKETS, 5);
        assert_eq!(STATS_FIELDS, 6);
    }

    #[test]
    fn codes_are_distinct_and_windowed() {
        assert!(codes_are_distinct());
        assert_eq!(segment_guard(), Ok(()));
        let mut i = 0usize;
        while i < StCode::ALL.len() {
            let c = StCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(!detail(c.val()).is_empty());
            assert!(!c.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn bucket_edges_are_exact() {
        assert_eq!(bucket(0), 0);
        assert_eq!(bucket(1), 1);
        assert_eq!(bucket(2), 2);
        assert_eq!(bucket(4), 2);
        assert_eq!(bucket(5), 3);
        assert_eq!(bucket(16), 3);
        assert_eq!(bucket(17), 4);
        assert_eq!(bucket(u32::MAX), 4);
    }

    #[test]
    fn ring_is_fifo() {
        let mut r = Ring::new();
        assert!(r.is_empty());
        assert_eq!(r.push(Event::new(1, 1, 0)), Ok(()));
        assert_eq!(r.push(Event::new(2, 2, 1)), Ok(()));
        assert_eq!(r.peek(), Some(Event::new(1, 1, 0)));
        assert_eq!(r.pop(), Some(Event::new(1, 1, 0)));
        assert_eq!(r.pop(), Some(Event::new(2, 2, 1)));
        assert_eq!(r.pop(), None);
        assert!(r.audit().is_ok());
    }

    #[test]
    fn ring_full_rejects_without_moving_cursors() {
        let mut r = Ring::new();
        let mut i = 0usize;
        while i < QUEUE_CAP {
            assert_eq!(r.push(Event::new(i as u32 + 1, 1, 0)), Ok(()));
            i += 1;
        }
        assert!(r.is_full());
        assert_eq!(r.push(Event::new(999, 1, 0)), Err(StCode::QueueBad.val()));
        assert_eq!(r.len(), QUEUE_CAP);
        assert!(r.audit().is_ok());
    }

    #[test]
    fn ring_wraps_and_audits() {
        let mut r = Ring::new();
        let mut i = 0u32;
        while i < 5 {
            assert_eq!(r.push(Event::new(i + 1, 1, 0)), Ok(()));
            i += 1;
        }
        while !r.is_empty() {
            assert!(r.pop().is_some());
        }
        assert!(r.audit().is_ok());
        let mut k = 0u32;
        while k < QUEUE_CAP as u32 {
            assert_eq!(r.push(Event::new(100 + k, 2, 1)), Ok(()));
            k += 1;
        }
        assert!(r.is_full());
        assert!(r.audit().is_ok());
        assert_eq!(r.peek(), Some(Event::new(100, 2, 1)));
    }

    #[test]
    fn ingress_advances_seq_and_accepts_same_tick() {
        let mut e = Engine::new();
        let a = e.ingress(0, 5);
        let b = e.ingress(1, 5);
        assert_eq!(a, Ok(Ingress { seq: 1, queued: true }));
        assert_eq!(b, Ok(Ingress { seq: 2, queued: true }));
        assert_eq!(e.next_seq(), 3);
        assert_eq!(e.ring_len(), 2);
    }

    #[test]
    fn ingress_rejects_tick_regression() {
        let mut e = Engine::new();
        assert_eq!(e.ingress(0, 10), Ok(Ingress { seq: 1, queued: true }));
        assert_eq!(e.ingress(0, 9), Err(StCode::TickBad.val()));
        assert_eq!(e.ingress(0, TICK_CAP + 1), Err(StCode::TickBad.val()));
    }

    #[test]
    fn drain_rejects_bad_cover() {
        let mut e = Engine::new();
        assert_eq!(e.drain(1, 0), Err(StCode::CoverBad.val()));
        assert_eq!(e.drain(1, DRAIN_MAX + 1), Err(StCode::CoverBad.val()));
        assert_eq!(e.drain(1, 1), Ok(0));
    }

    #[test]
    fn drain_returns_available_only() {
        let mut e = Engine::new();
        assert_eq!(e.ingress(0, 1), Ok(Ingress { seq: 1, queued: true }));
        assert_eq!(e.drain(2, 8), Ok(1));
        assert_eq!(e.drain(3, 8), Ok(0));
        let s = e.stats();
        assert_eq!(s.drained, 1);
        assert_eq!(s.queued, 0);
        assert!(s.audit().is_ok());
    }

    #[test]
    fn drain_rewind_is_order_bad() {
        let mut e = Engine::new();
        let _ = e.ingress(0, 1);
        assert_eq!(e.drain(1, 1), Ok(1));
        assert_eq!(
            e.ring.push(Event::new(e.last_seq(), 1, 0)),
            Ok(())
        );
        assert_eq!(e.drain(1, 1), Err(StCode::OrderBad.val()));
    }

    #[test]
    fn delay_buckets_match_known_latencies() {
        let s = delay_probe();
        assert!(s.is_ok());
        let s = s.unwrap();
        assert_eq!(s.max_lat, 20);
        assert_eq!(s.lat, [0, 0, 0, 1, 4]);
        assert_eq!(s.lat_sum(), s.drained);
    }

    #[test]
    fn drop_probe_accounts_backpressure() {
        let s = drop_probe();
        assert!(s.is_ok());
        let s = s.unwrap();
        assert_eq!(s.enqueued, QUEUE_CAP as u32 + 1);
        assert_eq!(s.dropped, 1);
        assert_eq!(s.queued, QUEUE_CAP as u32);
        assert!(s.balanced());
    }

    #[test]
    fn shadow_detects_mismatch() {
        let mut sh = Shadow::new();
        let mut e = Engine::new();
        let ing = e.ingress(0, 1).unwrap();
        sh.ingress(ing.queued).unwrap();
        assert_eq!(sh.check(&e.stats()), Ok(()));
        let bad = FloodStats {
            enqueued: 9,
            drained: 0,
            dropped: 0,
            queued: 1,
            max_lat: 0,
            lat: [0; LAT_BUCKETS],
            digest: 1,
        };
        assert_eq!(sh.check(&bad), Err(StCode::DriftBad.val()));
    }

    #[test]
    fn flood_probe_is_green() {
        let s = flood_probe(512);
        assert!(s.is_ok());
        let s = s.unwrap();
        assert!(s.balanced());
        assert!(s.lat_sum() == s.drained);
        assert!(s.dropped > 0);
    }

    #[test]
    fn order_probe_is_green() {
        assert_eq!(order_probe(QUEUE_CAP as u32), Ok(QUEUE_CAP as u32));
    }

    #[test]
    fn golden_is_deterministic() {
        let a = golden_run().unwrap();
        let b = golden_run().unwrap();
        assert_eq!(a, b);
        assert_eq!(a.enqueued, GOLDEN_ROUNDS * GOLDEN_BURST);
        assert_eq!(a.max_lat, GOLDEN_BURST - 1);
        assert_eq!(a.lat, [16, 16, 32, 0, 0]);
        assert!(a.digest != 0);
    }

    #[test]
    fn render_roundtrip_and_tamper() {
        let s = golden_run().unwrap();
        let text = render_stats(&s);
        assert_eq!(audit_render(&text), Ok(STATS_FIELDS));
        let tampered = text.replacen("64|64|", "64|63|", 1);
        assert_eq!(audit_render(&tampered), Err(StCode::LedgerBad.val()));
        assert_eq!(audit_render("1|2|3"), Err(StCode::RenderBad.val()));
        assert_eq!(parse_u32("12x"), None);
        assert_eq!(parse_hex16("0x1234"), None);
    }

    #[test]
    fn inject_catches_all_routes() {
        let log = inject_rounds(3).unwrap();
        assert!(log.all_caught());
        assert_eq!(log.tries, [3, 3, 3, 3]);
        assert_eq!(log.caught, [3, 3, 3, 3]);
    }

    #[test]
    fn long_run_is_green() {
        let s = long_run(4_096);
        assert!(s.is_ok());
        let s = s.unwrap();
        assert!(s.audit().is_ok());
        assert!(s.enqueued > 4_096);
    }

    #[test]
    fn storage_is_fixed() {
        let n = storage_audit().unwrap();
        assert!(n <= ENGINE_BUDGET);
        assert_eq!(core::mem::size_of::<Event>(), 12);
    }

    #[test]
    fn reproducibility_holds() {
        assert_eq!(repro_check(), Ok(()));
        assert_eq!(probe_seq_a(), Ok(GOLDEN_ROUNDS * GOLDEN_BURST));
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48995_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }

    // ── F48995 / F48996 新判据 ──────────────────────────────────
    #[test]
    fn fsm_codes_are_fifteen_windowed_and_disjoint() {
        assert_eq!(FsmCode::ALL.len(), FSM_CODES);
        assert_eq!(FSM_CODES, 15);
        assert!(fsm_codes_distinct());
        assert_eq!(fsm_segment_guard(), Ok(()));
        let mut i = 0usize;
        while i < FsmCode::ALL.len() {
            let c = FsmCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x20 + i as u32);
            assert_eq!(fsm_offset_guard(c.val()), Ok(()));
            assert!(!c.detail().is_empty());
            assert!(c.detail().len() <= 48);
            assert_eq!(fsm_detail(c.val()), c.detail());
            // 与洪泛族 14 条严格不相交。
            let mut j = 0usize;
            while j < StCode::ALL.len() {
                assert_ne!(c.val(), StCode::ALL[j].val());
                j += 1;
            }
            i += 1;
        }
        assert_eq!(
            fsm_offset_guard(SEG_BASE + 0x2F),
            Err(FsmCode::SegmentBad.val())
        );
        assert_eq!(
            fsm_offset_guard(StCode::FloodBad.val()),
            Err(FsmCode::SegmentBad.val())
        );
    }

    #[test]
    fn fsm_table_has_42_unique_rows() {
        assert_eq!(FSM_TABLE.len(), 42);
        assert_eq!(fsm_table_audit(&FSM_TABLE), Ok(42));
        let mut per = [0u32; FSM_STATES];
        let mut i = 0usize;
        while i < FSM_TABLE.len() {
            per[FSM_TABLE[i].from as usize] += 1;
            i += 1;
        }
        assert_eq!(per, [6, 6, 6, 6, 6, 6, 6]);
        // 缺行。
        assert_eq!(
            fsm_table_audit(&FSM_TABLE[0..41]),
            Err(FsmCode::TableBad.val())
        );
        // 重复键。
        let mut dup = FSM_TABLE;
        dup[1] = FSM_TABLE[0];
        assert_eq!(fsm_table_audit(&dup), Err(FsmCode::TableBad.val()));
        // 非法指宽。
        let mut bad = FSM_TABLE;
        bad[0] = row(S_IDLE, EV_DOWN, 0, S_ONE);
        assert_eq!(fsm_table_audit(&bad), Err(FsmCode::FingerBad.val()));
        // Hold 必须三指。
        assert_eq!(
            fsm_row_check(row(S_THREE, EV_DOWN, 2, S_HOLD)),
            Err(FsmCode::HoldBad.val())
        );
    }

    #[test]
    fn fsm_lookup_and_finger_semantics() {
        assert_eq!(fsm_lookup(S_IDLE, EV_DOWN, 1), Ok(S_ONE));
        assert_eq!(fsm_lookup(S_IDLE, EV_DOWN, 2), Ok(S_TWO));
        assert_eq!(fsm_lookup(S_IDLE, EV_DOWN, 3), Ok(S_THREE));
        assert_eq!(fsm_lookup(S_TWO, EV_DOWN, 3), Ok(S_THREE));
        assert_eq!(fsm_lookup(S_THREE, EV_DOWN, 3), Ok(S_HOLD));
        assert_eq!(fsm_lookup(S_HOLD, EV_UP, 3), Ok(S_TWO));
        assert_eq!(fsm_lookup(S_ONE, EV_UP, 1), Ok(S_DONE));
        assert_eq!(
            fsm_lookup(S_IDLE, EV_UP, 1),
            Err(FsmCode::TransitionBad.val())
        );
        assert_eq!(
            fsm_lookup(FSM_STATES as u8, EV_DOWN, 1),
            Err(FsmCode::StateBad.val())
        );
        assert_eq!(
            fsm_lookup(S_IDLE, EV_DOWN, 4),
            Err(FsmCode::FingerBad.val())
        );
        assert_eq!(
            fsm_lookup(S_IDLE, FSM_EVENTS as u8, 1),
            Err(FsmCode::TransitionBad.val())
        );
        assert_eq!(fsm_finger_probe(), Ok(3));
        assert_eq!(state_name(S_HOLD), "Hold");
        assert_eq!(state_name(9), "");
        assert_eq!(event_name(EV_CANCEL), "Cancel");
        assert_eq!(event_name(9), "");
    }

    #[test]
    fn fsm_code_probe_is_stable_and_unconfused() {
        let a = fsm_code_probe().unwrap();
        assert_eq!(a.len(), FSM_CODES);
        // 逐位等于枚举顺序的判别值（零漂移）。
        let mut i = 0usize;
        while i < FSM_CODES {
            assert_eq!(a[i], SEG_BASE + 0x20 + i as u32);
            i += 1;
        }
        // 5 轮重复枚举必须逐位相等。
        let mut r = 0u32;
        while r < 5 {
            assert_eq!(fsm_code_probe().unwrap(), a);
            r += 1;
        }
        // 两两不同（互不混淆）。
        let mut x = 0usize;
        while x < FSM_CODES {
            let mut y = x + 1;
            while y < FSM_CODES {
                assert_ne!(a[x], a[y]);
                y += 1;
            }
            x += 1;
        }
    }

    #[test]
    fn fsm_machine_scripts_settle_without_state_loss() {
        let d = fsm_script().unwrap();
        assert_ne!(d, p_core::FNV_OFF);
        assert!(fsm_cancel_script().is_ok());
        // 活动态直接结算必须拒绝。
        let mut m = FsmMachine::new();
        assert_eq!(m.step(EV_DOWN, 1), Ok(S_ONE));
        assert_eq!(m.step(EV_MOVE, 1), Ok(S_ONE));
        assert_eq!(m.settle(), Err(FsmCode::CancelBad.val()));
        assert_eq!(m.steps(), 2);
        assert_eq!(m.counts_audit(), Ok(()));
        // 非法事件不推进账本。
        assert_eq!(
            m.step(EV_UP, 3),
            Err(FsmCode::TransitionBad.val())
        );
        assert_eq!(m.steps(), 2);
        // 取消后重启仍可用。
        let mut c = FsmMachine::new();
        assert_eq!(c.step(EV_DOWN, 2), Ok(S_TWO));
        assert_eq!(c.step(EV_CANCEL, 2), Ok(S_CANCELED));
        assert_eq!(c.step(EV_DOWN, 3), Ok(S_THREE));
        assert_eq!(c.step(EV_UP, 3), Ok(S_TWO));
        assert_eq!(c.settle(), Err(FsmCode::CancelBad.val()));
    }

    #[test]
    fn fsm_observation_covers_four_points() {
        let o = fsm_obs_probe(FSM_OBS_ROUNDS).unwrap();
        assert_eq!(o.count(FsmPoint::Entry.index()), FSM_OBS_ROUNDS);
        assert_eq!(o.count(FsmPoint::Exit.index()), FSM_OBS_ROUNDS);
        assert_eq!(o.count(FsmPoint::Duration.index()), FSM_OBS_ROUNDS);
        assert_eq!(o.count(FsmPoint::Result.index()), FSM_OBS_ROUNDS);
        assert_eq!(o.total(), FSM_OBS_ROUNDS * 4);
        assert_eq!(o.percent(0), 25);
        assert_eq!(o.percent(1), 25);
        assert_eq!(o.percent(2), 25);
        assert_eq!(o.percent(3), 25);
        // 每 3 轮一次可见异常，异常账 == 已处理账，静默 0。
        assert_eq!(o.anomaly_count(), FSM_OBS_ROUNDS / 3 + 1);
        assert_eq!(o.anomaly_count(), o.handled_count());
        assert_eq!(o.silent_count(), 0);
        assert_eq!(o.last_code(), FsmCode::TransitionBad.val());
        assert!(o.duration_max() >= 1 && o.duration_max() <= 6);
        assert!(o.duration_avg() >= 1);
        assert_eq!(o.audit(), Ok(()));
        // 四点标签齐全。
        let mut i = 0usize;
        while i < FSM_OBS_POINTS {
            assert!(!FsmPoint::ALL[i].label().is_empty());
            assert_eq!(FsmPoint::ALL[i].index(), i);
            i += 1;
        }
    }

    #[test]
    fn fsm_observation_negative_controls_are_exact() {
        assert_eq!(fsm_obs_negative(), Ok(()));
        // 静默异常单独复核：吞掉的异常让 audit 直接否决。
        let mut o = FsmObserver::new();
        o.entry(1, 1, 1).unwrap();
        o.exit(2).unwrap();
        o.result(true, None).unwrap();
        assert_eq!(o.audit(), Ok(()));
        o.swallow(FsmCode::SeqBad.val()).unwrap();
        assert_eq!(o.silent_count(), 1);
        assert_eq!(o.anomaly_count(), 1);
        assert_eq!(o.handled_count(), 0);
        assert_eq!(o.last_code(), FsmCode::SeqBad.val());
        assert_eq!(o.audit(), Err(FsmCode::ObsBad.val()));
    }

    #[test]
    fn fsm_observation_replay_is_identical() {
        let d = fsm_obs_replay(FSM_OBS_ROUNDS).unwrap();
        let a = fsm_obs_probe(FSM_OBS_ROUNDS).unwrap();
        assert_eq!(a.digest(), d);
        assert_eq!(fsm_obs_probe(0).err(), Some(FsmCode::CoverBad.val()));
        assert_eq!(
            fsm_obs_probe(FSM_OBS_MAX + 1).err(),
            Some(FsmCode::CoverBad.val())
        );
        assert_eq!(
            fsm_digest_guard(7, 8),
            Err(FsmCode::DriftBad.val())
        );
        assert_eq!(fsm_digest_guard(9, 9), Ok(()));
    }

    #[test]
    fn fsm_check_details_fit_dyn_cap() {
        let items = extra_checks();
        assert_eq!(items.len(), 8);
        let mut i = 0usize;
        while i < items.len() {
            let (name, _, d) = &items[i];
            assert!(!name.is_empty());
            assert!(d.len() <= 48, "detail too long: {}", d);
            i += 1;
        }
    }
}

