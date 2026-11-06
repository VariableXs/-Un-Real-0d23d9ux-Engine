//! UNX-F11462 rm -r 递归删除顺序（0x20cb0 段）
//!
//! `rm -r` 的正确性不在“删掉多少”，而在“按什么顺序删”。目录只能在其所有
//! 后代消失之后才能被移除，否则 `rmdir` 会看到非空目录。本模型把一次删除
//! 规划成 `Plan`：一个严格的**后序**节点序列、释放的块数、释放的 inode 数。
//! 序列由显式父链 DFS 生成，子节点按 id 升序访问，因此同一棵树每次规划得到
//! 同一条序列——这是可重放性的基础。
//!
//! 删除是**全有或全无**的：只要子树里有一个挂载点或一个被打开（busy）的
//! 节点，整个计划在第一步之前就被拒绝，`present` 位一个也不改。这条原子性
//! 让“删到一半失败”不可能出现，也解释了为什么 `EBUSY` 格执行后块数不变。
//! `-f` 只吞掉“目标不存在”，不改变其他错误；目录不带 `-r` 是 `EISDIR`，
//! `rmdir` 遇到非空是 `ENOTEMPTY`，两条语义不能互相顶替。
//!
//! 不变量：任一 present 节点的父节点必须 present（根除外）；空目录的
//! present 子节点数为零；释放块数等于被删节点块数之和。审计逐节点重算这三条，
//! 因此“删了父留下子”这种半截状态会被当场抓住。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20cb0;
pub const RM_EID: u32 = SEG_BASE + 0x01;
pub const RM_EORDER: u32 = SEG_BASE + 0x02;
pub const RM_EDUP: u32 = SEG_BASE + 0x03;
pub const RM_EFROZEN: u32 = SEG_BASE + 0x04;
pub const RM_ENOBS: u32 = SEG_BASE + 0x05;
pub const RM_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const RM_EBAL: u32 = SEG_BASE + 0x07;
pub const RM_ESEAL: u32 = SEG_BASE + 0x08;
pub const RM_ENOENT: u32 = SEG_BASE + 0x09;
pub const RM_EISDIR: u32 = SEG_BASE + 0x0a;
pub const RM_ENOTDIR: u32 = SEG_BASE + 0x0b;
pub const RM_ENOTEMPTY: u32 = SEG_BASE + 0x0c;
pub const RM_EBUSY: u32 = SEG_BASE + 0x0d;
pub const RM_EPERM: u32 = SEG_BASE + 0x0e;
pub const RM_EBADF: u32 = SEG_BASE + 0x0f;

/// 目录。
pub const K_DIR: u8 = 0;
/// 普通文件。
pub const K_REG: u8 = 1;
/// 仅删除，目录不带递归即拒绝。
pub const OP_RM: u8 = 1;
/// rmdir 语义，只接受空目录。
pub const OP_RMDIR: u8 = 2;
/// 强制：目标不存在视为成功。
pub const F_FORCE: u8 = 1;
/// 递归。
pub const F_RECUR: u8 = 2;

pub const NAME_CAP: usize = 8;
pub const PATH_CAP: usize = 20;
pub const NODES: usize = 14;
pub const SAMPLES: usize = 14;

pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
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

fn take<const N: usize>(buf: &[u8; N], len: u8) -> &[u8] {
    let n = len as usize;
    if n > N {
        return &[];
    }
    match buf.get(..n) {
        Some(v) => v,
        None => &[],
    }
}

/// 树节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: u16,
    pub parent: u16,
    pub kind: u8,
    pub blocks: u32,
    pub present: bool,
    pub mount: bool,
    pub busy: bool,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
}

impl Node {
    /// 构造。
    pub fn make(id: u16, parent: u16, kind: u8, blocks: u32, name: &[u8]) -> Node {
        let (b, n) = fill::<NAME_CAP>(name);
        Node { id, parent, kind, blocks, present: true, mount: false, busy: false, name: b, nlen: n }
    }
    /// 名字。
    pub fn name(&self) -> &[u8] {
        take(&self.name, self.nlen)
    }
}

/// 删除计划。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub order: Vec<u16>,
    pub blocks: u32,
}

/// 树与执行器。
pub struct Warden {
    nodes: Vec<Node>,
    errors: u32,
}

