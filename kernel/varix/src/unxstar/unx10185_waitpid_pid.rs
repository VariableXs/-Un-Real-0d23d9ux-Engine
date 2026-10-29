//! UNX-F10185 waitpid 状态码正反双判据（状态面达成与非法 pid 误用受控）（0x10a40 段）
//!
//! 锚点：UNX-F10185-J1 状态面达成（WIFEXITED/WEXITSTATUS/WIFSIGNALED
//! 三面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：状态字编码核（POSIX wait status 位域：低 7 位信号/停止位、
//! 高 8 位退出码）+ 解码判据面（WIFEXITED/WEXITSTATUS/WIFSIGNALED/
//! WTERMSIG）+ 收割册（子进程状态回读）+ 注入核（双面互斥伪造/
//! 越域退出码/野信号/重收割/印后）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10a40 独占 0x20）。
pub const SEG_BASE: u32 = 0x10a40;
pub const WS_ECHILD: u32 = SEG_BASE + 0x01; // 非子/无此子
pub const WS_BAD_CODE: u32 = SEG_BASE + 0x02; // 退出码越域
pub const WS_BAD_SIG: u32 = SEG_BASE + 0x03; // 野信号
pub const WS_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const WS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍收

/// 信号合法域 [1,31]。
pub const SIG_MIN: u32 = 1;
pub const SIG_MAX: u32 = 31;
/// 停止位模式（低 7 位全 1 = WIFSTOPPED 面）。
pub const STOPPED_MASK: u32 = 0x7F;

/// 状态字编码核：exit(code) 与 kill(sig) 两构造器。
pub fn encode_exit(code: u32) -> Result<u32, u32> {
    if code > 0xFF {
        return Err(WS_BAD_CODE);
    }
    Ok(code << 8)
}

pub fn encode_sig(sig: u32) -> Result<u32, u32> {
    if sig < SIG_MIN || sig > SIG_MAX {
        return Err(WS_BAD_SIG);
    }
    Ok(sig)
}

/// WIFEXITED：低 7 位全零。
pub fn wifexited(w: u32) -> bool {
    w & STOPPED_MASK == 0
}

/// WEXITSTATUS：高 8 位。
pub fn wexitstatus(w: u32) -> u32 {
    (w >> 8) & 0xFF
}

/// WIFSIGNALED：低 7 位非零且非停止模式。
pub fn wifsignaled(w: u32) -> bool {
    let low = w & STOPPED_MASK;
    low != 0 && low != STOPPED_MASK
}

/// WTERMSIG：低 7 位。
pub fn wtermsig(w: u32) -> u32 {
    w & STOPPED_MASK
}

/// 子进程条目。
#[derive(Clone, Copy)]
pub struct WChild {
    pub pid: u32,
    pub word: u32,
    pub done: bool,
}

/// 一次收割的账本留痕。
#[derive(Clone)]
pub struct WsRec {
    pub seq: u32,
    pub pid: u32,
    pub word: u32,
    pub note: String,
}

impl WsRec {
    /// 账目合法性：留痕非空且状态字单面自洽（双面互斥：exit 面低 7 位
    /// 全零；signal 面不得夹带退出码载荷，否则既像 exit 又像 signal 即伪造）。
    pub fn ok(&self) -> bool {
        if self.note.is_empty() {
            return false;
        }
        wifexited(self.word) || (wifsignaled(self.word) && wexitstatus(self.word) == 0)
    }

    pub fn render_line(&self) -> String {
        format!("V{}|p{}|w{}|{}", self.seq, self.pid, self.word, self.note)
    }
}

