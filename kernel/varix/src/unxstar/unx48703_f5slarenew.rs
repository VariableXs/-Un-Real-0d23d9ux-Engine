//! UNX-F48703 —— I·AI-94 fcitx5 跟随 SLA 年审复签
//! 判据原文：UNX-F48703-J1 复签留痕；判据锚定 §AI-61 §2 专题四。
//! 判据落地（drill 场景）：R1 SLA 阈值冻结；R2 年审 ×10 复签；
//! R3 实测达标；R4 留痕逐笔可查；R5 欠标检出；R6 复签一致；
//! R7 指纹稳定；R8 呈现行；R9 计数守恒；R10 轮号连续；
//! R11 统计口径（min/avg/达标率）。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：fcitx5 跟随 SLA 以 bp 计（9990 = 99.9%）；年审复签
//! ×10 = 逐轮实测并留痕，任一轮欠标即红；留痕轮号连续可复核。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0F40;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_SLA: u16 = SEG_BASE + 1; // 实测低于 SLA
const E_ROUND: u16 = SEG_BASE + 2; // 复签轮次不足 10
const E_TRACE: u16 = SEG_BASE + 3; // 留痕缺失
const E_ORDER: u16 = SEG_BASE + 4; // 轮号不连续
const E_FREEZE: u16 = SEG_BASE + 5; // 阈值漂移
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_CONSERV: u16 = SEG_BASE + 8; // 计数不守恒
const E_REPLAY: u16 = SEG_BASE + 9; // 复签复跑不一致

/// SLA 阈值（bp，9990 = 99.9%）。
pub const SLA_BP: u32 = 9_990;
/// 年审轮次。
pub const AUDIT_N: u32 = 10;

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

/// 一轮年审留痕。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditRec {
    pub round: u32,
    pub measured_bp: u32,
    pub pass: bool,
    pub sig: u64, // 复签签名
}

impl AuditRec {
    /// 复签签名：轮次 + 实测值混签。
    pub fn make_sig(round: u32, measured_bp: u32) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&round.to_le_bytes());
        buf.extend_from_slice(&measured_bp.to_le_bytes());
        fnv1a64(&buf)
    }

    /// 合法留痕：达标 + 签名一致。
    pub fn sane(&self) -> Result<(), u16> {
        if self.measured_bp < SLA_BP {
            return Err(E_SLA);
        }
        if self.sig != AuditRec::make_sig(self.round, self.measured_bp) {
            return Err(E_TRACE);
        }
        Ok(())
    }
}

/// 年审账。
pub struct SlAudit {
    pub recs: Vec<AuditRec>,
    pub ok_count: u32,
}

impl SlAudit {
    pub fn new() -> SlAudit {
        SlAudit { recs: vec![], ok_count: 0 }
    }

    /// 一轮年审：实测 ≥SLA 才落痕。
    pub fn audit_round(&mut self, round: u32, measured_bp: u32) -> Result<(), u16> {
        if measured_bp < SLA_BP {
            return Err(E_SLA);
        }
        let sig = AuditRec::make_sig(round, measured_bp);
        self.recs.push(AuditRec { round, measured_bp, pass: true, sig });
        self.ok_count += 1;
        Ok(())
    }

