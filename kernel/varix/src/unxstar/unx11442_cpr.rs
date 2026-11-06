//! UNX-F11442 cp -r 递归遍历（0x20b60 段）
//!
//! 递归复制是一棵源树到目标目录的前序遍历。模型先解析源与目标，再把源的每个子项
//! 按表中顺序处理：目录先创建目标条目再下潜，普通文件直接创建，符号链接在不跟随
//! 时原样复制目标名，在跟随（-L）时先解析再决定是复制文件、下潜目录还是报错。
//! 前序顺序会写入日志，审计用日志验证“父先于子、兄弟保持原序”。
//!
//! 跟随时环路检测依赖祖先栈而不是全局已访问集合：栈里保存从本次源根到当前节点的
//! 源下标，符号链接的目标只要落在栈内就是 ELOOP。这样同一个目录出现在两个不同
//! 分支下会被复制两次（真实 cp -L 的行为），但沿当前分支回到祖先会被立即截断。
//! 源子项顺序被刻意排成 a、b、cyc、l1、l2，使 /src 的跟随复制在 cyc 处中止，
//! 先前已复制的 a、b 保留下来，结果标记为部分完成。
//!
//! 悬空链接跟随时返回 ENOENT；目标目录不存在返回 ENOENT，目标是文件返回 ENOTDIR，
//! 目标只读或源目录不可读返回 EACCES，目标同名子项已存在返回 EEXIST（模型不覆盖），
//! 相对目标路径返回 EINVAL。单文件与单链接复制走同一入口，只是不进入子项循环。
//!
//! 审计要求：复制出的节点名字与源同名、父节点是目录、模式等于源模式与掩码取反后
//! 的与（链接恒为 0777）、父链在有限步内到达根、兄弟名字唯一、创建计数与日志长度
//! 一致。十五格账本六成九败，部分完成的失败调用也计入日志，这是递归复制与单文件
//! 复制最重要的差别。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20b60;
/// 编号非法。
pub const CR_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const CR_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const CR_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const CR_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const CR_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const CR_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const CR_EBAL: u32 = SEG_BASE + 0x07;
/// 不存在。
pub const CR_ENOENT: u32 = SEG_BASE + 0x08;
/// 目标非目录。
pub const CR_ENOTDIR: u32 = SEG_BASE + 0x09;
/// 目标已存在。
pub const CR_EEXIST: u32 = SEG_BASE + 0x0a;
/// 循环链接。
pub const CR_ELOOP: u32 = SEG_BASE + 0x0b;
/// 拒绝访问。
pub const CR_EACCES: u32 = SEG_BASE + 0x0c;
/// 参数非法。
pub const CR_EINVAL: u32 = SEG_BASE + 0x0d;
/// 票据不一致。
pub const CR_ESEAL: u32 = SEG_BASE + 0x0e;

/// 目录。
pub const K_DIR: u8 = 1;
/// 普通文件。
pub const K_FILE: u8 = 2;
/// 符号链接。
pub const K_LNK: u8 = 3;
/// 无父哨兵。
pub const NO_PARENT: u16 = 0xffff;
/// 无目标哨兵。
pub const NO_NODE: u16 = 0xffff;

/// 名字容量。
pub const NAME_CAP: usize = 8;
/// 路径容量。
pub const PATH_CAP: usize = 24;
/// 掩码。
pub const UMASK: u16 = 0o022;
/// 样本格数。
pub const SAMPLES: usize = 15;
/// 父链上限。
pub const CHAIN_MAX: usize = 64;

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

fn cut(a: &[u8], n: u8) -> &[u8] {
    let k = n as usize;
    if k > a.len() {
        return &[];
    }
    match a.get(..k) {
        Some(v) => v,
        None => &[],
    }
}

fn put(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        dst[i] = src[i];
        i += 1;
    }
    i as u8
}

/// 树节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub parent: u16,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub kind: u8,
    pub perm: u16,
    pub size: u32,
    pub ro: bool,
    pub copied: bool,
    pub src_of: u16,
    pub link: [u8; NAME_CAP],
    pub llen: u8,
}

impl Node {
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }

    pub fn link(&self) -> &[u8] {
        cut(&self.link, self.llen)
    }
}

/// 统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    pub dirs: u8,
    pub files: u8,
    pub links: u8,
}

/// 树。
pub struct Tree {
    nodes: Vec<Node>,
    journal: Vec<u16>,
    reject: u32,
    calls: u32,
}

