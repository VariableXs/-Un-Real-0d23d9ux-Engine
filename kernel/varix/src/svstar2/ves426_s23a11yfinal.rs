//! ves426_s23a11yfinal — VE-F4186 · T 域无障碍终审
//! 终审：T 域多语言无障碍本体终审——三特色维（翻译质量/地区合规/多语言读屏）证据终版核验
//! +五套剧本走查前置资格核（F4175 预演转正审）；
//! 证据缺→回补；特色维指标未达→立案+预案；资格缺→禁入终走查；
//! 本条即域本色终审（多语言读屏口径终版）；证据终版脱敏复核。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x10E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AfCode {
    Ok = 0,
    EvidenceMissing = SEG_BASE + 0x01, // 0x10E1
    MetricShort = SEG_BASE + 0x02,     // 0x10E2
    QualDenied = SEG_BASE + 0x03,      // 0x10E3
}

/// 三特色维（域本色本体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeatureDim { TranslateQuality = 0, RegionCompliance = 1, MultilingualScreenReader = 2 }

impl FeatureDim {
    pub const ALL: [FeatureDim; 3] = [
        FeatureDim::TranslateQuality, FeatureDim::RegionCompliance, FeatureDim::MultilingualScreenReader];
}

/// 特色维证据（终版）：指标值 + 脱敏终版标记。
#[derive(Clone, PartialEq)]
pub struct Evidence {
    pub dim: FeatureDim,
    pub metric_bp: u32,   // 达标度（万分数，须 ≥ FLOOR_BP）
    pub sanitized: bool,  // 证据脱敏终版
    pub final_version: bool,
}

pub const FLOOR_BP: u32 = 9_000;

/// 五套剧本资格闸（F4175 预演转正审）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptQual { S1 = 0, S2 = 1, S3 = 2, S4 = 3, S5 = 4 }

/// 终审册。
pub struct A11yFinalBook {
    pub evidence: Vec<Evidence>,
    pub qualified: Vec<ScriptQual>, // 转正剧本
    pub plans: Vec<FeatureDim>,     // 未达维预案
    pub filed: Vec<FeatureDim>,     // 立案维
}

impl A11yFinalBook {
    pub fn new() -> Self { A11yFinalBook { evidence: Vec::new(), qualified: Vec::new(), plans: Vec::new(), filed: Vec::new() } }
    pub fn put_evidence(&mut self, e: Evidence) {
        self.evidence.retain(|x| x.dim != e.dim);
        self.evidence.push(e);
    }
    pub fn qualify(&mut self, s: ScriptQual) { if !self.qualified.contains(&s) { self.qualified.push(s); } }
    /// 三特色维终版核验：证据齐+终版+脱敏+指标达标；未达→立案+预案；缺→回补码。
    pub fn audit_dims(&mut self) -> Result<(), AfCode> {
        self.filed.clear();
        self.plans.clear();
        for d in FeatureDim::ALL {
            let ev = self.evidence.iter().find(|e| e.dim == d);
            match ev {
                None => return Err(AfCode::EvidenceMissing),
                Some(e) => {
                    if !e.final_version || !e.sanitized { return Err(AfCode::EvidenceMissing); }
                    if e.metric_bp < FLOOR_BP {
                        self.filed.push(d);
                        self.plans.push(d);
                    }
                }
            }
        }
        if self.filed.is_empty() { Ok(()) } else { Err(AfCode::MetricShort) }
    }
    /// 五套剧本资格闸：资格缺→禁入终走查。
    pub fn walk_gate(&self) -> Result<(), AfCode> {
        for s in [ScriptQual::S1, ScriptQual::S2, ScriptQual::S3, ScriptQual::S4, ScriptQual::S5] {
            if !self.qualified.contains(&s) { return Err(AfCode::QualDenied); }
        }
        Ok(())
    }
}

// ───────────── 深化块 D426-A · 资格批账本 ─────────────
// 资格闸只记"转正名单"，缺批次留痕与批内构成。补结构化批账本。

/// 一条剧本转正批记录：批次 + 剧本 + 走查采样数 + 预演轮次。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QualBatchRec {
    pub batch: u16,
    pub script: ScriptQual,
    pub walk_samples: u32,
    pub rehearsals: u32,
}

/// 资格批账本：定容量、批去重（同批同剧本幂等）、守恒与分位。
pub struct QualLedger {
    pub recs: Vec<QualBatchRec>,
    pub cap: usize,
}

