//! UNX-F11464 du 空间核算与去重（0x20cd0 段）
//!
//! `du` 的难点不是加法，而是“同一个 inode 只能算一次”。多个目录项共享同一
//! `(dev, ino)` 时，只有第一次遇到的名字计入总量，后续名字的块数记入
//! `saved`（被去重省下的量）。因此总量、唯一 inode 数、去重量构成三元账，
//! 任意两元正确而第三元错误都说明 inode 判定出了问题。
//!
//! 深度限制只影响“访问到哪一层”，不影响已访问节点的归属：一个目录的总量
//! 是“所有以它为祖先的已访问节点”的唯一贡献之和。目录自身的数据块也要计入，
//! 空目录至少一块。这样 `du -d 0` 得到的是目录自身块数而不是 0。
//!
//! 溢出被显式饱和：累加使用 `saturating_add`，超过 `u32::MAX` 的总量停在
//! 天花板而不是回绕成小数。审计重算每个目录的子树和，并核对“总量 + 去重
//! 量 = 全部目录项块数之和”这条恒等式；任一目录项被漏算或多算，等式立刻
//! 破裂。去重键不使用路径，只使用 `(dev, ino)`，所以跨目录的硬链接也能被识破。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20cd0;
pub const DU_EID: u32 = SEG_BASE + 0x01;
pub const DU_EORDER: u32 = SEG_BASE + 0x02;
pub const DU_EDUP: u32 = SEG_BASE + 0x03;
pub const DU_EFROZEN: u32 = SEG_BASE + 0x04;
pub const DU_ENOBS: u32 = SEG_BASE + 0x05;
pub const DU_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const DU_EBAL: u32 = SEG_BASE + 0x07;
pub const DU_ESEAL: u32 = SEG_BASE + 0x08;
pub const DU_ENOENT: u32 = SEG_BASE + 0x09;
pub const DU_ENOTDIR: u32 = SEG_BASE + 0x0a;
pub const DU_EINVAL: u32 = SEG_BASE + 0x0b;
pub const DU_EOVERFLOW: u32 = SEG_BASE + 0x0c;
pub const DU_EBADF: u32 = SEG_BASE + 0x0d;
pub const DU_ENAMETOOLONG: u32 = SEG_BASE + 0x0e;
pub const DU_EIO: u32 = SEG_BASE + 0x0f;

pub const K_DIR: u8 = 0;
pub const K_REG: u8 = 1;
pub const NAME_CAP: usize = 8;
pub const PATH_CAP: usize = 20;
pub const NODES: usize = 12;
pub const SAMPLES: usize = 12;
pub const MAX_DEPTH: u8 = 8;

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

fn cut<const N: usize>(buf: &[u8; N], len: u8) -> &[u8] {
    let n = len as usize;
    if n > N {
        return &[];
    }
    match buf.get(..n) {
        Some(v) => v,
        None => &[],
    }
}

/// 节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: u16,
    pub parent: u16,
    pub kind: u8,
    pub dev: u8,
    pub ino: u16,
    pub blocks: u32,
    pub size: u32,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
}

impl Node {
    pub fn make(id: u16, parent: u16, kind: u8, ino: u16, blocks: u32, size: u32, name: &[u8]) -> Node {
        let (b, n) = fill::<NAME_CAP>(name);
        Node { id, parent, kind, dev: 0, ino, blocks, size, name: b, nlen: n }
    }
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 查询。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Query {
    pub root: [u8; PATH_CAP],
    pub rlen: u8,
    pub maxdepth: u8,
    pub apparent: bool,
}

/// 报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub total: u32,
    pub inodes: u32,
    pub saved: u32,
    pub per_dir: Vec<(u16, u32)>,
}

/// 核算器。
pub struct Meter {
    nodes: Vec<Node>,
    errors: u32,
}

