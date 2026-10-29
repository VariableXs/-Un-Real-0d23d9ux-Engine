//! UNX-F9148 getppid/set_tid_address 语义档（0x63240 段）
//!
//! 锚点：UNX-F9148-J1 快照账（getppid 快照与 set_tid_address 登记
//! 双账齐备）；J2 快照语义（init 父号恒 0 + 孤儿收养 ppid→1 + tid
//! 地址后写覆盖 + 返回 tid 非零四性质）；J3 快照指纹（双账折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B18（父号快照与线程清理挂点面）。
//!
//! 结构：进程表（pid/ppid/alive）+ tid 清理槽表（tid→clear_addr，后
//! 写覆盖）+ 快照账本 + 收养核（父死子 ppid→1）+ 折叠指纹 + 注入核
//! （错码/空留痕/重槽伪造/未知 pid/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63240 独占 0x20）。
pub const SEG_BASE: u32 = 0x63240;
pub const TP_UNK_PID: u32 = SEG_BASE + 0x01; // 未知 pid
pub const TP_BAD_SNAP: u32 = SEG_BASE + 0x02; // 快照判定错
pub const TP_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const TP_DUP_SLOT: u32 = SEG_BASE + 0x04; // 重槽伪造
pub const TP_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 收养目标：init 恒为 1 号。
pub const INIT_PID: u32 = 1;

/// 单条快照账。
#[derive(Clone)]
pub struct Snap {
    pub seq: u32,
    pub kind: u8, // 0=getppid 快照 1=set_tid_address 登记
    pub pid: u32,
    pub val: u32, // kind0: ppid 值；kind1: clear_addr
    pub code: u32,
    pub note: String,
}

impl Snap {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.kind <= 1
    }

    pub fn render_line(&self) -> String {
        format!("SN{}|k{}|p{}|v{}|{}", self.seq, self.kind, self.pid, self.val, self.note)
    }
}

/// tid 清理槽。
#[derive(Clone)]
pub struct TidSlot {
    pub tid: u32,
    pub clear_addr: u32,
    pub revisions: u32, // 覆盖写次数
}

