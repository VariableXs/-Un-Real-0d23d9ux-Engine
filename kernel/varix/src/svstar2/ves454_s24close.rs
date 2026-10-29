//! ves454_s24close — VE-F4220 · U01 组收口双签
//! U01 组收口双签：20 项自查+联调回执+API 冻结哈希+文档齐备证明——四件齐备双签；
//! 双签分离禁同人；四件缺一→拒签；同人→作废；API 未冻结→硬性拒签；
//! 签收含无障碍契约层确认项（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x15E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    Ok = 0,
    PieceMissing = SEG_BASE + 0x01, // 0x15E1
    SamePerson = SEG_BASE + 0x02,   // 0x15E2
    NotFrozen = SEG_BASE + 0x03,    // 0x15E3 硬性拒签
    A11yLayerShort = SEG_BASE + 0x04, // 0x15E4（域本色）
}

/// 收口四件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosePiece { SelfCheck20 = 0, JointReceipt = 1, ApiHash = 2, DocsProof = 3 }

impl ClosePiece {
    pub const ALL: [ClosePiece; 4] = [
        ClosePiece::SelfCheck20, ClosePiece::JointReceipt, ClosePiece::ApiHash, ClosePiece::DocsProof];
}

/// 收口清单。
pub struct CloseList {
    pub pieces: Vec<ClosePiece>,
    pub api_frozen: bool,     // API 冻结哈希在册
    pub a11y_confirmed: bool, // 无障碍契约层确认项（域本色）
}

impl CloseList {
    pub fn new() -> Self { CloseList { pieces: Vec::new(), api_frozen: false, a11y_confirmed: false } }
    pub fn put(&mut self, p: ClosePiece) { if !self.pieces.contains(&p) { self.pieces.push(p); } }
    pub fn complete(&self) -> bool { ClosePiece::ALL.iter().all(|p| self.pieces.contains(p)) }
}

/// 双签。
pub struct DualSign { pub a: Option<&'static str>, pub b: Option<&'static str>, pub valid: bool }

impl DualSign {
    pub fn new() -> Self { DualSign { a: None, b: None, valid: false } }
    pub fn sign_a(&mut self, who: &'static str) { self.a = Some(who); }
    pub fn sign_b(&mut self, who: &'static str) -> Result<(), ClCode> {
        if let Some(x) = self.a {
            if x == who { return Err(ClCode::SamePerson); }
        }
        self.b = Some(who);
        self.valid = true;
        Ok(())
    }
}

/// 收口裁决：四件齐+API 冻结+a11y 确认+双签有效。
pub fn try_close(list: &CloseList, sign: &DualSign) -> Result<(), ClCode> {
    if !list.complete() { return Err(ClCode::PieceMissing); }
    if !list.api_frozen { return Err(ClCode::NotFrozen); }
    if !sign.valid { return Err(ClCode::SamePerson); }
    if !list.a11y_confirmed { return Err(ClCode::A11yLayerShort); }
    Ok(())
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves454_checks() -> CheckSet {
    let mut set = CheckSet::new("ves454");
    let mut list = CloseList::new();

    // C454-01 四件缺→拒签（缺件定位）。
    list.put(ClosePiece::SelfCheck20);
    list.put(ClosePiece::JointReceipt);
    let mut sign = DualSign::new();
    sign.sign_a("wb-a");
    chk(&mut set, "C454-01", try_close(&list, &sign) == Err(ClCode::PieceMissing), "缺件拒签");

    // C454-02 四件齐但 API 未冻结→硬性拒签。
    list.put(ClosePiece::ApiHash);
    list.put(ClosePiece::DocsProof);
    chk(&mut set, "C454-02", try_close(&list, &sign) == Err(ClCode::NotFrozen), "未冻结硬拒");

    // C454-03 API 冻结后缺 B 签→拒。
    list.api_frozen = true;
    chk(&mut set, "C454-03", try_close(&list, &sign) == Err(ClCode::SamePerson), "缺 B 签拒");

    // C454-04 B 签齐但 a11y 契约层未确认→拒（域本色）。
    sign.sign_b("wb-b").unwrap();
    chk(&mut set, "C454-04", try_close(&list, &sign) == Err(ClCode::A11yLayerShort), "契约层缺拒");

    // C454-05 全条件齐→收口通过。
    list.a11y_confirmed = true;
    chk(&mut set, "C454-05", try_close(&list, &sign).is_ok(), "收口通过");

    // C454-06 同人双签拒。
    let mut bad = DualSign::new();
    bad.sign_a("wb-a");
    chk(&mut set, "C454-06", bad.sign_b("wb-a") == Err(ClCode::SamePerson) && !bad.valid, "同人作废");

    // C454-07 四件名单单源。
    chk(&mut set, "C454-07", ClosePiece::ALL.len() == 4, "四件单源");

    // C454-08 API 冻结优先于双签校验（未冻结先拒）。
    let l2 = CloseList { pieces: ClosePiece::ALL.to_vec(), api_frozen: false, a11y_confirmed: true };
    let s2 = DualSign { a: Some("x"), b: Some("y"), valid: true };
    chk(&mut set, "C454-08", try_close(&l2, &s2) == Err(ClCode::NotFrozen), "冻结先拒");

    // C454-09 空双签（缺 A）→拒。
    let empty = DualSign::new();
    let l3 = CloseList { pieces: ClosePiece::ALL.to_vec(), api_frozen: true, a11y_confirmed: true };
    chk(&mut set, "C454-09", try_close(&l3, &empty) == Err(ClCode::SamePerson), "空签拒");

    // C454-10 a11y 确认优先于通过（最后闸）。
    let l4 = CloseList { pieces: ClosePiece::ALL.to_vec(), api_frozen: true, a11y_confirmed: false };
    chk(&mut set, "C454-10", try_close(&l4, &s2) == Err(ClCode::A11yLayerShort), "末闸确认");

    // C454-11 件登记幂等。
    list.put(ClosePiece::DocsProof);
    chk(&mut set, "C454-11", list.pieces.len() == 4, "件幂等");

    // C454-12 错误码段位单源。
    chk(&mut set, "C454-12", ClCode::PieceMissing as u16 == SEG_BASE + 1
        && ClCode::A11yLayerShort as u16 == SEG_BASE + 4, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves454_smoke() {
        let set = run_ves454_checks();
        assert!(set.failed_count() == 0, "ves454 red: {}", set.red_items_text());
    }
}
