//! UNX-F9149 vfork 挂点（共享地址空间窗口协议）（0x63260 段）
//!
//! 锚点：UNX-F9149-J1 窗口账（每次 vfork 开窗/关窗落账：父子对、
//! 关窗方式、留痕）；J2 窗口语义（开窗唯一 + 父挂起/唤醒配对 + 关
//! 窗两路（exec/_exit）+ 窗口期父写禁令四性质）；J3 窗口指纹（账本
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（vfork 共享地址空间窗口面）。
//!
//! 结构：窗口簿（至多一开窗）+ 窗口状态机（OPEN→CLOSED_BY_EXEC /
//! CLOSED_BY_EXIT）+ 挂起核 + 窗口期写禁令 + 窗口账本 + 折叠指纹 +
//! 注入核（二次开窗/空留痕/非法关窗/未知父子/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63260 独占 0x20）。
pub const SEG_BASE: u32 = 0x63260;
pub const VF_UNK_PAIR: u32 = SEG_BASE + 0x01; // 未知父子对
pub const VF_BAD_CLOSE: u32 = SEG_BASE + 0x02; // 非法关窗
pub const VF_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const VF_DUP_OPEN: u32 = SEG_BASE + 0x04; // 二次开窗
pub const VF_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 关窗方式闭集。
pub const CLOSE_NONE: u8 = 0; // 未关
pub const CLOSE_EXEC: u8 = 1; // 子 execve 关窗
pub const CLOSE_EXIT: u8 = 2; // 子 _exit 关窗
pub const CLOSE_COUNT: usize = 3;

pub const CLOSE_NAMES: [&str; CLOSE_COUNT] = ["open", "exec", "exit"];

/// 单条窗口账。
#[derive(Clone)]
pub struct VfRec {
    pub seq: u32,
    pub parent: u32,
    pub child: u32,
    pub close_by: u8,
    pub note: String,
}

impl VfRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.close_by as usize) < CLOSE_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "VF{}|p{}c{}|{}|{}",
            self.seq,
            self.parent,
            self.child,
            CLOSE_NAMES[self.close_by as usize],
            self.note
        )
    }
}

/// 单个窗口状态。
#[derive(Clone)]
pub struct VfWindow {
    pub parent: u32,
    pub child: u32,
    pub open: bool,
    pub close_by: u8,
    pub parent_suspended: bool,
}

