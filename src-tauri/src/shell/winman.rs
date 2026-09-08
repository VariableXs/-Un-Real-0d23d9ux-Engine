//! L3 shell — winman.rs（批次D）
//! 桌面窗口管理（规格 4.3 红绿灯补全 / 4.4.3 Win 键）：
//! - 🟢 覆盖/避让 Windows 任务栏切换（避让 = 桌面窗口缩到工作区，露出系统任务栏）
//! - 🔴 "隐藏到托盘"（窗口隐藏，Variable 继续运行；托盘左键恢复）
//! - 裸 Win 键全局捕获（RegisterHotKey MOD_WIN）→ 前端开始菜单开关
//! 仅 Windows 有真实行为；其余平台占位（与 hardware.rs 同策略）。

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};

use std::collections::HashMap;
use std::sync::Mutex;

/// 全局快捷键表（批次E，规格 4.7）：accel → action。
/// 默认表由 init_shortcuts 注册；用户自定义经 shortcuts_apply 整表重注册。
static SHORTCUT_MAP: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// action → 前端事件分发（与旧批次C/D 事件名保持一致，前端零迁移）。
fn dispatch_action(app: &AppHandle, action: &str) {
    match action {
        // 系统窗口交由桌面窗口内的 VWM 打开虚拟窗口（不再另开 OS 窗口）
        "explorer" | "explorerCtrl" => {
            let _ = app.emit_to("desktop", "sys://open-system", "explorer");
        }
        // F-1：设置中心呼出（Win+I 被系统保留，默认 ctrl+alt+i）
        "settingsCenter" => {
            let _ = app.emit_to("desktop", "sys://open-settings", ());
        }
        // F-2：剪贴板历史呼出（Win+V 被系统保留，默认 ctrl+alt+v）
        "clipboardHistory" => {
            let _ = app.emit_to("desktop", "sys://open-clipboard", ());
        }
        // AI-08 Z-28：运行对话框呼出（Win+R 被系统保留 → ctrl+alt+r 降级口径；
        // 真正的 Win+R 转译属 Z-09 系统组合键让位协议（键位纪律组）领地）
        "runDialog" => {
            let _ = app.emit_to("desktop", "sys://open-run", ());
        }
        "quickBluetooth" => {
            let _ = app.emit_to("desktop", "quickpanel://open", "bluetooth");
        }
        "quickAudio" => {
            let _ = app.emit_to("desktop", "quickpanel://open", "audio");
        }
        "notifyCenter" => {
            let _ = app.emit_to("desktop", "quickpanel://open", "");
        }
        "dnd" => {
            let _ = app.emit_to("desktop", "dnd://toggle", ());
        }
        "showDesktop" => {
            // Delegate the host gesture to the Windows shell first; the
            // desktop event only keeps Variable's own virtual windows in
            // sync.  No custom desktop bitmap is drawn here.
            let _ = crate::shell::compat::shell_forward_gesture("showDesktop".into());
            let _ = app.emit_to("desktop", "sys://show-desktop", ());
        }
        "toggleHide" => {
            let _ = app.emit_to("desktop", "sys://toggle-hide", ());
        }
        "snapLeft" | "snapRight" | "snapUp" | "snapDown" => {
            let dir = action.trim_start_matches("snap").to_lowercase();
            let _ = crate::shell::compat::shell_forward_gesture(action.to_string());
            let _ = app.emit("sys://snap", dir);
        }
        "minimizeAll" => {
            let _ = app.emit_to("desktop", "sys://minimize-all", ());
        }
        other => {
            if let Some(n) = other
                .strip_prefix("launch")
                .and_then(|d| d.parse::<u8>().ok())
                .filter(|n| (1..=9).contains(n))
            {
                let _ = app.emit_to("desktop", "sys://launch-index", n);
            }
        }
    }
}

pub fn shortcut_action(accel: &str) -> Option<String> {
    SHORTCUT_MAP
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(accel).cloned())
}

/// lib.rs handler 入口（pub 包装，避免 lib.rs 引入事件依赖细节）。
pub fn dispatch_action_pub(app: &AppHandle, action: &str) {
    dispatch_action(app, action);
}

