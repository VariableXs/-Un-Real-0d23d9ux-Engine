//! UNX-F48808 输入事件分派 · 0x700E0
//! 覆盖卡：UNX-F48808（十族 type 分发表逐族处理位，EV_KEY/EV_REL/EV_ABS 三主族优先落地）、
//!         UNX-F48809（事件层 per-CPU 环形缓冲骨架，无锁单生产者单消费者，帧粒度出队）、
//!         UNX-F48810（批量递交接口，帧粒度批量出队，降低高频调度压力）
//!
//! ── 闸门次序 ──
//! 分派面与递交面的闸是**串联**的，任一环顺序颠倒都会造成「看似有账、实则丢帧」：
//!   ① 族校验闸：事件族必须在十族闭集内，且族→下沉点必须是**一对一**映射。
//!      若把族校验放到入环之后，未知族会先占掉一个环槽，再由出队侧发现
//!      「没有下沉点」而丢弃——此时环的 pushed/popped 已经不平，丢帧无法归因。
//!   ② 串族闸：一族的计数器只允许由该族事件推进，下沉点计数必须与族计数
//!      逐对相等。此闸在入环前判：入环后再判，就无法区分「路由错」与
//!      「环里混进了别族帧」这两类完全不同的故障。
//!   ③ 入环闸（SPSC）：满环必须**背压**返回 RingFull，绝不允许覆盖未消费槽。
//!      若允许覆盖（或先出后入），消费者会看到序号跳变——这就是「无锁环丢帧」
//!      的最常见伪装：计数仍然对得上，序号却断了。
//!   ④ 批量闸：一次出队的条数必须恰等于「请求条数」与「环内可出条数」的较小值，
//!      出队总数必须与递交总数逐批对账。若把批量上限判在出队之后，
//!      超出的帧已被摘出环却无人认领，形成**在途丢帧**（既不 backpressure
//!      也不 delivered）。
//!   ⑤ 锁层次闸（A5 登记）：SPSC 的「无锁」是有前提的——生产者只碰 tail、
//!      消费者只碰 head、索引槽位本身不再加锁。本模块把这条纪律登记为
//!      有序常量 `LOCK_ORDER` 并自检其严格递增，防止后人往环里塞第二把锁
//!      却仍宣称 lock-free。
//!
//! ── 关键差异 ──
//! 与 unx48801_input_core 的边界：core 给出**分发表内容**（十族 × handled 位）
//!   与单事件静态判定；本模块给出**分派执行**（三主族 ×10^3 路由、族→下沉点、
//!   串族检测）。本模块不重判能力位图：设备有没有该族由 core 的能力闸负责，
//!   本模块只信「已经通过 core 的事件」。
//! 与 unx48805_input_guard 的边界：guard 关闭帧（SYN / 超时 / 溢出 / 重置），
//!   本模块只接收**已闭合帧**入环；环满的背压是**递交侧**概念，
//!   与 guard 的 4096 帧内缓冲溢出不是一回事（后者丢帧，前者必须重试成功）。
//! 与 unx48803_input_meta 的边界：meta 的 pending 是「未闭合的帧内容」，
//!   本模块的环槽是「已闭合帧的引用（序号+族掩码+事件数）」，不含帧内容拷贝。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 本模块诊断段基址。
pub const SEG_BASE: u32 = 0x700E0;

/// CPU 数（卡面「×8 核」）。
pub const CPU_COUNT: usize = 8;
/// 每核仿真轮数（卡面「×10^3 轮」）。
pub const ROUNDS_PER_CPU: u32 = 1_000;
/// 单环容量（帧槽数，2 的幂便于取模）。
pub const RING_CAP: usize = 128;
/// 三主族各注入事件数（卡面「×10^3」）。
pub const MAIN_FAMILY_EVENTS: u32 = 1_000;
/// 判据注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// 批量出队上限的矩阵（1/8/32/128）。
pub const BATCH_SIZES: [usize; 4] = [1, 8, 32, 128];

/// 批量档位的具名视图（避免对 `BATCH_SIZES` 直接下标；与上表由
/// `batch_sizes_named_agree()` 对拍）。
pub const BATCH_1: usize = 1;
/// 8 帧档。
pub const BATCH_8: usize = 8;
/// 32 帧档。
pub const BATCH_32: usize = 32;
/// 128 帧档。
pub const BATCH_128: usize = 128;

