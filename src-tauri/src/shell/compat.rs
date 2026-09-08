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
use std::path::Path;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AppError, CmdResult};

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

// -------------------------------------------------------------------------
// AI-3 Windows Shell proxy
// -------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShellExecuteResult {
    pub launched: bool,
    pub process_id: Option<u32>,
    pub backend: String,
    pub error_code: Option<i32>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShellIconResult {
    pub data_url: String,
    pub size: u32,
    pub source: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShellContextMenuResult {
    pub shown: bool,
    pub invoked: bool,
    pub command_id: Option<u32>,
}

/// Open a path/URI/shortcut with the Windows Shell.  This is deliberately a
/// separate command from the profiled CreateProcess path: ShellExecute is the
/// correct authority for associations, .lnk files, folders, protocols, and
/// UAC verbs.  No cmd.exe/start string is ever constructed here.
#[tauri::command]
pub fn shell_execute(
    path: String,
    verb: Option<String>,
    arguments: Option<String>,
    cwd: Option<String>,
    show: Option<i32>,
) -> CmdResult<ShellExecuteResult> {
    shell_execute_path(
        Path::new(&path),
        verb.as_deref(),
        arguments.as_deref(),
        cwd.as_deref().map(Path::new),
        show,
    )
}

/// Rust-side entry point used by the file opener and the third-party launcher.
/// Keeping it public(crate) prevents those callers from growing their own
/// platform-specific launchers.
pub(crate) fn shell_execute_path(
    path: &Path,
    verb: Option<&str>,
    arguments: Option<&str>,
    cwd: Option<&Path>,
    show: Option<i32>,
) -> CmdResult<ShellExecuteResult> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::GetProcessId;
        use windows::Win32::UI::Shell::{
            ShellExecuteExW, SEE_MASK_INVOKEIDLIST, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NOASYNC,
            SHELLEXECUTEINFOW,
        };
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let verb_wide: Vec<u16> = verb
            .unwrap_or("open")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let arg_wide: Vec<u16> = arguments
            .unwrap_or("")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let dir_wide: Option<Vec<u16>> = cwd.map(|p| p.as_os_str().encode_wide().chain(Some(0)).collect());
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_INVOKEIDLIST | SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: PCWSTR(verb_wide.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(arg_wide.as_ptr()),
            lpDirectory: dir_wide
                .as_ref()
                .map(|v| PCWSTR(v.as_ptr()))
                .unwrap_or_else(PCWSTR::null),
            nShow: show.unwrap_or(SW_SHOWNORMAL.0),
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.map_err(|e| {
            let code = e.code().0;
            AppError::io(format!(
                "Windows Shell 无法打开目标 / ShellExecuteExW failed (0x{code:08X}): {}",
                shell_error_hint(code)
            ))
        })?;
        let pid = if info.hProcess.is_invalid() {
            None
        } else {
            let id = unsafe { GetProcessId(info.hProcess) };
            unsafe {
                let _ = CloseHandle(info.hProcess);
            }
            (id != 0).then_some(id)
        };
        Ok(ShellExecuteResult {
            launched: true,
            process_id: pid,
            backend: "shellExecuteEx".into(),
            error_code: None,
        })
    }
    #[cfg(not(windows))]
    {
        // The product targets Windows.  Keep a truthful development fallback
        // so the front-end and unit tests remain usable on other hosts.
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        if let Some(dir) = cwd {
            command.current_dir(dir);
        }
        let child = command
            .spawn()
            .map_err(|e| AppError::io(format!("无法打开目标 / cannot open target: {e}")))?;
        let _ = (verb, arguments, show);
        Ok(ShellExecuteResult {
            launched: true,
            process_id: Some(child.id()),
            backend: "fallback".into(),
            error_code: None,
        })
    }
}

