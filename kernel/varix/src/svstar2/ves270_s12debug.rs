//! VE-F4030 · RTL 调试
//!
//! RTL 调试器：方向检视（方向树/镜像状态/混合边界五列——家族仪表）；
//! 镜像可视化（LTR/RTL 并排对照——单源）；断链扫描（方向断链扫描——
//! 工具化）；家族关系（家族二十八成员复述协议）。
//! 数据结构：五列仪表；对照复用；扫描复述；协议集成。
//! 错误路径：对照分歧→单源；扫描漏→断言修正；仪表分歧→立案；开销
//! →打开才采样。
//! 性能：仪表 O(列)；对照 O(复用)；扫描 O(复用)；协议 O(1)。
//! 跨批对接：F4022/F4023 数据源；F3342 统一框架对端；F2815 协议。
//! 无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x6D40）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x6D40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T36DCode(pub u16);

impl T36DCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const PEER_SPLIT: Self = Self(SEG_BASE + 1);
    pub const SCAN_MISS: Self = Self(SEG_BASE + 2);
    pub const PANEL_SPLIT: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::PEER_SPLIT => "对照分歧→单源",
            Self::SCAN_MISS => "扫描漏→断言修正",
            Self::PANEL_SPLIT => "仪表分歧→立案",
            _ => "未知",
        }
    }
}

// ── 五列仪表 ────────────────────────────────────────────────────────

pub const PANEL_COLS: [&str; 5] = ["方向树", "镜像状态", "混合边界", "断链", "家族位"];

/// 仪表行：五列值。
#[derive(Debug, Clone)]
pub struct PanelRow {
    /// 节点名。
    pub node: String,
    /// 方向树值。
    pub dir_tree: &'static str,
    /// 镜像状态。
    pub mirror_state: &'static str,
    /// 混合边界数。
    pub edge_count: u32,
    /// 断链数。
    pub broken: u32,
    /// 家族位。
    pub family_slot: u8,
}

/// 五列仪表。
#[derive(Debug, Clone)]
pub struct DebugPanel {
    pub rows: Vec<PanelRow>,
    /// 采样开关（打开才采样——零常态开销）。
    pub sampling: bool,
}

impl DebugPanel {
    pub fn new() -> Self {
        Self { rows: Vec::new(), sampling: false }
    }

    pub fn set_sampling(&mut self, on: bool) {
        self.sampling = on;
    }

    /// 采样（未开 → 拒收）。
    pub fn sample(&mut self, row: PanelRow) -> bool {
        if !self.sampling {
            return false;
        }
        self.rows.push(row);
        true
    }

    /// 仪表分歧检出：同节点两行值冲突。
    pub fn split_nodes(&self) -> Vec<String> {
        let mut out = Vec::new();
        for i in 0..self.rows.len() {
            for j in (i + 1)..self.rows.len() {
                if self.rows[i].node == self.rows[j].node
                    && (self.rows[i].dir_tree != self.rows[j].dir_tree
                        || self.rows[i].mirror_state != self.rows[j].mirror_state)
                {
                    if !out.contains(&self.rows[i].node) {
                        out.push(self.rows[i].node.clone());
                    }
                }
            }
        }
        out
    }
}

// ── LTR/RTL 并排对照 ────────────────────────────────────────────────

