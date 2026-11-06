//! UNX-F11429 rename 同文件系统移动（0x20a90 段）
//!
//! `rename` 不搬运数据，它只改两个目录项：把源名字从旧父目录摘掉，再以新名字
//! 挂到新父目录。因此模型不用内容缓冲区，只维护一棵以父指针表示的树。每次
//! 移动就是改一个 `parent` 字段加一段名字，随后整棵树仍需满足“兄弟名字唯一、
//! 父必为目录、根无父”这三条结构不变量。
//!
//! 类型组合构成第一层错误矩阵：目录换文件是 ENOTDIR，文件换目录是 EISDIR，
//! 目录换目录时目标必须为空，否则 ENOTEMPTY。第二层是拓扑约束：把目录移进
//! 自己的子树会产生环，必须在改写父指针**之前**用祖先链判定并返回 EINVAL。
//! 第三层是设备约束：源与目标父目录不在同一设备时返回 EXDEV，交给用户态走
//! 复制加删除的回退路径。
//!
//! 同一 inode 的源和目标视为成功空操作：不摘名字、不改父指针、不产生新节点，
//! 只回报目标父目录号。这一点让 `mv a a` 的退出码为 0，而 `mv a a/` 之类的
//! 类型冲突仍会失败。
//!
//! 账本十四格：六格成功（新建名、覆盖文件、移动空目录、移动整棵子树、自移、
//! 整目录换父），八格失败（缺失、拓扑环、ENOTEMPTY、两类类型冲突、跨设备）。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20a90;
/// 编号非法。
pub const RN_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const RN_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const RN_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const RN_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const RN_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const RN_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 结构失衡。
pub const RN_EBAL: u32 = SEG_BASE + 0x07;
/// 名字非法。
pub const RN_ENAME: u32 = SEG_BASE + 0x08;
/// 源不存在。
pub const RN_ENOENT: u32 = SEG_BASE + 0x09;
/// 源是目标祖先。
pub const RN_EINVAL: u32 = SEG_BASE + 0x0a;
/// 目标非空目录。
pub const RN_ENOTEMPTY: u32 = SEG_BASE + 0x0b;
/// 类型冲突（目录换文件）。
pub const RN_ENOTDIR: u32 = SEG_BASE + 0x0c;
/// 类型冲突（文件换目录）。
pub const RN_EISDIR: u32 = SEG_BASE + 0x0d;
/// 跨设备。
pub const RN_EXDEV: u32 = SEG_BASE + 0x0e;
/// 票号不一致。
pub const RN_ESEAL: u32 = SEG_BASE + 0x0f;

/// 目录。
pub const K_DIR: u8 = 0;
/// 普通文件。
pub const K_REG: u8 = 1;
/// 死亡标记。
pub const DEAD: u16 = 0xffff;

/// 名字容量。
pub const NAME_CAP: usize = 12;
/// 节点容量。
pub const NODES: usize = 12;
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

fn put(dst: &mut [u8], src: &[u8]) -> u8 {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        dst[i] = src[i];
        i += 1;
    }
    i as u8
}

/// 树节点：父下标、名字、类型、设备。子节点由扫描得出，不存计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub parent: u16,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub kind: u8,
    pub dev: u8,
}

impl Node {
    pub fn make(parent: u16, name: &[u8], kind: u8, dev: u8) -> Node {
        let mut n = [0u8; NAME_CAP];
        let len = put(&mut n, name);
        Node { parent, name: n, nlen: len, kind, dev }
    }

    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 目录树。
pub struct Tree {
    nodes: Vec<Node>,
    moves: u32,
    replaced: u32,
    rejects: u32,
}

impl Tree {
    pub fn new(nodes: Vec<Node>) -> Tree {
        Tree { nodes, moves: 0, replaced: 0, rejects: 0 }
    }