impl QualLedger {
    pub fn new(cap: usize) -> Self { QualLedger { recs: Vec::new(), cap } }
    /// 入账：满→显性 Err；同批同剧本重复→幂等零副作用。
    pub fn push(&mut self, r: QualBatchRec) -> Result<bool, AfCode> {
        if self.recs.len() >= self.cap { return Err(AfCode::QualDenied); }
        if self.recs.iter().any(|x| x.batch == r.batch && x.script == r.script) {
            return Ok(false); // 幂等闸：二次提交零副作用
        }
        self.recs.push(r);
        Ok(true)
    }
    /// 守恒恒等式：∑走查采样 ≡ ∑(按剧本分列求和)。
    pub fn conserves(&self) -> bool {
        let total: u64 = self.recs.iter().map(|r| r.walk_samples as u64).sum();
        let mut by_script: u64 = 0;
        for s in [ScriptQual::S1, ScriptQual::S2, ScriptQual::S3, ScriptQual::S4, ScriptQual::S5] {
            by_script += self.recs.iter().filter(|r| r.script == s)
                .map(|r| r.walk_samples as u64).sum::<u64>();
        }
        total == by_script
    }
    /// 剧本走查采样分位（整数排序取点，禁浮点）。
    pub fn script_quantile(&self, s: ScriptQual, q: u32) -> u32 {
        let mut vals: Vec<u32> = self.recs.iter().filter(|r| r.script == s)
            .map(|r| r.walk_samples).collect();
        if vals.is_empty() { return 0; }
        vals.sort_unstable();
        let n = vals.len();
        let idx = ((q as usize).max(1) * n + 99) / 100;
        vals[idx.min(n) - 1]
    }
    /// 预演密度投影：总预演轮次 / 总走查采样（整数千分比）。
    pub fn rehearsal_density_pm(&self) -> u32 {
        let w: u64 = self.recs.iter().map(|r| r.walk_samples as u64).sum();
        let h: u64 = self.recs.iter().map(|r| r.rehearsals as u64).sum();
        if w == 0 { return 0; }
        (h * 1000 / w) as u32
    }
    /// 批次去重计数。
    pub fn batch_count(&self) -> usize {
        let mut bs: Vec<u16> = self.recs.iter().map(|r| r.batch).collect();
        bs.sort_unstable();
        bs.dedup();
        bs.len()
    }
}

// ───────────── 深化块 D426-B · 边界探针 ─────────────

/// 探针 P1：空剧本分位→显性 Err。
pub fn probe_empty_script_quantile(lg: &QualLedger, s: ScriptQual, q: u32) -> Result<u32, AfCode> {
    if q == 0 || q > 100 { return Err(AfCode::MetricShort); }
    if lg.recs.iter().filter(|r| r.script == s).count() == 0 { return Err(AfCode::EvidenceMissing); }
    Ok(lg.script_quantile(s, q))
}

/// 探针 P2：零采样批（走查采样=0）不可作为转正依据→显性 Err。
pub fn probe_zero_sample_batch(r: &QualBatchRec) -> Result<(), AfCode> {
    if r.walk_samples == 0 { return Err(AfCode::EvidenceMissing); }
    Ok(())
}

// ───────────── 深化块 D426-C · LCG 注入回归 ─────────────

/// 8 行确定性 LCG。
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn pick(&mut self, n: u32) -> u32 { ((self.next() >> 33) as u32) % n }
}

/// 注入回归：N 轮注入未达标证据（终版+脱敏但指标低于 FLOOR_BP），
/// audit_dims 必立案该维（抓取率 100%）；负对照：达标证据零误报。
pub fn lcg_inject_underproof(rounds: usize) -> (usize, usize, usize) {
    let mut g = Lcg(0x26_26_26_26);
    let mut caught = 0usize;
    let mut false_pos = 0usize;
    for _ in 0..rounds {
        let d = FeatureDim::ALL[g.pick(3) as usize];
        let metric = g.pick(9_000); // 必 < FLOOR_BP
        let mut bk = A11yFinalBook::new();
        for x in FeatureDim::ALL {
            let v = if x == d { metric } else { FLOOR_BP };
            bk.put_evidence(Evidence { dim: x, metric_bp: v, sanitized: true, final_version: true });
        }
        match bk.audit_dims() {
            Err(AfCode::MetricShort) if bk.filed == vec![d] => caught += 1,
            _ => false_pos += 1,
        }
    }
    (rounds, caught, false_pos)
}

// ───────────── 深化块 D426-D · 序列化投影 + FNV 指纹 ─────────────

/// Evidence 定长 7 字节编码：dim(1) + metric_bp(4,LE) + sanitized(1) + final(1)。
pub fn encode_evidence(e: &Evidence) -> Vec<u8> {
    let mut out = Vec::with_capacity(7);
    out.push(e.dim as u8);
    out.extend_from_slice(&e.metric_bp.to_le_bytes());
    out.push(if e.sanitized { 1 } else { 0 });
    out.push(if e.final_version { 1 } else { 0 });
    out
}

