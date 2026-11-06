//! UNX-F11461 mv 重命名语义（0x20ca0 段）
//!
//! `rename()` 是唯一同时改动两个目录项的元数据操作：它把源名字从旧目录摘除，
//! 再挂到新目录，必要时先顶掉已存在的目标。与 `link()+unlink()` 的组合不同，
//! 替换必须原子——不允许出现“目标已删、源还在旧处”的中间态。本模型把一次
//! 调用记成 `Act`：被移动的 inode、被顶掉的目标 inode（无则 0）、源类型。
//!
//! 拒绝矩阵按解析顺序分层。第一层是路径：旧路径必须存在，新旧父目录都必须
//! 是目录；第二层是标志：只接受 `RENAME_NOREPLACE(1)` 与 `RENAME_EXCHANGE(2)`，
//! 两者同时置位非法，越界位一律 `EINVAL`；第三层是类型：目录不能覆盖文件
//! （`ENOTDIR`），文件不能覆盖目录（`EISDIR`），目标目录必须为空
//! （`ENOTEMPTY`）；第四层是设备与环：跨设备 `EXDEV`，目录搬进自身子树
//! `EINVAL`。顺序不可交换，否则同一组参数会得到不同 errno。
//!
//! 三条不变量：目录内名字唯一；inode 的链接计数等于引用它的目录项数；目录
//! 父链无环。前两条由 `audit()` 全量重算，第三条在 rename 写入前判定。
//! EXCHANGE 只交换两个目录项的 inode 指针，名字与位置不动，交换两次必须
//! 精确回到原态——`inject_four()` 第二路就是这条往返。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ca0;
/// 格号非法。
pub const MV_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const MV_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const MV_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const MV_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const MV_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果与预期不符。
pub const MV_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const MV_EBAL: u32 = SEG_BASE + 0x07;
/// 票号不一致。
pub const MV_ESEAL: u32 = SEG_BASE + 0x08;
/// 源或父目录缺失。
pub const MV_ENOENT: u32 = SEG_BASE + 0x09;
/// 目标已存在。
pub const MV_EEXIST: u32 = SEG_BASE + 0x0a;
/// 跨设备。
pub const MV_EXDEV: u32 = SEG_BASE + 0x0b;
/// 目录覆盖文件。
pub const MV_EISDIR: u32 = SEG_BASE + 0x0c;
/// 文件覆盖目录。
pub const MV_ENOTDIR: u32 = SEG_BASE + 0x0d;
/// 标志或环非法。
pub const MV_EINVAL: u32 = SEG_BASE + 0x0e;
/// 目标目录非空。
pub const MV_ENOTEMPTY: u32 = SEG_BASE + 0x0f;

/// 目录。
pub const K_DIR: u8 = 0;
/// 普通文件。
pub const K_REG: u8 = 1;
/// 普通替换。
pub const F_NONE: u8 = 0;
/// 目标存在即拒绝。
pub const F_NOREPLACE: u8 = 1;
/// 原子交换。
pub const F_EXCHANGE: u8 = 2;
/// 名字容量。
pub const NAME_CAP: usize = 8;
/// 路径容量。
pub const PATH_CAP: usize = 20;
/// inode 槽位。
pub const INODES: usize = 10;
/// 样本格数。
pub const SAMPLES: usize = 15;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

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

fn fill<const N: usize>(src: &[u8]) -> ([u8; N], u8) {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < src.len() && i < N {
        out[i] = src[i];
        i += 1;
    }
    let n = if src.len() > N { N as u8 } else { src.len() as u8 };
    (out, n)
}

fn slice<const N: usize>(buf: &[u8; N], len: u8) -> &[u8] {
    let n = len as usize;
    if n > N {
        return &[];
    }
    match buf.get(..n) {
        Some(v) => v,
        None => &[],
    }
}

/// inode。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ino {
    pub ino: u16,
    pub dev: u8,
    pub kind: u8,
    pub nlink: u16,
    pub size: u32,
}

