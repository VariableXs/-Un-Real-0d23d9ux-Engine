//! UNX-F11484 符号链接展开与 ELOOP（0x20e10 段）
//!
//! 符号链接的目标是一段路径文本。解析器遇到链接时必须展开目标：绝对
//! 目标从根目录重新开始，相对目标以“链接所在的目录”为基准，而不是以
//! 当前工作目录或链接自身为基准。展开后剩余的路径分量继续在目标节点上
//! 解析，因此链接可以指向目录并接续子路径，也可以指向文件从而让后续
//! 分量得到 `ENOTDIR`。
//!
//! 链接展开次数受预算限制。自指链接、两节点环、九节点长链都会在预算
//! 耗尽时返回 `ELOOP`；七节点链在预算之内，正常解析到目标文件。悬空
//! 目标返回 `ENOENT`。`.` 与 `..` 都要求当前节点是目录，`..` 在根处
//! 钳制；被解析的每一段都基于真实父链接，不存在字符串层面的特判。
//!
//! 不变量：每次成功解析的 `hops` 等于实际展开的链接数且不超过预算；
//! 返回节点必须是表中有效节点；错误结果不携带节点；链接目标为空视为
//! 悬空；相对目标一定带有合法的父目录。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20e10;
pub const SY_EID: u32 = SEG_BASE + 0x01;
pub const SY_EORDER: u32 = SEG_BASE + 0x02;
pub const SY_EDUP: u32 = SEG_BASE + 0x03;
pub const SY_EFROZEN: u32 = SEG_BASE + 0x04;
pub const SY_ENOBS: u32 = SEG_BASE + 0x05;
pub const SY_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const SY_EBAL: u32 = SEG_BASE + 0x07;
pub const SY_ESEAL: u32 = SEG_BASE + 0x08;
pub const SY_ELOOP: u32 = SEG_BASE + 0x09;
pub const SY_ENOENT: u32 = SEG_BASE + 0x0a;
pub const SY_ENOTDIR: u32 = SEG_BASE + 0x0b;
pub const SY_EINVAL: u32 = SEG_BASE + 0x0c;

pub const SAMPLES: usize = 15;
pub const NAME_CAP: usize = 8;
pub const TARGET_CAP: usize = 16;
pub const MAX_LINKS: u32 = 8;

pub const K_DIR: u8 = 0;
pub const K_FILE: u8 = 1;
pub const K_LINK: u8 = 2;

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

/// 节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: u32,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub parent: i32,
    pub kind: u8,
    pub target: [u8; TARGET_CAP],
    pub tlen: u8,
}

fn fill(buf: &mut [u8], s: &[u8]) -> u8 {
    let n = if s.len() > buf.len() { buf.len() } else { s.len() };
    let mut i = 0usize;
    while i < n {
        if let Some(&b) = s.get(i) {
            if let Some(slot) = buf.get_mut(i) {
                *slot = b;
            }
        }
        i += 1;
    }
    n as u8
}

/// 文件系统树。
pub struct Tree {
    pub nodes: Vec<Node>,
}

impl Tree {
    fn push(&mut self, name: &[u8], parent: i32, kind: u8, target: &[u8]) -> u32 {
        let id = self.nodes.len() as u32;
        let mut nb = [0u8; NAME_CAP];
        let nlen = fill(&mut nb, name);
        let mut tb = [0u8; TARGET_CAP];
        let tlen = fill(&mut tb, target);
        self.nodes.push(Node { id, name: nb, nlen, parent, kind, target: tb, tlen });
        id
    }

