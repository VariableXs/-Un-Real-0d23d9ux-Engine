//! ves409_s22secasset — VE-F4169 · 安全资产汇总
//! T 域安全资产汇总：八组安全断言/红线/审计项归集；汇总规则（每资产挂组/挂条/挂红线三指针）；
//! 专项面（机翻出站面/语言包供应链面/地区数据路由面三面复审）；红线缺→立案补立；
//! 注入面未覆盖→补测；审计缺→回补；用户内容不出站/数据按驻留落地双核心（红线复述）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0880;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaCode {
    Ok = 0,
    RedlineMissing = SEG_BASE + 0x01, // 0x0881
    InjectUncovered = SEG_BASE + 0x02, // 0x0882
    AuditMissing = SEG_BASE + 0x03,   // 0x0883
}

/// 三专项复审面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReviewFace { MtOutbound = 0, SupplyChain = 1, ResidencyRoute = 2 }

impl ReviewFace {
    pub const ALL: [ReviewFace; 3] = [
        ReviewFace::MtOutbound, ReviewFace::SupplyChain, ReviewFace::ResidencyRoute];
    pub fn name(self) -> &'static str {
        match self {
            ReviewFace::MtOutbound => "mt-outbound", ReviewFace::SupplyChain => "supply-chain",
            ReviewFace::ResidencyRoute => "residency-route",
        }
    }
}

/// 安全资产条目：三指针（组/条/红线）+ 注入面覆盖。
#[derive(Clone, Copy)]
pub struct SecAsset {
    pub group: u8,       // 挂组
    pub card: u32,       // 挂条
    pub redline: u32,    // 挂红线锚（0 = 红线缺）
    pub inject_covered: bool, // 注入面覆盖
    pub audited: bool,   // 审计完成
}

impl SecAsset {
    pub fn linked(&self) -> bool { self.redline != 0 && self.card != 0 && self.group < 8 }
}

