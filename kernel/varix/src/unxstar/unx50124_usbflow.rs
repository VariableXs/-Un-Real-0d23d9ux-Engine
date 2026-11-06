//! UNX-F50124 USB 打印流控（bulk 端点 STALL/NAK 处理账）（0x43ba0 段）
//!
//! 锚点：UNX-F50124-J1 STALL 注入 ×20 恢复路径零死锁；判据锚定 §AI-63 §2 专题八。
//!
//! bulk 管道没有重传定时器，只有三种「不前进」的信号：NAK（设备暂时收不了）、
//! STALL（端点 halt，必须主机发 CLEAR_FEATURE 才能解）、超时（硬件层面的沉默）。
//! 流控器的职责不是「等」而是**记账并收敛**：
//!
//! * NAK 必须 1:1 转成 retry 计数，超过预算立即 `TimedOut`，绝不裸循环；
//! * STALL 必须走 `Stalled → Recovering → Ready` 的显式恢复路径，
//!   期间收到的数据包一律拒绝（halt 未清时设备不可能给出有效载荷）；
//! * 脚本跑完仍停在 Naked/Stalled/Recovering 就是死锁，必须被 `run_script`
//!   当场标出（`deadlock = true`），而不是让上层等一个永远不来的事件；
//! * 恢复成功（CLEAR_FEATURE 之后重新收到 ACK）才算 `recovered`，只清 halt
//!   不算——halt 清了但数据仍不来，等于没恢复。
//!
//! 数据结构：
//! * `BulkEp` —— 端点参数（地址/包长/NAK 预算/STALL 预算）；
//! * `FlowController` —— 端点级状态机（Ready/Naked/Stalled/Recovering/Done/
//!   Halted/TimedOut）+ 计数；
//! * `TxnOutcome` —— 一次脚本执行的可复算结果（含 deadlock 标志）；
//! * `FlowLedger` —— 定容、按事务号去重的流控账（累计 stall/recovered/deadlock）。
//!
//! 错误闭集：`SEG_BASE + n`（0x43ba0 段），所有码值都被真实构造。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43ba0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43ba0;

pub const E_EMPTY: u16 = 1;
pub const E_PACKET: u16 = 2;
pub const E_ADDR: u16 = 3;
pub const E_ZERO_CHUNK: u16 = 4;
pub const E_OVER_MAX: u16 = 5;
pub const E_STALL: u16 = 6;
pub const E_NAK_BUDGET: u16 = 7;
pub const E_TIMEOUT: u16 = 8;
pub const E_BAD_STATE: u16 = 9;
pub const E_CAPACITY: u16 = 10;
pub const E_DUP_TXN: u16 = 11;
pub const E_HALTED: u16 = 12;
pub const E_PHASE: u16 = 13;

/// 单包上限：高速 bulk 的 wMaxPacketSize 上限（USB 2.0 §5.8.3）。
pub const MAX_PACKET: u16 = 1024;
/// NAK 预算上限（16 次）。超过 16 次说明设备已经不在「暂时忙」的语义里。
pub const MAX_NAK_LIMIT: u16 = 16;
/// STALL 预算上限（4 次）。同一事务里 halt 反复出现，基本可以判定设备固件异常。
pub const MAX_STALL_LIMIT: u16 = 4;

/// 流控错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowError {
    Empty,
    Packet,
    Addr,
    ZeroChunk,
    OverMax,
    Stall,
    NakBudget,
    Timeout,
    BadState,
    Capacity,
    DupTxn,
    Halted,
    Phase,
}

