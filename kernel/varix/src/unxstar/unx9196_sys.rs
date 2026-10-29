//! UNX-F9196 身份/休眠族参数探针总成（0x63840 段）
//!
//! 锚点：UNX-F9196-J1 探针账（身份/休眠参数面 op × 原始参数 × 展开
//! 参数 × 规则四元组逐条落账）；J2 探针语义（KEEP 保持 + 零 pid 自
//! 指 + 负域拒 + 时钟基闭集四规则闭集）；J3 探针指纹（账本折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B20（身份/休眠参数展开面）。
//!
//! 结构：规则闭集 + 展开核 + 探针账本 + 折叠指纹 + 注入核（错规则/
//! 空留痕/伪 op/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63840 独占 0x20）。
pub const SEG_BASE: u32 = 0x63840;
pub const IP_UNK_OP: u32 = SEG_BASE + 0x01; // 未知 op
pub const IP_BAD_RULE: u32 = SEG_BASE + 0x02; // 规则判定错
pub const IP_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const IP_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const IP_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EINVAL: u32 = 1;
pub const R_COUNT: usize = 2;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EINVAL"];

/// 操作闭集。
pub const OP_RESUID_KEEP: usize = 0; // setresuid 各位（KEEP=−1 保持）
pub const OP_CLOCK_CLK: usize = 1; // clock_nanosleep 时钟基位
pub const OP_CLOCK_FLAG: usize = 2; // flag 位（0/1）
pub const OP_PID: usize = 3; // pid 位（0=自身）
pub const OP_COUNT: usize = 4;

pub const OP_NAMES: [&str; OP_COUNT] =
    ["resuid.keep", "clock.clk", "clock.flag", "pid.self"];

/// 规则闭集。
pub const K_KEEP: usize = 0; // u32::MAX → KEEP 哨兵
pub const K_CLK_RANGE: usize = 1; // 时钟基 <2 直传
pub const K_FLAG_RANGE: usize = 2; // flag ∈ {0,1}
pub const K_PID_SELF: usize = 3; // 0 → caller
pub const K_NEG_REJ: usize = 4; // 负域（非 KEEP）拒
pub const K_COUNT: usize = 5;

pub const RULE_NAMES: [&str; K_COUNT] =
    ["keep", "clk-range", "flag-range", "pid-self", "neg-rej"];

pub const KEEP: u32 = u32::MAX;
pub const NEG_SENTINEL: u32 = 0x8000_0000;

/// 单条探针账。
#[derive(Clone)]
pub struct IpProbe {
    pub seq: u32,
    pub op: usize,
    pub raw: u32,
    pub expanded: u32,
    pub rule: usize,
    pub code: u32,
    pub note: String,
}