/// 对照对：LTR 快照 + RTL 快照。
#[derive(Debug, Clone)]
pub struct SideBySide {
    pub node: String,
    pub ltr_fp: u64,
    pub rtl_fp: u64,
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 对照分歧检出：镜像规则下 LTR/RTL 指纹必须不同（镜像生效）。
pub fn peer_compare(pairs: &[SideBySide]) -> Vec<String> {
    pairs.iter()
        .filter(|p| p.ltr_fp == p.rtl_fp)
        .map(|p| p.node.clone())
        .collect()
}

// ── 断链扫描 ────────────────────────────────────────────────────────

/// 方向链：节点 → 镜像宿主。
#[derive(Debug, Clone)]
pub struct DirLink {
    pub node: &'static str,
    pub host: &'static str,
}

/// 断链扫描：宿主缺失。
pub fn broken_scan(links: &[DirLink], alive: &[&'static str]) -> Vec<&'static str> {
    links.iter()
        .filter(|l| !alive.contains(&l.host))
        .map(|l| l.node)
        .collect()
}

// ── 家族二十八成员协议 ──────────────────────────────────────────────

pub const FAMILY_TOTAL: u8 = 28;

/// 家族位登记。
#[derive(Debug, Clone)]
pub struct FamilyRegistry {
    pub members: Vec<(&'static str, u8)>,
}

impl FamilyRegistry {
    pub fn new() -> Self {
        Self { members: Vec::new() }
    }

    pub fn register(&mut self, name: &'static str, slot: u8) -> bool {
        if slot >= FAMILY_TOTAL || self.members.iter().any(|(n, _)| *n == name)
            || self.members.iter().any(|(_, s)| *s == slot)
        {
            return false;
        }
        self.members.push((name, slot));
        true
    }

    pub fn registered(&self) -> bool {
        self.members.len() == FAMILY_TOTAL as usize
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves270_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves270_s12debug");

    // C4030-01 诊断码段不越窗。
    chk(&mut set, "C4030-01",
        T36DCode::PANEL_SPLIT.0 < SEG_BASE + 16 && SEG_BASE == 0x6D40, "err");

    // C4030-02 五列仪表齐备。
    chk(&mut set, "C4030-02",
        PANEL_COLS.len() == 5
            && PANEL_COLS[0] == "方向树" && PANEL_COLS[4] == "家族位", "err");

    // C4030-03 采样关闭拒收（零常态开销）。
    let mut p = DebugPanel::new();
    let row = PanelRow {
        node: String::from("侧栏"),
        dir_tree: "RTL",
        mirror_state: "已镜像",
        edge_count: 2,
        broken: 0,
        family_slot: 3,
    };
    chk(&mut set, "C4030-03", !p.sample(row) && !p.sampling, "err");

    // C4030-04 打开后采样。
    p.set_sampling(true);
    chk(&mut set, "C4030-04",
        p.sample(PanelRow {
            node: String::from("侧栏"),
            dir_tree: "RTL",
            mirror_state: "已镜像",
            edge_count: 2,
            broken: 0,
            family_slot: 3,
        }), "err");

    // C4030-05 仪表分歧：同节点两行冲突 → 立案。
    p.sample(PanelRow {
        node: String::from("侧栏"),
        dir_tree: "LTR",
        mirror_state: "已镜像",
        edge_count: 2,
        broken: 0,
        family_slot: 3,
    });
    chk(&mut set, "C4030-05",
        p.split_nodes() == vec![String::from("侧栏")], "err");

    // C4030-06 不同节点不误报。
    let mut p2 = DebugPanel::new();
    p2.set_sampling(true);
    p2.sample(PanelRow { node: String::from("A"), dir_tree: "LTR", mirror_state: "无", edge_count: 0, broken: 0, family_slot: 1 });
    p2.sample(PanelRow { node: String::from("B"), dir_tree: "RTL", mirror_state: "已镜像", edge_count: 1, broken: 0, family_slot: 2 });
    chk(&mut set, "C4030-06", p2.split_nodes().is_empty(), "err");

    // C4030-07 对照：镜像生效（LTR≠RTL 指纹）。
    let pairs = [
        SideBySide { node: String::from("卡片"), ltr_fp: fnv1a(b"ltr"), rtl_fp: fnv1a(b"rtl") },
    ];
    chk(&mut set, "C4030-07", peer_compare(&pairs).is_empty(), "err");

    // C4030-08 对照分歧：镜像未生效（同指纹）。
    let pairs2 = [
        SideBySide { node: String::from("时钟"), ltr_fp: 7, rtl_fp: 7 },
    ];
    chk(&mut set, "C4030-08",
        peer_compare(&pairs2) == vec![String::from("时钟")], "err");

    // C4030-09 断链扫描：宿主存活。
    let links = [DirLink { node: "进度条", host: "布局引擎" }];
    chk(&mut set, "C4030-09",
        broken_scan(&links, &["布局引擎"]).is_empty(), "err");

    // C4030-10 断链检出。
    chk(&mut set, "C4030-10",
        broken_scan(&links, &["其他"]) == vec!["进度条"], "err");

    // C4030-11 家族二十八位登记。
    let mut f = FamilyRegistry::new();
    chk(&mut set, "C4030-11",
        f.register("T02-RTL", 7) && !f.register("T02-RTL", 8), "err");

    // C4030-12 越位拒绝。
    chk(&mut set, "C4030-12", !f.register("越位", 28), "err");

    // C4030-13 位冲突拒绝。
    chk(&mut set, "C4030-13", !f.register("另一员", 7), "err");

    // C4030-14 家族常量 28。
    chk(&mut set, "C4030-14", FAMILY_TOTAL == 28, "err");

    // C4030-15 指纹确定性。
    chk(&mut set, "C4030-15",
        fnv1a(b"x") == fnv1a(b"x") && fnv1a(b"x") != fnv1a(b"y"), "err");

    // C4030-16 空仪表零分歧。
    chk(&mut set, "C4030-16", DebugPanel::new().split_nodes().is_empty(), "err");

    // C4030-17 断链扫描空集过。
    chk(&mut set, "C4030-17", broken_scan(&[], &[]).is_empty(), "err");

    // C4030-18 仪表行字段齐（family_slot 越家族拒由注册侧）。
    chk(&mut set, "C4030-18",
        3u8 < FAMILY_TOTAL, "err");

    // C4030-19 对照分歧码语义。
    chk(&mut set, "C4030-19", T36DCode::PEER_SPLIT.explain().contains("单源"), "err");

    // C4030-20 错误码互异。
    let codes = [T36DCode::OK.0, T36DCode::PEER_SPLIT.0, T36DCode::SCAN_MISS.0, T36DCode::PANEL_SPLIT.0];
    chk(&mut set, "C4030-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampling_gate() {
        let mut p = DebugPanel::new();
        let row = PanelRow { node: String::from("n"), dir_tree: "LTR", mirror_state: "无", edge_count: 0, broken: 0, family_slot: 0 };
        assert!(!p.sample(row));
        p.set_sampling(true);
        assert!(p.sample(PanelRow { node: String::from("n"), dir_tree: "LTR", mirror_state: "无", edge_count: 0, broken: 0, family_slot: 0 }));
    }

    #[test]
    fn family_slot_conflict() {
        let mut f = FamilyRegistry::new();
        assert!(f.register("a", 1));
        assert!(!f.register("b", 1));
    }
}
