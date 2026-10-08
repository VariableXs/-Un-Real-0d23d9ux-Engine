//! F038 深化批次四 · 进程树隔离面（compatstar2/deep3 · G-A-38）。
//!
//! 批次一深化覆盖九宫格矩阵/临时放行/继承，批次二覆盖进程代际/生效
//! 时点，批次三覆盖档位执行流水线/配额令牌桶；本批补齐【功能定义】
//! 档位隔离「进程树」全语义对齐的结构/继承/终结面：进程树模型（定长
//! 16 节点 × 父指针，深度上限 4，越深拒绝——主册【设计细节】树深纪律）、
//! 逐节点档位继承账（子节点档位特权 ≤ 父，越权声明检出——主册【功能
//! 定义】「档位继承」的执行面）、杀树传播定序（后序遍历：先子后父，
//! 定长输出序——父先亡则子进程收尸重派逃逸，MS 进程树终止惯例语义
//! 对拍）、孤儿节点收敛（父亡子节点 → 挂根 + 记账，零静默丢进程）。
//!
//! 判据对账：主册 G-A-38【功能定义】「每应用三档隔离」+【状态与异常】
//! 「档位变更不崩溃正在运行的系统其他部分」（树结构操作全部显性化，
//! 不静默改结构）；档位数值化 = 特权量级（域内量化口径）。
//!
//! 零堆纪律：定长 16 节点表 + 定长输出序，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）

/// 树容量 16 节点（域内模型口径）。
pub const MAX_NODES: usize = 16;
/// 深度上限 4（根为第 1 层——主册【设计细节】树深纪律）。
pub const MAX_DEPTH: usize = 4;
/// 空父指针。
pub const NO_PARENT: u8 = 0xFF;
/// 根节点槽位（根恒活——孤儿挂根的锚点）。
pub const ROOT: usize = 0;
/// 档位：严格 = 0（特权最少：沙盒 + 禁网络 + 文档只读）。
pub const TIER_STRICT: u8 = 0;
/// 档位：标准 = 1（沙盒 + 网络允许）。
pub const TIER_STANDARD: u8 = 1;
/// 档位：宽松 = 2（特权最多：全直通 + 审计）。
pub const TIER_LOOSE: u8 = 2;

// ---------------------------------------------------------------------------
// 进程树模型

/// 一个进程节点：父指针 + 档位 + 存活位。
#[derive(Clone, Copy)]
pub struct ProcNode {
    pub parent: u8,
    pub tier: u8,
    pub alive: bool,
}

/// 进程树模型：定长 16 节点 × 父指针 + 档位继承账 + 杀树定序。
pub struct ProcTree {
    nodes: [Option<ProcNode>; MAX_NODES],
    pub count: usize,
    /// 越权声明检出计数（子档位特权 > 父——零静默）。
    pub tier_violations: u32,
    /// 孤儿收敛计数（父亡 → 挂根）。
    pub orphans: u32,
}

impl ProcTree {
    pub const fn new() -> Self {
        ProcTree { nodes: [None; MAX_NODES], count: 0, tier_violations: 0, orphans: 0 }
    }

    /// 节点深度（根 = 1；父链逐级上溯计数）。
    pub fn depth_of(&self, idx: usize) -> usize {
        let mut d = 1usize;
        let mut cur = idx;
        while let Some(n) = self.nodes[cur] {
            if n.parent == NO_PARENT {
                break;
            }
            cur = n.parent as usize;
            d += 1;
        }
        d
    }

