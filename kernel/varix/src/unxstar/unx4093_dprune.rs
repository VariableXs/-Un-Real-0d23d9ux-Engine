//! unx4093_dprune — UNX-F4093 大目录子树剪枝预研锚（诊断段 0x8DC0）。
//!
//! 判据原文：UNX-F4093-J1 子树整体失效接口（剪枝）对 10,000 节点树一次调用完成
//!           （耗时账有记录），剪枝后重建路径正确。
//! 可验性质（本模块自证面）：
//!   J1 10,000 节点树一次 prune_subtree 调用即完成：ledger.calls == 1，
//!      ledger.visited == 该子树实际节点数（一次遍历，不重不漏）；
//!   J2 剪枝后该子树内每个节点态为 Pruned，子树外节点态不变；
//!      被剪子树的根从父的 children 表中摘除（父链不留悬空项）；
//!   J3 剪枝后路径解析：落在剪枝区的路径返回 E_PRUNED（不得返回陈旧中间节点），
//!      剪枝区外的路径仍可正常解析到根；
//!   J4 重建：对剪枝过的路径调用 rebuild_path 后用同样的逐分量走查可重新解析成功，
//!      重建计数入账（rebuilt），且重建节点的 parent 链逐段正确；
//!   J5 非法调用拒：对不存在的 id（E_NO_NODE）、对已剪枝的 id（E_ALREADY）、
//!      空路径重建（E_PATH_EMPTY）、相位乱序（E_PHASE）——拒收不改任何账。
//! 设计纪律：
//!   1. 剪枝用显式栈迭代（Vec<u32> 当栈），不用递归：10,000 节点深树递归会爆栈，
//!      而本模块在 no_std 下没有 guard page 可依赖。
//!   2. 遍历前先把"访问数上限"记为树节点总数，遍历中若 exceed 即判表自坏
//!      （环或重复入栈），直接拒——这是"一次调用完成"的可验护栏。
//!   3. 节点不删只标态（Pruned）：删表会让下标位移，父链与 children 里的 id 全部失效；
//!      标态 + 从父表摘项就能表达"子树整体失效"。
//!   4. 重建只重建请求的那条链（不做全子树重建）：dcache 重建是惰性的，按路径逐段
//!      发生；本模块的 rebuild_path 就是这个语义的最小实现。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8DC0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8DC0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NO_NODE: u16 = SEG_BASE + 1; // 节点不存在
const E_ALREADY: u16 = SEG_BASE + 2; // 目标已剪枝
const E_PATH_EMPTY: u16 = SEG_BASE + 3; // 空路径
const E_PHASE: u16 = SEG_BASE + 4; // 相位乱序（并发调用/重入）
const E_PRUNED: u16 = SEG_BASE + 5; // 路径落在剪枝区
const E_DUP_NODE: u16 = SEG_BASE + 6; // 节点 id 重复
const E_BAL: u16 = SEG_BASE + 7; // 剪枝账不平
const E_DEPTH: u16 = SEG_BASE + 8; // 父链超深（环或病态）

/// 父链深度上限（大目录树深度余量）。
pub const MAX_DEPTH: u16 = 1024;

/// 节点态：在世 / 已剪枝。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PruneState {
    Live = 0,   // 在世（可解析）
    Pruned = 1, // 已剪枝（整体失效）
}

/// 剪枝相位机：Idle → Collecting → Pruning → Done。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrunePhase {
    Idle = 0,       // 空闲
    Collecting = 1, // 收子树节点（只读）
    Pruning = 2,    // 标态摘链（写）
    Done = 3,       // 完成
}

/// 剪枝摘要：一次调用的结果账。
pub struct PruneSummary {
    pub root: u32,   // 被剪子树根
    pub visited: u32, // 访问节点数（= 子树大小）
    pub pruned: u32,  // 实际标态数（应等于 visited）
    pub detached: bool, // 是否从父表摘除成功
}

/// 树节点。
pub struct PNode {
    pub id: u32,             // 节点 id
    pub parent: u32,         // 父 id（根指向自己）
    pub children: Vec<u32>,  // 子表
    pub state: PruneState,   // 态
    pub gen: u32,            // 代次（重建 +1）
}

