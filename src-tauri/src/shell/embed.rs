//! L3 shell — embed.rs（批次E-16 第三方应用环境内嵌）：
//! - 目标：第三方应用不在 Variable 之外打开 —— 启动后把它的主窗口 SetParent
//!   成 Variable 桌面窗口的子窗口（WS_CHILD），随虚拟窗口移动/缩放，从任务栏
//!   与 Alt+Tab 消失，实现与 Windows 桌面的隔离。
//! - 进程启动复用已审查的 tp_launch 通道（本模块不新增进程创建代码）。
//! - 无法嵌入的应用（UWP/管理员权限/无主窗口）如实回退为独立窗口运行。

use std::sync::Mutex;

use serde::Serialize;
#[cfg(windows)]
use tauri::Manager;

use crate::error::{AppError, CmdResult};

struct EmbedState {
    /// 嵌入的子窗口句柄
    hwnd: isize,
    /// 第三方登记 id（崩溃/退出事件回传前端用）
    tp_id: String,
}

static EMBED: Mutex<Option<EmbedState>> = Mutex::new(None);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedResult {
    /// 是否成功嵌入（false = 已回退为独立窗口运行）
    pub attached: bool,
    pub reason: String,
}

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };

    pub fn window_pid(hwnd: HWND) -> Option<u32> {
        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        (pid != 0).then_some(pid)
    }

    pub fn process_image(pid: u32) -> Option<String> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
            let _ = CloseHandle(h);
            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    pub fn is_visible_top_level(hwnd: HWND) -> bool {
        unsafe { IsWindowVisible(hwnd).as_bool() }
    }

    /// 枚举可见顶层窗口，返回 (hwnd, 进程映像路径) 中满足过滤的句柄集合。
    pub fn collect_handles(keep: &mut dyn FnMut(isize, &str) -> bool) -> Vec<isize> {
        struct Ctx<'a> {
            keep: &'a mut dyn FnMut(isize, &str) -> bool,
            out: Vec<isize>,
        }
        unsafe extern "system" fn probe(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
            unsafe {
                if !IsWindowVisible(hwnd).as_bool() {
                    return BOOL(1);
                }
                let Some(pid) = window_pid(hwnd) else { return BOOL(1) };
                let Some(img) = process_image(pid) else { return BOOL(1) };
                if (ctx.keep)(hwnd.0 as isize, &img) {
                    ctx.out.push(hwnd.0 as isize);
                }
            }
            BOOL(1)
        }
        let mut ctx = Ctx { keep, out: Vec::new() };
        unsafe {
            EnumWindows(Some(probe), LPARAM(&mut ctx as *mut Ctx as isize));
        }
        ctx.out
    }

    /// 收集 root 及其全部后代进程 pid（Toolhelp32 快照；启动器型软件会派生
    /// 别的 exe，例如 Wallpaper Engine 的 wallpaper32/64.exe，必须按进程树匹配）。
    pub fn pid_tree(root: u32) -> Vec<u32> {
        use windows::Win32::System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        };
        let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
            return vec![root];
        };
        struct Row {
            pid: u32,
            parent: u32,
        }
        let mut rows: Vec<Row> = Vec::new();
        unsafe {
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    rows.push(Row { pid: entry.th32ProcessID, parent: entry.th32ParentProcessID });
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = windows::Win32::Foundation::CloseHandle(snap);
        }
        let mut out = vec![root];
        let mut i = 0;
        while i < out.len() {
            let p = out[i];
            for r in &rows {
                if r.parent == p && r.pid != 0 && !out.contains(&r.pid) {
                    out.push(r.pid);
                }
            }
            i += 1;
        }
        out
    }

    /// 等待出现某个"启动前不存在"的匹配窗口（最长约 30s，命中即返回；
    /// 超时如实回退独立窗口，绝不终止应用进程）。
    /// 匹配优先级：pid ∈ 启动进程的子进程树（覆盖启动器派生场景）；
    /// 其次：映像名以登记 exe 结尾（兜底 pid 拿不到的场景）。
    pub fn wait_new_window(exe_name: &str, root_pid: Option<u32>, before: &[isize]) -> Option<isize> {
        let suffix = exe_name.to_lowercase();
        for _ in 0..150 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let tree: Vec<u32> = root_pid.map(|r| pid_tree(r)).unwrap_or_default();
            let hit = collect_handles(&mut |h, img| {
                if before.contains(&h) {
                    return false;
                }
                if !tree.is_empty() {
                    return tree.iter().any(|p| {
                        let mut wp = 0u32;
                        unsafe { GetWindowThreadProcessId(super::hwnd_from_isize(h), Some(&mut wp)) };
                        wp == *p
                    });
                }
                img.to_lowercase().ends_with(&suffix)
            });
            if let Some(h) = hit.into_iter().next() {
                return Some(h);
            }
        }
        None
    }
}