impl Ino {
    /// 构造。
    pub const fn make(ino: u16, dev: u8, kind: u8, size: u32) -> Ino {
        Ino { ino, dev, kind, nlink: 0, size }
    }
}

/// 目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    pub dir: u16,
    pub ino: u16,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
}

impl Ent {
    /// 构造。
    pub fn make(dir: u16, name: &[u8], ino: u16) -> Ent {
        let (b, n) = fill::<NAME_CAP>(name);
        Ent { dir, ino, name: b, nlen: n }
    }
    /// 名字。
    pub fn name(&self) -> &[u8] {
        slice(&self.name, self.nlen)
    }
}

/// 一次调用的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Act {
    pub ino: u16,
    pub target: u16,
    pub kind: u8,
}

/// 树模型。
pub struct Tree {
    inos: Vec<Ino>,
    ents: Vec<Ent>,
    acts: Vec<Act>,
    errors: u32,
}

impl Tree {
    /// 构造并重算计数。
    pub fn new(inos: Vec<Ino>, ents: Vec<Ent>) -> Tree {
        let mut t = Tree { inos, ents, acts: Vec::new(), errors: 0 };
        t.recount();
        t
    }
    /// inode。
    pub fn ino(&self, n: u16) -> Option<Ino> {
        if (n as usize) >= self.inos.len() {
            return None;
        }
        self.inos.get(n as usize).copied()
    }
    /// 目录项。
    pub fn ent(&self, i: usize) -> Option<Ent> {
        self.ents.get(i).copied()
    }
    /// 目录项数。
    pub fn ents_len(&self) -> usize {
        self.ents.len()
    }
    /// 错误数。
    pub fn errors(&self) -> u32 {
        self.errors
    }
    /// 引用数。
    pub fn refs(&self, ino: u16) -> u16 {
        let mut n = 0u16;
        let mut i = 0;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.ino == ino {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
    /// 重算链接计数。
    pub fn recount(&mut self) {
        let mut i = 0;
        while i < self.inos.len() {
            let ino = match self.inos.get(i) {
                Some(v) => v.ino,
                None => {
                    i += 1;
                    continue;
                }
            };
            let r = self.refs(ino);
            if let Some(v) = self.inos.get_mut(i) {
                v.nlink = r;
            }
            i += 1;
        }
    }
    /// 查找。
    pub fn find(&self, dir: u16, name: &[u8]) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let mut i = 0;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.dir == dir && e.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
    /// 解析绝对路径。
    pub fn resolve(&self, path: &[u8]) -> Result<u16, u32> {
        let mut cur = 0u16;
        let mut i = 0;
        let mut guard = 0;
        while i < path.len() && guard < PATH_CAP {
            guard += 1;
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
                None => return Err(MV_ENOENT),
            };
            if comp == b"." {
                continue;
            }
            if comp == b".." {
                cur = self.parent_of(cur).unwrap_or(0);
                continue;
            }
            match self.ino(cur) {
                Some(n) => {
                    if n.kind != K_DIR {
                        return Err(MV_ENOTDIR);
                    }
                }
                None => return Err(MV_ENOENT),
            }
            match self.find(cur, comp) {
                Some(idx) => match self.ents.get(idx) {
                    Some(e) => cur = e.ino,
                    None => return Err(MV_ENOENT),
                },
                None => return Err(MV_ENOENT),
            }
        }
        Ok(cur)
    }
    /// 拆分父目录与末名字。
    pub fn split(&self, path: &[u8]) -> Result<(u16, [u8; NAME_CAP], u8), u32> {
        if path.is_empty() || path.len() > PATH_CAP {
            return Err(MV_EINVAL);
        }
        let mut cut = 0usize;
        let mut seen = false;
        let mut i = 0;
        while i < path.len() {
            if path[i] == b'/' {
                cut = i;
                seen = true;
            }
            i += 1;
        }
        let prefix = if seen {
            match path.get(..cut) {
                Some(v) => v,
                None => return Err(MV_EINVAL),
            }
        } else {
            &[] as &[u8]
        };
        let name = match path.get(cut + usize::from(seen)..) {
            Some(v) => v,
            None => return Err(MV_EINVAL),
        };
        if name.is_empty() || name.len() > NAME_CAP {
            return Err(MV_EINVAL);
        }
        let dir = if prefix.is_empty() { 0 } else { self.resolve(prefix)? };
        match self.ino(dir) {
            Some(n) => {
                if n.kind != K_DIR {
                    return Err(MV_ENOTDIR);
                }
            }
            None => return Err(MV_ENOENT),
        }
        let (b, n) = fill::<NAME_CAP>(name);
        Ok((dir, b, n))
    }
    /// 父目录。
    pub fn parent_of(&self, dir: u16) -> Option<u16> {
        if dir == 0 {
            return Some(0);
        }
        let mut i = 0;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.ino == dir {
                    return Some(e.dir);
                }
            }
            i += 1;
        }
        None
    }
    /// 目录为空。
    pub fn empty_dir(&self, dir: u16) -> bool {
        let mut i = 0;
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                if e.dir == dir {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
    /// 祖先判据。
    pub fn is_ancestor(&self, mut node: u16, anc: u16) -> bool {
        let mut guard = 0;
        loop {
            if node == anc {
                return true;
            }
            if node == 0 || guard > INODES {
                return false;
            }
            guard += 1;
            match self.parent_of(node) {
                Some(p) => {
                    if p == node {
                        return false;
                    }
                    node = p;
                }
                None => return false,
            }
        }
    }
    fn drop_ent(&mut self, idx: usize) {
        if idx < self.ents.len() {
            self.ents.remove(idx);
        }
    }
    /// 执行 rename。
    pub fn rename(&mut self, old: &[u8], new: &[u8], flags: u8) -> Result<Act, u32> {
        if flags & !(F_NOREPLACE | F_EXCHANGE) != 0 || flags == (F_NOREPLACE | F_EXCHANGE) {
            self.errors += 1;
            return Err(MV_EINVAL);
        }
        let (od, on, onl) = match self.split(old) {
            Ok(v) => v,
            Err(e) => {
                self.errors += 1;
                return Err(e);
            }
        };
        let (nd, nn, nnl) = match self.split(new) {
            Ok(v) => v,
            Err(e) => {
                self.errors += 1;
                return Err(e);
            }
        };
        let oidx = match self.find(od, slice(&on, onl)) {
            Some(v) => v,
            None => {
                self.errors += 1;
                return Err(MV_ENOENT);
            }
        };
        let oino = match self.ents.get(oidx) {
            Some(e) => e.ino,
            None => {
                self.errors += 1;
                return Err(MV_ENOENT);
            }
        };
        let src = match self.ino(oino) {
            Some(v) => v,
            None => {
                self.errors += 1;
                return Err(MV_ENOENT);
            }
        };
        let dst = match self.ino(nd) {
            Some(v) => v,
            None => {
                self.errors += 1;
                return Err(MV_ENOENT);
            }
        };
        if src.dev != dst.dev {
            self.errors += 1;
            return Err(MV_EXDEV);
        }
        let tidx = self.find(nd, slice(&nn, nnl));
        let tinfo = match tidx {
            Some(i) => match self.ents.get(i) {
                Some(e) => self.ino(e.ino),
                None => None,
            },
            None => None,
        };
        if flags == F_NOREPLACE && tidx.is_some() {
            self.errors += 1;
            return Err(MV_EEXIST);
        }
        if flags == F_EXCHANGE {
            let ti = match tidx {
                Some(v) => v,
                None => {
                    self.errors += 1;
                    return Err(MV_ENOENT);
                }
            };
            let tino = match self.ents.get(ti) {
                Some(e) => e.ino,
                None => {
                    self.errors += 1;
                    return Err(MV_ENOENT);
                }
            };
            let tn = match self.ino(tino) {
                Some(v) => v,
                None => {
                    self.errors += 1;
                    return Err(MV_ENOENT);
                }
            };
            if tn.dev != src.dev {
                self.errors += 1;
                return Err(MV_EXDEV);
            }
            if tino != oino {
                if let Some(a) = self.ents.get_mut(oidx) {
                    a.ino = tino;
                }
                if let Some(b) = self.ents.get_mut(ti) {
                    b.ino = oino;
                }
            }
            let act = Act { ino: oino, target: tino, kind: src.kind };
            self.acts.push(act);
            return Ok(act);
        }
        if let Some(t) = tinfo {
            if t.ino == oino {
                let act = Act { ino: oino, target: t.ino, kind: src.kind };
                self.acts.push(act);
                return Ok(act);
            }
            if src.kind == K_DIR && t.kind != K_DIR {
                self.errors += 1;
                return Err(MV_ENOTDIR);
            }
            if src.kind != K_DIR && t.kind == K_DIR {
                self.errors += 1;
                return Err(MV_EISDIR);
            }
            if t.kind == K_DIR && !self.empty_dir(t.ino) {
                self.errors += 1;
                return Err(MV_ENOTEMPTY);
            }
        }
        if src.kind == K_DIR && self.is_ancestor(nd, oino) {
            self.errors += 1;
            return Err(MV_EINVAL);
        }
        let target = match tinfo {
            Some(t) => t.ino,
            None => 0,
        };
        match tidx {
            Some(ti) => {
                if let Some(e) = self.ents.get_mut(ti) {
                    e.ino = oino;
                }
                self.drop_ent(oidx);
            }
            None => {
                if let Some(e) = self.ents.get_mut(oidx) {
                    e.dir = nd;
                    e.name = nn;
                    e.nlen = nnl;
                }
            }
        }
        self.recount();
        let act = Act { ino: oino, target, kind: src.kind };
        self.acts.push(act);
        Ok(act)
    }
    /// 审计不变量。
    pub fn audit(&self) -> Result<(), u32> {
        if self.inos.is_empty() || self.ents.is_empty() {
            return Err(MV_EBAL);
        }
        let mut i = 0;
        while i < self.inos.len() {
            let n = match self.inos.get(i) {
                Some(v) => v,
                None => return Err(MV_ENOBS),
            };
            if n.nlink != self.refs(n.ino) {
                return Err(MV_EBAL);
            }
            i += 1;
        }
        let mut a = 0;
        while a < self.ents.len() {
            let x = match self.ents.get(a) {
                Some(v) => v,
                None => return Err(MV_ENOBS),
            };
            if self.ino(x.ino).is_none() {
                return Err(MV_EBAL);
            }
            let mut b = a + 1;
            while b < self.ents.len() {
                if let Some(y) = self.ents.get(b) {
                    if x.dir == y.dir && x.name() == y.name() {
                        return Err(MV_EBAL);
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
        while i < self.ents.len() {
            if let Some(e) = self.ents.get(i) {
                h = fold(h, e.dir as u64);
                h = fold(h, e.ino as u64);
                let mut k = 0;
                while k < e.nlen as usize && k < NAME_CAP {
                    h ^= e.name[k] as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    k += 1;
                }
            }
            i += 1;
        }
        let mut j = 0;
        while j < self.inos.len() {
            if let Some(n) = self.inos.get(j) {
                h = fold(h, n.nlink as u64);
            }
            j += 1;
        }
        fold(h, self.errors as u64)
    }
}

/// 单格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub old: [u8; PATH_CAP],
    pub olen: u8,
    pub new: [u8; PATH_CAP],
    pub nlen: u8,
    pub flags: u8,
    pub expect_ok: bool,
    pub expect: u16,
    pub expect_target: u16,
}

impl Sample {
    /// 旧路径。
    pub fn old(&self) -> &[u8] {
        slice(&self.old, self.olen)
    }
    /// 新路径。
    pub fn new(&self) -> &[u8] {
        slice(&self.new, self.nlen)
    }
}

/// 一格结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub value: u16,
    pub target: u16,
}

/// 账本。
pub struct Ledger {
    tree: Tree,
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger {
            tree: Tree::new(build_inodes(), build_entries()),
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }
    /// 登记并执行一格。
    pub fn register(
        &mut self,
        slot: u8,
        old: &[u8],
        new: &[u8],
        flags: u8,
        ok: bool,
        expect: u16,
        expect_target: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(MV_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(MV_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(MV_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(MV_EORDER);
        }
        if old.is_empty() || new.is_empty() || old.len() > PATH_CAP || new.len() > PATH_CAP {
            self.rejects += 1;
            return Err(MV_EINVAL);
        }
        let (ob, ol) = fill::<PATH_CAP>(old);
        let (nb, nl) = fill::<PATH_CAP>(new);
        let s = Sample {
            old: ob,
            olen: ol,
            new: nb,
            nlen: nl,
            flags,
            expect_ok: ok,
            expect,
            expect_target,
        };
        let out = match self.tree.rename(old, new, flags) {
            Ok(a) => {
                if !ok || a.ino != expect || a.target != expect_target {
                    return Err(MV_EMISMATCH);
                }
                Outcome { ok: true, value: a.ino, target: a.target }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != expect {
                    return Err(MV_EMISMATCH);
                }
                Outcome { ok: false, value: (e & 0xffff) as u16, target: 0 }
            }
        };
        self.samples.push(s);
        self.outcomes.push(out);
        Ok(())
    }
    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    /// 空判据。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
    /// 样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        self.samples.get(i).copied()
    }
    /// 结果。
    pub fn outcome(&self, i: usize) -> Option<Outcome> {
        self.outcomes.get(i).copied()
    }
    /// 成功数。
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
    /// 失败数。
    pub fn err_count(&self) -> usize {
        self.outcomes.len().saturating_sub(self.ok_count())
    }
    /// 树。
    pub fn tree(&self) -> &Tree {
        &self.tree
    }
    /// 账本自洽。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(MV_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(MV_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => v,
                None => return Err(MV_ENOBS),
            };
            if s.expect_ok != o.ok || s.expect != o.value || s.expect_target != o.target {
                return Err(MV_EMISMATCH);
            }
            i += 1;
        }
        self.tree.audit()
    }
    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(MV_EFROZEN);
        }
        self.root = self.tree.seal();
        self.frozen = true;
        Ok(self.root)
    }
    /// 根。
    pub fn root(&self) -> u64 {
        self.root
    }
    /// 拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    /// 重放比对。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let sig = fresh.tree.seal();
        if sig != self.tree.seal()
            || fresh.ok_count() != self.ok_count()
            || fresh.err_count() != self.err_count()
        {
            return Err(MV_ESEAL);
        }
        Ok(sig)
    }
}

