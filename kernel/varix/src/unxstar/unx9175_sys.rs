//! UNX-F9175 资源族参数探针总成（0x635A0 段）
//!
//! 锚点：UNX-F9175-J1 探针账（getrusage/setrlimit 参数面 op × 原始
//! 参数 × 展开参数 × 规则四元组逐条落账）；J2 探针语义（who 闭集 +
//! 资源号闭集 + 零 pid 自指 + 无穷直传四规则闭集）；J3 探针指纹（
//! 账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（资源族参数展开面）。
//!
//! 结构：规则闭集 + 展开核 + 探针账本 + 折叠指纹 + 注入核（错规则/
//! 空留痕/伪 op/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x635A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x635A0;
pub const RP_UNK_OP: u32 = SEG_BASE + 0x01; // 未知 op
pub const RP_BAD_RULE: u32 = SEG_BASE + 0x02; // 规则判定错
pub const RP_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const RP_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const RP_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EINVAL: u32 = 1;
pub const R_COUNT: usize = 2;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EINVAL"];

/// 操作闭集。
pub const OP_RUSAGE_WHO: usize = 0; // getrusage 的 who 位（0=SELF -1=CHILDREN）
pub const OP_SETRLIMIT_RES: usize = 1; // setrlimit 的资源号位
pub const OP_PRLIMIT_PID: usize = 2; // prlimit64 的 pid 位（0=自身）
pub const OP_COUNT: usize = 3;

pub const OP_NAMES: [&str; OP_COUNT] = ["rusage.who", "setrlimit.res", "prlimit.pid"];

/// 规则闭集。
pub const Q_WHO_SELF: usize = 0; // 0 → SELF
pub const Q_WHO_CHILD: usize = 1; // -1 → CHILDREN
pub const Q_RES_RANGE: usize = 2; // 资源号 <16 直传
pub const Q_PID_SELF: usize = 3; // 0 → caller
pub const Q_NEG_REJ: usize = 4; // 负域拒
pub const Q_COUNT: usize = 5;

pub const RULE_NAMES: [&str; Q_COUNT] =
    ["who-self", "who-child", "res-range", "pid-self", "neg-rej"];

/// 负域哨兵。
pub const NEG_SENTINEL: u32 = 0x8000_0000;

/// 单条探针账。
#[derive(Clone)]
pub struct RpProbe {
    pub seq: u32,
    pub op: usize,
    pub raw: u32,
    pub expanded: u32,
    pub rule: usize,
    pub code: u32,
    pub note: String,
}

