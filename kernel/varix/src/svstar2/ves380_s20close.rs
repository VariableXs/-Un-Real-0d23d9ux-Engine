//! ves380_s20close — VE-F4140 · T07 组收口双签
//! 四件齐备双签：20 项自查 + 清账证明 + 基线冻结哈希 + 衔接回执；
//! 自查签与复核签分离禁同人；四件缺一→拒签；同人→作废；基线未冻→硬性拒签；
//! 签收含无障碍预算保底确认项（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xDC40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    Ok = 0,
    ItemMissing = SEG_BASE + 0x01,
    SamePerson = SEG_BASE + 0x02,
    BaselineUnfrozen = SEG_BASE + 0x03,
    A11yFloorMissing = SEG_BASE + 0x04,
    Invalidated = SEG_BASE + 0x05,
}

/// 收口四件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseItem { SelfCheck = 0, LedgerProof = 1, BaselineHash = 2, LinkReceipt = 3 }

impl CloseItem {
    pub const ALL: [CloseItem; 4] = [
        CloseItem::SelfCheck, CloseItem::LedgerProof, CloseItem::BaselineHash, CloseItem::LinkReceipt];
    pub fn name(self) -> &'static str {
        match self {
            CloseItem::SelfCheck => "self-check",
            CloseItem::LedgerProof => "ledger-proof",
            CloseItem::BaselineHash => "baseline-hash",
            CloseItem::LinkReceipt => "link-receipt",
        }
    }
}

/// 收口清单：四件登记 + 基线冻结哈希。
pub struct CloseChecklist {
    pub items: Vec<CloseItem>,
    pub baseline_hash: u64, // 0 = 未冻结
}

impl CloseChecklist {
    pub fn new() -> Self { CloseChecklist { items: Vec::new(), baseline_hash: 0 } }

    pub fn put(&mut self, it: CloseItem) { if !self.items.contains(&it) { self.items.push(it); } }

    /// 冻结基线（哈希非零才有效）。
    pub fn freeze(&mut self, hash: u64) -> bool {
        if hash == 0 { return false; }
        self.baseline_hash = hash;
        true
    }

    pub fn frozen(&self) -> bool { self.baseline_hash != 0 }

    pub fn missing(&self) -> Vec<&'static str> {
        CloseItem::ALL.iter().filter(|c| !self.items.contains(&**c)).map(|c| c.name()).collect()
    }

    pub fn four_complete(&self) -> bool { self.missing().is_empty() }
}

/// 双签记录：自查签 + 复核签（禁同人）。
pub struct DualSign {
    pub signer_a: Option<&'static str>,
    pub signer_b: Option<&'static str>,
    pub valid: bool,
    pub void_reason: Option<ClCode>,
}

impl DualSign {
    pub fn new() -> Self { DualSign { signer_a: None, signer_b: None, valid: false, void_reason: None } }

