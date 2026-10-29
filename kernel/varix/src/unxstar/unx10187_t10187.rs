//! UNX-F10187 孤儿/僵尸正反双判据（收养回收达成与失管误用受控）（0x10a80 段）
//!
//! 锚点：UNX-F10187-J1 收养达成（父先亡子被 init 收养后可回收 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：进程树（pid × parent × 状态 Running/Zombie/Dead）+ 退出核
//! （Running→Zombie）+ 死亡核（父亡→子改认 init）+ 收割核（亲父或
//! init 才可收）+ 账本 + 注入核（非亲收割/收养目标非 init/
//! 活体收割/空留痕/印后操作）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x10a80 独占 0x20）。
pub const SEG_BASE: u32 = 0x10a80;
pub const OZ_ECHILD: u32 = SEG_BASE + 0x01; // 非亲收割/无此子
pub const OZ_LIVE: u32 = SEG_BASE + 0x02; // 活体收割
pub const OZ_BAD_ADOPT: u32 = SEG_BASE + 0x03; // 收养目标非 init
pub const OZ_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const OZ_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍操作

/// init 进程号（收养者恒为 1）。
pub const INIT_PID: u32 = 1;

/// 进程状态闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PStat {
    Running = 1,
    Zombie = 2,
    Dead = 3,
}

/// 进程条目。
#[derive(Clone, Copy)]
pub struct Pr {
    pub pid: u32,
    pub parent: u32,
    pub state: PStat,
}

/// 一次收养/收割的账本留痕。
#[derive(Clone)]
pub struct OzRec {
    pub seq: u32,
    pub pid: u32,
    /// true=收养 / false=收割。
    pub adopt: bool,
    /// 收养新父或收割者。
    pub who: u32,
    pub note: String,
}

impl OzRec {
    /// 账目合法性：留痕非空；收养账新父恒为 init。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (!self.adopt || self.who == INIT_PID)
    }

    pub fn render_line(&self) -> String {
        format!(
            "A{}|p{}|{}|w{}|{}",
            self.seq,
            self.pid,
            if self.adopt { "ad" } else { "rp" },
            self.who,
            self.note
        )
    }
}

/// 进程树：注册/退出/死亡收养/收割。
pub struct ProcTree {
    pub procs: Vec<Pr>,
    pub ledger: Vec<OzRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ProcTree {
    pub fn new() -> ProcTree {
        // init 常驻。
        ProcTree {
            procs: vec![Pr { pid: INIT_PID, parent: 0, state: PStat::Running }],
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// fork：登记子进程（重号必拒；父必须存活在册）。
    pub fn fork_child(&mut self, parent: u32, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(OZ_FROZEN);
        }
        if pid == 0 || self.procs.iter().any(|p| p.pid == pid) {
            return Err(OZ_ECHILD);
        }
        match self.procs.iter().find(|p| p.pid == parent) {
            Some(p) if p.state == PStat::Running => {}
            _ => return Err(OZ_ECHILD),
        }
        self.procs.push(Pr { pid, parent, state: PStat::Running });
        Ok(())
    }

    /// 退出：Running→Zombie（重复退出按 ECHILD 拒）。
    pub fn exit_child(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(OZ_FROZEN);
        }
        match self.procs.iter_mut().find(|p| p.pid == pid) {
            Some(p) if p.state == PStat::Running => {
                p.state = PStat::Zombie;
                Ok(())
            }
            _ => Err(OZ_ECHILD),
        }
    }

    /// 死亡核：父亡→全部 Running/Zombie 子改认 init（收养账留痕）。
    pub fn die(&mut self, parent: u32, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(OZ_FROZEN);
        }
        if note.is_empty() {
            return Err(OZ_EMPTY_NOTE);
        }
        if parent == INIT_PID {
            return Err(OZ_BAD_ADOPT); // init 不亡
        }
        let n = self
            .procs
            .iter()
            .filter(|p| p.parent == parent && p.state != PStat::Dead)
            .count();
        for p in &mut self.procs {
            if p.parent == parent && p.state != PStat::Dead {
                p.parent = INIT_PID;
            }
        }
        if let Some(p) = self.procs.iter_mut().find(|p| p.pid == parent) {
            p.state = PStat::Dead;
        }
        if n > 0 {
            self.ledger.push(OzRec {
                seq: self.seq_next,
                pid: parent,
                adopt: true,
                who: INIT_PID,
                note: note.to_string(),
            });
            self.seq_next += 1;
        }
        Ok(n)
    }

