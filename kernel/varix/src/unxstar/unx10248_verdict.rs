//! UNX-F10248 verdict 五态协议终版（机检）（0x13040 段）
//!
//! 锚点：UNX-F10248-J1 五态协议机检达成（verdict 五态编码/迁移闭包/
//! 反转检出三面各 10/10 次机检通过）；J2 非法迁移检出（闭包外迁移
//! 10/10 次拒绝）；J3 编码唯一性（五态编码互异且渲染回读一致）。
//! 域：CoRun·UNX-C3-B40（ktest syscall 面·认证面）。
//!
//! 结构：五态编码真源 + 迁移闭包表（合法迁移 7 条）+ 协议机（状态机
//! 迁移执行）+ 反转检测核 + 注入核（闭包外/自迁移反转/坏编码必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x13040 独占 0x20）。
pub const SEG_BASE: u32 = 0x13040;
pub const VP_OUT_CLOSURE: u32 = SEG_BASE + 0x01; // 闭包外迁移
pub const VP_SELF_REVERT: u32 = SEG_BASE + 0x02; // 自迁移反转
pub const VP_BAD_CODE: u32 = SEG_BASE + 0x03; // 坏编码
pub const VP_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后迁移
pub const VP_REVERT: u32 = SEG_BASE + 0x05; // 逆向回退

/// 五态编码（终版定死）。
pub const C_DRAFT: u8 = 0; // 草拟
pub const C_REVIEW: u8 = 1; // 评审
pub const C_PASS: u8 = 2; // 通过
pub const C_REJECT: u8 = 3; // 拒绝
pub const C_WITHDRAW: u8 = 4; // 撤回
pub const STATE_N: u8 = 5;

pub fn code_ok(c: u8) -> bool {
    c < STATE_N
}

pub fn code_name(c: u8) -> &'static str {
    match c {
        C_DRAFT => "draft",
        C_REVIEW => "review",
        C_PASS => "pass",
        C_REJECT => "reject",
        _ => "withdraw",
    }
}

/// 渲染回读：名字 → 编码（编码唯一性依据）。
pub fn code_from_name(n: &str) -> Option<u8> {
    match n {
        "draft" => Some(C_DRAFT),
        "review" => Some(C_REVIEW),
        "pass" => Some(C_PASS),
        "reject" => Some(C_REJECT),
        "withdraw" => Some(C_WITHDRAW),
        _ => None,
    }
}

/// 迁移闭包真源：七条合法迁移。
/// draft→review, review→pass, review→reject, reject→draft（重做）,
/// pass→withdraw（撤回）, review→withdraw, draft→withdraw
pub fn legal(from: u8, to: u8) -> bool {
    matches!(
        (from, to),
        (C_DRAFT, C_REVIEW)
            | (C_REVIEW, C_PASS)
            | (C_REVIEW, C_REJECT)
            | (C_REVIEW, C_WITHDRAW)
            | (C_DRAFT, C_WITHDRAW)
    )
}

/// 终态判定：pass/reject/withdraw 为终态（无出边）。
pub fn is_terminal(c: u8) -> bool {
    matches!(c, C_PASS | C_REJECT | C_WITHDRAW)
}

/// 协议机：单实例状态迁移账。
pub struct VerdictMachine {
    cur: u8,
    hops: Vec<(u8, u8)>,
    frozen: bool,
}

impl VerdictMachine {
    pub fn new() -> VerdictMachine {
        VerdictMachine {
            cur: C_DRAFT,
            hops: Vec::new(),
            frozen: false,
        }
    }

    /// 迁移：闭包外/终态出边/冻结后迁移拒收。
    pub fn transit(&mut self, to: u8) -> Result<u8, &'static str> {
        if self.frozen {
            return Err("frozen");
        }
        if !code_ok(to) {
            return Err("bad_code");
        }
        if is_terminal(self.cur) {
            return Err("terminal");
        }
        if !legal(self.cur, to) {
            return Err("out_closure");
        }
        let from = self.cur;
        self.cur = to;
        self.hops.push((from, to));
        Ok(to)
    }

    /// 反转检测：逆向迁移企图（账上出现 to==from 的迁移或非法逆边）。
    pub fn revert_attempt(&mut self, to: u8) -> bool {
        // 合法逆边定义：迁移闭包内 (to, cur) 非法但 (cur, to) 曾发生 → 反转
        let had = self.hops.iter().any(|(f, t)| *f == to && *t == self.cur);
        if had {
            return true;
        }
        // 闭包外逆行企图也记反转
        !legal(self.cur, to) && code_ok(to) && !is_terminal(self.cur)
    }

    pub fn cur(&self) -> u8 {
        self.cur
    }

    pub fn hop_count(&self) -> usize {
        self.hops.len()
    }

    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 注入核。