/// 双核心红线（域级复述）。
pub const REDLINE_CORE: [&'static str; 2] = ["user-content-no-outbound", "data-residency-landing"];

/// 台账。
pub struct SecBook {
    pub items: Vec<SecAsset>,
    pub faces: Vec<(ReviewFace, bool)>, // 三面复审
    pub cases: Vec<&'static str>,       // 立案
    pub backfills: Vec<&'static str>,   // 回补留痕
}

impl SecBook {
    pub fn new() -> Self { SecBook { items: Vec::new(), faces: Vec::new(), cases: Vec::new(), backfills: Vec::new() } }

    pub fn put(&mut self, e: SecAsset) {
        match self.items.iter_mut().find(|x| x.group == e.group) {
            Some(x) => *x = e,
            None => self.items.push(e),
        }
    }

    /// 八组归集完备。
    pub fn groups_covered(&self) -> bool { (0..8).all(|g| self.items.iter().any(|x| x.group == g)) }

    /// 红线缺立案定位。
    pub fn missing_redlines(&self) -> Vec<usize> {
        self.items.iter().enumerate().filter(|(_, x)| x.redline == 0).map(|(i, _)| i).collect()
    }

    pub fn add_redline(&mut self, idx: usize, rl: u32) -> bool {
        match self.items.get_mut(idx) {
            Some(x) => { x.redline = rl; self.cases.push("redline-added"); true }
            None => false,
        }
    }

    /// 注入面未覆盖 → 补测定位。
    pub fn inject_gaps(&self) -> Vec<usize> {
        self.items.iter().enumerate().filter(|(_, x)| !x.inject_covered).map(|(i, _)| i).collect()
    }

    pub fn cover_inject(&mut self, idx: usize) -> bool {
        match self.items.get_mut(idx) {
            Some(x) => { x.inject_covered = true; self.backfills.push("inject-covered"); true }
            None => false,
        }
    }

    /// 三面复审登记（幂等）。
    pub fn review_face(&mut self, f: ReviewFace, ok: bool) {
        match self.faces.iter_mut().find(|x| x.0 == f) {
            Some(x) => x.1 = ok,
            None => self.faces.push((f, ok)),
        }
    }
    pub fn faces_ok(&self) -> bool {
        ReviewFace::ALL.iter().all(|f| self.faces.iter().any(|x| x.0 == *f && x.1))
    }

    /// 双核心红线在册核验。
    pub fn core_redlines_registered(&self, registered: &[&'static str]) -> bool {
        REDLINE_CORE.iter().all(|c| registered.contains(c))
    }

    pub fn all_ok(&self) -> bool {
        self.groups_covered() && self.missing_redlines().is_empty()
            && self.inject_gaps().is_empty()
            && self.items.iter().all(|x| x.audited)
            && self.faces_ok()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves409_checks() -> CheckSet {
    let mut set = CheckSet::new("ves409");
    let mut bk = SecBook::new();

    // C409-01 八组归集（三指针齐 + 注入覆盖 + 审计完成）。
    for g in 0..8u8 {
        bk.put(SecAsset { group: g, card: 4000 + g as u32, redline: 0xD000 + g as u32,
            inject_covered: true, audited: true });
    }
    for f in ReviewFace::ALL.iter() { bk.review_face(*f, true); }
    chk(&mut set, "C409-01", bk.all_ok(), "八组三面全绿");

    // C409-02 红线缺立案与补立。
    let mut bk2 = SecBook::new();
    for g in 0..8u8 {
        bk2.put(SecAsset { group: g, card: 1, redline: if g == 3 { 0 } else { 2 }, inject_covered: true, audited: true });
    }
    chk(&mut set, "C409-02", bk2.missing_redlines() == vec![3], "T04 红线缺定位");

    // C409-03 补立留痕。
    bk2.add_redline(3, 0xD003);
    chk(&mut set, "C409-03", bk2.missing_redlines().is_empty() && bk2.cases.contains(&"redline-added"), "补立留痕");

    // C409-04 注入面未覆盖 → 补测。
    let mut bk3 = SecBook::new();
    for g in 0..8u8 {
        bk3.put(SecAsset { group: g, card: 1, redline: 2, inject_covered: g != 5, audited: true });
    }
    chk(&mut set, "C409-04", bk3.inject_gaps() == vec![5], "T06 注入面缺");

    // C409-05 补测留痕。
    bk3.cover_inject(5);
    chk(&mut set, "C409-05", bk3.inject_gaps().is_empty() && bk3.backfills == vec!["inject-covered"], "补测留痕");

    // C409-06 审计缺回补。
    bk3.items[2].audited = false;
    chk(&mut set, "C409-06", !bk3.all_ok(), "审计缺红");

    // C409-07 三面复审：单面失败红。
    let mut bk4 = SecBook::new();
    for g in 0..8u8 {
        bk4.put(SecAsset { group: g, card: 1, redline: 2, inject_covered: true, audited: true });
    }
    bk4.review_face(ReviewFace::MtOutbound, true);
    bk4.review_face(ReviewFace::SupplyChain, true);
    bk4.review_face(ReviewFace::ResidencyRoute, false);
    chk(&mut set, "C409-07", !bk4.faces_ok(), "驻留路由面红");

    // C409-08 双核心红线在册（域级复述）。
    chk(&mut set, "C409-08", bk4.core_redlines_registered(&[
        "user-content-no-outbound", "data-residency-landing", "extra-ok"]), "双核心在册");

    // C409-09 双核心缺一即红。
    chk(&mut set, "C409-09", !bk4.core_redlines_registered(&["user-content-no-outbound"]), "缺驻留核心红");

    // C409-10 三面名单单源。
    chk(&mut set, "C409-10", ReviewFace::ALL.len() == 3
        && ReviewFace::ALL[0].name() == "mt-outbound"
        && ReviewFace::ALL[2].name() == "residency-route", "三面单源");

    // C409-11 覆盖登记幂等。
    bk4.review_face(ReviewFace::ResidencyRoute, true);
    chk(&mut set, "C409-11", bk4.faces.len() == 3 && bk4.faces_ok(), "复审翻绿不增行");

    // C409-12 空册不全。
    let empty = SecBook::new();
    chk(&mut set, "C409-12", !empty.groups_covered() && !empty.all_ok(), "空册红");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves409_smoke() {
        let set = run_ves409_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves409 red: {:?}", red);
    }
}
