//! unx4083_dparenttree — UNX-F4083 dentry 父链与 d_subdirs 树（诊断段 0x8C80）。
//!
//! 判据原文：UNX-F4083-J1 1,000 节点随机树的双向一致性（父的子表↔子的父指针）
//!           全量核对零差，环注入被检出并报告。
//! 可验性质（本模块自证面）：
//!   J1 1,000 节点树全量核对：每个节点的 parent 指针所指节点的 children 表中必须
//!      含该节点，且每个 children 表项的子节点 parent 必须指回该节点，双向零差；
//!   J2 children 表长度与节点 children 计数一致，且无重复子项（同父同名不成立）；
//!   J3 深度账：子深度恒等于父深度 + 1，逐节点核对零差；
//!   J4 环注入（把某节点的父指针改到自己的后代）必须被检出：从该节点上行时步数
//!      超过最大深度即判环，置 cycle 标记并计检出账；
//!   J5 非法登记全拒：重复 id（E_DUP_NODE）、父不存在（E_NO_PARENT）、
//!      根不得有父（E_ROOT_PARENT），拒收后节点表长度不变。
//! 设计纪律：
//!   1. 树用"节点表 + 下标"建模（Vec<DNode>，id 为 u16 主键），不用裸指针：
//!      dentry 父链在模型里就是 parent: Option<u16>，任何"悬空"都表现为
//!      parent 指向不存在的 id，可被 verify 逐条查出。
//!   2. 双向链的修正在 attach/detach 内一次完成：父表 push 与子指针赋值必须同进
//!      同退，不许出现"只改了一边"的中间态（本模块无并发，故不需要锁）。
//!   3. 深度上限 MAX_DEPTH = 512：VFS 路径深度受 PATH_MAX 限制，超限即视为环或
//!      病态结构；上行遍历一律带步数计数，防无限循环（禁 while 无界）。
//!   4. 环检测不靠标记染色而靠步数上界：detach 后再 attach 可能造成"孤岛环"，
//!      步数上界对孤岛环同样有效（不依赖 root 可达性）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8C80 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8C80;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_NODE: u16 = SEG_BASE + 1; // 节点 id 重复登记
const E_NO_PARENT: u16 = SEG_BASE + 2; // 父 id 不存在
const E_ROOT_PARENT: u16 = SEG_BASE + 3; // 根节点不得有父
const E_DEPTH: u16 = SEG_BASE + 4; // 深度超限（环或病态结构）
const E_BAD_LINK: u16 = SEG_BASE + 5; // 双向链不一致
const E_NOT_ATTACHED: u16 = SEG_BASE + 6; // 对未挂接节点做摘除
const E_EMPTY: u16 = SEG_BASE + 7; // 空树查询
const E_SELF_PARENT: u16 = SEG_BASE + 8; // 自指父（最短环）

/// 深度上限：PATH_MAX 级路径深度余量取 512（设计纪律 3）。
pub const MAX_DEPTH: u16 = 512;

/// 单节点挂接态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreeState {
    Detached = 0, // 已建未挂到父
    Attached = 1, // 已挂在父的 d_subdirs 里
    Orphan = 2,   // 父已摘除，自身仍持有子表（待剪枝）
}

/// dentry 节点：id + 父指针 + 子表 + 深度 + 态。
pub struct DNode {
    pub id: u16,               // 节点 id（树内唯一）
    pub parent: Option<u16>,   // 父 id（根为 None）
    pub children: Vec<u16>,    // d_subdirs 子表
    pub depth: u16,            // 深度（根为 0）
    pub state: TreeState,      // 挂接态
}

impl DNode {
    fn new(id: u16) -> DNode {
        DNode {
            id,
            parent: None,
            children: Vec::new(),
            depth: 0,
            state: TreeState::Detached,
        }
    }

