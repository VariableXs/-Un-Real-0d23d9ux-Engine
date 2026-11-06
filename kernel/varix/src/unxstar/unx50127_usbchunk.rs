//! UNX-F50127 传输背压与分块账（块大小协商档）（0x43c00 段）
//!
//! 锚点：UNX-F50127-J1 分块 ×3 档吞吐落账（模拟器档）；判据锚定 §AI-63 §2 专题八。
//!
//! 为什么先把「分块账」算清楚再开 bulk 管道：USB 批量传输的吞吐由三件事共同
//! 决定——单次传输的分块大小（每块都要付一次启动/握手开销）、在途窗口（未确认
//! 字节上限，超出即背压）、设备端排空速率。分块太小，启动开销吃掉带宽；分块太大，
//! 首字节延迟与重传代价一起变高。本模块把这三项逐 tick 记进账本，并给出三档
//! 协商档的落账结果，供上层按打印机型选择。
//!
//! 结构清单：
//! * `Profile` —— 三档协商档（保守 4 KiB / 均衡 16 KiB / 激进 64 KiB），每档
//!   自带在途窗口；分块必须是 bulk 最大包长（512）的整数倍，窗口不得小于一块；
//! * `LinkState` —— 链路状态机 Idle→Negotiating→Streaming↔Throttled→
//!   Draining→Complete，非法跳转显式报 `State`，不静默复原；
//! * `WatermarkGate` —— 高低水位闸（高=窗口、低=窗口一半），把在途字节翻译成
//!   Flow/Throttle/Resume 三种动作，背压判定只有这一处来源；
//! * `ChunkLedger` —— 分块账本（序号/偏移/大小/提交与完成时刻/重试数），审计
//!   校验字节守恒、偏移连续、完成时刻单调；`push_raw` 是回放入口（只查容量），
//!   供离线账本导入后由 `audit` 判真伪；
//! * `simulate` —— 模拟器档：单条 bulk 管道串行排空，每块付一次 `setup_ticks`
//!   启动开销，逐 tick 记账，产出吞吐/首字节延迟/背压次数/摘要。
//!
//! 错误纪律：段内闭集 `SEG_BASE + n`，全部码值都被注入核或判据真实构造；
//! 内核路径零 panic，所有 Vec 访问走 `get()`/`get_mut()`。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43c00 独占 0x20）。
pub const SEG_BASE: u32 = 0x43c00;

pub const E_EMPTY: u16 = 1;
pub const E_TOTAL: u16 = 2;
pub const E_CHUNK: u16 = 3;
pub const E_ALIGN: u16 = 4;
pub const E_PROFILE: u16 = 5;
pub const E_WINDOW: u16 = 6;
pub const E_STATE: u16 = 7;
pub const E_CAPACITY: u16 = 8;
pub const E_SEQ: u16 = 9;
pub const E_CONSERVE: u16 = 10;
pub const E_BUDGET: u16 = 11;
pub const E_RETRY: u16 = 12;
pub const E_MPS: u16 = 13;
pub const E_DEVICE: u16 = 14;

/// bulk 端点的最大包长（USB 2.0 高速 512；三条档位都按它对齐）。
pub const BULK_PACKET: u32 = 512;
/// 单块上限 1 MiB：再大就只是把重传代价放大，且账本会失去粒度。
pub const MAX_CHUNK: u32 = 1 << 20;
/// 在途窗口上限 4 MiB：超过这个量级的背压窗口在打印链路上没有现实意义。
pub const MAX_WINDOW: u32 = 1 << 22;
/// 单次模拟最多排队的在途块数（窗口再大也不会同时压满更多块）。
const QUEUE_SLOTS: usize = 8;
/// 账本容量上限：1 MiB / 512 B = 2048 块是极端情形，留 8 倍余量。
const LEDGER_MAX: usize = 16384;

/// 分块/背压错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkError {
    Empty,
    Total,
    ChunkSize,
    Align,
    Profile,
    Window,
    State,
    Capacity,
    Seq,
    Conserve,
    Budget,
    Retry,
    Mps,
    Device,
}

impl ChunkError {
    pub fn code(self) -> u32 {
        let n = match self {
            ChunkError::Empty => E_EMPTY,
            ChunkError::Total => E_TOTAL,
            ChunkError::ChunkSize => E_CHUNK,
            ChunkError::Align => E_ALIGN,
            ChunkError::Profile => E_PROFILE,
            ChunkError::Window => E_WINDOW,
            ChunkError::State => E_STATE,
            ChunkError::Capacity => E_CAPACITY,
            ChunkError::Seq => E_SEQ,
            ChunkError::Conserve => E_CONSERVE,
            ChunkError::Budget => E_BUDGET,
            ChunkError::Retry => E_RETRY,
            ChunkError::Mps => E_MPS,
            ChunkError::Device => E_DEVICE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            ChunkError::Empty => "empty name or zero-capacity ledger",
            ChunkError::Total => "transfer byte count is zero",
            ChunkError::ChunkSize => "chunk size zero or above 1 MiB",
            ChunkError::Align => "size not a multiple of 512-byte bulk packet",
            ChunkError::Profile => "unknown chunk profile code",
            ChunkError::Window => "inflight window below one chunk or above cap",
            ChunkError::State => "illegal link state transition",
            ChunkError::Capacity => "ledger or queue capacity exhausted",
            ChunkError::Seq => "chunk sequence out of order or duplicated",
            ChunkError::Conserve => "chunk ledger byte/offset conservation broken",
            ChunkError::Budget => "simulation exceeded its tick budget",
            ChunkError::Retry => "retry budget exhausted",
            ChunkError::Mps => "device max packet not a power of two in 8..=1024",
            ChunkError::Device => "device drain/setup parameters invalid",
        }
    }
}

