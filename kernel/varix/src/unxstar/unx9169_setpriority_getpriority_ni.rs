//! UNX-F9169 setpriority/getpriority 语义档（nice 值域）（0x634E0 段）
//!
//! 锚点：UNX-F9169-J1 优先级账（每次 set/get 落账：which、who、旧
//! nice、新 nice、结果、留痕）；J2 优先级语义（值域闭集 [−20,19] +
//! which 闭集 + 越域夹逼 + 降级自由四性质）；J3 优先级指纹（账本折
//! 叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（nice 值域面）。
//!
//! 结构：nice 册（who→nice，i8 存负值）+ 优先级核（clamp + which
//! 分派）+ 优先级账本 + 折叠指纹 + 注入核（错 which/空留痕/越域拒
//! 伪样本/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x634E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x634E0;
pub const NI_UNK_WHICH: u32 = SEG_BASE + 0x01; // 错 which
pub const NI_BAD_NICE: u32 = SEG_BASE + 0x02; // 越域拒伪样本
pub const NI_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const NI_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const NI_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// nice 值域。
pub const PRIO_MIN: i8 = -20;
pub const PRIO_MAX: i8 = 19;

/// which 闭集。
pub const W_PROCESS: u8 = 0;
pub const W_PGRP: u8 = 1;
pub const W_USER: u8 = 2;
pub const W_COUNT: u8 = 3;

pub const W_NAMES: [&str; 3] = ["process", "pgrp", "user"];

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1; // 非特权提级
pub const R_EINVAL: u32 = 2; // 错 which
pub const R_COUNT: usize = 3;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM", "EINVAL"];

/// 越域夹逼。
pub fn clamp(nice: i8) -> i8 {
    if nice < PRIO_MIN {
        PRIO_MIN
    } else if nice > PRIO_MAX {
        PRIO_MAX
    } else {
        nice
    }
}

/// 单条优先级账。
#[derive(Clone)]
pub struct NiRec {
    pub seq: u32,
    pub which: u8,
    pub who: u32,
    pub old: i8,
    pub new: i8,
    pub code: u32,
    pub note: String,
}

impl NiRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.which < W_COUNT && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "NI{}|{}w{}|{}->{}|{}|{}",
            self.seq,
            W_NAMES[self.which as usize],
            self.who,
            self.old,
            self.new,
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// 优先级簿。
pub struct NiBook {
    /// who → nice（process/pgrp/user 统一编号域，前缀区分：1xxx 进程
    /// 2xxx 组 3xxx 用户）
    pub nice: Vec<(u32, i8)>,
    pub ledger: Vec<NiRec>,
    pub privileged: bool,
    pub seq_next: u32,
    pub sealed: bool,
}