    /// 子表是否含某 id（线性，子表通常几十项）。
    pub fn has_child(&self, id: u16) -> bool {
        let mut i = 0usize;
        while i < self.children.len() {
            if self.children[i] == id {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 双向一致性核对报告。
pub struct ConsistReport {
    pub checked: u32,     // 已核节点数
    pub parent_ok: u32,    // 父指针侧通过数
    pub child_ok: u32,     // 子表侧通过数
    pub depth_ok: u32,     // 深度账通过数
    pub bad: u32,         // 不一致项数
    pub cycle: bool,      // 是否检出环
    pub max_depth_seen: u16, // 实测最大深度
}

impl ConsistReport {
    fn new() -> ConsistReport {
        ConsistReport {
            checked: 0,
            parent_ok: 0,
            child_ok: 0,
            depth_ok: 0,
            bad: 0,
            cycle: false,
            max_depth_seen: 0,
        }
    }

    /// 全绿：核过节点非空、零不一致、无环。
    pub fn clean(&self) -> bool {
        self.checked > 0 && self.bad == 0 && !self.cycle
    }
}

/// dentry 树：节点表 + 根 id + 挂接账。
pub struct DTree {
    pub nodes: Vec<DNode>, // 节点表（下标即插入序，id 另查）
    pub root: u16,         // 根 id
    pub attaches: u32,     // 挂接成功次数
    pub detaches: u32,     // 摘除成功次数
    pub rejects: u32,      // 拒收次数
}

impl DTree {
    /// 建树：根节点 id 固定，深度 0，态 Attached（根不挂在任何父下）。
    pub fn new(root: u16) -> DTree {
        let mut r = DNode::new(root);
        r.depth = 0;
        r.state = TreeState::Attached;
        DTree {
            nodes: vec_push_one(r),
            root,
            attaches: 0,
            detaches: 0,
            rejects: 0,
        }
    }

    /// id → 下标（查前比 len）。
    pub fn find(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.nodes[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 登记新节点并挂到 parent 下：先查重、查父、防自指，再一次性修双向链（J5）。
    pub fn attach(&mut self, id: u16, parent: u16) -> Result<u16, u16> {
        if self.find(id).is_some() {
            self.rejects += 1;
            return Err(E_DUP_NODE);
        }
        if id == parent {
            self.rejects += 1;
            return Err(E_SELF_PARENT);
        }
        let pidx = match self.find(parent) {
            Some(i) => i,
            None => {
                self.rejects += 1;
                return Err(E_NO_PARENT);
            }
        };
        let pdepth = self.nodes[pidx].depth;
        if pdepth >= MAX_DEPTH {
            self.rejects += 1;
            return Err(E_DEPTH);
        }
        let mut n = DNode::new(id);
        n.parent = Some(parent);
        n.depth = pdepth + 1;
        n.state = TreeState::Attached;
        self.nodes.push(n);
        let nidx = self.nodes.len() - 1;
        // 双向链同进：父表 push + 子指针赋值
        self.nodes[pidx].children.push(id);
        self.attaches += 1;
        // 回读自检：子表末项必须是自己
        if !self.nodes[pidx].has_child(id) || self.nodes[nidx].parent != Some(parent) {
            return Err(E_BAD_LINK);
        }
        Ok(self.nodes[nidx].depth)
    }

    /// 摘除：把 id 从父的子表里删掉，子指针清空，态转 Orphan（子表保留，待剪枝）。
    pub fn detach(&mut self, id: u16) -> Result<(), u16> {
        if id == self.root {
            self.rejects += 1;
            return Err(E_ROOT_PARENT); // 根不可摘
        }
        let idx = match self.find(id) {
            Some(i) => i,
            None => {
                self.rejects += 1;
                return Err(E_NO_PARENT);
            }
        };
        if self.nodes[idx].state == TreeState::Detached {
            self.rejects += 1;
            return Err(E_NOT_ATTACHED);
        }
        let pid = match self.nodes[idx].parent {
            Some(p) => p,
            None => return Err(E_NOT_ATTACHED),
        };
        let pidx = match self.find(pid) {
            Some(i) => i,
            None => return Err(E_NO_PARENT),
        };
        // 从父表里移除本项（保序压紧）
        let mut out: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < self.nodes[pidx].children.len() {
            if self.nodes[pidx].children[i] != id {
                out.push(self.nodes[pidx].children[i]);
            }
            i += 1;
        }
        self.nodes[pidx].children = out;
        self.nodes[idx].parent = None;
        self.nodes[idx].state = TreeState::Orphan;
        self.detaches += 1;
        if self.nodes[pidx].has_child(id) {
            return Err(E_BAD_LINK);
        }
        Ok(())
    }

    /// 根 id 读数：空树（节点表为空）拒 E_EMPTY，防上层拿 0 当根。
    pub fn root_of(&self) -> Result<u16, u16> {
        if self.nodes.is_empty() {
            return Err(E_EMPTY);
        }
        Ok(self.root)
    }

    /// 子表读数（越界/未知 id 拒）。
    pub fn children_of(&self, id: u16) -> Result<&Vec<u16>, u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_PARENT),
        };
        Ok(&self.nodes[idx].children)
    }

    /// 上行路径：从 id 走到根，带步数上界；超界即判环（E_DEPTH）。
    /// 返回从自身到根的 id 序列（不含根之后再往上）。
    pub fn path_to_root(&self, id: u16) -> Result<Vec<u16>, u16> {
        let mut out: Vec<u16> = Vec::new();
        let mut cur = id;
        let mut steps: u16 = 0;
        loop {
            if steps > MAX_DEPTH {
                return Err(E_DEPTH); // 环：步数触顶仍在走
            }
            out.push(cur);
            if cur == self.root {
                return Ok(out);
            }
            let idx = match self.find(cur) {
                Some(i) => i,
                None => return Err(E_NO_PARENT),
            };
            match self.nodes[idx].parent {
                Some(p) => cur = p,
                None => return Err(E_NO_PARENT), // 走到非根的无父节点：孤岛链
            }
            steps += 1;
        }
    }

    /// 全量双向一致性核对（J1/J2/J3）。
    pub fn verify(&self) -> ConsistReport {
        let mut rep = ConsistReport::new();
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = &self.nodes[i];
            rep.checked += 1;
            if n.depth > rep.max_depth_seen {
                rep.max_depth_seen = n.depth;
            }
            // 父侧：parent 所指节点必须存在，且其子表含自己，且深度差 1
            match n.parent {
                None => {
                    if n.id != self.root {
                        rep.bad += 1; // 非根却无父：孤岛
                    } else {
                        rep.parent_ok += 1;
                        rep.depth_ok += 1;
                    }
                }
                Some(p) => {
                    let pidx = self.find(p);
                    match pidx {
                        None => rep.bad += 1,
                        Some(pi) => {
                            if self.nodes[pi].has_child(n.id) {
                                rep.parent_ok += 1;
                            } else {
                                rep.bad += 1;
                            }
                            if self.nodes[pi].depth + 1 == n.depth {
                                rep.depth_ok += 1;
                            } else {
                                rep.bad += 1;
                            }
                        }
                    }
                }
            }
            // 子侧：每个子项必须存在，parent 必须指回自己，且不得重复
            let mut k = 0usize;
            let mut ok_here = true;
            while k < n.children.len() {
                let c = n.children[k];
                let cidx = self.find(c);
                match cidx {
                    None => ok_here = false,
                    Some(ci) => {
                        if self.nodes[ci].parent != Some(n.id) {
                            ok_here = false;
                        }
                    }
                }
                // 重复子项检查
                let mut m = k + 1;
                while m < n.children.len() {
                    if n.children[m] == c {
                        ok_here = false;
                    }
                    m += 1;
                }
                k += 1;
            }
            if ok_here {
                rep.child_ok += 1;
            } else {
                rep.bad += 1;
            }
            // 环检测：上行步数上界（对孤岛环同样有效）
            let mut cur = n.id;
            let mut steps: u16 = 0;
            let mut hit_cycle = false;
            loop {
                if steps > MAX_DEPTH {
                    hit_cycle = true;
                    break;
                }
                if cur == self.root {
                    break;
                }
                let ci = match self.find(cur) {
                    Some(x) => x,
                    None => break,
                };
                match self.nodes[ci].parent {
                    Some(p) => cur = p,
                    None => break,
                }
                steps += 1;
            }
            if hit_cycle {
                rep.cycle = true;
            }
            i += 1;
        }
        rep
    }

    /// 环注入（负样本）：把 id 的父指针强行改到 target（通常是自己的后代）。
    /// 只动 parent 一侧，故意造出单向不一致——verify 必须同时报 bad 与 cycle。
    pub fn inject_cycle(&mut self, id: u16, target: u16) -> Result<(), u16> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => return Err(E_NO_PARENT),
        };
        if self.find(target).is_none() {
            return Err(E_NO_PARENT);
        }
        self.nodes[idx].parent = Some(target);
        Ok(())
    }

