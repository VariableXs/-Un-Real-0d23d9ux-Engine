//! UNX-F10226 失绿传播链（失绿依赖链定位根因项，错根检出）（0x10f60 段）
//!
//! 锚点：UNX-F10226-J1 传播链达成（失绿依赖链定位根因项
//! 10/10 次），错根 10/10 次检出。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：传播链账（依赖边 + 失绿集）+ 根因核（沿链上溯至首个
//! 失绿节点）+ 错根拦截核（根因必须无上游失绿）+ 折叠指纹 +
//! 注入核（错根指定/环链/幽灵节点/重复根/封账上溯）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10f60;
/// 诊断码：节点未知
pub const ERR_UNKNOWN_NODE: u32 = SEG_BASE + 0x01;
/// 诊断码：环链
pub const ERR_CYCLE: u32 = SEG_BASE + 0x02;
/// 诊断码：错根
pub const ERR_WRONG_ROOT: u32 = SEG_BASE + 0x03;
/// 诊断码：重复根
pub const ERR_DUP_ROOT: u32 = SEG_BASE + 0x04;
/// 诊断码：封账上溯
pub const ERR_FROZEN: u32 = SEG_BASE + 0x05;

/// 传播链账
pub struct PropChain {
    /// 依赖边（from 依赖 to）
    pub edges: Vec<(String, String)>,
    /// 失绿节点集
    pub failing: Vec<String>,
    /// 已定根因
    pub roots: Vec<String>,
    /// 封账
    pub frozen: bool,
}

impl PropChain {
    /// 空账
    pub fn new() -> Self {
        PropChain { edges: Vec::new(), failing: Vec::new(), roots: Vec::new(), frozen: false }
    }

    /// 声明节点（隐式于边亦可，此为显式孤立节点）
    pub fn declare(&mut self, node: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if node.is_empty() {
            return Err(ERR_UNKNOWN_NODE);
        }
        if !self.known(node) {
            self.push_node(node)?;
        }
        Ok(())
    }

    fn known(&self, node: &str) -> bool {
        self.edges.iter().any(|(a, b)| a == node || b == node)
    }

    fn push_node(&mut self, node: &str) -> Result<(), u32> {
        // 孤立节点以自环边外的空依赖记录
        self.edges.push((node.to_string(), String::new()));
        Ok(())
    }

    /// 声明依赖边
    pub fn depend(&mut self, from: &str, to: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if from.is_empty() || to.is_empty() || from == to {
            return Err(ERR_UNKNOWN_NODE);
        }
        // 环检：沿 to 的下游能否回到 from
        if self.reaches(to, from) {
            return Err(ERR_CYCLE);
        }
        // 空依赖记录清理（from 曾作孤立节点）
        self.edges.retain(|(a, b)| !(a == from && b.is_empty()));
        self.edges.push((from.to_string(), to.to_string()));
        Ok(())
    }

    /// 可达判定：a 沿依赖边能否到 b
    pub fn reaches(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        let mut visited: Vec<String> = Vec::new();
        let mut stack = vec![a.to_string()];
        while let Some(cur) = stack.pop() {
            if cur == b {
                return true;
            }
            if visited.contains(&cur) {
                continue;
            }
            visited.push(cur.clone());
            for (f, t) in &self.edges {
                if f == &cur && !t.is_empty() {
                    stack.push(t.clone());
                }
            }
        }
        false
    }

    /// 标记失绿
    pub fn mark_fail(&mut self, node: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if !self.known(node) {
            return Err(ERR_UNKNOWN_NODE);
        }
        if !self.failing.contains(&node.to_string()) {
            self.failing.push(node.to_string());
        }
        Ok(())
    }

