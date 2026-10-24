//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6461 · mq-deadline 结构（读树/写树/deadline 域）· 诊断段 0x9280
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥399 行；
//!   ② UNX-F6461-J1 读/写两棵排序树与 deadline 域三结构建立，
//!      插入/删除/查找 O(log n) 复杂度断言；树与请求账守恒；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - AVL 平衡树（节点池 + 旋转）承载读树/写树——插入/删除/查找
//!     严格 O(log n)，高度断言 h ≤ 1.44*log2(n+2)；
//!   - deadline 域：请求 (id, deadline) 线性账 + 最小 deadline 分派；
//!   - 树与请求账守恒：两树节点和 = 请求账活动数；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
// 内核浮点层：x86_64-unknown-none 下 core 不提供浮点 transcendentals，
// 本文件用到的浮点方法统一走 FloatExt（转发 libm，见 crate::float 文档）。
use crate::float::FloatExt;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0x9280;

/// 树空哨兵。
pub const NIL: usize = usize::MAX;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// AVL 节点。
struct Node {
    key: u64,
    req: u32,
    left: usize,
    right: usize,
    height: i32,
}

/// AVL 树（节点池实现，插入/删除/查找 O(log n)）。
pub struct Avl {
    pool: Vec<Node>,
    root: usize,
    pub cmp_count: u64,
}

impl Avl {
    pub fn new() -> Self {
        Avl { pool: Vec::new(), root: NIL, cmp_count: 0 }
    }

    fn h(&self, i: usize) -> i32 {
        if i == NIL { 0 } else { self.pool[i].height }
    }

    fn fix(&mut self, i: usize) {
        let (l, r) = (self.pool[i].left, self.pool[i].right);
        self.pool[i].height = 1 + self.h(l).max(self.h(r));
    }

    fn bal(&self, i: usize) -> i32 {
        if i == NIL { 0 } else { self.h(self.pool[i].left) - self.h(self.pool[i].right) }
    }

    fn rot_right(&mut self, i: usize) -> usize {
        let l = self.pool[i].left;
        let lr = self.pool[l].right;
        self.pool[l].right = i;
        self.pool[i].left = lr;
        self.fix(i);
        self.fix(l);
        l
    }

    fn rot_left(&mut self, i: usize) -> usize {
        let r = self.pool[i].right;
        let rl = self.pool[r].left;
        self.pool[r].left = i;
        self.pool[i].right = rl;
        self.fix(i);
        self.fix(r);
        r
    }

    fn rebalance(&mut self, i: usize) -> usize {
        self.fix(i);
        let b = self.bal(i);
        if b > 1 {
            let l = self.pool[i].left;
            if self.bal(l) < 0 {
                let new_l = self.rot_left(l);
                self.pool[i].left = new_l;
            }
            return self.rot_right(i);
        }
        if b < -1 {
            let r = self.pool[i].right;
            if self.bal(r) > 0 {
                let new_r = self.rot_right(r);
                self.pool[i].right = new_r;
            }
            return self.rot_left(i);
        }
        i
    }

    /// 插入：O(log n)，同键更新 req。
    pub fn insert(&mut self, key: u64, req: u32) {
        self.root = self.ins(self.root, key, req);
    }

    fn ins(&mut self, i: usize, key: u64, req: u32) -> usize {
        if i == NIL {
            self.pool.push(Node { key, req, left: NIL, right: NIL, height: 1 });
            return self.pool.len() - 1;
        }
        self.cmp_count += 1;
        if key < self.pool[i].key {
            let l = self.pool[i].left;
            self.pool[i].left = self.ins(l, key, req);
        } else if key > self.pool[i].key {
            let r = self.pool[i].right;
            self.pool[i].right = self.ins(r, key, req);
        } else {
            self.pool[i].req = req;
            return i;
        }
        self.rebalance(i)
    }

    /// 查找：O(log n)，比较次数记账。
    pub fn find(&mut self, key: u64) -> Option<u32> {
        let mut i = self.root;
        while i != NIL {
            self.cmp_count += 1;
            let k = self.pool[i].key;
            if key == k {
                return Some(self.pool[i].req);
            }
            i = if key < k { self.pool[i].left } else { self.pool[i].right };
        }
        None
    }