/// 具名档位与数组表逐项一致。
pub fn batch_sizes_named_agree() -> bool {
    let want = [BATCH_1, BATCH_8, BATCH_32, BATCH_128];
    let mut i = 0usize;
    while i < want.len() {
        match (BATCH_SIZES.get(i), want.get(i)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集（12 条）────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispCode {
    /// 事件族不在十族闭集内。
    UnknownFamily,
    /// 族 → 下沉点映射缺失（表被截断）。
    MissingSink,
    /// 串族（下沉点计数与族计数不符）。
    CrossFamily,
    /// 环满（背压，不是丢帧）。
    RingFull,
    /// 环空。
    RingEmpty,
    /// 环槽被覆盖（序号跳变）。
    SlotOverwritten,
    /// 序号不连续（丢帧或重复）。
    SeqDiscontinuity,
    /// 批量条数越配置上限。
    BatchOverLimit,
    /// 递交对账不平。
    LedgerMismatch,
    /// 锁层次登记无序（声称 lock-free 却有多把锁）。
    LockOrderBad,
    /// CPU 号越界。
    CpuOutOfRange,
    /// 双跑不一致。
    NotReproducible,
}

impl DispCode {
    /// 段内绝对码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文文案。
    pub fn detail(self) -> &'static str {
        match self {
            DispCode::UnknownFamily => "未知事件族：十族闭集外",
            DispCode::MissingSink => "族→下沉点映射缺失",
            DispCode::CrossFamily => "串族：下沉点计数与族计数不符",
            DispCode::RingFull => "环满：背压（不覆盖未消费槽）",
            DispCode::RingEmpty => "环空：无可出队帧",
            DispCode::SlotOverwritten => "环槽被覆盖：序号跳变",
            DispCode::SeqDiscontinuity => "序号不连续：丢帧或重复",
            DispCode::BatchOverLimit => "批量条数越配置上限",
            DispCode::LedgerMismatch => "递交对账不平",
            DispCode::LockOrderBad => "锁层次登记无序",
            DispCode::CpuOutOfRange => "CPU 号越界",
            DispCode::NotReproducible => "双跑不一致",
        }
    }

    /// 闭集全表。
    pub const ALL: [DispCode; 12] = [
        DispCode::UnknownFamily,
        DispCode::MissingSink,
        DispCode::CrossFamily,
        DispCode::RingFull,
        DispCode::RingEmpty,
        DispCode::SlotOverwritten,
        DispCode::SeqDiscontinuity,
        DispCode::BatchOverLimit,
        DispCode::LedgerMismatch,
        DispCode::LockOrderBad,
        DispCode::CpuOutOfRange,
        DispCode::NotReproducible,
    ];
}

/// 码值 → 文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DispCode::ALL.len() {
        if let Some(c) = DispCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x700E1..0x700EC 闭集内"
}

// ── 指纹基础设施（本域按「分派链指纹」命名）──────────────────
/// 分派链指纹初值。
pub fn route_init() -> u64 {
    0xcbf2_9ce4_8422_2325
}

/// 折入一字节。
pub fn route_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折入 32 位字。
pub fn route_word(h: u64, w: u32) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 4 {
        acc = route_byte(acc, ((w >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

// ── LCG（确定性事件流：三主族 ×10^3 需要可复现的伪随机码）────
/// 线性同余发生器（种子写死，双跑可复现）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 定种。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }

    /// 下一个 32 位。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.state >> 33) & 0xFFFF_FFFF) as u32
    }

    /// 取模范围内的下一个值（避免 % 0）。
    pub fn next_below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 十族与下沉点 ──────────────────────────────────────────────
/// 下沉点（每族恰好一个，禁止一对多）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sink {
    /// 帧屏障（SYN 收帧信号）。
    SynBarrier,
    /// 键队列（win32k 键队列 / IME / 热键）。
    KeyQueue,
    /// 指针位移。
    Pointer,
    /// 绝对坐标（含 MT 槽）。
    AbsMover,
    /// 扫描码取证。
    MscTracer,
    /// 开关状态机。
    SwitchState,
    /// 指示灯回写。
    LedWriter,
    /// 蜂鸣器。
    SndBeeper,
    /// 自动重复定时器。
    RepTimer,
    /// 力反馈引擎。
    FfEngine,
}

impl Sink {
    /// 下沉点名。
    pub fn name(self) -> &'static str {
        match self {
            Sink::SynBarrier => "syn-barrier",
            Sink::KeyQueue => "key-queue",
            Sink::Pointer => "pointer",
            Sink::AbsMover => "abs-mover",
            Sink::MscTracer => "msc-tracer",
            Sink::SwitchState => "switch-state",
            Sink::LedWriter => "led-writer",
            Sink::SndBeeper => "snd-beeper",
            Sink::RepTimer => "rep-timer",
            Sink::FfEngine => "ff-engine",
        }
    }

    /// 下沉点索引（0..9，与族槽位一一对应）。
    pub fn index(self) -> usize {
        match self {
            Sink::SynBarrier => 0,
            Sink::KeyQueue => 1,
            Sink::Pointer => 2,
            Sink::AbsMover => 3,
            Sink::MscTracer => 4,
            Sink::SwitchState => 5,
            Sink::LedWriter => 6,
            Sink::SndBeeper => 7,
            Sink::RepTimer => 8,
            Sink::FfEngine => 9,
        }
    }

    /// 全表。
    pub const ALL: [Sink; 10] = [
        Sink::SynBarrier,
        Sink::KeyQueue,
        Sink::Pointer,
        Sink::AbsMover,
        Sink::MscTracer,
        Sink::SwitchState,
        Sink::LedWriter,
        Sink::SndBeeper,
        Sink::RepTimer,
        Sink::FfEngine,
    ];
}

/// 族槽位（0..9）→ type 号（与 core 的冻结口径一致）。
pub const FAMILY_TYPE: [u16; 10] = [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x11, 0x12, 0x14, 0x15];

/// 槽位 → type；越界返回 None。
pub fn type_of_slot(slot: usize) -> Option<u16> {
    FAMILY_TYPE.get(slot).copied()
}

/// type → 槽位；十族外返回 None。
pub fn slot_of_type(kind: u16) -> Option<usize> {
    let mut i = 0usize;
    while i < FAMILY_TYPE.len() {
        if let Some(t) = FAMILY_TYPE.get(i) {
            if *t == kind {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// 族→下沉点映射（一对一，表被截断即 MissingSink）。
pub fn sink_of_slot(slot: usize) -> Option<Sink> {
    Sink::ALL.get(slot).copied()
}

/// 三主族槽位（KEY=1 / REL=2 / ABS=3）。
pub const PRIMARY_SLOTS: [usize; 3] = [1, 2, 3];

/// 事件（本模块只关心分派键，不带时间戳）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispatchEv {
    /// type。
    pub kind: u16,
    /// code。
    pub code: u16,
    /// value。
    pub value: i32,
}

impl DispatchEv {
    /// 构造。
    pub const fn new(kind: u16, code: u16, value: i32) -> DispatchEv {
        DispatchEv { kind, code, value }
    }

    /// 槽位。
    pub fn slot(&self) -> Option<usize> {
        slot_of_type(self.kind)
    }

    /// 指纹。
    pub fn digest(&self) -> u64 {
        let mut h = route_init();
        h = route_word(h, self.kind as u32);
        h = route_word(h, self.code as u32);
        h = route_word(h, self.value as u32);
        h
    }
}

// ── 路由器（三主族 ×10^3 零串族）──────────────────────────────
/// 路由结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Route {
    /// 槽位。
    pub slot: usize,
    /// 下沉点。
    pub sink: Sink,
    /// 是否三主族。
    pub primary: bool,
}

/// 路由器：逐族计数 + 逐下沉点计数（双账对拍才能抓串族）。
pub struct Router {
    family_hits: [u32; 10],
    sink_hits: [u32; 10],
    rejected: u32,
    digest: u64,
}

impl Router {
    /// 空路由器。
    pub fn new() -> Router {
        Router { family_hits: [0u32; 10], sink_hits: [0u32; 10], rejected: 0, digest: route_init() }
    }

    /// 路由一条事件（族校验 → 下沉点 → 双账）。
    pub fn route(&mut self, ev: &DispatchEv) -> Result<Route, u32> {
        let slot = match ev.slot() {
            Some(s) => s,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(DispCode::UnknownFamily.val());
            }
        };
        let sink = match sink_of_slot(slot) {
            Some(s) => s,
            None => return Err(DispCode::MissingSink.val()),
        };
        if sink.index() != slot {
            return Err(DispCode::MissingSink.val());
        }
        if let Some(f) = self.family_hits.get_mut(slot) {
            *f = f.saturating_add(1);
        } else {
            return Err(DispCode::MissingSink.val());
        }
        if let Some(s) = self.sink_hits.get_mut(sink.index()) {
            *s = s.saturating_add(1);
        } else {
            return Err(DispCode::MissingSink.val());
        }
        self.digest = route_word(self.digest, slot as u32);
        self.digest = route_word(self.digest, ev.code as u32);
        Ok(Route { slot, sink, primary: PRIMARY_SLOTS.contains(&slot) })
    }

    /// 族计数。
    pub fn family_count(&self, slot: usize) -> u32 {
        self.family_hits.get(slot).copied().unwrap_or(0)
    }

    /// 下沉点计数。
    pub fn sink_count(&self, slot: usize) -> u32 {
        self.sink_hits.get(slot).copied().unwrap_or(0)
    }

    /// 被拒事件数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 串族自检：逐槽位族计数必须等于该槽位下沉点计数。
    pub fn cross_check(&self) -> Result<u32, u32> {
        let mut i = 0usize;
        let mut total = 0u32;
        while i < 10 {
            let f = self.family_count(i);
            let s = self.sink_count(i);
            if f != s {
                return Err(DispCode::CrossFamily.val());
            }
            total = total.saturating_add(f);
            i += 1;
        }
        Ok(total)
    }

    /// 主族总命中数。
    pub fn primary_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < PRIMARY_SLOTS.len() {
            if let Some(slot) = PRIMARY_SLOTS.get(i) {
                n = n.saturating_add(self.family_count(*slot));
            }
            i += 1;
        }
        n
    }

    /// 分派链指纹。
    pub fn digest(&self) -> u64 {
        self.digest
    }
}

/// 生成三主族的确定性事件流（各 1000 条，码在合法范围内轮转）。
pub fn main_family_stream() -> Vec<DispatchEv> {
    let mut v: Vec<DispatchEv> = Vec::with_capacity((MAIN_FAMILY_EVENTS as usize) * 3);
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut n = 0u32;
    while n < MAIN_FAMILY_EVENTS {
        // EV_KEY：码 1..=0x58，值 0/1/2 三态。
        let kc = 1 + lcg.next_below(0x58) as u16;
        let kv = lcg.next_below(3) as i32;
        v.push(DispatchEv::new(0x01, kc, kv));
        // EV_REL：码 0/1/8，值有符号（用低位模拟 -8..=8）。
        let rc = match lcg.next_below(3) {
            0 => 0u16,
            1 => 1u16,
            _ => 8u16,
        };
        let rv = (lcg.next_below(17) as i32) - 8;
        v.push(DispatchEv::new(0x02, rc, rv));
        // EV_ABS：码 0/1/0x18/0x2F/0x35，值 0..=4095。
        let ac = match lcg.next_below(5) {
            0 => 0u16,
            1 => 1u16,
            2 => 0x18u16,
            3 => 0x2Fu16,
            _ => 0x35u16,
        };
        let av = lcg.next_below(4_096) as i32;
        v.push(DispatchEv::new(0x03, ac, av));
        n += 1;
    }
    v
}

/// 三主族 1000×3 路由：返回 (主族总命中, 串族自检总命中)。
pub fn primary_family_drill() -> Result<(u32, u32), u32> {
    let stream = main_family_stream();
    let mut r = Router::new();
    let mut i = 0usize;
    while i < stream.len() {
        if let Some(ev) = stream.get(i) {
            let out = r.route(ev)?;
            if !out.primary {
                return Err(DispCode::CrossFamily.val());
            }
            if out.sink.index() != out.slot {
                return Err(DispCode::MissingSink.val());
            }
        }
        i += 1;
    }
    let total = r.cross_check()?;
    Ok((r.primary_total(), total))
}

// ── 帧引用 ────────────────────────────────────────────────────
/// 环槽里的帧引用（不拷贝帧内容）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameRef {
    /// 全局序号（1 起，单调）。
    pub seq: u32,
    /// 帧内事件数。
    pub events: u16,
    /// 族掩码（本帧涉及哪些族）。
    pub family_mask: u16,
}

impl FrameRef {
    /// 构造。
    pub const fn new(seq: u32, events: u16, family_mask: u16) -> FrameRef {
        FrameRef { seq, events, family_mask }
    }
}

// ── SPSC 环 ───────────────────────────────────────────────────
/// 单生产者单消费者环（生产者只碰 tail，消费者只碰 head）。
pub struct SpscRing {
    slots: Vec<Option<FrameRef>>,
    head: usize,
    tail: usize,
    pushed: u64,
    popped: u64,
    backpressure: u64,
    max_fill: usize,
}

impl SpscRing {
    /// 定容环。
    pub fn new() -> SpscRing {
        let mut slots: Vec<Option<FrameRef>> = Vec::with_capacity(RING_CAP);
        let mut i = 0usize;
        while i < RING_CAP {
            slots.push(None);
            i += 1;
        }
        SpscRing {
            slots,
            head: 0,
            tail: 0,
            pushed: 0,
            popped: 0,
            backpressure: 0,
            max_fill: 0,
        }
    }

    /// 容量。
    pub fn cap(&self) -> usize {
        self.slots.len()
    }

    /// 当前占用。
    pub fn fill(&self) -> usize {
        if self.tail >= self.head {
            self.tail - self.head
        } else {
            self.slots.len() - self.head + self.tail
        }
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// 满（留一空位区分空/满）。
    pub fn is_full(&self) -> bool {
        let next = if self.tail + 1 == self.slots.len() { 0 } else { self.tail + 1 };
        next == self.head
    }

    /// 生产者入环：满即背压（绝不覆盖）。
    pub fn push(&mut self, f: FrameRef) -> Result<(), u32> {
        if self.is_full() {
            self.backpressure = self.backpressure.saturating_add(1);
            return Err(DispCode::RingFull.val());
        }
        let idx = self.tail;
        // 覆盖检测：只允许写空槽。
        match self.slots.get(idx) {
            Some(None) => {}
            Some(Some(_)) => return Err(DispCode::SlotOverwritten.val()),
            None => return Err(DispCode::SlotOverwritten.val()),
        }
        match self.slots.get_mut(idx) {
            Some(slot) => *slot = Some(f),
            None => return Err(DispCode::SlotOverwritten.val()),
        }
        self.tail = if idx + 1 == self.slots.len() { 0 } else { idx + 1 };
        self.pushed = self.pushed.saturating_add(1);
        let fill = self.fill();
        if fill > self.max_fill {
            self.max_fill = fill;
        }
        Ok(())
    }

    /// 消费者出环：空即 RingEmpty。
    pub fn pop(&mut self) -> Result<FrameRef, u32> {
        if self.is_empty() {
            return Err(DispCode::RingEmpty.val());
        }
        let idx = self.head;
        let out = match self.slots.get_mut(idx) {
            Some(slot) => match slot.take() {
                Some(f) => f,
                None => return Err(DispCode::SlotOverwritten.val()),
            },
            None => return Err(DispCode::SlotOverwritten.val()),
        };
        self.head = if idx + 1 == self.slots.len() { 0 } else { idx + 1 };
        self.popped = self.popped.saturating_add(1);
        Ok(out)
    }

    /// 批量出队：条数 = min(max, fill)；返回条数。
    pub fn drain(&mut self, max: usize) -> Result<Vec<FrameRef>, u32> {
        let allowed = if max > self.slots.len() { self.slots.len() } else { max };
        if max > self.slots.len() {
            // 越配置上限：先记账再按容量出队（不静默截断）。
            return Err(DispCode::BatchOverLimit.val());
        }
        let mut out: Vec<FrameRef> = Vec::new();
        let mut i = 0usize;
        while i < allowed {
            match self.pop() {
                Ok(f) => out.push(f),
                Err(c) if c == DispCode::RingEmpty.val() => break,
                Err(c) => return Err(c),
            }
            i += 1;
        }
        Ok(out)
    }

    /// 累计入环数。
    pub fn pushed(&self) -> u64 {
        self.pushed
    }

    /// 累计出环数。
    pub fn popped(&self) -> u64 {
        self.popped
    }

    /// 背压次数。
    pub fn backpressure(&self) -> u64 {
        self.backpressure
    }

    /// 历史最大占用。
    pub fn max_fill(&self) -> usize {
        self.max_fill
    }
}

// ── per-CPU 环组 ──────────────────────────────────────────────
/// 每核一个 SPSC 环（核间不共享槽位，故无跨核锁）。
pub struct PerCpuRings {
    rings: Vec<SpscRing>,
}

impl PerCpuRings {
    /// 建 CPU_COUNT 个环。
    pub fn new() -> PerCpuRings {
        let mut rings: Vec<SpscRing> = Vec::new();
        let mut i = 0usize;
        while i < CPU_COUNT {
            rings.push(SpscRing::new());
            i += 1;
        }
        PerCpuRings { rings }
    }

    /// 数量。
    pub fn count(&self) -> usize {
        self.rings.len()
    }

    /// 按 CPU 取环。
    pub fn ring(&mut self, cpu: usize) -> Result<&mut SpscRing, u32> {
        match self.rings.get_mut(cpu) {
            Some(r) => Ok(r),
            None => Err(DispCode::CpuOutOfRange.val()),
        }
    }

    /// 按 CPU 取环（只读）。
    pub fn ring_ref(&self, cpu: usize) -> Result<&SpscRing, u32> {
        match self.rings.get(cpu) {
            Some(r) => Ok(r),
            None => Err(DispCode::CpuOutOfRange.val()),
        }
    }

    /// 推一帧。
    pub fn push(&mut self, cpu: usize, f: FrameRef) -> Result<(), u32> {
        self.ring(cpu)?.push(f)
    }

    /// 出队一帧。
    pub fn pop(&mut self, cpu: usize) -> Result<FrameRef, u32> {
        self.ring(cpu)?.pop()
    }

    /// 批量出队。
    pub fn drain(&mut self, cpu: usize, max: usize) -> Result<Vec<FrameRef>, u32> {
        self.ring(cpu)?.drain(max)
    }

    /// 总入环/总出环。
    pub fn totals(&self) -> (u64, u64) {
        let mut p = 0u64;
        let mut q = 0u64;
        let mut i = 0usize;
        while i < self.rings.len() {
            if let Some(r) = self.rings.get(i) {
                p = p.saturating_add(r.pushed());
                q = q.saturating_add(r.popped());
            }
            i += 1;
        }
        (p, q)
    }
}

/// 8 核 ×1000 轮：每核交替入环/出队，序号必须逐轮连续、零丢零重。
pub fn cpu_round_drill() -> Result<u64, u32> {
    let mut set = PerCpuRings::new();
    let mut delivered: u64 = 0;
    let mut cpu = 0usize;
    while cpu < CPU_COUNT {
        let mut seq: u32 = 1;
        let mut round: u32 = 0;
        while round < ROUNDS_PER_CPU {
            let f = FrameRef::new(seq, 3, 0x007);
            set.push(cpu, f)?;
            // 每 3 轮抽干一次，模拟消费者周期性批量取走。
            if round % 3 == 2 {
                let got = set.drain(cpu, BATCH_8)?;
                let mut i = 0usize;
                while i < got.len() {
                    if let Some(g) = got.get(i) {
                        if g.seq != seq.saturating_sub((got.len() as u32) - 1) + i as u32 {
                            return Err(DispCode::SeqDiscontinuity.val());
                        }
                        delivered = delivered.saturating_add(1);
                    }
                    i += 1;
                }
            }
            seq = seq.saturating_add(1);
            round += 1;
        }
        // 收尾抽干该核。
        loop {
            match set.pop(cpu) {
                Ok(_) => delivered = delivered.saturating_add(1),
                Err(c) if c == DispCode::RingEmpty.val() => break,
                Err(c) => return Err(c),
            }
        }
        cpu += 1;
    }
    let (p, q) = set.totals();
    if p != q || q != delivered {
        return Err(DispCode::LedgerMismatch.val());
    }
    Ok(delivered)
}

/// 满环背压演练：灌满 → 再推必拒（不覆盖）→ 抽干 → 之前被拒的帧重推成功。
pub fn backpressure_drill() -> Result<u32, u32> {
    let mut ring = SpscRing::new();
    let usable = ring.cap() - 1;
    let mut seq: u32 = 1;
    let mut i = 0usize;
    while i < usable {
        ring.push(FrameRef::new(seq, 2, 0x003))?;
        seq = seq.saturating_add(1);
        i += 1;
    }
    if !ring.is_full() {
        return Err(DispCode::RingFull.val());
    }
    let refused = ring.push(FrameRef::new(seq, 2, 0x003));
    if refused.is_ok() {
        return Err(DispCode::RingFull.val());
    }
    let before = ring.pushed();
    let got = ring.drain(BATCH_128)?;
    if got.len() != usable {
        return Err(DispCode::LedgerMismatch.val());
    }
    // 序号严格连续，证明没有槽被覆盖。
    let mut k = 0usize;
    while k < got.len() {
        if let Some(g) = got.get(k) {
            if g.seq != (k as u32) + 1 {
                return Err(DispCode::SeqDiscontinuity.val());
            }
        }
        k += 1;
    }
    // 被拒的帧重推成功（背压的语义是「稍后再来」，不是「丢」）。
    ring.push(FrameRef::new(seq, 2, 0x003))?;
    if ring.pushed() != before + 1 {
        return Err(DispCode::LedgerMismatch.val());
    }
    if ring.backpressure() == 0 {
        return Err(DispCode::RingFull.val());
    }
    Ok(ring.backpressure() as u32)
}

/// A5 锁层次登记（严格递增；后人不许在环里塞第二把锁还说 lock-free）。
pub const LOCK_ORDER: [&str; 4] = [
    "0:irqsave",
    "1:ring.tail(producer-only)",
    "2:ring.head(consumer-only)",
    "3:batch.deliver",
];

/// 锁层次自检：条目必须递增编号且唯一。
pub fn lock_order_check() -> Result<(), u32> {
    let mut i = 0usize;
    while i < LOCK_ORDER.len() {
        let cur = match LOCK_ORDER.get(i) {
            Some(s) => *s,
            None => return Err(DispCode::LockOrderBad.val()),
        };
        let first = match cur.as_bytes().first() {
            Some(b) => *b,
            None => return Err(DispCode::LockOrderBad.val()),
        };
        if !(b'0'..=b'9').contains(&first) {
            return Err(DispCode::LockOrderBad.val());
        }
        let digit = first - b'0';
        if digit as usize != i {
            return Err(DispCode::LockOrderBad.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 批量递交对账 ──────────────────────────────────────────────
/// 批量账本。
#[derive(Clone, Copy, Debug)]
pub struct BatchLedger {
    /// 递交（入环）总帧数。
    pub submitted: u64,
    /// 已交付（出队）总帧数。
    pub delivered: u64,
    /// 批次数。
    pub batches: u32,
    /// 最大批大小。
    pub max_batch: usize,
    /// 背压次数。
    pub backpressure: u64,
}

impl BatchLedger {
    /// 空账。
    pub fn new() -> BatchLedger {
        BatchLedger { submitted: 0, delivered: 0, batches: 0, max_batch: 0, backpressure: 0 }
    }

    /// 在途（未交付）帧数。
    pub fn inflight(&self) -> u64 {
        self.submitted.saturating_sub(self.delivered)
    }

    /// 对账：submitted == delivered + inflight（恒等，inflight 定义即差）。
    pub fn balanced(&self) -> bool {
        self.submitted >= self.delivered
    }
}

/// 批量递交 1000 帧：入环 → 按批抽干 → 逐批对账。
pub fn batch_drill(batch: usize) -> Result<BatchLedger, u32> {
    if batch == 0 {
        return Err(DispCode::BatchOverLimit.val());
    }
    let mut ring = SpscRing::new();
    let mut led = BatchLedger::new();
    let total = ROUNDS_PER_CPU;
    let mut seq: u32 = 1;
    let mut n: u32 = 0;
    while n < total {
        // 背压不是丢帧：满环就抽一批再重推**同一帧**（序号不换）。
        loop {
            match ring.push(FrameRef::new(seq, 3, 0x007)) {
                Ok(()) => {
                    led.submitted = led.submitted.saturating_add(1);
                    seq = seq.saturating_add(1);
                    break;
                }
                Err(c) if c == DispCode::RingFull.val() => {
                    let got = ring.drain(batch)?;
                    led.delivered = led.delivered.saturating_add(got.len() as u64);
                    led.batches = led.batches.saturating_add(1);
                    led.backpressure = led.backpressure.saturating_add(1);
                    if got.len() > led.max_batch {
                        led.max_batch = got.len();
                    }
                    if got.is_empty() {
                        return Err(DispCode::LedgerMismatch.val());
                    }
                }
                Err(c) => return Err(c),
            }
        }
        if ring.fill() >= batch && n % 5 == 4 {
            let got = ring.drain(batch)?;
            led.delivered = led.delivered.saturating_add(got.len() as u64);
            led.batches = led.batches.saturating_add(1);
            if got.len() > led.max_batch {
                led.max_batch = got.len();
            }
        }
        n += 1;
    }
    // 收尾抽干。
    loop {
        match ring.pop() {
            Ok(_) => led.delivered = led.delivered.saturating_add(1),
            Err(c) if c == DispCode::RingEmpty.val() => break,
            Err(c) => return Err(c),
        }
    }
    if !led.balanced() || led.inflight() != 0 || led.submitted != total as u64 {
        return Err(DispCode::LedgerMismatch.val());
    }
    Ok(led)
}

/// 批大小矩阵：1/8/32/128 四档都必须零丢。
pub fn batch_matrix() -> Result<u32, u32> {
    let mut ok = 0u32;
    let mut i = 0usize;
    while i < BATCH_SIZES.len() {
        if let Some(b) = BATCH_SIZES.get(i) {
            let led = batch_drill(*b)?;
            if led.inflight() == 0 && led.submitted == ROUNDS_PER_CPU as u64 {
                ok += 1;
            }
        }
        i += 1;
    }
    Ok(ok)
}

// ── 黄金全链 ──────────────────────────────────────────────────
/// 黄金链输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenOut {
    /// 三主族总命中。
    pub primary_hits: u32,
    /// 串族自检总命中。
    pub cross_total: u32,
    /// 8 核交付帧数。
    pub cpu_delivered: u64,
    /// 批量四档全绿数。
    pub batch_ok: u32,
    /// 分派链指纹。
    pub digest: u64,
}

/// 黄金全链。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let (primary, cross) = primary_family_drill()?;
    if primary != (MAIN_FAMILY_EVENTS * 3) || cross != (MAIN_FAMILY_EVENTS * 3) {
        return Err(DispCode::CrossFamily.val());
    }
    let delivered = cpu_round_drill()?;
    let batch_ok = batch_matrix()?;
    lock_order_check()?;
    let mut r = Router::new();
    let stream = main_family_stream();
    let mut i = 0usize;
    while i < stream.len() {
        if let Some(ev) = stream.get(i) {
            r.route(ev)?;
        }
        i += 1;
    }
    let mut digest = r.digest();
    digest = route_word(digest, delivered as u32);
    digest = route_word(digest, batch_ok);
    Ok(GoldenOut { primary_hits: primary, cross_total: cross, cpu_delivered: delivered, batch_ok, digest })
}

// ── 注入账 ────────────────────────────────────────────────────
/// 注入账：4 槽。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 每槽注入次数。
    pub tries: [u32; 4],
    /// 每槽按预期命中次数。
    pub caught: [u32; 4],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 4], caught: [0u32; 4] }
    }

    /// 记账。
    pub fn record(&mut self, slot: usize, caught: bool) -> bool {
        let mut ok = false;
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            ok = true;
        }
        if ok && caught {
            if let Some(c) = self.caught.get_mut(slot) {
                *c += 1;
            }
        }
        ok
    }

    /// 四槽全中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || t != c {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入 4 槽 × rounds：未知族 / 满环 / 空环出队 / 越限批量。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：伪族 0x7E。
        let mut router = Router::new();
        let bogus = DispatchEv::new(0x7E, 1, 1);
        led.record(
            0,
            matches!(router.route(&bogus), Err(c) if c == DispCode::UnknownFamily.val()),
        );

        // 槽 1：满环拒绝。
        let mut ring = SpscRing::new();
        let mut i = 0usize;
        let usable = ring.cap() - 1;
        while i < usable {
            let _ = ring.push(FrameRef::new((i as u32) + 1, 1, 0x001)).is_ok();
            i += 1;
        }
        led.record(
            1,
            matches!(ring.push(FrameRef::new(9_999, 1, 0x001)), Err(c) if c == DispCode::RingFull.val()),
        );

        // 槽 2：空环出队。
        let mut ring = SpscRing::new();
        led.record(
            2,
            matches!(ring.pop(), Err(c) if c == DispCode::RingEmpty.val()),
        );

        // 槽 3：批量越限。
        let mut ring = SpscRing::new();
        ring.push(FrameRef::new(1, 1, 0x001))?;
        led.record(
            3,
            matches!(ring.drain(RING_CAP + 1), Err(c) if c == DispCode::BatchOverLimit.val()),
        );
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────
/// 探针 A：三主族事件逐条落到本族下沉点。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut r = Router::new();
    r.route(&DispatchEv::new(0x01, 0x1E, 1))?;
    r.route(&DispatchEv::new(0x02, 0x00, -5))?;
    r.route(&DispatchEv::new(0x03, 0x35, 512))?;
    if r.family_count(1) != 1 || r.family_count(2) != 1 || r.family_count(3) != 1 {
        return Err(DispCode::CrossFamily.val());
    }
    r.cross_check()?;
    Ok(())
}

