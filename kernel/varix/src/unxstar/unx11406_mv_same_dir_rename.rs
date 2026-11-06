//! UNX-F11406 mv 同目录改名语义
//!
//! `mv a b` 在同一个目录里发生时，内核并不搬运任何数据，只动一个目录项。这条
//! 「最短的移动」恰恰是最容易写错的地方：有人重建条目，结果 inode 变了；有人
//! 先删后建，结果覆盖时目标非空目录被静默清掉；有人把自改名当成错误，破坏了
//! `mv x x` 的幂等语义。本账把同目录改名拆成一条只有目录项视图的模型，逐条
//! 钉住四个不变量：
//!
//! 1. **inode 不变**：改名返回源条目的 inode，改名后新名字仍指向同一个 inode；
//! 2. **位置不变**：源条目在表中的槽位（相对次序）不因改名而漂移；覆盖目标时
//!    先摘掉目标槽位，源条目只做一次必要的左移；
//! 3. **覆盖原子**：目标已存在时，其条目整体消失（inode 链接数归零），源条目
//!    接替名字；目标是非空目录则整笔拒绝，绝不清空；
//! 4. **自改名空操作**：`old == new` 直接成功，不增删任何条目。
//!
//! ## `rename` 的闸门次序
//! 1. 名字为 0 → `EINVAL`；名字超过 `NAME_MAX` → `ENAMETOOLONG`；
//! 2. 目录只读 → `EROFS`（先于查源，只读目录里连失败原因都不外泄）；
//! 3. 源条目不存在 → `ENOENT`；源条目被钉住 → `EBUSY`；
//! 4. 自改名 → 成功返回源 inode；
//! 5. 目标条目存在时：目标被钉住 → `EBUSY`；目录改到文件 → `ENOTDIR`；
//!    文件改到目录 → `EISDIR`；目录覆盖非空目录 → `ENOTEMPTY`；
//! 6. 摘目标、改名字、返回源 inode。
//!
//! ## 账本契约
//! 十四格：前八格成功（三次登记、两次普通改名、一次覆盖、一次自改名、一次
//! inode 查询），后六格依次撞上缺项、空名、长名、文件改目录、非空目录、只读。
//! `audit` 核对槽号/成功位/错误码/观测值，`seal` 为整账指纹，`replay` 重建
//! 账本要求指纹一致。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xef60;

/// 名字或登记参数非法（含重名与表满）。
pub const MV_EINVAL: u32 = SEG_BASE + 0x01;
/// 源条目不存在。
pub const MV_ENOENT: u32 = SEG_BASE + 0x02;
/// 目录改到文件上。
pub const MV_ENOTDIR: u32 = SEG_BASE + 0x03;
/// 文件改到目录上。
pub const MV_EISDIR: u32 = SEG_BASE + 0x04;
/// 目标目录非空，拒绝覆盖。
pub const MV_ENOTEMPTY: u32 = SEG_BASE + 0x05;
/// 目录只读。
pub const MV_EROFS: u32 = SEG_BASE + 0x06;
/// 条目被钉住，不能改动。
pub const MV_EBUSY: u32 = SEG_BASE + 0x07;
/// 名字超过上界。
pub const MV_ENAMETOOLONG: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const MV_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const MV_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const MV_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const MV_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const MV_ENOBS: u32 = SEG_BASE + 0x0d;

/// 目录项表容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 名字上界。
pub const NAME_MAX: u32 = 0xff;

/// 目录类型。
pub const KIND_DIR: u8 = 1;
/// 普通文件类型。
pub const KIND_FILE: u8 = 2;

/// 操作码：登记条目。
pub const OP_CREATE: u8 = 1;
/// 操作码：改名。
pub const OP_RENAME: u8 = 2;
/// 操作码：按名字查 inode。
pub const OP_LOOKUP: u8 = 3;
/// 操作码：查子项数。
pub const OP_KIDS: u8 = 4;

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

/// 一个目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    /// 名字。
    pub name: u32,
    /// inode 号。
    pub ino: u16,
    /// 类型。
    pub kind: u8,
    /// 子项数（仅目录）。
    pub kids: u16,
    /// 是否被钉住。
    pub pin: bool,
}