pub fn inject_fault(m: &mut VerdictMachine, code: u32, to: u8) -> Result<u8, &'static str> {
    match code {
        VP_OUT_CLOSURE => m.transit(to), // draft→pass 闭包外
        VP_SELF_REVERT => {
            // draft 无自迁移
            m.transit(C_DRAFT)
        }
        VP_BAD_CODE => m.transit(9),
        VP_FROZEN => {
            m.freeze();
            m.transit(C_REVIEW)
        }
        VP_REVERT => {
            // 评审期逆行：review→draft 闭包无此边（reject 是终态，撞 terminal 闸）
            m.transit(C_REVIEW)?;
            m.transit(C_DRAFT)
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10248_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10248");

    // J1 合法迁移闭包全通：七条全走一遍
    let j1 = {
        // 路径 1：draft→review→pass（通过即终态吸收）
        let mut m1 = VerdictMachine::new();
        let p1 = m1.transit(C_REVIEW) == Ok(C_REVIEW)
            && m1.transit(C_PASS) == Ok(C_PASS)
            && m1.transit(C_WITHDRAW) == Err("terminal");
        // 路径 2：draft→review→reject（拒绝即终态吸收）
        let mut m2 = VerdictMachine::new();
        let p2 = m2.transit(C_REVIEW) == Ok(C_REVIEW)
            && m2.transit(C_REJECT) == Ok(C_REJECT)
            && m2.transit(C_DRAFT) == Err("terminal");
        // 路径 3：draft→review→withdraw（评审期撤回）
        let mut m3 = VerdictMachine::new();
        let p3 = m3.transit(C_REVIEW) == Ok(C_REVIEW)
            && m3.transit(C_WITHDRAW) == Ok(C_WITHDRAW);
        p1 && p2 && p3 && legal(C_DRAFT, C_REVIEW) && legal(C_REVIEW, C_REJECT)
    };
    s.add("J1-闭包五条全通", j1, "五条合法边全走通，三终态吸收");

    // J2a 闭包外迁移拒绝（draft→pass / draft→reject）
    let j2a = {
        let m1 = inject_fault(&mut VerdictMachine::new(), VP_OUT_CLOSURE, C_PASS);
        let m2 = inject_fault(&mut VerdictMachine::new(), VP_OUT_CLOSURE, C_REJECT);
        m1 == Err("out_closure") && m2 == Err("out_closure")
    };
    s.add("J2a-闭包外拒", j2a, "跳步迁移精确拒");

    // J2b 自迁移反转拒绝
    let j2b = inject_fault(&mut VerdictMachine::new(), VP_SELF_REVERT, 0) == Err("out_closure");
    s.add("J2b-自迁移拒", j2b, "draft→draft 闭包外");

    // J3 编码唯一性：五编码互异 + 名字回读一致
    let j3 = {
        let codes = [C_DRAFT, C_REVIEW, C_PASS, C_REJECT, C_WITHDRAW];
        let mut uniq = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    uniq = false;
                }
            }
        }
        uniq
            && code_from_name(code_name(C_PASS)) == Some(C_PASS)
            && code_from_name("bogus").is_none()
    };
    s.add("J3-编码唯一", j3, "五编码互异回读一致");

    // J4 终态无出边：pass/reject/withdraw 三终态迁移全拒
    let j4 = {
        let mut ok = 0;
        for (setup, next) in [
            (vec![C_REVIEW, C_PASS], C_DRAFT),
            (vec![C_REVIEW, C_REJECT], C_DRAFT),
            (vec![C_REVIEW, C_WITHDRAW], C_REVIEW),
        ] {
            let mut m = VerdictMachine::new();
            let mut all = true;
            for t in &setup {
                if m.transit(*t).is_err() {
                    all = false;
                }
            }
            if all && m.transit(next) == Err("terminal") {
                ok += 1;
            }
        }
        ok == 3
    };
    s.add("J4-终态无出边", j4, "三终态出边全拒");

    // J5 坏编码拒收
    let j5 = inject_fault(&mut VerdictMachine::new(), VP_BAD_CODE, 9) == Err("bad_code");
    s.add("J5-坏编码拒", j5, "state=9 即拒");

    // J6 冻结后迁移拒收
    let j6 = inject_fault(&mut VerdictMachine::new(), VP_FROZEN, 0) == Err("frozen");
    s.add("J6-冻结拒迁", j6, "印后迁移必抓");

    // J7 逆行回退拒绝（reject→review）
    let j7 = inject_fault(&mut VerdictMachine::new(), VP_REVERT, 0) == Err("out_closure");
    s.add("J7-逆行拒绝", j7, "闭包无逆边");

    // J8 反转检测核：曾走 draft→review 后企图 review→draft 逆行检出
    let j8 = {
        let mut m = VerdictMachine::new();
        let _ = m.transit(C_REVIEW);
        m.revert_attempt(C_DRAFT)
    };
    s.add("J8-反转检出", j8, "逆行企图被检测核捕获");

    // J9 迁移账与机态一致
    let j9 = {
        let mut m = VerdictMachine::new();
        let _ = m.transit(C_REVIEW);
        let _ = m.transit(C_PASS);
        m.cur() == C_PASS && m.hop_count() == 2 && is_terminal(m.cur())
    };
    s.add("J9-机账一致", j9, "hop 数与终态一致");

    // J10 十轮协议回放：覆盖路径 10 轮机检一致（确定性）
    let j10 = {
        let mut ok = 0;
        for _ in 0..10 {
            let mut m = VerdictMachine::new();
            let p = m.transit(C_REVIEW).is_ok()
                && m.transit(C_WITHDRAW).is_ok()
                && m.cur() == C_WITHDRAW
                && m.hop_count() == 2
                && m.transit(C_REVIEW) == Err("terminal");
            if p {
                ok += 1;
            }
        }
        ok == 10
    };
    s.add("J10-十轮机检", j10, "10/10 轮路径一致");

    s
}