impl Warden {
    /// 构造。
    pub fn new(nodes: Vec<Node>) -> Warden {
        Warden { nodes, errors: 0 }
    }
    /// 节点。
    pub fn node(&self, id: u16) -> Option<Node> {
        if (id as usize) >= self.nodes.len() {
            return None;
        }
        self.nodes.get(id as usize).copied()
    }
    /// 错误数。
    pub fn errors(&self) -> u32 {
        self.errors
    }
    /// 找子项。
    pub fn child(&self, parent: u16, name: &[u8]) -> Option<u16> {
        let mut i = 0;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.present && n.parent == parent && n.name() == name {
                    return Some(n.id);
                }
            }
            i += 1;
        }
        None
    }
    /// 解析路径。
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
                None => return Err(RM_ENOENT),
            };
            let n = match self.node(cur) {
                Some(v) => v,
                None => return Err(RM_ENOENT),
            };
            if n.kind != K_DIR {
                return Err(RM_ENOTDIR);
            }
            match self.child(cur, comp) {
                Some(id) => cur = id,
                None => return Err(RM_ENOENT),
            }
        }
        Ok(cur)
    }
    /// 是否有 present 子项。
    pub fn has_children(&self, id: u16) -> bool {
        let mut i = 0;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.present && n.parent == id && n.id != id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
    /// 后序收集。
    pub fn postorder(&self, id: u16, out: &mut Vec<u16>) -> bool {
        if out.len() >= NODES {
            return false;
        }
        let mut k = 0u16;
        while (k as usize) < self.nodes.len() {
            if let Some(n) = self.nodes.get(k as usize) {
                if n.present && n.parent == id && n.id != id {
                    if !self.postorder(n.id, out) {
                        return false;
                    }
                }
            }
            k += 1;
        }
        out.push(id);
        true
    }
    /// 子树是否触碰挂载点或 busy 节点。
    fn blocked(&self, id: u16, seen_mount: &mut bool, seen_busy: &mut bool) {
        if let Some(n) = self.node(id) {
            if n.mount {
                *seen_mount = true;
            }
            if n.busy {
                *seen_busy = true;
            }
        }
        let mut i = 0;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.present && n.parent == id && n.id != id {
                    self.blocked(n.id, seen_mount, seen_busy);
                }
            }
            i += 1;
        }
    }
    /// 规划删除。
    pub fn plan(&self, path: &[u8], op: u8, flags: u8) -> Result<Plan, u32> {
        let id = match self.resolve(path) {
            Ok(v) => v,
            Err(e) => {
                if e == RM_ENOENT && flags & F_FORCE != 0 {
                    return Ok(Plan { order: Vec::new(), blocks: 0 });
                }
                return Err(e);
            }
        };
        let n = match self.node(id) {
            Some(v) => v,
            None => return Err(RM_ENOENT),
        };
        if id == 0 {
            return Err(RM_EBUSY);
        }
        let mut mount = false;
        let mut busy = false;
        self.blocked(id, &mut mount, &mut busy);
        if mount || busy {
            return Err(RM_EBUSY);
        }
        if n.kind == K_DIR && op == OP_RM && flags & F_RECUR == 0 {
            return Err(RM_EISDIR);
        }
        if n.kind != K_DIR && op == OP_RMDIR {
            return Err(RM_ENOTDIR);
        }
        if n.kind == K_DIR && op == OP_RMDIR && self.has_children(id) {
            return Err(RM_ENOTEMPTY);
        }
        let mut order = Vec::new();
        let recursive = flags & F_RECUR != 0;
        if recursive {
            if !self.postorder(id, &mut order) {
                return Err(RM_EBADF);
            }
        } else {
            order.push(id);
        }
        let mut blocks = 0u32;
        let mut i = 0;
        while i < order.len() {
            if let Some(x) = order.get(i) {
                if let Some(v) = self.node(*x) {
                    blocks = blocks.saturating_add(v.blocks);
                }
            }
            i += 1;
        }
        Ok(Plan { order, blocks })
    }
    /// 执行规划（先规划后落盘，原子）。
    pub fn remove(&mut self, path: &[u8], op: u8, flags: u8) -> Result<Plan, u32> {
        let p = match self.plan(path, op, flags) {
            Ok(v) => v,
            Err(e) => {
                self.errors += 1;
                return Err(e);
            }
        };
        let mut i = 0;
        while i < p.order.len() {
            if let Some(id) = p.order.get(i) {
                if let Some(n) = self.nodes.get_mut(*id as usize) {
                    n.present = false;
                }
            }
            i += 1;
        }
        Ok(p)
    }
    /// 审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.nodes.is_empty() {
            return Err(RM_EBAL);
        }
        let mut i = 0;
        while i < self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(v) => v,
                None => return Err(RM_ENOBS),
            };
            if n.present && n.id != 0 {
                match self.node(n.parent) {
                    Some(p) => {
                        if !p.present {
                            return Err(RM_EBAL);
                        }
                    }
                    None => return Err(RM_EBAL),
                }
            }
            i += 1;
        }
        Ok(())
    }
    /// 票面签名。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                h = fold(h, n.id as u64);
                h = fold(h, n.parent as u64);
                h = fold(h, u64::from(n.present));
                h = fold(h, n.blocks as u64);
                let mut k = 0;
                while k < n.nlen as usize && k < NAME_CAP {
                    h ^= n.name[k] as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    k += 1;
                }
            }
            i += 1;
        }
        fold(h, self.errors as u64)
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub op: u8,
    pub flags: u8,
    pub path: [u8; PATH_CAP],
    pub plen: u8,
    pub expect_ok: bool,
    pub expect_n: u16,
    pub expect_blocks: u16,
}

