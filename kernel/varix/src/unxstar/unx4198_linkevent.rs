//! unx4198_linkevent — UNX-F4198 链接事件打点 K2 消费锚（诊断段 0x9A40）。
//!
//! 判据原文：J1 五类链接事件（Create/Resolve/Unlink/Rename/Loop）全
//!           打点，事件账 attempted = dispatched + dropped；
//!           J2 消费方 K2 订阅掩码：只收掩码内事件，掩码外事件计入
//!           dropped 且不进消费账——一事件恰好一个归宿；
//!           J3 序号账零跳号：打点序号连续递增，消费侧按序确认；
//!           J4 回放一致：同事件序列重放两次，两份消费账逐项相同。
//! 设计纪律：
//!   1. Linux VFS fsnotify 语义：链接事件源在 symlink()/readlink()/
//!      unlink()/rename()/路径解析 ELOOP 处打点；本模型以同步账本
//!      替代异步通知，K2 消费者以订阅掩码取件。
//!   2. 事件状态机：Logged → Dispatched / Dropped，单向一跳，终态
//!      不改；掩码命中 → Dispatched 并入消费账，未命中 → Dropped。
//!   3. 回放确定性：消费账快照只依赖事件序列与掩码，与打点时刻无关。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9A40 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9A40;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SEQ_GAP: u16 = SEG_BASE + 1;    // 序号跳号
const E_DOUBLE_ACK: u16 = SEG_BASE + 2; // 事件被重复确认
const E_STATE_BAD: u16 = SEG_BASE + 3;  // 事件状态非法迁移
const E_BAL: u16 = SEG_BASE + 4;        // 事件账不守恒
const E_MASK_ZERO: u16 = SEG_BASE + 5;  // 全零掩码订阅（拒绝）
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;// 回放账不一致

/// 链接事件五类（位掩码位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    Create = 1,  // symlink() 打点
    Resolve = 2, // readlink()/walk 跟随打点
    Unlink = 4,  // unlink() 链接条目打点
    Rename = 8,  // rename() 链接条目打点
    Loop = 16,   // ELOOP 触发打点
}

pub const ALL_KINDS: u32 = 1 | 2 | 4 | 8 | 16;

/// 事件状态机：Logged → Dispatched / Dropped。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvState {
    Logged,
    Dispatched,
    Dropped,
}

/// 单条事件。
pub struct Ev {
    pub seq: u64,
    pub kind: u32, // EvKind 位或
    pub inode: u64,
    state: EvState,
}

impl Ev {
    pub fn state(&self) -> EvState {
        self.state
    }
}

/// 打点 + 订阅分发枢纽。
pub struct EventHub {
    events: Vec<Ev>,
    next_seq: u64,
    pub mask: u32,
    /// 消费账：按类分列五槽
    pub consumed: [u32; 5],
    pub dispatched: u32,
    pub dropped: u32,
}

impl EventHub {
    pub fn new() -> Result<EventHub, u16> {
        Ok(EventHub {
            events: Vec::new(),
            next_seq: 1,
            mask: ALL_KINDS,
            consumed: [0; 5],
            dispatched: 0,
            dropped: 0,
        })
    }

    /// 订阅掩码设置（全零拒收，判据 J2 前置）。
    pub fn subscribe(&mut self, mask: u32) -> Result<(), u16> {
        if mask == 0 {
            return Err(E_MASK_ZERO);
        }
        if mask & !ALL_KINDS != 0 {
            return Err(E_MASK_ZERO);
        }
        self.mask = mask;
        Ok(())
    }

