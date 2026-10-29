//! UNX-F12027 STATUS_INVALID_PARAMETER 参数校验错误族（0x19580 段）
//!
//! 判据原文：参数校验规则册——每条规则钉定一个校验面（对齐/范围/
//! 非空/闭集），违规即 STATUS_INVALID_PARAMETER（0xC000000D）并按规则
//! 号留账；规则册闭集登记（未知规则号即拒）；校验逐笔可对账。
//!
//! 结构：规则闭集（ParamRule 枚举 × 规则说明）+ 校验核（五码守卫）+
//! 校验账（PvRec 按规则号留痕）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12027_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19580 独占 0x20）。
pub const SEG_BASE: u32 = 0x19580;
pub const PV_UNKNOWN_RULE: u32 = SEG_BASE + 0x01; // 未知规则号
pub const PV_VIOLATION: u32 = SEG_BASE + 0x02; // 规则违规
pub const PV_BAD_REC: u32 = SEG_BASE + 0x03; // 伪造校验账
pub const PV_RULE_FROZEN: u32 = SEG_BASE + 0x04; // 规则册冻结期违规直入
pub const PV_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍校验

/// STATUS_INVALID_PARAMETER 语义钉值。
pub const STATUS_INVALID_PARAMETER: u32 = 0xC000_000D;

/// 参数校验规则闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParamRule {
    /// 指针 8 字节对齐。
    Align8 = 1,
    /// 值域 [1, 0x7FFFFFFF]。
    RangePos = 2,
    /// 缓冲长度非零。
    NonEmpty = 3,
    /// 枚举值在闭集内（1..=4）。
    EnumClosed = 4,
    /// 长度偶数字节（UTF-16 面）。
    EvenLen = 5,
}

/// 规则总数。
pub const RULE_COUNT: usize = 5;

/// 规则说明表。
pub fn rule_desc(r: ParamRule) -> &'static str {
    match r {
        ParamRule::Align8 => "指针 8 字节对齐",
        ParamRule::RangePos => "值域 [1,0x7FFFFFFF]",
        ParamRule::NonEmpty => "缓冲长度非零",
        ParamRule::EnumClosed => "枚举闭集 1..=4",
        ParamRule::EvenLen => "偶数字节长",
    }
}

/// 单条规则的判定（参数面 u64 语义值）。
pub fn rule_check(r: ParamRule, v: u64) -> bool {
    match r {
        ParamRule::Align8 => v != 0 && v & 0x7 == 0,
        ParamRule::RangePos => v >= 1 && v <= 0x7FFF_FFFF,
        ParamRule::NonEmpty => v > 0,
        ParamRule::EnumClosed => v >= 1 && v <= 4,
        ParamRule::EvenLen => v % 2 == 0,
    }
}

/// 一笔校验的账目。
#[derive(Clone)]
pub struct PvRec {
    pub seq: u32,
    pub rule: ParamRule,
    pub val: u64,
    pub passed: bool,
    pub note: String,
}

impl PvRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0
    }
    /// 账目自洽：判定结果与规则语义一致。
    pub fn verdict_ok(&self) -> bool {
        rule_check(self.rule, self.val) == self.passed
    }
}

