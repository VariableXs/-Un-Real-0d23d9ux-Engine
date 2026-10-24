//! ves400_s21close — VE-F4160 · T08 组收口双签
//! 四件齐备双签：20 项自查 + 走查汇总回执 + fuzz 总闸三零线 + 资产汇总齐备；
//! 自查签与复核签分离禁同人；四件缺一→拒签；同人→作废；阻断未消→硬性拒签；
//! 双签后 T 域八组全收口（T01-T08 8/8）触发域收官预备；签收含无障碍工具族齐备确认项（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x00A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    Ok = 0,
    ItemMissing = SEG_BASE + 0x01,   // 0x00A1
    SamePerson = SEG_BASE + 0x02,    // 0x00A2
    BlockOpen = SEG_BASE + 0x03,     // 0x00A3
    A11yToolShort = SEG_BASE + 0x04, // 0x00A4
    Invalidated = SEG_BASE + 0x05,   // 0x00A5
}

/// 收口四件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseItem { SelfCheck = 0, WalkReceipt = 1, FuzzZeroLine = 2, AssetComplete = 3 }

impl CloseItem {
    pub const ALL: [CloseItem; 4] = [
        CloseItem::SelfCheck, CloseItem::WalkReceipt, CloseItem::FuzzZeroLine, CloseItem::AssetComplete];
    pub fn name(self) -> &'static str {
        match self {
            CloseItem::SelfCheck => "self-check", CloseItem::WalkReceipt => "walk-receipt",
            CloseItem::FuzzZeroLine => "fuzz-zero-line", CloseItem::AssetComplete => "asset-complete",
        }
    }
}

/// 收口清单：四件 + 走查阻断状态 + a11y 工具族确认。
pub struct CloseChecklist {
    pub items: Vec<CloseItem>,
    pub blockers_open: bool,      // 走查阻断未消
    pub a11y_tools_ready: bool,   // 无障碍工具族齐备确认（域本色）
}

impl CloseChecklist {
    pub fn new() -> Self { CloseChecklist { items: Vec::new(), blockers_open: false, a11y_tools_ready: false } }

    pub fn put(&mut self, it: CloseItem) { if !self.items.contains(&it) { self.items.push(it); } }

    pub fn missing(&self) -> Vec<&'static str> {
        CloseItem::ALL.iter().filter(|c| !self.items.contains(&**c)).map(|c| c.name()).collect()
    }
    pub fn four_complete(&self) -> bool { self.missing().is_empty() }
}

/// 双签记录。
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
        self.valid = false;
        self.void_reason = Some(why);
        self.signer_a = None;
        self.signer_b = None;
    }
}

/// 域收官预备：T01-T08 八组收口计数（8/8 触发预备）。
pub struct DomainWrapup { pub groups_closed: u8 }

impl DomainWrapup {
    pub const TOTAL: u8 = 8;
    pub fn ready(&self) -> bool { self.groups_closed >= Self::TOTAL }
    pub fn progress(&self) -> u8 { self.groups_closed.min(Self::TOTAL) }
}

/// 收口裁决。
pub struct CloseVerdict { pub passed: bool, pub rejected_by: Option<ClCode> }

pub fn try_close(list: &CloseChecklist, sign: &DualSign) -> CloseVerdict {
    let rejected_by = if !list.four_complete() {
        Some(ClCode::ItemMissing)
    } else if list.blockers_open {
        Some(ClCode::BlockOpen)
    } else if !sign.valid {
        Some(sign.void_reason.unwrap_or(ClCode::SamePerson))
    } else if !list.a11y_tools_ready {
        Some(ClCode::A11yToolShort)
    } else {
        None
    };
    CloseVerdict { passed: rejected_by.is_none(), rejected_by }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves400_checks() -> CheckSet {
    let mut set = CheckSet::new("ves400");
    let mut list = CloseChecklist::new();

    // C400-01 四件缺一拒签。
    list.put(CloseItem::SelfCheck);
    list.put(CloseItem::WalkReceipt);
    let mut sign = DualSign::new();
    sign.sign_a("wb-a");
    sign.sign_b("wb-b").unwrap();
    let v = try_close(&list, &sign);
    chk(&mut set, "C400-01", !v.passed && v.rejected_by == Some(ClCode::ItemMissing), "缺两件即拒");

    // C400-02 缺件定位。
    chk(&mut set, "C400-02", list.missing() == vec!["fuzz-zero-line", "asset-complete"], "缺两件定位");

    // C400-03 阻断未消硬拒（四件齐但走查有阻断）。
    list.put(CloseItem::FuzzZeroLine);
    list.put(CloseItem::AssetComplete);
    list.blockers_open = true;
    let v = try_close(&list, &sign);
    chk(&mut set, "C400-03", !v.passed && v.rejected_by == Some(ClCode::BlockOpen), "阻断硬拒");

    // C400-04 阻断消解后 a11y 工具族缺仍拒（域本色）。
    list.blockers_open = false;
    let v = try_close(&list, &sign);
    chk(&mut set, "C400-04", !v.passed && v.rejected_by == Some(ClCode::A11yToolShort), "工具族缺拒");

    // C400-05 a11y 确认后收口通过。
    list.a11y_tools_ready = true;
    let v = try_close(&list, &sign);
    chk(&mut set, "C400-05", v.passed, "四件+双签+无阻断+工具族齐 → 过");

    // C400-06 同人双签作废。
    let mut bad = DualSign::new();
    bad.sign_a("wb-a");
    chk(&mut set, "C400-06", bad.sign_b("wb-a") == Err(ClCode::SamePerson) && !bad.valid, "同人作废");

    // C400-07 作废后换人重签复通（留痕）。
    let mut re = DualSign::new();
    re.sign_a("wb-a");
    re.sign_b("wb-a").unwrap_err();
    re.void(ClCode::SamePerson);
    re.sign_a("wb-a");
    re.sign_b("wb-c").unwrap();
    chk(&mut set, "C400-07", re.valid && re.void_reason == Some(ClCode::SamePerson), "换签复通留痕");

    // C400-08 域收官预备：T 域八组计数。
    let mut w = DomainWrapup { groups_closed: 7 };
    chk(&mut set, "C400-08", !w.ready() && w.progress() == 7, "7/8 未触发");

    // C400-09 8/8 触发域收官预备。
    w.groups_closed = 8;
    chk(&mut set, "C400-09", w.ready() && DomainWrapup::TOTAL == 8, "8/8 触发预备");

    // C400-10 超计数钳制。
    w.groups_closed = 12;
    chk(&mut set, "C400-10", w.progress() == 8 && w.ready(), "计数钳制 8");

    // C400-11 四件名单单源。
    chk(&mut set, "C400-11", CloseItem::ALL.len() == 4
        && CloseItem::ALL[0].name() == "self-check"
        && CloseItem::ALL[3].name() == "asset-complete", "四件单源");

    // C400-12 缺复核签不通过。
    let half = DualSign::new();
    let mut half = half;
    half.sign_a("wb-a");
    let v = try_close(&list, &half);
    chk(&mut set, "C400-12", !v.passed && v.rejected_by.is_some(), "缺 B 签不通过");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves400_smoke() {
        let set = run_ves400_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves400 red: {:?}", red);
    }

    #[test]
    fn ves400_void_clears() {
        let mut d = DualSign::new();
        d.sign_a("x");
        d.sign_b("y").unwrap();
        d.void(ClCode::Invalidated);
        assert!(!d.valid);
        assert!(d.signer_a.is_none() && d.signer_b.is_none());
    }
}
