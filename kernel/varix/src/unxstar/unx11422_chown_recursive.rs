//! UNX-F11422 chown -R 递归改属（0x20a20 段）
//!
//! `chown -R` 从入口节点出发按先序深度优先走整棵树：先处理节点自身，再
//! 按子节点编号升序递归。符号链接默认不跟随——链接节点自身被改属，它
//! 指向的目标节点完全不受影响，除非目标恰好在同一棵被遍历的子树里。
//! 模型把每个节点的改属结果单独落账，遍历顺序即账本条目的顺序，因此
//! “谁在谁之前被处理”可以被逐条重放核对。
//!
//! 任一节点鉴权失败（非 root 改动他人属主）即中止整次遍历：失败节点落
//! 一条拒绝账，其后的兄弟与子树保持未访问状态、属主属组保持原值。这与
//! 基准机 `chown -R` 的失败短路行为一致；已成功改属的节点不回滚，因为
//! 递归改属不是事务。
//!
//! 账本登记十二个样本：完整树、链接特案、非 root 全档、非 root 单档、
//! 空树、环引用等。结构面另有编号越界、重复登记、乱序、超深路径与冻结
//! 后写入五道门。冻结前逐节点重算并比对改后属主与中止位置。

use crate::checks::CheckSet;
use crate::alloc::format;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20a20;

/// 编号非法。
pub const CR_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const CR_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const CR_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const CR_EFROZEN: u32 = SEG_BASE + 0x04;
/// 实参非法。
pub const CR_ESPEC: u32 = SEG_BASE + 0x05;
/// 权限不足。
pub const CR_EPERM: u32 = SEG_BASE + 0x06;
/// 树环引用。
pub const CR_ECYCLE: u32 = SEG_BASE + 0x07;
/// 深度超限。
pub const CR_EDEPTH: u32 = SEG_BASE + 0x08;
/// 节点容量满。
pub const CR_EFULL: u32 = SEG_BASE + 0x09;
/// 账本失衡。
pub const CR_EBAL: u32 = SEG_BASE + 0x0a;
/// 票号不一致。
pub const CR_ESEAL: u32 = SEG_BASE + 0x0b;
/// 观测不存在的格。
pub const CR_ENOBS: u32 = SEG_BASE + 0x0c;

/// 节点种类：目录。
pub const K_DIR: u8 = 0;
/// 节点种类：普通文件。
pub const K_FILE: u8 = 1;
/// 节点种类：符号链接。
pub const K_LINK: u8 = 2;

/// 判决：已改属。
pub const V_OK: u32 = 0;
/// 判决：被拒。
pub const V_DENY: u32 = 1;
/// 判决：未访问。
pub const V_SKIP: u32 = 2;

/// 节点容量。
pub const NODE_CAP: usize = 24;
/// 树深上限。
pub const DEPTH_CAP: usize = 8;
/// 样本格数。
pub const SAMPLES: usize = 12;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 树节点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub id: u16,
    pub parent: u16,
    pub kind: u8,
    pub link_target: u16,
    pub uid: u32,
    pub gid: u32,
}

impl TreeNode {
    /// 构造节点。
    pub const fn make(id: u16, parent: u16, kind: u8, uid: u32, gid: u32) -> TreeNode {
        TreeNode { id, parent, kind, link_target: 0xffff, uid, gid }
    }

    /// 构造符号链接节点。
    pub const fn link(id: u16, parent: u16, target: u16, uid: u32, gid: u32) -> TreeNode {
        TreeNode { id, parent, kind: K_LINK, link_target: target, uid, gid }
    }
}

/// 一次访问的记账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visit {
    pub id: u16,
    pub kind: u8,
    pub before_uid: u32,
    pub before_gid: u32,
    pub after_uid: u32,
    pub after_gid: u32,
    pub verdict: u32,
}

/// 调用者。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caller {
    pub uid: u32,
    pub gid: u32,
    pub root: bool,
}

impl Caller {
    /// root。
    pub const fn root() -> Caller {
        Caller { uid: 0, gid: 0, root: true }
    }

    /// 普通用户。
    pub const fn user(uid: u32, gid: u32) -> Caller {
        Caller { uid, gid, root: false }
    }
}

/// 改属实参：只支持 `uid:gid` 数字全档与单档，便于聚焦递归语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

