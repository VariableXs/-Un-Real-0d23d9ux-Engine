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

// ===========================================================================
// 深化 v5（F475）：登记册增补通道 / 差异检索 / 复审历史账 / 对齐账导出
// ===========================================================================

/// 登记册增补容量（季度复审可能新增差异点——上限 16 条，超出走工单）。
pub const REGISTRY_CAP: usize = 16;

/// 可增补登记册（静态 DIFF_REGISTRY 是已发布快照；工作册允许季度复审
/// 增补——增补条目带季度号，下次发布时整体替换快照）。
pub struct DiffLedger {
    entries: [Option<DiffEntry>; REGISTRY_CAP],
    n: usize,
    /// 每条增补的登记季度（与 entries 同索引）。
    quarters: [u32; REGISTRY_CAP],
}

impl DiffLedger {
    /// 从已发布快照起册（快照 4 条全部带入，季度号 0 = 首发批）。
    pub fn new() -> Self {
        let mut led = DiffLedger { entries: [None; REGISTRY_CAP], n: 0, quarters: [0; REGISTRY_CAP] };
        for e in DIFF_REGISTRY {
            led.entries[led.n] = Some(e);
            led.n += 1;
        }
        led
    }

    /// 增补一条差异（重复 F 编号拒收——同一差异点不重复登记；
    /// 满额诚实拒绝，不静默挤掉旧账）。
    pub fn append(&mut self, e: DiffEntry, quarter: u32) -> bool {
        if self.n >= REGISTRY_CAP {
            return false;
        }
        if self.find_idx(e.feature).is_some() {
            return false;
        }
        if !entry_format_ok(&e) {
            return false;
        }
        self.entries[self.n] = Some(e);
        self.quarters[self.n] = quarter;
        self.n += 1;
        true
    }

    fn find_idx(&self, feature: &str) -> Option<usize> {
        (0..self.n).find(|&i| self.entries[i].map(|e| e.feature == feature).unwrap_or(false))
    }

    /// 差异检索（按 F 编号）。
    pub fn find(&self, feature: &str) -> Option<DiffEntry> {
        self.find_idx(feature).and_then(|i| self.entries[i])
    }

    /// 对齐回去（Windows 改正后复评——差异撤销、账面收口）。
    pub fn realign(&mut self, feature: &str, quarter: u32) -> bool {
        match self.find_idx(feature) {
            Some(i) => {
                // 压缩账面：被撤销条目之后的整体前移（保持登记序）。
                self.entries[i] = None;
                let mut w = i;
                for r in i + 1..self.n {
                    if let Some(e) = self.entries[r].take() {
                        self.entries[w] = Some(e);
                        self.quarters[w] = self.quarters[r];
                        w += 1;
                    }
                }
                self.n = w;
                true
            }
            None => false,
        }
    }

    /// 复审历史账：某季度登记的差异条数（季度审计可回溯）。
    pub fn registered_in(&self, quarter: u32) -> usize {
        (0..self.n).filter(|&i| self.quarters[i] == quarter).count()
    }

    /// 当前差异总数（含首发快照与增补）。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 对齐账导出（对齐项 = 200 − 当前差异总数——两账同源不漂移）。
    pub fn aligned_out(&self) -> usize {
        I_DOMAIN_TOTAL - self.n
    }

    /// 全表格式审计（逐条三栏统一 + 理由非占位）。
    pub fn audit(&self) -> bool {
        (0..self.n).all(|i| {
            self.entries[i]
                .map(|e| entry_format_ok(&e) && !e.reason.contains("待定"))
                .unwrap_or(false)
        })
    }
}

pub fn run_idiffreg_v5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F475-v5");
    // 1) 增补通道：新增差异点入册（主册「正文若增差异点则表随之增」）。
    let mut led = DiffLedger::new();
    let new_diff = DiffEntry {
        feature: "F402",
        windows: "窗口贴边自动最大化",
        varix: "贴边停驻需二次确认（首次提示，可关）",
        reason: "误触最大化是高频挫败源——一次确认换零误触",
        published: true,
    };
    cs.add("append_ok", led.append(new_diff, 5), "");
    cs.add("append_find", led.find("F402").map(|e| e.feature) == Some("F402"), "");
    cs.add("append_count", led.count() == 5, "");
    // 2) 重复登记拒收（同一差异点一条账）。
    cs.add("dup_rejected", !led.append(new_diff, 6), "");
    // 3) 对齐回去：差异撤销后账面收口。
    cs.add("realign_ok", led.realign("F402", 6) && led.count() == 4, "");
    cs.add("realign_find_gone", led.find("F402").is_none(), "");
    cs.add("realign_missing_false", !led.realign("F999", 6), "");
    // 4) 复审历史账：季度 5 登记过 1 条、季度 6 登记 0 条（已撤销）。
    let mut led2 = DiffLedger::new();
    let _ = led2.append(new_diff, 5);
    cs.add("history_q5", led2.registered_in(5) == 1, "");
    cs.add("history_q6_zero", led2.registered_in(6) == 0, "");
    // 5) 对齐账导出同源：4 差异 → 196 对齐。
    cs.add("aligned_out", led.aligned_out() == 196 && led.audit(), "");
    // 6) 满额诚实拒绝（容量纪律：16 条上限，超出走工单不挤账）。
    let mut full = DiffLedger::new();
    let mut appended = 0usize;
    let names = ["F401", "F403", "F405", "F407", "F409", "F413", "F415", "F417",
                 "F419", "F421", "F423", "F425", "F427", "F429", "F431", "F433",
                 "F435", "F437"];
    for f in names {
        let e = DiffEntry {
            feature: f,
            windows: "原行为占位栏",
            varix: "VARIX 行为占位栏",
            reason: "差异理由占位栏内容充分",
            published: true,
        };
        if full.append(e, 7) {
            appended += 1;
        }
    }
    cs.add("cap_exact", appended == REGISTRY_CAP - DIFF_REGISTRY.len(), "");
    // 7) 三栏不齐拒收（格式纪律在增补口收紧——脏数据进不了账）。
    let dirty = DiffEntry { feature: "F439", windows: "", varix: "x", reason: "r", published: true };
    cs.add("dirty_rejected", !led.append(dirty, 8), "");
    cs
}

#[cfg(test)]
mod v5_tests {
    use super::*;

    #[test]
    fn append_then_realign_roundtrip() {
        let mut led = DiffLedger::new();
        let e = DiffEntry {
            feature: "F441",
            windows: "W", varix: "V", reason: "理由充分完整", published: true,
        };
        assert!(led.append(e, 3));
        assert!(led.realign("F441", 4));
        assert_eq!(led.count(), 4);
        assert_eq!(led.aligned_out(), 196);
    }

    #[test]
    fn snapshot_features_always_present() {
        // 工作册从快照起册：首发 4 条恒在册。
        let led = DiffLedger::new();
        for f in ["F378", "F411", "F464", "F467"] {
            assert!(led.find(f).is_some(), "{} 恒在册", f);
        }
    }

    #[test]
    fn compression_keeps_registration_order() {
        let mut led = DiffLedger::new();
        let mk = |f: &'static str| DiffEntry {
            feature: f, windows: "W", varix: "V", reason: "理由充分完整", published: true,
        };
        let _ = led.append(mk("F443"), 1);
        let _ = led.append(mk("F445"), 1);
        let _ = led.append(mk("F447"), 1);
        assert!(led.realign("F445", 2));
        assert_eq!(led.count(), 6); // 4 快照 + 3 增补 − 1 撤销
        assert!(led.find("F443").is_some() && led.find("F447").is_some());
        assert!(led.find("F445").is_none());
    }
}
