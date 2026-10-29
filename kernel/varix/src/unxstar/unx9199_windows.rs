//! UNX-F9199 身份族与 Windows 令牌特权对照格（0x638A0 段）
//!
//! 锚点：UNX-F9199-J1 对照账（Unix 身份面符号 ↔ Win32 令牌特权符
//! 号逐格落账）；J2 对照语义（两侧在册闸 + 四级闭集 + EQUIV 一对一
//! + NONE 恒 '-' 四性质）；J3 对照指纹（格子折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B20（身份族跨系对照格面）。
//!
//! 结构：双侧符号册 + 对照格核 + 格账本 + 折叠指纹 + 注入核（野
//! unix/野 win/野等级/EQUIV 空档/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x638A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x638A0;
pub const CG_UNK_UNIX: u32 = SEG_BASE + 0x01; // 野 unix 符号
pub const CG_UNK_WIN: u32 = SEG_BASE + 0x02; // 野 win 符号
pub const CG_BAD_LEVEL: u32 = SEG_BASE + 0x03; // 等级出闭集
pub const CG_LEVEL_MISMATCH: u32 = SEG_BASE + 0x04; // 等级与档位不符
pub const CG_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 对照等级闭集。
pub const L_EQUIV: u8 = 0; // 语义等价
pub const L_PARTIAL: u8 = 1; // 部分等价
pub const L_ANALOG: u8 = 2; // 仅类比
pub const L_NONE: u8 = 3; // 无对应
pub const L_COUNT: u8 = 4;

pub const L_NAMES: [&str; 4] = ["equiv", "partial", "analog", "none"];

/// 无对应占位符。
pub const WIN_DASH: &str = "-";

/// Unix 身份面符号册（核心六符号）。
pub const UNIX_SYMS: [&str; 6] =
    ["setuid", "setgid", "getgroups", "umask", "setfsuid", "setresuid"];

/// Win32 令牌特权符号册（核心六符号 + 占位）。
pub const WIN_SYMS: [&str; 6] = [
    "AdjustTokenPrivileges",
    "CheckTokenMembership",
    "CreateProcessAsUser",
    "SetTokenInformation",
    "ImpersonateNamedPipeClient",
    "RevertToSelf",
];

/// 单条对照格。
#[derive(Clone)]
pub struct CgRec {
    pub seq: u32,
    pub unix_sym: String,
    pub win_sym: String,
    pub level: u8,
    pub note: String,
}

impl CgRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.level < L_COUNT
            && !self.unix_sym.is_empty()
            && match self.level {
                L_NONE => self.win_sym == WIN_DASH,
                _ => !self.win_sym.is_empty() && self.win_sym != WIN_DASH,
            }
    }

    pub fn render_line(&self) -> String {
        format!(
            "CG{}|{}<->{}|{}|{}",
            self.seq,
            self.unix_sym,
            self.win_sym,
            L_NAMES[self.level as usize],
            self.note
        )
    }
}