impl Tree {
    pub fn new() -> Tree {
        let mut v = Vec::new();
        v.push(dir(NO_PARENT, b"/", 0o755, false));
        v.push(dir(0, b"src", 0o755, false));
        v.push(dir(1, b"a", 0o750, false));
        v.push(file(2, b"x", 0o640, 10));
        v.push(file(2, b"y", 0o600, 20));
        v.push(file(1, b"b", 0o644, 30));
        v.push(lnk(1, b"cyc", 1));
        v.push(lnk(1, b"l1", 2));
        v.push(lnk(1, b"l2", NO_NODE));
        v.push(dir(1, b"deep", 0o755, false));
        v.push(dir(9, b"d2", 0o755, false));
        v.push(file(10, b"d3", 0o644, 40));
        v.push(dir(1, b"ro", 0o755, true));
        v.push(file(12, b"z", 0o644, 50));
        v.push(dir(0, b"ok", 0o755, false));
        v.push(file(14, b"q", 0o644, 60));
        v.push(dir(14, b"sub", 0o755, false));
        v.push(file(16, b"r", 0o644, 70));
        v.push(lnk(14, b"ld", 16));
        v.push(dir(0, b"dst", 0o755, false));
        v.push(dir(0, b"dst2", 0o755, false));
        v.push(dir(0, b"dst3", 0o755, false));
        v.push(dir(0, b"dst4", 0o755, false));
        v.push(dir(0, b"dst5", 0o755, false));
        Tree { nodes: v, journal: Vec::new(), reject: 0, calls: 0 }
    }

