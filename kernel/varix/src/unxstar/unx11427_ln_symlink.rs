//! UNX-F11427 符号链接创建与目标规范化（0x20a70 段）
//!
//! `symlink(target, linkpath)` 与硬链接最根本的差别是它不要求 target 存在。
//! 系统调用只做两件事：把目标字符串原样存进新的链接节点，并在父目录里加一个
//! 名字。因此悬空链接是合法产物，成功与否只取决于父目录、名字冲突与字符串
//! 长度这三个约束。
//!
//! 目标字符串有两种书写方式：绝对目标从根重新起算，相对目标以链接所在目录为
//! 基准。账本同时登记两类，并用纯词法规范化（折叠 `.`、弹栈 `..`、丢弃重复
//! 斜杠）算出规范路径。规范化只在字符串层面进行，不触碰文件系统，因此即使
//! 目标不存在也能得到确定的规范形式——这正是它与 rename 语义分裂的地方。
//!
//! 解析链接时逐跳跟随：取出节点的规范目标，再在该路径上找下一个链接，直到
//! 落到非链接节点或超过跳数上限。自指链接 `loop -> loop` 在创建时被允许，
//! 但一跟随就立刻触顶返回 ELOOP，对应内核侧的 MAXSYMLINKS。
//!
//! 账本登记十四格：六格创建成功（含悬空、自指、跨目录相对目标），八格失败
//! （重名、父缺失、父为文件、空目标、空名字、名字过长、目标过长）。不变量：
//! 同目录名字唯一、目标串非空、长度字段不越界、规范化结果以 `/` 开头。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
pub const SEG_BASE: u32 = 0x20a70;
pub const SL_EID: u32 = SEG_BASE + 0x01;
pub const SL_EORDER: u32 = SEG_BASE + 0x02;
pub const SL_EDUP: u32 = SEG_BASE + 0x03;
pub const SL_EFROZEN: u32 = SEG_BASE + 0x04;
pub const SL_ENOBS: u32 = SEG_BASE + 0x05;
pub const SL_EMISMATCH: u32 = SEG_BASE + 0x06;
pub const SL_EBAL: u32 = SEG_BASE + 0x07;
pub const SL_ENAM: u32 = SEG_BASE + 0x08;
pub const SL_ETGT: u32 = SEG_BASE + 0x09;
pub const SL_ENOENT: u32 = SEG_BASE + 0x0a;
pub const SL_ENOTDIR: u32 = SEG_BASE + 0x0b;
pub const SL_EEXIST: u32 = SEG_BASE + 0x0c;
pub const SL_ENAMETOOLONG: u32 = SEG_BASE + 0x0d;
pub const SL_ELOOP: u32 = SEG_BASE + 0x0e;
pub const SL_ESEAL: u32 = SEG_BASE + 0x0f;
pub const NAME_CAP: usize = 8;
pub const TARGET_CAP: usize = 24;
pub const PATH_CAP: usize = 40;
pub const MAX_SYMS: usize = 32;
pub const SAMPLES: usize = 14;
pub const MAX_HOPS: u8 = 4;
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;
fn hash_bytes(h: u64, b: &[u8]) -> u64 {
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
fn put(dst: &mut [u8], src: &[u8]) -> usize {
    let mut i = 0usize;
    while i < src.len() && i < dst.len() {
        dst[i] = src[i];
        i += 1;
    }
    i
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sym {
    pub dir: [u8; PATH_CAP],
    pub dlen: u8,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub target: [u8; TARGET_CAP],
    pub tlen: u8,
}
impl Sym {
    pub fn make(dir: &[u8], name: &[u8], target: &[u8]) -> Sym {
        let mut s = Sym {
            dir: [0; PATH_CAP],
            dlen: 0,
            name: [0; NAME_CAP],
            nlen: 0,
            target: [0; TARGET_CAP],
            tlen: 0,
        };
        s.dlen = put(&mut s.dir, dir) as u8;
        s.nlen = put(&mut s.name, name) as u8;
        s.tlen = put(&mut s.target, target) as u8;
        s
    }
    pub fn dir(&self) -> &[u8] {
        cut(&self.dir, self.dlen)
    }
    pub fn name(&self) -> &[u8] {
        cut(&self.name, self.nlen)
    }
    pub fn target(&self) -> &[u8] {
        cut(&self.target, self.tlen)
    }
    pub fn absolute(&self) -> bool {
        self.target().first() == Some(&b'/')
    }
}
pub fn is_dir(p: &[u8]) -> bool {
    p == b"/" || p == b"/a" || p == b"/a/b" || p == b"/c"
}
pub fn is_file(p: &[u8]) -> bool {
    p == b"/a/f" || p == b"/c/g"
}
pub fn normalize(input: &[u8], out: &mut [u8; PATH_CAP]) -> usize {
    let mut w = 0usize;
    let mut i = 0usize;
    while i < input.len() {
        while i < input.len() && input[i] == b'/' {
            i += 1;
        }
        if i >= input.len() {
            break;
        }
        let s = i;
        while i < input.len() && input[i] != b'/' {
            i += 1;
        }
        let comp = match input.get(s..i) {
            Some(v) => v,
            None => return 0,
        };
        if comp == b"." {
            continue;
        }
        if comp == b".." {
            while w > 0 && out[w - 1] != b'/' {
                w -= 1;
            }
            if w > 0 {
                w -= 1;
            }
            continue;
        }
        if w + comp.len() + 1 > PATH_CAP {
            return 0;
        }
        out[w] = b'/';
        w += 1;
        let mut j = 0usize;
        while j < comp.len() {
            out[w] = comp[j];
            w += 1;
            j += 1;
        }
    }
    if w == 0 {
        out[0] = b'/';
        return 1;
    }
    w
}
pub struct LinkTable {
    syms: Vec<Sym>,
    creates: u32,
    rejects: u32,
    frozen: bool,
    root: u64,
}
impl LinkTable {
    pub fn fresh() -> LinkTable {
        let mut v = Vec::new();
        v.push(Sym::make(b"/", b"l1", b"a"));
        v.push(Sym::make(b"/a", b"l2", b"/"));
        v.push(Sym::make(b"/a/b", b"up", b".."));
        LinkTable { syms: v, creates: 0, rejects: 0, frozen: false, root: 0 }
    }
    pub fn len(&self) -> usize {
        self.syms.len()
    }
    pub fn get(&self, i: usize) -> Option<Sym> {
        match self.syms.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }
    pub fn creates(&self) -> u32 {
        self.creates
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn find(&self, dir: &[u8], name: &[u8]) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let mut i = 0usize;
        while i < self.syms.len() {
            if let Some(s) = self.syms.get(i) {
                if s.dir() == dir && s.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
    pub fn find_abs(&self, path: &[u8]) -> Option<usize> {
        let mut cut_at = None;
        let mut i = 0usize;
        while i < path.len() {
            if path[i] == b'/' {
                cut_at = Some(i);
            }
            i += 1;
        }
        let (dir, name) = match cut_at {
            None => (b"/" as &[u8], path),
            Some(p) => match (path.get(..p), path.get(p + 1..)) {
                (Some(a), Some(b)) => (if a.is_empty() { b"/" as &[u8] } else { a }, b),
                _ => return None,
            },
        };
        self.find(dir, name)
    }
    pub fn create(&mut self, dir: &[u8], name: &[u8], target: &[u8]) -> Result<u16, u32> {
        if name.is_empty() {
            self.rejects += 1;
            return Err(SL_ENAM);
        }
        if target.is_empty() {
            self.rejects += 1;
            return Err(SL_ETGT);
        }
        if name.len() > NAME_CAP || target.len() > TARGET_CAP || dir.len() >= PATH_CAP {
            self.rejects += 1;
            return Err(SL_ENAMETOOLONG);
        }
        if is_file(dir) {
            self.rejects += 1;
            return Err(SL_ENOTDIR);
        }
        if !is_dir(dir) {
            self.rejects += 1;
            return Err(SL_ENOENT);
        }
        if self.find(dir, name).is_some() {
            self.rejects += 1;
            return Err(SL_EEXIST);
        }
        if self.syms.len() >= MAX_SYMS {
            self.rejects += 1;
            return Err(SL_EBAL);
        }
        self.syms.push(Sym::make(dir, name, target));
        self.creates += 1;
        Ok(target.len() as u16)
    }
    pub fn canonical(&self, idx: usize) -> Option<([u8; PATH_CAP], usize)> {
        let s = match self.syms.get(idx) {
            Some(v) => *v,
            None => return None,
        };
        let mut joined = [0u8; PATH_CAP];
        let mut n = 0usize;
        if !s.absolute() {
            n = put(&mut joined, s.dir());
            if n == 0 {
                joined[0] = b'/';
                n = 1;
            }
        }
        let t = s.target();
        if !s.absolute() {
            if n >= PATH_CAP {
                return None;
            }
            joined[n] = b'/';
            n += 1;
        }
        let mut j = 0usize;
        while j < t.len() {
            if n >= PATH_CAP {
                return None;
            }
            joined[n] = t[j];
            n += 1;
            j += 1;
        }
        let mut out = [0u8; PATH_CAP];
        let w = normalize(match joined.get(..n) {
            Some(v) => v,
            None => return None,
        }, &mut out);
        if w == 0 {
            return None;
        }
        Some((out, w))
    }
    pub fn follow(&self, path: &[u8], limit: u8) -> Result<([u8; PATH_CAP], usize), u32> {
        let mut cur = [0u8; PATH_CAP];
        let mut n = put(&mut cur, path);
        if n == 0 {
            return Err(SL_ENOENT);
        }
        let mut hops = 0u8;
        loop {
            let here = match cur.get(..n) {
                Some(v) => v,
                None => return Err(SL_ENOENT),
            };
            let idx = match self.find_abs(here) {
                Some(v) => v,
                None => return Ok((cur, n)),
            };
            hops += 1;
            if hops > limit {
                return Err(SL_ELOOP);
            }
            let (c, w) = match self.canonical(idx) {
                Some(v) => v,
                None => return Err(SL_EMISMATCH),
            };
            cur = c;
            n = w;
        }
    }
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.syms.len() {
            if let Some(s) = self.syms.get(i) {
                h = hash_bytes(h, cut(&s.dir, s.dlen));
                h = hash_bytes(h, cut(&s.name, s.nlen));
                h = hash_bytes(h, cut(&s.target, s.tlen));
            }
            i += 1;
        }
        h = fold(h, self.creates as u64);
        fold(h, self.rejects as u64)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub sym: Sym,
    pub ok: bool,
    pub expect: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ending {
    pub ok: bool,
    pub value: u16,
}
pub struct Ledger {
    table: LinkTable,
    rows: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}
impl Ledger {
    pub fn new() -> Ledger {
        Ledger { table: LinkTable::fresh(), rows: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }
    pub fn register(&mut self, slot: u8, dir: &[u8], name: &[u8], target: &[u8], ok: bool, expect: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SL_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(SL_EID);
        }
        if (slot as usize) < self.rows.len() {
            self.rejects += 1;
            return Err(SL_EDUP);
        }
        if (slot as usize) != self.rows.len() {
            self.rejects += 1;
            return Err(SL_EORDER);
        }
        let long = name.len() > NAME_CAP || target.len() > TARGET_CAP;
        let r = if long {
            Err(SL_ENAMETOOLONG)
        } else {
            self.table.create(dir, name, target)
        };
        let end = match r {
            Ok(v) => {
                if !ok || v != expect {
                    self.rejects += 1;
                    return Err(SL_EMISMATCH);
                }
                Ending { ok: true, value: v }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != expect {
                    self.rejects += 1;
                    return Err(SL_EMISMATCH);
                }
                Ending { ok: false, value: (e & 0xffff) as u16 }
            }
        };
        self.rows.push(Case { sym: Sym::make(dir, name, target), ok, expect });
        self.ends.push(end);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    pub fn row(&self, i: usize) -> Option<Case> {
        match self.rows.get(i) {
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
    pub fn table(&self) -> &LinkTable {
        &self.table
    }
    pub fn audit(&self) -> Result<(), u32> {
        if self.rows.len() != self.ends.len() || self.rows.is_empty() {
            return Err(SL_EBAL);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SL_ENOBS),
            };
            let e = match self.ends.get(i) {
                Some(v) => *v,
                None => return Err(SL_ENOBS),
            };
            if r.ok != e.ok || r.expect != e.value {
                return Err(SL_EMISMATCH);
            }
            i += 1;
        }
        let mut a = 0usize;
        while a < self.table.len() {
            let x = match self.table.get(a) {
                Some(v) => v,
                None => return Err(SL_ENOBS),
            };
            if x.tlen == 0 || x.tlen as usize > TARGET_CAP || x.nlen == 0 {
                return Err(SL_EBAL);
            }
            let mut b = a + 1;
            while b < self.table.len() {
                if let Some(y) = self.table.get(b) {
                    if x.dir() == y.dir() && x.name() == y.name() {
                        return Err(SL_EBAL);
                    }
                }
                b += 1;
            }
            a += 1;
        }
        Ok(())
    }
    pub fn seal(&self) -> u64 {
        self.table.seal()
    }
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SL_EFROZEN);
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
            Err(SL_ESEAL)
        }
    }
    pub fn root(&self) -> u64 {
        self.root
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eexist = (SL_EEXIST & 0xffff) as u16;
    let enoent = (SL_ENOENT & 0xffff) as u16;
    let enotdir = (SL_ENOTDIR & 0xffff) as u16;
    let etgt = (SL_ETGT & 0xffff) as u16;
    let enam = (SL_ENAM & 0xffff) as u16;
    let elong = (SL_ENAMETOOLONG & 0xffff) as u16;
    let _ = l.register(0, b"/a", b"s1", b"target", true, 6);
    let _ = l.register(1, b"/a", b"s2", b"../c", true, 4);
    let _ = l.register(2, b"/a", b"s3", b"/a/b/none", true, 9);
    let _ = l.register(3, b"/a", b"s1", b"other", false, eexist);
    let _ = l.register(4, b"/z", b"s4", b"t", false, enoent);
    let _ = l.register(5, b"/a/f", b"s5", b"t", false, enotdir);
    let _ = l.register(6, b"/a", b"s6", b"", false, etgt);
    let _ = l.register(7, b"/a", b"toolongname", b"t", false, elong);
    let _ = l.register(8, b"/a", b"s7", b"012345678901234567890123456", false, elong);
    let _ = l.register(9, b"/a", b"", b"t", false, enam);
    let _ = l.register(10, b"/a/b", b"s8", b"../../a/s1", true, 10);
    let _ = l.register(11, b"/a", b"loop", b"loop", true, 4);
    let _ = l.register(12, b"/a", b"s1", b"again", false, eexist);
    let _ = l.register(13, b"/a", b"s10", b"/x/y", true, 4);
    l
}
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
    let idx = l.table().find(b"/a", b"s3");
    let dangling = match idx {
        Some(i) => match l.table().canonical(i) {
            Some((p, n)) => cut(&p, n as u8) == b"/a/b/none",
            None => false,
        },
        None => false,
    };
    let miss = l.table().follow(b"/a/s3", MAX_HOPS);
    out[1] = dangling && miss.is_ok();
    let mut t = LinkTable::fresh();
    let before = t.len();
    let first = t.create(b"/a", b"dup", b"one");
    let second = t.create(b"/a", b"dup", b"two");
    let kept = match t.find(b"/a", b"dup") {
        Some(i) => t.get(i).map(|s| s.target() == b"one" as &[u8]),
        None => None,
    };
    out[2] = first == Ok(3) && second == Err(SL_EEXIST) && t.len() == before + 1 && kept == Some(true);
    let looped = l.table().follow(b"/a/loop", MAX_HOPS) == Err(SL_ELOOP);
    let chain = match l.table().follow(b"/a/b/s8", MAX_HOPS) {
        Ok((p, n)) => cut(&p, n as u8) == b"/a/target",
        Err(_) => false,
    };
    out[3] = looped && chain;
    out
}
pub fn run_unx11427_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11427.ln_symlink");
    let l = build_ledger();
    s.add(
        "SL1-十四格局面齐备",
        l.len() == SAMPLES && l.row(0).map(|r| r.sym.dir() == b"/a" as &[u8]) == Some(true) && l.table().len() == 9,
        "static",
    );
    s.add_dyn(
        "SL2-六应八拒分布",
        l.ok_count() == 6 && l.err_count() == 8 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );
    s.add(
        "SL3-悬空目标可创建",
        l.ending(2).map(|e| e.ok) == Some(true) && l.table().find(b"/a", b"s3").is_some(),
        "static",
    );
    s.add(
        "SL4-重名拒绝且不覆写",
        l.ending(3).map(|e| e.value) == Some((SL_EEXIST & 0xffff) as u16)
            && match l.table().find(b"/a", b"s1") {
                Some(i) => l.table().get(i).map(|x| x.target() == b"target" as &[u8]) == Some(true),
                None => false,
            },
        "static",
    );
    s.add(
        "SL5-父缺失与父为文件分野",
        l.ending(4).map(|e| e.value) == Some((SL_ENOENT & 0xffff) as u16)
            && l.ending(5).map(|e| e.value) == Some((SL_ENOTDIR & 0xffff) as u16),
        "static",
    );
    s.add(
        "SL6-空目标与空名字拒绝",
        l.ending(6).map(|e| e.value) == Some((SL_ETGT & 0xffff) as u16)
            && l.ending(9).map(|e| e.value) == Some((SL_ENAM & 0xffff) as u16),
        "static",
    );
    s.add(
        "SL7-长度双向上限",
        l.ending(7).map(|e| e.value) == Some((SL_ENAMETOOLONG & 0xffff) as u16)
            && l.ending(8).map(|e| e.value) == Some((SL_ENAMETOOLONG & 0xffff) as u16)
            && l.table().find(b"/a", b"s7").is_none(),
        "static",
    );
    let up_ok = match l.table().find(b"/a/b", b"up") {
        Some(i) => match l.table().canonical(i) {
            Some((p, n)) => cut(&p, n as u8) == b"/a",
            None => false,
        },
        None => false,
    };
    s.add("SL8-相对目标词法规范化", up_ok, "static");
    let s2_ok = match l.table().find(b"/a", b"s2") {
        Some(i) => match l.table().canonical(i) {
            Some((p, n)) => cut(&p, n as u8) == b"/c",
            None => false,
        },
        None => false,
    };
    s.add("SL9-父目录弹栈上溯", s2_ok, "static");
    s.add(
        "SL10-自指创建允许跟随触顶",
        l.ending(11).map(|e| e.ok) == Some(true) && l.table().follow(b"/a/loop", MAX_HOPS) == Err(SL_ELOOP),
        "static",
    );
    let chain_ok = match l.table().follow(b"/a/b/s8", MAX_HOPS) {
        Ok((p, n)) => cut(&p, n as u8) == b"/a/target",
        Err(_) => false,
    };
    s.add("SL11-多跳链式跟随", chain_ok, "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(SL_EFROZEN);
    let rej = f.rejects();
    s.add_dyn(
        "SL12-冻结重放幂等",
        root != 0 && rep == Ok(root) && again && rej == 1 && f.root() == root,
        &format!("root={:016x}", root),
    );
    let inj = inject_four();
    s.add_dyn("SL13-四路注入全绿", inj == [true; 4], &format!("inject={:?}", inj));
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dangling_is_legal() {
        let mut t = LinkTable::fresh();
        assert_eq!(t.create(b"/a", b"d", b"/nowhere"), Ok(8));
        assert!(t.find(b"/a", b"d").is_some());
    }
    #[test]
    fn duplicate_rejected() {
        let mut t = LinkTable::fresh();
        assert_eq!(t.create(b"/a", b"x", b"one"), Ok(3));
        assert_eq!(t.create(b"/a", b"x", b"two"), Err(SL_EEXIST));
        assert_eq!(t.find(b"/a", b"x").is_some(), true);
    }
    #[test]
    fn relative_normalization() {
        let mut buf = [0u8; PATH_CAP];
        let n = normalize(b"/a/./b/../c//", &mut buf);
        assert_eq!(buf.get(..n), Some(b"/a/c" as &[u8]));
        let m = normalize(b"../../..", &mut buf);
        assert_eq!(buf.get(..m), Some(b"/" as &[u8]));
    }
    #[test]
    fn self_loop_detected() {
        let mut t = LinkTable::fresh();
        assert_eq!(t.create(b"/a", b"self", b"self"), Ok(4));
        assert_eq!(t.follow(b"/a/self", MAX_HOPS), Err(SL_ELOOP));
        assert_eq!(t.follow(b"/a/self", 0), Err(SL_ELOOP));
    }
    #[test]
    fn chain_and_limits() {
        let t = LinkTable::fresh();
        match t.follow(b"/l1", MAX_HOPS) {
            Ok((p, n)) => assert_eq!(p.get(..n), Some(b"/a" as &[u8])),
            Err(_) => panic_guard(),
        }
        let mut m = LinkTable::fresh();
        assert_eq!(m.create(b"/a", b"abcdefghi", b"t"), Err(SL_ENAMETOOLONG));
        assert_eq!(m.create(b"/a", b"ok", b""), Err(SL_ETGT));
        assert_eq!(m.create(b"/no", b"ok", b"t"), Err(SL_ENOENT));
        assert_eq!(m.rejects(), 3);
    }
    fn panic_guard() {
        assert!(false);
    }
    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 6);
        assert_eq!(l.err_count(), 8);
        assert!(l.audit().is_ok());
                assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11427_checks().failed_count(), 0);
    }
}