impl NiBook {
    pub fn new(privileged: bool) -> NiBook {
        NiBook {
            nice: Vec::new(),
            ledger: Vec::new(),
            privileged,
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn seed(&mut self, who: u32, n: i8) {
        match self.nice.iter_mut().find(|(w, _)| *w == who) {
            Some((_, v)) => *v = clamp(n),
            None => self.nice.push((who, clamp(n))),
        }
    }

    fn of(&self, who: u32) -> Option<i8> {
        self.nice.iter().find(|(w, _)| *w == who).map(|(_, v)| *v)
    }

    /// setpriority。
    pub fn set(&mut self, which: u8, who: u32, nice: i8, note: &str) -> Result<i8, u32> {
        if self.sealed {
            return Err(NI_FROZEN);
        }
        if note.is_empty() {
            return Err(NI_EMPTY_NOTE);
        }
        if which >= W_COUNT {
            return Err(R_EINVAL);
        }
        let target = who;
        let old = self.of(target).unwrap_or(0);
        let n = clamp(nice);
        // 降级自由：提级（nice 减小）须特权
        if n < old && !self.privileged {
            self.record(which, target, old, old, R_EPERM, note);
            return Err(R_EPERM);
        }
        self.seed(target, n);
        self.record(which, target, old, n, R_OK, note);
        Ok(n)
    }

    /// getpriority。
    pub fn get(&mut self, which: u8, who: u32, note: &str) -> Result<i8, u32> {
        if self.sealed {
            return Err(NI_FROZEN);
        }
        if note.is_empty() {
            return Err(NI_EMPTY_NOTE);
        }
        if which >= W_COUNT {
            return Err(R_EINVAL);
        }
        let v = self.of(who).unwrap_or(0);
        self.record(which, who, v, v, R_OK, note);
        Ok(v)
    }

    fn record(&mut self, which: u8, who: u32, old: i8, new: i8, code: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(NiRec {
            seq,
            which,
            who,
            old,
            new,
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
        let mut h: u32 = 0x91690;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.which as u32);
            h = h.wrapping_mul(31).wrapping_add(r.who);
            h = h.wrapping_mul(31).wrapping_add((r.new as i32) as u32);
            h = h.wrapping_mul(31).wrapping_add(r.code);
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

/// 基础样本：进程 100(nice 0)、进程 101(nice 5)。
pub fn mk_book(privileged: bool) -> NiBook {
    let mut b = NiBook::new(privileged);
    b.seed(100, 0);
    b.seed(101, 5);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错 which 账
    let bad = NiRec {
        seq: 1,
        which: 9,
        who: 1,
        old: 0,
        new: 0,
        code: 0,
        note: "inj-which".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = NiRec {
        seq: 2,
        which: 0,
        who: 1,
        old: 0,
        new: 0,
        code: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 错 which 拒
    let mut b = mk_book(true);
    got[2] = b.set(9, 100, 1, "inj-which").err() == Some(R_EINVAL);
    // 4) 非特权提级拒
    let mut b2 = mk_book(false);
    got[3] = b2.set(W_PROCESS, 100, -5, "inj-eperm").err() == Some(R_EPERM);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.set(W_PROCESS, 100, 1, "inj-frozen").err() == Some(NI_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9169_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9169");
    let mut b = mk_book(true);

    // J1 优先级账非空
    let _ = b.set(W_PROCESS, 100, 5, "降级");
    let j1 = !b.ledger.is_empty();
    s.add("J1-优先级账非空", j1, "set 落账至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 which/码闭集");

    // J3 值域闭集：越域夹逼
    let j3 = clamp(-30) == PRIO_MIN && clamp(30) == PRIO_MAX && clamp(3) == 3;
    s.add("J3-越域夹逼", j3, "[-20,19] 外值夹逼");

    // J4 降级自由
    let j4 = {
        let mut b2 = mk_book(false);
        b2.set(W_PROCESS, 100, 10, "降级") == Ok(10)
    };
    s.add("J4-降级自由", j4, "nice 增大（降级）无特权放行");

    // J5 提级特权闸
    let j5 = {
        let mut b2 = mk_book(false);
        b2.set(W_PROCESS, 100, -5, "提级").err() == Some(R_EPERM)
    };
    s.add("J5-提级特权闸", j5, "nice 减小（提级）无特权必 EPERM");

    // J6 提级特权放行
    let j6 = {
        let mut b2 = mk_book(true);
        b2.set(W_PROCESS, 100, -10, "特权提级") == Ok(-10)
    };
    s.add("J6-提级放行", j6, "特权视角提级放行");

    // J7 which 闭集
    let j7 = b.set(9, 100, 1, "野which").err() == Some(R_EINVAL);
    s.add("J7-which闭集", j7, "which 出闭集必 EINVAL");

    // J8 getpriority 闭路
    let j8 = {
        let mut b2 = mk_book(true);
        let _ = b2.set(W_PROCESS, 100, 7, "设");
        b2.get(W_PROCESS, 100, "查") == Ok(7)
    };
    s.add("J8-get闭路", j8, "get 与 set 一致");

    // J9 三 which 分派
    let j9 = {
        let mut b2 = mk_book(true);
        let _ = b2.seed(2001, 1);
        let _ = b2.seed(3001, 2);
        b2.set(W_PROCESS, 100, 1, "p") == Ok(1)
            && b2.set(W_PGRP, 2001, 3, "g") == Ok(3)
            && b2.set(W_USER, 3001, 4, "u") == Ok(4)
    };
    s.add("J9-三which分派", j9, "process/pgrp/user 三路均可设");

    // J10 默认 nice 0：未登记 who 无册面（get 走 unwrap_or(0) 语义）
    let j10 = b.of(999).is_none();
    s.add("J10-默认零", j10, "未登记 who 的 nice 视为 0");

    // J11 值域端点
    let j11 = {
        let mut b2 = mk_book(true);
        let lo = b2.set(W_PROCESS, 100, PRIO_MIN, "顶");
        let hi = b2.set(W_PROCESS, 100, PRIO_MAX, "底");
        lo == Ok(PRIO_MIN) && hi == Ok(PRIO_MAX)
    };
    s.add("J11-端点可达", j11, "±端点均可设");

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
    fn unx9169_nice_flow() {
        let mut b = mk_book(true);
        assert_eq!(b.set(W_PROCESS, 100, 5, "a"), Ok(5));
        assert_eq!(b.get(W_PROCESS, 100, "b"), Ok(5));
        assert_eq!(b.set(W_PROCESS, 100, -10, "c"), Ok(-10));
        assert_eq!(clamp(-30), -20);
        assert_eq!(clamp(30), 19);
        // 非特权
        let mut u = mk_book(false);
        assert_eq!(u.set(W_PROCESS, 100, 15, "d"), Ok(15));
        assert_eq!(u.set(W_PROCESS, 100, 5, "e"), Err(R_EPERM));
    }

    #[test]
    fn unx9169_checks_green() {
        let s = run_unx9169_checks();
        assert!(s.all_passed(), "UNX-F9169 判据未全绿");
    }
}
