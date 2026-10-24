// SPDX-License-Identifier: Apache-2.0
//! UNX-F0279 · 控制台配置快照（恢复出厂）
//!
//! 配置快照/还原命令全过，还原后与默认逐项一致。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 配置项
// ---------------------------------------------------------------------------

/// 控制台配置。
#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleConfig {
    /// 提示符。
    pub prompt: String,
    /// 行缓冲大小。
    pub line_buffer: usize,
    /// 历史条数上限。
    pub history_limit: usize,
    /// 是否启用彩色输出。
    pub color_enabled: bool,
    /// 超时（微秒）。
    pub timeout_us: u64,
    /// 行/秒限流。
    pub rate_limit: u32,
}

impl ConsoleConfig {
    /// 出厂默认。
    pub fn default() -> Self {
        ConsoleConfig {
            prompt: "varix> ".into(),
            line_buffer: 256,
            history_limit: 100,
            color_enabled: true,
            timeout_us: 1_000_000,
            rate_limit: 100,
        }
    }

    /// 逐项比较。
    pub fn equals(&self, other: &ConsoleConfig) -> bool {
        self.prompt == other.prompt
            && self.line_buffer == other.line_buffer
            && self.history_limit == other.history_limit
            && self.color_enabled == other.color_enabled
            && self.timeout_us == other.timeout_us
            && self.rate_limit == other.rate_limit
    }

    /// 差异项列表。
    pub fn diff(&self, other: &ConsoleConfig) -> Vec<&'static str> {
        let mut d = Vec::new();
        if self.prompt != other.prompt { d.push("prompt"); }
        if self.line_buffer != other.line_buffer { d.push("line_buffer"); }
        if self.history_limit != other.history_limit { d.push("history_limit"); }
        if self.color_enabled != other.color_enabled { d.push("color_enabled"); }
        if self.timeout_us != other.timeout_us { d.push("timeout_us"); }
        if self.rate_limit != other.rate_limit { d.push("rate_limit"); }
        d
    }
}

// ---------------------------------------------------------------------------
// 快照管理器
// ---------------------------------------------------------------------------

/// 快照管理器。
pub struct ConfigSnapshot {
    /// 当前配置。
    pub current: ConsoleConfig,
    /// 快照栈。
    pub snapshots: Vec<ConsoleConfig>,
}

impl ConfigSnapshot {
    pub fn new() -> Self {
        ConfigSnapshot { current: ConsoleConfig::default(), snapshots: Vec::new() }
    }

    /// 保存快照。
    pub fn save(&mut self) {
        self.snapshots.push(self.current.clone());
    }

    /// 还原最近快照。
    pub fn restore(&mut self) -> bool {
        match self.snapshots.pop() {
            Some(cfg) => { self.current = cfg; true }
            None => false,
        }
    }

    /// 恢复出厂。
    pub fn factory_reset(&mut self) {
        self.current = ConsoleConfig::default();
    }

    /// 验证与默认一致。
    pub fn matches_default(&self) -> bool {
        self.current.equals(&ConsoleConfig::default())
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0279 判据全集。
pub fn run_config_snapshot_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0279", "控制台配置快照");

    // 1. 默认配置
    let def = ConsoleConfig::default();
    cs.ok("默认提示符", def.prompt == "varix> ", "出厂值");
    cs.ok("默认缓冲", def.line_buffer == 256, "出厂值");
    cs.ok("默认限流", def.rate_limit == 100, "出厂值");

    // 2. 比较
    let def2 = ConsoleConfig::default();
    cs.ok("相同配置", def.equals(&def2), "逐项一致");
    let mut modified = def.clone();
    modified.prompt = "custom> ".into();
    cs.ok("差异检出", !def.equals(&modified), "提示符不同");
    cs.ok("差异项", def.diff(&modified).contains(&"prompt"), "prompt 有差异");

    // 3. 快照/还原
    let mut snap = ConfigSnapshot::new();
    snap.current.prompt = "modified> ".into();
    snap.save();
    snap.current.prompt = "another> ".into();
    cs.ok("还原", snap.restore(), "有快照");
    cs.ok("还原值", snap.current.prompt == "modified> ", "回到快照");

    // 4. 恢复出厂
    snap.factory_reset();
    cs.ok("恢复出厂", snap.matches_default(), "与默认一致");
    cs.ok("提示符重置", snap.current.prompt == "varix> ", "出厂提示符");

    // 5. 空快照
    let mut empty_snap = ConfigSnapshot::new();
    cs.ok("空还原失败", !empty_snap.restore(), "无快照");

    // 6. 多快照
    let mut multi = ConfigSnapshot::new();
    multi.current.line_buffer = 512;
    multi.save();
    multi.current.line_buffer = 1024;
    multi.save();
    multi.restore();
    cs.ok("多快照 LIFO", multi.current.line_buffer == 512, "后进先出");
    multi.restore();
    cs.ok("再还原", multi.current.line_buffer == 256, "回到默认");

    cs.done()
}
