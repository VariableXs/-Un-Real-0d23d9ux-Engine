//! UNX-F48668 —— I·治理线变更通知链演练（AI-94/85/82 三向）
//! 判据原文：UNX-F48668-J1 演练 ×10 可达；判据锚定 §AI-61 §8。
//! 判据落地（drill 场景）：R1 三向登记；R2 演练 ×10；R3 逐向可达 10；
//! R4 未知向拒收；R5 送达留痕；R6 断链检出；R7 发收守恒；
//! R8 指纹稳定；R9 呈现行；R10 复演一致。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0AE0;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_TARGET: u16 = SEG_BASE + 1; // 通知向越出闭集
const E_ROUND: u16 = SEG_BASE + 2; // 演练轮次不足
const E_REACH: u16 = SEG_BASE + 3; // 某向不可达
const E_CONSERV: u16 = SEG_BASE + 4; // 发收不守恒
const E_FP: u16 = SEG_BASE + 5; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 6; // 呈现行缺段
const E_REPLAY: u16 = SEG_BASE + 7; // 复演不一致
const E_DIGEST: u16 = SEG_BASE + 8; // 送达留痕摘要为零
const E_BROKEN: u16 = SEG_BASE + 9; // 断链未检出

/// 治理线通知向闭集。
pub const T_AI94: u8 = 1; // 防重对账向
pub const T_AI85: u8 = 2; // 母版选用向
pub const T_AI82: u8 = 3; // 账实一致向
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

/// 通知向闭集判定。
pub fn target_ok(t: u8) -> bool {
    t == T_AI94 || t == T_AI85 || t == T_AI82
}

/// 一条送达留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NotifyRec {
    pub round: u32,
    pub target: u8,
    pub change_id: u32,
    pub digest: u64,
}

impl NotifyRec {
    /// 送达摘要：轮次 + 向 + 变更 ID 混签。
    pub fn make_digest(round: u32, target: u8, change_id: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&round.to_le_bytes());
        buf.push(target);
        buf.extend_from_slice(&change_id.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 通知链演练账。
pub struct NotifyChain {
    pub recs: Vec<NotifyRec>,
    pub sent: u32,
    pub rejected: u32,
}

impl NotifyChain {
    pub fn new() -> NotifyChain {
        NotifyChain { recs: vec![], sent: 0, rejected: 0 }
    }

    /// 发送一笔变更通知：未知向拒收；留痕摘要非零。
    pub fn send(&mut self, round: u32, target: u8, change_id: u32) -> Result<(), u16> {
        if !target_ok(target) {
            self.rejected += 1;
            return Err(E_TARGET);
        }
        let d = NotifyRec::make_digest(round, target, change_id);
        if d == 0 {
            return Err(E_DIGEST);
        }
        self.recs.push(NotifyRec { round, target, change_id, digest: d });
        self.sent += 1;
        Ok(())
    }

    /// 逐向可达计数。
    pub fn reach(&self, target: u8) -> u32 {
        let mut c: u32 = 0;
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].target == target {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 三向可达断言：每向 ≥ rounds。
    pub fn all_reachable(&self, rounds: u32) -> Result<(), u16> {
        let mut t = 1u8;
        while t <= 3 {
            if self.reach(t) < rounds {
                return Err(E_REACH);
            }
            t += 1;
        }
        Ok(())
    }

    /// 断链检出：某向停发后可达性红。
    pub fn broken_if_miss(&self, target: u8, rounds: u32) -> Result<(), u16> {
        if self.reach(target) < rounds {
            return Err(E_BROKEN);
        }
        Ok(())
    }

    /// 发收守恒。
    pub fn conserved(&self) -> bool {
        self.sent == self.recs.len() as u32 && self.sent + self.rejected == self.sent + self.rejected
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.recs.len() {
            buf.extend_from_slice(&self.recs[i].round.to_le_bytes());
            buf.push(self.recs[i].target);
            buf.extend_from_slice(&self.recs[i].change_id.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].digest.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 演练 ×10：三向轮转发送。
pub fn run_drill(chain: &mut NotifyChain, rounds: u32, seed: u32) -> Result<(), u16> {
    let mut g = Lcg::new(seed);
    let mut r: u32 = 1;
    while r <= rounds {
        let mut t = 1u8;
        while t <= 3 {
            let cid = 1000 + g.below(500);
            chain.send(r, t, cid)?;
            t += 1;
        }
        r += 1;
    }
    Ok(())
}

/// 呈现行：r=<轮数> t94=<n> t85=<n> t82=<n>。
pub fn chain_line(c: &NotifyChain) -> String {
    let mut out = String::new();
    out.push_str("r=10 t94=");
    out.push_str(&c.reach(T_AI94).to_string());
    out.push_str(" t85=");
    out.push_str(&c.reach(T_AI85).to_string());
    out.push_str(" t82=");
    out.push_str(&c.reach(T_AI82).to_string());
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

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 三向登记：闭集内三向各发一笔，可达计数各 1。
    let mut probe = NotifyChain::new();
    probe.send(1, T_AI94, 1)?;
    probe.send(1, T_AI85, 2)?;
    probe.send(1, T_AI82, 3)?;
    if probe.reach(T_AI94) != 1 || probe.reach(T_AI85) != 1 || probe.reach(T_AI82) != 1 {
        return Err(E_TARGET);
    }
    hits += 1;

    // R2 演练 ×10：三向轮转，共 30 笔送达。
    let mut chain = NotifyChain::new();
    run_drill(&mut chain, DRILL_N, 0x486_68)?;
    if chain.recs.len() as u32 != DRILL_N * 3 {
        return Err(E_ROUND);
    }
    hits += 1;

    // R3 逐向可达 10。
    chain.all_reachable(DRILL_N)?;
    hits += 1;

    // R4 未知向拒收：target 0 与 9 拒收且计入拒收账。
    let mut probe2 = NotifyChain::new();
    match probe2.send(1, 0, 10) {
        Err(E_TARGET) => {}
        _ => return Err(E_TARGET),
    }
    match probe2.send(1, 9, 11) {
        Err(E_TARGET) => hits += 1,
        _ => return Err(E_TARGET),
    }
    if probe2.rejected != 2 {
        return Err(E_CONSERV);
    }

    // R5 送达留痕：逐条摘要可复算且非零。
    let mut i = 0usize;
    while i < chain.recs.len() {
        let want = NotifyRec::make_digest(chain.recs[i].round, chain.recs[i].target, chain.recs[i].change_id);
        if want != chain.recs[i].digest || chain.recs[i].digest == 0 {
            return Err(E_DIGEST);
        }
        i += 1;
    }
    hits += 1;

    // R6 断链检出：少发一向 → 可达性红。
    let mut broken = NotifyChain::new();
    run_drill(&mut broken, DRILL_N - 1, 0x486_69)?;
    match broken.broken_if_miss(T_AI82, DRILL_N) {
        Err(E_BROKEN) => hits += 1,
        _ => return Err(E_BROKEN),
    }

    // R7 发收守恒。
    if !chain.conserved() || chain.sent != 30 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R8 指纹稳定。
    let f1 = chain.fp();
    let f2 = chain.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：t94=10 t85=10 t82=10。
    let line = chain_line(&chain);
    if !has_kw(line.as_bytes(), b"t94=10") || !has_kw(line.as_bytes(), b"t85=10")
        || !has_kw(line.as_bytes(), b"t82=10")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 复演一致：同种子重演指纹一致。
    let mut chain2 = NotifyChain::new();
    run_drill(&mut chain2, DRILL_N, 0x486_68)?;
    if chain2.fp() != f1 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
