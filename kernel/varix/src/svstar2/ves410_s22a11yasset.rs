//! ves410_s22a11yasset — VE-F4170 · 无障碍与隐私资产汇总
//! T 域无障碍与隐私资产汇总：八组多语言读屏证据/翻译无障碍必检/隐私评审项归集；
//! 汇总规则（每资产挂组/挂条/挂证据双指针）；T 域本色专项（多语言读屏跟读口径统一核对 +
//! 翻译无障碍必检口径核对）；口径不一→仲裁统一；证据缺→回补；隐私项缺→立案；证据脱敏复核。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0A40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AaCode {
    Ok = 0,
    CaliberConflict = SEG_BASE + 0x01, // 0x0A41
    EvidenceMissing = SEG_BASE + 0x02, // 0x0A42
    PrivacyMissing = SEG_BASE + 0x03,  // 0x0A43
}

/// 资产三类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A11yKind { SrEvidence = 0, TransMandatory = 1, PrivacyReview = 2 }

impl A11yKind {
    pub const ALL: [A11yKind; 3] = [
        A11yKind::SrEvidence, A11yKind::TransMandatory, A11yKind::PrivacyReview];
    pub fn name(self) -> &'static str {
        match self {
            A11yKind::SrEvidence => "sr-evidence", A11yKind::TransMandatory => "trans-mandatory",
            A11yKind::PrivacyReview => "privacy-review",
        }
    }
}

/// 条目：双指针（组/条）+ 证据锚 + 脱敏。
#[derive(Clone, Copy)]
pub struct A11yAsset {
    pub kind: A11yKind,
    pub group: u8,      // 挂组
    pub card: u32,      // 挂条
    pub evidence: u32,  // 挂证据（0 = 缺）
    pub sanitized: bool, // 证据脱敏复核
}

impl A11yAsset {
    pub fn linked(&self) -> bool { self.evidence != 0 && self.card != 0 && self.group < 8 }
}

/// 汇总册 + 口径统一表。
pub struct A11yBook {
    pub items: Vec<A11yAsset>,
    pub sr_caliber: Option<u32>,        // 读屏跟读口径（统一后）
    pub mandatory_caliber: Option<u32>, // 必检口径
    pub arbitrations: Vec<&'static str>,
    pub cases: Vec<&'static str>,
}

impl A11yBook {
    pub fn new() -> Self {
        A11yBook { items: Vec::new(), sr_caliber: None, mandatory_caliber: None, arbitrations: Vec::new(), cases: Vec::new() }
    }

    pub fn put(&mut self, e: A11yAsset) {
        match self.items.iter_mut().find(|x| x.kind == e.kind && x.group == e.group) {
            Some(x) => *x = e,
            None => self.items.push(e),
        }
    }

    /// 三类×八组完备（24 格）。
    pub fn grid_complete(&self) -> bool {
        A11yKind::ALL.iter().all(|k| (0..8).all(|g| self.items.iter().any(|x| x.kind == *k && x.group == g)))
    }

    /// 读屏跟读口径统一核对：各证据口径值须一致，不一 → 仲裁统一。
    pub fn check_sr_caliber(&mut self, values: &[u32]) -> bool {
        if values.is_empty() { return false; }
        let first = values[0];
        if values.iter().any(|v| *v != first) {
            self.arbitrations.push("sr-caliber-arbitrated");
            self.sr_caliber = Some(first); // 仲裁统一取首口径
            false
        } else {
            self.sr_caliber = Some(first);
            true
        }
    }

    /// 必检口径核对。
    pub fn set_mandatory(&mut self, v: u32) { self.mandatory_caliber = Some(v); }

    /// 证据缺回补定位。
    pub fn evidence_gaps(&self) -> Vec<usize> {
        self.items.iter().enumerate().filter(|(_, x)| x.evidence == 0).map(|(i, _)| i).collect()
    }

    /// 隐私项缺立案。
    pub fn privacy_gaps(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for g in 0..8u8 {
            if !self.items.iter().any(|x| x.kind == A11yKind::PrivacyReview && x.group == g) {
                out.push(g);
            }
        }
        out
    }