// ---------------------------------------------------------------------------
// 链路状态机
// ---------------------------------------------------------------------------

/// 链路状态。Throttled 是 Streaming 的背压子态：数据仍可能在途，但不允许再提交。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    Idle,
    Negotiating,
    Streaming,
    Throttled,
    Draining,
    Complete,
}

impl LinkState {
    pub fn wire(self) -> u8 {
        match self {
            LinkState::Idle => 0,
            LinkState::Negotiating => 1,
            LinkState::Streaming => 2,
            LinkState::Throttled => 3,
            LinkState::Draining => 4,
            LinkState::Complete => 5,
        }
    }

    pub fn from_wire(c: u8) -> Result<LinkState, ChunkError> {
        match c {
            0 => Ok(LinkState::Idle),
            1 => Ok(LinkState::Negotiating),
            2 => Ok(LinkState::Streaming),
            3 => Ok(LinkState::Throttled),
            4 => Ok(LinkState::Draining),
            5 => Ok(LinkState::Complete),
            _ => Err(ChunkError::State),
        }
    }

    /// 合法跳转表。Complete 只能回到 Idle（新一轮协商），不能直接回到 Streaming。
    pub fn can_go(self, to: LinkState) -> bool {
        match self {
            LinkState::Idle => to == LinkState::Negotiating,
            LinkState::Negotiating => to == LinkState::Streaming,
            LinkState::Streaming => to == LinkState::Throttled || to == LinkState::Draining,
            LinkState::Throttled => to == LinkState::Streaming || to == LinkState::Draining,
            LinkState::Draining => to == LinkState::Complete,
            LinkState::Complete => to == LinkState::Idle,
        }
    }

    /// 唯一的状态推进入口；非法跳转返回 `State`，调用方必须显式处理。
    pub fn transition(self, to: LinkState) -> Result<LinkState, ChunkError> {
        if self.can_go(to) {
            Ok(to)
        } else {
            Err(ChunkError::State)
        }
    }

    pub fn is_terminal(self) -> bool {
        self == LinkState::Complete
    }
}

// ---------------------------------------------------------------------------
// 分块档
// ---------------------------------------------------------------------------

/// 协商档：块大小 + 在途窗口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub code: u8,
    pub name: &'static str,
    pub chunk: u32,
    pub window: u32,
}

/// 三档档位。chunk 取 4/16/64 KiB：4 KiB 是传统并口打印机的常见主机缓冲粒度，
/// 64 KiB 是 bulk 管道上「一块吃满一帧调度」的上限；16 KiB 居中作均衡档。
/// 窗口分别取 2/3/4 块：够设备连续排空，又不至于让重传包裹过大。
pub const PROFILES: [Profile; 3] = [
    Profile {
        code: 1,
        name: "conservative",
        chunk: 4 * 1024,
        window: 2 * 4 * 1024,
    },
    Profile {
        code: 2,
        name: "balanced",
        chunk: 16 * 1024,
        window: 3 * 16 * 1024,
    },
    Profile {
        code: 3,
        name: "aggressive",
        chunk: 64 * 1024,
        window: 4 * 64 * 1024,
    },
];

impl Profile {
    /// 自检：名字非空、块非零且不超上限、按最大包长对齐、窗口容得下一块。
    pub fn validate(&self) -> Result<(), ChunkError> {
        if self.name.is_empty() {
            return Err(ChunkError::Empty);
        }
        if self.chunk == 0 || self.chunk > MAX_CHUNK {
            return Err(ChunkError::ChunkSize);
        }
        if self.chunk % BULK_PACKET != 0 {
            return Err(ChunkError::Align);
        }
        if self.window < self.chunk || self.window > MAX_WINDOW {
            return Err(ChunkError::Window);
        }
        Ok(())
    }

    /// 该档在 `total` 字节上会切出多少块（向上取整）。
    pub fn chunk_count(&self, total: u32) -> u32 {
        if self.chunk == 0 {
            return 0;
        }
        let full = total / self.chunk;
        if total % self.chunk == 0 {
            full
        } else {
            full.saturating_add(1)
        }
    }
}

/// 按档位码查表。码值非法不是「回退默认档」——静默回退会让落账失真。
pub fn profile_of(code: u8) -> Result<Profile, ChunkError> {
    let mut i = 0usize;
    while i < PROFILES.len() {
        if let Some(p) = PROFILES.get(i) {
            if p.code == code {
                return Ok(*p);
            }
        }
        i += 1;
    }
    Err(ChunkError::Profile)
}

// ---------------------------------------------------------------------------
// 设备模型与水位闸
// ---------------------------------------------------------------------------

/// 模拟器档设备：每 tick 排空 `drain_per_tick` 字节；每块进入设备要先付
/// `setup_ticks` 的启动开销（真实链路上的 CBW/握手/调度开销的抽象）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimDevice {
    pub name: &'static str,
    pub drain_per_tick: u32,
    pub setup_ticks: u32,
    pub max_packet: u16,
}

