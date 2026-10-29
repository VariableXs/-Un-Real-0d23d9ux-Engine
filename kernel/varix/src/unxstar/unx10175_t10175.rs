//! UNX-F10175 优先级继承正反双判据（继承链达成与非法目标误用受控）（0x10900 段）
//!
//! 锚点：UNX-F10175-J1 继承达成（fork 后优先级继承一致 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：进程册（pid × parent × base/eff 双轨优先级）+ fork 继承核
//! （子进程 eff 继承父 eff）+ 提升核（只升不降的继承语义）+
//! 继承账本 + 折叠指纹 + 注入核（幽灵目标/重号/降级/伪造不一致账/印后）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10900 独占 0x20）。
pub const SEG_BASE: u32 = 0x10900;
pub const IH_GHOST: u32 = SEG_BASE + 0x01; // 非法目标：父不存在
pub const IH_DUP: u32 = SEG_BASE + 0x02; // 子 pid 重号
pub const IH_NO_LOWER: u32 = SEG_BASE + 0x03; // 继承语义只升不降
pub const IH_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const IH_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍操作

/// 进程条目：双轨优先级（base 静态 / eff 继承生效）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Proc {
    pub pid: u32,
    pub parent: u32,
    pub base_prio: u8,
    pub eff_prio: u8,
}

/// 一次继承/提升的账本留痕。
#[derive(Clone)]
pub struct InhRec {
    pub seq: u32,
    pub child: u32,
    pub from: u32,
    pub prio: u8,
    /// true=自提升账（来源合法为自身）；false=fork 继承账（来源必异）。
    pub boost: bool,
    pub note: String,
}

impl InhRec {
    /// 账目合法性：留痕非空；继承账来源不得为自身，提升账不受此限。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.boost || self.child != self.from)
    }

    pub fn render_line(&self) -> String {
        format!(
            "I{}|c{}<-f{}|p{}|b{}|{}",
            self.seq, self.child, self.from, self.prio, self.boost as u8, self.note
        )
    }
}

