//! UNX-F10366 回归失绿即时归因（入 B32 面）（0x1b840 段）
//!
//! 锚点：UNX-F10366-J1 回归失绿即时归因——失绿用例逐条入册（名单唯
//! 一），逐条归因四类（代码缺陷/环境/数据/偶发），一例一因不得重归，
//! 全册归因后方可推 B32 面，且只推代码缺陷类（非代码缺陷不入 B32），
//! 推送一次性出数。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：失绿册（例名+归因+注）+ 归因核（一例一因）+ 类别核（四类之
//! 内）+ 推送核（全归因后只推代码缺陷）+ 注入核（重归/未归因先推/重
//! 例/推重四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b840 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b840;
pub const FA_ZERO: u32 = SEG_BASE + 0x01; // 例名空
pub const FA_DUP: u32 = SEG_BASE + 0x02; // 失绿重例
pub const FA_NOCASE: u32 = SEG_BASE + 0x03; // 例未在册
pub const FA_CAUSE: u32 = SEG_BASE + 0x04; // 归因越类
pub const FA_REATTR: u32 = SEG_BASE + 0x05; // 一例两归
pub const FA_UNATTR: u32 = SEG_BASE + 0x06; // 未全归因先推
pub const FA_REPUSH: u32 = SEG_BASE + 0x07; // 重复推送
pub const FA_SHORT: u32 = SEG_BASE + 0x08; // 失绿不足锚

/// 归因四类。
pub const CA_CODE: u8 = 1;
pub const CA_ENV: u8 = 2;
pub const CA_DATA: u8 = 3;
pub const CA_FLAKY: u8 = 4;
pub const CA_N: u8 = 5;

/// 失绿数锚。
pub const FAILS_NEEDED: usize = 4;
/// 册定容。
pub const CAP: usize = 12;

/// 失绿行：例名 + 归因 + 注。
#[derive(Clone)]
pub struct FailRow {
    pub rid: String,
    pub cause: Option<u8>,
    pub note: String,
}

/// 失绿归因册。
#[derive(Clone)]
pub struct FailAttr {
    pub rows: Vec<FailRow>,
    pub pushed: Vec<String>,
    pub sealed: bool,
}

impl FailAttr {
    /// 空册。
    pub fn new() -> Self {
        FailAttr {
            rows: Vec::new(),
            pushed: Vec::new(),
            sealed: false,
        }
    }

    /// 找例。
    pub fn find(&self, rid: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].rid == rid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 录失绿例。
    pub fn record_fail(&mut self, rid: &str, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(FA_REPUSH);
        }
        if rid.is_empty() {
            return Err(FA_ZERO);
        }
        if self.rows.len() >= CAP {
            return Err(FA_SHORT);
        }
        if self.find(rid).is_some() {
            return Err(FA_DUP);
        }
        self.rows.push(FailRow {
            rid: rid.to_string(),
            cause: None,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 归因（一例一因）。
    pub fn attr(&mut self, rid: &str, cause: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(FA_REPUSH);
        }
        let idx = match self.find(rid) {
            Some(i) => i,
            None => return Err(FA_NOCASE),
        };
        if cause < CA_CODE || cause >= CA_N {
            return Err(FA_CAUSE);
        }
        if self.rows[idx].cause.is_some() {
            return Err(FA_REATTR);
        }
        self.rows[idx].cause = Some(cause);
        Ok(())
    }

    /// 未归因数。
    pub fn unattr_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].cause.is_none() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 代码缺陷清单。
    pub fn code_defects(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].cause == Some(CA_CODE) {
                out.push(self.rows[i].rid.clone());
            }
            i += 1;
        }
        out
    }

    /// 某类归因数。
    pub fn cause_count(&self, cause: u8) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].cause == Some(cause) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 推 B32 面：全归因且达锚方可，只推代码缺陷类，一次为限。
    pub fn push_to_b32(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(FA_REPUSH);
        }
        if !self.pushed.is_empty() {
            return Err(FA_REPUSH);
        }
        if self.rows.len() < FAILS_NEEDED {
            return Err(FA_SHORT);
        }
        if self.unattr_count() > 0 {
            return Err(FA_UNATTR);
        }
        let list = self.code_defects();
        let mut i = 0;
        while i < list.len() {
            self.pushed.push(list[i].clone());
            i += 1;
        }
        Ok(list.len())
    }

    /// 归因齐备：全册归因且达锚。
    pub fn attr_ok(&self) -> bool {
        self.rows.len() >= FAILS_NEEDED && self.unattr_count() == 0
    }

    /// 封账：归因未齐即拒。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(FA_REPUSH);
        }
        if !self.attr_ok() {
            return Err(FA_UNATTR);
        }
        self.sealed = true;
        Ok(())
    }

    /// 折叠指纹：例名/归因/推送位叠算。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b8401b8;
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            h = (h ^ (r.cause.unwrap_or(0) as u32)).wrapping_mul(0x01000193);
            let bytes = r.rid.as_bytes();
            let mut j = 0;
            while j < bytes.len() {
                h = (h ^ (bytes[j] as u32)).wrapping_mul(0x01000193);
                j += 1;
            }
            i += 1;
        }
        h = (h ^ (self.pushed.len() as u32)).wrapping_mul(0x01000193);
        h
    }
}

