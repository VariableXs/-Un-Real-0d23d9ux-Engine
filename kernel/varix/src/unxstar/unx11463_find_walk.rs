//! UNX-F11463 find 遍历谓词机（0x20cc0 段）
//!
//! `find` 的语义不是“列出所有文件”，而是“在下降过程中按谓词决定访问与剪枝”。
//! 本模型把一次遍历写成 `Query`：起点、最大深度、类型过滤、名字模式、是否
//! `-prune`、是否 `-xdev`。遍历器返回 `Trace`：访问序列、命中序列、成环计数。
//! 访问序列是判断正确性的主证据——命中数相同但访问了不该访问的子树，仍然是错。
//!
//! 深度与剪枝是两条独立开关。`maxdepth=0` 只访问起点；`maxdepth=n` 允许下降到
//! 第 n 层，第 n 层的目录只访问不再展开。`-prune` 只在“目录名不匹配模式”时
//! 阻止下降，目录本身仍算一次访问；因此 `-name f -prune` 在根名不等于 f 时
//! 访问数恒为 1，这是它和“不匹配就跳过”最容易混淆的地方。
//!
//! `-xdev` 不阻止访问挂载点本身，只阻止越过它继续下降，所以挂载点会在访问
//! 序列里出现、其子项不会。别名项（目录项指向另一个已访问目录）会先计入访问，
//! 再因“解析后的 id 已在 seen 集”而记一次成环并停止下降——环不会被无限展开，
//! 也不会被静默忽略，成环计数是显式的第三维账。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20cc0;
pub const FD_EID: u32 = SEG_BASE + 0x01;
pub const FD_EORDER: u32 = SEG_BASE + 0x02;
pub const FD_EDUP: u32 = SEG_BASE + 0x03;
pub const FD_EFROZEN: u32 = SEG_BASE + 0x04;
pub const FD_ENOBS: u32 = SEG_BASE + 0x05;
pub const FD_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const FD_EBAL: u32 = SEG_BASE + 0x07;
pub const FD_ESEAL: u32 = SEG_BASE + 0x08;
pub const FD_ENOENT: u32 = SEG_BASE + 0x09;
pub const FD_ENOTDIR: u32 = SEG_BASE + 0x0a;
pub const FD_EINVAL: u32 = SEG_BASE + 0x0b;
pub const FD_ELOOP: u32 = SEG_BASE + 0x0c;
pub const FD_EBADF: u32 = SEG_BASE + 0x0d;
pub const FD_EOVERFLOW: u32 = SEG_BASE + 0x0e;
pub const FD_ENAMETOOLONG: u32 = SEG_BASE + 0x0f;

pub const K_ANY: u8 = 0;
pub const K_DIR: u8 = 1;
pub const K_REG: u8 = 2;
pub const NAME_CAP: usize = 8;
pub const PATH_CAP: usize = 20;
pub const NODES: usize = 12;
pub const SAMPLES: usize = 12;
pub const MAX_DEPTH: u8 = 9;

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
    pub alias: u16,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
}

impl Node {
    pub fn make(id: u16, parent: u16, kind: u8, dev: u8, name: &[u8]) -> Node {
        let (b, n) = fill::<NAME_CAP>(name);
        Node { id, parent, kind, dev, alias: 0, name: b, nlen: n }
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
    pub kind: u8,
    pub pat: [u8; NAME_CAP],
    pub plen: u8,
    pub prune: bool,
    pub xdev: bool,
}

/// 轨迹。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    pub visited: Vec<u16>,
    pub matched: Vec<u16>,
    pub loops: u16,
}

/// 遍历器。
pub struct Finder {
    nodes: Vec<Node>,
    errors: u32,
}

