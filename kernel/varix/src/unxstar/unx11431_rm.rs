//! UNX-F11431 rm 删除语义与 -r/-f 矩阵（0x20ab0 段）
//!
//! `rm` 的核心不是“删掉一个名字”，而是先决定这次调用允许碰哪些对象。模型的
//! 做法是先规划再执行：根据 `-f`、`-r` 两个标志位和对象类型生成一张删除计划，
//! 计划里列出将被摘除的每一个名字；只要计划中出现一个被禁止的对象，整次调用
//! 在动第一刀之前就被拒绝，因此不存在“删了一半”的中间态。
//!
//! 类型与标志的组合构成主错误矩阵：目录没有 `-r` 是 EISDIR；普通文件没有写位
//! 且没有 `-f` 是 EACCES；缺失路径在 `-f` 下视为成功且删除数为零，否则 ENOENT。
//! 此外还有三条与名字本身有关的规则：末分量为 `.` 或 `..` 一律 EINVAL，根目录
//! 是 EBUSY，挂载点是 EBUSY。符号链接只删链接自身，绝不递归到目标——这正是
//! `rm symlink` 与 `rm -r symlink/` 的分水岭。
//!
//! `-r` 的删除集用路径前缀前缀匹配得出，并按深度降序排列：先删最深的后代，再删
//! 祖先。计划必须满足“每个被删节点的所有后代也在计划中”，审计会重算这一闭包
//! 性质；任何遗漏都判失衡。
//!
//! 账本十四格：六格成功（单文件、强制只读文件、空目录递归、整树递归、符号链接、
//! 缺失加 -f），八格失败（缺 -f 的只读、缺 -r 的目录、缺失、点、点双、根、
//! 挂载点、忙节点）。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20ab0;
/// 编号非法。
pub const RM_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const RM_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const RM_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const RM_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测缺失。
pub const RM_ENOBS: u32 = SEG_BASE + 0x05;
/// 结果不符。
pub const RM_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 计划失衡。
pub const RM_EBAL: u32 = SEG_BASE + 0x07;
/// 路径为空。
pub const RM_ENAME: u32 = SEG_BASE + 0x08;
/// 路径缺失。
pub const RM_ENOENT: u32 = SEG_BASE + 0x09;
/// 目录缺 -r。
pub const RM_EISDIR: u32 = SEG_BASE + 0x0a;
/// 只读缺 -f。
pub const RM_EACCES: u32 = SEG_BASE + 0x0b;
/// 点或点双。
pub const RM_EINVAL: u32 = SEG_BASE + 0x0c;
/// 根或挂载点。
pub const RM_EBUSY: u32 = SEG_BASE + 0x0d;
/// 票据不一致。
pub const RM_ESEAL: u32 = SEG_BASE + 0x0e;

/// 普通文件。
pub const K_REG: u8 = 0;
/// 目录。
pub const K_DIR: u8 = 1;
/// 符号链接。
pub const K_LNK: u8 = 2;

/// 强制。
pub const F_FORCE: u8 = 0x01;
/// 递归。
pub const F_REC: u8 = 0x02;

/// 路径容量。
pub const PATH_CAP: usize = 16;
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

fn starts_with(path: &[u8], base: &[u8]) -> bool {
    if base.len() > path.len() {
        return false;
    }
    match path.get(..base.len()) {
        Some(v) => v == base,
        None => false,
    }
}

fn depth(path: &[u8]) -> u16 {
    let mut d = 0u16;
    let mut i = 0usize;
    while i < path.len() {
        if path[i] == b'/' {
            d += 1;
        }
        i += 1;
    }
    d
}

/// 一个可删对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: [u8; PATH_CAP],
    pub plen: u8,
    pub kind: u8,
    pub writable: bool,
    pub mount: bool,
    pub busy: bool,
    pub alive: bool,
}

impl Item {
    pub fn make(path: &[u8], kind: u8, writable: bool) -> Item {
        let mut p = [0u8; PATH_CAP];
        let l = put(&mut p, path);
        Item { path: p, plen: l, kind, writable, mount: false, busy: false, alive: true }
    }

    pub fn path(&self) -> &[u8] {
        cut(&self.path, self.plen)
    }
}

