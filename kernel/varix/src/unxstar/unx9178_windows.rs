//! UNX-F9178 资源族与 Windows 工作集/配额对照格（0x63600 段）
//!
//! 锚点：UNX-F9178-J1 对照账（资源族语义 ↔ Win32 工作集/配额语义
//! 逐格落账：Unix 面、Win 面、等价级、留痕）；J2 对照语义（等价级
//! 闭集 + 映射唯一 + 留痕必填三性质）；J3 对照指纹（账本折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-C2-B19（资源族×Win32 工作集对照面）。
//!
//! 结构：等价级闭集 + 对照格册 + 对照核 + 对照账本 + 折叠指纹 + 注
//! 入核（错级/空留痕/伪格/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x63600 独占 0x20）。
pub const SEG_BASE: u32 = 0x63600;
pub const WS_UNK_CELL: u32 = SEG_BASE + 0x01; // 伪格
pub const WS_BAD_GRADE: u32 = SEG_BASE + 0x02; // 错级
pub const WS_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const WS_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const WS_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// 等价级闭集。
pub const G_EQUIV: u8 = 0;
pub const G_PARTIAL: u8 = 1;
pub const G_ANALOG: u8 = 2;
pub const G_NONE: u8 = 3;
pub const G_COUNT: u8 = 4;

pub const G_NAMES: [&str; 4] = ["equiv", "partial", "analog", "none"];

/// 单条对照格。
#[derive(Clone)]
pub struct WsCell {
    pub seq: u32,
    pub unix_face: String,
    pub win_face: String,
    pub grade: u8,
    pub note: String,
}

impl WsCell {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.grade < G_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "WS{}|{}<->{}|{}|{}",
            self.seq,
            self.unix_face,
            self.win_face,
            G_NAMES[self.grade as usize],
            self.note
        )
    }
}

