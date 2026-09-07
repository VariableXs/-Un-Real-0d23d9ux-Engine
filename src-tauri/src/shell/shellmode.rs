//! D-1（22.1）：VM 档「登录即 Variable」——Shell 模式支持。
//!
//! Winlogon 的 Shell 值指向 Variable.exe 时：
//! 1. 启动时探测 Shell 模式（GetShellWindow 的归属进程 != explorer.exe）；
//! 2. 兜底拉起 explorer 服务进程（能力层可用：文件对话框 / 右键菜单 / 拖拽集成），
//!    并隐藏其窗口层（任务栏/桌面窗口）；登记 pid 供零残留退出时一并回收；
//! 3. 锁屏复用 winman::power_action("lock")（LockWorkStation 已有）；
//! 4. 安全模式兜底：修复脚本见 portable/AI3/Set-VmShell.ps1 -Restore（离线改回 explorer）。
//!
//! 零残留：explorer 服务进程随 Variable 退出被终结（Shell 模式下本就是 VM 会话）。

#![cfg(windows)]

use std::collections::HashSet;
use std::sync::Mutex;

static EXPLORER_PIDS: Mutex<Option<HashSet<u32>>> = Mutex::new(None);

/// Shell 模式判定：系统 Shell 窗口（Progman/WorkerW 等由 explorer 持有）
/// 的归属进程不是 explorer.exe = Variable 本身作为 Shell 运行。
pub fn is_shell_mode() -> bool {
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};
    unsafe {
        let hwnd = GetShellWindow();
        if hwnd.is_invalid() {
            // 无 Shell 窗口：启动早期（登录瞬间）——按命令行兜底（父进程 = userinit/winlogon）
            return std::env::var("SESSIONNAME").is_ok()
                && std::env::var("VAR_RUNTIME_MODE").as_deref() == Ok("vm");
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return false;
        }
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if !ok.is_ok() {
            return false;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
        !path.ends_with("\\explorer.exe")
    }
}

/// 拉起 explorer 服务进程并隐藏其窗口层（幂等：已拉起则跳过）。
pub fn ensure_explorer_service() {
    let mut guard = EXPLORER_PIDS.lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().is_some_and(|s| !s.is_empty()) {
        return;
    }
    let Ok(child) = std::process::Command::new("explorer.exe").spawn() else {
        return;
    };
    let pid = child.id();
    guard.get_or_insert_with(HashSet::new).insert(pid);
    // explorer 是先 fork 一个新进程再退出父进程——真正的 shell 进程 pid 需扫描：
    // 延迟 1.5s 枚举所有 explorer.exe 并全部隐藏（Shell 模式下用户无需其窗口层）
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        hide_explorer_windows();
    });
}

/// 隐藏 explorer 的窗口层（任务栏 / 桌面窗口）；能力层（COM/Shell API）不受影响。
pub fn hide_explorer_windows() {
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, ShowWindow, SW_HIDE,
    };
    unsafe {
        // 任务栏（含次级显示器任务栏）与桌面图标层
        for class in ["Shell_TrayWnd", "Shell_SecondaryTrayWnd", "Progman", "WorkerW"] {
            let class_w: Vec<u16> = class.encode_utf16().chain([0]).collect();
            let mut hwnd = FindWindowExW(None, None, windows::core::PCWSTR(class_w.as_ptr()), None);
            while let Ok(h) = hwnd {
                if h.is_invalid() {
                    break;
                }
                let _ = ShowWindow(h, SW_HIDE);
                hwnd = FindWindowExW(None, h, windows::core::PCWSTR(class_w.as_ptr()), None);
            }
        }
    }
}

/// 零残留退出钩子：终结我们拉起的 explorer 服务进程
pub fn cleanup_explorer_service() {
    let guard = EXPLORER_PIDS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pids) = guard.as_ref() {
        for pid in pids {
            if let Ok(handle) = unsafe {
                windows::Win32::System::Threading::OpenProcess(
                    windows::Win32::System::Threading::PROCESS_TERMINATE,
                    false,
                    *pid,
                )
            } {
                unsafe {
                    let _ = windows::Win32::System::Threading::TerminateProcess(handle, 0);
                    let _ = windows::Win32::Foundation::CloseHandle(handle);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-1 自检：普通模式（explorer 为 Shell）必须判定为非 Shell 模式
    #[test]
    fn not_shell_mode_under_explorer() {
        // CI/测试机均为普通桌面会话
        assert!(!is_shell_mode());
    }
}
