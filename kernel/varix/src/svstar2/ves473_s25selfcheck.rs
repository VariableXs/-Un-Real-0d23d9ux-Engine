//! ves473_s25selfcheck — VE-F4239 · U02 预备自查
//! U02 预备自查：20 项逐项核验+联调回执+性能三线达标+安全三道记录——四件汇总为收口预备报告；
//! 核验不过→补齐+复验；回执缺→催办；三线未达→阻断；
//! 含无障碍判据链核验（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1980;
pub const ITEM_N: usize = 20;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsCode {
    Ok = 0,
    ItemFail = SEG_BASE + 0x01,     // 0x1981
    ReceiptMissing = SEG_BASE + 0x02, // 0x1982
    PerfNotMet = SEG_BASE + 0x03,   // 0x1983
    A11yChainGap = SEG_BASE + 0x04, // 0x1984（域本色）
}

/// 四件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Piece { Verify20 = 0, JointReceipt = 1, PerfLines = 2, SecRecord = 3 }

impl Piece {
    pub const ALL: [Piece; 4] = [Piece::Verify20, Piece::JointReceipt, Piece::PerfLines, Piece::SecRecord];
}

/// 自查册。
pub struct SelfCheckBook {
    pub items_ok: u32,
    pub a11y_chain_ok: bool, // a11y 判据链核验（域本色）
    pub pieces: Vec<Piece>,
    pub perf_lines: (bool, bool, bool), // 三线达标
    pub sec_records: u32,               // 安全三道记录数
    pub urged: u32,
    pub reverify: u32,
}

impl SelfCheckBook {
    pub fn new() -> Self {
        SelfCheckBook { items_ok: 0, a11y_chain_ok: false, pieces: Vec::new(),
            perf_lines: (false, false, false), sec_records: 0, urged: 0, reverify: 0 }
    }
    pub fn put(&mut self, p: Piece) { if !self.pieces.contains(&p) { self.pieces.push(p); } }
    pub fn missing(&self) -> Vec<Piece> {
        Piece::ALL.iter().filter(|p| !self.pieces.contains(p)).map(|p| *p).collect()
    }
    pub fn urge(&mut self) -> bool {
        if self.missing().is_empty() { false } else { self.urged += 1; true }
    }
    pub fn verify_items(&mut self, ok: u32) -> Result<(), RsCode> {
        if ok < ITEM_N as u32 { self.reverify += 1; return Err(RsCode::ItemFail); }
        self.items_ok = ok;
        Ok(())
    }
    pub fn set_perf(&mut self, t: bool, b: bool, a: bool) { self.perf_lines = (t, b, a); }
    /// 预备报告：四件齐+20 项过+三线达标+安全三道+a11y 链核验（域本色）。
    pub fn prelim(&self) -> Result<(), RsCode> {
        if self.missing().len() > 0 { return Err(RsCode::ReceiptMissing); }
        if self.items_ok < ITEM_N as u32 { return Err(RsCode::ItemFail); }
        if !(self.perf_lines.0 && self.perf_lines.1 && self.perf_lines.2) { return Err(RsCode::PerfNotMet); }
        if self.sec_records < 3 { return Err(RsCode::ReceiptMissing); }
        if !self.a11y_chain_ok { return Err(RsCode::A11yChainGap); }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves473_checks() -> CheckSet {
    let mut set = CheckSet::new("ves473");
    let mut bk = SelfCheckBook::new();

    // C473-01 回执缺→催办。
    bk.put(Piece::Verify20);
    bk.put(Piece::JointReceipt);
    chk(&mut set, "C473-01", bk.urge() && bk.urged == 1, "回执缺催办");

    // C473-02 缺件定位。
    chk(&mut set, "C473-02", bk.missing() == vec![Piece::PerfLines, Piece::SecRecord], "缺件定位");

    // C473-03 四件齐后催办停。
    bk.put(Piece::PerfLines);
    bk.put(Piece::SecRecord);
    chk(&mut set, "C473-03", !bk.urge(), "催办停");

    // C473-04 20 项核验不过→复验。
    chk(&mut set, "C473-04", bk.verify_items(17) == Err(RsCode::ItemFail) && bk.reverify == 1, "核验复验");

    // C473-05 补齐全过。
    chk(&mut set, "C473-05", bk.verify_items(20).is_ok(), "复验全过");

    // C473-06 三线未达→阻断。
    bk.set_perf(true, false, true);
    chk(&mut set, "C473-06", bk.prelim() == Err(RsCode::PerfNotMet), "三线阻断");

    // C473-07 三线达标。
    bk.set_perf(true, true, true);
    chk(&mut set, "C473-07", bk.prelim() == Err(RsCode::ReceiptMissing), "安全道缺");

    // C473-08 安全三道记录补齐。
    bk.sec_records = 3;
    chk(&mut set, "C473-08", bk.prelim() == Err(RsCode::A11yChainGap), "a11y 链缺拒");

    // C473-09 a11y 链核验→预备报告过（域本色）。
    bk.a11y_chain_ok = true;
    chk(&mut set, "C473-09", bk.prelim().is_ok(), "预备报告过");

    // C473-10 四件单源断言。
    chk(&mut set, "C473-10", Piece::ALL.len() == 4
        && Piece::ALL[2] == Piece::PerfLines, "四件单源");

    // C473-11 复验累计。
    bk.verify_items(5).unwrap_err();
    chk(&mut set, "C473-11", bk.reverify == 2, "复验累计");

    // C473-12 错误码段位单源。
    chk(&mut set, "C473-12", RsCode::A11yChainGap as u16 == SEG_BASE + 4, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves473_smoke() {
        let set = run_ves473_checks();
        assert!(set.failed_count() == 0, "ves473 red: {}", set.red_items_text());
    }
}
