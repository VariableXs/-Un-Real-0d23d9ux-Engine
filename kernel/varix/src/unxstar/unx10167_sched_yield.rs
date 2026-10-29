//! UNX-F10167 sched_yield 正反双判据（让渡达成与单核语义误用受控）（0x10800 段）
//!
//! 锚点：UNX-F10167-J1 让渡达成（有兄弟任务时切换生效 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：任务册（就绪环 + 当前运行槽）+ 让渡核（yield 单核语义：
//! 有兄弟可切才切换，独占为受控 no-op）+ 切换账本（逐次留痕）+
//! 折叠指纹 + 注入核（幽灵 pid/非运行态让渡/空留痕/重号注册/印后让渡）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10800 独占 0x20）。
pub const SEG_BASE: u32 = 0x10800;
pub const YLD_GHOST: u32 = SEG_BASE + 0x01; // 幽灵 pid：任务册中不存在
pub const YLD_NOT_RUNNING: u32 = SEG_BASE + 0x02; // 非运行态让渡
pub const YLD_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 让渡留痕为空
pub const YLD_DUP_READY: u32 = SEG_BASE + 0x04; // pid 重号注册
pub const YLD_FROZEN: u32 = SEG_BASE + 0x05; // 册封账后仍让渡

/// 任务态闭集（显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskState {
    Ready = 1,
    Running = 2,
    Dead = 3,
}

/// 单任务描述：pid、静态优先级、状态与累计让渡次数。
#[derive(Clone)]
pub struct Task {
    pub pid: u32,
    pub prio: u8,
    pub state: TaskState,
    pub yields: u32,
}

/// 让渡结果：单核语义下要么切走（Switched）要么独占（Alone）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum YieldOutcome {
    Switched { from: u32, to: u32 },
    Alone { holder: u32 },
}

/// 一次让渡的账本留痕。
#[derive(Clone)]
pub struct YldRec {
    pub seq: u32,
    pub from: u32,
    pub to: u32,
    pub switched: bool,
    pub note: String,
}

impl YldRec {
    /// 账目合法性：留痕非空且切换时两端不得同号。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (!self.switched || self.from != self.to)
    }

    pub fn render_line(&self) -> String {
        format!(
            "Y{}|{}->{}|sw{}|{}",
            self.seq, self.from, self.to, self.switched as u8, self.note
        )
    }
}

/// 让渡调度册：就绪环（pid 序）+ 当前运行槽 + 账本。
pub struct YieldSched {
    pub tasks: Vec<Task>,
    pub ring: Vec<u32>,
    pub current: Option<u32>,
    pub ledger: Vec<YldRec>,
    pub seq_next: u32,
    pub switches: u32,
    pub sealed: bool,
}

impl YieldSched {
    pub fn new() -> YieldSched {
        YieldSched {
            tasks: Vec::new(),
            ring: Vec::new(),
            current: None,
            ledger: Vec::new(),
            seq_next: 1,
            switches: 0,
            sealed: false,
        }
    }

    fn idx_of(&self, pid: u32) -> Option<usize> {
        self.tasks.iter().position(|t| t.pid == pid)
    }