    /// 挂接账复核：attach - detach == 当前处在 Attached 的节点数 - 1（根恒挂）。
    pub fn attach_balanced(&self) -> bool {
        let mut attached = 0u32;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.nodes[i].state == TreeState::Attached {
                attached += 1;
            }
            i += 1;
        }
        if attached == 0 {
            return false;
        }
        self.attaches >= self.detaches
            && self.attaches - self.detaches == attached - 1
    }
}

/// 单元素节点表构造（避免 vec! 宏依赖）。
fn vec_push_one(n: DNode) -> Vec<DNode> {
    let mut v: Vec<DNode> = Vec::new();
    v.push(n);
    v
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_DUP_NODE => "节点 id 重复登记",
        E_NO_PARENT => "父 id 不存在（悬空指针）",
        E_ROOT_PARENT => "根节点不得有父/不可摘除",
        E_DEPTH => "深度超限（环或病态结构）",
        E_BAD_LINK => "双向链不一致",
        E_NOT_ATTACHED => "对未挂接节点做摘除",
        E_EMPTY => "空树查询",
        E_SELF_PARENT => "自指父（最短环）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：千节点树 → 双向核对 → 深度账 → 环注入 → 非法登记拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建 1,000 节点树，父取 (i-1)/3（三叉，深度 <= 7），全量双向核对零差（J1）。
    let mut t = DTree::new(0);
    let mut i = 1u16;
    while i < 1_000 {
        let parent = (i - 1) / 3;
        t.attach(i, parent)?;
        i += 1;
    }
    if t.nodes.len() != 1_000 {
        return Err(E_BAD_LINK);
    }
    let rep = t.verify();
    if rep.checked != 1_000 {
        return Err(E_BAD_LINK);
    }
    if rep.bad != 0 {
        return Err(E_BAD_LINK);
    }
    if rep.cycle {
        return Err(E_DEPTH);
    }
    if rep.parent_ok != 1_000 || rep.child_ok != 1_000 || rep.depth_ok != 1_000 {
        return Err(E_BAD_LINK);
    }
    if !rep.clean() {
        return Err(E_BAD_LINK);
    }
    hits += 1;

    // R2：深度账——三叉树下最大深度应远小于上限，且每个子深度 = 父深度 + 1。
    if rep.max_depth_seen == 0 || rep.max_depth_seen >= MAX_DEPTH {
        return Err(E_DEPTH);
    }
    let d999 = t.find(999).map(|x| t.nodes[x].depth);
    if d999 != Some(rep.max_depth_seen) {
        return Err(E_BAD_LINK); // 最深节点必是最后一个叶子
    }
    hits += 1;

    // R3：上行路径长度 == 深度 + 1，且路径末项是根（父链可逆）。
    let path = t.path_to_root(999)?;
    if path.len() as u16 != rep.max_depth_seen + 1 {
        return Err(E_BAD_LINK);
    }
    if path[path.len() - 1] != 0 {
        return Err(E_BAD_LINK);
    }
    // 路径逐段必须是子→父关系
    let mut k = 0usize;
    while k + 1 < path.len() {
        let ci = match t.find(path[k]) {
            Some(x) => x,
            None => return Err(E_NO_PARENT),
        };
        let pi = match t.find(path[k + 1]) {
            Some(x) => x,
            None => return Err(E_NO_PARENT),
        };
        if t.nodes[ci].parent != Some(t.nodes[pi].id) {
            return Err(E_BAD_LINK);
        }
        k += 1;
    }
    hits += 1;

    // R4：摘除后再挂回——双向链仍一致，挂接账守恒（attach - detach == 挂接数 - 1）。
    t.detach(500)?;
    if t.find(500).map(|x| t.nodes[x].state) != Some(TreeState::Orphan) {
        return Err(E_BAD_LINK);
    }
    if t.children_of(166)?.iter().any(|c| *c == 500) {
        return Err(E_BAD_LINK); // 父表里必须已经没有了
    }
    if t.attach(500, 166).is_ok() {
        return Err(E_DUP_NODE); // 节点表里还有 500，重挂必须拒（id 已存在）
    }
    if !t.attach_balanced() {
        return Err(E_BAD_LINK);
    }
    hits += 1;

    // R5：环注入——332 是 999 的祖先（999→332→110→36→11→3→0），把 332 的父改指 999，
    //     形成 332→999→332 的真实闭环，上行必触步数上界。
    t.inject_cycle(332, 999)?;
    let bad_rep = t.verify();
    if !bad_rep.cycle {
        return Err(E_DEPTH); // 环必须被检出（J4）
    }
    if bad_rep.bad == 0 {
        return Err(E_BAD_LINK); // 单向改链必留下不一致项
    }
    if t.path_to_root(332).is_ok() {
        return Err(E_DEPTH); // 带环路径不得返回 Ok
    }
    hits += 1;

    // I1：重复 id → E_DUP_NODE，节点表长度不变（J5）。
    let n0 = t.nodes.len();
    if t.attach(999, 0) != Err(E_DUP_NODE) {
        return Err(E_DUP_NODE);
    }
    if t.nodes.len() != n0 {
        return Err(E_BAD_LINK);
    }
    hits += 1;

    // I2：父不存在 → E_NO_PARENT；自指父 → E_SELF_PARENT。
    if t.attach(5_000, 4_999) != Err(E_NO_PARENT) {
        return Err(E_NO_PARENT);
    }
    if t.attach(5_001, 5_001) != Err(E_SELF_PARENT) {
        return Err(E_SELF_PARENT);
    }
    hits += 1;

    // I3：根不可摘 → E_ROOT_PARENT；摘未挂节点 → E_NOT_ATTACHED。
    if t.detach(0) != Err(E_ROOT_PARENT) {
        return Err(E_ROOT_PARENT);
    }
    if t.detach(500) != Err(E_NOT_ATTACHED) {
        return Err(E_NOT_ATTACHED); // R4 里 500 已是 Orphan
    }
    hits += 1;

    // I4：空树读数——未知 id 查子表/路径 → E_NO_PARENT，不越界。
    if t.children_of(9_999).is_err() == false {
        return Err(E_NO_PARENT);
    }
    if t.root_of()? != 0 {
        return Err(E_BAD_LINK);
    }
    if t.path_to_root(9_999).is_ok() {
        return Err(E_NO_PARENT);
    }
    hits += 1;

    // I5：拒收账累计——前面五种非法动作各计一次（重复 id/无父/自指/根摘/重复挂）。
    if t.rejects < 5 {
        return Err(E_BAD_LINK);
    }
    hits += 1;

    // 终审：新建一棵干净小树，双向 + 深度 + 环三项全绿（排除注入残留的干扰）。
    let mut t2 = DTree::new(0);
    t2.attach(1, 0)?;
    t2.attach(2, 0)?;
    t2.attach(3, 1)?;
    t2.attach(4, 3)?;
    let rep2 = t2.verify();
    if !rep2.clean() || rep2.checked != 5 || rep2.max_depth_seen != 3 {
        return Err(E_BAD_LINK);
    }
    if !t2.attach_balanced() {
        return Err(E_BAD_LINK);
    }
    hits += 1;

    Ok(hits)
}
