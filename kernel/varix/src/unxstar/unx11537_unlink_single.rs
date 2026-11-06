//! UNX-F11537 unlink 单件删除
//!
//! `unlink` 删除一个目录项，而不是删除文件本身。文件是否真正消失，取决于链接
//! 计数：每删除一个目录项，目标 inode 的 `st_nlink` 减一，减到零才释放数据。
//! 因此同一个文件可以有两个路径，删掉其中一个之后另一个仍可访问；这条不变式
//! 是硬链接语义的全部，也是本模块账本里「首删剩余 1、再删剩余 0」两格的分界。
//!
//! 路径闸门先于文件闸门。空路径、相对路径、尾斜杠、`//`、`/./`、`/../` 都在
//! 语法层拒绝（`UNL_EPATH`）；超过名字上限的路径在长度层拒绝（`UNL_ENAME`）。
//! 这两类错误不需要查表，因此它们比 `UNL_ENOENT` 更早出现，调用方不会因为
//! 「路径写错」而误判成「文件不存在」。
//!
//! 查到目录项之后，闸门按固定次序展开：目录不可 unlink（`UNL_EISDIR`）；
//! 只读项（`UNL_EROFS`）；不可变项（`UNL_EPERM`）；仍被打开的文件
//! （`UNL_EBUSY`）；链接计数已经为零的残项（`UNL_ELINK`）。次序固定意味着
//! 同一个坏项无论从哪条测试进入，返回的 errno 都一致，账本才可复算。
//!
//! 删除成功后目录项不立即挪动，只把链接计数减一；计数归零时把种类标成 FREE，
//! 路径仍占着槽位，直到 `compact` 压缩。压缩是单向操作，第二次调用返回
//! `UNL_ESTATE`，因为此时的槽位布局已经改变，重复压缩没有定义。
//!
//! `walk` 有两种统计模式：LIVE 数存活目录项，LINKS 汇总存活项的链接计数。
//! 模式码非法返回 `UNL_EMODE`。这两个统计值在删除前后必须满足守恒：删除一个
//! 双链接文件后 LIVE 减一、LINKS 减一；删除最后一个链接后 LINKS 再减一。
//!
//! 闸门次序：
//!
//! - `path_ok`：空/相对/尾斜杠/点段 → 超长；
//! - `Fs::unlink`：查表 → 目录 → 只读 → 不可变 → 占用 → 链接计数；
//! - `Fs::walk`：模式码；
//! - `Fs::compact`：重复压缩。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf2a0;

/// 路径语法非法。
pub const UNL_EPATH: u32 = SEG_BASE + 0x01;
/// 路径超长。
pub const UNL_ENAME: u32 = SEG_BASE + 0x02;
/// 不存在。
pub const UNL_ENOENT: u32 = SEG_BASE + 0x03;
/// 目标是目录。
pub const UNL_EISDIR: u32 = SEG_BASE + 0x04;
/// 只读。
pub const UNL_EROFS: u32 = SEG_BASE + 0x05;
/// 权限或不可变。
pub const UNL_EPERM: u32 = SEG_BASE + 0x06;
/// 仍被占用。
pub const UNL_EBUSY: u32 = SEG_BASE + 0x07;
/// 链接计数异常。
pub const UNL_ELINK: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const UNL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const UNL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const UNL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const UNL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const UNL_ENOBS: u32 = SEG_BASE + 0x0d;
/// 遍历模式码非法（既不是 LIVE 也不是 LINKS，一个计数都不产出）。
pub const UNL_EMODE: u32 = SEG_BASE + 0x0e;
/// 状态违例：压缩是单向操作，第二次调用没有定义。
pub const UNL_ESTATE: u32 = SEG_BASE + 0x0f;

/// 判定类错误码个数（路径/名字/存在性/种类/占用/链接）。
pub const GATE_CODE_COUNT: usize = 8;
/// 账本类错误码个数（平衡/槽位/取值/封印/观测/状态）。
pub const LEDGER_CODE_COUNT: usize = 6;
/// 错误码总数（闸门 8 + 账本 6 + 用法 1）。
pub const CODE_COUNT: usize = GATE_CODE_COUNT + LEDGER_CODE_COUNT + 1;

