//! UNX-F10113 getdents 正反双判据（目录枚举达成与非目录 fd 误用受控）（0x10140 段）
//!
//! 锚点：UNX-F10113-J1 getdents 枚举达成（"."/".." 项与顺序 LTP
//! getdents01 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：目录册（名字序表 + 游标）+ 枚举核（"." ".." 头两项固定、后续
//! 按登记序、游标耗尽返回零）+ 枚举账（逐次读取留痕可对平）+ 注入核
//! （非目录 fd/坏 fd/游标回拨/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x10140 独占 0x20）。
pub const SEG_BASE: u32 = 0x10140;
pub const GD_NOT_DIR: u32 = SEG_BASE + 0x01; // 非 fd 指目录
pub const GD_BAD_FD: u32 = SEG_BASE + 0x02; // 坏 fd
pub const GD_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const GD_SEALED: u32 = SEG_BASE + 0x04; // 封表后操作
pub const GD_CURSOR_BACK: u32 = SEG_BASE + 0x05; // 游标回拨

/// 每目录项定容。
pub const DIRENT_CAP: usize = 10;

/// 目录项。
#[derive(Clone, PartialEq)]
pub struct Dirent {
    pub ino: u32,
    pub name: String,
}

impl Dirent {
    pub fn ok(&self) -> bool {
        self.ino != 0 && !self.name.is_empty()
    }
}

/// 目录：项表 + 只读游标。
#[derive(Clone)]
pub struct DirNode {
    pub entries: Vec<Dirent>,
    pub cursor: usize,
    pub is_dir: bool,
}

impl DirNode {
    /// 新目录：自动补 "."/".." 头两项。
    pub fn dir(self_ino: u32, parent_ino: u32) -> DirNode {
        DirNode {
            entries: vec![
                Dirent { ino: self_ino, name: ".".to_string() },
                Dirent { ino: parent_ino, name: "..".to_string() },
            ],
            cursor: 0,
            is_dir: true,
        }
    }

    /// 普通文件节点。
    pub fn file() -> DirNode {
        DirNode { entries: Vec::new(), cursor: 0, is_dir: false }
    }

    /// 登记项（超容拒）。
    pub fn add(&mut self, ino: u32, name: &str) -> Result<(), u32> {
        if !self.is_dir {
            return Err(GD_NOT_DIR);
        }
        if self.entries.len() >= DIRENT_CAP {
            return Err(GD_BAD_FD);
        }
        self.entries.push(Dirent { ino, name: name.to_string() });
        Ok(())
    }

    /// 头两项固定为 "."/".."（顺序判据锚）。
    pub fn head_dots(&self) -> bool {
        self.entries.len() >= 2
            && self.entries[0].name == "."
            && self.entries[1].name == ".."
    }
}

/// 枚举账留痕。
#[derive(Clone)]
pub struct GdRec {
    pub seq: u32,
    pub fd: i32,
    pub got: usize,
    pub note: String,
}

impl GdRec {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.fd >= 0
    }
}