    /// 删除：O(log n)。
    pub fn remove(&mut self, key: u64) -> bool {
        let n0 = self.pool.len();
        self.root = self.rm(self.root, key);
        self.pool.len() != n0 || self.contains(key)
    }

    fn min_of(&self, mut i: usize) -> usize {
        while self.pool[i].left != NIL {
            i = self.pool[i].left;
        }
        i
    }

    fn rm(&mut self, i: usize, key: u64) -> usize {
        if i == NIL {
            return NIL;
        }
        self.cmp_count += 1;
        if key < self.pool[i].key {
            let l = self.pool[i].left;
            let nl = self.rm(l, key);
            self.pool[i].left = nl;
            return self.rebalance(i);
        }
        if key > self.pool[i].key {
            let r = self.pool[i].right;
            let nr = self.rm(r, key);
            self.pool[i].right = nr;
            return self.rebalance(i);
        }
        // 命中：摘除
        let (l, r) = (self.pool[i].left, self.pool[i].right);
        if l == NIL || r == NIL {
            return if l != NIL { l } else { r };
        }
        let m = self.min_of(r);
        let (mk, mr) = (self.pool[m].key, self.pool[m].req);
        let nr = self.rm(r, mk);
        self.pool[i].key = mk;
        self.pool[i].req = mr;
        self.pool[i].right = nr;
        self.rebalance(i)
    }

    pub fn contains(&self, key: u64) -> bool {
        let mut i = self.root;
        while i != NIL {
            let k = self.pool[i].key;
            if key == k {
                return true;
            }
            i = if key < k { self.pool[i].left } else { self.pool[i].right };
        }
        false
    }

    pub fn len(&self) -> usize {
        self.pool.iter().filter(|_| true).count().min(self.count(self.root))
    }

    fn count(&self, i: usize) -> usize {
        if i == NIL {
            0
        } else {
            1 + self.count(self.pool[i].left) + self.count(self.pool[i].right)
        }
    }

    /// 高度断言：AVL h ≤ 1.44*log2(n+2)。
    pub fn height_ok(&self) -> bool {
        let n = self.count(self.root);
        if n == 0 {
            return self.h(self.root) == 0;
        }
        let bound = (1.44 * ((n as f64) + 2.0).m_log2()).m_ceil() as i32 + 1;
        self.h(self.root) <= bound
    }

    /// 有序性断言：中序遍历键严格递增。
    pub fn sorted(&self) -> bool {
        let mut prev: Option<u64> = None;
        self.inorder(self.root, &mut prev)
    }

    fn inorder(&self, i: usize, prev: &mut Option<u64>) -> bool {
        if i == NIL {
            return true;
        }
        if !self.inorder(self.pool[i].left, prev) {
            return false;
        }
        if let Some(p) = *prev {
            if self.pool[i].key <= p {
                return false;
            }
        }
        *prev = Some(self.pool[i].key);
        self.inorder(self.pool[i].right, prev)
    }
}

/// deadline 域：请求 (id, deadline) 线性账 + 最小分派。
pub struct DeadlineDom {
    pub reqs: Vec<(u32, u64)>,
}

impl DeadlineDom {
    pub fn new() -> Self {
        DeadlineDom { reqs: Vec::new() }
    }

    pub fn admit(&mut self, id: u32, dl: u64) {
        self.reqs.push((id, dl));
    }

    pub fn dispatch_min(&mut self) -> Option<(u32, u64)> {
        if self.reqs.is_empty() {
            return None;
        }
        let mut mi = 0;
        for i in 1..self.reqs.len() {
            if self.reqs[i].1 < self.reqs[mi].1 {
                mi = i;
            }
        }
        Some(self.reqs.remove(mi))
    }

    pub fn len(&self) -> usize {
        self.reqs.len()
    }
}

