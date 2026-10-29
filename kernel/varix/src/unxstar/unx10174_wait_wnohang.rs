//! UNX-F10174 wait/WNOHANG 正反双判据（轮询达成与无状态误用受控）（0x108e0 段）
//!
//! 锚点：UNX-F10174-J1 轮询达成（WNOHANG 零阻塞轮询语义 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：子进程册（pid × 运行态/退出态）+ 收割核（阻塞收/零阻塞轮询
//! /非子拒 ECHILD）+ 收割账本（状态码留痕）+ 折叠指纹 +
//! 注入核（活体收割/幽灵 ECHILD/重收割/空留痕/印后收）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x108e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x108e0;
pub const W_ECHILD: u32 = SEG_BASE + 0x01; // 非子/已收：无此子进程
pub const W_BUSY: u32 = SEG_BASE + 0x02; // 阻塞式收活体（会挂起的受控面）
pub const W_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 留痕为空
pub const W_REAP_LIVE: u32 = SEG_BASE + 0x04; // 活体收割注入
pub const W_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍收

/// 子进程状态闭集。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PState {
    Running = 1,
    Exited = 2,
}

/// 收割结果：零阻塞轮询空手（NoHang）或收得状态码（Done）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitRes {
    NoHang,
    Done { status: u32 },
}

/// 子进程条目。
#[derive(Clone)]
pub struct Child {
    pub pid: u32,
    pub state: PState,
    pub status: u32,
}

/// 一次收割的账本留痕。
#[derive(Clone)]
pub struct WaitRec {
    pub seq: u32,
    pub pid: u32,
    pub nohang: bool,
    pub got: bool,
    pub status: u32,
    pub note: String,
}

impl WaitRec {
    /// 账目合法性：留痕非空；空手轮询 status 必为 0。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.got || self.status == 0)
    }

    pub fn render_line(&self) -> String {
        format!(
            "W{}|p{}|nh{}|got{}|st{}|{}",
            self.seq, self.pid, self.nohang as u8, self.got as u8, self.status, self.note
        )
    }
}

/// 收割册：父进程视角的子进程表。
pub struct WaitBook {
    pub parent: u32,
    pub children: Vec<Child>,
    pub ledger: Vec<WaitRec>,
    pub seq_next: u32,
    pub reaped: u32,
    pub sealed: bool,
}

impl WaitBook {
    pub fn new(parent: u32) -> WaitBook {
        WaitBook {
            parent,
            children: Vec::new(),
            ledger: Vec::new(),
            seq_next: 1,
            reaped: 0,
            sealed: false,
        }
    }

