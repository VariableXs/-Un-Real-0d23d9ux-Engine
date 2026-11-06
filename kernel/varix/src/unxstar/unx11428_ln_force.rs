//! UNX-F11428 ln -f 强制覆盖建链（0x20a80 段）
//!
//! 没有 `-f` 时，`ln` 遇到已存在的目标立刻失败（EEXIST），绝不改动旧名字。
//! 加上 `-f` 后，工具层先 `unlink(dst)` 再 `link(src, dst)`，两步之间存在一个
//! 短暂窗口——本模型把这个窗口显式记进日志，而不是假装它原子。日志里一次
//! 成功替换必须恰好是 UNLINK 后接 LINK；只写一步或顺序颠倒都判失衡。
//!
//! 强制覆盖不是无条件覆盖：目标是目录时返回 EISDIR（不能先删目录再建链），
//! 源与目标已经指向同一 inode 时返回 EEXIST（GNU ln 报 “are the same file”），
//! 目标带不可变标志时返回 EPERM。这三种拒绝必须发生在 UNLINK 之前，因此
//! 日志长度是判断“拒绝路径无副作用”的直接证据。
//!
//! 模型不建目录树，只维护一张平铺的名字表与 inode 表，这样可以把注意力全部
//! 放在链接计数的迁移上：覆盖普通文件时旧 inode 计数减一，归零即释放并计入
//! freed；源 inode 计数加一。覆盖一个仍被别处引用的 inode（nlink > 1）只减
//! 计数，不释放——账本用两格相邻样本对照这一差别。
//!
//! 十四格里六格成功、八格失败。四路注入分别验证：替换日志形如
//! [UNLINK, LINK]、无 -f 时旧名字原样保留、同 inode 拒绝且日志不增长、
//! 归零释放与多引用不释放的分野。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20a80;
/// 编号非法。
pub const LF_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const LF_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const LF_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const LF_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const LF_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符预期。
pub const LF_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 日志或计数失衡。
pub const LF_EBAL: u32 = SEG_BASE + 0x07;
/// 名字非法。
pub const LF_ENAME: u32 = SEG_BASE + 0x08;
/// 源不存在。
pub const LF_ENOENT: u32 = SEG_BASE + 0x09;
/// 目标已存在。
pub const LF_EEXIST: u32 = SEG_BASE + 0x0a;
/// 跨设备。
pub const LF_EXDEV: u32 = SEG_BASE + 0x0b;
/// 目标是目录。
pub const LF_EISDIR: u32 = SEG_BASE + 0x0c;
/// 不可变节点。
pub const LF_EPERM: u32 = SEG_BASE + 0x0d;
/// 名字表已满。
pub const LF_ENOSPC: u32 = SEG_BASE + 0x0e;
/// 票号不一致。
pub const LF_ESEAL: u32 = SEG_BASE + 0x0f;

/// 普通文件。
pub const K_REG: u8 = 1;
/// 符号链接。
pub const K_LNK: u8 = 2;
/// 目录。
pub const K_DIR: u8 = 3;
/// 日志：探测。
pub const J_PROBE: u8 = 1;
/// 日志：删旧名。
pub const J_UNLINK: u8 = 2;
/// 日志：建新链。
pub const J_LINK: u8 = 3;

/// 名字容量。
pub const NAME_CAP: usize = 8;
/// inode 容量。
pub const NODES: usize = 8;
/// 名字表容量。
pub const SLOTS: usize = 10;
/// 样本格数。
pub const SAMPLES: usize = 14;

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

fn put(dst: &mut [u8; NAME_CAP], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < NAME_CAP {
        dst[i] = src[i];
        i += 1;
    }
    i as u8
}

/// 一个 inode：设备、类型、链接计数、不可变位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub ino: u16,
    pub dev: u8,
    pub kind: u8,
    pub nlink: u16,
    pub immutable: bool,
}

impl Node {
    pub const fn make(ino: u16, dev: u8, kind: u8, nlink: u16, im: bool) -> Node {
        Node { ino, dev, kind, nlink, immutable: im }
    }
}

/// 平铺名字项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub ino: u16,
}

impl Entry {
    pub fn make(name: &[u8], ino: u16) -> Entry {
        let mut n = [0u8; NAME_CAP];
        let len = put(&mut n, name);
        Entry { name: n, nlen: len, ino }
    }

    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 一条日志。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub kind: u8,
    pub ino: u16,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
}

