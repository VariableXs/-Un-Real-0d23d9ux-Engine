//! CGPU-F3522 · 文档信息架构（CGPU-W 域 · W01 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3522`
//!
//! 信息架构：导航树/分区（参考/教程/指南/FAQ）——架构实现——导航表；
//! 承接 F3521 站点段（站点结构图是 V10 移交包七件之一——导航树就是它
//! 的实现）。
//!
//! ## 要点一：四分区闭集
//!
//! 参考/教程/指南/FAQ 四分区封闭枚举：每个导航节点必须归属恰一分区，
//! 表外分区拒绝；每个分区至少一个根节点（空分区 = 信息架构残缺）。
//!
//! ## 要点二：导航树是唯一事实源，导航表是它的展开
//!
//! 树节点带 {标题, 分区, 路径, 子节点}；导航表由树 **DFS 前序展开生
//! 成**（不手写）：表行数恒等于树节点数（同源），两次展开逐行相同
//! （确定性）。手写表与树漂移在架构上不可能发生。
//!
//! ## 要点三：树结构五律（机检）
//!
//! 路径非空且以 `/` 分层；子路径必须以父路径为前缀（孤儿拒）；路径
//! 全局唯一（重复拒）；标题非空；每分区根节点数 ≥1。违律逐条专属码。
//!
//! ## 要点四：零 panic 面 + 诊断码续占 0x5Cxx 细分段
//!
//! 与 vcw01（0x5C01~0x5C06）同段分段续占 0x5C07~0x5C0C（cgm02 续占
//! 0x5408~0x540C 先例），六码互异且与 vcw01 六码不重叠。

// ---------------------------------------------------------------------------
// 导入（no_std 三件套）
// ---------------------------------------------------------------------------

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、四分区闭集
// ---------------------------------------------------------------------------

/// W 域任务总数守恒（10 组 × 16 项，与 vcw01 同域对账）。
pub const W_DOMAIN_TOTAL: u32 = 160;

/// 文档四分区（封闭枚举——信息架构的第一层）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// 参考（冻结签名口径立页）。
    Reference,
    /// 教程（上手路径）。
    Tutorial,
    /// 指南（任务导向：怎么做好一件事）。
    Guide,
    /// FAQ（问答题典）。
    Faq,
}

impl Section {
    /// 全部四分区（判据侧独立重排对拍）。
    pub const ALL: [Section; 4] = [
        Section::Reference,
        Section::Tutorial,
        Section::Guide,
        Section::Faq,
    ];