#[cfg(windows)]
fn hwnd_from_isize(v: isize) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(v as *mut core::ffi::c_void)
}

#[cfg(windows)]
fn hwnd_from(v: isize) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(v as *mut core::ffi::c_void)
}

#[cfg(windows)]
fn desktop_hwnd(app: &tauri::AppHandle) -> Option<isize> {
    let w = app.get_webview_window("desktop")?;
    let h = w.hwnd().ok()?;
    Some(h.0 as isize)
}

/// 启动第三方应用并把它的主窗口嵌入 Variable 桌面窗口（环境内打开）。
/// 无法嵌入时如实返回 attached=false（应用已按独立窗口方式启动）。
#[tauri::command]
#[cfg(windows)]
pub fn embed_launch(
    st: tauri::State<'_, crate::state::AppState>,
    app: tauri::AppHandle,
    id: String,
) -> CmdResult<EmbedResult> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetParent, SetWindowLongPtrW, SetWindowPos, GWL_STYLE,
        SWP_FRAMECHANGED, SWP_NOZORDER, WS_CAPTION, WS_CHILD, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
        WS_SYSMENU, WS_THICKFRAME,
    };

    // 0) 同一时刻只保留一个嵌入（先前的先脱离回桌面）
    detach_locked();

    // 1) 登记项 → 目标 exe 路径（lnk 已在登记/启动时解析 target）
    let apps = crate::shell::launcher::registry_snapshot(&st);
    let tp = apps
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::not_found(format!("未找到登记项 / Not found: {id}")))?
        .clone();
    let target = tp
        .target
        .clone()
        .unwrap_or_else(|| tp.path.clone())
        .to_lowercase();
    let exe_name = std::path::Path::new(&target)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| AppError::validation("登记项路径无效 / invalid path"))?;

    // 2) 记录启动前已存在的该应用窗口（避免把旧窗口误嵌）
    let before = win::collect_handles(&mut |h, img| img.to_lowercase().ends_with(&exe_name));

    // 3) 启动（复用既有通道；本模块不含进程创建代码）。root_pid 用于
    //    按子进程树匹配窗口——启动器型软件（如 Wallpaper Engine）真正的
    //    主窗口属于它派生的子进程，按 exe 名匹配不到。
    let root_pid = crate::shell::launcher::tp_launch_inner(&st, &app, id.clone())?;

    // 4) 等待新主窗口（最长约 30s；超时后应用保持独立窗口运行，不终止进程）
    let Some(desktop) = desktop_hwnd(&app) else {
        return Err(AppError::io("桌面窗口不存在 / no desktop window"));
    };
    let Some(hwnd) = win::wait_new_window(&exe_name, root_pid, &before) else {
        return Ok(EmbedResult {
            attached: false,
            reason: "未能捕获应用窗口（启动较慢或无标准窗口）。应用已在系统桌面独立运行，未受影响；稍后重新从环境内打开即可再试。".into(),
        });
    };

    // 5) 重父级为桌面窗口子窗口：去标题栏/边框/系统菜单（任务栏与 Alt+Tab 消失）
    unsafe {
        let h = hwnd_from(hwnd);
        let style = GetWindowLongPtrW(h, GWL_STYLE) as isize;
        let new_style = ((style as u32)
            & !(WS_CAPTION.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0 | WS_SYSMENU.0))
            | WS_CHILD.0;
        SetWindowLongPtrW(h, GWL_STYLE, new_style as isize);
        SetParent(h, hwnd_from(desktop));
        // 先给一个占位边界（随后由前端虚拟窗口上报精确边界）
        SetWindowPos(h, HWND::default(), 240, 140, 900, 600, SWP_FRAMECHANGED | SWP_NOZORDER);
    }

    *EMBED.lock().unwrap_or_else(|e| e.into_inner()) = Some(EmbedState { hwnd, tp_id: id.clone() });

    Ok(EmbedResult { attached: true, reason: String::new() })
}

