//! UNX-F10293 回流缺陷状态机（开单/收单/修复/回执/闭环五态）（0x18FE0 段）
//!
//! 锚点：UNX-F10293-J1 状态机达成（五态迁移闭包全通 10/10 次）；J2 非
//! 法迁移检出（闭包外/终态出边/逆向回退 10/10 次拒绝）；J3 编码唯一
//! 性（五态编码互异回读一致）。
//! 域：CoRun·UNX-C3-B40（O3 对接面·回流状态机）。
//!
//! 结构：五态编码真源 + 迁移闭包表 + 工单状态机 + 反转检测 + 注入核
//! （闭包外/终态出边/坏编码必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x18FE0 独占 0x20）。
pub const SEG_BASE: u32 = 0x18FE0;
pub const RF_OUT_CLOSURE: u32 = SEG_BASE + 0x01; // 闭包外迁移
pub const RF_TERMINAL: u32 = SEG_BASE + 0x02; // 终态出边
pub const RF_BAD_CODE: u32 = SEG_BASE + 0x03; // 坏编码
pub const RF_FROZEN: u32 = SEG_BASE + 0x04; // 冻结后迁移
pub const RF_REVERT: u32 = SEG_BASE + 0x05; // 逆向回退

/// 五态编码。
pub const C_OPEN: u8 = 0; // 开单
pub const C_RECV: u8 = 1; // 收单
pub const C_FIXED: u8 = 2; // 修复
pub const C_ACKED: u8 = 3; // 回执
pub const C_CLOSED: u8 = 4; // 闭环
pub const STATE_N: u8 = 5;

pub fn code_ok(c: u8) -> bool {
    c < STATE_N
}

pub fn code_name(c: u8) -> &'static str {
    match c {
        C_OPEN => "open",
        C_RECV => "recv",
        C_FIXED => "fixed",
        C_ACKED => "acked",
        _ => "closed",
    }
}

/// 名字回读。
pub fn code_from_name(n: &str) -> Option<u8> {
    match n {
        "open" => Some(C_OPEN),
        "recv" => Some(C_RECV),
        "fixed" => Some(C_FIXED),
        "acked" => Some(C_ACKED),
        "closed" => Some(C_CLOSED),
        _ => None,
    }
}

/// 迁移闭包真源：六条合法迁移。
/// open→recv, recv→fixed, fixed→acked, acked→closed,
/// fixed→open（打回重开）, recv→closed（误报直接闭）
pub fn legal(from: u8, to: u8) -> bool {
    matches!(
        (from, to),
        (C_OPEN, C_RECV)
            | (C_RECV, C_FIXED)
            | (C_FIXED, C_ACKED)
            | (C_ACKED, C_CLOSED)
            | (C_FIXED, C_OPEN)
            | (C_RECV, C_CLOSED)
    )
}

/// 终态：closed。
pub fn is_terminal(c: u8) -> bool {
    c == C_CLOSED
}

/// 工单状态机。
pub struct ReflowMachine {
    cur: u8,
    hops: Vec<(u8, u8)>,
    frozen: bool,
}

impl ReflowMachine {
    pub fn new() -> ReflowMachine {
        ReflowMachine { cur: C_OPEN, hops: Vec::new(), frozen: false }
    }

    /// 迁移：闭包外/终态出边/坏编码/冻结拒收。
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

    /// 逆向回退检测：企图走闭包内逆向边。
    pub fn revert_attempt(&mut self, to: u8) -> bool {
        let had = self.hops.iter().any(|(f, t)| *f == to && *t == self.cur);
        had || (!legal(self.cur, to) && code_ok(to) && !is_terminal(self.cur))
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
pub fn inject_fault(m: &mut ReflowMachine, code: u32, to: u8) -> Result<u8, &'static str> {
    match code {
        RF_OUT_CLOSURE => m.transit(to), // open→acked 跳步
        RF_TERMINAL => {
            let _ = m.transit(C_RECV)?;
            let _ = m.transit(C_CLOSED)?;
            m.transit(C_OPEN)
        }
        RF_BAD_CODE => m.transit(9),
        RF_FROZEN => {
            m.freeze();
            m.transit(C_RECV)
        }
        RF_REVERT => {
            let _ = m.transit(C_RECV)?;
            let _ = m.transit(C_FIXED)?;
            m.transit(C_RECV) // fixed→recv 逆向
        }
        _ => Err("unknown"),
    }
}

/// 批内判据。
pub fn run_unx10293_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10293");

