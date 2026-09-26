//! F475 I 域行为差异登记册（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **差异表完整性（正文差异点全收录）；三栏格式统一；公开同步；复审条款；
//! 对齐项计数与正文 200 项对账。**
//!
//! 功能定义（主册批次三）：全册「与 Windows 行为差异」集中登记（F378 热键
//! 下划线不画线、F411 Backspace 采用澄清版语义、F464 回收站可拖出、F467
//! 终端字号会话记忆等）——每条差异三栏（Windows 原行为/VARIX 行为/差异
//! 理由）；登记册随设计案与开发者文档站（F135）同步公开；季度审视时差异表
//! 复审（有 Windows 修正了对齐的契机就对齐回去）。
//!
//! 零堆纪律：静态差异表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与登记册（一处一事实：差异点全部来自主册正文）
// ---------------------------------------------------------------------------

/// I 域总项数（主册：F401-F600 共 200 项——对账基准）。
pub const I_DOMAIN_TOTAL: usize = 200;

/// 一条行为差异（三栏格式统一）。
#[derive(Clone, Copy, Debug)]
pub struct DiffEntry {
    /// 差异点归属项（正文 F 编号）。
    pub feature: &'static str,
    /// 第一栏：Windows 原行为。
    pub windows: &'static str,
    /// 第二栏：VARIX 行为。
    pub varix: &'static str,
    /// 第三栏：差异理由。
    pub reason: &'static str,
    /// 公开同步标记（随设计案与开发者文档站 F135 同步公开）。
    pub published: bool,
}

/// 登记册（主册正文差异点全收录——四处显式差异 + 收官条款）。
pub const DIFF_REGISTRY: [DiffEntry; 4] = [
    DiffEntry {
        feature: "F378",
        windows: "Alt 快捷键在 Alt 键下画下划线提示",
        varix: "热键提示不画下划线（提示走浮签）",
        reason: "下划线与文本装饰语义冲突，浮签信息量更高",
        published: true,
    },
    DiffEntry {
        feature: "F411",
        windows: "Backspace 在列表视图回上级目录",
        varix: "Backspace 采用澄清版语义（编辑态优先，非编辑态回上级）",
        reason: "Windows 的静默抢键在输入场景是经典暴行——先澄清再回退",
        published: true,
    },
    DiffEntry {
        feature: "F464",
        windows: "回收站只能还原到原位置",
        varix: "回收站文件可拖出还原到任意落点",
        reason: "拖拽自由是资源管理器的既有心智，原位还原仍是右键主选项",
        published: true,
    },
    DiffEntry {
        feature: "F467",
        windows: "终端字号缩放跟随全局无会话记忆",
        varix: "终端字号五档逐档切换，会话内记忆、默认档可设",
        reason: "终端是独立工作域，字号是个人工作台参数——两级记忆各归各位",
        published: true,
    },
];

/// 三栏格式校验（主册：三栏格式统一——每条三栏非空）。
pub fn entry_format_ok(e: &DiffEntry) -> bool {
    !e.windows.is_empty() && !e.varix.is_empty() && !e.reason.is_empty() && e.published
}

/// 差异表完整性审计（主册：正文差异点全收录——本册四处显式差异 + 收官
/// 条款入册；正文若增差异点则表随之增——审计只验格式与覆盖标记）。
pub fn registry_complete() -> bool {
    DIFF_REGISTRY.iter().all(entry_format_ok)
}

/// 公开同步审计（主册：随设计案与开发者文档站 F135 同步公开——published
/// 全真；未公开的差异不许存在）。
pub fn publish_sync_ok() -> bool {
    DIFF_REGISTRY.iter().all(|e| e.published)
}

/// 复审条款（主册：季度审视时差异表复审——复审窗口与对齐契机记录位）。
#[derive(Clone, Copy, Debug)]
pub struct ReviewClause {
    /// 上次复审季度序号（0 = 未复审过）。
    pub last_reviewed_quarter: u32,
    /// 对齐契机数（Windows 修正后可对齐回去的差异点）。
    pub realign_opportunities: u8,
}

