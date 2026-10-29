//! UNX-F10168 时间片轮转正反双判据（RR 轮转达成与越级误用受控）（0x10820 段）
//!
//! 锚点：UNX-F10168-J1 轮转达成（同优先级任务轮转序一致 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：RR 册（预算制轮转：每任务配额 QUANTUM，tick 逐次扣减，耗尽即
//! 环转下家并复位配额）+ 轮转账本 + 折叠指纹 + 注入核（越级占跑/
//! 幽灵 tick/重号注册/空留痕/印后 tick 必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x10820 独占 0x20）。
pub const SEG_BASE: u32 = 0x10820;
pub const RR_GHOST: u32 = SEG_BASE + 0x01; // 幽灵 pid tick
pub const RR_NOT_TURN: u32 = SEG_BASE + 0x02; // 越级：非轮主占跑
pub const RR_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const RR_DUP: u32 = SEG_BASE + 0x04; // pid 重号注册
pub const RR_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍 tick

/// 时间片配额（每任务每次轮到后重置）。
pub const QUANTUM: u32 = 3;

/// RR 任务：预算、轮到次数。
#[derive(Clone)]
pub struct RrTask {
    pub pid: u32,
    pub prio: u8,
    pub budget: u32,
    pub rotations: u32,
}

/// 一次轮转的账本留痕。
#[derive(Clone)]
pub struct RotRec {
    pub seq: u32,
    pub from: u32,
    pub to: u32,
    pub at_tick: u64,
    pub note: String,
}

impl RotRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.from != self.to && self.at_tick > 0
    }

    pub fn render_line(&self) -> String {
        format!("R{}|{}->{}|t{}|{}", self.seq, self.from, self.to, self.at_tick, self.note)
    }
}

/// tick 结果：保留在轮（Kept）或耗尽轮转（Rotated）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TickOutcome {
    Kept { left: u32 },
    Rotated { from: u32, to: u32 },
}