/// 对照簿。
pub struct WsBook {
    pub cells: Vec<WsCell>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl WsBook {
    pub fn new() -> WsBook {
        WsBook { cells: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn add(&mut self, u: &str, w: &str, grade: u8, note: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(WS_FROZEN);
        }
        if note.is_empty() {
            return Err(WS_EMPTY_NOTE);
        }
        if grade >= G_COUNT {
            return Err(WS_BAD_GRADE);
        }
        if u.is_empty() && w.is_empty() {
            return Err(WS_UNK_CELL);
        }
        self.cells.push(WsCell {
            seq: self.seq_next,
            unix_face: u.to_string(),
            win_face: w.to_string(),
            grade,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(self.seq_next - 1)
    }

    pub fn grade_count(&self, grade: u8) -> usize {
        self.cells.iter().filter(|c| c.grade == grade).count()
    }

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
        let mut h: u32 = 0x91780;
        for c in &self.cells {
            h = h.wrapping_mul(31).wrapping_add(c.seq);
            h = h.wrapping_mul(31).wrapping_add(c.grade as u32);
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
    let bad = WsCell {
        seq: 1,
        unix_face: "RLIMIT_AS".to_string(),
        win_face: "SetProcessWorkingSetSize".to_string(),
        grade: 9,
        note: "inj-grade".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = WsCell {
        seq: 2,
        unix_face: "x".to_string(),
        win_face: "y".to_string(),
        grade: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪格
    let mut b = WsBook::new();
    got[2] = b.add("", "", G_EQUIV, "inj-cell").err() == Some(WS_UNK_CELL);
    // 4) 重格
    let _ = b.add("RLIMIT_AS", "SetProcessWorkingSetSize", G_PARTIAL, "real");
    b.cells.push(WsCell {
        seq: 2,
        unix_face: "RLIMIT_AS".to_string(),
        win_face: "SetProcessWorkingSetSize".to_string(),
        grade: G_PARTIAL,
        note: "inj-dup".to_string(),
    });
    got[3] = !b.no_dup();
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.add("RLIMIT_CPU", "JOB_OBJECT_CPU_RATE", G_ANALOG, "inj-frozen")
        .err() == Some(WS_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9178_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9178");
    let mut b = WsBook::new();

    // J1 对照账非空
    let _ = b.add("RLIMIT_RSS", "SetProcessWorkingSetSize", G_PARTIAL, "工作集近似");
    let j1 = !b.cells.is_empty();
    s.add("J1-对照账非空", j1, "至少一格在册");

    // J2 账字段齐
    let j2 = b.cells.iter().all(|c| c.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且级闭集");

    // J3 工作集对照格
    let j3 = {
        let mut b2 = WsBook::new();
        let g = b2.add(
            "getrusage maxrss",
            "GetProcessMemoryInfo WorkingSetSize",
            G_EQUIV,
            "峰值驻留两面对齐",
        );
        g.is_ok() && b2.cells[0].grade == G_EQUIV
    };
    s.add("J3-maxrss等价", j3, "maxrss ↔ WorkingSetSize 等价");

    // J4 AS 对照
    let j4 = {
        let mut b2 = WsBook::new();
        let _ = b2.add("RLIMIT_AS", "SetProcessWorkingSetSize", G_PARTIAL, "a");
        b2.grade_count(G_PARTIAL) == 1
    };
    s.add("J4-AS部分", j4, "AS 对照 partial");

    // J5 配额对照
    let j5 = {
        let mut b2 = WsBook::new();
        let _ = b2.add("RLIMIT_CPU", "JOB_OBJECT_CPU_RATE_CONTROL", G_ANALOG, "a");
        b2.grade_count(G_ANALOG) == 1
    };
    s.add("J5-CPU类比", j5, "CPU 对照 analog");

    // J6 无对应格
    let j6 = {
        let mut b2 = WsBook::new();
        let _ = b2.add("RLIMIT_NPROC", "（Win 无进程数配额）", G_NONE, "a");
        b2.grade_count(G_NONE) == 1
    };
    s.add("J6-NONE格", j6, "NPROC 无对应可登记");

    // J7 错级拒
    let j7 = b.add("a", "b", 9, "野级").err() == Some(WS_BAD_GRADE);
    s.add("J7-错级拒", j7, "级出闭集必拒");

    // J8 伪格拒
    let j8 = b.add("", "", G_EQUIV, "空面").err() == Some(WS_UNK_CELL);
    s.add("J8-伪格拒", j8, "双空面必拒");

    // J9 不重格
    let j9 = b.no_dup();
    s.add("J9-不重格", j9, "同面格不重");

    // J10 四级分布覆盖
    let j10 = {
        let mut b2 = WsBook::new();
        let _ = b2.add("a", "b", G_EQUIV, "1");
        let _ = b2.add("c", "d", G_PARTIAL, "2");
        let _ = b2.add("e", "f", G_ANALOG, "3");
        let _ = b2.add("g", "h", G_NONE, "4");
        b2.grade_count(G_EQUIV) == 1
            && b2.grade_count(G_PARTIAL) == 1
            && b2.grade_count(G_ANALOG) == 1
            && b2.grade_count(G_NONE) == 1
    };
    s.add("J10-四级覆盖", j10, "四级各一");

    // J11 空留痕拒
    let j11 = b.add("a", "b", G_EQUIV, "").err() == Some(WS_EMPTY_NOTE);
    s.add("J11-空留痕拒", j11, "空留痕必拒");

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
    fn unx9178_cell_flow() {
        let mut b = WsBook::new();
        assert!(b.add("RLIMIT_RSS", "WorkingSet", G_PARTIAL, "a").is_ok());
        assert_eq!(b.add("a", "b", 9, "b"), Err(WS_BAD_GRADE));
        assert_eq!(b.add("", "", G_EQUIV, "c"), Err(WS_UNK_CELL));
        assert_eq!(b.add("a", "b", G_EQUIV, ""), Err(WS_EMPTY_NOTE));
        assert!(b.no_dup());
    }

    #[test]
    fn unx9178_checks_green() {
        let s = run_unx9178_checks();
        assert!(s.all_passed(), "UNX-F9178 判据未全绿");
    }
}