/// 解码：错长/坏维号→显性 Err。
pub fn decode_evidence(b: &[u8]) -> Result<Evidence, AfCode> {
    if b.len() != 7 { return Err(AfCode::EvidenceMissing); }
    let dim = match FeatureDim::ALL.iter().find(|d| **d as u8 == b[0]) {
        Some(d) => *d,
        None => return Err(AfCode::EvidenceMissing),
    };
    let mut v = [0u8; 4];
    v.copy_from_slice(&b[1..5]);
    Ok(Evidence { dim, metric_bp: u32::from_le_bytes(v),
        sanitized: b[5] == 1, final_version: b[6] == 1 })
}

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for x in bytes { h ^= *x as u32; h = h.wrapping_mul(0x0100_0193); }
    h
}

/// 证据册指纹：三维证据序列化后 FNV（脱敏终版快照指纹）。
pub fn evidence_fingerprint(bk: &A11yFinalBook) -> u32 {
    let mut bytes = Vec::new();
    for d in FeatureDim::ALL {
        match bk.evidence.iter().find(|e| e.dim == d) {
            Some(e) => bytes.extend_from_slice(&encode_evidence(e)),
            None => bytes.extend_from_slice(&[0xFFu8; 7]),
        }
    }
    fnv1a(&bytes)
}