    /// 打点：序号连续分配（判据 J3 源头）。
    pub fn log(&mut self, kind: u32, inode: u64) -> Result<u64, u16> {
        if kind == 0 || kind & !ALL_KINDS != 0 {
            return Err(E_STATE_BAD);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.events.push(Ev {
            seq,
            kind,
            inode,
            state: EvState::Logged,
        });
        Ok(seq)
    }

    /// 派发一轮：把所有 Logged 事件按掩码归位（一事件恰好一归宿）。
    pub fn dispatch(&mut self) -> Result<usize, u16> {
        let mut moved = 0usize;
        for i in 0..self.events.len() {
            if self.events[i].state != EvState::Logged {
                continue;
            }
            if self.events[i].kind & self.mask != 0 {
                self.events[i].state = EvState::Dispatched;
                self.dispatched += 1;
                // 分槽消费账：k 位 1..=16 → 槽 0..4
                let slot = kind_slot(self.events[i].kind);
                self.consumed[slot] += 1;
            } else {
                self.events[i].state = EvState::Dropped;
                self.dropped += 1;
            }
            moved += 1;
        }
        Ok(moved)
    }

    /// 序号账零跳号校验（判据 J3）。
    pub fn seq_dense(&self) -> bool {
        for (i, e) in self.events.iter().enumerate() {
            if e.seq != i as u64 + 1 {
                return false;
            }
        }
        self.next_seq == self.events.len() as u64 + 1
    }

    /// 事件账守恒（判据 J1）：attempted = dispatched + dropped。
    pub fn event_balanced(&self) -> bool {
        (self.dispatched + self.dropped) as usize == self.events.len()
    }

    /// 消费账守恒：Σ槽 = dispatched。
    pub fn consume_balanced(&self) -> bool {
        let sum: u32 = self.consumed.iter().sum();
        sum == self.dispatched
    }

    /// 消费账快照（回放比对用，判据 J4）。
    pub fn snapshot(&self) -> [u32; 5] {
        self.consumed
    }

    /// 双次确认拦截：终态事件再 dispatch 不改账。
    pub fn redispatch_noop(&mut self) -> Result<usize, u16> {
        let d0 = self.dispatched;
        let r = self.dispatch()?;
        if self.dispatched != d0 {
            return Err(E_DOUBLE_ACK);
        }
        Ok(r)
    }
}

/// kind 位 → 消费槽 0..4（唯一落槽）。
fn kind_slot(kind: u32) -> usize {
    // 取最低置位位：1→0, 2→1, 4→2, 8→3, 16→4
    let mut k = kind;
    let mut slot = 0usize;
    while k & 1 == 0 && slot < 4 {
        k >>= 1;
        slot += 1;
    }
    slot
}

/// 回放一致性：同事件序列 + 同掩码 → 两 hub 快照逐项相同。
pub fn replay_consistent(seq_kinds: &[u32], mask: u32) -> Result<bool, u16> {
    let mut a = EventHub::new()?;
    let mut b = EventHub::new()?;
    a.subscribe(mask)?;
    b.subscribe(mask)?;
    for k in seq_kinds.iter() {
        a.log(*k, 0x100)?;
        b.log(*k, 0x100)?;
    }
    a.dispatch()?;
    b.dispatch()?;
    Ok(a.snapshot() == b.snapshot())
}

/// 自例程：R1 五类打点序号稠密；R2 掩码分发一事件一归宿；
/// R3 掩码外 dropped；R4 重复派发零增量；R5 回放一致；R6 全账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：五类各打点 2 次，序号 1..=10 连续。
    let mut h = EventHub::new()?;
    let kinds = [EvKind::Create as u32, EvKind::Resolve as u32, EvKind::Unlink as u32, EvKind::Rename as u32, EvKind::Loop as u32];
    for round in 0..2 {
        for k in kinds.iter() {
            h.log(*k, 0x10 + round)?;
        }
    }
    if !h.seq_dense() {
        return Err(E_SEQ_GAP);
    }
    hits += 1;

    // R2：全掩码订阅 → 全部 dispatched，五槽各 2。
    h.subscribe(ALL_KINDS)?;
    let moved = h.dispatch()?;
    if moved != 10 || h.dispatched != 10 || h.dropped != 0 {
        return Err(E_BAL);
    }
    if h.consumed != [2, 2, 2, 2, 2] {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：掩码外事件 dropped——新 hub 只订阅 Create|Unlink。
    let mut h2 = EventHub::new()?;
    h2.subscribe(EvKind::Create as u32 | EvKind::Unlink as u32)?;
    h2.log(EvKind::Create as u32, 1)?;
    h2.log(EvKind::Resolve as u32, 2)?; // 掩码外
    h2.log(EvKind::Unlink as u32, 3)?;
    h2.log(EvKind::Loop as u32, 4)?; // 掩码外
    h2.dispatch()?;
    if h2.dispatched != 2 || h2.dropped != 2 || !h2.event_balanced() {
        return Err(E_BAL);
    }
    if h2.consumed[0] != 1 || h2.consumed[2] != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：重复派发零增量（终态不改账）。
    h2.redispatch_noop()?;
    if h2.dispatched != 2 || h2.dropped != 2 {
        return Err(E_DOUBLE_ACK);
    }
    hits += 1;

    // R5：回放一致 + 反例（掩码不同则快照不同）。
    let seq = [1u32, 2, 4, 16, 1, 2];
    if !replay_consistent(&seq, ALL_KINDS)? {
        return Err(E_REPLAY_DIFF);
    }
    let mut a = EventHub::new()?;
    let mut b = EventHub::new()?;
    a.subscribe(ALL_KINDS)?;
    b.subscribe(EvKind::Create as u32)?;
    for k in seq.iter() {
        a.log(*k, 9)?;
        b.log(*k, 9)?;
    }
    a.dispatch()?;
    b.dispatch()?;
    if a.snapshot() == b.snapshot() {
        return Err(E_REPLAY_DIFF); // 不同掩码快照必须不同
    }
    hits += 1;

    // R6：注入拦截 + 守恒终验。
    if EventHub::new()?.subscribe(0) != Err(E_MASK_ZERO) {
        return Err(E_MASK_ZERO);
    }
    if h.log(0, 1) != Err(E_STATE_BAD) {
        return Err(E_STATE_BAD);
    }
    if h.log(32, 1) != Err(E_STATE_BAD) {
        return Err(E_STATE_BAD);
    }
    if !h.event_balanced() || !h.consume_balanced() || !h.seq_dense() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
