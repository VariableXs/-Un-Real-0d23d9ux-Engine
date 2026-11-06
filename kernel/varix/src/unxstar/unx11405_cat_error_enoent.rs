//! UNX-F11405 cat 错误处理（ENOENT 档）
//!
//! `cat` 是用户与命名空间之间最短的一条路径：给出若干路径分量，内核逐级解析，
//! 最后把目标文件的字节数报回来。这条路上每一步都可能失败，而**失败的先后
//! 次序本身就是契约**：同一条非法路径，先撞上长度闸门就必须报
//! `ENAMETOOLONG`，先撞上缺项就必须报 `ENOENT`，绝不能因为实现里的循环次序
//! 不同而给出两种答案。本账把这个次序钉死，并用十四格样本把每一类失败各留
//! 一条可复算的记录。
//!
//! 命名空间被压成一张定容条目表：根目录固定占 0 号，其余条目记录父目录下标、
//! 名字、类型（目录/普通文件/符号链接）、权限位与字节数。符号链接只记目标
//! 条目下标，因此既能指向尚不存在的条目（悬空链接，解析时报 `ENOENT`），也能
//! 指向自身（成环，预算耗尽时报 `ELOOP`）。
//!
//! ## `cat` 的闸门次序
//! 1. 空路径 → `EINVAL`；
//! 2. 分量数超过 `DEPTH_MAX` → `ENAMETOOLONG`，此闸先于任何查表；
//! 3. 逐分量：名字为 0 → `EINVAL`；名字超过 `NAME_MAX` → `ENAMETOOLONG`；
//!    当前目录缺执行位 → `EACCES`；查不到子项 → `ENOENT`；中间分量是链接则
//!    跟随，预算耗尽 → `ELOOP`；中间分量最终不是目录 → `ENOTDIR`；
//! 4. 末端：是目录 → `EISDIR`（先于读权限判定）；缺读位 → `EACCES`；
//!    字节数超过 `LIMIT` → `EFBIG`；否则报字节数，空文件报 0。
//!
//! ## 账本契约
//! 十四格：前八格成功（新登记两枚条目、普通读数、链接解引用、根目录子项计数、
//! 权限位、空文件、子目录计数），后六格按次序各撞一类失败（空路径、缺项、
//! 中段非目录、终端目录、读权限、链接环）。`audit` 逐格核对槽号、成功位、
//! 错误码与观测值；`seal` 对整账做 FNV-1a 指纹；`replay` 重建账本并要求指纹
//! 逐位相同。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xef50;

/// 路径或登记参数非法。
pub const CE_EINVAL: u32 = SEG_BASE + 0x01;
/// 路径分量在父目录中不存在。
pub const CE_ENOENT: u32 = SEG_BASE + 0x02;
/// 中间分量不是目录。
pub const CE_ENOTDIR: u32 = SEG_BASE + 0x03;
/// 末端是目录，`cat` 拒绝。
pub const CE_EISDIR: u32 = SEG_BASE + 0x04;
/// 目录缺执行位或文件缺读位。
pub const CE_EACCES: u32 = SEG_BASE + 0x05;
/// 符号链接预算耗尽（成环）。
pub const CE_ELOOP: u32 = SEG_BASE + 0x06;
/// 名字过长或路径过深。
pub const CE_ENAMETOOLONG: u32 = SEG_BASE + 0x07;
/// 文件字节数超过读取上限。
pub const CE_EFBIG: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CE_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CE_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CE_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CE_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CE_ENOBS: u32 = SEG_BASE + 0x0d;

/// 条目表容量。
pub const CAP: usize = 16;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 单文件读取上限（1 MiB）。
pub const LIMIT: u32 = 0x10_0000;
/// 名字分量上界。
pub const NAME_MAX: u32 = 0xff;
/// 路径分量数上界。
pub const DEPTH_MAX: usize = 32;
/// 单次解析允许跟随的链接数。
pub const LINK_MAX: u8 = 8;

/// 目录类型。
pub const KIND_DIR: u8 = 1;
/// 普通文件类型。
pub const KIND_FILE: u8 = 2;
/// 符号链接类型。
pub const KIND_LINK: u8 = 3;

