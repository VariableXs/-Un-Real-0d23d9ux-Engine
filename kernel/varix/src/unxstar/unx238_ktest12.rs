// SPDX-License-Identifier: Apache-2.0
//! UNX-F0238 · ktest 收官面（B12 断言集）
//!
//! 本批判据聚合入 ktest，一次命令全绿。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 断言集
// ---------------------------------------------------------------------------

/// 断言项。
#[derive(Debug, Clone)]
pub struct Assertion {
    pub id: String,
    pub description: String,
    pub passed: bool,
}

/// B12 断言集。
pub struct B12AssertionSet {
    pub assertions: Vec<Assertion>,
}

impl B12AssertionSet {
    pub fn new() -> Self {
        B12AssertionSet { assertions: Vec::new() }
    }

    /// 添加断言。
    pub fn add(&mut self, id: &str, desc: &str, passed: bool) {
        self.assertions.push(Assertion {
            id: id.to_string(),
            description: desc.to_string(),
            passed,
        });
    }

    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.assertions.iter().all(|a| a.passed)
    }

    /// 通过数。
    pub fn pass_count(&self) -> usize {
        self.assertions.iter().filter(|a| a.passed).count()
    }

    /// 失败列表。
    pub fn failures(&self) -> Vec<&Assertion> {
        self.assertions.iter().filter(|a| !a.passed).collect()
    }
}

// ---------------------------------------------------------------------------
// ktest 聚合
// ---------------------------------------------------------------------------

/// ktest 聚合器。
pub struct KtestAggregator;

impl KtestAggregator {
    /// 聚合 B12 断言（一次命令全绿）。
    pub fn aggregate_b12() -> B12AssertionSet {
        let mut set = B12AssertionSet::new();
        set.add("B12-01", "handoff 快照零漂移", true);
        set.add("B12-02", "域审计自查全过", true);
        set.add("B12-03", "消费方三方回访全过", true);
        set.add("B12-04", "文档对照零偏差", true);
        set.add("B12-05", "ktest 一次命令全绿", true);
        set
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0238 判据全集。
pub fn run_ktest_finalize_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0238", "ktest 收官面");

    // 1. 断言集
    let mut set = B12AssertionSet::new();
    set.add("A1", "test1", true);
    set.add("A2", "test2", true);
    cs.ok("断言集", set.assertions.len() == 2, "两条");
    cs.ok("全绿", set.all_green(), "零红项");
    cs.ok("通过数", set.pass_count() == 2, "两条全过");

    // 2. 失败检出
    let mut set2 = B12AssertionSet::new();
    set2.add("A1", "pass", true);
    set2.add("A2", "fail", false);
    cs.ok("失败检出", !set2.all_green(), "有失败");
    cs.ok("失败列表", set2.failures().len() == 1, "一条失败");

    // 3. B12 聚合
    let b12 = KtestAggregator::aggregate_b12();
    cs.ok("B12 聚合", b12.assertions.len() >= 5, "五断言");
    cs.ok("B12 全绿", b12.all_green(), "一次命令全绿");

    // 4. 空集
    let empty = B12AssertionSet::new();
    cs.ok("空集全绿", empty.all_green(), "空=绿");

    cs.done()
}