/// 默认快捷键表（action, accel）。
/// 默认表避开 Windows 系统保留组合（Win+E/D/M/N/数字/方向键 RegisterHotKey
/// 必然失败），统一用 ctrl+alt+*；super+* 仍可由用户自定义（占用时降级）。
pub fn default_binds() -> Vec<(&'static str, &'static str)> {
    vec![
        ("explorer", "ctrl+alt+e"),
        ("settingsCenter", "ctrl+alt+i"),
        ("clipboardHistory", "ctrl+alt+v"),
        ("commandPalette", "ctrl+alt+p"),
        ("explorerCtrl", "ctrl+e"),
        ("quickBluetooth", "ctrl+alt+b"),
        ("quickAudio", "ctrl+alt+k"),
        ("notifyCenter", "ctrl+alt+n"),
        ("dnd", "ctrl+shift+m"),
        ("showDesktop", "ctrl+alt+d"),
        ("toggleHide", "ctrl+shift+d"),
        ("minimizeAll", "ctrl+alt+m"),
        ("wintab", "super+tab"),
        ("snapLeft", "ctrl+alt+left"),
        ("snapRight", "ctrl+alt+right"),
        ("snapUp", "ctrl+alt+up"),
        ("snapDown", "ctrl+alt+down"),
        ("launch1", "ctrl+alt+1"),
        ("launch2", "ctrl+alt+2"),
        ("launch3", "ctrl+alt+3"),
        ("launch4", "ctrl+alt+4"),
        ("launch5", "ctrl+alt+5"),
        ("launch6", "ctrl+alt+6"),
        ("launch7", "ctrl+alt+7"),
        ("launch8", "ctrl+alt+8"),
        ("launch9", "ctrl+alt+9"),
    ]
}

/// 启动时注册默认表（setup 调用；被系统/他方占用的项自动尝试备选组合键，
/// 仍失败的仅记录日志）。
pub fn init_shortcuts(app: &AppHandle) {
    let binds = default_binds()
        .into_iter()
        .map(|(a, accel)| ShortcutBind { action: a.into(), accel: accel.into() })
        .collect();
    match register_binds(app, binds) {
        Ok(res) => {
            for f in &res.failed {
                eprintln!("shortcut {f} register failed (degraded)");
            }
            for r in &res.remapped {
                eprintln!("shortcut {} occupied → remapped to {}", r.from, r.to);
            }
        }
        Err(e) => eprintln!("shortcuts unregister_all failed: {e}"),
    }
}