impl FlowError {
    pub fn code(self) -> u32 {
        let n = match self {
            FlowError::Empty => E_EMPTY,
            FlowError::Packet => E_PACKET,
            FlowError::Addr => E_ADDR,
            FlowError::ZeroChunk => E_ZERO_CHUNK,
            FlowError::OverMax => E_OVER_MAX,
            FlowError::Stall => E_STALL,
            FlowError::NakBudget => E_NAK_BUDGET,
            FlowError::Timeout => E_TIMEOUT,
            FlowError::BadState => E_BAD_STATE,
            FlowError::Capacity => E_CAPACITY,
            FlowError::DupTxn => E_DUP_TXN,
            FlowError::Halted => E_HALTED,
            FlowError::Phase => E_PHASE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FlowError::Empty => "empty probe script",
            FlowError::Packet => "max packet size not a power of two in 8..=1024",
            FlowError::Addr => "bulk OUT address invalid or IN-directioned",
            FlowError::ZeroChunk => "zero-length ACK is not a data packet",
            FlowError::OverMax => "ACK longer than MAX_PACKET",
            FlowError::Stall => "endpoint stalled, halt not cleared",
            FlowError::NakBudget => "NAK budget exhausted",
            FlowError::Timeout => "transfer timed out",
            FlowError::BadState => "event illegal in current flow state",
            FlowError::Capacity => "flow ledger full or parameter out of range",
            FlowError::DupTxn => "transaction id recorded twice",
            FlowError::Halted => "stall budget exhausted, endpoint halted",
            FlowError::Phase => "transfer ended in a non-terminal phase",
        }
    }
}

/// 端点参数。`fix_packet` 之外的字段都参与判据，不做默认值兜底。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkEp {
    pub addr: u8,
    pub max_packet: u16,
    pub nak_limit: u16,
    pub stall_limit: u16,
}

impl BulkEp {
    /// 校验并构造。打印数据管道必须是 OUT 方向（bit7=0）、端点号非 0、
    /// 保留位为 0；包长 2 的幂；预算 1..=上限。
    pub fn new(
        addr: u8,
        max_packet: u16,
        nak_limit: u16,
        stall_limit: u16,
    ) -> Result<BulkEp, FlowError> {
        if addr == 0 || addr & 0x70 != 0 || addr & 0x80 != 0 {
            return Err(FlowError::Addr);
        }
        let v = max_packet;
        let pow2 = v >= 8 && v <= MAX_PACKET && (v & (v - 1)) == 0;
        if !pow2 {
            return Err(FlowError::Packet);
        }
        if nak_limit == 0 || nak_limit > MAX_NAK_LIMIT {
            return Err(FlowError::BadState);
        }
        if stall_limit == 0 || stall_limit > MAX_STALL_LIMIT {
            return Err(FlowError::BadState);
        }
        Ok(BulkEp { addr, max_packet, nak_limit, stall_limit })
    }

    pub fn label(&self) -> &'static str {
        "bulk-out"
    }
}

/// 线缆上观测到的一次结果（脚本事件）。真实驱动里由 HCD 完成回调产生，
/// 这里显式列举，保证每个分支都能被脚本化测试覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireEvent {
    /// 收到一个数据包（长度 < 包长即短包，事务正常终止）。
    Ack { bytes: u16 },
    /// 设备忙（NAK）。
    Nak,
    /// 端点 halt（STALL）。
    Stall,
    /// 主机发 CLEAR_FEATURE(ENDPOINT_HALT)。
    ClearFeature,
    /// 硬件超时。
    Timeout,
    /// 零长度包（精确整数倍传输的收尾）。
    Zlp,
}

impl WireEvent {
    pub fn label(self) -> &'static str {
        match self {
            WireEvent::Ack { .. } => "ack",
            WireEvent::Nak => "nak",
            WireEvent::Stall => "stall",
            WireEvent::ClearFeature => "clear-feature",
            WireEvent::Timeout => "timeout",
            WireEvent::Zlp => "zlp",
        }
    }
}

/// 流控状态。`Naked`/`Stalled`/`Recovering` 都是**暂态**：脚本结束时停在
/// 它们身上即判死锁。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowState {
    Ready,
    Naked,
    Stalled,
    Recovering,
    Done,
    Halted,
    TimedOut,
}

impl FlowState {
    pub fn is_transient(self) -> bool {
        matches!(self, FlowState::Naked | FlowState::Stalled | FlowState::Recovering)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, FlowState::Done | FlowState::Halted | FlowState::TimedOut)
    }

    pub fn label(self) -> &'static str {
        match self {
            FlowState::Ready => "ready",
            FlowState::Naked => "naked",
            FlowState::Stalled => "stalled",
            FlowState::Recovering => "recovering",
            FlowState::Done => "done",
            FlowState::Halted => "halted",
            FlowState::TimedOut => "timed-out",
        }
    }
}

