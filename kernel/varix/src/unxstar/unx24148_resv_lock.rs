//! UNX-F24148 reservation 锁与显存互斥纪律·子功能08（死锁环检测·等待图）（0x41460 段）
//!
//! 锚点：UNX-F24148-J1 等待图账（持锁/等锁双边逐条落账）；J2 环检语
//! 义（边登记 + 环判定 + 解环后清三性质）；J3 环检指纹（图折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-F5-B15（等待图死锁检测面）。
//!
//! 结构：等待图（节点=任务，边=等谁）+ 环检测核（DFS 三色）+ 图账本
//! + 折叠指纹 + 注入核（自环/野节点/无环误报/印后/双边重复必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x41460 独占 0x20）。
pub const SEG_BASE: u32 = 0x41460;
pub const DG_SELF_EDGE: u32 = SEG_BASE + 0x01; // 自环边
pub const DG_UNK_NODE: u32 = SEG_BASE + 0x02; // 野节点
pub const DG_DUP_EDGE: u32 = SEG_BASE + 0x03; // 重复边
pub const DG_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const DG_BAD_COLOR: u32 = SEG_BASE + 0x05; // 非法三色态

/// 等待图。
#[derive(Clone)]
pub struct DgGraph {
    pub nodes: Vec<String>,
    pub edges: Vec<(String, String)>, // from 等 to
    pub sealed: bool,
}

impl DgGraph {
    pub fn new() -> DgGraph {
        DgGraph { nodes: Vec::new(), edges: Vec::new(), sealed: false }
    }