impl Sample {
    /// 路径。
    pub fn path(&self) -> &[u8] {
        take(&self.path, self.plen)
    }
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub steps: u16,
    pub blocks: u16,
}

/// 账本。
pub struct Ledger {
    w: Warden,
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 构造。
    pub fn new() -> Ledger {
        Ledger {
            w: Warden::new(build_nodes()),
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }
    /// 登记。
    pub fn register(
        &mut self,
        slot: u8,
        op: u8,
        flags: u8,
        path: &[u8],
        ok: bool,
        n: u16,
        blocks: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RM_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(RM_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(RM_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(RM_EORDER);
        }
        if path.is_empty() || path.len() > PATH_CAP {
            self.rejects += 1;
            return Err(RM_EBADF);
        }
        let (pb, pl) = fill::<PATH_CAP>(path);
        let s = Sample { op, flags, path: pb, plen: pl, expect_ok: ok, expect_n: n, expect_blocks: blocks };
        let out = match self.w.remove(path, op, flags) {
            Ok(p) => {
                if !ok || p.order.len() != n as usize || p.blocks != blocks as u32 {
                    return Err(RM_EMISMATCH);
                }
                Outcome { ok: true, steps: p.order.len() as u16, blocks: blocks }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != n {
                    return Err(RM_EMISMATCH);
                }
                Outcome { ok: false, steps: 0, blocks: 0 }
            }
        };
        self.samples.push(s);
        self.outcomes.push(out);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.samples.len()
    }
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
    pub fn sample(&self, i: usize) -> Option<Sample> {
        self.samples.get(i).copied()
    }
    pub fn outcome(&self, i: usize) -> Option<Outcome> {
        self.outcomes.get(i).copied()
    }
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
    pub fn err_count(&self) -> usize {
        self.outcomes.len().saturating_sub(self.ok_count())
    }
    pub fn w(&self) -> &Warden {
        &self.w
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(RM_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(RM_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => v,
                None => return Err(RM_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(RM_EMISMATCH);
            }
            if s.expect_ok && (s.expect_n != o.steps || s.expect_blocks != o.blocks) {
                return Err(RM_EMISMATCH);
            }
            i += 1;
        }
        self.w.audit()
    }
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RM_EFROZEN);
        }
        self.root = self.w.seal();
        self.frozen = true;
        Ok(self.root)
    }
    pub fn root(&self) -> u64 {
        self.root
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let sig = fresh.w.seal();
        if sig != self.w.seal() || fresh.ok_count() != self.ok_count() {
            return Err(RM_ESEAL);
        }
        Ok(sig)
    }
}

/// 节点表。
pub fn build_nodes() -> Vec<Node> {
    let mut v = Vec::new();
    v.push(Node::make(0, 0, K_DIR, 0, b""));
    v.push(Node::make(1, 0, K_DIR, 0, b"r"));
    v.push(Node::make(2, 1, K_DIR, 0, b"a"));
    v.push(Node::make(3, 2, K_REG, 4, b"f"));
    v.push(Node::make(4, 2, K_DIR, 0, b"g"));
    v.push(Node::make(5, 4, K_REG, 2, b"h"));
    v.push(Node::make(6, 1, K_DIR, 0, b"b"));
    v.push(Node::make(7, 6, K_REG, 8, b"x"));
    v.push(Node::make(8, 1, K_DIR, 0, b"c"));
    v.push(Node::make(9, 8, K_DIR, 0, b"d"));
    v.push(Node::make(10, 9, K_REG, 1, b"e"));
    v.push(Node::make(11, 1, K_DIR, 0, b"lonely"));
    v.push(Node::make(12, 1, K_REG, 5, b"f2"));
    v.push(Node::make(13, 1, K_DIR, 0, b"z"));
    let mut w = Warden::new(v);
    if let Some(n) = w.nodes.get_mut(6) {
        n.mount = true;
    }
    if let Some(n) = w.nodes.get_mut(12) {
        n.busy = true;
    }
    w.nodes
}