const BLANK: Ent = Ent { name: 0, ino: 0, kind: 0, kids: 0, pin: false };

/// 单目录改名账。
#[derive(Clone, Copy, Debug)]
pub struct Ren {
    ents: [Ent; CAP],
    n: usize,
    ro: bool,
    moves: u32,
    replaced: u32,
    same: u32,
    unlinked: u32,
    refused: u32,
}

impl Ren {
    /// 空目录。
    pub const fn open() -> Ren {
        Ren {
            ents: [BLANK; CAP],
            n: 0,
            ro: false,
            moves: 0,
            replaced: 0,
            same: 0,
            unlinked: 0,
            refused: 0,
        }
    }

    /// 条目数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 是否空目录。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 是否只读。
    pub fn is_ro(&self) -> bool {
        self.ro
    }

    /// 设置只读。
    pub fn set_ro(&mut self, on: bool) {
        self.ro = on;
    }

    /// 第 i 条名字。
    pub fn name_at(&self, i: usize) -> Option<u32> {
        if i >= self.n {
            return None;
        }
        self.ents.get(i).map(|e| e.name)
    }

    /// 第 i 条 inode。
    pub fn ino_at(&self, i: usize) -> Option<u16> {
        if i >= self.n {
            return None;
        }
        self.ents.get(i).map(|e| e.ino)
    }

    /// 第 i 条的完整视图。
    pub fn ent_at(&self, i: usize) -> Option<Ent> {
        if i >= self.n {
            return None;
        }
        self.ents.get(i).copied()
    }

    /// 成功改名次数。
    pub fn move_count(&self) -> u32 {
        self.moves
    }

    /// 覆盖次数。
    pub fn replace_count(&self) -> u32 {
        self.replaced
    }

    /// 自改名次数。
    pub fn same_count(&self) -> u32 {
        self.same
    }

    /// 被覆盖而消失的 inode 数。
    pub fn unlink_count(&self) -> u32 {
        self.unlinked
    }

    /// 被拒次数。
    pub fn refused_count(&self) -> u32 {
        self.refused
    }

