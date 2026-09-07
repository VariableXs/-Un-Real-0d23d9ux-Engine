//! S-1 防截屏模式（隐私盾）
//! - 机制：对 Variable 全部顶层窗口（桌面 + 独立软件窗口 + 系统窗口）调用
//!   `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`，系统截图/录屏 API
//!   （Win+Shift+S、OBS 窗口捕获等）不可见；嵌入的第三方窗口（L2 容器宿主，
//!   W-1 起多嵌入并发）同步打标。
//! - 边界如实声明：仅防 Win32 截屏/截窗 API；物理拍摄、内核级捕获、宿主侧
//!   键盘记录无法防御（设置页 UI 双声明）。
//! - VM 档：VMConnect 窗口属宿主进程树之外，宿主侧截屏打标由宿主负责；VM 内
//!   截屏不受影响 —— 双层语义分开声明，此处不做跨进程越权操作。
//! - 回滚：纯开关功能，关闭即对所有窗口恢复 WDA_NONE（立即恢复可截）。

use serde::Serialize;

use crate::error::CmdResult;

static SHIELD_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_on() -> bool {
    SHIELD_ON.load(std::sync::atomic::Ordering::Relaxed)
}

/// 隐私感知日志（X-4 插件 HostApi.log 等使用）：防截屏开启时静默，
/// 避免插件消息落进宿主控制台（截图会带上的泄漏面）。
pub fn privacy_shield_log(msg: &str) {
    if is_on() {
        return;
    }
    eprintln!("[log] {msg}");
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShieldStatus {
    pub on: bool,
    /// 本次成功打标的窗口数（0 = 开启但暂无可打标窗口，看护线程会补打）
    pub tagged: usize,
}

#[cfg(windows)]
mod imp {
    use super::{ShieldStatus, SHIELD_ON};
    use std::sync::OnceLock;
    use tauri::{AppHandle, Manager};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    };

    static APP: OnceLock<AppHandle> = OnceLock::new();

    /// 对单个 hwnd 打标；返回是否成功。
    fn tag_hwnd(hwnd: HWND) -> bool {
        unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_ok() }
    }

    /// 全部 Variable 顶层窗口 + 嵌入的第三方窗口（tauri 与本 crate 的 windows
    /// crate 版本不同，HWND 一律按裸指针跨类型构造）。
    fn all_hwnds(app: &AppHandle) -> Vec<HWND> {
        let mut out = Vec::new();
        for (_, w) in app.webview_windows() {
            if let Ok(h) = w.hwnd() {
                out.push(HWND(h.0));
            }
        }
        for h in crate::shell::embed::current_embed_hwnds() {
            out.push(HWND(h as *mut core::ffi::c_void));
        }
        out
    }

    fn tag_all(app: &AppHandle) -> usize {
        all_hwnds(app).into_iter().filter(|h| tag_hwnd(*h)).count()
    }

    /// 关闭恢复：WDA_NONE 立即恢复可截。
    fn untag_all(app: &AppHandle) {
        for h in all_hwnds(app) {
            unsafe {
                let _ = SetWindowDisplayAffinity(h, WDA_NONE);
            }
        }
    }

    /// 看护线程：开启期间每 2s 补打新出现的窗口（新建窗口/新嵌入）。
    pub fn spawn_watcher(app: AppHandle) {
        let _ = APP.set(app);
        std::thread::spawn(|| loop {
            if SHIELD_ON.load(std::sync::atomic::Ordering::Relaxed) {
                if let Some(app) = APP.get() {
                    tag_all(app);
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        });
    }

    pub fn set(on: bool) -> ShieldStatus {
        SHIELD_ON.store(on, std::sync::atomic::Ordering::Relaxed);
        let Some(app) = APP.get() else {
            return ShieldStatus { on, tagged: 0 };
        };
        if on {
            ShieldStatus { on, tagged: tag_all(app) }
        } else {
            untag_all(app);
            ShieldStatus { on, tagged: 0 }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::ShieldStatus;
    use tauri::AppHandle;

    pub fn spawn_watcher(_app: AppHandle) {}
    pub fn set(on: bool) -> ShieldStatus {
        ShieldStatus { on, tagged: 0 }
    }
}

/// 开关防截屏模式（设置页 / 托盘菜单共用）。关闭即恢复可截（纯开关，回滚=关）。
#[tauri::command]
pub fn shield_set(on: bool) -> CmdResult<ShieldStatus> {
    Ok(imp::set(on))
}

#[tauri::command]
pub fn shield_get() -> CmdResult<bool> {
    Ok(is_on())
}

pub fn spawn_watcher(app: tauri::AppHandle) {
    imp::spawn_watcher(app);
}
