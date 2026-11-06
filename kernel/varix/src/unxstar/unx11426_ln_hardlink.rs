//! UNX-F11426 硬链接建立与引用计数（0x20a60 段）
//!
//! 硬链接不是“复制目录项指向的内容”，而是让两个目录项共享同一个 inode，
//! 并把 inode 的链接计数加一。因此 `link()` 的成功判据不是“目标可写”，
//! 而是一整套 inode 层面的前置条件：目标必须存在、不能是目录、必须与新的
//! 父目录同设备、名字不能已存在、链接计数不能触顶。任何一条不满足都必须
//! 在落盘前拒绝，且拒绝路径不产生目录项、不改计数。
//!
//! `link()` 对末分量为符号链接时有两种约定：POSIX `link()` 不改写链接，
//! 直接给链接节点加一个名字；`linkat(AT_SYMLINK_FOLLOW)` 则先展开链接，
//! 给指向的文件加名字。模型把两种约定都实现，并在账本里给出“同一路径、
//! 不同约定、不同 inode”的对照格。
//!
//! 目录项与 inode 的分离让不变量可以被显式重算：任一 inode 的链接计数必须
//! 等于引用它的目录项数加上预置外部引用数（用于模拟已存在的 65000 链接触顶
//! 场景）。审计逐 inode 重算，一旦目录项被误删或多加，票号立刻失衡。
//!
//! 账本登记十四格：六格成功（建链、两种约定、解链），八格失败（跨设备、
//! 重名、缺失、目录、触顶、中间非目录、重复解链、解链目录）。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20a60;

/// 编号非法。
pub const HL_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const HL_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const HL_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const HL_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const HL_ENOBS: u32 = SEG_BASE + 0x05;
/// 观测结果不符。
pub const HL_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const HL_EBAL: u32 = SEG_BASE + 0x07;
/// 空路径。
pub const HL_EEMPTY: u32 = SEG_BASE + 0x08;
/// 源不存在。
pub const HL_ENOENT: u32 = SEG_BASE + 0x09;
/// 名字已存在。
pub const HL_EEXIST: u32 = SEG_BASE + 0x0a;
/// 跨设备。
pub const HL_EXDEV: u32 = SEG_BASE + 0x0b;
/// 链接计数触顶。
pub const HL_EMLINK: u32 = SEG_BASE + 0x0c;
/// 不可建链/不可解链。
pub const HL_EPERM: u32 = SEG_BASE + 0x0d;
/// 中间分量不是目录。
pub const HL_ENOTDIR: u32 = SEG_BASE + 0x0e;
/// 票号不一致。
pub const HL_ESEAL: u32 = SEG_BASE + 0x0f;

/// 目录。
pub const K_DIR: u8 = 0;
/// 普通文件。
pub const K_REG: u8 = 1;
/// 符号链接。
pub const K_LNK: u8 = 2;

/// 建立硬链接。
pub const OP_LINK: u8 = 1;
/// 解链。
pub const OP_UNLINK: u8 = 2;

/// 名字容量。
pub const NAME_CAP: usize = 8;
/// 链接目标容量。
pub const TARGET_CAP: usize = 12;
/// inode 容量。
pub const INODES: usize = 8;
/// 目录项容量。
pub const ENTRIES: usize = 24;
/// 链接计数上限。
pub const MAX_LINK: u16 = 65000;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个 inode。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inode {
    pub ino: u16,
    pub dev: u8,
    pub kind: u8,
    pub nlink: u16,
    pub pins: u16,
    pub uid: u32,
    pub target: [u8; TARGET_CAP],
    pub tlen: u8,
}

impl Inode {
    /// 构造。
    pub const fn make(ino: u16, dev: u8, kind: u8, nlink: u16, pins: u16, uid: u32) -> Inode {
        Inode { ino, dev, kind, nlink, pins, uid, target: [0; TARGET_CAP], tlen: 0 }
    }

    /// 带链接目标构造。
    pub const fn symlink(ino: u16, dev: u8, uid: u32, target: &[u8]) -> Inode {
        let mut t = [0u8; TARGET_CAP];
        let mut i = 0;
        while i < target.len() && i < TARGET_CAP {
            t[i] = target[i];
            i += 1;
        }
        let n = if target.len() > TARGET_CAP { TARGET_CAP as u8 } else { target.len() as u8 };
        Inode { ino, dev, kind: K_LNK, nlink: 1, pins: 0, uid, target: t, tlen: n }
    }

