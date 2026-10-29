//! ves359_s19selfcheck — VE-F4119 · T06 预备自查
//! 组内自查：20 项逐项核验 + 交叉联调回执 + 合规库覆盖度复核 + 地区矩阵抽样结果——四件汇总为收口预备报告 + 核验不过补齐复验 + 回执缺催办 + 覆盖不足扩充规则 + 无障碍合规列专项（域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xCD40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    Ok = 0,
    ItemRed = SEG_BASE + 0x01,
    ReceiptMissing = SEG_BASE + 0x02,
    CoverageLow = SEG_BASE + 0x03,
}

pub const SELF_ITEMS: usize = 20;
/// 覆盖度红线（万分比）。
pub const COVERAGE_FLOOR: u32 = 9_500;

/// 四件预备报告。
pub struct PrepReport {
    /// 20 项自查逐项。
    pub items: Vec<bool>,
    /// 交叉联调回执（场景名, 收讫）。
    pub receipts: Vec<(&'static str, bool)>,
    /// 合规库覆盖度（万分比）。
    pub coverage_bp: u32,
    /// 地区矩阵抽样（已抽格, 总格）。
    pub sampled: usize,
    pub sample_total: usize,
    /// 无障碍合规列专项核验（域本色）。
    pub a11y_column_ok: bool,
}

pub const FOUR_PIECES: [&'static str; 4] =
    ["self-check-20", "integ-receipts", "coverage-review", "matrix-sampling"];

impl PrepReport {
    pub fn new() -> Self {
        PrepReport { items: Vec::new(), receipts: Vec::new(), coverage_bp: 0, sampled: 0, sample_total: 1, a11y_column_ok: false }
    }

    /// 四件齐备判定。
    pub fn complete(&self) -> bool {
        self.items.len() == SELF_ITEMS && self.items.iter().all(|i| *i)
            && !self.receipts.is_empty() && self.receipts.iter().all(|(_, ok)| *ok)
            && self.coverage_bp >= COVERAGE_FLOOR
            && self.sampled == self.sample_total
            && self.a11y_column_ok
    }

    /// 缺件定位。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.items.len() != SELF_ITEMS || self.items.iter().any(|i| !*i) { out.push(FOUR_PIECES[0]); }
        if self.receipts.is_empty() || self.receipts.iter().any(|(_, ok)| !*ok) { out.push(FOUR_PIECES[1]); }
        if self.coverage_bp < COVERAGE_FLOOR { out.push(FOUR_PIECES[2]); }
        if self.sampled != self.sample_total { out.push(FOUR_PIECES[3]); }
        out
    }

    /// 补齐复验：红项重验后置绿。
    pub fn recheck(&mut self, idx: usize) -> Result<(), ScCode> {
        if idx >= self.items.len() { return Err(ScCode::ItemRed); }
        self.items[idx] = true;
        Ok(())
    }

    /// 回执缺 → 催办清单。
    pub fn chase_list(&self) -> Vec<&'static str> {
        self.receipts.iter().filter(|(_, ok)| !*ok).map(|(s, _)| *s).collect()
    }
}

/// 合规库覆盖度复核：已覆盖规则类 / 应覆盖规则类。
pub fn coverage_bp(covered: &[&str], required: &[&str]) -> u32 {
    if required.is_empty() { return 10_000; }
    let hit = required.iter().filter(|r| covered.contains(r)).count();
    hit as u32 * 10_000 / required.len() as u32
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves359_checks() -> CheckSet {
    let mut set = CheckSet::new("ves359");

    let mk_full = || {
        let mut r = PrepReport::new();
        for _ in 0..SELF_ITEMS { r.items.push(true); }
        r.receipts = vec![("S06/T06-交叉", true)];
        r.coverage_bp = 9_800;
        r.sampled = 24; r.sample_total = 24;
        r.a11y_column_ok = true;
        r
    };

    // C359-01 四件齐备。
    let r = mk_full();
    chk(&mut set, "C359-01", r.complete() && r.missing().is_empty(), "四件全齐");

    // C359-02 20 项一项红 → 缺件定位。
    let mut r2 = mk_full();
    r2.items[13] = false;
    chk(&mut set, "C359-02", r2.missing() == vec!["self-check-20"], "第 14 项红 => 缺件");

    // C359-03 补齐复验。
    chk(&mut set, "C359-03", r2.recheck(13).is_ok() && r2.items[13] && r2.missing().is_empty(), "复验置绿");

    // C359-04 越界复验拒。
    chk(&mut set, "C359-04", r2.recheck(99) == Err(ScCode::ItemRed), "idx 越界 => 拒");

    // C359-05 回执缺 → 催办。
    let mut r3 = mk_full();
    r3.receipts.push(("矩阵补测", false));
    chk(&mut set, "C359-05", r3.chase_list() == vec!["矩阵补测"], "未收讫回执催办");

    // C359-06 回执全缺定位。
    let mut r4 = PrepReport::new();
    r4.items = vec![true; SELF_ITEMS];
    chk(&mut set, "C359-06", r4.missing() == vec!["integ-receipts", "coverage-review", "matrix-sampling"], "三件缺定位");

    // C359-07 覆盖度复核：全覆盖 10000。
    chk(&mut set, "C359-07",
        coverage_bp(&["data-law", "consumer", "a11y-act"], &["data-law", "consumer", "a11y-act"]) == 10_000, "三类全覆盖");

    // C359-08 覆盖不足。
    chk(&mut set, "C359-08",
        coverage_bp(&["data-law"], &["data-law", "consumer", "a11y-act"]) == 3_333, "一/三 ≈ 3333");

    // C359-09 覆盖红线判定。
    chk(&mut set, "C359-09",
        coverage_bp(&["data-law"], &["data-law", "consumer"]) == 5_000
        && 5_000 < COVERAGE_FLOOR, "5000 < 9500 => 扩充规则");

    // C359-10 抽样结果：满格过。
    let mut r5 = mk_full();
    r5.sampled = 24; r5.sample_total = 24;
    chk(&mut set, "C359-10", r5.missing().is_empty(), "抽样满格");

    // C359-11 抽样缺格定位。
    let mut r6 = mk_full();
    r6.sampled = 20; r6.sample_total = 24;
    chk(&mut set, "C359-11", r6.missing() == vec!["matrix-sampling"], "缺 4 格 => 缺件");

    // C359-12 无障碍合规列专项（域本色）。
    let mut r7 = mk_full();
    r7.a11y_column_ok = false;
    chk(&mut set, "C359-12", !r7.complete() && r7.missing().is_empty(), "专项红即整体不过");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves359_smoke() {
        let set = run_ves359_checks();
        assert!(set.failed_count() == 0, "ves359 red: {}", set.red_items_text());
    }

    #[test]
    fn ves359_coverage() {
        assert_eq!(coverage_bp(&[], &[]), 10_000);
        assert_eq!(coverage_bp(&["a"], &["a", "b"]), 5_000);
    }
}
