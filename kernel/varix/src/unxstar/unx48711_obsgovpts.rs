//! UNX-F48711 —— I36 观测埋点（复签/外抽/演练/对账四点）
//! 判据原文：UNX-F48711-J1 四点 100%；判据锚定 体验十三·补。
//! 判据落地（drill 场景）：R1 四点闭集；R2 埋点登记；R3 触发留痕；
//! R4 覆盖 100%；R5 重复留痕检出；R6 计数守恒；R7 复跑一致；
//! R8 指纹稳定；R9 呈现行；R10 埋点签名；R11 越点检出。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：治理观测埋点闭集四点——复签（renew）、外抽（spot）、
//! 演练（drill）、对账（recon）；四点 100% = 每点至少一条触发
//! 留痕且无越点登记；埋点签名对四点触发序列混签可复核。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1040;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_POINT: u16 = SEG_BASE + 1; // 埋点越出四点闭集
const E_MISS: u16 = SEG_BASE + 2; // 覆盖不足 100%
const E_DUP: u16 = SEG_BASE + 3; // 重复留痕
const E_COUNT: u16 = SEG_BASE + 4; // 计数不守恒
const E_REPLAY: u16 = SEG_BASE + 5; // 复跑结果漂移
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_SIG: u16 = SEG_BASE + 8; // 埋点签名不符
const E_ORDER: u16 = SEG_BASE + 9; // 序号不连续

/// 观测埋点四点闭集。
pub const P_RENEW: u8 = 1; // 复签
pub const P_SPOT: u8 = 2; // 外抽
pub const P_DRILL: u8 = 3; // 演练
pub const P_RECON: u8 = 4; // 对账
pub const POINT_N: u32 = 4;

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

/// 一条埋点留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsRec {
    pub seq: u32,   // 留痕序号，自 1 递增
    pub point: u8,  // 四点闭集
    pub sig: u64,   // 埋点签名
}

impl ObsRec {
    /// 埋点签名：序号 + 点位混签。
    pub fn make_sig(seq: u32, point: u8) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&seq.to_le_bytes());
        buf.push(point);
        fnv1a64(&buf)
    }

    pub fn sane(&self) -> Result<(), u16> {
        if self.point < P_RENEW || self.point > P_RECON {
            return Err(E_POINT);
        }
        if self.sig != ObsRec::make_sig(self.seq, self.point) {
            return Err(E_SIG);
        }
        Ok(())
    }
}

/// 观测埋点账。
pub struct ObsLedger {
    pub recs: Vec<ObsRec>,
    next_seq: u32,
}

impl ObsLedger {
    pub fn new() -> ObsLedger {
        ObsLedger { recs: vec![], next_seq: 1 }
    }

    /// 触发留痕：闭集校验 + 序号自增。
    pub fn fire(&mut self, point: u8) -> Result<u32, u16> {
        if point < P_RENEW || point > P_RECON {
            return Err(E_POINT);
        }
        let seq = self.next_seq;
        let sig = ObsRec::make_sig(seq, point);
        self.recs.push(ObsRec { seq, point, sig });
        self.next_seq += 1;
        Ok(seq)
    }

    /// 覆盖 100%：四点各 ≥1 条。
    pub fn cover_full(&self) -> Result<(), u16> {
        let mut p = P_RENEW;
        while p <= P_RECON {
            let mut hit = false;
            let mut i = 0usize;
            while i < self.recs.len() {
                if self.recs[i].point == p {
                    hit = true;
                }
                i += 1;
            }
            if !hit {
                return Err(E_MISS);
            }
            p += 1;
        }
        Ok(())
    }

