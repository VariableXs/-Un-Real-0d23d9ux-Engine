//! mech_sim — 确定性离散事件仿真底盘（AI-K1 深化批次四 · 共用件）。
//!
//! 主册依据（为什么这个件必须存在）：
//! - F041「帧率图与实测录屏**逐帧对得上**」、F047「压力混载……p99 仍达标」、
//!   F050「flood 测试 70 级零丢弃」、F057「24h 混载测试全部完成」——这些判据
//!   的宿主侧形态都是「**可复现的确定性时间线**」：同一串输入跑两遍必须逐
//!   事件一致，否则「全绿」不可信（巧合绿不是绿）。
//! - 分工书通用十二查第 10 查「证据三件套」：仿真摘要 + 复现命令 + 日期——
//!   摘要的可复现性由本件的 **replay digest** 保证（同种子同事件流 → 同摘要）。
//!
//! 与他件的关系（零冗余声明）：
//! - 各域既有检查项里的"模拟"（如 iotier 的 24h 混载、intrcoal 的 flood）是
//!   域内专用循环；本件提供的是**跨域共用的事件队列/时钟/PRNG/摘要**底盘，
//!   把「确定性」从各域自觉升级为底盘保证。已有域内循环不回改（收口纪律），
//!   批次四起新建的仿真一律走本件。
//!
//! 零堆纪律：事件队列与全部状态定长数组；PRNG 为 xorshift64*（确定性、
//! 无外部熵）；无浮点（时间与预算一律整数）。

// ---------------------------------------------------------------------------
// 1. XorShift64* 伪随机数（确定性）
// ---------------------------------------------------------------------------

/// xorshift64* 乘法常数（Vigna 2014，全周期）。
const XORSHIFT_MUL: u64 = 0x2545_F491_4F6C_DD1D;

/// 确定性伪随机源。种子为 0 时按规范修正为非零（xorshift 吸收态防御）。
#[derive(Clone, Copy, Debug)]
pub struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    /// 用任意种子构造（0 会被替换为黄金常数——不静默产出全零序列）。
    pub const fn new(seed: u64) -> Self {
        XorShift64 { state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed } }
    }

    /// 下一个 64 位随机数（xorshift64*：移位 → 乘法，单步确定性）。
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(XORSHIFT_MUL)
    }

    /// [0, bound) 内均匀整数（拒绝采样——模偏差不进证据链）。
    /// bound == 0 返回 0（调用侧契约：无界取样无意义，如实给 0 不 panic）。
    pub fn next_below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        // 拒绝阈值：2^64 折叠到 bound 的余数区。
        let zone = u64::MAX - (u64::MAX % bound);
        loop {
            let v = self.next_u64();
            if v < zone {
                return v % bound;
            }
        }
    }

    /// permille 概率判定（true 概率 ≈ permille/1000）——千分位是全域口径
    /// （F042 shares_permille / F056 dirty_permille 同源），不引入浮点。
    pub fn permille_chance(&mut self, permille: u32) -> bool {
        self.next_below(1000) < permille as u64
    }
}

// ---------------------------------------------------------------------------
// 2. 虚拟时钟与事件
// ---------------------------------------------------------------------------

/// 单位：微秒（全域时间口径：F041 busy_us / F047 预算 μs / F050 窗口 ns 换算
/// 后统一 μs 落账；ns 级判据由各域自行换算）。
pub type Ts = u64;

/// 事件：时刻 + 域内标签 + 载荷（载荷语义由消费域定义——底盘不做解释）。
#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub at_us: Ts,
    /// 域内事件类别（如 F050 的设备键、F057 的队列类）。
    pub kind: u16,
    /// 数值载荷一。
    pub a: u32,
    /// 数值载荷二。
    pub b: u32,
    /// 同时刻事件的全序序号（确定性保证：同刻事件按插入序稳定展开）。
    seq: u64,
}

/// 事件队列容量：flood 70 级 × 每级一批 + 混载双流余量——72 条。
pub const EVENT_CAP: usize = 72;

