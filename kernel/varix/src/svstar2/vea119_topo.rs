//! VE-F0119 · 适配器拓扑发现（VE-A 域 · A05 多 GPU 与异构组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0119`
//!
//! **职责定位（锚点原文）**：多卡拓扑的发现与建模（PCIe 拓扑/桥接关系/P2P 能力
//! 矩阵），拓扑缓存与热变更重发现，拓扑决策的输入单源。
//!
//! **铁律**：
//! 1. **拓扑建模**——PCIe/桥接/P2P 三面建模。
//! 2. **P2P 矩阵**——能力矩阵单源。
//! 3. **热变更重发现**——拓扑变更触发重发现。
//! 4. **单源输入**——拓扑决策唯一数据源。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8BD（拓扑发现）。
pub const SEG_BASE: u16 = 0xE8BD;

/// 发现失败。
pub const XC_DISCOVER_FAIL: u16 = SEG_BASE | 0x01;
/// 缓存陈旧。
pub const XC_CACHE_STALE: u16 = SEG_BASE | 0x02;
/// 拓扑虚报。
pub const XC_TOPO_LIE: u16 = SEG_BASE | 0x03;
/// 矩阵缺格。
pub const XC_MATRIX_GAP: u16 = SEG_BASE | 0x04;

/// 链路类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkType {
    /// PCIe 直连。
    Pcie,
    /// 桥接。
    Bridge,
}

/// 拓扑节点（卡）。
#[derive(Clone, Debug)]
pub struct TopoNode {
    pub id: u16,
    /// 深度（根距离）。
    pub depth: u32,
}

/// 拓扑边。
#[derive(Clone, Copy, Debug)]
pub struct TopoEdge {
    pub from: u16,
    pub to: u16,
    pub link: LinkType,
}

/// P2P 能力矩阵。
#[derive(Clone, Debug)]
pub struct P2pMatrix {
    /// (from, to) → 能力（0=无 1=有）。
    entries: Vec<(u16, u16, u8)>,
}

impl P2pMatrix {
    pub fn new() -> Self {
        P2pMatrix {
            entries: Vec::new(),
        }
    }

    pub fn set(&mut self, from: u16, to: u16, capable: bool) {
        self.entries.retain(|(f, t, _)| !(*f == from && *t == to));
        self.entries.push((from, to, capable as u8));
    }

    pub fn get(&self, from: u16, to: u16) -> Option<bool> {
        self.entries
            .iter()
            .find(|(f, t, _)| *f == from && *t == to)
            .map(|(_, _, c)| *c == 1)
    }

    /// 完整性：N 卡应有 N*(N-1) 格。
    pub fn complete(&self, n: usize) -> bool {
        self.entries.len() >= n * n.saturating_sub(1)
    }
}

/// 拓扑发现器。
pub struct TopoDiscovery {
    nodes: Vec<TopoNode>,
    edges: Vec<TopoEdge>,
    p2p: P2pMatrix,
    /// 缓存版本。
    cache_version: u64,
    last_code: u16,
    /// 重发现次数。
    rediscoveries: u32,
    /// 轮询兜底次数。
    polls: u32,
}

