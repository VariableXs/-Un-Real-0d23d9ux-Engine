//! UNX-F11448 规范化路径（0x20bc0 段）
//!
//! getcwd 与 canonicalize 的共同内核是逐分量消解：空分量与点号直接丢弃，双点号
//! 在栈顶回退一格但绝不越过根，普通名字必须存在于当前目录，符号链接则把自身
//! 目标重新插回待处理串的头部。这样得到的绝对路径不含 `.`、`..`、重复斜杠或
//! 任何符号链接，长度也不会超过 PATH_MAX。
//!
//! 两个错误必须分清：中间分量不是目录报 ENOTDIR，链接目标指向文件却还要求继续
//! 下钻同样报 ENOTDIR；链接展开次数超过上限报 ELOOP，而不是 ENOTDIR。名字分量
//! 超过单个文件名上限报 ENAMETOOLONG，拼接结果超过路径上限也报同一个错误。
//! 空路径没有可规范化的对象，返回 ENOENT。
//!
//! 相对路径从账本指定的工作目录节点出发；绝对路径一律从根出发。审计重建整棵
//! 目录树并对每一行重放解析，比较结果串的每一个字节或错误码。封印把输入、起点
//! 与输出一起折叠，冻结后登记被拒绝，重放重建账本比较签名。四路注入覆盖根、
//! 相对链接展开、循环拒绝与非目录拒绝，防止实现把双点号简单删除或在链接处死循环。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20bc0;
/// 编号非法。
pub const CW_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const CW_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const CW_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const CW_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const CW_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const CW_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const CW_EBAL: u32 = SEG_BASE + 0x07;
/// 名字不存在。
pub const CW_ENOENT: u32 = SEG_BASE + 0x08;
/// 不是目录。
pub const CW_ENOTDIR: u32 = SEG_BASE + 0x09;
/// 链接层数过多。
pub const CW_ELOOP: u32 = SEG_BASE + 0x0a;
/// 名字过长。
pub const CW_ENAMETOOLONG: u32 = SEG_BASE + 0x0b;
/// 票据不一致。
pub const CW_ESEAL: u32 = SEG_BASE + 0x0c;

/// 单名上限。
pub const NAME_MAX: usize = 8;
/// 路径上限。
pub const PATH_MAX: usize = 64;
/// 链接展开上限。
pub const LINK_MAX: u32 = 8;
/// 账本格数。
pub const ROWS: usize = 15;

/// 树节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub parent: i16,
    pub name: [u8; NAME_MAX],
    pub nlen: u8,
    pub dir: bool,
    pub link: bool,
    pub target: [u8; PATH_MAX],
    pub tlen: u8,
}

impl Node {
    pub fn name_bytes(&self) -> &[u8] {
        let n = self.nlen as usize;
        if n > NAME_MAX {
            return &[];
        }
        match self.name.get(..n) {
            Some(v) => v,
            None => &[],
        }
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

fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if x != y {
            return false;
        }
        i += 1;
    }
    true
}

/// 目录树。
pub struct Fs {
    pub nodes: Vec<Node>,
}

impl Fs {
    pub fn fresh() -> Fs {
        let mut nodes: Vec<Node> = Vec::new();
        let mut make = |parent: i16, name: &[u8], dir: bool, link: bool, target: &[u8]| -> u16 {
            let mut n = Node {
                parent,
                name: [0; NAME_MAX],
                nlen: 0,
                dir,
                link,
                target: [0; PATH_MAX],
                tlen: 0,
            };
            n.nlen = put(&mut n.name, name);
            n.tlen = put(&mut n.target, target);
            nodes.push(n);
            (nodes.len() - 1) as u16
        };
        let root = make(-1, b"/", true, false, b"");
        let a = make(root as i16, b"a", true, false, b"");
        let _b = make(a as i16, b"b", true, false, b"");
        let _c = make(_b as i16, b"c", false, false, b"");
        let _link = make(root as i16, b"link", true, true, b"a/b");
        let _rel = make(root as i16, b"rel", true, true, b"a");
        let _loop = make(root as i16, b"loop", true, true, b"/loop");
        let _x = make(root as i16, b"x", false, true, b"a/b/c");
        let _up = make(root as i16, b"up", true, true, b"..");
        drop(make);
        Fs { nodes }
    }

