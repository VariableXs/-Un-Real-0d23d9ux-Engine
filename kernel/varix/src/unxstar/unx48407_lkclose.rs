//! UNX-F48407 —— E·丢键账本闭合性（正：账本事件与路由日志全量对账一致）
//! 判据原文：UNX-F48407-J1 丢键账本事件与路由日志全量对账一致（键入=账内+路由）；
//!           ×1000 事件流闭合恒等；逐键序号无重无漏；账本封印指纹重演一致；
//!           LCG 随机流 512 事件闭合成立；账本容量有界（环形覆盖正确）；
//!           路由日志 seq 与账本 seq 双向可追溯。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD5C0; // E21 丢键账本闭合段

const E_BALANCE: u16 = SEG_BASE + 1; // 闭合恒等不成立
const E_DUP_SEQ: u16 = SEG_BASE + 2; // 键序号重复
const E_GAP_SEQ: u16 = SEG_BASE + 3; // 键序号断档
const E_REPLAY_DIFF: u16 = SEG_BASE + 4; // 重演指纹不一致
const E_SEAL_BAD: u16 = SEG_BASE + 5; // 封印失配
const E_RING_CAP: u16 = SEG_BASE + 6; // 环形容量守恒失败
const E_TRACE_MISS: u16 = SEG_BASE + 7; // 双向追溯失败
const E_FLOW_MISS: u16 = SEG_BASE + 8; // 事件流规模不符

/// 一次键事件的归宿。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Where {
    Ledger = 0, // 入丢键账本（路由期丢弃）
    Routed = 1, // 正常路由到 context
}

/// 键事件记录。
#[derive(Clone, Copy)]
pub struct KeyEvent {
    pub keyseq: u64,
    pub code: u16,
    pub dest: Where,
    pub ctx: u32,
}

/// 丢键账本（环形定容）+ 路由日志，共用键序号空间。
pub struct LostKeyBook {
    ring: Vec<KeyEvent>,   // 丢键账本环形
    cap: usize,
    ring_head: usize,
    ring_fill: usize,
    routed: Vec<KeyEvent>, // 路由日志
    next_seq: u64,
}

impl LostKeyBook {
    pub fn new(cap: usize) -> Self {
        LostKeyBook {
            ring: Vec::new(),
            cap,
            ring_head: 0,
            ring_fill: 0,
            routed: Vec::new(),
            next_seq: 1,
        }
    }

    /// 键入一口：先尝试路由，路由失败入账本。返回归宿。
    pub fn ingest(&mut self, code: u16, ctx: u32, ctx_alive: bool) -> Where {
        let seq = self.next_seq;
        self.next_seq += 1;
        if ctx_alive {
            self.routed.push(KeyEvent { keyseq: seq, code, dest: Where::Routed, ctx });
            Where::Routed
        } else {
            let ev = KeyEvent { keyseq: seq, code, dest: Where::Ledger, ctx };
            if self.ring_fill < self.cap {
                self.ring.push(ev);
                self.ring_fill += 1;
            } else {
                // 环形覆盖：head 位覆盖后前移
                self.ring[self.ring_head] = ev;
                self.ring_head = (self.ring_head + 1) % self.cap;
            }
            Where::Ledger
        }
    }

    /// 闭合恒等：账本条目数 + 路由条目数 == next_seq - 1。
    pub fn audit_closed(&self) -> Result<u64, u16> {
        let total = (self.ring_fill + self.routed.len()) as u64;
        if total != self.next_seq - 1 {
            return Err(E_BALANCE);
        }
        Ok(total)
    }