/// 删除规划器。
pub struct Planner {
    items: Vec<Item>,
    plan: Vec<u16>,
    removed: u32,
    plans: u32,
    rejects: u32,
}

impl Planner {
    pub fn new(items: Vec<Item>) -> Planner {
        Planner { items, plan: Vec::new(), removed: 0, plans: 0, rejects: 0 }
    }

    pub fn item(&self, i: usize) -> Option<Item> {
        match self.items.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn find(&self, path: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                if it.alive && it.path() == path {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    pub fn plan_len(&self) -> usize {
        self.plan.len()
    }

    pub fn plan_at(&self, i: usize) -> Option<u16> {
        match self.plan.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    pub fn removed(&self) -> u32 {
        self.removed
    }

    pub fn plans(&self) -> u32 {
        self.plans
    }

    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 某路径的子项数（直接后代）。
    fn subtree(&self, path: &[u8]) -> Vec<u16> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                if !it.alive || it.path() == path {
                    i += 1;
                    continue;
                }
                let p = it.path();
                if starts_with(p, path) && p.len() > path.len() && p.get(path.len()) == Some(&b'/') {
                    out.push(i as u16);
                }
            }
            i += 1;
        }
        out
    }

    /// 生成计划并把计划中的名字全部标死。返回删除个数。
    pub fn remove(&mut self, path: &[u8], flags: u8) -> Result<u16, u32> {
        self.plan.clear();
        if path.is_empty() || path.len() > PATH_CAP {
            self.rejects += 1;
            return Err(RM_ENAME);
        }
        let mut last = 0usize;
        let mut li = 0usize;
        while li < path.len() {
            if path[li] == b'/' {
                last = li;
            }
            li += 1;
        }
        let tail = match path.get(last..) {
            Some(v) => v,
            None => {
                self.rejects += 1;
                return Err(RM_ENAME);
            }
        };
        let name = if tail.starts_with(b"/") && tail.len() > 1 { &tail[1..] } else { tail };
        if name == b"." || name == b".." {
            self.rejects += 1;
            return Err(RM_EINVAL);
        }
        if path == b"/" {
            self.rejects += 1;
            return Err(RM_EBUSY);
        }
        let root = match self.find(path) {
            Some(v) => v,
            None => {
                if flags & F_FORCE != 0 {
                    self.plans += 1;
                    return Ok(0);
                }
                self.rejects += 1;
                return Err(RM_ENOENT);
            }
        };
        let r = match self.items.get(root) {
            Some(v) => *v,
            None => {
                self.rejects += 1;
                return Err(RM_ENOENT);
            }
        };
        if r.mount || r.busy {
            self.rejects += 1;
            return Err(RM_EBUSY);
        }
        if r.kind == K_DIR && flags & F_REC == 0 {
            self.rejects += 1;
            return Err(RM_EISDIR);
        }
        if r.kind == K_DIR && r.mount {
            self.rejects += 1;
            return Err(RM_EBUSY);
        }
        if r.kind != K_DIR && !r.writable && flags & F_FORCE == 0 {
            self.rejects += 1;
            return Err(RM_EACCES);
        }
        let mut set: Vec<u16> = Vec::new();
        if r.kind == K_DIR && flags & F_REC != 0 {
            let mut i = 0usize;
            while i < self.items.len() {
                if let Some(it) = self.items.get(i) {
                    if it.alive {
                        let p = it.path();
                        if starts_with(p, path) && p.len() > path.len() && p.get(path.len()) == Some(&b'/') {
                            if it.mount || it.busy {
                                self.rejects += 1;
                                return Err(RM_EBUSY);
                            }
                            set.push(i as u16);
                        }
                    }
                }
                i += 1;
            }
        }
        set.push(root as u16);
        // 深度降序：冒泡，后代先删。
        let mut a = 0usize;
        while a < set.len() {
            let mut b = 0usize;
            while b + 1 < set.len() {
                let pa = match set.get(b) {
                    Some(v) => *v,
                    None => 0,
                };
                let pb = match set.get(b + 1) {
                    Some(v) => *v,
                    None => 0,
                };
                let da = match self.item(pa as usize) {
                    Some(v) => depth(v.path()),
                    None => 0,
                };
                let db = match self.item(pb as usize) {
                    Some(v) => depth(v.path()),
                    None => 0,
                };
                if da < db {
                    set.swap(b, b + 1);
                }
                b += 1;
            }
            a += 1;
        }
        let mut k = 0usize;
        while k < set.len() {
            if let Some(id) = set.get(k) {
                if let Some(m) = self.items.get_mut(*id as usize) {
                    m.alive = false;
                }
                self.plan.push(*id);
            }
            k += 1;
        }
        self.removed += set.len() as u32;
        self.plans += 1;
        Ok(set.len() as u16)
    }

    /// 计划闭包审计：每个被删目录的后代也必须在计划中。
    pub fn audit(&self) -> Result<(), u32> {
        if self.items.is_empty() {
            return Err(RM_EBAL);
        }
        let mut i = 0usize;
        while i < self.plan.len() {
            let id = match self.plan.get(i) {
                Some(v) => *v,
                None => return Err(RM_ENOBS),
            };
            let it = match self.item(id as usize) {
                Some(v) => v,
                None => return Err(RM_ENOBS),
            };
            if it.alive {
                return Err(RM_EBAL);
            }
            if it.kind == K_DIR {
                let mut j = 0usize;
                while j < self.items.len() {
                    if let Some(ch) = self.items.get(j) {
                        let p = ch.path();
                        if starts_with(p, it.path()) && p.len() > it.path().len() && p.get(it.path().len()) == Some(&b'/') {
                            let planned = self.plan.iter().any(|x| *x as usize == j);
                            if !planned && ch.alive {
                                return Err(RM_EBAL);
                            }
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        Ok(())
    }

    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(it) = self.items.get(i) {
                h = fold(h, it.kind as u64);
                h = fold(h, it.alive as u64);
                h = fold(h, it.mount as u64);
                let mut k = 0usize;
                while k < it.plen as usize {
                    if let Some(b) = it.path.get(k) {
                        h ^= *b as u64;
                        h = h.wrapping_mul(FNV_PRIME);
                    }
                    k += 1;
                }
            }
            i += 1;
        }
        h = fold(h, self.removed as u64);
        h = fold(h, self.plans as u64);
        fold(h, self.rejects as u64)
    }
}

/// 起始表。
pub fn build_items() -> Vec<Item> {
    let mut v = Vec::new();
    v.push(Item::make(b"/t", K_DIR, true));
    v.push(Item::make(b"/t/a", K_REG, true));
    v.push(Item::make(b"/t/b", K_REG, true));
    v.push(Item::make(b"/t/sub", K_DIR, true));
    v.push(Item::make(b"/t/sub/c", K_REG, true));
    v.push(Item::make(b"/ro", K_REG, false));
    v.push(Item::make(b"/empty", K_DIR, true));
    v.push(Item::make(b"/mnt", K_DIR, true));
    let mut m = Item::make(b"/mnt/disk", K_DIR, true);
    m.mount = true;
    v.push(m);
    let mut b = Item::make(b"/busy", K_DIR, true);
    b.busy = true;
    v.push(b);
    v.push(Item::make(b"/lnk", K_LNK, true));
    v
}

/// 一格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub path: [u8; PATH_CAP],
    pub plen: u8,
    pub flags: u8,
    pub ok: bool,
    pub expect: u16,
}

impl Case {
    pub fn path(&self) -> &[u8] {
        cut(&self.path, self.plen)
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
    p: Planner,
    cases: Vec<Case>,
    ends: Vec<Ending>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    pub fn new() -> Ledger {
        Ledger { p: Planner::new(build_items()), cases: Vec::new(), ends: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    pub fn register(&mut self, slot: u8, path: &[u8], flags: u8, ok: bool, expect: u16) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RM_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(RM_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(RM_EDUP);
        }
        if (slot as usize) != self.cases.len() {
            self.rejects += 1;
            return Err(RM_EORDER);
        }
        let r = self.p.remove(path, flags);
        let end = match r {
            Ok(v) => {
                if !ok || v != expect {
                    self.rejects += 1;
                    return Err(RM_EMISMATCH);
                }
                Ending { ok: true, value: v }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != expect {
                    self.rejects += 1;
                    return Err(RM_EMISMATCH);
                }
                Ending { ok: false, value: (e & 0xffff) as u16 }
            }
        };
        let mut c = Case { path: [0; PATH_CAP], plen: 0, flags, ok, expect };
        c.plen = put(&mut c.path, path);
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

    pub fn p(&self) -> &Planner {
        &self.p
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.ends.len() || self.cases.is_empty() {
            return Err(RM_EBAL);
        }
        let mut i = 0usize;
        while i < self.cases.len() {
            let e = match self.ends.get(i) {
                Some(v) => *v,
                None => return Err(RM_ENOBS),
            };
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return Err(RM_ENOBS),
            };
            if c.ok != e.ok || c.expect != e.value {
                return Err(RM_EMISMATCH);
            }
            i += 1;
        }
        self.p.audit()
    }

    pub fn seal(&self) -> u64 {
        self.p.seal()
    }

    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(RM_EFROZEN);
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
            Err(RM_ESEAL)
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
    let enoent = (RM_ENOENT & 0xffff) as u16;
    let eacces = (RM_EACCES & 0xffff) as u16;
    let eisdir = (RM_EISDIR & 0xffff) as u16;
    let einval = (RM_EINVAL & 0xffff) as u16;
    let ebusy = (RM_EBUSY & 0xffff) as u16;
    let _ = l.register(0, b"/t/a", 0, true, 1);
    let _ = l.register(1, b"/ro", 0, false, eacces);
    let _ = l.register(2, b"/ro", F_FORCE, true, 1);
    let _ = l.register(3, b"/empty", 0, false, eisdir);
    let _ = l.register(4, b"/empty", F_REC, true, 1);
    let _ = l.register(5, b"/t", F_REC, true, 4);
    let _ = l.register(6, b"/t/a", F_FORCE, true, 0);
    let _ = l.register(7, b"/t/a", 0, false, enoent);
    let _ = l.register(8, b"/.", F_REC, false, einval);
    let _ = l.register(9, b"/t/..", F_REC, false, einval);
    let _ = l.register(10, b"/", F_REC | F_FORCE, false, ebusy);
    let _ = l.register(11, b"/mnt/disk", F_REC, false, ebusy);
    let _ = l.register(12, b"/lnk", 0, true, 1);
    let _ = l.register(13, b"/busy", F_REC, false, ebusy);
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

    let mut a = Planner::new(build_items());
    let r = a.remove(b"/t", F_REC);
    let order = a.plan_len() == 5
        && a.plan_at(0).map(|id| a.item(id as usize).map(|x| x.path() == b"/t/sub/c" as &[u8])) == Some(Some(true));
    out[1] = r == Ok(5) && order && a.find(b"/t").is_none() && a.removed() == 5;

    let mut b = Planner::new(build_items());
    let r2 = b.remove(b"/t", 0);
    let r3 = b.remove(b"/ro", 0);
    out[2] = r2 == Err(RM_EISDIR)
        && r3 == Err(RM_EACCES)
        && b.removed() == 0
        && b.find(b"/t/a").is_some();

    let mut c = Planner::new(build_items());
    let r4 = c.remove(b"/lnk", 0);
    let target = c.find(b"/t/a").is_some();
    let r5 = c.remove(b"/nothere", F_FORCE);
    out[3] = r4 == Ok(1) && target && r5 == Ok(0) && c.removed() == 1 && c.plans() == 2;
    out
}

/// 十三判据。
pub fn run_unx11431_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11431.rm");
    let l = build_ledger();
    let ec = |v: u32| (v & 0xffff) as u16;
    s.add("RM1-十四格齐备", l.len() == SAMPLES && l.is_empty() == false, "static");
    s.add_dyn("RM2-六应八拒", l.ok_count() == 6 && l.err_count() == 8 && l.audit().is_ok(), &format!("ok={} err={}", l.ok_count(), l.err_count()));
    s.add("RM3-只读拒绝与强制", l.ending(1).map(|e| e.value) == Some(ec(RM_EACCES)) && l.ending(2).map(|e| e.value) == Some(1), "static");
    s.add("RM4-目录缺r", l.ending(3).map(|e| e.value) == Some(ec(RM_EISDIR)) && l.p().find(b"/empty").is_none(), "static");
    s.add("RM5-递归删除计数", l.ending(5).map(|e| e.value) == Some(4) && l.p().find(b"/t/sub/c").is_none(), "static");
    let mut q = Planner::new(build_items());
    let qr = q.remove(b"/t", F_REC);
    let order = q.plan_len() == 5
        && q.plan_at(0).map(|id| q.item(id as usize).map(|x| x.path() == b"/t/sub/c" as &[u8])) == Some(Some(true));
    s.add("RM6-删除集深度降序", qr == Ok(5) && order && q.removed() == 5, "static");
    s.add("RM7-缺失与-f", l.ending(6).map(|e| e.value) == Some(0) && l.ending(7).map(|e| e.value) == Some(ec(RM_ENOENT)), "static");
    s.add("RM8-点双拒绝", l.ending(8).map(|e| e.value) == Some(ec(RM_EINVAL)) && l.ending(9).map(|e| e.value) == Some(ec(RM_EINVAL)), "static");
    s.add("RM9-根拒绝", l.ending(10).map(|e| e.value) == Some(ec(RM_EBUSY)), "static");
    s.add("RM10-挂载点拒绝", l.ending(11).map(|e| e.value) == Some(ec(RM_EBUSY)) && l.p().find(b"/mnt/disk").is_some(), "static");
    let mut z = Planner::new(build_items());
    let zr = z.remove(b"/lnk", 0);
    s.add("RM11-符号链接只删自身", l.ending(12).map(|e| e.value) == Some(1) && zr == Ok(1) && z.find(b"/t/b").is_some(), "static");
    s.add("RM12-忙节点拒绝", l.ending(13).map(|e| e.value) == Some(ec(RM_EBUSY)) && l.p().find(b"/busy").is_some(), "static");
    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(RM_EFROZEN);
    let inj = inject_four();
    s.add_dyn("RM13-冻结重放四注入", root != 0 && rep == Ok(root) && again && inj == [true; 4], &format!("root={:016x} {:?}", root, inj));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_file() {
        let mut p = Planner::new(build_items());
        assert_eq!(p.remove(b"/t/a", 0), Ok(1));
        assert!(p.find(b"/t/a").is_none());
        assert_eq!(p.removed(), 1);
    }

    #[test]
    fn read_only_needs_force() {
        let mut p = Planner::new(build_items());
        assert_eq!(p.remove(b"/ro", 0), Err(RM_EACCES));
        assert_eq!(p.remove(b"/ro", F_FORCE), Ok(1));
    }

    #[test]
    fn dir_needs_recursive() {
        let mut p = Planner::new(build_items());
        assert_eq!(p.remove(b"/empty", 0), Err(RM_EISDIR));
        assert_eq!(p.remove(b"/empty", F_REC), Ok(1));
    }

    #[test]
    fn recursion_depth_order() {
        let mut p = Planner::new(build_items());
        assert_eq!(p.remove(b"/t", F_REC), Ok(5));
        assert_eq!(p.plan_at(0).map(|id| p.item(id as usize).map(|x| x.path() == b"/t/sub/c" as &[u8])), Some(Some(true)));
    }

    #[test]
    fn dot_and_root_refused() {
        let mut p = Planner::new(build_items());
        assert_eq!(p.remove(b"/.", F_REC), Err(RM_EINVAL));
        assert_eq!(p.remove(b"/", F_REC), Err(RM_EBUSY));
        assert_eq!(p.removed(), 0);
    }

    #[test]
    fn mount_and_force_missing() {
        let mut p = Planner::new(build_items());
        assert_eq!(p.remove(b"/mnt/disk", F_REC), Err(RM_EBUSY));
        assert_eq!(p.remove(b"/none", F_FORCE), Ok(0));
        assert_eq!(p.plans(), 1);
    }

    #[test]
    fn ledger_is_consistent() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(inject_four(), [true; 4]);
        assert_eq!(run_unx11431_checks().failed_count(), 0);
    }
}
