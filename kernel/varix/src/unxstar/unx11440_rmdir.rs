//! UNX-F11440 rmdir 删除目录（0x20b40 段）
//!
//! rmdir 的难点全在“能否删”而不是“怎么删”。模型保存一棵以父下标连接的目录树，
//! 每个节点有类型、只读标志、挂载标志与死亡标志；删除只做一次原子判定，任何一条
//! 前置条件不满足都直接返回错误并且不改动树。判定顺序为：路径解析（缺失 ENOENT、
//! 含 `.` 或 `..` 分量 EINVAL）、根目录 EBUSY、非目录 ENOTDIR、挂载点 EBUSY、
//! 当前工作目录 EBUSY、仍有存活子项 ENOTEMPTY、父目录只读 EROFS。
//!
//! 判定顺序本身是可观察语义：一个挂载点同时又是当前目录时报告 EBUSY 而不是
//! ENOTEMPTY；一个非空目录位于只读父目录下时报告 ENOTEMPTY 而不是 EROFS；根目录
//! 无论是否为空都报告 EBUSY。模型把这些组合固定进账本，防止实现者随意调换检查
//! 次序。`.` 与 `..` 在任意位置都返回 EINVAL，这是模型的保守选择：树没有硬链接，
//! 回退分量只会让删除目标变得含糊。
//!
//! 成功删除会把节点标记为死亡并清空名字，但保留槽位，父下标不变，因此死亡节点
//! 永远不会被误认为某个存活节点的父。删除不回收槽位，审计通过“存活子项计数”而
//! 不是向量长度来判断空目录。当前工作目录只能指向存活目录，切换失败不影响原值；
//! 删除当前目录本身则被 EBUSY 挡住，所以工作目录在成功路径上始终有效。
//!
//! 十六格账本六成十败：成功格覆盖空目录、嵌套空目录、只读目录自身与切换工作目录，
//! 失败格覆盖重复删除、非空、非目录、挂载点、工作目录、点分量、只读父目录、根目录
//! 与缺失路径。审计检查根存活、工作目录有效、存活节点父链完整、死亡节点名字为空、
//! 同级存活名字唯一。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20b40;
/// 编号非法。
pub const RD_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const RD_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const RD_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const RD_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const RD_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const RD_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const RD_EBAL: u32 = SEG_BASE + 0x07;
/// 节点不存在。
pub const RD_ENOENT: u32 = SEG_BASE + 0x08;
/// 不是目录。
pub const RD_ENOTDIR: u32 = SEG_BASE + 0x09;
/// 目录非空。
pub const RD_ENOTEMPTY: u32 = SEG_BASE + 0x0a;
/// 忙碌。
pub const RD_EBUSY: u32 = SEG_BASE + 0x0b;
/// 参数非法。
pub const RD_EINVAL: u32 = SEG_BASE + 0x0c;
/// 只读父目录。
pub const RD_EROFS: u32 = SEG_BASE + 0x0d;
/// 票据不一致。
pub const RD_ESEAL: u32 = SEG_BASE + 0x0e;

/// 操作：删除目录。
pub const OP_RMDIR: u8 = 1;
/// 操作：切换工作目录。
pub const OP_CHDIR: u8 = 2;
/// 目录。
pub const K_DIR: u8 = 1;
/// 普通文件。
pub const K_FILE: u8 = 2;
/// 无父哨兵。
pub const NO_PARENT: u16 = 0xffff;

/// 名字容量。
pub const NAME_CAP: usize = 8;
/// 路径容量。
pub const PATH_CAP: usize = 24;
/// 样本格数。
pub const SAMPLES: usize = 16;

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
    pub ro: bool,
    pub mounted: bool,
    pub dead: bool,
}

impl Node {
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
}

/// 目录树。
pub struct Tree {
    nodes: Vec<Node>,
    cwd: u16,
    removals: u32,
    rejects: u32,
}

impl Tree {
    pub fn new() -> Tree {
        let mut v = Vec::new();
        v.push(node(NO_PARENT, b"/", K_DIR, false));
        v.push(node(0, b"a", K_DIR, false));
        v.push(node(1, b"b", K_DIR, false));
        v.push(node(1, b"f", K_FILE, false));
        v.push(node(0, b"e", K_DIR, false));
        v.push(node(0, b"e2", K_DIR, false));
        v.push(node(0, b"e3", K_DIR, false));
        let mut mnt = node(0, b"mnt", K_DIR, false);
        mnt.mounted = true;
        v.push(mnt);
        v.push(node(0, b"cw", K_DIR, false));
        let mut ro = node(0, b"ro", K_DIR, false);
        ro.ro = true;
        v.push(ro);
        v.push(node(9, b"sub", K_DIR, false));
        v.push(node(0, b"e4", K_DIR, false));
        Tree { nodes: v, cwd: 0, removals: 0, rejects: 0 }
    }