/// 对照格簿。
pub struct CgBook {
    pub unix_syms: Vec<String>,
    pub win_syms: Vec<String>,
    pub grid: Vec<CgRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl CgBook {
    pub fn new() -> CgBook {
        let mut b = CgBook {
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

    pub fn has_unix(&self, sym: &str) -> bool {
        self.unix_syms.iter().any(|s| s == sym)
    }

    pub fn has_win(&self, sym: &str) -> bool {
        self.win_syms.iter().any(|s| s == sym)
    }

    /// 登记一条对照格。
    pub fn cmp(&mut self, unix_sym: &str, win_sym: &str, level: u8, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(CG_FROZEN);
        }
        if note.is_empty() {
            return Err(CG_BAD_LEVEL);
        }
        if level >= L_COUNT {
            return Err(CG_BAD_LEVEL);
        }
        if !self.has_unix(unix_sym) {
            return Err(CG_UNK_UNIX);
        }
        if level == L_NONE {
            if win_sym != WIN_DASH {
                return Err(CG_LEVEL_MISMATCH);
            }
        } else {
            if win_sym == WIN_DASH || win_sym.is_empty() {
                return Err(CG_LEVEL_MISMATCH);
            }
            if !self.has_win(win_sym) {
                return Err(CG_UNK_WIN);
            }
            // EQUIV 一对一：同一 unix 符号不得有第二条 EQUIV
            if level == L_EQUIV
                && self
                    .grid
                    .iter()
                    .any(|g| g.unix_sym == unix_sym && g.level == L_EQUIV)
            {
                return Err(CG_LEVEL_MISMATCH);
            }
        }
        // 同对 (unix,win,level) 不得重复登记
        if self
            .grid
            .iter()
            .any(|g| g.unix_sym == unix_sym && g.win_sym == win_sym && g.level == level)
        {
            return Err(CG_LEVEL_MISMATCH);
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        self.grid.push(CgRec {
            seq,
            unix_sym: unix_sym.to_string(),
            win_sym: win_sym.to_string(),
            level,
            note: note.to_string(),
        });
        Ok(seq)
    }

    /// 四级齐备。
    pub fn levels_complete(&self) -> bool {
        for l in 0..L_COUNT {
            if !self.grid.iter().any(|g| g.level == l) {
                return false;
            }
        }
        true
    }

    /// EQUIV 一对一全局复核。
    pub fn equiv_one_to_one(&self) -> bool {
        for i in 0..self.grid.len() {
            for j in (i + 1)..self.grid.len() {
                let (a, b) = (&self.grid[i], &self.grid[j]);
                if a.level == L_EQUIV && b.level == L_EQUIV {
                    if a.unix_sym == b.unix_sym {
                        return false;
                    }
                    if a.win_sym == b.win_sym {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 无等级漂移（同对同格重复即漂移，已在登记时拒，此处账面复核）。
    pub fn no_drift(&self) -> bool {
        for i in 0..self.grid.len() {
            for j in (i + 1)..self.grid.len() {
                let (a, b) = (&self.grid[i], &self.grid[j]);
                if a.unix_sym == b.unix_sym && a.win_sym == b.win_sym && a.level != b.level {
                    return false;
                }
            }
        }
        true
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.grid.len() {
            for j in (i + 1)..self.grid.len() {
                if self.grid[i].seq == self.grid[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x919A0;
        for g in &self.grid {
            h = h.wrapping_mul(31).wrapping_add(g.seq);
            h = h.wrapping_mul(31).wrapping_add(g.level as u32);
            for by in g.unix_sym.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
            for by in g.win_sym.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
            for by in g.note.bytes() {
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

/// 基础样本：四级齐备 + 身份族核心符号全覆盖。
pub fn mk_book() -> CgBook {
    let mut b = CgBook::new();
    let _ = b.cmp("setresuid", "SetTokenInformation", L_EQUIV, "四元身份整体置换语义等价");
    let _ = b.cmp("setuid", "CreateProcessAsUser", L_PARTIAL, "仅进程创建点生效, 非即时换身份");
    let _ = b.cmp("umask", "AdjustTokenPrivileges", L_ANALOG, "创建掩码类比特权禁用掩码");
    let _ = b.cmp("setfsuid", WIN_DASH, L_NONE, "文件系统临时 uid 无 Win32 对应");
    let _ = b.cmp("getgroups", "CheckTokenMembership", L_PARTIAL, "逐组查询非批量枚举");
    let _ = b.cmp("setgid", "ImpersonateNamedPipeClient", L_ANALOG, "组身份类比管道模拟");
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 野 unix 符号
    let mut b = mk_book();
    got[0] = b.cmp("setpgrp", "RevertToSelf", L_EQUIV, "inj-unix").err() == Some(CG_UNK_UNIX);
    // 2) 野 win 符号
    got[1] = b
        .cmp("setuid", "ImpersonateSelf", L_PARTIAL, "inj-win")
        .err()
        == Some(CG_UNK_WIN);
    // 3) 等级出闭集
    got[2] = b.cmp("setuid", "RevertToSelf", 9, "inj-level").err() == Some(CG_BAD_LEVEL);
    // 4) EQUIV 带 '-'（等级与档位不符）
    got[3] = b
        .cmp("setuid", WIN_DASH, L_EQUIV, "inj-dash")
        .err()
        == Some(CG_LEVEL_MISMATCH);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b
        .cmp("setuid", "RevertToSelf", L_ANALOG, "inj-frozen")
        .err()
        == Some(CG_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9199_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9199");
    let mut b = mk_book();

    // J1 对照账非空且至少 5 格
    let j1 = b.grid.len() >= 5;
    s.add("J1-对照账非空", j1, "基础样本 6 格");

    // J2 格字段齐
    let j2 = b.grid.iter().all(|g| g.ok());
    s.add("J2-格字段齐备", j2, "留痕非空且等级/档位自洽");

    // J3 四级齐备
    let j3 = b.levels_complete();
    s.add("J3-四级齐备", j3, "EQUIV/PARTIAL/ANALOG/NONE 全有");

    // J4 NONE 恒 '-'
    let j4 = b.grid.iter().filter(|g| g.level == L_NONE).all(|g| g.win_sym == WIN_DASH);
    s.add("J4-NONE恒占位", j4, "NONE 格 win 侧恒 '-'");

    // J5 非NONE 档真符号
    let j5 = b
        .grid
        .iter()
        .filter(|g| g.level != L_NONE)
        .all(|g| !g.win_sym.is_empty() && g.win_sym != WIN_DASH);
    s.add("J5-非NONE真符号", j5, "非 NONE 格 win 侧必真符号");

    // J6 EQUIV 一对一
    let j6 = b.equiv_one_to_one();
    s.add("J6-等价一对一", j6, "EQUIV 两侧均不出多条");

    // J7 同对无重复
    let j7 = b
        .cmp("setresuid", "SetTokenInformation", L_EQUIV, "重复对")
        .err()
        == Some(CG_LEVEL_MISMATCH);
    s.add("J7-同对重复拒", j7, "(unix,win,level) 三元重复必拒");

    // J8 侧符号全在册
    let j8 = b.grid.iter().all(|g| {
        b.has_unix(&g.unix_sym) && (g.win_sym == WIN_DASH || b.has_win(&g.win_sym))
    });
    s.add("J8-两侧全在册", j8, "unix/win 符号均不越册");

    // J9 身份族核心符号覆盖
    let j9 = UNIX_SYMS.iter().all(|sym| b.grid.iter().any(|g| g.unix_sym == *sym));
    s.add("J9-身份族全覆盖", j9, "六身份符号全出现于对照格");

    // J10 无等级漂移
    let j10 = b.no_drift();
    s.add("J10-无等级漂移", j10, "同对不出现相异等级");

    // J11 野等级拒
    let j11 = b.cmp("umask", "RevertToSelf", L_COUNT, "野等级").err() == Some(CG_BAD_LEVEL);
    s.add("J11-野等级拒", j11, "等级出闭集必拒");

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
    fn unx9199_grid_flow() {
        let mut b = CgBook::new();
        assert!(b.cmp("setresuid", "SetTokenInformation", L_EQUIV, "a").is_ok());
        assert!(b.cmp("setfsuid", WIN_DASH, L_NONE, "b").is_ok());
        assert_eq!(b.cmp("setpgrp", "RevertToSelf", L_EQUIV, "c"), Err(CG_UNK_UNIX));
        assert_eq!(b.cmp("setuid", "ImpersonateSelf", L_ANALOG, "d"), Err(CG_UNK_WIN));
        assert_eq!(b.cmp("setuid", WIN_DASH, L_PARTIAL, "e"), Err(CG_LEVEL_MISMATCH));
        assert_eq!(b.grid.len(), 2);
    }

    #[test]
    fn unx9199_checks_green() {
        let s = run_unx9199_checks();
        assert!(s.all_passed(), "UNX-F9199 判据未全绿");
    }
}
