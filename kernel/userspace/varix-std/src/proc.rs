//! F057 · 进程生命周期包装：退出、等待、让出.

use crate::abi::{SYS_EXIT, SYS_WAIT, SYS_YIELD};
use crate::error::{invoke, Result};

/// F057 · 退出状态。布局按 Linux 约定，兼容层可以直通。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExitStatus {
    Exited(u8),
    Signaled { signal: u8, core: bool },
    Stopped(u8),
}

impl ExitStatus {
    pub const fn decode(raw: u32) -> ExitStatus {
        if raw & 0x7F == 0x7F && raw & 0xFF != 0 {
            ExitStatus::Stopped((raw >> 8) as u8)
        } else if raw & 0x7F != 0 {
            ExitStatus::Signaled {
                signal: (raw & 0x7F) as u8,
                core: raw & 0x80 != 0,
            }
        } else {
            ExitStatus::Exited((raw >> 8) as u8)
        }
    }

    pub const fn success(&self) -> bool {
        matches!(self, ExitStatus::Exited(0))
    }

    /// 结束该进程的信号编号（`None` 表示正常退出）。
    pub const fn terminating_signal(&self) -> Option<u8> {
        match self {
            ExitStatus::Signaled { signal, .. } => Some(*signal),
            ExitStatus::Stopped(s) => Some(*s),
            ExitStatus::Exited(_) => None,
        }
    }
}

/// F057 · `exit(code)` —— 不返回。
///
/// 内核保证这条调用不返回。`loop` 兜住"万一返回了"的内核 bug：用户态宁可
/// 空转，也不要在没有栈的情况下跑飞。
pub fn exit(code: u8) -> ! {
    unsafe {
        let _ = invoke(SYS_EXIT, [code as u64, 0, 0, 0, 0, 0]);
    }
    loop {
        core::hint::spin_loop();
    }
}

/// F057 · `wait(pid)`；`pid == 0` 表示"任意子进程"。无子进程时 `EChild`。
pub fn wait(pid: u32) -> Result<ExitStatus> {
    let raw = unsafe { invoke(SYS_WAIT, [pid as u64, 0, 0, 0, 0, 0])? };
    Ok(ExitStatus::decode(raw as u32))
}

/// F066 · 让出 CPU。它**不**在 vDSO 白名单里——它动运行队列，有副作用。
pub fn yield_now() {
    unsafe {
        let _ = invoke(SYS_YIELD, [0; 6]);
    }
}