/// 十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let noent = (RM_ENOENT & 0xffff) as u16;
    let eisdir = (RM_EISDIR & 0xffff) as u16;
    let enotdir = (RM_ENOTDIR & 0xffff) as u16;
    let notempty = (RM_ENOTEMPTY & 0xffff) as u16;
    let busy = (RM_EBUSY & 0xffff) as u16;
    let _ = l.register(0, OP_RM, 0, b"/r/a/g", false, eisdir, 0);
    let _ = l.register(1, OP_RM, F_RECUR, b"/r/a/g", true, 2, 2);
    let _ = l.register(2, OP_RM, F_RECUR, b"/r/a/g", false, noent, 0);
    let _ = l.register(3, OP_RM, F_RECUR | F_FORCE, b"/r/a/g", true, 0, 0);
    let _ = l.register(4, OP_RMDIR, 0, b"/r/a", false, notempty, 0);
    let _ = l.register(5, OP_RMDIR, 0, b"/r/lonely", true, 1, 0);
    let _ = l.register(6, OP_RMDIR, 0, b"/r/a/f", false, enotdir, 0);
    let _ = l.register(7, OP_RM, F_RECUR, b"/r/b", false, busy, 0);
    let _ = l.register(8, OP_RM, 0, b"/r/f2", false, busy, 0);
    let _ = l.register(9, OP_RM, F_RECUR, b"/r/c", true, 3, 1);
    let _ = l.register(10, OP_RM, F_RECUR | F_FORCE, b"/r/c", true, 0, 0);
    let _ = l.register(11, OP_RM, F_RECUR, b"/", false, busy, 0);
    let _ = l.register(12, OP_RM, 0, b"/r/none", false, noent, 0);
    let _ = l.register(13, OP_RM, F_RECUR, b"/r/a/f", true, 1, 4);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 6 && l.err_count() == 8 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(RM_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let w = Warden::new(build_nodes());
    let mut order = Vec::new();
    let ok = w.postorder(1, &mut order);
    let mut valid = ok && order.len() == 13;
    let mut i = 0;
    while i < order.len() {
        if let Some(id) = order.get(i) {
            if *id != 1 {
                let par = w.node(*id).map(|n| n.parent);
                let mut found = false;
                let mut j = i + 1;
                while j < order.len() {
                    if Some(order[j]) == par {
                        found = true;
                    }
                    j += 1;
                }
                if !found {
                    valid = false;
                }
            }
        }
        i += 1;
    }
    out[1] = valid && order.last().copied() == Some(1);

    let mut a = Warden::new(build_nodes());
    let before = a.node(7).map(|n| n.present);
    let r = a.remove(b"/r/b", OP_RM, F_RECUR);
    out[2] = r == Err(RM_EBUSY)
        && a.node(7).map(|n| n.present) == before
        && a.node(6).map(|n| n.present) == Some(true)
        && a.audit().is_ok();

    let mut f = Warden::new(build_nodes());
    let q1 = f.remove(b"/r/none", OP_RM, F_FORCE);
    let q2 = f.remove(b"/r/none", OP_RM, 0);
    let q3 = f.remove(b"/r/a/g", OP_RMDIR, 0);
    let q4 = f.remove(b"/r/a/g", OP_RM, F_RECUR);
    out[3] = q1 == Ok(Plan { order: Vec::new(), blocks: 0 })
        && q2 == Err(RM_ENOENT)
        && q3 == Err(RM_ENOTEMPTY)
        && q4.map(|p| p.blocks) == Ok(2)
        && f.audit().is_ok();
    out
}