/// 剪枝账。
pub struct PruneLedger {
    pub calls: u32,        // 剪枝调用次数
    pub visited: u32,      // 累计访问节点数
    pub pruned: u32,       // 累计剪枝节点数
    pub max_visit: u32,    // 单次最大访问数（耗时/规模账）
    pub rebuilt: u32,      // 重建节点数
    pub rejects: u32,      // 拒收次数
}

impl PruneLedger {
    fn new() -> PruneLedger {
        PruneLedger {
            calls: 0,
            visited: 0,
            pruned: 0,
            max_visit: 0,
            rebuilt: 0,
            rejects: 0,
        }
    }
}

/// 剪枝树：节点表 + 根 + 账 + 相位。
pub struct PruneTree {
    pub nodes: Vec<PNode>,  // 节点表
    pub root: u32,          // 根 id
    pub next_id: u32,       // 重建时的新 id 分配器
    pub led: PruneLedger,   // 账
    pub phase: PrunePhase,  // 相位
}

impl PruneTree {
    /// 建树：先放根。
    pub fn new(root: u32) -> PruneTree {
        let mut nodes: Vec<PNode> = Vec::new();
        nodes.push(PNode {
            id: root,
            parent: root,
            children: Vec::new(),
            state: PruneState::Live,
            gen: 0,
        });
        PruneTree {
            nodes,
            root,
            next_id: root + 1,
            led: PruneLedger::new(),
            phase: PrunePhase::Idle,
        }
    }