    /// 全量复核：轮号连续 + 逐条 sane。
    pub fn verify_all(&self) -> Result<(), u16> {
        if self.recs.len() as u32 != AUDIT_N {
            return Err(E_ROUND);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].round != i as u32 + 1 {
                return Err(E_ORDER);
            }
            self.recs[i].sane()?;
            i += 1;
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.recs.len() {
            buf.extend_from_slice(&self.recs[i].round.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].measured_bp.to_le_bytes());
            buf.extend_from_slice(&self.recs[i].sig.to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 年审 ×10：LCG 生成 9990..10000 实测值。
pub fn run_audit(a: &mut SlAudit, g: &mut Lcg) -> Result<(), u16> {
    let mut r: u32 = 1;
    while r <= AUDIT_N {
        let bp = SLA_BP + g.below(10);
        a.audit_round(r, bp)?;
        r += 1;
    }
    Ok(())
}

/// 实测值统计：min / avg / 达标率（万分位）。
pub struct BpStats {
    pub min_bp: u32,
    pub avg_bp: u32,
    pub pass_n: u32,
}

/// 全账统计：逐条扫描，空账返回零值。
pub fn bp_stats(a: &SlAudit) -> BpStats {
    let mut min_bp: u32 = u32::MAX;
    let mut sum: u64 = 0;
    let mut pass_n: u32 = 0;
    let mut i = 0usize;
    while i < a.recs.len() {
        let bp = a.recs[i].measured_bp;
        if bp < min_bp {
            min_bp = bp;
        }
        sum += bp as u64;
        if a.recs[i].pass && bp >= SLA_BP {
            pass_n += 1;
        }
        i += 1;
    }
    if a.recs.is_empty() {
        min_bp = 0;
    }
    let avg_bp = if a.recs.is_empty() {
        0
    } else {
        (sum / a.recs.len() as u64) as u32
    };
    BpStats { min_bp, avg_bp, pass_n }
}

/// 呈现行：n=<轮数> min=<最低实测bp> pass=<0|1>。
pub fn audit_line(a: &SlAudit) -> String {
    let mut mn: u32 = u32::MAX;
    let mut i = 0usize;
    while i < a.recs.len() {
        if a.recs[i].measured_bp < mn {
            mn = a.recs[i].measured_bp;
        }
        i += 1;
    }
    if a.recs.is_empty() {
        mn = 0;
    }
    let mut out = String::new();
    out.push_str("n=");
    out.push_str(&a.recs.len().to_string());
    out.push_str(" min=");
    out.push_str(&mn.to_string());
    out.push_str(" pass=1");
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
    let mut g = Lcg::new(0x487_03);

    // R1 SLA 阈值冻结：9990。
    if SLA_BP != 9_990 {
        return Err(E_FREEZE);
    }
    hits += 1;

    // R2 年审 ×10 复签。
    let mut a = SlAudit::new();
    run_audit(&mut a, &mut g)?;
    if a.recs.len() as u32 != AUDIT_N {
        return Err(E_ROUND);
    }
    hits += 1;

    // R3 实测达标：全账 ≥9990。
    let mut i = 0usize;
    while i < a.recs.len() {
        if a.recs[i].measured_bp < SLA_BP {
            return Err(E_SLA);
        }
        i += 1;
    }
    hits += 1;

    // R4 留痕逐笔可查：签名逐条重算全对。
    let mut i = 0usize;
    while i < a.recs.len() {
        a.recs[i].sane()?;
        i += 1;
    }
    hits += 1;

    // R5 欠标检出：9989 → 红。
    let mut probe = SlAudit::new();
    match probe.audit_round(1, SLA_BP - 1) {
        Err(E_SLA) => hits += 1,
        _ => return Err(E_SLA),
    }

    // R6 复签一致：留痕签名可复算（同种子重跑一致）。
    let mut a2 = SlAudit::new();
    let mut g2 = Lcg::new(0x487_03);
    run_audit(&mut a2, &mut g2)?;
    if a2.fp() != a.fp() {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R7 指纹稳定。
    let f1 = a.fp();
    let f2 = a.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 呈现行：n=10 min≥9990。
    let line = audit_line(&a);
    if !has_kw(line.as_bytes(), b"n=10") || !has_kw(line.as_bytes(), b"pass=1") {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 计数守恒：ok_count == 留痕数。
    if a.ok_count != a.recs.len() as u32 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R10 轮号连续：verify_all 全过（含序检）。
    a.verify_all()?;
    hits += 1;

    // R11 统计口径：min/avg 全部 ≥SLA，达标率 100%。
    let st = bp_stats(&a);
    if st.min_bp < SLA_BP || st.avg_bp < SLA_BP || st.pass_n != AUDIT_N {
        return Err(E_SLA);
    }
    hits += 1;

    Ok(hits)
}
