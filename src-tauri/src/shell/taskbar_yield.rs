//! L3 shell — taskbar_yield.rs：Windows 任务栏智能让位检测。
//!
//! 背景：Variable 桌面 fullscreen + alwaysOnTop 时，Steam 等 CEF 应用获得前台
//! 会把 Windows 任务栏（Shell_TrayWnd）顶到 Variable 全屏窗口上——与 Variable
//! 自身底栏重叠。本模块 1s 轮询任务栏可见性/矩形，与桌面窗口求交，
//! 状态变化推事件 `sys://taskbar-yield` { visible, height }，前端底栏上移让位。
//!
//! 自动隐藏任务栏语义天然覆盖：隐藏时不可见/矩形在屏外 → 不让位；
//! 悬停/前台切换浮现时矩形侵入屏幕 → 让位。
//! 仅 Windows 有真实行为；其余平台恒 visible=false。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::CmdResult;

#[derive(Serialize, Clone, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TaskbarYieldStatus {
    /// Windows 任务栏正浮在桌面窗口上（前端应让位）
    pub visible: bool,
    /// 任务栏侵入桌面窗口底边的高度（物理 px，让位距离）
    pub height: u32,
}

/// 纯判定：两矩形相交返回纵向重叠高度，否则 0。
pub fn overlap_height(tray: (i32, i32, i32, i32), desk: (i32, i32, i32, i32)) -> u32 {
    let ix1 = tray.0.max(desk.0);
    let iy1 = tray.1.max(desk.1);
    let ix2 = tray.2.min(desk.2);
    let iy2 = tray.3.min(desk.3);
    if ix1 < ix2 && iy1 < iy2 {
        (iy2 - iy1) as u32
    } else {
        0
    }
}

#[cfg(windows)]
fn current_status(app: &AppHandle) -> TaskbarYieldStatus {
    use tauri::Manager;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowRect, IsWindowVisible,
    };

    let Some(win) = app.get_webview_window("desktop") else {
        return TaskbarYieldStatus::default();
    };
    // 前台仍是桌面窗口时不让位（fullscreen 独占下任务栏本就隐藏）
    if win.is_focused().unwrap_or(false) {
        return TaskbarYieldStatus::default();
    }
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return TaskbarYieldStatus::default();
    };
    let desk = (
        pos.x,
        pos.y,
        pos.x + size.width as i32,
        pos.y + size.height as i32,
    );

    unsafe {
        let mut cls: Vec<u16> = "Shell_TrayWnd".encode_utf16().collect();
        cls.push(0);
        let Ok(hwnd): Result<HWND, _> = FindWindowW(PCWSTR(cls.as_ptr()), None) else {
            return TaskbarYieldStatus::default();
        };
        if !IsWindowVisible(hwnd).as_bool() {
            return TaskbarYieldStatus::default();
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return TaskbarYieldStatus::default();
        }
        let h = overlap_height(
            (rect.left, rect.top, rect.right, rect.bottom),
            desk,
        );
        TaskbarYieldStatus {
            visible: h > 0,
            height: h,
        }
    }
}

#[cfg(not(windows))]
fn current_status(_app: &AppHandle) -> TaskbarYieldStatus {
    TaskbarYieldStatus::default()
}

/// 手动查询（诊断用；运行期状态由 watcher 推送）。
#[tauri::command(async)]
pub fn taskbar_yield_check(app: AppHandle) -> CmdResult<TaskbarYieldStatus> {
    Ok(current_status(&app))
}

/// 1s 轮询 watcher：状态变化才推送（height 变化也推——让位距离跟随 DPI/位置）。
pub fn spawn_taskbar_yield_watcher(app: AppHandle) {
    std::thread::Builder::new()
        .name("taskbar-yield".into())
        .spawn(move || {
            let mut last = current_status(&app);
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let cur = current_status(&app);
                if cur != last {
                    last = cur.clone();
                    let _ = app.emit("sys://taskbar-yield", &cur);
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_height_bottom_taskbar() {
        // 任务栏贴底：桌面 1920x1080，任务栏 (0,1040,1920,1080) → 侵入 40
        assert_eq!(overlap_height((0, 1040, 1920, 1080), (0, 0, 1920, 1080)), 40);
    }

    #[test]
    fn overlap_height_disjoint_is_zero() {
        assert_eq!(overlap_height((0, 1040, 1920, 1080), (0, 0, 1920, 1039)), 0);
        assert_eq!(overlap_height((-1920, 0, -100, 1080), (0, 0, 1920, 1080)), 0);
    }

    #[test]
    fn overlap_height_taskbar_above_desktop_is_zero() {
        // 顶置任务栏在桌面外之上 → 不相交
        assert_eq!(overlap_height((0, -48, 1920, 0), (0, 0, 1920, 1080)), 0);
    }
}
