//! UNX-F11425 chown -h 符号链接特义（0x20a50 段）
//!
//! `chown` 与 `lchown` 的分界只落在路径的最后一个分量上：中间分量上的符号
//! 链接永远被跟随，末分量是否跟随却由调用方式决定。缺省 `chown` 跟随末链，
//! 改的是链接指向的目标；`chown -h`（等价 `lchown`）不跟随，改的是链接节点
//! 自身的属主属组。这条差别在悬挂链接上最锋利：`lchown dangling` 成功，因为
//! 链接节点确实存在；`chown dangling` 失败 ENOENT，因为目标不存在。
//!
//! 模型实现一个最小的名字空间：节点有父、名字、种类（目录/普通/链接），链接
//! 节点携带目标路径（可绝对可相对）。解析器逐分量查表，遇到链接时按“是否末
//! 分量 + 是否跟随”决定是否展开；展开有跳数预算，超过预算判 ELOOP，目标缺失
//! 判 ENOENT，中间分量落在非目录上判 ENOTDIR。跳数预算必须在展开前自增且
//! 展开后继续累加，否则自环只能靠栈溢出暴露而拿不到确定错误码。
//!
//! 递归改属另起一路：物理遍历（`-R` 缺省）不进入链接，但会把链接节点本身当作
//! 叶子改写；逻辑遍历（`-RL`）跟随链接，指向目录时继续下降。两者在账本里必须
//! 给出不同的改写顺序，否则“是否下降”这件事在轨迹上不可见。
//!
//! 账本登记十四格：十格成功、四格失败（悬挂/自环/缺失/中间非目录各一）。
//! 冻结前重放全部样本，重放必须与冻结票号一致。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20a50;

/// 编号非法。
pub const SL_EID: u32 = SEG_BASE + 0x01;
/// 状态顺序错误。
pub const SL_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const SL_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const SL_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const SL_ENOBS: u32 = SEG_BASE + 0x05;
/// 观测结果不符。
pub const SL_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const SL_EBAL: u32 = SEG_BASE + 0x07;
/// 空样本。
pub const SL_EEMPTY: u32 = SEG_BASE + 0x08;
/// 实参非法。
pub const SL_ESPEC: u32 = SEG_BASE + 0x09;
/// 权限不足。
pub const SL_EPERM: u32 = SEG_BASE + 0x0a;
/// 目标不存在。
pub const SL_ENOENT: u32 = SEG_BASE + 0x0b;
/// 中间分量不是目录。
pub const SL_ENOTDIR: u32 = SEG_BASE + 0x0c;
/// 链接展开成环。
pub const SL_ELOOP: u32 = SEG_BASE + 0x0d;
/// 票号不一致。
pub const SL_ESEAL: u32 = SEG_BASE + 0x0e;

/// 节点种类：目录。
pub const K_DIR: u8 = 0;
/// 节点种类：普通文件。
pub const K_REG: u8 = 1;
/// 节点种类：符号链接。
pub const K_LNK: u8 = 2;
/// 节点种类：空位。
pub const K_NONE: u8 = 3;

/// 跟随末链。
pub const M_FOLLOW: u8 = 0;
/// 不跟随末链（-h）。
pub const M_LCHOWN: u8 = 1;

/// 路径缓冲容量。
pub const PATH_CAP: usize = 16;
/// 实参缓冲容量。
pub const SPEC_CAP: usize = 12;
/// 名字缓冲容量。
pub const NAME_CAP: usize = 8;
/// 链接目标缓冲容量。
pub const TARGET_CAP: usize = 12;
/// 节点容量。
pub const NODES: usize = 12;
/// 链接展开跳数预算。
pub const MAX_HOPS: u32 = 8;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: u8,
    pub kind: u8,
    pub parent: u8,
    pub uid: u32,
    pub gid: u32,
    pub name: [u8; NAME_CAP],
    pub nlen: u8,
    pub target: [u8; TARGET_CAP],
    pub tlen: u8,
}

impl Node {
    /// 空位。
    pub const fn blank() -> Node {
        Node {
            id: 0xff,
            kind: K_NONE,
            parent: 0xff,
            uid: 0,
            gid: 0,
            name: [0; NAME_CAP],
            nlen: 0,
            target: [0; TARGET_CAP],
            tlen: 0,
        }
    }

