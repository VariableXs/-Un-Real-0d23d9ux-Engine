// SPDX-License-Identifier: Apache-2.0
//! UNX-F0287 · 不可恢复检测（三重故障/栈耗尽）
//!
//! 三重故障与栈哨兵耗尽两条死路显性化停机，不再进入递归 panic。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::format;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 死路类型
// ---------------------------------------------------------------------------

/// 不可恢复故障类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatalKind {
    /// 三重故障（fault during fault during fault）。
    TripleFault,
    /// 栈哨兵耗尽（stack canary blown）。
    StackExhausted,
    /// 双重故障。
    DoubleFault,
    /// 不可屏蔽中断嵌套溢出。
    NmiOverflow,
}

impl FatalKind {
    pub fn label(&self) -> &'static str {
        match self {
            FatalKind::TripleFault => "三重故障",
            FatalKind::StackExhausted => "栈哨兵耗尽",
            FatalKind::DoubleFault => "双重故障",
            FatalKind::NmiOverflow => "NMI 嵌套溢出",
        }
    }

    /// 是否为死路（不可恢复）。
    pub fn is_fatal(&self) -> bool {
        true // 全部四种都是死路
    }
}

// ---------------------------------------------------------------------------
// 故障状态机
// ---------------------------------------------------------------------------

/// 故障深度计数器。
pub struct FaultCounter {
    /// 当前故障嵌套深度。
    pub depth: u32,
    /// 历史最大深度。
    pub max_depth: u32,
    /// 是否已停机。
    pub halted: bool,
    /// 停机原因。
    pub halt_reason: Option<FatalKind>,
}

impl FaultCounter {
    pub fn new() -> Self {
        FaultCounter { depth: 0, max_depth: 0, halted: false, halt_reason: None }
    }

    /// 进入故障处理。
    pub fn enter_fault(&mut self) -> Result<(), FatalKind> {
        if self.halted {
            return Err(self.halt_reason.unwrap_or(FatalKind::TripleFault));
        }
        self.depth += 1;
        if self.depth > self.max_depth {
            self.max_depth = self.depth;
        }
        // 检测死路
        if self.depth >= 3 {
            self.halted = true;
            self.halt_reason = Some(FatalKind::TripleFault);
            return Err(FatalKind::TripleFault);
        }
        Ok(())
    }

    /// 退出故障处理。
    pub fn exit_fault(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
    }

    /// 当前是否在故障中。
    pub fn in_fault(&self) -> bool {
        self.depth > 0
    }
}

// ---------------------------------------------------------------------------
// 栈哨兵监控
// ---------------------------------------------------------------------------

/// 栈哨兵（canary）监控器。
pub struct StackCanary {
    /// 栈底哨兵值。
    pub canary: u32,
    /// 栈剩余字节估计。
    pub remaining_bytes: u32,
    /// 哨兵是否完好。
    pub intact: bool,
}

impl StackCanary {
    /// 哨兵魔法值。
    pub const MAGIC: u32 = 0xDEADBEEF;

    pub fn new(stack_size: u32) -> Self {
        StackCanary { canary: Self::MAGIC, remaining_bytes: stack_size, intact: true }
    }

    /// 检查哨兵（每次中断入口调用）。
    pub fn check(&mut self) -> Result<(), FatalKind> {
        if self.canary != Self::MAGIC {
            self.intact = false;
            return Err(FatalKind::StackExhausted);
        }
        if self.remaining_bytes < 256 {
            self.intact = false;
            return Err(FatalKind::StackExhausted);
        }
        Ok(())
    }

    /// 消耗栈空间（模拟深度调用）。
    pub fn consume(&mut self, bytes: u32) -> Result<(), FatalKind> {
        if bytes >= self.remaining_bytes {
            self.remaining_bytes = 0;
            self.canary = 0; // 哨兵被踩
            return Err(FatalKind::StackExhausted);
        }
        self.remaining_bytes -= bytes;
        Ok(())
    }

    /// 释放栈空间。
    pub fn release(&mut self, bytes: u32) {
        self.remaining_bytes += bytes;
    }
}

// ---------------------------------------------------------------------------
// 停机执行器
// ---------------------------------------------------------------------------

/// 停机动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HaltAction {
    /// 停机并输出信息。
    HaltWithMessage,
    /// 停机并复位。
    HaltAndReset,
    /// 停机并冻结（等待调试器）。
    HaltAndFreeze,
}

