//! VE-F3614 · 创作文档体系（VE-S 域 · 批次 S01 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3614`
//!
//! **判据（锚点原文六条）**：四册体系、SOP 复述、任务索引、2 跳复用、
//! 家族管道、判据。逐条落位：
//! - **四册体系**：创作指南/许可指南/发布指南/API 参考四册结构
//!   （[`DocSuite`]），缺册 → [`V0ECode::VOLUME_MISSING`]。
//! - **SOP 复述**：发布指南含上架流程 SOP（复述红线）——SOP 缺节 →
//!   [`V0ECode::SOP_GAP`]。
//! - **任务索引**：任务索引复用 F3348（[`TaskIndex`]），索引与文档
//!   对账（漂移 → [`V0ECode::DRIFT`]）。
//! - **2 跳复用**：检索迷宫 ≤ 2 跳（复用红线）——[`NavGraph::depth`] 
//!   超 2 → [`V0ECode::NAV_MAZE`]。
//! - **家族管道**：F2879 家族模式复用（家族九用）。
//! - **判据**：`run_ves14_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 手册漂移→对账（复述） | [`V0ECode::DRIFT`]：索引与文档对账 |
//! | SOP 缺→补（复述红线） | [`V0ECode::SOP_GAP`]：SOP 节点缺失立案 |
//! | 示例失效→断言（复述） | [`V0ECode::EXAMPLE_DEAD`]：示例引用不存在 |
//! | 检索迷宫→2 跳（复用红线） | [`V0ECode::NAV_MAZE`]：导航深度超限 |
//!
//! **码段**：独占 **0x3exx**。
//!
//! **性能逐项分解**：构建 O(文档)；对账 O(清单)；索引 O(任务)；
//! 断言增量。
//!
//! **跨批对接点**：F2879 家族模式复用；F3356 地图模式复用；
//! CI 钩子——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3614 诊断码段（独占 0x3exx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V0ECode(pub u16);

impl V0ECode {
    /// 四册缺一。
    pub const VOLUME_MISSING: V0ECode = V0ECode(0x3e01);
    /// SOP 缺节。
    pub const SOP_GAP: V0ECode = V0ECode(0x3e02);
    /// 索引与文档漂移。
    pub const DRIFT: V0ECode = V0ECode(0x3e03);
    /// 示例引用失效。
    pub const EXAMPLE_DEAD: V0ECode = V0ECode(0x3e04);
    /// 检索迷宫（>2 跳）。
    pub const NAV_MAZE: V0ECode = V0ECode(0x3e05);
    /// 空册。
    pub const EMPTY_VOLUME: V0ECode = V0ECode(0x3e06);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V0ECode::VOLUME_MISSING => {
                "文档四册（创作/许可/发布/API）缺一。建议：补齐缺失册目后重新对账。"
            }
            V0ECode::SOP_GAP => {
                "发布 SOP 节点缺失。SOP 是上架流程红线，缺节即不可发布。建议：补齐 SOP 节点。"
            }
            V0ECode::DRIFT => {
                "任务索引与文档内容漂移。建议：以文档为准刷新索引，或反向修订文档。"
            }
            V0ECode::EXAMPLE_DEAD => {
                "示例引用的文档节点不存在。建议：修复引用或删除失效示例。"
            }
            V0ECode::NAV_MAZE => {
                "检索深度超过 2 跳（迷宫红线）。建议：增加直连索引压缩跳数。"
            }
            V0ECode::EMPTY_VOLUME => {
                "册目内容为空。建议：至少填充导读章节。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 四册体系 ────────────────────────────────────────────────────────

/// 册目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Volume {
    /// 创作指南。
    Creation,
    /// 许可指南。
    License,
    /// 发布指南。
    Publish,
    /// API 参考。
    Api,
}

impl Volume {
    /// 全集（四册）。
    pub fn all() -> [Volume; 4] {
        [
            Volume::Creation,
            Volume::License,
            Volume::Publish,
            Volume::Api,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Volume::Creation => "创作指南",
            Volume::License => "许可指南",
            Volume::Publish => "发布指南",
            Volume::Api => "API 参考",
        }
    }
}

/// 一册。
#[derive(Clone, Debug)]
pub struct Book {
    /// 册目。
    pub volume: Volume,
    /// 章节标题。
    pub chapters: Vec<String>,
}

impl Book {
    /// 是否非空。
    pub fn non_empty(&self) -> bool {
        !self.chapters.is_empty()
    }
}

/// 文档套件（四册）。
#[derive(Clone, Debug, Default)]
pub struct DocSuite {
    books: Vec<Book>,
}

impl DocSuite {
    /// 空套件。
    pub fn new() -> DocSuite {
        DocSuite { books: Vec::new() }
    }

