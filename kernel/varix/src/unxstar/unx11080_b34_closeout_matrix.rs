//! UNX-F11080 B34 批收口（五族回归矩阵总断言）（0x1ea00 段）
//!
//! B34 批把 19 条判据分到 5 个族（4/4/4/4/3）。每条判据登记“期望行数”，
//! 重放时回填“实测行数 + 绿位”；总断言要求 19 条全部实测等于期望且全绿。
//! 行数不变式：五族期望行数之和 = 全局行数账 = 五族实测行数之和，且族成员
//! 数之和必须回到 19。任何一条错值都会把批收口钉在非绿。
//!
//! 结构：判据行（族/期望/实测/绿位）+ 族累计（成员/行数/绿数）+ 批收口账
//! （19 条总断言、行数守恒、冻结根）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址：11080 号卡独占 0x20 步长窗口
const SEG_BASE: u32 = 0x1ea00;

/// 空账 —— 批内没有任何判据行。
pub const BC_EMPTY: u32 = SEG_BASE + 0x01;
/// 范围 —— cid 不在 1..=19、族号不在 0..5 或期望行数为零。
pub const BC_RANGE: u32 = SEG_BASE + 0x02;
/// 重号 —— cid 重复登记，或同一判据重复回填实测。
pub const BC_DUP: u32 = SEG_BASE + 0x03;
/// 错值 —— 实测行数不等于期望，或绿位为假。
pub const BC_MISMATCH: u32 = SEG_BASE + 0x04;
/// 行数不守恒 —— 族累计、全局账、实测行三者对不平。
pub const BC_SUM: u32 = SEG_BASE + 0x05;
/// 封账后写 —— 收口账冻结后仍登记或重放。
pub const BC_FROZEN: u32 = SEG_BASE + 0x06;

/// fold 域标（B34 收口账）。
pub const BC_TAG: u64 = 0x1ea_80;

/// B34 批判据总数。
pub const BC_ROWS: usize = 19;
/// B34 五族。
pub const BC_FAMS: usize = 5;

/// 一条批内判据行。
#[derive(Clone, Debug)]
pub struct CriterionRow {
    pub cid: u32,
    pub fam: u8,
    pub expect_rows: u32,
    pub actual_rows: u32,
    pub passed: bool,
}

impl CriterionRow {
    /// 已重放且数值/绿位同时成立。
    pub fn green(&self) -> bool {
        self.actual_rows != 0 && self.passed && self.actual_rows == self.expect_rows
    }

    /// 行差（实测减期望，带符号）。
    pub fn row_delta(&self) -> i32 {
        (self.actual_rows as i64 - self.expect_rows as i64) as i32
    }
}

/// 一个族的累计。
#[derive(Clone, Debug)]
pub struct FamilyTally {
    pub fam: u8,
    pub members: u8,
    pub rows: u32,
    pub green: u8,
}

/// B34 批收口账。
#[derive(Clone, Debug)]
pub struct B34Closeout {
    pub rows: Vec<CriterionRow>,
    pub fams: Vec<FamilyTally>,
    pub row_total: u32,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl B34Closeout {
    pub fn new() -> B34Closeout {
        let mut fams = Vec::new();
        let mut f = 0u8;
        while f < BC_FAMS as u8 {
            fams.push(FamilyTally { fam: f, members: 0, rows: 0, green: 0 });
            f = f.wrapping_add(1);
        }
        B34Closeout { rows: Vec::new(), fams, row_total: 0, frozen: false, root: None }
    }

    fn row_at(&self, cid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].cid == cid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记一条判据；cid 与族号都要在固定窗口内。
    pub fn enroll(&mut self, cid: u32, fam: u8, expect_rows: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(BC_FROZEN);
        }
        if cid == 0 || cid > BC_ROWS as u32 || fam >= BC_FAMS as u8 || expect_rows == 0 {
            return Err(BC_RANGE);
        }
        if self.row_at(cid).is_some() {
            return Err(BC_DUP);
        }
        if self.rows.len() >= BC_ROWS {
            return Err(BC_SUM);
        }
        self.rows.push(CriterionRow {
            cid,
            fam,
            expect_rows,
            actual_rows: 0,
            passed: false,
        });
        let fi = fam as usize;
        self.fams[fi].members = self.fams[fi].members.wrapping_add(1);
        self.fams[fi].rows = self.fams[fi].rows.wrapping_add(expect_rows);
        Ok(())
    }

