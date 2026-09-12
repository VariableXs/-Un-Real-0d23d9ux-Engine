//! F071 · varix-std —— 无 libc 的 Varix 用户态基础库（雏形）.
//!
//! 目标：`x86_64-unknown-none`，`no_std`，不含任何分配器，不含任何 libc。
//! 本库只做三件事：
//!
//! 1. **入口胶水**（[`entry`]）：把 6 个参数放进 ABI 寄存器并执行 `syscall`。
//!    内联汇编是唯一的机器相关代码，集中在一处。
//! 2. **返回通道解码**（[`error`]）：非负即成功，`[-4095,-1]` 即错误。
//! 3. **类型化包装**（[`io`] / [`mem`] / [`time`] / [`proc`]）：让调用方写
//!    `write(FD_STDOUT, b"hi")`，而不是手搓调用号和寄存器。
//!
//! 与内核的一致性由内核侧自检反向断言（`kernel/varix/src/syscall/userlib.rs`）：
//! 号表里每个已实现调用都必须在这里有包装（或在内核侧登记的例外清单里），
//! 本库每个包装绑定的号也必须在内核号表里存在。两边漂移会让内核自检变红。

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

pub mod abi;
pub mod error;
pub mod event;
pub mod io;
pub mod mem;
pub mod proc;
pub mod time;

pub use abi::*;
pub use error::{invoke, Error, Result};

/// F051 · 用户态入口桩。所有包装最终都走到这里。
///
/// `syscall` 约定：号在 RAX，参数在 RDI/RSI/RDX/R10/R8/R9，返回在 RAX；
/// `RCX` 与 `R11` 被硬件用作暂存（用户 RIP/RFLAGS），声明为 lateout 让编译器
/// 知道它们被破坏了——这正是 `int 0x80` 备路（F052）与之唯一的可见差别。
///
/// # Safety
/// 调用方必须保证 `nr` 是内核号表里存在的调用，且 `args` 对该调用语义合法。
#[inline(always)]
pub unsafe fn syscall0_raw(nr: u64, args: [u64; 6]) -> u64 {
    let ret: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") nr => ret,
            in("rdi") args[0],
            in("rsi") args[1],
            in("rdx") args[2],
            in("r10") args[3],
            in("r8")  args[4],
            in("r9")  args[5],
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags),
        );
    }
    ret
}

/// F052 · 备路：经 `int 0x80` 门进入内核。
///
/// 语义与 [`syscall0_raw`] 完全一致（内核侧自检断言两条路解码出同一个
/// 调用帧）；差别只在 `RCX`/`R11` 不被破坏——调试器和降级引导需要这条路径。
///
/// # Safety
/// 同 [`syscall0_raw`]。
#[inline(always)]
pub unsafe fn int80_raw(nr: u64, args: [u64; 6]) -> u64 {
    let ret: u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inlateout("rax") nr => ret,
            in("rdi") args[0],
            in("rsi") args[1],
            in("rdx") args[2],
            in("r10") args[3],
            in("r8")  args[4],
            in("r9")  args[5],
            options(nostack),
        );
    }
    ret
}

/// F069 · 探测内核 ABI。程序启动后第一件事就该做这个：拿到版本与能力位，
/// 再决定哪些调用可用，而不是先调一次再猜为什么 `ENOSYS`。
pub fn abi() -> Result<AbiInfo> {
    // 入参：请求的 ABI 版本、本进程指针宽度。两值都由内核核对。
    let raw = unsafe { invoke(SYS_ABI, [ABI_VERSION as u64, 8, 0, 0, 0, 0])? };
    Ok(AbiInfo::decode(raw))
}

/// `SYS_ABI` 的返回布局：单个 u64 打包（版本 16 位 / 能力位 32 位 / 页大小 16 位）。
/// 内核在 `syscall::table::abi_info` 里按同一布局打包，用户态只读不猜。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AbiInfo {
    pub version: u32,
    pub features: u64,
}

impl AbiInfo {
    pub const fn decode(raw: u64) -> AbiInfo {
        AbiInfo {
            version: (raw & 0xFFFF) as u32,
            features: (raw >> 16) & 0xFFFF_FFFF,
        }
    }

    pub const fn has(&self, bits: u64) -> bool {
        self.features & bits == bits
    }
}

/// F071 · 调试输出（`SYS_LOG`），仅在调试构建的镜像里可用。
pub fn debug_log(msg: &[u8]) -> Result<usize> {
    let n = unsafe { invoke(SYS_LOG, [2, msg.as_ptr() as u64, msg.len() as u64, 0, 0, 0])? };
    Ok(n as usize)
}
