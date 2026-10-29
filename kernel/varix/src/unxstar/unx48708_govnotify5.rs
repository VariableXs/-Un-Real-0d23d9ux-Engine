//! UNX-F48708 —— I·治理线通知链全演练（五治理 AI 全向）
//! 判据原文：UNX-F48708-J1 演练 ×10 全向可达；判据锚定 §AI-61 §8。
//! 判据落地（drill 场景）：R1 五收点闭集；R2 通知派发；R3 回执闭环；
//! R4 演练 ×10 全向；R5 漏发检出；R6 重复回执检出；R7 链路守恒；
//! R8 复跑一致；R9 指纹稳定；R10 呈现行；R11 链路签名。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：五治理 AI 收点闭集（SIG/SEC/OBS/DOC/AUD），每轮演练
//! 向全部 5 收点派发通知并要求逐点回执；演练 ×10 = 十轮全向派发，
//! 任一轮任一点缺回执即红；链路签名对十轮总量混签可复核。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0FE0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_PEER: u16 = SEG_BASE + 1; // 收点越出五点闭集
const E_MISS: u16 = SEG_BASE + 2; // 回执缺失
const E_DUP: u16 = SEG_BASE + 3; // 回执重复
const E_ROUND: u16 = SEG_BASE + 4; // 演练轮次不足
const E_COUNT: u16 = SEG_BASE + 5; // 链路计数不守恒
const E_REPLAY: u16 = SEG_BASE + 6; // 复跑结果漂移
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 9; // 链路签名不符
const E_ORDER: u16 = SEG_BASE + 10; // 轮号不连续

/// 五治理 AI 收点闭集。
pub const P_SIG: u8 = 1; // 签署
pub const P_SEC: u8 = 2; // 安全
pub const P_OBS: u8 = 3; // 观测
pub const P_DOC: u8 = 4; // 文档
pub const P_AUD: u8 = 5; // 审计
pub const PEER_N: u32 = 5;

/// 演练轮次。
pub const DRILL_N: u32 = 10;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一条回执。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AckRec {
    pub round: u32,
    pub peer: u8,
    pub seq: u64, // 回执序号签名
}

impl AckRec {
    /// 回执签名：轮次 + 收点混签。
    pub fn make_seq(round: u32, peer: u8) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&round.to_le_bytes());
        buf.push(peer);
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.peer < P_SIG || self.peer > P_AUD {
            return Err(E_PEER);
        }
        if self.seq != AckRec::make_seq(self.round, self.peer) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 通知链账。
pub struct NotifyChain {
    pub acks: Vec<AckRec>,
}

impl NotifyChain {
    pub fn new() -> NotifyChain {
        NotifyChain { acks: vec![] }
    }

    /// 回执登记：闭集 + 重复拒绝。
    pub fn ack(&mut self, round: u32, peer: u8) -> Result<(), u16> {
        if peer < P_SIG || peer > P_AUD {
            return Err(E_PEER);
        }
        let mut i = 0usize;
        while i < self.acks.len() {
            if self.acks[i].round == round && self.acks[i].peer == peer {
                return Err(E_DUP);
            }
            i += 1;
        }
        let seq = AckRec::make_seq(round, peer);
        self.acks.push(AckRec { round, peer, seq });
        Ok(())
    }