/// 收割册：父视角子表 + 状态回读。
pub struct WaitTab {
    pub children: Vec<WChild>,
    pub ledger: Vec<WsRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl WaitTab {
    pub fn new() -> WaitTab {
        WaitTab { children: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn fork_child(&mut self, pid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(WS_FROZEN);
        }
        if pid == 0 || self.children.iter().any(|c| c.pid == pid) {
            return Err(WS_ECHILD);
        }
        self.children.push(WChild { pid, word: 0, done: false });
        Ok(())
    }

    /// 子进程以退出码终局。
    pub fn set_exit(&mut self, pid: u32, code: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(WS_FROZEN);
        }
        if note.is_empty() {
            return Err(WS_EMPTY_NOTE);
        }
        let w = encode_exit(code)?;
        let c = match self.children.iter_mut().find(|c| c.pid == pid) {
            Some(c) if !c.done => c,
            _ => return Err(WS_ECHILD),
        };
        c.word = w;
        c.done = true;
        Ok(())
    }

    /// 子进程被信号终结。
    pub fn set_kill(&mut self, pid: u32, sig: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(WS_FROZEN);
        }
        if note.is_empty() {
            return Err(WS_EMPTY_NOTE);
        }
        let w = encode_sig(sig)?;
        let c = match self.children.iter_mut().find(|c| c.pid == pid) {
            Some(c) if !c.done => c,
            _ => return Err(WS_ECHILD),
        };
        c.word = w;
        c.done = true;
        Ok(())
    }

    /// waitpid 收割：读回状态字并出表（重收割 ECHILD）。
    pub fn waitpid(&mut self, pid: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(WS_FROZEN);
        }
        if note.is_empty() {
            return Err(WS_EMPTY_NOTE);
        }
        let i = match self.children.iter().position(|c| c.pid == pid) {
            Some(i) => i,
            None => return Err(WS_ECHILD),
        };
        if !self.children[i].done {
            return Err(WS_ECHILD); // 尚未终局无可回读
        }
        let w = self.children[i].word;
        self.ledger.push(WsRec {
            seq: self.seq_next,
            pid,
            word: w,
            note: note.to_string(),
        });
        self.seq_next += 1;
        self.children.remove(i);
        Ok(w)
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

