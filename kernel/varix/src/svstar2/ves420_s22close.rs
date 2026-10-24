//! ves420_s22close — VE-F4180 · T09 组收口双签
//! T09 组收口双签：20 项核验汇总 + 28 对对账回执 + 六维资产汇总 + 预备报告出具——四件齐备后双签；
//! 双签规则（自查签 + 复核签分离，禁同人）；四件缺一→拒签；同人双签→作废重签；逾期→催办；
//! 签收含多语言无障碍资产齐备确认项（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x0FA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    Ok = 0,
    ItemMissing = SEG_BASE + 0x01,   // 0x0FA1
    SamePerson = SEG_BASE + 0x02,    // 0x0FA2
    A11yAssetShort = SEG_BASE + 0x03, // 0x0FA3
    Invalidated = SEG_BASE + 0x04,   // 0x0FA4
}

/// 收口四件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseItem { VerifySummary = 0, PairReceipt = 1, SixDimAssets = 2, GoReport = 3 }

impl CloseItem {
    pub const ALL: [CloseItem; 4] = [
        CloseItem::VerifySummary, CloseItem::PairReceipt, CloseItem::SixDimAssets, CloseItem::GoReport];
    pub fn name(self) -> &'static str {
        match self {
            CloseItem::VerifySummary => "verify-summary", CloseItem::PairReceipt => "pair-receipt",
            CloseItem::SixDimAssets => "six-dim-assets", CloseItem::GoReport => "go-report",
        }
    }
}

/// 收口清单 + 催办。
pub struct CloseChecklist {
    pub items: Vec<CloseItem>,
    pub a11y_assets_ready: bool, // 多语言无障碍资产齐备确认（域本色）
    pub overdue_days: u32,
    pub urges: u32,
}

impl CloseChecklist {
    pub const OVERDUE_LIMIT: u32 = 14;

    pub fn new() -> Self { CloseChecklist { items: Vec::new(), a11y_assets_ready: false, overdue_days: 0, urges: 0 } }

    pub fn put(&mut self, it: CloseItem) { if !self.items.contains(&it) { self.items.push(it); } }
    pub fn missing(&self) -> Vec<&'static str> {
        CloseItem::ALL.iter().filter(|c| !self.items.contains(&**c)).map(|c| c.name()).collect()
    }
    pub fn four_complete(&self) -> bool { self.missing().is_empty() }

    /// 逾期催办（幂等计数）。
    pub fn urge_if_overdue(&mut self) -> bool {
        if self.overdue_days > Self::OVERDUE_LIMIT { self.urges += 1; true } else { false }
    }
}

/// 双签。
pub struct DualSign {
    pub signer_a: Option<&'static str>,
    pub signer_b: Option<&'static str>,
    pub valid: bool,
    pub void_reason: Option<ClCode>,
}

impl DualSign {
    pub fn new() -> Self { DualSign { signer_a: None, signer_b: None, valid: false, void_reason: None } }
    pub fn sign_a(&mut self, who: &'static str) { self.signer_a = Some(who); }
    pub fn sign_b(&mut self, who: &'static str) -> Result<(), ClCode> {
        if let Some(a) = self.signer_a {
            if a == who {
                self.valid = false;
                self.void_reason = Some(ClCode::SamePerson);
                return Err(ClCode::SamePerson);
            }
        }
        self.signer_b = Some(who);
        self.valid = true;
        Ok(())
    }
    pub fn void(&mut self, why: ClCode) {
        self.valid = false; self.void_reason = Some(why);
        self.signer_a = None; self.signer_b = None;
    }
}

/// 收口裁决。
pub struct CloseVerdict { pub passed: bool, pub rejected_by: Option<ClCode> }

pub fn try_close(list: &CloseChecklist, sign: &DualSign) -> CloseVerdict {
    let rejected_by = if !list.four_complete() {
        Some(ClCode::ItemMissing)
    } else if !sign.valid {
        Some(sign.void_reason.unwrap_or(ClCode::SamePerson))
    } else if !list.a11y_assets_ready {
        Some(ClCode::A11yAssetShort)
    } else {
        None
    };
    CloseVerdict { passed: rejected_by.is_none(), rejected_by }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves420_checks() -> CheckSet {
    let mut set = CheckSet::new("ves420");
    let mut list = CloseChecklist::new();

    // C420-01 四件缺一拒签。
    list.put(CloseItem::VerifySummary);
    list.put(CloseItem::PairReceipt);
    let mut sign = DualSign::new();
    sign.sign_a("wb-a");
    sign.sign_b("wb-b").unwrap();
    let v = try_close(&list, &sign);
    chk(&mut set, "C420-01", !v.passed && v.rejected_by == Some(ClCode::ItemMissing), "缺两件拒");

    // C420-02 缺件定位。
    chk(&mut set, "C420-02", list.missing() == vec!["six-dim-assets", "go-report"], "缺件定位");

    // C420-03 四件齐但 a11y 资产缺 → 拒（域本色）。
    list.put(CloseItem::SixDimAssets);
    list.put(CloseItem::GoReport);
    let v = try_close(&list, &sign);
    chk(&mut set, "C420-03", !v.passed && v.rejected_by == Some(ClCode::A11yAssetShort), "a11y 资产缺拒");

    // C420-04 a11y 确认后通过。
    list.a11y_assets_ready = true;
    chk(&mut set, "C420-04", try_close(&list, &sign).passed, "全件齐通过");

    // C420-05 同人双签作废。
    let mut bad = DualSign::new();
    bad.sign_a("wb-a");
    chk(&mut set, "C420-05", bad.sign_b("wb-a") == Err(ClCode::SamePerson) && !bad.valid, "同人作废");

    // C420-06 作废重签复通。
    let mut re = DualSign::new();
    re.sign_a("wb-a");
    re.sign_b("wb-a").unwrap_err();
    re.void(ClCode::SamePerson);
    re.sign_a("wb-a");
    re.sign_b("wb-c").unwrap();
    chk(&mut set, "C420-06", re.valid && re.void_reason == Some(ClCode::SamePerson), "重签复通留痕");

    // C420-07 逾期催办（14 天线）。
    let mut l2 = CloseChecklist::new();
    l2.overdue_days = 15;
    chk(&mut set, "C420-07", l2.urge_if_overdue() && l2.urges == 1, "逾期催办");

    // C420-08 压线不催。
    l2.overdue_days = 14;
    chk(&mut set, "C420-08", !l2.urge_if_overdue(), "压线不催");

    // C420-09 催办幂等计数。
    l2.overdue_days = 20;
    l2.urge_if_overdue();
    l2.urge_if_overdue();
    chk(&mut set, "C420-09", l2.urges == 3, "催办累计");

    // C420-10 四件名单单源。
    chk(&mut set, "C420-10", CloseItem::ALL.len() == 4
        && CloseItem::ALL[0].name() == "verify-summary"
        && CloseItem::ALL[3].name() == "go-report", "四件单源");

    // C420-11 缺 B 签不通过。
    let mut half = DualSign::new();
    half.sign_a("wb-a");
    chk(&mut set, "C420-11", !try_close(&list, &half).passed, "缺复核签不通过");

    // C420-12 void 清场。
    let mut d = DualSign::new();
    d.sign_a("x"); d.sign_b("y").unwrap();
    d.void(ClCode::Invalidated);
    chk(&mut set, "C420-12", !d.valid && d.signer_a.is_none() && d.signer_b.is_none(), "作废清场");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves420_smoke() {
        let set = run_ves420_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves420 red: {:?}", red);
    }
}