    /// 回填一条重放结果；每个 cid 只能回填一次。
    pub fn replay(&mut self, cid: u32, actual_rows: u32, passed: bool) -> Result<(), u32> {
        if self.frozen {
            return Err(BC_FROZEN);
        }
        let ri = match self.row_at(cid) {
            Some(i) => i,
            None => return Err(BC_RANGE),
        };
        if actual_rows == 0 {
            return Err(BC_RANGE);
        }
        if self.rows[ri].actual_rows != 0 {
            return Err(BC_DUP);
        }
        let expect = self.rows[ri].expect_rows;
        let fam = self.rows[ri].fam as usize;
        self.rows[ri].actual_rows = actual_rows;
        self.rows[ri].passed = passed;
        self.row_total = self.row_total.wrapping_add(actual_rows);
        if actual_rows == expect && passed {
            self.fams[fam].green = self.fams[fam].green.wrapping_add(1);
            Ok(())
        } else {
            Err(BC_MISMATCH)
        }
    }

    /// 顺序重放全批（固定样本全绿）。
    pub fn replay_all(&mut self) -> Result<u32, u32> {
        let mut green = 0u32;
        let mut cid = 1u32;
        while cid <= BC_ROWS as u32 {
            let idx = match self.row_at(cid) {
                Some(i) => i,
                None => return Err(BC_EMPTY),
            };
            let expect = self.rows[idx].expect_rows;
            if self.replay(cid, expect, true).is_ok() {
                green = green.wrapping_add(1);
            }
            cid = cid.wrapping_add(1);
        }
        Ok(green)
    }

    /// 五族成员数之和。
    pub fn member_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.fams.len() {
            n = n.wrapping_add(self.fams[i].members as u32);
            i += 1;
        }
        n
    }

    /// 五族期望行数之和。
    pub fn expected_sum(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.fams.len() {
            n = n.wrapping_add(self.fams[i].rows);
            i += 1;
        }
        n
    }

    /// 某族实际回填行数。
    pub fn family_actual(&self, fam: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].fam == fam {
                n = n.wrapping_add(self.rows[i].actual_rows);
            }
            i += 1;
        }
        n
    }

    /// 全绿：19 行、逐行实测等于期望、绿位真。
    pub fn all_green(&self) -> bool {
        if self.rows.len() != BC_ROWS {
            return false;
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if !self.rows[i].green() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 行数守恒：族期望和、族实际和、全局行账三者相等。
    pub fn rows_conserved(&self) -> bool {
        if self.member_sum() != BC_ROWS as u32 {
            return false;
        }
        if self.expected_sum() != self.row_total {
            return false;
        }
        let mut f = 0u8;
        while f < BC_FAMS as u8 {
            if self.family_actual(f) != self.fams[f as usize].rows {
                return false;
            }
            f = f.wrapping_add(1);
        }
        true
    }

    /// 批收口：空、短、错值、行不平分别给出不同诊断。
    pub fn closeout(&self) -> Result<u64, u32> {
        if self.rows.is_empty() {
            return Err(BC_EMPTY);
        }
        if self.rows.len() != BC_ROWS {
            return Err(BC_SUM);
        }
        if !self.all_green() {
            return Err(BC_MISMATCH);
        }
        if !self.rows_conserved() {
            return Err(BC_SUM);
        }
        Ok(self.fold())
    }

    /// FNV-1a：逐行 cid/族/期望/实测/绿位与族累计全入链。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = BC_TAG;
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = &self.rows[i];
            h = (h ^ r.cid as u64).wrapping_mul(0x100000001b3);
            h = (h ^ r.fam as u64).wrapping_mul(0x100000001b3);
            h = (h ^ r.expect_rows as u64).wrapping_mul(0x100000001b3);
            h = (h ^ r.actual_rows as u64).wrapping_mul(0x100000001b3);
            h = (h ^ if r.passed { 1 } else { 0 }).wrapping_mul(0x100000001b3);
            i += 1;
        }
        let mut f = 0usize;
        while f < self.fams.len() {
            let t = &self.fams[f];
            h = (h ^ t.fam as u64).wrapping_mul(0x100000001b3);
            h = (h ^ t.members as u64).wrapping_mul(0x100000001b3);
            h = (h ^ t.rows as u64).wrapping_mul(0x100000001b3);
            h = (h ^ t.green as u64).wrapping_mul(0x100000001b3);
            f += 1;
        }
        h = (h ^ self.row_total as u64).wrapping_mul(0x100000001b3);
        h
    }

    /// 冻结收口账。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }

    /// 族行摘要。
    pub fn summary(&self) -> String {
        let label = "b34".to_string();
        let mut parts = String::new();
        let mut f = 0usize;
        while f < self.fams.len() {
            if f != 0 {
                parts.push(',');
            }
            parts.push_str(&format!("{}:{}", self.fams[f].fam, self.fams[f].rows));
            f += 1;
        }
        format!("{}|rows={}|total={}|fams={}", label, self.rows.len(), self.row_total, parts)
    }
}

