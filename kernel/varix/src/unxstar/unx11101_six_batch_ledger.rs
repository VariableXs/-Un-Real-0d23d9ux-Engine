//! UNX-F11101 I 型六批总账（B29–B35 段总对账）（0x1f581 段）
//!
//! 锚点：UNX-F11101-J1 六批总账逐批对平（10/10），段账守恒闭合（10/10）。
//! 段界：B29 为期初基线（已由 F11100 收口），本段六批为 B30–B35。
//! 域：CoRun·UNX-C4-B36（总账段）。
//!
//! 结构：段账册（SectionBook：B29–B35 六行逐批登记判据/行数/件数 → 三数
//! 申报 → 守恒闭合）+ 批行（BatchRow：逐批对平位）+ 冻结根 + 注入核
//! （缺批/越段批号/重号/三数不平四路必抓）。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1f581 独占 0x20）。
pub const SEG_BASE: u32 = 0x1f581;
pub const LB_NOBATCH: u32 = SEG_BASE + 0x01; // 缺批
pub const LB_DUP: u32 = SEG_BASE + 0x02; // 批号重号
pub const LB_RANGE: u32 = SEG_BASE + 0x03; // 批号越段
pub const LB_UNBAL: u32 = SEG_BASE + 0x04; // 三数不平
pub const LB_RED: u32 = SEG_BASE + 0x05; // 批未全绿
pub const LB_OVER: u32 = SEG_BASE + 0x06; // 超定容
pub const LB_FROZEN: u32 = SEG_BASE + 0x07; // 冻结后写账

/// 期初基线批（B29，已收口，不入本段六批）。
pub const BASE_BATCH: u32 = 29;
/// 段内批号区间（B30–B35，六批）。
pub const BATCH_FIRST: u32 = 30;
pub const BATCH_LAST: u32 = 35;
/// 段内批数（六批）。
pub const ROWS: usize = 6;
/// 总账定容。
pub const CAP: usize = 8;
/// 每批判据条数。
pub const CRIT_PER_BATCH: u32 = 20;
/// 每批行数底线。
pub const LINE_PER_BATCH: u32 = 260;
/// 段账标（fold 种子）。
pub const SEG_TAG: u64 = 0x1f5_8181;
/// 指纹步进。
pub const FP_STEP: u64 = 0x100000001b3;

/// 段内单批行：判据数 + 行数 + 件数 + 全绿位。
#[derive(Clone, Debug, PartialEq)]
pub struct BatchRow {
    pub no: u32,
    pub criteria: u32,
    pub lines: u32,
    pub files: u32,
    pub green: bool,
}

impl BatchRow {
    /// 逐批对平：全绿、判据足数、行数达标、件数非零。
    pub fn balanced(&self) -> bool {
        self.green
            && self.criteria == CRIT_PER_BATCH
            && self.lines >= LINE_PER_BATCH
            && self.files > 0
    }

    /// 行渲染：批号 | 判据 | 行数 | 件数。
    pub fn render(&self) -> String {
        format!("B{}|c{}|l{}|f{}", self.no, self.criteria, self.lines, self.files)
    }
}

/// 段账册：六行 + 三数申报 + 冻结根。
#[derive(Clone, Debug)]
pub struct SectionBook {
    pub rows: Vec<BatchRow>,
    pub declared_criteria: u32,
    pub declared_lines: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl SectionBook {
    pub fn new() -> SectionBook {
        SectionBook {
            rows: Vec::new(),
            declared_criteria: 0,
            declared_lines: 0,
            frozen: false,
            root: None,
        }
    }

    /// 批行登记：冻结/越段/重号/超容/坏行皆拒，登记后按批号定序。
    pub fn add(
        &mut self,
        no: u32,
        criteria: u32,
        lines: u32,
        files: u32,
        green: bool,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(LB_FROZEN);
        }
        if no < BATCH_FIRST || no > BATCH_LAST {
            return Err(LB_RANGE);
        }
        if self.rows.iter().any(|r| r.no == no) {
            return Err(LB_DUP);
        }
        if self.rows.len() >= CAP {
            return Err(LB_OVER);
        }
        if criteria != CRIT_PER_BATCH || lines < LINE_PER_BATCH || files == 0 {
            return Err(LB_UNBAL);
        }
        self.rows.push(BatchRow { no, criteria, lines, files, green });
        self.rows.sort_by_key(|r| r.no);
        Ok(())
    }

    pub fn sum_criteria(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.rows.len() {
            t = t.wrapping_add(self.rows[k].criteria);
            k += 1;
        }
        t
    }

    pub fn sum_lines(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.rows.len() {
            t = t.wrapping_add(self.rows[k].lines);
            k += 1;
        }
        t
    }

    pub fn sum_files(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.rows.len() {
            t = t.wrapping_add(self.rows[k].files);
            k += 1;
        }
        t
    }