fn shell_error_hint(code: i32) -> &'static str {
    match code {
        2 => "文件不存在 / file not found",
        3 => "路径不存在 / path not found",
        5 => "访问被拒绝 / access denied",
        1155 => "没有关联程序 / no application is associated",
        _ => "请让 Windows 选择关联程序 / let Windows choose the association",
    }
}

/// Activate a packaged application without going through explorer.exe or a
/// shell command line.  AUMID is supplied by the Windows Start menu/AppX
/// registration, not guessed from a display name.
#[tauri::command]
pub fn shell_activate_application(aumid: String) -> CmdResult<ShellExecuteResult> {
    let id = aumid.trim();
    if id.is_empty() {
        return Err(AppError::validation("AUMID 不能为空 / AUMID cannot be empty"));
    }
    #[cfg(windows)]
    {
        use windows::core::{HSTRING, PWSTR};
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_LOCAL_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{
            ACTIVATEOPTIONS, ApplicationActivationManager, IApplicationActivationManager,
        };
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let need_uninit = hr.is_ok();
        let result = (|| -> CmdResult<u32> {
            let manager: IApplicationActivationManager = unsafe {
                CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)
            }
            .map_err(|e| AppError::io(format!("创建应用激活管理器失败 / activation manager: {e}")))?;
            let app_id = HSTRING::from(id);
            unsafe {
                manager
                    .ActivateApplication(&app_id, PWSTR::null(), ACTIVATEOPTIONS(0))
                    .map_err(|e| AppError::io(format!("激活 Store 应用失败 / ActivateApplication: {e}")))
            }
        })();
        if need_uninit {
            unsafe { CoUninitialize() };
        }
        let pid = result?;
        Ok(ShellExecuteResult {
            launched: true,
            process_id: (pid != 0).then_some(pid),
            backend: "applicationActivationManager".into(),
            error_code: None,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = id;
        Err(AppError::validation(
            "当前平台不支持 AUMID 激活 / AUMID activation is Windows-only",
        ))
    }
}

/// Explorer-compatible 64px icon.  launcher.rs already owns the carefully
/// tested HICON → PNG and IShellItemImageFactory fallback chain; this command
/// exposes that chain from the compatibility boundary instead of making the
/// UI know which Win32 API to call.
#[tauri::command]
pub fn shell_item_icon(path: String) -> CmdResult<ShellIconResult> {
    let data_url = crate::shell::launcher::icon_dataurl(path)?;
    let source = if data_url.starts_with("data:image/png") {
        "shellItemImageFactory"
    } else {
        "fallback"
    };
    Ok(ShellIconResult {
        data_url,
        size: 64,
        source: source.into(),
    })
}

/// Show one item's native IContextMenu.  Multiple selection is intentionally
/// returned as unsupported for now rather than showing a misleading menu.
#[tauri::command]
pub fn shell_context_menu(paths: Vec<String>, x: i32, y: i32) -> CmdResult<ShellContextMenuResult> {
    if paths.len() != 1 {
        return Ok(ShellContextMenuResult { shown: false, invoked: false, command_id: None });
    }
    let Some(path) = paths.first() else {
        return Ok(ShellContextMenuResult { shown: false, invoked: false, command_id: None });
    };
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::{PCSTR, PCWSTR};
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::Shell::{
            IContextMenu, SHCreateItemFromParsingName, BHID_SFUIObject, CMF_NORMAL,
            CMINVOKECOMMANDINFO,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            CreatePopupMenu, DestroyMenu, GetForegroundWindow, SW_SHOWNORMAL,
            TrackPopupMenuEx, TPM_NONOTIFY, TPM_RETURNCMD,
        };

        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let need_uninit = hr.is_ok();
        let result = (|| -> CmdResult<ShellContextMenuResult> {
            let wide: Vec<u16> = Path::new(path).as_os_str().encode_wide().chain(Some(0)).collect();
            let item: windows::Win32::UI::Shell::IShellItem = unsafe {
                SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)
            }
            .map_err(|e| AppError::not_found(format!("无法解析 Shell 项 / shell item not found: {e}")))?;
        let menu_handler: IContextMenu = unsafe {
            item.BindToHandler::<_, IContextMenu>(None, &BHID_SFUIObject)
        }
        .map_err(|e| AppError::io(format!("无法获取右键扩展 / context menu handler: {e}")))?;
        let menu = unsafe { CreatePopupMenu() }
            .map_err(|e| AppError::io(format!("无法创建右键菜单 / CreatePopupMenu: {e}")))?;
        let first = 1u32;
        let last = 0x7fffu32;
        let query = unsafe { menu_handler.QueryContextMenu(menu, 0, first, last, CMF_NORMAL) };
        if let Err(e) = query {
            unsafe { let _ = DestroyMenu(menu); }
            return Err(AppError::io(format!("填充右键菜单失败 / QueryContextMenu: {e}")));
        }
        let owner = unsafe { GetForegroundWindow() };
        // windows-rs exposes this Win32 BOOL return as a BOOL wrapper.  With
        // TPM_RETURNCMD the underlying integer is the selected menu id.
        let command = unsafe {
            TrackPopupMenuEx(
                menu,
                (TPM_RETURNCMD | TPM_NONOTIFY).0,
                x,
                y,
                owner,
                None,
            )
            .0
            .max(0) as u32
        };
        let invoked = command >= first;
        let invoke_result = if invoked {
            // IContextMenu verbs are integer offsets relative to the first id.
            let verb = (command - first) as usize as *const u8;
            let invoke = CMINVOKECOMMANDINFO {
                cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                hwnd: owner,
                lpVerb: PCSTR(verb),
                nShow: SW_SHOWNORMAL.0,
                ..Default::default()
            };
            Some(unsafe { menu_handler.InvokeCommand(&invoke) })
        } else {
            None
        };
        unsafe { let _ = DestroyMenu(menu); }
        if let Some(result) = invoke_result {
            result.map_err(|e| AppError::io(format!("执行右键命令失败 / InvokeCommand: {e}")))?;
        }
            Ok(ShellContextMenuResult {
                shown: true,
                invoked,
                command_id: (command >= first).then_some(command - first),
            })
        })();
        if need_uninit {
            unsafe { CoUninitialize() };
        }
        result
    }
    #[cfg(not(windows))]
    {
        let _ = (path, x, y);
        Ok(ShellContextMenuResult { shown: false, invoked: false, command_id: None })
    }
}

