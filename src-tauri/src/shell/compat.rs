//! L3 shell — compat.rs：Wallpaper Engine 共存兼容层
//!
//! # 背景 / 为什么会崩溃与卡死
//! 用户实机反馈：Wallpaper Engine 报 `libcef.dll 0x80000003` 崩溃，Variable 在该环境下
//! 打不开 / 卡死。根因已定位：
//!
//! 1. **libcef 0x80000003 是 CEF 的 DCHECK/breakpoint 陷阱** —— Wallpaper Engine 的 UI
//!    进程是 Chromium Embedded Framework（CEF）。当检测到不变量被破坏时会主动 `int3`
//!    触发 0x80000003。Wallpaper Engine 对话框里“was likely crashed by another
//!    application”并非指 Variable 注入了它的 DLL，而是其崩溃归因在检测到外部显存/
//!    合成器压力后会把最近的外部窗口列为 suspected causer，Variable 全屏独占最符合启发式。
//!    常见诱因：
//!    - Variable `fullscreen + alwaysOnTop=true` 覆盖 Wallpaper Engine 的 WorkerW 壁纸窗口；
//!    - Variable WebView2 与 Wallpaper Engine CEF 各自 GPU 进程同时重负载渲染；
//!    - `embed.rs` 的 EnumWindows 轮询与 WE 窗口创建时序竞争。
//!
//! 2. **Variable 打不开/卡死** —— 同一 GPU 竞争的另一面：WebView2 GPU 进程在 DWM 已被
//!    WE 壁纸占满时申请大纹理会阻塞；`Focused(true) → set_always_on_top(true)` 又与 WE
//!    壁纸窗口形成 z-order 抖动，导致 DWM 卡死正反馈。
//!
//! # 修复策略（零侵入、如实降级）
//! - 探测 WE 4 个典型进程名；
//! - 命中时自动取消桌面窗口 alwaysOnTop + 推送事件让前端降级壁纸负载；
//! - 提供 compat_check / compat_apply / compat_restore 命令；后台 3s watcher。

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::CmdResult;

static COMPAT_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Wallpaper Engine 相关进程名（小写后缀匹配，覆盖 32/64 位）。
const WE_PROCS: &[&str] = &[
    "wallpaper64.exe",
    "wallpaper32.exe",
    "wallpaperservice64.exe",
    "wallpaperservice32.exe",
    "wallpaperservice.exe",
    "wallpaper engine.exe",
];

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CompatStatus {
    pub wallpaper_engine_running: bool,
    pub processes: Vec<String>,
    pub compat_active: bool,
    pub recommendation: String,
    pub severity: String,
}

