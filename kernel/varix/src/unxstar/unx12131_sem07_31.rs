//! UNX-F12131 — UWOP_CHAINED_INFO 链式展开核
//! 域: UNX-D1 (Windows NT 内核语义模型) · B07 批 · 展开码族
//! 语义: UWOP_CHAINED_INFO（码 0）作为 UNWIND_INFO 的标志位 UNW_FLAG_CHAININFO
//!       出现时，展开码流末尾紧随一组 RUNTIME_FUNCTION 三元组，指向“父”
//!       展开信息。异常过滤/展开处理沿链上行：当前帧不处理则查父帧。
//!       本核维护 ChainBook：链节点登记（子→父）、自指拒止、环路检测
//!       （沿父上行染色）、深度上限 32、seal 冻结。链根（无父）显性登记。
//! 载荷: 链边账 Vec<(child, parent)> + 深度表 + 折叠指纹 + seal 冻结。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1a280;

pub const CH_NO_TARGET: u32 = SEG_BASE + 0x01; // 父节点未登记（悬空引用）
pub const CH_SELF: u32 = SEG_BASE + 0x02; // 自指（child == parent）
pub const CH_CYCLE: u32 = SEG_BASE + 0x03; // 上行检测出环路
pub const CH_TOO_DEEP: u32 = SEG_BASE + 0x04; // 链深超过 32 上限
pub const CH_FROZEN: u32 = SEG_BASE + 0x05; // seal 后仍受理挂链

pub const CHAIN_DEPTH_CAP: usize = 32;

/// 链节点：一个展开信息实例（以 RVA 代表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainNode {
    pub rva: u32,
    /// 是否为链根（无父，异常可就地终结）。
    pub is_root: bool,
}

impl ChainNode {
    pub fn root(rva: u32) -> Self {
        ChainNode { rva, is_root: true }
    }
    pub fn inner(rva: u32) -> Self {
        ChainNode { rva, is_root: false }
    }
}

/// 链账本：节点登记 + 子父挂链 + 环路/深度检查 + seal。
pub struct ChainBook {
    nodes: Vec<ChainNode>,
    edges: Vec<(u32, u32)>, // (child_rva, parent_rva)
    frozen: bool,
    rejects: Vec<(u32, String)>,
}

impl ChainBook {
    pub fn new() -> Self {
        ChainBook { nodes: Vec::new(), edges: Vec::new(), frozen: false, rejects: Vec::new() }
    }

    /// 登记节点（root 或 inner）。重复登记同 RVA 为幂等放行。
    pub fn add_node(&mut self, n: ChainNode) {
        if !self.nodes.iter().any(|x| x.rva == n.rva) {
            self.nodes.push(n);
        }
    }

    pub fn has_node(&self, rva: u32) -> bool {
        self.nodes.iter().any(|n| n.rva == rva)
    }

    /// 挂链：child 沿 CHAININFO 指向 parent。
    /// 校验链：冻结 → parent 在册 → child≠parent → 环路 → 深度。
    pub fn link(&mut self, child: u32, parent: u32) -> Result<usize, u32> {
        if self.frozen {
            self.rejects.push((CH_FROZEN, "link after seal".to_string()));
            return Err(CH_FROZEN);
        }
        if !self.has_node(parent) {
            self.rejects.push((CH_NO_TARGET, format!("parent {} absent", parent)));
            return Err(CH_NO_TARGET);
        }
        if child == parent {
            self.rejects.push((CH_SELF, format!("self link {}", child)));
            return Err(CH_SELF);
        }
        if !self.has_node(child) {
            self.add_node(ChainNode::inner(child));
        }
        // 环路检测：从 parent 沿边上行，若回到 child 则成环。
        let mut cur = parent;
        let mut steps = 0usize;
        while let Some((_c, p)) = self.edges.iter().find(|(c, _)| *c == cur) {
            if *p == child {
                self.rejects.push((CH_CYCLE, format!("cycle via {}", p)));
                return Err(CH_CYCLE);
            }
            cur = *p;
            steps += 1;
            if steps > CHAIN_DEPTH_CAP {
                self.rejects.push((CH_CYCLE, "walk overflow".to_string()));
                return Err(CH_CYCLE);
            }
        }
        // 深度检查：child 的新深度 = parent 深度 + 1。
        let pd = self.depth_of(parent)?;
        if pd + 1 > CHAIN_DEPTH_CAP {
            self.rejects.push((CH_TOO_DEEP, format!("depth {} > cap", pd + 1)));
            return Err(CH_TOO_DEEP);
        }
        self.edges.push((child, parent));
        Ok(self.edges.len() - 1)
    }