/// 端点级流控器。所有计数都是 u32/u16，不用浮点——恢复路径必须逐位可复算。
pub struct FlowController {
    ep: BulkEp,
    state: FlowState,
    bytes: u32,
    packets: u16,
    naks: u16,
    retries: u16,
    stalls: u16,
    recovered: u16,
    zlp: u16,
    steps: u16,
}

impl FlowController {
    pub fn new(ep: BulkEp) -> FlowController {
        FlowController {
            ep,
            state: FlowState::Ready,
            bytes: 0,
            packets: 0,
            naks: 0,
            retries: 0,
            stalls: 0,
            recovered: 0,
            zlp: 0,
            steps: 0,
        }
    }

    pub fn state(&self) -> FlowState {
        self.state
    }

    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    pub fn stalls(&self) -> u16 {
        self.stalls
    }

    pub fn recovered(&self) -> u16 {
        self.recovered
    }

    /// 推进一步。返回 `Err` 时状态已落到一个**终态或暂态**，绝不留「已报错但
    /// 状态未定义」的组合——那种组合正是死锁的温床。
    pub fn step(&mut self, ev: WireEvent) -> Result<(), FlowError> {
        if self.state == FlowState::Done {
            return Err(FlowError::Phase);
        }
        if self.state == FlowState::Halted || self.state == FlowState::TimedOut {
            return Err(FlowError::Phase);
        }
        match ev {
            WireEvent::Ack { bytes } => {
                if bytes == 0 {
                    return Err(FlowError::ZeroChunk);
                }
                if bytes > self.ep.max_packet {
                    return Err(FlowError::OverMax);
                }
                // halt 未清时收到「数据」：这不是数据，是控制器把别的东西当成了数据。
                if self.state == FlowState::Stalled {
                    return Err(FlowError::Stall);
                }
                self.bytes = self.bytes.saturating_add(bytes as u32);
                self.packets = self.packets.saturating_add(1);
                self.state = if bytes < self.ep.max_packet {
                    FlowState::Done
                } else if self.state == FlowState::Recovering {
                    // 恢复成功的定义：清 halt 之后重新收到完整包。
                    self.recovered = self.recovered.saturating_add(1);
                    FlowState::Ready
                } else {
                    FlowState::Ready
                };
            }
            WireEvent::Nak => {
                if self.state == FlowState::Stalled {
                    return Err(FlowError::Stall);
                }
                self.naks = self.naks.saturating_add(1);
                self.retries = self.retries.saturating_add(1);
                if self.naks > self.ep.nak_limit {
                    self.state = FlowState::TimedOut;
                    return Err(FlowError::NakBudget);
                }
                self.state = FlowState::Naked;
            }
            WireEvent::Stall => {
                self.stalls = self.stalls.saturating_add(1);
                if self.stalls > self.ep.stall_limit {
                    self.state = FlowState::Halted;
                    return Err(FlowError::Halted);
                }
                self.state = FlowState::Stalled;
            }
            WireEvent::ClearFeature => {
                if self.state != FlowState::Stalled {
                    return Err(FlowError::BadState);
                }
                self.state = FlowState::Recovering;
            }
            WireEvent::Timeout => {
                if self.state == FlowState::Stalled {
                    // halt 期间等不到 CLEAR_FEATURE：恢复超时，端点保持 halted。
                    self.state = FlowState::Halted;
                    return Err(FlowError::Stall);
                }
                self.state = FlowState::TimedOut;
                return Err(FlowError::Timeout);
            }
            WireEvent::Zlp => {
                if self.state != FlowState::Ready && self.state != FlowState::Recovering {
                    return Err(FlowError::BadState);
                }
                self.zlp = self.zlp.saturating_add(1);
                self.state = FlowState::Done;
            }
        }
        Ok(())
    }
}

/// 一次脚本执行的结果。`deadlock` 是 J1 的核心指标：脚本走完（或中途报错）
/// 时仍停在暂态即为死锁。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TxnOutcome {
    pub state: FlowState,
    pub bytes: u32,
    pub packets: u16,
    pub naks: u16,
    pub retries: u16,
    pub stalls: u16,
    pub recovered: u16,
    pub steps: u16,
    pub deadlock: bool,
    pub error: Option<FlowError>,
}

