//! ves306_s15lqa — VE-F4066 · T04 翻译验证 LQA
//! 错误分类学（准确/术语/一致/风格/本地惯例/格式六类×Critical/Major/Minor 三级）+ 抽样双轨（新句全检+存量抽检）+ Critical 硬门阻断合入 + 证据指针 + 脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8A40;

pub const SAMPLING_RATE: u32 = 10; // 存量抽检 10%

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum LqaCode {
    Ok = 0,
    CriticalOpen = SEG_BASE + 0x01,
    SampleSkew = SEG_BASE + 0x02,
    ClassDispute = SEG_BASE + 0x03,
    EvidenceMissing = SEG_BASE + 0x04,
    PrivacyBreach = SEG_BASE + 0x05,
}

/// 六类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ErrClass { Accuracy = 0, Terminology = 1, Consistency = 2, Style = 3, Locale = 4, Format = 5 }

impl ErrClass {
    pub const ALL: [ErrClass; 6] = [
        ErrClass::Accuracy, ErrClass::Terminology, ErrClass::Consistency,
        ErrClass::Style, ErrClass::Locale, ErrClass::Format];
    pub fn name(self) -> &'static str {
        match self {
            ErrClass::Accuracy => "准确", ErrClass::Terminology => "术语", ErrClass::Consistency => "一致",
            ErrClass::Style => "风格", ErrClass::Locale => "本地惯例", ErrClass::Format => "格式",
        }
    }
}

/// 三级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity { Critical = 2, Major = 1, Minor = 0 }

impl Severity {
    pub fn name(self) -> &'static str {
        match self { Severity::Critical => "Critical", Severity::Major => "Major", Severity::Minor => "Minor" }
    }
}

/// LQA 错误记录（证据指针必挂）。
#[derive(Clone)]
pub struct LqaFinding {
    pub sentence_id: u64,
    pub class: ErrClass,
    pub severity: Severity,
    pub evidence_ptr: String,
    pub is_new: bool,
    pub cleared: bool,
}

pub struct LqaBook {
    pub findings: Vec<LqaFinding>,
}

impl LqaBook {
    pub fn new() -> Self { LqaBook { findings: Vec::new() } }

    pub fn record(&mut self, f: LqaFinding) -> Result<(), LqaCode> {
        if f.evidence_ptr.is_empty() { return Err(LqaCode::EvidenceMissing); }
        if f.sentence_id == 0 { return Err(LqaCode::PrivacyBreach); }
        self.findings.push(f);
        Ok(())
    }

    /// Critical 硬门：有未清 Critical → 阻断合入。
    pub fn merge_blocked(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Critical && !f.cleared)
    }

    /// 清除错误。
    pub fn clear(&mut self, sentence_id: u64) -> Result<(), LqaCode> {
        let mut found = false;
        for f in self.findings.iter_mut() {
            if f.sentence_id == sentence_id {
                f.cleared = true;
                found = true;
            }
        }
        if found { Ok(()) } else { Err(LqaCode::ClassDispute) }
    }

    /// 抽样双轨：新句全检 + 存量按比例。
    pub fn sample_plan(&self, total_existing: u32) -> (u32, u32) {
        let new_count = self.findings.iter().filter(|f| f.is_new).count() as u32;
        let sampled = total_existing * SAMPLING_RATE / 100;
        (new_count, sampled)
    }

    /// 抽样偏斜检出：存量抽检数低于计划一半。
    pub fn sample_skew(planned: u32, actual: u32) -> bool {
        planned > 0 && actual * 2 < planned
    }

    /// 分类仲裁：六类之外登记拒。
    pub fn class_valid(c: ErrClass) -> bool {
        ErrClass::ALL.contains(&c)
    }
}

pub fn run_ves306_checks() -> CheckSet {
    let mut set = CheckSet::new("ves306_s15lqa");
    let mut lb = LqaBook::new();

    // 1) 六类三级齐。
    set.add("C4066-01", ErrClass::ALL.len() == 6 && Severity::Critical > Severity::Major && Severity::Major > Severity::Minor, "六类三级");

    // 2) 证据缺拒。
    set.add("C4066-02", lb.record(LqaFinding { sentence_id: 1, class: ErrClass::Accuracy, severity: Severity::Minor, evidence_ptr: String::new(), is_new: true, cleared: false }) == Err(LqaCode::EvidenceMissing), "证据缺拒");

    // 3) 入库。
    let _ = lb.record(LqaFinding { sentence_id: 1, class: ErrClass::Accuracy, severity: Severity::Critical, evidence_ptr: String::from("ptr:1"), is_new: true, cleared: false });
    let _ = lb.record(LqaFinding { sentence_id: 2, class: ErrClass::Style, severity: Severity::Minor, evidence_ptr: String::from("ptr:2"), is_new: false, cleared: false });
    set.add("C4066-03", lb.findings.len() == 2, "入库");

    // 4) Critical 硬门。
    set.add("C4066-04", lb.merge_blocked(), "Critical 阻断");

    // 5) 清除后放行。
    let _ = lb.clear(1);
    set.add("C4066-05", !lb.merge_blocked(), "清除放行");

    // 6) 清除查无拒。
    set.add("C4066-06", lb.clear(99) == Err(LqaCode::ClassDispute), "清除查无拒");

    // 7) 抽样双轨。
    let (n, s) = lb.sample_plan(100);
    set.add("C4066-07", n == 1 && s == SAMPLING_RATE, "抽样双轨");

    // 8) 抽样偏斜检出。
    set.add("C4066-08", LqaBook::sample_skew(10, 4) && !LqaBook::sample_skew(10, 5), "偏斜检出");

    // 9) 分类有效。
    set.add("C4066-09", ErrClass::ALL.iter().all(|c| LqaBook::class_valid(*c)), "六类有效");

    // 10) 类名双射。
    let mut names: Vec<&str> = Vec::new();
    for c in ErrClass::ALL.iter() { names.push(c.name()); }
    let mut ndup = false;
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            if names[i] == names[j] { ndup = true; }
        }
    }
    set.add("C4066-10", !ndup, "类名双射");

    // 11) 隐私：sentence_id 零拒。
    set.add("C4066-11", lb.record(LqaFinding { sentence_id: 0, class: ErrClass::Format, severity: Severity::Minor, evidence_ptr: String::from("p"), is_new: false, cleared: false }) == Err(LqaCode::PrivacyBreach), "隐私红线");

    // 12) 段位自查。
    set.add("C4066-12", SEG_BASE == 0x8A40 && SAMPLING_RATE == 10, "段位与抽样率");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_gate() {
        let mut lb = LqaBook::new();
        let _ = lb.record(LqaFinding { sentence_id: 5, class: ErrClass::Terminology, severity: Severity::Critical, evidence_ptr: String::from("p"), is_new: true, cleared: false });
        assert!(lb.merge_blocked());
        let _ = lb.clear(5);
        assert!(!lb.merge_blocked());
    }
}