/// 目录项容量。
pub const CAP: usize = 12;
/// 路径字节上限。
pub const NAMEMAX: usize = 32;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 种类：空槽。
pub const KIND_FREE: u8 = 0;
/// 种类：普通文件。
pub const KIND_FILE: u8 = 1;
/// 种类：目录。
pub const KIND_DIR: u8 = 2;
/// 种类：符号链接。
pub const KIND_LINK: u8 = 3;

/// 标志：只读。
pub const FLAG_RO: u8 = 1;
/// 标志：不可变。
pub const FLAG_IMM: u8 = 2;

/// 遍历模式：存活项。
pub const WALK_LIVE: u8 = 1;
/// 遍历模式：链接总数。
pub const WALK_LINKS: u8 = 2;

/// 操作码：删除。
pub const OP_UNLINK: u8 = 1;
/// 操作码：查找。
pub const OP_LOOKUP: u8 = 2;
/// 操作码：遍历。
pub const OP_WALK: u8 = 3;
/// 操作码：压缩。
pub const OP_COMPACT: u8 = 4;

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
pub struct Entry {
    /// 路径字节。
    pub path: [u8; NAMEMAX],
    /// 路径长度。
    pub plen: u8,
    /// 种类。
    pub kind: u8,
    /// 链接计数。
    pub links: u16,
    /// 打开计数。
    pub open: u16,
    /// 标志位。
    pub flags: u8,
}

impl Entry {
    /// 空槽。
    pub const fn free() -> Entry {
        Entry { path: [0u8; NAMEMAX], plen: 0, kind: KIND_FREE, links: 0, open: 0, flags: 0 }
    }

    /// 路径切片。
    pub fn name(&self) -> &[u8] {
        self.path.get(..self.plen as usize).unwrap_or(&[])
    }
}

/// 删除结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 剩余链接。
    pub links: u16,
    /// 是否释放。
    pub freed: bool,
}

