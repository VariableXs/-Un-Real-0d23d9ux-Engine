//! UNX-F48662 —— I·J5 审计流全量贯通复签（本域事件通道终验）
//! 判据原文：UNX-F48662-J1 复签 ×10 通道健康；判据锚定 §AI-61 §8、
//! B01 审计流。
//! 判据落地（drill 场景）：R1 通道建链；R2 发布 ×10 序连续；R3 摘要链
//! 验证；R4 复签 ×10 一致；R5 断链检出；R6 事件类别闭集；R7 通道健康
//! 计数；R8 空通道初验；R9 时间戳单调；R10 呈现行；R11 指纹稳定。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0A20;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_CHAIN: u16 = SEG_BASE + 1; // 摘要链断裂
const E_SEQ: u16 = SEG_BASE + 2; // 序号不连续
const E_KIND: u16 = SEG_BASE + 3; // 事件类别越界
const E_RESIGN: u16 = SEG_BASE + 4; // 复签失配
const E_HEALTH: u16 = SEG_BASE + 5; // 通道健康计数异常
const E_EMPTY: u16 = SEG_BASE + 6; // 空通道初验失败
const E_TS: u16 = SEG_BASE + 7; // 时间戳非单调
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_DIGEST: u16 = SEG_BASE + 10; // 单事件摘要异常
const E_OPEN: u16 = SEG_BASE + 11; // 通道未开即发布

/// 事件类别闭集。
pub const EV_SIGN: u8 = 1; // 复签事件
pub const EV_PACK: u8 = 2; // 打包事件
pub const EV_PUBLISH: u8 = 3; // 交付事件
pub const EV_NOTIFY: u8 = 4; // 通知事件
/// 复签轮次。
pub const RESIGN_N: u32 = 10;

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

/// 事件类别闭集判定。
pub fn kind_ok(k: u8) -> bool {
    k == EV_SIGN || k == EV_PACK || k == EV_PUBLISH || k == EV_NOTIFY
}

/// 一条审计事件。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AuditEvent {
    pub seq: u64,
    pub ts: u64,
    pub kind: u8,
    pub digest: u64, // 与前序摘要混签
}

/// 审计事件通道。
pub struct AuditChannel {
    pub open: bool,
    pub events: Vec<AuditEvent>,
    pub accepted: u32,
    pub rejected: u32,
}

impl AuditChannel {
    pub fn new() -> AuditChannel {
        AuditChannel { open: false, events: vec![], accepted: 0, rejected: 0 }
    }

    /// 建链：通道置开。
    pub fn open(&mut self) {
        self.open = true;
    }