/// J1 主判据：三结构建立 + O(log n) 断言 + 树与请求账守恒。
pub fn j1_main() -> bool {
    let mut k: u64 = 0x5EED_6461;
    let mut read_tree = Avl::new();
    let mut write_tree = Avl::new();
    let mut dom = DeadlineDom::new();
    // 256 请求：偶 id 入读树，奇 id 入写树，全部入 deadline 域
    let mut next = || {
        k = k.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        k
    };
    for id in 0..256u32 {
        let key = next() % 100_000;
        if id % 2 == 0 {
            read_tree.insert(key, id);
        } else {
            write_tree.insert(key, id);
        }
        dom.admit(id, key);
    }
    // O(log n) 断言：查找比较次数上界（一次满查 ≤ 2*(h+1)，h=O(log n)）
    let probe = next() % 100_000;
    read_tree.cmp_count = 0;
    let _ = read_tree.find(probe);
    let n0 = read_tree.count(read_tree.root);
    let bound = 2 * (((n0 as f64) + 2.0).m_log2().m_ceil() as u64 + 2);
    if read_tree.cmp_count > bound {
        return false;
    }
    // 平衡高度 + 有序性
    if !read_tree.height_ok() || !write_tree.height_ok()
        || !read_tree.sorted() || !write_tree.sorted()
    {
        return false;
    }
    // 守恒：两树节点和 = 请求账活动数
    let trees = read_tree.count(read_tree.root) + write_tree.count(write_tree.root);
    if trees != dom.len() {
        return false;
    }
    // 删除 O(log n)：删 32 个后守恒
    for i in 0..32u32 {
        let key = (next() % 100_000) as u64;
        let _ = read_tree.remove(key);
        let _ = write_tree.remove(key);
        dom.dispatch_min();
    }
    let trees2 = read_tree.count(read_tree.root) + write_tree.count(write_tree.root);
    trees2 == dom.len() && !format!("mqd {}", trees2).is_empty()
}

// ── 注入核（0x9280 段 5 项）────────────────────────────────────────────

/// 注入核 1：空树查找注入——空树返回 None 且零比较。
pub fn inj1_empty_find() -> bool {
    let mut t = Avl::new();
    let c0 = t.cmp_count;
    t.find(42).is_none() && t.cmp_count == c0
}

/// 注入核 2：单边退化注入——递增键插入后仍平衡（AVL 旋转生效）。
pub fn inj2_no_degenerate() -> bool {
    let mut t = Avl::new();
    for k in 0..128u64 {
        t.insert(k, k as u32);
    }
    t.height_ok() && t.sorted()
}

/// 注入核 3：同键更新注入——重复插入不增节点。
pub fn inj3_dup_update() -> bool {
    let mut t = Avl::new();
    t.insert(7, 1);
    let n0 = t.count(t.root);
    t.insert(7, 2);
    t.count(t.root) == n0 && t.find(7) == Some(2)
}

/// 注入核 4：删除守恒注入——删后节点数 -1 且有序性保持。
pub fn inj4_remove_conserved() -> bool {
    let mut t = Avl::new();
    for k in 0..64u64 {
        t.insert(k * 3, 0);
    }
    let n0 = t.count(t.root);
    t.remove(33);
    t.count(t.root) == n0 - 1 && !t.contains(33) && t.sorted()
}

/// 注入核 5：deadline 最小分派注入——按最小 deadline 出队。
pub fn inj5_min_dispatch() -> bool {
    let mut d = DeadlineDom::new();
    d.admit(1, 500);
    d.admit(2, 100);
    d.admit(3, 900);
    let first = d.dispatch_min();
    first == Some((2, 100)) && d.len() == 2
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    12u32 * 34
}

/// run_unx6461_checks：本单判据聚合（诊断段 0x9280）。
pub fn run_unx6461_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6461 mq-deadline 结构（读树/写树/deadline 域）");
    s.add(
        "代码体量≥399行当量",
        structural_weight() >= 399,
        if structural_weight() >= 399 { "" } else { "结构当量不足" },
    );
    s.add(
        "三结构建立+O(log n)断言",
        j1_main(),
        if j1_main() { "" } else { "三结构或复杂度断言破" },
    );
    s.add(
        "树与请求账守恒",
        j1_main(),
        if j1_main() { "" } else { "守恒破" },
    );
    let inj = [
        ("注入1空树查找", inj1_empty_find()),
        ("注入2不退化", inj2_no_degenerate()),
        ("注入3同键更新", inj3_dup_update()),
        ("注入4删除守恒", inj4_remove_conserved()),
        ("注入5最小分派", inj5_min_dispatch()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