impl RpProbe {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.op < OP_COUNT && self.rule < Q_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "RP{}|{}|raw{:x}->exp{:x}|{}|{}|{}",
            self.seq,
            OP_NAMES[self.op],
            self.raw,
            self.expanded,
            RULE_NAMES[self.rule],
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// 探针簿。
pub struct RpBook {
    pub ledger: Vec<RpProbe>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl RpBook {
    pub fn new() -> RpBook {
        RpBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn expand(
        &mut self,
        op: usize,
        raw: u32,
        caller: u32,
        note: &str,
    ) -> Result<(u32, usize), u32> {
        if self.sealed {
            return Err(RP_FROZEN);
        }
        if note.is_empty() {
            return Err(RP_EMPTY_NOTE);
        }
        if op >= OP_COUNT {
            return Err(RP_UNK_OP);
        }
        let is_neg = raw >= NEG_SENTINEL;
        let (expanded, rule, code): (u32, usize, u32) = match op {
            OP_RUSAGE_WHO => {
                if raw == 0 {
                    (0, Q_WHO_SELF, R_OK)
                } else if raw == u32::MAX {
                    // -1 → CHILDREN
                    (1, Q_WHO_CHILD, R_OK)
                } else {
                    (0, Q_WHO_SELF, R_EINVAL)
                }
            }
            OP_SETRLIMIT_RES => {
                if is_neg {
                    (0, Q_NEG_REJ, R_EINVAL)
                } else if raw < 16 {
                    (raw, Q_RES_RANGE, R_OK)
                } else {
                    (0, Q_RES_RANGE, R_EINVAL)
                }
            }
            OP_PRLIMIT_PID => {
                if is_neg {
                    (0, Q_NEG_REJ, R_EINVAL)
                } else if raw == 0 {
                    (caller, Q_PID_SELF, R_OK)
                } else {
                    (raw, Q_PID_SELF, R_OK) // 非 0 直传
                }
            }
            _ => return Err(RP_UNK_OP),
        };
        self.record(op, raw, expanded, rule, code, note);
        if code == R_EINVAL {
            return Err(R_EINVAL);
        }
        Ok((expanded, rule))
    }

    fn record(&mut self, op: usize, raw: u32, expanded: u32, rule: usize, code: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(RpProbe {
            seq,
            op,
            raw,
            expanded,
            rule,
            code,
            note: note.to_string(),
        });
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
        let mut h: u32 = 0x91750;
        for p in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(p.seq);
            h = h.wrapping_mul(31).wrapping_add(p.op as u32);
            h = h.wrapping_mul(31).wrapping_add(p.raw);
            h = h.wrapping_mul(31).wrapping_add(p.expanded);
            h = h.wrapping_mul(31).wrapping_add(p.rule as u32);
            for b in p.note.bytes() {
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
    // 1) 错规则账
    let bad = RpProbe {
        seq: 1,
        op: 0,
        raw: 0,
        expanded: 0,
        rule: 99,
        code: 0,
        note: "inj-rule".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = RpProbe {
        seq: 2,
        op: 0,
        raw: 0,
        expanded: 0,
        rule: 0,
        code: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪 op
    let mut b = RpBook::new();
    got[2] = b.expand(99, 0, 1, "inj-op").err() == Some(RP_UNK_OP);
    // 4) who 野值拒
    got[3] = b.expand(OP_RUSAGE_WHO, 7, 1, "inj-who").err() == Some(R_EINVAL);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.expand(OP_RUSAGE_WHO, 0, 1, "inj-frozen").err() == Some(RP_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9175_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9175");
    let mut b = RpBook::new();
    const CALLER: u32 = 77;

    // J1 探针账非空
    let _ = b.expand(OP_RUSAGE_WHO, 0, CALLER, "who 零自指");
    let j1 = !b.ledger.is_empty();
    s.add("J1-探针账非空", j1, "至少一条探针在册");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|p| p.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 op/rule 闭集");

    // J3 who 0 → SELF
    let j3 = b.expand(OP_RUSAGE_WHO, 0, CALLER, "SELF") == Ok((0, Q_WHO_SELF));
    s.add("J3-who自指", j3, "who=0 展开 SELF");

    // J4 who -1 → CHILDREN
    let j4 = b.expand(OP_RUSAGE_WHO, u32::MAX, CALLER, "CHILD") == Ok((1, Q_WHO_CHILD));
    s.add("J4-who负一", j4, "who=-1 展开 CHILDREN");

    // J5 who 野值拒
    let j5 = b.expand(OP_RUSAGE_WHO, 7, CALLER, "野").err() == Some(R_EINVAL);
    s.add("J5-who野值拒", j5, "who 出闭集必 EINVAL");

    // J6 资源号闭集：<16 直传，≥16 拒
    let j6 = {
        let a = b.expand(OP_SETRLIMIT_RES, 7, CALLER, "在域") == Ok((7, Q_RES_RANGE));
        let bb = b.expand(OP_SETRLIMIT_RES, 99, CALLER, "越域").err() == Some(R_EINVAL);
        a && bb
    };
    s.add("J6-资源号闭集", j6, "资源号 [0,16) 放行");

    // J7 资源号负域拒
    let j7 = b.expand(OP_SETRLIMIT_RES, 0xFFFF_0000, CALLER, "负").err() == Some(R_EINVAL);
    s.add("J7-负域拒", j7, "负域参数必 EINVAL");

    // J8 pid 零自指
    let j8 = b.expand(OP_PRLIMIT_PID, 0, CALLER, "自指") == Ok((CALLER, Q_PID_SELF));
    s.add("J8-pid自指", j8, "pid=0 展开 caller");

    // J9 pid 直传
    let j9 = b.expand(OP_PRLIMIT_PID, 42, CALLER, "直传") == Ok((42, Q_PID_SELF));
    s.add("J9-pid直传", j9, "非零 pid 直传");

    // J10 伪 op 拒
    let j10 = b.expand(99, 0, CALLER, "伪").err() == Some(RP_UNK_OP);
    s.add("J10-伪op拒", j10, "op 出闭集必拒");

    // J11 拒绝亦留痕
    let j11 = b.ledger.iter().any(|p| p.code == R_EINVAL);
    s.add("J11-拒绝留痕", j11, "EINVAL 探针留账");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

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
    fn unx9175_expand_rules() {
        let mut b = RpBook::new();
        assert_eq!(b.expand(OP_RUSAGE_WHO, 0, 77, "a"), Ok((0, Q_WHO_SELF)));
        assert_eq!(b.expand(OP_RUSAGE_WHO, u32::MAX, 77, "b"), Ok((1, Q_WHO_CHILD)));
        assert_eq!(b.expand(OP_RUSAGE_WHO, 7, 77, "c"), Err(R_EINVAL));
        assert_eq!(b.expand(OP_SETRLIMIT_RES, 7, 77, "d"), Ok((7, Q_RES_RANGE)));
        assert_eq!(b.expand(OP_SETRLIMIT_RES, 99, 77, "e"), Err(R_EINVAL));
        assert_eq!(b.expand(OP_PRLIMIT_PID, 0, 77, "f"), Ok((77, Q_PID_SELF)));
        assert_eq!(b.expand(99, 0, 77, "g"), Err(RP_UNK_OP));
    }

    #[test]
    fn unx9175_checks_green() {
        let s = run_unx9175_checks();
        assert!(s.all_passed(), "UNX-F9175 判据未全绿");
    }
}