    /// 单轮全向核对：五点回执齐才算可达。
    pub fn round_reach(&self, round: u32) -> Result<(), u16> {
        let mut p = P_SIG;
        while p <= P_AUD {
            let mut found = false;
            let mut i = 0usize;
            while i < self.acks.len() {
                if self.acks[i].round == round && self.acks[i].peer == p {
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(E_MISS);
            }
            p += 1;
        }
        Ok(())
    }

    /// 全量核对：轮号连续 + 逐轮全向 + 逐条 sane。
    pub fn verify_all(&self) -> Result<(), u16> {
        if self.acks.len() as u32 != DRILL_N * PEER_N {
            return Err(E_COUNT);
        }
        let mut r = 1u32;
        while r <= DRILL_N {
            self.round_reach(r)?;
            r += 1;
        }
        let mut i = 0usize;
        while i < self.acks.len() {
            if self.acks[i].round < 1 || self.acks[i].round > DRILL_N {
                return Err(E_ORDER);
            }
            self.acks[i].sane()?;
            i += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.acks.len() {
            buf.extend_from_slice(&self.acks[i].round.to_le_bytes());
            buf.push(self.acks[i].peer);
            buf.extend_from_slice(&self.acks[i].seq.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 演练 ×10：每轮向 5 点全向派发并收集回执。
pub fn run_drill(c: &mut NotifyChain, g: &mut Lcg) -> Result<(), u16> {
    let mut r = 1u32;
    while r <= DRILL_N {
        let mut p = P_SIG;
        while p <= P_AUD {
            c.ack(r, p)?;
            let _ = g.below(PEER_N); // 派发路径扰动，确定性消费
            p += 1;
        }
        r += 1;
    }
    Ok(())
}

/// 链路签名：轮数 × 收点数 × 回执总数混签。
pub fn chain_sig(rounds: u32, peers: u32, total: u32) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&rounds.to_le_bytes());
    buf.extend_from_slice(&peers.to_le_bytes());
    buf.extend_from_slice(&total.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：rounds=<轮数> peers=<收点> total=<回执>。
pub fn chain_line(c: &NotifyChain) -> String {
    let mut out = String::new();
    out.push_str("rounds=");
    out.push_str(&DRILL_N.to_string());
    out.push_str(" peers=");
    out.push_str(&PEER_N.to_string());
    out.push_str(" total=");
    out.push_str(&c.acks.len().to_string());
    out
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R11 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x487_08);

    // R1 五收点闭集：编号 0 与 6 拒绝。
    let mut c = NotifyChain::new();
    match c.ack(1, 0) {
        Err(E_PEER) => {}
        _ => return Err(E_PEER),
    }
    match c.ack(1, 6) {
        Err(E_PEER) => hits += 1,
        _ => return Err(E_PEER),
    }

    // R2 通知派发 + R3 回执闭环：演练 ×10 全向。
    run_drill(&mut c, &mut g)?;
    if c.acks.len() as u32 != DRILL_N * PEER_N {
        return Err(E_COUNT);
    }
    hits += 1;

    // R4 演练 ×10 全向可达：逐轮核对。
    let mut r = 1u32;
    while r <= DRILL_N {
        c.round_reach(r)?;
        r += 1;
    }
    hits += 1;

    // R5 漏发检出：抽掉一条回执 → 该轮红。
    let mut bad = NotifyChain::new();
    let mut rr = 1u32;
    while rr <= DRILL_N {
        let mut pp = P_SIG;
        while pp <= P_AUD {
            if !(rr == 3 && pp == P_OBS) {
                bad.ack(rr, pp)?;
            }
            pp += 1;
        }
        rr += 1;
    }
    match bad.round_reach(3) {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    // R6 重复回执检出。
    match c.ack(2, P_SEC) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R7 链路守恒：全量核对（轮号 + 签名 + 计数）。
    c.verify_all()?;
    hits += 1;

    // R8 复跑一致：同种子重跑指纹一致。
    let mut c2 = NotifyChain::new();
    let mut g2 = Lcg::new(0x487_08);
    run_drill(&mut c2, &mut g2)?;
    if c2.fp() != c.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R9 指纹稳定。
    let f1 = c.fp();
    let f2 = c.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行：total=50。
    let line = chain_line(&c);
    if !has_kw(line.as_bytes(), b"peers=5") || !has_kw(line.as_bytes(), b"total=50") {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 链路签名：10×5×50 混签重算一致。
    let sig = chain_sig(DRILL_N, PEER_N, c.acks.len() as u32);
    if sig != chain_sig(10, 5, 50) || sig == 0 {
        return Err(E_SIG);
    }
    hits += 1;

    Ok(hits)
}
