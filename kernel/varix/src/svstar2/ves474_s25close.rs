//! ves474_s25close — VE-F4240 · U02 组收口双签
//! U02 组收口双签：20 项自查+联调回执+性能达标证明+安全记录——四件齐备双签；
//! 自查签与复核签分离禁同人；四件缺一→拒签；同人→作废；三线未达→硬性拒签；
//! 签收含无障碍判据确认项（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x19A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    Ok = 0,
    PieceMissing = SEG_BASE + 0x01, // 0x19A1
    SamePerson = SEG_BASE + 0x02,   // 0x19A2
    PerfNotMet = SEG_BASE + 0x03,   // 0x19A3 硬性拒签
    A11yUntagged = SEG_BASE + 0x04, // 0x19A4（域本色）
}

/// 四件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Piece { SelfCheck20 = 0, JointReceipt = 1, PerfProof = 2, SecRecord = 3 }

impl Piece {
    pub const ALL: [Piece; 4] = [Piece::SelfCheck20, Piece::JointReceipt, Piece::PerfProof, Piece::SecRecord];
}

pub struct CloseList {
    pub pieces: Vec<Piece>,
    pub perf_met: bool,       // 三线达标证明
    pub a11y_confirmed: bool, // a11y 判据确认项（域本色）
}

impl CloseList {
    pub fn new() -> Self { CloseList { pieces: Vec::new(), perf_met: false, a11y_confirmed: false } }
    pub fn put(&mut self, p: Piece) { if !self.pieces.contains(&p) { self.pieces.push(p); } }
    pub fn complete(&self) -> bool { Piece::ALL.iter().all(|p| self.pieces.contains(p)) }
}

pub struct DualSign { pub a: Option<&'static str>, pub b: Option<&'static str>, pub valid: bool }

impl DualSign {
    pub fn new() -> Self { DualSign { a: None, b: None, valid: false } }
    pub fn sign_a(&mut self, w: &'static str) { self.a = Some(w); }
    pub fn sign_b(&mut self, w: &'static str) -> Result<(), ClCode> {
        if let Some(x) = self.a { if x == w { return Err(ClCode::SamePerson); } }
        self.b = Some(w);
        self.valid = true;
        Ok(())
    }
}

pub fn try_close(list: &CloseList, sign: &DualSign) -> Result<(), ClCode> {
    if !list.complete() { return Err(ClCode::PieceMissing); }
    if !list.perf_met { return Err(ClCode::PerfNotMet); }
    if !sign.valid { return Err(ClCode::SamePerson); }
    if !list.a11y_confirmed { return Err(ClCode::A11yUntagged); }
    Ok(())
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves474_checks() -> CheckSet {
    let mut set = CheckSet::new("ves474");
    let mut list = CloseList::new();

    // C474-01 四件缺→拒签。
    list.put(Piece::SelfCheck20);
    list.put(Piece::JointReceipt);
    let mut sign = DualSign::new();
    sign.sign_a("wb-a");
    chk(&mut set, "C474-01", try_close(&list, &sign) == Err(ClCode::PieceMissing), "缺件拒签");

    // C474-02 四件齐但三线未达→硬性拒签。
    list.put(Piece::PerfProof);
    list.put(Piece::SecRecord);
    chk(&mut set, "C474-02", try_close(&list, &sign) == Err(ClCode::PerfNotMet), "三线硬拒");

    // C474-03 三线证明后缺 B 签→拒。
    list.perf_met = true;
    chk(&mut set, "C474-03", try_close(&list, &sign) == Err(ClCode::SamePerson), "缺 B 签拒");

    // C474-04 双签齐但 a11y 确认缺→拒（域本色）。
    sign.sign_b("wb-b").unwrap();
    chk(&mut set, "C474-04", try_close(&list, &sign) == Err(ClCode::A11yUntagged), "确认项缺拒");

    // C474-05 全条件→收口通过。
    list.a11y_confirmed = true;
    chk(&mut set, "C474-05", try_close(&list, &sign).is_ok(), "收口通过");

    // C474-06 同人双签拒。
    let mut bad = DualSign::new();
    bad.sign_a("wb-a");
    chk(&mut set, "C474-06", bad.sign_b("wb-a") == Err(ClCode::SamePerson) && !bad.valid, "同人作废");

    // C474-07 三线证明优先于双签。
    let l2 = CloseList { pieces: Piece::ALL.to_vec(), perf_met: false, a11y_confirmed: true };
    let s2 = DualSign { a: Some("x"), b: Some("y"), valid: true };
    chk(&mut set, "C474-07", try_close(&l2, &s2) == Err(ClCode::PerfNotMet), "三线先拒");

    // C474-08 空签拒。
    let l3 = CloseList { pieces: Piece::ALL.to_vec(), perf_met: true, a11y_confirmed: true };
    chk(&mut set, "C474-08", try_close(&l3, &DualSign::new()) == Err(ClCode::SamePerson), "空签拒");

    // C474-09 四件单源断言。
    chk(&mut set, "C474-09", Piece::ALL.len() == 4, "四件单源");

    // C474-10 件登记幂等。
    list.put(Piece::SecRecord);
    chk(&mut set, "C474-10", list.pieces.len() == 4, "件幂等");

    // C474-11 末闸 a11y。
    let l4 = CloseList { pieces: Piece::ALL.to_vec(), perf_met: true, a11y_confirmed: false };
    chk(&mut set, "C474-11", try_close(&l4, &s2) == Err(ClCode::A11yUntagged), "末闸确认");

    // C474-12 错误码段位单源。
    chk(&mut set, "C474-12", ClCode::PerfNotMet as u16 == SEG_BASE + 3, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves474_smoke() {
        let set = run_ves474_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves474 red: {:?}", red);
    }
}
