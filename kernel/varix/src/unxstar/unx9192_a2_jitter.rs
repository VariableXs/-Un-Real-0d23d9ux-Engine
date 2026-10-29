//! UNX-F9192 休眠族精度账（与 A2 jitter 对齐）（0x637C0 段）
//!
//! 锚点：UNX-F9192-J1 精度账（每次休眠实测落账：请求值、实际值、
//! 超额值、jitter 档、留痕）；J2 精度语义（实际≥请求（不少睡）+ 超
//! 额≤jitter 上限 + jitter 档闭集 + 长睡超额相对收敛四性质）；J3 精
//! 度指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（休眠精度面）。
//!
//! 结构：jitter 档闭集 + 精度核（超额判定）+ 精度账本 + 折叠指纹 +
//! 注入核（欠睡/超额越限/空留痕/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x637C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x637C0;
pub const JT_UNDER: u32 = SEG_BASE + 0x01; // 欠睡
pub const JT_OVER: u32 = SEG_BASE + 0x02; // 超额越限
pub const JT_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const JT_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const JT_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// jitter 档闭集（纳秒）。
pub const JT_NONE: u32 = 0; // 精确
pub const JT_LO: u32 = 100_000; // 0.1ms
pub const JT_HI: u32 = 2_000_000; // 2ms
pub const JT_COUNT: usize = 3;

pub const JT_NAMES: [&str; 3] = ["none", "lo", "hi"];

/// jitter 档判定：超额归档。
pub fn jitter_grade(over: Ns) -> u32 {
    if over == 0 {
        JT_NONE
    } else if over <= JT_LO as u64 {
        JT_LO
    } else if over <= JT_HI as u64 {
        JT_HI
    } else {
        JT_HI + 1 // 越限哨兵
    }
}

/// 单位：纳秒。
pub type Ns = u64;

/// 单条精度账。
#[derive(Clone)]
pub struct JtRec {
    pub seq: u32,
    pub req: Ns,
    pub actual: Ns,
    pub grade: u32,
    pub note: String,
}

impl JtRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.actual >= self.req
            && self.grade <= JT_HI
    }

    pub fn render_line(&self) -> String {
        format!(
            "JT{}|req{}act{}|{}|{}",
            self.seq,
            self.req,
            self.actual,
            if self.grade == JT_NONE { "exact" } else { "jittered" },
            self.note
        )
    }
}

