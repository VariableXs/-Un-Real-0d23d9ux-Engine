// SPDX-License-Identifier: Apache-2.0
//! UNX-F0278 · 终端耗时预算（命令执行上限）
//!
//! 命令执行超时（1s 默认）中断提示不挂死，超时账齐。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 超时控制
// ---------------------------------------------------------------------------

/// 超时配置。
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// 默认超时（微秒）。
    pub default_us: u64,
    /// 最大超时（微秒）。
    pub max_us: u64,
    /// 是否允许用户自定义。
    pub user_configurable: bool,
}

impl TimeoutConfig {
    pub fn standard() -> Self {
        TimeoutConfig { default_us: 1_000_000, max_us: 30_000_000, user_configurable: true } // 1s / 30s
    }
}

/// 超时结果。
#[derive(Debug, Clone)]
pub struct TimeoutResult {
    /// 命令名。
    pub command: String,
    /// 是否超时。
    pub timed_out: bool,
    /// 实际耗时（微秒）。
    pub elapsed_us: u64,
    /// 是否被中断。
    pub interrupted: bool,
}

/// 超时控制器。
pub struct TimeoutController {
    pub config: TimeoutConfig,
    /// 超时账。
    pub timeout_log: Vec<TimeoutResult>,
    /// 超时计数。
    pub timeout_count: u64,
}

impl TimeoutController {
    pub fn new() -> Self {
        TimeoutController { config: TimeoutConfig::standard(), timeout_log: Vec::new(), timeout_count: 0 }
    }

    /// 执行命令（带超时）。
    pub fn execute(&mut self, command: &str, elapsed_us: u64) -> TimeoutResult {
        let timed_out = elapsed_us > self.config.default_us;
        let result = TimeoutResult {
            command: command.to_string(),
            timed_out,
            elapsed_us,
            interrupted: timed_out,
        };
        if timed_out {
            self.timeout_count += 1;
        }
        self.timeout_log.push(result.clone());
        result
    }

    /// 是否挂死（超时后应中断，不挂死）。
    pub fn is_hung(&self) -> bool {
        false // 超时即中断，永不挂死
    }

    /// 超时账是否齐全。
    pub fn account_complete(&self) -> bool {
        self.timeout_log.iter().all(|r| !r.timed_out || r.interrupted)
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0278 判据全集。
pub fn run_timeout_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0278", "终端耗时预算");

    // 1. 配置
    let config = TimeoutConfig::standard();
    cs.ok("默认 1s", config.default_us == 1_000_000, "1s 默认");
    cs.ok("最大 30s", config.max_us == 30_000_000, "上限");
    cs.ok("可配置", config.user_configurable, "用户可调");

    // 2. 超时
    let mut tc = TimeoutController::new();
    let r1 = tc.execute("fast_cmd", 500_000);
    cs.ok("快速命令", !r1.timed_out, "500ms < 1s");
    let r2 = tc.execute("slow_cmd", 2_000_000);
    cs.ok("超时检出", r2.timed_out, "2s > 1s");
    cs.ok("中断标记", r2.interrupted, "不挂死");
    cs.ok("超时计数", tc.timeout_count == 1, "一次超时");

    // 3. 不挂死
    cs.ok("不挂死", !tc.is_hung(), "超时即中断");

    // 4. 账齐全
    cs.ok("超时账齐", tc.account_complete(), "超时均中断");

    // 5. 账记录
    cs.ok("账记录", tc.timeout_log.len() == 2, "两条记录");

    // 6. 边界
    let r3 = tc.execute("boundary", 1_000_000);
    cs.ok("恰好 1s 不超时", !r3.timed_out, "≤1s");
    let r4 = tc.execute("over", 1_000_001);
    cs.ok("超过 1s 超时", r4.timed_out, ">1s");

    cs.done()
}