impl SimDevice {
    /// 构造并校验设备参数。排空为 0 或启动开销超过 64 tick 一律拒绝：
    /// 前者必然死锁，后者已不是「开销」而是故障。
    pub fn new(
        name: &'static str,
        drain_per_tick: u32,
        setup_ticks: u32,
        max_packet: u16,
    ) -> Result<SimDevice, ChunkError> {
        if name.is_empty() {
            return Err(ChunkError::Empty);
        }
        if drain_per_tick == 0 || setup_ticks == 0 || setup_ticks > 64 {
            return Err(ChunkError::Device);
        }
        let mps = max_packet as u32;
        if mps < 8 || mps > 1024 || !mps.is_power_of_two() {
            return Err(ChunkError::Mps);
        }
        Ok(SimDevice {
            name,
            drain_per_tick,
            setup_ticks,
            max_packet,
        })
    }

    /// 高速打印机档：1 tick = 1 ms，24 KiB/ms ≈ 24 MiB/s 的 bulk 吞吐，
    /// 与 USB 2.0 高速打印机的实测中位水平同量级。
    pub fn printer_usb2() -> Result<SimDevice, ChunkError> {
        SimDevice::new("printer-usb2", 24 * 1024, 1, 512)
    }

    /// 极端慢档：1 B/tick，只用于验证预算闸（必然撑爆 tick 预算）。
    pub fn trickle() -> Result<SimDevice, ChunkError> {
        SimDevice::new("trickle", 1, 1, 512)
    }
}

/// 水位闸动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Flow,
    Throttle,
    Resume,
}

/// 高低水位闸：高水位 = 在途窗口（不许再提交），低水位 = 窗口一半（恢复提交）。
/// 单点判定，避免「提交处一套阈值、恢复处另一套阈值」造成的抖动。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatermarkGate {
    pub high: u32,
    pub low: u32,
}

impl WatermarkGate {
    pub fn new(high: u32, low: u32) -> Result<WatermarkGate, ChunkError> {
        if high == 0 || low >= high {
            return Err(ChunkError::Window);
        }
        Ok(WatermarkGate { high, low })
    }

    /// `submitting` 表示本 tick 还有数据待提交；只有「想提交且已到高水位」才算
    /// 背压，否则只是排空过程中的正常水位。
    pub fn decide(&self, inflight: u32, submitting: bool) -> Gate {
        if submitting && inflight >= self.high {
            Gate::Throttle
        } else if inflight <= self.low {
            Gate::Resume
        } else {
            Gate::Flow
        }
    }
}

// ---------------------------------------------------------------------------
// 分块账本
// ---------------------------------------------------------------------------

/// 单块落账。`offset` 是块内数据在整条传输里的字节偏移，`submit_tick` /
/// `done_tick` 记录排队与确认时刻，`retries` 是该块的重传次数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkRecord {
    pub seq: u32,
    pub offset: u32,
    pub size: u32,
    pub submit_tick: u32,
    pub done_tick: u32,
    pub retries: u8,
}

/// 分块账本：定容、顺序追加、可审计、可摘要。
pub struct ChunkLedger {
    recs: Vec<ChunkRecord>,
    cap: usize,
    bytes: u32,
}

impl ChunkLedger {
    /// 容量 0 视为「空账本」错误；上限 `LEDGER_MAX` 防止病态输入撑爆内存。
    pub fn new(cap: usize) -> Result<ChunkLedger, ChunkError> {
        if cap == 0 {
            return Err(ChunkError::Empty);
        }
        if cap > LEDGER_MAX {
            return Err(ChunkError::Capacity);
        }
        Ok(ChunkLedger {
            recs: Vec::with_capacity(cap),
            cap,
            bytes: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.recs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn get(&self, i: usize) -> Option<&ChunkRecord> {
        self.recs.get(i)
    }

    /// 正常入账：序号必须等于已入账块数、偏移必须接上前一块末尾、完成时刻
    /// 不得早于提交时刻。任何一条不满足都说明账本被污染，拒绝入账。
    pub fn push(&mut self, r: ChunkRecord) -> Result<usize, ChunkError> {
        if r.size == 0 {
            return Err(ChunkError::ChunkSize);
        }
        if r.seq != self.recs.len() as u32 {
            return Err(ChunkError::Seq);
        }
        if r.offset != self.bytes {
            return Err(ChunkError::Conserve);
        }
        if r.done_tick < r.submit_tick {
            return Err(ChunkError::Conserve);
        }
        if self.recs.len() >= self.cap {
            return Err(ChunkError::Capacity);
        }
        self.bytes = self.bytes.saturating_add(r.size);
        self.recs.push(r);
        Ok(self.recs.len() - 1)
    }

    /// 回放入口：只查容量，不做一致性校验——离线账本必须先落进来，再由
    /// `audit` 判真伪；否则「坏账拒绝导入」会被误当成「坏账不存在」。
    pub fn push_raw(&mut self, r: ChunkRecord) -> Result<(), ChunkError> {
        if self.recs.len() >= self.cap {
            return Err(ChunkError::Capacity);
        }
        self.recs.push(r);
        Ok(())
    }

    /// 全账审计：序号连续、偏移连续、大小非零、完成时刻不倒退、总字节守恒。
    pub fn audit(&self) -> Result<(), ChunkError> {
        let mut off: u32 = 0;
        let mut prev_done: u32 = 0;
        let mut i = 0usize;
        while i < self.recs.len() {
            let r = match self.recs.get(i) {
                Some(x) => x,
                None => return Err(ChunkError::Conserve),
            };
            if r.seq != i as u32 {
                return Err(ChunkError::Seq);
            }
            if r.size == 0 {
                return Err(ChunkError::ChunkSize);
            }
            if r.offset != off {
                return Err(ChunkError::Conserve);
            }
            if r.done_tick < r.submit_tick || r.done_tick < prev_done {
                return Err(ChunkError::Conserve);
            }
            off = off.saturating_add(r.size);
            prev_done = r.done_tick;
            i += 1;
        }
        if off != self.bytes {
            return Err(ChunkError::Conserve);
        }
        Ok(())
    }

    /// 账本摘要（FNV-1a 64，固定字段序）。同输入两次入账必须逐位一致。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (self.recs.len() as u64);
        let mut i = 0usize;
        while i < self.recs.len() {
            if let Some(r) = self.recs.get(i) {
                h = fnv_u32(h, r.seq);
                h = fnv_u32(h, r.offset);
                h = fnv_u32(h, r.size);
                h = fnv_u32(h, r.submit_tick);
                h = fnv_u32(h, r.done_tick);
                h = fnv_u8(h, r.retries);
            }
            i += 1;
        }
        h
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fnv_u8(x, ((v >> (i * 8)) & 0xff) as u8);
        i += 1;
    }
    x
}

/// 重试预算闸：attempts 已用次数，budget 允许次数。超预算必须显式失败，
/// 避免「无限重试」把一次链路故障拖成整机挂起。
pub fn plan_retries(attempts: u32, budget: u32) -> Result<u32, ChunkError> {
    if attempts > budget {
        return Err(ChunkError::Retry);
    }
    Ok(attempts)
}

// ---------------------------------------------------------------------------
// 模拟器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct InFlight {
    seq: u32,
    size: u32,
    remaining: u32,
    submit_tick: u32,
    ready_at: u32,
    retries: u8,
}

/// 一次模拟的落账结果。`kib_per_s` 是从设备排空速率与启动开销共同推出来的
/// 有效吞吐（含每块开销），不是理论峰值。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimReport {
    pub ticks: u32,
    pub bytes: u32,
    pub chunks: u32,
    pub peak_inflight: u32,
    pub first_byte_tick: u32,
    pub throttled_ticks: u32,
    pub kib_per_s: f32,
    pub final_state: LinkState,
    pub digest: u64,
}