/// 四路坏样本：cid 重号、族号越界、错值回填、冻结重放。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];

    let mut a = B34Closeout::new();
    let _ = a.enroll(1, 0, 1);
    got[0] = a.enroll(1, 1, 1).err() == Some(BC_DUP);

    let mut b = B34Closeout::new();
    got[1] = b.enroll(2, 5, 1).err() == Some(BC_RANGE);

    let mut c = B34Closeout::new();
    let _ = c.enroll(3, 2, 4);
    got[2] = c.replay(3, 7, true).err() == Some(BC_MISMATCH);

    let mut d = B34Closeout::new();
    let _ = d.enroll(4, 3, 1);
    let _ = d.replay(4, 1, true);
    let _ = d.freeze();
    got[3] = d.replay(4, 1, true).err() == Some(BC_FROZEN);
    got
}

/// B34 批 19 判据与行数守恒判据。
pub fn run_unx11080_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11080");

    let mut b = B34Closeout::new();
    let mut cid = 1u32;
    while cid <= BC_ROWS as u32 {
        let fam = ((cid - 1) % BC_FAMS as u32) as u8;
        let _ = b.enroll(cid, fam, 1);
        cid = cid.wrapping_add(1);
    }
    let _ = b.replay_all();

    // J1 两半：19 判据重放全绿，行数守恒且域累计对平。
    let j1a = b.all_green() && b.rows.len() == 19;
    let j1b = b.rows_conserved() && b.row_total == 19 && b.expected_sum() == 19;
    s.add("J1-十九判据全绿", j1a, "19 条 B34 判据逐条回填为绿");
    s.add("J1-行数域累计对平", j1b, "族期望、实测、全局行账三者相等");

    // J2 族分布 4/4/4/4/3。
    let dist_ok = b.fams[0].members == 4 && b.fams[1].members == 4
        && b.fams[2].members == 4 && b.fams[3].members == 4 && b.fams[4].members == 3;
    s.add("J2-五族分布", dist_ok && b.member_sum() == 19, "五族成员数严格为 4/4/4/4/3");

    // J3 四路注入。
    let inj = inject_four();
    s.add("J3-重号注入", inj[0], "cid 二次登记必拒");
    s.add("J3-越界注入", inj[1], "族号 5 越界必拒");
    s.add("J3-错值注入", inj[2], "实测行数错值必拒");
    s.add("J3-冻结注入", inj[3], "冻结后重放必拒");

    // J4 冻结：根稳定且登记/重放双拒。
    let mut f = b.clone();
    let root = f.freeze();
    let j4 = f.fold() == root && f.root == Some(root)
        && f.enroll(20, 0, 1).err() == Some(BC_FROZEN)
        && f.replay(1, 1, true).err() == Some(BC_FROZEN);
    s.add("J4-收口不可变", j4, "冻结根稳定且写路径关闭");

    // J5 指纹敏感：单条绿位翻转即改根。
    let mut c = b.clone();
    c.rows[7].passed = false;
    let j5 = c.fold() != b.fold();
    s.add("J5-指纹敏感", j5, "单条判据绿位翻转即改变批根");

    // J6 空账/短账/范围。
    let mut e = B34Closeout::new();
    let empty = e.closeout();
    let range = e.enroll(0, 0, 1);
    let _ = e.enroll(1, 0, 1);
    let short = e.closeout();
    let j6 = empty.err() == Some(BC_EMPTY)
        && range.err() == Some(BC_RANGE)
        && short.err() == Some(BC_SUM);
    s.add("J6-空短范围", j6, "空账、零号、十八条短账均拒");

    // J7 唯一有序：cid 1..19 严格递增且不重复。
    let mut ordered = true;
    let mut i = 0usize;
    while i < b.rows.len() {
        if b.rows[i].cid != i as u32 + 1 {
            ordered = false;
        }
        if i > 0 && b.rows[i].cid <= b.rows[i - 1].cid {
            ordered = false;
        }
        i += 1;
    }
    s.add("J7-唯一有序", ordered, "19 条 cid 严格递增 1..19");

    // J8 负语义：一条错值把全批钉为非绿。
    let mut bad = B34Closeout::new();
    let mut k = 1u32;
    while k <= 19 {
        let _ = bad.enroll(k, ((k - 1) % 5) as u8, 1);
        k = k.wrapping_add(1);
    }
    let mut x = 1u32;
    while x <= 19 {
        if x == 12 {
            let _ = bad.replay(x, 2, true);
        } else {
            let _ = bad.replay(x, 1, true);
        }
        x = x.wrapping_add(1);
    }
    let j8 = !bad.all_green() && !bad.rows_conserved() && bad.closeout().is_err()
        && bad.rows[11].row_delta() == 1;
    s.add("J8-错值非绿", j8, "单条行差即否定 19 判据全绿");

    // J9 动态族账。
    s.add_dyn("J9-族账动态", dist_ok,
        &format!("f0={} f1={} f2={} f3={} f4={}", b.fams[0].rows, b.fams[1].rows,
            b.fams[2].rows, b.fams[3].rows, b.fams[4].rows));

    // J10 空转期间局部状态不串账：新账独立从零开始。
    let mut fresh = B34Closeout::new();
    let j10 = fresh.fams.len() == 5 && fresh.row_total == 0 && fresh.member_sum() == 0
        && fresh.rows.is_empty();
    s.add("J10-新账清零", j10, "新收口账五族计数全部归零");

    // J11 收口根与摘要。
    let closed = b.closeout();
    let text = b.summary();
    let j11 = closed.ok() == Some(b.fold()) && text.starts_with("b34|") && b.fold() != 0;
    s.add_dyn("J11-收口摘要", j11, &text);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11080_flow() {
        let mut b = B34Closeout::new();
        assert!(b.closeout().err() == Some(BC_EMPTY));
        assert!(b.enroll(1, 0, 1).is_ok());
        assert!(b.enroll(1, 1, 1).err() == Some(BC_DUP));
        assert!(b.enroll(2, 5, 1).err() == Some(BC_RANGE));
        assert!(b.replay(1, 1, true).is_ok());
        assert!(b.replay(1, 1, true).err() == Some(BC_DUP));
        let fp = b.fold();
        let root = b.freeze();
        assert!(fp == root && b.fold() == root);
        assert!(b.replay(1, 1, true).err() == Some(BC_FROZEN));
        for hit in inject_four() {
            assert!(hit, "注入路径必须全部拒止");
        }
    }

    #[test]
    fn unx11080_checks_green() {
        assert!(run_unx11080_checks().all_passed(), "UNX-F11080 判据未全绿");
    }
}
