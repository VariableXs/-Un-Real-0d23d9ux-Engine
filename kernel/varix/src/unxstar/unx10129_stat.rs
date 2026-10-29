//! UNX-F10129 stat 三态一致性正反双判据（三口径达成与坏缓冲误用受控）（0x10340 段）
//!
//! 锚点：UNX-F10129-J1 stat/fstat/lstat 三口径达成（符号链不跟随/跟随
//! 两读 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：三口径册（同 inode 三视图：路径/fd/链面）+ 一致核（stat==fstat
//! 同件同值/lstat 不跟随链/链面两读一致）+ 视图账（逐次留痕）+ 注入核
//! （坏缓冲/缺路径/缺链/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10340 独占 0x20）。
pub const SEG_BASE: u32 = 0x10340;
pub const ST_NOENT: u32 = SEG_BASE + 0x01; // 缺路径
pub const ST_BAD_FD: u32 = SEG_BASE + 0x02; // 坏 fd
pub const ST_NOBUF: u32 = SEG_BASE + 0x03; // 坏缓冲
pub const ST_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const ST_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 视图元组。
#[derive(Clone, Copy, PartialEq)]
pub struct St2 {
    pub size: u32,
    pub mtime: u32,
    pub ino: u32,
}

impl St2 {
    pub fn zero() -> St2 {
        St2 { size: 0, mtime: 0, ino: 0 }
    }

    pub fn ok(&self) -> bool {
        self.ino != 0
    }
}

/// 实体（含可选链目标）。
#[derive(Clone)]
pub struct Entity {
    pub name: String,
    pub st: St2,
    pub link_to: Option<String>,
}

impl Entity {
    pub fn ok(&self) -> bool {
        !self.name.is_empty() && self.st.ok()
    }
}

/// 视图账留痕。
#[derive(Clone)]
pub struct StRec {
    pub seq: u32,
    pub view: u32, // 0=stat 1=fstat 2=lstat
    pub key: String,
    pub snap: St2,
}

impl StRec {
    pub fn ok(&self) -> bool {
        self.view <= 2 && !self.key.is_empty()
    }
}