/// 指纹十六进制投影。
pub fn fingerprint_hex(h: u32) -> String {
    let mut out = String::new();
    for i in 0..8 {
        let nib = ((h >> (28 - i * 4)) & 0xF) as u8;
        out.push(if nib < 10 { (b'0' + nib) as char } else { (b'a' + nib - 10) as char });
    }
    out
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves426_checks() -> CheckSet {
    let mut set = CheckSet::new("ves426");
    let mut bk = A11yFinalBook::new();

    // C426-01 证据缺→回补码。
    chk(&mut set, "C426-01", bk.audit_dims() == Err(AfCode::EvidenceMissing), "证据缺回补");

    // C426-02 证据非终版/未脱敏拒。
    bk.put_evidence(Evidence { dim: FeatureDim::TranslateQuality, metric_bp: 9_500, sanitized: false, final_version: false });
    chk(&mut set, "C426-02", bk.audit_dims() == Err(AfCode::EvidenceMissing), "非终版拒");

    // C426-03 终版+脱敏后过该维。
    bk.evidence[0].sanitized = true;
    bk.evidence[0].final_version = true;
    chk(&mut set, "C426-03", bk.audit_dims() == Err(AfCode::EvidenceMissing), "补齐其余两维前拒");

    // C426-04 三维齐→指标达标全过。
    bk.put_evidence(Evidence { dim: FeatureDim::RegionCompliance, metric_bp: 9_200, sanitized: true, final_version: true });
    bk.put_evidence(Evidence { dim: FeatureDim::MultilingualScreenReader, metric_bp: 9_800, sanitized: true, final_version: true });
    chk(&mut set, "C426-04", bk.audit_dims().is_ok(), "三维终版全过");

    // C426-05 指标未达→立案+预案（同维双登记）。
    bk.put_evidence(Evidence { dim: FeatureDim::RegionCompliance, metric_bp: 8_500, sanitized: true, final_version: true });
    chk(&mut set, "C426-05", bk.audit_dims() == Err(AfCode::MetricShort)
        && bk.filed == vec![FeatureDim::RegionCompliance]
        && bk.plans == vec![FeatureDim::RegionCompliance], "未达立案+预案");

    // C426-06 阈值单源（压线过）。
    bk.put_evidence(Evidence { dim: FeatureDim::RegionCompliance, metric_bp: FLOOR_BP, sanitized: true, final_version: true });
    chk(&mut set, "C426-06", bk.audit_dims().is_ok(), "压线达标");

    // C426-07 资格闸：零资格→禁入。
    chk(&mut set, "C426-07", bk.walk_gate() == Err(AfCode::QualDenied), "零资格禁入");

    // C426-08 逐套转正：差一套仍拒。
    bk.qualify(ScriptQual::S1);
    bk.qualify(ScriptQual::S2);
    bk.qualify(ScriptQual::S3);
    bk.qualify(ScriptQual::S4);
    chk(&mut set, "C426-08", bk.walk_gate() == Err(AfCode::QualDenied), "差一套拒");

    // C426-09 五套齐→资格闸放行（F4175 预演转正）。
    bk.qualify(ScriptQual::S5);
    chk(&mut set, "C426-09", bk.walk_gate().is_ok() && bk.qualified.len() == 5, "五套齐放行");

    // C426-10 资格幂等。
    bk.qualify(ScriptQual::S5);
    chk(&mut set, "C426-10", bk.qualified.len() == 5, "资格幂等");

    // C426-11 证据覆盖幂等（同维替换不重复）。
    bk.put_evidence(Evidence { dim: FeatureDim::RegionCompliance, metric_bp: 9_300, sanitized: true, final_version: true });
    chk(&mut set, "C426-11", bk.evidence.len() == 3, "同维替换");

    // C426-12 三特色维单源断言（读屏口径终版位）。
    chk(&mut set, "C426-12", FeatureDim::ALL.len() == 3
        && FeatureDim::ALL[2] == FeatureDim::MultilingualScreenReader, "三特色维单源");

    // ── 深化块 D426 判据 ──
    // D426-01 资格批账本：两批五剧本入账 + 守恒恒等式。
    let mut lg = QualLedger::new(32);
    let _ = lg.push(QualBatchRec { batch: 1, script: ScriptQual::S1, walk_samples: 120, rehearsals: 6 });
    let _ = lg.push(QualBatchRec { batch: 1, script: ScriptQual::S2, walk_samples: 90, rehearsals: 4 });
    let _ = lg.push(QualBatchRec { batch: 2, script: ScriptQual::S3, walk_samples: 200, rehearsals: 10 });
    let _ = lg.push(QualBatchRec { batch: 2, script: ScriptQual::S4, walk_samples: 80, rehearsals: 5 });
    let _ = lg.push(QualBatchRec { batch: 2, script: ScriptQual::S5, walk_samples: 110, rehearsals: 5 });
    chk(&mut set, "D426-01", lg.recs.len() == 5 && lg.conserves(), "批账本守恒");

    // D426-02 幂等闸：同批同剧本二次提交零副作用。
    chk(&mut set, "D426-02", lg.push(QualBatchRec { batch: 1, script: ScriptQual::S1, walk_samples: 999, rehearsals: 0 }) == Ok(false)
        && lg.recs.len() == 5, "二次提交零副作用");

    // D426-03 整数分位：S3 剧本 P50/P100。
    chk(&mut set, "D426-03", lg.script_quantile(ScriptQual::S3, 50) == 200
        && lg.script_quantile(ScriptQual::S3, 100) == 200
        && lg.script_quantile(ScriptQual::S1, 50) == 120, "剧本分位");

    // D426-04 边界探针：空剧本/越界分位号→显性 Err。
    chk(&mut set, "D426-04", probe_empty_script_quantile(&lg, ScriptQual::S5, 0) == Err(AfCode::MetricShort)
        && probe_empty_script_quantile(&lg, ScriptQual::S5, 101) == Err(AfCode::MetricShort), "越界分位探针");

    // D426-05 边界探针：零采样批不可作转正依据。
    chk(&mut set, "D426-05", probe_zero_sample_batch(&QualBatchRec { batch: 9, script: ScriptQual::S1, walk_samples: 0, rehearsals: 1 }) == Err(AfCode::EvidenceMissing)
        && probe_zero_sample_batch(&QualBatchRec { batch: 9, script: ScriptQual::S1, walk_samples: 1, rehearsals: 1 }).is_ok(), "零采样批探针");

    // D426-06 预演密度 + 批次去重投影。
    chk(&mut set, "D426-06", lg.rehearsal_density_pm() == 60 && lg.batch_count() == 2, "预演密度投影");

    // D426-07 LCG 注入回归：12 轮未达标证据 100% 立案 + 零误报。
    let (n, c, fp) = lcg_inject_underproof(12);
    chk(&mut set, "D426-07", n == 12 && c == 12 && fp == 0, "注入立案全抓");

    // D426-08 序列化往返：证据编码→解码恒等。
    let e0 = Evidence { dim: FeatureDim::MultilingualScreenReader, metric_bp: 9_800, sanitized: true, final_version: true };
    chk(&mut set, "D426-08", decode_evidence(&encode_evidence(&e0)) == Ok(e0.clone()), "证据序列化往返");

    // D426-09 解码边界：错长/坏维号→显性 Err。
    chk(&mut set, "D426-09", decode_evidence(&[0u8; 6]) == Err(AfCode::EvidenceMissing)
        && decode_evidence(&{ let mut v = encode_evidence(&e0); v[0] = 0x7F; v }) == Err(AfCode::EvidenceMissing), "证据解码边界");

    // D426-10 证据册指纹：确定性 + 维缺失占位稳定 + 指标变即指纹变。
    let mut bk1 = A11yFinalBook::new();
    let f_empty1 = evidence_fingerprint(&bk1);
    let f_empty2 = evidence_fingerprint(&bk1);
    bk1.put_evidence(e0);
    let f_full = evidence_fingerprint(&bk1);
    chk(&mut set, "D426-10", f_empty1 == f_empty2 && f_empty1 != f_full
        && fingerprint_hex(f_full).len() == 8, "证据册指纹");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves426_smoke() {
        let set = run_ves426_checks();
        assert!(set.failed_count() == 0, "ves426 red: {}", set.red_items_text());
    }
}