    /// 构造包含普通文件、目录与多种链接的树。
    pub fn base() -> Tree {
        let mut t = Tree { nodes: Vec::new() };
        t.push(b"", -1, K_DIR, b"");
        t.push(b"a", 0, K_DIR, b"");
        t.push(b"b", 0, K_DIR, b"");
        t.push(b"f", 0, K_FILE, b"");
        t.push(b"g", 1, K_FILE, b"");
        t.push(b"l1", 0, K_LINK, b"f");
        t.push(b"self", 0, K_LINK, b"self");
        t.push(b"c1", 0, K_LINK, b"c2");
        t.push(b"c2", 0, K_LINK, b"c1");
        t.push(b"up", 1, K_LINK, b"../f");
        t.push(b"abs", 1, K_LINK, b"/a/g");
        t.push(b"chain1", 0, K_LINK, b"chain2");
        t.push(b"chain2", 0, K_LINK, b"chain3");
        t.push(b"chain3", 0, K_LINK, b"chain4");
        t.push(b"chain4", 0, K_LINK, b"chain5");
        t.push(b"chain5", 0, K_LINK, b"chain6");
        t.push(b"chain6", 0, K_LINK, b"chain7");
        t.push(b"chain7", 0, K_LINK, b"f");
        t.push(b"dangle", 0, K_LINK, b"ghost");
        t.push(b"dirlink", 0, K_LINK, b"a");
        t.push(b"long1", 0, K_LINK, b"long2");
        t.push(b"long2", 0, K_LINK, b"long3");
        t.push(b"long3", 0, K_LINK, b"long4");
        t.push(b"long4", 0, K_LINK, b"long5");
        t.push(b"long5", 0, K_LINK, b"long6");
        t.push(b"long6", 0, K_LINK, b"long7");
        t.push(b"long7", 0, K_LINK, b"long8");
        t.push(b"long8", 0, K_LINK, b"long9");
        t.push(b"long9", 0, K_LINK, b"f");
        t
    }

    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    fn child_of(&self, dir: u32, comp: &[u8]) -> Option<u32> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.parent == (dir as i32)
                    && (n.nlen as usize) == comp.len()
                    && n.name.get(0..comp.len()).map(|s| s == comp).unwrap_or(false)
                {
                    return Some(n.id);
                }
            }
            i += 1;
        }
        None
    }

    fn walk(&self, base: u32, path: &[u8], budget: &mut u32) -> Result<u32, u32> {
        let mut cur = base;
        let mut idx = 0usize;
        while idx < path.len() {
            while idx < path.len() && path.get(idx) == Some(&b'/') {
                idx += 1;
            }
            if idx >= path.len() {
                break;
            }
            let start = idx;
            while idx < path.len() && path.get(idx) != Some(&b'/') {
                idx += 1;
            }
            let comp = match path.get(start..idx) {
                Some(c) => c,
                None => return Err(SY_ENOENT),
            };
            let here = match self.nodes.get(cur as usize) {
                Some(v) => v,
                None => return Err(SY_ENOENT),
            };
            if comp == b"." {
                if here.kind != K_DIR {
                    return Err(SY_ENOTDIR);
                }
                continue;
            }
            if comp == b".." {
                if here.kind != K_DIR {
                    return Err(SY_ENOTDIR);
                }
                if here.parent >= 0 {
                    cur = here.parent as u32;
                }
                continue;
            }
            if here.kind != K_DIR {
                return Err(SY_ENOTDIR);
            }
            let child = match self.child_of(cur, comp) {
                Some(v) => v,
                None => return Err(SY_ENOENT),
            };
            let (kind, parent, target, tlen) = match self.nodes.get(child as usize) {
                Some(n) => (n.kind, n.parent, n.target, n.tlen),
                None => return Err(SY_ENOENT),
            };
            if kind != K_LINK {
                cur = child;
                continue;
            }
            *budget += 1;
            if *budget > MAX_LINKS {
                return Err(SY_ELOOP);
            }
            let tslice = match target.get(0..(tlen as usize)) {
                Some(s) => s,
                None => return Err(SY_ENOENT),
            };
            if tslice.is_empty() {
                return Err(SY_ENOENT);
            }
            let target_base = if tslice.first() == Some(&b'/') {
                0
            } else if parent >= 0 {
                parent as u32
            } else {
                0
            };
            cur = self.walk(target_base, tslice, budget)?;
        }
        Ok(cur)
    }

    /// 从根解析路径，返回节点与展开次数。
    pub fn resolve(&self, path: &[u8]) -> Result<(u32, u32), u32> {
        if path.is_empty() {
            return Err(SY_EINVAL);
        }
        let mut budget = 0u32;
        let node = self.walk(0, path, &mut budget)?;
        Ok((node, budget))
    }
}