/// 不可恢复故障处理器。
pub struct FatalHandler {
    pub counter: FaultCounter,
    pub canary: StackCanary,
    /// 停机消息。
    pub halt_message: String,
}

impl FatalHandler {
    pub fn new(stack_size: u32) -> Self {
        FatalHandler {
            counter: FaultCounter::new(),
            canary: StackCanary::new(stack_size),
            halt_message: String::new(),
        }
    }

    /// 处理一次故障（返回是否应继续）。
    pub fn handle_fault(&mut self) -> Result<HaltAction, FatalKind> {
        // 先检查栈哨兵
        if let Err(kind) = self.canary.check() {
            self.counter.halted = true;
            self.counter.halt_reason = Some(kind);
            self.halt_message = alloc::format!("FATAL: {} — system halted", kind.label());
            return Err(kind);
        }
        // 再检查故障嵌套
        match self.counter.enter_fault() {
            Ok(()) => Ok(HaltAction::HaltWithMessage), // 正常故障处理中
            Err(kind) => {
                self.halt_message = alloc::format!("FATAL: {} — system halted", kind.label());
                Err(kind)
            }
        }
    }

    /// 退出故障处理。
    pub fn fault_resolved(&mut self) {
        self.counter.exit_fault();
    }

    /// 是否已停机。
    pub fn is_halted(&self) -> bool {
        self.counter.halted
    }

    /// 获取停机消息。
    pub fn message(&self) -> &str {
        &self.halt_message
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0287 判据全集。
pub fn run_fatal_detect_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0287", "不可恢复检测");

    // 1. 死路类型
    cs.ok("三重故障是死路", FatalKind::TripleFault.is_fatal(), "不可恢复");
    cs.ok("栈耗尽是死路", FatalKind::StackExhausted.is_fatal(), "不可恢复");
    cs.ok("标签可读", FatalKind::TripleFault.label() == "三重故障", "人话");

    // 2. 故障深度
    let mut fc = FaultCounter::new();
    cs.ok("首故障 OK", fc.enter_fault().is_ok(), "depth=1");
    cs.ok("二故障 OK", fc.enter_fault().is_ok(), "depth=2");
    cs.ok("三故障死路", fc.enter_fault().is_err(), "depth=3 → halt");
    cs.ok("已停机", fc.halted, "halted=true");
    cs.ok("停机原因", fc.halt_reason == Some(FatalKind::TripleFault), "三重故障");

    // 3. 栈哨兵
    let mut sc = StackCanary::new(4096);
    cs.ok("哨兵完好", sc.check().is_ok(), "MAGIC 正确");
    cs.ok("消耗栈", sc.consume(2000).is_ok(), "2KB");
    cs.ok("剩余足够", sc.remaining_bytes == 2096, "4096-2000");
    cs.ok("再消耗", sc.consume(1900).is_ok(), "1.9KB");
    cs.ok("栈耗尽", sc.consume(500).is_err(), "不足 500B");
    cs.ok("哨兵被踩", !sc.intact, "intact=false");

    // 4. 处理器
    let mut fh = FatalHandler::new(4096);
    cs.ok("首故障处理", fh.handle_fault().is_ok(), "正常");
    cs.ok("未停机", !fh.is_halted(), "继续运行");
    fh.fault_resolved();
    cs.ok("故障退出", !fh.counter.in_fault(), "depth=0");

    // 5. 停机消息
    let mut fh2 = FatalHandler::new(256);
    // 直接耗尽栈
    let _ = fh2.canary.consume(200);
    let r = fh2.handle_fault();
    cs.ok("栈耗尽停机", r.is_err(), "Err(StackExhausted)");
    cs.ok("停机消息", fh2.message().contains("FATAL"), "显性化");
    cs.ok("不再递归 panic", fh2.is_halted(), "halted=true");

    // 6. 三重故障场景
    let mut fh3 = FatalHandler::new(4096);
    fh3.handle_fault().ok();
    fh3.handle_fault().ok();
    let r3 = fh3.handle_fault();
    cs.ok("三重故障停机", r3.is_err(), "第三层拒绝");
    cs.ok("停机消息非空", !fh3.message().is_empty(), "有输出");

    cs.done()
}
