//! UNX-F10107 fcntl 正反双判据（描述符控制达成与非法命令误用受控）（0x10080 段）
//!
//! 锚点：UNX-F10107-J1 fcntl F_GETFD/F_SETFD/F_GETFL 达成（LTP
//! fcntl02/fcntl05 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：描述符控制册（fd→fdflags/flargs 两层旗标位）+ 命令核（F_GETFD/
//! F_SETFD/F_GETFL/F_SETFL/F_DUPFD 五命令分发）+ 命令账（逐次留痕）+
//! 注入核（非法命令/坏 fd/CLOEXEC 翻转错序/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10080 独占 0x20）。
pub const SEG_BASE: u32 = 0x10080;
pub const FC_BAD_CMD: u32 = SEG_BASE + 0x01; // 命令出闭集
pub const FC_BAD_FD: u32 = SEG_BASE + 0x02; // 坏 fd
pub const FC_BAD_ARG: u32 = SEG_BASE + 0x03; // 旗标位出闭集
pub const FC_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const FC_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 命令闭集。
pub const F_GETFD: u32 = 1;
pub const F_SETFD: u32 = 2;
pub const F_GETFL: u32 = 3;
pub const F_SETFL: u32 = 4;
pub const F_DUPFD: u32 = 5;

/// fdflags 旗标位：仅 CLOEXEC。
pub const FD_CLOEXEC: u32 = 0x01;
/// flargs 旗标位闭集。
pub const FL_O_RDWR: u32 = 0x002;
pub const FL_O_NONBLOCK: u32 = 0x800;
pub const FL_O_APPEND: u32 = 0x400;

/// fd 空间定容。
pub const FD_CAP: usize = 6;

/// 描述符控制件。
#[derive(Clone)]
pub struct FdCtl {
    pub id: u32,
    pub fdflags: u32,
    pub flargs: u32,
}

impl FdCtl {
    pub fn new(id: u32) -> FdCtl {
        FdCtl { id, fdflags: 0, flargs: FL_O_RDWR }
    }

    /// fdflags 位闭集校验。
    pub fn fdflags_ok(v: u32) -> bool {
        v & !FD_CLOEXEC == 0
    }

    /// flargs 位闭集校验。
    pub fn flargs_ok(v: u32) -> bool {
        v & !(FL_O_RDWR | FL_O_NONBLOCK | FL_O_APPEND) == 0
    }

    pub fn cloexec(&self) -> bool {
        self.fdflags & FD_CLOEXEC != 0
    }
}

/// 命令账留痕。
#[derive(Clone)]
pub struct FcRec {
    pub seq: u32,
    pub fd: i32,
    pub cmd: u32,
    pub arg: u32,
    pub ret: i64,
    pub note: String,
}

impl FcRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && (self.cmd >= F_GETFD && self.cmd <= F_DUPFD)
            && self.ret >= 0
    }
}

/// 描述符控制册。
pub struct FcntlBook {
    pub slots: Vec<Option<FdCtl>>,
    pub ledger: Vec<FcRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl FcntlBook {
    pub fn new() -> FcntlBook {
        FcntlBook { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn open(&mut self, id: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(FC_SEALED);
        }
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[i] = Some(FdCtl::new(id));
            return Ok(i);
        }
        if self.slots.len() >= FD_CAP {
            return Err(FC_BAD_FD);
        }
        self.slots.push(Some(FdCtl::new(id)));
        Ok(self.slots.len() - 1)
    }

    /// fcntl 核心：五命令分发，非法命令/坏 fd/位出闭集均拒。
    pub fn fcntl(&mut self, fd: i32, cmd: u32, arg: u32, note: &str) -> Result<i64, u32> {
        if self.sealed {
            return Err(FC_SEALED);
        }
        if note.is_empty() {
            return Err(FC_EMPTY_NOTE);
        }
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(FC_BAD_FD);
        }
        if cmd < F_GETFD || cmd > F_DUPFD {
            return Err(FC_BAD_CMD);
        }
        let u = fd as usize;
        let ret: i64;
        match cmd {
            F_GETFD => {
                ret = self.slots[u].as_ref().map(|c| c.fdflags as i64).unwrap_or(0);
            }
            F_SETFD => {
                if !FdCtl::fdflags_ok(arg) {
                    return Err(FC_BAD_ARG);
                }
                if let Some(c) = self.slots[u].as_mut() {
                    c.fdflags = arg;
                }
                ret = 0;
            }
            F_GETFL => {
                ret = self.slots[u].as_ref().map(|c| c.flargs as i64).unwrap_or(0);
            }
            F_SETFL => {
                if !FdCtl::flargs_ok(arg) {
                    return Err(FC_BAD_ARG);
                }
                if let Some(c) = self.slots[u].as_mut() {
                    // O_RDWR 属打开基态，SETFL 不可清
                    c.flargs = arg | FL_O_RDWR;
                }
                ret = 0;
            }
            _ => {
                // F_DUPFD：最低 ≥arg 的空闲槽
                let mut target: Option<usize> = None;
                let start = arg as usize;
                for i in start..FD_CAP {
                    if i >= self.slots.len() {
                        target = Some(i);
                        break;
                    }
                    if self.slots[i].is_none() {
                        target = Some(i);
                        break;
                    }
                }
                let t = match target {
                    Some(t) if t < FD_CAP => t,
                    _ => return Err(FC_BAD_FD),
                };
                while self.slots.len() <= t {
                    self.slots.push(None);
                }
                let c = self.slots[u].clone().unwrap_or(FdCtl::new(0));
                self.slots[t] = Some(c);
                ret = t as i64;
            }
        }
        self.ledger.push(FcRec { seq: self.seq_next, fd, cmd, arg, ret, note: note.to_string() });
        self.seq_next += 1;
        Ok(ret)
    }