/// 读权限位。
pub const MODE_READ: u8 = 0x04;
/// 执行（检索）权限位。
pub const MODE_EXEC: u8 = 0x01;

/// 操作码：登记条目。
pub const OP_ADD: u8 = 1;
/// 操作码：读取文件。
pub const OP_CAT: u8 = 2;
/// 操作码：取字节数。
pub const OP_STAT: u8 = 3;
/// 操作码：取权限位。
pub const OP_MODE: u8 = 4;
/// 操作码：数子项。
pub const OP_WALK: u8 = 5;

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

/// 一个命名空间条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    /// 名字（数值化分量）。
    pub name: u32,
    /// 父目录条目下标。
    pub parent: u16,
    /// 类型。
    pub kind: u8,
    /// 权限位（低三位 rwx）。
    pub perm: u8,
    /// 字节数。
    pub size: u32,
    /// 链接目标条目下标（仅链接有意义）。
    pub link: u16,
}

const BLANK_ENT: Ent =
    Ent { name: 0, parent: 0, kind: 0, perm: 0, size: 0, link: 0 };

/// 定容命名空间。
#[derive(Clone, Copy, Debug)]
pub struct Ns {
    ents: [Ent; CAP],
    n: usize,
    reads: u32,
    hops: u32,
    refused: u32,
}

impl Ns {
    /// 只有根目录的空命名空间。
    pub fn new() -> Ns {
        let mut ents = [BLANK_ENT; CAP];
        if let Some(slot) = ents.get_mut(0) {
            *slot = Ent { name: 0, parent: 0, kind: KIND_DIR, perm: 0o5, size: 0, link: 0 };
        }
        Ns { ents, n: 1, reads: 0, hops: 0, refused: 0 }
    }

    /// 条目数（含根）。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否只有根。
    pub fn is_empty(&self) -> bool {
        self.n <= 1
    }

    /// 第 i 个条目。
    pub fn ent_at(&self, i: usize) -> Option<Ent> {
        if i >= self.n {
            return None;
        }
        self.ents.get(i).copied()
    }

    /// 成功读取次数。
    pub fn read_count(&self) -> u32 {
        self.reads
    }

    /// 链接跟随次数。
    pub fn hop_count(&self) -> u32 {
        self.hops
    }

    /// 被拒次数。
    pub fn refused_count(&self) -> u32 {
        self.refused
    }