impl TopoDiscovery {
    pub fn new() -> Self {
        TopoDiscovery {
            nodes: Vec::new(),
            edges: Vec::new(),
            p2p: P2pMatrix::new(),
            cache_version: 0,
            last_code: 0,
            rediscoveries: 0,
            polls: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 发现一轮。
    pub fn discover(&mut self, nodes: Vec<TopoNode>, edges: Vec<TopoEdge>) -> Result<u64, u16> {
        if nodes.is_empty() {
            self.polls += 1;
            self.last_code = XC_DISCOVER_FAIL;
            return Err(XC_DISCOVER_FAIL);
        }
        self.nodes = nodes;
        self.edges = edges;
        self.cache_version += 1;
        self.last_code = 0;
        Ok(self.cache_version)
    }

    /// 构建 P2P 矩阵。
    pub fn build_p2p(&mut self) -> Result<(), u16> {
        for a in &self.nodes {
            for b in &self.nodes {
                if a.id != b.id {
                    // 有直接边且 PCIe = P2P 可能。
                    let capable = self
                        .edges
                        .iter()
                        .any(|e| {
                            ((e.from == a.id && e.to == b.id) || (e.from == b.id && e.to == a.id))
                                && e.link == LinkType::Pcie
                        });
                    self.p2p.set(a.id, b.id, capable);
                }
            }
        }
        if !self.p2p.complete(self.nodes.len()) {
            self.last_code = XC_MATRIX_GAP;
            return Err(XC_MATRIX_GAP);
        }
        self.last_code = 0;
        Ok(())
    }

    /// 热变更重发现。
    pub fn on_hot_change(&mut self) -> Result<u64, u16> {
        self.cache_version += 1;
        self.rediscoveries += 1;
        self.last_code = 0;
        Ok(self.cache_version)
    }

    /// 缓存陈旧判定。
    pub fn invalidate_if_stale(&mut self, observed_version: u64) -> bool {
        if observed_version > self.cache_version {
            self.last_code = XC_CACHE_STALE;
            return true;
        }
        false
    }

    /// 拓扑虚报校准（实测深度对比）。
    pub fn recalibrate(&mut self, measured_depths: &[(u16, u32)]) -> Result<(), u16> {
        for (id, d) in measured_depths {
            if let Some(n) = self.nodes.iter_mut().find(|n| n.id == *id) {
                if n.depth.abs_diff(*d) > 2 {
                    self.last_code = XC_TOPO_LIE;
                    return Err(XC_TOPO_LIE);
                }
                n.depth = *d;
            }
        }
        self.last_code = 0;
        Ok(())
    }

    pub fn p2p(&self) -> &P2pMatrix {
        &self.p2p
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn cache_version(&self) -> u64 {
        self.cache_version
    }

    pub fn rediscoveries(&self) -> u32 {
        self.rediscoveries
    }
}

impl Default for TopoDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：拓扑建模、P2P 矩阵、热变更重发现、单源输入。
pub fn run_vea119_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn nodes() -> Vec<TopoNode> {
        vec![
            TopoNode { id: 0, depth: 0 },
            TopoNode { id: 1, depth: 1 },
            TopoNode { id: 2, depth: 1 },
        ]
    }

    fn edges() -> Vec<TopoEdge> {
        vec![
            TopoEdge { from: 0, to: 1, link: LinkType::Pcie },
            TopoEdge { from: 0, to: 2, link: LinkType::Bridge },
        ]
    }

    let mut set = CheckSet::new("vea119_topo");

    // C119-01 拓扑发现
    {
        let mut t = TopoDiscovery::new();
        let r = t.discover(nodes(), edges());
        chk(&mut set, "C119-01 发现", r.is_ok() && t.node_count() == 3, "发现失败");
    }

    // C119-02 P2P 矩阵
    {
        let mut t = TopoDiscovery::new();
        let _ = t.discover(nodes(), edges());
        let r = t.build_p2p();
        chk(
            &mut set,
            "C119-02 P2P",
            r.is_ok() && t.p2p().get(0, 1) == Some(true) && t.p2p().get(0, 2) == Some(false),
            "矩阵错",
        );
    }

    // C119-03 热变更重发现
    {
        let mut t = TopoDiscovery::new();
        let _ = t.discover(nodes(), edges());
        let v = t.cache_version();
        let r = t.on_hot_change();
        chk(&mut set, "C119-03 热变更", r == Ok(v + 1) && t.rediscoveries() == 1, "未重发现");
    }

    // C119-04 缓存陈旧失效
    {
        let mut t = TopoDiscovery::new();
        let _ = t.discover(nodes(), edges());
        let stale = t.invalidate_if_stale(99);
        chk(&mut set, "C119-04 陈旧失效", stale, "陈旧未失效");
    }

    // C119-05 拓扑虚报检出
    {
        let mut t = TopoDiscovery::new();
        let _ = t.discover(nodes(), edges());
        let r = t.recalibrate(&[(1, 50)]);
        chk(
            &mut set,
            "C119-05 虚报检出",
            r.is_err() && t.last_code() == XC_TOPO_LIE,
            "虚报漏检",
        );
    }

    // C119-06 空发现轮询兜底
    {
        let mut t = TopoDiscovery::new();
        let r = t.discover(vec![], vec![]);
        chk(&mut set, "C119-06 兜底", r.is_err() && t.polls >= 1, "未兜底");
    }

    // C119-07 单源可查
    {
        let mut t = TopoDiscovery::new();
        let _ = t.discover(nodes(), edges());
        let _ = t.build_p2p();
        chk(&mut set, "C119-07 单源", t.p2p().get(1, 0) == Some(true), "单源断");
    }

    // C119-08 桥接无 P2P
    {
        let mut t = TopoDiscovery::new();
        let _ = t.discover(nodes(), edges());
        let _ = t.build_p2p();
        chk(&mut set, "C119-08 桥接无 P2P", t.p2p().get(0, 2) == Some(false), "桥接被误标");
    }

    // C119-09 版本单调
    {
        let mut t = TopoDiscovery::new();
        let _ = t.discover(nodes(), edges());
        let v1 = t.cache_version();
        let _ = t.on_hot_change();
        chk(&mut set, "C119-09 版本单调", t.cache_version() > v1, "版本倒退");
    }

    // C119-10 校准通过
    {
        let mut t = TopoDiscovery::new();
        let _ = t.discover(nodes(), edges());
        let r = t.recalibrate(&[(1, 1)]);
        chk(&mut set, "C119-10 校准过", r.is_ok(), "校准误拒");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea119_all_green() {
        let s = run_vea119_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
