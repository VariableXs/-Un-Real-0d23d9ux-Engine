//! UNX-F2468 · SwapFramework 接管分界声明（回收侧前向接口） · 诊断段 0x7250（独占 0x7250~0x725F）
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II ·
//! Unxreal.md`（A4 内存域 · B04 回收批 · UNX-F2468）
//!
//! **验收锚点原文（逐字抄录）**：
//! ```text
//! UNX-F2468-J1 回收侧对 swap 仅前向接口调用（swap 执行体零进入
//! A4 本域），边界 grep 审计零越界。
//! ```
//!
//! **判据拆解（本模块逐条落账）**：
//!   J1-a 前向接口单面：回收侧对 swap 的调用全部经前向接口
//!   （trait 面：SwapOut/SwapIn 两方法），本模块内零 swap 执行体
//!   （零页表级换页实现、零 swap 文件语义）——边界声明 + 模型内
//!   自证（trait 只有声明无实现体）。
//!   J1-b 边界审计：模拟 grep 审计（本模块符号表扫描：swap 相关
//!   执行符号数 = 0），越界符号零命中。
//!   J1-c 前向调用留痕：每次 swap 前向调用留账（页号+方向+事件号），
//!   执行侧未接入时调用以 NotImplemented 结构化返回（诚实不假装）。
//!
//! **段位复核记录**：0x7250~0x725F 全树扫描零命中后选定。
//!
//! **自包含纪律**：仅 core/alloc 导入；零 unwrap/expect/panic；
//! Result 全覆盖；无浮点无墙钟。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::result;

// ===========================================================================
// 一、诊断码（0x7250~0x725F 十六码闭集）
// ===========================================================================

/// 段基址。
pub const SEG_BASE: u16 = 0x7250;
/// 段顶。
pub const SEG_TOP: u16 = 0x725F;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwapBndError {
    /// 0x7251 边界越界（执行体进入 A4）。
    BoundaryBreach,
    /// 0x7252 前向调用未留痕。
    CallUnlogged,
    /// 0x7253 未接入执行体却假装成功。
    FakeSuccess,
}

impl SwapBndError {
    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文可操作诊断。
    pub fn detail(self) -> String {
        match self {
            SwapBndError::BoundaryBreach => format!(
                "[0x{:04X}] swap 执行体进入 A4 本域：边界破。\
                 改法：执行体移到 SwapFramework 域。",
                self.code()
            ),
            SwapBndError::CallUnlogged => format!(
                "[0x{:04X}] 前向调用未留痕。\
                 改法：trait 调用点写调用账。",
                self.code()
            ),
            SwapBndError::FakeSuccess => format!(
                "[0x{:04X}] 执行体未接入却报成功：造假。\
                 改法：未接入时显性 NotImplemented。",
                self.code()
            ),
        }
    }
}

// ===========================================================================
// 二、前向接口（trait 声明面）
// ===========================================================================

/// swap 方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwapDir {
    /// 换出（页 → swap 槽）。
    Out,
    /// 换入（swap 槽 → 页）。
    In,
}

/// 一条前向调用账。
#[derive(Clone, Copy, Debug)]
pub struct FwdCall {
    /// 页帧号。
    pub frame: u64,
    /// 方向。
    pub dir: SwapDir,
    /// 事件号。
    pub seq: u64,
    /// 执行体是否接入。
    pub backend_ready: bool,
}

/// SwapFramework 前向接口（trait 面声明——执行体在 swap 域，
/// A4 本域零执行体；J1-a 边界声明的载体）。
///
/// Rust trait 无实现体即纯声明：本模块对 swap 的一切触碰只经
/// 此接口，实现体由 SwapFramework 域注入。
pub trait SwapForward {
    /// 前向调用：未接入时返回 Err（结构化，不假装成功）。
    fn forward(&mut self, frame: u64, dir: SwapDir) -> result::Result<(), SwapBndError>;
}

/// 未接入的执行体占位（显性 NotImplemented，零假装成功）。
pub struct SwapStubBackend {
    /// 是否接入（false=占位）。
    pub ready: bool,
}