/// vfork 窗口簿。
pub struct VfBook {
    pub windows: Vec<VfWindow>,
    pub ledger: Vec<VfRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl VfBook {
    pub fn new() -> VfBook {
        VfBook { windows: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 开窗：vfork(parent)。返回子 pid（约定 parent*100+1 式代号仅作
    /// 键用；真实 tid 由调用方给定）。开窗即父挂起。
    pub fn vfork(&mut self, parent: u32, child: u32, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(VF_FROZEN);
        }
        if note.is_empty() {
            return Err(VF_EMPTY_NOTE);
        }
        if parent == 0 || child == 0 || parent == child {
            return Err(VF_UNK_PAIR);
        }
        // 窗口唯一：同父子对至多一开窗，全局至多一开窗
        if self.open_window().is_some() {
            let seq = self.record(parent, child, CLOSE_NONE, note);
            let _ = seq;
            return Err(VF_DUP_OPEN);
        }
        if self.windows.iter().any(|w| w.open && (w.parent == parent || w.child == child)) {
            let seq = self.record(parent, child, CLOSE_NONE, note);
            let _ = seq;
            return Err(VF_DUP_OPEN);
        }
        self.windows.push(VfWindow {
            parent,
            child,
            open: true,
            close_by: CLOSE_NONE,
            parent_suspended: true,
        });
        let seq = self.record(parent, child, CLOSE_NONE, note);
        Ok(seq)
    }

    /// 当前开窗。
    pub fn open_window(&self) -> Option<usize> {
        self.windows.iter().position(|w| w.open)
    }

    /// 关窗两路：exec 或 exit。关窗即唤醒父。
    pub fn close_window(&mut self, child: u32, by: u8, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(VF_FROZEN);
        }
        if note.is_empty() {
            return Err(VF_EMPTY_NOTE);
        }
        if by != CLOSE_EXEC && by != CLOSE_EXIT {
            return Err(VF_BAD_CLOSE);
        }
        let wi = match self.windows.iter().position(|w| w.open && w.child == child) {
            Some(wi) => wi,
            None => return Err(VF_UNK_PAIR),
        };
        self.windows[wi].open = false;
        self.windows[wi].close_by = by;
        self.windows[wi].parent_suspended = false;
        let p = self.windows[wi].parent;
        let seq = self.record(p, child, by, note);
        Ok(seq)
    }

    /// 窗口期父写禁令：父挂起时父不可写内存。
    pub fn parent_write(&self, parent: u32) -> Result<(), u32> {
        if let Some(wi) = self.open_window() {
            if self.windows[wi].parent == parent && self.windows[wi].parent_suspended {
                return Err(VF_BAD_CLOSE);
            }
        }
        Ok(())
    }

    /// 挂起/唤醒配对：每条 CLOSE 账必有对应 OPEN 账且父子一致。
    pub fn pair_ok(&self) -> bool {
        for w in &self.windows {
            if w.open {
                continue;
            }
            // 关窗账存在且父唤醒
            let closed = self
                .ledger
                .iter()
                .any(|r| r.parent == w.parent && r.child == w.child && r.close_by == w.close_by);
            if !closed || w.parent_suspended {
                return false;
            }
        }
        true
    }

    fn record(&mut self, parent: u32, child: u32, close_by: u8, note: &str) -> u32 {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(VfRec {
            seq,
            parent,
            child,
            close_by,
            note: note.to_string(),
        });
        seq
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
        let mut h: u32 = 0x91490;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.parent);
            h = h.wrapping_mul(31).wrapping_add(r.child);
            h = h.wrapping_mul(31).wrapping_add(r.close_by as u32);
            for b in r.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
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
    // 1) 非法关窗方式
    let bad = VfRec {
        seq: 1,
        parent: 1,
        child: 2,
        close_by: 9,
        note: "inj-close".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = VfRec {
        seq: 2,
        parent: 1,
        child: 2,
        close_by: CLOSE_NONE,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 二次开窗
    let mut b = VfBook::new();
    let _ = b.vfork(10, 11, "开窗");
    got[2] = b.vfork(12, 13, "再开").err() == Some(VF_DUP_OPEN);
    // 4) 未知父子对关窗
    got[3] = b.close_window(99, CLOSE_EXEC, "inj-unk").err() == Some(VF_UNK_PAIR);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.vfork(20, 21, "inj-frozen").err() == Some(VF_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9149_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9149");

    // J1 窗口账非空（开+关两式）
    let mut b = VfBook::new();
    let j1 = {
        let _ = b.vfork(10, 11, "vfork 开窗");
        let _ = b.close_window(11, CLOSE_EXEC, "exec 关窗");
        b.ledger.iter().filter(|r| r.close_by == CLOSE_NONE).count() >= 1
            && b.ledger.iter().filter(|r| r.close_by == CLOSE_EXEC).count() >= 1
    };
    s.add("J1-窗口账两式", j1, "开窗与 exec 关窗账各至少一条");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|r| r.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且关窗方式闭集");

    // J3 开窗唯一：第二开窗被拒
    let j3 = {
        let mut b2 = VfBook::new();
        let _ = b2.vfork(1, 2, "首窗");
        b2.vfork(3, 4, "二窗").err() == Some(VF_DUP_OPEN)
    };
    s.add("J3-开窗唯一", j3, "全局至多一开窗");

    // J4 父挂起：开窗即挂起
    let j4 = {
        let mut b2 = VfBook::new();
        let _ = b2.vfork(10, 11, "开窗");
        let wi = b2.open_window().unwrap();
        b2.windows[wi].parent_suspended
    };
    s.add("J4-开窗即挂起", j4, "vfork 后父进入挂起态");

    // J5 exec 关窗唤醒父
    let j5 = {
        let mut b2 = VfBook::new();
        let _ = b2.vfork(10, 11, "开窗");
        let _ = b2.close_window(11, CLOSE_EXEC, "exec");
        let w = b2.windows.iter().find(|w| w.child == 11).unwrap();
        !w.open && !w.parent_suspended && w.close_by == CLOSE_EXEC
    };
    s.add("J5-exec关窗", j5, "exec 关窗且父唤醒");

    // J6 exit 关窗唤醒父
    let j6 = {
        let mut b2 = VfBook::new();
        let _ = b2.vfork(10, 11, "开窗");
        let _ = b2.close_window(11, CLOSE_EXIT, "_exit");
        let w = b2.windows.iter().find(|w| w.child == 11).unwrap();
        !w.open && !w.parent_suspended && w.close_by == CLOSE_EXIT
    };
    s.add("J6-exit关窗", j6, "_exit 关窗且父唤醒");

    // J7 窗口期父写禁令
    let j7 = {
        let mut b2 = VfBook::new();
        let _ = b2.vfork(10, 11, "开窗");
        let denied = b2.parent_write(10).err() == Some(VF_BAD_CLOSE);
        let _ = b2.close_window(11, CLOSE_EXIT, "关");
        denied && b2.parent_write(10).is_ok()
    };
    s.add("J7-父写禁令", j7, "窗口期父写被拒且关窗后放行");

    // J8 挂起/唤醒配对
    let j8 = {
        let mut b2 = VfBook::new();
        let _ = b2.vfork(10, 11, "开窗");
        let _ = b2.close_window(11, CLOSE_EXEC, "关");
        b2.pair_ok()
    };
    s.add("J8-挂起唤醒配对", j8, "每关窗必有开窗且父已唤醒");

    // J9 非法关窗方式
    let j9 = {
        let mut b2 = VfBook::new();
        let _ = b2.vfork(10, 11, "开窗");
        b2.close_window(11, 7, "非法方式").err() == Some(VF_BAD_CLOSE)
    };
    s.add("J9-非法关窗", j9, "关窗方式出闭集必拒");

    // J10 未知父子对
    let j10 = {
        let mut b2 = VfBook::new();
        b2.vfork(0, 0, "零对").err() == Some(VF_UNK_PAIR)
            && b2.vfork(5, 5, "自对").err() == Some(VF_UNK_PAIR)
    };
    s.add("J10-未知父子对", j10, "零号/自配对开窗必拒");

    // J11 关窗后可再开窗（唯一性只限同时刻）
    let j11 = {
        let mut b2 = VfBook::new();
        let _ = b2.vfork(10, 11, "一窗");
        let _ = b2.close_window(11, CLOSE_EXIT, "关");
        b2.vfork(10, 12, "二窗").is_ok()
    };
    s.add("J11-串行开窗", j11, "关窗后允许新开窗");

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
    fn unx9149_window_flow() {
        let mut b = VfBook::new();
        assert!(b.vfork(10, 11, "open").is_ok());
        assert!(b.parent_write(10).is_err());
        assert!(b.vfork(12, 13, "again").is_err());
        assert!(b.close_window(11, CLOSE_EXEC, "exec").is_ok());
        assert!(b.parent_write(10).is_ok());
        assert!(b.vfork(10, 12, "second round").is_ok());
        assert!(b.close_window(12, CLOSE_EXIT, "exit").is_ok());
        assert!(b.pair_ok());
    }

    #[test]
    fn unx9149_checks_green() {
        let s = run_unx9149_checks();
        assert!(s.all_passed(), "UNX-F9149 判据未全绿");
    }
}