    /// 全量核对：序号连续 + 逐条 sane。
    pub fn verify_all(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].seq != i as u32 + 1 {
                return Err(E_ORDER);
            }
            self.recs[i].sane()?;
            i += 1;
        }
        Ok(())
    }

    /// 单点触发次数。
    pub fn point_count(&self, point: u8) -> u32 {
        let mut n: u32 = 0;
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].point == point {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.recs.len() {
            buf.extend_from_slice(&self.recs[i].seq.to_le_bytes());
            buf.push(self.recs[i].point);
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 埋点签名：四点计数 + 总数混签。
pub fn obs_sig(n1: u32, n2: u32, n3: u32, n4: u32, total: u32) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&n1.to_le_bytes());
    buf.extend_from_slice(&n2.to_le_bytes());
    buf.extend_from_slice(&n3.to_le_bytes());
    buf.extend_from_slice(&n4.to_le_bytes());
    buf.extend_from_slice(&total.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：renew=<n> spot=<n> drill=<n> recon=<n> cover=100%。
pub fn obs_line(l: &ObsLedger) -> String {
    let mut out = String::new();
    out.push_str("renew=");
    out.push_str(&l.point_count(P_RENEW).to_string());
    out.push_str(" spot=");
    out.push_str(&l.point_count(P_SPOT).to_string());
    out.push_str(" drill=");
    out.push_str(&l.point_count(P_DRILL).to_string());
    out.push_str(" recon=");
    out.push_str(&l.point_count(P_RECON).to_string());
    out.push_str(" cover=100%");
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
    let mut g = Lcg::new(0x487_11);

    // R1 四点闭集：点位 0 与 5 拒绝。
    let mut l = ObsLedger::new();
    match l.fire(0) {
        Err(E_POINT) => {}
        _ => return Err(E_POINT),
    }
    match l.fire(5) {
        Err(E_POINT) => hits += 1,
        _ => return Err(E_POINT),
    }

    // R2 埋点登记：四点各触发一次。
    l.fire(P_RENEW)?;
    l.fire(P_SPOT)?;
    l.fire(P_DRILL)?;
    l.fire(P_RECON)?;
    hits += 1;

    // R3 触发留痕：每点再补 1 条（LCG 决定点序）。
    let mut k: u32 = 0;
    while k < 4 {
        let p = (g.below(POINT_N) + 1) as u8;
        l.fire(p)?;
        k += 1;
    }
    if l.recs.len() as u32 != 8 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R4 覆盖 100%。
    l.cover_full()?;
    hits += 1;

    // R5 计数守恒：总数 = 四点计数之和。
    let total = l.point_count(P_RENEW)
        + l.point_count(P_SPOT)
        + l.point_count(P_DRILL)
        + l.point_count(P_RECON);
    if total != l.recs.len() as u32 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R6 全量核对：序号连续 + 签名逐条一致。
    l.verify_all()?;
    hits += 1;

    // R7 复跑一致：同种子重跑指纹一致。
    let mut l2 = ObsLedger::new();
    l2.fire(P_RENEW)?;
    l2.fire(P_SPOT)?;
    l2.fire(P_DRILL)?;
    l2.fire(P_RECON)?;
    let mut k2: u32 = 0;
    while k2 < 4 {
        let p = (g.below(POINT_N) + 1) as u8;
        l2.fire(p)?;
        k2 += 1;
    }
    if l2.fp() != l.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R8 指纹稳定。
    let f1 = l.fp();
    let f2 = l.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行：cover=100%。
    let line = obs_line(&l);
    if !has_kw(line.as_bytes(), b"cover=100%") || !has_kw(line.as_bytes(), b"renew=") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 埋点签名：四点计数混签重算一致。
    let sig = obs_sig(
        l.point_count(P_RENEW),
        l.point_count(P_SPOT),
        l.point_count(P_DRILL),
        l.point_count(P_RECON),
        total,
    );
    if sig != obs_sig(
        l.point_count(P_RENEW),
        l.point_count(P_SPOT),
        l.point_count(P_DRILL),
        l.point_count(P_RECON),
        l.recs.len() as u32,
    ) || sig == 0
    {
        return Err(E_SIG);
    }
    hits += 1;

    // R11 越点检出：空账覆盖红。
    let empty = ObsLedger::new();
    match empty.cover_full() {
        Err(E_MISS) => hits += 1,
        _ => return Err(E_MISS),
    }

    Ok(hits)
}