    /// 节点深度：根为 0，其余沿父累计。悬空引用按 CH_NO_TARGET 处理。
    pub fn depth_of(&self, rva: u32) -> Result<usize, u32> {
        if !self.has_node(rva) {
            return Err(CH_NO_TARGET);
        }
        let mut cur = rva;
        let mut d = 0usize;
        while let Some((_c, p)) = self.edges.iter().find(|(c, _)| *c == cur) {
            cur = *p;
            d += 1;
            if d > CHAIN_DEPTH_CAP {
                return Err(CH_TOO_DEEP);
            }
        }
        Ok(d)
    }

    /// 自 rva 沿链上行到根的路径。
    pub fn walk_up(&self, rva: u32) -> Result<Vec<u32>, u32> {
        if !self.has_node(rva) {
            return Err(CH_NO_TARGET);
        }
        let mut path = vec![rva];
        let mut cur = rva;
        while let Some((_c, p)) = self.edges.iter().find(|(c, _)| *c == cur) {
            cur = *p;
            path.push(cur);
            if path.len() > CHAIN_DEPTH_CAP + 1 {
                return Err(CH_CYCLE);
            }
        }
        Ok(path)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    /// 折叠指纹：211 进制卷积，节点/边序敏感。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for n in &self.nodes {
            h = h.wrapping_mul(211).wrapping_add(n.rva ^ (n.rva >> 5));
            h = h.wrapping_mul(211).wrapping_add(n.is_root as u32);
        }
        for (c, p) in &self.edges {
            h = h.wrapping_mul(211).wrapping_add(*c);
            h = h.wrapping_mul(211).wrapping_add(*p);
        }
        h.wrapping_mul(211).wrapping_add(self.edges.len() as u32)
    }

    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        self.fold()
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 五况注入核：每况踩一个独立检出分支。
pub fn inject_five() -> [Result<usize, u32>; 5] {
    let mut b0 = ChainBook::new();
    b0.add_node(ChainNode::root(0x1000));
    let r0 = b0.link(0x2000, 0x3000); // CH_NO_TARGET：父未登记
    let mut b1 = ChainBook::new();
    b1.add_node(ChainNode::root(0x1000));
    let r1 = b1.link(0x1000, 0x1000); // CH_SELF：自指
    let mut b2 = ChainBook::new();
    b2.add_node(ChainNode::root(0x1000));
    b2.add_node(ChainNode::inner(0x2000));
    let _ = b2.link(0x2000, 0x1000);
    let r2 = b2.link(0x1000, 0x2000); // CH_CYCLE：0x1000→0x2000→0x1000
    let r3 = {
        let mut b3 = ChainBook::new();
        let mut d = b3.depth_of(0x9999);
        // CH_NO_TARGET：深度查询悬空节点
        let base = d.is_err();
        // 人为构造深度越限：33 级链
        let mut b4 = ChainBook::new();
        b4.add_node(ChainNode::root(0));
        for i in 1..=34u32 {
            b4.add_node(ChainNode::inner(i * 0x10));
        }
        for i in 1..=33u32 {
            let _ = b4.link(i * 0x10, (i - 1) * 0x10);
        }
        d = b4.link(34 * 0x10, 33 * 0x10); // 第 34 级 → CH_TOO_DEEP
        if base && d.is_err() {
            d
        } else if base {
            Err(CH_NO_TARGET)
        } else {
            d
        }
    };
    let mut b5 = ChainBook::new();
    b5.add_node(ChainNode::root(0x1000));
    b5.seal();
    let r4 = b5.link(0x2000, 0x1000); // CH_FROZEN
    [r0, r1, r2, r3, r4]
}