impl TxnOutcome {
    fn failed(e: FlowError) -> TxnOutcome {
        TxnOutcome {
            state: FlowState::Ready,
            bytes: 0,
            packets: 0,
            naks: 0,
            retries: 0,
            stalls: 0,
            recovered: 0,
            steps: 0,
            deadlock: false,
            error: Some(e),
        }
    }

    pub fn ok(&self) -> bool {
        self.error.is_none() && !self.deadlock && self.state == FlowState::Done
    }

    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_u8(h, state_code(self.state));
        h = fnv_u32(h, self.bytes);
        h = fnv_u16(h, self.packets);
        h = fnv_u16(h, self.naks);
        h = fnv_u16(h, self.retries);
        h = fnv_u16(h, self.stalls);
        h = fnv_u16(h, self.recovered);
        h = fnv_u16(h, self.steps);
        h = fnv_u8(h, if self.deadlock { 1 } else { 0 });
        let code = match self.error {
            Some(e) => (e.code() - SEG_BASE) as u8,
            None => 0,
        };
        fnv_u8(h, code)
    }
}

fn state_code(s: FlowState) -> u8 {
    match s {
        FlowState::Ready => 1,
        FlowState::Naked => 2,
        FlowState::Stalled => 3,
        FlowState::Recovering => 4,
        FlowState::Done => 5,
        FlowState::Halted => 6,
        FlowState::TimedOut => 7,
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_u8(fnv_u8(h, (v & 0xff) as u8), (v >> 8) as u8)
}

fn fnv_u32(h: u64, v: u32) -> u64 {
    fnv_u16(fnv_u16(h, (v & 0xffff) as u16), (v >> 16) as u16)
}

/// 执行脚本。步数预算 = 脚本长度 + 8（每个事件正好一步，余量只用于检测
/// 「事件处理里再自旋」这类实现错误）。
pub fn run_script(ep: &BulkEp, script: &[WireEvent]) -> TxnOutcome {
    if script.is_empty() {
        return TxnOutcome::failed(FlowError::Empty);
    }
    let mut c = FlowController::new(*ep);
    let budget = script.len().saturating_add(8);
    let mut err: Option<FlowError> = None;
    let mut i = 0usize;
    while i < script.len() && i < budget {
        let ev = match script.get(i) {
            Some(v) => *v,
            None => break,
        };
        c.steps = c.steps.saturating_add(1);
        if let Err(e) = c.step(ev) {
            err = Some(e);
            break;
        }
        i += 1;
    }
    let mut deadlock = false;
    if c.state.is_transient() {
        // 停在暂态：无论是脚本自然结束还是错误中断，都没有人能再推进它。
        deadlock = true;
        if err.is_none() {
            err = Some(FlowError::Stall);
        }
    } else if err.is_none() && c.state != FlowState::Done {
        // 非终态且无错：传输没收尾（例如整倍数传输缺 ZLP）。
        err = Some(FlowError::Phase);
    }
    TxnOutcome {
        state: c.state,
        bytes: c.bytes,
        packets: c.packets,
        naks: c.naks,
        retries: c.retries,
        stalls: c.stalls,
        recovered: c.recovered,
        steps: c.steps,
        deadlock,
        error: err,
    }
}

/// 把一次传输切成整包 + 是否需要 ZLP。
///
/// 规则来自 bulk 语义：短包即终止，整倍数传输必须补一个 ZLP，否则设备
/// 会一直等下一个包。0 字节传输也需要 ZLP（空传输同样要「收尾」）。
pub fn packetize(total: u32, max_packet: u16) -> Result<(u32, bool), FlowError> {
    if max_packet == 0 {
        return Err(FlowError::Packet);
    }
    let mp = max_packet as u32;
    if total == 0 {
        return Ok((0, true));
    }
    let rem = total % mp;
    let packets = total / mp + if rem == 0 { 0 } else { 1 };
    Ok((packets, rem == 0))
}

// ---------------------------------------------------------------------------
// STALL 脚本族：20 条恢复路径
// ---------------------------------------------------------------------------

/// 第 i 条 STALL 脚本（i < 20）。
///
/// 结构：若干完整包 → STALL → CLEAR_FEATURE → 完整包（恢复确认）→ 短包收尾。
/// 变量（包数、STALL 前的 NAK 数、短包长度）覆盖 20 种位置组合，但恢复路径
/// 完全相同——J1 要证的就是「位置变了，收敛性不变」。
pub fn stall_script(i: usize) -> Option<Vec<WireEvent>> {
    if i >= 20 {
        return None;
    }
    let mp = 512u16;
    let chunks = 1 + (i % 3);
    let naks_before = (i % 3) as u16;
    let mut v = Vec::with_capacity(10);
    let mut n = 0u16;
    while n < naks_before {
        v.push(WireEvent::Nak);
        n += 1;
    }
    let mut c = 0usize;
    while c < chunks {
        v.push(WireEvent::Ack { bytes: mp });
        c += 1;
    }
    v.push(WireEvent::Stall);
    v.push(WireEvent::ClearFeature);
    v.push(WireEvent::Ack { bytes: mp });
    v.push(WireEvent::Ack { bytes: (i as u16 % 7) + 1 });
    Some(v)
}

/// STALL 脚本的字节期望：(chunks + 1) × 512 + 短包长度。
pub fn stall_expected_bytes(i: usize) -> Option<u32> {
    if i >= 20 {
        return None;
    }
    let chunks = 1 + (i % 3);
    Some((chunks as u32 + 1) * 512 + ((i as u16 % 7) + 1) as u32)
}

// ---------------------------------------------------------------------------
// 流控账
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TxnRecord {
    pub id: u32,
    pub ok: bool,
    pub bytes: u32,
    pub naks: u16,
    pub stalls: u16,
    pub recovered: u16,
    pub steps: u16,
    pub code_n: u16,
}

pub struct FlowLedger {
    entries: Vec<TxnRecord>,
    cap: usize,
    dups: u16,
    deadlocks: u16,
    total_stalls: u16,
    total_recovered: u16,
}

impl FlowLedger {
    pub fn new(cap: usize) -> Result<FlowLedger, FlowError> {
        if cap == 0 || cap > 64 {
            return Err(FlowError::Capacity);
        }
        Ok(FlowLedger {
            entries: Vec::with_capacity(cap),
            cap,
            dups: 0,
            deadlocks: 0,
            total_stalls: 0,
            total_recovered: 0,
        })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn dups(&self) -> u16 {
        self.dups
    }

    pub fn deadlocks(&self) -> u16 {
        self.deadlocks
    }

    pub fn total_stalls(&self) -> u16 {
        self.total_stalls
    }

    pub fn total_recovered(&self) -> u16 {
        self.total_recovered
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 入账。事务号 0 保留（0 无法区分「未编号」），重复号显式拒绝。
    pub fn record(&mut self, id: u32, o: &TxnOutcome) -> Result<u32, FlowError> {
        if id == 0 {
            return Err(FlowError::Empty);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == id {
                    self.dups += 1;
                    return Err(FlowError::DupTxn);
                }
            }
            i += 1;
        }
        if self.entries.len() >= self.cap {
            return Err(FlowError::Capacity);
        }
        let seq = self.entries.len() as u32;
        let code_n = match o.error {
            Some(e) => (e.code() - SEG_BASE) as u16,
            None => 0,
        };
        self.entries.push(TxnRecord {
            id,
            ok: o.ok(),
            bytes: o.bytes,
            naks: o.naks,
            stalls: o.stalls,
            recovered: o.recovered,
            steps: o.steps,
            code_n,
        });
        if o.deadlock {
            self.deadlocks = self.deadlocks.saturating_add(1);
        }
        self.total_stalls = self.total_stalls.saturating_add(o.stalls);
        self.total_recovered = self.total_recovered.saturating_add(o.recovered);
        Ok(seq)
    }

    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.entries.len() as u64;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fnv_u32(h, e.id);
                h = fnv_u32(h, e.bytes);
                h = fnv_u16(h, e.naks);
                h = fnv_u16(h, e.stalls);
                h = fnv_u16(h, e.recovered);
                h = fnv_u16(h, e.steps);
                h = fnv_u8(h, if e.ok { 1 } else { 0 });
                h = fnv_u16(h, e.code_n);
            }
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 注入核：十类坏脚本 / 坏端点必须全部被拒
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;
    let ep = match BulkEp::new(0x01, 512, 2, 2) {
        Ok(v) => v,
        Err(_) => return (0, total),
    };

    // 1 空脚本
    if run_script(&ep, &[]).error == Some(FlowError::Empty) {
        caught += 1;
    }
    // 2 端点地址 0（控制管道）
    if BulkEp::new(0x00, 512, 2, 2).err() == Some(FlowError::Addr) {
        caught += 1;
    }
    // 3 数据管道写成 IN 方向
    if BulkEp::new(0x81, 512, 2, 2).err() == Some(FlowError::Addr) {
        caught += 1;
    }
    // 4 包长非 2 的幂
    if BulkEp::new(0x01, 100, 2, 2).err() == Some(FlowError::Packet) {
        caught += 1;
    }
    // 5 零长 ACK
    if run_script(&ep, &[WireEvent::Ack { bytes: 0 }]).error == Some(FlowError::ZeroChunk) {
        caught += 1;
    }
    // 6 超长 ACK
    if run_script(&ep, &[WireEvent::Ack { bytes: 2048 }]).error == Some(FlowError::OverMax) {
        caught += 1;
    }
    // 7 未 STALL 就发 CLEAR_FEATURE
    if run_script(&ep, &[WireEvent::ClearFeature]).error == Some(FlowError::BadState) {
        caught += 1;
    }
    // 8 NAK 超预算
    let nak_burst = [WireEvent::Nak, WireEvent::Nak, WireEvent::Nak];
    let o8 = run_script(&ep, &nak_burst);
    if o8.error == Some(FlowError::NakBudget) && o8.state == FlowState::TimedOut {
        caught += 1;
    }
    // 9 无 STALL 的超时
    let o9 = run_script(&ep, &[WireEvent::Ack { bytes: 512 }, WireEvent::Timeout]);
    if o9.error == Some(FlowError::Timeout) && o9.state == FlowState::TimedOut {
        caught += 1;
    }
    // 10 STALL 后未清 halt：脚本结束即死锁
    let o10 = run_script(&ep, &[WireEvent::Ack { bytes: 512 }, WireEvent::Stall]);
    if o10.error == Some(FlowError::Stall) && o10.deadlock && o10.state == FlowState::Stalled {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50124_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50124");

    let ep = match BulkEp::new(0x01, 512, 4, 2) {
        Ok(v) => v,
        Err(_) => return s,
    };

    // J1-1 20 条 STALL 脚本全部走到 Done（无残留暂态）
    let mut done = 0usize;
    let mut i = 0usize;
    while i < 20 {
        if let Some(script) = stall_script(i) {
            let o = run_script(&ep, &script);
            if o.ok() && o.state == FlowState::Done {
                done += 1;
            }
        }
        i += 1;
    }
    let ok = done == 20;
    let det = format!("done={}/20", done);
    s.add_dyn("50124-J1-STALL20恢复", ok, &det);

    // J1-2 20 条脚本累计 stall=20、recovered=20、deadlock=0（J1 核心）
    let mut ledger = match FlowLedger::new(32) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let mut rec = 0usize;
    let mut j = 0usize;
    while j < 20 {
        if let Some(script) = stall_script(j) {
            let o = run_script(&ep, &script);
            if ledger.record(j as u32 + 1, &o).is_ok() {
                rec += 1;
            }
        }
        j += 1;
    }
    let ok = rec == 20
        && ledger.ok_count() == 20
        && ledger.total_stalls() == 20
        && ledger.total_recovered() == 20
        && ledger.deadlocks() == 0;
    let det = format!(
        "rec={} stalls={} recov={} dead={}",
        rec,
        ledger.total_stalls(),
        ledger.total_recovered(),
        ledger.deadlocks()
    );
    s.add_dyn("50124-J1-零死锁", ok, &det);

    // J1-3 恢复路径顺序：每个脚本必有一次 Stall→ClearFeature→Ack，步数 = 脚本长度
    let mut path_ok = true;
    let mut k = 0usize;
    while k < 20 {
        if let Some(script) = stall_script(k) {
            let o = run_script(&ep, &script);
            if o.steps as usize != script.len()
                || o.stalls != 1
                || o.recovered != 1
                || o.naks != (k % 3) as u16
                || o.naks != o.retries
            {
                path_ok = false;
            }
        }
        k += 1;
    }
    s.add("50124-J1-恢复路径", path_ok, if path_ok { "" } else { "恢复计数/步数不符" });

    // J1-4 字节守恒：每个脚本收到的字节 = (chunks+1)×512 + 短包
    let mut bytes_ok = true;
    let mut b = 0usize;
    while b < 20 {
        if let (Some(script), Some(exp)) = (stall_script(b), stall_expected_bytes(b)) {
            let o = run_script(&ep, &script);
            if o.bytes != exp {
                bytes_ok = false;
            }
        }
        b += 1;
    }
    s.add("50124-J1-字节守恒", bytes_ok, if bytes_ok { "" } else { "恢复后丢/重字节" });

    // J1-5 步数上界：任何脚本的执行步数不得超过 len+8（无内部自旋）
    let mut bound_ok = true;
    let mut n = 0usize;
    while n < 20 {
        if let Some(script) = stall_script(n) {
            let o = run_script(&ep, &script);
            if o.steps as usize > script.len() + 8 {
                bound_ok = false;
            }
        }
        n += 1;
    }
    s.add("50124-J1-步数有界", bound_ok, if bound_ok { "" } else { "执行步数越界" });

    // J1-6 确定性：同一脚本两次执行结果逐位一致（含 deadlock/error）
    let script = match stall_script(7) {
        Some(v) => v,
        None => return s,
    };
    let a = run_script(&ep, &script);
    let a2 = run_script(&ep, &script);
    let ok = a == a2 && a.digest() == a2.digest();
    s.add("50124-J1-确定性", ok, if ok { "" } else { "同脚本结果漂移" });

    // J1-7 分包/ZLP：整倍数必须补 ZLP，非整倍数不补，空传输也要 ZLP
    let p1 = packetize(1024, 512);
    let p2 = packetize(1000, 512);
    let p3 = packetize(0, 512);
    let p4 = packetize(512, 512);
    let ok = p1 == Ok((2, true))
        && p2 == Ok((2, false))
        && p3 == Ok((0, true))
        && p4 == Ok((1, true))
        && packetize(10, 0).err() == Some(FlowError::Packet);
    s.add("50124-J1-分包ZLP", ok, if ok { "" } else { "分包/ZLP 判定错" });

    // J1-8 整倍数传输缺 ZLP：停在 Ready 且报 Phase（不是静默成功）
    let no_zlp = run_script(&ep, &[WireEvent::Ack { bytes: 512 }]);
    let with_zlp = run_script(&ep, &[WireEvent::Ack { bytes: 512 }, WireEvent::Zlp]);
    let ok = no_zlp.error == Some(FlowError::Phase)
        && no_zlp.state == FlowState::Ready
        && !no_zlp.deadlock
        && with_zlp.ok()
        && with_zlp.state == FlowState::Done;
    s.add("50124-J1-ZLP收尾", ok, if ok { "" } else { "整倍数传输收尾判定错" });

    // J1-9 NAK 预算：超预算立即 TimedOut，naks 停在上限+1，retry 与 NAK 一一对应
    let tight = match BulkEp::new(0x02, 64, 2, 1) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let burst = [WireEvent::Nak, WireEvent::Nak, WireEvent::Nak, WireEvent::Nak];
    let o = run_script(&tight, &burst);
    let ok = o.error == Some(FlowError::NakBudget)
        && o.state == FlowState::TimedOut
        && o.naks == 3
        && o.retries == o.naks
        && !o.deadlock
        && o.steps == 3;
    let det = format!("naks={} steps={} st={}", o.naks, o.steps, o.state.label());
    s.add_dyn("50124-J1-NAK预算", ok, &det);

    // J1-10 NAK 后恢复：Naked → Ack 回到 Ready，字节不丢
    let recover = run_script(
        &ep,
        &[
            WireEvent::Nak,
            WireEvent::Nak,
            WireEvent::Ack { bytes: 512 },
            WireEvent::Ack { bytes: 10 },
        ],
    );
    let ok = recover.ok() && recover.naks == 2 && recover.bytes == 522 && recover.recovered == 0;
    s.add("50124-J1-NAK恢复", ok, if ok { "" } else { "NAK 后恢复计数错" });

    // J1-11 STALL 预算：连续 STALL 超限直接 Halted，不再接受任何事件
    let one_stall = match BulkEp::new(0x03, 64, 2, 1) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let o = run_script(&one_stall, &[WireEvent::Stall, WireEvent::Stall]);
    let after = run_script(
        &one_stall,
        &[WireEvent::Stall, WireEvent::Stall, WireEvent::ClearFeature],
    );
    let ok = o.error == Some(FlowError::Halted)
        && o.state == FlowState::Halted
        && after.error == Some(FlowError::Halted)
        && after.steps == 2;
    s.add("50124-J1-STALL预算", ok, if ok { "" } else { "STALL 预算未生效" });

    // J1-12 halt 未清时收到 ACK：拒绝而不是把 halt 当数据通道
    let stalled_data = run_script(&ep, &[WireEvent::Stall, WireEvent::Ack { bytes: 512 }]);
    let ok = stalled_data.error == Some(FlowError::Stall)
        && stalled_data.bytes == 0
        && stalled_data.deadlock
        && stalled_data.state == FlowState::Stalled;
    s.add("50124-J1-halt拒绝", ok, if ok { "" } else { "halt 期间接受了数据" });

    // J1-13 无 STALL 的 CLEAR_FEATURE 是状态错误，且不改状态
    let bad_clear = run_script(&ep, &[WireEvent::ClearFeature]);
    let ok = bad_clear.error == Some(FlowError::BadState)
        && bad_clear.state == FlowState::Ready
        && !bad_clear.deadlock
        && bad_clear.steps == 1;
    s.add("50124-J1-清错halt", ok, if ok { "" } else { "未 STALL 时清 halt 被放行" });

    // J1-14 死锁必须入账：坏脚本让 deadlocks 与 ok_count 同时反映真相
    let mut bad_ledger = match FlowLedger::new(4) {
        Ok(l) => l,
        Err(_) => return s,
    };
    let dead_script = [WireEvent::Stall];
    let dead = run_script(&ep, &dead_script);
    let r1 = bad_ledger.record(1, &dead);
    let r2 = bad_ledger.record(1, &dead);
    let ok = r1 == Ok(0)
        && r2.err() == Some(FlowError::DupTxn)
        && bad_ledger.dups() == 1
        && bad_ledger.deadlocks() == 1
        && bad_ledger.ok_count() == 0
        && bad_ledger.len() == 1;
    s.add("50124-J1-死锁入账", ok, if ok { "" } else { "死锁未入账" });

    // J1-15 账本边界：容量 0/65 拒绝，事务号 0 拒绝，拒绝不改摘要
    let d0 = ledger.digest();
    let zero_id = ledger.record(0, &a);
    let ok = FlowLedger::new(0).err() == Some(FlowError::Capacity)
        && FlowLedger::new(65).err() == Some(FlowError::Capacity)
        && zero_id.err() == Some(FlowError::Empty)
        && ledger.digest() == d0;
    s.add("50124-J1-账本边界", ok, if ok { "" } else { "账本边界失守" });

    // J1-16 注入核：十类坏脚本/坏端点全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50124-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50124_checks_green() {
        let set = run_unx50124_checks();
        assert!(set.all_passed(), "UNX-F50124 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50124_twenty_stall_scripts_converge() {
        let ep = BulkEp::new(0x01, 512, 4, 2);
        assert!(ep.is_ok());
        if let Ok(ep) = ep {
            let mut i = 0usize;
            while i < 20 {
                let script = stall_script(i);
                assert!(script.is_some());
                if let Some(sc) = script {
                    let o = run_script(&ep, &sc);
                    assert!(o.ok(), "stall script {} did not converge", i);
                    assert!(!o.deadlock);
                    assert_eq!(o.stalls, 1);
                    assert_eq!(o.recovered, 1);
                }
                i += 1;
            }
        }
    }

    #[test]
    fn unx50124_stall_without_clear_is_deadlock() {
        let ep = BulkEp::new(0x01, 512, 2, 2);
        assert!(ep.is_ok());
        if let Ok(ep) = ep {
            let o = run_script(&ep, &[WireEvent::Stall]);
            assert!(o.deadlock);
            assert_eq!(o.error, Some(FlowError::Stall));
        }
    }

    #[test]
    fn unx50124_error_codes_in_segment() {
        assert_eq!(FlowError::Empty.code(), SEG_BASE + 1);
        assert_eq!(FlowError::Phase.code(), SEG_BASE + 13);
    }
}
