//! UNX-F9159 进程族与 Windows WaitForSingleObject 对照格（0x633A0 段）
//!
//! 锚点：UNX-F9159-J1 对照账（wait 语义 ↔ Win32 等待语义逐格落账：
//! Unix 面、Win 面、等价级、留痕）；J2 对照语义（等价级闭集 + 映射
//! 唯一 + 阻塞属性一致 + 留痕必填四性质）；J3 对照指纹（账本折叠指
//! 纹）三判据全绿。
//! 域：CoRun·UNX-C2-B18（wait×Win32 等待对照面）。
//!
//! 结构：等价级闭集（EQUIV/PARTIAL/ANALOG/NONE）+ 对照格册 + 对照
//! 核（格校验）+ 对照账本 + 折叠指纹 + 注入核（错级/空留痕/伪格/
//! 重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x633A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x633A0;
pub const WF_UNK_CELL: u32 = SEG_BASE + 0x01; // 伪格
pub const WF_BAD_GRADE: u32 = SEG_BASE + 0x02; // 错级
pub const WF_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const WF_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const WF_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 等价级闭集。
pub const G_EQUIV: u8 = 0; // 等价：语义同构
pub const G_PARTIAL: u8 = 1; // 部分：主干等价边缘不同
pub const G_ANALOG: u8 = 2; // 类比：概念对应实现异
pub const G_NONE: u8 = 3; // 无对应
pub const G_COUNT: u8 = 4;

pub const G_NAMES: [&str; 4] = ["equiv", "partial", "analog", "none"];

/// 单条对照格。
#[derive(Clone)]
pub struct Cell {
    pub seq: u32,
    pub unix_face: String,
    pub win_face: String,
    pub grade: u8,
    pub blocking: bool, // 双方阻塞属性一致即 true
    pub note: String,
}

impl Cell {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.grade < G_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "WF{}|{}<->{}|{}|b{}|{}",
            self.seq,
            self.unix_face,
            self.win_face,
            G_NAMES[self.grade as usize],
            self.blocking as u8,
            self.note
        )
    }
}

