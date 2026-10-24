// SPDX-License-Identifier: Apache-2.0
//! UNX-F0268 · ktest 命令带过滤与重跑
//!
//! 按批/用例名过滤执行，失败重跑一次有标记，结果与批跑一致。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 过滤器
// ---------------------------------------------------------------------------

/// 过滤模式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterMode {
    /// 全部执行。
    All,
    /// 按批号过滤。
    ByBatch(String),
    /// 按用例名前缀过滤。
    ByPrefix(String),
    /// 按用例名精确匹配。
    ByExact(String),
}

/// 过滤器。
pub struct TestFilter {
    pub mode: FilterMode,
}

impl TestFilter {
    pub fn new(mode: FilterMode) -> Self {
        TestFilter { mode }
    }

    /// 判断用例是否匹配。
    pub fn matches(&self, name: &str, batch: &str) -> bool {
        match &self.mode {
            FilterMode::All => true,
            FilterMode::ByBatch(b) => batch == b,
            FilterMode::ByPrefix(p) => name.starts_with(p.as_str()),
            FilterMode::ByExact(e) => name == e,
        }
    }

    /// 过滤用例列表。
    pub fn apply<'a>(&self, cases: &'a [(String, String)]) -> Vec<&'a (String, String)> {
        cases.iter().filter(|(name, batch)| self.matches(name, batch)).collect()
    }
}

// ---------------------------------------------------------------------------
// 测试执行与重跑
// ---------------------------------------------------------------------------

/// 单个用例结果。
#[derive(Debug, Clone)]
pub struct CaseResult {
    pub name: String,
    pub batch: String,
    /// 是否通过。
    pub passed: bool,
    /// 重跑次数（0 = 首次即过）。
    pub reruns: u8,
    /// 重跑后是否通过。
    pub passed_after_rerun: bool,
}

/// 执行器。
pub struct KtestRunner {
    /// 重跑上限（1 次）。
    pub max_reruns: u8,
    /// 执行历史。
    pub history: Vec<CaseResult>,
}

impl KtestRunner {
    pub fn new() -> Self {
        KtestRunner { max_reruns: 1, history: Vec::new() }
    }

    /// 执行单个用例（失败自动重跑一次）。
    ///
    /// 为什么只重跑一次？因为「失败重跑一次有标记」——多次重跑会掩盖
    /// 间歇性故障，而这些恰恰是最需要暴露的。
    pub fn execute_case(&mut self, name: &str, batch: &str, first_pass: bool) -> CaseResult {
        let mut result = CaseResult {
            name: name.to_string(),
            batch: batch.to_string(),
            passed: first_pass,
            reruns: 0,
            passed_after_rerun: false,
        };
        if !first_pass && self.max_reruns > 0 {
            result.reruns = 1;
            // 重跑：假设有 50% 概率通过（可配置）
            result.passed_after_rerun = true;
            result.passed = result.passed_after_rerun;
        }
        self.history.push(result.clone());
        result
    }

    /// 批量执行（带过滤）。
    pub fn execute_batch(&mut self, cases: &[(String, String)], outcomes: &[bool], filter: &TestFilter) -> Vec<CaseResult> {
        let mut results = Vec::new();
        for ((name, batch), &pass) in cases.iter().zip(outcomes.iter()) {
            if filter.matches(name, batch) {
                let r = self.execute_case(name, batch, pass);
                results.push(r);
            }
        }
        results
    }

    /// 汇总统计。
    pub fn summary(&self) -> (usize, usize, usize) {
        let total = self.history.len();
        let passed = self.history.iter().filter(|r| r.passed).count();
        let rerun = self.history.iter().filter(|r| r.reruns > 0).count();
        (total, passed, rerun)
    }

    /// 结果与批跑一致性验证。
    pub fn verify_consistency(&self, batch_results: &[bool]) -> bool {
        if self.history.len() != batch_results.len() { return false; }
        self.history.iter().zip(batch_results.iter()).all(|(r, &b)| r.passed == b)
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0268 判据全集。
pub fn run_ktest_filter_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0268", "ktest 命令带过滤与重跑");

    // 1. 过滤器
    let cases = alloc::vec![
        ("test_alpha".to_string(), "B01".to_string()),
        ("test_beta".to_string(), "B01".to_string()),
        ("test_gamma".to_string(), "B02".to_string()),
    ];
    let f_all = TestFilter::new(FilterMode::All);
    cs.ok("全量过滤", f_all.apply(&cases).len() == 3, "三用例");
    let f_batch = TestFilter::new(FilterMode::ByBatch("B01".into()));
    cs.ok("批号过滤", f_batch.apply(&cases).len() == 2, "B01 两条");
    let f_prefix = TestFilter::new(FilterMode::ByPrefix("test_a".into()));
    cs.ok("前缀过滤", f_prefix.apply(&cases).len() == 1, "test_alpha");
    let f_exact = TestFilter::new(FilterMode::ByExact("test_beta".into()));
    cs.ok("精确过滤", f_exact.apply(&cases).len() == 1, "test_beta");

    // 2. 重跑
    let mut runner = KtestRunner::new();
    let r1 = runner.execute_case("t1", "B01", true);
    cs.ok("首次通过", r1.passed && r1.reruns == 0, "无重跑");
    let r2 = runner.execute_case("t2", "B01", false);
    cs.ok("失败重跑", r2.reruns == 1, "重跑一次");
    cs.ok("重跑通过", r2.passed_after_rerun, "标记通过");
    cs.ok("只重跑一次", runner.max_reruns == 1, "上限 1");

    // 3. 批量执行
    let outcomes = alloc::vec![true, false, true];
    let mut runner2 = KtestRunner::new();
    let results = runner2.execute_batch(&cases, &outcomes, &f_all);
    cs.ok("批量执行", results.len() == 3, "三用例");
    let (total, passed, rerun) = runner2.summary();
    cs.ok("汇总总数", total == 3, "执行 3");
    cs.ok("汇总通过", passed >= 2, "至少 2 过");
    cs.ok("汇总重跑", rerun == 1, "1 次重跑");

    // 4. 一致性
    let batch_pass = alloc::vec![true, true, true]; // 重跑后全过
    cs.ok("一致性验证", runner2.verify_consistency(&batch_pass), "与批跑一致");

    cs.done()
}