    fn child(&self, parent: usize, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.parent >= 0 && n.parent as usize == parent && same(n.name_bytes(), name) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn depth(&self, i: usize) -> u32 {
        let mut d = 0u32;
        let mut cur = i;
        while cur != 0 {
            let p = match self.nodes.get(cur) {
                Some(n) => n.parent,
                None => break,
            };
            if p < 0 {
                break;
            }
            cur = p as usize;
            d += 1;
        }
        d
    }

    fn to_path(&self, stack: &[usize]) -> Result<Vec<u8>, u32> {
        let mut out: Vec<u8> = Vec::new();
        out.push(b'/');
        let mut i = 1usize;
        while i < stack.len() {
            let n = match self.nodes.get(stack[i]) {
                Some(v) => *v,
                None => return Err(CW_EBAL),
            };
            if i > 1 {
                out.push(b'/');
            }
            let mut k = 0usize;
            while k < n.nlen as usize {
                if let Some(b) = n.name.get(k) {
                    out.push(*b);
                }
                k += 1;
            }
            if out.len() > PATH_MAX {
                return Err(CW_ENAMETOOLONG);
            }
            i += 1;
        }
        Ok(out)
    }

    /// 从工作目录节点解析路径。
    pub fn resolve(&self, cwd: usize, path: &[u8]) -> Result<Vec<u8>, u32> {
        if path.is_empty() {
            return Err(CW_ENOENT);
        }
        if path.len() > PATH_MAX {
            return Err(CW_ENAMETOOLONG);
        }
        let mut stack: Vec<usize> = Vec::new();
        if path.get(0) == Some(&b'/') {
            stack.push(0);
        } else {
            if cwd >= self.nodes.len() {
                return Err(CW_ENOENT);
            }
            let mut chain: Vec<usize> = Vec::new();
            let mut cur = cwd;
            loop {
                chain.push(cur);
                let p = match self.nodes.get(cur) {
                    Some(n) => n.parent,
                    None => return Err(CW_ENOENT),
                };
                if p < 0 {
                    break;
                }
                cur = p as usize;
            }
            let mut i = chain.len();
            while i > 0 {
                i -= 1;
                if let Some(v) = chain.get(i) {
                    stack.push(*v);
                }
            }
            if stack.is_empty() {
                stack.push(0);
            }
        }
        let mut work: Vec<u8> = Vec::new();
        let mut wi = 0usize;
        while wi < path.len() {
            if let Some(b) = path.get(wi) {
                work.push(*b);
            }
            wi += 1;
        }
        let mut pos = 0usize;
        let mut depth = 0u32;
        while pos < work.len() {
            while pos < work.len() && work.get(pos) == Some(&b'/') {
                pos += 1;
            }
            if pos >= work.len() {
                break;
            }
            let start = pos;
            while pos < work.len() && work.get(pos) != Some(&b'/') {
                pos += 1;
            }
            let comp = match work.get(start..pos) {
                Some(v) => v,
                None => return Err(CW_EBAL),
            };
            if comp.is_empty() || same(comp, b".") {
                continue;
            }
            if same(comp, b"..") {
                let top = match stack.last() {
                    Some(v) => *v,
                    None => return Err(CW_EBAL),
                };
                let is_dir = match self.nodes.get(top) {
                    Some(n) => n.dir,
                    None => return Err(CW_EBAL),
                };
                if !is_dir {
                    return Err(CW_ENOTDIR);
                }
                if stack.len() > 1 {
                    stack.pop();
                }
                continue;
            }
            if comp.len() > NAME_MAX {
                return Err(CW_ENAMETOOLONG);
            }
            let top = match stack.last() {
                Some(v) => *v,
                None => return Err(CW_EBAL),
            };
            let is_dir = match self.nodes.get(top) {
                Some(n) => n.dir,
                None => return Err(CW_EBAL),
            };
            if !is_dir {
                return Err(CW_ENOTDIR);
            }
            let child = match self.child(top, comp) {
                Some(v) => v,
                None => return Err(CW_ENOENT),
            };
            let node = match self.nodes.get(child) {
                Some(v) => *v,
                None => return Err(CW_EBAL),
            };
            if node.link {
                depth += 1;
                if depth > LINK_MAX {
                    return Err(CW_ELOOP);
                }
                let mut next: Vec<u8> = Vec::new();
                let mut k = 0usize;
                while k < node.tlen as usize {
                    if let Some(b) = node.target.get(k) {
                        next.push(*b);
                    }
                    k += 1;
                }
                let mut rest = pos;
                while rest < work.len() && work.get(rest) == Some(&b'/') {
                    rest += 1;
                }
                if rest < work.len() {
                    next.push(b'/');
                    let mut r = rest;
                    while r < work.len() {
                        if let Some(b) = work.get(r) {
                            next.push(*b);
                        }
                        r += 1;
                    }
                }
                work = next;
                pos = 0;
                continue;
            }
            stack.push(child);
            let nodes = self.depth(child);
            if nodes > 32 {
                return Err(CW_EBAL);
            }
        }
        if stack.is_empty() {
            stack.push(0);
        }
        self.to_path(&stack)
    }
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub cwd: u8,
    pub path: [u8; PATH_MAX],
    pub plen: u8,
    pub out: [u8; PATH_MAX],
    pub olen: u8,
    pub code: u16,
    pub ok: bool,
}

impl Row {
    pub fn path_bytes(&self) -> &[u8] {
        let n = self.plen as usize;
        if n > PATH_MAX {
            return &[];
        }
        match self.path.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 账本。
pub struct Ledger {
    fs: Fs,
    rows: Vec<Row>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { fs: Fs::fresh(), rows: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, cwd: u8, path: &[u8], want: &[u8], ok: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CW_EFROZEN);
        }
        if (slot as usize) >= ROWS {
            self.rejects += 1;
            return Err(CW_EID);
        }
        if (slot as usize) < self.rows.len() {
            self.rejects += 1;
            return Err(CW_EDUP);
        }
        if (slot as usize) != self.rows.len() {
            self.rejects += 1;
            return Err(CW_EORDER);
        }
        if path.len() > PATH_MAX || want.len() > PATH_MAX {
            self.rejects += 1;
            return Err(CW_EBAL);
        }
        let got = self.fs.resolve(cwd as usize, path);
        let mut r = Row { cwd, path: [0; PATH_MAX], plen: 0, out: [0; PATH_MAX], olen: 0, code: 0, ok };
        r.plen = put(&mut r.path, path);
        match got {
            Ok(v) => {
                if !ok || v.as_slice() != want {
                    self.rejects += 1;
                    return Err(CW_EMISMATCH);
                }
                r.olen = put(&mut r.out, &v);
            }
            Err(e) => {
                if ok {
                    self.rejects += 1;
                    return Err(CW_EMISMATCH);
                }
                r.code = (e & 0xffff) as u16;
            }
        }
        self.rows.push(r);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn row(&self, i: usize) -> Option<Row> {
        match self.rows.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn err_count(&self) -> usize {
        self.rows.len() - self.ok_count()
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.rows.is_empty() {
            return Err(CW_EBAL);
        }
        let fs = Fs::fresh();
        let mut i = 0usize;
        let mut ok = 0usize;
        let mut bad = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CW_ENOBS),
            };
            match fs.resolve(r.cwd as usize, r.path_bytes()) {
                Ok(v) => {
                    if !r.ok || r.olen as usize != v.len() || r.out.get(..v.len()) != Some(v.as_slice()) || r.code != 0 {
                        return Err(CW_EBAL);
                    }
                    ok += 1;
                }
                Err(e) => {
                    if r.ok || r.code != (e & 0xffff) as u16 || r.olen != 0 {
                        return Err(CW_EBAL);
                    }
                    bad += 1;
                }
            }
            i += 1;
        }
        if ok != self.ok_count() || bad != self.err_count() {
            return Err(CW_EBAL);
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.cwd as u64);
                h = fold(h, r.plen as u64);
                let mut k = 0usize;
                while k < r.plen as usize {
                    if let Some(b) = r.path.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
                h = fold(h, r.olen as u64);
                let mut j = 0usize;
                while j < r.olen as usize {
                    if let Some(b) = r.out.get(j) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    j += 1;
                }
                h = fold(h, r.code as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CW_EFROZEN);
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
            Err(CW_ESEAL)
        }
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

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

/// 十五行账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let _ = l.register(0, 0, b"/", b"/", true);
    let _ = l.register(1, 0, b"/a", b"/a", true);
    let _ = l.register(2, 0, b"/a/./b", b"/a/b", true);
    let _ = l.register(3, 0, b"/a/b/../b/c", b"/a/b/c", true);
    let _ = l.register(4, 0, b"/link/c", b"/a/b/c", true);
    let _ = l.register(5, 0, b"/../..", b"/", true);
    let _ = l.register(6, 1, b"b/c", b"/a/b/c", true);
    let _ = l.register(7, 0, b"/loop", b"", false);
    let _ = l.register(8, 0, b"/a/nope", b"", false);
    let _ = l.register(9, 0, b"/a/b/c/x", b"", false);
    let _ = l.register(10, 0, b"/x/..", b"", false);
    let _ = l.register(11, 0, b"0123456789", b"", false);
    let _ = l.register(12, 0, b"", b"", false);
    let _ = l.register(13, 0, b"/a//b", b"/a/b", true);
    let _ = l.register(14, 0, b"/up", b"/", true);
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    let mut l = build_ledger();
    let base = l.len() == ROWS && l.ok_count() == 9 && l.err_count() == 6 && l.audit().is_ok();
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let fs = Fs::fresh();
    out[1] = fs.resolve(0, b"/a/b/../b").map(|v| v == b"/a/b".to_vec()) == Ok(true)
        && fs.resolve(0, b"///").map(|v| v == b"/".to_vec()) == Ok(true);