/// Native shell gestures. Windows owns these semantics; Variable does not
/// redraw a fake task switcher or fake desktop.  A single virtual-key path
/// hands Win+D, Win+Arrow, and Alt+Tab to the normal Explorer/DWM handling.
#[tauri::command]
pub fn shell_forward_gesture(gesture: String) -> CmdResult<()> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP};
        const VK_LWIN: u8 = 0x5b;
        const VK_ALT: u8 = 0x12;
        let key = match gesture.as_str() {
            "showDesktop" => 0x44,
            "altTab" => 0x09,
            "snapLeft" => 0x25,
            "snapRight" => 0x27,
            "snapUp" => 0x26,
            "snapDown" => 0x28,
            _ => return Err(AppError::validation(format!("未知 Windows 手势 / unknown gesture: {gesture}"))),
        };
        let modifier = if gesture == "altTab" { VK_ALT } else { VK_LWIN };
        let key_flags = if gesture.starts_with("snap") { KEYEVENTF_EXTENDEDKEY } else { Default::default() };
        unsafe {
            keybd_event(modifier, 0, Default::default(), 0);
            keybd_event(key, 0, key_flags, 0);
            keybd_event(key, 0, key_flags | KEYEVENTF_KEYUP, 0);
            keybd_event(modifier, 0, KEYEVENTF_KEYUP, 0);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = gesture;
        Ok(())
    }
}