/// 路径语法闸门。
pub fn path_ok(path: &[u8]) -> Result<(), u32> {
    if path.is_empty() {
        return Err(UNL_EPATH);
    }
    if path.len() > NAMEMAX {
        return Err(UNL_ENAME);
    }
    match path.first() {
        Some(b'/') => {}
        _ => return Err(UNL_EPATH),
    }
    if path.len() > 1 {
        match path.last() {
            Some(b'/') => return Err(UNL_EPATH),
            _ => {}
        }
    }
    let mut start = 1usize;
    let mut i = 1usize;
    while i <= path.len() {
        let at_end = i == path.len();
        let is_sep = match path.get(i) {
            Some(b'/') => true,
            None => false,
            _ => false,
        };
        if at_end || is_sep {
            let comp = match path.get(start..i) {
                Some(v) => v,
                None => return Err(UNL_EPATH),
            };
            if !at_end {
                if comp.is_empty() {
                    return Err(UNL_EPATH);
                }
                if comp == b"." || comp == b".." {
                    return Err(UNL_EPATH);
                }
            } else if comp.is_empty() {
                // 根路径 "/" 单独处理：目录闸门会拒绝。
                if path.len() != 1 {
                    return Err(UNL_EPATH);
                }
            }
            let mut k = 0usize;
            while k < comp.len() {
                match comp.get(k) {
                    Some(b) => {
                        if *b < 0x20 || *b > 0x7e {
                            return Err(UNL_EPATH);
                        }
                    }
                    None => return Err(UNL_EPATH),
                }
                k += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    Ok(())
}

/// 目录项工厂。
fn mk(path: &[u8], kind: u8, links: u16, open: u16, flags: u8) -> Entry {
    let mut e = Entry::free();
    e.kind = kind;
    e.links = links;
    e.open = open;
    e.flags = flags;
    if path.len() <= NAMEMAX {
        let mut i = 0usize;
        while i < path.len() {
            if let Some(b) = path.get(i) {
                if let Some(dst) = e.path.get_mut(i) {
                    *dst = *b;
                }
            }
            i += 1;
        }
        e.plen = path.len() as u8;
    }
    e
}

/// 固定表的迷你文件系统。
#[derive(Clone, Copy, Debug)]
pub struct Fs {
    rows: [Entry; CAP],
    used: usize,
    compacted: bool,
}

impl Fs {
    /// 空表。
    pub const fn new() -> Fs {
        Fs { rows: [Entry::free(); CAP], used: 0, compacted: false }
    }

    /// 装填一项（内部使用，保持路径唯一）。
    fn put(&mut self, e: Entry) {
        if self.used < CAP {
            if let Some(dst) = self.rows.get_mut(self.used) {
                *dst = e;
            }
            self.used += 1;
        }
    }

    /// 查找下标。
    fn locate(&self, path: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.used {
            if let Some(e) = self.rows.get(i) {
                if e.kind != KIND_FREE && e.name() == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 查找目录项。
    pub fn lookup(&self, path: &[u8]) -> Result<Entry, u32> {
        path_ok(path)?;
        match self.locate(path) {
            Some(i) => match self.rows.get(i) {
                Some(e) => Ok(*e),
                None => Err(UNL_ENOENT),
            },
            None => Err(UNL_ENOENT),
        }
    }

    /// 删除一个目录项。
    pub fn unlink(&mut self, path: &[u8]) -> Result<Outcome, u32> {
        path_ok(path)?;
        let i = match self.locate(path) {
            Some(v) => v,
            None => return Err(UNL_ENOENT),
        };
        let mut e = match self.rows.get(i) {
            Some(v) => *v,
            None => return Err(UNL_ENOENT),
        };
        if e.kind == KIND_DIR {
            return Err(UNL_EISDIR);
        }
        if e.flags & FLAG_RO != 0 {
            return Err(UNL_EROFS);
        }
        if e.flags & FLAG_IMM != 0 {
            return Err(UNL_EPERM);
        }
        if e.open != 0 {
            return Err(UNL_EBUSY);
        }
        if e.links == 0 {
            return Err(UNL_ELINK);
        }
        e.links -= 1;
        let freed = e.links == 0;
        if freed {
            e.kind = KIND_FREE;
        }
        if let Some(dst) = self.rows.get_mut(i) {
            *dst = e;
        }
        Ok(Outcome { links: e.links, freed })
    }

    /// 统计。
    pub fn walk(&self, mode: u8) -> Result<u32, u32> {
        if mode != WALK_LIVE && mode != WALK_LINKS {
            return Err(UNL_EMODE);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.used {
            if let Some(e) = self.rows.get(i) {
                if e.kind != KIND_FREE {
                    if mode == WALK_LIVE {
                        n = n.saturating_add(1);
                    } else {
                        n = n.saturating_add(e.links as u32);
                    }
                }
            }
            i += 1;
        }
        Ok(n)
    }

    /// 压缩空槽（只允许一次）。
    pub fn compact(&mut self) -> Result<usize, u32> {
        if self.compacted {
            return Err(UNL_ESTATE);
        }
        let mut w = 0usize;
        let mut i = 0usize;
        let mut removed = 0usize;
        while i < self.used {
            let e = match self.rows.get(i) {
                Some(v) => *v,
                None => break,
            };
            if e.kind != KIND_FREE {
                if w != i {
                    if let Some(dst) = self.rows.get_mut(w) {
                        *dst = e;
                    }
                }
                w += 1;
            } else {
                removed += 1;
            }
            i += 1;
        }
        while w < self.used {
            if let Some(dst) = self.rows.get_mut(w) {
                *dst = Entry::free();
            }
            w += 1;
        }
        self.used = self.used.saturating_sub(removed);
        self.compacted = true;
        Ok(removed)
    }

    /// 已用槽位。
    pub fn used(&self) -> usize {
        self.used
    }

    /// 是否已压缩。
    pub fn compacted(&self) -> bool {
        self.compacted
    }
}

/// 种子文件系统。
fn seed() -> Fs {
    let mut fs = Fs::new();
    fs.put(mk(b"/", KIND_DIR, 2, 0, 0));
    fs.put(mk(b"/a", KIND_FILE, 2, 0, 0));
    fs.put(mk(b"/b", KIND_FILE, 1, 0, 0));
    fs.put(mk(b"/dir", KIND_DIR, 2, 0, 0));
    fs.put(mk(b"/ro", KIND_FILE, 1, 0, FLAG_RO));
    fs.put(mk(b"/imm", KIND_FILE, 1, 0, FLAG_IMM));
    fs.put(mk(b"/busy", KIND_FILE, 1, 1, 0));
    fs
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

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
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
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UNL_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
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
            if let Some(s) = self.rows.get(i) {
                if s.ok {
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
            return Err(UNL_EBAL);
        }
        const VALS: [u32; 8] = [1, 0, 0, 1, 6, 7, 10, 1];
        const CODES: [u32; 6] = [
            UNL_EPATH,
            UNL_EISDIR,
            UNL_EROFS,
            UNL_EPERM,
            UNL_EBUSY,
            UNL_ENOENT,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(UNL_EBAL),
            };
            if s.slot as usize != i {
                return Err(UNL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(UNL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(UNL_EVALUE);
                        }
                    }
                    None => return Err(UNL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(UNL_EVALUE);
                        }
                    }
                    None => return Err(UNL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(UNL_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(UNL_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let r = fresh.seal();
        if r == self.seal() {
            Ok(r)
        } else {
            Err(UNL_ESEAL)
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

/// 十四格账本：链接递减、释放、遍历、压缩与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let mut fs0 = seed();
    let _ = l.register(word_cell(0, OP_UNLINK, fs0.unlink(b"/a").map(|o| o.links as u32)));

    let mut fs1 = seed();
    let _ = l.register(word_cell(1, OP_UNLINK, fs1.unlink(b"/b").map(|o| o.links as u32)));

    let mut fs2 = seed();
    let _ = fs2.unlink(b"/a");
    let _ = l.register(word_cell(2, OP_UNLINK, fs2.unlink(b"/a").map(|o| o.links as u32)));

    let mut fs3 = seed();
    let _ = l.register(word_cell(3, OP_LOOKUP, fs3.lookup(b"/b").map(|e| e.links as u32)));

    let mut fs4 = seed();
    let _ = fs4.unlink(b"/b");
    let _ = l.register(count_cell(4, OP_WALK, fs4.walk(WALK_LIVE).map(|n| n as usize)));

    let fs5 = seed();
    let _ = l.register(word_cell(5, OP_WALK, fs5.walk(WALK_LIVE)));

    let fs6 = seed();
    let _ = l.register(word_cell(6, OP_WALK, fs6.walk(WALK_LINKS)));

    let mut fs7 = seed();
    let _ = fs7.unlink(b"/b");
    let _ = l.register(count_cell(7, OP_COMPACT, fs7.compact()));

    let mut fs8 = seed();
    let _ = l.register(word_cell(8, OP_UNLINK, fs8.unlink(b"a").map(|o| o.links as u32)));

    let mut fs9 = seed();
    let _ = l.register(word_cell(9, OP_UNLINK, fs9.unlink(b"/dir").map(|o| o.links as u32)));

    let mut fs10 = seed();
    let _ = l.register(word_cell(10, OP_UNLINK, fs10.unlink(b"/ro").map(|o| o.links as u32)));

    let mut fs11 = seed();
    let _ = l.register(word_cell(11, OP_UNLINK, fs11.unlink(b"/imm").map(|o| o.links as u32)));

    let mut fs12 = seed();
    let _ = l.register(word_cell(12, OP_UNLINK, fs12.unlink(b"/busy").map(|o| o.links as u32)));

    let mut fs13 = seed();
    let _ = l.register(word_cell(13, OP_UNLINK, fs13.unlink(b"/missing").map(|o| o.links as u32)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：链接递减与最终释放。
    let mut fs = seed();
    let a = fs.unlink(b"/a");
    let b = fs.unlink(b"/a");
    let gone = fs.lookup(b"/a").is_err();
    out[1] = a == Ok(Outcome { links: 1, freed: false })
        && b == Ok(Outcome { links: 0, freed: true })
        && gone
        && fs.walk(WALK_LIVE) == Ok(6);

    // 二路：压缩、顺序与统计往返。
    let mut fs2 = seed();
    let _ = fs2.unlink(b"/b");
    let before = fs2.walk(WALK_LINKS);
    let removed = fs2.compact();
    let again = fs2.compact();
    let a_kept = fs2.lookup(b"/a").map(|e| e.links) == Ok(2);
    let live = fs2.walk(WALK_LIVE);
    let bad_mode = fs2.walk(9) == Err(UNL_EMODE);
    out[2] = before == Ok(9)
        && removed == Ok(1)
        && again == Err(UNL_ESTATE)
        && a_kept
        && live == Ok(6)
        && bad_mode
        && fs2.compacted()
        && fs2.used() == 6;

    // 三路：六类拒绝与冻结重放。
    let mut fa = seed();
    let epath = fa.unlink(b"/a/") == Err(UNL_EPATH);
    let edot = fa.unlink(b"/a/../b") == Err(UNL_EPATH);
    let ename = fa.unlink(&[b'/'; NAMEMAX + 1]) == Err(UNL_ENAME);
    let edir = fa.unlink(b"/dir") == Err(UNL_EISDIR);
    let ero = fa.unlink(b"/ro") == Err(UNL_EROFS);
    let eperm = fa.unlink(b"/imm") == Err(UNL_EPERM);
    let ebusy = fa.unlink(b"/busy") == Err(UNL_EBUSY);
    let enoent = fa.unlink(b"/nope") == Err(UNL_ENOENT);
    let untouched = fa.lookup(b"/a").map(|e| e.links) == Ok(2);

    let mut led = build_ledger();
    let one_freeze = led.freeze();
    let rp = led.replay();
    let two_freeze = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = epath
        && edot
        && ename
        && edir
        && ero
        && eperm
        && ebusy
        && enoent
        && untouched
        && one_freeze.is_ok()
        && rp == one_freeze
        && two_freeze == Err(UNL_EBAL)
        && reg == Err(UNL_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2
        && led.replay() == Err(UNL_ESEAL);

    out
}

/// 十三项判据。
pub fn run_unx11537_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11537.unlink_single");

    s.add(
        "UNL01-链接递减",
        {
            let mut fs = seed();
            fs.unlink(b"/a") == Ok(Outcome { links: 1, freed: false })
                && fs.lookup(b"/a").map(|e| e.links) == Ok(1)
                && fs.walk(WALK_LIVE) == Ok(7)
        },
        "static",
    );

    s.add(
        "UNL02-末链接释放",
        {
            let mut fs = seed();
            let _ = fs.unlink(b"/a");
            let r = fs.unlink(b"/a");
            r == Ok(Outcome { links: 0, freed: true })
                && fs.lookup(b"/a") == Err(UNL_ENOENT)
                && fs.walk(WALK_LIVE) == Ok(6)
        },
        "static",
    );

    s.add(
        "UNL03-目录拒绝",
        {
            let mut fs = seed();
            fs.unlink(b"/dir") == Err(UNL_EISDIR)
                && fs.unlink(b"/") == Err(UNL_EISDIR)
                && fs.lookup(b"/dir").is_ok()
        },
        "static",
    );

    s.add(
        "UNL04-只读与不可变",
        {
            let mut fs = seed();
            fs.unlink(b"/ro") == Err(UNL_EROFS)
                && fs.unlink(b"/imm") == Err(UNL_EPERM)
                && fs.walk(WALK_LIVE) == Ok(7)
        },
        "static",
    );

    s.add(
        "UNL05-占用拒绝",
        {
            let mut fs = seed();
            fs.unlink(b"/busy") == Err(UNL_EBUSY)
                && fs.lookup(b"/busy").map(|e| e.links) == Ok(1)
        },
        "static",
    );

    s.add(
        "UNL06-路径语法",
        {
            let mut fs = seed();
            path_ok(b"/a") == Ok(())
                && path_ok(b"/a/b") == Ok(())
                && path_ok(b"a") == Err(UNL_EPATH)
                && path_ok(b"") == Err(UNL_EPATH)
                && path_ok(b"/a/") == Err(UNL_EPATH)
                && path_ok(b"//a") == Err(UNL_EPATH)
                && path_ok(b"/a//b") == Err(UNL_EPATH)
                && path_ok(b"/./a") == Err(UNL_EPATH)
                && path_ok(b"/a/../b") == Err(UNL_EPATH)
                && fs.unlink(b"/a/").is_err()
        },
        "static",
    );

    s.add(
        "UNL07-路径长度",
        {
            let mut fs = seed();
            let long = [b'x'; NAMEMAX + 1];
            let mut max = [b'a'; NAMEMAX];
            if let Some(cell) = max.get_mut(0) {
                *cell = b'/';
            }
            path_ok(&long) == Err(UNL_ENAME)
                && fs.unlink(&long) == Err(UNL_ENAME)
                && path_ok(&max).is_ok()
        },
        "static",
    );

    s.add(
        "UNL08-查找与计数",
        {
            let fs = seed();
            fs.lookup(b"/b").map(|e| (e.kind, e.links)) == Ok((KIND_FILE, 1))
                && fs.lookup(b"/").map(|e| e.kind) == Ok(KIND_DIR)
                && fs.lookup(b"/nope") == Err(UNL_ENOENT)
                && fs.walk(WALK_LIVE) == Ok(7)
                && fs.walk(WALK_LINKS) == Ok(10)
                && fs.used() == 7
        },
        "static",
    );

    s.add(
        "UNL09-压缩与状态",
        {
            let mut fs = seed();
            let _ = fs.unlink(b"/b");
            let _ = fs.unlink(b"/a");
            let _ = fs.unlink(b"/a");
            let first = fs.compact() == Ok(2);
            let twice = fs.compact() == Err(UNL_ESTATE);
            first && twice && fs.compacted() && fs.used() == 5 && fs.walk(WALK_LIVE) == Ok(5)
        },
        "static",
    );

    s.add(
        "UNL10-遍历模式闸门",
        {
            let fs = seed();
            fs.walk(0) == Err(UNL_EMODE)
                && fs.walk(3) == Err(UNL_EMODE)
                && fs.walk(WALK_LIVE).is_ok()
                && fs.walk(WALK_LINKS).is_ok()
        },
        "static",
    );

    s.add(
        "UNL11-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(6).map(|x| x.value) == Some(10)
                && l.sample(13).map(|x| x.code) == Some(UNL_ENOENT)
        },
        "static",
    );

    s.add(
        "UNL12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen()
                && a.is_ok()
                && b == Err(UNL_EBAL)
                && c == Err(UNL_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(UNL_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "UNL13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_gate() {
        assert_eq!(path_ok(b"/a"), Ok(()));
        assert_eq!(path_ok(b"/a/b"), Ok(()));
        assert_eq!(path_ok(b""), Err(UNL_EPATH));
        assert_eq!(path_ok(b"a"), Err(UNL_EPATH));
        assert_eq!(path_ok(b"/a/"), Err(UNL_EPATH));
        assert_eq!(path_ok(b"/a/../b"), Err(UNL_EPATH));
    }

    #[test]
    fn unlink_links() {
        let mut fs = seed();
        assert_eq!(fs.unlink(b"/a"), Ok(Outcome { links: 1, freed: false }));
        assert_eq!(fs.lookup(b"/a").map(|e| e.links), Ok(1));
        assert_eq!(fs.unlink(b"/a"), Ok(Outcome { links: 0, freed: true }));
        assert_eq!(fs.lookup(b"/a"), Err(UNL_ENOENT));
    }

    #[test]
    fn rejection_order() {
        let mut fs = seed();
        assert_eq!(fs.unlink(b"/dir"), Err(UNL_EISDIR));
        assert_eq!(fs.unlink(b"/ro"), Err(UNL_EROFS));
        assert_eq!(fs.unlink(b"/imm"), Err(UNL_EPERM));
        assert_eq!(fs.unlink(b"/busy"), Err(UNL_EBUSY));
        assert_eq!(fs.unlink(b"/nope"), Err(UNL_ENOENT));
    }

    #[test]
    fn walk_totals() {
        let fs = seed();
        assert_eq!(fs.walk(WALK_LIVE), Ok(7));
        assert_eq!(fs.walk(WALK_LINKS), Ok(10));
        assert_eq!(fs.walk(9), Err(UNL_EMODE));
    }

    #[test]
    fn compact_once() {
        let mut fs = seed();
        let _ = fs.unlink(b"/b");
        assert_eq!(fs.compact(), Ok(1));
        assert_eq!(fs.compact(), Err(UNL_ESTATE));
        assert_eq!(fs.used(), 6);
        assert_eq!(fs.lookup(b"/a").map(|e| e.links), Ok(2));
    }

    #[test]
    fn name_limit() {
        let long = [b'/'; NAMEMAX + 1];
        assert_eq!(path_ok(&long), Err(UNL_ENAME));
        let mut fs = seed();
        assert_eq!(fs.unlink(&long), Err(UNL_ENAME));
    }

    #[test]
    fn ledger_cycle() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert!(r != 0);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(UNL_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(UNL_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(UNL_ESEAL));
        assert_eq!(inject_four(), [true; 4]);
    }
}