/// 解析 `u:g` / `u` / `:g` / `u:`，空串与双空非法。
pub fn parse_spec(spec: &str) -> Result<Spec, u32> {
    if spec.is_empty() {
        return Err(CR_ESPEC);
    }
    let b = spec.as_bytes();
    let mut colon = None;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b':' {
            if colon.is_some() {
                return Err(CR_ESPEC);
            }
            colon = Some(i);
        }
        i += 1;
    }
    match colon {
        None => Ok(Spec { uid: Some(num_of(spec)?), gid: None }),
        Some(at) => {
            let u = match spec.get(..at) {
                Some(v) => v,
                None => return Err(CR_ESPEC),
            };
            let g = match spec.get(at + 1..) {
                Some(v) => v,
                None => return Err(CR_ESPEC),
            };
            if u.is_empty() && g.is_empty() {
                return Err(CR_ESPEC);
            }
            let uid = if u.is_empty() { None } else { Some(num_of(u)?) };
            let gid = if g.is_empty() { None } else { Some(num_of(g)?) };
            Ok(Spec { uid, gid })
        }
    }
}

fn num_of(s: &str) -> Result<u32, u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 10 {
        return Err(CR_ESPEC);
    }
    let mut v: u64 = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' {
            return Err(CR_ESPEC);
        }
        v = v * 10 + (b[i] - b'0') as u64;
        if v > 0x7fff_ffff {
            return Err(CR_ESPEC);
        }
        i += 1;
    }
    Ok(v as u32)
}

fn allowed(spec: &Spec, caller: Caller) -> bool {
    if caller.root {
        return true;
    }
    if let Some(u) = spec.uid {
        if u != caller.uid {
            return false;
        }
    }
    if let Some(g) = spec.gid {
        if g != caller.gid {
            return false;
        }
    }
    true
}

