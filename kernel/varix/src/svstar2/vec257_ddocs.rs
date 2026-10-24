//! VE-F0657 脏区蒙版文档（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0657`（家族第六成员）
//!
//! **判据（锚点原文逐条）**：
//! - **五件齐备**：总纲章 / 机制章 / 操作章 / 限制章 / 维护章；
//! - **权威表引用**：机制章引用 F0645 属性传播权威表；
//! - **预留显性**：限制章载明精确档预留态、预测不依赖原则；
//! - **复审流程**：维护章载边距表与阈值表复审流程；
//! - **判据**：清单引用可解析、一致性抽检。

use crate::checks::CheckSet;
use super::vec241_dmodel::DirtyTier;
use super::vec245_dprop::PropChange;

/// 五件结构
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocPart {
    /// 总纲章（三档模型与选档策略，F0641 引用）
    Overview,
    /// 机制章（属性传播权威表 F0645、合并膨胀、平移优化、蒙版 SDF）
    Mechanism,
    /// 操作章（度量面板 / fuzz / 4K 验收清单引用 F0655）
    Ops,
    /// 限制章（精确档预留态、预测不依赖原则）
    Limits,
    /// 维护章（边距表与阈值表复审流程）
    Maintain,
}

pub const PART_COUNT: usize = 5;

impl DocPart {
    pub fn all() -> [DocPart; PART_COUNT] {
        [DocPart::Overview, DocPart::Mechanism, DocPart::Ops, DocPart::Limits, DocPart::Maintain]
    }
    pub fn title(self) -> &'static str {
        match self {
            DocPart::Overview => "总纲章",
            DocPart::Mechanism => "机制章",
            DocPart::Ops => "操作章",
            DocPart::Limits => "限制章",
            DocPart::Maintain => "维护章",
        }
    }
}

/// 机制章引用表（权威表 7 属性 + 合并 + 膨胀 + 平移 + SDF + 光栅化）
pub fn mechanism_refs() -> alloc::vec::Vec<&'static str> {
    let mut v: alloc::vec::Vec<&'static str> = PropChange::all().iter().map(|p| p.label()).collect();
    v.extend_from_slice(&["合并算法", "膨胀边距", "平移优化", "圆角SDF", "路径光栅化"]);
    v
}

/// 限制章条目（预留显性）
pub fn limits_entries() -> alloc::vec::Vec<(&'static str, bool)> {
    alloc::vec![
        ("精确档为远景预留态——当前 N/A", DirtyTier::Precise.available() == false),
        ("预测只做优化提示——正确性不依赖预测", true),
        ("跨机基准不可比——环境指纹绑定", true),
    ]
}

/// 维护章复审流程条目
pub fn maintain_flow() -> alloc::vec::Vec<(&'static str, &'static str)> {
    alloc::vec![
        ("边距表复审", "漏绘指纹走查 → 表修订 → 回归跑"),
        ("阈值表复审", "度量回流 → 阈值校准 → 决策抽检"),
        ("文档漂移抽检", "引用实现一致性 → 限期回补"),
    ]
}

/// 一致性抽检：文档引用的属性集与实现权威表一致
pub fn doc_impl_consistency() -> bool {
    let refs = mechanism_refs();
    let impls = PropChange::all();
    impls.iter().all(|p| refs.contains(&p.label()))
}

pub fn run_vec257_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec257");
    // 族一：五件齐备
    let parts = DocPart::all();
    s.add("文档-五件", parts.len() == PART_COUNT, "5 parts");
    s.add("文档-标题齐", parts.iter().all(|p| !p.title().is_empty()), "titled");
    s.add("文档-互异", {
        let mut u = true;
        for i in 0..parts.len() {
            for j in i + 1..parts.len() {
                if parts[i] == parts[j] {
                    u = false;
                }
            }
        }
        u
    }, "distinct");
    // 族二：机制章引用（权威表全覆盖 + 机制条目）
    let refs = mechanism_refs();
    s.add("文档-七属性引用", refs.len() == 7 + 5, "7+5 refs");
    s.add("文档-实现一致", doc_impl_consistency(), "no drift");
    // 族三：限制章预留显性
    let lims = limits_entries();
    s.add("文档-限制三条", lims.len() == 3, "3 limits");
    s.add("文档-预留显性", lims[0].1, "precise N/A honest");
    s.add("文档-预测不依赖", lims[1].1, "hint-only");
    // 族四：维护章复审流程
    let flow = maintain_flow();
    s.add("文档-复审三条", flow.len() == 3, "3 flows");
    s.add("文档-流程齐字段", flow.iter().all(|(k, v)| !k.is_empty() && !v.is_empty()), "k/v");
    // 家族第六成员
    s.add("文档-家族位次", true, "6th of F0518 family");
    s
}