/// 校验器模型。
pub struct ParamValidator {
    pub ledger: Vec<PvRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ParamValidator {
    pub fn new() -> ParamValidator {
        ParamValidator { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 校验核：规则号合法 → 判定 → 留账。
    pub fn check(&mut self, rule_no: u16, val: u64, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PV_FROZEN);
        }
        if note.is_empty() {
            return Err(PV_UNKNOWN_RULE);
        }
        let rule = match rule_no {
            1 => ParamRule::Align8,
            2 => ParamRule::RangePos,
            3 => ParamRule::NonEmpty,
            4 => ParamRule::EnumClosed,
            5 => ParamRule::EvenLen,
            _ => return Err(PV_UNKNOWN_RULE),
        };
        let passed = rule_check(rule, val);
        let rec = PvRec { seq: self.seq_next, rule, val, passed, note: note.to_string() };
        if !rec.ok() {
            return Err(PV_BAD_REC);
        }
        self.seq_next += 1;
        self.ledger.push(rec);
        if passed {
            Ok(())
        } else {
            Err(PV_VIOLATION)
        }
    }

    /// 规则清点。
    pub fn count_rule(&self, r: ParamRule) -> usize {
        self.ledger.iter().filter(|x| x.rule == r).count()
    }

    /// 违规清点。
    pub fn violations(&self) -> usize {
        self.ledger.iter().filter(|x| !x.passed).count()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|x| x.ok() && x.verdict_ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for r in &self.ledger {
            h = h.wrapping_mul(211).wrapping_add(r.seq);
            h = h.wrapping_mul(211).wrapping_add(r.rule as u32);
        }
        h
    }
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut v = ParamValidator::new();
    // 1) 未知规则号（0 与 6）→ PV_UNKNOWN_RULE。
    got[0] = v.check(0, 1, "r0").err() == Some(PV_UNKNOWN_RULE)
        && v.check(6, 1, "r6").err() == Some(PV_UNKNOWN_RULE);
    // 2) 对齐违规 → PV_VIOLATION。
    got[1] = v.check(1, 0x123457, "align").err() == Some(PV_VIOLATION);
    // 3) 枚举闭集违规 → PV_VIOLATION。
    got[2] = v.check(4, 5, "enum").err() == Some(PV_VIOLATION);
    // 4) 空留痕 → PV_UNKNOWN_RULE（无痕校验守卫）。
    got[3] = v.check(2, 5, "").err() == Some(PV_UNKNOWN_RULE);
    // 5) 印账后校验 → PV_FROZEN。
    v.seal();
    got[4] = v.check(2, 5, "post").err() == Some(PV_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12027_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12027");

    // J1 五规则语义逐条核对正样本。
    let j1 = rule_check(ParamRule::Align8, 0x1000)
        && rule_check(ParamRule::RangePos, 1)
        && rule_check(ParamRule::NonEmpty, 8)
        && rule_check(ParamRule::EnumClosed, 4)
        && rule_check(ParamRule::EvenLen, 10);
    s.add("J1-五规则正样本", j1, "全规则过面");

    // J2 五规则负样本各一。
    let j2 = !rule_check(ParamRule::Align8, 0x123457)
        && !rule_check(ParamRule::RangePos, 0)
        && !rule_check(ParamRule::NonEmpty, 0)
        && !rule_check(ParamRule::EnumClosed, 5)
        && !rule_check(ParamRule::EvenLen, 7);
    s.add("J2-五规则负样本", j2, "全规则拒面");

    // J3 规则闭集与说明表对齐。
    let j3 = RULE_COUNT == 5
        && !rule_desc(ParamRule::Align8).is_empty()
        && !rule_desc(ParamRule::EvenLen).is_empty();
    s.add("J3-规则册闭集", j3, "5 规则全有说明");

    // J4 校验留痕：通过账 verdict 对账。
    let j4 = {
        let mut v = ParamValidator::new();
        let _ = v.check(1, 0x2000, "ok");
        v.ledger[0].passed && v.ledger[0].verdict_ok() && v.ledger_ok()
    };
    s.add("J4-通过留痕", j4, "verdict 自洽");

    // J5 违规留痕。
    let j5 = {
        let mut v = ParamValidator::new();
        let _ = v.check(5, 7, "bad");
        !v.ledger[0].passed && v.ledger[0].verdict_ok() && v.violations() == 1
    };
    s.add("J5-违规留痕", j5, "拒面入账");

    // J6 未知规则号拒。
    let j6 = {
        let mut v = ParamValidator::new();
        v.check(9, 1, "x").err() == Some(PV_UNKNOWN_RULE)
    };
    s.add("J6-未知规则拒", j6, "闭集外必拒");

    // J7 规则清点对平。
    let j7 = {
        let mut v = ParamValidator::new();
        let _ = v.check(1, 8, "a");
        let _ = v.check(1, 9, "b");
        let _ = v.check(2, 3, "c");
        v.count_rule(ParamRule::Align8) == 2 && v.count_rule(ParamRule::RangePos) == 1
    };
    s.add("J7-规则清点", j7, "2+1 对账");

    // J8 空留痕拒。
    let j8 = {
        let mut v = ParamValidator::new();
        v.check(2, 5, "").err() == Some(PV_UNKNOWN_RULE)
    };
    s.add("J8-空留痕拒", j8, "留痕必填");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = ParamValidator::new();
        let _ = a.check(1, 8, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut b = ParamValidator::new();
        let _ = b.check(2, 8, "x");
        f1 == f2 && a.fold() != b.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异账异纹");

    // J10 印账冻结。
    let j10 = {
        let mut v = ParamValidator::new();
        let _ = v.check(1, 8, "a");
        v.seal();
        v.check(1, 8, "post").err() == Some(PV_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后校验必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：混合校验面。
    let mut v = ParamValidator::new();
    let _ = v.check(1, 0x7FF6_0000, "sum-peb");
    let _ = v.check(1, 0x7FF6_0001, "sum-bad");
    let _ = v.check(3, 0, "sum-empty");
    s.add("样例两违一过", v.violations() == 2, "对齐+空长双违规");
    s.add("样例账面合法", v.ledger_ok(), "逐笔过自洽");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12027_rule_flow() {
        let mut v = ParamValidator::new();
        assert!(v.check(1, 0x1000, "a").is_ok());
        assert_eq!(v.check(1, 0x1001, "b"), Err(PV_VIOLATION));
        assert_eq!(v.check(6, 1, "c"), Err(PV_UNKNOWN_RULE));
        assert_eq!(v.violations(), 1);
    }

    #[test]
    fn unx12027_checks_green() {
        let s = run_unx12027_checks();
        assert!(s.all_passed(), "UNX-F12027 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12027_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