    pub fn node(&self, i: usize) -> Option<Node> {
        match self.nodes.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn cwd(&self) -> u16 {
        self.cwd
    }

    pub fn removals(&self) -> u32 {
        self.removals
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    pub fn alive(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(d) = self.nodes.get(i) {
                if !d.dead {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn child_count(&self, i: usize) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.nodes.len() {
            if let Some(d) = self.nodes.get(k) {
                if !d.dead && d.parent as usize == i {
                    n += 1;
                }
            }
            k += 1;
        }
        n
    }

    fn child(&self, parent: usize, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(d) = self.nodes.get(i) {
                if !d.dead && d.parent as usize == parent && d.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 解析绝对路径，拒绝点分量。
    pub fn resolve(&self, path: &[u8]) -> Result<usize, u32> {
        if path.first() != Some(&b'/') {
            return Err(RD_EINVAL);
        }
        let mut cur = 0usize;
        let mut start = 1usize;
        let mut i = 1usize;
        while i <= path.len() {
            let at_end = i == path.len();
            let sep = !at_end && path.get(i) == Some(&b'/');
            if at_end || sep {
                if i > start {
                    let comp = match path.get(start..i) {
                        Some(v) => v,
                        None => return Err(RD_EINVAL),
                    };
                    if comp == b"." || comp == b".." {
                        return Err(RD_EINVAL);
                    }
                    match self.child(cur, comp) {
                        Some(k) => cur = k,
                        None => return Err(RD_ENOENT),
                    }
                }
                start = i + 1;
            }
            i += 1;
        }
        Ok(cur)
    }

    /// 切换工作目录。
    pub fn chdir(&mut self, path: &[u8]) -> Result<u16, u32> {
        let idx = match self.resolve(path) {
            Ok(v) => v,
            Err(e) => {
                self.rejects += 1;
                return Err(e);
            }
        };
        let d = match self.nodes.get(idx) {
            Some(v) => *v,
            None => {
                self.rejects += 1;
                return Err(RD_ENOENT);
            }
        };
        if d.kind != K_DIR {
            self.rejects += 1;
            return Err(RD_ENOTDIR);
        }
        if d.dead {
            self.rejects += 1;
            return Err(RD_ENOENT);
        }
        self.cwd = idx as u16;
        Ok(idx as u16)
    }

    /// 删除空目录。
    pub fn rmdir(&mut self, path: &[u8]) -> Result<u16, u32> {
        let idx = match self.resolve(path) {
            Ok(v) => v,
            Err(e) => {
                self.rejects += 1;
                return Err(e);
            }
        };
        let d = match self.nodes.get(idx) {
            Some(v) => *v,
            None => {
                self.rejects += 1;
                return Err(RD_ENOENT);
            }
        };
        if d.parent == NO_PARENT {
            self.rejects += 1;
            return Err(RD_EBUSY);
        }
        if d.kind != K_DIR {
            self.rejects += 1;
            return Err(RD_ENOTDIR);
        }
        if d.mounted {
            self.rejects += 1;
            return Err(RD_EBUSY);
        }
        if idx == self.cwd as usize {
            self.rejects += 1;
            return Err(RD_EBUSY);
        }
        if self.child_count(idx) != 0 {
            self.rejects += 1;
            return Err(RD_ENOTEMPTY);
        }
        let par = match self.nodes.get(d.parent as usize) {
            Some(v) => *v,
            None => {
                self.rejects += 1;
                return Err(RD_EBAL);
            }
        };
        if par.ro || par.kind != K_DIR {
            self.rejects += 1;
            return Err(RD_EROFS);
        }
        if let Some(n) = self.nodes.get_mut(idx) {
            n.dead = true;
            n.nlen = 0;
            n.name = [0; NAME_CAP];
        }
        self.removals += 1;
        Ok(idx as u16)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.nodes.is_empty() {
            return Err(RD_EBAL);
        }
        let root = match self.nodes.get(0) {
            Some(v) => *v,
            None => return Err(RD_ENOBS),
        };
        if root.dead || root.kind != K_DIR {
            return Err(RD_EBAL);
        }
        let cw = match self.nodes.get(self.cwd as usize) {
            Some(v) => *v,
            None => return Err(RD_EBAL),
        };
        if cw.dead || cw.kind != K_DIR {
            return Err(RD_EBAL);
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            let d = match self.nodes.get(i) {
                Some(v) => *v,
                None => return Err(RD_ENOBS),
            };
            if d.dead {
                if d.nlen != 0 {
                    return Err(RD_EBAL);
                }
            } else {
                if i != 0 {
                    let p = match self.nodes.get(d.parent as usize) {
                        Some(v) => *v,
                        None => return Err(RD_EBAL),
                    };
                    if p.dead || p.kind != K_DIR {
                        return Err(RD_EBAL);
                    }
                }
                if d.kind != K_DIR && d.kind != K_FILE {
                    return Err(RD_EBAL);
                }
                if d.nlen == 0 {
                    return Err(RD_EBAL);
                }
                let mut k = i + 1;
                while k < self.nodes.len() {
                    if let Some(e) = self.nodes.get(k) {
                        if !e.dead && e.parent == d.parent && e.name() == d.name() {
                            return Err(RD_EBAL);
                        }
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = fold(FNV_OFF, self.cwd as u64);
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(d) = self.nodes.get(i) {
                h = fold(h, d.parent as u64);
                h = fold(h, d.kind as u64);
                h = fold(h, d.ro as u64);
                h = fold(h, d.mounted as u64);
                h = fold(h, d.dead as u64);
                let mut k = 0usize;
                while k < d.nlen as usize {
                    if let Some(b) = d.name.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.removals as u64);
        fold(h, self.rejects as u64)
    }
}

fn node(parent: u16, name: &[u8], kind: u8, ro: bool) -> Node {
    let mut n = Node { parent, name: [0; NAME_CAP], nlen: 0, kind, ro, mounted: false, dead: false };
    n.nlen = put(&mut n.name, name);
    n
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub op: u8,
    pub path: [u8; PATH_CAP],
    pub plen: u8,
    pub ok: bool,
    pub removed: u16,
}

/// 结局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub removed: bool,
    pub cwd: u16,
    pub code: u16,
}

/// 账本。
pub struct Ledger {
    t: Tree,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { t: Tree::new(), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, op: u8, path: &[u8], ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RD_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(RD_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(RD_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(RD_EORDER);
        }
        let r = if op == OP_RMDIR {
            self.t.rmdir(path)
        } else if op == OP_CHDIR {
            self.t.chdir(path)
        } else {
            Err(RD_EINVAL)
        };
        let end = match r {
            Ok(v) => {
                if !ok {
                    self.rejects += 1;
                    return Err(RD_EMISMATCH);
                }
                Ending { removed: op == OP_RMDIR, cwd: v, code: 0 }
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(RD_EMISMATCH);
                }
                Ending { removed: false, cwd: self.t.cwd(), code: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { op, path: [0; PATH_CAP], plen: 0, ok, removed: end.cwd };
        c.plen = put(&mut c.path, path);
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
            return Err(RD_EBAL);
        }
        self.t.audit()
    }

    pub fn seal(&self) -> u64 {
        self.t.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RD_EFROZEN);
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
            Err(RD_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十六格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, OP_RMDIR, b"/a/b", true);
    let _ = l.register(1, OP_RMDIR, b"/a/b", false);
    let _ = l.register(2, OP_RMDIR, b"/a", false);
    let _ = l.register(3, OP_RMDIR, b"/a/f", false);
    let _ = l.register(4, OP_RMDIR, b"/mnt", false);
    let _ = l.register(5, OP_CHDIR, b"/cw", true);
    let _ = l.register(6, OP_RMDIR, b"/cw", false);
    let _ = l.register(7, OP_RMDIR, b"/cw/.", false);
    let _ = l.register(8, OP_RMDIR, b"/a/..", false);
    let _ = l.register(9, OP_RMDIR, b"/ro/sub", false);
    let _ = l.register(10, OP_RMDIR, b"/", false);
    let _ = l.register(11, OP_RMDIR, b"/e", true);
    let _ = l.register(12, OP_RMDIR, b"/e2", true);
    let _ = l.register(13, OP_RMDIR, b"/e2", false);
    let _ = l.register(14, OP_RMDIR, b"/e3", true);
    let _ = l.register(15, OP_RMDIR, b"/e4", true);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == SAMPLES && l.ok_count() == 6 && l.err_count() == 10 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut a = Tree::new();
    let r = a.rmdir(b"/e");
    out[1] = r == Ok(4) && a.node(4).map(|n| (n.dead, n.nlen)) == Some((true, 0))
        && a.alive() == 11 && a.child_count(0) == 7;

    let mut b = Tree::new();
    let nonempty = b.rmdir(b"/a");
    let file = b.rmdir(b"/a/f");
    let mounted = b.rmdir(b"/mnt");
    out[2] = nonempty == Err(RD_ENOTEMPTY) && file == Err(RD_ENOTDIR) && mounted == Err(RD_EBUSY);

    let mut c = Tree::new();
    let ch = c.chdir(b"/cw");
    let busy = c.rmdir(b"/cw");
    let root = c.rmdir(b"/");
    let dot = c.rmdir(b"/cw/..");
    out[3] = ch == Ok(8) && busy == Err(RD_EBUSY) && root == Err(RD_EBUSY)
        && dot == Err(RD_EINVAL) && c.cwd() == 8;
    out
}

/// 十三判据。
pub fn run_unx11440_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11440.rmdir");
    let l = build_ledger();
    s.add("RD1-十六格齐备", l.len() == SAMPLES, "static");
    s.add_dyn("RD2-六成十拒", l.ok_count() == 6 && l.err_count() == 10 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("RD3-嵌套删除", l.ending(0).map(|e| e.removed) == Some(true) && l.tree().node(2).map(|n| n.dead) == Some(true), "static");
    s.add("RD4-重复删除", l.ending(1).map(|e| e.code) == Some((RD_ENOENT & 0xffff) as u16), "static");
    s.add("RD5-非空目录", l.ending(2).map(|e| e.code) == Some((RD_ENOTEMPTY & 0xffff) as u16), "static");
    s.add("RD6-非目录", l.ending(3).map(|e| e.code) == Some((RD_ENOTDIR & 0xffff) as u16), "static");
    s.add("RD7-挂载点", l.ending(4).map(|e| e.code) == Some((RD_EBUSY & 0xffff) as u16), "static");
    s.add("RD8-工作目录", l.ending(5).map(|e| (e.cwd, e.code)) == Some((8, 0)) && l.ending(6).map(|e| e.code) == Some((RD_EBUSY & 0xffff) as u16), "static");
    s.add("RD9-点分量", l.ending(7).map(|e| e.code) == Some((RD_EINVAL & 0xffff) as u16) && l.ending(8).map(|e| e.code) == Some((RD_EINVAL & 0xffff) as u16), "static");
    s.add("RD10-只读父目录", l.ending(9).map(|e| e.code) == Some((RD_EROFS & 0xffff) as u16), "static");
    s.add("RD11-根目录", l.ending(10).map(|e| e.code) == Some((RD_EBUSY & 0xffff) as u16), "static");
    s.add("RD12-连续删除与计数", l.ending(11).map(|e| e.removed) == Some(true) && l.ending(12).map(|e| e.removed) == Some(true) && l.ending(13).map(|e| e.code) == Some((RD_ENOENT & 0xffff) as u16) && l.ending(14).map(|e| e.removed) == Some(true) && l.ending(15).map(|e| e.removed) == Some(true) && l.tree().removals() == 5 && l.tree().alive() == 7 && l.tree().cwd() == 8, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(RD_EFROZEN);
    let inj = inject_four();
    s.add_dyn("RD13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_dir_removed() {
        let mut t = Tree::new();
        assert_eq!(t.rmdir(b"/e"), Ok(4));
        assert_eq!(t.node(4).map(|n| n.dead), Some(true));
        assert_eq!(t.rmdir(b"/e"), Err(RD_ENOENT));
    }

    #[test]
    fn nonempty_rejected() {
        let mut t = Tree::new();
        assert_eq!(t.rmdir(b"/a"), Err(RD_ENOTEMPTY));
        assert_eq!(t.rmdir(b"/a/f"), Err(RD_ENOTDIR));
    }

    #[test]
    fn mount_and_cwd_busy() {
        let mut t = Tree::new();
        assert_eq!(t.rmdir(b"/mnt"), Err(RD_EBUSY));
        assert_eq!(t.chdir(b"/cw"), Ok(8));
        assert_eq!(t.rmdir(b"/cw"), Err(RD_EBUSY));
        assert_eq!(t.rmdir(b"/"), Err(RD_EBUSY));
    }

    #[test]
    fn dot_components_rejected() {
        let mut t = Tree::new();
        assert_eq!(t.rmdir(b"/a/."), Err(RD_EINVAL));
        assert_eq!(t.rmdir(b"/a/.."), Err(RD_EINVAL));
        assert_eq!(t.rmdir(b"a"), Err(RD_EINVAL));
    }

    #[test]
    fn read_only_parent() {
        let mut t = Tree::new();
        assert_eq!(t.rmdir(b"/ro/sub"), Err(RD_EROFS));
        assert_eq!(t.rmdir(b"/ro"), Err(RD_ENOTEMPTY));
    }

    #[test]
    fn last_component_semantics() {
        let mut t = Tree::new();
        assert_eq!(t.rmdir(b"/e/"), Ok(4));
        assert_eq!(t.rmdir(b"//e2//"), Ok(5));
        assert_eq!(t.rmdir(b"/none"), Err(RD_ENOENT));
    }

    #[test]
    fn ledger_green() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11440_checks().failed_count(), 0);
    }
}