/// 十三项判据。
pub fn run_unx11462_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11462.rm_r");
    let l = build_ledger();
    s.add(
        "RM1-十四格齐备",
        l.len() == SAMPLES && l.sample(0).map(|x| x.path() == b"/r/a/g" as &[u8]) == Some(true),
        "static",
    );
    s.add_dyn(
        "RM2-六应八拒",
        l.ok_count() == 6 && l.err_count() == 8 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "RM3-递归两步后序",
        l.outcome(1).map(|o| (o.ok, o.steps, o.blocks)) == Some((true, 2, 2))
            && l.w().node(5).map(|n| n.present) == Some(false)
            && l.w().node(4).map(|n| n.present) == Some(false),
        "static",
    );
    s.add(
        "RM4-目录不带递归即 EISDIR",
        l.outcome(0).map(|o| o.ok) == Some(false) && l.w().node(4).map(|n| n.present) == Some(false),
        "static",
    );
    s.add(
        "RM5-rmdir 非空拒绝",
        l.outcome(4).map(|o| o.ok) == Some(false) && l.w().node(2).map(|n| n.present) == Some(true),
        "static",
    );
    s.add(
        "RM6-rmdir 空目录成功",
        l.outcome(5).map(|o| (o.ok, o.steps)) == Some((true, 1))
            && l.w().node(11).map(|n| n.present) == Some(false),
        "static",
    );
    s.add(
        "RM7-rmdir 文件 ENOTDIR",
        l.outcome(6).map(|o| o.ok) == Some(false) && l.w().node(12).map(|n| n.present) == Some(true),
        "static",
    );
    s.add(
        "RM8-挂载点整树拒绝",
        l.outcome(7).map(|o| o.ok) == Some(false)
            && l.w().node(6).map(|n| n.present) == Some(true)
            && l.w().node(7).map(|n| n.present) == Some(true),
        "static",
    );
    s.add(
        "RM9-busy 文件拒绝",
        l.outcome(8).map(|o| o.ok) == Some(false) && l.w().node(12).map(|n| n.present) == Some(true),
        "static",
    );
    s.add(
        "RM10-三层子树三步",
        l.outcome(9).map(|o| (o.steps, o.blocks)) == Some((3, 1))
            && l.w().node(10).map(|n| n.present) == Some(false),
        "static",
    );
    s.add(
        "RM11-强制对缺失零步",
        l.outcome(3).map(|o| (o.ok, o.steps)) == Some((true, 0))
            && l.outcome(10).map(|o| (o.ok, o.steps)) == Some((true, 0)),
        "static",
    );
    s.add(
        "RM12-根与递归文件边界",
        l.outcome(11).map(|o| o.ok) == Some(false)
            && l.outcome(13).map(|o| (o.steps, o.blocks)) == Some((1, 4)),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(RM_EFROZEN);
    s.add_dyn(
        "RM13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} blocks={}", l.ok_count(), l.err_count(), l.outcome(1).map(|o| o.blocks).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Warden {
        Warden::new(build_nodes())
    }

    #[test]
    fn postorder_children_first() {
        let w = fresh();
        let mut v = Vec::new();
        assert!(w.postorder(1, &mut v));
        assert_eq!(v.last().copied(), Some(1));
        let g = v.iter().position(|x| *x == 4).unwrap_or(0);
        let h = v.iter().position(|x| *x == 5).unwrap_or(0);
        assert!(h < g);
    }

    #[test]
    fn recursive_frees_subtree() {
        let mut w = fresh();
        let p = w.remove(b"/r/a/g", OP_RM, F_RECUR);
        assert_eq!(p.map(|x| (x.order.len(), x.blocks)), Ok((2, 2)));
        assert_eq!(w.node(5).map(|n| n.present), Some(false));
        assert_eq!(w.node(4).map(|n| n.present), Some(false));
        assert!(w.audit().is_ok());
    }

    #[test]
    fn mount_refusal_is_atomic() {
        let mut w = fresh();
        assert_eq!(w.remove(b"/r/b", OP_RM, F_RECUR), Err(RM_EBUSY));
        assert_eq!(w.node(6).map(|n| n.present), Some(true));
        assert_eq!(w.node(7).map(|n| n.present), Some(true));
    }

    #[test]
    fn force_only_swallows_missing() {
        let mut w = fresh();
        assert_eq!(w.remove(b"/r/none", OP_RM, F_FORCE).map(|p| p.order.len()), Ok(0));
        assert_eq!(w.remove(b"/r/none", OP_RM, 0), Err(RM_ENOENT));
        assert_eq!(w.remove(b"/r/f2", OP_RM, F_FORCE), Err(RM_EBUSY));
    }

    #[test]
    fn dir_semantics_split() {
        let mut w = fresh();
        assert_eq!(w.remove(b"/r/a", OP_RM, 0), Err(RM_EISDIR));
        assert_eq!(w.remove(b"/r/a", OP_RMDIR, 0), Err(RM_ENOTEMPTY));
        assert_eq!(w.remove(b"/r/a/f", OP_RMDIR, 0), Err(RM_ENOTDIR));
        assert_eq!(w.remove(b"/r/lonely", OP_RMDIR, 0).map(|p| p.order.len()), Ok(1));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 6);
        assert_eq!(l.err_count(), 8);
        assert!(l.audit().is_ok());
        let s = run_unx11462_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