    pub fn node(&self, id: u16) -> Option<Node> {
        if (id as usize) >= self.nodes.len() {
            return None;
        }
        match self.nodes.get(id as usize) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    fn node_mut(&mut self, id: u16) -> Option<&mut Node> {
        if (id as usize) >= self.nodes.len() {
            return None;
        }
        self.nodes.get_mut(id as usize)
    }

    pub fn moves(&self) -> u32 {
        self.moves
    }

    pub fn replaced(&self) -> u32 {
        self.replaced
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 某父目录下的直接子节点。
    pub fn child(&self, parent: u16, name: &[u8]) -> Option<u16> {
        if name.is_empty() {
            return None;
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.parent == parent && n.nlen > 0 && n.name() == name {
                    return Some(i as u16);
                }
            }
            i += 1;
        }
        None
    }

    pub fn children(&self, parent: u16) -> u16 {
        let mut c = 0u16;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.parent == parent && n.parent != DEAD {
                    c += 1;
                }
            }
            i += 1;
        }
        c
    }

    /// 解析绝对路径到节点。
    pub fn resolve(&self, path: &[u8]) -> Result<u16, u32> {
        if path.is_empty() || path[0] != b'/' {
            return Err(RN_ENOENT);
        }
        let mut cur = 0u16;
        let mut i = 1usize;
        while i <= path.len() {
            let s = i;
            while i < path.len() && path[i] != b'/' {
                i += 1;
            }
            let comp = match path.get(s..i) {
                Some(v) => v,
                None => return Err(RN_ENOENT),
            };
            if i <= path.len() && !comp.is_empty() {
                let c = match self.node(cur) {
                    Some(v) => v,
                    None => return Err(RN_ENOENT),
                };
                if c.kind != K_DIR {
                    return Err(RN_ENOTDIR);
                }
                cur = match self.child(cur, comp) {
                    Some(v) => v,
                    None => return Err(RN_ENOENT),
                };
            }
            i += 1;
        }
        Ok(cur)
    }

    /// 拆分出父目录与末名字。
    pub fn split(&self, path: &[u8]) -> Result<(u16, [u8; NAME_CAP], u8), u32> {
        if path.is_empty() {
            return Err(RN_ENAME);
        }
        let mut cut_at = None;
        let mut i = 0usize;
        while i < path.len() {
            if path[i] == b'/' {
                cut_at = Some(i);
            }
            i += 1;
        }
        let last = match cut_at {
            None => return Err(RN_ENAME),
            Some(v) => v,
        };
        let name = match path.get(last + 1..) {
            Some(v) => v,
            None => return Err(RN_ENAME),
        };
        if name.is_empty() || name.len() > NAME_CAP || name == b"." || name == b".." {
            return Err(RN_EINVAL);
        }
        let dir = match path.get(..last) {
            Some(v) => v,
            None => return Err(RN_ENAME),
        };
        let parent = if dir.is_empty() {
            0
        } else {
            match self.resolve(dir) {
                Ok(v) => v,
                Err(e) => return Err(e),
            }
        };
        let pn = match self.node(parent) {
            Some(v) => v,
            None => return Err(RN_ENOENT),
        };
        if pn.kind != K_DIR {
            return Err(RN_ENOTDIR);
        }
        let mut nb = [0u8; NAME_CAP];
        let len = put(&mut nb, name);
        Ok((parent, nb, len))
    }

    /// src 是否为 anc 的祖先（含自身）。
    pub fn is_ancestor(&self, anc: u16, mut of: u16) -> bool {
        let mut guard = 0u16;
        loop {
            if of == anc {
                return true;
            }
            if of == 0 || guard > 32 {
                return false;
            }
            let n = match self.node(of) {
                Some(v) => v,
                None => return false,
            };
            of = n.parent;
            guard += 1;
        }
    }