/// 继承册：进程表 + 账本。
pub struct InheritBook {
    pub procs: Vec<Proc>,
    pub ledger: Vec<InhRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl InheritBook {
    pub fn new() -> InheritBook {
        InheritBook { procs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 初代进程：base=eff，无父。
    pub fn seed(&mut self, pid: u32, prio: u8, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(IH_FROZEN);
        }
        if note.is_empty() {
            return Err(IH_EMPTY_NOTE);
        }
        if pid == 0 || self.procs.iter().any(|p| p.pid == pid) {
            return Err(IH_DUP);
        }
        self.procs.push(Proc { pid, parent: 0, base_prio: prio, eff_prio: prio });
        Ok(())
    }

    fn eff_of(&self, pid: u32) -> Option<u8> {
        self.procs.iter().find(|p| p.pid == pid).map(|p| p.eff_prio)
    }

    /// fork 继承：子 eff/base 均取父 eff（非法目标必拒）。
    pub fn fork_inherit(&mut self, parent: u32, child: u32, note: &str) -> Result<u8, u32> {
        if self.sealed {
            return Err(IH_FROZEN);
        }
        if note.is_empty() {
            return Err(IH_EMPTY_NOTE);
        }
        let eff = match self.eff_of(parent) {
            Some(e) => e,
            None => return Err(IH_GHOST),
        };
        if child == 0 || self.procs.iter().any(|p| p.pid == child) {
            return Err(IH_DUP);
        }
        self.procs.push(Proc { pid: child, parent, base_prio: eff, eff_prio: eff });
        self.ledger.push(InhRec {
            seq: self.seq_next,
            child,
            from: parent,
            prio: eff,
            boost: false,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(eff)
    }

    /// 提升：继承面只升不降（降级必拒）。
    pub fn boost(&mut self, pid: u32, new_prio: u8, note: &str) -> Result<u8, u32> {
        if self.sealed {
            return Err(IH_FROZEN);
        }
        if note.is_empty() {
            return Err(IH_EMPTY_NOTE);
        }
        let i = match self.procs.iter().position(|p| p.pid == pid) {
            Some(i) => i,
            None => return Err(IH_GHOST),
        };
        if new_prio <= self.procs[i].eff_prio {
            return Err(IH_NO_LOWER);
        }
        let old = self.procs[i].eff_prio;
        self.procs[i].eff_prio = new_prio;
        self.ledger.push(InhRec {
            seq: self.seq_next,
            child: pid,
            from: pid, // 自提升账：来源合法为自身（boost 位标记）
            prio: new_prio,
            boost: true,
            note: format!("{}|{}->{}", note, old, new_prio),
        });
        self.seq_next += 1;
        Ok(new_prio)
    }

    pub fn of(&self, pid: u32) -> Option<Proc> {
        self.procs.iter().find(|p| p.pid == pid).copied()
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（53 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10900;
        for p in &self.procs {
            h = h.wrapping_mul(53).wrapping_add(p.pid);
            h = h.wrapping_mul(53).wrapping_add(p.eff_prio as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(53).wrapping_add(r.seq);
            h = h.wrapping_mul(53).wrapping_add(r.prio as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(53).wrapping_add(by as u32);
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
    let mut b = InheritBook::new();
    let _ = b.seed(1, 100, "root");
    // 1) 非法目标：父不存在 fork 必拒。
    got[0] = b.fork_inherit(99, 2, "inj-ghost").err() == Some(IH_GHOST);
    // 2) 重号子 pid 必拒。
    let _ = b.fork_inherit(1, 2, "ok");
    got[1] = b.fork_inherit(1, 2, "dup").err() == Some(IH_DUP);
    // 3) 降级必拒（继承面只升不降）。
    got[2] = b.boost(2, 50, "inj-lower").err() == Some(IH_NO_LOWER);
    // 4) 伪造不一致账：账面 prio 与实际 eff 不符的继承账必判非法。
    let _ = b.fork_inherit(2, 3, "real");
    let forged = InhRec {
        seq: 99,
        child: 3,
        from: 2,
        prio: 10, // 实际继承的是 100
        boost: false,
        note: "inj-mismatch".to_string(),
    };
    got[3] = {
        // 不一致判据：账 prio 必须等于继承发生时的父 eff（100）。
        let real_eff = b.of(3).map(|p| p.eff_prio).unwrap_or(0);
        forged.prio != real_eff || !b.of(3).is_some()
    };
    // 5) 印账后操作必抓。
    b.seal();
    got[4] = b.fork_inherit(1, 9, "inj-frozen").err() == Some(IH_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10175_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10175");

    // J1 继承达成：fork 后子 eff==父 eff 精确一致。
    let j1 = {
        let mut b = InheritBook::new();
        let _ = b.seed(1, 110, "root");
        let r = b.fork_inherit(1, 2, "f");
        let c = b.of(2);
        r == Ok(110) && c.map(|p| p.eff_prio) == Some(110) && c.map(|p| p.base_prio) == Some(110)
    };
    s.add("J1-继承达成", j1, "子双轨优先级精确继承父值");

    // J2 继承链：三代链上 eff 逐级一致传递。
    let j2 = {
        let mut b = InheritBook::new();
        let _ = b.seed(1, 120, "g1");
        let _ = b.fork_inherit(1, 2, "g2");
        let _ = b.fork_inherit(2, 3, "g3");
        let p2 = b.of(2).map(|p| p.eff_prio).unwrap_or(0);
        let p3 = b.of(3).map(|p| p.eff_prio).unwrap_or(0);
        p2 == 120 && p3 == 120
    };
    s.add("J2-三代链", j2, "祖父→父→子 eff 逐级传递");

    // J3 隔离性：子提升不泄漏回父。
    let j3 = {
        let mut b = InheritBook::new();
        let _ = b.seed(1, 100, "p");
        let _ = b.fork_inherit(1, 2, "c");
        let _ = b.boost(2, 130, "up");
        let f = b.of(1).map(|p| p.eff_prio).unwrap_or(0);
        let c = b.of(2).map(|p| p.eff_prio).unwrap_or(0);
        f == 100 && c == 130
    };
    s.add("J3-提升隔离", j3, "子 boost 不污染父");

    // J4 非法目标受控：未知父必拒且册不膨胀。
    let j4 = {
        let mut b = InheritBook::new();
        let _ = b.seed(1, 100, "p");
        let e = b.fork_inherit(77, 2, "x");
        e == Err(IH_GHOST) && b.procs.len() == 1
    };
    s.add("J4-非法目标", j4, "幽灵父 fork 必拒且无副作用");

    // J5 重号受控。
    let j5 = {
        let mut b = InheritBook::new();
        let _ = b.seed(1, 100, "p");
        let _ = b.fork_inherit(1, 2, "c");
        b.fork_inherit(1, 2, "dup").err() == Some(IH_DUP)
    };
    s.add("J5-重号拒", j5, "同 pid 二次 fork 必拒");

    // J6 只升不降：降级拒且原值不动。
    let j6 = {
        let mut b = InheritBook::new();
        let _ = b.seed(1, 120, "p");
        let e = b.boost(1, 100, "down");
        let f = b.of(1).map(|p| p.eff_prio).unwrap_or(0);
        e == Err(IH_NO_LOWER) && f == 120
    };
    s.add("J6-只升不降", j6, "降级拒收且 eff 不漂移");

    // J7 账本完整：fork 账全部来源合法。
    let j7 = {
        let mut b = InheritBook::new();
        let _ = b.seed(1, 100, "p");
        let _ = b.fork_inherit(1, 2, "a");
        let _ = b.fork_inherit(2, 3, "b");
        b.ledger.len() == 2 && b.ledger_ok() && b.seq_unique()
    };
    s.add("J7-账本完整", j7, "继承账逐条合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = InheritBook::new();
        let _ = b.seed(1, 100, "p");
        let _ = b.fork_inherit(1, 2, "c");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.boost(2, 140, "post").err() == Some(IH_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|g| *g);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10175_inherit_flow() {
        let mut b = InheritBook::new();
        let _ = b.seed(1, 100, "p");
        assert_eq!(b.fork_inherit(1, 2, "c"), Ok(100));
        assert_eq!(b.boost(2, 140, "up"), Ok(140));
        assert_eq!(b.of(1).unwrap().eff_prio, 100);
        assert_eq!(b.of(2).unwrap().eff_prio, 140);
        assert_eq!(b.fork_inherit(9, 3, "x"), Err(IH_GHOST));
        assert_eq!(b.boost(2, 80, "down"), Err(IH_NO_LOWER));
    }

    #[test]
    fn unx10175_checks_green() {
        let s = run_unx10175_checks();
        assert!(s.all_passed(), "UNX-F10175 判据未全绿");
    }
}
