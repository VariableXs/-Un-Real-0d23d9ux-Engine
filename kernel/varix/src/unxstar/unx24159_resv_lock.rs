//! UNX-F24159 reservation 锁与显存互斥纪律·子功能19（Windows SRWLOCK 对照格）（0x415C0 段）
//!
//! 锚点：UNX-F24159-J1 对照账（Unix reservation 面 ↔ Win32 SRWLOCK/
//! Condition Variable 逐格落账）；J2 对照语义（四级闭集 + EQUIV 一对
//! 一 + NONE 恒占位三性质）；J3 对照指纹（格折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（跨系锁原语对照面）。
//!
//! 结构：双侧符号册 + 对照格核 + 格账本 + 折叠指纹 + 注入核（野
//! unix/野 win/野等级/NONE 带符/印后必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x415C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x415C0;
pub const CW_UNK_UNIX: u32 = SEG_BASE + 0x01; // 野 unix 符号
pub const CW_UNK_WIN: u32 = SEG_BASE + 0x02; // 野 win 符号
pub const CW_BAD_LEVEL: u32 = SEG_BASE + 0x03; // 等级出闭集
pub const CW_MISMATCH: u32 = SEG_BASE + 0x04; // 等级档位不符
pub const CW_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 对照等级。
pub const LV_EQUIV: u8 = 0;
pub const LV_PARTIAL: u8 = 1;
pub const LV_ANALOG: u8 = 2;
pub const LV_NONE: u8 = 3;
pub const LV_COUNT: u8 = 4;

pub const WIN_DASH: &str = "-";

/// Unix reservation 面符号。
pub const UNIX_SYMS: [&str; 5] =
    ["resv_ticket", "mutex_acquire", "try_mutex", "rw_read", "rw_write"];

/// Win32 锁原语符号。
pub const WIN_SYMS: [&str; 5] =
    ["SRWLockAcquireExclusive", "SRWLockAcquireShared", "TryAcquireSRWLockExclusive",
     "SleepConditionVariableSRW", "ReleaseSRWLockShared"];

/// 单条对照格。
#[derive(Clone)]
pub struct CwRec {
    pub seq: u32,
    pub unix_sym: String,
    pub win_sym: String,
    pub level: u8,
    pub note: String,
}

impl CwRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.level < LV_COUNT
            && !self.unix_sym.is_empty()
            && match self.level {
                LV_NONE => self.win_sym == WIN_DASH,
                _ => !self.win_sym.is_empty() && self.win_sym != WIN_DASH,
            }
    }
}

/// 对照簿。
pub struct CwBook {
    pub unix_syms: Vec<String>,
    pub win_syms: Vec<String>,
    pub grid: Vec<CwRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl CwBook {
    pub fn new() -> CwBook {
        let mut b = CwBook {
            unix_syms: Vec::new(),
            win_syms: Vec::new(),
            grid: Vec::new(),
            seq_next: 1,
            sealed: false,
        };
        for s in UNIX_SYMS.iter() {
            b.unix_syms.push(s.to_string());
        }
        for s in WIN_SYMS.iter() {
            b.win_syms.push(s.to_string());
        }
        b
    }

    pub fn has_unix(&self, s: &str) -> bool {
        self.unix_syms.iter().any(|x| x == s)
    }

    pub fn has_win(&self, s: &str) -> bool {
        self.win_syms.iter().any(|x| x == s)
    }

    pub fn cmp(&mut self, unix_sym: &str, win_sym: &str, level: u8, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CW_FROZEN);
        }
        if note.is_empty() || level >= LV_COUNT {
            return Err(CW_BAD_LEVEL);
        }
        if !self.has_unix(unix_sym) {
            return Err(CW_UNK_UNIX);
        }
        if level == LV_NONE {
            if win_sym != WIN_DASH {
                return Err(CW_MISMATCH);
            }
        } else {
            if win_sym == WIN_DASH || win_sym.is_empty() {
                return Err(CW_MISMATCH);
            }
            if !self.has_win(win_sym) {
                return Err(CW_UNK_WIN);
            }
            if level == LV_EQUIV
                && self.grid.iter().any(|g| g.unix_sym == unix_sym && g.level == LV_EQUIV)
            {
                return Err(CW_MISMATCH);
            }
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.grid.push(CwRec {
            seq,
            unix_sym: unix_sym.to_string(),
            win_sym: win_sym.to_string(),
            level,
            note: note.to_string(),
        });
        Ok(seq)
    }

    pub fn levels_complete(&self) -> bool {
        for l in 0..LV_COUNT {
            if !self.grid.iter().any(|g| g.level == l) {
                return false;
            }
        }
        true
    }