/// 失绿样例：四例（例名, 归因, 注）。
pub fn fail_samples() -> [(&'static str, u8, &'static str); 4] {
    [
        ("reg-lat", CA_CODE, "慢在锁竞争"),
        ("reg-auth", CA_CODE, "越权分支漏判"),
        ("reg-mem", CA_ENV, "压测机内存扰"),
        ("reg-flaky", CA_FLAKY, "偶发时序抖动"),
    ]
}

/// 注入核：四路坏账必抓。
/// 0: 一例两归；1: 未全归因先推；2: 失绿重例；3: 归因越类。
pub fn inject_four() -> [bool; 4] {
    let mut b = FailAttr::new();
    let mut k = 0;
    while k < fail_samples().len() {
        let (rid, cause, note) = fail_samples()[k];
        let _ = b.record_fail(rid, note);
        let _ = b.attr(rid, cause);
        k += 1;
    }
    let d0 = b.attr("reg-lat", CA_ENV).err() == Some(FA_REATTR);
    let d1 = {
        let mut half = FailAttr::new();
        let mut m = 0;
        while m < fail_samples().len() {
            let (rid, _, note) = fail_samples()[m];
            let _ = half.record_fail(rid, note);
            if m < 2 {
                let c = fail_samples()[m].1;
                let _ = half.attr(rid, c);
            }
            m += 1;
        }
        half.push_to_b32().err() == Some(FA_UNATTR)
    };
    let d2 = b.record_fail("reg-lat", "重例").err() == Some(FA_DUP);
    let d3 = b.attr("reg-mem", 9).err() == Some(FA_CAUSE);
    [d0, d1, d2, d3]
}

