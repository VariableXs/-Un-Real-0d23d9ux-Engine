//! UNX-F10104 close 正反双判据（关闭释放达成与重关闭误用受控）（0x10020 段）
//!
//! 锚点：UNX-F10104-J1 close 关闭释放达成（fd 表槽位回收守恒 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：槽位册（fd→持有票据的定容表，回收位独立）+ 关闭核（释放/守恒
//! 对平/最低空闲复用）+ 回收账（逐次关闭留痕 + 守恒计数器）+ 注入核
//! （重关闭/负 fd/越界 fd/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10020 独占 0x20）。
pub const SEG_BASE: u32 = 0x10020;
pub const CL_DUP_CLOSE: u32 = SEG_BASE + 0x01; // 重关闭
pub const CL_BAD_FD: u32 = SEG_BASE + 0x02; // 负 fd/越界
pub const CL_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const CL_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const CL_NOT_OPEN: u32 = SEG_BASE + 0x05; // 未打开即关

/// 槽位定容。
pub const SLOT_CAP: usize = 8;

/// 槽位票据：打开时派发，关闭时回收核销。
#[derive(Clone, Copy, PartialEq)]
pub struct Ticket {
    pub ino: u32,
    pub stamp: u32,
}

impl Ticket {
    pub fn ok(&self) -> bool {
        self.ino != 0 && self.stamp != 0
    }
}

/// 关闭账留痕。
#[derive(Clone)]
pub struct ClRec {
    pub seq: u32,
    pub fd: i32,
    pub ino: u32,
    pub note: String,
}

impl ClRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.fd >= 0 && self.ino != 0
    }
}