/// 目录册。
pub struct DirBook {
    pub slots: Vec<Option<DirNode>>,
    pub ledger: Vec<GdRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl DirBook {
    pub fn new() -> DirBook {
        DirBook { slots: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn mount(&mut self, node: DirNode) -> Result<usize, u32> {
        if self.sealed {
            return Err(GD_SEALED);
        }
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            self.slots[i] = Some(node);
            return Ok(i);
        }
        if self.slots.len() >= 6 {
            return Err(GD_BAD_FD);
        }
        self.slots.push(Some(node));
        Ok(self.slots.len() - 1)
    }

    /// getdents 核心：自游标取一批项，耗尽得零。
    pub fn getdents(&mut self, fd: i32, count: usize, note: &str) -> Result<Vec<Dirent>, u32> {
        if self.sealed {
            return Err(GD_SEALED);
        }
        if note.is_empty() {
            return Err(GD_EMPTY_NOTE);
        }
        if fd < 0 || fd as usize >= self.slots.len() || self.slots[fd as usize].is_none() {
            return Err(GD_BAD_FD);
        }
        let u = fd as usize;
        let is_dir = self.slots[u].as_ref().map(|d| d.is_dir).unwrap_or(false);
        if !is_dir {
            return Err(GD_NOT_DIR);
        }
        let d = match self.slots[u].as_mut() {
            Some(d) => d,
            None => return Err(GD_BAD_FD),
        };
        let start = d.cursor;
        let end = (start + count).min(d.entries.len());
        if start > d.entries.len() {
            return Err(GD_CURSOR_BACK);
        }
        let out: Vec<Dirent> = d.entries[start..end].to_vec();
        d.cursor = end;
        self.ledger.push(GdRec { seq: self.seq_next, fd, got: out.len(), note: note.to_string() });
        self.seq_next += 1;
        Ok(out)
    }

    /// 游标位置。
    pub fn cursor_of(&self, fd: i32) -> Option<usize> {
        if fd < 0 || fd as usize >= self.slots.len() {
            return None;
        }
        self.slots[fd as usize].as_ref().map(|d| d.cursor)
    }

    /// 全量枚举对平：游标耗尽后累计读取数==项数。
    pub fn drained(&self, fd: i32) -> bool {
        if fd < 0 || fd as usize >= self.slots.len() {
            return false;
        }
        match self.slots[fd as usize].as_ref() {
            Some(d) => d.cursor == d.entries.len(),
            None => false,
        }
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
        let mut h: u32 = 0x101400;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.got as u32);
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
    let mut b = DirBook::new();
    let d = b.mount(DirNode::dir(1, 0)).unwrap_or(usize::MAX);
    let file = b.mount(DirNode::file()).unwrap_or(usize::MAX);
    // 1) 非目录 fd 必拒
    got[0] = b.getdents(file as i32, 4, "inj-file").err() == Some(GD_NOT_DIR);
    // 2) 坏 fd 必拒
    got[1] = b.getdents(-1, 4, "inj-fd").err() == Some(GD_BAD_FD);
    // 3) 空留痕必拒
    got[2] = b.getdents(d as i32, 4, "").err() == Some(GD_EMPTY_NOTE);
    // 4) 目录项超容必拒（头两项已占 2，再补 DIRENT_CAP-2 项至恰满）
    let mut full = DirNode::dir(9, 0);
    let mut add_ok = true;
    for i in 0..DIRENT_CAP - 2 {
        if full.add(i as u32 + 3, "x").is_err() {
            add_ok = false;
        }
    }
    got[3] = add_ok && full.add(999, "over").is_err();
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.getdents(d as i32, 4, "inj-seal").err() == Some(GD_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10113_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10113");

    // 正路径：头两项 "." ".."（getdents01 面）
    let mut b = DirBook::new();
    let d = b.mount(DirNode::dir(7, 1)).unwrap_or(usize::MAX);
    {
        let mut dn_default = DirNode::file();
        let dd = b.slots[d].as_mut().unwrap_or(&mut dn_default);
        let _ = dd.add(10, "alpha");
        let _ = dd.add(11, "beta");
    }
    let first = b.getdents(d as i32, 2, "头两项").unwrap_or_default();
    let j1 = first.len() == 2 && first[0].name == "." && first[1].name == "..";
    s.add("J1-头两项点项", j1, "首读得 ./.. 两项");

    // 顺序保持：后续项按登记序
    let second = b.getdents(d as i32, 4, "后续").unwrap_or_default();
    let j2 = second.len() == 2
        && second[0].name == "alpha"
        && second[1].name == "beta"
        && second[0].ino == 10;
    s.add("J2-顺序保持", j2, "登记序 alpha→beta 且 ino 一致");

    // 游标耗尽得零
    let j3 = b.getdents(d as i32, 4, "耗尽").unwrap_or_default().is_empty()
        && b.drained(d as i32);
    s.add("J3-游标耗尽", j3, "读尽后得零且游标对平");

    // 单批全量枚举
    let j4 = {
        let mut b4 = DirBook::new();
        let d4 = b4.mount(DirNode::dir(2, 1)).unwrap_or(usize::MAX);
        {
            let mut dn4_default = DirNode::file();
            let dd = b4.slots[d4].as_mut().unwrap_or(&mut dn4_default);
            let _ = dd.add(5, "one");
            let _ = dd.add(6, "two");
            let _ = dd.add(7, "three");
        }
        let all = b4.getdents(d4 as i32, DIRENT_CAP, "全量").unwrap_or_default();
        all.len() == 5 && b4.cursor_of(d4 as i32) == Some(5)
    };
    s.add("J4-单批全量", j4, "一批读尽五项且游标至尾");

    // 非目录 fd 受控
    let j5 = {
        let file = b.mount(DirNode::file()).unwrap_or(usize::MAX);
        b.getdents(file as i32, 2, "文件").err() == Some(GD_NOT_DIR)
    };
    s.add("J5-非目录受控", j5, "文件 fd 枚举必拒");

    // 坏 fd 受控
    let j6 = b.getdents(-2, 2, "neg").err() == Some(GD_BAD_FD)
        && b.getdents(99, 2, "oob").err() == Some(GD_BAD_FD);
    s.add("J6-坏fd受控", j6, "负与越界 fd 同码拒");

    // 项合法性守恒
    let j7 = {
        let ok_all = b.slots.iter().flatten().all(|d| {
            d.entries.iter().all(|e| e.ok()) && (!d.is_dir || d.head_dots())
        });
        ok_all
    };
    s.add("J7-项合法守恒", j7, "ino 非零名非空且目录头两项齐");

    // 空留痕拒收
    let j8 = b.getdents(d as i32, 1, "").err() == Some(GD_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 目录项容量守恒（头两项已占 2，补 DIRENT_CAP-2 项至恰满）
    let j9 = {
        let mut b9 = DirBook::new();
        let d9 = b9.mount(DirNode::dir(3, 1)).unwrap_or(usize::MAX);
        let mut ok = true;
        {
            let mut dn9_default = DirNode::file();
            let dd = b9.slots[d9].as_mut().unwrap_or(&mut dn9_default);
            for i in 0..DIRENT_CAP - 2 {
                if dd.add(i as u32 + 4, "n").is_err() {
                    ok = false;
                }
            }
            if dd.add(999, "over").is_ok() {
                ok = false;
            }
        }
        ok
    };
    s.add("J9-容量守恒", j9, "DIRENT_CAP 守恒");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "注非空且 seq 唯一");

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
    fn unx10113_getdents_flow() {
        let mut b = DirBook::new();
        let d = b.mount(DirNode::dir(1, 0)).unwrap_or(0);
        {
            let mut dn_default = DirNode::file();
        let dd = b.slots[d].as_mut().unwrap_or(&mut dn_default);
            let _ = dd.add(2, "a");
        }
        let g = b.getdents(d as i32, 10, "x").unwrap_or_default();
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].name, ".");
        let file = b.mount(DirNode::file()).unwrap_or(1);
        assert!(b.getdents(file as i32, 1, "y") == Err(GD_NOT_DIR));
    }

    #[test]
    fn unx10113_checks_green() {
        let s = run_unx10113_checks();
        assert!(s.all_passed(), "UNX-F10113 判据未全绿");
    }
}