/// 定长最小堆事件队列（键 = (at_us, seq)）。满时 `push` 返回 false——
/// **诚实拒绝**，不静默覆盖（零静默纪律）。
pub struct EventQueue {
    heap: [Option<Event>; EVENT_CAP],
    len: usize,
    seq_counter: u64,
    /// 因满被拒绝的次数（证据链口径：拒绝必须可问、可对账）。
    pub rejected: u32,
}

impl EventQueue {
    pub const fn new() -> Self {
        EventQueue { heap: [None; EVENT_CAP], len: 0, seq_counter: 0, rejected: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn less(a: &Event, b: &Event) -> bool {
        (a.at_us, a.seq) < (b.at_us, b.seq)
    }

    fn swap(heap: &mut [Option<Event>; EVENT_CAP], i: usize, j: usize) {
        let t = heap[i];
        heap[i] = heap[j];
        heap[j] = t;
    }

    fn sift_up(&mut self, mut i: usize) {
        while i > 0 {
            let p = (i - 1) / 2;
            let (ok_i, ok_p) = (self.heap[i].take().unwrap(), self.heap[p].take().unwrap());
            if Self::less(&ok_i, &ok_p) {
                self.heap[i] = Some(ok_p);
                self.heap[p] = Some(ok_i);
                i = p;
            } else {
                self.heap[i] = Some(ok_i);
                self.heap[p] = Some(ok_p);
                break;
            }
        }
    }

    fn sift_down(&mut self, mut i: usize) {
        loop {
            let l = 2 * i + 1;
            let r = 2 * i + 2;
            let mut m = i;
            if l < self.len {
                let ok_m = self.heap[m].unwrap();
                let ok_l = self.heap[l].unwrap();
                if Self::less(&ok_l, &ok_m) {
                    m = l;
                }
            }
            if r < self.len {
                let ok_m = self.heap[m].unwrap();
                let ok_r = self.heap[r].unwrap();
                if Self::less(&ok_r, &ok_m) {
                    m = r;
                }
            }
            if m == i {
                break;
            }
            Self::swap(&mut self.heap, i, m);
            i = m;
        }
    }

    /// 入队（满则拒绝并计数——调用侧必须处理 false，不替它吞）。
    pub fn push(&mut self, at_us: Ts, kind: u16, a: u32, b: u32) -> bool {
        if self.len == EVENT_CAP {
            self.rejected += 1;
            return false;
        }
        let e = Event { at_us, kind, a, b, seq: self.seq_counter };
        self.seq_counter += 1;
        self.heap[self.len] = Some(e);
        self.sift_up(self.len);
        self.len += 1;
        true
    }

    /// 弹出最早（同刻按序）事件。
    pub fn pop(&mut self) -> Option<Event> {
        if self.len == 0 {
            return None;
        }
        let top = self.heap[0].take().unwrap();
        self.len -= 1;
        if self.len > 0 {
            self.heap[0] = self.heap[self.len].take();
            self.sift_down(0);
        }
        Some(top)
    }

    /// 只读窥视（不改序——调度器「看一眼下一个事件再决定」的合法入口）。
    pub fn peek(&self) -> Option<&Event> {
        self.heap[0].as_ref()
    }
}

/// 虚拟时钟：只被「取出的事件」推进——仿真世界里没有并发不定性。
#[derive(Clone, Copy, Debug)]
pub struct SimClock {
    now_us: Ts,
}

impl SimClock {
    pub const fn new() -> Self {
        SimClock { now_us: 0 }
    }
    pub fn now_us(&self) -> Ts {
        self.now_us
    }
    /// 推进到事件时刻（只许前进：时间回拨是仿真世界里的错误输入，
    /// 如实拒绝——与 F182 回拨保护同语义）。
    pub fn advance_to(&mut self, at_us: Ts) -> Result<(), ()> {
        if at_us < self.now_us {
            return Err(());
        }
        self.now_us = at_us;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 3. 确定性回放摘要（FNV-1a）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基与素数。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 回放摘要器：把事件流折叠成 64 位指纹。**同种子同输入 → 同摘要**是
/// 「确定性」的判据本体——验收时两跑摘要相等即可断言逐事件一致。
#[derive(Clone, Copy, Debug)]
pub struct ReplayDigest {
    h: u64,
    pub events_fed: u64,
}

impl ReplayDigest {
    pub const fn new() -> Self {
        ReplayDigest { h: FNV_OFFSET, events_fed: 0 }
    }

    pub fn feed(&mut self, e: &Event) {
        for v in [e.at_us, e.seq, e.kind as u64, e.a as u64, e.b as u64] {
            for shift in [0u32, 8, 16, 24, 32, 40, 48, 56] {
                self.h ^= (v >> shift) as u8 as u64;
                self.h = self.h.wrapping_mul(FNV_PRIME);
            }
        }
        self.events_fed += 1;
    }

    pub fn digest(&self) -> u64 {
        self.h
    }
}

// ---------------------------------------------------------------------------
// 4. 仿真驱动器：队列 + 时钟 + 摘要三合一
// ---------------------------------------------------------------------------

/// 把事件队列抽干：逐事件推进时钟、喂摘要、调 `step` 回调。
/// 返回处理的事件数。`step` 的语义完全由消费域定义（底盘不越权）。
pub fn drain<F: FnMut(&Event)>(q: &mut EventQueue, clock: &mut SimClock, dg: &mut ReplayDigest, mut step: F) -> usize {
    let mut n = 0usize;
    while let Some(e) = q.pop() {
        // 时钟只前进：事件时刻早于当前时钟是生成侧 bug，如实吞入（摘要仍
        // 记录，判定由消费域做——底盘不静默修正数据）。
        let _ = clock.advance_to(e.at_us);
        dg.feed(&e);
        step(&e);
        n += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// CheckSet（挂 F041——账本是 B 域共同前提，确定性底盘随之注册）
// ---------------------------------------------------------------------------

/// 运行检查项（判据锚点见对账表批次四段）。
use crate::checks::CheckSet;

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F041-mech-sim");
    // 1) 事件队列稳定序（同刻按插入序展开）。
    let mut q = EventQueue::new();
    q.push(100, 1, 0, 0);
    q.push(50, 2, 0, 0);
    q.push(100, 3, 0, 0);
    let e1 = q.pop().unwrap();
    let e2 = q.pop().unwrap();
    let e3 = q.pop().unwrap();
    cs.add(
        "eq_stable_order",
        (e1.at_us, e1.kind) == (50, 2) && (e2.at_us, e2.kind) == (100, 1) && (e3.at_us, e3.kind) == (100, 3),
        "",
    );
    // 2) 回放摘要确定性：同种子两跑摘要相等、异种子不等。
    let run = |seed: u64| -> u64 {
        let mut r = XorShift64::new(seed);
        let mut q = EventQueue::new();
        let mut dg = ReplayDigest::new();
        for i in 0..60u64 {
            let dt = r.next_below(500);
            q.push(i * 100 + dt, (r.next_below(3)) as u16, i as u32, dt as u32);
        }
        let mut clock = SimClock::new();
        drain(&mut q, &mut clock, &mut dg, |_| {});
        dg.digest()
    };
    cs.add("digest_reproducible", run(2026) == run(2026) && run(2026) != run(2027), "");
    // 3) 时钟不回拨（F182 同语义）。
    let mut c = SimClock::new();
    c.advance_to(1000).unwrap();
    cs.add("clock_no_time_travel", c.advance_to(999).is_err() && c.advance_to(1000).is_ok(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift_is_deterministic_and_nonzero() {
        let mut a = XorShift64::new(42);
        let mut b = XorShift64::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut c = XorShift64::new(42);
        assert_ne!(c.next_u64(), 0);
        // 种子 0 修正为非零序列。
        let mut z = XorShift64::new(0);
        assert_ne!(z.next_u64(), 0);
    }

    #[test]
    fn next_below_rejects_bias() {
        let mut r = XorShift64::new(7);
        // 小 bound 下大量采样，值域必须严格落在 [0, bound)。
        for _ in 0..10000 {
            let v = r.next_below(17);
            assert!(v < 17);
        }
        // permille 判定：0‰ 恒假、1000‰ 恒真。
        assert!(!r.permille_chance(0));
        assert!(r.permille_chance(1000));
    }

    #[test]
    fn event_queue_is_stable_min_heap() {
        let mut q = EventQueue::new();
        // 乱序入队：同刻事件必须按插入序（seq）展开。
        q.push(100, 1, 0, 0);
        q.push(50, 2, 0, 0);
        q.push(100, 3, 0, 0);
        q.push(50, 4, 0, 0);
        let e1 = q.pop().unwrap();
        let e2 = q.pop().unwrap();
        let e3 = q.pop().unwrap();
        let e4 = q.pop().unwrap();
        assert_eq!((e1.at_us, e1.kind), (50, 2));
        assert_eq!((e2.at_us, e2.kind), (50, 4));
        assert_eq!((e3.at_us, e3.kind), (100, 1));
        assert_eq!((e4.at_us, e4.kind), (100, 3));
        // 空队列弹出 None，不 panic。
        assert!(q.pop().is_none());
    }

    #[test]
    fn event_queue_full_is_honest() {
        let mut q = EventQueue::new();
        for i in 0..EVENT_CAP {
            assert!(q.push(i as u64, 0, 0, 0));
        }
        assert!(!q.push(0, 0, 0, 0));
        assert_eq!(q.rejected, 1);
        assert_eq!(q.len(), EVENT_CAP);
    }

    #[test]
    fn clock_rejects_time_travel() {
        let mut c = SimClock::new();
        c.advance_to(1000).unwrap();
        assert_eq!(c.now_us(), 1000);
        assert!(c.advance_to(999).is_err());
        assert!(c.advance_to(1000).is_ok()); // 同刻合法
    }

    #[test]
    fn replay_digest_is_reproducible() {
        // 同种子两跑：逐事件一致 → 摘要必须相等。这是「确定性」的验收本体。
        // 事件数 60 < EVENT_CAP(72)：判例容量内全收（push 失败即静默丢事件，
        // 会破坏 n1==N 断言——容量上界是容器语义，判例不越界）。
        let run = |seed: u64| -> (u64, u64) {
            let mut r = XorShift64::new(seed);
            let mut q = EventQueue::new();
            for i in 0..60u64 {
                let dt = r.next_below(500);
                q.push(i * 100 + dt, (r.next_below(3)) as u16, i as u32, dt as u32);
            }
            let mut clock = SimClock::new();
            let mut dg = ReplayDigest::new();
            let n = drain(&mut q, &mut clock, &mut dg, |_| {});
            (dg.digest(), n as u64)
        };
        let (d1, n1) = run(2026);
        let (d2, n2) = run(2026);
        assert_eq!(d1, d2);
        assert_eq!(n1, n2);
        assert_eq!(n1, 60);
        // 不同种子 → 摘要必须不同（摘要不是常数——那才叫指纹）。
        let (d3, _) = run(2027);
        assert_ne!(d1, d3);
    }

    #[test]
    fn digest_changes_when_any_field_changes() {
        let base = Event { at_us: 1, kind: 2, a: 3, b: 4, seq: 5 };
        let variants = [
            Event { at_us: 2, ..base },
            Event { kind: 3, ..base },
            Event { a: 9, ..base },
            Event { b: 9, ..base },
            Event { seq: 6, ..base },
        ];
        let mut d0 = ReplayDigest::new();
        d0.feed(&base);
        for v in variants {
            let mut d = ReplayDigest::new();
            d.feed(&v);
            assert_ne!(d.digest(), d0.digest(), "任一字段变化都必须改变摘要");
        }
    }
}