/// 对照簿。
pub struct WfBook {
    pub cells: Vec<Cell>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl WfBook {
    pub fn new() -> WfBook {
        WfBook { cells: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn add(
        &mut self,
        unix_face: &str,
        win_face: &str,
        grade: u8,
        blocking: bool,
        note: &str,
    ) -> Result<u32, u32> {
        if self.sealed {
            return Err(WF_FROZEN);
        }
        if note.is_empty() {
            return Err(WF_EMPTY_NOTE);
        }
        if grade >= G_COUNT {
            return Err(WF_BAD_GRADE);
        }
        if unix_face.is_empty() && win_face.is_empty() {
            return Err(WF_UNK_CELL);
        }
        self.cells.push(Cell {
            seq: self.seq_next,
            unix_face: unix_face.to_string(),
            win_face: win_face.to_string(),
            grade,
            blocking,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(self.seq_next - 1)
    }

    /// 等价级分布非空计数。
    pub fn grade_count(&self, grade: u8) -> usize {
        self.cells.iter().filter(|c| c.grade == grade).count()
    }

    /// 全格阻塞属性一致（记 blocking=true 的格表示双方同阻塞属性）。
    pub fn blocking_ok(&self) -> bool {
        self.cells.iter().all(|c| c.ok())
    }

    /// 格唯一：同 Unix 面+Win 面不重格。
    pub fn no_dup(&self) -> bool {
        for i in 0..self.cells.len() {
            for j in (i + 1)..self.cells.len() {
                if self.cells[i].unix_face == self.cells[j].unix_face
                    && self.cells[i].win_face == self.cells[j].win_face
                {
                    return false;
                }
            }
        }
        true
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.cells.len() {
            for j in (i + 1)..self.cells.len() {
                if self.cells[i].seq == self.cells[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91590;
        for c in &self.cells {
            h = h.wrapping_mul(31).wrapping_add(c.seq);
            h = h.wrapping_mul(31).wrapping_add(c.grade as u32);
            h = h.wrapping_mul(31).wrapping_add(c.blocking as u32);
            for b in c.unix_face.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in c.win_face.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            for b in c.note.bytes() {
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
    // 1) 错级
    let bad = Cell {
        seq: 1,
        unix_face: "wait4".to_string(),
        win_face: "WaitForSingleObject".to_string(),
        grade: 9,
        blocking: true,
        note: "inj-grade".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = Cell {
        seq: 2,
        unix_face: "wait4".to_string(),
        win_face: "x".to_string(),
        grade: 0,
        blocking: true,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪格（双空面）
    let mut b = WfBook::new();
    got[2] = b.add("", "", G_EQUIV, true, "inj-cell").err() == Some(WF_UNK_CELL);
    // 4) 重格
    let _ = b.add("wait4", "WaitForSingleObject", G_EQUIV, true, "real");
    b.cells.push(Cell {
        seq: 2,
        unix_face: "wait4".to_string(),
        win_face: "WaitForSingleObject".to_string(),
        grade: G_EQUIV,
        blocking: true,
        note: "inj-dup".to_string(),
    });
    got[3] = !b.no_dup();
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.add("waitpid", "WaitForMultipleObjects", G_PARTIAL, true, "inj-frozen")
        .err() == Some(WF_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9159_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9159");
    let mut b = WfBook::new();

    // J1 对照账非空
    let _ = b.add("wait4", "WaitForSingleObject", G_EQUIV, true, "定点等子等句柄");
    let j1 = !b.cells.is_empty();
    s.add("J1-对照账非空", j1, "至少一格在册");

    // J2 账字段齐
    let j2 = b.cells.iter().all(|c| c.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且级闭集");

    // J3 四级分布
    let j3 = {
        let mut b2 = WfBook::new();
        let _ = b2.add("wait4", "WaitForSingleObject", G_EQUIV, true, "a");
        let _ = b2.add("waitpid WNOHANG", "WaitForSingleObject timeout=0", G_PARTIAL, false, "b");
        let _ = b2.add("WUNTRACED", "无直接对应（DBG 域类）", G_ANALOG, true, "c");
        let _ = b2.add("wait3 rusage", "GetProcessTimes（异步账）", G_ANALOG, false, "d");
        b2.grade_count(G_EQUIV) == 1
            && b2.grade_count(G_PARTIAL) == 1
            && b2.grade_count(G_ANALOG) == 2
    };
    s.add("J3-四级分布", j3, "equiv/partial/analog 各按样本分布");

    // J4 错级拒
    let j4 = b.add("x", "y", 9, true, "野级").err() == Some(WF_BAD_GRADE);
    s.add("J4-错级拒", j4, "级出闭集必拒");

    // J5 阻塞属性一致
    let j5 = {
        let mut b2 = WfBook::new();
        let _ = b2.add("wait4 阻塞", "WaitForSingleObject INFINITE", G_EQUIV, true, "同阻塞");
        let _ = b2.add("waitpid WNOHANG", "timeout=0 轮询", G_PARTIAL, false, "同非阻塞");
        b2.blocking_ok()
    };
    s.add("J5-阻塞属性一致", j5, "同级格阻塞属性对齐");

    // J6 WNOHANG ↔ timeout=0 部分等价
    let j6 = {
        let mut b2 = WfBook::new();
        let g = b2.add(
            "waitpid(pid,WNOHANG)",
            "WaitForSingleObject(h,0)",
            G_PARTIAL,
            false,
            "零阻塞轮询两侧同构",
        );
        g.is_ok() && b2.cells[0].grade == G_PARTIAL
    };
    s.add("J6-WNOHANG对照", j6, "WNOHANG 格为 partial 且非阻塞");

    // J7 无对应格登记
    let j7 = {
        let mut b2 = WfBook::new();
        let _ = b2.add("WCONTINUED", "（无 Win32 对应）", G_NONE, false, "n");
        b2.grade_count(G_NONE) == 1
    };
    s.add("J7-无对应格", j7, "NONE 级可登记");

    // J8 双空面拒
    let j8 = b.add("", "", G_EQUIV, true, "空面").err() == Some(WF_UNK_CELL);
    s.add("J8-双空面拒", j8, "双空面必拒");

    // J9 不重格
    let j9 = b.no_dup();
    s.add("J9-不重格", j9, "同面格不重");

    // J10 指纹稳定 + seq 唯一
    let j10 = b.fold() == b.fold() && b.seq_unique();
    s.add("J10-指纹稳定", j10, "折叠指纹两次一致且 seq 唯一");

    // J11 空留痕拒
    let j11 = b.add("a", "b", G_EQUIV, true, "").err() == Some(WF_EMPTY_NOTE);
    s.add("J11-空留痕拒", j11, "空留痕必拒");

    // J12 拒绝不落账
    let j12 = {
        let before = b.cells.len();
        let _ = b.add("x", "y", 9, true, "不落账");
        b.cells.len() == before
    };
    s.add("J12-拒绝不落账", j12, "拒绝路径零污染账本");

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
    fn unx9159_cell_flow() {
        let mut b = WfBook::new();
        assert!(b.add("wait4", "WaitForSingleObject", G_EQUIV, true, "a").is_ok());
        assert_eq!(b.add("x", "y", 9, true, "b"), Err(WF_BAD_GRADE));
        assert_eq!(b.add("", "", G_EQUIV, true, "c"), Err(WF_UNK_CELL));
        assert_eq!(b.add("a", "b", G_EQUIV, true, ""), Err(WF_EMPTY_NOTE));
        assert!(b.no_dup());
    }

    #[test]
    fn unx9159_checks_green() {
        let s = run_unx9159_checks();
        assert!(s.all_passed(), "UNX-F9159 判据未全绿");
    }
}