/// 一次注册尝试的净结果：failed = 连备选都全部失败的原始组合；
/// remapped = 被占用后自动改用的备选组合（前端如实提示）。
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutsApplyResult {
    pub failed: Vec<String>,
    pub remapped: Vec<ShortcutRemap>,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutRemap {
    pub from: String,
    pub to: String,
}

/// 组合键候选序列：原始 → 键名别名（left/ArrowLeft 等解析差异）→
/// 备选修饰键组合。被系统/他方占用的组合借此"彻底解决"：
/// 总有一个可用组合落地，而不是整表报错不可用。
fn accel_candidates(accel: &str) -> Vec<String> {
    let parts: Vec<String> = accel
        .split('+')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let key = parts.last().cloned().unwrap_or_default();
    let mods: Vec<&String> = parts[..parts.len().saturating_sub(1)].iter().collect();
    let has = |m: &str| mods.iter().any(|x| x.as_str() == m);
    let ctrl = has("ctrl") || has("control");
    let alt = has("alt") || has("option");
    let shift = has("shift");
    let super_ = has("super") || has("cmd") || has("command") || has("meta") || has("win");

    // 键名别名（global-hotkey 解析接受 "ArrowLeft"；用户写 "left" 也能命中）
    let key_variants: Vec<String> = match key.as_str() {
        "left" | "arrowleft" => vec!["left".into(), "ArrowLeft".into()],
        "right" | "arrowright" => vec!["right".into(), "ArrowRight".into()],
        "up" | "arrowup" => vec!["up".into(), "ArrowUp".into()],
        "down" | "arrowdown" => vec!["down".into(), "ArrowDown".into()],
        k => vec![k.into()],
    };
    // 修饰键组合候选：原样 → +shift → ctrl+shift → alt+shift
    let mut mod_sets: Vec<(bool, bool, bool)> = vec![(ctrl, alt, shift)];
    if !shift {
        mod_sets.push((ctrl, alt, true));
    }
    if !super_ {
        mod_sets.push((ctrl, !alt, true));
        mod_sets.push((!ctrl, alt, true));
    }
    let mut out: Vec<String> = Vec::new();
    for (c, a, s) in mod_sets {
        let mut m = String::new();
        if c {
            m.push_str("ctrl+");
        }
        if a {
            m.push_str("alt+");
        }
        if s {
            m.push_str("shift+");
        }
        if super_ {
            m.push_str("super+");
        }
        for k in &key_variants {
            let cand = format!("{m}{k}");
            if !out.contains(&cand) {
                out.push(cand);
            }
        }
    }
    out
}

fn register_binds(
    app: &AppHandle,
    binds: Vec<ShortcutBind>,
) -> Result<ShortcutsApplyResult, String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let gs = app.global_shortcut();
    gs.unregister_all().map_err(|e| e.to_string())?;
    let mut map = HashMap::new();
    let mut failed = Vec::new();
    let mut remapped = Vec::new();
    for b in binds {
        let mut done = false;
        let mut first_err: Option<String> = None;
        for cand in accel_candidates(&b.accel) {
            match gs.register(cand.as_str()) {
                Ok(()) => {
                    let is_remap = cand != b.accel;
                    let to = cand.clone();
                    map.insert(cand, b.action.clone());
                    if is_remap {
                        remapped.push(ShortcutRemap { from: b.accel.clone(), to });
                    }
                    done = true;
                    break;
                }
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e.to_string());
                    }
                }
            }
        }
        if !done {
            eprintln!(
                "shortcut {} register failed (all fallbacks exhausted): {}",
                b.accel,
                first_err.unwrap_or_default()
            );
            failed.push(b.accel);
        }
    }
    *SHORTCUT_MAP.lock().unwrap_or_else(|e| e.into_inner()) = Some(map);
    Ok(ShortcutsApplyResult { failed, remapped })
}

#[derive(Deserialize)]
pub struct ShortcutBind {
    pub action: String,
    pub accel: String,
}

/// 批次E（规格 4.7）：整表应用用户自定义快捷键（unregister_all → 重新注册）。
/// 实机反馈"彻底解决"：被系统/他方占用的组合自动改用备选组合键并如实回报
/// （remapped）；连备选都失败的才进 failed，由前端提示。
#[tauri::command]
pub fn shortcuts_apply(app: AppHandle, binds: Vec<ShortcutBind>) -> Result<ShortcutsApplyResult, String> {
    register_binds(&app, binds)
}

/// 🟢 切换"避让 Windows 任务栏"（avoid=true 时露出系统任务栏）。
#[tauri::command]
pub fn win_set_avoid_taskbar(app: AppHandle, avoid: bool) -> Result<(), String> {
    avoid_taskbar_impl(&app, avoid)
}

/// 🔴 选择框"隐藏到托盘"：隐藏桌面窗口，Variable 继续运行（托盘左键恢复）。
#[tauri::command]
pub fn win_hide_to_tray(app: AppHandle) -> Result<(), String> {
    let w = app.get_webview_window("desktop").ok_or("no desktop window")?;
    w.hide().map_err(|e| e.to_string())
}

/// AI-01 M-04 窗口体检（Window Health）：按 pid 列表检测无响应窗口。
/// 枚举全部可见顶层窗口，`IsHungAppWindow` 命中且 pid 在名单内 → 视为无响应。
/// 返回确认无响应的 pid 子集；非 Windows 平台恒为空（如实降级，不伪造结果）。
#[tauri::command]
pub fn win_health_scan(pids: Vec<u32>) -> Result<Vec<u32>, String> {
    #[cfg(windows)]
    {
        if pids.is_empty() {
            return Ok(vec![]);
        }
        use std::collections::HashSet;
        use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, IsHungAppWindow, IsWindowVisible,
        };
        let want: HashSet<u32> = pids.into_iter().collect();
        let mut hung: Vec<u32> = Vec::new();
        unsafe extern "system" fn probe(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let out = unsafe { &mut *(lparam.0 as *mut Vec<u32>) };
            unsafe {
                if !IsWindowVisible(hwnd).as_bool() {
                    return BOOL(1);
                }
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                if pid != 0 && IsHungAppWindow(hwnd).as_bool() && !out.contains(&pid) {
                    out.push(pid);
                }
            }
            BOOL(1)
        }
        unsafe {
            EnumWindows(Some(probe), LPARAM(&mut hung as *mut Vec<u32> as isize));
        }
        Ok(hung.into_iter().filter(|p| want.contains(p)).collect())
    }
    #[cfg(not(windows))]
    {
        let _ = pids;
        Ok(vec![])
    }
}