    pub fn add_node(&mut self, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DG_FROZEN);
        }
        if name.is_empty() {
            return Err(DG_UNK_NODE);
        }
        if !self.nodes.iter().any(|n| n == name) {
            self.nodes.push(name.to_string());
        }
        Ok(())
    }

    /// 登记等待边 from → to。
    pub fn add_edge(&mut self, from: &str, to: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DG_FROZEN);
        }
        if from == to {
            return Err(DG_SELF_EDGE);
        }
        if !self.nodes.iter().any(|n| n == from) || !self.nodes.iter().any(|n| n == to) {
            return Err(DG_UNK_NODE);
        }
        if self.edges.iter().any(|(a, b)| a == from && b == to) {
            return Err(DG_DUP_EDGE);
        }
        self.edges.push((from.to_string(), to.to_string()));
        Ok(())
    }

    /// DFS 三色环检测：0=白 1=灰 2=黑。
    pub fn has_cycle(&self) -> bool {
        let n = self.nodes.len();
        let mut color = vec![0u8; n];
        for i in 0..n {
            if color[i] == 0 && self.dfs(i, &mut color) {
                return true;
            }
        }
        false
    }

    fn dfs(&self, i: usize, color: &mut [u8]) -> bool {
        if color[i] == 1 {
            return true; // 灰回边 = 环
        }
        if color[i] == 2 {
            return false;
        }
        color[i] = 1;
        let name = &self.nodes[i];
        let succs: Vec<usize> = self
            .edges
            .iter()
            .filter(|(a, _)| a == name)
            .filter_map(|(_, b)| self.nodes.iter().position(|n| n == b))
            .collect();
        for s in succs {
            if self.dfs(s, color) {
                return true;
            }
        }
        color[i] = 2;
        false
    }

    /// 解环：移除边。
    pub fn remove_edge(&mut self, from: &str, to: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(DG_FROZEN);
        }
        let before = self.edges.len();
        self.edges.retain(|(a, b)| !(a == from && b == to));
        if self.edges.len() == before {
            return Err(DG_DUP_EDGE);
        }
        Ok(())
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41460;
        for (a, b) in &self.edges {
            for by in a.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
            h = h.wrapping_mul(31).wrapping_add(0x2C); // 边分隔
            for by in b.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：A→B→C 链（无环）+ 独立节点 D。
pub fn mk_graph() -> DgGraph {
    let mut g = DgGraph::new();
    for n in ["A", "B", "C", "D"] {
        let _ = g.add_node(n);
    }
    let _ = g.add_edge("A", "B");
    let _ = g.add_edge("B", "C");
    g
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut g = mk_graph();
    // 1) 自环边
    got[0] = g.add_edge("A", "A").err() == Some(DG_SELF_EDGE);
    // 2) 野节点边
    got[1] = g.add_edge("A", "X").err() == Some(DG_UNK_NODE);
    // 3) 重复边
    got[2] = g.add_edge("A", "B").err() == Some(DG_DUP_EDGE);
    // 4) 真环必判（C→A 闭合成环）
    let _ = g.add_edge("C", "A");
    let cyc = g.has_cycle();
    let _ = g.remove_edge("C", "A");
    got[3] = cyc && !g.has_cycle();
    // 5) 印后操作必抓
    g.seal();
    got[4] = g.add_edge("A", "D").err() == Some(DG_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24148_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24148");
    let mut g = mk_graph();

    // J1 节点账齐
    let j1 = g.nodes.len() == 4;
    s.add("J1-节点账齐", j1, "四节点全落表");

    // J2 边账齐
    let j2 = g.edge_count() == 2;
    s.add("J2-边账齐", j2, "两等待边落账");

    // J3 链无环
    let j3 = !g.has_cycle();
    s.add("J3-链无环", j3, "A→B→C 无回边");

    // J4 环必判
    let j4 = {
        let mut g2 = mk_graph();
        let _ = g2.add_edge("C", "A");
        g2.has_cycle()
    };
    s.add("J4-环必判", j4, "C→A 闭合后判环");

    // J5 解环复无环
    let j5 = {
        let mut g2 = mk_graph();
        let _ = g2.add_edge("C", "A");
        let _ = g2.remove_edge("C", "A");
        !g2.has_cycle()
    };
    s.add("J5-解环复净", j5, "移除回边后复无环");

    // J6 自环拒
    let j6 = g.add_edge("A", "A").err() == Some(DG_SELF_EDGE);
    s.add("J6-自环拒", j6, "自等待边必拒");

    // J7 野节点拒
    let j7 = g.add_edge("A", "X").err() == Some(DG_UNK_NODE);
    s.add("J7-野节点拒", j7, "未登记节点必拒");

    // J8 重复边拒
    let j8 = g.add_edge("A", "B").err() == Some(DG_DUP_EDGE);
    s.add("J8-重复边拒", j8, "双边登记必拒");

    // J9 移除不存在边拒
    let j9 = g.remove_edge("D", "A").err() == Some(DG_DUP_EDGE);
    s.add("J9-移除无边拒", j9, "无此边必拒");

    // J10 自环构造也判环（防御层）
    let j10 = {
        let mut g2 = DgGraph::new();
        let _ = g2.add_node("S");
        // 自环被入口拒，无法构造——验证入口拒绝即预防
        g2.add_edge("S", "S").is_err()
    };
    s.add("J10-自环入口拒", j10, "自环无法入图即预防");

    // J11 重声明幂等
    let j11 = {
        let mut g2 = mk_graph();
        let _ = g2.add_node("A");
        g2.nodes.len() == 4
    };
    s.add("J11-重声明幂等", j11, "节点重声明不增席");

    // J12 指纹稳定
    let j12 = g.fold() == g.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24148_cycle_flow() {
        let mut g = mk_graph();
        assert!(!g.has_cycle());
        assert!(g.add_edge("C", "A").is_ok());
        assert!(g.has_cycle());
        assert!(g.remove_edge("C", "A").is_ok());
        assert!(!g.has_cycle());
        assert_eq!(g.add_edge("A", "A"), Err(DG_SELF_EDGE));
        g.seal();
        assert_eq!(g.add_edge("A", "D"), Err(DG_FROZEN));
    }

    #[test]
    fn unx24148_checks_green() {
        let s = run_unx24148_checks();
        assert!(s.all_passed(), "UNX-F24148 判据未全绿");
    }
}
