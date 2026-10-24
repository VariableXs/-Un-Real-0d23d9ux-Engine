//! ves360_s19close — VE-F4120 · T06 组收口双签
//! 20 项自查汇总 + 预备报告四件 + 交叉联调回执 + 合规库覆盖证明——齐备后双签；自查签与复核签分离禁同人；覆盖度未达红线硬性拒签；无障碍合规列确认项（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xCE40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    Ok = 0,
    MissingPiece = SEG_BASE + 0x01,
    SameSigner = SEG_BASE + 0x02,
    CoverageNotClean = SEG_BASE + 0x03,
}

/// 四件套。
pub const FOUR_PIECES: [&'static str; 4] =
    ["self-check-20", "prep-report", "integ-receipts", "coverage-proof"];

pub const SELF_TOTAL: usize = 20;
pub const COVERAGE_FLOOR_BP: u32 = 9_500;

/// 收口清单。
pub struct ClosureList {
    pub items: Vec<bool>,
    pub prep_report_ok: bool,
    /// 交叉联调回执（场景, 收讫）。
    pub receipts: Vec<(&'static str, bool)>,
    /// 合规库覆盖度（万分比）。
    pub coverage_bp: u32,
    /// 无障碍合规列确认项（域本色）。
    pub a11y_confirmed: bool,
}

impl ClosureList {
    pub fn new() -> Self {
        ClosureList { items: Vec::new(), prep_report_ok: false, receipts: Vec::new(), coverage_bp: 0, a11y_confirmed: false }
    }

    /// 四件齐备。
    pub fn pieces_complete(&self) -> bool {
        self.items.len() == SELF_TOTAL && self.items.iter().all(|i| *i)
            && self.prep_report_ok
            && !self.receipts.is_empty() && self.receipts.iter().all(|(_, ok)| *ok)
            && self.coverage_bp >= COVERAGE_FLOOR_BP
    }

    /// 缺件定位。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.items.len() != SELF_TOTAL || self.items.iter().any(|i| !*i) { out.push(FOUR_PIECES[0]); }
        if !self.prep_report_ok { out.push(FOUR_PIECES[1]); }
        if self.receipts.is_empty() || self.receipts.iter().any(|(_, ok)| !*ok) { out.push(FOUR_PIECES[2]); }
        if self.coverage_bp < COVERAGE_FLOOR_BP { out.push(FOUR_PIECES[3]); }
        out
    }

    /// 覆盖度硬门。
    pub fn coverage_clean(&self) -> bool { self.coverage_bp >= COVERAGE_FLOOR_BP }
}

/// 双签册。
#[derive(Clone)]
pub struct DualSign { pub first: &'static str, pub reviewer: &'static str, pub valid: bool }

pub struct SignBook { pub signs: Vec<DualSign> }

impl SignBook {
    pub fn new() -> Self { SignBook { signs: Vec::new() } }

    /// 双签：缺件/覆盖未达/同人 全拒；异人全齐才有效。
    pub fn sign(&mut self, l: &ClosureList, first: &'static str, reviewer: &'static str) -> Result<(), ClCode> {
        if !l.pieces_complete() { return Err(ClCode::MissingPiece); }
        if !l.coverage_clean() { return Err(ClCode::CoverageNotClean); }
        let same = first == reviewer;
        self.signs.push(DualSign { first, reviewer, valid: !same });
        if same { return Err(ClCode::SameSigner); }
        Ok(())
    }

    pub fn has_valid(&self) -> bool { self.signs.iter().any(|s| s.valid) }

    /// 作废重签。
    pub fn invalidate(&mut self) -> usize {
        let n = self.signs.iter().filter(|s| !s.valid).count();
        self.signs.retain(|s| s.valid);
        n
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves360_checks() -> CheckSet {
    let mut set = CheckSet::new("ves360");

    let mk_ok = || {
        let mut l = ClosureList::new();
        for _ in 0..SELF_TOTAL { l.items.push(true); }
        l.prep_report_ok = true;
        l.receipts = vec![("八场景交叉", true), ("矩阵补测", true)];
        l.coverage_bp = 9_800;
        l.a11y_confirmed = true;
        l
    };

    // C360-01 四件齐备可签。
    let l = mk_ok();
    chk(&mut set, "C360-01", l.pieces_complete() && l.missing().is_empty(), "四件齐备");

    // C360-02 缺件定位：回执未收讫。
    let mut l2 = mk_ok();
    l2.receipts[1].1 = false;
    chk(&mut set, "C360-02", l2.missing() == vec!["integ-receipts"], "回执红 => 缺件");

    // C360-03 覆盖未达红线 → 硬性拒签。
    let mut l3 = mk_ok();
    l3.coverage_bp = 9_000;
    let mut sb = SignBook::new();
    chk(&mut set, "C360-03", sb.sign(&l3, "ai-a", "ai-b") == Err(ClCode::CoverageNotClean), "9000 < 9500 => 硬拒");

    // C360-04 四件缺一拒签。
    let mut sb2 = SignBook::new();
    chk(&mut set, "C360-04", sb2.sign(&l2, "ai-a", "ai-b") == Err(ClCode::MissingPiece), "缺件 => 拒");

    // C360-05 合法双签。
    let mut sb3 = SignBook::new();
    chk(&mut set, "C360-05", sb3.sign(&l, "ai-wb", "ai-qc").is_ok() && sb3.has_valid(), "异人双签有效");

    // C360-06 同人签作废。
    let mut sb4 = SignBook::new();
    chk(&mut set, "C360-06", sb4.sign(&l, "ai-wb", "ai-wb") == Err(ClCode::SameSigner) && !sb4.has_valid(), "同人 => 作废");

    // C360-07 作废重签。
    let n = sb4.invalidate();
    chk(&mut set, "C360-07", n == 1 && sb4.sign(&l, "ai-wb", "ai-qc").is_ok(), "清空后重签");

    // C360-08 自查 20 项缺一即缺件。
    let mut l5 = mk_ok();
    l5.items[19] = false;
    chk(&mut set, "C360-08", l5.missing() == vec!["self-check-20"], "第 20 项红 => 缺件");

    // C360-09 覆盖度硬门单源。
    chk(&mut set, "C360-09", l.coverage_clean() && !l3.coverage_clean(), "硬门双态");

    // C360-10 四件名单单源。
    chk(&mut set, "C360-10", FOUR_PIECES.len() == 4 && FOUR_PIECES[1] == "prep-report", "四件单源");

    // C360-11 无障碍合规列确认项（域本色）。
    chk(&mut set, "C360-11", l.a11y_confirmed, "确认项在签收单");

    // C360-12 覆盖度阈值单源。
    chk(&mut set, "C360-12", COVERAGE_FLOOR_BP == 9_500 && SELF_TOTAL == 20, "阈值常量单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves360_smoke() {
        let set = run_ves360_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves360 red: {:?}", red);
    }

    #[test]
    fn ves360_gate() {
        let mut l = ClosureList::new();
        assert_eq!(l.missing(), vec!["self-check-20", "prep-report", "integ-receipts", "coverage-proof"]);
        let mut sb = SignBook::new();
        assert_eq!(sb.sign(&l, "a", "b"), Err(ClCode::MissingPiece));
    }
}
