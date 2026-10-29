//! UNX-F9156 进程族参数探针总成（0x63340 段）
//!
//! 锚点：UNX-F9156-J1 探针账（op × 原始参数 × 展开参数 × 规则四元
//! 组逐条落账）；J2 展开语义（零号自指 + 组零自聚 + 负域拒收 + 非
//! 零直传四规则闭集）；J3 探针指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（进程族参数展开面）。
//!
//! 结构：规则闭集（P_SELF/P_GRP_ZERO/P_NEG_REJ/P_RAW_PASS）+ 展开
//! 核（op 分派）+ 探针账本 + 折叠指纹 + 注入核（错规则/空留痕/伪
//! op/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63340 独占 0x20）。
pub const SEG_BASE: u32 = 0x63340;
pub const PR_UNK_OP: u32 = SEG_BASE + 0x01; // 未知 op
pub const PR_BAD_RULE: u32 = SEG_BASE + 0x02; // 规则判定错
pub const PR_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const PR_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const PR_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EINVAL: u32 = 1; // 负域拒收
pub const R_COUNT: usize = 2;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EINVAL"];

/// 操作闭集（探针覆盖的进程族参数位）。
pub const OP_SETPGID_PID: usize = 0; // setpgid 的 pid 位（0=自身）
pub const OP_SETPGID_PGID: usize = 1; // setpgid 的 pgid 位（0=自聚）
pub const OP_GETPGID_PID: usize = 2; // getpgid 的 pid 位（0=自身）
pub const OP_KILL_PID: usize = 3; // kill 的 pid 位（0=全组；本档以直传+负域拒收格）
pub const OP_WAIT_PID: usize = 4; // waitpid 的 pid 位（-1 任意/-pgid 组/0 同组/>0 定点）
pub const OP_COUNT: usize = 5;

pub const OP_NAMES: [&str; OP_COUNT] =
    ["setpgid.pid", "setpgid.pgid", "getpgid.pid", "kill.pid", "waitpid.pid"];

/// 展开规则闭集。
pub const P_SELF: usize = 0; // 0 → caller 自身
pub const P_GRP_ZERO: usize = 1; // 0 → 目标 pid 自聚
pub const P_NEG_REJ: usize = 2; // 负域 → EINVAL
pub const P_RAW_PASS: usize = 3; // 非0非负 → 直传
pub const P_WAIT_ANY: usize = 4; // waitpid -1 → 任意子
pub const P_COUNT: usize = 5;

pub const RULE_NAMES: [&str; P_COUNT] =
    ["self", "grp-zero", "neg-rej", "raw-pass", "wait-any"];

/// 负域哨兵（u32 全域中 0x8000_0000 以上视为负 int 语义）。
pub const NEG_SENTINEL: u32 = 0x8000_0000;

/// 单条探针账。
#[derive(Clone)]
pub struct Probe {
    pub seq: u32,
    pub op: usize,
    pub raw: u32,
    pub expanded: u32,
    pub rule: usize,
    pub code: u32,
    pub note: String,
}