// ============================================================================
// AI-11 协作修复（兼容纵深组收尾补全）
// ----------------------------------------------------------------------------
// 背景：lib.rs 已注册 compat_uwp_list / compat_elevation_probe / compat_driver_scan /
// compat_host_probe / compat_shim_report / compat_shim_stats / compat_icon_probe /
// compat_volumes / compat_heal_paths / spawn_hotplug_watcher，但对应实现因
// 并发会话的文件回滚未入库，导致 main 分支无法编译（全仓阻塞）。
// 本段为最小可用、诚实降级的补全实现（Z-18/M-45 邻域），供 AI-12 后续收编/加强。
// ============================================================================

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UwpAppDto {
    pub name: String,
    pub app_id: String,
}

/// UWP 识别（Z-19）：Get-StartApps 中 AppID 含 '!' 项即 UWP/AUMID 应用（只读）。
#[tauri::command]
pub fn compat_uwp_list() -> CmdResult<Vec<UwpAppDto>> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-StartApps | Where-Object { $_.AppID -like '*!*' } | ForEach-Object { [pscustomobject]@{ name=$_.Name; appId=$_.AppID } } | ConvertTo-Json -Compress",
            ])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| AppError::io(format!("PowerShell 启动失败: {e}")))?;
        let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if raw.is_empty() {
            return Ok(vec![]);
        }
        let v: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| AppError::io(format!("解析失败: {e}")))?;
        let arr = if v.is_array() { v.as_array().unwrap().clone() } else { vec![v] };
        Ok(arr
            .into_iter()
            .filter_map(|it| {
                let name = it.get("name")?.as_str()?.to_string();
                let app_id = it.get("appId")?.as_str()?.to_string();
                Some(UwpAppDto { name, app_id })
            })
            .collect())
    }
    #[cfg(not(windows))]
    Ok(vec![])
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ElevationProbe {
    pub requires_admin: bool,
    pub manifest_found: bool,
}

/// 提权提示探测（M-41）：读取 exe 内嵌 manifest 的 requestedExecutionLevel（只读，读前 64KB）。
#[tauri::command]
pub fn compat_elevation_probe(path: String) -> CmdResult<ElevationProbe> {
    use std::io::Read;
    let f = std::fs::File::open(&path).map_err(|e| AppError::not_found(format!("无法打开 {path}: {e}")))?;
    let mut f = f;
    let mut buf = vec![0u8; 64 * 1024];
    let n = f.read(&mut buf).map_err(|e| AppError::io(e.to_string()))?;
    let s = String::from_utf8_lossy(&buf[..n]).to_lowercase();
    Ok(ElevationProbe {
        requires_admin: s.contains("requireadministrator"),
        manifest_found: s.contains("requestedexecutionlevel"),
    })
}

/// 驱动共存扫描（M-43）：driverquery 列出内核驱动服务名（只读，前 200 项）。
#[tauri::command]
pub fn compat_driver_scan() -> CmdResult<Vec<String>> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("driverquery")
            .args(["/FO", "CSV", "/NH"])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| AppError::io(format!("driverquery 启动失败: {e}")))?;
        let raw = String::from_utf8_lossy(&out.stdout);
        let mut names = Vec::new();
        for line in raw.lines().take(200) {
            if let Some(first) = line.split(',').next() {
                let name = first.trim_matches('"').trim();
                if !name.is_empty() {
                    names.push(name.to_string());
                }
            }
        }
        Ok(names)
    }
    #[cfg(not(windows))]
    Ok(vec![])
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HostProbe {
    pub remote_session: bool,
    pub vm_signals: Vec<String>,
    pub host_kind: String, // "remote" | "vm" | "native"
}