/// RR 轮转册：任务线性表（即轮转环）+ 轮主下标。
pub struct RrSched {
    pub tasks: Vec<RrTask>,
    pub turn: usize,
    pub ticks: u64,
    pub ledger: Vec<RotRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl RrSched {
    pub fn new() -> RrSched {
        RrSched {
            tasks: Vec::new(),
            turn: 0,
            ticks: 0,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    fn idx_of(&self, pid: u32) -> Option<usize> {
        self.tasks.iter().position(|t| t.pid == pid)
    }

    /// 注册任务并配满额；首任务即轮主（重号/空留痕必拒）。
    pub fn enroll(&mut self, pid: u32, prio: u8, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(RR_FROZEN);
        }
        if note.is_empty() {
            return Err(RR_EMPTY_NOTE);
        }
        if pid == 0 || self.idx_of(pid).is_some() {
            return Err(RR_DUP);
        }
        self.tasks.push(RrTask { pid, prio, budget: QUANTUM, rotations: 0 });
        Ok(())
    }

    /// 轮主消耗一个 tick；配额耗尽即轮转下家并复位配额。
    pub fn tick(&mut self, pid: u32, note: &str) -> Result<TickOutcome, u32> {
        if self.sealed {
            return Err(RR_FROZEN);
        }
        if note.is_empty() {
            return Err(RR_EMPTY_NOTE);
        }
        if self.tasks.is_empty() {
            return Err(RR_GHOST);
        }
        let idx = match self.idx_of(pid) {
            Some(i) => i,
            None => return Err(RR_GHOST),
        };
        // 越级守卫：只有轮主可以消耗 tick。
        if idx != self.turn {
            return Err(RR_NOT_TURN);
        }
        self.ticks += 1;
        self.tasks[idx].budget -= 1;
        if self.tasks[idx].budget > 0 {
            return Ok(TickOutcome::Kept { left: self.tasks[idx].budget });
        }
        // 轮转：复位配额、推进轮主、留痕。
        let from = self.tasks[idx].pid;
        self.tasks[idx].budget = QUANTUM;
        self.tasks[idx].rotations += 1;
        self.turn = (self.turn + 1) % self.tasks.len();
        let to = self.tasks[self.turn].pid;
        self.ledger.push(RotRec {
            seq: self.seq_next,
            from,
            to,
            at_tick: self.ticks,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(TickOutcome::Rotated { from, to })
    }

    pub fn turn_pid(&self) -> Option<u32> {
        self.tasks.get(self.turn).map(|t| t.pid)
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

    /// 折叠指纹：账本 + 全体配额状态一并卷入。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10820;
        for r in &self.ledger {
            h = h.wrapping_mul(33).wrapping_add(r.seq);
            h = h.wrapping_mul(33).wrapping_add(r.from);
            h = h.wrapping_mul(33).wrapping_add(r.to);
            for by in r.note.bytes() {
                h = h.wrapping_mul(33).wrapping_add(by as u32);
            }
        }
        for t in &self.tasks {
            h = h.wrapping_mul(33).wrapping_add(t.pid);
            h = h.wrapping_mul(33).wrapping_add(t.budget);
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
    let mut b = RrSched::new();
    let _ = b.enroll(1, 1, "a");
    let _ = b.enroll(2, 1, "b");
    // 1) 越级占跑：轮主是 1，2 直接 tick 必拒。
    got[0] = b.tick(2, "inj-jump").err() == Some(RR_NOT_TURN);
    // 2) 幽灵 tick。
    got[1] = b.tick(77, "inj-ghost").err() == Some(RR_GHOST);
    // 3) 重号注册。
    got[2] = b.enroll(1, 1, "dup").err() == Some(RR_DUP);
    // 4) 空留痕 tick。
    got[3] = b.tick(1, "").err() == Some(RR_EMPTY_NOTE);
    // 5) 印账后 tick 必抓。
    b.seal();
    got[4] = b.tick(1, "inj-frozen").err() == Some(RR_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10168_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10168");

    // J1 轮转序一致：三任务同优先级满轮转一圈 1→2→3→1。
    let j1 = {
        let mut b = RrSched::new();
        let _ = b.enroll(1, 1, "a");
        let _ = b.enroll(2, 1, "b");
        let _ = b.enroll(3, 1, "c");
        // 各消耗满配额：1 耗尽→2，2 耗尽→3，3 耗尽→1。
        let mut seq: Vec<u32> = Vec::new();
        for _ in 0..(QUANTUM * 3) {
            let cur = b.turn_pid().unwrap_or(0);
            if let Ok(TickOutcome::Rotated { to, .. }) = b.tick(cur, "t") {
                seq.push(to);
            }
        }
        seq == vec![2, 3, 1]
    };
    s.add("J1-轮转序一致", j1, "三任务满圈轮转序为 2,3,1");

    // J2 配额耗尽才轮转：前两 tick Kept，第三 tick Rotated。
    let j2 = {
        let mut b = RrSched::new();
        let _ = b.enroll(9, 1, "a");
        let _ = b.enroll(8, 1, "b");
        let k1 = b.tick(9, "k1");
        let k2 = b.tick(9, "k2");
        let r = b.tick(9, "r");
        k1 == Ok(TickOutcome::Kept { left: 2 })
            && k2 == Ok(TickOutcome::Kept { left: 1 })
            && r == Ok(TickOutcome::Rotated { from: 9, to: 8 })
    };
    s.add("J2-配额语义", j2, "Kept/Kept/Rotated 三拍吻合");

    // J3 越级受控：非轮主 tick 拒且轮主不变。
    let j3 = {
        let mut b = RrSched::new();
        let _ = b.enroll(1, 1, "a");
        let _ = b.enroll(2, 1, "b");
        let before = b.turn_pid();
        let e = b.tick(2, "jump");
        before == Some(1) && e == Err(RR_NOT_TURN) && b.turn_pid() == Some(1)
    };
    s.add("J3-越级受控", j3, "越级拒后轮主位不漂移");

    // J4 轮转后配额复位：轮转后老轮主配额回满。
    let j4 = {
        let mut b = RrSched::new();
        let _ = b.enroll(1, 1, "a");
        let _ = b.enroll(2, 1, "b");
        let _ = b.tick(1, "t1");
        let _ = b.tick(1, "t2");
        let _ = b.tick(1, "t3");
        let old = b.tasks.iter().find(|t| t.pid == 1).cloned().unwrap_or(RrTask {
            pid: 0,
            prio: 0,
            budget: 0,
            rotations: 0,
        });
        old.budget == QUANTUM && old.rotations == 1
    };
    s.add("J4-配额复位", j4, "轮转后配额回满且记一轮");

    // J5 幽灵 tick 拒。
    let j5 = {
        let mut b = RrSched::new();
        b.tick(404, "ghost").err() == Some(RR_GHOST)
    };
    s.add("J5-幽灵拒", j5, "未注册 pid tick 必拒");

    // J6 重号注册拒。
    let j6 = {
        let mut b = RrSched::new();
        let _ = b.enroll(3, 1, "a");
        b.enroll(3, 1, "dup").err() == Some(RR_DUP)
    };
    s.add("J6-重号拒", j6, "同 pid 二次注册必拒");

    // J7 账本完整：留痕合法、seq 唯一、tick 数与轮转数守恒。
    let j7 = {
        let mut b = RrSched::new();
        let _ = b.enroll(1, 1, "a");
        let _ = b.enroll(2, 1, "b");
        for _ in 0..(QUANTUM * 2) {
            let cur = b.turn_pid().unwrap_or(0);
            let _ = b.tick(cur, "t");
        }
        b.ledger_ok()
            && b.seq_unique()
            && b.ledger.len() == 2
            && b.ticks == (QUANTUM as u64) * 2
    };
    s.add("J7-账本守恒", j7, "轮转账与 tick 总数守恒");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = RrSched::new();
        let _ = b.enroll(1, 1, "a");
        let _ = b.enroll(2, 1, "b");
        let _ = b.tick(1, "t");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.tick(1, "post").err() == Some(RR_FROZEN)
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
    fn unx10168_rr_flow() {
        let mut b = RrSched::new();
        let _ = b.enroll(1, 1, "a");
        let _ = b.enroll(2, 1, "b");
        let _ = b.tick(1, "t1");
        let _ = b.tick(1, "t2");
        let r = b.tick(1, "t3").unwrap();
        assert_eq!(r, TickOutcome::Rotated { from: 1, to: 2 });
        assert_eq!(b.turn_pid(), Some(2));
        assert_eq!(b.tick(1, "jump"), Err(RR_NOT_TURN));
    }

    #[test]
    fn unx10168_checks_green() {
        let s = run_unx10168_checks();
        assert!(s.all_passed(), "UNX-F10168 判据未全绿");
    }
}
