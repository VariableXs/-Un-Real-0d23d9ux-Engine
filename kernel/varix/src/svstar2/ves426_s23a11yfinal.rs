//! ves426_s23a11yfinal — VE-F4186 · T 域无障碍终审
//! 终审：T 域多语言无障碍本体终审——三特色维（翻译质量/地区合规/多语言读屏）证据终版核验
//! +五套剧本走查前置资格核（F4175 预演转正审）；
//! 证据缺→回补；特色维指标未达→立案+预案；资格缺→禁入终走查；
//! 本条即域本色终审（多语言读屏口径终版）；证据终版脱敏复核。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
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

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves426_smoke() {
        let set = run_ves426_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves426 red: {:?}", red);
    }
}