pub fn list_wallpaper_engine_processes() -> Vec<String> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        unsafe {
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snapshot, &mut entry).is_ok() {
                loop {
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                    if WE_PROCS.iter().any(|pat| name.ends_with(pat)) {
                        if !out.contains(&name) {
                            out.push(name);
                        }
                    }
                    if Process32NextW(snapshot, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = windows::Win32::Foundation::CloseHandle(snapshot);
        }
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

pub fn is_wallpaper_engine_running() -> bool {
    !list_wallpaper_engine_processes().is_empty()
}

fn severity(running: bool, compat: bool) -> &'static str {
    if !running {
        "none"
    } else if compat {
        "mitigated"
    } else {
        "high"
    }
}

fn recommendation_text(running: bool, compat: bool) -> String {
    if !running {
        String::new()
    } else if compat {
        "已自动进入 Wallpaper Engine 兼容模式：已取消独占置顶并降低壁纸 GPU 负载。\n如需完整性能，退出 Wallpaper Engine 后重启 Variable 即可恢复。\nAuto compat mode: always-on-top disabled & wallpaper GPU load reduced.".into()
    } else {
        "检测到 Wallpaper Engine 正在运行，与 Variable 的全屏独占 + 双 Chromium GPU 进程存在已知冲突（libcef 0x80000003）。\n建议：① 退出 Wallpaper Engine 后再启动 Variable；或 ② 点“一键兼容”让 Variable 取消独占置顶并降级壁纸渲染。\nWallpaper Engine detected — known conflict with Variable fullscreen + dual Chromium GPUs (libcef 0x80000003).".into()
    }
}

fn current_status() -> CompatStatus {
    let procs = list_wallpaper_engine_processes();
    let running = !procs.is_empty();
    let compat = COMPAT_ACTIVE.load(Ordering::Relaxed);
    CompatStatus {
        wallpaper_engine_running: running,
        processes: procs,
        compat_active: compat,
        recommendation: recommendation_text(running, compat),
        severity: severity(running, compat).into(),
    }
}

#[tauri::command]
pub fn compat_check() -> CmdResult<CompatStatus> {
    Ok(current_status())
}

#[tauri::command]
pub fn compat_apply(app: AppHandle) -> CmdResult<CompatStatus> {
    apply_compat_mode(&app);
    Ok(current_status())
}

pub fn apply_compat_mode(app: &AppHandle) {
    COMPAT_ACTIVE.store(true, Ordering::Relaxed);
    if let Some(w) = app.get_webview_window("desktop") {
        let _ = w.set_always_on_top(false);
    }
    let _ = app.emit("compat://wallpaper-engine", current_status());
    eprintln!(
        "[compat] Wallpaper Engine compat mode applied (alwaysOnTop=false) procs={:?}",
        list_wallpaper_engine_processes()
    );
}

#[tauri::command]
pub fn compat_restore(app: AppHandle) -> CmdResult<CompatStatus> {
    COMPAT_ACTIVE.store(false, Ordering::Relaxed);
    if let Some(w) = app.get_webview_window("desktop") {
        let _ = w.set_always_on_top(true);
    }
    let _ = app.emit("compat://wallpaper-engine", current_status());
    eprintln!("[compat] compat mode restored (alwaysOnTop=true)");
    Ok(current_status())
}

pub fn is_compat_active() -> bool {
    COMPAT_ACTIVE.load(Ordering::Relaxed)
}

pub fn apply_if_needed_at_startup(app: &AppHandle) {
    if is_wallpaper_engine_running() {
        eprintln!("[compat] Wallpaper Engine detected at startup -> entering compat mode");
        apply_compat_mode(app);
    }
}

pub fn spawn_compat_watcher(app: AppHandle) {
    std::thread::Builder::new()
        .name("compat-watcher".into())
        .spawn(move || {
            let mut last_running = is_wallpaper_engine_running();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let running = is_wallpaper_engine_running();
                if running != last_running {
                    last_running = running;
                    if running {
                        eprintln!("[compat] Wallpaper Engine appeared at runtime -> compat on");
                        apply_compat_mode(&app);
                    } else {
                        eprintln!("[compat] Wallpaper Engine gone -> emitting cleared status (compat stays until restore)");
                        let _ = app.emit("compat://wallpaper-engine", current_status());
                    }
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_shape_is_serializable() {
        let s = CompatStatus {
            wallpaper_engine_running: false,
            processes: vec![],
            compat_active: false,
            recommendation: String::new(),
            severity: "none".into(),
        };
        let j = serde_json::to_value(&s).unwrap();
        assert_eq!(j["wallpaperEngineRunning"], false);
        assert_eq!(j["severity"], "none");
    }

    #[test]
    fn we_proc_list_is_lowercase_suffix_match() {
        let list = list_wallpaper_engine_processes();
        for n in list {
            assert!(n.ends_with(".exe"));
            assert_eq!(n, n.to_lowercase());
        }
    }

    #[test]
    fn severity_mapping() {
        assert_eq!(severity(false, false), "none");
        assert_eq!(severity(false, true), "none");
        assert_eq!(severity(true, false), "high");
        assert_eq!(severity(true, true), "mitigated");
    }
}