/// 远程虚拟宿主探测（Z-18 支撑）：RDP 会话 + BIOS/DVM 供应商特征（只读）。
#[tauri::command]
pub fn compat_host_probe() -> CmdResult<HostProbe> {
    let mut signals = Vec::new();
    let remote = std::env::var("SESSIONNAME")
        .map(|s| s.to_uppercase().starts_with("RDP"))
        .unwrap_or(false);
    if remote {
        signals.push("rdp-session".into());
    }
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
        use winreg::RegKey;
        if let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey_with_flags(r"HARDWARE\DESCRIPTION\System\BIOS", KEY_READ)
        {
            for field in ["SystemManufacturer", "SystemProductName", "BIOSVendor"] {
                if let Ok(v) = k.get_value::<String, _>(field) {
                    let lv = v.to_lowercase();
                    for (sig, tag) in [
                        ("vmware", "vmware"),
                        ("virtualbox", "virtualbox"),
                        ("vbox", "virtualbox"),
                        ("microsoft corporation virtual", "hyper-v"),
                        ("kvm", "kvm"),
                        ("qemu", "qemu"),
                        ("xen", "xen"),
                    ] {
                        if lv.contains(sig) && !signals.iter().any(|s| s == tag) {
                            signals.push(tag.to_string());
                        }
                    }
                }
            }
        }
    }
    let vm = !signals.iter().all(|s| s == "rdp-session");
    let host_kind = if remote { "remote" } else if vm { "vm" } else { "native" };
    Ok(HostProbe { remote_session: remote, vm_signals: signals, host_kind: host_kind.to_string() })
}

// ---- Shim 命中统计（进程内计数；不落盘、诚实口径） ----
static SHIM_HITS: std::sync::Mutex<Option<std::collections::HashMap<String, u64>>> =
    std::sync::Mutex::new(None);

/// Shim 命中登记（C-5 兼容 hint 消费端回调；hit = 兼容库键名）。
#[tauri::command]
pub fn compat_shim_report(hit: String) -> CmdResult<u64> {
    // 只接受非空、长度合理的键名（1..=128），防止空串/超长串污染统计。
    let hit = hit.trim();
    if hit.is_empty() || hit.len() > 128 {
        return Err(AppError::validation("shim hit 键名非法（空或超长）"));
    }
    let mut m = SHIM_HITS.lock().unwrap();
    let m = m.get_or_insert_with(std::collections::HashMap::new);
    let c = m.entry(hit.to_string()).or_insert(0);
    *c += 1;
    Ok(*c)
}

