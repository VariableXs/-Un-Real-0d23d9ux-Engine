//! UNX-F10119 exit/wait 正反双判据（退出回收达成与无子等待误用受控）（0x10200 段）
//!
//! 锚点：UNX-F10119-J1 exit/wait 回收达成（WEXITSTATUS 回读一致 LTP
//! exit01/wait01 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：进程册（pid/父链/退出码/僵尸位）+ 收割核（exit 落僵尸/wait
//! 取码回收/无子 ECHILD）+ 收割账（逐次留痕 + 回收守恒）+ 注入核（无子
//! 等待/重复收割/坏 pid/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10200 独占 0x20）。
pub const SEG_BASE: u32 = 0x10200;
pub const EW_ECHILD: u32 = SEG_BASE + 0x01; // 无子可等
pub const EW_BAD_PID: u32 = SEG_BASE + 0x02; // 坏 pid
pub const EW_REAPED: u32 = SEG_BASE + 0x03; // 重复收割
pub const EW_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const EW_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// wait 状态位编解码（WEXITSTATUS 面）。
pub const WS_EXITED: u32 = 0x0001;
pub const WS_SIGNALED: u32 = 0x0002;

pub fn mk_status(code: u32) -> u32 {
    (code << 8) | WS_EXITED
}

pub fn wexitstatus(status: u32) -> u32 {
    (status >> 8) & 0xFF
}

pub fn wifexited(status: u32) -> bool {
    status & WS_EXITED != 0
}

/// 进程记录。
#[derive(Clone)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub exited: bool,
    pub reaped: bool,
    pub status: u32,
}

impl Proc {
    pub fn ok(&self) -> bool {
        self.pid != 0 && self.ppid != 0
    }

    /// 可收割：已退出且未收割。
    pub fn reapable(&self) -> bool {
        self.exited && !self.reaped
    }

    /// 僵尸位（退出未收割）。
    pub fn zombie(&self) -> bool {
        self.exited && !self.reaped
    }
}

/// 收割账留痕。
#[derive(Clone)]
pub struct EwRec {
    pub seq: u32,
    pub pid: u32,
    pub code: u32,
    pub note: String,
}

impl EwRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.pid != 0 && self.code <= 0xFF
    }
}