    /// 构造节点。
    pub const fn make(
        id: u8,
        kind: u8,
        parent: u8,
        uid: u32,
        gid: u32,
        name: &[u8],
        target: &[u8],
    ) -> Node {
        let mut n = [0u8; NAME_CAP];
        let mut t = [0u8; TARGET_CAP];
        let mut i = 0;
        while i < name.len() && i < NAME_CAP {
            n[i] = name[i];
            i += 1;
        }
        i = 0;
        while i < target.len() && i < TARGET_CAP {
            t[i] = target[i];
            i += 1;
        }
        let nl = if name.len() > NAME_CAP { NAME_CAP as u8 } else { name.len() as u8 };
        let tl = if target.len() > TARGET_CAP { TARGET_CAP as u8 } else { target.len() as u8 };
        Node { id, kind, parent, uid, gid, name: n, nlen: nl, target: t, tlen: tl }
    }

    /// 名字切片。
    pub fn name(&self) -> &[u8] {
        let n = self.nlen as usize;
        if n > NAME_CAP {
            return &[];
        }
        match self.name.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 链接目标切片。
    pub fn target(&self) -> &[u8] {
        let n = self.tlen as usize;
        if n > TARGET_CAP {
            return &[];
        }
        match self.target.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 调用者。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caller {
    pub uid: u32,
    pub root: bool,
}

impl Caller {
    /// root。
    pub const fn root() -> Caller {
        Caller { uid: 0, root: true }
    }

    /// 普通用户。
    pub const fn user(uid: u32) -> Caller {
        Caller { uid, root: false }
    }
}

/// 一次改写记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub node: u8,
    pub mode: u8,
    pub before_uid: u32,
    pub before_gid: u32,
    pub after_uid: u32,
    pub after_gid: u32,
}

/// 解析后的属主属组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub user: Option<u32>,
    pub group: Option<u32>,
}

/// 解析 `u:g` 数字实参；两侧都可省略，但不能同时省略。
pub fn parse_spec(spec: &[u8]) -> Result<Spec, u32> {
    if spec.is_empty() {
        return Err(SL_ESPEC);
    }
    let mut colon = None;
    let mut i = 0;
    while i < spec.len() {
        if spec[i] == b':' {
            if colon.is_some() {
                return Err(SL_ESPEC);
            }
            colon = Some(i);
        }
        i += 1;
    }
    match colon {
        None => Ok(Spec { user: Some(num(spec)?), group: None }),
        Some(p) => {
            let u = match spec.get(..p) {
                Some(v) => v,
                None => return Err(SL_ESPEC),
            };
            let g = match spec.get(p + 1..) {
                Some(v) => v,
                None => return Err(SL_ESPEC),
            };
            if u.is_empty() && g.is_empty() {
                return Err(SL_ESPEC);
            }
            let user = if u.is_empty() { None } else { Some(num(u)?) };
            let group = if g.is_empty() { None } else { Some(num(g)?) };
            Ok(Spec { user, group })
        }
    }
}

fn num(s: &[u8]) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(SL_ESPEC);
    }
    let mut v: u64 = 0;
    let mut i = 0;
    while i < s.len() {
        if s[i] < b'0' || s[i] > b'9' {
            return Err(SL_ESPEC);
        }
        v = v * 10 + (s[i] - b'0') as u64;
        if v > 0x7fff_ffff {
            return Err(SL_ESPEC);
        }
        i += 1;
    }
    Ok(v as u32)
}

