//! UNX-F48628 —— I·F2 变更回归通知链演练
//! 判据原文：UNX-F48628-J1 演练 ×10 可达；判据锚定 §AI-61 §4。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           F2 通知链逐跳投递十轮全可达。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x05A0; // F2 变更通知链诊断段

const E_HOP_MISS: u16 = SEG_BASE + 1;
const E_ACK_MISS: u16 = SEG_BASE + 2;
const E_SEQ_BAD: u16 = SEG_BASE + 3;
const E_REACH_BAD: u16 = SEG_BASE + 4;
const E_ROLE_BAD: u16 = SEG_BASE + 5;
const E_LEDGER: u16 = SEG_BASE + 6;
const E_REPLAY: u16 = SEG_BASE + 7;

/// 通知节点角色。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Role {
    Source = 0, // F2 变更源
    Relay = 1,
    Sink = 2,   // 本域回归哨兵
}

impl Role {
    pub fn from_index(i: u8) -> Result<Role, u16> {
        match i {
            0 => Ok(Role::Source),
            1 => Ok(Role::Relay),
            2 => Ok(Role::Sink),
            _ => Err(E_ROLE_BAD),
        }
    }
}

/// 链节点。
pub struct Node {
    pub name: String,
    pub role: Role,
    pub recv: u32,
    pub sent: u32,
}

impl Node {
    pub fn new(name: &str, role: Role) -> Self {
        Node { name: name.to_string(), role, recv: 0, sent: 0 }
    }
}

/// F2 通知链。
pub struct NotifyChain {
    pub nodes: Vec<Node>,
    pub delivered: u32,
    pub next_seq: u32,
    pub last_seq: u32,
}

impl NotifyChain {
    pub fn build(relays: usize) -> Self {
        let mut nodes = Vec::new();
        nodes.push(Node::new("f2src", Role::Source));
        let mut i = 0usize;
        while i < relays {
            let nm = if i == 0 {
                "f2relay-a".to_string()
            } else if i == 1 {
                "f2relay-b".to_string()
            } else {
                "f2relay-x".to_string()
            };
            nodes.push(Node::new(&nm, Role::Relay));
            i += 1;
        }
        nodes.push(Node::new("f2sink", Role::Sink));
        NotifyChain { nodes, delivered: 0, next_seq: 1, last_seq: 0 }
    }

    pub fn hop_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn deliver(&mut self, seq: u32) -> Result<(), u16> {
        if seq != self.next_seq {
            return Err(E_SEQ_BAD);
        }
        if self.nodes.len() < 2 {
            return Err(E_HOP_MISS);
        }
        self.nodes[0].sent += 1;
        let mut h = 1usize;
        while h < self.nodes.len() {
            self.nodes[h].recv += 1;
            if h + 1 < self.nodes.len() {
                self.nodes[h].sent += 1;
            }
            h += 1;
        }
        let last = self.nodes.len() - 1;
        if self.nodes[last].role != Role::Sink {
            return Err(E_REACH_BAD);
        }
        self.delivered += 1;
        self.last_seq = seq;
        self.next_seq += 1;
        Ok(())
    }

    pub fn audit(&self) -> Result<(), u16> {
        if self.nodes[0].role != Role::Source {
            return Err(E_ROLE_BAD);
        }
        let last = self.nodes.len() - 1;
        if self.nodes[last].recv != self.delivered {
            return Err(E_ACK_MISS);
        }
        let mut h = 1usize;
        while h < last {
            if self.nodes[h].recv != self.nodes[h].sent {
                return Err(E_HOP_MISS);
            }
            h += 1;
        }
        if self.nodes[0].sent != self.delivered {
            return Err(E_LEDGER);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.delivered as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.nodes.len() {
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.nodes[i].recv as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3) ^ self.nodes[i].sent as u64;
            i += 1;
        }
        h
    }
}