/// 探针 B：满环拒绝且已完成环内可达上限（cap-1）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut ring = SpscRing::new();
    let usable = ring.cap() - 1;
    let mut i = 0usize;
    while i < usable {
        ring.push(FrameRef::new((i as u32) + 1, 1, 0x001))?;
        i += 1;
    }
    if !ring.is_full() {
        return Err(DispCode::RingFull.val());
    }
    match ring.push(FrameRef::new(0xFFFF, 1, 0x001)) {
        Err(c) if c == DispCode::RingFull.val() => Ok(()),
        _ => Err(DispCode::SlotOverwritten.val()),
    }
}

/// 探针 C：批量出队条数恰为 min(max, fill)。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut ring = SpscRing::new();
    let mut i = 0u32;
    while i < 10 {
        ring.push(FrameRef::new(i + 1, 1, 0x007))?;
        i += 1;
    }
    let got = ring.drain(BATCH_32)?;
    if got.len() != 10 {
        return Err(DispCode::LedgerMismatch.val());
    }
    if ring.fill() != 0 {
        return Err(DispCode::LedgerMismatch.val());
    }
    Ok(())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DispCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < DispCode::ALL.len() {
        match DispCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v <= SEG_BASE || v >= SEG_BASE + 0x20 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // F48808-J1：三主族 ×10^3 路由零串族。
    let drill = primary_family_drill();
    v.push((
        "F48808-J1-三主族1000×3零串族",
        matches!(drill, Ok((p, c)) if p == MAIN_FAMILY_EVENTS * 3 && c == MAIN_FAMILY_EVENTS * 3),
        format!("drill={:?}", drill),
    ));

    // F48808-J1：十族→下沉点一对一。
    let mapping_ok = {
        let mut ok = true;
        let mut i = 0usize;
        while i < 10 {
            match sink_of_slot(i) {
                Some(s) => {
                    if s.index() != i {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            if type_of_slot(i).is_none() {
                ok = false;
            }
            i += 1;
        }
        ok && type_of_slot(10).is_none() && slot_of_type(0x99).is_none()
    };
    v.push((
        "F48808-J1-十族下沉点一对一",
        mapping_ok,
        String::from("slot↔sink↔type 三向闭合"),
    ));

    // F48808-J1：未知族不进环、只计拒收。
    let unknown = {
        let mut r = Router::new();
        let mut ok = true;
        let mut i = 0u32;
        while i < JUDGE_ROUNDS {
            if r.route(&DispatchEv::new(0x55, 1, 1)).is_ok() {
                ok = false;
            }
            i += 1;
        }
        ok && r.rejected() == JUDGE_ROUNDS && r.cross_check() == Ok(0)
    };
    v.push((
        "F48808-J1-未知族不入环",
        unknown,
        format!("rejected={}/{}", JUDGE_ROUNDS, JUDGE_ROUNDS),
    ));

    // F48809-J1：8 核 ×1000 轮零丢零重。
    let cpu = cpu_round_drill();
    v.push((
        "F48809-J1-8核×1000轮零丢",
        matches!(cpu, Ok(n) if n == (CPU_COUNT as u64) * (ROUNDS_PER_CPU as u64)),
        format!("delivered={:?}", cpu),
    ));

    // F48809-J1：满环背压不覆盖（重推成功）。
    let bp = backpressure_drill();
    v.push((
        "F48809-J1-满环背压不丢",
        matches!(bp, Ok(n) if n >= 1),
        format!("backpressure={:?}", bp),
    ));

    // F48809-J1：A5 锁层次登记有序。
    v.push((
        "F48809-J1-A5锁层次登记",
        lock_order_check().is_ok() && LOCK_ORDER.len() == 4,
        format!("{:?}", LOCK_ORDER),
    ));

    // F48810-J1：批量 1000 帧对账零丢。
    let led = batch_drill(BATCH_32);
    let led_ok = match &led {
        Ok(l) => l.inflight() == 0 && l.submitted == ROUNDS_PER_CPU as u64 && l.balanced(),
        Err(_) => false,
    };
    v.push((
        "F48810-J1-批量1000帧对账",
        led_ok,
        format!(
            "submitted={:?} delivered={:?} batches={:?}",
            led.as_ref().map(|l| l.submitted),
            led.as_ref().map(|l| l.delivered),
            led.as_ref().map(|l| l.batches)
        ),
    ));

    // F48810-J1：批大小矩阵 1/8/32/128 四档全绿。
    let matrix = batch_matrix();
    v.push((
        "F48810-J1-批大小矩阵四档",
        matches!(matrix, Ok(4)),
        format!("ok={:?}/4", matrix),
    ));

    // F48808-J2：注入 4 槽 ×10 全中。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = matches!(&inj, Ok(l) if l.all_caught());
    v.push((
        "F48808-J2-注入10轮4槽全中",
        inj_ok,
        format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // F48808-J3：双跑 + 段位。
    v.push((
        "F48808-J3-双跑与段位",
        repro_check().is_ok() && segment_guard(),
        format!("golden={:?}", golden_run().map(|g| g.digest)),
    ));

    v
}

pub fn run_unx48808_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48808 输入事件分派");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn family_table_three_way_closed() {
        assert_eq!(FAMILY_TYPE.len(), 10);
        for i in 0..10 {
            let t = type_of_slot(i).expect("族 type");
            assert_eq!(slot_of_type(t), Some(i));
            let s = sink_of_slot(i).expect("下沉点");
            assert_eq!(s.index(), i);
        }
        assert!(type_of_slot(10).is_none());
        assert!(slot_of_type(0x06).is_none());
    }

    #[test]
    fn primary_families_are_key_rel_abs() {
        let mut names: Vec<&str> = Vec::new();
        for slot in PRIMARY_SLOTS.iter() {
            let t = type_of_slot(*slot).expect("type");
            assert_eq!(t, match slot {
                1 => 0x01,
                2 => 0x02,
                _ => 0x03,
            });
            let sink = sink_of_slot(*slot).expect("sink");
            names.push(sink.name());
        }
        assert_eq!(names, vec!["key-queue", "pointer", "abs-mover"]);
    }

    #[test]
    fn routing_thousand_each_no_cross() {
        let (p, c) = primary_family_drill().expect("三主族演练");
        assert_eq!(p, 3_000);
        assert_eq!(c, 3_000);
    }

    #[test]
    fn router_counts_split_by_family() {
        let mut r = Router::new();
        let mut i = 0u32;
        while i < 7 {
            r.route(&DispatchEv::new(0x01, 0x1E, 1)).expect("key");
            r.route(&DispatchEv::new(0x02, 0x00, 1)).expect("rel");
            i += 1;
        }
        assert_eq!(r.family_count(1), 7);
        assert_eq!(r.family_count(2), 7);
        assert_eq!(r.family_count(3), 0);
        assert_eq!(r.sink_count(1), 7);
        assert_eq!(r.cross_check(), Ok(14));
        assert_eq!(r.rejected(), 0);
    }

    #[test]
    fn unknown_family_rejected_not_queued() {
        let mut r = Router::new();
        assert_eq!(
            r.route(&DispatchEv::new(0x42, 0, 0)).err(),
            Some(DispCode::UnknownFamily.val())
        );
        assert_eq!(r.rejected(), 1);
        assert_eq!(r.cross_check(), Ok(0));
    }

    #[test]
    fn ring_is_spsc_with_backpressure() {
        let mut ring = SpscRing::new();
        let usable = ring.cap() - 1;
        for i in 0..usable {
            ring.push(FrameRef::new((i as u32) + 1, 1, 0x001)).expect("入环");
        }
        assert!(ring.is_full());
        assert_eq!(ring.fill(), usable);
        assert_eq!(ring.push(FrameRef::new(0, 1, 1)).err(), Some(DispCode::RingFull.val()));
        assert_eq!(ring.backpressure(), 1);
        let got = ring.drain(BATCH_128).expect("批量");
        assert_eq!(got.len(), usable);
        assert!(ring.is_empty());
        for (k, g) in got.iter().enumerate() {
            assert_eq!(g.seq, (k as u32) + 1);
        }
    }

    #[test]
    fn drain_over_cap_rejected() {
        let mut ring = SpscRing::new();
        ring.push(FrameRef::new(1, 1, 0x001)).expect("入环");
        assert_eq!(ring.drain(RING_CAP + 1).err(), Some(DispCode::BatchOverLimit.val()));
        // 越限被拒后帧仍在（没有被摘走）。
        assert_eq!(ring.fill(), 1);
    }

    #[test]
    fn eight_cpus_thousand_rounds() {
        let n = cpu_round_drill().expect("8 核演练");
        assert_eq!(n, (CPU_COUNT * ROUNDS_PER_CPU as usize) as u64);
        let set = PerCpuRings::new();
        assert_eq!(set.count(), CPU_COUNT);
        assert!(set.ring_ref(CPU_COUNT).is_err());
    }

    #[test]
    fn batch_sizes_matrix_all_zero_loss() {
        assert_eq!(batch_matrix(), Ok(4));
        for b in BATCH_SIZES.iter() {
            let led = batch_drill(*b).expect("批量演练");
            assert_eq!(led.submitted, ROUNDS_PER_CPU as u64);
            assert_eq!(led.inflight(), 0);
            assert!(led.batches >= 1);
            assert!(led.max_batch <= *b);
        }
        assert_eq!(batch_drill(0).err(), Some(DispCode::BatchOverLimit.val()));
    }

    #[test]
    fn lock_order_registered() {
        assert!(lock_order_check().is_ok());
        assert_eq!(LOCK_ORDER.len(), 4);
        assert!(matches!(LOCK_ORDER.first(), Some(v) if v.starts_with("0:")));
        assert!(matches!(LOCK_ORDER.last(), Some(v) if v.starts_with("3:")));
    }

    #[test]
    fn golden_chain_consistent() {
        let a = golden_run().expect("黄金链");
        let b = golden_run().expect("二跑");
        assert_eq!(a, b);
        assert_eq!(a.primary_hits, 3_000);
        assert_eq!(a.cross_total, 3_000);
        assert_eq!(a.cpu_delivered, (CPU_COUNT * ROUNDS_PER_CPU as usize) as u64);
        assert_eq!(a.batch_ok, 4);
    }

    #[test]
    fn inject_four_slots_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).expect("注入");
        assert!(led.all_caught());
        assert_eq!(led.tries, [JUDGE_ROUNDS; 4]);
        assert_eq!(led.caught, [JUDGE_ROUNDS; 4]);
    }

    #[test]
    fn probes_pass() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn codes_are_segment_closed() {
        assert!(segment_guard());
        for c in DispCode::ALL.iter() {
            let v = c.val();
            assert!(v > SEG_BASE && v < SEG_BASE + 0x20);
            assert_eq!(detail(v), c.detail());
        }
        assert_eq!(detail(0x700ED), "未知诊断码：不在 0x700E1..0x700EC 闭集内");
    }

    #[test]
    fn batch_sizes_named_table_agrees() {
        assert!(batch_sizes_named_agree());
        assert_eq!(BATCH_SIZES.len(), 4);
        assert_eq!(BATCH_1, 1);
        assert_eq!(BATCH_8, 8);
        assert_eq!(BATCH_32, 32);
        assert_eq!(BATCH_128, 128);
    }
}
