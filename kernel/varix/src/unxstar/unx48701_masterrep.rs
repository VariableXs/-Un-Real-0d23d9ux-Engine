//! UNX-F48701 —— I·AI-85 母版选用匹配终报（M1/M4/P4/S1/X5 匹配率终值）
//! 判据原文：UNX-F48701-J1 终报在册匹配率如实；判据锚定 附则一。
//! 判据落地（drill 场景）：R1 五母版全录；R2 匹配率域回归；
//! R3 终报在册可查；R4 如实标注（缺实测项不编造）；R5 匹配率
//! 复算；R6 终报指纹；R7 重复登记拒；R8 查无终报红；R9 呈现行；
//! R10 母版闭集。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：AI-85 母版闭集 = {M1, M4, P4, S1, X5}；匹配率以 bp 计
//! （10000 = 100%）；如实 = 未实测项 measured=false 且 rate 不得编造
//! （置 0 并如实标注），已实测项必须有证据摘要。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0F00;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_MASTER: u16 = SEG_BASE + 1; // 母版越出闭集
const E_RATE: u16 = SEG_BASE + 2; // 匹配率越域
const E_HONEST: u16 = SEG_BASE + 3; // 如实标注缺失（编造）
const E_DUP: u16 = SEG_BASE + 4; // 重复登记
const E_NOTFOUND: u16 = SEG_BASE + 5; // 查无终报
const E_EVID: u16 = SEG_BASE + 6; // 证据摘要缺失
const E_FP: u16 = SEG_BASE + 7; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 8; // 呈现行缺段
const E_COUNT: u16 = SEG_BASE + 9; // 终报条目不足 5

/// 母版闭集。
pub const M_M1: u8 = 1; // 输入框架母版
pub const M_M4: u8 = 2; // 词库母版
pub const M_P4: u8 = 3; // 部署母版
pub const M_S1: u8 = 4; // 签名母版
pub const M_X5: u8 = 5; // X 协议母版
/// 母版数。
pub const MASTER_N: usize = 5;

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

/// 母版闭集判定。
pub fn master_ok(m: u8) -> bool {
    m == M_M1 || m == M_M4 || m == M_P4 || m == M_S1 || m == M_X5
}

/// 母版名（呈现辅助）。
pub fn master_name(m: u8) -> &'static str {
    match m {
        M_M1 => "M1",
        M_M4 => "M4",
        M_P4 => "P4",
        M_S1 => "S1",
        _ => "X5",
    }
}

/// 一条母版匹配终值。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MasterMatch {
    pub master: u8,
    pub rate_bp: u32,     // 匹配率（未实测恒 0）
    pub measured: bool,   // 如实位：是否已实测
    pub evidence: u64,    // 证据摘要（已实测必非零）
}

impl MasterMatch {
    /// 如实性校验：未实测 → rate 恒 0；已实测 → rate ≤10000 且证据非零。
    pub fn honest(&self) -> Result<(), u16> {
        if !master_ok(self.master) {
            return Err(E_MASTER);
        }
        if self.measured {
            if self.rate_bp > 10_000 {
                return Err(E_RATE);
            }
            if self.evidence == 0 {
                return Err(E_EVID);
            }
        } else {
            if self.rate_bp != 0 {
                return Err(E_HONEST);
            }
            if self.evidence != 0 {
                return Err(E_HONEST);
            }
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.push(self.master);
        buf.extend_from_slice(&self.rate_bp.to_le_bytes());
        buf.push(self.measured as u8);
        buf.extend_from_slice(&self.evidence.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 终报账。
pub struct FinalReport {
    pub items: Vec<MasterMatch>,
}

impl FinalReport {
    pub fn new() -> FinalReport {
        FinalReport { items: vec![] }
    }

    /// 登记：如实校验 + 母版唯一。
    pub fn register(&mut self, m: MasterMatch) -> Result<(), u16> {
        m.honest()?;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].master == m.master {
                return Err(E_DUP);
            }
            i += 1;
        }
        self.items.push(m);
        Ok(())
    }

    /// 查询。
    pub fn lookup(&self, master: u8) -> Result<&MasterMatch, u16> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].master == master {
                return Ok(&self.items[i]);
            }
            i += 1;
        }
        Err(E_NOTFOUND)
    }

    /// 五母版齐。
    pub fn complete(&self) -> Result<(), u16> {
        if self.items.len() != MASTER_N {
            return Err(E_COUNT);
        }
        let mut m = 1u8;
        while m <= 5 {
            self.lookup(m)?;
            m += 1;
        }
        Ok(())
    }

    /// 匹配率复算：已实测项均值（bp，整除）。
    pub fn avg_measured_bp(&self) -> u32 {
        let mut sum: u32 = 0;
        let mut n: u32 = 0;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].measured {
                sum += self.items[i].rate_bp;
                n += 1;
            }
            i += 1;
        }
        if n == 0 {
            return 0;
        }
        sum / n
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        let mut i = 0usize;
        while i < self.items.len() {
            buf.extend_from_slice(&self.items[i].fp().to_le_bytes());
            i += 1;
        }
        fnv1a64(&buf)
    }
}