    /// 派生根：全树仅一个根（重复派生根显性报错）。
    pub fn spawn_root(&mut self, tier: u8) -> Result<usize, &'static str> {
        if self.count > 0 {
            return Err("root-exists");
        }
        self.nodes[ROOT] = Some(ProcNode { parent: NO_PARENT, tier, alive: true });
        self.count += 1;
        Ok(ROOT)
    }

    /// 派生子节点：父存活 + 深度 ≤ 4 + 子档位特权 ≤ 父
    /// （越权声明 → Err + tier_violations 记账，零静默）。
    pub fn spawn(&mut self, parent: usize, tier: u8) -> Result<usize, &'static str> {
        if self.count >= MAX_NODES {
            return Err("tree-full");
        }
        if parent >= MAX_NODES {
            return Err("bad-parent");
        }
        let p = match self.nodes[parent] {
            Some(n) if n.alive => n,
            _ => return Err("parent-dead"),
        };
        if tier > p.tier {
            self.tier_violations += 1;
            return Err("tier-escalation");
        }
        if self.depth_of(parent) + 1 > MAX_DEPTH {
            return Err("depth-limit");
        }
        for i in 0..MAX_NODES {
            if self.nodes[i].is_none() {
                self.nodes[i] = Some(ProcNode { parent: parent as u8, tier, alive: true });
                self.count += 1;
                return Ok(i);
            }
        }
        Err("tree-full")
    }

    /// 后序收集（递归：先子后父——子进程先于父终结的定序前提）。
    fn collect_post(&self, idx: usize, out: &mut [u8; MAX_NODES], n: &mut usize) {
        for c in 0..MAX_NODES {
            if let Some(node) = self.nodes[c] {
                if node.alive && c != idx && node.parent == idx as u8 {
                    self.collect_post(c, out, n);
                }
            }
        }
        out[*n] = idx as u8;
        *n += 1;
    }

    /// 杀树传播：后序遍历（先子后父）定长输出终结序，并逐节点落死位
    /// （返回终结节点数；输出序确定性——审计可复现）。
    pub fn kill_tree(&mut self, root: usize, out: &mut [u8; MAX_NODES]) -> Result<usize, &'static str> {
        if root >= MAX_NODES || self.nodes[root].is_none() {
            return Err("bad-root");
        }
        let mut n = 0usize;
        self.collect_post(root, out, &mut n);
        for k in 0..n {
            let idx = out[k] as usize;
            if let Some(node) = self.nodes[idx].as_mut() {
                node.alive = false;
            }
        }
        Ok(n)
    }

    /// 单节点终结（kill_tree 的最小情形——孤儿收敛路径用）。
    pub fn kill_one(&mut self, idx: usize) -> bool {
        if idx >= MAX_NODES {
            return false;
        }
        match self.nodes[idx].as_mut() {
            Some(n) if n.alive => {
                n.alive = false;
                true
            }
            _ => false,
        }
    }

    /// 孤儿收敛：父亡子节点挂根 + 记账（返回收敛数；根恒活，
    /// 不静默丢进程——主册【状态与异常】显性化纪律）。
    pub fn converge_orphans(&mut self) -> u32 {
        let mut n = 0u32;
        for i in 0..MAX_NODES {
            let parent = match self.nodes[i] {
                Some(node) if node.alive && node.parent != NO_PARENT => node.parent,
                _ => continue,
            };
            let pdead = match self.nodes[parent as usize] {
                Some(p) => !p.alive,
                None => true,
            };
            if pdead {
                if let Some(nd) = self.nodes[i].as_mut() {
                    nd.parent = ROOT as u8;
                }
                self.orphans += 1;
                n += 1;
            }
        }
        n
    }

    /// 节点读数（检查/测试用）。
    pub fn node(&self, idx: usize) -> Option<ProcNode> {
        if idx < MAX_NODES {
            self.nodes[idx]
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化批次四）

/// 域自检（深化批次四）。
pub fn run_f038f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F038-proctree-d4");
    // 1) 派生根 + 两子节点：父指针/计数自洽。
    let mut t = ProcTree::new();
    let root = t.spawn_root(TIER_STANDARD).unwrap();
    let c1 = t.spawn(root, TIER_STANDARD).unwrap();
    let c2 = t.spawn(root, TIER_STRICT).unwrap();
    cs.add(
        "spawn_basic",
        root == 0 && c1 == 1 && c2 == 2 && t.count == 3
            && t.node(1).unwrap().parent == 0 && t.node(2).unwrap().parent == 0,
        "",
    );
    // 2) 档位继承：子特权 ≤ 父放行（标准 ≤ 标准、严格 ≤ 标准）。
    cs.add(
        "tier_inherit_ok",
        t.node(1).unwrap().tier == TIER_STANDARD && t.node(2).unwrap().tier == TIER_STRICT,
        "",
    );
    // 3) 越权声明检出：宽松子 > 标准父 → Err + 记账（零静默）。
    let esc = t.spawn(root, TIER_LOOSE);
    cs.add(
        "tier_escalation_detected",
        matches!(esc, Err("tier-escalation")) && t.tier_violations == 1 && t.count == 3,
        "",
    );
    // 4) 深度上限 4：根(1)→(2)→(3)→(4) 可达，第 5 层拒绝。
    let g1 = t.spawn(c1, TIER_STRICT).unwrap(); // 深度 3
    let g2 = t.spawn(g1, TIER_STRICT).unwrap(); // 深度 4
    let deep = t.spawn(g2, TIER_STRICT); // 深度 5 → 拒
    cs.add(
        "depth_limit",
        t.depth_of(root) == 1 && t.depth_of(g1) == 3 && t.depth_of(g2) == 4
            && matches!(deep, Err("depth-limit")),
        "",
    );
    // 5) 杀树传播定序：后序遍历先子后父（0 根 + 1/2 子 + 1 的孙 3
    // → 终结序 [3,1,2,0]）。
    let mut t2 = ProcTree::new();
    let r0 = t2.spawn_root(TIER_LOOSE).unwrap();
    let s1 = t2.spawn(r0, TIER_LOOSE).unwrap();
    let s2 = t2.spawn(r0, TIER_LOOSE).unwrap();
    let s3 = t2.spawn(s1, TIER_LOOSE).unwrap();
    let mut seq = [0u8; MAX_NODES];
    let killed = t2.kill_tree(r0, &mut seq).unwrap();
    cs.add(
        "kill_tree_postorder",
        killed == 4 && seq[0] == s3 as u8 && seq[1] == s1 as u8 && seq[2] == s2 as u8
            && seq[3] == r0 as u8,
        "",
    );
    // 6) 杀树落死位：被终结节点全部 alive == false。
    cs.add(
        "kill_tree_marks_dead",
        !t2.node(0).unwrap().alive && !t2.node(1).unwrap().alive
            && !t2.node(2).unwrap().alive && !t2.node(3).unwrap().alive,
        "",
    );
    // 7) 死父派生显性报错（杀后不能再挂子）。
    cs.add("spawn_on_dead_parent", matches!(t2.spawn(1, TIER_STRICT), Err("parent-dead")), "");
    // 8) 孤儿收敛：父亡子节点挂根 + 记账。
    let mut t3 = ProcTree::new();
    let r = t3.spawn_root(TIER_LOOSE).unwrap();
    let p = t3.spawn(r, TIER_STANDARD).unwrap();
    let child = t3.spawn(p, TIER_STRICT).unwrap();
    assert!(t3.kill_one(p));
    let conv = t3.converge_orphans();
    cs.add(
        "orphan_rehang_root",
        conv == 1 && t3.orphans == 1 && t3.node(child).unwrap().parent == ROOT as u8
            && t3.node(child).unwrap().alive,
        "",
    );
    // 9) 表满显性报错（16 节点满后第 17 个派生 → Err("tree-full")）。
    let mut tf = ProcTree::new();
    let rt = tf.spawn_root(TIER_LOOSE).unwrap();
    let mut ok_all = true;
    for _ in 1..MAX_NODES {
        ok_all &= tf.spawn(rt, TIER_LOOSE).is_ok();
    }
    cs.add("tree_full_explicit", ok_all && matches!(tf.spawn(rt, TIER_LOOSE), Err("tree-full")), "");
    // 10) 重复派生根显性报错。
    let mut tr = ProcTree::new();
    let _ = tr.spawn_root(TIER_STANDARD);
    cs.add("root_unique", matches!(tr.spawn_root(TIER_STRICT), Err("root-exists")), "");
    // 11) 重复收敛不再记账（孤儿挂根后父为活根——幂等）。
    let conv2 = t3.converge_orphans();
    cs.add("orphan_converge_idempotent", conv2 == 0 && t3.orphans == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postorder_sequence_exact() {
        // 树形：0→1→3、0→2；后序必须 [3,1,2,0]（子先于父，审计可复现）。
        let mut t = ProcTree::new();
        let r = t.spawn_root(TIER_LOOSE).unwrap();
        let a = t.spawn(r, TIER_LOOSE).unwrap();
        let b = t.spawn(r, TIER_LOOSE).unwrap();
        let c = t.spawn(a, TIER_LOOSE).unwrap();
        let mut seq = [0u8; MAX_NODES];
        let n = t.kill_tree(r, &mut seq).unwrap();
        assert_eq!(n, 4);
        assert_eq!(&seq[..4], &[c as u8, a as u8, b as u8, r as u8]);
    }

    #[test]
    fn depth_chain_rejects_fifth_level() {
        let mut t = ProcTree::new();
        let r = t.spawn_root(TIER_STRICT).unwrap();
        let mut cur = r;
        for _ in 1..MAX_DEPTH {
            cur = t.spawn(cur, TIER_STRICT).expect("4 层内必成");
        }
        assert_eq!(t.depth_of(cur), MAX_DEPTH);
        assert!(matches!(t.spawn(cur, TIER_STRICT), Err("depth-limit")), "第 5 层拒绝");
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f038f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