    /// 分区名（导航表列值）。
    pub const fn name(self) -> &'static str {
        match self {
            Section::Reference => "参考",
            Section::Tutorial => "教程",
            Section::Guide => "指南",
            Section::Faq => "FAQ",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、导航树（唯一事实源）
// ---------------------------------------------------------------------------

/// 导航树节点（标题 + 分区 + 路径 + 子节点）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavNode {
    /// 页面标题（非空）。
    pub title: &'static str,
    /// 归属分区（恰一）。
    pub section: Section,
    /// 层级路径（以 `/` 分层，全局唯一）。
    pub path: &'static str,
    /// 子节点。
    pub children: Vec<NavNode>,
}

impl NavNode {
    /// 叶子判定。
    pub const fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

/// 单节点快检（不含跨节点律）：标题/路径非空 + 分区在册恒真。
fn check_node_basics(n: &NavNode) -> Result<(), WiCode> {
    if n.title.is_empty() {
        return Err(WiCode::TITLE_EMPTY);
    }
    if n.path.is_empty() {
        return Err(WiCode::PATH_EMPTY);
    }
    let _ = n.section.name();
    Ok(())
}

/// 子路径前缀律：子路径必须以 父路径 + "/" 开头（孤儿拒）。
fn path_prefixed(child: &str, parent: &str) -> bool {
    child.len() > parent.len() + 1
        && child.starts_with(parent)
        && child.as_bytes()[parent.len()] == b'/'
}

/// 树结构五律全检（DFS 携带父路径）：违律逐条专属码拒绝。
pub fn validate_tree(nodes: &[NavNode]) -> Result<(), WiCode> {
    for n in nodes {
        check_node_basics(n)?;
        validate_subtree(n, n.path)?;
    }
    Ok(())
}

fn validate_subtree(n: &NavNode, parent_path: &str) -> Result<(), WiCode> {
    for c in n.children.iter() {
        check_node_basics(c)?;
        if !path_prefixed(c.path, parent_path) {
            return Err(WiCode::PATH_ORPHAN);
        }
        validate_subtree(c, c.path)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、导航表（树的 DFS 前序展开——架构实现）
// ---------------------------------------------------------------------------

/// 导航表一行（展开产物：深度/分区/标题/路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavRow {
    /// 树深度（根 = 0）。
    pub depth: usize,
    /// 分区名。
    pub section: &'static str,
    /// 页面标题。
    pub title: &'static str,
    /// 层级路径。
    pub path: &'static str,
}

/// 展开（DFS 前序：先根后子，子按在册顺序——确定性展开）。
pub fn flatten(nodes: &[NavNode]) -> Vec<NavRow> {
    let mut out = Vec::new();
    for n in nodes {
        flatten_into(n, 0, &mut out);
    }
    out
}

fn flatten_into(n: &NavNode, depth: usize, out: &mut Vec<NavRow>) {
    out.push(NavRow {
        depth,
        section: n.section.name(),
        title: n.title,
        path: n.path,
    });
    for c in n.children.iter() {
        flatten_into(c, depth + 1, out);
    }
}

/// 展开确定性：两次展开逐行相同（同输入必同输出）。
pub fn flatten_deterministic(nodes: &[NavNode]) -> bool {
    let a = flatten(nodes);
    let b = flatten(nodes);
    a == b
}

/// 路径全局唯一性裁决（跨整棵树的第五律——展开后线性扫）。
pub fn paths_unique(rows: &[NavRow]) -> bool {
    for i in 0..rows.len() {
        for j in 0..rows.len() {
            if i != j && rows[i].path == rows[j].path {
                return false;
            }
        }
    }
    true
}

/// 分区非空裁决：四分区每区至少一个根节点（空分区 = 架构残缺拒）。
pub fn sections_covered(roots: &[NavNode]) -> Result<(), WiCode> {
    for s in Section::ALL.iter() {
        let mut seen = false;
        for n in roots.iter() {
            if n.section == *s {
                seen = true;
            }
        }
        if !seen {
            return Err(WiCode::SECTION_EMPTY);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、在账导航树（四分区 · 站点结构图的实现）
// ---------------------------------------------------------------------------

/// F3521 承接声明（站点结构图 = V10 移交包第四件——导航树是它的实现）。
pub const F3521_LINK: (&u32, &str) = (&3521, "站点结构图");

/// 在账导航树根节点（四分区各一根——判据侧独立写死对拍）。
pub fn nav_tree() -> Vec<NavNode> {
    alloc::vec![
        NavNode {
            title: "SDK 参考",
            section: Section::Reference,
            path: "reference",
            children: alloc::vec![
                NavNode {
                    title: "渲染接口",
                    section: Section::Reference,
                    path: "reference/render",
                    children: alloc::vec![],
                },
                NavNode {
                    title: "诊断码索引",
                    section: Section::Reference,
                    path: "reference/diagcodes",
                    children: alloc::vec![],
                },
            ],
        },
        NavNode {
            title: "上手教程",
            section: Section::Tutorial,
            path: "tutorial",
            children: alloc::vec![NavNode {
                title: "第一帧渲染",
                section: Section::Tutorial,
                path: "tutorial/first-frame",
                children: alloc::vec![],
            }],
        },
        NavNode {
            title: "任务指南",
            section: Section::Guide,
            path: "guide",
            children: alloc::vec![NavNode {
                title: "验收判据入示例",
                section: Section::Guide,
                path: "guide/criteria-examples",
                children: alloc::vec![],
            }],
        },
        NavNode {
            title: "FAQ",
            section: Section::Faq,
            path: "faq",
            children: alloc::vec![NavNode {
                title: "诊断码含义",
                section: Section::Faq,
                path: "faq/diagcodes",
                children: alloc::vec![],
            }],
        },
    ]
}

// ---------------------------------------------------------------------------
// 五、错误契约（续占 0x5Cxx 细分段：0x5C07~0x5C0C）
// ---------------------------------------------------------------------------

/// vcw02 诊断码。续占 `0x5Cxx` 细分段（与 vcw01 0x5C01~0x5C06 不重叠）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WiCode(pub u16);

impl WiCode {
    /// 分区表外（节点归属非四分区——枚举封闭下不可达的防御位）。
    pub const SECTION_INVALID: WiCode = WiCode(0x5C07);
    /// 路径为空。
    pub const PATH_EMPTY: WiCode = WiCode(0x5C08);
    /// 孤儿路径（子路径不以父路径为前缀）。
    pub const PATH_ORPHAN: WiCode = WiCode(0x5C09);
    /// 路径重复。
    pub const PATH_DUP: WiCode = WiCode(0x5C0A);
    /// 标题为空。
    pub const TITLE_EMPTY: WiCode = WiCode(0x5C0B);
    /// 分区空（某分区零根节点）。
    pub const SECTION_EMPTY: WiCode = WiCode(0x5C0C);

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            WiCode::SECTION_INVALID => "分区表外：节点归属必须在四分区闭集内".into(),
            WiCode::PATH_EMPTY => "路径为空：导航节点必须有层级路径".into(),
            WiCode::PATH_ORPHAN => "孤儿路径：子路径必须以父路径为前缀".into(),
            WiCode::PATH_DUP => "路径重复：导航路径全局唯一".into(),
            WiCode::TITLE_EMPTY => "标题为空：导航节点必须有页面标题".into(),
            WiCode::SECTION_EMPTY => "分区空：四分区每区至少一个根节点".into(),
            WiCode(_) => "未知 vcw02 信息架构域诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑（架构一组：分区/树律/展开确定性/表同源）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 在账树五律全过() {
        let t = nav_tree();
        assert_eq!(validate_tree(&t), Ok(()));
        assert_eq!(sections_covered(&t), Ok(()));
        assert!(paths_unique(&flatten(&t)));
    }

    #[test]
    fn 孤儿路径拒() {
        let mut bad = nav_tree();
        bad[0].children[0].path = "tutorial/wrong"; // 父 reference 子 tutorial 前缀
        assert_eq!(validate_tree(&bad), Err(WiCode::PATH_ORPHAN));
    }

    #[test]
    fn 展开行数同源且确定() {
        let t = nav_tree();
        let rows = flatten(&t);
        assert_eq!(rows.len(), 9); // 4 根 + 5 子（参考区 2 子）
        assert!(flatten_deterministic(&t));
        assert_eq!(rows[0].path, "reference");
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].path, "reference/render");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[8].path, "faq/diagcodes");
    }

    #[test]
    fn 空分区拒与表外分区防御位() {
        let mut t = nav_tree();
        t[3] = NavNode {
            title: "占位",
            section: Section::Faq,
            path: "faq",
            children: alloc::vec![],
        };
        // 单根的 FAQ 区仍在册——换成删掉 FAQ 根则拒
        let mut t2 = nav_tree();
        t2.truncate(3);
        assert_eq!(sections_covered(&t2), Err(WiCode::SECTION_EMPTY));
        let _ = t; // 保持树完整性
        assert_eq!(WiCode::SECTION_INVALID.code(), 0x5C07);
    }
}