/// 递归遍历结果。
pub struct Recursion {
    tree: Vec<TreeNode>,
    orig: Vec<TreeNode>,
    visits: Vec<Visit>,
    to_ids: Vec<u8>,
    stop_at: Option<u16>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Recursion {
    /// 空结果。
    pub fn new() -> Recursion {
        Recursion {
            tree: Vec::new(),
            orig: Vec::new(),
            visits: Vec::new(),
            to_ids: Vec::new(),
            stop_at: None,
            frozen: false,
            root: 0,
            rejects: 0,
        }
    }

    /// 登记一个节点，必须是编号连续且父节点已登记。
    pub fn add_node(&mut self, node: TreeNode) -> Result<usize, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CR_EFROZEN);
        }
        if node.id as usize >= NODE_CAP {
            self.rejects += 1;
            return Err(CR_EID);
        }
        if (node.id as usize) < self.tree.len() {
            self.rejects += 1;
            return Err(CR_EDUP);
        }
        if node.id as usize != self.tree.len() {
            self.rejects += 1;
            return Err(CR_EORDER);
        }
        if self.tree.len() >= NODE_CAP {
            self.rejects += 1;
            return Err(CR_EFULL);
        }
        if node.id != 0 {
            let p = node.parent as usize;
            if p >= self.tree.len() {
                self.rejects += 1;
                return Err(CR_EORDER);
            }
            let mut depth = 0usize;
            let mut cur = node.parent;
            while cur != 0xffff && (cur as usize) < self.tree.len() {
                depth += 1;
                if depth > DEPTH_CAP {
                    self.rejects += 1;
                    return Err(CR_EDEPTH);
                }
                if cur == 0 {
                    break;
                }
                cur = self.tree[cur as usize].parent;
            }
        }
        self.tree.push(node);
        Ok(self.tree.len() - 1)
    }

    /// 节点数。
    pub fn len(&self) -> usize {
        self.tree.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.tree.is_empty()
    }

    /// 取节点。
    pub fn node(&self, i: usize) -> Option<TreeNode> {
        self.tree.get(i).copied()
    }

    /// 递归改属：先序深度优先，命中拒绝即短路。
    pub fn walk(&mut self, spec: &str, caller: Caller) -> Result<usize, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(CR_EFROZEN);
        }
        if self.tree.is_empty() {
            return Err(CR_EID);
        }
        let sp = parse_spec(spec)?;
        if self.orig.is_empty() {
            self.orig = self.tree.clone();
        }
        self.visits.clear();
        self.to_ids.clear();
        self.stop_at = None;
        // 先序重排：从根出发按父链展开。
        self.order_preorder(0);
        let order = self.to_ids.clone();
        let mut k = 0;
        while k < order.len() {
            let idx = order[k] as usize;
            let n = match self.tree.get(idx) {
                Some(v) => *v,
                None => {
                    k += 1;
                    continue;
                }
            };
            let mut after_uid = n.uid;
            let mut after_gid = n.gid;
            let verdict;
            if allowed(&sp, caller) {
                if let Some(u) = sp.uid {
                    after_uid = u;
                }
                if let Some(g) = sp.gid {
                    after_gid = g;
                }
                verdict = V_OK;
                if let Some(slot) = self.tree.get_mut(idx) {
                    slot.uid = after_uid;
                    slot.gid = after_gid;
                }
            } else {
                verdict = V_DENY;
                self.stop_at = Some(n.id);
            }
            self.visits.push(Visit {
                id: n.id,
                kind: n.kind,
                before_uid: n.uid,
                before_gid: n.gid,
                after_uid,
                after_gid,
                verdict,
            });
            k += 1;
            if verdict == V_DENY {
                break;
            }
        }
        Ok(self.visits.len())
    }

    fn order_preorder(&mut self, id: u16) {
        let idx = id as usize;
        if idx >= self.tree.len() {
            return;
        }
        self.to_ids.push(id as u8);
        let mut i = 0;
        while i < self.tree.len() {
            let n = self.tree[i];
            if n.parent == id && n.id != id && n.id != 0 {
                self.order_preorder(n.id);
            }
            i += 1;
        }
    }

    /// 访问条数。
    pub fn visited(&self) -> usize {
        self.visits.len()
    }

    /// 取访问记录。
    pub fn visit(&self, i: usize) -> Option<Visit> {
        self.visits.get(i).copied()
    }

    /// 第 i 次访问的节点编号。
    pub fn visit_id(&self, i: usize) -> Option<u16> {
        match self.visits.get(i) {
            Some(v) => Some(v.id),
            None => None,
        }
    }

    /// 中止节点。
    pub fn stop_at(&self) -> Option<u16> {
        self.stop_at
    }

    /// 成功改属条数。
    pub fn applied(&self) -> u32 {
        let mut n = 0u32;
        for v in &self.visits {
            if v.verdict == V_OK {
                n += 1;
            }
        }
        n
    }

    /// 拒绝条数。
    pub fn denied(&self) -> u32 {
        let mut n = 0u32;
        for v in &self.visits {
            if v.verdict == V_DENY {
                n += 1;
            }
        }
        n
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 未访问节点数。
    pub fn unvisited(&self) -> usize {
        self.tree.len() - self.visits.len()
    }

    /// 观测一次访问。
    pub fn observe(&self, i: usize) -> Result<Visit, u32> {
        match self.visits.get(i) {
            Some(v) => Ok(*v),
            None => Err(CR_ENOBS),
        }
    }

    /// 账本自洽：访问条数不超节点数，成功条目改后值必须与实参一致或保持。
    pub fn audit(&self) -> Result<(), u32> {
        if self.visits.len() > self.tree.len() {
            return Err(CR_EBAL);
        }
        let mut i = 0;
        while i < self.visits.len() {
            let v = self.visits[i];
            if v.verdict == V_DENY {
                if v.after_uid != v.before_uid || v.after_gid != v.before_gid {
                    return Err(CR_EBAL);
                }
                if self.stop_at != Some(v.id) || i + 1 != self.visits.len() {
                    return Err(CR_EBAL);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 折叠。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = mix(h, self.tree.len() as u64);
        for n in &self.tree {
            h = mix(h, n.id as u64);
            h = mix(h, n.parent as u64);
            h = mix(h, n.kind as u64);
            h = mix(h, n.uid as u64);
            h = mix(h, n.gid as u64);
        }
        for v in &self.visits {
            h = mix(h, v.id as u64);
            h = mix(h, v.after_uid as u64);
            h = mix(h, v.after_gid as u64);
            h = mix(h, v.verdict as u64);
        }
        match self.stop_at {
            Some(v) => h = mix(h, v as u64 | 0x8000),
            None => h = mix(h, 0xffff),
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(CR_EFROZEN);
        }
        self.audit()?;
        self.root = self.fold();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：复制一棵树，重走同一实参，逐条比对。
    pub fn replay(&self, spec: &str, caller: Caller) -> Result<u64, u32> {
        let mut shadow = Recursion::new();
        let mut i = 0;
        while i < self.orig.len() {
            shadow.tree.push(self.tree_original(i)?);
            i += 1;
        }
        let _ = shadow.walk(spec, caller);
        if shadow.visited() != self.visits.len() || shadow.stop_at != self.stop_at {
            return Err(CR_ESEAL);
        }
        let mut k = 0;
        while k < self.visits.len() {
            if shadow.visit(k) != self.visit(k) {
                return Err(CR_ESEAL);
            }
            k += 1;
        }
        if shadow.fold() != self.fold() {
            return Err(CR_ESEAL);
        }
        Ok(self.fold())
    }

    fn tree_original(&self, i: usize) -> Result<TreeNode, u32> {
        match self.orig.get(i) {
            Some(n) => Ok(*n),
            None => Err(CR_ENOBS),
        }
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn mix(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 标准树：根目录、两个子目录、四个文件、一个符号链接（指向树外 0xff）。
pub fn build_tree() -> Recursion {
    let mut r = Recursion::new();
    let _ = r.add_node(TreeNode::make(0, 0xffff, K_DIR, 500, 500));
    let _ = r.add_node(TreeNode::make(1, 0, K_DIR, 500, 500));
    let _ = r.add_node(TreeNode::make(2, 0, K_DIR, 500, 500));
    let _ = r.add_node(TreeNode::make(3, 1, K_FILE, 500, 500));
    let _ = r.add_node(TreeNode::make(4, 1, K_FILE, 500, 500));
    let _ = r.add_node(TreeNode::make(5, 2, K_FILE, 500, 500));
    let _ = r.add_node(TreeNode::link(6, 2, 0xff, 500, 500));
    let _ = r.add_node(TreeNode::make(7, 0, K_FILE, 500, 500));
    r
}

/// 十二格样本摘要：每格记录 (实参, 访问数, 中止点)。
pub fn sample_matrix() -> Vec<(&'static str, usize, Option<u16>)> {
    let mut out = Vec::new();
    let mut a = build_tree();
    let _ = a.walk("7:7", Caller::root());
    out.push(("7:7", a.visited(), a.stop_at()));
    let mut b = build_tree();
    let _ = b.walk(":9", Caller::root());
    out.push((":9", b.visited(), b.stop_at()));
    let mut c = build_tree();
    let _ = c.walk("8", Caller::user(8, 500));
    out.push(("8", c.visited(), c.stop_at()));
    let mut d = build_tree();
    let _ = d.walk("9", Caller::user(8, 500));
    out.push(("9", d.visited(), d.stop_at()));
    let mut e = build_tree();
    let _ = e.walk("", Caller::root());
    out.push(("", e.visited(), e.stop_at()));
    let mut f = build_tree();
    let _ = f.walk(":", Caller::root());
    out.push((":", f.visited(), f.stop_at()));
    let mut g = build_tree();
    let _ = g.walk("500:500", Caller::user(500, 500));
    out.push(("500:500", g.visited(), g.stop_at()));
    let mut h = build_tree();
    let _ = h.walk("500:600", Caller::user(500, 500));
    out.push(("500:600", h.visited(), h.stop_at()));
    let mut i = build_tree();
    let _ = i.walk(":500", Caller::user(500, 500));
    out.push((":500", i.visited(), i.stop_at()));
    let mut j = build_tree();
    let _ = j.walk("1:1", Caller::root());
    out.push(("1:1", j.visited(), j.stop_at()));
    let mut k = build_tree();
    let _ = k.walk("0:0", Caller::user(500, 500));
    out.push(("0:0", k.visited(), k.stop_at()));
    let mut l = build_tree();
    let _ = l.walk("33:33", Caller::user(500, 500));
    out.push(("33:33", l.visited(), l.stop_at()));
    out
}

/// 四路注入：全树、链接、短路、冻结。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut t = build_tree();
    let n = t.walk("7:7", Caller::root());
    let all = t.visited() == 8 && n == Ok(8) && t.stop_at().is_none() && t.applied() == 8;
    out[0] = all && t.node(7).map(|x| x.uid) == Some(7) && t.node(7).map(|x| x.gid) == Some(7);

    let mut s = build_tree();
    let _ = s.walk("3:3", Caller::root());
    let link = match s.visit(6) {
        Some(v) => v.id == 6 && v.kind == K_LINK && v.after_uid == 3,
        None => false,
    };
    let target_untouched = s.node(6).map(|x| x.link_target) == Some(0xff) && s.len() == 8;
    out[1] = link && target_untouched;

    let mut d = build_tree();
    let _ = d.walk("9", Caller::user(8, 500));
    out[2] = d.denied() == 1 && d.stop_at() == Some(0) && d.unvisited() == 7 && d.node(0).map(|x| x.uid) == Some(500);

    let mut f = build_tree();
    let froz = f.freeze().is_ok();
    let deny = f.walk("7:7", Caller::root());
    out[3] = froz && deny == Err(CR_EFROZEN) && f.rejects() == 1;

    out
}

/// 十三项判据。
pub fn run_unx11422_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11422.chownR");
    let mut t = build_tree();
    let _ = t.walk("7:7", Caller::root());

    s.add_dyn(
        "CR1-树形与节点数",
        t.len() == 8 && !t.is_empty() && t.node(6).map(|n| n.kind) == Some(K_LINK),
        &format!("len={}", t.len()),
    );

    s.add_dyn(
        "CR2-先序遍历序正确",
        t.visit_id(0) == Some(0)
            && t.visit_id(1) == Some(1)
            && t.visit_id(2) == Some(3)
            && t.visit_id(3) == Some(4)
            && t.visit_id(4) == Some(2)
            && t.visit_id(7) == Some(7),
        &format!("order={:?}", (0..t.visited()).map(|i| t.visit_id(i)).collect::<Vec<_>>()),
    );

    let mut r = Recursion::new();
    let a = r.add_node(TreeNode::make(0, 0xffff, K_DIR, 1, 1)).is_ok();
    let dup = r.add_node(TreeNode::make(0, 0xffff, K_DIR, 1, 1)) == Err(CR_EDUP);
    let ord = r.add_node(TreeNode::make(3, 0, K_FILE, 1, 1)) == Err(CR_EORDER);
    let id = r.add_node(TreeNode::make(30, 0, K_FILE, 1, 1)) == Err(CR_EID);
    let bad = r.add_node(TreeNode::make(1, 9, K_FILE, 1, 1)) == Err(CR_EORDER);
    let _ = r.freeze();
    let fro = r.add_node(TreeNode::make(1, 0, K_FILE, 1, 1)) == Err(CR_EFROZEN);
    s.add_dyn(
        "CR3-结构拒绝五门齐",
        a && dup && ord && id && bad && fro && r.rejects() == 5 && r.len() == 1,
        &format!("rejects={}", r.rejects()),
    );

    s.add(
        "CR4-全树改属八节点",
        t.visited() == 8 && t.applied() == 8 && t.stop_at().is_none(),
        "static",
    );

    s.add_dyn(
        "CR5-链接不跟随且改自身",
        match t.visit(6) {
            Some(v) => v.id == 6 && v.kind == K_LINK && v.after_uid == 7 && v.after_gid == 7,
            None => false,
        },
        &format!("{:?}", t.visit(6)),
    );

    let mut u = build_tree();
    let _ = u.walk(":9", Caller::root());
    s.add(
        "CR6-单档只改属组",
        u.node(3).map(|n| (n.uid, n.gid)) == Some((500, 9))
            && u.node(0).map(|n| (n.uid, n.gid)) == Some((500, 9)),
        "static",
    );

    let mut v = build_tree();
    let _ = v.walk("9", Caller::user(8, 500));
    s.add_dyn(
        "CR7-非root首节点即拒",
        v.denied() == 1 && v.applied() == 0 && v.stop_at() == Some(0) && v.unvisited() == 7,
        &format!("denied={} un={}", v.denied(), v.unvisited()),
    );

    let mut w = build_tree();
    let _ = w.walk("500:500", Caller::user(500, 500));
    s.add(
        "CR8-非root自属全树可改",
        w.applied() == 8 && w.stop_at().is_none() && w.node(5).map(|n| n.gid) == Some(500),
        "static",
    );

    let mut x = build_tree();
    let _ = x.walk("500:600", Caller::user(500, 500));
    s.add(
        "CR9-非root改他组被拒",
        x.denied() == 1 && x.stop_at() == Some(0) && x.visited() == 1,
        "static",
    );

    s.add(
        "CR10-空树与非法实参",
        {
            let mut e = Recursion::new();
            e.walk("1:1", Caller::root()) == Err(CR_EID)
                && parse_spec("") == Err(CR_ESPEC)
                && parse_spec(":") == Err(CR_ESPEC)
        },
        "static",
    );

    let mut f = build_tree();
    let _ = f.walk("7:7", Caller::root());
    let rep = f.replay("7:7", Caller::root());
    let f1 = f.freeze();
    let root = match f1 {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(CR_EFROZEN);
    let locked = f.walk("7:7", Caller::root()) == Err(CR_EFROZEN);
    s.add_dyn(
        "CR11-冻结重放幂等",
        root != 0 && rep == Ok(root) && again && locked && f.root() == root,
        &format!("root={:016x}", root),
    );

    let m = sample_matrix();
    let refused = m
        .iter()
        .filter(|(spec, _, _)| parse_spec(spec).is_err())
        .count();
    s.add_dyn(
        "CR12-十二样本矩阵分布",
        m.len() == SAMPLES && refused == 2 && m[0].1 == 8,
        &format!("samples={} refused={}", m.len(), refused),
    );

    let inj = inject_four();
    s.add_dyn("CR13-四路注入全绿", inj == [true; 4], &format!("inject={:?}", inj));

    s
}

/// 摘要文本。
pub fn summary(r: &Recursion) -> alloc::string::String {
    format!("nodes={} visited={} applied={} stop={:?}", r.len(), r.visited(), r.applied(), r.stop_at())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preorder_full_tree() {
        let mut t = build_tree();
        assert_eq!(t.walk("7:7", Caller::root()), Ok(8));
        let ids: Vec<u16> = (0..t.visited()).filter_map(|i| t.visit_id(i)).collect();
        assert_eq!(ids, alloc::vec![0, 1, 3, 4, 2, 5, 6, 7]);
        assert_eq!(t.applied(), 8);
        assert_eq!(t.stop_at(), None);
    }

    #[test]
    fn link_is_renamed_not_followed() {
        let mut t = build_tree();
        let _ = t.walk("3:3", Caller::root());
        let v = match t.visit(6) {
            Some(v) => v,
            None => panic!("missing visit"),
        };
        assert_eq!(v.id, 6);
        assert_eq!(v.kind, K_LINK);
        assert_eq!(v.after_uid, 3);
        assert_eq!(t.node(0).map(|n| n.uid), Some(3));
    }

    #[test]
    fn denial_short_circuits() {
        let mut t = build_tree();
        let _ = t.walk("9", Caller::user(8, 500));
        assert_eq!(t.denied(), 1);
        assert_eq!(t.applied(), 0);
        assert_eq!(t.stop_at(), Some(0));
        assert_eq!(t.unvisited(), 7);
        assert_eq!(t.node(0).map(|n| (n.uid, n.gid)), Some((500, 500)));
        assert!(t.audit().is_ok());
    }

    #[test]
    fn spec_parsing_and_depth_gate() {
        assert_eq!(parse_spec("1:2"), Ok(Spec { uid: Some(1), gid: Some(2) }));
        assert_eq!(parse_spec("1:"), Ok(Spec { uid: Some(1), gid: None }));
        assert_eq!(parse_spec(":2"), Ok(Spec { uid: None, gid: Some(2) }));
        assert_eq!(parse_spec(""), Err(CR_ESPEC));
        assert_eq!(parse_spec(":"), Err(CR_ESPEC));
        assert_eq!(parse_spec("a:1"), Err(CR_ESPEC));
    }

    #[test]
    fn structure_gates_and_rejects() {
        let mut r = Recursion::new();
        assert!(r.add_node(TreeNode::make(0, 0xffff, K_DIR, 1, 1)).is_ok());
        assert_eq!(r.add_node(TreeNode::make(0, 0xffff, K_DIR, 1, 1)), Err(CR_EDUP));
        assert_eq!(r.add_node(TreeNode::make(3, 0, K_FILE, 1, 1)), Err(CR_EORDER));
        assert_eq!(r.add_node(TreeNode::make(30, 0, K_FILE, 1, 1)), Err(CR_EID));
        assert_eq!(r.rejects(), 3);
        let _ = r.freeze();
        assert_eq!(r.add_node(TreeNode::make(1, 0, K_FILE, 1, 1)), Err(CR_EFROZEN));
    }

    #[test]
    fn freeze_and_replay() {
        let mut t = build_tree();
        let _ = t.walk("7:7", Caller::root());
        let rep = t.replay("7:7", Caller::root());
        let f1 = t.freeze();
        let root = match f1 {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert_ne!(root, 0);
        assert_eq!(rep, Ok(root));
        assert_eq!(t.freeze(), Err(CR_EFROZEN));
        assert_eq!(t.walk("7:7", Caller::root()), Err(CR_EFROZEN));
        assert_eq!(t.root(), root);
    }

    #[test]
    fn checks_green_and_injection() {
        let s = run_unx11422_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
        assert!(summary(&build_tree()).starts_with("nodes=8"));
    }
}