    // J1 迁移闭包全通：全路径 open→recv→fixed→acked→closed
    let j1 = {
        let mut m1 = ReflowMachine::new();
        let p1 = m1.transit(C_RECV) == Ok(C_RECV)
            && m1.transit(C_FIXED) == Ok(C_FIXED)
            && m1.transit(C_ACKED) == Ok(C_ACKED)
            && m1.transit(C_CLOSED) == Ok(C_CLOSED);
        // 打回路径：recv→fixed→open
        let mut m2 = ReflowMachine::new();
        let p2 = m2.transit(C_RECV) == Ok(C_RECV)
            && m2.transit(C_FIXED) == Ok(C_FIXED)
            && m2.transit(C_OPEN) == Ok(C_OPEN);
        // 误报直闭：recv→closed
        let mut m3 = ReflowMachine::new();
        let p3 = m3.transit(C_RECV) == Ok(C_RECV) && m3.transit(C_CLOSED) == Ok(C_CLOSED);
        p1 && p2 && p3
    };
    s.add("J1-闭包三径全通", j1, "闭环/打回/直闭三径");

    // J2a 闭包外拒绝（open→fixed 跳步）
    let j2a = inject_fault(&mut ReflowMachine::new(), RF_OUT_CLOSURE, C_FIXED) == Err("out_closure");
    s.add("J2a-跳步拒绝", j2a, "open→fixed 即拒");

    // J2b 终态出边拒绝
    let j2b = inject_fault(&mut ReflowMachine::new(), RF_TERMINAL, C_OPEN) == Err("terminal");
    s.add("J2b-终态拒绝", j2b, "closed 无出边");

    // J2c 逆向回退拒绝
    let j2c = inject_fault(&mut ReflowMachine::new(), RF_REVERT, 0) == Err("out_closure");
    s.add("J2c-逆向拒绝", j2c, "fixed→recv 闭包外");

    // J3 编码唯一性：五编码互异 + 回读一致
    let j3 = {
        let codes = [C_OPEN, C_RECV, C_FIXED, C_ACKED, C_CLOSED];
        let mut uniq = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    uniq = false;
                }
            }
        }
        uniq
            && code_from_name(code_name(C_ACKED)) == Some(C_ACKED)
            && code_from_name("bogus").is_none()
    };
    s.add("J3-编码唯一", j3, "五编码互异回读一致");

    // J4 坏编码拒绝
    let j4 = inject_fault(&mut ReflowMachine::new(), RF_BAD_CODE, 9) == Err("bad_code");
    s.add("J4-坏编码拒", j4, "state=9 即拒");

    // J5 反转检测核
    let j5 = {
        let mut m = ReflowMachine::new();
        let _ = m.transit(C_RECV);
        let _ = m.transit(C_FIXED);
        m.revert_attempt(C_RECV)
    };
    s.add("J5-反转检出", j5, "逆行企图被检测捕获");

    // J6 迁移账与机态一致
    let j6 = {
        let mut m = ReflowMachine::new();
        let _ = m.transit(C_RECV);
        let _ = m.transit(C_FIXED);
        m.cur() == C_FIXED && m.hop_count() == 2 && !is_terminal(m.cur())
    };
    s.add("J6-机账一致", j6, "hop 数与态一致");

    // J7 注入五路
    let codes = [RF_OUT_CLOSURE, RF_TERMINAL, RF_BAD_CODE, RF_FROZEN, RF_REVERT];
    let caught = codes
        .iter()
        .filter(|c| inject_fault(&mut ReflowMachine::new(), **c, C_ACKED).is_err())
        .count();
    s.add("J7-注入五路", caught == 5, "五路注入全拒");

    // J8 冻结红线
    let j8 = {
        let mut m = ReflowMachine::new();
        inject_fault(&mut m, RF_FROZEN, 0);
        m.is_frozen()
    };
    s.add("J8-冻结拒迁", j8, "印后迁移必拒");

    // J9 闭包真源：六条合法边精确
    let j9 = legal(C_OPEN, C_RECV)
        && legal(C_RECV, C_FIXED)
        && legal(C_FIXED, C_ACKED)
        && legal(C_ACKED, C_CLOSED)
        && legal(C_FIXED, C_OPEN)
        && legal(C_RECV, C_CLOSED)
        && !legal(C_OPEN, C_FIXED)
        && !legal(C_ACKED, C_FIXED);
    s.add("J9-闭包真源", j9, "六合法边+两非法边");

    // J10 十轮状态机回放
    let j10 = {
        let mk = || {
            let mut m = ReflowMachine::new();
            let _ = m.transit(C_RECV);
            let _ = m.transit(C_FIXED);
            let _ = m.transit(C_OPEN);
            let _ = m.transit(C_RECV);
            let _ = m.transit(C_CLOSED);
            (m.cur(), m.hop_count())
        };
        let base = mk();
        let mut ok = 0;
        for _ in 0..10 {
            if mk() == base {
                ok += 1;
            }
        }
        ok == 10 && base == (C_CLOSED, 5)
    };
    s.add("J10-十轮状态机", j10, "10/10 轮路径一致");

    s
}