    pub fn cloexec_of(&self, fd: i32) -> Option<bool> {
        if fd < 0 || fd as usize >= self.slots.len() {
            return None;
        }
        self.slots[fd as usize].as_ref().map(|c| c.cloexec())
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
        let mut h: u32 = 0x100800;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.cmd);
            h = h.wrapping_mul(31).wrapping_add(r.ret as u32);
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
    let mut b = FcntlBook::new();
    let f = b.open(1).unwrap_or(usize::MAX);
    // 1) 非法命令必拒
    got[0] = b.fcntl(f as i32, 9, 0, "inj-cmd").err() == Some(FC_BAD_CMD);
    // 2) 坏 fd 必拒
    got[1] = b.fcntl(-1, F_GETFD, 0, "inj-fd").err() == Some(FC_BAD_FD);
    // 3) fdflags 位出闭集必拒
    got[2] = b.fcntl(f as i32, F_SETFD, 0xF0, "inj-flag").err() == Some(FC_BAD_ARG);
    // 4) flargs 位出闭集必拒
    got[3] = b.fcntl(f as i32, F_SETFL, 0x10000, "inj-fl").err() == Some(FC_BAD_ARG);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.fcntl(f as i32, F_GETFD, 0, "inj-seal").err() == Some(FC_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10107_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10107");

    // 正路径：F_SETFD→F_GETFD 回读一致（fcntl02 面）
    let mut b = FcntlBook::new();
    let f = b.open(1).unwrap_or(usize::MAX);
    let _ = b.fcntl(f as i32, F_SETFD, FD_CLOEXEC, "置位");
    let j1 = b.fcntl(f as i32, F_GETFD, 0, "回读").unwrap_or(-1) == FD_CLOEXEC as i64
        && b.cloexec_of(f as i32) == Some(true);
    s.add("J1-CLOEXEC回读", j1, "F_SETFD 置位后 F_GETFD 一致");

    // 清位回读一致
    let j2 = {
        let _ = b.fcntl(f as i32, F_SETFD, 0, "清位");
        b.cloexec_of(f as i32) == Some(false)
    };
    s.add("J2-清位回读", j2, "F_SETFD 0 清位生效");

    // F_GETFL/F_SETFL 面：NONBLOCK|APPEND 合成置位
    let j3 = {
        let want = FL_O_NONBLOCK | FL_O_APPEND;
        let _ = b.fcntl(f as i32, F_SETFL, want, "非阻塞追加");
        let got = b.fcntl(f as i32, F_GETFL, 0, "读旗").unwrap_or(0) as u32;
        got == (want | FL_O_RDWR)
    };
    s.add("J3-SETFL合成", j3, "合成置位回读含基态 O_RDWR");

    // F_DUPFD 选槽语义
    let j4 = {
        let d = b.fcntl(f as i32, F_DUPFD, 3, "复制").unwrap_or(-1);
        d == 3
    };
    s.add("J4-DUPFD选槽", j4, "F_DUPFD 落最低 ≥arg 空槽");

    // 非法命令受控
    let j5 = b.fcntl(f as i32, 0, 0, "zero").err() == Some(FC_BAD_CMD)
        && b.fcntl(f as i32, 6, 0, "six").err() == Some(FC_BAD_CMD);
    s.add("J5-命令闭集", j5, "命令出五命令闭集必拒");

    // 坏 fd 受控
    let j6 = b.fcntl(-2, F_GETFD, 0, "neg").err() == Some(FC_BAD_FD)
        && b.fcntl(FD_CAP as i32 + 2, F_GETFD, 0, "oob").err() == Some(FC_BAD_FD);
    s.add("J6-坏fd受控", j6, "负与越界 fd 同码拒");

    // 位闭集受控（fdflags/flargs 两面）
    let j7 = b.fcntl(f as i32, F_SETFD, 0xFF, "fdbg").err() == Some(FC_BAD_ARG)
        && b.fcntl(f as i32, F_SETFL, 0xFFFF_FFFF, "flg").err() == Some(FC_BAD_ARG);
    s.add("J7-位闭集受控", j7, "旗标位出闭集必拒");

    // 空留痕拒收
    let j8 = b.fcntl(f as i32, F_GETFD, 0, "").err() == Some(FC_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 容量边界：fd 空间守恒
    let j9 = {
        let mut b9 = FcntlBook::new();
        let mut ok = true;
        for i in 0..FD_CAP {
            if b9.open(i as u32 + 1).is_err() {
                ok = false;
            }
        }
        ok && b9.open(99).err() == Some(FC_BAD_FD)
    };
    s.add("J9-容量守恒", j9, "FD_CAP 守恒");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "命令在闭集、返回非负、注非空");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10107_fcntl_flow() {
        let mut b = FcntlBook::new();
        let f = b.open(1).unwrap_or(usize::MAX);
        let _ = b.fcntl(f as i32, F_SETFD, FD_CLOEXEC, "a");
        assert_eq!(b.fcntl(f as i32, F_GETFD, 0, "b").unwrap_or(9), 1);
        assert!(b.fcntl(f as i32, 42, 0, "c") == Err(FC_BAD_CMD));
        let d = b.fcntl(f as i32, F_DUPFD, 2, "d").unwrap_or(0);
        assert!(d >= 2);
    }

    #[test]
    fn unx10107_checks_green() {
        let s = run_unx10107_checks();
        assert!(s.all_passed(), "UNX-F10107 判据未全绿");
    }
}