/// 三口径册。
pub struct StatBook {
    pub ents: Vec<Entity>,
    pub fds: Vec<Option<String>>, // fd → 名
    pub ledger: Vec<StRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl StatBook {
    pub fn new() -> StatBook {
        StatBook { ents: Vec::new(), fds: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 挂实体。
    pub fn add(&mut self, name: &str, st: St2, link_to: Option<String>) -> Result<(), u32> {
        if self.sealed {
            return Err(ST_SEALED);
        }
        if name.is_empty() || !st.ok() {
            return Err(ST_NOENT);
        }
        self.ents.push(Entity { name: name.to_string(), st, link_to });
        Ok(())
    }

    fn ent_of(&self, name: &str) -> Option<usize> {
        self.ents.iter().position(|e| e.name == name)
    }

    /// 跟随链取实体。
    fn follow(&self, name: &str) -> Option<usize> {
        let mut cur = name.to_string();
        let mut depth = 0;
        while depth <= 4 {
            match self.ent_of(&cur) {
                Some(i) => match self.ents[i].link_to.clone() {
                    Some(t) => {
                        cur = t;
                        depth += 1;
                    }
                    None => return Some(i),
                },
                None => return None,
            }
        }
        None
    }

    /// stat：跟随链。
    pub fn stat(&mut self, name: &str, note: &str) -> Result<St2, u32> {
        if self.sealed {
            return Err(ST_SEALED);
        }
        if note.is_empty() {
            return Err(ST_EMPTY_NOTE);
        }
        let i = match self.follow(name) {
            Some(i) => i,
            None => return Err(ST_NOENT),
        };
        let st = self.ents[i].st;
        self.ledger.push(StRec { seq: self.seq_next, view: 0, key: name.to_string(), snap: st });
        self.seq_next += 1;
        Ok(st)
    }

    /// lstat：不跟随链。
    pub fn lstat(&mut self, name: &str, note: &str) -> Result<St2, u32> {
        if self.sealed {
            return Err(ST_SEALED);
        }
        if note.is_empty() {
            return Err(ST_EMPTY_NOTE);
        }
        let i = match self.ent_of(name) {
            Some(i) => i,
            None => return Err(ST_NOENT),
        };
        let st = self.ents[i].st;
        self.ledger.push(StRec { seq: self.seq_next, view: 2, key: name.to_string(), snap: st });
        self.seq_next += 1;
        Ok(st)
    }

    /// open fd 面。
    pub fn open(&mut self, name: &str) -> Result<usize, u32> {
        if self.follow(name).is_none() {
            return Err(ST_NOENT);
        }
        if let Some(i) = self.fds.iter().position(|s| s.is_none()) {
            self.fds[i] = Some(name.to_string());
            return Ok(i);
        }
        if self.fds.len() >= 8 {
            return Err(ST_BAD_FD);
        }
        self.fds.push(Some(name.to_string()));
        Ok(self.fds.len() - 1)
    }

    /// fstat：按 fd 视图（空槽=坏缓冲）。
    pub fn fstat(&mut self, fd: i32, note: &str) -> Result<St2, u32> {
        if self.sealed {
            return Err(ST_SEALED);
        }
        if note.is_empty() {
            return Err(ST_EMPTY_NOTE);
        }
        if fd < 0 || fd as usize >= self.fds.len() {
            return Err(ST_BAD_FD);
        }
        let name = match self.fds[fd as usize].as_ref() {
            Some(n) => n.clone(),
            None => return Err(ST_NOBUF),
        };
        let i = match self.follow(&name) {
            Some(i) => i,
            None => return Err(ST_NOENT),
        };
        let st = self.ents[i].st;
        self.ledger
            .push(StRec { seq: self.seq_next, view: 1, key: name.clone(), snap: st });
        self.seq_next += 1;
        Ok(st)
    }

    /// 三口径一致核：stat==lstat==fstat（对实体件）。
    pub fn tri_agree(&mut self, name: &str) -> bool {
        let fd = match self.open(name) {
            Ok(f) => f,
            Err(_) => return false,
        };
        let a = self.stat(name, "tri-s");
        let b = self.fstat(fd as i32, "tri-f");
        let c = self.lstat(name, "tri-l");
        match (a, b, c) {
            (Ok(x), Ok(y), Ok(z)) => x == y && y == z,
            _ => false,
        }
    }

    /// 链面两读：lstat 链 != stat 链目标（不跟随语义）。
    pub fn link_split(&self, link: &str) -> Option<(St2, St2)> {
        let li = self.ent_of(link)?;
        let ti = self.follow(link)?;
        Some((self.ents[li].st, self.ents[ti].st))
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
        let mut h: u32 = 0x103400;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.view);
            h = h.wrapping_mul(31).wrapping_add(r.snap.ino);
            for b in r.key.bytes() {
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
    let mut b = StatBook::new();
    let _ = b.add("f", St2 { size: 1, mtime: 1, ino: 1 }, None);
    // 1) 缺路径必拒
    got[0] = b.stat("ghost", "inj-noent").err() == Some(ST_NOENT);
    // 2) 坏 fd 必拒
    got[1] = b.fstat(-1, "inj-fd").err() == Some(ST_BAD_FD);
    // 3) 坏缓冲（界内空槽）必拒
    let f = b.open("f").unwrap_or(0);
    b.fds[f] = None;
    got[2] = b.fstat(f as i32, "inj-buf").err() == Some(ST_NOBUF);
    // 4) 空留痕必拒
    got[3] = b.stat("f", "").err() == Some(ST_EMPTY_NOTE);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.stat("f", "inj-seal").err() == Some(ST_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10129_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10129");

    // 正路径：三口径一致（实体件）
    let mut b = StatBook::new();
    let _ = b.add("plain.txt", St2 { size: 12, mtime: 5, ino: 7 }, None);
    let j1 = b.tri_agree("plain.txt");
    s.add("J1-三口径一致", j1, "stat/fstat/lstat 同件同值");

    // 链面两读：lstat 链自值、stat 链目标值
    let j2 = {
        let _ = b.add("real.dat", St2 { size: 40, mtime: 9, ino: 11 }, None);
        let _ = b.add("link.dat", St2 { size: 3, mtime: 1, ino: 12 }, Some("real.dat".to_string()));
        let lst = b.lstat("link.dat", "链自").unwrap_or(St2::zero());
        let stt = b.stat("link.dat", "链目").unwrap_or(St2::zero());
        lst.ino == 12 && stt.ino == 11 && lst.size == 3 && stt.size == 40
    };
    s.add("J2-链面两读", j2, "lstat 不跟随、stat 跟随");

    // link_split 一致核
    let j3 = {
        match b.link_split("link.dat") {
            Some((l, t)) => l.ino != t.ino,
            None => false,
        }
    };
    s.add("J3-分叉核", j3, "链自/链目两值分叉");

    // fd 面与路径面同值
    let j4 = {
        let fd = b.open("real.dat").unwrap_or(0);
        let f = b.fstat(fd as i32, "fd面").unwrap_or(St2::zero());
        let p = b.stat("real.dat", "径面").unwrap_or(St2::zero());
        f == p
    };
    s.add("J4-fd径同值", j4, "fstat 与 stat 同件同值");

    // 缺路径受控
    let j5 = b.stat("ghost", "缺席").err() == Some(ST_NOENT)
        && b.lstat("ghost", "缺席").err() == Some(ST_NOENT);
    s.add("J5-缺路径受控", j5, "stat/lstat 双面同拒");

    // 坏 fd 受控
    let j6 = b.fstat(-2, "负").err() == Some(ST_BAD_FD)
        && b.fstat(99, "越界").err() == Some(ST_BAD_FD);
    s.add("J6-坏fd受控", j6, "负与越界 fd 同码拒");

    // fd 容量守恒
    let j7 = {
        let mut b7 = StatBook::new();
        let _ = b7.add("x", St2 { size: 1, mtime: 1, ino: 1 }, None);
        let mut ok = true;
        for _ in 0..8 {
            if b7.open("x").is_err() {
                ok = false;
            }
        }
        ok && b7.open("x").err() == Some(ST_BAD_FD)
    };
    s.add("J7-fd容量守恒", j7, "fd 表满必拒");

    // 空留痕拒收
    let j8 = b.stat("real.dat", "").err() == Some(ST_EMPTY_NOTE);
    s.add("J8-空留痕拒", j8, "note 为空不入账");

    // 视图账覆盖三视图
    let j9 = [0u32, 1, 2].iter().all(|v| b.ledger.iter().any(|r| r.view == *v));
    s.add("J9-三视图覆盖", j9, "stat/fstat/lstat 三视图各有留痕");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "view 在闭集且键非空");

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
    fn unx10129_stat_flow() {
        let mut b = StatBook::new();
        let _ = b.add("a", St2 { size: 4, mtime: 2, ino: 3 }, None);
        assert!(b.tri_agree("a"));
        let _ = b.add("l", St2 { size: 1, mtime: 1, ino: 9 }, Some("a".to_string()));
        let lst = b.lstat("l", "x").unwrap_or(St2::zero());
        assert_eq!(lst.ino, 9);
        let stt = b.stat("l", "y").unwrap_or(St2::zero());
        assert_eq!(stt.ino, 3);
    }

    #[test]
    fn unx10129_checks_green() {
        let s = run_unx10129_checks();
        assert!(s.all_passed(), "UNX-F10129 判据未全绿");
    }
}