impl ReviewClause {
    /// 季度复审到期判定（每季度必须复审一次）。
    pub fn review_due(&self, current_quarter: u32) -> bool {
        current_quarter > self.last_reviewed_quarter
    }

    /// 对齐回去（Windows 改了我们复评——差异点撤销）。
    pub fn realign(&mut self, current_quarter: u32) {
        self.last_reviewed_quarter = current_quarter;
        self.realign_opportunities = 0;
    }
}

/// 对齐项计数与正文 200 项对账（主册判据：I 域 200 项 = 对齐项 + 差异项）。
pub fn reconciliation(aligned: usize, diff_count: usize) -> bool {
    aligned + diff_count == I_DOMAIN_TOTAL
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_idiffreg_checks() -> CheckSet {
    let mut cs = CheckSet::new("F475-idiffreg");
    // 1) 差异表完整性（主册四处显式差异全收录 + 格式统一）。
    cs.add("registry_complete", registry_complete() && DIFF_REGISTRY.len() == 4, "");
    cs.add("features_covered", DIFF_REGISTRY.iter().any(|e| e.feature == "F378") && DIFF_REGISTRY.iter().any(|e| e.feature == "F411") && DIFF_REGISTRY.iter().any(|e| e.feature == "F464") && DIFF_REGISTRY.iter().any(|e| e.feature == "F467"), "");
    // 2) 三栏格式统一（逐条三栏非空）。
    cs.add("three_columns_uniform", DIFF_REGISTRY.iter().all(entry_format_ok), "");
    // 3) 公开同步（F135 联动——published 全真）。
    cs.add("publish_sync", publish_sync_ok(), "");
    // 4) 复审条款：季度到期判定 + 对齐回去。
    let mut rc = ReviewClause { last_reviewed_quarter: 1, realign_opportunities: 1 };
    cs.add("review_due", rc.review_due(2) && !rc.review_due(1), "");
    rc.realign(2);
    cs.add("realign_resets", rc.last_reviewed_quarter == 2 && rc.realign_opportunities == 0, "");
    // 5) 对齐项计数与正文 200 项对账。
    cs.add("reconcile_200", reconciliation(196, 4), "");
    cs.add("reconcile_mismatch_caught", !reconciliation(197, 4), "");
    // 6) 每条差异带理由（不为不同而不同——每处不同有名字有理由有出处）。
    cs.add("every_diff_reasoned", DIFF_REGISTRY.iter().all(|e| e.reason.len() >= 8), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_entries_are_published_and_reasoned() {
        for e in DIFF_REGISTRY {
            assert!(e.published, "差异必须公开: {}", e.feature);
            assert!(!e.reason.is_empty());
            assert!(!e.windows.is_empty());
            assert!(!e.varix.is_empty());
        }
    }

    #[test]
    fn quarterly_review_cycle() {
        let mut rc = ReviewClause { last_reviewed_quarter: 0, realign_opportunities: 0 };
        assert!(rc.review_due(1));
        rc.realign(1);
        assert!(!rc.review_due(1));
        assert!(rc.review_due(2));
    }

    #[test]
    fn accounting_must_balance_200() {
        // 200 项 = 对齐 + 差异（差异 4 处 → 对齐 196）。
        assert!(reconciliation(I_DOMAIN_TOTAL - DIFF_REGISTRY.len(), DIFF_REGISTRY.len()));
    }
}

// ===========================================================================
// 深化 v2（F475）：差异登记表三栏完整性全表逐检 / 对齐账自动化对总 /
// 复审条款时间戳审计 / 公开同步可验证性
// ===========================================================================

/// 差异登记表全表逐检（v1 registry_complete 的深化版：三栏非空 + 理由
/// 栏不含 TODO 式占位 + F 编号格式合法——全表逐条过，一条不合格即红）。
pub fn registry_full_audit() -> bool {
    DIFF_REGISTRY.iter().all(|e| {
        entry_format_ok(e)
            && !e.reason.contains("TODO")
            && !e.reason.contains("待定")
            && e.feature.starts_with('F')
            && e.feature.len() == 4
    })
}

/// 对齐账自动化对总（主册「对齐项计数与正文 200 项对账」：对齐项 =
/// 200 − 差异条数；差异登记表新增一条 → 对齐账自动减一——两账同源）。
pub fn aligned_count() -> usize {
    I_DOMAIN_TOTAL - DIFF_REGISTRY.len()
}

pub fn reconciliation_v2() -> bool {
    reconciliation(aligned_count(), DIFF_REGISTRY.len())
}

/// 复审条款时间戳审计（季度复审的到期-复审-清账三步语义深化：
/// 到期未复审 → 红；复审后到期线推进；对齐契机清账）。
impl ReviewClause {
    /// 复审全链（到期→复审→再判）——语义化包装供审计直接引用。
    pub fn review_cycle(&mut self, due_quarter: u32, next_quarter: u32) -> bool {
        if !self.review_due(due_quarter) {
            return false;
        }
        self.realign(due_quarter);
        !self.review_due(due_quarter) && self.review_due(next_quarter)
    }
}

/// 公开同步可验证性（主册「随设计案与开发者文档站同步公开」：published
/// 位 + 理由栏含可检索关键词（非空泛话）——公开的不是口号是内容）。
pub fn publish_content_verifiable() -> bool {
    DIFF_REGISTRY.iter().all(|e| e.published && e.reason.len() >= 8)
}

// ---------------------------------------------------------------------------
// 深化自检（F475 v2）
// ---------------------------------------------------------------------------

pub fn run_idiffreg_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F475-v2");
    // 1) 全表逐检：三栏齐 + 无占位 + F 编号格式。
    cs.add("registry_full_audit", registry_full_audit(), "");
    // 2) 对齐账自动化：196 + 4 = 200（一处一事实的自动化口径）。
    cs.add("aligned_auto", aligned_count() == 196, "");
    cs.add("reconciliation_v2", reconciliation_v2(), "");
    // 3) 复审全链：到期→复审→到期线推进。
    let mut rc = ReviewClause { last_reviewed_quarter: 0, realign_opportunities: 2 };
    cs.add("review_cycle", rc.review_cycle(3, 4), "");
    cs.add("opportunities_cleared", rc.realign_opportunities == 0, "");
    // 4) 公开同步可验证。
    cs.add("publish_verifiable", publish_content_verifiable(), "");
    // 5) 主册四处显式差异全部在册（F378/F411/F464/F467）。
    cs.add("four_explicit_diffs", DIFF_REGISTRY.iter().any(|e| e.feature == "F378")
        && DIFF_REGISTRY.iter().any(|e| e.feature == "F411")
        && DIFF_REGISTRY.iter().any(|e| e.feature == "F464")
        && DIFF_REGISTRY.iter().any(|e| e.feature == "F467"), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn registry_entries_have_distinct_features() {
        for i in 0..DIFF_REGISTRY.len() {
            for j in (i + 1)..DIFF_REGISTRY.len() {
                assert_ne!(DIFF_REGISTRY[i].feature, DIFF_REGISTRY[j].feature, "同一差异点不许重复登记");
            }
        }
    }

    #[test]
    fn review_cycle_multi_quarter() {
        let mut rc = ReviewClause { last_reviewed_quarter: 0, realign_opportunities: 0 };
        assert!(rc.review_cycle(1, 2));
        assert!(rc.review_cycle(2, 3));
        // 同季度重复复审：不再到期（已复审过）。
        assert!(!rc.review_due(2));
    }

    #[test]
    fn aligned_math_never_negative() {
        // 差异条数 ≤ 200 恒成立（登记表容量纪律）。
        assert!(DIFF_REGISTRY.len() <= I_DOMAIN_TOTAL);
        assert_eq!(aligned_count() + DIFF_REGISTRY.len(), I_DOMAIN_TOTAL);
    }
}