/// 执行场景。
pub fn run_scen(id: u8) -> Result<(u32, u32), u32> {
    let t = Tree::base();
    let path: &[u8] = match id {
        0 => b"f",
        1 => b"l1",
        2 => b"self",
        3 => b"c1",
        4 => b"a/up",
        5 => b"a/abs",
        6 => b"chain1",
        7 => b"long1",
        8 => b"dangle",
        9 => b"dirlink",
        10 => b"dirlink/g",
        11 => b"l1/x",
        12 => b"a/../a/up",
        13 => b"missing",
        14 => b"a/../f",
        _ => return Err(SY_EID),
    };
    t.resolve(path)
}

/// 样本行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub scen: u8,
    pub expect_ok: bool,
    pub node: i32,
    pub hops: u32,
    pub code: u16,
}

/// 实际结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub node: i32,
    pub hops: u32,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger {
            samples: Vec::new(),
            outcomes: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    pub fn register(
        &mut self,
        slot: u8,
        scen: u8,
        ok: bool,
        node: i32,
        hops: u32,
        code: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SY_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(SY_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(SY_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(SY_EORDER);
        }
        let s = Sample { scen, expect_ok: ok, node, hops, code };
        let out = match run_scen(scen) {
            Ok((n, h)) => {
                if !ok || (n as i32) != node || h != hops {
                    return Err(SY_EMISMATCH);
                }
                Outcome { ok: true, node: n as i32, hops: h, code: 0 }
            }
            Err(e) => {
                let low = (e & 0xffff) as u16;
                if ok || low != code {
                    return Err(SY_EMISMATCH);
                }
                Outcome { ok: false, node: -1, hops: 0, code: low }
            }
        };
        self.samples.push(s);
        self.outcomes.push(out);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.samples.len()
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

    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(SY_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => *v,
                None => return Err(SY_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => *v,
                None => return Err(SY_ENOBS),
            };
            if s.expect_ok != o.ok {
                return Err(SY_EMISMATCH);
            }
            match run_scen(s.scen) {
                Ok((n, h)) => {
                    if !o.ok || (n as i32) != o.node || h != o.hops {
                        return Err(SY_EMISMATCH);
                    }
                    if h > MAX_LINKS {
                        return Err(SY_EBAL);
                    }
                }
                Err(e) => {
                    let low = (e & 0xffff) as u16;
                    if o.ok || low != o.code {
                        return Err(SY_EMISMATCH);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SY_EFROZEN);
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
                h = fold(h, u64::from(o.ok));
                h = fold(h, o.node as u64);
                h = fold(h, o.hops as u64);
                h = fold(h, o.code as u64);
            }
            if let Some(s) = self.samples.get(i) {
                h = fold(h, s.scen as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal()
            && fresh.ok_count() == self.ok_count()
            && fresh.err_count() == self.err_count()
        {
            Ok(fresh.seal())
        } else {
            Err(SY_ESEAL)
        }
    }
}

/// 十五行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eloop = (SY_ELOOP & 0xffff) as u16;
    let enoent = (SY_ENOENT & 0xffff) as u16;
    let enotdir = (SY_ENOTDIR & 0xffff) as u16;
    let _ = l.register(0, 0, true, 3, 0, 0);
    let _ = l.register(1, 1, true, 3, 1, 0);
    let _ = l.register(2, 2, false, -1, 0, eloop);
    let _ = l.register(3, 3, false, -1, 0, eloop);
    let _ = l.register(4, 4, true, 3, 1, 0);
    let _ = l.register(5, 5, true, 4, 1, 0);
    let _ = l.register(6, 6, true, 3, 7, 0);
    let _ = l.register(7, 7, false, -1, 0, eloop);
    let _ = l.register(8, 8, false, -1, 0, enoent);
    let _ = l.register(9, 9, true, 1, 1, 0);
    let _ = l.register(10, 10, true, 4, 1, 0);
    let _ = l.register(11, 11, false, -1, 0, enotdir);
    let _ = l.register(12, 12, true, 3, 1, 0);
    let _ = l.register(13, 13, false, -1, 0, enoent);
    let _ = l.register(14, 14, true, 3, 0, 0);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.ok_count() == 9 && l.err_count() == 6 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let twice = l.freeze() == Err(SY_EFROZEN);
    out[0] = base && root != 0 && rep == Ok(root) && twice;

    let t = Tree::base();
    out[1] = t.resolve(b"f") == Ok((3, 0))
        && t.resolve(b"l1") == Ok((3, 1))
        && t.resolve(b"a/abs") == Ok((4, 1))
        && t.resolve(b"a/up") == Ok((3, 1));

    out[2] = t.resolve(b"self") == Err(SY_ELOOP)
        && t.resolve(b"c1") == Err(SY_ELOOP)
        && t.resolve(b"long1") == Err(SY_ELOOP)
        && t.resolve(b"dangle") == Err(SY_ENOENT)
        && t.resolve(b"chain1") == Ok((3, 7));

    out[3] = t.resolve(b"dirlink") == Ok((1, 1))
        && t.resolve(b"dirlink/g") == Ok((4, 1))
        && t.resolve(b"l1/x") == Err(SY_ENOTDIR)
        && t.resolve(b"a/up") == Ok((3, 1))
        && t.resolve(b"a/../f") == Ok((3, 0));
    out
}

/// 十三项判据。
pub fn run_unx11484_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11484_symlink_loop");
    let l = build_ledger();
    s.add(
        "SY1-十五行齐备",
        l.len() == SAMPLES && l.sample(0).map(|x| x.scen) == Some(0),
        "static",
    );
    s.add_dyn(
        "SY2-九应六拒",
        l.ok_count() == 9 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "SY3-普通文件零展开",
        run_scen(0) == Ok((3, 0)),
        "static",
    );
    s.add(
        "SY4-单层链接",
        run_scen(1) == Ok((3, 1)),
        "static",
    );
    s.add(
        "SY5-自指链接",
        run_scen(2) == Err(SY_ELOOP),
        "static",
    );
    s.add(
        "SY6-双节点环",
        run_scen(3) == Err(SY_ELOOP),
        "static",
    );
    s.add(
        "SY7-相对目标基于链接目录",
        run_scen(4) == Ok((3, 1)),
        "static",
    );
    s.add(
        "SY8-绝对目标从根开始",
        run_scen(5) == Ok((4, 1)),
        "static",
    );
    s.add(
        "SY9-预算边界与超限",
        run_scen(6) == Ok((3, 7)) && run_scen(7) == Err(SY_ELOOP),
        "static",
    );
    s.add(
        "SY10-悬空链接",
        run_scen(8) == Err(SY_ENOENT),
        "static",
    );
    s.add(
        "SY11-链接到目录并接续",
        run_scen(9) == Ok((1, 1)) && run_scen(10) == Ok((4, 1)),
        "static",
    );
    s.add(
        "SY12-文件后接分量",
        run_scen(11) == Err(SY_ENOTDIR) && run_scen(12) == Ok((3, 1)),
        "static",
    );
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(SY_EFROZEN);
    s.add_dyn(
        "SY13-冻结重放与四路",
        root != 0 && rep == Ok(root) && again && inject_four() == [true; 4],
        &format!("root={:016x}", root),
    );
    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("ok={} err={} rows={}", l.ok_count(), l.err_count(), l.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_link_follows_once() {
        let t = Tree::base();
        assert_eq!(t.resolve(b"l1"), Ok((3, 1)));
    }

    #[test]
    fn relative_and_absolute_targets() {
        let t = Tree::base();
        assert_eq!(t.resolve(b"a/up"), Ok((3, 1)));
        assert_eq!(t.resolve(b"a/abs"), Ok((4, 1)));
    }

    #[test]
    fn cycles_hit_loop_limit() {
        let t = Tree::base();
        assert_eq!(t.resolve(b"self"), Err(SY_ELOOP));
        assert_eq!(t.resolve(b"c1"), Err(SY_ELOOP));
        assert_eq!(t.resolve(b"long1"), Err(SY_ELOOP));
    }

    #[test]
    fn chain_within_budget_resolves() {
        let t = Tree::base();
        assert_eq!(t.resolve(b"chain1"), Ok((3, 7)));
    }

    #[test]
    fn dangling_and_wrong_type() {
        let t = Tree::base();
        assert_eq!(t.resolve(b"dangle"), Err(SY_ENOENT));
        assert_eq!(t.resolve(b"l1/x"), Err(SY_ENOTDIR));
    }

    #[test]
    fn directory_link_continues() {
        let t = Tree::base();
        assert_eq!(t.resolve(b"dirlink/g"), Ok((4, 1)));
        assert_eq!(t.resolve(b"a/../f"), Ok((3, 0)));
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 9);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        let s = run_unx11484_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