    /// 收割核：Zombie 态且收割者为当前亲父才可收；活体/非亲必拒。
    pub fn reap(&mut self, reaper: u32, pid: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(OZ_FROZEN);
        }
        if note.is_empty() {
            return Err(OZ_EMPTY_NOTE);
        }
        let i = match self.procs.iter().position(|p| p.pid == pid) {
            Some(i) => i,
            None => return Err(OZ_ECHILD),
        };
        if self.procs[i].state == PStat::Running {
            return Err(OZ_LIVE);
        }
        if self.procs[i].state == PStat::Dead || self.procs[i].parent != reaper {
            return Err(OZ_ECHILD);
        }
        self.procs.remove(i);
        self.ledger.push(OzRec {
            seq: self.seq_next,
            pid,
            adopt: false,
            who: reaper,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(())
    }

    pub fn parent_of(&self, pid: u32) -> Option<u32> {
        self.procs.iter().find(|p| p.pid == pid).map(|p| p.parent)
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

    /// 折叠指纹（107 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10a80;
        for p in &self.procs {
            h = h.wrapping_mul(107).wrapping_add(p.pid);
            h = h.wrapping_mul(107).wrapping_add(p.parent);
            h = h.wrapping_mul(107).wrapping_add(p.state as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(107).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(107).wrapping_add(by as u32);
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
    let mut t = ProcTree::new();
    let _ = t.fork_child(INIT_PID, 2);
    let _ = t.fork_child(2, 3);
    let _ = t.fork_child(INIT_PID, 4); // 叔叔：与亲父 2 平辈的 init 之子
    // 1) 非亲收割必拒（叔叔收侄）。
    let _ = t.exit_child(3);
    got[0] = t.reap(4, 3, "inj-uncle").is_err();
    // 2) 活体收割必拒。
    got[1] = t.reap(2, 2, "inj-live").err() == Some(OZ_LIVE);
    // 3) 收养目标非 init 的伪造账必判非法。
    let forged = OzRec {
        seq: 1,
        pid: 3,
        adopt: true,
        who: 7,
        note: "inj-adopt".to_string(),
    };
    got[2] = !forged.ok();
    // 4) init 死亡面必拒（init 不亡）。
    got[3] = t.die(INIT_PID, "inj-init").err() == Some(OZ_BAD_ADOPT);
    // 5) 印账后操作必抓。
    t.seal();
    got[4] = t.exit_child(3).err() == Some(OZ_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10187_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10187");

    // J1 收养达成：父先亡→子改认 init。
    let j1 = {
        let mut t = ProcTree::new();
        let _ = t.fork_child(INIT_PID, 2);
        let _ = t.fork_child(2, 3);
        let _ = t.die(2, "p dies");
        t.parent_of(3) == Some(INIT_PID)
    };
    s.add("J1-收养达成", j1, "子 parent 改认 init=1");

    // J2 init 回收：收养后的僵尸子 init 可收。
    let j2 = {
        let mut t = ProcTree::new();
        let _ = t.fork_child(INIT_PID, 2);
        let _ = t.fork_child(2, 3);
        let _ = t.exit_child(3);
        let _ = t.die(2, "p dies");
        t.reap(INIT_PID, 3, "init reap").is_ok()
    };
    s.add("J2-init 回收", j2, "init 收养后可收割僵尸");

    // J3 僵尸态守恒：退出后保持 Zombie 直至收割。
    let j3 = {
        let mut t = ProcTree::new();
        let _ = t.fork_child(INIT_PID, 2);
        let _ = t.exit_child(2);
        let z = t.procs.iter().find(|p| p.pid == 2).map(|p| p.state);
        z == Some(PStat::Zombie)
    };
    s.add("J3-僵尸守恒", j3, "Zombie 态驻留待收");

    // J4 失管受控：非亲收割 ECHILD。
    let j4 = {
        let mut t = ProcTree::new();
        let _ = t.fork_child(INIT_PID, 2);
        let _ = t.fork_child(INIT_PID, 3);
        let _ = t.exit_child(2);
        t.reap(3, 2, "steal").err() == Some(OZ_ECHILD)
    };
    s.add("J4-失管受控", j4, "非亲收割必拒");

    // J5 活体收割拒。
    let j5 = {
        let mut t = ProcTree::new();
        let _ = t.fork_child(INIT_PID, 2);
        t.reap(INIT_PID, 2, "live").err() == Some(OZ_LIVE)
    };
    s.add("J5-活体拒", j5, "Running 态不可收割");

    // J6 收割后出表：不可重收。
    let j6 = {
        let mut t = ProcTree::new();
        let _ = t.fork_child(INIT_PID, 2);
        let _ = t.exit_child(2);
        let _ = t.reap(INIT_PID, 2, "r1");
        t.reap(INIT_PID, 2, "r2").err() == Some(OZ_ECHILD)
    };
    s.add("J6-重收割拒", j6, "收割后条目出表");

    // J7 多子全收养：父亡时多个子全部改认。
    let j7 = {
        let mut t = ProcTree::new();
        let _ = t.fork_child(INIT_PID, 2);
        let _ = t.fork_child(2, 3);
        let _ = t.fork_child(2, 4);
        let _ = t.fork_child(2, 5);
        let n = t.die(2, "p dies").unwrap_or(0);
        n == 3
            && t.parent_of(3) == Some(INIT_PID)
            && t.parent_of(4) == Some(INIT_PID)
            && t.parent_of(5) == Some(INIT_PID)
    };
    s.add("J7-多子全收养", j7, "三子全数改认 init");

    // J8 账本完整：收养/收割账全过合法性。
    let j8 = {
        let mut t = ProcTree::new();
        let _ = t.fork_child(INIT_PID, 2);
        let _ = t.fork_child(2, 3);
        let _ = t.exit_child(3);
        let _ = t.die(2, "d");
        let _ = t.reap(INIT_PID, 3, "r");
        t.ledger.len() == 2 && t.ledger_ok() && t.seq_unique()
    };
    s.add("J8-账本完整", j8, "收养+收割双账合法");

    // J9 指纹稳定且印账后封禁。
    let j9 = {
        let mut t = ProcTree::new();
        let _ = t.fork_child(INIT_PID, 2);
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2 && t.fork_child(INIT_PID, 9).err() == Some(OZ_FROZEN)
    };
    s.add("J9-指纹封账", j9, "指纹幂等且印后必拒");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10187_orphan_flow() {
        let mut t = ProcTree::new();
        let _ = t.fork_child(INIT_PID, 2);
        let _ = t.fork_child(2, 3);
        let _ = t.exit_child(3);
        assert_eq!(t.die(2, "d"), Ok(1));
        assert_eq!(t.parent_of(3), Some(INIT_PID));
        assert!(t.reap(INIT_PID, 3, "r").is_ok());
        assert_eq!(t.reap(2, 3, "r2"), Err(OZ_ECHILD));
    }

    #[test]
    fn unx10187_checks_green() {
        let s = run_unx10187_checks();
        assert!(s.all_passed(), "UNX-F10187 判据未全绿");
    }
}