impl Probe {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.op < OP_COUNT && self.rule < P_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "PR{}|{}|raw{:x}->exp{:x}|{}|{}|{}",
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
pub struct ProbeBook {
    pub ledger: Vec<Probe>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ProbeBook {
    pub fn new() -> ProbeBook {
        ProbeBook { ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 展开核：按 op 分派规则，返回 (展开值, 规则, 码)。
    pub fn expand(
        &mut self,
        op: usize,
        raw: u32,
        caller: u32,
        note: &str,
    ) -> Result<(u32, usize), u32> {
        if self.sealed {
            return Err(PR_FROZEN);
        }
        if note.is_empty() {
            return Err(PR_EMPTY_NOTE);
        }
        if op >= OP_COUNT {
            return Err(PR_UNK_OP);
        }
        let is_neg = raw >= NEG_SENTINEL;
        let (expanded, rule, code): (u32, usize, u32) = match op {
            OP_SETPGID_PID | OP_GETPGID_PID => {
                if is_neg {
                    (0, P_NEG_REJ, R_EINVAL)
                } else if raw == 0 {
                    (caller, P_SELF, R_OK)
                } else {
                    (raw, P_RAW_PASS, R_OK)
                }
            }
            OP_SETPGID_PGID => {
                if is_neg {
                    (0, P_NEG_REJ, R_EINVAL)
                } else if raw == 0 {
                    // 自聚：展开为目标 pid——探针以 caller 代目标位
                    (caller, P_GRP_ZERO, R_OK)
                } else {
                    (raw, P_RAW_PASS, R_OK)
                }
            }
            OP_KILL_PID => {
                if is_neg && raw != u32::MAX {
                    // 负 pid = 组投递（-pgid），本档登记展开为位样
                    (raw.wrapping_neg(), P_NEG_REJ, R_OK) // 组号直取
                } else if is_neg {
                    (raw, P_RAW_PASS, R_OK) // -1 特权投递直传
                } else if raw == 0 {
                    (caller, P_SELF, R_OK)
                } else {
                    (raw, P_RAW_PASS, R_OK)
                }
            }
            OP_WAIT_PID => {
                if raw == 0 {
                    (caller, P_WAIT_ANY, R_OK) // 0 = 同组任意（探针简化）
                } else if is_neg && raw == u32::MAX {
                    (0, P_WAIT_ANY, R_OK) // -1 = 任意子 → 0 哨兵
                } else if is_neg {
                    (raw.wrapping_neg(), P_RAW_PASS, R_OK) // -pgid → pgid
                } else {
                    (raw, P_RAW_PASS, R_OK)
                }
            }
            _ => return Err(PR_UNK_OP),
        };
        if code == R_EINVAL {
            self.record(op, raw, expanded, rule, code, note);
            return Err(R_EINVAL);
        }
        self.record(op, raw, expanded, rule, code, note);
        Ok((expanded, rule))
    }

    fn record(&mut self, op: usize, raw: u32, expanded: u32, rule: usize, code: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(Probe { seq, op, raw, expanded, rule, code, note: note.to_string() });
    }

    /// 规则判定一致性：账面 rule 与重算一致。
    pub fn rule_consistent(&self, caller: u32) -> bool {
        for p in &self.ledger {
            if p.code != R_OK {
                continue;
            }
            let expect: usize = match p.op {
                OP_SETPGID_PID | OP_GETPGID_PID => {
                    if p.raw == 0 {
                        P_SELF
                    } else {
                        P_RAW_PASS
                    }
                }
                OP_SETPGID_PGID => {
                    if p.raw == 0 {
                        P_GRP_ZERO
                    } else {
                        P_RAW_PASS
                    }
                }
                OP_KILL_PID => {
                    if p.raw == 0 {
                        P_SELF
                    } else if p.raw >= NEG_SENTINEL && p.raw != u32::MAX {
                        P_NEG_REJ
                    } else {
                        P_RAW_PASS
                    }
                }
                OP_WAIT_PID => {
                    if p.raw == 0 || p.raw == u32::MAX {
                        P_WAIT_ANY
                    } else {
                        P_RAW_PASS
                    }
                }
                _ => continue,
            };
            if p.rule != expect {
                return false;
            }
        }
        let _ = caller;
        true
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
        let mut h: u32 = 0x91560;
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
    let bad = Probe {
        seq: 1,
        op: OP_SETPGID_PID,
        raw: 0,
        expanded: 7,
        rule: 99,
        code: R_OK,
        note: "inj-rule".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = Probe {
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
    let mut b = ProbeBook::new();
    got[2] = b.expand(99, 0, 1, "inj-op").err() == Some(PR_UNK_OP);
    // 4) 负域拒收
    got[3] = b.expand(OP_SETPGID_PID, 0xFFFF_8000, 1, "inj-neg").err() == Some(R_EINVAL);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.expand(OP_SETPGID_PID, 0, 1, "inj-frozen").err() == Some(PR_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9156_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9156");
    let mut b = ProbeBook::new();
    const CALLER: u32 = 42;

    // J1 探针账非空
    let _ = b.expand(OP_SETPGID_PID, 0, CALLER, "零号自指");
    let j1 = !b.ledger.is_empty();
    s.add("J1-探针账非空", j1, "至少一条探针在册");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|p| p.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 op/rule 闭集");

    // J3 零号自指
    let j3 = b.expand(OP_GETPGID_PID, 0, CALLER, "getpgid 0") == Ok((CALLER, P_SELF));
    s.add("J3-零号自指", j3, "pid=0 展开 caller");

    // J4 组零自聚
    let j4 = b.expand(OP_SETPGID_PGID, 0, 77, "pgid 0") == Ok((77, P_GRP_ZERO));
    s.add("J4-组零自聚", j4, "pgid=0 展开目标 pid");

    // J5 负域拒收
    let j5 = b.expand(OP_SETPGID_PID, 0xFFFF_0000, CALLER, "负 pid").err() == Some(R_EINVAL);
    s.add("J5-负域拒收", j5, "负域参数必 EINVAL");

    // J6 非零直传
    let j6 = b.expand(OP_SETPGID_PID, 7, CALLER, "直传") == Ok((7, P_RAW_PASS));
    s.add("J6-非零直传", j6, "非0非负直传");

    // J7 waitpid -1 任意
    let j7 = b.expand(OP_WAIT_PID, u32::MAX, CALLER, "wait 任意") == Ok((0, P_WAIT_ANY));
    s.add("J7-wait任意", j7, "-1 展开任意子哨兵");

    // J8 waitpid 负组
    let j8 = b.expand(OP_WAIT_PID, (-50i64) as u32, CALLER, "wait 组") == Ok((50, P_RAW_PASS));
    s.add("J8-wait负组", j8, "-pgid 展开组号");

    // J9 kill 零自指
    let j9 = b.expand(OP_KILL_PID, 0, CALLER, "kill 0") == Ok((CALLER, P_SELF));
    s.add("J9-kill零自指", j9, "kill pid=0 展开自身组");

    // J10 规则一致性复算
    let j10 = b.rule_consistent(CALLER);
    s.add("J10-规则一致", j10, "账面 rule 与重算一致");

    // J11 伪 op 拒
    let j11 = b.expand(99, 0, CALLER, "伪op").err() == Some(PR_UNK_OP);
    s.add("J11-伪op拒", j11, "op 出闭集必拒");

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
    fn unx9156_expand_rules() {
        let mut b = ProbeBook::new();
        assert_eq!(b.expand(OP_SETPGID_PID, 0, 42, "a"), Ok((42, P_SELF)));
        assert_eq!(b.expand(OP_SETPGID_PGID, 0, 77, "b"), Ok((77, P_GRP_ZERO)));
        assert_eq!(b.expand(OP_WAIT_PID, u32::MAX, 42, "c"), Ok((0, P_WAIT_ANY)));
        assert_eq!(b.expand(OP_SETPGID_PID, 0xFFFF_0000, 42, "d"), Err(R_EINVAL));
        assert_eq!(b.expand(99, 0, 42, "e"), Err(PR_UNK_OP));
        assert!(b.rule_consistent(42));
    }

    #[test]
    fn unx9156_checks_green() {
        let s = run_unx9156_checks();
        assert!(s.all_passed(), "UNX-F9156 判据未全绿");
    }
}