/// AI-01 M-06 窗口挂起（Suspend）：对第三方进程树的根进程挂起全部线程。
/// 走 ntdll `NtSuspendProcess`（与 Process Explorer 同口径），无需特权。
/// 非Windows / 进程已退出 → false（前端如实提示，不伪造成功）。
#[tauri::command]
pub fn win_suspend(pid: u32) -> Result<bool, String> {
    #[cfg(windows)]
    unsafe {
        proc_nt_call(pid, windows::core::s!("NtSuspendProcess"))
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Ok(false)
    }
}

/// AI-01 M-06 窗口恢复（Resume）：解除挂起（`NtResumeProcess`）。
#[tauri::command]
pub fn win_resume(pid: u32) -> Result<bool, String> {
    #[cfg(windows)]
    unsafe {
        proc_nt_call(pid, windows::core::s!("NtResumeProcess"))
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
        Ok(false)
    }
}

#[cfg(windows)]
unsafe fn proc_nt_call(pid: u32, name: windows::core::PCSTR) -> Result<bool, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_SUSPEND_RESUME};
    if pid == 0 {
        return Ok(false);
    }
    let handle = OpenProcess(PROCESS_SUSPEND_RESUME, false, pid).map_err(|e| e.to_string())?;
    let call = || -> bool {
        let Ok(ntdll) = GetModuleHandleA(windows::core::s!("ntdll.dll")) else {
            return false;
        };
        let Some(f) = GetProcAddress(ntdll, name) else {
            return false;
        };
        let f: unsafe extern "system" fn(isize) -> i32 = std::mem::transmute(f);
        f(handle.0 as isize) == 0 // STATUS_SUCCESS
    };
    let ok = call();
    let _ = CloseHandle(handle);
    Ok(ok)
}

/// 开始菜单电源操作（批次E，规格 4.6.3）：
/// - lock = LockWorkStation（锁屏，无需特权）
/// - logoff / reboot / shutdown = 调系统 shutdown.exe（诚实走 Windows 既有流程）
#[tauri::command]
pub fn power_action(app: AppHandle, action: String) -> Result<(), String> {
    let _ = &app; // logoff/reboot/shutdown 在 Windows 走 shutdown.exe，app 仅保留给非 Windows 分支
    match action.as_str() {
        "lock" => {
            #[cfg(windows)]
            {
                use windows::Win32::System::Shutdown::LockWorkStation;
                unsafe { LockWorkStation() }.map_err(|e| e.to_string())
            }
            #[cfg(not(windows))]
            {
                let _ = app;
                Ok(())
            }
        }
        "logoff" | "reboot" | "shutdown" => {
            let flag = match action.as_str() {
                "logoff" => "/l",
                "reboot" => "/r /t 0",
                _ => "/s /t 0",
            };
            let mut args = flag.split(' ');
            let (Some(_prog), rest) = (args.next(), args.collect::<Vec<_>>()) else {
                return Err("bad action".into());
            };
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                std::process::Command::new("shutdown")
                    .args(rest)
                    .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                    .spawn()
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            #[cfg(not(windows))]
            {
                let _ = (prog, app);
                Ok(())
            }
        }
        "sleep" => {
            // AI-04 开始菜单电源项：睡眠走系统 powrprof（若开启休眠则为休眠，诚实沿用 Windows 行为）
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                std::process::Command::new("rundll32")
                    .args(["powrprof.dll,SetSuspendState", "0,1,0"])
                    .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
                    .spawn()
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            #[cfg(not(windows))]
            {
                let _ = &app;
                Ok(())
            }
        }
        _ => Err(format!("unknown power action: {action}")),
    }
}