/// 目录树与改写轨迹。
pub struct Vfs {
    tree: Vec<Node>,
    events: Vec<Event>,
    errors: u32,
    hops_peak: u32,
    last_hops: u32,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Vfs {
    /// 以给定节点表构造。
    pub fn new(tree: Vec<Node>) -> Vfs {
        Vfs { tree, events: Vec::new(), errors: 0, hops_peak: 0, last_hops: 0, frozen: false, root: 0, rejects: 0 }
    }

    /// 取节点。
    pub fn node(&self, id: u8) -> Option<Node> {
        if (id as usize) >= self.tree.len() {
            return None;
        }
        match self.tree.get(id as usize) {
            Some(n) => Some(*n),
            None => None,
        }
    }

    /// 事件条数。
    pub fn events_len(&self) -> usize {
        self.events.len()
    }

    /// 最近一条事件。
    pub fn last_event(&self) -> Option<Event> {
        match self.events.last() {
            Some(e) => Some(*e),
            None => None,
        }
    }

    /// 错误累计。
    pub fn errors(&self) -> u32 {
        self.errors
    }

    /// 跳数峰值。
    pub fn hops_peak(&self) -> u32 {
        self.hops_peak
    }

    /// 最近一次解析的跳数。
    pub fn last_hops(&self) -> u32 {
        self.last_hops
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 在父节点下按名字查子节点。
    pub fn lookup(&self, parent: u8, name: &[u8]) -> Option<u8> {
        if name.is_empty() {
            return None;
        }
        let mut i = 0;
        while i < self.tree.len() {
            let n = match self.tree.get(i) {
                Some(v) => v,
                None => return None,
            };
            if n.kind != K_NONE && n.parent == parent && n.name() == name {
                return Some(n.id);
            }
            i += 1;
        }
        None
    }

    /// 解析路径，返回末节点与跳数。
    pub fn resolve(&self, start: u8, path: &[u8], follow_final: bool) -> Result<(u8, u32), u32> {
        let mut hops = 0;
        let id = self.resolve_into(start, path, follow_final, &mut hops)?;
        Ok((id, hops))
    }

    fn resolve_into(
        &self,
        start: u8,
        path: &[u8],
        follow_final: bool,
        hops: &mut u32,
    ) -> Result<u8, u32> {
        let mut cur = start;
        let mut i = 0;
        while i < path.len() {
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
                None => return Err(SL_ENOENT),
            };
            if self.node(cur).map(|n| n.kind) != Some(K_DIR) {
                return Err(SL_ENOTDIR);
            }
            let child = match self.lookup(cur, comp) {
                Some(c) => c,
                None => return Err(SL_ENOENT),
            };
            let mut j = i;
            while j < path.len() && path[j] == b'/' {
                j += 1;
            }
            let last = j >= path.len();
            let kid = match self.node(child) {
                Some(k) => k,
                None => return Err(SL_ENOENT),
            };
            if kid.kind == K_LNK && (!last || follow_final) {
                *hops += 1;
                if *hops > MAX_HOPS {
                    return Err(SL_ELOOP);
                }
                let t = kid.target();
                if t.is_empty() {
                    return Err(SL_ENOENT);
                }
                let from = if t[0] == b'/' { 0 } else { kid.parent };
                cur = self.resolve_into(from, t, true, hops)?;
            } else {
                cur = child;
            }
            if !last && self.node(cur).map(|n| n.kind) != Some(K_DIR) {
                return Err(SL_ENOTDIR);
            }
        }
        Ok(cur)
    }

    /// 改写路径末节点。失败不落盘。
    pub fn change(
        &mut self,
        path: &[u8],
        spec: &[u8],
        mode: u8,
        caller: Caller,
    ) -> Result<u8, u32> {
        self.last_hops = 0;
        if path.is_empty() {
            self.errors += 1;
            return Err(SL_EEMPTY);
        }
        let follow = mode == M_FOLLOW;
        let mut hops = 0u32;
        let id = match self.resolve_into(0, path, follow, &mut hops) {
            Ok(v) => v,
            Err(e) => {
                self.last_hops = hops;
                if hops > self.hops_peak {
                    self.hops_peak = hops;
                }
                self.errors += 1;
                return Err(e);
            }
        };
        self.last_hops = hops;
        if hops > self.hops_peak {
            self.hops_peak = hops;
        }
        let s = match parse_spec(spec) {
            Ok(v) => v,
            Err(e) => {
                self.errors += 1;
                return Err(e);
            }
        };
        let before = match self.node(id) {
            Some(n) => n,
            None => {
                self.errors += 1;
                return Err(SL_ENOENT);
            }
        };
        if !caller.root && before.uid != caller.uid {
            self.errors += 1;
            return Err(SL_EPERM);
        }
        let mut after = before;
        if let Some(u) = s.user {
            after.uid = u;
        }
        if let Some(g) = s.group {
            after.gid = g;
        }
        self.tree[id as usize] = after;
        self.events.push(Event {
            node: id,
            mode,
            before_uid: before.uid,
            before_gid: before.gid,
            after_uid: after.uid,
            after_gid: after.gid,
        });
        Ok(id)
    }

    /// 直接改写某节点的身份，供递归遍历内部使用。
    fn touch(&mut self, id: u8, s: Spec) -> Result<(), u32> {
        let before = match self.node(id) {
            Some(n) => n,
            None => return Err(SL_ENOENT),
        };
        let mut after = before;
        if let Some(u) = s.user {
            after.uid = u;
        }
        if let Some(g) = s.group {
            after.gid = g;
        }
        self.tree[id as usize] = after;
        self.events.push(Event {
            node: id,
            mode: M_LCHOWN,
            before_uid: before.uid,
            before_gid: before.gid,
            after_uid: after.uid,
            after_gid: after.gid,
        });
        Ok(())
    }

    /// 递归改属：物理遍历不下降链接，逻辑遍历跟随链接。
    pub fn chown_r(&mut self, start: u8, spec: &[u8], follow: bool) -> Result<(usize, u32), u32> {
        let s = match parse_spec(spec) {
            Ok(v) => v,
            Err(e) => {
                self.errors += 1;
                return Err(e);
            }
        };
        let mut seen = [false; NODES];
        let mut order: Vec<u8> = Vec::new();
        let mut errs = 0u32;
        match self.node(start) {
            Some(n) => {
                let _ = self.touch(start, s);
                seen[start as usize] = true;
                if n.kind == K_DIR {
                    let _ = self.walk(start, s, follow, &mut seen, &mut order, &mut errs);
                }
            }
            None => {
                self.errors += 1;
                return Err(SL_ENOENT);
            }
        }
        order.insert(0, start);
        self.errors += errs;
        Ok((order.len(), errs))
    }

    fn walk(
        &mut self,
        dir: u8,
        s: Spec,
        follow: bool,
        seen: &mut [bool; NODES],
        order: &mut Vec<u8>,
        errs: &mut u32,
    ) -> Result<(), u32> {
        let mut i = 0;
        while i < self.tree.len() {
            let n = match self.tree.get(i) {
                Some(v) => *v,
                None => return Ok(()),
            };
            if n.kind == K_NONE || n.parent != dir {
                i += 1;
                continue;
            }
            let id = n.id;
            if n.kind == K_LNK {
                if follow {
                    let t = n.target();
                    let from = if t.is_empty() {
                        0
                    } else if t[0] == b'/' {
                        0
                    } else {
                        n.parent
                    };
                    if t.is_empty() {
                        *errs += 1;
                    } else {
                        match self.resolve(from, t, true) {
                            Ok((target, _)) => {
                                if (target as usize) < NODES && !seen[target as usize] {
                                    let _ = self.touch(target, s);
                                    seen[target as usize] = true;
                                    order.push(target);
                                    if self.node(target).map(|x| x.kind) == Some(K_DIR) {
                                        let _ = self.walk(target, s, follow, seen, order, errs);
                                    }
                                }
                            }
                            Err(_) => *errs += 1,
                        }
                    }
                } else {
                    let _ = self.touch(id, s);
                    if (id as usize) < NODES {
                        seen[id as usize] = true;
                    }
                    order.push(id);
                }
            } else {
                let _ = self.touch(id, s);
                if (id as usize) < NODES {
                    seen[id as usize] = true;
                }
                order.push(id);
                if n.kind == K_DIR {
                    let _ = self.walk(id, s, follow, seen, order, errs);
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
        while i < self.tree.len() {
            if let Some(n) = self.tree.get(i) {
                h = fold(h, n.id as u64);
                h = fold(h, n.kind as u64);
                h = fold(h, n.uid as u64);
                h = fold(h, n.gid as u64);
            }
            i += 1;
        }
        let mut j = 0;
        while j < self.events.len() {
            if let Some(e) = self.events.get(j) {
                h = fold(h, e.node as u64);
                h = fold(h, e.mode as u64);
                h = fold(h, e.after_uid as u64);
                h = fold(h, e.after_gid as u64);
            }
            j += 1;
        }
        h = fold(h, self.errors as u64);
        h = fold(h, self.hops_peak as u64);
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SL_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
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

/// 标准目录树。
pub fn build_tree() -> Vec<Node> {
    let mut t = Vec::new();
    t.push(Node::make(0, K_DIR, 0xff, 0, 0, b"/", b""));
    t.push(Node::make(1, K_DIR, 0, 0, 0, b"a", b""));
    t.push(Node::make(2, K_REG, 1, 500, 500, b"f", b""));
    t.push(Node::make(3, K_LNK, 1, 500, 500, b"l", b"f"));
    t.push(Node::make(4, K_LNK, 1, 500, 500, b"l2", b"l"));
    t.push(Node::blank());
    t.push(Node::make(6, K_LNK, 1, 500, 500, b"bad", b"/a/nope"));
    t.push(Node::make(7, K_LNK, 1, 500, 500, b"loop", b"/a/loop"));
    t.push(Node::make(8, K_DIR, 1, 500, 500, b"dir", b""));
    t.push(Node::make(9, K_LNK, 1, 500, 500, b"dl", b"dir"));
    t.push(Node::make(10, K_DIR, 0, 500, 500, b"b", b""));
    t.push(Node::make(11, K_LNK, 1, 500, 500, b"far", b"/b"));
    t
}

/// 单格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub path: [u8; PATH_CAP],
    pub plen: u8,
    pub spec: [u8; SPEC_CAP],
    pub slen: u8,
    pub mode: u8,
    pub expect_ok: bool,
    pub expect: u16,
}

impl Sample {
    /// 路径文本。
    pub fn path(&self) -> &[u8] {
        let n = self.plen as usize;
        if n > PATH_CAP {
            return &[];
        }
        match self.path.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }

    /// 实参文本。
    pub fn spec(&self) -> &[u8] {
        let n = self.slen as usize;
        if n > SPEC_CAP {
            return &[];
        }
        match self.spec.get(..n) {
            Some(v) => v,
            None => &[],
        }
    }
}

/// 一格结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    pub value: u16,
    pub hops: u32,
}

/// 样本账本。
pub struct Ledger {
    vfs: Vfs,
    samples: Vec<Sample>,
    outcomes: Vec<Outcome>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub fn new() -> Ledger {
        Ledger { vfs: Vfs::new(build_tree()), samples: Vec::new(), outcomes: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(
        &mut self,
        slot: u8,
        path: &[u8],
        spec: &[u8],
        mode: u8,
        ok: bool,
        expect: u16,
    ) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SL_EFROZEN);
        }
        if (slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(SL_EID);
        }
        if (slot as usize) < self.samples.len() {
            self.rejects += 1;
            return Err(SL_EDUP);
        }
        if (slot as usize) != self.samples.len() {
            self.rejects += 1;
            return Err(SL_EORDER);
        }
        if path.is_empty() || spec.is_empty() || path.len() > PATH_CAP || spec.len() > SPEC_CAP {
            self.rejects += 1;
            return Err(SL_ESPEC);
        }
        let mut s = Sample {
            path: [0; PATH_CAP],
            plen: path.len() as u8,
            spec: [0; SPEC_CAP],
            slen: spec.len() as u8,
            mode,
            expect_ok: ok,
            expect,
        };
        let mut i = 0;
        while i < path.len() {
            s.path[i] = path[i];
            i += 1;
        }
        i = 0;
        while i < spec.len() {
            s.spec[i] = spec[i];
            i += 1;
        }
        let before_events = self.vfs.events_len();
        let r = self.vfs.change(path, spec, mode, Caller::root());
        let hops = self.vfs.last_hops();
        let out = match r {
            Ok(id) => {
                if !ok || id as u16 != expect {
                    return Err(SL_EMISMATCH);
                }
                Outcome { ok: true, value: id as u16, hops }
            }
            Err(e) => {
                if ok || (e & 0xffff) as u16 != expect {
                    return Err(SL_EMISMATCH);
                }
                Outcome { ok: false, value: (e & 0xffff) as u16, hops }
            }
        };
        if self.vfs.events_len() != before_events + usize::from(out.ok) {
            return Err(SL_EBAL);
        }
        self.samples.push(s);
        self.outcomes.push(out);
        Ok(())
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        match self.samples.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 取结果。
    pub fn outcome(&self, i: usize) -> Option<Outcome> {
        match self.outcomes.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 成功格数。
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

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.outcomes.len() - self.ok_count()
    }

    /// 账本自洽。
    pub fn audit(&self) -> Result<(), u32> {
        if self.samples.len() != self.outcomes.len() || self.samples.is_empty() {
            return Err(SL_EBAL);
        }
        let mut i = 0;
        while i < self.samples.len() {
            let s = match self.samples.get(i) {
                Some(v) => v,
                None => return Err(SL_ENOBS),
            };
            let o = match self.outcomes.get(i) {
                Some(v) => v,
                None => return Err(SL_ENOBS),
            };
            if s.expect_ok != o.ok || s.expect != o.value {
                return Err(SL_EMISMATCH);
            }
            i += 1;
        }
        Ok(())
    }

    /// 访问目录树。
    pub fn vfs(&self) -> &Vfs {
        &self.vfs
    }

    /// 可变访问目录树。
    pub fn vfs_mut(&mut self) -> &mut Vfs {
        &mut self.vfs
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SL_EFROZEN);
        }
        self.root = self.vfs.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：以全新目录树重建账本并比对票号。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.vfs.seal() == self.vfs.seal() {
            Ok(fresh.vfs.seal())
        } else {
            Err(SL_ESEAL)
        }
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 构建十四样本账本：十成功、四失败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let noent = (SL_ENOENT & 0xffff) as u16;
    let eloop = (SL_ELOOP & 0xffff) as u16;
    let enotdir = (SL_ENOTDIR & 0xffff) as u16;
    let _ = l.register(0, b"/a/f", b"1001:1001", M_FOLLOW, true, 2);
    let _ = l.register(1, b"/a/l", b"1002:1002", M_FOLLOW, true, 2);
    let _ = l.register(2, b"/a/l", b"1003:1003", M_LCHOWN, true, 3);
    let _ = l.register(3, b"/a/l2", b"1004:1004", M_FOLLOW, true, 2);
    let _ = l.register(4, b"/a/l2", b"1005:1005", M_LCHOWN, true, 4);
    let _ = l.register(5, b"/a/bad", b"1006:1006", M_LCHOWN, true, 6);
    let _ = l.register(6, b"/a/bad", b"1007:1007", M_FOLLOW, false, noent);
    let _ = l.register(7, b"/a/loop", b"1008:1008", M_FOLLOW, false, eloop);
    let _ = l.register(8, b"/a/loop", b"1009:1009", M_LCHOWN, true, 7);
    let _ = l.register(9, b"/a/f", b":100", M_LCHOWN, true, 2);
    let _ = l.register(10, b"/x/missing", b"1010", M_FOLLOW, false, noent);
    let _ = l.register(11, b"/a/f/deep", b"1011", M_FOLLOW, false, enotdir);
    let _ = l.register(12, b"/", b"1012:1012", M_FOLLOW, true, 0);
    let _ = l.register(13, b"/a/dl", b":100", M_LCHOWN, true, 9);
    l
}

/// 四路注入：末链跟随、-h 停链、悬挂双语义、递归范围。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut l = build_ledger();
    let base = l.len() == SAMPLES
        && l.ok_count() == 10
        && l.err_count() == 4
        && l.audit().is_ok()
        && l.vfs().hops_peak() == MAX_HOPS + 1;
    let rep = l.replay();
    let root = match l.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    out[0] = base && root != 0 && rep == Ok(root);

    let mut f = Vfs::new(build_tree());
    let a = f.change(b"/a/l", b"2000:2000", M_FOLLOW, Caller::root()) == Ok(2);
    let b = f.change(b"/a/l", b"2001:2001", M_LCHOWN, Caller::root()) == Ok(3);
    out[1] = a && b && f.node(2).map(|n| n.uid) == Some(2000) && f.node(3).map(|n| n.uid) == Some(2001);

    let mut d = Vfs::new(build_tree());
    let x = d.change(b"/a/bad", b"2002", M_LCHOWN, Caller::root()) == Ok(6);
    let y = d.change(b"/a/bad", b"2002", M_FOLLOW, Caller::root()) == Err(SL_ENOENT);
    out[2] = x && y && d.node(6).map(|n| n.uid) == Some(2002);

    let mut p = Vfs::new(build_tree());
    let phys = p.chown_r(1, b"2100", false);
    let mut q = Vfs::new(build_tree());
    let logic = q.chown_r(1, b"2100", true);
    out[3] = phys == Ok((9, 0))
        && logic == Ok((4, 2))
        && p.node(9).map(|n| n.uid) == Some(2100)
        && q.node(9).map(|n| n.uid) == Some(500)
        && q.node(10).map(|n| n.uid) == Some(2100);

    out
}

/// 十三项判据。
pub fn run_unx11425_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11425.symlinkh");
    let l = build_ledger();

    s.add(
        "SL1-十四格账本齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.path() == b"/a/f" as &[u8]) == Some(true),
        "static",
    );

    s.add_dyn(
        "SL2-十应四拒分布正确",
        l.ok_count() == 10 && l.err_count() == 4 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "SL3-缺省跟随末链改写目标",
        l.outcome(1).map(|o| o.value) == Some(2)
            && l.outcome(2).map(|o| o.value) == Some(3)
            && l.vfs().node(2).map(|n| n.uid) == Some(1004),
        "static",
    );

    s.add(
        "SL4--h 停链改写链接自身",
        l.outcome(2).map(|o| o.value) == Some(3)
            && l.vfs().node(3).map(|n| n.uid) == Some(1003)
            && l.vfs().node(3).map(|n| n.gid) == Some(1003),
        "static",
    );

    s.add(
        "SL5-链式展开逐跳计数",
        l.outcome(3).map(|o| o.hops) == Some(2)
            && l.outcome(1).map(|o| o.hops) == Some(1)
            && l.outcome(4).map(|o| o.value) == Some(4),
        "static",
    );

    s.add(
        "SL6-悬挂链接双语义",
        l.outcome(5).map(|o| o.value) == Some(6)
            && l.outcome(6).map(|o| o.value) == Some((SL_ENOENT & 0xffff) as u16)
            && l.vfs().node(6).map(|n| n.uid) == Some(1006),
        "static",
    );

    s.add_dyn(
        "SL7-自环预算超限判环",
        l.outcome(7).map(|o| o.value) == Some((SL_ELOOP & 0xffff) as u16)
            && l.outcome(7).map(|o| o.hops) == Some(MAX_HOPS + 1)
            && l.outcome(8).map(|o| o.value) == Some(7),
        &format!("peak={}", l.vfs().hops_peak()),
    );

    s.add(
        "SL8-中间分量须为目录",
        l.outcome(11).map(|o| o.value) == Some((SL_ENOTDIR & 0xffff) as u16)
            && Vfs::new(build_tree()).resolve(0, b"/a/f", true).is_ok(),
        "static",
    );

    s.add(
        "SL9-缺失路径拒于解析",
        l.outcome(10).map(|o| o.value) == Some((SL_ENOENT & 0xffff) as u16)
            && Vfs::new(build_tree()).resolve(0, b"/x/missing", true) == Err(SL_ENOENT),
        "static",
    );

    s.add(
        "SL10-单侧属组止于链接",
        l.outcome(13).map(|o| o.value) == Some(9)
            && l.vfs().node(9).map(|n| n.gid) == Some(100)
            && l.vfs().node(9).map(|n| n.uid) == Some(500)
            && l.vfs().node(8).map(|n| n.gid) == Some(500),
        "static",
    );

    let mut p = Vfs::new(build_tree());
    let phys = p.chown_r(1, b"2200:2200", false);
    let mut q = Vfs::new(build_tree());
    let logic = q.chown_r(1, b"2200:2200", true);
    s.add_dyn(
        "SL11-物理与逻辑遍历分野",
        phys == Ok((9, 0))
            && logic == Ok((4, 2))
            && p.node(11).map(|n| n.uid) == Some(2200)
            && q.node(10).map(|n| n.uid) == Some(2200)
            && q.node(11).map(|n| n.uid) == Some(500),
        &format!("phys={:?} logic={:?}", phys, logic),
    );

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

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!(
        "ok={} err={} events={} peak={}",
        l.ok_count(),
        l.err_count(),
        l.vfs().events_len(),
        l.vfs().hops_peak()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_and_nofollow_split() {
        let mut v = Vfs::new(build_tree());
        assert_eq!(v.change(b"/a/l", b"1001", M_FOLLOW, Caller::root()), Ok(2));
        assert_eq!(v.change(b"/a/l", b"1002", M_LCHOWN, Caller::root()), Ok(3));
        assert_eq!(v.node(2).map(|n| n.uid), Some(1001));
        assert_eq!(v.node(3).map(|n| n.uid), Some(1002));
    }

    #[test]
    fn dangling_asymmetry() {
        let mut v = Vfs::new(build_tree());
        assert_eq!(v.change(b"/a/bad", b"7", M_LCHOWN, Caller::root()), Ok(6));
        assert_eq!(v.change(b"/a/bad", b"7", M_FOLLOW, Caller::root()), Err(SL_ENOENT));
        assert_eq!(v.errors(), 1);
    }

    #[test]
    fn loop_budget_errors() {
        let v = Vfs::new(build_tree());
        assert_eq!(v.resolve(0, b"/a/loop", true), Err(SL_ELOOP));
        assert_eq!(v.resolve(0, b"/a/loop", false), Ok((7, 0)));
    }

    #[test]
    fn chain_two_hops() {
        let v = Vfs::new(build_tree());
        assert_eq!(v.resolve(0, b"/a/l2", true), Ok((2, 2)));
        assert_eq!(v.resolve(0, b"/a/l", true), Ok((2, 1)));
    }

    #[test]
    fn recursion_modes_differ() {
        let mut p = Vfs::new(build_tree());
        assert_eq!(p.chown_r(1, b"300", false), Ok((9, 0)));
        let mut q = Vfs::new(build_tree());
        assert_eq!(q.chown_r(1, b"300", true), Ok((4, 2)));
        assert_eq!(q.node(10).map(|n| n.uid), Some(300));
        assert_eq!(q.node(9).map(|n| n.uid), Some(500));
    }

    #[test]
    fn ledger_distribution_and_audit() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 10);
        assert_eq!(l.err_count(), 4);
        assert!(l.audit().is_ok());
        assert!(summary(&l).starts_with("ok=10"));
    }

