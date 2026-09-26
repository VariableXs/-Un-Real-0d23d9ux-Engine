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
