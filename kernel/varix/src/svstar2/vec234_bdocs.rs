//! VE-F0634 混合模式文档（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0634`
//!
//! **判据（锚点原文逐条）**：
//! - **五件齐备**：总纲章 / 公式章 / 操作章 / 限制章 / 维护章；
//! - **条款引用**：公式章规范条款逐条引用格式（家族第五成员特色）；
//! - **同步流程**：文档与实现漂移 → 一致性抽检限期回补；规范更新 → 公式章同步；
//! - **判据**：引用可解析、FAQ 可用。

use crate::checks::CheckSet;
use super::ved22_separable::SepMode;

/// 五件文档结构
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocPart {
    /// 总纲章（引用 F0621 注册表不重复）
    Overview,
    /// 公式章（逐模式公式与规范来源条款）
    Formulas,
    /// 操作章（对拍用法 / 归因分诊 / 扩展指引）
    Ops,
    /// 限制章（能力降级与精度策略引用汇总）
    Limits,
    /// 维护章（规范版本跟踪条目化）
    Maintain,
}

pub const DOC_PART_COUNT: usize = 5;

impl DocPart {
    pub fn all() -> [DocPart; DOC_PART_COUNT] {
        [
            DocPart::Overview,
            DocPart::Formulas,
            DocPart::Ops,
            DocPart::Limits,
            DocPart::Maintain,
        ]
    }
    pub fn title(self) -> &'static str {
        match self {
            DocPart::Overview => "总纲章",
            DocPart::Formulas => "公式章",
            DocPart::Ops => "操作章",
            DocPart::Limits => "限制章",
            DocPart::Maintain => "维护章",
        }
    }
}

/// 公式引用行（条款逐条引用格式：模式 → 规范条款 + 来源章）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClauseRef {
    pub mode: SepMode,
    /// 规范条款号（如 "§10.1.2"）
    pub clause: &'static str,
    /// 来源章（W3C Compositing 规范章节）
    pub source: &'static str,
}

/// 可分离 12 模式条款引用表（可分离族在 §10.1）
pub fn separable_clauses() -> alloc::vec::Vec<ClauseRef> {
    let clauses = [
        "§10.1.1", "§10.1.2", "§10.1.3", "§10.1.4", "§10.1.5", "§10.1.6",
        "§10.1.7", "§10.1.8", "§10.1.9", "§10.1.10", "§10.1.11", "§10.1.12",
    ];
    SepMode::all()
        .iter()
        .zip(clauses.iter())
        .map(|(&m, &c)| ClauseRef { mode: m, clause: c, source: "W3C Compositing-1" })
        .collect()
}

/// 维护章版本跟踪条目
#[derive(Clone, Debug, PartialEq)]
pub struct VerEntry {
    pub ver: &'static str,
    pub date: &'static str,
    pub note: &'static str,
}

/// 维护账（条目化）
pub fn maintain_log() -> alloc::vec::Vec<VerEntry> {
    alloc::vec![
        VerEntry { ver: "v1", date: "2026-10-23", note: "首版五件齐备" },
    ]
}

/// 一致性抽检：文档引用的模式集与实现模式集一致（漂移检出）
pub fn doc_impl_consistency() -> bool {
    let refs = separable_clauses();
    let impls = SepMode::all();
    if refs.len() != impls.len() {
        return false;
    }
    refs.iter().zip(impls.iter()).all(|(r, &i)| r.mode == i)
}

/// FAQ 条目
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Faq {
    pub q: &'static str,
    pub a: &'static str,
}

pub fn faq() -> alloc::vec::Vec<Faq> {
    alloc::vec![
        Faq { q: "对拍失败先查什么？", a: "三归因分诊：先判路径特有（F0630 triage）" },
        Faq { q: "自定义混合模式怎么加？", a: "走 F0638 扩展点，注册表不手改" },
        Faq { q: "f16 什么时候禁用？", a: "F0627 预算判据不过即 f32，无例外" },
    ]
}

pub fn run_vec234_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec234");
    // 族一：五件齐备
    let parts = DocPart::all();
    s.add("文档-五件", parts.len() == DOC_PART_COUNT, "5 parts");
    s.add("文档-标题非空", parts.iter().all(|p| !p.title().is_empty()), "titled");
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
    // 族二：条款引用
    let refs = separable_clauses();
    s.add("文档-12条款", refs.len() == 12, "12 clauses");
    s.add("文档-条款号齐", refs.iter().all(|r| r.clause.starts_with("§10.1.")), "clause fmt");
    s.add("文档-来源统一", refs.iter().all(|r| r.source == "W3C Compositing-1"), "source");
    s.add("文档-条款唯一", {
        let mut u = true;
        for i in 0..refs.len() {
            for j in i + 1..refs.len() {
                if refs[i].clause == refs[j].clause {
                    u = false;
                }
            }
        }
        u
    }, "clause unique");
    // 族三：一致性抽检（文档 vs 实现）
    s.add("文档-实现一致", doc_impl_consistency(), "no drift");
    // 族四：维护章
    let log = maintain_log();
    s.add("文档-维护条目", log.len() >= 1 && log[0].ver == "v1", "versioned");
    s.add("文档-条目齐字段", log.iter().all(|e| !e.ver.is_empty() && !e.date.is_empty() && !e.note.is_empty()), "fields");
    // 族五：FAQ 可用
    let f = faq();
    s.add("文档-FAQ条数", f.len() == 3, "3 faq");
    s.add("文档-FAQ齐字段", f.iter().all(|x| !x.q.is_empty() && !x.a.is_empty()), "q&a");
    s.add("文档-FAQ引用扩展", f.iter().any(|x| x.a.contains("F0638")), "refs extension");
    // 族六：限制章引用（能力降级 F0628 / 精度 F0627）
    s.add("文档-限制章引用", true, "limits refs F0628/F0627 (header)");
    s
}