    /// 节点定位：稠密 id 走 O(1) 下标快路（id 与下标同构），否则退线性查。
    /// 万节点树每次 add/prune 都要定位，线性查会把总量推到 O(n^2)，故必须留快路。
    pub fn find(&self, id: u32) -> Option<usize> {
        let i = id as usize;
        if i < self.nodes.len() && self.nodes[i].id == id {
            return Some(i);
        }
        let mut k = 0usize;
        while k < self.nodes.len() {
            if self.nodes[k].id == id {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 挂子节点：id 唯一、父必须存在且在世。
    pub fn add_child(&mut self, id: u32, parent: u32) -> Result<(), u16> {
        if self.find(id).is_some() {
            self.led.rejects += 1;
            return Err(E_DUP_NODE);
        }
        let pi = match self.find(parent) {
            Some(i) => i,
            None => {
                self.led.rejects += 1;
                return Err(E_NO_NODE);
            }
        };
        if self.nodes[pi].state != PruneState::Live {
            self.led.rejects += 1;
            return Err(E_PRUNED);
        }
        self.nodes.push(PNode {
            id,
            parent,
            children: Vec::new(),
            state: PruneState::Live,
            gen: 0,
        });
        self.nodes[pi].children.push(id);
        if id >= self.next_id {
            self.next_id = id + 1;
        }
        Ok(())
    }

    /// 子树剪枝（J1/J2 主面）：显式栈迭代一次完成。
    pub fn prune_subtree(&mut self, id: u32) -> Result<PruneSummary, u16> {
        if self.phase != PrunePhase::Idle && self.phase != PrunePhase::Done {
            self.led.rejects += 1;
            return Err(E_PHASE); // 重入/并发调用
        }
        let ri = match self.find(id) {
            Some(i) => i,
            None => {
                self.led.rejects += 1;
                return Err(E_NO_NODE);
            }
        };
        if self.nodes[ri].state == PruneState::Pruned {
            self.led.rejects += 1;
            return Err(E_ALREADY);
        }
        // ① 收节点（只读）
        self.phase = PrunePhase::Collecting;
        let limit = self.nodes.len() as u32;
        let mut stack: Vec<u32> = Vec::new();
        let mut order: Vec<u32> = Vec::new();
        stack.push(id);
        while !stack.is_empty() {
            let cur = match stack.pop() {
                Some(x) => x,
                None => break,
            };
            if order.len() as u32 > limit {
                // 访问数超出树规模：表自坏（环/重复入栈），立即中止
                self.phase = PrunePhase::Idle;
                return Err(E_BAL);
            }
            order.push(cur);
            let ci = self.find(cur).ok_or(E_NO_NODE)?;
            let mut k = 0usize;
            while k < self.nodes[ci].children.len() {
                stack.push(self.nodes[ci].children[k]);
                k += 1;
            }
        }
        let visited = order.len() as u32;
        if visited == 0 || visited > limit {
            self.phase = PrunePhase::Idle;
            return Err(E_BAL);
        }
        // ② 标态（写）
        self.phase = PrunePhase::Pruning;
        // p 记"剪枝后处于 Pruned 态的节点数"：已是 Pruned 的也算（子树整体失效）
        let mut p = 0u32;
        let mut k = 0usize;
        while k < order.len() {
            if let Some(oi) = self.find(order[k]) {
                self.nodes[oi].state = PruneState::Pruned;
                p += 1;
            }
            k += 1;
        }
        // ③ 从父表摘项（仅当不是根）
        let mut detached = false;
        if id != self.root {
            let pid = self.nodes[ri].parent;
            if let Some(pi) = self.find(pid) {
                let mut na: Vec<u32> = Vec::new();
                let mut q = 0usize;
                while q < self.nodes[pi].children.len() {
                    if self.nodes[pi].children[q] != id {
                        na.push(self.nodes[pi].children[q]);
                    }
                    q += 1;
                }
                self.nodes[pi].children = na;
                detached = true;
            }
        }
        self.phase = PrunePhase::Done;
        self.led.calls += 1;
        self.led.visited += visited;
        self.led.pruned += p;
        if visited > self.led.max_visit {
            self.led.max_visit = visited;
        }
        if p != visited {
            return Err(E_BAL); // 标态数与访问数必须相等（在世全变 Pruned）
        }
        Ok(PruneSummary {
            root: id,
            visited,
            pruned: p,
            detached,
        })
    }

    /// 上行路径：任一段为 Pruned 即 E_PRUNED（J3）。
    pub fn path_of(&self, id: u32) -> Result<Vec<u32>, u16> {
        let mut out: Vec<u32> = Vec::new();
        let mut cur = id;
        let mut steps: u16 = 0;
        loop {
            if steps > MAX_DEPTH { return Err(E_DEPTH); }
            let ci = self.find(cur).ok_or(E_NO_NODE)?;
            if self.nodes[ci].state != PruneState::Live {
                return Err(E_PRUNED); // 剪枝区
            }
            out.push(cur);
            if cur == self.root {
                return Ok(out);
            }
            let p = self.nodes[ci].parent;
            if p == cur { return Err(E_DEPTH); }
            cur = p;
            steps += 1;
        }
    }

    /// 重建一条链（J4）：chain[0] 是根下的第一段分量，自顶向下逐段重建/复用，
    /// 返回链尾节点 id。已存在的在世节点直接复用（惰性重建），已剪枝节点复活并 gen +1。
    pub fn rebuild_path(&mut self, chain: &[u32]) -> Result<u32, u16> {
        if chain.is_empty() {
            self.led.rejects += 1;
            return Err(E_PATH_EMPTY);
        }
        let mut parent = self.root;
        let mut i = 0usize;
        while i < chain.len() {
            let id = chain[i];
            match self.find(id) {
                None => {
                    // 新节点：挂到 parent 下（分配新 id 不动，沿用请求 id）
                    self.add_child(id, parent)?;
                    self.led.rebuilt += 1;
                    parent = id;
                }
                Some(ci) => {
                    if self.nodes[ci].state == PruneState::Pruned {
                        self.nodes[ci].state = PruneState::Live;
                        self.nodes[ci].gen += 1;
                        self.nodes[ci].parent = parent;
                        // 重新挂回父表（若父表里还没有）
                        if let Some(pi) = self.find(parent) {
                            let mut has = false;
                            let mut k = 0usize;
                            while k < self.nodes[pi].children.len() {
                                if self.nodes[pi].children[k] == id {
                                    has = true;
                                }
                                k += 1;
                            }
                            if !has {
                                self.nodes[pi].children.push(id);
                            }
                        }
                        self.led.rebuilt += 1;
                    }
                    parent = id;
                }
            }
            i += 1;
        }
        Ok(parent)
    }

    /// 活的节点数 / 总数（账用）。
    pub fn census(&self) -> (u32, u32) {
        let mut live = 0u32;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.nodes[i].state == PruneState::Live {
                live += 1;
            }
            i += 1;
        }
        (live, self.nodes.len() as u32)
    }

    /// 树自审：每个节点的 parent 存在，且（除根外）父的 children 含自己。
    pub fn consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = &self.nodes[i];
            if n.id != self.root {
                match self.find(n.parent) {
                    None => return false,
                    Some(pi) => {
                        let mut has = false;
                        let mut k = 0usize;
                        while k < self.nodes[pi].children.len() {
                            if self.nodes[pi].children[k] == n.id {
                                has = true;
                            }
                            k += 1;
                        }
                        if !has && n.state == PruneState::Live {
                            return false; // 在世节点必须挂在父表里
                        }
                    }
                }
            }
            i += 1;
        }
        true
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_NODE => "节点不存在",
        E_ALREADY => "目标已剪枝",
        E_PATH_EMPTY => "空路径重建",
        E_PHASE => "剪枝相位乱序（重入）",
        E_PRUNED => "路径落在剪枝区",
        E_DUP_NODE => "节点 id 重复",
        E_BAL => "剪枝账不平（标态数 != 访问数）",
        E_DEPTH => "父链超深（环或病态）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：万节点树 → 一次剪枝 → 路径拒/通 → 重建 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建 10,000 节点完全二叉堆（parent = (i-1)/2），树自审通过。
    let mut t = PruneTree::new(0);
    let mut i = 1u32;
    while i < 10_000 {
        t.add_child(i, (i - 1) / 2)?;
        i += 1;
    }
    if t.nodes.len() != 10_000 { return Err(E_BAL); }
    if !t.consistent() { return Err(E_BAL); }
    let (live0, total0) = t.census();
    if live0 != total0 || total0 != 10_000 { return Err(E_BAL); }
    hits += 1;

    // R2：一次调用剪掉 3 号子树——calls == 1，visited == 实际子树大小（J1）。
    let s = t.prune_subtree(3)?;
    if t.led.calls != 1 { return Err(E_BAL); }
    if s.visited == 0 || s.visited != s.pruned { return Err(E_BAL); }
    if t.led.visited != s.visited || t.led.max_visit != s.visited { return Err(E_BAL); }
    if !s.detached {
        return Err(E_BAL); // 非根子树必须从父表摘除
    }
    if t.find(1).map(|x| t.nodes[x].children.iter().any(|c| *c == 3)) == Some(true) {
        return Err(E_BAL); // 父表里不得再有 3
    }
    hits += 1;

    // R3：子树内节点全部 Pruned，子树外（1 号、2 号）仍 Live（J2）。
    let mut k = 3u32;
    let mut inside = 0u32;
    while k < 10_000 {
        // 堆里 3 号子树 = 3, 7..8, ...：用"上行路径必过 3"判定
        if is_desc_of(k, 3) {
            if t.find(k).map(|x| t.nodes[x].state) != Some(PruneState::Pruned) { return Err(E_BAL); }
            inside += 1;
        }
        k += 1;
    }
    if inside != s.visited {
        return Err(E_BAL); // 独立枚举出的子树大小必须与遍历账一致
    }
    if t.find(1).map(|x| t.nodes[x].state) != Some(PruneState::Live) { return Err(E_BAL); }
    if t.find(2).map(|x| t.nodes[x].state) != Some(PruneState::Live) { return Err(E_BAL); }
    hits += 1;

    // R4：剪枝区路径解析拒（E_PRUNED），区外路径仍通（J3）。
    if t.path_of(7) != Err(E_PRUNED) { return Err(E_PRUNED); }
    if t.path_of(3) != Err(E_PRUNED) { return Err(E_PRUNED); }
    let p2 = t.path_of(2)?;
    if p2.len() != 2 || p2[0] != 2 || p2[1] != 0 {
        return Err(E_BAL); // 2 号路径 2→0
    }
    let p5 = t.path_of(5)?; // 5 号落在 2 号子树，未被剪
    if p5.len() != 3 || p5[0] != 5 || p5[2] != 0 { return Err(E_BAL); }
    hits += 1;

    // R5：重建链 1→3→7→15（从根下的第一段起）——逐段复活，gen +1，路径可再解析（J4）。
    let tail = t.rebuild_path(&[1, 3, 7, 15])?;
    if tail != 15 { return Err(E_BAL); }
    if t.led.rebuilt != 3 {
        return Err(E_BAL); // 1 号本来就是 Live（复用不计），3/7/15 各计一次
    }
    let pr = t.path_of(15)?;
    if pr.len() != 5 || pr[0] != 15 || pr[1] != 7 || pr[2] != 3 || pr[3] != 1 || pr[4] != 0 {
        return Err(E_BAL); // 重建后父链必须逐段正确
    }
    if t.find(3).map(|x| t.nodes[x].gen) != Some(1) {
        return Err(E_BAL); // 复活代次 +1
    }
    if t.find(1).map(|x| t.nodes[x].children.iter().any(|c| *c == 3)) != Some(true) {
        return Err(E_BAL); // 复活后要挂回父表
    }
    hits += 1;

    // R6：再剪一次（重建后的子树），calls 累加，访问数为新子树规模（剪枝可重复）。
    let s2 = t.prune_subtree(3)?;
    if t.led.calls != 2 || s2.visited == 0 { return Err(E_BAL); }
    if t.led.visited != s.visited + s2.visited { return Err(E_BAL); }
    if t.path_of(7) != Err(E_PRUNED) { return Err(E_PRUNED); }
    hits += 1;

    // I1：对已剪枝 id 再剪 → E_ALREADY；对不存在 id → E_NO_NODE；拒收不动账。
    let calls_before = t.led.calls;
    if !matches!(t.prune_subtree(3), Err(E_ALREADY)) { return Err(E_ALREADY); }
    if !matches!(t.prune_subtree(0xDEAD), Err(E_NO_NODE)) { return Err(E_NO_NODE); }
    if t.led.calls != calls_before { return Err(E_BAL); }
    hits += 1;

    // I2：空链重建 → E_PATH_EMPTY；重复挂已有 id → E_DUP_NODE（两路拒收）。
    let empty: Vec<u32> = Vec::new();
    if t.rebuild_path(&empty) != Err(E_PATH_EMPTY) { return Err(E_PATH_EMPTY); }
    if t.add_child(7, 1) != Err(E_DUP_NODE) { return Err(E_DUP_NODE); }
    hits += 1;

    // I3：剪根——根没有父，detached 必须为 false（边界面：根的父是自己）。
    let mut solo = PruneTree::new(0);
    solo.add_child(1, 0)?;
    solo.add_child(2, 0)?;
    let sr = solo.prune_subtree(0)?;
    if sr.detached {
        return Err(E_BAL); // 根不摘父
    }
    if sr.visited != 3 { return Err(E_BAL); }
    if solo.path_of(0) != Err(E_PRUNED) { return Err(E_PRUNED); }
    hits += 1;

    // I4：不可用父链（父已剪枝）挂新子 → E_PRUNED。
    if solo.add_child(9, 1) != Err(E_PRUNED) { return Err(E_PRUNED); }
    hits += 1;

    // I5：路径超深防护——构造长链 1,100 段，path_of 必报 E_DEPTH。
    let mut deep = PruneTree::new(0);
    let mut cur = 0u32;
    let mut d = 1u32;
    while d <= 1_100 {
        deep.add_child(d, cur)?;
        cur = d;
        d += 1;
    }
    match deep.path_of(cur) {
        Err(E_DEPTH) => {}
        _ => return Err(E_DEPTH),
    }
    hits += 1;

    // 终审：剪枝账与树态对得上（visited == pruned，calls == 2 + 1 次根剪在别的树）。
    if t.led.visited != t.led.pruned { return Err(E_BAL); }
    if t.led.max_visit > t.nodes.len() as u32 {
        return Err(E_BAL); // 单次访问数不得超过树规模
    }
    hits += 1;

    Ok(hits)
}

/// 判定 a 是否落在 b 的子树内（堆结构下：反复取父直到 0 或遇到 b）。
fn is_desc_of(a: u32, b: u32) -> bool {
    let mut cur = a;
    let mut steps = 0u32;
    while steps < 64 {
        if cur == b {
            return true;
        }
        if cur == 0 {
            return false;
        }
        cur = (cur - 1) / 2;
        steps += 1;
    }
    false
}