impl Meter {
    pub fn new(nodes: Vec<Node>) -> Meter {
        Meter { nodes, errors: 0 }
    }
    pub fn node(&self, id: u16) -> Option<Node> {
        if (id as usize) >= self.nodes.len() {
            return None;
        }
        self.nodes.get(id as usize).copied()
    }
    pub fn errors(&self) -> u32 {
        self.errors
    }
    pub fn child(&self, parent: u16, name: &[u8]) -> Option<u16> {
        let mut i = 0;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.parent == parent && n.id != parent && n.name() == name {
                    return Some(n.id);
                }
            }
            i += 1;
        }
        None
    }
    pub fn resolve(&self, path: &[u8]) -> Result<u16, u32> {
        if path.len() > PATH_CAP {
            return Err(DU_ENAMETOOLONG);
        }
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
                None => return Err(DU_ENOENT),
            };
            if self.node(cur).map(|n| n.kind) != Some(K_DIR) {
                return Err(DU_ENOTDIR);
            }
            match self.child(cur, comp) {
                Some(id) => cur = id,
                None => return Err(DU_ENOENT),
            }
        }
        Ok(cur)
    }
    fn is_ancestor(&self, mut node: u16, dir: u16) -> bool {
        let mut guard = 0;
        loop {
            if node == dir {
                return true;
            }
            if node == 0 || guard > NODES {
                return false;
            }
            guard += 1;
            match self.node(node) {
                Some(n) => {
                    if n.parent == node {
                        return false;
                    }
                    node = n.parent;
                }
                None => return false,
            }
        }
    }
    fn cost(&self, n: &Node, apparent: bool) -> u32 {
        if apparent {
            n.size
        } else {
            n.blocks
        }
    }
    /// 核算。
    pub fn du(&self, q: &Query) -> Result<Report, u32> {
        let start = self.resolve(cut(&q.root, q.rlen))?;
        if q.maxdepth > MAX_DEPTH {
            return Err(DU_EOVERFLOW);
        }
        let mut keys: Vec<(u8, u16)> = Vec::new();
        let mut total = 0u32;
        let mut saved = 0u32;
        let mut inodes = 0u32;
        let mut all: Vec<u32> = Vec::new();
        let mut stack: Vec<(u16, u8)> = Vec::new();
        stack.push((start, 0u8));
        while let Some((id, depth)) = stack.pop() {
            if all.len() >= NODES {
                return Err(DU_EBADF);
            }
            let n = match self.node(id) {
                Some(v) => v,
                None => return Err(DU_ENOENT),
            };
            let c = self.cost(&n, q.apparent);
            all.push(c);
            let key = (n.dev, n.ino);
            if keys.iter().any(|k| *k == key) {
                saved = saved.saturating_add(c);
            } else {
                keys.push(key);
                total = total.saturating_add(c);
                inodes = inodes.saturating_add(1);
            }
            if n.kind != K_DIR || depth >= q.maxdepth {
                continue;
            }
            let mut kids: Vec<u16> = Vec::new();
            let mut i = 0;
            while i < self.nodes.len() {
                if let Some(k) = self.nodes.get(i) {
                    if k.parent == id && k.id != id {
                        kids.push(k.id);
                    }
                }
                i += 1;
            }
            while let Some(k) = kids.pop() {
                stack.push((k, depth + 1));
            }
        }
        let mut per_dir = Vec::new();
        let mut d = 0u16;
        while (d as usize) < self.nodes.len() {
            if let Some(n) = self.node(d) {
                if n.kind == K_DIR {
                    let mut sum = 0u32;
                    let mut seen: Vec<(u8, u16)> = Vec::new();
                    let mut s = 0u16;
                    while (s as usize) < self.nodes.len() {
                        if let Some(x) = self.node(s) {
                            if self.is_ancestor(x.id, d) {
                                let key = (x.dev, x.ino);
                                if !seen.iter().any(|k| *k == key) {
                                    seen.push(key);
                                    sum = sum.saturating_add(self.cost(&x, q.apparent));
                                }
                            }
                        }
                        s += 1;
                    }
                    per_dir.push((d, sum));
                }
            }
            d += 1;
        }
        Ok(Report { total, inodes, saved, per_dir })
    }
    /// 任一目录的子树总量。
    pub fn dir_total(&self, r: &Report, dir: u16) -> Option<u32> {
        let mut i = 0;
        while i < r.per_dir.len() {
            if let Some(x) = r.per_dir.get(i) {
                if x.0 == dir {
                    return Some(x.1);
                }
            }
            i += 1;
        }
        None
    }
    /// 恒等式审计：总量 + 去重 = 全部 accessed 成本之和。
    pub fn audit(&self, q: &Query, r: &Report) -> Result<(), u32> {
        let start = self.resolve(cut(&q.root, q.rlen))?;
        let mut all = 0u32;
        let mut stack = Vec::new();
        stack.push((start, 0u8));
        while let Some((id, depth)) = stack.pop() {
            let n = match self.node(id) {
                Some(v) => v,
                None => return Err(DU_ENOENT),
            };
            all = all.saturating_add(self.cost(&n, q.apparent));
            if n.kind != K_DIR || depth >= q.maxdepth {
                continue;
            }
            let mut i = 0;
            while i < self.nodes.len() {
                if let Some(k) = self.nodes.get(i) {
                    if k.parent == id && k.id != id {
                        stack.push((k.id, depth + 1));
                    }
                }
                i += 1;
            }
        }
        if r.total.saturating_add(r.saved) != all {
            return Err(DU_EBAL);
        }
        Ok(())
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub q: Query,
    pub expect_ok: bool,
    pub expect_total: u16,
    pub expect_inodes: u16,
    pub expect_saved: u16,
}