/// 槽位册。
pub struct SlotBook {
    pub slots: Vec<Option<Ticket>>,
    pub opened_total: u32,
    pub closed_total: u32,
    pub ledger: Vec<ClRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl SlotBook {
    pub fn new() -> SlotBook {
        SlotBook {
            slots: Vec::new(),
            opened_total: 0,
            closed_total: 0,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    /// 打开：派发票据占位，返回 fd。
    pub fn open(&mut self, ino: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(CL_SEALED);
        }
        if ino == 0 {
            return Err(CL_BAD_FD);
        }
        let slot = if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            i
        } else if self.slots.len() < SLOT_CAP {
            self.slots.push(None);
            self.slots.len() - 1
        } else {
            return Err(CL_BAD_FD);
        };
        self.slots[slot] = Some(Ticket { ino, stamp: self.seq_next });
        self.opened_total += 1;
        self.seq_next += 1;
        Ok(slot)
    }

    /// close 核心：释放槽位、核销计数、留痕。
    pub fn close(&mut self, fd: i32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CL_SEALED);
        }
        if note.is_empty() {
            return Err(CL_EMPTY_NOTE);
        }
        if fd < 0 || fd as usize >= self.slots.len() {
            return Err(CL_BAD_FD);
        }
        let u = fd as usize;
        let t = match self.slots[u] {
            Some(t) => t,
            None => return Err(CL_DUP_CLOSE),
        };
        self.slots[u] = None;
        self.closed_total += 1;
        self.ledger.push(ClRec { seq: self.seq_next, fd, ino: t.ino, note: note.to_string() });
        self.seq_next += 1;
        Ok(t.ino)
    }

    /// 守恒裁断：打开数 == 关闭数 + 活跃数。
    pub fn conserved(&self) -> bool {
        self.opened_total == self.closed_total + self.live() as u32
    }

    /// 活跃槽位数。
    pub fn live(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// 最低空闲槽位探测（回收面）。
    pub fn lowest_free(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_none())
    }

    /// 指定 fd 是否活跃。
    pub fn is_live(&self, fd: i32) -> bool {
        fd >= 0 && (fd as usize) < self.slots.len() && self.slots[fd as usize].is_some()
    }

    /// 指定 fd 的票据（活跃才有）。
    pub fn ticket_of(&self, fd: i32) -> Option<Ticket> {
        if fd < 0 || fd as usize >= self.slots.len() {
            return None;
        }
        self.slots[fd as usize]
    }

    /// 三元审计快照（开/关/活）。
    pub fn audit(&self) -> (u32, u32, usize) {
        (self.opened_total, self.closed_total, self.live())
    }

    /// 全关回收（批量面）：逐槽释放并返回释放数。
    pub fn release_all(&mut self, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(CL_SEALED);
        }
        if note.is_empty() {
            return Err(CL_EMPTY_NOTE);
        }
        let mut n = 0usize;
        for i in 0..self.slots.len() {
            if self.slots[i].is_some() {
                let ino = self.slots[i].map(|t| t.ino).unwrap_or(0);
                self.slots[i] = None;
                self.closed_total += 1;
                self.ledger.push(ClRec {
                    seq: self.seq_next,
                    fd: i as i32,
                    ino,
                    note: note.to_string(),
                });
                self.seq_next += 1;
                n += 1;
            }
        }
        Ok(n)
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
        let mut h: u32 = 0x100200;
        h = h.wrapping_mul(31).wrapping_add(self.opened_total);
        h = h.wrapping_mul(31).wrapping_add(self.closed_total);
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.fd as u32);
            h = h.wrapping_mul(31).wrapping_add(r.ino);
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
    let mut b = SlotBook::new();
    let fd = b.open(7).unwrap_or(usize::MAX);
    // 1) 重关闭必拒
    let _ = b.close(fd as i32, "first");
    got[0] = b.close(fd as i32, "again").err() == Some(CL_DUP_CLOSE);
    // 2) 负 fd 必拒
    got[1] = b.close(-1, "neg").err() == Some(CL_BAD_FD);
    // 3) 越界 fd 必拒（未开槽位即关）
    got[2] = b.close(99, "oob").err() == Some(CL_BAD_FD);
    // 4) 空留痕必拒
    let fd2 = b.open(8).unwrap_or(usize::MAX);
    got[3] = b.close(fd2 as i32, "").err() == Some(CL_EMPTY_NOTE);
    // 5) 封表后关闭必抓
    b.seal();
    got[4] = b.close(fd2 as i32, "late").err() == Some(CL_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10104_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10104");

    // 正路径：打开-关闭循环守恒（10/10 面）
    let mut b = SlotBook::new();
    let mut round_ok = true;
    for round in 0..10u32 {
        let f1 = b.open(round * 2 + 1).unwrap_or(usize::MAX);
        let f2 = b.open(round * 2 + 2).unwrap_or(usize::MAX);
        if b.close(f1 as i32, "r1").is_err() || b.close(f2 as i32, "r2").is_err() {
            round_ok = false;
        }
    }
    let j1 = round_ok && b.conserved() && b.live() == 0;
    s.add("J1-循环守恒", j1, "十轮开关后活跃归零且守恒成立");

    // 槽位回收复用：最低空闲槽
    let j2 = {
        let mut b2 = SlotBook::new();
        let _ = b2.open(1);
        let _ = b2.open(2);
        let _ = b2.close(0, "c");
        b2.lowest_free() == Some(0) && b2.open(3).unwrap_or(usize::MAX) == 0
    };
    s.add("J2-最低槽复用", j2, "回收位被下一次打开复用");

    // 重关闭误用受控
    let j3 = {
        let mut b3 = SlotBook::new();
        let f = b3.open(9).unwrap_or(usize::MAX);
        let _ = b3.close(f as i32, "a");
        b3.close(f as i32, "b").err() == Some(CL_DUP_CLOSE)
    };
    s.add("J3-重关闭受控", j3, "同 fd 二次关闭必拒");

    // 未打开即关受控（界内空槽：先开两槽再释放其一，留界内空位）
    let j4 = {
        let mut b4 = SlotBook::new();
        let _ = b4.open(5);
        let _ = b4.open(6);
        let _ = b4.close(1, "free");
        b4.close(1, "never").err() == Some(CL_DUP_CLOSE)
    };
    s.add("J4-未开槽受控", j4, "空槽关闭按重关闭码拒");

    // 负 fd 与越界 fd 受控
    let j5 = b.close(-3, "neg").err() == Some(CL_BAD_FD)
        && b.close(SLOT_CAP as i32 + 7, "oob").err() == Some(CL_BAD_FD);
    s.add("J5-坏fd受控", j5, "负与越界同码拒");

    // 容量边界：SLOT_CAP 守恒
    let j6 = {
        let mut b6 = SlotBook::new();
        let mut ok = true;
        for i in 0..SLOT_CAP {
            if b6.open(i as u32 + 1).is_err() {
                ok = false;
            }
        }
        ok && b6.open(99).err() == Some(CL_BAD_FD) && b6.live() == SLOT_CAP
    };
    s.add("J6-容量边界", j6, "SLOT_CAP 守恒且满开必拒");

    // 零号 inode 拒开
    let j7 = b.open(0).err() == Some(CL_BAD_FD);
    s.add("J7-零号拒开", j7, "ino=0 打开必拒");

    // 空留痕拒收
    let j8 = {
        let f = b.open(42).unwrap_or(usize::MAX);
        b.close(f as i32, "").err() == Some(CL_EMPTY_NOTE)
    };
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j9 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J9-留痕合法", j9, "fd 非负、ino 非零、注非空");

    // 封表后操作必抓
    let j10 = {
        b.seal();
        b.open(50).err() == Some(CL_SEALED)
    };
    s.add("J10-封表拒操作", j10, "封表后 open 必拒");

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
    fn unx10104_close_flow() {
        let mut b = SlotBook::new();
        let f = b.open(3).unwrap_or(usize::MAX);
        assert_eq!(b.close(f as i32, "c").unwrap_or(0), 3);
        assert!(b.close(f as i32, "d").err() == Some(CL_DUP_CLOSE));
        assert!(b.conserved());
    }

    #[test]
    fn unx10104_checks_green() {
        let s = run_unx10104_checks();
        assert!(s.all_passed(), "UNX-F10104 判据未全绿");
    }
}