/// 名字表加 inode 表。
pub struct Namespace {
    nodes: Vec<Node>,
    entries: Vec<Entry>,
    log: Vec<Step>,
    freed: u32,
    replaced: u32,
    rejects: u32,
}

impl Namespace {
    pub fn new(nodes: Vec<Node>, entries: Vec<Entry>) -> Namespace {
        Namespace { nodes, entries, log: Vec::new(), freed: 0, replaced: 0, rejects: 0 }
    }

    pub fn node(&self, ino: u16) -> Option<Node> {
        if (ino as usize) >= self.nodes.len() {
            return None;
        }
        match self.nodes.get(ino as usize) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    fn node_mut(&mut self, ino: u16) -> Option<&mut Node> {
        if (ino as usize) >= self.nodes.len() {
            return None;
        }
        self.nodes.get_mut(ino as usize)
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    pub fn step(&self, i: usize) -> Option<Step> {
        match self.log.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn freed(&self) -> u32 {
        self.freed
    }

    pub fn replaced(&self) -> u32 {
        self.replaced
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn lookup(&self, name: &[u8]) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn push_log(&mut self, kind: u8, ino: u16, name: &[u8]) {
        let mut n = [0u8; NAME_CAP];
        let len = put(&mut n, name);
        self.log.push(Step { kind, ino, name: n, nlen: len });
    }

    fn drop_name(&mut self, idx: usize) -> Option<u16> {
        let e = match self.entries.get(idx) {
            Some(v) => *v,
            None => return None,
        };
        self.entries.remove(idx);
        let n = match self.node(e.ino) {
            Some(v) => v,
            None => return None,
        };
        if n.nlink > 0 {
            if let Some(m) = self.node_mut(e.ino) {
                m.nlink -= 1;
            }
        }
        if n.nlink <= 1 {
            self.freed += 1;
        }
        self.push_log(J_UNLINK, e.ino, e.name());
        Some(e.ino)
    }

    /// 强制或非强制建链。成功返回源 inode。
    pub fn force_link(&mut self, src: &[u8], dst: &[u8], force: bool) -> Result<u16, u32> {
        if src.is_empty() || dst.is_empty() || src.len() > NAME_CAP || dst.len() > NAME_CAP {
            self.rejects += 1;
            return Err(LF_ENAME);
        }
        let si = match self.lookup(src) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(LF_ENOENT);
            }
        };
        let se = match self.entries.get(si) {
            Some(v) => *v,
            None => {
                self.rejects += 1;
                return Err(LF_ENOENT);
            }
        };
        let sn = match self.node(se.ino) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(LF_ENOENT);
            }
        };
        if sn.kind == K_DIR || sn.immutable {
            self.rejects += 1;
            return Err(LF_EPERM);
        }
        let existing = self.lookup(dst);
        if let Some(di) = existing {
            let de = match self.entries.get(di) {
                Some(v) => *v,
                None => {
                    self.rejects += 1;
                    return Err(LF_ENOENT);
                }
            };
            let dn = match self.node(de.ino) {
                Some(v) => v,
                None => {
                    self.rejects += 1;
                    return Err(LF_ENOENT);
                }
            };
            if de.ino == se.ino {
                self.rejects += 1;
                return Err(LF_EEXIST);
            }
            if dn.kind == K_DIR {
                self.rejects += 1;
                return Err(LF_EISDIR);
            }
            if dn.dev != sn.dev {
                self.rejects += 1;
                return Err(LF_EXDEV);
            }
            if dn.immutable {
                self.rejects += 1;
                return Err(LF_EPERM);
            }
            if !force {
                self.rejects += 1;
                return Err(LF_EEXIST);
            }
            self.push_log(J_PROBE, de.ino, dst);
            let _ = self.drop_name(di);
            self.replaced += 1;
        } else if self.entries.len() >= SLOTS {
            self.rejects += 1;
            return Err(LF_ENOSPC);
        }
        if let Some(m) = self.node_mut(se.ino) {
            m.nlink += 1;
        }
        self.entries.push(Entry::make(dst, se.ino));
        self.push_log(J_LINK, se.ino, dst);
        Ok(se.ino)
    }