/// 裸 Win 键捕获（注册成功 = Windows 开始菜单不再弹出，由 Variable 开始菜单接管）。
/// 失败（被系统/他方占用）仅记录日志，诚实降级 —— 开始菜单仍可由任务栏/搜索进入。
pub fn spawn_win_key_hook(app: AppHandle) {
    #[cfg(windows)]
    {
        std::thread::spawn(move || win_key_thread(app));
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

// ---------- Windows 实现 ----------

#[cfg(windows)]
fn avoid_taskbar_impl(app: &AppHandle, avoid: bool) -> Result<(), String> {
    use tauri::PhysicalPosition;
    use tauri::PhysicalSize;
    let w = app.get_webview_window("desktop").ok_or("no desktop window")?;
    if avoid {
        let (x, y, width, height) = work_area(&w)?;
        w.set_fullscreen(false).map_err(|e| e.to_string())?;
        w.set_position(PhysicalPosition::new(x, y))
            .map_err(|e| e.to_string())?;
        w.set_size(PhysicalSize::new(width as u32, height as u32))
            .map_err(|e| e.to_string())?;
    } else {
        // 覆盖：恢复全屏（覆盖整个显示器，含系统任务栏）
        w.set_fullscreen(true).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn avoid_taskbar_impl(_app: &AppHandle, _avoid: bool) -> Result<(), String> {
    Ok(())
}

/// 工作区 = 桌面窗口所在显示器矩形减去任务栏（Shell_TrayWnd 实际矩形，支持任务栏在四边）。
#[cfg(windows)]
fn work_area(w: &tauri::WebviewWindow) -> Result<(i32, i32, u32, u32), String> {
    use windows::core::w;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect};

    let mon = w
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no monitor")?;
    let mx = mon.position().x;
    let my = mon.position().y;
    let mw = mon.size().width as i32;
    let mh = mon.size().height as i32;

    // 任务栏实际矩形；找不到 Shell_TrayWnd（异常 shell 环境）→ 不避让，返回整屏。
    let tray = unsafe { FindWindowW(w!("Shell_TrayWnd"), None) };
    let tr = match tray {
        Ok(h) => {
            let mut r = RECT::default();
            unsafe { GetWindowRect(h, &mut r) }
                .map_err(|e| e.to_string())?;
            r
        }
        Err(_) => RECT { left: mx, top: my + mh - 48, right: mx + mw, bottom: my + mh },
    };

    let (ax, ay, aw, ah) = if tr.top > my + mh / 2 {
        // 底部停靠（最常见）
        (mx, my, mw, tr.top - my)
    } else if tr.bottom < my + mh / 2 {
        // 顶部停靠
        (mx, tr.bottom, mw, my + mh - tr.bottom)
    } else if tr.left > mx + mw / 2 {
        // 右侧停靠
        (mx, my, tr.left - mx, mh)
    } else {
        // 左侧停靠
        (tr.right, my, mx + mw - tr.right, mh)
    };
    Ok((ax, ay, aw.max(400) as u32, ah.max(300) as u32))
}

/// 裸 Win 键线程：RegisterHotKey(MOD_WIN, 0) + 消息循环，WM_HOTKEY → 前端事件。
#[cfg(windows)]
fn win_key_thread(app: AppHandle) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_WIN};
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG};
    const WM_HOTKEY: u32 = 0x0312;

    // id=1、修饰=仅 Win、vk=0 → 裸 Win 键（按下+松开）。
    let ok = unsafe { RegisterHotKey(HWND::default(), 1, MOD_WIN, 0) };
    if ok.is_err() {
        eprintln!("[winman] bare Win key unavailable (reserved by system) — degraded");
        return;
    }
    let mut msg = MSG::default();
    loop {
        let r = unsafe { GetMessageW(&mut msg, HWND::default(), 0, 0) };
        if r.0 <= 0 {
            break;
        }
        if msg.message == WM_HOTKEY {
            let _ = app.emit("sys://win-key", ());
        }
    }
}

// ---------- 批次W-5：显示器热切换看护 ----------