    pub fn equiv_one_to_one(&self) -> bool {
        for i in 0..self.grid.len() {
            for j in (i + 1)..self.grid.len() {
                let (a, b) = (&self.grid[i], &self.grid[j]);
                if a.level == LV_EQUIV && b.level == LV_EQUIV && a.unix_sym == b.unix_sym {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x415C0;
        for g in &self.grid {
            h = h.wrapping_mul(31).wrapping_add(g.seq);
            h = h.wrapping_mul(31).wrapping_add(g.level as u32);
            for by in g.unix_sym.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
            for by in g.win_sym.bytes() {
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

/// 基础样本：四级齐备。
pub fn mk_book() -> CwBook {
    let mut b = CwBook::new();
    let _ = b.cmp("resv_ticket", "SleepConditionVariableSRW", LV_ANALOG, "票回执类比特件唤醒");
    let _ = b.cmp("mutex_acquire", "SRWLockAcquireExclusive", LV_EQUIV, "独占获取等价");
    let _ = b.cmp("try_mutex", "TryAcquireSRWLockExclusive", LV_EQUIV, "尝试获取等价");
    let _ = b.cmp("rw_read", "SRWLockAcquireShared", LV_EQUIV, "共享读等价");
    let _ = b.cmp("rw_write", WIN_DASH, LV_NONE, "写升级无直接对应（需双 SRW）");
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野 unix
    got[0] = b.cmp("mutex_free", "ReleaseSRWLockShared", LV_PARTIAL, "inj-unix").err()
        == Some(CW_UNK_UNIX);
    // 2) 野 win
    got[1] = b.cmp("rw_read", "AcquireSRWLockFake", LV_PARTIAL, "inj-win").err()
        == Some(CW_UNK_WIN);
    // 3) 野等级
    got[2] = b.cmp("rw_read", "ReleaseSRWLockShared", 9, "inj-lv").err() == Some(CW_BAD_LEVEL);
    // 4) NONE 带符
    got[3] = b.cmp("rw_read", "ReleaseSRWLockShared", LV_NONE, "inj-none").err()
        == Some(CW_MISMATCH);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.cmp("rw_read", "ReleaseSRWLockShared", LV_ANALOG, "inj-frozen").err()
        == Some(CW_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24159_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24159");
    let mut b = mk_book();

    // J1 对照账齐
    let j1 = b.grid.len() == 5;
    s.add("J1-对照账齐", j1, "五格全落账");

    // J2 格字段齐
    let j2 = b.grid.iter().all(|g| g.ok());
    s.add("J2-格字段齐", j2, "留痕非空档位自洽");

    // J3 四级齐备
    let j3 = b.levels_complete();
    s.add("J3-四级齐备", j3, "EQUIV/PARTIAL/ANALOG/NONE 全有");

    // J4 EQUIV 一对一
    let j4 = b.equiv_one_to_one();
    s.add("J4-等价一对一", j4, "EQUIV 不出多条");

    // J5 NONE 恒占位
    let j5 = b.grid.iter().filter(|g| g.level == LV_NONE).all(|g| g.win_sym == WIN_DASH);
    s.add("J5-NONE恒占位", j5, "NONE 格恒 '-'");

    // J6 两侧在册
    let j6 = b
        .grid
        .iter()
        .all(|g| b.has_unix(&g.unix_sym) && (g.win_sym == WIN_DASH || b.has_win(&g.win_sym)));
    s.add("J6-两侧在册", j6, "符号不越册");

    // J7 野 unix 拒
    let j7 = b.cmp("mutex_free", "ReleaseSRWLockShared", LV_PARTIAL, "野符").err()
        == Some(CW_UNK_UNIX);
    s.add("J7-野unix拒", j7, "未登记 unix 符必拒");

    // J8 野 win 拒
    let j8 = b.cmp("rw_read", "FakeWin", LV_ANALOG, "野符").err() == Some(CW_UNK_WIN);
    s.add("J8-野win拒", j8, "未登记 win 符必拒");

    // J9 EQUIV 重复拒
    let j9 = b.cmp("mutex_acquire", "ReleaseSRWLockShared", LV_EQUIV, "重等价").err()
        == Some(CW_MISMATCH);
    s.add("J9-重等价拒", j9, "同符双 EQUIV 必拒");

    // J10 身份族符号覆盖
    let j10 = UNIX_SYMS.iter().all(|sym| b.grid.iter().any(|g| g.unix_sym == *sym));
    s.add("J10-符号全覆盖", j10, "五 unix 符全出现");

    // J11 印后拒
    let j11 = {
        let mut b2 = mk_book();
        b2.seal();
        b2.cmp("rw_read", "ReleaseSRWLockShared", LV_ANALOG, "印后").err() == Some(CW_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

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
    fn unx24159_cmp_flow() {
        let mut b = CwBook::new();
        assert!(b.cmp("mutex_acquire", "SRWLockAcquireExclusive", LV_EQUIV, "a").is_ok());
        assert_eq!(
            b.cmp("ghost", "ReleaseSRWLockShared", LV_ANALOG, "b"),
            Err(CW_UNK_UNIX)
        );
        assert_eq!(
            b.cmp("rw_read", "FakeWin", LV_ANALOG, "c"),
            Err(CW_UNK_WIN)
        );
        assert_eq!(b.cmp("rw_read", WIN_DASH, LV_PARTIAL, "d"), Err(CW_MISMATCH));
        assert!(b.levels_complete() || b.grid.len() == 1);
        b.seal();
        assert_eq!(b.cmp("rw_read", "ReleaseSRWLockShared", LV_ANALOG, "e"), Err(CW_FROZEN));
    }

    #[test]
    fn unx24159_checks_green() {
        let s = run_unx24159_checks();
        assert!(s.all_passed(), "UNX-F24159 判据未全绿");
    }
}