    pub fn node(&self, i: usize) -> Option<Node> {
        match self.nodes.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    pub fn journal(&self) -> &[u16] {
        &self.journal
    }

    pub fn calls(&self) -> u32 {
        self.calls
    }

    pub fn rejects(&self) -> u32 {
        self.reject
    }

    fn child(&self, parent: usize, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.parent as usize == parent && n.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 解析绝对路径。
    pub fn resolve(&self, path: &[u8]) -> Option<usize> {
        if path.first() != Some(&b'/') {
            return None;
        }
        let mut cur = 0usize;
        let mut start = 1usize;
        let mut i = 1usize;
        while i <= path.len() {
            let at_end = i == path.len();
            let sep = !at_end && path.get(i) == Some(&b'/');
            if at_end || sep {
                if i > start {
                    let comp = path.get(start..i)?;
                    if comp == b"." || comp == b".." {
                        return None;
                    }
                    match self.child(cur, comp) {
                        Some(k) => cur = k,
                        None => return None,
                    }
                }
                start = i + 1;
            }
            i += 1;
        }
        Some(cur)
    }

    fn create(&mut self, parent: usize, name: &[u8], kind: u8, perm: u16, size: u32, src_of: u16, link: &[u8]) -> Option<usize> {
        let mut n = Node {
            parent: parent as u16,
            name: [0; NAME_CAP],
            nlen: 0,
            kind,
            perm,
            size,
            ro: false,
            copied: true,
            src_of,
            link: [0; NAME_CAP],
            llen: 0,
        };
        n.nlen = put(&mut n.name, name);
        if kind == K_LNK {
            n.llen = put(&mut n.link, link);
        }
        let idx = self.nodes.len();
        self.nodes.push(n);
        Some(idx)
    }

    /// 复制入口。follow 为真时解析符号链接。
    pub fn copy(&mut self, src_path: &[u8], dst_path: &[u8], follow: bool) -> Result<Stats, u32> {
        if src_path.first() != Some(&b'/') || dst_path.first() != Some(&b'/') {
            self.reject += 1;
            return Err(CR_EINVAL);
        }
        let si = match self.resolve(src_path) {
            Some(v) => v,
            None => {
                self.reject += 1;
                return Err(CR_ENOENT);
            }
        };
        let di = match self.resolve(dst_path) {
            Some(v) => v,
            None => {
                self.reject += 1;
                return Err(CR_ENOENT);
            }
        };
        let d = match self.nodes.get(di) {
            Some(v) => *v,
            None => {
                self.reject += 1;
                return Err(CR_ENOENT);
            }
        };
        if d.kind != K_DIR {
            self.reject += 1;
            return Err(CR_ENOTDIR);
        }
        if d.ro {
            self.reject += 1;
            return Err(CR_EACCES);
        }
        let s = match self.nodes.get(si) {
            Some(v) => *v,
            None => {
                self.reject += 1;
                return Err(CR_ENOENT);
            }
        };
        if s.kind == K_DIR && s.ro {
            self.reject += 1;
            return Err(CR_EACCES);
        }
        self.calls += 1;
        let mut stats = Stats { dirs: 0, files: 0, links: 0 };
        if s.kind == K_DIR {
            let mut anc = Vec::new();
            anc.push(si as u16);
            self.copy_into(si, di, follow, &mut anc, &mut stats)?;
        } else if s.kind == K_LNK && follow {
            let t = self.follow_target(si)?;
            let tn = match self.nodes.get(t) {
                Some(v) => *v,
                None => return Err(CR_ENOENT),
            };
            if tn.kind == K_DIR {
                let mut anc = Vec::new();
                anc.push(t as u16);
                let pos = match self.create(di, s.name(), K_DIR, tn.perm & !UMASK, 0, t as u16, &[]) {
                    Some(v) => v,
                    None => return Err(CR_EBAL),
                };
                stats.dirs += 1;
                self.journal.push(t as u16);
                self.copy_into(t, pos, follow, &mut anc, &mut stats)?;
            } else {
                self.copy_one(si, di, follow, &mut stats)?;
            }
        } else {
            self.copy_one(si, di, follow, &mut stats)?;
        }
        Ok(stats)
    }

    fn follow_target(&self, idx: usize) -> Result<usize, u32> {
        let n = match self.nodes.get(idx) {
            Some(v) => *v,
            None => return Err(CR_ENOENT),
        };
        if n.kind != K_LNK {
            return Ok(idx);
        }
        if n.src_of == NO_NODE {
            return Err(CR_ENOENT);
        }
        let t = n.src_of as usize;
        match self.nodes.get(t) {
            Some(v) => {
                if v.kind == K_LNK {
                    Err(CR_ELOOP)
                } else {
                    Ok(t)
                }
            }
            None => Err(CR_ENOENT),
        }
    }

    fn copy_one(&mut self, si: usize, di: usize, follow: bool, stats: &mut Stats) -> Result<(), u32> {
        let s = match self.nodes.get(si) {
            Some(v) => *v,
            None => return Err(CR_ENOENT),
        };
        let mut kind = s.kind;
        let mut perm = s.perm & !UMASK;
        let mut src_of = si as u16;
        if s.kind == K_LNK {
            if follow {
                let t = self.follow_target(si)?;
                let tn = match self.nodes.get(t) {
                    Some(v) => *v,
                    None => return Err(CR_ENOENT),
                };
                kind = tn.kind;
                perm = tn.perm & !UMASK;
                src_of = t as u16;
            } else {
                perm = 0o777;
            }
        }
        if self.child(di, s.name()).is_some() {
            self.reject += 1;
            return Err(CR_EEXIST);
        }
        match self.create(di, s.name(), kind, perm, if kind == K_FILE { s.size } else { 0 }, src_of, s.link()) {
            Some(_) => {}
            None => return Err(CR_EBAL),
        }
        self.journal.push(si as u16);
        if kind == K_DIR {
            stats.dirs += 1;
        } else if kind == K_FILE {
            stats.files += 1;
        } else {
            stats.links += 1;
        }
        Ok(())
    }

    fn copy_into(&mut self, src_idx: usize, dst_idx: usize, follow: bool, anc: &mut Vec<u16>, stats: &mut Stats) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            let child = match self.nodes.get(i) {
                Some(v) => *v,
                None => {
                    self.reject += 1;
                    return Err(CR_ENOBS);
                }
            };
            if child.parent as usize == src_idx && child.nlen > 0 {
                if self.child(dst_idx, child.name()).is_some() {
                    self.reject += 1;
                    return Err(CR_EEXIST);
                }
                if child.kind == K_LNK && follow {
                    let t = match child.src_of {
                        NO_NODE => {
                            self.reject += 1;
                            return Err(CR_ENOENT);
                        }
                        x => x,
                    };
                    let tn = match self.nodes.get(t as usize) {
                        Some(v) => *v,
                        None => {
                            self.reject += 1;
                            return Err(CR_ENOENT);
                        }
                    };
                    if anc.contains(&t) || tn.kind == K_LNK {
                        self.reject += 1;
                        return Err(CR_ELOOP);
                    }
                    let kind = tn.kind;
                    let perm = if kind == K_LNK { 0o777 } else { tn.perm & !UMASK };
                    let pos = match self.create(dst_idx, child.name(), kind, perm, if kind == K_FILE { tn.size } else { 0 }, t, &[]) {
                        Some(v) => v,
                        None => return Err(CR_EBAL),
                    };
                    self.journal.push(t);
                    if kind == K_DIR {
                        stats.dirs += 1;
                        anc.push(t);
                        self.copy_into(t as usize, pos, follow, anc, stats)?;
                        anc.pop();
                    } else if kind == K_FILE {
                        stats.files += 1;
                    } else {
                        stats.links += 1;
                    }
                } else {
                    let kind = if child.kind == K_LNK { K_LNK } else { child.kind };
                    let perm = if kind == K_LNK { 0o777 } else { child.perm & !UMASK };
                    let pos = match self.create(dst_idx, child.name(), kind, perm, if kind == K_FILE { child.size } else { 0 }, i as u16, child.link()) {
                        Some(v) => v,
                        None => return Err(CR_EBAL),
                    };
                    self.journal.push(i as u16);
                    if kind == K_DIR {
                        stats.dirs += 1;
                        anc.push(i as u16);
                        self.copy_into(i, pos, follow, anc, stats)?;
                        anc.pop();
                    } else if kind == K_FILE {
                        stats.files += 1;
                    } else {
                        stats.links += 1;
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.nodes.is_empty() || self.journal.len() > 64 {
            return Err(CR_EBAL);
        }
        let mut created = 0usize;
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(v) => *v,
                None => return Err(CR_ENOBS),
            };
            if n.copied {
                created += 1;
                if n.kind != K_DIR && n.kind != K_FILE && n.kind != K_LNK {
                    return Err(CR_EBAL);
                }
                let p = match self.nodes.get(n.parent as usize) {
                    Some(v) => *v,
                    None => return Err(CR_EBAL),
                };
                if p.kind != K_DIR {
                    return Err(CR_EBAL);
                }
                let s = match self.nodes.get(n.src_of as usize) {
                    Some(v) => *v,
                    None => return Err(CR_EBAL),
                };
                let want = if n.kind == K_LNK { 0o777 } else { s.perm & !UMASK };
                if n.perm != want {
                    return Err(CR_EBAL);
                }
            }
            let mut j = i + 1;
            while j < self.nodes.len() {
                if let Some(m) = self.nodes.get(j) {
                    if m.parent == n.parent && m.name() == n.name() && n.nlen > 0 {
                        return Err(CR_EBAL);
                    }
                }
                j += 1;
            }
            let mut cur = i;
            let mut steps = 0usize;
            while steps < CHAIN_MAX {
                let c = match self.nodes.get(cur) {
                    Some(v) => *v,
                    None => return Err(CR_EBAL),
                };
                if c.parent == NO_PARENT {
                    break;
                }
                cur = c.parent as usize;
                steps += 1;
            }
            if steps >= CHAIN_MAX {
                return Err(CR_EBAL);
            }
            i += 1;
        }
        if created != self.journal.len() || created > 64 {
            return Err(CR_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                h = fold(h, n.parent as u64);
                h = fold(h, n.kind as u64);
                h = fold(h, n.perm as u64);
                h = fold(h, n.size as u64);
                h = fold(h, n.copied as u64);
                h = fold(h, n.src_of as u64);
                let mut k = 0usize;
                while k < n.nlen as usize {
                    if let Some(b) = n.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.journal.len() as u64);
        h = fold(h, self.calls as u64);
        fold(h, self.reject as u64)
    }
}

fn dir(parent: u16, name: &[u8], perm: u16, ro: bool) -> Node {
    let mut n = Node {
        parent,
        name: [0; NAME_CAP],
        nlen: 0,
        kind: K_DIR,
        perm,
        size: 0,
        ro,
        copied: false,
        src_of: NO_NODE,
        link: [0; NAME_CAP],
        llen: 0,
    };
    n.nlen = put(&mut n.name, name);
    n
}

fn file(parent: u16, name: &[u8], perm: u16, size: u32) -> Node {
    let mut n = dir(parent, name, perm, false);
    n.kind = K_FILE;
    n.size = size;
    n
}

fn lnk(parent: u16, name: &[u8], target: u16) -> Node {
    let mut n = dir(parent, name, 0o777, false);
    n.kind = K_LNK;
    n.src_of = target;
    n
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub src: [u8; PATH_CAP],
    pub slen: u8,
    pub dst: [u8; PATH_CAP],
    pub dlen: u8,
    pub follow: bool,
    pub stats: Stats,
    pub ok: bool,
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub stats: Stats,
    pub code: u16,
}

fn zero_stats() -> Stats {
    Stats { dirs: 0, files: 0, links: 0 }
}

/// 账本。
pub struct Ledger {
    t: Tree,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    reject: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { t: Tree::new(), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, reject: 0 }
    }

    pub fn register(&mut self, slot: u8, src: &[u8], dst: &[u8], follow: bool, expect: Stats, ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.reject += 1;
            return Err(CR_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.reject += 1;
            return Err(CR_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.reject += 1;
            return Err(CR_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.reject += 1;
            return Err(CR_EORDER);
        }
        let r = self.t.copy(src, dst, follow);
        let end = match r {
            Ok(s) => {
                if !ok || s != expect {
                    self.reject += 1;
                    return Err(CR_EMISMATCH);
                }
                Ending { stats: s, code: 0 }
            }
            Err(e) => {
                if ok {
                    self.reject += 1;
                    return Err(CR_EMISMATCH);
                }
                Ending { stats: zero_stats(), code: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { src: [0; PATH_CAP], slen: 0, dst: [0; PATH_CAP], dlen: 0, follow, stats: end.stats, ok };
        c.slen = put(&mut c.src, src);
        c.dlen = put(&mut c.dst, dst);
        self.cases.push(c);
        self.ends.push(end);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn ending(&self, i: usize) -> Option<Ending> {
        match self.ends.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn tree(&self) -> &Tree {
        &self.t
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.ends.len() {
            if let Some(e) = self.ends.get(i) {
                if e.code == 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn err_count(&self) -> usize {
        self.ends.len() - self.ok_count()
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(CR_EBAL);
        }
        self.t.audit()
    }

    pub fn seal(&self) -> u64 {
        self.t.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.reject += 1;
            return Err(CR_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(CR_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.reject
    }
}

fn st(dirs: u8, files: u8, links: u8) -> Stats {
    Stats { dirs, files, links }
}

/// 十五格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, b"/src", b"/dst", false, st(4, 5, 3), true);
    let _ = l.register(1, b"/src", b"/dst", false, zero_stats(), false);
    let _ = l.register(2, b"/src", b"/dst2", true, zero_stats(), false);
    let _ = l.register(3, b"/ok", b"/dst3", true, st(2, 3, 0), true);
    let _ = l.register(4, b"/src/l2", b"/dst5", true, zero_stats(), false);
    let _ = l.register(5, b"/src/l2", b"/dst5", false, st(0, 0, 1), true);
    let _ = l.register(6, b"/src/l1", b"/dst5", false, st(0, 0, 1), true);
    let _ = l.register(7, b"/src/b", b"/dst5", false, st(0, 1, 0), true);
    let _ = l.register(8, b"/src/b", b"/dst", false, zero_stats(), false);
    let _ = l.register(9, b"/src/a", b"/dst/a", false, zero_stats(), false);
    let _ = l.register(10, b"/src", b"/src/b", false, zero_stats(), false);
    let _ = l.register(11, b"/src/ro", b"/dst5", false, zero_stats(), false);
    let _ = l.register(12, b"/nope", b"/dst5", false, zero_stats(), false);
    let _ = l.register(13, b"/src", b"dest", false, zero_stats(), false);
    let _ = l.register(14, b"/ok", b"/dst4", true, st(2, 3, 0), true);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 6 && l.err_count() == 9 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut a = Tree::new();
    let r = a.copy(b"/src", b"/dst", false);
    out[1] = r == Ok(st(4, 5, 3)) && a.journal().len() == 12
        && a.journal().get(0) == Some(&2u16) && a.journal().get(1) == Some(&3u16)
        && a.journal().get(2) == Some(&4u16) && a.journal().get(3) == Some(&5u16);

    let mut b = Tree::new();
    let r2 = b.copy(b"/src", b"/dst2", true);
    let created_before = b.count();
    out[2] = r2 == Err(CR_ELOOP) && b.journal().len() == 4 && created_before == 28
        && b.child(20, b"a").is_some() && b.child(20, b"b").is_some();

    let mut c = Tree::new();
    let r3 = c.copy(b"/ok", b"/dst3", true);
    out[3] = r3 == Ok(st(2, 3, 0)) && c.journal().len() == 5
        && c.child(21, b"ld").map(|i| c.node(i).map(|n| n.kind)) == Some(Some(K_DIR));
    out
}

/// 十三判据。
pub fn run_unx11442_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11442.cpr");
    let l = build_ledger();
    s.add("CR1-十五格齐备", l.len() == SAMPLES, "static");
    s.add_dyn("CR2-六成九拒", l.ok_count() == 6 && l.err_count() == 9 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("CR3-前序统计", l.ending(0).map(|e| e.stats) == Some(st(4, 5, 3)), "static");
    s.add("CR4-重复目标", l.ending(1).map(|e| e.code) == Some((CR_EEXIST & 0xffff) as u16), "static");
    s.add("CR5-跟随环路", l.ending(2).map(|e| e.code) == Some((CR_ELOOP & 0xffff) as u16), "static");
    s.add("CR6-跟随成功", l.ending(3).map(|e| e.stats) == Some(st(2, 3, 0)) && l.ending(14).map(|e| e.stats) == Some(st(2, 3, 0)), "static");
    s.add("CR7-悬空链接", l.ending(4).map(|e| e.code) == Some((CR_ENOENT & 0xffff) as u16), "static");
    s.add("CR8-链接不跟随", l.ending(5).map(|e| e.stats) == Some(st(0, 0, 1)) && l.ending(6).map(|e| e.stats) == Some(st(0, 0, 1)), "static");
    s.add("CR9-单文件", l.ending(7).map(|e| e.stats) == Some(st(0, 1, 0)) && l.tree().child(23, b"b").is_some(), "static");
    s.add("CR10-同名冲突", l.ending(8).map(|e| e.code) == Some((CR_EEXIST & 0xffff) as u16) && l.ending(9).map(|e| e.code) == Some((CR_EEXIST & 0xffff) as u16), "static");
    s.add("CR11-目标非目录", l.ending(10).map(|e| e.code) == Some((CR_ENOTDIR & 0xffff) as u16), "static");
    s.add("CR12-只读缺失与日志", l.ending(11).map(|e| e.code) == Some((CR_EACCES & 0xffff) as u16) && l.ending(12).map(|e| e.code) == Some((CR_ENOENT & 0xffff) as u16) && l.ending(13).map(|e| e.code) == Some((CR_EINVAL & 0xffff) as u16) && l.tree().journal().len() == 29 && l.tree().count() == 53 && l.tree().calls() == 11 && l.tree().child(20, b"a").is_some() && l.tree().child(20, b"b").is_some(), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(CR_EFROZEN);
    let inj = inject_four();
    s.add_dyn("CR13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preorder_copy() {
        let mut t = Tree::new();
        assert_eq!(t.copy(b"/src", b"/dst", false), Ok(st(4, 5, 3)));
        assert_eq!(t.journal().len(), 12);
        assert_eq!(t.child(19, b"a").is_some(), true);
        assert_eq!(t.child(19, b"deep").is_some(), true);
    }

    #[test]
    fn duplicate_destination() {
        let mut t = Tree::new();
        assert!(t.copy(b"/src", b"/dst", false).is_ok());
        assert_eq!(t.copy(b"/src", b"/dst", false), Err(CR_EEXIST));
    }

    #[test]
    fn follow_loop_partial() {
        let mut t = Tree::new();
        assert_eq!(t.copy(b"/src", b"/dst2", true), Err(CR_ELOOP));
        assert_eq!(t.journal().len(), 4);
        assert_eq!(t.child(20, b"a").is_some(), true);
        assert_eq!(t.child(20, b"cyc").is_none(), true);
    }

    #[test]
    fn follow_success() {
        let mut t = Tree::new();
        assert_eq!(t.copy(b"/ok", b"/dst3", true), Ok(st(2, 3, 0)));
        assert_eq!(t.journal().len(), 5);
    }

    #[test]
    fn dangling_and_plain_links() {
        let mut t = Tree::new();
        assert_eq!(t.copy(b"/src/l2", b"/dst5", true), Err(CR_ENOENT));
        assert_eq!(t.copy(b"/src/l2", b"/dst5", false), Ok(st(0, 0, 1)));
    }

    #[test]
    fn destination_checks() {
        let mut t = Tree::new();
        assert_eq!(t.copy(b"/src", b"/src/b", false), Err(CR_ENOTDIR));
        assert_eq!(t.copy(b"/src", b"dest", false), Err(CR_EINVAL));
        assert_eq!(t.copy(b"/nope", b"/dst5", false), Err(CR_ENOENT));
        assert_eq!(t.copy(b"/src/ro", b"/dst5", false), Err(CR_EACCES));
    }

    #[test]
    fn ledger_green() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11442_checks().failed_count(), 0);
    }
}