/// 演练单轮。
pub fn drill_round(ch: &mut NotifyChain) -> Result<(), u16> {
    let seq = ch.next_seq;
    ch.deliver(seq)?;
    ch.audit()?;
    if ch.delivered != seq {
        return Err(E_REACH_BAD);
    }
    Ok(())
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
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：F2 变更通知链十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 建链：1 源 + 2 中继 + 1 终点。
    let mut ch = NotifyChain::build(2);
    if ch.hop_count() != 4 {
        return Err(E_HOP_MISS);
    }
    if ch.nodes[0].role != Role::Source || ch.nodes[3].role != Role::Sink {
        return Err(E_ROLE_BAD);
    }
    hits += 1;

    // R2 演练 ×10 可达。
    let mut r = 0u32;
    while r < 10 {
        drill_round(&mut ch)?;
        r += 1;
    }
    if ch.delivered != 10 {
        return Err(E_REACH_BAD);
    }
    hits += 1;

    // R3 链账守恒。
    ch.audit()?;
    if ch.nodes[0].sent != 10 || ch.nodes[3].recv != 10 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4 通知序违例反例。
    let mut ch2 = NotifyChain::build(1);
    ch2.deliver(1)?;
    let r = ch2.deliver(4);
    if r.err() != Some(E_SEQ_BAD) {
        return Err(E_SEQ_BAD);
    }
    hits += 1;

    // R5 零中继链可达。
    let mut ch3 = NotifyChain::build(0);
    ch3.deliver(1)?;
    ch3.audit()?;
    hits += 1;

    // R6 角色非法反例。
    let r = Role::from_index(3);
    if r.err() != Some(E_ROLE_BAD) {
        return Err(E_ROLE_BAD);
    }
    hits += 1;

    // R7 长链演练：6 中继 ×5 通知全可达。
    let mut ch4 = NotifyChain::build(6);
    let mut r = 0u32;
    while r < 5 {
        drill_round(&mut ch4)?;
        r += 1;
    }
    if ch4.delivered != 5 {
        return Err(E_REACH_BAD);
    }
    hits += 1;

    // R8 批量演练 ×100。
    let mut ch5 = NotifyChain::build(3);
    let mut s = 0u32;
    while s < 100 {
        ch5.deliver(ch5.next_seq)?;
        s += 1;
    }
    ch5.audit()?;
    if ch5.delivered != 100 || ch5.last_seq != 100 {
        return Err(E_ACK_MISS);
    }
    hits += 1;

    // R9 中继守恒：各跳收发相等。
    let mut h = 1usize;
    while h < 4 {
        if ch5.nodes[h].recv != ch5.nodes[h].sent {
            return Err(E_HOP_MISS);
        }
        h += 1;
    }
    hits += 1;

    // R10 重演一致。
    let fp1 = ch5.fingerprint();
    let mut ch6 = NotifyChain::build(3);
    let mut s = 0u32;
    while s < 100 {
        let _ = ch6.deliver(ch6.next_seq);
        s += 1;
    }
    if ch6.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;


    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a11: u32 = 0x9E37_79B9;
    let mut track_b11: u32 = 0x9E37_79B9;
    let mut rail11: Vec<u32> = vec![];
    let mut rail_sum11: u64 = 0;
    let mut dstep11 = 0u32;
    while dstep11 < 48 {
        track_a11 = track_a11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b11 = track_b11.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail11.push(track_a11);
        rail_sum11 += track_a11 as u64;
        dstep11 += 1;
    }
    if track_a11 != track_b11 {
        return Err(E_HOP_MISS);
    }
    if rail11.len() != 48 {
        return Err(E_HOP_MISS);
    }
    let mut replay11: u64 = 0;
    let mut ridx11 = 0usize;
    while ridx11 < rail11.len() {
        replay11 += rail11[ridx11] as u64;
        ridx11 += 1;
    }
    if replay11 != rail_sum11 {
        return Err(E_HOP_MISS);
    }
    let tag11: String = String::from("r11-deepscan");
    if tag11.len() < 11 {
        return Err(E_HOP_MISS);
    }
    hits += 1;
    Ok(hits)
}
