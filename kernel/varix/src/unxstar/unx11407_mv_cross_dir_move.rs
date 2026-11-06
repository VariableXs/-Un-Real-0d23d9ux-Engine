//! UNX-F11407 mv 跨目录移动语义
//!
//! 同目录改名只动一个名字，跨目录移动却要同时动**两个目录项**：源目录里摘掉
//! 一条，目标目录里添上一条，而 inode 本身一次都不搬。移动目录时还要多改一处
//! ——目录对象的父指针，否则 `..` 会指回旧家。本账用「目录表 + 目录项表」两层
//! 结构把这三件事分开记账，并钉住四条不变量：
//!
//! 1. **inode 不变**：跨目录移动返回源 inode，目标目录里的新名字指向它；
//! 2. **两目录同时收敛**：源目录条目数减一，目标目录条目数加一，总数守恒
//!    （覆盖时目标旧条目一并消失，总数减一）；
//! 3. **父指针跟随**：移动目录后，该目录对象的父指针必须等于目标目录 inode，
//!    整棵子树的条目不动，`..` 自动指向新家；
//! 4. **自移被拒**：把目录移进它自己或它的后代，会让子树从名字空间里脱落，
//!    一律 `EINVAL`。
//!
//! ## `move` 的闸门次序
//! 1. 任一路径为空 → `EINVAL`；末端名字为 0 → `EINVAL`；名字超界 →
//!    `ENAMETOOLONG`（先于任何解析）；
//! 2. 解析源父目录并确认源条目存在（缺项 `ENOENT`、中段是文件 `ENOTDIR`）；
//! 3. 解析目标父目录（同样两闸）；
//! 4. 源目录或目标目录被钉住 → `EBUSY`；只读 → `EROFS`；
//! 5. 源是目录且目标是它自己/后代 → `EINVAL`；
//! 6. 目标已存在：目录改到文件 `ENOTDIR`、文件改到目录 `EISDIR`、目录覆盖
//!    非空目录 `ENOTEMPTY`；同名同目录为空操作；
//! 7. 摘源、摘目标（若有）、在目标目录追加新条目、必要时改父指针。
//!
//! ## 账本契约
//! 十四格：前八格成功（三次建目录、三次登记文件、一次跨目录移动、一次子项
//! 计数），后六格依次撞上缺项、空路径、中段文件、文件改目录、非空目录、只读。
//! `audit` 逐格核对，`seal` 做 FNV-1a 指纹，`replay` 重建比对。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xef70;

/// 路径或名字非法。
pub const MX_EINVAL: u32 = SEG_BASE + 0x01;
/// 目录或条目不存在。
pub const MX_ENOENT: u32 = SEG_BASE + 0x02;
/// 路径中段不是目录，或目录改到文件。
pub const MX_ENOTDIR: u32 = SEG_BASE + 0x03;
/// 文件改到目录。
pub const MX_EISDIR: u32 = SEG_BASE + 0x04;
/// 目标目录非空。
pub const MX_ENOTEMPTY: u32 = SEG_BASE + 0x05;
/// 目录只读。
pub const MX_EROFS: u32 = SEG_BASE + 0x06;
/// 目录被钉住。
pub const MX_EBUSY: u32 = SEG_BASE + 0x07;
/// 名字超过上界。
pub const MX_ENAMETOOLONG: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const MX_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const MX_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const MX_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const MX_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const MX_ENOBS: u32 = SEG_BASE + 0x0d;

/// 目录表容量。
pub const CAP_DIR: usize = 6;
/// 目录项表容量。
pub const CAP_ENT: usize = 12;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 名字上界。
pub const NAME_MAX: u32 = 0xff;
/// 路径分量上界。
pub const DEPTH_MAX: usize = 16;
/// 根目录 inode。
pub const ROOT: u16 = 1;

/// 目录类型。
pub const KIND_DIR: u8 = 1;
/// 普通文件类型。
pub const KIND_FILE: u8 = 2;

/// 操作码：建目录。
pub const OP_MKDIR: u8 = 1;
/// 操作码：登记文件。
pub const OP_LINK: u8 = 2;
/// 操作码：移动。
pub const OP_MOVE: u8 = 3;
/// 操作码：数子项。
pub const OP_CHILDREN: u8 = 4;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 目录对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dir {
    /// inode 号。
    pub ino: u16,
    /// 父目录 inode（根指向 0）。
    pub parent: u16,
    /// 是否占用。
    pub used: bool,
    /// 是否只读。
    pub ro: bool,
    /// 是否被钉住。
    pub busy: bool,
}

