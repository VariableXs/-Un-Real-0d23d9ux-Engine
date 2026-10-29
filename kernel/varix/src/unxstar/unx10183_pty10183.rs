//! UNX-F10183 pty 会话控制正反双判据（会话达成与无控制终端误用受控）（0x10a00 段）
//!
//! 锚点：UNX-F10183-J1 会话达成（forkpty 子进程得控制终端 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：会话册（进程表 × 会话表 × ctty 归属）+ forkpty 核（新会话
//! + 控制终端一次性认领）+ 终端使用核（ctty 匹配才可用）+ 会话账本 +
//! 折叠指纹 + 注入核（幽灵父/重复认领/无 ctty 误用/空留痕/印后 fork）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10a00 独占 0x20）。
pub const SEG_BASE: u32 = 0x10a00;
pub const SS_GHOST: u32 = SEG_BASE + 0x01; // 幽灵父进程
pub const SS_BUSY: u32 = SEG_BASE + 0x02; // 控制终端已被认领
pub const SS_NOCTTY: u32 = SEG_BASE + 0x03; // 无控制终端误用
pub const SS_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const SS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍 fork

/// 进程条目：pid × 会话归属。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProcSess {
    pub pid: u32,
    pub sid: u32,
}

/// 会话条目：sid × 控制终端（pty 对 id）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Session {
    pub sid: u32,
    pub ctty: Option<u32>,
}

/// 一次 forkpty 的账本留痕。
#[derive(Clone)]
pub struct ForkRec {
    pub seq: u32,
    pub parent: u32,
    pub child: u32,
    pub sid: u32,
    pub ctty: u32,
    pub note: String,
}

impl ForkRec {
    /// 账目合法性：留痕非空且新会话 id 恒等于子 pid（forkpty 语义）。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.sid == self.child && self.parent != self.child
    }

    pub fn render_line(&self) -> String {
        format!(
            "F{}|p{}->c{}|sid{}|ctty{}|{}",
            self.seq, self.parent, self.child, self.sid, self.ctty, self.note
        )
    }
}