/// inode 表。
pub fn build_inodes() -> Vec<Ino> {
    let mut v = Vec::new();
    v.push(Ino::make(0, 0, K_DIR, 0));
    v.push(Ino::make(1, 0, K_DIR, 0));
    v.push(Ino::make(2, 1, K_DIR, 0));
    v.push(Ino::make(3, 0, K_DIR, 0));
    v.push(Ino::make(4, 0, K_REG, 512));
    v.push(Ino::make(5, 0, K_REG, 256));
    v.push(Ino::make(6, 1, K_REG, 128));
    v.push(Ino::make(7, 0, K_DIR, 0));
    v.push(Ino::make(8, 0, K_DIR, 0));
    v.push(Ino::make(9, 0, K_REG, 64));
    v
}

/// 目录项表。
pub fn build_entries() -> Vec<Ent> {
    let mut v = Vec::new();
    v.push(Ent::make(0, b"a", 1));
    v.push(Ent::make(0, b"b", 2));
    v.push(Ent::make(1, b"f", 4));
    v.push(Ent::make(1, b"t", 5));
    v.push(Ent::make(2, b"h", 6));
    v.push(Ent::make(1, b"d1", 7));
    v.push(Ent::make(1, b"d2", 8));
    v.push(Ent::make(8, b"leaf", 9));
    v.push(Ent::make(1, b"sub", 3));
    v
}