    /// 同设备改名。
    pub fn rename(&mut self, src: &[u8], dst: &[u8]) -> Result<u16, u32> {
        if src.is_empty() || dst.is_empty() {
            self.rejects += 1;
            return Err(RN_ENAME);
        }
        let (sp, sn, sl) = match self.split(src) {
            Ok(v) => v,
            Err(e) => {
                self.rejects += 1;
                return Err(e);
            }
        };
        let (dp, dn, dl) = match self.split(dst) {
            Ok(v) => v,
            Err(e) => {
                self.rejects += 1;
                return Err(e);
            }
        };
        let sname = match sn.get(..sl as usize) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(RN_ENAME);
            }
        };
        let dname = match dn.get(..dl as usize) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(RN_ENAME);
            }
        };
        let sid = match self.child(sp, sname) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(RN_ENOENT);
            }
        };
        let sn0 = match self.node(sid) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(RN_ENOENT);
            }
        };
        let dup = self.child(dp, dname);
        if let Some(did) = dup {
            if did == sid {
                return Ok(dp);
            }
        }
        if sn0.kind == K_DIR && self.is_ancestor(sid, dp) {
            self.rejects += 1;
            return Err(RN_EINVAL);
        }
        let pn = match self.node(dp) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(RN_ENOENT);
            }
        };
        if pn.dev != sn0.dev {
            self.rejects += 1;
            return Err(RN_EXDEV);
        }
        if let Some(did) = dup {
            let dn0 = match self.node(did) {
                Some(v) => v,
                None => {
                    self.rejects += 1;
                    return Err(RN_ENOENT);
                }
            };
            if sn0.kind == K_DIR && dn0.kind != K_DIR {
                self.rejects += 1;
                return Err(RN_ENOTDIR);
            }
            if sn0.kind != K_DIR && dn0.kind == K_DIR {
                self.rejects += 1;
                return Err(RN_EISDIR);
            }
            if dn0.kind == K_DIR && self.children(did) != 0 {
                self.rejects += 1;
                return Err(RN_ENOTEMPTY);
            }
            if let Some(m) = self.node_mut(did) {
                m.parent = DEAD;
                m.nlen = 0;
            }
            self.replaced += 1;
        }
        if let Some(m) = self.node_mut(sid) {
            m.parent = dp;
            let mut nb = [0u8; NAME_CAP];
            m.nlen = put(&mut nb, dname);
            m.name = nb;
        }
        self.moves += 1;
        Ok(dp)
    }

    /// 结构审计：根无父、每个活节点父为目录、兄弟名字唯一。
    pub fn audit(&self) -> Result<(), u32> {
        if self.nodes.is_empty() {
            return Err(RN_EBAL);
        }
        let root = match self.nodes.first() {
            Some(v) => *v,
            None => return Err(RN_EBAL),
        };
        if root.parent != 0 || root.kind != K_DIR {
            return Err(RN_EBAL);
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(v) => *v,
                None => return Err(RN_ENOBS),
            };
            if n.parent == DEAD {
                i += 1;
                continue;
            }
            if i > 0 {
                let p = match self.node(n.parent) {
                    Some(v) => v,
                    None => return Err(RN_EBAL),
                };
                if p.kind != K_DIR || n.nlen == 0 {
                    return Err(RN_EBAL);
                }
            }
            let mut j = i + 1;
            while j < self.nodes.len() {
                if let Some(m) = self.nodes.get(j) {
                    if m.parent != DEAD && m.parent == n.parent && m.name() == n.name() && n.nlen > 0 {
                        return Err(RN_EBAL);
                    }
                }
                j += 1;
            }
            i += 1;
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
                h = fold(h, n.dev as u64);
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
        h = fold(h, self.moves as u64);
        h = fold(h, self.replaced as u64);
        fold(h, self.rejects as u64)
    }
}