    /// 根因核：沿链上溯，找下游失绿节点的最上游失绿者
    pub fn find_root(&mut self, leaf: &str) -> Result<String, u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        if !self.known(leaf) {
            return Err(ERR_UNKNOWN_NODE);
        }
        if !self.failing.contains(&leaf.to_string()) {
            return Err(ERR_WRONG_ROOT);
        }
        // 上溯：收集 leaf 的全部上游中失绿者，取无失绿上游者
        let mut best: Option<String> = None;
        for f in &self.failing {
            if self.reaches(leaf, f) {
                let upstream_fail = self.failing.iter().any(|g| {
                    g != f && self.reaches(f, g)
                });
                if !upstream_fail {
                    best = Some(f.clone());
                    break;
                }
            }
        }
        match best {
            Some(r) => {
                if self.roots.contains(&r) {
                    Err(ERR_DUP_ROOT)
                } else {
                    self.roots.push(r.clone());
                    Ok(r)
                }
            }
            None => Err(ERR_WRONG_ROOT),
        }
    }

    /// 错根拦截：指定根必须是下游失绿的上游根
    pub fn root_consistent(&self, root: &str, leaf: &str) -> bool {
        self.failing.contains(&root.to_string())
            && self.failing.contains(&leaf.to_string())
            && self.reaches(leaf, root)
            && !self.failing.iter().any(|g| g != root && self.reaches(root, g))
    }

    /// 诊断面：失绿清单
    pub fn failing_list(&self) -> Vec<String> {
        self.failing.clone()
    }

    /// 诊断面：已定根清单
    pub fn root_list(&self) -> Vec<String> {
        self.roots.clone()
    }

    /// 诊断面：边计数
    pub fn edge_count(&self) -> usize {
        self.edges.iter().filter(|(_, b)| !b.is_empty()).count()
    }

    /// 诊断面：某节点直接上游集
    pub fn direct_upstream(&self, node: &str) -> Vec<String> {
        let mut out = Vec::new();
        for (a, b) in &self.edges {
            if b == node && !out.contains(a) {
                out.push(a.clone());
            }
        }
        out
    }

    /// 诊断面：失绿密度（基点）
    pub fn fail_density_bps(&self) -> u32 {
        let total = self.node_count();
        if total == 0 {
            return 0;
        }
        (self.failing.len() * 10000 / total) as u32
    }

    /// 诊断面：节点总数（按边集去重）
    pub fn node_count(&self) -> usize {
        let mut nodes: Vec<String> = Vec::new();
        for (a, b) in &self.edges {
            if !a.is_empty() && !nodes.contains(a) {
                nodes.push(a.clone());
            }
            if !b.is_empty() && !nodes.contains(b) {
                nodes.push(b.clone());
            }
        }
        nodes.len()
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, (a, b)) in self.edges.iter().enumerate() {
            acc = acc.wrapping_mul(4561).wrapping_add(((a.len() + b.len()) as u32) + i as u32);
        }
        for (i, f) in self.failing.iter().enumerate() {
            acc = acc.wrapping_mul(8161).wrapping_add((f.len() as u32) << (i % 3));
        }
        for (i, r) in self.roots.iter().enumerate() {
            acc = acc.wrapping_mul(2179).wrapping_add((r.len() as u32) + i as u32);
        }
        acc
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 错根指定检出（叶未失绿）
    let mut c1 = PropChain::new();
    let _ = c1.depend("leaf", "root");
    out[0] = c1.find_root("leaf") == Err(ERR_WRONG_ROOT);
    // 2: 环链拒绝
    let mut c2 = PropChain::new();
    let _ = c2.depend("a", "b");
    out[1] = c2.depend("b", "a") == Err(ERR_CYCLE);
    // 3: 幽灵节点拒绝（标记与上溯两条路径均拒收未知节点）
    let mut c3 = PropChain::new();
    out[2] = c3.mark_fail("ghost") == Err(ERR_UNKNOWN_NODE)
        && c3.find_root("ghost") == Err(ERR_UNKNOWN_NODE);
    // 4: 重复根拒绝
    let mut c4 = PropChain::new();
    let _ = c4.depend("l", "r");
    let _ = c4.mark_fail("r");
    let _ = c4.mark_fail("l");
    let _ = c4.find_root("l");
    out[3] = c4.find_root("l") == Err(ERR_DUP_ROOT);
    // 5: 封账上溯拒绝
    let mut c5 = PropChain::new();
    let _ = c5.depend("l5", "r5");
    let _ = c5.mark_fail("r5");
    let _ = c5.mark_fail("l5");
    let _ = c5.seal();
    out[4] = c5.find_root("l5") == Err(ERR_FROZEN);
    out
}