impl IpProbe {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.op < OP_COUNT && self.rule < K_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "IP{}|{}|raw{:x}->exp{:x}|{}|{}|{}",
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
pub struct IpBook {
    pub ledger: Vec<IpProbe>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl IpBook {
    pub fn new() -> IpBook {
        IpBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn expand(
        &mut self,
        op: usize,
        raw: u32,
        caller: u32,
        note: &str,
    ) -> Result<(u32, usize), u32> {
        if self.sealed {
            return Err(IP_FROZEN);
        }
        if note.is_empty() {
            return Err(IP_EMPTY_NOTE);
        }
        if op >= OP_COUNT {
            return Err(IP_UNK_OP);
        }
        let (expanded, rule, code): (u32, usize, u32) = match op {
            OP_RESUID_KEEP => {
                if raw == KEEP {
                    (KEEP, K_KEEP, R_OK)
                } else if raw >= NEG_SENTINEL {
                    (0, K_NEG_REJ, R_EINVAL)
                } else {
                    (raw, K_KEEP, R_OK)
                }
            }
            OP_CLOCK_CLK => {
                if raw >= NEG_SENTINEL {
                    (0, K_NEG_REJ, R_EINVAL)
                } else if raw < 2 {
                    (raw, K_CLK_RANGE, R_OK)
                } else {
                    (0, K_CLK_RANGE, R_EINVAL)
                }
            }
            OP_CLOCK_FLAG => {
                if raw <= 1 {
                    (raw, K_FLAG_RANGE, R_OK)
                } else {
                    (0, K_FLAG_RANGE, R_EINVAL)
                }
            }
            OP_PID => {
                if raw >= NEG_SENTINEL && raw != KEEP {
                    (0, K_NEG_REJ, R_EINVAL)
                } else if raw == 0 {
                    (caller, K_PID_SELF, R_OK)
                } else {
                    (raw, K_PID_SELF, R_OK)
                }
            }
            _ => return Err(IP_UNK_OP),
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
        self.ledger.push(IpProbe { seq, op, raw, expanded, rule, code, note: note.to_string() });
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
        let mut h: u32 = 0x91960;
        for p in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(p.seq);
            h = h.wrapping_mul(31).wrapping_add(p.op as u32);
            h = h.wrapping_mul(31).wrapping_add(p.raw);
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
    let bad = IpProbe {
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
    let bad2 = IpProbe {
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
    let mut b = IpBook::new();
    got[2] = b.expand(99, 0, 1, "inj-op").err() == Some(IP_UNK_OP);
    // 4) 时钟基越域
    got[3] = b.expand(OP_CLOCK_CLK, 7, 1, "inj-clk").err() == Some(R_EINVAL);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.expand(OP_CLOCK_CLK, 0, 1, "inj-frozen").err() == Some(IP_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9196_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9196");
    let mut b = IpBook::new();
    const CALLER: u32 = 55;

    // J1 探针账非空
    let _ = b.expand(OP_RESUID_KEEP, KEEP, CALLER, "KEEP 探针");
    let j1 = !b.ledger.is_empty();
    s.add("J1-探针账非空", j1, "至少一条探针在册");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|p| p.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 op/rule 闭集");

    // J3 KEEP 哨兵
    let j3 = b.expand(OP_RESUID_KEEP, KEEP, CALLER, "KEEP") == Ok((KEEP, K_KEEP));
    s.add("J3-KEEP哨兵", j3, "u32::MAX → KEEP 保持");

    // J4 KEEP 位负域拒（非 KEEP 的负值）
    let j4 = b.expand(OP_RESUID_KEEP, 0xFFFF_0000, CALLER, "负").err() == Some(R_EINVAL);
    s.add("J4-负域拒", j4, "非 KEEP 负值必 EINVAL");

    // J5 时钟基闭集：<2 放行
    let j5 = {
        let a = b.expand(OP_CLOCK_CLK, 0, CALLER, "realtime") == Ok((0, K_CLK_RANGE));
        let bb = b.expand(OP_CLOCK_CLK, 1, CALLER, "monotonic") == Ok((1, K_CLK_RANGE));
        a && bb
    };
    s.add("J5-时钟基闭集", j5, "clk∈{0,1} 放行");

    // J6 时钟基越域拒
    let j6 = b.expand(OP_CLOCK_CLK, 2, CALLER, "越").err() == Some(R_EINVAL);
    s.add("J6-时钟基越域", j6, "clk≥2 必 EINVAL");

    // J7 flag 闭集
    let j7 = {
        let a = b.expand(OP_CLOCK_FLAG, 0, CALLER, "rel") == Ok((0, K_FLAG_RANGE));
        let bb = b.expand(OP_CLOCK_FLAG, 1, CALLER, "abstime") == Ok((1, K_FLAG_RANGE));
        let c = b.expand(OP_CLOCK_FLAG, 3, CALLER, "野").err() == Some(R_EINVAL);
        a && bb && c
    };
    s.add("J7-flag闭集", j7, "flag∈{0,1} 放行其余拒");

    // J8 pid 零自指
    let j8 = b.expand(OP_PID, 0, CALLER, "自指") == Ok((CALLER, K_PID_SELF));
    s.add("J8-pid自指", j8, "pid=0 展开 caller");

    // J9 pid 直传
    let j9 = b.expand(OP_PID, 42, CALLER, "直传") == Ok((42, K_PID_SELF));
    s.add("J9-pid直传", j9, "非零 pid 直传");

    // J10 伪 op 拒
    let j10 = b.expand(99, 0, CALLER, "伪").err() == Some(IP_UNK_OP);
    s.add("J10-伪op拒", j10, "op 出闭集必拒");

    // J11 拒绝留痕
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
    fn unx9196_expand_rules() {
        let mut b = IpBook::new();
        assert_eq!(b.expand(OP_RESUID_KEEP, KEEP, 55, "a"), Ok((KEEP, K_KEEP)));
        assert_eq!(b.expand(OP_RESUID_KEEP, 0xFFFF_0000, 55, "b"), Err(R_EINVAL));
        assert_eq!(b.expand(OP_CLOCK_CLK, 1, 55, "c"), Ok((1, K_CLK_RANGE)));
        assert_eq!(b.expand(OP_CLOCK_CLK, 2, 55, "d"), Err(R_EINVAL));
        assert_eq!(b.expand(OP_CLOCK_FLAG, 3, 55, "e"), Err(R_EINVAL));
        assert_eq!(b.expand(OP_PID, 0, 55, "f"), Ok((55, K_PID_SELF)));
        assert_eq!(b.expand(99, 0, 55, "g"), Err(IP_UNK_OP));
    }

    #[test]
    fn unx9196_checks_green() {
        let s = run_unx9196_checks();
        assert!(s.all_passed(), "UNX-F9196 判据未全绿");
    }
}
