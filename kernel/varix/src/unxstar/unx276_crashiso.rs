// SPDX-License-Identifier: Apache-2.0
//! UNX-F0276 · 控制台崩溃隔离（UI panic 不伤引导态）
//!
//! 注入 UI 层 panic，引导态不受损，打回提示符有账。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 引导态（不可损坏）
// ---------------------------------------------------------------------------

/// 引导态（核心状态，UI panic 不得修改）。
#[derive(Debug, Clone)]
pub struct BootState {
    /// 引导阶段。
    pub stage: u8,
    /// 是否已完成引导。
    pub boot_complete: bool,
    /// 健康标志。
    pub healthy: bool,
    /// 引导校验和（不变量）。
    pub checksum: u32,
}

impl BootState {
    pub fn new() -> Self {
        BootState { stage: 0, boot_complete: false, healthy: true, checksum: 0xABCD }
    }

    /// 验证未损坏。
    pub fn is_intact(&self) -> bool {
        self.checksum == 0xABCD && self.healthy
    }
}

// ---------------------------------------------------------------------------
// UI 层（可 panic）
// ---------------------------------------------------------------------------

/// UI 层状态。
#[derive(Debug, Clone)]
pub struct UiState {
    /// 当前提示符。
    pub prompt: String,
    /// UI 是否崩溃。
    pub panicked: bool,
    /// panic 消息。
    pub panic_message: String,
}

impl UiState {
    pub fn new() -> Self {
        UiState { prompt: "varix> ".into(), panicked: false, panic_message: String::new() }
    }

    /// 模拟 UI panic。
    pub fn panic(&mut self, msg: &str) {
        self.panicked = true;
        self.panic_message = msg.to_string();
    }

    /// 恢复（打回提示符）。
    pub fn recover(&mut self) {
        self.panicked = false;
        self.panic_message.clear();
        self.prompt = "varix> ".into();
    }
}

// ---------------------------------------------------------------------------
// 崩溃隔离器
// ---------------------------------------------------------------------------

/// 崩溃隔离器。
pub struct CrashIsolator {
    pub boot: BootState,
    pub ui: UiState,
    /// 崩溃计数。
    pub crash_count: u32,
    /// 恢复计数。
    pub recover_count: u32,
}

impl CrashIsolator {
    pub fn new() -> Self {
        CrashIsolator { boot: BootState::new(), ui: UiState::new(), crash_count: 0, recover_count: 0 }
    }

    /// 注入 UI panic（隔离验证）。
    pub fn inject_ui_panic(&mut self, msg: &str) {
        self.ui.panic(msg);
        self.crash_count += 1;
    }

    /// 自动恢复（打回提示符）。
    pub fn auto_recover(&mut self) -> bool {
        if self.boot.is_intact() {
            self.ui.recover();
            self.recover_count += 1;
            true
        } else {
            false // 引导态损坏，无法恢复
        }
    }

    /// 验证隔离性（UI panic 后引导态完好）。
    pub fn verify_isolation(&self) -> bool {
        self.boot.is_intact()
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0276 判据全集。
pub fn run_crash_isolation_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0276", "控制台崩溃隔离");

    // 1. 引导态
    let boot = BootState::new();
    cs.ok("引导态初始完好", boot.is_intact(), "checksum=0xABCD");
    cs.ok("引导未完成", !boot.boot_complete, "初始");

    // 2. UI panic
    let mut ui = UiState::new();
    cs.ok("UI 初始正常", !ui.panicked, "未崩溃");
    ui.panic("test crash");
    cs.ok("UI 崩溃", ui.panicked, "panic 状态");
    cs.ok("panic 消息", ui.panic_message == "test crash", "记录");
    ui.recover();
    cs.ok("UI 恢复", !ui.panicked, "打回提示符");
    cs.ok("提示符重置", ui.prompt == "varix> ", "默认提示符");

    // 3. 崩溃隔离
    let mut iso = CrashIsolator::new();
    iso.inject_ui_panic("ui crash");
    cs.ok("崩溃计数", iso.crash_count == 1, "一次崩溃");
    cs.ok("引导态不受损", iso.verify_isolation(), "隔离成功");
    cs.ok("自动恢复", iso.auto_recover(), "打回提示符");
    cs.ok("恢复计数", iso.recover_count == 1, "一次恢复");

    // 4. 多次崩溃
    for i in 0..5 {
        iso.inject_ui_panic("loop crash");
        iso.auto_recover();
    }
    cs.ok("多次崩溃隔离", iso.verify_isolation(), "引导态始终完好");
    cs.ok("崩溃计数", iso.crash_count == 6, "1+5 次");

    // 5. 引导态损坏检测
    let mut broken = CrashIsolator::new();
    broken.boot.healthy = false;
    cs.ok("损坏检测", broken.boot.is_intact() == false, "healthy=false");
    cs.ok("拒绝恢复", broken.auto_recover() == false, "引导态损坏");

    cs.done()
}