/// 构造终报：M1/M4/P4/S1 已实测（确定性速率），X5 未实测（如实置 0）。
pub fn build_report(g: &mut Lcg) -> Result<FinalReport, u16> {
    let mut rep = FinalReport::new();
    let mut m = 1u8;
    while m <= 5 {
        if m == M_X5 {
            rep.register(MasterMatch { master: m, rate_bp: 0, measured: false, evidence: 0 })?;
        } else {
            let rate = 8_500 + g.below(1_400);
            let ev = (g.next_u32() as u64) << 32 | g.next_u32() as u64;
            rep.register(MasterMatch { master: m, rate_bp: rate, measured: true, evidence: ev })?;
        }
        m += 1;
    }
    Ok(rep)
}

/// 呈现行：m=<条数> avg=<均值bp> honest=<0|1>。
pub fn report_line(r: &FinalReport) -> String {
    let mut honest: u32 = 0;
    let mut i = 0usize;
    while i < r.items.len() {
        if r.items[i].honest().is_ok() {
            honest += 1;
        }
        i += 1;
    }
    let mut out = String::new();
    out.push_str("m=");
    out.push_str(&r.items.len().to_string());
    out.push_str(" avg=");
    out.push_str(&r.avg_measured_bp().to_string());
    out.push_str(" honest=");
    out.push_str(&honest.to_string());
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
    let mut g = Lcg::new(0x487_01);

    // R1 五母版全录。
    let rep = build_report(&mut g)?;
    rep.complete()?;
    hits += 1;

    // R2 匹配率域回归：逐条 honest。
    let mut i = 0usize;
    while i < rep.items.len() {
        rep.items[i].honest()?;
        i += 1;
    }
    hits += 1;

    // R3 终报在册可查。
    let m1 = rep.lookup(M_M1)?;
    if m1.master != M_M1 || !m1.measured || m1.rate_bp == 0 {
        return Err(E_NOTFOUND);
    }
    hits += 1;

    // R4 如实标注：X5 未实测 rate==0；编造检出。
    let x5 = rep.lookup(M_X5)?;
    if x5.measured || x5.rate_bp != 0 || x5.evidence != 0 {
        return Err(E_HONEST);
    }
    let fake = MasterMatch { master: M_X5, rate_bp: 9_900, measured: false, evidence: 0 };
    match fake.honest() {
        Err(E_HONEST) => hits += 1,
        _ => return Err(E_HONEST),
    }

    // R5 匹配率复算：均值 = 四项实测均值。
    let avg = rep.avg_measured_bp();
    if avg == 0 || avg > 10_000 {
        return Err(E_RATE);
    }
    hits += 1;

    // R6 终报指纹稳定。
    let f1 = rep.fp();
    let f2 = rep.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R7 重复登记拒。
    let mut probe = build_report(&mut g)?;
    let dupm = MasterMatch { master: M_M1, rate_bp: 1, measured: true, evidence: 5 };
    match probe.register(dupm) {
        Err(E_DUP) => hits += 1,
        _ => return Err(E_DUP),
    }

    // R8 查无终报红。
    match probe2_empty().lookup(M_S1) {
        Err(E_NOTFOUND) => hits += 1,
        _ => return Err(E_NOTFOUND),
    }

    // R9 呈现行：m=5 honest=5。
    let line = report_line(&rep);
    if !has_kw(line.as_bytes(), b"m=5") || !has_kw(line.as_bytes(), b"honest=5") {
        return Err(E_LINE);
    }
    hits += 1;

    // R10 母版闭集：名表冻结 + 越界拒收。
    if master_name(M_M1) != "M1" || master_name(M_X5) != "X5" || master_name(M_P4) != "P4" {
        return Err(E_MASTER);
    }
    let bad = MasterMatch { master: 9, rate_bp: 0, measured: false, evidence: 0 };
    match bad.honest() {
        Err(E_MASTER) => hits += 1,
        _ => return Err(E_MASTER),
    }

    Ok(hits)
}

/// 空终报（查无场景）。
pub fn probe2_empty() -> FinalReport {
    FinalReport::new()
}