/// 会话册：进程表 + 会话表 + forkpty 核。
pub struct SessionTab {
    pub procs: Vec<ProcSess>,
    pub sessions: Vec<Session>,
    pub pairs_next: u32,
    pub pid_next: u32,
    pub ledger: Vec<ForkRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SessionTab {
    pub fn new() -> SessionTab {
        SessionTab {
            procs: Vec::new(),
            sessions: Vec::new(),
            pairs_next: 1,
            pid_next: 100,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 初代进程（无会话、无 ctty）。
    pub fn seed(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SS_FROZEN);
        }
        if pid == 0 || self.procs.iter().any(|p| p.pid == pid) {
            return Err(SS_GHOST);
        }
        self.procs.push(ProcSess { pid, sid: 0 });
        if pid >= self.pid_next {
            self.pid_next = pid + 1;
        }
        Ok(())
    }

    /// forkpty 核：子进程领新会话并一次性认领控制终端。
    pub fn forkpty(&mut self, parent: u32, note: &str) -> Result<(u32, u32), u32> {
        if self.sealed {
            return Err(SS_FROZEN);
        }
        if note.is_empty() {
            return Err(SS_EMPTY_NOTE);
        }
        if !self.procs.iter().any(|p| p.pid == parent) {
            return Err(SS_GHOST);
        }
        let child = self.pid_next;
        self.pid_next += 1;
        let ctty = self.pairs_next;
        self.pairs_next += 1;
        // 新会话：sid = child pid，ctty 一次性认领。
        self.sessions.push(Session { sid: child, ctty: Some(ctty) });
        self.procs.push(ProcSess { pid: child, sid: child });
        self.ledger.push(ForkRec {
            seq: self.seq_next,
            parent,
            child,
            sid: child,
            ctty,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok((child, ctty))
    }

    /// 控制终端认领：仅当该会话尚无 ctty（重复认领必拒）。
    pub fn claim_ctty(&mut self, sid: u32, pair: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SS_FROZEN);
        }
        match self.sessions.iter_mut().find(|s| s.sid == sid) {
            Some(s) if s.ctty.is_none() => {
                s.ctty = Some(pair);
                Ok(())
            }
            Some(_) => Err(SS_BUSY),
            None => Err(SS_GHOST),
        }
    }

    /// 挂断：会话 ctty 摘除（后续终端使用必拒）。
    pub fn detach_ctty(&mut self, sid: u32) -> Result<(), u32> {
        match self.sessions.iter_mut().find(|s| s.sid == sid) {
            Some(s) if s.ctty.is_some() => {
                s.ctty = None;
                Ok(())
            }
            Some(_) => Err(SS_NOCTTY),
            None => Err(SS_GHOST),
        }
    }

    /// 终端使用核：pid 的会话 ctty 与所持 pair 匹配才可用。
    pub fn use_ctty(&self, pid: u32, pair: u32) -> Result<(), u32> {
        let p = match self.procs.iter().find(|p| p.pid == pid) {
            Some(p) => p,
            None => return Err(SS_GHOST),
        };
        match self.sessions.iter().find(|s| s.sid == p.sid) {
            Some(s) if s.ctty == Some(pair) => Ok(()),
            Some(_) => Err(SS_NOCTTY),
            None => Err(SS_NOCTTY),
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

    /// 折叠指纹（89 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10a00;
        for s in &self.sessions {
            h = h.wrapping_mul(89).wrapping_add(s.sid);
            h = h.wrapping_mul(89).wrapping_add(s.ctty.unwrap_or(0));
        }
        for r in &self.ledger {
            h = h.wrapping_mul(89).wrapping_add(r.child);
            for by in r.note.bytes() {
                h = h.wrapping_mul(89).wrapping_add(by as u32);
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
    let mut t = SessionTab::new();
    let _ = t.seed(1);
    // 1) 幽灵父 forkpty 必拒。
    got[0] = t.forkpty(99, "inj-ghost").err() == Some(SS_GHOST);
    // 2) 无 ctty 误用：初代进程（sid=0 无会话）用终端必拒。
    got[1] = t.use_ctty(1, 1).err() == Some(SS_NOCTTY);
    // 3) 重复认领必拒：forkpty 会话已带 ctty，再认领按 BUSY 拒。
    let (child, ctty) = t.forkpty(1, "ok").unwrap_or((0, 0));
    got[2] = t.claim_ctty(child, ctty).err() == Some(SS_BUSY);
    // 4) 伪造账：sid != child 的 forkpty 账必判非法。
    let forged = ForkRec {
        seq: 1,
        parent: 1,
        child: 100,
        sid: 7,
        ctty: 1,
        note: "inj-sid".to_string(),
    };
    got[3] = !forged.ok();
    // 5) 印账后 forkpty 必抓。
    t.seal();
    got[4] = t.forkpty(1, "inj-frozen").err() == Some(SS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10183_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10183");

    // J1 会话达成：forkpty 子进程 sid==pid 且得 ctty。
    let j1 = {
        let mut t = SessionTab::new();
        let _ = t.seed(1);
        let r = t.forkpty(1, "f");
        match r {
            Ok((child, ctty)) => {
                child == 100
                    && ctty == 1
                    && t.sessions[0].sid == child
                    && t.sessions[0].ctty == Some(ctty)
            }
            Err(_) => false,
        }
    };
    s.add("J1-会话达成", j1, "子进程新会话 sid==pid 且持 ctty");

    // J2 控制终端生效：持对进程用对即通。
    let j2 = {
        let mut t = SessionTab::new();
        let _ = t.seed(1);
        let (child, ctty) = t.forkpty(1, "f").unwrap_or((0, 0));
        t.use_ctty(child, ctty).is_ok()
    };
    s.add("J2-终端生效", j2, "ctty 匹配使用放行");

    // J3 无控制终端受控：非会话首进程用对必拒。
    let j3 = {
        let mut t = SessionTab::new();
        let _ = t.seed(1);
        let _ = t.forkpty(1, "f");
        // 初代进程 sid=0 无会话；错对子进程也拒。
        t.use_ctty(1, 1).err() == Some(SS_NOCTTY)
            && t.use_ctty(100, 2).err() == Some(SS_NOCTTY)
    };
    s.add("J3-无 ctty 受控", j3, "无匹配 ctty 使用必拒");

    // J4 重复认领受控：detach 后可再认领、未 detach 再认领必拒。
    let j4 = {
        let mut t = SessionTab::new();
        let _ = t.seed(1);
        let (child, ctty) = t.forkpty(1, "f").unwrap_or((0, 0));
        let busy = t.claim_ctty(child, ctty).err() == Some(SS_BUSY);
        let _ = t.detach_ctty(child);
        let reclaim = t.claim_ctty(child, ctty).is_ok();
        busy && reclaim
    };
    s.add("J4-认领时序", j4, "持有时拒、摘除后可再领");

    // J5 挂断受控：detach 后终端使用必拒。
    let j5 = {
        let mut t = SessionTab::new();
        let _ = t.seed(1);
        let (child, ctty) = t.forkpty(1, "f").unwrap_or((0, 0));
        let _ = t.detach_ctty(child);
        t.use_ctty(child, ctty).err() == Some(SS_NOCTTY)
    };
    s.add("J5-挂断失效", j5, "ctty 摘除后使用必拒");

    // J6 幽灵父拒。
    let j6 = {
        let mut t = SessionTab::new();
        t.forkpty(42, "g").err() == Some(SS_GHOST)
    };
    s.add("J6-幽灵父拒", j6, "未注册父 forkpty 必拒");

    // J7 多会话隔离：两会话各持各对互不越用。
    let j7 = {
        let mut t = SessionTab::new();
        let _ = t.seed(1);
        let (c1, p1) = t.forkpty(1, "s1").unwrap_or((0, 0));
        let (c2, p2) = t.forkpty(1, "s2").unwrap_or((0, 0));
        t.use_ctty(c1, p1).is_ok() && t.use_ctty(c2, p2).is_ok()
            && t.use_ctty(c1, p2).err() == Some(SS_NOCTTY)
    };
    s.add("J7-会话隔离", j7, "跨会话用对必拒");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut t = SessionTab::new();
        let _ = t.seed(1);
        let _ = t.forkpty(1, "f");
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2 && t.forkpty(1, "post").err() == Some(SS_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10183_session_flow() {
        let mut t = SessionTab::new();
        let _ = t.seed(1);
        let (child, ctty) = t.forkpty(1, "f").unwrap();
        assert_eq!(child, 100);
        assert!(t.use_ctty(child, ctty).is_ok());
        let _ = t.detach_ctty(child);
        assert_eq!(t.use_ctty(child, ctty), Err(SS_NOCTTY));
        assert!(t.claim_ctty(child, ctty).is_ok());
    }

    #[test]
    fn unx10183_checks_green() {
        let s = run_unx10183_checks();
        assert!(s.all_passed(), "UNX-F10183 判据未全绿");
    }
}