/// 显示器热切换看护：2s 轮询主屏分辨率 + 逻辑显示器数（等效 WM_DISPLAYCHANGE，
/// 无需消息窗口，天然 2s 防抖）。签名变化 → `sys://display-changed`
/// （payload = 新签名 "宽x高"）→ 前端分屏记忆恢复 / 出屏窗口吸附回主屏。
pub fn spawn_display_watcher(app: AppHandle) {
    #[cfg(windows)]
    {
        std::thread::spawn(move || {
            use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
            let sign = || -> String {
                let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
                let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
                format!("{w}x{h}")
            };
            let mut last = sign();
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let cur = sign();
                if cur != last {
                    last = cur.clone();
                    let _ = app.emit("sys://display-changed", cur);
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

// ---------- 批次E-6：全屏应用检测（自动避让） ----------

/// 全屏检测线程：2s 轮询前台窗口是否铺满其所在显示器。
/// 状态变化时发 `sys://fullscreen`（bool）→ 前端隐藏任务栏/红绿灯，全屏应用退出即恢复。
/// 前台是 Variable 桌面自身时不视为全屏应用。
pub fn spawn_fullscreen_watcher(app: AppHandle) {
    #[cfg(windows)]
    {
        std::thread::spawn(move || {
            let mut last = false;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let fs = fullscreen_foreground(&app);
                if fs != last {
                    last = fs;
                    let _ = app.emit("sys://fullscreen", fs);
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

/// 前台窗口是否为铺满显示器的全屏应用（排除 Variable 自身窗口）。
#[cfg(windows)]
fn fullscreen_foreground(app: &AppHandle) -> bool {
    use tauri::Manager;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_invalid() {
        return false;
    }
    // Variable 自己的任何窗口（桌面/软件窗口）不算"全屏应用"
    let self_hwnds: Vec<isize> = app
        .webview_windows()
        .values()
        .filter_map(|w| w.hwnd().ok().map(|h| h.0 as isize))
        .collect();
    if self_hwnds.contains(&(hwnd.0 as isize)) {
        return false;
    }
    let mon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if mon.is_invalid() {
        return false;
    }
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(mon, &mut mi) }.as_bool() {
        return false;
    }
    let mut wr = windows::Win32::Foundation::RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut wr) }.is_err() {
        return false;
    }
    wr.left == mi.rcMonitor.left
        && wr.top == mi.rcMonitor.top
        && wr.right == mi.rcMonitor.right
        && wr.bottom == mi.rcMonitor.bottom
}

// ---------- 批次C-5：L4 反作弊看护（kbdhook 停用 + 前端横幅） ----------

/// 反作弊/守护服务进程名（小写）。命中任一 → Variable 停用全局键监控并横幅声明，
/// 进程与数据通道全保留（不注入、不结束、不干预）。
#[cfg(windows)]
const ANTICHEAT_PROCESSES: &[&str] = &[
    "easyanticheat.exe",
    "easyanticheat_eos.exe",
    "beservice.exe",
    "beservice_x64.exe",
    "vanguard.exe",
    "vgc.exe",
    "faceitclient.exe",
    "eseaclient.exe",
    "rainbowsix_vc.exe",
];

/// 反作弊看护线程：3s 轮询进程快照 → 状态变化时置位 kbdhook::ANTICHEAT
/// 并发 `sys://anticheat`（bool）→ 前端横幅；退出恢复（桌面 2s 内全量回来）。
pub fn spawn_anticheat_watcher(app: AppHandle) {
    #[cfg(windows)]
    {
        std::thread::spawn(move || {
            use std::sync::atomic::Ordering;
            let mut last = false;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let hit = anticheat_running();
                if hit != last {
                    last = hit;
                    crate::shell::kbdhook::ANTICHEAT.store(hit, Ordering::Relaxed);
                    let _ = app.emit("sys://anticheat", hit);
                }
            }
        });
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

/// 当前是否有反作弊进程在运行（ToolHelp 进程快照，只读）。
#[cfg(windows)]
fn anticheat_running() -> bool {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let name = String::from_utf16_lossy(
                    &entry.szExeFile[..entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(0)],
                )
                .to_lowercase();
                if ANTICHEAT_PROCESSES.contains(&name.as_str()) {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snap);
        found
    }
}