    /// 在同一父目录下按名字查条目。
    fn lookup(&self, parent: u16, name: u32) -> Option<u16> {
        let mut i = 0usize;
        while i < self.n {
            match self.ents.get(i) {
                Some(e) => {
                    if e.parent == parent && e.name == name {
                        return Some(i as u16);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 登记一枚普通条目（目录或普通文件），返回条目下标。
    pub fn add(
        &mut self,
        parent: u16,
        name: u32,
        kind: u8,
        perm: u8,
        size: u32,
    ) -> Result<usize, u32> {
        if name == 0 || kind == KIND_LINK || (kind != KIND_DIR && kind != KIND_FILE) {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_EINVAL);
        }
        if name > NAME_MAX {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_ENAMETOOLONG);
        }
        let p = match self.ents.get(parent as usize) {
            Some(v) => *v,
            None => {
                self.refused = self.refused.saturating_add(1);
                return Err(CE_ENOENT);
            }
        };
        if p.kind != KIND_DIR {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_ENOTDIR);
        }
        if self.lookup(parent, name).is_some() || self.n >= CAP {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_EINVAL);
        }
        if let Some(slot) = self.ents.get_mut(self.n) {
            *slot = Ent { name, parent, kind, perm, size, link: 0 };
        }
        self.n += 1;
        Ok(self.n - 1)
    }

    /// 登记一枚符号链接（目标允许尚不存在），返回条目下标。
    pub fn add_link(&mut self, parent: u16, name: u32, target: u16) -> Result<usize, u32> {
        if name == 0 {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_EINVAL);
        }
        if name > NAME_MAX {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_ENAMETOOLONG);
        }
        let p = match self.ents.get(parent as usize) {
            Some(v) => *v,
            None => {
                self.refused = self.refused.saturating_add(1);
                return Err(CE_ENOENT);
            }
        };
        if p.kind != KIND_DIR {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_ENOTDIR);
        }
        if self.lookup(parent, name).is_some() || self.n >= CAP {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_EINVAL);
        }
        if let Some(slot) = self.ents.get_mut(self.n) {
            *slot = Ent { name, parent, kind: KIND_LINK, perm: 0o7, size: 0, link: target };
        }
        self.n += 1;
        Ok(self.n - 1)
    }

    /// 解析路径，返回末端条目下标。
    ///
    /// 闸门次序：空路径 → 过深 → 逐分量（零名/长名 → 目录执行位 → 缺项 →
    /// 链接预算 → 中间分量非目录）。
    fn resolve(&mut self, path: &[u32]) -> Result<u16, u32> {
        if path.is_empty() {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_EINVAL);
        }
        if path.len() > DEPTH_MAX {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_ENAMETOOLONG);
        }
        let mut cur: u16 = 0;
        let mut hops: u8 = 0;
        let mut i = 0usize;
        while i < path.len() {
            let name = match path.get(i) {
                Some(v) => *v,
                None => return Err(CE_EINVAL),
            };
            if name == 0 {
                self.refused = self.refused.saturating_add(1);
                return Err(CE_EINVAL);
            }
            if name > NAME_MAX {
                self.refused = self.refused.saturating_add(1);
                return Err(CE_ENAMETOOLONG);
            }
            let here = match self.ents.get(cur as usize) {
                Some(v) => *v,
                None => return Err(CE_EINVAL),
            };
            if here.kind != KIND_DIR {
                self.refused = self.refused.saturating_add(1);
                return Err(CE_ENOTDIR);
            }
            if here.perm & MODE_EXEC == 0 {
                self.refused = self.refused.saturating_add(1);
                return Err(CE_EACCES);
            }
            let mut idx = match self.lookup(cur, name) {
                Some(v) => v,
                None => {
                    self.refused = self.refused.saturating_add(1);
                    return Err(CE_ENOENT);
                }
            };
            loop {
                let e = match self.ents.get(idx as usize) {
                    Some(v) => *v,
                    None => {
                        self.refused = self.refused.saturating_add(1);
                        return Err(CE_ENOENT);
                    }
                };
                if e.kind != KIND_LINK {
                    break;
                }
                hops = hops.saturating_add(1);
                if hops > LINK_MAX {
                    self.refused = self.refused.saturating_add(1);
                    return Err(CE_ELOOP);
                }
                idx = e.link;
                if (idx as usize) >= self.n {
                    self.refused = self.refused.saturating_add(1);
                    return Err(CE_ENOENT);
                }
                self.hops = self.hops.saturating_add(1);
            }
            let e = match self.ents.get(idx as usize) {
                Some(v) => *v,
                None => return Err(CE_ENOENT),
            };
            if i + 1 < path.len() && e.kind != KIND_DIR {
                self.refused = self.refused.saturating_add(1);
                return Err(CE_ENOTDIR);
            }
            cur = idx;
            i += 1;
        }
        Ok(cur)
    }

    /// 读取一个文件的字节数。
    pub fn cat(&mut self, path: &[u32]) -> Result<u32, u32> {
        let idx = self.resolve(path)?;
        let e = match self.ents.get(idx as usize) {
            Some(v) => *v,
            None => return Err(CE_ENOENT),
        };
        if e.kind == KIND_DIR {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_EISDIR);
        }
        if e.perm & MODE_READ == 0 {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_EACCES);
        }
        if e.size > LIMIT {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_EFBIG);
        }
        self.reads = self.reads.saturating_add(1);
        Ok(e.size)
    }

    /// 取字节数（不要求读权限，目录报 0）。
    pub fn stat(&mut self, path: &[u32]) -> Result<u32, u32> {
        let idx = self.resolve(path)?;
        match self.ents.get(idx as usize) {
            Some(e) => Ok(e.size),
            None => Err(CE_ENOENT),
        }
    }

    /// 取权限位。
    pub fn mode_of(&mut self, path: &[u32]) -> Result<u8, u32> {
        let idx = self.resolve(path)?;
        match self.ents.get(idx as usize) {
            Some(e) => Ok(e.perm),
            None => Err(CE_ENOENT),
        }
    }

    /// 数一个目录的直接子项数。
    pub fn walk(&mut self, path: &[u32]) -> Result<u32, u32> {
        let idx = if path.is_empty() { 0 } else { self.resolve(path)? };
        let root = match self.ents.get(idx as usize) {
            Some(v) => *v,
            None => return Err(CE_ENOENT),
        };
        if root.kind != KIND_DIR {
            self.refused = self.refused.saturating_add(1);
            return Err(CE_EISDIR);
        }
        let mut count = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.ents.get(i) {
                Some(e) => {
                    if e.parent == idx && (i as u16) != idx {
                        count = count.saturating_add(1);
                    }
                }
                None => break,
            }
            i += 1;
        }
        Ok(count)
    }

    /// 不变式自检：根必须是目录，父下标有效，同父下不重名。
    pub fn consistent(&self) -> bool {
        if self.n < 1 || self.n > CAP {
            return false;
        }
        let root = match self.ents.get(0) {
            Some(v) => *v,
            None => return false,
        };
        if root.kind != KIND_DIR || root.parent != 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.ents.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a.kind != KIND_DIR && a.kind != KIND_FILE && a.kind != KIND_LINK {
                return false;
            }
            if (a.parent as usize) >= self.n {
                return false;
            }
            if a.name > NAME_MAX {
                return false;
            }
            let parent = match self.ents.get(a.parent as usize) {
                Some(v) => *v,
                None => return false,
            };
            if i != 0 {
                if a.name == 0 || parent.kind != KIND_DIR {
                    return false;
                }
            }
            let mut j = i + 1;
            while j < self.n {
                match self.ents.get(j) {
                    Some(b) => {
                        if b.parent == a.parent && b.name == a.name {
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
            return Err(CE_ESLOT);
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

    /// 对账：形状、槽号、成功位、错误码、观测值。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(CE_EBAL);
        }
        const VALS: [u32; 8] = [9, 10, 120, 120, 7, 4, 0, 3];
        const CODES: [u32; 6] =
            [CE_EINVAL, CE_ENOENT, CE_ENOTDIR, CE_EISDIR, CE_EACCES, CE_ELOOP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CE_EBAL),
            };
            if r.slot as usize != i {
                return Err(CE_ESLOT);
            }
            if r.ok {
                if r.code != 0 {
                    return Err(CE_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if r.value != *w {
                            return Err(CE_EVALUE);
                        }
                    }
                    None => return Err(CE_EBAL),
                }
                vi += 1;
            } else {
                if r.value != 0 {
                    return Err(CE_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if r.code != *w {
                            return Err(CE_EVALUE);
                        }
                    }
                    None => return Err(CE_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CE_EBAL);
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
            return Err(CE_EBAL);
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
            Err(CE_ESEAL)
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

/// 账本用的固定命名空间。
///
/// ```text
/// 0  /            DIR  0o5
/// 1  /10          DIR  0o5
/// 2  /10/20       FILE 0o4  120 字节
/// 3  /10/30       FILE 0o0   80 字节
/// 4  /40          LINK -> 4（自环）
/// 5  /50          FILE 0o4    0 字节
/// 6  /60          FILE 0o4  LIMIT+1 字节
/// 7  /70          LINK -> 2（指向普通文件）
/// 8  /80          LINK -> 99（悬空）
/// ```
fn fixture() -> Ns {
    let mut f = Ns::new();
    let _ = f.add(0, 10, KIND_DIR, 0o5, 0);
    let _ = f.add(1, 20, KIND_FILE, 0o4, 120);
    let _ = f.add(1, 30, KIND_FILE, 0o0, 80);
    let _ = f.add_link(0, 40, 4);
    let _ = f.add(0, 50, KIND_FILE, 0o4, 0);
    let _ = f.add(0, 60, KIND_FILE, 0o4, LIMIT + 1);
    let _ = f.add_link(0, 70, 2);
    let _ = f.add_link(0, 80, 99);
    f
}

/// 十四格账本：登记两枚条目、读数、链接、计数、权限位、空文件、六类失败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut f = fixture();

    let _ = l.register(cell(0, OP_ADD, f.add(0, 90, KIND_FILE, 0o4, 40)));
    let _ = l.register(cell(1, OP_ADD, f.add(1, 21, KIND_DIR, 0o5, 0)));
    let _ = l.register(word_cell(2, OP_CAT, f.cat(&[10, 20])));
    let _ = l.register(word_cell(3, OP_CAT, f.cat(&[70])));
    let _ = l.register(word_cell(4, OP_WALK, f.walk(&[])));
    let _ = l.register(byte_cell(5, OP_MODE, f.mode_of(&[10, 20])));
    let _ = l.register(word_cell(6, OP_CAT, f.cat(&[50])));
    let _ = l.register(word_cell(7, OP_WALK, f.walk(&[10])));

    let _ = l.register(word_cell(8, OP_CAT, f.cat(&[])));
    let _ = l.register(word_cell(9, OP_CAT, f.cat(&[99])));
    let _ = l.register(word_cell(10, OP_CAT, f.cat(&[50, 20])));
    let _ = l.register(word_cell(11, OP_CAT, f.cat(&[10])));
    let _ = l.register(word_cell(12, OP_CAT, f.cat(&[10, 30])));
    let _ = l.register(word_cell(13, OP_CAT, f.cat(&[40])));

    l
}

/// 四路注入：账本身、正例、八类失败、闸门次序。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：普通读数、链接解引用、目录计数、权限位。
    let mut a = fixture();
    let plain = a.cat(&[10, 20]);
    let empty = a.cat(&[50]);
    let via_link = a.cat(&[70]);
    let dir_bytes = a.stat(&[10]);
    let kids_root = a.walk(&[]);
    let kids_sub = a.walk(&[10]);
    let perm = a.mode_of(&[10, 20]);
    let new_idx = a.add(0, 90, KIND_FILE, 0o4, 40);
    out[1] = plain == Ok(120)
        && empty == Ok(0)
        && via_link == Ok(120)
        && dir_bytes == Ok(0)
        && kids_root == Ok(6)
        && kids_sub == Ok(2)
        && perm == Ok(4)
        && new_idx == Ok(9)
        && a.read_count() == 3
        && a.hop_count() == 1
        && a.consistent();

    // 二路：八类失败各自可辨。
    let mut b = fixture();
    let empty_path = b.cat(&[]) == Err(CE_EINVAL);
    let zero_name = b.cat(&[0]) == Err(CE_EINVAL);
    let missing = b.cat(&[99]) == Err(CE_ENOENT);
    let dangling = b.cat(&[80]) == Err(CE_ENOENT);
    let not_dir = b.cat(&[50, 20]) == Err(CE_ENOTDIR);
    let is_dir = b.cat(&[10]) == Err(CE_EISDIR);
    let denied = b.cat(&[10, 30]) == Err(CE_EACCES);
    let looping = b.cat(&[40]) == Err(CE_ELOOP);
    let long_name = b.cat(&[NAME_MAX + 1]) == Err(CE_ENAMETOOLONG);
    let mut deep = [10u32; DEPTH_MAX + 1];
    let too_deep = b.cat(&deep) == Err(CE_ENAMETOOLONG);
    let huge = b.cat(&[60]) == Err(CE_EFBIG);
    if let Some(first) = deep.get_mut(0) {
        *first = 99;
    }
    out[2] = empty_path
        && zero_name
        && missing
        && dangling
        && not_dir
        && is_dir
        && denied
        && looping
        && long_name
        && too_deep
        && huge
        && b.consistent();

    // 三路：闸门次序（长度先于查表、目录先于读权限、权限先于大小）。
    let mut c = fixture();
    let _ = c.add(0, 12, KIND_DIR, 0o1, 0);
    let _ = c.add(0, 13, KIND_FILE, 0o0, LIMIT + 1);
    let _ = c.add(0, 14, KIND_DIR, 0o0, 0);
    let order_long = c.cat(&[NAME_MAX + 1]) == Err(CE_ENAMETOOLONG);
    let order_missing = c.cat(&[10, 61]) == Err(CE_ENOENT);
    let order_dir = c.cat(&[12]) == Err(CE_EISDIR);
    let order_deny = c.cat(&[13]) == Err(CE_EACCES);
    let order_exec = c.cat(&[14, 99]) == Err(CE_EACCES);
    let order_link = c.cat(&[70]) == Ok(120);
    out[3] = order_long
        && order_missing
        && order_dir
        && order_deny
        && order_exec
        && order_link
        && c.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11405_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11405.cat_error_enoent");
    let l = build_ledger();

    s.add_dyn(
        "CE1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CE2-普通文件读数",
        l.sample(2).map(|x| x.value) == Some(120)
            && l.sample(6).map(|x| x.value) == Some(0)
            && l.sample(0).map(|x| x.value) == Some(9),
        "static",
    );

    s.add(
        "CE3-链接解引用",
        {
            let mut a = fixture();
            let through = a.cat(&[70]);
            let dangling = a.cat(&[80]);
            through == Ok(120)
                && dangling == Err(CE_ENOENT)
                && a.hop_count() == 1
                && l.sample(3).map(|x| x.value) == Some(120)
        },
        "static",
    );

    s.add(
        "CE4-目录子项计数",
        {
            let mut a = fixture();
            let root = a.walk(&[]);
            let sub = a.walk(&[10]);
            let on_file = a.walk(&[50]);
            root == Ok(6)
                && sub == Ok(2)
                && on_file == Err(CE_EISDIR)
                && l.sample(4).map(|x| x.value) == Some(7)
                && l.sample(7).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "CE5-空路径零名被拒",
        l.sample(8).map(|x| x.code) == Some(CE_EINVAL)
            && {
                let mut a = fixture();
                a.cat(&[0]) == Err(CE_EINVAL)
                    && a.add(0, 0, KIND_FILE, 0o4, 1) == Err(CE_EINVAL)
                    && a.consistent()
            },
        "static",
    );

    s.add(
        "CE6-缺项报 ENOENT",
        l.sample(9).map(|x| x.code) == Some(CE_ENOENT)
            && {
                let mut a = fixture();
                a.cat(&[10, 99]) == Err(CE_ENOENT)
                    && a.cat(&[80]) == Err(CE_ENOENT)
                    && a.consistent()
            },
        "static",
    );

    s.add(
        "CE7-中段非目录被拒",
        l.sample(10).map(|x| x.code) == Some(CE_ENOTDIR)
            && {
                let mut a = fixture();
                a.cat(&[70, 20]) == Err(CE_ENOTDIR)
                    && a.add(2, 22, KIND_FILE, 0o4, 1) == Err(CE_ENOTDIR)
            },
        "static",
    );

    s.add(
        "CE8-终端目录被拒",
        l.sample(11).map(|x| x.code) == Some(CE_EISDIR)
            && {
                let mut a = fixture();
                let _ = a.add(0, 12, KIND_DIR, 0o1, 0);
                a.cat(&[12]) == Err(CE_EISDIR)
            },
        "static",
    );

    s.add(
        "CE9-读权限先于大小",
        l.sample(12).map(|x| x.code) == Some(CE_EACCES)
            && {
                let mut a = fixture();
                let _ = a.add(0, 13, KIND_FILE, 0o0, LIMIT + 1);
                a.cat(&[13]) == Err(CE_EACCES)
                    && a.stat(&[13]) == Ok(LIMIT + 1)
            },
        "static",
    );

    s.add(
        "CE10-链接环预算",
        l.sample(13).map(|x| x.code) == Some(CE_ELOOP)
            && {
                let mut a = Ns::new();
                let _ = a.add(0, 1, KIND_FILE, 0o4, 7);
                let mut prev = 1u16;
                let mut i = 0u32;
                let mut ok = true;
                while i < 7 {
                    match a.add_link(0, 10 + i, prev) {
                        Ok(idx) => prev = idx as u16,
                        Err(_) => ok = false,
                    }
                    i += 1;
                }
                let mut b = Ns::new();
                let _ = b.add(0, 1, KIND_FILE, 0o4, 7);
                let mut prev2 = 1u16;
                let mut j = 0u32;
                while j < 9 {
                    match b.add_link(0, 20 + j, prev2) {
                        Ok(idx) => prev2 = idx as u16,
                        Err(_) => ok = false,
                    }
                    j += 1;
                }
                ok && a.cat(&[16]) == Ok(7) && b.cat(&[28]) == Err(CE_ELOOP)
            },
        "static",
    );

    s.add(
        "CE11-超长名与过深路径",
        {
            let mut a = fixture();
            let long = a.cat(&[NAME_MAX + 1]) == Err(CE_ENAMETOOLONG);
            let deep = [10u32; DEPTH_MAX + 1];
            let path = a.cat(&deep) == Err(CE_ENAMETOOLONG);
            let zero_first = a.cat(&[0]) == Err(CE_EINVAL);
            let add_long = a.add(0, NAME_MAX + 1, KIND_FILE, 0o4, 1) == Err(CE_ENAMETOOLONG);
            long && path && zero_first && add_long
        },
        "static",
    );

    s.add(
        "CE12-过大文件被拒",
        {
            let mut a = fixture();
            let big = a.cat(&[60]);
            let size = a.stat(&[60]);
            big == Err(CE_EFBIG) && size == Ok(LIMIT + 1) && a.read_count() == 0
        },
        "static",
    );

    s.add(
        "CE13-冻结重放四注入",
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

    fn ns() -> Ns {
        fixture()
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
        let s = run_unx11405_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn cat_plain_and_links() {
        let mut a = ns();
        assert_eq!(a.cat(&[10, 20]), Ok(120));
        assert_eq!(a.cat(&[70]), Ok(120));
        assert_eq!(a.cat(&[50]), Ok(0));
        assert_eq!(a.cat(&[80]), Err(CE_ENOENT));
        assert_eq!(a.hop_count(), 1);
        assert!(a.consistent());
    }

    #[test]
    fn missing_and_not_dir() {
        let mut a = ns();
        assert_eq!(a.cat(&[99]), Err(CE_ENOENT));
        assert_eq!(a.cat(&[10, 99]), Err(CE_ENOENT));
        assert_eq!(a.cat(&[50, 20]), Err(CE_ENOTDIR));
        assert_eq!(a.cat(&[70, 20]), Err(CE_ENOTDIR));
        assert_eq!(a.cat(&[]), Err(CE_EINVAL));
    }

    #[test]
    fn dir_and_permission() {
        let mut a = ns();
        assert_eq!(a.cat(&[10]), Err(CE_EISDIR));
        assert_eq!(a.cat(&[10, 30]), Err(CE_EACCES));
        assert_eq!(a.mode_of(&[10, 30]), Ok(0));
        let _ = a.add(0, 12, KIND_DIR, 0o1, 0);
        assert_eq!(a.cat(&[12]), Err(CE_EISDIR));
        assert_eq!(a.stat(&[12]), Ok(0));
    }

    #[test]
    fn loop_and_limits() {
        let mut a = ns();
        assert_eq!(a.cat(&[40]), Err(CE_ELOOP));
        assert_eq!(a.cat(&[60]), Err(CE_EFBIG));
        assert_eq!(a.cat(&[NAME_MAX + 1]), Err(CE_ENAMETOOLONG));
        let deep = [10u32; DEPTH_MAX + 1];
        assert_eq!(a.cat(&deep), Err(CE_ENAMETOOLONG));
        assert_eq!(a.walk(&[]), Ok(6));
        assert!(a.consistent());
    }

    #[test]
    fn registrations_are_guarded() {
        let mut a = ns();
        assert_eq!(a.add(0, 0, KIND_FILE, 0o4, 1), Err(CE_EINVAL));
        assert_eq!(a.add(0, 10, KIND_FILE, 0o4, 1), Err(CE_EINVAL));
        assert_eq!(a.add(99, 111, KIND_FILE, 0o4, 1), Err(CE_ENOENT));
        assert_eq!(a.add(2, 22, KIND_FILE, 0o4, 1), Err(CE_ENOTDIR));
        assert!(a.add_link(0, 90, 2).is_ok());
        assert_eq!(a.walk(&[90]), Err(CE_EISDIR));
        assert!(a.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