/// 十五格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let noent = (MV_ENOENT & 0xffff) as u16;
    let eexist = (MV_EEXIST & 0xffff) as u16;
    let exdev = (MV_EXDEV & 0xffff) as u16;
    let eisdir = (MV_EISDIR & 0xffff) as u16;
    let enotdir = (MV_ENOTDIR & 0xffff) as u16;
    let einval = (MV_EINVAL & 0xffff) as u16;
    let enotempty = (MV_ENOTEMPTY & 0xffff) as u16;
    let _ = l.register(0, b"/a/f", b"/a/g", F_NONE, true, 4, 0);
    let _ = l.register(1, b"/a/g", b"/b/h", F_NONE, false, exdev, 0);
    let _ = l.register(2, b"/a/g", b"/a/d1", F_NONE, false, eisdir, 0);
    let _ = l.register(3, b"/a/d1", b"/a/g", F_NONE, false, enotdir, 0);
    let _ = l.register(4, b"/a/none", b"/a/x", F_NONE, false, noent, 0);
    let _ = l.register(5, b"/a/g", b"/a/none/x", F_NONE, false, noent, 0);
    let _ = l.register(6, b"/a/d2", b"/a/d2/zz", F_NONE, false, einval, 0);
    let _ = l.register(7, b"/a/g", b"/a/t", F_NOREPLACE, false, eexist, 0);
    let _ = l.register(8, b"/a/t", b"/a/g", F_NONE, true, 5, 4);
    let _ = l.register(9, b"/a/d1", b"/a/sub/d1", F_NONE, true, 7, 0);
    let _ = l.register(10, b"/a/d2", b"/a/sub", F_NONE, false, enotempty, 0);
    let _ = l.register(11, b"/a/g", b"/a/g", F_NONE, true, 5, 5);
    let _ = l.register(12, b"/a/g", b"/a/sub", F_EXCHANGE, true, 5, 3);
    let _ = l.register(13, b"/a/g", b"/a/zzz", F_EXCHANGE, false, noent, 0);
    let _ = l.register(14, b"/a/g", b"/a/q", 3, false, einval, 0);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 5 && l.err_count() == 10 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(MV_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice && l.rejects() == 1;

    let mut t = Tree::new(build_inodes(), build_entries());
    let a = t.rename(b"/a/t", b"/a/f", F_NONE);
    out[1] = a == Ok(Act { ino: 5, target: 4, kind: K_REG })
        && t.ino(4).map(|n| n.nlink) == Some(0)
        && t.find(1, b"f").is_some()
        && t.refs(4) == 0
        && t.audit().is_ok();

    let mut x = Tree::new(build_inodes(), build_entries());
    let e1 = x.rename(b"/a/f", b"/a/t", F_EXCHANGE);
    let swapped = x.ent(2).map(|e| e.ino) == Some(5) && x.ent(3).map(|e| e.ino) == Some(4);
    let e2 = x.rename(b"/a/f", b"/a/t", F_EXCHANGE);
    let back = x.ent(2).map(|e| e.ino) == Some(4) && x.ent(3).map(|e| e.ino) == Some(5) && x.audit().is_ok();
    out[2] = e1 == Ok(Act { ino: 4, target: 5, kind: K_REG })
        && e2 == Ok(Act { ino: 5, target: 4, kind: K_REG })
        && swapped
        && back;

    let mut m = Tree::new(build_inodes(), build_entries());
    let r1 = m.rename(b"/a/f", b"/a/t", F_NOREPLACE);
    let r2 = m.rename(b"/a/f", b"/a/d1", F_NONE);
    let r3 = m.rename(b"/a/d1", b"/a/f", F_NONE);
    let r4 = m.rename(b"/a/d1", b"/a/d2", F_NONE);
    let r5 = m.rename(b"/a/f", b"/b/h", F_NONE);
    let r6 = m.rename(b"/a/d2", b"/a/d2/zz", F_NONE);
    out[3] = r1 == Err(MV_EEXIST)
        && r2 == Err(MV_EISDIR)
        && r3 == Err(MV_ENOTDIR)
        && r4 == Err(MV_ENOTEMPTY)
        && r5 == Err(MV_EXDEV)
        && r6 == Err(MV_EINVAL)
        && m.audit().is_ok();
    out
}