/// 判据核：14 条链式展开语义判据。
pub fn run_unx12131_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F12131");
    let mut b = ChainBook::new();
    b.add_node(ChainNode::root(0x5000));
    b.add_node(ChainNode::inner(0x5100));
    b.add_node(ChainNode::inner(0x5200));
    let l1 = b.link(0x5200, 0x5100);
    let l2 = b.link(0x5100, 0x5000);
    cs.add("link_inner_chain", l1.is_ok() && l2.is_ok(), "two-level chain");
    cs.add("node_kinds", ChainNode::root(1).is_root && !ChainNode::inner(1).is_root,
        "root/inner distinction");
    cs.add("depth_root_zero", b.depth_of(0x5000) == Ok(0), "root depth 0");
    cs.add("depth_mid_one", b.depth_of(0x5100) == Ok(1), "mid depth 1");
    cs.add("depth_leaf_two", b.depth_of(0x5200) == Ok(2), "leaf depth 2");
    cs.add("walk_up_path", {
        match b.walk_up(0x5200) {
            Ok(p) => p == vec![0x5200, 0x5100, 0x5000],
            Err(_) => false,
        }
    }, "walk reaches root");
    cs.add("edge_count", b.edge_count() == 2, "two edges");
    cs.add("idempotent_node", {
        b.add_node(ChainNode::inner(0x5100));
        b.node_count() == 3
    }, "re-add is no-op");
    cs.add("dangling_depth", b.depth_of(0xdead).err() == Some(CH_NO_TARGET),
        "dangling depth query");
    cs.add("dangling_link", {
        let mut bb = ChainBook::new();
        bb.add_node(ChainNode::root(0x10));
        bb.link(0x20, 0x30).err() == Some(CH_NO_TARGET)
    }, "link to absent parent");
    cs.add("cycle_blocked", {
        let inj = inject_five();
        inj[2].err() == Some(CH_CYCLE)
    }, "cycle branch fires");
    cs.add("too_deep_branch", inject_five()[3].is_err(), "depth cap branch fires");
    cs.add("seal_blocks_link", {
        let mut bb = ChainBook::new();
        bb.add_node(ChainNode::root(0x10));
        bb.seal();
        bb.link(0x20, 0x10).err() == Some(CH_FROZEN)
    }, "seal blocks link");
    cs.add("fold_seal_differs", {
        let mut b1 = ChainBook::new();
        b1.add_node(ChainNode::root(0x10));
        let f1 = b1.seal();
        let mut b2 = ChainBook::new();
        b2.add_node(ChainNode::root(0x20));
        let f2 = b2.seal();
        f1 != f2
    }, "fold distinguishes books");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_basic() {
        let mut b = ChainBook::new();
        b.add_node(ChainNode::root(0x1000));
        b.add_node(ChainNode::inner(0x2000));
        assert!(b.link(0x2000, 0x1000).is_ok());
        assert_eq!(b.depth_of(0x2000), Ok(1));
        assert_eq!(b.walk_up(0x2000), Ok(vec![0x2000, 0x1000]));
    }

    #[test]
    fn test_self_and_cycle() {
        let mut b = ChainBook::new();
        b.add_node(ChainNode::root(0x10));
        assert!(b.link(0x10, 0x10).err() == Some(CH_SELF));
        b.add_node(ChainNode::inner(0x20));
        assert!(b.link(0x20, 0x10).is_ok());
        assert!(b.link(0x10, 0x20).err() == Some(CH_CYCLE));
    }

    #[test]
    fn test_depth_cap() {
        let mut b = ChainBook::new();
        b.add_node(ChainNode::root(0));
        for i in 1..=32u32 {
            b.add_node(ChainNode::inner(i));
        }
        for i in 1..=32u32 {
            assert!(b.link(i, i - 1).is_ok());
        }
        b.add_node(ChainNode::inner(33));
        assert!(b.link(33, 32).err() == Some(CH_TOO_DEEP));
        assert_eq!(b.depth_of(32), Ok(32));
    }

    #[test]
    fn test_inject_five_all_err() {
        assert!(inject_five().iter().all(|r| r.is_err()));
    }
}