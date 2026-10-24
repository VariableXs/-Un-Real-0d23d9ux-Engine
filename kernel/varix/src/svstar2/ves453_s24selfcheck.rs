//! ves453_s24selfcheck — VE-F4219 · U01 预备自查
//! U01 预备自查：20 项逐项核验+联调回执+API 冻结确认+文档齐备——四件汇总为收口预备报告；
//! 核验不过→补齐+复验；回执缺→催办；冻结未完成→阻断；
//! 自查含无障碍契约层核验（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x15A0;
pub const ITEM_N: usize = 20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsCode {
    Ok = 0,
    ItemFail = SEG_BASE + 0x01,   // 0x15A1
    ReceiptMissing = SEG_BASE + 0x02, // 0x15A2
    NotFrozen = SEG_BASE + 0x03,  // 0x15A3
}

/// 四件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Piece { Verify20 = 0, JointReceipt = 1, ApiFrozen = 2, DocsReady = 3 }

impl Piece {
    pub const ALL: [Piece; 4] = [Piece::Verify20, Piece::JointReceipt, Piece::ApiFrozen, Piece::DocsReady];
}

/// 自查册。
pub struct SelfCheckBook {
    pub items_ok: u32,        // 20 项核验通过数
    pub a11y_layer_ok: bool,  // 无障碍契约层核验（域本色）
    pub pieces: Vec<Piece>,
    pub urged: u32,
    pub reverify: u32,
}

impl SelfCheckBook {
    pub fn new() -> Self { SelfCheckBook { items_ok: 0, a11y_layer_ok: false, pieces: Vec::new(), urged: 0, reverify: 0 } }
    pub fn put_piece(&mut self, p: Piece) { if !self.pieces.contains(&p) { self.pieces.push(p); } }
    pub fn missing_pieces(&self) -> Vec<Piece> {
        Piece::ALL.iter().filter(|p| !self.pieces.contains(p)).map(|p| *p).collect()
    }
    pub fn urge(&mut self) -> bool {
        if self.missing_pieces().is_empty() { false } else { self.urged += 1; true }
    }
    /// 20 项核验：全过才算（不过→补齐+复验）。
    pub fn verify_items(&mut self, ok: u32) -> Result<(), RsCode> {
        if ok < ITEM_N as u32 {
            self.reverify += 1;
            return Err(RsCode::ItemFail);
        }
        self.items_ok = ok;
        Ok(())
    }
    /// 预备报告：四件齐+20 项全过+a11y 契约层核验（域本色）。
    pub fn prelim_report(&self) -> Result<(), RsCode> {
        if self.missing_pieces().len() > 0 { return Err(RsCode::ReceiptMissing); }
        if self.items_ok < ITEM_N as u32 { return Err(RsCode::ItemFail); }
        if !self.pieces.contains(&Piece::ApiFrozen) { return Err(RsCode::NotFrozen); }
        if !self.a11y_layer_ok { return Err(RsCode::ItemFail); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves453_checks() -> CheckSet {
    let mut set = CheckSet::new("ves453");
    let mut bk = SelfCheckBook::new();

    // C453-01 回执缺→催办。
    bk.put_piece(Piece::Verify20);
    bk.put_piece(Piece::JointReceipt);
    chk(&mut set, "C453-01", bk.urge() && bk.urged == 1, "回执缺催办");

    // C453-02 缺件定位（API 冻结/文档）。
    let miss = bk.missing_pieces();
    chk(&mut set, "C453-02", miss == vec![Piece::ApiFrozen, Piece::DocsReady], "缺件定位");

    // C453-03 回执齐后催办停。
    bk.put_piece(Piece::ApiFrozen);
    bk.put_piece(Piece::DocsReady);
    chk(&mut set, "C453-03", !bk.urge(), "催办停");

    // C453-04 20 项核验：18 项不过→复验。
    chk(&mut set, "C453-04", bk.verify_items(18) == Err(RsCode::ItemFail) && bk.reverify == 1, "核验不过复验");

    // C453-05 补齐复验 20/20 过。
    chk(&mut set, "C453-05", bk.verify_items(20).is_ok() && bk.items_ok == 20, "复验全过");

    // C453-06 预备报告：a11y 契约层未核→拒（域本色）。
    chk(&mut set, "C453-06", bk.prelim_report() == Err(RsCode::ItemFail), "契约层缺拒");

    // C453-07 a11y 契约层核验后→预备报告过。
    bk.a11y_layer_ok = true;
    chk(&mut set, "C453-07", bk.prelim_report().is_ok(), "预备报告过");

    // C453-08 冻结件缺→阻断码。
    let mut bk2 = SelfCheckBook::new();
    for p in [Piece::Verify20, Piece::JointReceipt, Piece::DocsReady] { bk2.put_piece(p); }
    bk2.items_ok = 20; bk2.a11y_layer_ok = true;
    chk(&mut set, "C453-08", bk2.prelim_report() == Err(RsCode::ReceiptMissing), "缺冻结件拒");

    // C453-09 冻结件齐→过。
    bk2.put_piece(Piece::ApiFrozen);
    chk(&mut set, "C453-09", bk2.prelim_report().is_ok(), "四件齐过");

    // C453-10 四件单源断言。
    chk(&mut set, "C453-10", Piece::ALL.len() == 4
        && Piece::ALL[2] == Piece::ApiFrozen, "四件单源");

    // C453-11 复验计数累计。
    bk.verify_items(10).unwrap_err();
    chk(&mut set, "C453-11", bk.reverify == 2, "复验累计");

    // C453-12 催办幂等（再催）。
    let mut bk3 = SelfCheckBook::new();
    bk3.urge(); bk3.urge();
    chk(&mut set, "C453-12", bk3.urged == 2, "催办幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves453_smoke() {
        let set = run_ves453_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves453 red: {:?}", red);
    }
}