/// 判据：run_unx10366_checks。
pub fn run_unx10366_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10366");
    // J1 正路径：四例失绿入册。
    let mut b = FailAttr::new();
    let mut j1 = true;
    let mut k = 0;
    while k < fail_samples().len() {
        let (rid, _, note) = fail_samples()[k];
        if b.record_fail(rid, note).is_err() {
            j1 = false;
        }
        k += 1;
    }
    s.add("J1-四例入册", j1 && b.rows.len() == 4, "失绿四例全录");
    // J2 边界：空例名拒。
    let mut b2 = FailAttr::new();
    let j2 = b2.record_fail("", "空").err() == Some(FA_ZERO);
    s.add("J2-空名拒", j2, "例名空即 FA_ZERO");
    // J3 注入：失绿重例必抓。
    let mut b3 = FailAttr::new();
    let _ = b3.record_fail("dup", "原例");
    let j3 = b3.record_fail("dup", "重例").err() == Some(FA_DUP);
    s.add("J3-重例抓", j3, "例名重复即 FA_DUP");
    // J4 注入：归因越类必抓。
    let mut b4 = FailAttr::new();
    let _ = b4.record_fail("x", "注");
    let j4 = b4.attr("x", 0).err() == Some(FA_CAUSE)
        && b4.attr("x", 5).err() == Some(FA_CAUSE);
    s.add("J4-越类抓", j4, "归因不在 1..=4 即 FA_CAUSE");
    // J5 注入：一例两归必抓。
    let mut b5 = FailAttr::new();
    let _ = b5.record_fail("twice", "注");
    let _ = b5.attr("twice", CA_CODE);
    let j5 = b5.attr("twice", CA_ENV).err() == Some(FA_REATTR);
    s.add("J5-两归抓", j5, "一例两归即 FA_REATTR");
    // J6 注入：未全归因先推必抓。
    let mut b6 = FailAttr::new();
    let mut k6 = 0;
    while k6 < fail_samples().len() {
        let (rid, _, note) = fail_samples()[k6];
        let _ = b6.record_fail(rid, note);
        if k6 < 3 {
            let c = fail_samples()[k6].1;
            let _ = b6.attr(rid, c);
        }
        k6 += 1;
    }
    let j6 = b6.push_to_b32().err() == Some(FA_UNATTR) && b6.unattr_count() == 1;
    s.add("J6-缺归抓", j6, "一例未归先推即 FA_UNATTR");
    // J7 边界：未在册归因拒。
    let mut b7 = FailAttr::new();
    let j7 = b7.attr("ghost", CA_CODE).err() == Some(FA_NOCASE);
    s.add("J7-无例拒", j7, "未在册归因即 FA_NOCASE");
    // J8 边界：失绿不足锚推拒。
    let mut b8 = FailAttr::new();
    let _ = b8.record_fail("solo", "注");
    let _ = b8.attr("solo", CA_CODE);
    let j8 = b8.push_to_b32().err() == Some(FA_SHORT);
    s.add("J8-短推拒", j8, "一例不足锚即 FA_SHORT");
    // J9 正路径：四例全归因。
    let mut ok9 = true;
    let mut k9 = 0;
    while k9 < fail_samples().len() {
        let (rid, cause, _) = fail_samples()[k9];
        if b.attr(rid, cause).is_err() {
            ok9 = false;
        }
        k9 += 1;
    }
    let j9 = ok9 && b.attr_ok() && b.unattr_count() == 0;
    s.add("J9-全归因", j9, "四例一例一因全归");
    // J10 正路径：推 B32 只推代码缺陷类。
    let n10 = b.push_to_b32();
    let j10 = n10 == Ok(2)
        && b.pushed == vec!["reg-lat".to_string(), "reg-auth".to_string()];
    s.add("J10-推B32", j10, "只推两例代码缺陷入 B32");
    // J11 注入：重复推送必抓。
    let j11 = b.push_to_b32().err() == Some(FA_REPUSH);
    s.add("J11-重推抓", j11, "二次推送即 FA_REPUSH");
    // J12 正路径：归因齐后封账。
    let mut b12 = b.clone();
    let j12 = b12.seal().is_ok() && b12.sealed;
    s.add("J12-封账成", j12, "归因齐封账");
    // J13 动态：归因分布出数与指纹。
    let cc = b.cause_count(CA_CODE);
    let ce = b.cause_count(CA_ENV);
    let cf = b.cause_count(CA_FLAKY);
    let j13 = cc == 2 && ce == 1 && cf == 1 && b.fold() != 0;
    s.add_dyn(
        "J13-归因出数",
        j13,
        &format!(
            "代码缺陷 {} 环境 {} 偶发 {}，推 B32 {} 笔，指纹 {:08x}",
            cc,
            ce,
            cf,
            b.pushed.len(),
            b.fold()
        ),
    );
    // J14 注入：inject_four 四路必抓。
    let inj = inject_four();
    let j14 = inj[0] && inj[1] && inj[2] && inj[3];
    s.add("J14-注入抓", j14, "两归/缺归/重例/越类四路全检出");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_and_push() {
        let mut b = FailAttr::new();
        let mut k = 0;
        while k < fail_samples().len() {
            let (rid, cause, note) = fail_samples()[k];
            assert!(b.record_fail(rid, note).is_ok());
            assert!(b.attr(rid, cause).is_ok());
            k += 1;
        }
        assert!(b.attr_ok());
        let n = b.push_to_b32();
        assert!(n == Ok(2));
        assert!(b.push_to_b32().err() == Some(FA_REPUSH));
        assert!(b.seal().is_ok());
        assert!(b.fold() != 0);
        assert_eq!(b.code_defects().len(), 2);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10366_checks();
        assert!(s.all_passed());
    }
}