/// 十三项判据。
pub fn run_unx11461_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11461.rename");
    let l = build_ledger();
    s.add(
        "MV1-十五格账本齐备",
        l.len() == SAMPLES
            && !l.is_empty()
            && l.sample(0).map(|x| x.old() == b"/a/f" as &[u8]) == Some(true)
            && l.sample(14).map(|x| x.new() == b"/a/q" as &[u8]) == Some(true),
        "static",
    );
    s.add_dyn(
        "MV2-五应十拒分布正确",
        l.ok_count() == 5 && l.err_count() == 10 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "MV3-同路径退化为无操作",
        l.outcome(11).map(|o| (o.ok, o.value, o.target)) == Some((true, 5, 5))
            && l.tree().find(1, b"g").is_some()
            && l.tree().ents_len() == 8,
        "static",
    );
    s.add(
        "MV4-NOREPLACE 拒绝且无副作用",
        l.outcome(7).map(|o| o.value) == Some((MV_EEXIST & 0xffff) as u16)
            && l.tree().ino(5).map(|n| n.nlink) == Some(1)
            && l.tree().ents_len() == 8,
        "static",
    );
    s.add(
        "MV5-目录与文件方向分判",
        l.outcome(2).map(|o| o.value) == Some((MV_EISDIR & 0xffff) as u16)
            && l.outcome(3).map(|o| o.value) == Some((MV_ENOTDIR & 0xffff) as u16),
        "static",
    );
    s.add(
        "MV6-非空目标目录拒绝",
        l.outcome(10).map(|o| o.value) == Some((MV_ENOTEMPTY & 0xffff) as u16)
            && l.tree().find(1, b"sub").is_some()
            && l.tree().find(3, b"d1").is_some(),
        "static",
    );
    s.add(
        "MV7-跨设备拒绝且源保留",
        l.outcome(1).map(|o| o.value) == Some((MV_EXDEV & 0xffff) as u16)
            && l.tree().ino(6).map(|n| n.dev) == Some(1)
            && l.tree().ino(5).map(|n| n.dev) == Some(0),
        "static",
    );
    s.add(
        "MV8-源缺失与父缺失同码",
        l.outcome(4).map(|o| o.value) == Some((MV_ENOENT & 0xffff) as u16)
            && l.outcome(5).map(|o| o.value) == Some((MV_ENOENT & 0xffff) as u16),
        "static",
    );
    s.add(
        "MV9-目录搬入自身子树判环",
        l.outcome(6).map(|o| o.value) == Some((MV_EINVAL & 0xffff) as u16)
            && l.tree().find(8, b"leaf").is_some()
            && l.tree().find(8, b"zz").is_none(),
        "static",
    );
    s.add(
        "MV10-替换递减目标引用",
        l.outcome(8).map(|o| (o.value, o.target)) == Some((5, 4))
            && l.tree().refs(4) == 0
            && l.tree().ino(4).map(|n| n.nlink) == Some(0)
            && l.tree().ino(5).map(|n| n.nlink) == Some(1),
        "static",
    );
    s.add(
        "MV11-EXCHANGE 只换指针",
        l.outcome(12).map(|o| (o.value, o.target)) == Some((5, 3))
            && l.tree().find(1, b"g").is_some()
            && l.tree().find(1, b"sub").is_some()
            && l.tree().ent(2).map(|e| e.ino) == Some(3),
        "static",
    );
    s.add(
        "MV12-非法标志组合拒绝",
        l.outcome(14).map(|o| o.value) == Some((MV_EINVAL & 0xffff) as u16)
            && l.tree().find(1, b"q").is_none(),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(MV_EFROZEN);
    let rej = f.rejects();
    s.add_dyn(
        "MV13-冻结重放幂等且四路全绿",
        root != 0 && rep == Ok(root) && again && rej == 1 && f.root() == root && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!(
        "ok={} err={} ents={} refs5={}",
        l.ok_count(),
        l.err_count(),
        l.tree().ents_len(),
        l.tree().refs(5)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Tree {
        Tree::new(build_inodes(), build_entries())
    }

    #[test]
    fn plain_rename_moves_entry() {
        let mut t = fresh();
        assert_eq!(t.rename(b"/a/f", b"/a/g", F_NONE), Ok(Act { ino: 4, target: 0, kind: K_REG }));
        assert!(t.find(1, b"f").is_none());
        assert!(t.find(1, b"g").is_some());
        assert!(t.audit().is_ok());
    }

    #[test]
    fn replace_unlinks_target() {
        let mut t = fresh();
        assert_eq!(t.rename(b"/a/t", b"/a/f", F_NONE), Ok(Act { ino: 5, target: 4, kind: K_REG }));
        assert_eq!(t.refs(4), 0);
        assert_eq!(t.refs(5), 1);
        assert_eq!(t.ents_len(), 8);
    }

    #[test]
    fn noreplace_refuses() {
        let mut t = fresh();
        assert_eq!(t.rename(b"/a/f", b"/a/t", F_NOREPLACE), Err(MV_EEXIST));
        assert_eq!(t.refs(4), 1);
        assert_eq!(t.refs(5), 1);
    }

    #[test]
    fn type_conflicts_are_directional() {
        let mut t = fresh();
        assert_eq!(t.rename(b"/a/f", b"/a/d1", F_NONE), Err(MV_EISDIR));
        assert_eq!(t.rename(b"/a/d1", b"/a/f", F_NONE), Err(MV_ENOTDIR));
        assert_eq!(t.rename(b"/a/d1", b"/a/d2", F_NONE), Err(MV_ENOTEMPTY));
    }

    #[test]
    fn exchange_round_trip() {
        let mut t = fresh();
        assert_eq!(t.rename(b"/a/f", b"/a/t", F_EXCHANGE), Ok(Act { ino: 4, target: 5, kind: K_REG }));
        assert_eq!(t.rename(b"/a/f", b"/a/t", F_EXCHANGE), Ok(Act { ino: 5, target: 4, kind: K_REG }));
        assert_eq!(t.ent(2).map(|e| e.ino), Some(4));
        assert_eq!(t.ent(3).map(|e| e.ino), Some(5));
    }

    #[test]
    fn cycle_and_cross_device() {
        let mut t = fresh();
        assert_eq!(t.rename(b"/a/d2", b"/a/d2/zz", F_NONE), Err(MV_EINVAL));
        assert_eq!(t.rename(b"/a/f", b"/b/h", F_NONE), Err(MV_EXDEV));
        assert_eq!(t.errors(), 2);
    }

    #[test]
    fn ledger_and_checks_hold() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 5);
        assert_eq!(l.err_count(), 10);
        assert!(l.audit().is_ok());
        assert!(summary(&l).starts_with("ok=5"));
        let s = run_unx11461_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