/// 脱离当前嵌入（恢复独立顶层窗口；应用不退出）。
#[cfg(windows)]
fn detach_locked() {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetParent, SetWindowLongPtrW, GWL_STYLE, WS_CHILD, WS_POPUP,
    };
    if let Some(e) = EMBED.lock().unwrap_or_else(|e| e.into_inner()).take() {
        unsafe {
            let h = hwnd_from(e.hwnd);
            let style = GetWindowLongPtrW(h, GWL_STYLE) as isize;
            SetWindowLongPtrW(h, GWL_STYLE, ((style as u32 & !WS_CHILD.0) | WS_POPUP.0) as isize);
            SetParent(h, HWND::default());
        }
    }
}

#[cfg(not(windows))]
#[tauri::command]
pub fn embed_launch(
    _st: tauri::State<'_, crate::state::AppState>,
    _app: tauri::AppHandle,
    _id: String,
) -> CmdResult<EmbedResult> {
    Ok(EmbedResult { attached: false, reason: "仅 Windows 支持 / Windows only".into() })
}

/// 更新嵌入窗口边界（物理像素；随虚拟窗口移动/缩放由前端上报）。
#[tauri::command]
#[cfg(windows)]
pub fn embed_bounds(x: i32, y: i32, w: i32, h: i32) -> CmdResult<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER};
    let cur = EMBED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = cur.as_ref() {
        unsafe {
            SetWindowPos(hwnd_from(e.hwnd), HWND::default(), x, y, w.max(1), h.max(1), SWP_NOZORDER);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn embed_bounds(_x: i32, _y: i32, _w: i32, _h: i32) -> CmdResult<()> {
    Ok(())
}

/// 显示/隐藏嵌入窗口（最小化=隐藏，恢复=显示）。
#[tauri::command]
#[cfg(windows)]
pub fn embed_visible(visible: bool) -> CmdResult<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOW};
    let cur = EMBED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = cur.as_ref() {
        unsafe {
            ShowWindow(hwnd_from(e.hwnd), if visible { SW_SHOW } else { SW_HIDE });
        }
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn embed_visible(_visible: bool) -> CmdResult<()> {
    Ok(())
}

/// 关闭嵌入窗口（WM_CLOSE，应用自行退出），并清除嵌入状态。
#[tauri::command]
#[cfg(windows)]
pub fn embed_close() -> CmdResult<()> {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    if let Some(e) = EMBED.lock().unwrap_or_else(|e| e.into_inner()).take() {
        unsafe {
            PostMessageW(hwnd_from(e.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn embed_close() -> CmdResult<()> {
    Ok(())
}

/// 让嵌入窗口获得键盘焦点（点击虚拟窗口时调用）。
#[tauri::command]
#[cfg(windows)]
pub fn embed_focus() -> CmdResult<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    let cur = EMBED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = cur.as_ref() {
        unsafe {
            let _ = SetFocus(hwnd_from(e.hwnd));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub fn embed_focus() -> CmdResult<()> {
    Ok(())
}