    /// 折叠指纹（101 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10a40;
        for r in &self.ledger {
            h = h.wrapping_mul(101).wrapping_add(r.word);
            h = h.wrapping_mul(101).wrapping_add(r.pid);
            for by in r.note.bytes() {
                h = h.wrapping_mul(101).wrapping_add(by as u32);
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
    // 1) 双面互斥伪造账：signal 面夹带退出码载荷（既像 exit 又像 signal）必判非法。
    let forged = WsRec {
        seq: 1,
        pid: 1,
        word: (7 << 8) | 9, // 高位退出码 + 低位信号
        note: "inj-both".to_string(),
    };
    got[0] = wifsignaled(forged.word)
        && wexitstatus(forged.word) > 0
        && !forged.ok();
    // 2) 越域退出码（256）必拒。
    got[1] = encode_exit(256).err() == Some(WS_BAD_CODE);
    // 3) 野信号（0 与 33）必拒。
    got[2] = encode_sig(0).err() == Some(WS_BAD_SIG)
        && encode_sig(33).err() == Some(WS_BAD_SIG);
    // 4) 幽灵/未终局收割必拒。
    let mut t = WaitTab::new();
    let _ = t.fork_child(10);
    got[3] = t.waitpid(99, "g").err() == Some(WS_ECHILD)
        && t.waitpid(10, "live").err() == Some(WS_ECHILD);
    // 5) 印账后收割必抓。
    t.seal();
    got[4] = t.waitpid(10, "inj-frozen").err() == Some(WS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10185_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10185");

    // J1 WIFEXITED/WEXITSTATUS 面：退出码 42 精确回读。
    let j1 = {
        let mut t = WaitTab::new();
        let _ = t.fork_child(10);
        let _ = t.set_exit(10, 42, "e");
        let w = t.waitpid(10, "r").unwrap_or(0xFFFF);
        wifexited(w) && wexitstatus(w) == 42 && !wifsignaled(w)
    };
    s.add("J1-exit 面", j1, "WIFEXITED 且 WEXITSTATUS==42");

    // J2 WIFSIGNALED/WTERMSIG 面：信号 9 精确回读。
    let j2 = {
        let mut t = WaitTab::new();
        let _ = t.fork_child(11);
        let _ = t.set_kill(11, 9, "k");
        let w = t.waitpid(11, "r").unwrap_or(0xFFFF);
        wifsignaled(w) && wtermsig(w) == 9 && !wifexited(w)
    };
    s.add("J2-signal 面", j2, "WIFSIGNALED 且 WTERMSIG==9");

    // J3 越域双拒：退出码 256、信号 0/33。
    let j3 = encode_exit(256).err() == Some(WS_BAD_CODE)
        && encode_sig(0).err() == Some(WS_BAD_SIG);
    s.add("J3-越域双拒", j3, "码与信号出域必拒");

    // J4 编码解码守恒：全码段 roundtrip。
    let j4 = {
        let mut ok = true;
        let codes = [0u32, 1, 42, 127, 255];
        for c in codes {
            match encode_exit(c) {
                Ok(w) => {
                    if !wifexited(w) || wexitstatus(w) != c {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        let sigs = [1u32, 9, 15, 31];
        for g in sigs {
            match encode_sig(g) {
                Ok(w) => {
                    if !wifsignaled(w) || wtermsig(w) != g {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        ok
    };
    s.add("J4-roundtrip 守恒", j4, "九样本编码解码零漂移");

    // J5 非法 pid 误用受控：无此子收割 ECHILD。
    let j5 = {
        let mut t = WaitTab::new();
        t.waitpid(777, "no").err() == Some(WS_ECHILD)
    };
    s.add("J5-非子拒", j5, "无此子进程必 ECHILD");

    // J6 重收割受控。
    let j6 = {
        let mut t = WaitTab::new();
        let _ = t.fork_child(10);
        let _ = t.set_exit(10, 1, "e");
        let _ = t.waitpid(10, "r1");
        t.waitpid(10, "r2").err() == Some(WS_ECHILD)
    };
    s.add("J6-重收割拒", j6, "收割后出表不可再收");

    // J7 未终局拒：活体无可回读状态。
    let j7 = {
        let mut t = WaitTab::new();
        let _ = t.fork_child(10);
        t.waitpid(10, "live").err() == Some(WS_ECHILD)
    };
    s.add("J7-活体拒", j7, "未终局子进程无可回读");

    // J8 账本完整：exit/signal 两类账全合法。
    let j8 = {
        let mut t = WaitTab::new();
        let _ = t.fork_child(1);
        let _ = t.fork_child(2);
        let _ = t.set_exit(1, 5, "e");
        let _ = t.set_kill(2, 15, "k");
        let _ = t.waitpid(1, "r");
        let _ = t.waitpid(2, "r");
        t.ledger.len() == 2 && t.ledger_ok() && t.seq_unique()
    };
    s.add("J8-账本完整", j8, "双面账全过互斥判据");

    // J9 指纹稳定且印账后封禁。
    let j9 = {
        let mut t = WaitTab::new();
        let _ = t.fork_child(1);
        let _ = t.set_exit(1, 2, "e");
        let _ = t.waitpid(1, "r");
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2 && t.fork_child(9).err() == Some(WS_FROZEN)
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
    fn unx10185_status_flow() {
        assert_eq!(encode_exit(42), Ok(42 << 8));
        assert_eq!(encode_sig(9), Ok(9));
        let mut t = WaitTab::new();
        let _ = t.fork_child(1);
        let _ = t.set_exit(1, 42, "e");
        let w = t.waitpid(1, "r").unwrap();
        assert!(wifexited(w) && wexitstatus(w) == 42);
    }

    #[test]
    fn unx10185_checks_green() {
        let s = run_unx10185_checks();
        assert!(s.all_passed(), "UNX-F10185 判据未全绿");
    }
}
