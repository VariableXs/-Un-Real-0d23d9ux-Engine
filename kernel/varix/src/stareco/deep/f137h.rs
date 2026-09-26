//! 深化层五 · F137 API 稳定性承诺（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：冻结/破坏台账 → 文档站 API 页版本史渲染行、
//! 徽标位数据（稳定/冻结/弃用三态）、破坏性变更页面标红判定。

use super::f137f::{BreakKind, classify_break};
use super::f137g::FreezeTable;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 版本史渲染行：变更记录 → API 页行（版本/变更/级别），破坏性标红
// ---------------------------------------------------------------------------

pub struct HistoryRow {
    pub version: u32,
    pub kind_label: &'static str,
    pub breaking: bool,
}

pub fn history_rows(records: &[(u32, BreakKind)]) -> alloc::vec::Vec<HistoryRow> {
    records
        .iter()
        .map(|(ver, kind)| HistoryRow {
            version: *ver,
            kind_label: match kind {
                BreakKind::ParamAdded => "参数新增",
                BreakKind::ParamRemoved => "参数删除",
                BreakKind::ParamTypeChanged => "参数类型变更",
                BreakKind::ReturnChanged => "返回类型变更",
                BreakKind::SemanticChanged => "语义变更",
                BreakKind::DocsOnly => "文档修订",
            },
            breaking: classify_break(*kind) != super::f137f::BreakLevel::NonBreaking,
        })
        .collect()
}

/// 页面标红：破坏性行渲染时加警示位（ Silent 也标——最阴险的更显眼）。
pub fn page_alert(row: &HistoryRow) -> bool {
    row.breaking
}

// ---------------------------------------------------------------------------
// 徽标位数据：冻结表 + 弃用登记 → 三态徽标（稳定/冻结/弃用）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiBadge {
    Stable,
    Frozen,
    Deprecated,
}

/// 装配：冻结表命中 → Frozen；弃用登记命中 → Deprecated；否则 Stable。
/// 同时命中时弃用优先（最重状态最显眼）。
pub fn api_badge(
    api: &str,
    freeze: &FreezeTable,
    deprecated: &[&'static str],
) -> ApiBadge {
    if deprecated.iter().any(|d| *d == api) {
        return ApiBadge::Deprecated;
    }
    if freeze.is_frozen(api) {
        return ApiBadge::Frozen;
    }
    ApiBadge::Stable
}

// ---------------------------------------------------------------------------
// 消费方守卫接口：调用弃用 API → 编译期级警告数据（CI 消费）
// ---------------------------------------------------------------------------

pub struct DeprecationWarning {
    pub caller: &'static str,
    pub api: &'static str,
    pub severity: u8, // 1=警告 2=错误（Hard 破坏后仍在调用）
}

pub fn deprecation_warnings(
    callers: &[(&'static str, &'static str)],
    deprecated: &[&'static str],
    hard_removed: &[&'static str],
) -> alloc::vec::Vec<DeprecationWarning> {
    callers
        .iter()
        .filter(|(_, api)| deprecated.contains(api) || hard_removed.contains(api))
        .map(|(caller, api)| DeprecationWarning {
            caller: *caller,
            api: *api,
            severity: if hard_removed.contains(api) { 2 } else { 1 },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F137H_TAG: &str = "stareco-F137-deep5";

pub fn run_f137_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F137H_TAG);

    // 版本史行
    let rows = history_rows(&[
        (3, BreakKind::ParamAdded),
        (4, BreakKind::DocsOnly),
        (5, BreakKind::SemanticChanged),
    ]);
    set.add(
        "f137h labels",
        rows[0].kind_label == "参数新增" && rows[1].kind_label == "文档修订",
        "级别文案",
    );
    set.add(
        "f137h breaking flags",
        rows[0].breaking && !rows[1].breaking && rows[2].breaking,
        "破坏位三态",
    );
    set.add("f137h page alert", page_alert(&rows[2]), "语义变更也标红");

    // 徽标位
    let mut ft = FreezeTable::new();
    let _ = ft.freeze("api_a", 3, None);
    let deprecated = ["api_c"];
    set.add("f137h badge stable", api_badge("api_b", &ft, &deprecated) == ApiBadge::Stable, "默认稳定");
    set.add("f137h badge frozen", api_badge("api_a", &ft, &deprecated) == ApiBadge::Frozen, "冻结徽标");
    set.add(
        "f137h badge deprecated wins",
        api_badge("api_c", &ft, &deprecated) == ApiBadge::Deprecated,
        "弃用最显眼",
    );

    // 弃用警告
    let callers = [("files", "api_c"), ("editor", "api_d"), ("legacy", "api_hard")];
    let warns = deprecation_warnings(&callers, &["api_c"], &["api_hard"]);
    set.add(
        "f137h warns",
        warns.len() == 2
            && warns[0].severity == 1
            && warns[1].severity == 2,
        "警告/错误分级",
    );
    set.add(
        "f137h clean caller",
        !warns.iter().any(|w| w.caller == "editor"),
        "干净调用方零误伤",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn history_version_order_preserved() {
        let rows = history_rows(&[(2, BreakKind::DocsOnly), (1, BreakKind::DocsOnly)]);
        // 登记序即渲染序（排序是目录层职责，史行保持登记顺序）。
        assert_eq!(rows[0].version, 2);
        assert_eq!(rows[1].version, 1);
    }
}