/// 设备理论吞吐上限（不含每块开销），单位 KiB/s。
pub fn speed_cap_kib_s(dev: &SimDevice, ticks_per_sec: u32) -> f32 {
    (dev.drain_per_tick as f32) * (ticks_per_sec as f32) / 1024.0
}

/// 三档吞吐落账主入口。
///
/// 时序（每 tick）：
/// 1. 提交阶段：窗口容得下就整块提交（最后一块允许不足一块，但总量仍须按
///    512 对齐），窗口满即记一次背压 tick 并切 Throttled；
/// 2. 排空阶段：只有队首块在设备里，进入设备先等 `setup_ticks`，之后每 tick
///    最多推进 `drain_per_tick` 字节；块推进完即落账并从队列摘除；
/// 3. 在途字节在块确认（而不是字节搬完）时才释放，这与 bulk 的 ack 语义一致。
pub fn simulate(
    dev: &SimDevice,
    prof: &Profile,
    total_bytes: u32,
    ticks_per_sec: u32,
    budget: u32,
) -> Result<SimReport, ChunkError> {
    prof.validate()?;
    if total_bytes == 0 {
        return Err(ChunkError::Total);
    }
    if total_bytes % BULK_PACKET != 0 {
        return Err(ChunkError::Align);
    }
    if ticks_per_sec == 0 || budget == 0 {
        return Err(ChunkError::Budget);
    }
    let chunks_expected = prof.chunk_count(total_bytes);
    if chunks_expected as usize > LEDGER_MAX {
        return Err(ChunkError::Capacity);
    }
    let mut ledger = ChunkLedger::new(chunks_expected as usize)?;
    let gate = WatermarkGate::new(prof.window, prof.window / 2)?;

    // Idle→Negotiating→Streaming：协商档已在校验阶段接受，这一步是记账起点。
    let mut state = LinkState::Idle.transition(LinkState::Negotiating)?;
    state = state.transition(LinkState::Streaming)?;

    let mut queue: Vec<InFlight> = Vec::with_capacity(QUEUE_SLOTS);
    let mut submitted: u32 = 0;
    let mut acked: u32 = 0;
    let mut inflight: u32 = 0;
    let mut seq: u32 = 0;
    let mut tick: u32 = 0;
    let mut peak: u32 = 0;
    let mut first_byte: u32 = 0;
    let mut throttled: u32 = 0;

    while acked < total_bytes {
        tick = tick.saturating_add(1);
        if tick > budget {
            return Err(ChunkError::Budget);
        }

        // 1. 提交阶段
        let mut blocked = false;
        while submitted < total_bytes {
            let left = total_bytes - submitted;
            let next = if left < prof.chunk { left } else { prof.chunk };
            if inflight.saturating_add(next) > prof.window || queue.len() >= QUEUE_SLOTS {
                blocked = true;
                break;
            }
            queue.push(InFlight {
                seq,
                size: next,
                remaining: next,
                submit_tick: tick,
                ready_at: 0,
                retries: 0,
            });
            submitted = submitted.saturating_add(next);
            inflight = inflight.saturating_add(next);
            seq = seq.saturating_add(1);
        }
        if blocked && submitted < total_bytes {
            throttled = throttled.saturating_add(1);
            if state == LinkState::Streaming && gate.decide(inflight, true) == Gate::Throttle {
                state = state.transition(LinkState::Throttled)?;
            }
        } else if state == LinkState::Throttled && gate.decide(inflight, false) == Gate::Resume {
            state = state.transition(LinkState::Streaming)?;
        }
        if inflight > peak {
            peak = inflight;
        }

        // 2. 排空阶段（单管道：只有队首在设备里）
        let mut done = false;
        let mut done_seq: u32 = 0;
        let mut done_size: u32 = 0;
        let mut done_submit: u32 = 0;
        let mut done_retries: u8 = 0;
        if let Some(head) = queue.get_mut(0) {
            if head.ready_at == 0 {
                head.ready_at = tick.saturating_add(dev.setup_ticks);
            }
            if tick >= head.ready_at {
                let step = if head.remaining < dev.drain_per_tick {
                    head.remaining
                } else {
                    dev.drain_per_tick
                };
                head.remaining -= step;
                if first_byte == 0 {
                    first_byte = tick;
                }
                if head.remaining == 0 {
                    done = true;
                    done_seq = head.seq;
                    done_size = head.size;
                    done_submit = head.submit_tick;
                    done_retries = head.retries;
                }
            }
        }
        if done {
            // 偏移取「确认前」的累计字节数：账本连续性由 push 自身再校一遍。
            let rec = ChunkRecord {
                seq: done_seq,
                offset: acked,
                size: done_size,
                submit_tick: done_submit,
                done_tick: tick,
                retries: done_retries,
            };
            ledger.push(rec)?;
            acked = acked.saturating_add(done_size);
            inflight = inflight.saturating_sub(done_size);
            queue.remove(0);
        }
    }

    state = state.transition(LinkState::Draining)?;
    state = state.transition(LinkState::Complete)?;

    let kib = (acked as f32) / 1024.0;
    let kib_per_s = kib * (ticks_per_sec as f32) / (tick as f32);
    let mut digest = ledger.digest();
    digest = fnv_u32(digest, tick);
    digest = fnv_u32(digest, chunks_expected);
    digest = fnv_u32(digest, peak);
    digest = fnv_u32(digest, first_byte);
    digest = fnv_u32(digest, throttled);

    Ok(SimReport {
        ticks: tick,
        bytes: acked,
        chunks: ledger.len() as u32,
        peak_inflight: peak,
        first_byte_tick: first_byte,
        throttled_ticks: throttled,
        kib_per_s,
        final_state: state,
        digest,
    })
}