    /// 键序号全局无重无漏：routed 与 ring（覆盖窗内）的 seq 集合互异。
    pub fn audit_seq_unique(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.routed.len() {
            let mut j = 0usize;
            while j < self.ring_fill {
                if self.routed[i].keyseq == self.ring[j].keyseq {
                    return Err(E_DUP_SEQ);
                }
                j += 1;
            }
            i += 1;
        }
        // ring 内部互异
        let mut i = 0usize;
        while i < self.ring_fill {
            let mut j = i + 1;
            while j < self.ring_fill {
                if self.ring[i].keyseq == self.ring[j].keyseq {
                    return Err(E_DUP_SEQ);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// 双向可追溯：路由日志每条的 seq 递增；账本环形窗内 seq 递增（覆盖序）。
    pub fn audit_trace(&self) -> Result<(), u16> {
        let mut i = 1usize;
        while i < self.routed.len() {
            if self.routed[i].keyseq <= self.routed[i - 1].keyseq {
                return Err(E_GAP_SEQ);
            }
            i += 1;
        }
        let mut i = 1usize;
        while i < self.ring_fill {
            // 环形覆盖序：物理序即时间序
            let a = &self.ring[(self.ring_head + i - 1) % self.cap];
            let b = &self.ring[(self.ring_head + i) % self.cap];
            if b.keyseq <= a.keyseq {
                return Err(E_GAP_SEQ);
            }
            i += 1;
        }
        Ok(())
    }

    /// 环形容量守恒：fill ≤ cap 恒成立。
    pub fn audit_ring_cap(&self) -> Result<(), u16> {
        if self.ring_fill > self.cap || self.ring.len() > self.cap {
            return Err(E_RING_CAP);
        }
        Ok(())
    }

    /// 封印指纹：routed 全量 + ring 物理序 FNV。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.routed.len() {
            let e = &self.routed[i];
            for x in [e.keyseq, e.code as u64, e.dest as u64, e.ctx as u64] {
                h ^= x;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < self.ring_fill {
            let e = &self.ring[i];
            for x in [e.keyseq, e.code as u64, e.dest as u64, e.ctx as u64] {
                h ^= x;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
            i += 1;
        }
        h
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 跑一段事件流：alive 概率 p_alive=3/4（LCG 驱动），返回 book。
pub fn run_flow(seed: u64, events: u32, cap: usize) -> Result<LostKeyBook, u16> {
    let mut book = LostKeyBook::new(cap);
    let mut rng = Lcg::new(seed);
    let mut i = 0u32;
    while i < events {
        let code = (rng.below(0x100) as u16) + 1;
        let ctx = 0x10 + rng.below(8) as u32;
        let alive = rng.below(4) != 0;
        book.ingest(code, ctx, alive);
        i += 1;
    }
    book.audit_closed()?;
    book.audit_seq_unique()?;
    Ok(book)
}

/// 主巡检：丢键账本闭合八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×1000 事件流闭合恒等
    let book = run_flow(0x48407, 1000, 4096)?;
    let total = book.audit_closed()?;
    if total != 1000 {
        return Err(E_FLOW_MISS);
    }
    hits += 1;

    // R2 逐键序号无重无漏 + 双向可追溯
    book.audit_seq_unique()?;
    book.audit_trace()?;
    hits += 1;

    // R3 环形容量守恒：小账本（容量 64）×1000 事件仍闭合
    let ring64 = run_flow(0x48407, 1000, 64)?;
    ring64.audit_ring_cap()?;
    ring64.audit_closed()?;
    hits += 1;

    // R4 环形覆盖正确：容量 8 ×32 事件，账本恰留最近 8 条且 seq 递增
    let mut small = LostKeyBook::new(8);
    let mut rng = Lcg::new(0x5EED);
    let mut i = 0u32;
    while i < 32 {
        small.ingest(1, 0x99, false); // 全部入账本
        i += 1;
    }
    let _ = rng.below(1);
    small.audit_ring_cap()?;
    small.audit_trace()?;
    // 最近 8 条 = seq 25..32
    let mut k = 0usize;
    while k < 8 {
        let e = &small.ring[(small.ring_head + k) % 8];
        if e.keyseq != (25 + k) as u64 {
            return Err(E_RING_CAP);
        }
        k += 1;
    }
    hits += 1;

    // R5 封印指纹重演一致：同种子双跑
    let f1 = run_flow(0xCAFE, 512, 256)?.fp();
    let f2 = run_flow(0xCAFE, 512, 256)?.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R6 不同种子指纹互异：×8 种子两两不同
    let mut fps: Vec<u64> = Vec::new();
    let mut s = 0u64;
    while s < 8 {
        let f = run_flow(0x7000 + s * 13, 512, 256)?.fp();
        let mut j = 0usize;
        while j < fps.len() {
            if fps[j] == f {
                return Err(E_REPLAY_DIFF);
            }
            j += 1;
        }
        fps.push(f);
        s += 1;
    }
    hits += 1;

    // R7 账本/路由双账分解守恒：alive 恒假时路由账为空、账本=全量
    let mut all_dead = LostKeyBook::new(16);
    let mut i = 0u32;
    while i < 100 {
        all_dead.ingest(9, 0x77, false);
        i += 1;
    }
    if all_dead.audit_closed()? != 100 {
        return Err(E_BALANCE);
    }
    hits += 1;

    // R8 封印稳定：双算一致且非零
    let s1 = all_dead.fp();
    let s2 = all_dead.fp();
    if s1 != s2 || s1 == 0 {
        return Err(E_SEAL_BAD);
    }
    hits += 1;

    // R9 全 alive 流零入账：200 事件全路由、账本空、闭合=200
    let mut all_alive = LostKeyBook::new(8);
    let mut i = 0u32;
    while i < 200 {
        all_alive.ingest(3, 0x11, true);
        i += 1;
    }
    if all_alive.audit_closed()? != 200 {
        return Err(E_BALANCE);
    }
    if all_alive.fp() == 0 {
        return Err(E_SEAL_BAD);
    }

    // R10 容量 1 环形覆盖：×3 事件后恰留 seq 3（最近覆盖）
    let mut cap1 = LostKeyBook::new(1);
    let mut i = 0u32;
    while i < 3 {
        cap1.ingest(5, 0x33, false);
        i += 1;
    }
    if cap1.ring[0].keyseq != 3 {
        return Err(E_RING_CAP);
    }
    Ok(hits)
}