    /// 内置四册骨架。
    pub fn skeleton() -> DocSuite {
        let mut s = DocSuite::new();
        s.books.push(Book {
            volume: Volume::Creation,
            chapters: vec![
                String::from("资产制作教程"),
                String::from("设计规范"),
                String::from("示例集"),
            ],
        });
        s.books.push(Book {
            volume: Volume::License,
            chapters: vec![
                String::from("素材许可合规指引"),
                String::from("商用许可分级"),
                String::from("侵权应对"),
            ],
        });
        s.books.push(Book {
            volume: Volume::Publish,
            chapters: vec![
                String::from("上架流程 SOP"),
                String::from("定价与分发"),
                String::from("下架与撤回"),
            ],
        });
        s.books.push(Book {
            volume: Volume::Api,
            chapters: vec![
                String::from("资产模型"),
                String::from("工作流接口"),
                String::from("验证接口"),
            ],
        });
        s
    }

    /// 装入一册。
    pub fn put(&mut self, book: Book) {
        if let Some(slot) = self.books.iter_mut().find(|b| b.volume == book.volume) {
            *slot = book;
        } else {
            self.books.push(book);
        }
    }

    /// 取一册。
    pub fn get(&self, volume: Volume) -> Option<&Book> {
        self.books.iter().find(|b| b.volume == volume)
    }

    /// 四册齐全审计。
    pub fn audit_volumes(&self) -> Result<(), V0ECode> {
        for v in Volume::all() {
            match self.get(v) {
                Some(b) if b.non_empty() => {}
                Some(_) => return Err(V0ECode::EMPTY_VOLUME),
                None => return Err(V0ECode::VOLUME_MISSING),
            }
        }
        Ok(())
    }
}

// ── SOP 节点 ────────────────────────────────────────────────────────

/// SOP 节点（发布指南必备）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SopNode {
    /// 验证通过。
    Verified,
    /// 素材齐备。
    AssetsReady,
    /// 定价确认。
    Priced,
    /// 合规审查。
    Compliant,
    /// 上架提交。
    Submitted,
}

impl SopNode {
    /// 必备节点全集。
    pub fn required() -> [SopNode; 5] {
        [
            SopNode::Verified,
            SopNode::AssetsReady,
            SopNode::Priced,
            SopNode::Compliant,
            SopNode::Submitted,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            SopNode::Verified => "验证通过",
            SopNode::AssetsReady => "素材齐备",
            SopNode::Priced => "定价确认",
            SopNode::Compliant => "合规审查",
            SopNode::Submitted => "上架提交",
        }
    }
}

/// SOP 检查器。
#[derive(Clone, Debug, Default)]
pub struct SopChecklist {
    done: Vec<SopNode>,
}

impl SopChecklist {
    /// 新建。
    pub fn new() -> SopChecklist {
        SopChecklist { done: Vec::new() }
    }

    /// 完成一节。
    pub fn complete(&mut self, node: SopNode) {
        if !self.done.contains(&node) {
            self.done.push(node);
        }
    }

    /// SOP 完整性审计。
    pub fn audit(&self) -> Result<(), V0ECode> {
        for n in SopNode::required() {
            if !self.done.contains(&n) {
                return Err(V0ECode::SOP_GAP);
            }
        }
        Ok(())
    }

    /// 缺节清单。
    pub fn gaps(&self) -> Vec<SopNode> {
        SopNode::required()
            .iter()
            .filter(|n| !self.done.contains(n))
            .cloned()
            .collect()
    }
}

// ── 任务索引（F3348 复用）───────────────────────────────────────────

/// 索引条目。
#[derive(Clone, Debug)]
pub struct IndexEntry {
    /// 任务 ID。
    pub task: String,
    /// 文档节点名。
    pub node: String,
    /// 示例引用（可选）。
    pub example: Option<String>,
}

/// 任务索引。
#[derive(Clone, Debug, Default)]
pub struct TaskIndex {
    entries: Vec<IndexEntry>,
    /// 文档中实际存在的节点名。
    known_nodes: Vec<String>,
}

impl TaskIndex {
    /// 空索引。
    pub fn new() -> TaskIndex {
        TaskIndex {
            entries: Vec::new(),
            known_nodes: Vec::new(),
        }
    }

    /// 登记已知节点。
    pub fn declare_node(&mut self, node: &str) {
        self.known_nodes.push(String::from(node));
    }

    /// 加索引条目。
    pub fn add(&mut self, task: &str, node: &str, example: Option<&str>) {
        self.entries.push(IndexEntry {
            task: String::from(task),
            node: String::from(node),
            example: example.map(String::from),
        });
    }

    /// 漂移对账：索引指向的节点必须存在于文档。
    pub fn audit_drift(&self) -> Result<(), V0ECode> {
        for e in &self.entries {
            if !self.known_nodes.iter().any(|n| *n == e.node) {
                return Err(V0ECode::DRIFT);
            }
        }
        Ok(())
    }

