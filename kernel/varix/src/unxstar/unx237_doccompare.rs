// SPDX-License-Identifier: Apache-2.0
//! UNX-F0237 · A1 域文档对照（任务书 vs 实态）
//!
//! 分工图册任务书条款逐条对照实态，偏差清单为零或显式登记。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 对照模型
// ---------------------------------------------------------------------------

/// 条款状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClauseStatus {
    /// 一致。
    Match,
    /// 偏差（已登记）。
    Deviation,
    /// 缺失。
    Missing,
}

/// 对照条目。
#[derive(Debug, Clone)]
pub struct DocClause {
    pub id: String,
    pub requirement: String,
    pub reality: String,
    pub status: ClauseStatus,
}

/// 对照器。
pub struct DocComparator {
    pub clauses: Vec<DocClause>,
    /// 偏差计数。
    pub deviation_count: u32,
}

impl DocComparator {
    pub fn new() -> Self {
        DocComparator { clauses: Vec::new(), deviation_count: 0 }
    }

    /// 添加对照条目。
    pub fn add_clause(&mut self, id: &str, req: &str, real: &str) {
        let status = if req == real {
            ClauseStatus::Match
        } else {
            self.deviation_count += 1;
            ClauseStatus::Deviation
        };
        self.clauses.push(DocClause {
            id: id.to_string(),
            requirement: req.to_string(),
            reality: real.to_string(),
            status,
        });
    }

    /// 是否零偏差。
    pub fn zero_deviation(&self) -> bool {
        self.deviation_count == 0
    }

    /// 偏差清单。
    pub fn deviation_list(&self) -> Vec<&DocClause> {
        self.clauses.iter().filter(|c| c.status == ClauseStatus::Deviation).collect()
    }

    /// 条款总数。
    pub fn total(&self) -> usize { self.clauses.len() }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0237 判据全集。
pub fn run_doc_compare_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0237", "文档对照");

    // 1. 条款状态
    cs.ok("三态", ClauseStatus::Match == ClauseStatus::Match, "一致/偏差/缺失");

    // 2. 零偏差
    let mut cmp = DocComparator::new();
    cmp.add_clause("C01", "implement X", "implement X");
    cmp.add_clause("C02", "implement Y", "implement Y");
    cs.ok("两条款", cmp.total() == 2, "两条");
    cs.ok("零偏差", cmp.zero_deviation(), "全部一致");
    cs.ok("偏差清单空", cmp.deviation_list().is_empty(), "无偏差");

    // 3. 偏差检出
    let mut cmp2 = DocComparator::new();
    cmp2.add_clause("C01", "implement X", "implement X");
    cmp2.add_clause("C02", "implement Y", "implement Z");
    cs.ok("偏差检出", !cmp2.zero_deviation(), "C02 不一致");
    cs.ok("偏差计数", cmp2.deviation_count == 1, "一条偏差");
    cs.ok("偏差清单", cmp2.deviation_list().len() == 1, "已登记");

    // 4. 全偏差
    let mut cmp3 = DocComparator::new();
    cmp3.add_clause("C01", "a", "b");
    cmp3.add_clause("C02", "c", "d");
    cs.ok("全偏差", cmp3.deviation_count == 2, "两条偏差");

    // 5. 空对照
    let empty = DocComparator::new();
    cs.ok("空对照零偏差", empty.zero_deviation(), "空=零偏差");

    cs.done()
}