const BLANK_DIR: Dir = Dir { ino: 0, parent: 0, used: false, ro: false, busy: false };

/// 目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    /// 所在目录 inode。
    pub dir: u16,
    /// 名字。
    pub name: u32,
    /// 目标 inode。
    pub ino: u16,
    /// 类型。
    pub kind: u8,
}

const BLANK_ENT: Ent = Ent { dir: 0, name: 0, ino: 0, kind: 0 };

/// 两层命名空间。
#[derive(Clone, Copy, Debug)]
pub struct Tree {
    dirs: [Dir; CAP_DIR],
    ents: [Ent; CAP_ENT],
    n_ents: usize,
    moves: u32,
    replaced: u32,
    same: u32,
    refused: u32,
}

impl Tree {
    /// 只有根目录的树。
    pub fn open() -> Tree {
        let mut dirs = [BLANK_DIR; CAP_DIR];
        if let Some(slot) = dirs.get_mut(0) {
            *slot = Dir { ino: ROOT, parent: 0, used: true, ro: false, busy: false };
        }
        Tree {
            dirs,
            ents: [BLANK_ENT; CAP_ENT],
            n_ents: 0,
            moves: 0,
            replaced: 0,
            same: 0,
            refused: 0,
        }
    }

    /// 目录项数。
    pub fn ent_count(&self) -> usize {
        self.n_ents
    }

    /// 移动成功次数。
    pub fn move_count(&self) -> u32 {
        self.moves
    }

    /// 覆盖次数。
    pub fn replace_count(&self) -> u32 {
        self.replaced
    }

    /// 同项空移次数。
    pub fn same_count(&self) -> u32 {
        self.same
    }

    /// 被拒次数。
    pub fn refused_count(&self) -> u32 {
        self.refused
    }

    /// 第 i 个目录项。
    pub fn ent_at(&self, i: usize) -> Option<Ent> {
        if i >= self.n_ents {
            return None;
        }
        self.ents.get(i).copied()
    }