impl Finder {
    pub fn new(nodes: Vec<Node>) -> Finder {
        Finder { nodes, errors: 0 }
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
                if n.parent == parent && n.name() == name && n.id != parent {
                    return Some(n.id);
                }
            }
            i += 1;
        }
        None
    }
    pub fn resolve(&self, path: &[u8]) -> Result<u16, u32> {
        if path.len() > PATH_CAP {
            return Err(FD_ENAMETOOLONG);
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
                None => return Err(FD_ENOENT),
            };
            match self.node(cur) {
                Some(n) => {
                    if n.kind != K_DIR {
                        return Err(FD_ENOTDIR);
                    }
                }
                None => return Err(FD_ENOENT),
            }
            match self.child(cur, comp) {
                Some(id) => cur = id,
                None => return Err(FD_ENOENT),
            }
        }
        Ok(cur)
    }
    fn matches(&self, n: &Node, q: &Query) -> bool {
        let rid = if n.alias == 0 { n.id } else { n.alias };
        let rk = match self.node(rid) {
            Some(a) => a.kind,
            None => n.kind,
        };
        let want_k = match q.kind {
            K_DIR => K_DIR,
            K_REG => K_REG,
            _ => rk,
        };
        if q.kind != K_ANY && want_k != rk {
            return false;
        }
        if q.plen == 0 {
            return true;
        }
        n.name() == cut(&q.pat, q.plen)
    }
    /// 遍历。
    pub fn walk(&self, q: &Query) -> Result<Trace, u32> {
        let start = self.resolve(cut(&q.root, q.rlen))?;
        let st = match self.node(start) {
            Some(v) => v,
            None => return Err(FD_ENOENT),
        };
        if q.maxdepth > MAX_DEPTH {
            return Err(FD_EOVERFLOW);
        }
        let mut visited = Vec::new();
        let mut matched = Vec::new();
        let mut seen: Vec<u16> = Vec::new();
        let mut loops = 0u16;
        let mut stack: Vec<(u16, u8)> = Vec::new();
        stack.push((start, 0u8));
        while let Some((id, depth)) = stack.pop() {
            if visited.len() >= NODES * 2 {
                return Err(FD_EOVERFLOW);
            }
            let n = match self.node(id) {
                Some(v) => v,
                None => return Err(FD_ENOENT),
            };
            let rid = if n.alias == 0 { id } else { n.alias };
            let rn = match self.node(rid) {
                Some(v) => v,
                None => return Err(FD_EBADF),
            };
            visited.push(id);
            if self.matches(&n, q) {
                matched.push(id);
            }
            if n.kind != K_DIR {
                continue;
            }
            if depth >= q.maxdepth {
                continue;
            }
            if q.prune && q.plen > 0 && n.name() != cut(&q.pat, q.plen) {
                continue;
            }
            if q.xdev && rn.dev != st.dev {
                continue;
            }
            if seen.iter().any(|x| *x == rid) {
                loops = loops.saturating_add(1);
                continue;
            }
            seen.push(rid);
            let mut kids: Vec<u16> = Vec::new();
            let mut i = 0;
            while i < self.nodes.len() {
                if let Some(c) = self.nodes.get(i) {
                    if c.parent == rid && c.id != rid {
                        kids.push(c.id);
                    }
                }
                i += 1;
            }
            while let Some(k) = kids.pop() {
                stack.push((k, depth + 1));
            }
        }
        Ok(Trace { visited, matched, loops })
    }
    /// 父子次序审计：访问序列中父必先于子。
    pub fn ordered(&self, t: &Trace) -> bool {
        let mut i = 1;
        while i < t.visited.len() {
            let id = match t.visited.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let par = match self.node(id) {
                Some(n) => n.parent,
                None => return false,
            };
            let mut seen = false;
            let mut j = 0;
            while j < i {
                if t.visited.get(j) == Some(&par) {
                    seen = true;
                }
                j += 1;
            }
            if !seen {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub q: Query,
    pub expect_ok: bool,
    pub expect_vis: u16,
    pub expect_match: u16,
    pub expect_loops: u16,
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
    pub vis: u16,
    pub matched: u16,
    pub loops: u16,
}

/// 账本。
pub struct Ledger {
    f: Finder,
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            f: Finder::new(build_nodes()),
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }
    pub fn q(&self, root: &[u8], maxdepth: u8, kind: u8, pat: &[u8], prune: bool, xdev: bool) -> Query {
        let (rb, rl) = fill::<PATH_CAP>(root);
        let (pb, pl) = fill::<NAME_CAP>(pat);
        Query { root: rb, rlen: rl, maxdepth, kind, pat: pb, plen: pl, prune, xdev }
    }
    pub fn register(&mut self, slot: u8, q: Query, ok: bool, vis: u16, m: u16, loops: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FD_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(FD_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(FD_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(FD_EORDER);
        }
        if q.rlen == 0 || (q.rlen as usize) > PATH_CAP || (q.plen as usize) > NAME_CAP {
            self.rejects += 1;
            return Err(FD_EINVAL);
        }
        let s = Sample { q, expect_ok: ok, expect_vis: vis, expect_match: m, expect_loops: loops };
        let out = match self.f.walk(&q) {
            Ok(t) => {
                if !ok || t.visited.len() != vis as usize || t.matched.len() != m as usize || t.loops != loops {
                    return Err(FD_EMISMATCH);
                }
                Outcome { ok: true, vis: vis, matched: m, loops }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != vis {
                    return Err(FD_EMISMATCH);
                }
                Outcome { ok: false, vis: 0, matched: 0, loops: 0 }
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
    pub fn f(&self) -> &Finder {
        &self.f
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(FD_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(FD_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => v,
                None => return Err(FD_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(FD_EMISMATCH);
            }
            if s.expect_ok && (s.expect_vis != o.vis || s.expect_match != o.matched || s.expect_loops != o.loops) {
                return Err(FD_EMISMATCH);
            }
            i += 1;
        }
        Ok(())
    }
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FD_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.samples.len() {
            if let Some(s) = self.samples.get(i) {
                let mut k = 0;
                while k < s.q.rlen as usize && k < PATH_CAP {
                    h ^= s.q.root[k] as u64;
                    h = h.wrapping_mul(FNV_PRIME);
                    k += 1;
                }
                h = fold(h, s.expect_vis as u64);
                h = fold(h, s.expect_match as u64);
                h = fold(h, s.expect_loops as u64);
            }
            i += 1;
        }
        fold(h, self.f.errors as u64)
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
            Err(FD_ESEAL)
        }
    }
}

/// 节点表。
pub fn build_nodes() -> Vec<Node> {
    let mut v = Vec::new();
    v.push(Node::make(0, 0, K_DIR, 0, b""));
    v.push(Node::make(1, 0, K_DIR, 0, b"a"));
    v.push(Node::make(2, 1, K_REG, 0, b"f"));
    v.push(Node::make(3, 1, K_DIR, 0, b"g"));
    v.push(Node::make(4, 3, K_REG, 0, b"h"));
    v.push(Node::make(5, 0, K_DIR, 1, b"b"));
    v.push(Node::make(6, 5, K_REG, 1, b"x"));
    v.push(Node::make(7, 0, K_DIR, 0, b"c"));
    v.push(Node::make(8, 7, K_REG, 0, b"f"));
    v.push(Node::make(9, 7, K_DIR, 0, b"y"));
    v.push(Node::make(10, 9, K_REG, 0, b"z"));
    v.push(Node::make(11, 1, K_DIR, 0, b"self"));
    let mut f = Finder::new(v);
    if let Some(n) = f.nodes.get_mut(11) {
        n.alias = 1;
    }
    f.nodes
}

/// 十二格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let noent = (FD_ENOENT & 0xffff) as u16;
    let q0 = l.q(b"/", 0, K_ANY, b"", false, false);
    let q1 = l.q(b"/a", 9, K_ANY, b"", false, false);
    let q2 = l.q(b"/a", 1, K_ANY, b"", false, false);
    let q3 = l.q(b"/a", 9, K_REG, b"", false, false);
    let q4 = l.q(b"/a", 9, K_ANY, b"h", false, false);
    let q5 = l.q(b"/a", 9, K_ANY, b"a", true, false);
    let q6 = l.q(b"/", 3, K_ANY, b"", false, true);
    let q7 = l.q(b"/a/none", 9, K_ANY, b"", false, false);
    let q8 = l.q(b"/a", 0, K_REG, b"", false, false);
    let q9 = l.q(b"/a", 9, K_DIR, b"", false, false);
    let q10 = l.q(b"/a", 9, K_ANY, b"f", true, false);
    let q11 = l.q(b"/", 1, K_ANY, b"", false, false);
    let _ = l.register(0, q0, true, 1, 1, 0);
    let _ = l.register(1, q1, true, 5, 5, 1);
    let _ = l.register(2, q2, true, 4, 4, 0);
    let _ = l.register(3, q3, true, 5, 2, 1);
    let _ = l.register(4, q4, true, 5, 1, 1);
    let _ = l.register(5, q5, true, 4, 1, 0);
    let _ = l.register(6, q6, true, 11, 11, 1);
    let _ = l.register(7, q7, false, noent, 0, 0);
    let _ = l.register(8, q8, true, 1, 0, 0);
    let _ = l.register(9, q9, true, 5, 3, 1);
    let _ = l.register(10, q10, true, 1, 0, 0);
    let _ = l.register(11, q11, true, 4, 4, 0);
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
    let twice = l.freeze() == Err(FD_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let f = Finder::new(build_nodes());
    let qa = Query { root: fill::<PATH_CAP>(b"/a").0, rlen: 2, maxdepth: 9, kind: K_ANY, pat: [0; NAME_CAP], plen: 0, prune: false, xdev: false };
    let t = f.walk(&qa);
    let ok = match t {
        Ok(v) => v.visited.len() == 5 && v.loops == 1 && f.ordered(&v),
        Err(_) => false,
    };
    out[1] = ok;

    let qn = Query { root: fill::<PATH_CAP>(b"/a").0, rlen: 2, maxdepth: 9, kind: K_ANY, pat: fill::<NAME_CAP>(b"a").0, plen: 1, prune: true, xdev: false };
    let qm = Query { root: fill::<PATH_CAP>(b"/a").0, rlen: 2, maxdepth: 9, kind: K_ANY, pat: fill::<NAME_CAP>(b"a").0, plen: 1, prune: false, xdev: false };
    let tn = f.walk(&qn);
    let tm = f.walk(&qm);
    out[2] = match (tn, tm) {
        (Ok(a), Ok(b)) => a.visited.len() == 4 && b.visited.len() == 5 && b.loops == 1,
        _ => false,
    };

    let qx = Query { root: fill::<PATH_CAP>(b"/").0, rlen: 1, maxdepth: 9, kind: K_ANY, pat: [0; NAME_CAP], plen: 0, prune: false, xdev: true };
    let tx = f.walk(&qx);
    let mounted = f.child(0, b"b");
    out[3] = match tx {
        Ok(v) => v.visited.len() == 11 && mounted.map(|m| v.visited.iter().any(|x| *x == m)) == Some(true) && v.visited.iter().all(|x| *x != 6),
        Err(_) => false,
    };
    out
}

/// 十三项判据。
pub fn run_unx11463_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11463.find");
    let l = build_ledger();
    s.add(
        "FD1-十二格齐备",
        l.len() == SAMPLES && l.sample(0).map(|x| x.root() == b"/" as &[u8]) == Some(true),
        "static",
    );
    s.add_dyn(
        "FD2-十一应一拒",
        l.ok_count() == 11 && l.err_count() == 1 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "FD3-深度零只访问起点",
        l.outcome(0).map(|o| (o.vis, o.matched)) == Some((1, 1))
            && l.outcome(8).map(|o| (o.vis, o.matched)) == Some((1, 0)),
        "static",
    );
    s.add(
        "FD4-深度一截断孙辈",
        l.outcome(2).map(|o| (o.vis, o.matched)) == Some((4, 4))
            && l.outcome(11).map(|o| o.vis) == Some(4),
        "static",
    );
    s.add(
        "FD5-类型过滤不改访问",
        l.outcome(3).map(|o| (o.vis, o.matched)) == Some((5, 2))
            && l.outcome(9).map(|o| (o.vis, o.matched)) == Some((5, 3)),
        "static",
    );
    s.add(
        "FD6-名字命中唯一",
        l.outcome(4).map(|o| (o.vis, o.matched)) == Some((5, 1)),
        "static",
    );
    s.add(
        "FD7-剪枝止于不匹配目录",
        l.outcome(5).map(|o| (o.vis, o.matched)) == Some((4, 1))
            && l.outcome(10).map(|o| (o.vis, o.matched)) == Some((1, 0)),
        "static",
    );
    s.add(
        "FD8-xdev 访问挂载点不越界",
        l.outcome(6).map(|o| (o.vis, o.matched)) == Some((11, 11)),
        "static",
    );
    s.add(
        "FD9-别名成环计数显式",
        l.outcome(1).map(|o| o.loops) == Some(1) && l.outcome(6).map(|o| o.loops) == Some(1),
        "static",
    );
    s.add(
        "FD10-缺失起点拒绝",
        l.outcome(7).map(|o| o.ok) == Some(false) && l.sample(7).map(|x| x.expect_vis) == Some((FD_ENOENT & 0xffff) as u16),
        "static",
    );
    s.add(
        "FD11-访问序父先于子",
        {
            let q = l.q(b"/a", 9, K_ANY, b"", false, false);
            match l.f().walk(&q) {
                Ok(t) => l.f().ordered(&t) && t.visited.len() == 5,
                Err(_) => false,
            }
        },
        "static",
    );
    s.add(
        "FD12-名字表不被改写",
        l.sample(1).map(|x| x.q.maxdepth) == Some(9)
            && l.sample(5).map(|x| x.q.prune) == Some(true)
            && l.sample(6).map(|x| x.q.xdev) == Some(true),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(FD_EFROZEN);
    s.add_dyn(
        "FD13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} loops={}", l.ok_count(), l.err_count(), l.outcome(1).map(|o| o.loops).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f() -> Finder {
        Finder::new(build_nodes())
    }

    fn q(root: &[u8], depth: u8, kind: u8, pat: &[u8], prune: bool, xdev: bool) -> Query {
        Query {
            root: fill::<PATH_CAP>(root).0,
            rlen: root.len() as u8,
            maxdepth: depth,
            kind,
            pat: fill::<NAME_CAP>(pat).0,
            plen: pat.len() as u8,
            prune,
            xdev,
        }
    }

    #[test]
    fn depth_limits_descent() {
        let t = f().walk(&q(b"/a", 1, K_ANY, b"", false, false));
        assert_eq!(t.map(|x| (x.visited.len(), x.loops)), Ok((4, 0)));
    }

    #[test]
    fn prune_stops_at_root() {
        let t = f().walk(&q(b"/a", 9, K_ANY, b"f", true, false));
        assert_eq!(t.map(|x| x.visited.len()), Ok(1));
    }

    #[test]
    fn xdev_visits_mount_root_only() {
        let t = f().walk(&q(b"/", 9, K_ANY, b"", false, true));
        match t {
            Ok(v) => {
                assert_eq!(v.visited.len(), 11);
                assert!(!v.visited.iter().any(|x| *x == 6));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn alias_records_loop() {
        let t = f().walk(&q(b"/a", 9, K_ANY, b"", false, false));
        assert_eq!(t.map(|x| x.loops), Ok(1));
    }

    #[test]
    fn missing_root_refused() {
        assert_eq!(f().walk(&q(b"/a/none", 9, K_ANY, b"", false, false)), Err(FD_ENOENT));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 11);
        assert_eq!(l.err_count(), 1);
        assert!(l.audit().is_ok());
        let s = run_unx11463_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