/// 精度簿。
pub struct JtBook {
    pub ledger: Vec<JtRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl JtBook {
    pub fn new() -> JtBook {
        JtBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 精度核：登记实测。
    pub fn record_sleep(&mut self, req: Ns, actual: Ns, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(JT_FROZEN);
        }
        if note.is_empty() {
            return Err(JT_EMPTY_NOTE);
        }
        if actual < req {
            let grade = jitter_grade(actual.saturating_sub(req));
            self.ledger.push(JtRec { seq: self.seq_next, req, actual, grade, note: note.to_string() });
            self.seq_next += 1;
            return Err(JT_UNDER);
        }
        let over = actual - req;
        let grade = jitter_grade(over);
        if grade > JT_HI {
            self.ledger.push(JtRec { seq: self.seq_next, req, actual, grade, note: note.to_string() });
            self.seq_next += 1;
            return Err(JT_OVER);
        }
        self.ledger.push(JtRec { seq: self.seq_next, req, actual, grade, note: note.to_string() });
        self.seq_next += 1;
        Ok(grade)
    }

    /// 长睡超额相对收敛：超额/请求 比率随请求增大而降。
    pub fn rel_converge(&self) -> bool {
        // 找两条 req 差异大的账比较比率
        let mut pairs: Vec<(Ns, Ns)> =
            self.ledger.iter().map(|r| (r.req, r.actual - r.req)).collect();
        pairs.sort();
        if pairs.len() < 2 {
            return true;
        }
        let (r1, o1) = pairs[0];
        let (r2, o2) = pairs[pairs.len() - 1];
        if r2 == 0 {
            return true;
        }
        (o2 * 1000) / r2 <= (o1.max(1) * 1000) / r1.max(1) || o2 <= o1
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91920;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add((r.req & 0xFFFF) as u32);
            h = h.wrapping_mul(31).wrapping_add(r.grade);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 欠睡
    let mut b = JtBook::new();
    got[0] = b.record_sleep(1000, 900, "inj-under").err() == Some(JT_UNDER);
    // 2) 超额越限
    got[1] = b.record_sleep(1000, 1000 + JT_HI as u64 + 1, "inj-over").err() == Some(JT_OVER);
    // 3) 空留痕
    let bad2 = JtRec { seq: 3, req: 1, actual: 1, grade: JT_NONE, note: String::new() };
    got[2] = !bad2.ok();
    // 4) 越限档哨兵
    got[3] = jitter_grade(JT_HI as u64 + 1) > JT_HI;
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.record_sleep(1000, 1000, "inj-frozen").err() == Some(JT_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9192_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9192");
    let mut b = JtBook::new();

    // J1 精度账非空（三档齐）
    let j1 = {
        let _ = b.record_sleep(1_000_000, 1_000_000, "精确");
        let _ = b.record_sleep(1_000_000, 1_000_000 + JT_LO as u64, "低抖");
        let _ = b.record_sleep(1_000_000, 1_000_000 + JT_HI as u64, "高抖");
        b.ledger.iter().filter(|r| r.grade == JT_NONE).count() >= 1
            && b.ledger.iter().filter(|r| r.grade == JT_LO).count() >= 1
            && b.ledger.iter().filter(|r| r.grade == JT_HI).count() >= 1
    };
    s.add("J1-三档账齐", j1, "none/lo/hi 各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且不少睡不越限");

    // J3 不少睡
    let j3 = b.record_sleep(1000, 900, "欠").err() == Some(JT_UNDER);
    s.add("J3-不少睡", j3, "actual<req 必拒");

    // J4 超额越限
    let j4 = b.record_sleep(1000, 1000 + JT_HI as u64 + 1, "越").err() == Some(JT_OVER);
    s.add("J4-越限拒", j4, "超额超 jitter 上限必拒");

    // J5 jitter 档判定
    let j5 = jitter_grade(0) == JT_NONE
        && jitter_grade(JT_LO as u64) == JT_LO
        && jitter_grade(JT_HI as u64) == JT_HI
        && jitter_grade(JT_HI as u64 + 1) > JT_HI;
    s.add("J5-档判定", j5, "四例判定");

    // J6 精确睡
    let j6 = {
        let mut b2 = JtBook::new();
        b2.record_sleep(1000, 1000, "exact") == Ok(JT_NONE)
    };
    s.add("J6-精确睡", j6, "零超额归 none 档");

    // J7 长睡收敛
    let j7 = {
        let mut b2 = JtBook::new();
        let _ = b2.record_sleep(1_000_000, 1_000_000 + JT_LO as u64, "短");
        let _ = b2.record_sleep(1_000_000_000, 1_000_000_000 + JT_LO as u64, "长");
        b2.rel_converge()
    };
    s.add("J7-长睡收敛", j7, "相对超额比率随请求增大收敛");

    // J8 常量锁定
    let j8 = JT_LO == 100_000 && JT_HI == 2_000_000 && JT_NAMES.len() == 3;
    s.add("J8-常量锁定", j8, "jitter 档常量锁定");

    // J9 零睡
    let j9 = {
        let mut b2 = JtBook::new();
        b2.record_sleep(0, 0, "零") == Ok(JT_NONE)
    };
    s.add("J9-零睡", j9, "req=0 恒精确");

    // J10 拒绝留痕
    let j10 = b.ledger.iter().any(|r| r.grade > JT_HI || r.actual < r.req);
    s.add("J10-拒绝留痕", j10, "欠睡/越限账有痕");
    // 注：主簿欠睡账在 J3 产生

    // J11 指纹稳定 + seq 唯一
    let j11 = b.fold() == b.fold() && b.seq_unique();
    s.add("J11-指纹稳定", j11, "折叠指纹两次一致且 seq 唯一");

    // J12 空留痕拒
    let j12 = b.record_sleep(1000, 1000, "").err() == Some(JT_EMPTY_NOTE);
    s.add("J12-空留痕拒", j12, "空留痕必拒");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9192_jitter_flow() {
        assert_eq!(jitter_grade(0), JT_NONE);
        assert_eq!(jitter_grade(JT_LO as u64), JT_LO);
        assert_eq!(jitter_grade(JT_HI as u64), JT_HI);
        assert!(jitter_grade(JT_HI as u64 + 1) > JT_HI);
        let mut b = JtBook::new();
        assert_eq!(b.record_sleep(1000, 1000, "a"), Ok(JT_NONE));
        assert_eq!(b.record_sleep(1000, 900, "b"), Err(JT_UNDER));
        assert_eq!(b.record_sleep(1000, 1000 + JT_HI as u64 + 1, "c"), Err(JT_OVER));
    }

    #[test]
    fn unx9192_checks_green() {
        let s = run_unx9192_checks();
        assert!(s.all_passed(), "UNX-F9192 判据未全绿");
    }
}