    /// 红批数（未对平行）。
    pub fn reds(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.rows.len() {
            if !self.rows[k].balanced() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 缺批点名：段内未登记的批号。
    pub fn missing(&self) -> Vec<u32> {
        let mut m: Vec<u32> = Vec::new();
        let mut no = BATCH_FIRST;
        while no <= BATCH_LAST {
            if !self.rows.iter().any(|r| r.no == no) {
                m.push(no);
            }
            no += 1;
        }
        m
    }

    /// 三数申报：申报值必须与行内实算一致，否则不平。
    pub fn declare(&mut self, criteria: u32, lines: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(LB_FROZEN);
        }
        if criteria != self.sum_criteria() || lines != self.sum_lines() {
            return Err(LB_UNBAL);
        }
        self.declared_criteria = criteria;
        self.declared_lines = lines;
        Ok(())
    }

    /// 段账闭合：六批齐、零红、三数与申报对平、行数不低于段底线。
    pub fn closed(&self) -> bool {
        self.rows.len() == ROWS
            && self.reds() == 0
            && self.missing().is_empty()
            && self.sum_criteria() == ROWS as u32 * CRIT_PER_BATCH
            && self.declared_criteria == self.sum_criteria()
            && self.declared_lines == self.sum_lines()
            && self.sum_lines() >= ROWS as u32 * LINE_PER_BATCH
            && self.sum_files() > 0
    }

    /// 冻结：翻冻结位并落冻结根。
    pub fn seal(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 段账指纹：批号、三数、绿位与申报数链式入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = SEG_TAG;
        let mut k = 0usize;
        while k < self.rows.len() {
            let r = &self.rows[k];
            h = (h ^ r.no as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.criteria as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.lines as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.files as u64).wrapping_mul(FP_STEP);
            h = (h ^ r.green as u64).wrapping_mul(FP_STEP);
            k += 1;
        }
        if self.declared_criteria != 0 || self.declared_lines != 0 {
            h = (h ^ self.declared_criteria as u64).wrapping_mul(FP_STEP);
            h = (h ^ self.declared_lines as u64).wrapping_mul(FP_STEP);
        }
        h
    }
}

/// 标准段样本：B29–B35 六批逐批登记并申报三数。
pub fn build_section() -> (SectionBook, bool) {
    let mut b = SectionBook::new();
    let mut ok = true;
    let mut no = BATCH_FIRST;
    while no <= BATCH_LAST {
        let lines = LINE_PER_BATCH.wrapping_add((no % 5).wrapping_mul(10));
        let files = 3u32.wrapping_add(no % 4);
        if b.add(no, CRIT_PER_BATCH, lines, files, true).is_err() {
            ok = false;
        }
        no += 1;
    }
    let c = b.sum_criteria();
    let l = b.sum_lines();
    if b.declare(c, l).is_err() {
        ok = false;
    }
    (b, ok)
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 缺批必不闭合（只登 B29–B33）
    let mut a = SectionBook::new();
    let mut no = BATCH_FIRST;
    while no < BATCH_LAST {
        let _ = a.add(no, CRIT_PER_BATCH, LINE_PER_BATCH, 2, true);
        no += 1;
    }
    let _ = a.declare(a.sum_criteria(), a.sum_lines());
    got[0] = !a.closed() && a.missing().len() == 1 && a.missing().contains(&BATCH_LAST);
    // 2) 越段批号必拒
    let mut b = SectionBook::new();
    got[1] = b.add(BATCH_LAST + 1, CRIT_PER_BATCH, LINE_PER_BATCH, 2, true).err()
        == Some(LB_RANGE)
        && b.add(BATCH_FIRST - 1, CRIT_PER_BATCH, LINE_PER_BATCH, 2, true).err()
            == Some(LB_RANGE);
    // 3) 重号必拒
    let mut c = SectionBook::new();
    let _ = c.add(BATCH_FIRST, CRIT_PER_BATCH, LINE_PER_BATCH, 2, true);
    got[2] = c.add(BATCH_FIRST, CRIT_PER_BATCH, LINE_PER_BATCH, 3, true).err()
        == Some(LB_DUP);
    // 4) 三数不平必拒（申报判据数 +1）
    let (mut d, _) = build_section();
    let bad = d.sum_criteria().wrapping_add(1);
    got[3] = d.declare(bad, d.sum_lines()).err() == Some(LB_UNBAL)
        && d.declare(d.sum_criteria(), d.sum_lines()).is_ok();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx11101_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11101");

    let (mut book, built) = build_section();
    // J1 六批齐备
    s.add(
        "J1-六批齐备",
        built && book.rows.len() == ROWS && book.missing().is_empty(),
        "B30–B35 六批逐批在册",
    );

    // J2 逐批对平
    s.add("J2-逐批对平", book.reds() == 0, "六批判据足数、行数达标、件数非零");

    // J3 判据数和：六批各 20 条，合计 120
    s.add(
        "J3-判据和三数",
        book.sum_criteria() == 120 && book.declared_criteria == 120,
        "六批 20×6=120 条与申报对平",
    );

    // J4 段账守恒闭合
    s.add("J4-守恒闭合", book.closed(), "六批齐、零红、三数与申报对平");

    // J5 缺批必不闭合
    let j5 = {
        let mut c = SectionBook::new();
        let mut no = BATCH_FIRST;
        while no < BATCH_LAST {
            let _ = c.add(no, CRIT_PER_BATCH, LINE_PER_BATCH, 1, true);
            no += 1;
        }
        let _ = c.declare(c.sum_criteria(), c.sum_lines());
        !c.closed() && c.missing().len() == 1
    };
    s.add("J5-缺批不闭合", j5, "少一批即闭合为假且缺点名");

    // J6 注入检出：缺批 + 越段
    let inj = inject_four();
    s.add("J6-缺批越段检出", inj[0] && inj[1], "缺批闭合与越段批号两路必抓");

    // J7 注入检出：重号 + 三数不平
    s.add("J7-重号不平检出", inj[2] && inj[3], "批号重号与三数不平两路必抓");

    // J8 红批不算闭合
    let j8 = {
        let mut c = SectionBook::new();
        let mut no = BATCH_FIRST;
        while no <= BATCH_LAST {
            let _ = c.add(no, CRIT_PER_BATCH, LINE_PER_BATCH, 1, no != 32);
            no += 1;
        }
        let _ = c.declare(c.sum_criteria(), c.sum_lines());
        !c.closed() && c.reds() == 1
    };
    s.add("J8-红批不闭合", j8, "单批未全绿即段账不闭合");

    // J9 行数不达标拒登记
    let j9 = {
        let mut c = SectionBook::new();
        c.add(BATCH_FIRST, CRIT_PER_BATCH, LINE_PER_BATCH - 1, 1, true).err()
            == Some(LB_UNBAL)
    };
    s.add("J9-行数不足拒", j9, "行数低于批底线不得登记");

    // J10 冻结拒写
    let root = book.seal();
    let j10 = book.add(36, CRIT_PER_BATCH, LINE_PER_BATCH, 1, true).err()
        == Some(LB_FROZEN)
        && book.declare(1, 1).err() == Some(LB_FROZEN);
    s.add("J10-冻结拒写", j10, "封存后登记与申报两路必拒");

    // J11 冻结根稳定
    s.add(
        "J11-冻结根稳定",
        book.root == Some(root) && root == book.fold() && root != 0,
        "段账冻结根可重算零漂移",
    );

    // J12 定序：乱序登记后仍按批号定序
    let j12 = {
        let mut c = SectionBook::new();
        let _ = c.add(31, CRIT_PER_BATCH, LINE_PER_BATCH, 1, true);
        let _ = c.add(30, CRIT_PER_BATCH, LINE_PER_BATCH, 1, true);
        let _ = c.add(32, CRIT_PER_BATCH, LINE_PER_BATCH, 1, true);
        c.rows.len() == 3 && c.rows[0].no == 30 && c.rows[2].no == 32
    };
    s.add("J12-批号定序", j12, "乱序登记后行序按批号升序");

    // J13 渲染面：批号与三数可区分
    let j13 = {
        let (c, _) = build_section();
        c.rows[0].render() != c.rows[1].render()
            && c.rows[0].render().contains("B30")
            && c.rows[0].render().contains("c20")
    };
    s.add("J13-行渲染", j13, "批行渲染含批号与判据数");

    // J14 动态明细：三数与指纹
    s.add_dyn(
        "J14-段账明细",
        book.closed() && book.fold() != 0,
        &format!(
            "c={} l={} f={} fp={:#x}",
            book.sum_criteria(),
            book.sum_lines(),
            book.sum_files(),
            book.fold()
        ),
    );

    // J15 空册：零和且不闭合
    let j15 = {
        let e = SectionBook::new();
        e.rows.is_empty() && e.sum_lines() == 0 && !e.closed() && e.fold() == SEG_TAG
    };
    s.add("J15-空册零和", j15, "空册三数和为零且不闭合");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11101_six_batch_flow() {
        let (mut b, built) = build_section();
        assert!(built);
        assert!(b.closed());
        assert!(b.sum_criteria() == 120);
        assert!(b.seal() != 0);
        assert!(b.add(33, 20, 260, 1, true).err() == Some(LB_FROZEN));
        assert!(b.missing().is_empty());
    }

    #[test]
    fn unx11101_checks_green() {
        let s = run_unx11101_checks();
        assert!(s.all_passed(), "UNX-F11101 判据未全绿");
    }
}