    pub fn file_privacy(&mut self, g: u8) { if !self.cases.contains(&"privacy-gap") { self.cases.push("privacy-gap"); } let _ = g; }

    /// 脱敏复核全册。
    pub fn sanitized_all(&self) -> bool { self.items.iter().all(|x| x.sanitized) }

    /// 双口径就绪（域本色终态）。
    pub fn calibers_ready(&self) -> bool { self.sr_caliber.is_some() && self.mandatory_caliber.is_some() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves410_checks() -> CheckSet {
    let mut set = CheckSet::new("ves410");
    let mut bk = A11yBook::new();

    // C410-01 三类×八组 24 格归集。
    for k in A11yKind::ALL.iter() {
        for g in 0..8u8 {
            bk.put(A11yAsset { kind: *k, group: g, card: 4000 + g as u32, evidence: 0xE000 + g as u32, sanitized: true });
        }
    }
    chk(&mut set, "C410-01", bk.grid_complete() && bk.items.len() == 24, "24 格齐");

    // C410-02 读屏跟读口径统一（一致过）。
    chk(&mut set, "C410-02", bk.check_sr_caliber(&[7, 7, 7]) && bk.sr_caliber == Some(7), "口径 7 统一");

    // C410-03 口径不一 → 仲裁统一留痕。
    let mut bk2 = A11yBook::new();
    chk(&mut set, "C410-03", !bk2.check_sr_caliber(&[7, 5, 7])
        && bk2.arbitrations.contains(&"sr-caliber-arbitrated") && bk2.sr_caliber == Some(7), "仲裁统一取首");

    // C410-04 必检口径就绪（域本色双口径终态）。
    bk.set_mandatory(30); // 必检 30 项
    chk(&mut set, "C410-04", bk.calibers_ready(), "双口径就绪");

    // C410-05 证据缺回补定位。
    let mut bk3 = A11yBook::new();
    for g in 0..8u8 {
        bk3.put(A11yAsset { kind: A11yKind::SrEvidence, group: g, card: 1,
            evidence: if g == 2 { 0 } else { 9 }, sanitized: true });
    }
    chk(&mut set, "C410-05", bk3.evidence_gaps() == vec![2], "T03 证据缺定位");

    // C410-06 隐私项缺立案。
    let mut bk4 = A11yBook::new();
    for g in 0..7u8 {
        bk4.put(A11yAsset { kind: A11yKind::PrivacyReview, group: g, card: 1, evidence: 9, sanitized: true });
    }
    chk(&mut set, "C410-06", bk4.privacy_gaps() == vec![7] && { bk4.file_privacy(7); bk4.cases.contains(&"privacy-gap") }, "T08 隐私项缺立案");

    // C410-07 脱敏复核。
    bk4.items[0].sanitized = false;
    chk(&mut set, "C410-07", !bk4.sanitized_all(), "未脱敏检出");

    // C410-08 双指针核验。
    chk(&mut set, "C410-08", bk.items.iter().all(|x| x.card != 0 && x.evidence != 0), "双指针全挂");

    // C410-09 三类名单单源。
    chk(&mut set, "C410-09", A11yKind::ALL.len() == 3 && A11yKind::ALL[0].name() == "sr-evidence"
        && A11yKind::ALL[2].name() == "privacy-review", "三类单源");

    // C410-10 覆盖登记幂等。
    let before = bk.items.len();
    bk.put(A11yAsset { kind: A11yKind::SrEvidence, group: 0, card: 4000, evidence: 0xE001, sanitized: true });
    chk(&mut set, "C410-10", bk.items.len() == before, "覆盖不增行");

    // C410-11 证据回补复绿。
    bk3.items[2].evidence = 0xE002;
    chk(&mut set, "C410-11", bk3.evidence_gaps().is_empty(), "回补复绿");

    // C410-12 空册红。
    let empty = A11yBook::new();
    chk(&mut set, "C410-12", !empty.grid_complete() && !empty.calibers_ready(), "空册红");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves410_smoke() {
        let set = run_ves410_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves410 red: {:?}", red);
    }
}