    /// 自查签（第一签）。
    pub fn sign_a(&mut self, who: &'static str) { self.signer_a = Some(who); }

    /// 复核签（第二签）：同人→作废。
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

    /// 作废（重签前清场）。
    pub fn void(&mut self, why: ClCode) {
        self.valid = false;
        self.void_reason = Some(why);
        self.signer_a = None;
        self.signer_b = None;
    }
}

/// 收口裁决：四件齐备 + 双签分离 + 基线冻结 + 无障碍保底确认 → 通过。
pub struct CloseVerdict {
    pub passed: bool,
    pub rejected_by: Option<ClCode>,
    pub a11y_floor_ok: bool,
}

/// 无障碍预算保底确认项（域本色）：保底档位 id 必须非零且在登记档内。
pub const A11Y_FLOOR_TIERS: [u32; 3] = [150, 200, 250]; // ms 档

pub fn a11y_floor_ok(tier: u32) -> bool { A11Y_FLOOR_TIERS.contains(&tier) }

/// 收口核验：任一前置不满足即拒签并给出拒因。
pub fn try_close(
    list: &CloseChecklist, sign: &DualSign, a11y_tier: u32,
) -> CloseVerdict {
    let a11y = a11y_floor_ok(a11y_tier);
    let rejected_by = if !list.four_complete() {
        Some(ClCode::ItemMissing)
    } else if !list.frozen() {
        Some(ClCode::BaselineUnfrozen)
    } else if !sign.valid {
        Some(sign.void_reason.unwrap_or(ClCode::SamePerson))
    } else if !a11y {
        Some(ClCode::A11yFloorMissing)
    } else {
        None
    };
    CloseVerdict { passed: rejected_by.is_none(), rejected_by, a11y_floor_ok: a11y }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves380_checks() -> CheckSet {
    let mut set = CheckSet::new("ves380");
    let mut list = CloseChecklist::new();

    // C380-01 四件缺一：只挂两件 → 缺件拒。
    list.put(CloseItem::SelfCheck);
    list.put(CloseItem::LedgerProof);
    let mut sign = DualSign::new();
    sign.sign_a("wb-a");
    sign.sign_b("wb-b").unwrap();
    let v = try_close(&list, &sign, 150);
    chk(&mut set, "C380-01", !v.passed && v.rejected_by == Some(ClCode::ItemMissing), "缺两件即拒");

    // C380-02 缺件名单定位。
    chk(&mut set, "C380-02",
        list.missing() == vec!["baseline-hash", "link-receipt"], "缺两件定位");

    // C380-03 基线未冻→硬性拒签（四件齐但哈希 0）。
    list.put(CloseItem::BaselineHash);
    list.put(CloseItem::LinkReceipt);
    let v = try_close(&list, &sign, 150);
    chk(&mut set, "C380-03", !v.passed && v.rejected_by == Some(ClCode::BaselineUnfrozen), "未冻硬拒");

    // C380-04 冻结哈希 0 无效。
    chk(&mut set, "C380-04", !list.freeze(0) && !list.frozen(), "零哈希拒冻");

    // C380-05 冻结有效哈希后四件齐备。
    chk(&mut set, "C380-05", list.freeze(0x7E57C0DE) && list.frozen() && list.four_complete(), "冻结+四件齐");

    // C380-06 双签分离有效 → 收口通过（含 a11y 保底 150）。
    let v = try_close(&list, &sign, 150);
    chk(&mut set, "C380-06", v.passed && v.a11y_floor_ok, "四件+双签+冻结 → 过");

    // C380-07 同人双签→作废。
    let mut bad = DualSign::new();
    bad.sign_a("wb-a");
    let r = bad.sign_b("wb-a");
    chk(&mut set, "C380-07", r == Err(ClCode::SamePerson) && !bad.valid
        && bad.void_reason == Some(ClCode::SamePerson), "同人拒签作废");

    // C380-08 作废后重签（换人）可复通。
    let mut re = DualSign::new();
    re.sign_a("wb-a");
    re.sign_b("wb-a").unwrap_err();
    re.void(ClCode::SamePerson);
    re.sign_a("wb-a");
    re.sign_b("wb-c").unwrap();
    chk(&mut set, "C380-08", re.valid && re.void_reason == Some(ClCode::SamePerson), "留痕换签复通");

    // C380-09 a11y 保底档外（如 120）→ 拒签。
    let v = try_close(&list, &sign, 120);
    chk(&mut set, "C380-09", !v.passed && v.rejected_by == Some(ClCode::A11yFloorMissing), "保底档外拒");

    // C380-10 三档保底档位表核验。
    chk(&mut set, "C380-10",
        a11y_floor_ok(150) && a11y_floor_ok(200) && a11y_floor_ok(250) && !a11y_floor_ok(300), "三档保底单源");

    // C380-11 四件名单单源（四件名）。
    chk(&mut set, "C380-11",
        CloseItem::ALL[0].name() == "self-check" && CloseItem::ALL[3].name() == "link-receipt"
            && CloseItem::ALL.len() == 4, "四件单源");

    // C380-12 双签未完成（缺 B 签）不算有效。
    let half = DualSign::new();
    let mut half = half;
    half.sign_a("wb-a");
    let v = try_close(&list, &half, 150);
    chk(&mut set, "C380-12", !v.passed && v.rejected_by.is_some(), "缺复核签不通过");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves380_smoke() {
        let set = run_ves380_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves380 red: {:?}", red);
    }

    #[test]
    fn ves380_freeze_reject_zero() {
        let mut l = CloseChecklist::new();
        assert!(!l.freeze(0));
        assert!(l.freeze(1));
    }

    #[test]
    fn ves380_void_clears() {
        let mut d = DualSign::new();
        d.sign_a("x");
        d.sign_b("y").unwrap();
        d.void(ClCode::Invalidated);
        assert!(!d.valid);
        assert!(d.signer_a.is_none() && d.signer_b.is_none());
    }
}