    /// 按名字查槽位。
    fn find(&self, name: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.ents.get(i) {
                Some(e) => {
                    if e.name == name {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 按名字查 inode。
    pub fn ino_of(&self, name: u32) -> Result<u16, u32> {
        match self.find(name) {
            Some(i) => match self.ents.get(i) {
                Some(e) => Ok(e.ino),
                None => Err(MV_ENOENT),
            },
            None => Err(MV_ENOENT),
        }
    }

    /// 按名字查子项数。
    pub fn kids_of(&self, name: u32) -> Result<u16, u32> {
        match self.find(name) {
            Some(i) => match self.ents.get(i) {
                Some(e) => {
                    if e.kind != KIND_DIR {
                        Err(MV_ENOTDIR)
                    } else {
                        Ok(e.kids)
                    }
                }
                None => Err(MV_ENOENT),
            },
            None => Err(MV_ENOENT),
        }
    }

    /// 登记一个条目，返回槽位。
    pub fn create(&mut self, name: u32, ino: u16, kind: u8) -> Result<usize, u32> {
        if name == 0 || (kind != KIND_DIR && kind != KIND_FILE) || ino == 0 {
            self.refused = self.refused.saturating_add(1);
            return Err(MV_EINVAL);
        }
        if name > NAME_MAX {
            self.refused = self.refused.saturating_add(1);
            return Err(MV_ENAMETOOLONG);
        }
        if self.ro {
            self.refused = self.refused.saturating_add(1);
            return Err(MV_EROFS);
        }
        if self.find(name).is_some() || self.n >= CAP {
            self.refused = self.refused.saturating_add(1);
            return Err(MV_EINVAL);
        }
        if let Some(slot) = self.ents.get_mut(self.n) {
            *slot = Ent { name, ino, kind, kids: 0, pin: false };
        }
        self.n += 1;
        Ok(self.n - 1)
    }

    /// 设置目录子项数。
    pub fn set_kids(&mut self, name: u32, kids: u16) -> Result<(), u32> {
        if self.ro {
            self.refused = self.refused.saturating_add(1);
            return Err(MV_EROFS);
        }
        let i = match self.find(name) {
            Some(v) => v,
            None => {
                self.refused = self.refused.saturating_add(1);
                return Err(MV_ENOENT);
            }
        };
        match self.ents.get_mut(i) {
            Some(e) => {
                if e.kind != KIND_DIR {
                    self.refused = self.refused.saturating_add(1);
                    return Err(MV_ENOTDIR);
                }
                e.kids = kids;
                Ok(())
            }
            None => Err(MV_ENOENT),
        }
    }

    /// 钉住或解钉一个条目。
    pub fn set_pin(&mut self, name: u32, on: bool) -> Result<(), u32> {
        if self.ro {
            self.refused = self.refused.saturating_add(1);
            return Err(MV_EROFS);
        }
        let i = match self.find(name) {
            Some(v) => v,
            None => {
                self.refused = self.refused.saturating_add(1);
                return Err(MV_ENOENT);
            }
        };
        if let Some(e) = self.ents.get_mut(i) {
            e.pin = on;
            Ok(())
        } else {
            Err(MV_ENOENT)
        }
    }

    /// 摘掉第 i 条（内部使用，保证 i < n）。
    fn drop_at(&mut self, i: usize) {
        let mut k = i;
        while k + 1 < self.n {
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
        if self.n > 0 {
            self.n -= 1;
        }
        if let Some(tail) = self.ents.get_mut(self.n) {
            *tail = BLANK;
        }
    }

    /// 同目录改名，成功返回源 inode。
    pub fn rename(&mut self, old: u32, new: u32) -> Result<u16, u32> {
        if old == 0 || new == 0 {
            self.refused = self.refused.saturating_add(1);
            return Err(MV_EINVAL);
        }
        if old > NAME_MAX || new > NAME_MAX {
            self.refused = self.refused.saturating_add(1);
            return Err(MV_ENAMETOOLONG);
        }
        if self.ro {
            self.refused = self.refused.saturating_add(1);
            return Err(MV_EROFS);
        }
        let si = match self.find(old) {
            Some(v) => v,
            None => {
                self.refused = self.refused.saturating_add(1);
                return Err(MV_ENOENT);
            }
        };
        let src = match self.ents.get(si) {
            Some(v) => *v,
            None => return Err(MV_ENOENT),
        };
        if src.pin {
            self.refused = self.refused.saturating_add(1);
            return Err(MV_EBUSY);
        }
        if old == new {
            self.same = self.same.saturating_add(1);
            self.moves = self.moves.saturating_add(1);
            return Ok(src.ino);
        }
        let di = self.find(new);
        if let Some(d) = di {
            let dst = match self.ents.get(d) {
                Some(v) => *v,
                None => return Err(MV_ENOENT),
            };
            if dst.pin {
                self.refused = self.refused.saturating_add(1);
                return Err(MV_EBUSY);
            }
            if src.kind == KIND_DIR && dst.kind != KIND_DIR {
                self.refused = self.refused.saturating_add(1);
                return Err(MV_ENOTDIR);
            }
            if src.kind != KIND_DIR && dst.kind == KIND_DIR {
                self.refused = self.refused.saturating_add(1);
                return Err(MV_EISDIR);
            }
            if src.kind == KIND_DIR && dst.kind == KIND_DIR && dst.kids > 0 {
                self.refused = self.refused.saturating_add(1);
                return Err(MV_ENOTEMPTY);
            }
            let dst_ino = dst.ino;
            self.drop_at(d);
            let at = if d < si { si - 1 } else { si };
            if let Some(e) = self.ents.get_mut(at) {
                e.name = new;
            }
            self.replaced = self.replaced.saturating_add(1);
            if dst_ino != 0 {
                self.unlinked = self.unlinked.saturating_add(1);
            }
        } else if let Some(e) = self.ents.get_mut(si) {
            e.name = new;
        }
        self.moves = self.moves.saturating_add(1);
        Ok(src.ino)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.ents.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a.name == 0 || a.name > NAME_MAX || a.ino == 0 {
                return false;
            }
            if a.kind != KIND_DIR && a.kind != KIND_FILE {
                return false;
            }
            if a.kind == KIND_FILE && a.kids != 0 {
                return false;
            }
            let mut j = i + 1;
            while j < self.n {
                match self.ents.get(j) {
                    Some(b) => {
                        if b.name == a.name {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            i += 1;
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
            return Err(MV_ESLOT);
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
            return Err(MV_EBAL);
        }
        const VALS: [u32; 8] = [0, 1, 2, 101, 100, 101, 101, 101];
        const CODES: [u32; 6] =
            [MV_ENOENT, MV_EINVAL, MV_ENAMETOOLONG, MV_EISDIR, MV_ENOTEMPTY, MV_EROFS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(MV_EBAL),
            };
            if r.slot as usize != i {
                return Err(MV_ESLOT);
            }
            if r.ok {
                if r.code != 0 {
                    return Err(MV_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if r.value != *w {
                            return Err(MV_EVALUE);
                        }
                    }
                    None => return Err(MV_EBAL),
                }
                vi += 1;
            } else {
                if r.value != 0 {
                    return Err(MV_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if r.code != *w {
                            return Err(MV_EVALUE);
                        }
                    }
                    None => return Err(MV_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(MV_EBAL);
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
            return Err(MV_EBAL);
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
            Err(MV_ESEAL)
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

/// 十四格账本：三登记、两次改名、覆盖、自改名、一次查询、六类失败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut d = Ren::open();

    let _ = l.register(cell(0, OP_CREATE, d.create(10, 100, KIND_FILE)));
    let _ = l.register(cell(1, OP_CREATE, d.create(20, 101, KIND_FILE)));
    let _ = l.register(cell(2, OP_CREATE, d.create(30, 102, KIND_DIR)));
    let _ = l.register(word_cell(3, OP_RENAME, d.rename(20, 21).map(|v| v as u32)));
    let _ = l.register(word_cell(4, OP_RENAME, d.rename(10, 20).map(|v| v as u32)));
    let _ = l.register(word_cell(5, OP_RENAME, d.rename(21, 20).map(|v| v as u32)));
    let _ = l.register(word_cell(6, OP_RENAME, d.rename(20, 20).map(|v| v as u32)));
    let _ = l.register(word_cell(7, OP_LOOKUP, d.ino_of(20).map(|v| v as u32)));

    let _ = l.register(word_cell(8, OP_RENAME, d.rename(99, 5).map(|v| v as u32)));
    let _ = l.register(word_cell(9, OP_RENAME, d.rename(0, 5).map(|v| v as u32)));
    let _ = l.register(word_cell(10, OP_RENAME, d.rename(NAME_MAX + 1, 5).map(|v| v as u32)));
    let _ = l.register(word_cell(11, OP_RENAME, d.rename(20, 30).map(|v| v as u32)));

    let _ = d.create(40, 104, KIND_DIR);
    let _ = d.set_kids(30, 2);
    let _ = l.register(word_cell(12, OP_RENAME, d.rename(40, 30).map(|v| v as u32)));

    d.set_ro(true);
    let _ = l.register(word_cell(13, OP_RENAME, d.rename(20, 25).map(|v| v as u32)));

    l
}

/// 四路注入：账本身、改名正例、八类失败、闸门次序。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：改名保 inode、保位置；覆盖减一条；自改名空操作。
    let mut a = Ren::open();
    let c1 = a.create(10, 100, KIND_FILE);
    let c2 = a.create(20, 101, KIND_FILE);
    let c3 = a.create(30, 102, KIND_DIR);
    let m1 = a.rename(20, 21);
    let order_ok = a.name_at(0) == Some(10) && a.name_at(1) == Some(21) && a.name_at(2) == Some(30);
    let m2 = a.rename(21, 10);
    let count_after = a.count();
    let self_ok = a.rename(30, 30);
    out[1] = c1 == Ok(0)
        && c2 == Ok(1)
        && c3 == Ok(2)
        && m1 == Ok(101)
        && order_ok
        && m2 == Ok(101)
        && count_after == 2
        && a.name_at(0) == Some(10)
        && a.ino_at(0) == Some(101)
        && a.ino_of(10) == Ok(101)
        && a.replace_count() == 1
        && a.unlink_count() == 1
        && self_ok == Ok(102)
        && a.same_count() == 1
        && a.count() == 2
        && a.consistent();

    // 二路：八类失败各自可辨。
    let mut b = Ren::open();
    let bad_name = b.create(0, 1, KIND_FILE) == Err(MV_EINVAL);
    let long_name = b.create(NAME_MAX + 1, 2, KIND_FILE) == Err(MV_ENAMETOOLONG);
    let _ = b.create(10, 10, KIND_DIR);
    let _ = b.create(20, 20, KIND_FILE);
    let _ = b.create(30, 30, KIND_FILE);
    let missing = b.rename(99, 5) == Err(MV_ENOENT);
    let dir_to_file = b.rename(10, 20) == Err(MV_ENOTDIR);
    let file_to_dir = b.rename(20, 10) == Err(MV_EISDIR);
    let _ = b.set_kids(10, 1);
    let _ = b.create(40, 40, KIND_DIR);
    let nonempty = b.rename(40, 10) == Err(MV_ENOTEMPTY);
    let dup = b.create(30, 31, KIND_FILE) == Err(MV_EINVAL);
    let _ = b.set_pin(20, true);
    let busy = b.rename(20, 25) == Err(MV_EBUSY);
    b.set_ro(true);
    let ro = b.rename(30, 35) == Err(MV_EROFS);
    out[2] = bad_name
        && long_name
        && missing
        && dir_to_file
        && file_to_dir
        && nonempty
        && dup
        && busy
        && ro
        && b.consistent();

    // 三路：闸门次序（只读先于缺项、钉住先于类型、自改名也受钉住约束）。
    let mut c = Ren::open();
    let _ = c.create(10, 10, KIND_FILE);
    let _ = c.create(20, 20, KIND_DIR);
    c.set_ro(true);
    let ro_first = c.rename(99, 5) == Err(MV_EROFS);
    c.set_ro(false);
    let _ = c.set_pin(20, true);
    let pin_self = c.rename(20, 20) == Err(MV_EBUSY);
    let pin_before_type = c.rename(20, 10) == Err(MV_EBUSY);
    let _ = c.set_pin(20, false);
    let _ = c.set_kids(20, 3);
    let type_before_empty = c.rename(10, 20) == Err(MV_EISDIR);
    let empty_after = c.rename(10, 10) == Ok(10);
    let kids_kept = c.kids_of(20) == Ok(3);
    out[3] = ro_first
        && pin_self
        && pin_before_type
        && type_before_empty
        && empty_after
        && kids_kept
        && c.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11406_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11406.mv_same_dir_rename");
    let l = build_ledger();

    s.add_dyn(
        "MV1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "MV2-改名保留 inode",
        {
            let mut a = Ren::open();
            let _ = a.create(10, 100, KIND_FILE);
            let got = a.rename(10, 11);
            got == Ok(100)
                && a.ino_of(11) == Ok(100)
                && a.ino_of(10) == Err(MV_ENOENT)
                && a.count() == 1
                && l.sample(3).map(|x| x.value) == Some(101)
        },
        "static",
    );

    s.add(
        "MV3-改名保留位置",
        {
            let mut a = Ren::open();
            let _ = a.create(10, 1, KIND_FILE);
            let _ = a.create(20, 2, KIND_FILE);
            let _ = a.create(30, 3, KIND_FILE);
            let _ = a.rename(20, 25);
            a.name_at(0) == Some(10)
                && a.name_at(1) == Some(25)
                && a.name_at(2) == Some(30)
                && a.ino_at(1) == Some(2)
                && a.move_count() == 1
        },
        "static",
    );

    s.add(
        "MV4-覆盖摘掉目标条目",
        {
            let mut a = Ren::open();
            let _ = a.create(10, 100, KIND_FILE);
            let _ = a.create(20, 101, KIND_FILE);
            let got = a.rename(10, 20);
            got == Ok(100)
                && a.count() == 1
                && a.name_at(0) == Some(20)
                && a.ino_at(0) == Some(100)
                && a.replace_count() == 1
                && a.unlink_count() == 1
                && a.consistent()
                && l.sample(5).map(|x| x.value) == Some(101)
        },
        "static",
    );

    s.add(
        "MV5-自改名空操作",
        {
            let mut a = Ren::open();
            let _ = a.create(10, 100, KIND_FILE);
            let _ = a.create(20, 101, KIND_DIR);
            let one = a.rename(10, 10);
            let two = a.rename(20, 20);
            one == Ok(100)
                && two == Ok(101)
                && a.count() == 2
                && a.same_count() == 2
                && a.replace_count() == 0
                && a.unlink_count() == 0
                && l.sample(6).map(|x| x.value) == Some(101)
        },
        "static",
    );

    s.add(
        "MV6-空名与长名被拒",
        {
            let mut a = Ren::open();
            let zero = a.rename(0, 5) == Err(MV_EINVAL);
            let long = a.rename(NAME_MAX + 1, 5) == Err(MV_ENAMETOOLONG);
            let long_dst = a.rename(5, NAME_MAX + 1) == Err(MV_ENAMETOOLONG);
            zero && long && long_dst && l.sample(9).map(|x| x.code) == Some(MV_EINVAL)
        },
        "static",
    );

    s.add(
        "MV7-缺项被拒",
        {
            let mut a = Ren::open();
            let _ = a.create(10, 100, KIND_FILE);
            let missing = a.rename(99, 5) == Err(MV_ENOENT);
            let no_dst = a.rename(10, 11);
            missing
                && no_dst == Ok(100)
                && a.ino_of(11) == Ok(100)
                && l.sample(8).map(|x| x.code) == Some(MV_ENOENT)
        },
        "static",
    );

    s.add(
        "MV8-目录类型不匹配",
        {
            let mut a = Ren::open();
            let _ = a.create(10, 10, KIND_DIR);
            let _ = a.create(20, 20, KIND_FILE);
            let d2f = a.rename(10, 20) == Err(MV_ENOTDIR);
            let f2d = a.rename(20, 10) == Err(MV_EISDIR);
            d2f && f2d && a.count() == 2 && l.sample(11).map(|x| x.code) == Some(MV_EISDIR)
        },
        "static",
    );

    s.add(
        "MV9-非空目录被拒",
        {
            let mut a = Ren::open();
            let _ = a.create(10, 10, KIND_DIR);
            let _ = a.create(20, 20, KIND_DIR);
            let _ = a.set_kids(20, 1);
            let refused = a.rename(10, 20) == Err(MV_ENOTEMPTY);
            let _ = a.set_kids(20, 0);
            let allowed = a.rename(10, 20) == Ok(10);
            refused
                && allowed
                && a.count() == 1
                && a.kids_of(20) == Ok(0)
                && l.sample(12).map(|x| x.code) == Some(MV_ENOTEMPTY)
        },
        "static",
    );

    s.add(
        "MV10-只读先于缺项",
        {
            let mut a = Ren::open();
            let _ = a.create(10, 10, KIND_FILE);
            a.set_ro(true);
            let missing = a.rename(99, 5) == Err(MV_EROFS);
            let present = a.rename(10, 11) == Err(MV_EROFS);
            a.set_ro(false);
            missing && present && a.rename(10, 11) == Ok(10)
                && l.sample(13).map(|x| x.code) == Some(MV_EROFS)
        },
        "static",
    );

    s.add(
        "MV11-钉住条目被拒",
        {
            let mut a = Ren::open();
            let _ = a.create(10, 10, KIND_FILE);
            let _ = a.create(20, 20, KIND_DIR);
            let _ = a.set_pin(10, true);
            let src_pin = a.rename(10, 11) == Err(MV_EBUSY);
            let self_pin = a.rename(10, 10) == Err(MV_EBUSY);
            let _ = a.set_pin(10, false);
            let _ = a.set_pin(20, true);
            let dst_pin = a.rename(10, 20) == Err(MV_EBUSY);
            src_pin && self_pin && dst_pin && a.count() == 2 && a.consistent()
        },
        "static",
    );

    s.add(
        "MV12-重名与表满被拒",
        {
            let mut a = Ren::open();
            let dup_ok = a.create(10, 10, KIND_FILE).is_ok();
            let dup = a.create(10, 11, KIND_FILE) == Err(MV_EINVAL);
            let mut filled = true;
            while a.count() < CAP {
                let name = 100 + a.count() as u32;
                if a.create(name, (200 + a.count()) as u16, KIND_FILE).is_err() {
                    filled = false;
                }
            }
            let full = a.create(90, 900, KIND_FILE) == Err(MV_EINVAL);
            dup_ok && dup && filled && full && a.count() == CAP && a.consistent()
        },
        "static",
    );

    s.add(
        "MV13-冻结重放四注入",
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

    fn dir() -> Ren {
        Ren::open()
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
        let s = run_unx11406_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn rename_preserves_inode_and_slot() {
        let mut d = dir();
        assert_eq!(d.create(10, 100, KIND_FILE), Ok(0));
        assert_eq!(d.create(20, 101, KIND_FILE), Ok(1));
        assert_eq!(d.create(30, 102, KIND_FILE), Ok(2));
        assert_eq!(d.rename(20, 21), Ok(101));
        assert_eq!(d.name_at(1), Some(21));
        assert_eq!(d.ino_at(1), Some(101));
        assert_eq!(d.ino_of(21), Ok(101));
        assert_eq!(d.ino_of(20), Err(MV_ENOENT));
        assert_eq!(d.count(), 3);
        assert!(d.consistent());
    }

    #[test]
    fn replace_removes_target() {
        let mut d = dir();
        assert_eq!(d.create(10, 100, KIND_FILE), Ok(0));
        assert_eq!(d.create(20, 101, KIND_FILE), Ok(1));
        assert_eq!(d.create(30, 102, KIND_FILE), Ok(2));
        assert_eq!(d.rename(10, 30), Ok(100));
        assert_eq!(d.count(), 2);
        assert_eq!(d.name_at(0), Some(30));
        assert_eq!(d.ino_at(0), Some(100));
        assert_eq!(d.ino_of(20), Ok(101));
        assert_eq!(d.replace_count(), 1);
        assert_eq!(d.unlink_count(), 1);
        assert!(d.consistent());
    }

    #[test]
    fn self_rename_is_noop() {
        let mut d = dir();
        assert_eq!(d.create(10, 100, KIND_FILE), Ok(0));
        assert_eq!(d.rename(10, 10), Ok(100));
        assert_eq!(d.count(), 1);
        assert_eq!(d.same_count(), 1);
        assert_eq!(d.replace_count(), 0);
        assert_eq!(d.unlink_count(), 0);
    }

    #[test]
    fn type_and_emptiness_guards() {
        let mut d = dir();
        assert_eq!(d.create(10, 10, KIND_DIR), Ok(0));
        assert_eq!(d.create(20, 20, KIND_FILE), Ok(1));
        assert_eq!(d.rename(10, 20), Err(MV_ENOTDIR));
        assert_eq!(d.rename(20, 10), Err(MV_EISDIR));
        assert_eq!(d.create(30, 30, KIND_DIR), Ok(2));
        assert_eq!(d.set_kids(30, 2), Ok(()));
        assert_eq!(d.rename(10, 30), Err(MV_ENOTEMPTY));
        assert_eq!(d.set_kids(30, 0), Ok(()));
        assert_eq!(d.rename(10, 30), Ok(10));
        assert_eq!(d.count(), 2);
    }

    #[test]
    fn ro_and_pin_guards() {
        let mut d = dir();
        assert_eq!(d.create(10, 10, KIND_FILE), Ok(0));
        d.set_ro(true);
        assert_eq!(d.rename(99, 5), Err(MV_EROFS));
        assert_eq!(d.rename(10, 11), Err(MV_EROFS));
        assert_eq!(d.create(20, 20, KIND_FILE), Err(MV_EROFS));
        d.set_ro(false);
        assert_eq!(d.set_pin(10, true), Ok(()));
        assert_eq!(d.rename(10, 10), Err(MV_EBUSY));
        assert_eq!(d.set_pin(10, false), Ok(()));
        assert_eq!(d.rename(10, 10), Ok(10));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