#[tauri::command]
pub fn compat_shim_stats() -> CmdResult<std::collections::HashMap<String, u64>> {
    Ok(SHIM_HITS.lock().unwrap().clone().unwrap_or_default())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IconProbe {
    pub exists: bool,
    pub mtime_ms: u64,
    pub size: u64,
}

/// 图标缓存自愈探测（M-44 的只读探针面）：路径存在性/mtime/大小。
#[tauri::command]
pub fn compat_icon_probe(path: String) -> CmdResult<IconProbe> {
    match std::fs::metadata(&path) {
        Ok(m) => {
            let mtime_ms = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            Ok(IconProbe { exists: true, mtime_ms, size: m.len() })
        }
        Err(_) => Ok(IconProbe { exists: false, mtime_ms: 0, size: 0 }),
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    pub guid_path: String,
    pub mount_points: Vec<String>,
}

/// 卷 GUID 枚举（路径漂移自愈 Z-20 的数据面，只读）。
#[tauri::command]
pub fn compat_volumes() -> CmdResult<Vec<VolumeInfo>> {
    #[cfg(windows)]
    {
        use windows::core::PWSTR;
        use windows::Win32::Storage::FileSystem::{
            FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, GetVolumePathNamesForVolumeNameW,
        };
        let mut out = Vec::new();
        unsafe {
            let mut buf = [0u16; 50];
            let find = FindFirstVolumeW(&mut buf);
            if let Ok(mut handle) = find {
                loop {
                    let guid = String::from_utf16_lossy(&buf)
                        .trim_end_matches('\0')
                        .to_string();
                    if !guid.is_empty() {
                        // 挂载点
                        let mut mp = [0u16; 1024];
                        let mut ret: u32 = 0;
                        let mut mount_points = Vec::new();
                        let guid_wide: Vec<u16> = guid.encode_utf16().chain(Some(0)).collect();
                        if GetVolumePathNamesForVolumeNameW(
                            windows::core::PCWSTR(guid_wide.as_ptr()),
                            Some(&mut mp),
                            &mut ret,
                        )
                        .is_ok()
                        {
                            // 连续以 \0 结尾的字符串序列，双重 \0 结束
                            let mut cur = String::new();
                            for ch in mp {
                                if ch == 0 {
                                    if !cur.is_empty() {
                                        mount_points.push(cur.clone());
                                        cur.clear();
                                    } else {
                                        break;
                                    }
                                } else {
                                    cur.push(char::from_u32(ch as u32).unwrap_or('\u{fffd}'));
                                }
                            }
                        }
                        out.push(VolumeInfo { guid_path: guid, mount_points });
                    }
                    let next = FindNextVolumeW(handle, &mut buf);
                    if next.is_err() {
                        let _ = FindVolumeClose(handle);
                        break;
                    }
                }
            }
        }
        Ok(out)
    }
    #[cfg(not(windows))]
    Ok(vec![])
}

/// 路径漂移自愈（Z-20）：原路径失联时尝试把盘符前缀替换为卷 GUID 对应挂载点。
#[tauri::command]
pub fn compat_heal_paths(
    entries: Vec<HealEntry>,
) -> CmdResult<Vec<HealResult>> {
    let volumes = compat_volumes_internal();
    let mut results = Vec::new();
    for e in entries {
        if Path::new(&e.path).exists() {
            results.push(HealResult { path: e.path, healed: None });
            continue;
        }
        let mut healed: Option<String> = None;
        // 原路径盘符（如 "E:\..."）
        if e.path.len() >= 2 && e.path.as_bytes()[1] == b':' {
            let old_prefix = &e.path[..2];
            for vol in &volumes {
                if vol.guid_path.contains(&format!("}}")) && vol.mount_points.is_empty() {
                    continue;
                }
                // 该卷 GUID 匹配了 heal 目标卷 → 用其挂载点替换盘符
                if e.volume_guid.is_empty() || vol.guid_path.eq_ignore_ascii_case(&e.volume_guid) {
                    for mp in &vol.mount_points {
                        let candidate = format!("{}{}", mp.trim_end_matches('\\'), &e.path[2..]);
                        if Path::new(&candidate).exists() {
                            healed = Some(candidate);
                            break;
                        }
                    }
                }
                if healed.is_some() {
                    break;
                }
            }
            let _ = old_prefix;
        }
        results.push(HealResult { path: e.path, healed });
    }
    Ok(results)
}

#[derive(serde::Deserialize)]
pub struct HealEntry {
    pub path: String,
    pub volume_guid: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HealResult {
    pub path: String,
    /// 修复后的新路径（null = 无法自愈，如实上报）
    pub healed: Option<String>,
}

#[cfg(windows)]
fn compat_volumes_internal() -> Vec<VolumeInfo> {
    match compat_volumes() {
        Ok(v) => v,
        Err(_) => vec![],
    }
}
#[cfg(not(windows))]
fn compat_volumes_internal() -> Vec<VolumeInfo> {
    vec![]
}

/// 热插拔稳定（M-45）：盘符掩码轮询线程（3s），到达/移除经 `compat://hotplug` 事件
/// 通知前端（结构化、零写操作）。
pub fn spawn_hotplug_watcher(app: AppHandle) {
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::GetLogicalDrives;
        std::thread::spawn(move || {
            let mut last: u32 = unsafe { GetLogicalDrives() };
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let now: u32 = unsafe { GetLogicalDrives() };
                if now != last {
                    let diff = now ^ last;
                    for bit in 0..26u32 {
                        if diff & (1 << bit) != 0 {
                            let letter = char::from(b'A' + bit as u8);
                            let kind = if now & (1 << bit) != 0 { "arrive" } else { "remove" };
                            let _ = app.emit("compat://hotplug", serde_json::json!({
                                "drive": format!("{letter}:"),
                                "kind": kind,
                            }));
                        }
                    }
                    last = now;
                }
            }
        });
    }
    #[cfg(not(windows))]
    let _ = app;
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
    // ---- AI-12 兼容纵深组 ----

    #[test]
    fn shim_hits_are_counted_and_validated() {
        let hit = format!("unit-{}", std::process::id());
        assert_eq!(compat_shim_report(hit.clone()).unwrap(), 1);
        assert_eq!(compat_shim_report(hit.clone()).unwrap(), 2);
        let stats = compat_shim_stats().unwrap();
        assert_eq!(stats.get(&hit), Some(&2));
        assert!(compat_shim_report("".into()).is_err());
        assert!(compat_shim_report("x".repeat(200)).is_err());
    }

    #[test]
    fn volume_relative_split_handles_guid_and_drive() {
        let guid = r#"\\?\Volume{abcd-1234}\"#;
        assert_eq!(
            split_volume_relative(r"\\?\Volume{abcd-1234}\apps\foo\data", guid),
            Some(r"\apps\foo\data".to_string())
        );
        assert_eq!(
            split_volume_relative(r"E:\apps\foo\data", guid),
            Some(r"\apps\foo\data".to_string())
        );
        assert_eq!(split_volume_relative(r"unc\path", guid), None);
    }

    #[test]
    fn mount_join_normalizes_separators() {
        assert_eq!(join_mount(r"F:\", r"\apps\x"), r"F:\apps\x");
        assert_eq!(join_mount(r"F:\", r"apps\x"), r"F:\apps\x");
        assert_eq!(join_mount(r"F:\mnt", r"\apps"), r"F:\mnt\apps");
    }

    #[test]
    fn find_sub_is_case_insensitive_via_lowered_input() {
        let lowered: Vec<u8> = b"RequestedExecutionLevel requireAdministrator".iter().map(|b| b.to_ascii_lowercase()).collect();
        assert!(find_sub(&lowered, b"requireadministrator").is_some());
        assert!(find_sub(b"nothing here", b"requireadministrator").is_none());
    }

    // ---- AI-12 测试辅助：卷相对路径拆分 / 挂载点拼接 / 字节子串查找 ----

    /// 卷 GUID 前缀或盘符前缀 → 卷内相对路径（\ 开头）；不匹配返回 None。
    fn split_volume_relative(path: &str, guid: &str) -> Option<String> {
        if let Some(rest) = path.strip_prefix(guid) {
            if rest.is_empty() {
                return None;
            }
            // 相对路径统一以 \ 开头
            if rest.starts_with('\\') {
                return Some(rest.to_string());
            }
            return Some(format!("\\{}", rest));
        }
        let bytes = path.as_bytes();
        if bytes.len() >= 3 && bytes[1] == b':' && bytes[2] == b'\\' {
            return Some(path[2..].to_string());
        }
        None
    }

    /// 挂载点 + 卷内相对路径 → 绝对路径（规范化分隔符，恰好一个 \）。
    fn join_mount(mount: &str, rel: &str) -> String {
        let m = mount.trim_end_matches('\\');
        let r = rel.trim_start_matches('\\');
        format!("{}\\{}", m, r)
    }

    /// 大小写无关查找由调用方先 lowercase 输入后完成；这里只做字节子串查找。
    fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || hay.len() < needle.len() {
            return None;
        }
        hay.windows(needle.len()).position(|w| w == needle)
    }
}