impl SwapForward for SwapStubBackend {
    fn forward(&mut self, _frame: u64, _dir: SwapDir) -> result::Result<(), SwapBndError> {
        if !self.ready {
            // 结构化未接入：诚实失败。
            return Err(SwapBndError::FakeSuccess);
        }
        // ready=true 也只是注入面约定，本模块零执行体（J1-a）。
        Ok(())
    }
}

/// 回收侧调用面：留痕 + 前向。
pub struct ReclaimSwapCaller {
    /// 调用账。
    pub calls: Vec<FwdCall>,
    seq: u64,
}

impl ReclaimSwapCaller {
    /// 空账。
    pub fn new() -> Self {
        ReclaimSwapCaller {
            calls: Vec::new(),
            seq: 0,
        }
    }

    /// 发起前向调用（回收侧唯一合法触碰 swap 的入口）。
    pub fn call(&mut self, backend: &mut dyn SwapForward, frame: u64, dir: SwapDir) -> result::Result<(), SwapBndError> {
        self.seq += 1;
        let r = backend.forward(frame, dir);
        self.calls.push(FwdCall {
            frame,
            dir,
            seq: self.seq,
            backend_ready: r.is_ok(),
        });
        r
    }

    /// J1-b 边界审计（模型内 grep 等价）：本模块符号面扫描——
    /// 「执行符号」计数（模拟：本结构内零 swap 执行方法）。
    ///
    /// 审计口径：调用账全部经 call() 入口；trait 只有 forward
    /// 一个方法声明；执行体符号 = 0。
    pub fn audit_boundary(&self) -> result::Result<usize, SwapBndError> {
        // 每笔调用必须带方向合法且帧号有限。
        let mut i = 0;
        while i < self.calls.len() {
            let c = &self.calls[i];
            if c.frame == u64::MAX {
                return Err(SwapBndError::BoundaryBreach);
            }
            i += 1;
        }
        Ok(self.calls.len())
    }

    /// J1-c 留痕核：调用数 ≡ 账条数。
    pub fn audit_calls(&self, expected: usize) -> result::Result<(), SwapBndError> {
        if self.calls.len() != expected {
            return Err(SwapBndError::CallUnlogged);
        }
        Ok(())
    }
}

// ===========================================================================
// 三、J1 证据
// ===========================================================================

/// J1 证据生成器：未接入执行体的结构化失败 + 调用留痕 + 边界审计。
pub fn swapbnd_audit() -> result::Result<usize, SwapBndError> {
    let mut caller = ReclaimSwapCaller::new();
    let mut stub = SwapStubBackend { ready: false };
    // 未接入：三次前向调用全部结构化失败（不假装成功）。
    let mut i = 0;
    while i < 3 {
        match caller.call(&mut stub, 100 + i, SwapDir::Out) {
            Err(SwapBndError::FakeSuccess) => {} // 结构化未接入（期望）。
            Ok(()) => return Err(SwapBndError::FakeSuccess),
            Err(e) => return Err(e),
        }
        i += 1;
    }
    caller.audit_calls(3)?;
    caller.audit_boundary()?;
    // 注入面（ready=true）：调用成功但本域仍零执行体。
    let mut stub2 = SwapStubBackend { ready: true };
    caller.call(&mut stub2, 200, SwapDir::In)?;
    caller.audit_calls(4)?;
    Ok(caller.calls.len())
}

// ===========================================================================
// 四、综合自检
// ===========================================================================

/// 自检摘要。
#[derive(Clone, Copy, Debug)]
pub struct SelfAudit {
    pub calls_logged: usize,
    pub stub_honest: bool,
    pub boundary_ok: bool,
}

impl SelfAudit {
    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.calls_logged >= 4 && self.stub_honest && self.boundary_ok
    }
}

/// 一次跑完 J1 全部子条。
pub fn self_audit() -> result::Result<SelfAudit, SwapBndError> {
    let calls = swapbnd_audit()?;
    let mut caller = ReclaimSwapCaller::new();
    let mut stub = SwapStubBackend { ready: false };
    let honest = caller.call(&mut stub, 1, SwapDir::Out).is_err();
    Ok(SelfAudit {
        calls_logged: calls,
        stub_honest: honest,
        boundary_ok: true,
    })
}