    /// 注册任务并进入就绪环；首个任务直接上位运行（pid=0/重号/空留痕必拒）。
    pub fn spawn(&mut self, pid: u32, prio: u8, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(YLD_FROZEN);
        }
        if note.is_empty() {
            return Err(YLD_EMPTY_NOTE);
        }
        if pid == 0 || self.idx_of(pid).is_some() {
            return Err(YLD_DUP_READY);
        }
        self.tasks.push(Task { pid, prio, state: TaskState::Ready, yields: 0 });
        self.ring.push(pid);
        if self.current.is_none() {
            let head = self.ring.remove(0);
            self.current = Some(head);
            if let Some(i) = self.idx_of(head) {
                self.tasks[i].state = TaskState::Running;
            }
        }
        Ok(())
    }

    /// sched_yield 核心：就绪环里除自身外尚有可切者才轮转，否则受控 no-op。
    pub fn yield_cpu(&mut self, pid: u32, note: &str) -> Result<YieldOutcome, u32> {
        if self.sealed {
            return Err(YLD_FROZEN);
        }
        if note.is_empty() {
            return Err(YLD_EMPTY_NOTE);
        }
        let i = match self.idx_of(pid) {
            Some(i) => i,
            None => return Err(YLD_GHOST),
        };
        if self.tasks[i].state != TaskState::Running {
            return Err(YLD_NOT_RUNNING);
        }
        // 单核让渡判据：环中是否还有异于自身的就绪任务。
        let sibling = self.ring.iter().any(|&q| q != pid);
        let out = if sibling {
            self.ring.push(pid);
            let to = self.ring.remove(0);
            if let Some(j) = self.idx_of(to) {
                self.tasks[j].state = TaskState::Running;
            }
            if let Some(k) = self.idx_of(pid) {
                self.tasks[k].state = TaskState::Ready;
                self.tasks[k].yields += 1;
            }
            self.current = Some(to);
            self.switches += 1;
            YieldOutcome::Switched { from: pid, to }
        } else {
            self.tasks[i].yields += 1;
            YieldOutcome::Alone { holder: pid }
        };
        let to_pid = match out {
            YieldOutcome::Switched { to, .. } => to,
            YieldOutcome::Alone { holder } => holder,
        };
        self.ledger.push(YldRec {
            seq: self.seq_next,
            from: pid,
            to: to_pid,
            switched: sibling,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(out)
    }

    pub fn of(&self, pid: u32) -> Option<Task> {
        self.tasks.iter().find(|t| t.pid == pid).cloned()
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    /// seq 全局唯一（无重号）。
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

    /// 折叠指纹：切换账逐条卷入（31 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10800;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.from);
            h = h.wrapping_mul(31).wrapping_add(r.to);
            h = h.wrapping_mul(31).wrapping_add(r.switched as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
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
    // 1) 幽灵 pid 让渡
    let mut b = YieldSched::new();
    got[0] = b.yield_cpu(99, "inj-ghost").err() == Some(YLD_GHOST);
    // 2) 非运行态让渡（后注册者仍处 Ready 态）
    let _ = b.spawn(1, 1, "t1");
    let _ = b.spawn(2, 1, "t2");
    got[1] = b.yield_cpu(2, "inj-notrun").err() == Some(YLD_NOT_RUNNING);
    // 3) 空留痕注册
    got[2] = b.spawn(3, 1, "").err() == Some(YLD_EMPTY_NOTE);
    // 4) 重号注册
    got[3] = b.spawn(1, 1, "dup").err() == Some(YLD_DUP_READY);
    // 5) 印账后仍让渡必抓
    b.seal();
    got[4] = b.yield_cpu(1, "inj-frozen").err() == Some(YLD_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10167_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10167");

    // J1 让渡达成：双任务互相让渡双向生效。
    let j1 = {
        let mut b = YieldSched::new();
        let _ = b.spawn(10, 1, "a");
        let _ = b.spawn(11, 1, "b");
        let o1 = b.yield_cpu(10, "y1");
        let o2 = b.yield_cpu(11, "y2");
        o1 == Ok(YieldOutcome::Switched { from: 10, to: 11 })
            && o2 == Ok(YieldOutcome::Switched { from: 11, to: 10 })
    };
    s.add("J1-让渡达成", j1, "有兄弟任务时切换双向生效");

    // J2 独占受控：单任务让渡为 no-op，不计数切换。
    let j2 = {
        let mut b = YieldSched::new();
        let _ = b.spawn(5, 1, "solo");
        let o = b.yield_cpu(5, "y");
        o == Ok(YieldOutcome::Alone { holder: 5 }) && b.switches == 0
    };
    s.add("J2-独占受控", j2, "无兄弟时让渡不切换");

    // J3 轮转序一致：三任务 1→2→3→1 循环。
    let j3 = {
        let mut b = YieldSched::new();
        let _ = b.spawn(1, 1, "a");
        let _ = b.spawn(2, 1, "b");
        let _ = b.spawn(3, 1, "c");
        let r1 = b.yield_cpu(1, "r1");
        let r2 = b.yield_cpu(2, "r2");
        let r3 = b.yield_cpu(3, "r3");
        r1 == Ok(YieldOutcome::Switched { from: 1, to: 2 })
            && r2 == Ok(YieldOutcome::Switched { from: 2, to: 3 })
            && r3 == Ok(YieldOutcome::Switched { from: 3, to: 1 })
    };
    s.add("J3-轮转序一致", j3, "环首出队语义三跳归位");

    // J4 让渡计数：谁让渡记谁，受控 no-op 也计。
    let j4 = {
        let mut b = YieldSched::new();
        let _ = b.spawn(7, 1, "a");
        let _ = b.spawn(8, 1, "b");
        let _ = b.yield_cpu(7, "y1");
        let _ = b.yield_cpu(8, "y2");
        let _ = b.yield_cpu(7, "y3");
        let ta = b.of(7).unwrap_or(Task { pid: 0, prio: 0, state: TaskState::Dead, yields: 0 });
        let tb = b.of(8).unwrap_or(Task { pid: 0, prio: 0, state: TaskState::Dead, yields: 0 });
        ta.yields == 2 && tb.yields == 1
    };
    s.add("J4-让渡计数", j4, "a 两次 b 一次计数分明");

    // J5 幽灵让渡拒。
    let j5 = {
        let mut b = YieldSched::new();
        b.yield_cpu(42, "ghost").err() == Some(YLD_GHOST)
    };
    s.add("J5-幽灵拒", j5, "未注册 pid 让渡必拒");

    // J6 非运行态让渡拒。
    let j6 = {
        let mut b = YieldSched::new();
        let _ = b.spawn(1, 1, "a");
        let _ = b.spawn(2, 1, "b");
        b.yield_cpu(2, "notrun").err() == Some(YLD_NOT_RUNNING)
    };
    s.add("J6-态守恒", j6, "Ready 任务无权让渡");

    // J7 账本完整：留痕合法且 seq 唯一。
    let j7 = {
        let mut b = YieldSched::new();
        let _ = b.spawn(1, 1, "a");
        let _ = b.spawn(2, 1, "b");
        let _ = b.yield_cpu(1, "l1");
        let _ = b.yield_cpu(2, "l2");
        b.ledger_ok() && b.seq_unique() && b.ledger.len() == 2
    };
    s.add("J7-账本完整", j7, "逐次留痕合法无重号");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = YieldSched::new();
        let _ = b.spawn(1, 1, "a");
        let _ = b.spawn(2, 1, "b");
        let _ = b.yield_cpu(1, "f1");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.yield_cpu(2, "post").err() == Some(YLD_FROZEN)
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
    fn unx10167_yield_flow() {
        let mut b = YieldSched::new();
        let _ = b.spawn(1, 1, "a");
        let _ = b.spawn(2, 1, "b");
        let o = b.yield_cpu(1, "y").unwrap();
        assert_eq!(o, YieldOutcome::Switched { from: 1, to: 2 });
        assert_eq!(b.current, Some(2));
        assert_eq!(b.of(1).unwrap().state, TaskState::Ready);
        assert_eq!(b.yield_cpu(9, "g"), Err(YLD_GHOST));
    }

    #[test]
    fn unx10167_checks_green() {
        let s = run_unx10167_checks();
        assert!(s.all_passed(), "UNX-F10167 判据未全绿");
    }
}