    out[2] = fs.resolve(0, b"/rel/b/c").map(|v| v == b"/a/b/c".to_vec()) == Ok(true)
        && fs.resolve(0, b"/link/c").map(|v| v == b"/a/b/c".to_vec()) == Ok(true);

    out[3] = fs.resolve(0, b"/loop") == Err(CW_ELOOP)
        && fs.resolve(0, b"/a/b/c/x") == Err(CW_ENOTDIR)
        && fs.resolve(0, b"/a/nope") == Err(CW_ENOENT);
    out
}

/// 十三判据。
pub fn run_unx11448_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11448.canonical");
    let l = build_ledger();
    s.add("CW1-十五格", l.len() == ROWS, "static");
    s.add_dyn("CW2-九成六拒", l.ok_count() == 9 && l.err_count() == 6 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("CW3-根与普通", l.row(0).map(|r| r.olen) == Some(1) && l.row(1).map(|r| r.olen) == Some(2), "static");
    s.add("CW4-点号消除", l.row(2).map(|r| r.olen) == Some(4) && l.row(2).map(|r| r.out.get(0..4).map(|v| v == b"/a/b")) == Some(Some(true)), "static");
    s.add("CW5-双点回退", l.row(3).map(|r| r.out.get(0..6).map(|v| v == b"/a/b/c")) == Some(Some(true)), "static");
    s.add("CW6-链接展开", l.row(4).map(|r| r.out.get(0..6).map(|v| v == b"/a/b/c")) == Some(Some(true)), "static");
    s.add("CW7-不越根", l.row(5).map(|r| r.olen) == Some(1) && l.row(5).map(|r| r.out.get(0).copied()) == Some(Some(b'/')), "static");
    s.add("CW8-相对工作目录", l.row(6).map(|r| r.out.get(0..6).map(|v| v == b"/a/b/c")) == Some(Some(true)), "static");
    s.add("CW9-循环与缺失", l.row(7).map(|r| r.code) == Some((CW_ELOOP & 0xffff) as u16) && l.row(8).map(|r| r.code) == Some((CW_ENOENT & 0xffff) as u16), "static");
    s.add("CW10-非目录", l.row(9).map(|r| r.code) == Some((CW_ENOTDIR & 0xffff) as u16) && l.row(10).map(|r| r.code) == Some((CW_ENOTDIR & 0xffff) as u16), "static");
    s.add("CW11-过长与空", l.row(11).map(|r| r.code) == Some((CW_ENAMETOOLONG & 0xffff) as u16) && l.row(12).map(|r| r.code) == Some((CW_ENOENT & 0xffff) as u16), "static");
    s.add("CW12-重复斜杠与上跳", l.row(13).map(|r| r.olen) == Some(4) && l.row(14).map(|r| r.olen) == Some(1) && l.rejects() == 0, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(CW_EFROZEN);
    let inj = inject_four();
    s.add_dyn("CW13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_collapse() {
        let fs = Fs::fresh();
        assert_eq!(fs.resolve(0, b"/a/./b").map(|v| v == b"/a/b".to_vec()), Ok(true));
        assert_eq!(fs.resolve(0, b"/a/b/../b").map(|v| v == b"/a/b".to_vec()), Ok(true));
    }

    #[test]
    fn root_stays_root() {
        let fs = Fs::fresh();
        assert_eq!(fs.resolve(0, b"/..").map(|v| v == b"/".to_vec()), Ok(true));
        assert_eq!(fs.resolve(0, b"///").map(|v| v == b"/".to_vec()), Ok(true));
    }

    #[test]
    fn symlink_expansion() {
        let fs = Fs::fresh();
        assert_eq!(fs.resolve(0, b"/link/c").map(|v| v == b"/a/b/c".to_vec()), Ok(true));
        assert_eq!(fs.resolve(0, b"/rel/b/c").map(|v| v == b"/a/b/c".to_vec()), Ok(true));
    }

    #[test]
    fn loop_detected() {
        let fs = Fs::fresh();
        assert_eq!(fs.resolve(0, b"/loop"), Err(CW_ELOOP));
    }

    #[test]
    fn not_a_dir() {
        let fs = Fs::fresh();
        assert_eq!(fs.resolve(0, b"/a/b/c/x"), Err(CW_ENOTDIR));
        assert_eq!(fs.resolve(0, b"/x/.."), Err(CW_ENOTDIR));
    }

    #[test]
    fn too_long() {
        let fs = Fs::fresh();
        assert_eq!(fs.resolve(0, b"0123456789"), Err(CW_ENAMETOOLONG));
        assert_eq!(fs.resolve(0, b""), Err(CW_ENOENT));
    }

    #[test]
    fn ledger_and_checks() {
        let l = build_ledger();
        assert_eq!(l.len(), ROWS);
        assert_eq!(l.ok_count(), 9);
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11448_checks().failed_count(), 0);
    }
}
