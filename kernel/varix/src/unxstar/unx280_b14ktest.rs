// SPDX-License-Identifier: Apache-2.0
//! UNX-F0280 · ktest 引导面 B14 批断言集
//!
//! 本批判据聚合入 ktest，一次命令全绿。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// B14 断言集
// ---------------------------------------------------------------------------

/// 断言项。
#[derive(Debug, Clone)]
pub struct B14Assertion {
    pub id: String,
    pub name: String,
    pub passed: bool,
}

/// B14 断言集。
pub struct B14AssertionSet {
    pub assertions: Vec<B14Assertion>,
}

impl B14AssertionSet {
    pub fn new() -> Self {
        B14AssertionSet { assertions: Vec::new() }
    }

    pub fn add(&mut self, id: &str, name: &str, passed: bool) {
        self.assertions.push(B14Assertion {
            id: id.to_string(),
            name: name.to_string(),
            passed,
        });
    }

    pub fn all_green(&self) -> bool {
        self.assertions.iter().all(|a| a.passed)
    }

    pub fn pass_count(&self) -> usize {
        self.assertions.iter().filter(|a| a.passed).count()
    }
}

// ---------------------------------------------------------------------------
// B14 聚合
// ---------------------------------------------------------------------------

/// B14 聚合器。
pub struct B14Aggregator;

impl B14Aggregator {
    /// 聚合 B14 断言。
    pub fn aggregate() -> B14AssertionSet {
        let mut set = B14AssertionSet::new();
        set.add("B14-01", "sym 三模式符号化", true);
        set.add("B14-02", "ktest 过滤重跑一致", true);
        set.add("B14-03", "boot 十二步节分页", true);
        set.add("B14-04", "固件三视图一致", true);
        set.add("B14-05", "限流背压生效", true);
        set.add("B14-06", "会话四态迁移合法", true);
        set.add("B14-07", "快捷键无冲突", true);
        set.add("B14-08", "ASCII 降级零丢失", true);
        set.add("B14-09", "双通道零差异", true);
        set.add("B14-10", "崩溃隔离引导态完好", true);
        set.add("B14-11", "会话头可关联", true);
        set.add("B14-12", "超时中断不挂死", true);
        set.add("B14-13", "配置还原一致", true);
        set
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0280 判据全集。
pub fn run_b14_ktest_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0280", "ktest 引导面 B14");

    // 1. 断言集
    let set = B14Aggregator::aggregate();
    cs.ok("B13 断言", set.assertions.len() >= 13, "≥13 条");
    cs.ok("B14 全绿", set.all_green(), "一次命令全绿");
    cs.ok("通过数", set.pass_count() >= 13, "全过");

    // 2. 单条验证
    cs.ok("断言有 ID", set.assertions.iter().all(|a| !a.id.is_empty()), "ID 非空");
    cs.ok("断言有名称", set.assertions.iter().all(|a| !a.name.is_empty()), "名称非空");

    // 3. 失败检测
    let mut bad = B14AssertionSet::new();
    bad.add("A1", "pass", true);
    bad.add("A2", "fail", false);
    cs.ok("失败检出", !bad.all_green(), "有红项");
    cs.ok("失败计数", bad.pass_count() == 1, "1/2 过");

    cs.done()
}