    /// 示例存活审计。
    pub fn audit_examples(&self) -> Result<(), V0ECode> {
        for e in &self.entries {
            if let Some(ex) = &e.example {
                if !self.known_nodes.iter().any(|n| n == ex) {
                    return Err(V0ECode::EXAMPLE_DEAD);
                }
            }
        }
        Ok(())
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── 导航图（2 跳红线）───────────────────────────────────────────────

/// 导航边。
#[derive(Clone, Debug)]
pub struct NavEdge {
    /// 起点节点。
    pub from: String,
    /// 终点节点。
    pub to: String,
}

/// 导航图。
#[derive(Clone, Debug, Default)]
pub struct NavGraph {
    edges: Vec<NavEdge>,
}

impl NavGraph {
    /// 空图。
    pub fn new() -> NavGraph {
        NavGraph { edges: Vec::new() }
    }

    /// 加边。
    pub fn link(&mut self, from: &str, to: &str) {
        self.edges.push(NavEdge {
            from: String::from(from),
            to: String::from(to),
        });
    }

    /// BFS 最短跳数。不可达返回 None。
    pub fn depth(&self, from: &str, to: &str) -> Option<u32> {
        if from == to {
            return Some(0);
        }
        let mut visited: Vec<String> = vec![String::from(from)];
        let mut frontier: Vec<(String, u32)> = vec![(String::from(from), 0)];
        while let Some((node, d)) = {
            if frontier.is_empty() {
                None
            } else {
                Some(frontier.remove(0))
            }
        } {
            for e in self.edges.iter().filter(|e| e.from == node) {
                if e.to == to {
                    return Some(d + 1);
                }
                if !visited.iter().any(|v| *v == e.to) {
                    visited.push(e.to.clone());
                    frontier.push((e.to.clone(), d + 1));
                }
            }
        }
        None
    }

    /// 2 跳红线审计：任意导航 ≤ 2 跳。
    pub fn audit_maze(&self) -> Result<(), V0ECode> {
        // 对每条边的终点做从起点出发的深度检查；更深路径若存在直连则 OK。
        // 简化口径：图直径（所有可达对的最短路最大值）≤ 2。
        let mut nodes: Vec<String> = Vec::new();
        for e in &self.edges {
            if !nodes.contains(&e.from) {
                nodes.push(e.from.clone());
            }
            if !nodes.contains(&e.to) {
                nodes.push(e.to.clone());
            }
        }
        for a in &nodes {
            for b in &nodes {
                if a != b {
                    if let Some(d) = self.depth(a, b) {
                        if d > 2 {
                            return Err(V0ECode::NAV_MAZE);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// F2879 家族模式索引（家族九用）。
pub const FAMILY_MEMBER_INDEX: u16 = 9;

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F2879", "VE-F3356", "CI-HOOK"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves14_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3614");

    // 0x3e01 四册齐全
    let suite = DocSuite::skeleton();
    out.add("0x3e01-四册齐全", suite.audit_volumes().is_ok(), "");

    // 0x3e02 缺册检出
    let mut suite2 = DocSuite::new();
    suite2.put(Book {
        volume: Volume::Creation,
        chapters: vec![String::from("导读")],
    });
    out.add(
        "0x3e02-缺册被检出",
        suite2.audit_volumes().is_err(),
        "",
    );

    // 0x3e03 空册检出
    let mut suite3 = DocSuite::skeleton();
    suite3.put(Book {
        volume: Volume::Api,
        chapters: vec![],
    });
    out.add("0x3e03-空册被检出", suite3.audit_volumes().is_err(), "");

    // 0x3e04 SOP 全过
    let mut sop = SopChecklist::new();
    for n in SopNode::required() {
        sop.complete(n);
    }
    out.add("0x3e04-SOP 全过", sop.audit().is_ok(), "");

    // 0x3e05 SOP 缺节
    let mut sop2 = SopChecklist::new();
    sop2.complete(SopNode::Verified);
    out.add("0x3e05-SOP 缺节检出", sop2.audit().is_err() && sop2.gaps().len() == 4, "");

    // 0x3e06 索引无漂移
    let mut idx = TaskIndex::new();
    idx.declare_node("创作指南");
    idx.add("T1", "创作指南", None);
    out.add("0x3e06-索引无漂移", idx.audit_drift().is_ok(), "");

    // 0x3e07 漂移检出
    let mut idx2 = TaskIndex::new();
    idx2.add("T1", "不存在的节点", None);
    out.add("0x3e07-索引漂移检出", idx2.audit_drift().is_err(), "");

    // 0x3e08 示例存活
    let mut idx3 = TaskIndex::new();
    idx3.declare_node("示例A");
    idx3.add("T1", "示例A", Some("示例A"));
    out.add("0x3e08-示例存活", idx3.audit_examples().is_ok(), "");

    // 0x3e09 导航 2 跳内
    let mut nav = NavGraph::new();
    nav.link("首页", "创作");
    nav.link("首页", "发布");
    nav.link("创作", "深层");
    out.add("0x3e09-导航 2 跳", nav.depth("首页", "深层") == Some(2), "");

    // 0x3e0a 迷宫检出
    let mut nav2 = NavGraph::new();
    nav2.link("a", "b");
    nav2.link("b", "c");
    nav2.link("c", "d");
    out.add("0x3e0a-3 跳迷宫被检出", nav2.audit_maze().is_err(), "");

    out
}