    /// 目录对象。
    pub fn dir_at(&self, ino: u16) -> Option<Dir> {
        let mut i = 0usize;
        while i < CAP_DIR {
            match self.dirs.get(i) {
                Some(d) => {
                    if d.used && d.ino == ino {
                        return Some(*d);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 按 inode 取父目录。
    pub fn parent_of(&self, ino: u16) -> Result<u16, u32> {
        match self.dir_at(ino) {
            Some(d) => Ok(d.parent),
            None => Err(MX_ENOENT),
        }
    }

    /// 目录表中的目录数。
    pub fn dir_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < CAP_DIR {
            if let Some(d) = self.dirs.get(i) {
                if d.used {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 查目录项槽位。
    fn find(&self, dir: u16, name: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n_ents {
            match self.ents.get(i) {
                Some(e) => {
                    if e.dir == dir && e.name == name {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 数某目录的直接子项。
    fn count_in(&self, dir: u16) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n_ents {
            if let Some(e) = self.ents.get(i) {
                if e.dir == dir {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 解析目录路径（空路径表示根）。
    pub fn resolve_dir(&self, path: &[u32]) -> Result<u16, u32> {
        if path.len() > DEPTH_MAX {
            return Err(MX_ENAMETOOLONG);
        }
        let mut cur = ROOT;
        let mut i = 0usize;
        while i < path.len() {
            let name = match path.get(i) {
                Some(v) => *v,
                None => return Err(MX_EINVAL),
            };
            if name == 0 {
                return Err(MX_EINVAL);
            }
            if name > NAME_MAX {
                return Err(MX_ENAMETOOLONG);
            }
            let at = match self.find(cur, name) {
                Some(v) => v,
                None => return Err(MX_ENOENT),
            };
            let e = match self.ents.get(at) {
                Some(v) => *v,
                None => return Err(MX_ENOENT),
            };
            if e.kind != KIND_DIR {
                return Err(MX_ENOTDIR);
            }
            if self.dir_at(e.ino).is_none() {
                return Err(MX_ENOENT);
            }
            cur = e.ino;
            i += 1;
        }
        Ok(cur)
    }

    /// 解析「父目录 + 末端名字」。
    fn split(&self, path: &[u32]) -> Result<(u16, u32), u32> {
        if path.is_empty() {
            return Err(MX_EINVAL);
        }
        let name = match path.get(path.len() - 1) {
            Some(v) => *v,
            None => return Err(MX_EINVAL),
        };
        if name == 0 {
            return Err(MX_EINVAL);
        }
        if name > NAME_MAX {
            return Err(MX_ENAMETOOLONG);
        }
        if path.len() > DEPTH_MAX {
            return Err(MX_ENAMETOOLONG);
        }
        let parent = match path.get(..path.len() - 1) {
            Some(head) => self.resolve_dir(head)?,
            None => return Err(MX_EINVAL),
        };
        Ok((parent, name))
    }

    fn dir_mut(&mut self, ino: u16) -> Option<&mut Dir> {
        let mut i = 0usize;
        let mut at = CAP_DIR;
        while i < CAP_DIR {
            match self.dirs.get(i) {
                Some(d) => {
                    if d.used && d.ino == ino {
                        at = i;
                        break;
                    }
                }
                None => break,
            }
            i += 1;
        }
        if at >= CAP_DIR {
            return None;
        }
        self.dirs.get_mut(at)
    }

    fn free_dir_slot(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < CAP_DIR {
            match self.dirs.get(i) {
                Some(d) => {
                    if !d.used {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 建目录，返回新目录项的槽位。
    pub fn mkdir(&mut self, path: &[u32], ino: u16) -> Result<usize, u32> {
        let (parent, name) = self.split(path)?;
        if ino == 0 || self.dir_at(ino).is_some() || self.find(parent, name).is_some() {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EINVAL);
        }
        let p = match self.dir_at(parent) {
            Some(v) => v,
            None => {
                self.refused = self.refused.saturating_add(1);
                return Err(MX_ENOENT);
            }
        };
        if p.busy {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EBUSY);
        }
        if p.ro {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EROFS);
        }
        if self.n_ents >= CAP_ENT {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EINVAL);
        }
        let slot = match self.free_dir_slot() {
            Some(v) => v,
            None => {
                self.refused = self.refused.saturating_add(1);
                return Err(MX_EINVAL);
            }
        };
        if let Some(d) = self.dirs.get_mut(slot) {
            *d = Dir { ino, parent, used: true, ro: false, busy: false };
        }
        if let Some(e) = self.ents.get_mut(self.n_ents) {
            *e = Ent { dir: parent, name, ino, kind: KIND_DIR };
        }
        self.n_ents += 1;
        Ok(self.n_ents - 1)
    }

    /// 登记文件，返回目录项槽位。
    pub fn link(&mut self, path: &[u32], ino: u16) -> Result<usize, u32> {
        let (parent, name) = self.split(path)?;
        if ino == 0 || self.find(parent, name).is_some() {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EINVAL);
        }
        let p = match self.dir_at(parent) {
            Some(v) => v,
            None => {
                self.refused = self.refused.saturating_add(1);
                return Err(MX_ENOENT);
            }
        };
        if p.busy {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EBUSY);
        }
        if p.ro {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EROFS);
        }
        if self.n_ents >= CAP_ENT {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EINVAL);
        }
        if let Some(e) = self.ents.get_mut(self.n_ents) {
            *e = Ent { dir: parent, name, ino, kind: KIND_FILE };
        }
        self.n_ents += 1;
        Ok(self.n_ents - 1)
    }

    /// 设置目录属性（只读/钉住）。
    pub fn set_flags(&mut self, path: &[u32], ro: bool, busy: bool) -> Result<(), u32> {
        let ino = self.resolve_dir(path)?;
        match self.dir_mut(ino) {
            Some(d) => {
                d.ro = ro;
                d.busy = busy;
                Ok(())
            }
            None => Err(MX_ENOENT),
        }
    }

    /// 数目录子项。
    pub fn children(&self, path: &[u32]) -> Result<u32, u32> {
        let ino = self.resolve_dir(path)?;
        Ok(self.count_in(ino))
    }

    /// 判断 `node` 是否在 `anc` 的子树里（含相等）。
    fn within(&self, anc: u16, node: u16) -> bool {
        let mut p = node;
        let mut guard = 0u32;
        while guard <= CAP_DIR as u32 {
            if p == anc {
                return true;
            }
            if p == 0 || p == ROOT {
                return false;
            }
            match self.dir_at(p) {
                Some(d) => p = d.parent,
                None => return false,
            }
            guard += 1;
        }
        true
    }

    fn drop_ent(&mut self, i: usize) {
        let mut k = i;
        while k + 1 < self.n_ents {
            match self.ents.get(k + 1).copied() {
                Some(next) => {
                    if let Some(dst) = self.ents.get_mut(k) {
                        *dst = next;
                    }
                }
                None => break,
            }
            k += 1;
        }
        if self.n_ents > 0 {
            self.n_ents -= 1;
        }
        if let Some(tail) = self.ents.get_mut(self.n_ents) {
            *tail = BLANK_ENT;
        }
    }

    /// 跨目录移动，成功返回 inode。
    pub fn move_ent(&mut self, src: &[u32], dst: &[u32]) -> Result<u16, u32> {
        if src.is_empty() || dst.is_empty() {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EINVAL);
        }
        let (sdir, sname) = match self.split(src) {
            Ok(v) => v,
            Err(c) => {
                self.refused = self.refused.saturating_add(1);
                return Err(c);
            }
        };
        let si = match self.find(sdir, sname) {
            Some(v) => v,
            None => {
                self.refused = self.refused.saturating_add(1);
                return Err(MX_ENOENT);
            }
        };
        let src_ent = match self.ents.get(si) {
            Some(v) => *v,
            None => return Err(MX_ENOENT),
        };
        let (ddir, dname) = match self.split(dst) {
            Ok(v) => v,
            Err(c) => {
                self.refused = self.refused.saturating_add(1);
                return Err(c);
            }
        };
        let sd = match self.dir_at(sdir) {
            Some(v) => v,
            None => return Err(MX_ENOENT),
        };
        let dd = match self.dir_at(ddir) {
            Some(v) => v,
            None => {
                self.refused = self.refused.saturating_add(1);
                return Err(MX_ENOENT);
            }
        };
        if sd.busy || dd.busy {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EBUSY);
        }
        if sd.ro || dd.ro {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EROFS);
        }
        if src_ent.kind == KIND_DIR && self.within(src_ent.ino, ddir) {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EINVAL);
        }
        if sdir == ddir && sname == dname {
            self.same = self.same.saturating_add(1);
            return Ok(src_ent.ino);
        }
        let di = self.find(ddir, dname);
        if let Some(d) = di {
            let dst_ent = match self.ents.get(d) {
                Some(v) => *v,
                None => return Err(MX_ENOENT),
            };
            if src_ent.kind == KIND_DIR && dst_ent.kind != KIND_DIR {
                self.refused = self.refused.saturating_add(1);
                return Err(MX_ENOTDIR);
            }
            if src_ent.kind != KIND_DIR && dst_ent.kind == KIND_DIR {
                self.refused = self.refused.saturating_add(1);
                return Err(MX_EISDIR);
            }
            if src_ent.kind == KIND_DIR
                && dst_ent.kind == KIND_DIR
                && self.count_in(dst_ent.ino) > 0
            {
                self.refused = self.refused.saturating_add(1);
                return Err(MX_ENOTEMPTY);
            }
        }
        self.drop_ent(si);
        if let Some(di2) = self.find(ddir, dname) {
            self.drop_ent(di2);
            self.replaced = self.replaced.saturating_add(1);
        }
        if self.n_ents >= CAP_ENT {
            self.refused = self.refused.saturating_add(1);
            return Err(MX_EINVAL);
        }
        if let Some(e) = self.ents.get_mut(self.n_ents) {
            *e = Ent { dir: ddir, name: dname, ino: src_ent.ino, kind: src_ent.kind };
        }
        self.n_ents += 1;
        if src_ent.kind == KIND_DIR {
            if let Some(d) = self.dir_mut(src_ent.ino) {
                d.parent = ddir;
            }
        }
        self.moves = self.moves.saturating_add(1);
        Ok(src_ent.ino)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n_ents > CAP_ENT {
            return false;
        }
        let root = match self.dirs.get(0) {
            Some(v) => v,
            None => return false,
        };
        if !root.used || root.ino != ROOT || root.parent != 0 {
            return false;
        }
        let mut i = 1usize;
        while i < CAP_DIR {
            if let Some(d) = self.dirs.get(i) {
                if d.used {
                    if d.ino == 0 || d.ino == ROOT {
                        return false;
                    }
                    if d.parent == 0 || self.dir_at(d.parent).is_none() {
                        return false;
                    }
                }
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.n_ents {
            let a = match self.ents.get(j) {
                Some(v) => *v,
                None => return false,
            };
            if a.dir == 0 || self.dir_at(a.dir).is_none() {
                return false;
            }
            if a.name == 0 || a.name > NAME_MAX || a.ino == 0 {
                return false;
            }
            if a.kind != KIND_DIR && a.kind != KIND_FILE {
                return false;
            }
            if a.kind == KIND_DIR {
                match self.dir_at(a.ino) {
                    Some(d) => {
                        if d.parent != a.dir {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            let mut k = j + 1;
            while k < self.n_ents {
                match self.ents.get(k) {
                    Some(b) => {
                        if b.dir == a.dir && b.name == a.name {
                            return false;
                        }
                    }
                    None => return false,
                }
                k += 1;
            }
            j += 1;
        }
        true
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u8, u32>` → 样本。
fn byte_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen || (row.slot as usize) != self.len || (row.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MX_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = row;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(MX_EBAL);
        }
        const VALS: [u32; 8] = [0, 1, 2, 3, 4, 5, 500, 2];
        const CODES: [u32; 6] =
            [MX_ENOENT, MX_EINVAL, MX_ENOTDIR, MX_EISDIR, MX_ENOTEMPTY, MX_EROFS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MX_EBAL),
            };
            if r.slot as usize != i {
                return Err(MX_ESLOT);
            }
            if r.ok {
                if r.code != 0 {
                    return Err(MX_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if r.value != *w {
                            return Err(MX_EVALUE);
                        }
                    }
                    None => return Err(MX_EBAL),
                }
                vi += 1;
            } else {
                if r.value != 0 {
                    return Err(MX_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if r.code != *w {
                            return Err(MX_EVALUE);
                        }
                    }
                    None => return Err(MX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MX_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.slot as u64);
                h = fold(h, r.op as u64);
                h = fold(h, r.ok as u64);
                h = fold(h, r.code as u64);
                h = fold(h, r.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(MX_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(MX_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本。
///
/// ```text
/// /           1
/// /10         2   (目录)
/// /20         3   (目录)
/// /10/30      4   (目录)
/// /10/40   500   (文件) -> 移到 /20/42
/// /10/30/60 501  (文件)
/// /20/41   502   (文件)
/// ```
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Tree::open();

    let _ = l.register(cell(0, OP_MKDIR, t.mkdir(&[10], 2)));
    let _ = l.register(cell(1, OP_MKDIR, t.mkdir(&[20], 3)));
    let _ = l.register(cell(2, OP_MKDIR, t.mkdir(&[10, 30], 4)));
    let _ = l.register(cell(3, OP_LINK, t.link(&[10, 40], 500)));
    let _ = l.register(cell(4, OP_LINK, t.link(&[10, 30, 60], 501)));
    let _ = l.register(cell(5, OP_LINK, t.link(&[20, 41], 502)));
    let _ = l.register(word_cell(6, OP_MOVE, t.move_ent(&[10, 40], &[20, 42]).map(|v| v as u32)));
    let _ = l.register(word_cell(7, OP_CHILDREN, t.children(&[20])));

    let _ = l.register(word_cell(8, OP_MOVE, t.move_ent(&[10, 99], &[20, 98]).map(|v| v as u32)));
    let _ = l.register(word_cell(9, OP_MOVE, t.move_ent(&[], &[20, 1]).map(|v| v as u32)));
    let _ = l.register(word_cell(
        10,
        OP_MOVE,
        t.move_ent(&[20, 41], &[20, 41, 1]).map(|v| v as u32),
    ));
    let _ = l.register(word_cell(11, OP_MOVE, t.move_ent(&[20, 42], &[10, 30]).map(|v| v as u32)));
    let _ = l.register(word_cell(12, OP_MOVE, t.move_ent(&[10, 30], &[20]).map(|v| v as u32)));

    let _ = t.set_flags(&[10], true, false);
    let _ = l.register(word_cell(13, OP_MOVE, t.move_ent(&[10, 30], &[20, 31]).map(|v| v as u32)));

    l
}

/// 四路注入：账本身、移动正例、八类失败、闸门次序。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：文件跨目录移动与目录跨目录移动。
    let mut a = Tree::open();
    let d1 = a.mkdir(&[10], 2);
    let d2 = a.mkdir(&[20], 3);
    let f = a.link(&[10, 40], 500);
    let before_src = a.children(&[10]);
    let moved = a.move_ent(&[10, 40], &[20, 42]);
    let after_src = a.children(&[10]);
    let after_dst = a.children(&[20]);
    let sub = a.mkdir(&[10, 30], 4);
    let dir_move = a.move_ent(&[10, 30], &[50]);
    let back = a.move_ent(&[20, 42], &[10, 43]);
    out[1] = d1 == Ok(0)
        && d2 == Ok(1)
        && f == Ok(2)
        && before_src == Ok(1)
        && moved == Ok(500)
        && after_src == Ok(0)
        && after_dst == Ok(1)
        && sub == Ok(3)
        && dir_move == Ok(4)
        && a.parent_of(4) == Ok(ROOT)
        && a.resolve_dir(&[50]) == Ok(4)
        && back == Ok(500)
        && a.children(&[20]) == Ok(0)
        && a.children(&[10]) == Ok(1)
        && a.move_count() == 3
        && a.consistent();

    // 二路：八类失败各自可辨。
    let mut b = Tree::open();
    let _ = b.mkdir(&[10], 2);
    let _ = b.mkdir(&[20], 3);
    let _ = b.link(&[10, 40], 500);
    let empty = b.move_ent(&[], &[20, 1]) == Err(MX_EINVAL);
    let zero = b.move_ent(&[10, 0], &[20, 1]) == Err(MX_EINVAL);
    let long = b.move_ent(&[10, NAME_MAX + 1], &[20, 1]) == Err(MX_ENAMETOOLONG);
    let missing = b.move_ent(&[10, 99], &[20, 98]) == Err(MX_ENOENT);
    let mid_file = b.move_ent(&[10, 40], &[10, 40, 1]) == Err(MX_ENOTDIR);
    let _ = b.mkdir(&[20, 50], 5);
    let is_dir = b.move_ent(&[10, 40], &[20, 50]) == Err(MX_EISDIR);
    let _ = b.link(&[20, 50, 60], 501);
    let _ = b.mkdir(&[10, 70], 6);
    let nonempty = b.move_ent(&[10, 70], &[20, 50]) == Err(MX_ENOTEMPTY);
    let _ = b.set_flags(&[10], true, false);
    let ro = b.move_ent(&[10, 40], &[20, 61]) == Err(MX_EROFS);
    let _ = b.set_flags(&[10], false, true);
    let busy = b.move_ent(&[10, 40], &[20, 61]) == Err(MX_EBUSY);
    out[2] = empty
        && zero
        && long
        && missing
        && mid_file
        && is_dir
        && nonempty
        && ro
        && busy
        && b.consistent();

    // 三路：闸门次序与自移拒绝。
    let mut c = Tree::open();
    let _ = c.mkdir(&[10], 2);
    let _ = c.mkdir(&[10, 30], 4);
    let _ = c.link(&[10, 30, 60], 501);
    let long_first = c.move_ent(&[10, NAME_MAX + 1], &[99, 1]) == Err(MX_ENAMETOOLONG);
    let missing_first = c.move_ent(&[10, 99], &[10, 60, 1]) == Err(MX_ENOENT);
    let self_move = c.move_ent(&[10, 30], &[10, 30, 40]) == Err(MX_EINVAL);
    let _ = c.link(&[10, 62], 502);
    let dir_onto_file = c.move_ent(&[10, 30], &[10, 62]) == Err(MX_ENOTDIR);
    let same_move = c.move_ent(&[10, 30], &[10, 30]) == Ok(4);
    let _ = c.set_flags(&[10], false, true);
    let busy_before_ro = c.move_ent(&[10, 30], &[10, 31]) == Err(MX_EBUSY);
    out[3] = long_first
        && missing_first
        && self_move
        && dir_onto_file
        && same_move
        && busy_before_ro
        && c.parent_of(4) == Ok(2)
        && c.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11407_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11407.mv_cross_dir_move");
    let l = build_ledger();

    s.add_dyn(
        "MX1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "MX2-跨目录移动保 inode",
        {
            let mut a = Tree::open();
            let _ = a.mkdir(&[10], 2);
            let _ = a.mkdir(&[20], 3);
            let _ = a.link(&[10, 40], 500);
            let got = a.move_ent(&[10, 40], &[20, 42]);
            got == Ok(500)
                && a.ent_at(2).map(|e| e.ino) == Some(500)
                && l.sample(6).map(|x| x.value) == Some(500)
        },
        "static",
    );

    s.add(
        "MX3-源目录条目消失",
        {
            let mut a = Tree::open();
            let _ = a.mkdir(&[10], 2);
            let _ = a.mkdir(&[20], 3);
            let _ = a.link(&[10, 40], 500);
            let before = a.children(&[10]);
            let _ = a.move_ent(&[10, 40], &[20, 42]);
            before == Ok(1)
                && a.children(&[10]) == Ok(0)
                && a.ent_count() == 3
                && a.consistent()
        },
        "static",
    );

    s.add(
        "MX4-目标目录条目出现",
        {
            let mut a = Tree::open();
            let _ = a.mkdir(&[10], 2);
            let _ = a.mkdir(&[20], 3);
            let _ = a.link(&[10, 40], 500);
            let _ = a.move_ent(&[10, 40], &[20, 42]);
            a.children(&[20]) == Ok(1)
                && a.resolve_dir(&[20]) == Ok(3)
                && a.ent_at(2).map(|e| e.name) == Some(42)
                && a.ent_at(2).map(|e| e.dir) == Some(3)
                && l.sample(7).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "MX5-两目录总数守恒",
        {
            let mut a = Tree::open();
            let _ = a.mkdir(&[10], 2);
            let _ = a.mkdir(&[20], 3);
            let _ = a.link(&[10, 40], 500);
            let _ = a.link(&[10, 41], 501);
            let n0 = a.ent_count();
            let _ = a.move_ent(&[10, 40], &[20, 42]);
            let n1 = a.ent_count();
            let _ = a.move_ent(&[20, 42], &[10, 41]);
            n0 == 4
                && n1 == 4
                && a.ent_count() == 3
                && a.replace_count() == 1
                && a.children(&[20]) == Ok(0)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "MX6-目录移动改父指针",
        {
            let mut a = Tree::open();
            let _ = a.mkdir(&[10], 2);
            let _ = a.mkdir(&[20], 3);
            let _ = a.mkdir(&[10, 30], 4);
            let _ = a.link(&[10, 30, 60], 501);
            let before = a.parent_of(4);
            let got = a.move_ent(&[10, 30], &[20, 31]);
            before == Ok(2)
                && got == Ok(4)
                && a.parent_of(4) == Ok(3)
                && a.children(&[20]) == Ok(1)
                && a.resolve_dir(&[20, 31]) == Ok(4)
                && a.children(&[20, 31]) == Ok(1)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "MX7-空路径零名被拒",
        l.sample(9).map(|x| x.code) == Some(MX_EINVAL)
            && {
                let mut a = Tree::open();
                a.move_ent(&[], &[10]) == Err(MX_EINVAL)
                    && a.move_ent(&[10], &[]) == Err(MX_EINVAL)
                    && a.move_ent(&[0], &[10]) == Err(MX_EINVAL)
                    && a.mkdir(&[], 2) == Err(MX_EINVAL)
            },
        "static",
    );

    s.add(
        "MX8-缺项报 ENOENT",
        l.sample(8).map(|x| x.code) == Some(MX_ENOENT)
            && {
                let mut a = Tree::open();
                let _ = a.mkdir(&[10], 2);
                a.move_ent(&[10, 99], &[10, 98]) == Err(MX_ENOENT)
                    && a.move_ent(&[10, 99], &[77, 1]) == Err(MX_ENOENT)
                    && a.resolve_dir(&[99]) == Err(MX_ENOENT)
            },
        "static",
    );

    s.add(
        "MX9-中段文件报 ENOTDIR",
        l.sample(10).map(|x| x.code) == Some(MX_ENOTDIR)
            && {
                let mut a = Tree::open();
                let _ = a.mkdir(&[10], 2);
                let _ = a.link(&[10, 40], 500);
                a.move_ent(&[10, 40], &[10, 40, 1]) == Err(MX_ENOTDIR)
                    && a.resolve_dir(&[10, 40]) == Err(MX_ENOTDIR)
                    && a.children(&[10, 40]) == Err(MX_ENOTDIR)
            },
        "static",
    );

    s.add(
        "MX10-文件目录类型不匹配",
        l.sample(11).map(|x| x.code) == Some(MX_EISDIR)
            && {
                let mut a = Tree::open();
                let _ = a.mkdir(&[10], 2);
                let _ = a.mkdir(&[20], 3);
                let _ = a.link(&[10, 40], 500);
                let file_to_dir = a.move_ent(&[10, 40], &[20]) == Err(MX_EISDIR);
                let _ = a.mkdir(&[10, 30], 4);
                let dir_to_file = a.move_ent(&[10, 30], &[10, 40]) == Err(MX_ENOTDIR);
                file_to_dir && dir_to_file
            },
        "static"
    );

    s.add(
        "MX11-非空目录拒绝覆盖",
        l.sample(12).map(|x| x.code) == Some(MX_ENOTEMPTY)
            && {
                let mut a = Tree::open();
                let _ = a.mkdir(&[10], 2);
                let _ = a.mkdir(&[20], 3);
                let _ = a.mkdir(&[10, 30], 4);
                let _ = a.link(&[10, 30, 60], 501);
                let _ = a.link(&[20, 21], 502);
                let refused = a.move_ent(&[10, 30], &[20]) == Err(MX_ENOTEMPTY);
                let _ = a.move_ent(&[20, 21], &[10, 61]);
                let allowed = a.move_ent(&[10, 30], &[20]) == Ok(4);
                refused && allowed && a.parent_of(4) == Ok(ROOT) && a.consistent()
            },
        "static",
    );

    s.add(
        "MX12-只读与钉住先于执行",
        l.sample(13).map(|x| x.code) == Some(MX_EROFS)
            && {
                let mut a = Tree::open();
                let _ = a.mkdir(&[10], 2);
                let _ = a.mkdir(&[20], 3);
                let _ = a.link(&[10, 40], 500);
                let _ = a.set_flags(&[10], true, false);
                let ro = a.move_ent(&[10, 40], &[20, 42]) == Err(MX_EROFS);
                let _ = a.set_flags(&[10], false, true);
                let busy = a.move_ent(&[10, 40], &[20, 42]) == Err(MX_EBUSY);
                let _ = a.set_flags(&[20], false, true);
                let dst_busy = a.move_ent(&[10, 40], &[20, 42]) == Err(MX_EBUSY);
                ro && busy && dst_busy && a.ent_count() == 3
            },
        "static",
    );

    s.add(
        "MX13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Tree {
        let mut t = Tree::open();
        assert_eq!(t.mkdir(&[10], 2), Ok(0));
        assert_eq!(t.mkdir(&[20], 3), Ok(1));
        assert_eq!(t.link(&[10, 40], 500), Ok(2));
        t
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        assert_ne!(l.seal(), 0);
        assert_eq!(l.replay(), Ok(l.seal()));
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11407_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn file_moves_between_dirs() {
        let mut t = tree();
        assert_eq!(t.children(&[10]), Ok(1));
        assert_eq!(t.move_ent(&[10, 40], &[20, 42]), Ok(500));
        assert_eq!(t.children(&[10]), Ok(0));
        assert_eq!(t.children(&[20]), Ok(1));
        assert_eq!(t.ent_count(), 3);
        assert_eq!(t.move_count(), 1);
        assert!(t.consistent());
    }

    #[test]
    fn dir_move_updates_parent() {
        let mut t = tree();
        assert_eq!(t.mkdir(&[10, 30], 4), Ok(3));
        assert_eq!(t.link(&[10, 30, 60], 501), Ok(4));
        assert_eq!(t.parent_of(4), Ok(2));
        assert_eq!(t.move_ent(&[10, 30], &[50]), Ok(4));
        assert_eq!(t.parent_of(4), Ok(ROOT));
        assert_eq!(t.resolve_dir(&[50]), Ok(4));
        assert_eq!(t.children(&[50]), Ok(1));
        assert!(t.consistent());
    }

    #[test]
    fn missing_and_not_dir() {
        let mut t = tree();
        assert_eq!(t.move_ent(&[10, 99], &[20, 98]), Err(MX_ENOENT));
        assert_eq!(t.move_ent(&[10, 40], &[10, 40, 1]), Err(MX_ENOTDIR));
        assert_eq!(t.move_ent(&[], &[20, 1]), Err(MX_EINVAL));
        assert_eq!(t.move_ent(&[0], &[20, 1]), Err(MX_EINVAL));
        assert_eq!(t.move_ent(&[10, NAME_MAX + 1], &[20, 1]), Err(MX_ENAMETOOLONG));
    }

    #[test]
    fn type_and_nonempty() {
        let mut t = tree();
        assert_eq!(t.mkdir(&[10, 30], 4), Ok(3));
        assert_eq!(t.move_ent(&[10, 30], &[10, 40]), Err(MX_ENOTDIR));
        assert_eq!(t.move_ent(&[10, 40], &[10, 30]), Err(MX_EISDIR));
        assert_eq!(t.link(&[10, 30, 60], 501), Ok(4));
        assert_eq!(t.link(&[20, 21], 502), Ok(5));
        assert_eq!(t.move_ent(&[10, 30], &[20]), Err(MX_ENOTEMPTY));
        assert_eq!(t.move_ent(&[10, 30, 60], &[10, 61]), Ok(501));
        assert_eq!(t.move_ent(&[10, 40], &[10, 45]), Ok(500));
        assert_eq!(t.move_ent(&[20, 21], &[10, 62]), Ok(502));
        assert_eq!(t.move_ent(&[10, 30], &[20]), Ok(4));
    }

    #[test]
    fn ro_busy_and_self() {
        let mut t = tree();
        assert_eq!(t.mkdir(&[10, 30], 4), Ok(3));
        assert_eq!(t.move_ent(&[10, 30], &[10, 30, 40]), Err(MX_EINVAL));
        assert_eq!(t.set_flags(&[10], true, false), Ok(()));
        assert_eq!(t.move_ent(&[10, 30], &[20, 31]), Err(MX_EROFS));
        assert_eq!(t.set_flags(&[10], false, true), Ok(()));
        assert_eq!(t.move_ent(&[10, 30], &[20, 31]), Err(MX_EBUSY));
        assert_eq!(t.set_flags(&[10], false, false), Ok(()));
        assert_eq!(t.move_ent(&[10, 30], &[20, 31]), Ok(4));
        assert_eq!(t.parent_of(4), Ok(3));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