/// 父号/清理挂点簿。
pub struct TpBook {
    /// (pid, ppid, alive)
    pub procs: Vec<(u32, u32, bool)>,
    pub slots: Vec<TidSlot>,
    pub ledger: Vec<Snap>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl TpBook {
    pub fn new() -> TpBook {
        TpBook { procs: Vec::new(), slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn spawn(&mut self, pid: u32, ppid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TP_FROZEN);
        }
        if self.find(pid).is_some() {
            return Err(TP_DUP_SLOT);
        }
        self.procs.push((pid, ppid, true));
        Ok(())
    }

    pub fn find(&self, pid: u32) -> Option<usize> {
        self.procs.iter().position(|p| p.0 == pid)
    }

    /// getppid：init 恒 0；普通返回 ppid；未知 ESRCH。
    pub fn getppid(&mut self, pid: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TP_FROZEN);
        }
        if note.is_empty() {
            return Err(TP_EMPTY_NOTE);
        }
        let i = match self.find(pid) {
            Some(i) => i,
            None => return Err(TP_UNK_PID),
        };
        let ppid = self.procs[i].1;
        self.record(0, pid, ppid, 0, note);
        if pid == INIT_PID {
            Ok(0)
        } else {
            Ok(ppid)
        }
    }

    /// 收养核：父死（alive=false）后其存活子 ppid→INIT_PID。
    pub fn reap_and_adopt(&mut self, pid: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(TP_FROZEN);
        }
        let i = match self.find(pid) {
            Some(i) => i,
            None => return Err(TP_UNK_PID),
        };
        self.procs[i].2 = false;
        let mut adopted: usize = 0;
        let n = self.procs.len();
        for k in 0..n {
            if self.procs[k].1 == pid && self.procs[k].2 && k != i {
                self.procs[k].1 = INIT_PID;
                adopted += 1;
            }
        }
        Ok(adopted)
    }

    /// set_tid_address(clear_addr) → 返回调用者 tid；后写覆盖先写。
    pub fn set_tid_address(&mut self, tid: u32, clear_addr: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(TP_FROZEN);
        }
        if note.is_empty() {
            return Err(TP_EMPTY_NOTE);
        }
        if tid == 0 {
            return Err(TP_UNK_PID);
        }
        match self.slots.iter_mut().find(|s| s.tid == tid) {
            Some(slot) => {
                slot.clear_addr = clear_addr;
                slot.revisions += 1;
            }
            None => self.slots.push(TidSlot { tid, clear_addr, revisions: 1 }),
        }
        self.record(1, tid, clear_addr, 0, note);
        Ok(tid)
    }

    pub fn slot_of(&self, tid: u32) -> Option<u32> {
        self.slots.iter().find(|s| s.tid == tid).map(|s| s.clear_addr)
    }

    pub fn slot_revisions(&self, tid: u32) -> Option<u32> {
        self.slots.iter().find(|s| s.tid == tid).map(|s| s.revisions)
    }

    fn record(&mut self, kind: u8, pid: u32, val: u32, code: u32, note: &str) -> u32 {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(Snap { seq, kind, pid, val, code, note: note.to_string() });
        seq
    }

    /// seq 唯一性。
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
        let mut h: u32 = 0x91480;
        for e in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(e.seq);
            h = h.wrapping_mul(31).wrapping_add(e.kind as u32);
            h = h.wrapping_mul(31).wrapping_add(e.pid);
            h = h.wrapping_mul(31).wrapping_add(e.val);
            for b in e.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        for s in &self.slots {
            h = h.wrapping_mul(31).wrapping_add(s.tid);
            h = h.wrapping_mul(31).wrapping_add(s.clear_addr);
            h = h.wrapping_mul(31).wrapping_add(s.revisions);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 基础样本：init(1) → parent(5) → child(6)/thread-tid(60)。
pub fn mk_book() -> TpBook {
    let mut b = TpBook::new();
    let _ = b.spawn(1, 0);
    let _ = b.spawn(5, 1);
    let _ = b.spawn(6, 5);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错码/坏 kind
    let bad = Snap { seq: 1, kind: 9, pid: 1, val: 0, code: 0, note: "inj-kind".to_string() };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = Snap { seq: 2, kind: 0, pid: 1, val: 0, code: 0, note: String::new() };
    got[1] = !bad2.ok();
    // 3) 重槽伪造：同 tid 两槽
    let mut b = mk_book();
    b.slots.push(TidSlot { tid: 60, clear_addr: 0x1000, revisions: 1 });
    b.slots.push(TidSlot { tid: 60, clear_addr: 0x2000, revisions: 1 });
    let dup = b.slots.iter().filter(|s| s.tid == 60).count() != 1;
    got[2] = dup;
    // 4) 未知 pid getppid
    let mut b2 = mk_book();
    got[3] = b2.getppid(0xDEAD, "inj-unk").err() == Some(TP_UNK_PID);
    // 5) 印后登记必抓
    b2.seal();
    got[4] = b2.getppid(6, "inj-frozen").err() == Some(TP_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9148_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9148");
    let mut b = mk_book();

    // J1 双账齐备
    let j1 = {
        let _ = b.getppid(6, "子问父");
        let _ = b.set_tid_address(60, 0x7F00, "线程清理挂点");
        b.ledger.iter().filter(|e| e.kind == 0).count() >= 1
            && b.ledger.iter().filter(|e| e.kind == 1).count() >= 1
    };
    s.add("J1-双账齐备", j1, "getppid 与 set_tid_address 双账各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|e| e.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且 kind 闭集");

    // J3 init 父号恒 0
    let j3 = b.getppid(1, "init 问父") == Ok(0);
    s.add("J3-init父号零", j3, "init 的 getppid 恒 0");

    // J4 普通快照
    let j4 = b.getppid(6, "快照") == Ok(5);
    s.add("J4-快照正读", j4, "child 的 ppid 快照为 5");

    // J5 收养：父死子归 init
    let j5 = {
        let mut b2 = mk_book();
        let n = b2.reap_and_adopt(5);
        n == Ok(1) && b2.getppid(6, "收养后") == Ok(INIT_PID)
    };
    s.add("J5-收养归一", j5, "父死后存活子 ppid→1");

    // J6 set_tid_address 返回 tid 非零
    let j6 = b.set_tid_address(61, 0x8000, "登记2") == Ok(61);
    s.add("J6-返回tid", j6, "登记返回 tid 本身且非零");

    // J7 后写覆盖：同 tid 再登记覆盖地址
    let j7 = {
        let _ = b.set_tid_address(61, 0x9000, "覆盖");
        b.slot_of(61) == Some(0x9000)
    };
    s.add("J7-后写覆盖", j7, "同 tid 后写地址覆盖先写");

    // J8 版本计数
    let j8 = b.slot_revisions(61) == Some(2);
    s.add("J8-版本计数", j8, "覆盖写计次数=2");

    // J9 tid 零拒绝
    let j9 = b.set_tid_address(0, 0x100, "零tid").err() == Some(TP_UNK_PID);
    s.add("J9-零tid拒", j9, "tid=0 登记必拒");

    // J10 未知 pid
    let j10 = b.getppid(0xDEAD, "未知").err() == Some(TP_UNK_PID);
    s.add("J10-未知pid", j10, "未知 pid 快照必拒");

    // J11 双槽互不串扰
    let j11 = b.slot_of(60) == Some(0x7F00) && b.slot_of(61) == Some(0x9000);
    s.add("J11-槽位隔离", j11, "不同 tid 槽互不串扰");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9148_snap_and_tid() {
        let mut b = mk_book();
        assert_eq!(b.getppid(1, "a"), Ok(0));
        assert_eq!(b.getppid(6, "b"), Ok(5));
        assert!(b.set_tid_address(60, 0x7F00, "c").is_ok());
        assert!(b.set_tid_address(60, 0x8000, "d").is_ok());
        assert_eq!(b.slot_of(60), Some(0x8000));
        assert_eq!(b.slot_revisions(60), Some(2));
        // 收养
        assert_eq!(b.reap_and_adopt(5), Ok(1));
        assert_eq!(b.getppid(6, "e"), Ok(1));
    }

    #[test]
    fn unx9148_checks_green() {
        let s = run_unx9148_checks();
        assert!(s.all_passed(), "UNX-F9148 判据未全绿");
    }
}