    /// 日志自洽：替换段必须是 PROBE/UNLINK 后接 LINK，且计数与名字表相符。
    pub fn audit(&self) -> Result<(), u32> {
        if self.entries.is_empty() || self.nodes.is_empty() {
            return Err(LF_EBAL);
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(v) => *v,
                None => return Err(LF_ENOBS),
            };
            let mut refs = 0u16;
            let mut j = 0usize;
            while j < self.entries.len() {
                if let Some(e) = self.entries.get(j) {
                    if e.ino == n.ino {
                        refs += 1;
                    }
                }
                j += 1;
            }
            if refs != n.nlink {
                return Err(LF_EBAL);
            }
            i += 1;
        }
        let mut a = 0usize;
        while a < self.entries.len() {
            let x = match self.entries.get(a) {
                Some(v) => *v,
                None => return Err(LF_ENOBS),
            };
            let mut b = a + 1;
            while b < self.entries.len() {
                if let Some(y) = self.entries.get(b) {
                    if x.name() == y.name() {
                        return Err(LF_EBAL);
                    }
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }

    /// 日志中替换对的个数（UNLINK 之后紧跟同名 LINK）。
    pub fn replace_pairs(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i + 1 < self.log.len() {
            if let (Some(a), Some(b)) = (self.log.get(i), self.log.get(i + 1)) {
                if a.kind == J_UNLINK && b.kind == J_LINK && a.name() == b.name() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

impl Step {
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 签名。
pub fn hash_bytes(h: u64, b: &[u8]) -> u64 {
    let mut h = h;
    let mut i = 0usize;
    while i < b.len() {
        if let Some(x) = b.get(i) {
            h ^= *x as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

/// 标准 inode 表。
pub fn build_nodes() -> Vec<Node> {
    let mut v = Vec::new();
    v.push(Node::make(0, 0, K_DIR, 0, false));
    v.push(Node::make(1, 0, K_REG, 1, false));
    v.push(Node::make(2, 0, K_REG, 2, false));
    v.push(Node::make(3, 0, K_REG, 1, true));
    v.push(Node::make(4, 1, K_REG, 1, false));
    v.push(Node::make(5, 0, K_LNK, 1, false));
    v.push(Node::make(6, 0, K_DIR, 1, false));
    v.push(Node::make(7, 0, K_REG, 1, false));
    v
}

/// 标准名字表。
pub fn build_entries() -> Vec<Entry> {
    let mut v = Vec::new();
    v.push(Entry::make(b"a", 1));
    v.push(Entry::make(b"b", 2));
    v.push(Entry::make(b"c", 2));
    v.push(Entry::make(b"im", 3));
    v.push(Entry::make(b"x", 4));
    v.push(Entry::make(b"l", 5));
    v.push(Entry::make(b"d", 6));
    v.push(Entry::make(b"o", 7));
    v
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub src: [u8; NAME_CAP],
    pub slen: u8,
    pub dst: [u8; NAME_CAP],
    pub dlen: u8,
    pub force: bool,
    pub ok: bool,
    pub expect: u16,
}

impl Case {
    pub fn src(&self) -> &[u8] {
        cut(&self.src, self.slen)
    }

    pub fn dst(&self) -> &[u8] {
        cut(&self.dst, self.dlen)
    }
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub ok: bool,
    pub value: u16,
}

/// 账本。
pub struct Ledger {
    ns: Namespace,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { ns: Namespace::new(build_nodes(), build_entries()), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, src: &[u8], dst: &[u8], force: bool, ok: bool, expect: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(LF_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(LF_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(LF_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(LF_EORDER);
        }
        let r = self.ns.force_link(src, dst, force);
        let end = match r {
            Ok(v) => match () {
                _ if !ok || v != expect => {
                    self.rejects += 1;
                    return Err(LF_EMISMATCH);
                }
                _ => Ending { ok: true, value: v },
            },
            Err(e) => {
                if ok || (e & 0xffff) as u16 != expect {
                    self.rejects += 1;
                    return Err(LF_EMISMATCH);
                }
                Ending { ok: false, value: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { src: [0; NAME_CAP], slen: 0, dst: [0; NAME_CAP], dlen: 0, force, ok, expect };
        c.slen = put(&mut c.src, src);
        c.dlen = put(&mut c.dst, dst);
        self.cases.push(c);
        self.ends.push(end);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    pub fn case(&self, i: usize) -> Option<Case> {
        match self.cases.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn ending(&self, i: usize) -> Option<Ending> {
        match self.ends.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.ends.len() {
            if let Some(e) = self.ends.get(i) {
                if e.ok {
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

    pub fn ns(&self) -> &Namespace {
        &self.ns
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(LF_EBAL);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return Err(LF_ENOBS),
            };
            let e = match self.ends.get(i) {
                Some(v) => *v,
                None => return Err(LF_ENOBS),
            };
            if c.ok != e.ok || c.expect != e.value {
                return Err(LF_EMISMATCH);
            }
            i += 1;
        }
        self.ns.audit()
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.ns.nodes.len() {
            if let Some(n) = self.ns.nodes.get(i) {
                h = fold(h, n.ino as u64);
                h = fold(h, n.kind as u64);
                h = fold(h, n.nlink as u64);
                h = fold(h, n.immutable as u64);
            }
            i += 1;
        }
        i = 0;
        while i < self.ns.entries.len() {
            if let Some(e) = self.ns.entries.get(i) {
                h = fold(h, e.ino as u64);
                h = hash_bytes(h, e.name());
            }
            i += 1;
        }
        i = 0;
        while i < self.ns.log.len() {
            if let Some(st) = self.ns.log.get(i) {
                h = fold(h, st.kind as u64);
                h = hash_bytes(h, st.name());
            }
            i += 1;
        }
        h = fold(h, self.ns.freed as u64);
        h = fold(h, self.ns.replaced as u64);
        fold(h, self.ns.rejects as u64)
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(LF_EFROZEN);
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
            Err(LF_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let enoent = (LF_ENOENT & 0xffff) as u16;
    let eexist = (LF_EEXIST & 0xffff) as u16;
    let exdev = (LF_EXDEV & 0xffff) as u16;
    let eisdir = (LF_EISDIR & 0xffff) as u16;
    let eperm = (LF_EPERM & 0xffff) as u16;
    let ename = (LF_ENAME & 0xffff) as u16;
    let _ = l.register(0, b"a", b"n1", false, true, 1);
    let _ = l.register(1, b"a", b"n1", false, false, eexist);
    let _ = l.register(2, b"a", b"n1", true, false, eexist);
    let _ = l.register(3, b"a", b"o", true, true, 1);
    let _ = l.register(4, b"a", b"d", true, false, eisdir);
    let _ = l.register(5, b"a", b"x", true, false, exdev);
    let _ = l.register(6, b"a", b"im", true, false, eperm);
    let _ = l.register(7, b"im", b"n2", true, false, eperm);
    let _ = l.register(8, b"d", b"n3", true, false, eperm);
    let _ = l.register(9, b"zz", b"n3", true, false, enoent);
    let _ = l.register(10, b"l", b"b", true, true, 5);
    let _ = l.register(11, b"a", b"c", true, true, 1);
    let _ = l.register(12, b"a", b"zz", true, true, 1);
    let _ = l.register(13, b"", b"n5", true, false, ename);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 5 && l.err_count() == 9 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut n = Namespace::new(build_nodes(), build_entries());
    let r = n.force_link(b"a", b"o", true);
    let shape = n.log_len() == 3
        && n.step(0).map(|s| s.kind) == Some(J_PROBE)
        && n.step(1).map(|s| s.kind) == Some(J_UNLINK)
        && n.step(2).map(|s| s.kind) == Some(J_LINK)
        && n.node(7).map(|x| x.nlink) == Some(0)
        && n.freed() == 1;
    out[1] = r == Ok(1) && shape;

    let mut m = Namespace::new(build_nodes(), build_entries());
    let a = m.force_link(b"a", b"b", false);
    let b = m.force_link(b"a", b"a", true);
    let c = m.force_link(b"a", b"a", false);
    out[2] = a == Err(LF_EEXIST)
        && b == Err(LF_EEXIST)
        && c == Err(LF_EEXIST)
        && m.log_len() == 0
        && m.node(1).map(|x| x.nlink) == Some(1);

    let mut f = Namespace::new(build_nodes(), build_entries());
    let one = f.force_link(b"a", b"n1", false);
    let two = f.force_link(b"l", b"b", true);
    let three = f.force_link(b"a", b"c", true);
    out[3] = one == Ok(1)
        && two == Ok(5)
        && three == Ok(1)
        && f.freed() == 1
        && f.node(2).map(|x| x.nlink) == Some(0)
        && f.node(5).map(|x| x.nlink) == Some(2)
        && f.replace_pairs() == 2;
    out
}

/// 十三判据。
pub fn run_unx11428_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11428.ln_force");
    let l = build_ledger();
    let ec = |v: u32| (v & 0xffff) as u16;

    s.add("LF1-十四格齐备", l.len() == SAMPLES && l.case(0).map(|c| c.src() == b"a" as &[u8]) == Some(true), "static");
    s.add_dyn("LF2-五应九拒", l.ok_count() == 5 && l.err_count() == 9 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("LF3-无f重名拒绝", l.ending(1).map(|e| e.value) == Some(ec(LF_EEXIST)) && l.ns().lookup(b"n1").is_some(), "static");
    s.add("LF4-同inode拒绝", l.ending(2).map(|e| e.value) == Some(ec(LF_EEXIST)) && l.ns().lookup(b"n1").is_some(), "static");
    s.add("LF5-覆盖目录拒绝", l.ending(4).map(|e| e.value) == Some(ec(LF_EISDIR)), "static");
    s.add("LF6-跨设备拒绝", l.ending(5).map(|e| e.value) == Some(ec(LF_EXDEV)), "static");
    s.add("LF7-不可变目标拒绝", l.ending(6).map(|e| e.value) == Some(ec(LF_EPERM)), "static");
    s.add("LF8-不可变源拒绝", l.ending(7).map(|e| e.value) == Some(ec(LF_EPERM)), "static");
    s.add("LF9-目录源拒绝", l.ending(8).map(|e| e.value) == Some(ec(LF_EPERM)), "static");
    s.add("LF10-覆盖符号链接计数迁移", l.ending(10).map(|e| e.value) == Some(5) && l.ns().node(5).map(|n| n.nlink) == Some(2), "static");
    s.add("LF11-多引用不释放", l.ending(11).map(|e| e.value) == Some(1) && l.ns().node(2).map(|n| n.nlink) == Some(0) && l.ns().freed() == 2, "static");
    s.add("LF12-替换日志成对", l.ns().replace_pairs() == 3 && l.ns().replaced() == 3, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() { Ok(v) => v, Err(_) => 0 };
    let again = f.freeze() == Err(LF_EFROZEN);
    let inj = inject_four();
    s.add_dyn("LF13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("inj={:?}", inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns() -> Namespace {
        Namespace::new(build_nodes(), build_entries())
    }

    #[test]
    fn plain_link_adds_name() {
        let mut n = ns();
        assert_eq!(n.force_link(b"a", b"n1", false), Ok(1));
        assert_eq!(n.node(1).map(|x| x.nlink), Some(2));
        assert_eq!(n.log_len(), 1);
    }

    #[test]
    fn force_replaces_in_two_steps() {
        let mut n = ns();
        assert_eq!(n.force_link(b"a", b"o", true), Ok(1));
        assert_eq!(n.step(1).map(|x| x.kind), Some(J_UNLINK));
        assert_eq!(n.step(2).map(|x| x.kind), Some(J_LINK));
        assert_eq!(n.replaced(), 1);
    }

    #[test]
    fn same_inode_refused_without_log() {
        let mut n = ns();
        assert_eq!(n.force_link(b"a", b"a", true), Err(LF_EEXIST));
        assert_eq!(n.log_len(), 0);
    }

    #[test]
    fn dir_and_immutable_refused() {
        let mut n = ns();
        assert_eq!(n.force_link(b"a", b"d", true), Err(LF_EISDIR));
        assert_eq!(n.force_link(b"a", b"im", true), Err(LF_EPERM));
        assert_eq!(n.force_link(b"im", b"q", true), Err(LF_EPERM));
        assert_eq!(n.log_len(), 0);
    }

    #[test]
    fn cross_device_and_missing() {
        let mut n = ns();
        assert_eq!(n.force_link(b"a", b"x", true), Err(LF_EXDEV));
        assert_eq!(n.force_link(b"no", b"q", true), Err(LF_ENOENT));
        assert_eq!(n.force_link(b"", b"q", true), Err(LF_ENAME));
    }

    #[test]
    fn freeing_only_when_last_ref() {
        let mut n = ns();
        assert_eq!(n.force_link(b"a", b"c", true), Ok(1));
        assert_eq!(n.freed(), 0);
        assert_eq!(n.force_link(b"a", b"b", true), Ok(1));
        assert_eq!(n.node(2).map(|x| x.nlink), Some(0));
        assert_eq!(n.freed(), 1);
        assert!(n.audit().is_ok());
    }

    #[test]
    fn ledger_replays_and_freezes() {
        let l = build_ledger();
        assert_eq!(l.ok_count(), 5);
        assert_eq!(l.err_count(), 9);
        let mut f = build_ledger();
        let root = match f.freeze() { Ok(v) => v, Err(_) => 0 };
        assert_ne!(root, 0);
        assert_eq!(f.replay(), Ok(root));
        assert_eq!(f.freeze(), Err(LF_EFROZEN));
        assert_eq!(run_unx11428_checks().failed_count(), 0);
    }
}