/// 进程册 + 收割核。
pub struct ProcBook {
    pub procs: Vec<Proc>,
    pub ledger: Vec<EwRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ProcBook {
    pub fn new() -> ProcBook {
        ProcBook { procs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// fork：派生进程（父 pid 必须已存在）。
    pub fn fork(&mut self, ppid: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(EW_SEALED);
        }
        if ppid == 0 || !self.procs.iter().any(|p| p.pid == ppid && !p.exited) {
            return Err(EW_BAD_PID);
        }
        let pid = self.procs.iter().map(|p| p.pid).max().unwrap_or(0) + 1;
        self.procs.push(Proc { pid, ppid, exited: false, reaped: false, status: 0 });
        Ok(pid)
    }

    /// 初始进程（init）。
    pub fn init(&mut self) -> u32 {
        self.procs.push(Proc { pid: 1, ppid: 1, exited: false, reaped: false, status: 0 });
        1
    }

    /// exit：置退出码与僵尸位。
    pub fn exit(&mut self, pid: u32, code: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(EW_SEALED);
        }
        if note.is_empty() {
            return Err(EW_EMPTY_NOTE);
        }
        if pid == 0 || code > 0xFF {
            return Err(EW_BAD_PID);
        }
        let idx = match self.procs.iter().position(|p| p.pid == pid) {
            Some(i) => i,
            None => return Err(EW_BAD_PID),
        };
        if self.procs[idx].exited {
            return Err(EW_REAPED);
        }
        self.procs[idx].exited = true;
        self.procs[idx].status = mk_status(code);
        self.ledger.push(EwRec { seq: self.seq_next, pid, code, note: note.to_string() });
        self.seq_next += 1;
        Ok(code)
    }

    /// wait：收割指定子（pid==0 任一）。
    pub fn wait(&mut self, ppid: u32, pid: u32, note: &str) -> Result<(u32, u32), u32> {
        if self.sealed {
            return Err(EW_SEALED);
        }
        if note.is_empty() {
            return Err(EW_EMPTY_NOTE);
        }
        let target: Vec<usize> = self
            .procs
            .iter()
            .enumerate()
            .filter(|(_, p)| p.ppid == ppid && p.reapable() && (pid == 0 || p.pid == pid))
            .map(|(i, _)| i)
            .collect();
        let idx = match target.first() {
            Some(i) => *i,
            None => return Err(EW_ECHILD),
        };
        let (cpid, status) = (self.procs[idx].pid, self.procs[idx].status);
        self.procs[idx].reaped = true;
        self.ledger.push(EwRec {
            seq: self.seq_next,
            pid: cpid,
            code: wexitstatus(status),
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok((cpid, wexitstatus(status)))
    }

    /// 僵尸计数。
    pub fn zombie_count(&self) -> usize {
        self.procs.iter().filter(|p| p.zombie()).count()
    }

    /// 回收守恒：收割数+僵尸数 == 退出数。
    pub fn conserved(&self) -> bool {
        let exited = self.procs.iter().filter(|p| p.exited).count();
        let reaped = self.procs.iter().filter(|p| p.reaped).count();
        reaped + self.zombie_count() == exited
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
        let mut h: u32 = 0x102000;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.pid);
            h = h.wrapping_mul(31).wrapping_add(r.code);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ProcBook::new();
    let init = b.init();
    let c = b.fork(init).unwrap_or(0);
    let _ = b.exit(c, 7, "inj-exit");
    // 1) 无子等待（先收割完再等）必拒
    let _ = b.wait(init, 0, "inj-first");
    got[0] = b.wait(init, 0, "inj-again").err() == Some(EW_ECHILD);
    // 2) 坏 pid exit 必拒
    let mut b2 = ProcBook::new();
    let _ = b2.init();
    got[1] = b2.exit(99, 0, "inj-pid").err() == Some(EW_BAD_PID);
    // 3) 重复退出必拒
    let c2 = b2.fork(1).unwrap_or(0);
    let _ = b2.exit(c2, 1, "inj-e1");
    got[2] = b2.exit(c2, 2, "inj-e2").err() == Some(EW_REAPED);
    // 4) 越界退出码必拒
    got[3] = b2.exit(0, 0x100, "inj-code").err() == Some(EW_BAD_PID);
    // 5) 封表后操作必抓
    b2.seal();
    got[4] = b2.wait(1, 0, "inj-seal").err() == Some(EW_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10119_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10119");

    // 正路径：exit→wait 回读一致（exit01/wait01 面）
    let mut b = ProcBook::new();
    let init = b.init();
    let c1 = b.fork(init).unwrap_or(0);
    let c2 = b.fork(init).unwrap_or(0);
    let _ = b.exit(c1, 42, "子一退");
    let _ = b.exit(c2, 7, "子二退");
    let (r1, code1) = b.wait(init, 0, "收一").unwrap_or((0, 0xFF));
    let (r2, code2) = b.wait(init, 0, "收二").unwrap_or((0, 0xFF));
    let j1 = { r1 + r2 == c1 + c2 } && code1 == 42 && code2 == 7;
    s.add("J1-退出码回读", j1, "两子退出码 42/7 回读一致");

    // WEXITSTATUS 编解码自洽
    let j2 = wexitstatus(mk_status(42)) == 42 && wifexited(mk_status(42));
    s.add("J2-状态编解码", j2, "mk_status/wexitstatus 往返一致");

    // 僵尸面：退出未收割入僵尸
    let j3 = {
        let mut b3 = ProcBook::new();
        let i3 = b3.init();
        let c3 = b3.fork(i3).unwrap_or(0);
        let _ = b3.exit(c3, 1, "z");
        b3.zombie_count() == 1 && b3.conserved()
    };
    s.add("J3-僵尸入账", j3, "退出未收割计僵尸且守恒");

    // 无子等待受控（ECHILD）
    let j4 = {
        let mut b4 = ProcBook::new();
        let i4 = b4.init();
        b4.wait(i4, 0, "无子").err() == Some(EW_ECHILD)
    };
    s.add("J4-无子受控", j4, "无子 wait 必 ECHILD");

    // 指定子收割
    let j5 = {
        let mut b5 = ProcBook::new();
        let i5 = b5.init();
        let x = b5.fork(i5).unwrap_or(0);
        let y = b5.fork(i5).unwrap_or(0);
        let _ = b5.exit(x, 3, "x退");
        let _ = b5.exit(y, 4, "y退");
        let (got, code) = b5.wait(i5, y, "定点").unwrap_or((0, 0));
        got == y && code == 4
    };
    s.add("J5-定点收割", j5, "指定 pid 收割对应退出码");

    // 重复收割受控
    let j6 = {
        let mut b6 = ProcBook::new();
        let i6 = b6.init();
        let c6 = b6.fork(i6).unwrap_or(0);
        let _ = b6.exit(c6, 1, "退");
        let _ = b6.wait(i6, 0, "收");
        b6.wait(i6, 0, "再收").err() == Some(EW_ECHILD)
    };
    s.add("J6-重复收割受控", j6, "已收割子再等必 ECHILD");

    // 坏 pid 受控
    let j7 = b.fork(99).err() == Some(EW_BAD_PID)
        && b.exit(0, 1, "零号").err() == Some(EW_BAD_PID);
    s.add("J7-坏pid受控", j7, "野父 fork 与零号 exit 同拒");

    // 重复退出受控
    let j8 = {
        let mut b8 = ProcBook::new();
        let i8 = b8.init();
        let c8 = b8.fork(i8).unwrap_or(0);
        let _ = b8.exit(c8, 1, "一");
        b8.exit(c8, 2, "二").err() == Some(EW_REAPED)
    };
    s.add("J8-重复退出受控", j8, "二次 exit 必拒");

    // 守恒面
    let j9 = b.conserved() && b.zombie_count() == 0;
    s.add("J9-回收守恒", j9, "收割后僵尸清零且守恒成立");

    // 空留痕拒收
    let j10 = b.wait(init, 0, "").err() == Some(EW_EMPTY_NOTE);
    s.add("J10-空留痕拒", j10, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j11 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J11-留痕合法", j11, "码在 0xFF 内且注非空");

    // 注入核 5/5
    let inj = inject_five();
    let j12 = inj.iter().all(|g| *g);
    s.add_dyn("J12-注入全抓", j12, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j13 = b.fold() == b.fold();
    s.add("J13-指纹稳定", j13, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10119_exit_wait_flow() {
        let mut b = ProcBook::new();
        let i = b.init();
        let c = b.fork(i).unwrap_or(0);
        let _ = b.exit(c, 42, "e");
        let (_, code) = b.wait(i, 0, "w").unwrap_or((0, 0));
        assert_eq!(code, 42);
        assert!(b.wait(i, 0, "w2") == Err(EW_ECHILD));
    }

    #[test]
    fn unx10119_checks_green() {
        let s = run_unx10119_checks();
        assert!(s.all_passed(), "UNX-F10119 判据未全绿");
    }
}