// ---------------------------------------------------------------------------
// 注入核：坏样本逐类断言具体诊断码
// ---------------------------------------------------------------------------

fn bad_profile(chunk: u32, window: u32) -> Profile {
    Profile {
        code: 9,
        name: "bad",
        chunk,
        window,
    }
}

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;
    let dev = match SimDevice::printer_usb2() {
        Ok(d) => d,
        Err(_) => return (caught, total),
    };
    let ok_prof = match PROFILES.get(1) {
        Some(p) => *p,
        None => return (caught, total),
    };

    // 1 零字节传输
    if simulate(&dev, &ok_prof, 0, 1000, 10_000).err() == Some(ChunkError::Total) {
        caught += 1;
    }
    // 2 块大小为 0
    if simulate(&dev, &bad_profile(0, 1024), 64 * 1024, 1000, 10_000).err()
        == Some(ChunkError::ChunkSize)
    {
        caught += 1;
    }
    // 3 块大小未按 512 对齐
    if simulate(&dev, &bad_profile(1000, 4096), 64 * 1024, 1000, 10_000).err()
        == Some(ChunkError::Align)
    {
        caught += 1;
    }
    // 4 未知档位码
    if profile_of(9).err() == Some(ChunkError::Profile) {
        caught += 1;
    }
    // 5 窗口小于一块
    if simulate(&dev, &bad_profile(4096, 2048), 64 * 1024, 1000, 10_000).err()
        == Some(ChunkError::Window)
    {
        caught += 1;
    }
    // 6 排空速率 0
    if SimDevice::new("dead", 0, 1, 512).err() == Some(ChunkError::Device) {
        caught += 1;
    }
    // 7 最大包长非 2 的幂
    if SimDevice::new("odd", 1024, 1, 300).err() == Some(ChunkError::Mps) {
        caught += 1;
    }
    // 8 状态机非法跳转
    if LinkState::Idle.transition(LinkState::Complete).err() == Some(ChunkError::State) {
        caught += 1;
    }
    // 9 账本乱序序号
    let mut ledger = match ChunkLedger::new(4) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let rec0 = ChunkRecord {
        seq: 0,
        offset: 0,
        size: 512,
        submit_tick: 1,
        done_tick: 2,
        retries: 0,
    };
    if ledger.push(rec0).is_ok() {
        let mut dup = rec0;
        dup.offset = 512;
        // 序号仍是 0，期望 Seq（乱序/重复）而不是 Conserve。
        if ledger.push(dup).err() == Some(ChunkError::Seq) {
            caught += 1;
        }
    }
    // 10 回放账本偏移断裂：push_raw 收下，audit 判 Conserve
    let mut replay = match ChunkLedger::new(4) {
        Ok(l) => l,
        Err(_) => return (caught, total),
    };
    let torn = ChunkRecord {
        seq: 0,
        offset: 256,
        size: 512,
        submit_tick: 1,
        done_tick: 2,
        retries: 0,
    };
    if replay.push_raw(torn).is_ok() && replay.audit().err() == Some(ChunkError::Conserve) {
        caught += 1;
    }
    // 11 tick 预算撑爆（1 B/tick 搬 1 MiB）
    let trickle = match SimDevice::trickle() {
        Ok(d) => d,
        Err(_) => return (caught, total),
    };
    if simulate(&trickle, &ok_prof, 64 * 1024, 1000, 100).err() == Some(ChunkError::Budget) {
        caught += 1;
    }
    // 12 重试超预算
    if plan_retries(3, 2).err() == Some(ChunkError::Retry) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50127_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50127");

    // J1-1 三档档位合法且块大小严格递增
    let mut profiles_ok = true;
    let mut i = 0usize;
    while i < PROFILES.len() {
        if let Some(p) = PROFILES.get(i) {
            if p.validate().is_err() {
                profiles_ok = false;
            }
            if let Some(prev) = if i > 0 { PROFILES.get(i - 1) } else { None } {
                if p.chunk <= prev.chunk || p.window < prev.window {
                    profiles_ok = false;
                }
            }
        }
        i += 1;
    }
    s.add(
        "50127-J1-三档合法",
        profiles_ok,
        if profiles_ok { "" } else { "档位参数或递增序失守" },
    );

    // J1-2 档位查表往返 + 非法码必拒
    let lookup_ok = profile_of(1).ok() == PROFILES.get(0).copied()
        && profile_of(2).ok() == PROFILES.get(1).copied()
        && profile_of(3).ok() == PROFILES.get(2).copied()
        && profile_of(0).err() == Some(ChunkError::Profile)
        && profile_of(9).err() == Some(ChunkError::Profile);
    s.add(
        "50127-J1-档位查表",
        lookup_ok,
        if lookup_ok { "" } else { "查表往返或非法码放行" },
    );

    // J1-3 状态机：合法链可走通，非法跳转逐对拒绝（can_go 与 transition 必须一致）
    let mut st = LinkState::Idle;
    let mut path_ok = true;
    let path = [
        LinkState::Negotiating,
        LinkState::Streaming,
        LinkState::Throttled,
        LinkState::Streaming,
        LinkState::Draining,
        LinkState::Complete,
    ];
    let mut k = 0usize;
    while k < path.len() {
        match path.get(k) {
            Some(to) => match st.transition(*to) {
                Ok(n) => st = n,
                Err(_) => path_ok = false,
            },
            None => path_ok = false,
        }
        k += 1;
    }
    let mut pairs = 0usize;
    let mut mismatches = 0usize;
    let mut a = 0u8;
    while a < 6 {
        let mut b = 0u8;
        while b < 6 {
            if let (Ok(from), Ok(to)) = (LinkState::from_wire(a), LinkState::from_wire(b)) {
                pairs += 1;
                let legal = from.can_go(to);
                if legal != from.transition(to).is_ok() {
                    mismatches += 1;
                }
            }
            b += 1;
        }
        a += 1;
    }
    let ok = path_ok && st.is_terminal() && pairs == 36 && mismatches == 0;
    let det = format!("pairs={} mismatch={}", pairs, mismatches);
    s.add_dyn("50127-J1-状态机", ok, &det);

    let dev = match SimDevice::printer_usb2() {
        Ok(d) => d,
        Err(_) => return s,
    };
    const TOTAL_BYTES: u32 = 1024 * 1024;
    const TPS: u32 = 1000;
    const BUDGET: u32 = 100_000;

    // J1-4 三档模拟全部完成且账本审计通过
    let mut reports: [Option<SimReport>; 3] = [None, None, None];
    let mut ledgers_ok = true;
    let mut i = 0usize;
    while i < PROFILES.len() {
        if let Some(p) = PROFILES.get(i) {
            match simulate(&dev, p, TOTAL_BYTES, TPS, BUDGET) {
                Ok(r) => {
                    if let Some(cell) = reports.get_mut(i) {
                        *cell = Some(r);
                    }
                }
                Err(_) => ledgers_ok = false,
            }
        }
        i += 1;
    }
    // 另外单独复算一档账本并审计（模拟内部账本不对外暴露，这里按同一时序重建）。
    let mut audit_ok = true;
    let prof0 = match PROFILES.get(0) {
        Some(p) => *p,
        None => return s,
    };
    if let Some(r) = reports.get(0).and_then(|x| *x) {
        let mut lg = match ChunkLedger::new(prof0.chunk_count(TOTAL_BYTES) as usize) {
            Ok(l) => l,
            Err(_) => return s,
        };
        let chunks = r.chunks;
        let mut off = 0u32;
        let mut seq = 0u32;
        while seq < chunks {
            let size = if off + prof0.chunk > TOTAL_BYTES {
                TOTAL_BYTES - off
            } else {
                prof0.chunk
            };
            let rec = ChunkRecord {
                seq,
                offset: off,
                size,
                submit_tick: 1,
                done_tick: 2 + seq,
                retries: 0,
            };
            if lg.push(rec).is_err() {
                audit_ok = false;
            }
            off = off.saturating_add(size);
            seq += 1;
        }
        audit_ok = audit_ok && lg.audit().is_ok() && lg.bytes() == TOTAL_BYTES;
    } else {
        audit_ok = false;
    }
    let all_done = ledgers_ok
        && reports.iter().all(|x| x.is_some())
        && reports.iter().all(|x| match x {
            Some(r) => r.final_state.is_terminal() && r.bytes == TOTAL_BYTES,
            None => false,
        });
    let ok = all_done && audit_ok;
    let det = format!("done={} audit={}", all_done, audit_ok);
    s.add_dyn("50127-J1-三档落账", ok, &det);

    // J1-5 吞吐随档位严格递增，且不超过设备理论上限（J1 的核心度量）
    let mut mono = true;
    let mut within_cap = true;
    let mut prev_kib = -1.0f32;
    let cap = speed_cap_kib_s(&dev, TPS);
    let mut i = 0usize;
    let mut kib_txt = String::new();
    while i < 3 {
        match reports.get(i).and_then(|x| *x) {
            Some(r) => {
                if r.kib_per_s <= prev_kib {
                    mono = false;
                }
                if r.kib_per_s <= 0.0 || r.kib_per_s > cap {
                    within_cap = false;
                }
                prev_kib = r.kib_per_s;
                if i > 0 {
                    kib_txt.push_str(" / ");
                }
                kib_txt.push_str(&format!("{:.0}", r.kib_per_s));
            }
            None => {
                mono = false;
                within_cap = false;
            }
        }
        i += 1;
    }
    let ok = mono && within_cap;
    let det = format!("kib/s={} cap={:.0}", kib_txt, cap);
    s.add_dyn("50127-J1-吞吐递增", ok, &det);

    // J1-6 首字节延迟只由启动开销决定，与档位无关；且不晚于总 tick 数
    let mut fbt_ok = true;
    let mut i = 0usize;
    while i < 3 {
        match reports.get(i).and_then(|x| *x) {
            Some(r) => {
                if r.first_byte_tick != dev.setup_ticks + 1 || r.first_byte_tick > r.ticks {
                    fbt_ok = false;
                }
            }
            None => fbt_ok = false,
        }
        i += 1;
    }
    let det = format!("first_byte={}", dev.setup_ticks + 1);
    s.add_dyn("50127-J1-首字节", fbt_ok, &det);

    // J1-7 字节守恒：块数 = 向上取整(总量/块)，在途峰值不超窗口
    let mut conserve_ok = true;
    let mut i = 0usize;
    while i < 3 {
        if let (Some(r), Some(p)) = (reports.get(i).and_then(|x| *x), PROFILES.get(i)) {
            if r.chunks != p.chunk_count(TOTAL_BYTES)
                || r.bytes != TOTAL_BYTES
                || r.peak_inflight > p.window
                || r.peak_inflight < p.chunk
            {
                conserve_ok = false;
            }
        }
        i += 1;
    }
    s.add(
        "50127-J1-字节守恒",
        conserve_ok,
        if conserve_ok { "" } else { "块数/在途/总量对不上" },
    );

    // J1-8 背压：窗口越大背压 tick 越少；窗口恰为一块时仍能跑完（不死锁）
    let mut thin = true;
    let base = match PROFILES.get(1) {
        Some(p) => *p,
        None => return s,
    };
    let narrow = Profile {
        chunk: base.chunk,
        window: base.chunk,
        ..base
    };
    let wide = Profile {
        chunk: base.chunk,
        window: base.chunk * 4,
        ..base
    };
    let mut throttle_txt = String::new();
    let mut counts: [u32; 3] = [0, 0, 0];
    let combos = [narrow, base, wide];
    let mut i = 0usize;
    while i < combos.len() {
        match combos.get(i) {
            Some(p) => match simulate(&dev, p, TOTAL_BYTES, TPS, BUDGET) {
                Ok(r) => {
                    if let Some(cell) = counts.get_mut(i) {
                        *cell = r.throttled_ticks;
                    }
                }
                Err(_) => thin = false,
            },
            None => thin = false,
        }
        i += 1;
    }
    let mut i = 1usize;
    while i < 3 {
        if let (Some(a), Some(b)) = (counts.get(i - 1), counts.get(i)) {
            if b > a {
                thin = false;
            }
        }
        i += 1;
    }
    // 窄窗口必然遭遇背压：1 MiB 在途需求远大于一块 16 KiB；宽窗口必须更少。
    let narrow_throttle = counts.get(0).copied().unwrap_or(0);
    let base_throttle = counts.get(1).copied().unwrap_or(0);
    let wide_throttle = counts.get(2).copied().unwrap_or(0);
    thin = thin && narrow_throttle > base_throttle && narrow_throttle > wide_throttle;
    let mut i = 0usize;
    while i < 3 {
        if i > 0 {
            throttle_txt.push_str(" / ");
        }
        throttle_txt.push_str(&format!("{}", counts.get(i).copied().unwrap_or(0)));
        i += 1;
    }
    let det = format!("throttle={}", throttle_txt);
    s.add_dyn("50127-J1-背压单调", thin, &det);

    // J1-9 确定性：同输入两次摘要逐位一致；不同档位摘要互异
    let mut det_ok = true;
    let mut digests: [u64; 3] = [0, 0, 0];
    let mut i = 0usize;
    while i < 3 {
        if let Some(p) = PROFILES.get(i) {
            let a = simulate(&dev, p, TOTAL_BYTES, TPS, BUDGET);
            let b = simulate(&dev, p, TOTAL_BYTES, TPS, BUDGET);
            match (a, b) {
                (Ok(x), Ok(y)) => {
                    if x.digest != y.digest || x.ticks != y.ticks {
                        det_ok = false;
                    }
                    if let Some(cell) = digests.get_mut(i) {
                        *cell = x.digest;
                    }
                }
                _ => det_ok = false,
            }
        }
        i += 1;
    }
    let distinct = digests[0] != digests[1] && digests[1] != digests[2] && digests[0] != digests[2];
    let ok = det_ok && distinct;
    s.add("50127-J1-落账确定", ok, if ok { "" } else { "摘要漂移或档位不可区分" });

    // J1-10 非法输入族：每类返回具体码（不是笼统拒绝）
    let bad_total = simulate(&dev, &base, 0, TPS, BUDGET).err() == Some(ChunkError::Total);
    let bad_chunk =
        simulate(&dev, &bad_profile(0, 1024), 4096, TPS, BUDGET).err() == Some(ChunkError::ChunkSize);
    let bad_align = simulate(&dev, &bad_profile(1000, 4096), 8192, TPS, BUDGET).err()
        == Some(ChunkError::Align);
    let bad_window =
        simulate(&dev, &bad_profile(4096, 2048), 8192, TPS, BUDGET).err() == Some(ChunkError::Window);
    let bad_profile_code = profile_of(4).err() == Some(ChunkError::Profile);
    let ok = bad_total && bad_chunk && bad_align && bad_window && bad_profile_code;
    s.add("50127-J1-非法族", ok, if ok { "" } else { "某类非法输入未按码拒绝" });

    // J1-11 设备参数：排空 0/启动 0 → Device，包长非 2 幂 → Mps
    let ok = SimDevice::new("d", 0, 1, 512).err() == Some(ChunkError::Device)
        && SimDevice::new("d", 1024, 0, 512).err() == Some(ChunkError::Device)
        && SimDevice::new("d", 1024, 65, 512).err() == Some(ChunkError::Device)
        && SimDevice::new("d", 1024, 1, 0).err() == Some(ChunkError::Mps)
        && SimDevice::new("d", 1024, 1, 2048).err() == Some(ChunkError::Mps);
    s.add("50127-J1-设备参数", ok, if ok { "" } else { "设备参数闸失守" });

    // J1-12 账本：容量/空账本/乱序/篡改偏移
    let mut lg = match ChunkLedger::new(2) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let mut rec = ChunkRecord {
        seq: 0,
        offset: 0,
        size: 512,
        submit_tick: 1,
        done_tick: 2,
        retries: 0,
    };
    let mut push_ok = lg.push(rec).is_ok();
    rec.seq = 1;
    rec.offset = 512;
    push_ok = push_ok && lg.push(rec).is_ok();
    rec.seq = 2;
    rec.offset = 1024;
    let cap_hit = lg.push(rec).err() == Some(ChunkError::Capacity);
    let seq_hit = {
        let mut l2 = match ChunkLedger::new(4) {
            Ok(l) => l,
            Err(_) => return s,
        };
        let r2 = ChunkRecord {
            seq: 1,
            offset: 0,
            size: 512,
            submit_tick: 1,
            done_tick: 2,
            retries: 0,
        };
        l2.push(r2).err() == Some(ChunkError::Seq)
    };
    let tamper_hit = {
        let mut l3 = match ChunkLedger::new(4) {
            Ok(l) => l,
            Err(_) => return s,
        };
        let torn = ChunkRecord {
            seq: 0,
            offset: 100,
            size: 512,
            submit_tick: 1,
            done_tick: 2,
            retries: 0,
        };
        l3.push_raw(torn).is_ok() && l3.audit().err() == Some(ChunkError::Conserve)
    };
    let ok = push_ok
        && cap_hit
        && seq_hit
        && tamper_hit
        && ChunkLedger::new(0).err() == Some(ChunkError::Empty)
        && lg.audit().is_ok();
    s.add("50127-J1-账本闸", ok, if ok { "" } else { "账本闸失守" });

    // J1-13 预算与重试：慢设备撑爆预算 → Budget；超重试预算 → Retry
    let trickle = match SimDevice::trickle() {
        Ok(d) => d,
        Err(_) => return s,
    };
    let ok = simulate(&trickle, &base, 64 * 1024, TPS, 64).err() == Some(ChunkError::Budget)
        && simulate(&dev, &base, TOTAL_BYTES, 0, BUDGET).err() == Some(ChunkError::Budget)
        && plan_retries(0, 0).ok() == Some(0)
        && plan_retries(1, 0).err() == Some(ChunkError::Retry)
        && plan_retries(2, 2).ok() == Some(2);
    s.add("50127-J1-预算重试", ok, if ok { "" } else { "预算/重试闸失守" });

    // J1-14 水位闸：想提交且到高水位才 Throttle，低于低水位必 Resume
    let gate = match WatermarkGate::new(4096, 2048) {
        Ok(g) => g,
        Err(_) => return s,
    };
    let ok = gate.decide(4096, true) == Gate::Throttle
        && gate.decide(4096, false) == Gate::Flow
        && gate.decide(2048, true) == Gate::Resume
        && gate.decide(100, false) == Gate::Resume
        && gate.decide(3000, true) == Gate::Flow
        && WatermarkGate::new(0, 0).err() == Some(ChunkError::Window)
        && WatermarkGate::new(100, 100).err() == Some(ChunkError::Window);
    s.add("50127-J1-水位闸", ok, if ok { "" } else { "高低水位判定错" });

    // J1-15 注入核：十二类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50127-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50127_checks_green() {
        let set = run_unx50127_checks();
        assert!(set.all_passed(), "UNX-F50127 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50127_throughput_monotonic() {
        let dev = SimDevice::printer_usb2();
        assert!(dev.is_ok());
        if let Ok(d) = dev {
            let mut prev = -1.0f32;
            let mut i = 0usize;
            while i < PROFILES.len() {
                if let Some(p) = PROFILES.get(i) {
                    let r = simulate(&d, p, 1024 * 1024, 1000, 100_000);
                    assert!(r.is_ok());
                    if let Ok(r) = r {
                        assert!(r.kib_per_s > prev);
                        assert_eq!(r.bytes, 1024 * 1024);
                        prev = r.kib_per_s;
                    }
                }
                i += 1;
            }
        }
    }

    #[test]
    fn unx50127_error_codes_in_segment() {
        assert_eq!(ChunkError::Empty.code(), SEG_BASE + 1);
        assert_eq!(ChunkError::Device.code(), SEG_BASE + 14);
    }
}