/// 判据函数
pub fn run_unx10226_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10226");
    let mut c = PropChain::new();
    // 链：ut-app → ut-mid → ut-base；base 失绿向上传播
    let d1 = c.depend("ut-app", "ut-mid");
    let d2 = c.depend("ut-mid", "ut-base");
    s.add("链边受理", d1.is_ok() && d2.is_ok(), "两段依赖边登记");
    let m1 = c.mark_fail("ut-base");
    let m2 = c.mark_fail("ut-mid");
    let m3 = c.mark_fail("ut-app");
    s.add("三级失绿", m1.is_ok() && m2.is_ok() && m3.is_ok(), "链上三级全部失绿");
    let r = c.find_root("ut-app");
    s.add("根因定位", r == Ok("ut-base".to_string()), "上溯定位根因 ut-base");
    s.add("根因自洽", c.root_consistent("ut-base", "ut-app"), "根因无上游失绿");
    s.add("可达核", c.reaches("ut-app", "ut-base") && !c.reaches("ut-base", "ut-app"), "下游可达上游、反向不可达");
    let mut c2 = PropChain::new();
    let _ = c2.depend("leaf2", "mid2");
    // 失绿沿链传播到叶：mid2 与 leaf2 均入失绿集。
    let _ = c2.mark_fail("mid2");
    let _ = c2.mark_fail("leaf2");
    s.add("中间失绿传叶", c2.root_consistent("mid2", "leaf2"), "中游根因拦截叶判定");
    s.add("孤立节点声明", c2.declare("solo-node").is_ok() && c2.known("solo-node"), "孤立节点显式登记");
    s.add("自依赖拒绝", c2.depend("solo-node", "solo-node") == Err(ERR_UNKNOWN_NODE), "自依赖边被拒");
    s.add("根去重", c.find_root("ut-app") == Err(ERR_DUP_ROOT), "同根重复定谳被拒");
    s.add("失绿清单", c.failing_list().len() == 3, "三级失绿入册");
    s.add("根清单唯一", c.root_list() == vec!["ut-base".to_string()], "已定根仅 ut-base");
    s.add("边计数", c.edge_count() == 2, "两条实质依赖边");
    s.add("直接上游", c.direct_upstream("ut-mid") == vec!["ut-app".to_string()], "mid 的直接上游为 app");
    s.add("节点计数", c.node_count() == 3, "链上三节点");
    s.add("失绿密度满格", c.fail_density_bps() == 10000, "全链失绿密度满格");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = c.seal();
    s.add("封账拒绝", c.mark_fail("ut-base") == Err(ERR_FROZEN), "封账后标记被拒");
    s.add("封账指纹非零", fp != 0, "封账折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_found_at_head() {
        let mut c = PropChain::new();
        let _ = c.depend("a", "b");
        let _ = c.depend("b", "c");
        let _ = c.mark_fail("c");
        let _ = c.mark_fail("b");
        let _ = c.mark_fail("a");
        assert_eq!(c.find_root("a"), Ok("c".to_string()));
        assert!(c.root_consistent("c", "a"));
    }

    #[test]
    fn cycle_blocked() {
        let mut c = PropChain::new();
        let _ = c.depend("n1", "n2");
        let _ = c.depend("n2", "n3");
        assert_eq!(c.depend("n3", "n1"), Err(ERR_CYCLE));
        assert!(c.reaches("n1", "n3"));
    }

    #[test]
    fn partial_fail_density() {
        let mut c = PropChain::new();
        let _ = c.depend("u", "v");
        let _ = c.depend("v", "w");
        let _ = c.mark_fail("w");
        assert_eq!(c.fail_density_bps(), 3333);
        assert_eq!(c.node_count(), 3);
        assert!(c.root_consistent("w", "u"));
        assert_eq!(c.direct_upstream("w"), vec!["v".to_string()]);
    }
}