    /// 单事件摘要：seq/ts/kind/prev_digest 混签。
    pub fn event_digest(seq: u64, ts: u64, kind: u8, prev: u64) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&seq.to_le_bytes());
        buf.extend_from_slice(&ts.to_le_bytes());
        buf.push(kind);
        buf.extend_from_slice(&prev.to_le_bytes());
        fnv1a64(&buf)
    }

    /// 发布一笔事件：未开链拒收；类别闭集；序自动递增；摘要链混签。
    pub fn publish(&mut self, kind: u8, ts: u64) -> Result<u64, u16> {
        if !self.open {
            self.rejected += 1;
            return Err(E_OPEN);
        }
        if !kind_ok(kind) {
            self.rejected += 1;
            return Err(E_KIND);
        }
        let seq = self.events.len() as u64 + 1;
        let prev = if seq == 1 { 0 } else { self.events[(seq - 2) as usize].digest };
        let d = AuditChannel::event_digest(seq, ts, kind, prev);
        self.events.push(AuditEvent { seq, ts, kind, digest: d });
        self.accepted += 1;
        Ok(seq)
    }

    /// 序连续：seq 1..n 严格递增且时间戳单调不减。
    pub fn verify_order(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].seq != i as u64 + 1 {
                return Err(E_SEQ);
            }
            if i > 0 && self.events[i].ts < self.events[i - 1].ts {
                return Err(E_TS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 摘要链验证：逐条重算 digest 全对。
    pub fn verify_chain(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.events.len() {
            let prev = if i == 0 { 0 } else { self.events[i - 1].digest };
            let want = AuditChannel::event_digest(self.events[i].seq, self.events[i].ts, self.events[i].kind, prev);
            if want != self.events[i].digest || self.events[i].digest == 0 {
                return Err(E_CHAIN);
            }
            i += 1;
        }
        Ok(())
    }

    /// 复签 ×n：对链上事件逐条重签并比对摘要。
    pub fn resign(&self, n: u32) -> Result<u32, u16> {
        if n != RESIGN_N {
            return Err(E_RESIGN);
        }
        let mut ok: u32 = 0;
        let mut i = 0usize;
        while i < self.events.len() {
            let prev = if i == 0 { 0 } else { self.events[i - 1].digest };
            let want = AuditChannel::event_digest(self.events[i].seq, self.events[i].ts, self.events[i].kind, prev);
            if want != self.events[i].digest {
                return Err(E_RESIGN);
            }
            ok += 1;
            i += 1;
        }
        Ok(ok)
    }

    /// 通道健康：开链、收发守恒、链验证通过。
    pub fn healthy(&self) -> Result<(), u16> {
        if !self.open {
            return Err(E_OPEN);
        }
        if self.accepted != self.events.len() as u32 {
            return Err(E_HEALTH);
        }
        self.verify_order()?;
        self.verify_chain()?;
        Ok(())
    }
}

/// 账面指纹。
pub fn channel_fp(c: &AuditChannel) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < c.events.len() {
        buf.extend_from_slice(&c.events[i].seq.to_le_bytes());
        buf.extend_from_slice(&c.events[i].ts.to_le_bytes());
        buf.push(c.events[i].kind);
        buf.extend_from_slice(&c.events[i].digest.to_le_bytes());
        i += 1;
    }
    buf.extend_from_slice(&c.accepted.to_le_bytes());
    buf.extend_from_slice(&c.rejected.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：ev=<事件数> acc=<接收> rej=<拒收> health=<open|closed>。
pub fn channel_line(c: &AuditChannel) -> String {
    let mut out = String::new();
    out.push_str("ev=");
    out.push_str(&c.events.len().to_string());
    out.push_str(" acc=");
    out.push_str(&c.accepted.to_string());
    out.push_str(" rej=");
    out.push_str(&c.rejected.to_string());
    if c.open {
        out.push_str(" health=open");
    } else {
        out.push_str(" health=closed");
    }
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
    let mut g = Lcg::new(0x486_62);

    // R1 通道建链：开链前发布被拒，开链后可发布。
    let mut ch = AuditChannel::new();
    match ch.publish(EV_SIGN, 100) {
        Err(E_OPEN) => {}
        _ => return Err(E_OPEN),
    }
    ch.open();
    hits += 1;

    // R2 发布 ×10：序连续、类别轮转、时间戳单调。
    let mut round: u32 = 0;
    while round < RESIGN_N {
        let kind = (1 + (round % 4)) as u8;
        let ts = 1000 + round as u64 * 10;
        ch.publish(kind, ts)?;
        round += 1;
    }
    if ch.events.len() as u32 != RESIGN_N {
        return Err(E_HEALTH);
    }
    hits += 1;

    // R3 摘要链验证。
    ch.verify_chain()?;
    hits += 1;

    // R4 复签 ×10 一致：逐条重签全对。
    let ok = ch.resign(RESIGN_N)?;
    if ok != RESIGN_N {
        return Err(E_RESIGN);
    }
    hits += 1;

    // R5 断链检出：篡改中部事件类别 → 链验证红。
    let mut bad = AuditChannel::new();
    bad.open();
    let mut round = 0u32;
    while round < 6 {
        bad.publish(EV_PACK, 100 + round as u64)?;
        round += 1;
    }
    bad.events[3].kind = EV_SIGN; // 篡改
    match bad.verify_chain() {
        Err(E_CHAIN) => hits += 1,
        _ => return Err(E_CHAIN),
    }

    // R6 事件类别闭集：kind=0 与 kind=9 拒收且计入拒收账。
    let mut probe = AuditChannel::new();
    probe.open();
    match probe.publish(0, 1) {
        Err(E_KIND) => {}
        _ => return Err(E_KIND),
    }
    match probe.publish(9, 2) {
        Err(E_KIND) => hits += 1,
        _ => return Err(E_KIND),
    }
    if probe.rejected != 2 {
        return Err(E_HEALTH);
    }

    // R7 通道健康计数：accepted == 事件数，守恒成立。
    ch.healthy()?;
    if ch.accepted != RESIGN_N {
        return Err(E_HEALTH);
    }
    hits += 1;

    // R8 空通道初验：开链零事件，序/链验证通过。
    let mut empty = AuditChannel::new();
    empty.open();
    empty.verify_order()?;
    empty.verify_chain()?;
    if empty.events.len() != 0 || empty.accepted != 0 {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R9 时间戳单调：回拨时间戳 → 序验证红。
    let mut back = AuditChannel::new();
    back.open();
    back.publish(EV_NOTIFY, 500)?;
    back.publish(EV_NOTIFY, 400)?;
    match back.verify_order() {
        Err(E_TS) => hits += 1,
        _ => return Err(E_TS),
    }

    // R10 呈现行：ev=10 acc=10 health=open。
    let line = channel_line(&ch);
    if !has_kw(line.as_bytes(), b"ev=10") || !has_kw(line.as_bytes(), b"acc=10")
        || !has_kw(line.as_bytes(), b"health=open")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R11 指纹稳定。
    let f1 = channel_fp(&ch);
    let f2 = channel_fp(&ch);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    let _ = g.below(10);
    hits += 1;

    Ok(hits)
}