    #[test]
    fn refusal_matrix() {
        let mut l = Ledger::new();
        assert_eq!(l.register(0, b"/a/f", b"1", M_FOLLOW, true, 2), Ok(()));
        assert_eq!(l.register(0, b"/a/f", b"1", M_FOLLOW, true, 2), Err(SL_EDUP));
        assert_eq!(l.register(3, b"/a/f", b"1", M_FOLLOW, true, 2), Err(SL_EORDER));
        assert_eq!(l.register(14, b"/a/f", b"1", M_FOLLOW, true, 2), Err(SL_EID));
        assert_eq!(l.register(1, b"", b"1", M_FOLLOW, true, 2), Err(SL_ESPEC));
        assert_eq!(l.register(1, b"/a/f", b"1", M_FOLLOW, true, 9), Err(SL_EMISMATCH));
        let _ = l.freeze();
        assert_eq!(l.register(1, b"/a/f", b"1", M_FOLLOW, true, 2), Err(SL_EFROZEN));
    }

    #[test]
    fn permission_and_checks() {
        let mut v = Vfs::new(build_tree());
        assert_eq!(v.change(b"/a/f", b"900", M_FOLLOW, Caller::user(501)), Err(SL_EPERM));
        assert_eq!(v.change(b"/a/f", b"900", M_FOLLOW, Caller::user(500)), Ok(2));
        let s = run_unx11425_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
    }
}