    /// 目标切片。
    pub fn target(&self) -> &[u8] {
        let n = self.tlen as usize;
        if n > TARGET_CAP {
            return &[];
        }
        match self.target.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 一个目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub dir: u16,
    pub ino: u16,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
}

impl Entry {
    /// 构造。
    pub const fn make(dir: u16, name: &[u8], ino: u16) -> Entry {
        let mut n = [0u8; NAME_CAP];
        let mut i = 0;
        while i < name.len() && i < NAME_CAP {
            n[i] = name[i];
            i += 1;
        }
        let len = if name.len() > NAME_CAP { NAME_CAP as u8 } else { name.len() as u8 };
        Entry { dir, ino, name: n, nlen: len }
    }

    /// 名字切片。
    pub fn name(&self) -> &[u8] {
        let n = self.nlen as usize;
        if n > NAME_CAP {
            return &[];
        }
        match self.name.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 一条操作记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ev {
    pub op: u8,
    pub dir: u16,
    pub ino: u16,
    pub before: u16,
    pub after: u16,
}

/// inode 表加目录项表。
pub struct Fs {
    inodes: Vec<Inode>,
    entries: Vec<Entry>,
    events: Vec<Ev>,
    errors: u32,
    orphans: u32,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Fs {
    /// 以 inode 表与目录项表构造。
    pub fn new(inodes: Vec<Inode>, entries: Vec<Entry>) -> Fs {
        Fs { inodes, entries, events: Vec::new(), errors: 0, orphans: 0, frozen: false, root: 0, rejects: 0 }
    }

    /// 取 inode。
    pub fn inode(&self, ino: u16) -> Option<Inode> {
        if (ino as usize) >= self.inodes.len() {
            return None;
        }
        match self.inodes.get(ino as usize) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 取目录项。
    pub fn entry(&self, i: usize) -> Option<Entry> {
        match self.entries.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 目录项总数。
    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    /// 事件总数。
    pub fn events_len(&self) -> usize {
        self.events.len()
    }

    /// 最近事件。
    pub fn last_event(&self) -> Option<Ev> {
        match self.events.last() {
            Some(e) => Some(*e),
            None => None,
        }
    }

    /// 错误累计。
    pub fn errors(&self) -> u32 {
        self.errors
    }

    /// 归零 inode 数。
    pub fn orphans(&self) -> u32 {
        self.orphans
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 目录项索引。
    pub fn find(&self, dir: u16, name: &[u8]) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.dir == dir && e.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 某 inode 被多少个目录项引用。
    pub fn refs(&self, ino: u16) -> u16 {
        let mut n = 0u16;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.ino == ino {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 某 inode 所在目录（首个目录项）。
    pub fn dir_of(&self, ino: u16) -> Option<u16> {
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.ino == ino {
                    return Some(e.dir);
                }
            }
            i += 1;
        }
        None
    }

    /// 从某目录出发解析绝对路径。
    pub fn resolve(&self, start: u16, path: &[u8]) -> Result<u16, u32> {
        let mut cur = start;
        let mut i = 0;
        while i < path.len() {
            while i < path.len() && path[i] == b'/' {
                i += 1;
            }
            if i >= path.len() {
                break;
            }
            let s = i;
            while i < path.len() && path[i] != b'/' {
                i += 1;
            }
            let comp = match path.get(s..i) {
                Some(v) => v,
                None => return Err(HL_ENOENT),
            };
            if self.inode(cur).map(|n| n.kind) != Some(K_DIR) {
                return Err(HL_ENOTDIR);
            }
            match self.find(cur, comp) {
                Some(idx) => match self.entries.get(idx) {
                    Some(e) => cur = e.ino,
                    None => return Err(HL_ENOENT),
                },
                None => return Err(HL_ENOENT),
            }
        }
        Ok(cur)
    }

    /// 拆分出末名字并解析父目录。
    pub fn resolve_parent(&self, path: &[u8]) -> Result<(u16, [u8; NAME_CAP], u8), u32> {
        if path.is_empty() {
            return Err(HL_EEMPTY);
        }
        let mut cut = None;
        let mut i = 0;
        while i < path.len() {
            if path[i] == b'/' {
                cut = Some(i);
            }
            i += 1;
        }
        let (prefix, name) = match cut {
            None => (&[] as &[u8], path),
            Some(p) => {
                let a = match path.get(..p) {
                    Some(v) => v,
                    None => return Err(HL_EEMPTY),
                };
                let b = match path.get(p + 1..) {
                    Some(v) => v,
                    None => return Err(HL_EEMPTY),
                };
                (a, b)
            }
        };
        if name.is_empty() || name.len() > NAME_CAP {
            return Err(HL_EEMPTY);
        }
        let dir = self.resolve(0, prefix)?;
        if self.inode(dir).map(|n| n.kind) != Some(K_DIR) {
            return Err(HL_ENOTDIR);
        }
        let mut nb = [0u8; NAME_CAP];
        let mut j = 0;
        while j < name.len() {
            nb[j] = name[j];
            j += 1;
        }
        Ok((dir, nb, name.len() as u8))
    }

    /// 建立硬链接。
    pub fn link(&mut self, old: &[u8], new: &[u8], follow: bool) -> Result<u16, u32> {
        if old.is_empty() || new.is_empty() {
            self.errors += 1;
            return Err(HL_EEMPTY);
        }
        let (dir, name, nlen) = match self.resolve_parent(new) {
            Ok(v) => v,
            Err(e) => {
                self.errors += 1;
                return Err(e);
            }
        };
        let mut src = match self.resolve(0, old) {
            Ok(v) => v,
            Err(e) => {
                self.errors += 1;
                return Err(e);
            }
        };
        if follow {
            let n = match self.inode(src) {
                Some(v) => v,
                None => {
                    self.errors += 1;
                    return Err(HL_ENOENT);
                }
            };
            if n.kind == K_LNK {
                let t = n.target();
                if t.is_empty() {
                    self.errors += 1;
                    return Err(HL_ENOENT);
                }
                let base = self.dir_of(src).unwrap_or(0);
                src = match self.resolve(base, t) {
                    Ok(v) => v,
                    Err(e) => {
                        self.errors += 1;
                        return Err(e);
                    }
                };
            }
        }
        let sn = match self.inode(src) {
            Some(v) => v,
            None => {
                self.errors += 1;
                return Err(HL_ENOENT);
            }
        };
        if sn.kind == K_DIR {
            self.errors += 1;
            return Err(HL_EPERM);
        }
        let dn = match self.inode(dir) {
            Some(v) => v,
            None => {
                self.errors += 1;
                return Err(HL_ENOENT);
            }
        };
        if dn.dev != sn.dev {
            self.errors += 1;
            return Err(HL_EXDEV);
        }
        if self.find(dir, &name[..nlen as usize]).is_some() {
            self.errors += 1;
            return Err(HL_EEXIST);
        }
        if sn.nlink >= MAX_LINK {
            self.errors += 1;
            return Err(HL_EMLINK);
        }
        if self.entries.len() >= ENTRIES {
            self.errors += 1;
            return Err(HL_EBAL);
        }
        let before = sn.nlink;
        self.inodes[src as usize].nlink = sn.nlink + 1;
        self.entries.push(Entry { dir, ino: src, name, nlen });
        self.events.push(Ev { op: OP_LINK, dir, ino: src, before, after: before + 1 });
        Ok(src)
    }

    /// 解链。
    pub fn unlink(&mut self, path: &[u8]) -> Result<u16, u32> {
        let (dir, name, nlen) = match self.resolve_parent(path) {
            Ok(v) => v,
            Err(e) => {
                self.errors += 1;
                return Err(e);
            }
        };
        let idx = match self.find(dir, &name[..nlen as usize]) {
            Some(v) => v,
            None => {
                self.errors += 1;
                return Err(HL_ENOENT);
            }
        };
        let ino = match self.entries.get(idx) {
            Some(e) => e.ino,
            None => {
                self.errors += 1;
                return Err(HL_ENOENT);
            }
        };
        let n = match self.inode(ino) {
            Some(v) => v,
            None => {
                self.errors += 1;
                return Err(HL_ENOENT);
            }
        };
        if n.kind == K_DIR {
            self.errors += 1;
            return Err(HL_EPERM);
        }
        self.entries.remove(idx);
        self.inodes[ino as usize].nlink = n.nlink - 1;
        if n.nlink == 1 {
            self.orphans += 1;
        }
        self.events.push(Ev { op: OP_UNLINK, dir, ino, before: n.nlink, after: n.nlink - 1 });
        Ok(ino)
    }

    /// 逐 inode 重算计数，并与目录项表对照。
    pub fn audit(&self) -> Result<(), u32> {
        if self.inodes.is_empty() || self.entries.is_empty() {
            return Err(HL_EBAL);
        }
        let mut i = 0;
        while i < self.inodes.len() {
            let n = match self.inodes.get(i) {
                Some(v) => v,
                None => return Err(HL_ENOBS),
            };
            let refs = self.refs(n.ino);
            if refs as u32 + n.pins as u32 != n.nlink as u32 {
                return Err(HL_EBAL);
            }
            if n.nlink == 0 && refs != 0 {
                return Err(HL_EBAL);
            }
            i += 1;
        }
        let mut a = 0;
        while a < self.entries.len() {
            let x = match self.entries.get(a) {
                Some(v) => v,
                None => return Err(HL_ENOBS),
            };
            let mut b = a + 1;
            while b < self.entries.len() {
                if let Some(y) = self.entries.get(b) {
                    if x.dir == y.dir && x.name() == y.name() {
                        return Err(HL_EBAL);
                    }
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }

    /// 票面签名。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.inodes.len() {
            if let Some(n) = self.inodes.get(i) {
                h = fold(h, n.ino as u64);
                h = fold(h, n.dev as u64);
                h = fold(h, n.kind as u64);
                h = fold(h, n.nlink as u64);
                h = fold(h, n.pins as u64);
            }
            i += 1;
        }
        let mut j = 0;
        while j < self.entries.len() {
            if let Some(e) = self.entries.get(j) {
                h = fold(h, e.dir as u64);
                h = fold(h, e.ino as u64);
                let mut k = 0;
                while k < e.nlen as usize {
                    h ^= e.name[k] as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    k += 1;
                }
            }
            j += 1;
        }
        h = fold(h, self.errors as u64);
        h = fold(h, self.orphans as u64);
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(HL_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

fn fold(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 标准 inode 表。
pub fn build_inodes() -> Vec<Inode> {
    let mut v = Vec::new();
    v.push(Inode::make(0, 0, K_DIR, 1, 1, 0));
    v.push(Inode::make(1, 0, K_DIR, 1, 0, 0));
    v.push(Inode::make(2, 1, K_DIR, 1, 0, 0));
    v.push(Inode::make(3, 0, K_REG, 0, 0, 500));
    v.push(Inode::make(4, 0, K_REG, 1, 0, 500));
    v.push(Inode::make(5, 1, K_REG, 1, 0, 500));
    v.push(Inode::symlink(6, 0, 500, b"f"));
    v.push(Inode::make(7, 0, K_REG, MAX_LINK, MAX_LINK - 1, 500));
    v
}

/// 标准目录项表。
pub fn build_entries() -> Vec<Entry> {
    let mut v = Vec::new();
    v.push(Entry::make(0, b"a", 1));
    v.push(Entry::make(0, b"b", 2));
    v.push(Entry::make(1, b"f", 4));
    v.push(Entry::make(2, b"g", 5));
    v.push(Entry::make(1, b"l", 6));
    v.push(Entry::make(1, b"big", 7));
    v
}

/// 单格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub op: u8,
    pub follow: bool,
    pub old: [u8; 16],
    pub olen: u8,
    pub new: [u8; 16],
    pub nlen: u8,
    pub expect_ok: bool,
    pub expect: u16,
}

impl Sample {
    /// 源路径。
    pub fn old(&self) -> &[u8] {
        let n = self.olen as usize;
        if n > 16 {
            return &[];
        }
        match self.old.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 新路径。
    pub fn new(&self) -> &[u8] {
        let n = self.nlen as usize;
        if n > 16 {
            return &[];
        }
        match self.new.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 一格结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub value: u16,
}

/// 样本账本。
pub struct Ledger {
    fs: Fs,
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger { fs: Fs::new(build_inodes(), build_entries()), samples: Vec::new(), outcomes: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(
        &mut self,
        slot: u8,
        op: u8,
        old: &[u8],
        new: &[u8],
        follow: bool,
        ok: bool,
        expect: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(HL_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(HL_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(HL_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(HL_EORDER);
        }
        if old.len() > 16 || new.len() > 16 || old.is_empty() {
            self.rejects += 1;
            return Err(HL_EEMPTY);
        }
        let mut s = Sample {
            op,
            follow,
            old: [0; 16],
            olen: old.len() as u8,
            new: [0; 16],
            nlen: new.len() as u8,
            expect_ok: ok,
            expect,
        };
        let mut i = 0;
        while i < old.len() {
            s.old[i] = old[i];
            i += 1;
        }
        i = 0;
        while i < new.len() {
            s.new[i] = new[i];
            i += 1;
        }
        let before = self.fs.events_len();
        let r = if op == OP_LINK {
            self.fs.link(old, new, follow)
        } else {
            self.fs.unlink(old)
        };
        let out = match r {
            Ok(v) => {
                if !ok || v != expect {
                    return Err(HL_EMISMATCH);
                }
                Outcome { ok: true, value: v }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != expect {
                    return Err(HL_EMISMATCH);
                }
                Outcome { ok: false, value: (e & 0xffff) as u16 }
            }
        };
        if self.fs.events_len() != before + usize::from(out.ok) {
            return Err(HL_EBAL);
        }
        self.samples.push(s);
        self.outcomes.push(out);
        Ok(())
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        match self.samples.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 取结果。
    pub fn outcome(&self, i: usize) -> Option<Outcome> {
        match self.outcomes.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                if o.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.outcomes.len() - self.ok_count()
    }

    /// 账本自洽。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(HL_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(HL_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => v,
                None => return Err(HL_ENOBS),
            };
            if s.expect_ok != o.ok || s.expect != o.value {
                return Err(HL_EMISMATCH);
            }
            i += 1;
        }
        self.fs.audit()
    }

    /// 访问文件系统。
    pub fn fs(&self) -> &Fs {
        &self.fs
    }

    /// 可变访问文件系统。
    pub fn fs_mut(&mut self) -> &mut Fs {
        &mut self.fs
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(HL_EFROZEN);
        }
        self.root = self.fs.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建账本比对票号。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.fs.seal() == self.fs.seal() {
            Ok(fresh.fs.seal())
        } else {
            Err(HL_ESEAL)
        }
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 构建十四样本账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let noent = (HL_ENOENT & 0xffff) as u16;
    let eexist = (HL_EEXIST & 0xffff) as u16;
    let exdev = (HL_EXDEV & 0xffff) as u16;
    let emlink = (HL_EMLINK & 0xffff) as u16;
    let eperm = (HL_EPERM & 0xffff) as u16;
    let enotdir = (HL_ENOTDIR & 0xffff) as u16;
    let _ = l.register(0, OP_LINK, b"/a/f", b"/a/f2", false, true, 4);
    let _ = l.register(1, OP_LINK, b"/a/f", b"/b/h", false, false, exdev);
    let _ = l.register(2, OP_LINK, b"/a/f", b"/a/f2", false, false, eexist);
    let _ = l.register(3, OP_LINK, b"/a/missing", b"/a/x", false, false, noent);
    let _ = l.register(4, OP_LINK, b"/a", b"/a/alink", false, false, eperm);
    let _ = l.register(5, OP_LINK, b"/a/big", b"/a/big2", false, false, emlink);
    let _ = l.register(6, OP_LINK, b"/a/l", b"/a/ll", false, true, 6);
    let _ = l.register(7, OP_LINK, b"/a/l", b"/a/ll2", true, true, 4);
    let _ = l.register(8, OP_LINK, b"/a/f", b"/a/f/q", false, false, enotdir);
    let _ = l.register(9, OP_UNLINK, b"/a/f2", b"", false, true, 4);
    let _ = l.register(10, OP_UNLINK, b"/a/f2", b"", false, false, noent);
    let _ = l.register(11, OP_UNLINK, b"/a", b"", false, false, eperm);
    let _ = l.register(12, OP_LINK, b"/a/l", b"/a/ll3", true, true, 4);
    let _ = l.register(13, OP_UNLINK, b"/a/ll", b"", false, true, 6);
    l
}

/// 四路注入：共享 inode、两种约定、引用归零、拒绝矩阵。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES
        && l.ok_count() == 6
        && l.err_count() == 8
        && l.audit().is_ok()
        && l.fs().refs(4) == 3
        && l.fs().inode(4).map(|n| n.nlink) == Some(3);
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut f = Fs::new(build_inodes(), build_entries());
    let a = f.link(b"/a/f", b"/a/ff", false) == Ok(4);
    let b = f.inode(4).map(|n| n.nlink) == Some(2);
    let c = f.unlink(b"/a/f") == Ok(4);
    let d = f.inode(4).map(|n| n.nlink) == Some(1);
    let e = f.unlink(b"/a/ff") == Ok(4);
    out[1] = a && b && c && d && e && f.inode(4).map(|n| n.nlink) == Some(0) && f.orphans() == 1;

    let mut g = Fs::new(build_inodes(), build_entries());
    let x = g.link(b"/a/l", b"/a/ll", false) == Ok(6);
    let y = g.link(b"/a/l", b"/a/ll2", true) == Ok(4);
    out[2] = x && y && g.inode(6).map(|n| n.nlink) == Some(2) && g.inode(4).map(|n| n.nlink) == Some(2);

    let mut h = Fs::new(build_inodes(), build_entries());
    let r1 = h.link(b"/a/f", b"/b/h", false);
    let r2 = h.link(b"/a/f", b"/a/f2", false);
    let _ = h.link(b"/a/f", b"/a/f2", false);
    let r3 = h.link(b"/a/f", b"/a/f2", false);
    let r4 = h.link(b"/a/big", b"/a/big2", false);
    let r5 = h.link(b"/a", b"/a/alink", false);
    out[3] = r1 == Err(HL_EXDEV) && r2 == Ok(4) && r3 == Err(HL_EEXIST) && r4 == Err(HL_EMLINK) && r5 == Err(HL_EPERM);

    out
}

/// 十三项判据。
pub fn run_unx11426_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11426.hardlink");
    let l = build_ledger();

    s.add(
        "HL1-十四格账本齐备",
        l.len() == SAMPLES
            && !l.is_empty()
            && l.sample(0).map(|x| x.old() == b"/a/f" as &[u8]) == Some(true)
            && l.fs().inode(7).map(|n| n.nlink) == Some(MAX_LINK),
        "static",
    );

    s.add_dyn(
        "HL2-六应八拒分布正确",
        l.ok_count() == 6 && l.err_count() == 8 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "HL3-共享 inode 计数正确",
        l.fs().refs(4) == 3
            && l.fs().inode(4).map(|n| n.nlink) == Some(3)
            && l.fs().refs(6) == 1
            && l.fs().inode(6).map(|n| n.nlink) == Some(1),
        "static",
    );

    s.add(
        "HL4-重名拒绝",
        l.outcome(2).map(|o| o.value) == Some((HL_EEXIST & 0xffff) as u16)
            && l.fs().refs(4) == 3,
        "static",
    );

    s.add(
        "HL5-跨设备拒绝",
        l.outcome(1).map(|o| o.value) == Some((HL_EXDEV & 0xffff) as u16)
            && l.fs().find(2, b"h").is_none(),
        "static",
    );

    s.add(
        "HL6-链接计数触顶",
        l.outcome(5).map(|o| o.value) == Some((HL_EMLINK & 0xffff) as u16)
            && l.fs().inode(7).map(|n| n.nlink) == Some(MAX_LINK)
            && l.fs().find(1, b"big2").is_none(),
        "static",
    );

    s.add(
        "HL7-目录不可建链",
        l.outcome(4).map(|o| o.value) == Some((HL_EPERM & 0xffff) as u16)
            && l.outcome(11).map(|o| o.value) == Some((HL_EPERM & 0xffff) as u16),
        "static",
    );

    s.add(
        "HL8-源缺失拒绝",
        l.outcome(3).map(|o| o.value) == Some((HL_ENOENT & 0xffff) as u16)
            && l.outcome(10).map(|o| o.value) == Some((HL_ENOENT & 0xffff) as u16),
        "static",
    );

    s.add(
        "HL9-新路径中间须为目录",
        l.outcome(8).map(|o| o.value) == Some((HL_ENOTDIR & 0xffff) as u16)
            && Fs::new(build_inodes(), build_entries()).resolve(0, b"/a/f").is_ok(),
        "static",
    );

    s.add(
        "HL10-两种末链约定分野",
        l.outcome(6).map(|o| o.value) == Some(6)
            && l.outcome(7).map(|o| o.value) == Some(4)
            && l.outcome(12).map(|o| o.value) == Some(4),
        "static",
    );

    s.add(
        "HL11-解链递减与归零",
        l.outcome(9).map(|o| o.value) == Some(4)
            && l.outcome(13).map(|o| o.value) == Some(6)
            && l.fs().inode(6).map(|n| n.nlink) == Some(1),
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(HL_EFROZEN);
    let rej = f.rejects();
    s.add_dyn(
        "HL12-冻结重放幂等",
        root != 0 && rep == Ok(root) && again && rej == 1 && f.root() == root,
        &format!("root={:016x}", root),
    );

    let inj = inject_four();
    s.add_dyn("HL13-四路注入全绿", inj == [true; 4], &format!("inject={:?}", inj));

    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!(
        "ok={} err={} refs_f={} orphans={}",
        l.ok_count(),
        l.err_count(),
        l.fs().refs(4),
        l.fs().orphans()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Fs {
        Fs::new(build_inodes(), build_entries())
    }

    #[test]
    fn sharing_increments_and_decrements() {
        let mut f = fresh();
        assert_eq!(f.link(b"/a/f", b"/a/ff", false), Ok(4));
        assert_eq!(f.inode(4).map(|n| n.nlink), Some(2));
        assert_eq!(f.unlink(b"/a/ff"), Ok(4));
        assert_eq!(f.inode(4).map(|n| n.nlink), Some(1));
        assert_eq!(f.orphans(), 0);
    }

    #[test]
    fn cross_device_rejected() {
        let mut f = fresh();
        assert_eq!(f.link(b"/a/f", b"/b/h", false), Err(HL_EXDEV));
        assert!(f.find(2, b"h").is_none());
    }

    #[test]
    fn max_link_rejected() {
        let mut f = fresh();
        assert_eq!(f.link(b"/a/big", b"/a/big2", false), Err(HL_EMLINK));
        assert_eq!(f.inode(7).map(|n| n.nlink), Some(MAX_LINK));
    }

    #[test]
    fn symlink_two_conventions() {
        let mut f = fresh();
        assert_eq!(f.link(b"/a/l", b"/a/ll", false), Ok(6));
        assert_eq!(f.link(b"/a/l", b"/a/ll2", true), Ok(4));
        assert_eq!(f.refs(6), 2);
        assert_eq!(f.refs(4), 2);
    }

    #[test]
    fn dir_and_missing_refused() {
        let mut f = fresh();
        assert_eq!(f.link(b"/a", b"/a/alink", false), Err(HL_EPERM));
        assert_eq!(f.link(b"/a/none", b"/a/x", false), Err(HL_ENOENT));
        assert_eq!(f.unlink(b"/a"), Err(HL_EPERM));
        assert_eq!(f.errors(), 3);
    }

    #[test]
    fn ledger_distribution_and_audit() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 6);
        assert_eq!(l.err_count(), 8);
        assert!(l.audit().is_ok());
        assert!(summary(&l).starts_with("ok=6"));
    }

    #[test]
    fn refusal_matrix() {
        let mut l = Ledger::new();
        assert_eq!(l.register(0, OP_LINK, b"/a/f", b"/a/f2", false, true, 4), Ok(()));
        assert_eq!(l.register(0, OP_LINK, b"/a/f", b"/a/f2", false, true, 4), Err(HL_EDUP));
        assert_eq!(l.register(3, OP_LINK, b"/a/f", b"/a/f2", false, true, 4), Err(HL_EORDER));
        assert_eq!(l.register(14, OP_LINK, b"/a/f", b"/a/f2", false, true, 4), Err(HL_EID));
        assert_eq!(l.register(1, OP_LINK, b"/a/f", b"/a/f2", false, true, 9), Err(HL_EMISMATCH));
        let _ = l.freeze();
        assert_eq!(l.register(1, OP_LINK, b"/a/f", b"/a/f2", false, true, 4), Err(HL_EFROZEN));
    }

    #[test]
    fn checks_green_and_injection() {
        let s = run_unx11426_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