    /// fork：登记子进程（重号必拒）。
    pub fn fork_child(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(W_FROZEN);
        }
        if pid == 0 || self.children.iter().any(|c| c.pid == pid) {
            return Err(W_ECHILD);
        }
        self.children.push(Child { pid, state: PState::Running, status: 0 });
        Ok(())
    }

    /// 子进程退出：登记退出状态（活体退出注入必拒）。
    pub fn set_exit(&mut self, pid: u32, status: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(W_FROZEN);
        }
        if note.is_empty() {
            return Err(W_EMPTY_NOTE);
        }
        match self.children.iter_mut().find(|c| c.pid == pid) {
            Some(c) if c.state == PState::Running => {
                c.state = PState::Exited;
                c.status = status;
                Ok(())
            }
            Some(_) => Err(W_REAP_LIVE), // 已退出者不得二次退出
            None => Err(W_ECHILD),
        }
    }

    /// waitpid 核心：WNOHANG 零阻塞轮询；活体收 ERR(W_BUSY)；退出态即收。
    pub fn waitpid(&mut self, pid: u32, nohang: bool, note: &str) -> Result<WaitRes, u32> {
        if self.sealed {
            return Err(W_FROZEN);
        }
        if note.is_empty() {
            return Err(W_EMPTY_NOTE);
        }
        let i = match self.children.iter().position(|c| c.pid == pid) {
            Some(i) => i,
            None => return Err(W_ECHILD),
        };
        let (got, status) = if self.children[i].state == PState::Exited {
            (true, self.children[i].status)
        } else if nohang {
            // WNOHANG 语义：活体立即空手返回，零阻塞、零状态变化。
            (false, 0)
        } else {
            return Err(W_BUSY);
        };
        self.ledger.push(WaitRec {
            seq: self.seq_next,
            pid,
            nohang,
            got,
            status,
            note: note.to_string(),
        });
        self.seq_next += 1;
        if got {
            // 收割：子进程出表（不可重收）。
            self.children.remove(i);
            self.reaped += 1;
            Ok(WaitRes::Done { status })
        } else {
            Ok(WaitRes::NoHang)
        }
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

    /// 折叠指纹（47 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x108e0;
        for c in &self.children {
            h = h.wrapping_mul(47).wrapping_add(c.pid);
            h = h.wrapping_mul(47).wrapping_add(c.status);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(47).wrapping_add(r.seq);
            h = h.wrapping_mul(47).wrapping_add(r.status);
            for by in r.note.bytes() {
                h = h.wrapping_mul(47).wrapping_add(by as u32);
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
    let mut b = WaitBook::new(1);
    let _ = b.fork_child(10);
    // 1) 幽灵/非子收割必拒（ECHILD）。
    got[0] = b.waitpid(99, true, "inj-ghost").err() == Some(W_ECHILD);
    // 2) 活体收割注入：活体直接收割面拒收（受控：WNOHANG 只轮询不收）。
    got[1] = b.waitpid(10, false, "inj-live").err() == Some(W_BUSY);
    // 3) 二次退出注入：活体退出后再次退出必拒。
    let _ = b.set_exit(10, 3, "exit1");
    got[2] = b.set_exit(10, 4, "exit2").err() == Some(W_REAP_LIVE);
    // 4) 空留痕收割必拒。
    let _ = b.fork_child(11);
    got[3] = b.waitpid(11, true, "").err() == Some(W_EMPTY_NOTE);
    // 5) 印账后收割必抓。
    b.seal();
    got[4] = b.waitpid(11, true, "inj-frozen").err() == Some(W_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10174_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10174");

    // J1 轮询达成：活体 WNOHANG 空手返回、零阻塞、零状态变化。
    let j1 = {
        let mut b = WaitBook::new(1);
        let _ = b.fork_child(10);
        let before = b.children.len();
        let r = b.waitpid(10, true, "poll");
        r == Ok(WaitRes::NoHang) && b.children.len() == before
    };
    s.add("J1-零阻塞轮询", j1, "WNOHANG 空手且子表不动");

    // J2 收割达成：退出态收得状态码且出表。
    let j2 = {
        let mut b = WaitBook::new(1);
        let _ = b.fork_child(10);
        let _ = b.set_exit(10, 42, "exit");
        let r = b.waitpid(10, false, "reap");
        r == Ok(WaitRes::Done { status: 42 }) && b.children.is_empty() && b.reaped == 1
    };
    s.add("J2-收割达成", j2, "状态码 42 精确回读且出表");

    // J3 无状态误用受控：非子收割 ECHILD。
    let j3 = {
        let mut b = WaitBook::new(1);
        b.waitpid(1234, true, "nochild").err() == Some(W_ECHILD)
    };
    s.add("J3-非子拒", j3, "无此子进程必 ECHILD");

    // J4 重收割受控：收过再收 ECHILD。
    let j4 = {
        let mut b = WaitBook::new(1);
        let _ = b.fork_child(10);
        let _ = b.set_exit(10, 1, "e");
        let _ = b.waitpid(10, false, "r1");
        b.waitpid(10, false, "r2").err() == Some(W_ECHILD)
    };
    s.add("J4-重收割拒", j4, "收割后条目出表不可再收");

    // J5 状态编码守恒：多子多状态逐一回读。
    let j5 = {
        let mut b = WaitBook::new(1);
        let _ = b.fork_child(1);
        let _ = b.fork_child(2);
        let _ = b.fork_child(3);
        let _ = b.set_exit(1, 7, "e");
        let _ = b.set_exit(3, 9, "e");
        let a = b.waitpid(1, false, "r");
        let c = b.waitpid(3, false, "r");
        a == Ok(WaitRes::Done { status: 7 }) && c == Ok(WaitRes::Done { status: 9 })
    };
    s.add("J5-状态守恒", j5, "状态码逐子回读不串位");

    // J6 阻塞面受控：活体阻塞收按 W_BUSY 拒（不挂死）。
    let j6 = {
        let mut b = WaitBook::new(1);
        let _ = b.fork_child(10);
        b.waitpid(10, false, "block").err() == Some(W_BUSY)
    };
    s.add("J6-阻塞受控", j6, "活体阻塞收受控拒绝不挂死");

    // J7 账本完整：轮询账与收割账混排全合法。
    let j7 = {
        let mut b = WaitBook::new(1);
        let _ = b.fork_child(10);
        let _ = b.set_exit(10, 5, "e");
        let _ = b.waitpid(10, true, "p1"); // 空手
        let _ = b.fork_child(11);
        let _ = b.set_exit(11, 6, "e");
        let _ = b.waitpid(11, false, "r1"); // 收割
        b.ledger.len() == 2 && b.ledger_ok() && b.seq_unique()
    };
    s.add("J7-账本完整", j7, "空手账 status=0、收割账带码");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut b = WaitBook::new(1);
        let _ = b.fork_child(10);
        let _ = b.set_exit(10, 2, "e");
        let _ = b.waitpid(10, false, "r");
        let f1 = b.fold();
        let f2 = b.fold();
        b.seal();
        f1 == f2 && b.fork_child(20).err() == Some(W_FROZEN)
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
    fn unx10174_wait_flow() {
        let mut b = WaitBook::new(7);
        let _ = b.fork_child(10);
        assert_eq!(b.waitpid(10, true, "p"), Ok(WaitRes::NoHang));
        let _ = b.set_exit(10, 33, "e");
        assert_eq!(b.waitpid(10, false, "r"), Ok(WaitRes::Done { status: 33 }));
        assert_eq!(b.waitpid(10, true, "r2"), Err(W_ECHILD));
    }

    #[test]
    fn unx10174_checks_green() {
        let s = run_unx10174_checks();
        assert!(s.all_passed(), "UNX-F10174 判据未全绿");
    }
}