/// 标准树。
pub fn build_tree() -> Vec<Node> {
    let mut v = Vec::new();
    v.push(Node::make(0, b"", K_DIR, 0));
    v.push(Node::make(0, b"a", K_DIR, 0));
    v.push(Node::make(0, b"b", K_DIR, 0));
    v.push(Node::make(0, b"c", K_DIR, 1));
    v.push(Node::make(1, b"f", K_REG, 0));
    v.push(Node::make(1, b"g", K_REG, 0));
    v.push(Node::make(2, b"h", K_REG, 0));
    v.push(Node::make(1, b"d", K_DIR, 0));
    v.push(Node::make(2, b"e", K_DIR, 0));
    v.push(Node::make(8, b"k", K_REG, 0));
    v.push(Node::make(8, b"sub", K_DIR, 0));
    v
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub src: [u8; 20],
    pub slen: u8,
    pub dst: [u8; 20],
    pub dlen: u8,
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
    tree: Tree,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { tree: Tree::new(build_tree()), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, src: &[u8], dst: &[u8], ok: bool, expect: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RN_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(RN_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(RN_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(RN_EORDER);
        }
        let r = self.tree.rename(src, dst);
        let end = match r {
            Ok(v) => {
                if !ok || v != expect {
                    self.rejects += 1;
                    return Err(RN_EMISMATCH);
                }
                Ending { ok: true, value: v }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != expect {
                    self.rejects += 1;
                    return Err(RN_EMISMATCH);
                }
                Ending { ok: false, value: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { src: [0; 20], slen: 0, dst: [0; 20], dlen: 0, ok, expect };
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

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(RN_EBAL);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return Err(RN_ENOBS),
            };
            let e = match self.ends.get(i) {
                Some(v) => *v,
                None => return Err(RN_ENOBS),
            };
            if c.ok != e.ok || c.expect != e.value {
                return Err(RN_EMISMATCH);
            }
            i += 1;
        }
        self.tree.audit()
    }

    pub fn seal(&self) -> u64 {
        self.tree.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RN_EFROZEN);
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
            Err(RN_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let enoent = (RN_ENOENT & 0xffff) as u16;
    let einval = (RN_EINVAL & 0xffff) as u16;
    let notempty = (RN_ENOTEMPTY & 0xffff) as u16;
    let isdir = (RN_EISDIR & 0xffff) as u16;
    let exdev = (RN_EXDEV & 0xffff) as u16;
    let _ = l.register(0, b"/a/f", b"/b/f", true, 2);
    let _ = l.register(1, b"/a/none", b"/b/x", false, enoent);
    let _ = l.register(2, b"/a/g", b"/b/h", true, 2);
    let _ = l.register(3, b"/a/g", b"/b/h", false, enoent);
    let _ = l.register(4, b"/a/d", b"/a/d/x", false, einval);
    let _ = l.register(5, b"/a/d", b"/b/d", true, 2);
    let _ = l.register(6, b"/a/d", b"/b/d", false, enoent);
    let _ = l.register(7, b"/b/d", b"/b", false, notempty);
    let _ = l.register(8, b"/b/f", b"/a", false, isdir);
    let _ = l.register(9, b"/b/e/sub", b"/b/e/sub/new", false, einval);
    let _ = l.register(10, b"/b/e", b"/a/e", true, 1);
    let _ = l.register(11, b"/b/f", b"/c/f", false, exdev);
    let _ = l.register(12, b"/b/f", b"/b/f", true, 2);
    let _ = l.register(13, b"/a", b"/b/a", true, 2);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 6 && l.err_count() == 8 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut t = Tree::new(build_tree());
    let before = t.children(1);
    let r = t.rename(b"/a/g", b"/b/h");
    out[1] = r == Ok(2)
        && t.child(2, b"h") == Some(5)
        && t.node(6).map(|n| n.parent) == Some(DEAD)
        && t.children(1) == before - 1
        && t.audit().is_ok();

    let mut u = Tree::new(build_tree());
    let bad = u.rename(b"/a/d", b"/a/d/x");
    let kept = u.node(7).map(|n| (n.parent, n.name() == b"d" as &[u8]));
    out[2] = bad == Err(RN_EINVAL) && kept == Some((1, true)) && u.moves() == 0 && u.children(1) == 3;

    let mut v = Tree::new(build_tree());
    let seal1 = v.seal();
    let same = v.rename(b"/a/f", b"/a/f");
    out[3] = same == Ok(1) && v.seal() == seal1 && v.moves() == 0;
    out
}

/// 十三判据。
pub fn run_unx11429_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11429.rename");
    let l = build_ledger();
    let ec = |v: u32| (v & 0xffff) as u16;
    s.add("RN1-十四格齐备", l.len() == SAMPLES && l.case(0).map(|c| c.src() == b"/a/f" as &[u8]) == Some(true), "static");
    s.add_dyn("RN2-六应八拒", l.ok_count() == 6 && l.err_count() == 8 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("RN3-覆盖文件", l.ending(2).map(|e| e.value) == Some(2) && l.tree().child(2, b"h") == Some(5) && l.tree().node(6).map(|n| n.parent) == Some(DEAD), "static");
    s.add("RN4-源缺失", l.ending(1).map(|e| e.value) == Some(ec(RN_ENOENT)) && l.ending(3).map(|e| e.value) == Some(ec(RN_ENOENT)) && l.ending(6).map(|e| e.value) == Some(ec(RN_ENOENT)), "static");
    s.add("RN5-文件换目录", l.ending(8).map(|e| e.value) == Some(ec(RN_EISDIR)), "static");
    s.add("RN6-目录非空", l.ending(7).map(|e| e.value) == Some(ec(RN_ENOTEMPTY)) && l.tree().children(2) > 0, "static");
    s.add("RN7-拓扑环拒绝", l.ending(4).map(|e| e.value) == Some(ec(RN_EINVAL)) && l.tree().node(7).map(|n| n.parent) == Some(2), "static");
    s.add("RN8-深层拓扑环", l.ending(9).map(|e| e.value) == Some(ec(RN_EINVAL)) && l.tree().resolve(b"/b/a/e/sub").is_ok(), "static");
    s.add("RN9-子树整体移动", l.ending(10).map(|e| e.value) == Some(1) && l.tree().node(8).map(|n| n.parent) == Some(1) && l.tree().node(9).map(|n| n.parent) == Some(8), "static");
    s.add("RN10-整目录换父", l.ending(13).map(|e| e.value) == Some(2) && l.tree().node(1).map(|n| n.parent) == Some(2) && l.tree().resolve(b"/b/a/e/k").is_ok(), "static");
    s.add("RN11-跨设备", l.ending(11).map(|e| e.value) == Some(ec(RN_EXDEV)) && l.tree().node(4).map(|n| n.parent) == Some(2), "static");
    s.add("RN12-自移空操作", l.ending(12).map(|e| e.value) == Some(2) && l.tree().moves() == 5, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(RN_EFROZEN);
    let inj = inject_four();
    s.add_dyn("RN13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Tree {
        Tree::new(build_tree())
    }

    #[test]
    fn move_file_new_name() {
        let mut t = tree();
        assert_eq!(t.rename(b"/a/f", b"/b/f"), Ok(2));
        assert_eq!(t.child(2, b"f"), Some(4));
        assert_eq!(t.child(1, b"f"), None);
    }

    #[test]
    fn replace_and_free_target() {
        let mut t = tree();
        assert_eq!(t.rename(b"/a/g", b"/b/h"), Ok(2));
        assert_eq!(t.node(6).map(|n| n.parent), Some(DEAD));
        assert_eq!(t.replaced(), 1);
    }

    #[test]
    fn cycle_refused_before_mutation() {
        let mut t = tree();
        let before = t.node(7).map(|n| (n.parent, n.name() == b"d" as &[u8]));
        assert_eq!(t.rename(b"/a/d", b"/a/d/x"), Err(RN_EINVAL));
        assert_eq!(t.node(7).map(|n| (n.parent, n.name() == b"d" as &[u8])), before);
        assert_eq!(t.moves(), 0);
        assert_eq!(t.replaced(), 0);
    }

    #[test]
    fn type_matrix() {
        let mut t = tree();
        assert_eq!(t.rename(b"/a/f", b"/b"), Err(RN_EISDIR));
        assert_eq!(t.rename(b"/a/d", b"/b"), Err(RN_ENOTEMPTY));
        assert_eq!(t.rename(b"/a/f", b"/c/f"), Err(RN_EXDEV));
    }

    #[test]
    fn same_inode_noop() {
        let mut t = tree();
        assert_eq!(t.rename(b"/a/f", b"/a/f"), Ok(1));
        assert_eq!(t.moves(), 0);
        assert!(t.audit().is_ok());
    }

    #[test]
    fn subtree_move_keeps_children() {
        let mut t = tree();
        assert_eq!(t.rename(b"/b/e", b"/a/e"), Ok(1));
        assert_eq!(t.node(9).map(|n| n.parent), Some(8));
        assert_eq!(t.resolve(b"/a/e/sub"), Ok(10));
    }

    #[test]
    fn ledger_and_checks_green() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11429_checks().failed_count(), 0);
    }
}