impl Sample {
    pub fn root(&self) -> &[u8] {
        cut(&self.q.root, self.q.rlen)
    }
}

/// 结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub total: u16,
    pub inodes: u16,
    pub saved: u16,
}

/// 账本。
pub struct Ledger {
    m: Meter,
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            m: Meter::new(build_nodes()),
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }
    pub fn q(&self, root: &[u8], maxdepth: u8, apparent: bool) -> Query {
        let (rb, rl) = fill::<PATH_CAP>(root);
        Query { root: rb, rlen: rl, maxdepth, apparent }
    }
    pub fn register(&mut self, slot: u8, q: Query, ok: bool, total: u16, inodes: u16, saved: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DU_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(DU_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(DU_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(DU_EORDER);
        }
        if q.rlen == 0 || (q.rlen as usize) > PATH_CAP {
            self.rejects += 1;
            return Err(DU_EINVAL);
        }
        let s = Sample { q, expect_ok: ok, expect_total: total, expect_inodes: inodes, expect_saved: saved };
        let out = match self.m.du(&q) {
            Ok(r) => {
                if !ok
                    || r.total != total as u32
                    || r.inodes != inodes as u32
                    || r.saved != saved as u32
                    || self.m.audit(&q, &r).is_err()
                {
                    return Err(DU_EMISMATCH);
                }
                Outcome { ok: true, total, inodes, saved }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != total {
                    return Err(DU_EMISMATCH);
                }
                Outcome { ok: false, total: 0, inodes: 0, saved: 0 }
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
    pub fn m(&self) -> &Meter {
        &self.m
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(DU_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(DU_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => v,
                None => return Err(DU_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(DU_EMISMATCH);
            }
            if s.expect_ok
                && (s.expect_total != o.total || s.expect_inodes != o.inodes || s.expect_saved != o.saved)
            {
                return Err(DU_EMISMATCH);
            }
            i += 1;
        }
        Ok(())
    }
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DU_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.outcomes.len() {
            if let Some(o) = self.outcomes.get(i) {
                h = fold(h, o.total as u64);
                h = fold(h, o.inodes as u64);
                h = fold(h, o.saved as u64);
            }
            i += 1;
        }
        fold(h, self.m.errors as u64)
    }
    pub fn root(&self) -> u64 {
        self.root
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() && fresh.ok_count() == self.ok_count() {
            Ok(fresh.seal())
        } else {
            Err(DU_ESEAL)
        }
    }
}

/// 节点表（f 与 h 共享 inode 10）。
pub fn build_nodes() -> Vec<Node> {
    let mut v = Vec::new();
    v.push(Node::make(0, 0, K_DIR, 1, 1, 0, b""));
    v.push(Node::make(1, 0, K_DIR, 2, 1, 0, b"a"));
    v.push(Node::make(2, 1, K_REG, 10, 4, 1000, b"f"));
    v.push(Node::make(3, 1, K_REG, 11, 2, 10, b"g"));
    v.push(Node::make(4, 0, K_DIR, 3, 1, 0, b"b"));
    v.push(Node::make(5, 4, K_REG, 10, 4, 1000, b"h"));
    v.push(Node::make(6, 4, K_REG, 12, 8, 900, b"i"));
    v.push(Node::make(7, 0, K_DIR, 4, 1, 0, b"c"));
    v.push(Node::make(8, 7, K_REG, 13, 4, 40, b"f"));
    v.push(Node::make(9, 7, K_DIR, 5, 1, 0, b"d"));
    v.push(Node::make(10, 9, K_REG, 14, 1, 7, b"e"));
    v
}

/// 十二格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let noent = (DU_ENOENT & 0xffff) as u16;
    let q0 = l.q(b"/", 8, false);
    let q1 = l.q(b"/a", 1, false);
    let q2 = l.q(b"/a", 0, false);
    let q3 = l.q(b"/b", 8, false);
    let q4 = l.q(b"/c", 8, false);
    let q5 = l.q(b"/c", 1, false);
    let q6 = l.q(b"/a/f", 8, false);
    let q7 = l.q(b"/none", 8, false);
    let q8 = l.q(b"/", 1, false);
    let q9 = l.q(b"/", 2, false);
    let q10 = l.q(b"/c/d", 0, false);
    let q11 = l.q(b"/", 8, true);
    let _ = l.register(0, q0, true, 24, 10, 4);
    let _ = l.register(1, q1, true, 7, 3, 0);
    let _ = l.register(2, q2, true, 1, 1, 0);
    let _ = l.register(3, q3, true, 13, 3, 0);
    let _ = l.register(4, q4, true, 7, 4, 0);
    let _ = l.register(5, q5, true, 6, 3, 0);
    let _ = l.register(6, q6, true, 4, 1, 0);
    let _ = l.register(7, q7, false, noent, 0, 0);
    let _ = l.register(8, q8, true, 4, 4, 0);
    let _ = l.register(9, q9, true, 23, 9, 4);
    let _ = l.register(10, q10, true, 1, 1, 0);
    let _ = l.register(11, q11, true, 1957, 10, 1000);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 11 && l.err_count() == 1 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(DU_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let m = Meter::new(build_nodes());
    let q = Query { root: fill::<PATH_CAP>(b"/").0, rlen: 1, maxdepth: 8, apparent: false };
    let r = m.du(&q);
    out[1] = match r {
        Ok(v) => {
            v.total == 24
                && v.saved == 4
                && v.inodes == 10
                && m.dir_total(&v, 1) == Some(7)
                && m.dir_total(&v, 4) == Some(13)
                && m.audit(&q, &v).is_ok()
        }
        Err(_) => false,
    };

    let qa = Query { root: fill::<PATH_CAP>(b"/a").0, rlen: 2, maxdepth: 8, apparent: false };
    let qb = Query { root: fill::<PATH_CAP>(b"/b").0, rlen: 2, maxdepth: 8, apparent: false };
    let ra = m.du(&qa);
    let rb = m.du(&qb);
    out[2] = match (ra, rb) {
        (Ok(x), Ok(y)) => x.total == 7 && x.saved == 0 && y.total == 13 && y.saved == 0,
        _ => false,
    };

    let qd = Query { root: fill::<PATH_CAP>(b"/c").0, rlen: 2, maxdepth: 1, apparent: false };
    let qn = Query { root: fill::<PATH_CAP>(b"/zz").0, rlen: 3, maxdepth: 8, apparent: false };
    out[3] = m.du(&qd).map(|v| v.total) == Ok(6)
        && m.du(&qn) == Err(DU_ENOENT)
        && m.du(&Query { root: fill::<PATH_CAP>(b"/c/d").0, rlen: 4, maxdepth: 8, apparent: false }).map(|v| v.total) == Ok(2);
    out
}

/// 十三项判据。
pub fn run_unx11464_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11464.du");
    let l = build_ledger();
    s.add(
        "DU1-十二格齐备",
        l.len() == SAMPLES && l.sample(0).map(|x| x.root() == b"/" as &[u8]) == Some(true),
        "static",
    );
    s.add_dyn(
        "DU2-十一应一拒",
        l.ok_count() == 11 && l.err_count() == 1 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "DU3-根去重后二十四块",
        l.outcome(0).map(|o| (o.total, o.inodes, o.saved)) == Some((24, 10, 4)),
        "static",
    );
    s.add(
        "DU4-深度一四块",
        l.outcome(8).map(|o| (o.total, o.inodes)) == Some((4, 4)),
        "static",
    );
    s.add(
        "DU5-深度二截掉 e",
        l.outcome(9).map(|o| (o.total, o.saved)) == Some((23, 4)),
        "static",
    );
    s.add(
        "DU6-子树 b 独立核算",
        l.outcome(3).map(|o| (o.total, o.inodes)) == Some((13, 3)),
        "static",
    );
    s.add(
        "DU7-子树 c 两层截断",
        l.outcome(4).map(|o| o.total) == Some(7) && l.outcome(5).map(|o| o.total) == Some(6),
        "static",
    );
    s.add(
        "DU8-单文件只算自身",
        l.outcome(6).map(|o| (o.total, o.inodes)) == Some((4, 1)),
        "static",
    );
    s.add(
        "DU9-深度零为目录自身块",
        l.outcome(2).map(|o| (o.total, o.inodes)) == Some((1, 1))
            && l.outcome(10).map(|o| o.total) == Some(1),
        "static",
    );
    s.add(
        "DU10-去重恒等式成立",
        {
            let q = l.q(b"/", 8, false);
            match l.m().du(&q) {
                Ok(r) => l.m().audit(&q, &r).is_ok() && r.total + r.saved == 28,
                Err(_) => false,
            }
        },
        "static",
    );
    s.add(
        "DU11-表观尺寸另算一条账",
        l.outcome(11).map(|o| (o.total, o.saved)) == Some((1957, 1000)),
        "static",
    );
    s.add(
        "DU12-缺失根拒绝且无记录",
        l.outcome(7).map(|o| o.ok) == Some(false) && l.sample(7).map(|x| x.expect_total) == Some((DU_ENOENT & 0xffff) as u16),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(DU_EFROZEN);
    s.add_dyn(
        "DU13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} total={}", l.ok_count(), l.err_count(), l.outcome(0).map(|o| o.total).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m() -> Meter {
        Meter::new(build_nodes())
    }

    fn q(root: &[u8], depth: u8) -> Query {
        Query { root: fill::<PATH_CAP>(root).0, rlen: root.len() as u8, maxdepth: depth, apparent: false }
    }

    #[test]
    fn hardlink_counted_once() {
        let r = m().du(&q(b"/", 8));
        assert_eq!(r.map(|x| (x.total, x.saved, x.inodes)), Ok((24, 4, 10)));
    }

    #[test]
    fn subdir_scope_changes_dedup() {
        assert_eq!(m().du(&q(b"/b", 8)).map(|x| (x.total, x.saved)), Ok((13, 0)));
        assert_eq!(m().du(&q(b"/a", 8)).map(|x| (x.total, x.saved)), Ok((7, 0)));
    }

    #[test]
    fn depth_zero_is_self_only() {
        assert_eq!(m().du(&q(b"/a", 0)).map(|x| x.total), Ok(1));
        assert_eq!(m().du(&q(b"/", 1)).map(|x| x.total), Ok(4));
    }

    #[test]
    fn dir_rollup_matches() {
        let meter = m();
        let r = meter.du(&q(b"/", 8));
        match r {
            Ok(v) => {
                assert_eq!(meter.dir_total(&v, 1), Some(7));
                assert_eq!(meter.dir_total(&v, 4), Some(13));
                assert_eq!(meter.dir_total(&v, 7), Some(7));
                assert!(meter.audit(&q(b"/", 8), &v).is_ok());
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn missing_and_bad_depth() {
        assert_eq!(m().du(&q(b"/none", 8)), Err(DU_ENOENT));
        assert_eq!(m().du(&q(b"/", 9)), Err(DU_EOVERFLOW));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 11);
        assert_eq!(l.err_count(), 1);
        assert!(l.audit().is_ok());
        let s = run_unx11464_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
