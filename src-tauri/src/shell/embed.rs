//! L3 shell — embed.rs（批次E-16 第三方应用环境内嵌；批次W-1 多嵌入并发 + 注册中心）：
//! - 目标：第三方应用不在 Variable 之外打开 —— 启动后把它的主窗口 SetParent
//!   成 Variable 桌面窗口的子窗口（WS_CHILD），随虚拟窗口移动/缩放，从任务栏
//!   与 Alt+Tab 消失，实现与 Windows 桌面的隔离。
//! - W-1：单例 EmbedSession → EmbedRegistry（HashMap<embed_id, EmbedSession>），
//!   embed_id = 前端 VWM 虚拟窗口实例 id（占位窗口创建时分配）；≥3 个第三方
//!   窗口可同时嵌入、各自拖拽缩放独立。旧单嵌入口（不带 embed_id）映射 id="0" 兼容。
//! - W-1 焦点仲裁：VWM Z 序顶窗口 = 嵌入移交焦点对象；点击非顶嵌入窗口时前端
//!   先 pointerFocusVwm 置顶再 embed_focus（VirtualWindowFrame onPointerDown）。
//! - W-1 退出会话：Variable 退出前逐 session embed_close（WM_CLOSE，应用自行
//!   退出），30s 超时者**留在桌面**（脱离重父化），绝不强杀（embed_close_all）。
//! - 进程启动复用已审查的 tp_launch 通道（本模块不新增进程创建代码）。
//! - 无法嵌入的应用（UWP/管理员权限/无主窗口）如实回退为独立窗口运行。

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
#[cfg(windows)]
use tauri::Manager;

use crate::error::{AppError, CmdResult};

struct EmbedSession {
    /// 嵌入的子窗口句柄
    hwnd: isize,
    /// 第三方登记 id（崩溃/退出事件回传前端用）
    tp_id: String,
    /// 批次W-2 DPI 例外：登记 dpiFix=true 的应用不转发 WM_DPICHANGED
    dpi_fix: bool,
    /// 批次W-2 最近一次已知 DPI（跨屏拖动时检测变化 → 转发 WM_DPICHANGED）
    last_dpi: u32,
    /// 批次C-1：启动/收编时的根 pid（pid 树监护锚点）
    root_pid: u32,
    /// 批次C-1：监护线程 2s 刷新的存活 pid 树缓存（WinEventHook 过滤用，
    /// 覆盖 Chrome 类多进程派生；快照频率 = 兜底轮询，主通道是事件钩子）
    pids: Vec<u32>,
    /// 批次C-3 L2 容器包裹：Variable 原生宿主窗口句柄（hwnd 仍为第三方子窗口）。
    /// None = L1 直嵌桌面（无宿主）。
    host: Option<isize>,
    /// 批次C-4 L3：会话带画面采集（前端经 embed-frame 事件合成；close 时停止+还原窗口）。
    capture: bool,
}

/// W-1 嵌入注册中心：embed_id（VWM 虚拟窗口实例 id）→ 会话。
static EMBEDS: Mutex<Option<HashMap<String, EmbedSession>>> = Mutex::new(None);

fn with_registry<R>(f: impl FnOnce(&mut HashMap<String, EmbedSession>) -> R) -> R {
    let mut guard = EMBEDS.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// 旧单嵌入口兼容：缺省 embed_id 一律映射 "0"。
fn norm_id(embed_id: Option<String>) -> String {
    embed_id.unwrap_or_else(|| "0".to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedResult {
    /// 是否成功嵌入（false = 已回退为独立窗口运行）
    pub attached: bool,
    pub reason: String,
    /// 批次C-2：根 pid（失败后前端「框选窗口」收编用；成功时同样返回）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_pid: Option<u32>,
    /// 批次C-4：true = L3 画面捕获会话（前端开帧画布 + 输入转发）
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub capture: bool,
}

// ---------- 批次C-2：捕获可靠性（自适应超时 / 标题正则兜底 / 失败证据包） ----------

/// 每登记项捕获提示（data_dir/embed-capture.json；不进 apps.json——可由用户手工微调）。
#[derive(serde::Deserialize, serde::Serialize, Default, Clone)]
struct CaptureHint {
    /// 自适应捕获超时（ms；快启动应用缩短，慢启动延长；缺省 30s）
    #[serde(default)]
    capture_timeout_ms: Option<u64>,
    /// 窗口标题正则兜底（pid 树/映像名都匹配不到时用；登记可选）
    #[serde(default)]
    title_regex: Option<String>,
}

fn hints_path(st: &crate::state::AppState) -> std::path::PathBuf {
    st.data_dir.join("embed-capture.json")
}

fn load_hints(st: &crate::state::AppState) -> HashMap<String, CaptureHint> {
    match std::fs::read(hints_path(st)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn save_hints(st: &crate::state::AppState, hints: &HashMap<String, CaptureHint>) {
    if let Ok(json) = serde_json::to_vec_pretty(hints) {
        let _ = crate::fsutil::atomic_write(hints_path(st), json);
    }
}

/// 批次C-2：超时证据包 —— EnumWindows 可见顶层窗口快照 + 根进程树 →
/// logs/embed-fail-<ts>.json（C-8 实测矩阵归因用；绝不阻塞启动流程）。
#[cfg(windows)]
fn write_fail_evidence(st: &crate::state::AppState, tp_id: &str, root_pid: u32, exe_name: &str) {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    let snap = win::collect_handles(&mut |_h, _img| true);
    let mut windows_json: Vec<serde_json::Value> = Vec::new();
    for h in snap.into_iter().take(200) {
        let hwnd = hwnd_from_isize(h);
        let mut buf = [0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
        let title = String::from_utf16_lossy(&buf[..len as usize]);
        let pid = win::window_pid(hwnd).unwrap_or(0);
        windows_json.push(serde_json::json!({
            "hwnd": h,
            "pid": pid,
            "image": win::process_image(pid).unwrap_or_default(),
            "title": title,
        }));
    }
    let evidence = serde_json::json!({
        "ts": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        "tpId": tp_id,
        "exe": exe_name,
        "rootPid": root_pid,
        "pidTree": win::pid_tree(root_pid),
        "windows": windows_json,
    });
    let dir = st.data_dir.join("logs");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(
        dir.join(format!("embed-fail-{ts}.json")),
        serde_json::to_vec_pretty(&evidence).unwrap_or_default(),
    );
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

    /// 批次W-2：进程级 Per-Monitor V2 DPI 感知（幂等；清单已声明时静默失败）。
    pub fn ensure_per_monitor_dpi() {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        }
    }

    /// 批次W-2：窗口所在显示器的实际 DPI（≠ 主屏 DPI）。
    pub fn window_dpi(hwnd: isize) -> u32 {
        use windows::Win32::UI::HiDpi::GetDpiForWindow;
        // 96 = USER_DEFAULT_SCREEN_DPI；取不到时按 100% 处理（不转发）
        unsafe { GetDpiForWindow(super::hwnd_from_isize(hwnd)).max(96) }
    }

    /// 批次W-2：跨屏 DPI 变化 → 转发 WM_DPICHANGED（部分应用自处理重排；
    /// 不处理者保持前端已应用的边界，如实标注轻微模糊）。
    pub fn forward_dpichanged(hwnd: isize, new_dpi: u32, old_dpi: u32) {
        use windows::Win32::Foundation::{LPARAM, RECT, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, SendMessageW, WM_DPICHANGED};
        if old_dpi == 0 {
            return;
        }
        unsafe {
            let h = super::hwnd_from_isize(hwnd);
            let mut r = RECT::default();
            let _ = GetWindowRect(h, &mut r);
            let scale = |v: i32| -> i32 {
                ((v as i64 * new_dpi as i64) / old_dpi as i64).max(1) as i32
            };
            let suggested = RECT {
                left: r.left,
                top: r.top,
                right: r.left + scale(r.right - r.left),
                bottom: r.top + scale(r.bottom - r.top),
            };
            // wParam = MAKEWPARAM(x_dpi, y_dpi)
            SendMessageW(
                h,
                WM_DPICHANGED,
                WPARAM((new_dpi | (new_dpi << 16)) as usize),
                LPARAM(&suggested as *const RECT as isize),
            );
        }
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

    /// 等待出现某个"启动前不存在"的匹配窗口（批次C-2：超时自适应 + 标题正则
    /// 兜底；命中即返回，超时如实回退独立窗口，绝不终止应用进程）。
    /// 匹配优先级：pid ∈ 启动进程的子进程树（覆盖启动器派生场景）→
    /// 映像名以登记 exe 结尾（兜底 pid 拿不到的场景）→ 窗口标题正则（登记可选）。
    pub fn wait_new_window(
        exe_name: &str,
        root_pid: Option<u32>,
        before: &[isize],
        title_regex: Option<&str>,
        timeout_ms: u64,
    ) -> Option<isize> {
        let suffix = exe_name.to_lowercase();
        let re = title_regex.and_then(|p| regex::Regex::new(p).ok());
        let steps = ((timeout_ms / 200).max(1)) as usize;
        for _ in 0..steps {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let tree: Vec<u32> = root_pid.map(|r| pid_tree(r)).unwrap_or_default();
            let mut title_hit: Option<isize> = None;
            let hit = collect_handles(&mut |h, img| {
                if before.contains(&h) {
                    return false;
                }
                if !tree.is_empty() {
                    let mut wp = 0u32;
                    unsafe { GetWindowThreadProcessId(super::hwnd_from_isize(h), Some(&mut wp)) };
                    if tree.contains(&wp) {
                        return true;
                    }
                }
                if img.to_lowercase().ends_with(&suffix) {
                    return true;
                }
                // 批次C-2：标题正则兜底（pid 树/映像名都未命中时）
                if let Some(re) = &re {
                    let mut buf = [0u16; 256];
                    let len = unsafe {
                        windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(
                            super::hwnd_from_isize(h),
                            &mut buf,
                        )
                    };
                    let title = String::from_utf16_lossy(&buf[..len as usize]);
                    if !title.is_empty() && re.is_match(&title) {
                        title_hit = Some(h);
                        return true;
                    }
                }
                false
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
/// `embed_id` = 前端 VWM 虚拟窗口实例 id（占位窗口创建时分配；缺省 "0" 兼容旧单嵌）。
/// 无法嵌入时如实返回 attached=false（应用已按独立窗口方式启动）。
#[tauri::command(async)]
#[cfg(windows)]
pub fn embed_launch(
    st: tauri::State<'_, crate::state::AppState>,
    app: tauri::AppHandle,
    id: String,
    embed_id: Option<String>,
    arg: Option<String>,
) -> CmdResult<EmbedResult> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetParent, SetWindowLongPtrW, SetWindowPos, GWL_STYLE,
        SWP_FRAMECHANGED, SWP_NOZORDER, WS_CAPTION, WS_CHILD, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
        WS_SYSMENU, WS_THICKFRAME,
    };

    let key = norm_id(embed_id);
    // 同一嵌入槽位重嵌（应用崩溃后重开等）：先脱离旧窗口（留在桌面，不强杀）
    detach_by_id(&key);

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
    let root_pid = crate::shell::launcher::tp_launch_inner(&st, &app, id.clone(), arg.as_deref())?;

    // 4) 等待新主窗口（批次C-2 自适应超时 + 标题正则兜底；超时后应用保持
    //    独立窗口运行，不终止进程，并落地失败证据包）
    let Some(desktop) = desktop_hwnd(&app) else {
        return Err(AppError::io("桌面窗口不存在 / no desktop window"));
    };
    let mut hints = load_hints(&st);
    let mut hint = hints.get(&id).cloned().unwrap_or_default();
    let timeout_ms = hint.capture_timeout_ms.unwrap_or(30_000);
    let started = std::time::Instant::now();
    let Some(hwnd) = win::wait_new_window(&exe_name, root_pid, &before, hint.title_regex.as_deref(), timeout_ms)
    else {
        write_fail_evidence(&st, &id, root_pid.unwrap_or(0), &exe_name);
        // 自适应：超时 → 下次放宽到 60s（快启动命中后由下方收紧）
        if hint.capture_timeout_ms.is_none() {
            hint.capture_timeout_ms = Some(60_000);
            hints.insert(id.clone(), hint);
            save_hints(&st, &hints);
        }
        return Ok(EmbedResult {
            attached: false,
            reason: "未能捕获应用窗口（启动较慢或无标准窗口）。应用已在系统桌面独立运行，未受影响；可在占位卡上「框选窗口」手动收编。".into(),
            root_pid,
            capture: false,
        });
    };
    // 自适应：快启动（<5s 命中）→ 下次收紧到 5s，减少慢启动错觉等待
    let elapsed = started.elapsed().as_millis() as u64;
    if elapsed < 5_000 && hint.capture_timeout_ms.unwrap_or(30_000) != 5_000 {
        hint.capture_timeout_ms = Some(5_000);
        hints.insert(id.clone(), hint);
        save_hints(&st, &hints);
    }

    // 批次C-6：分级探测（改样式前采样；结果持久化 apps.json，用户覆盖最高优先）。
    // Native = 独立窗口如实降级；L2 = 容器包裹路由（C-3）。
    let compat = crate::shell::compat_probe::probe_and_persist(&st, &id, hwnd, Some(&target));
    match compat.effective() {
        crate::shell::compat_probe::CompatTier::Native => {
            return Ok(EmbedResult {
                attached: false,
                reason: format!(
                    "「{}」判定为 Native 层级（{}）→ 保持独立窗口运行。",
                    id,
                    compat.evidence.get("reason").and_then(|v| v.as_str()).unwrap_or("?")
                ),
                root_pid,
                capture: false,
            });
        }
        crate::shell::compat_probe::CompatTier::L2 => {
            // 批次C-3 L2 容器包裹引擎：自绘/非标框架窗口不剥样式，而是包进
            // Variable 原生宿主窗口（WS_POPUP）→ 宿主作为嵌入对象。
            let Some(host) = crate::shell::container::win::create_host_on_main_thread(&app) else {
                // 宿主创建失败 → 如实降级为独立窗口（不留半嵌状态）
                return Ok(EmbedResult {
                    attached: false,
                    reason: "容器宿主窗口创建失败（L2 包裹不可用）。应用保持独立窗口运行。".into(),
                    root_pid,
                    capture: false,
                });
            };
            // 第十四轮大检查：命令已 async 化（线程池运行），宿主窗口消息泵在
            // 主线程 → wrap 的 SetParent/尺寸同步必须经主线程调度。
            if !crate::shell::container::win::wrap_child_on_main_thread(&app, host, hwnd) {
                return Ok(EmbedResult {
                    attached: false,
                    reason: "容器包裹失败（L2）。应用保持独立窗口运行。".into(),
                    root_pid,
                    capture: false,
                });
            }
            with_registry(|map| {
                map.insert(
                    key.clone(),
                    EmbedSession {
                        hwnd,
                        tp_id: id.clone(),
                        dpi_fix: tp.dpi_fix,
                        last_dpi: win::window_dpi(hwnd),
                        root_pid: root_pid.unwrap_or(0),
                        pids: Vec::new(),
                        host: Some(host),
                        capture: false,
                    },
                );
            });
            ensure_event_hook(&app);
            spawn_session_watcher(app, key, root_pid.unwrap_or(0), hwnd);
            return Ok(EmbedResult { attached: true, reason: String::new(), root_pid, capture: false });
        }
        crate::shell::compat_probe::CompatTier::L3 => {
            // 批次C-4 L3 画面捕获：真实窗口屏外隐藏 + WGC 采集 → 前端合成；
            // 输入经 embed_input PostMessage 直注。失败降级独立窗口 + 横幅。
            match crate::shell::capture::win::start_capture(app.clone(), key.clone(), hwnd) {
                Ok(()) => {
                    let _ = crate::shell::capture::win::hide_offscreen(hwnd);
                    with_registry(|map| {
                        map.insert(
                            key.clone(),
                            EmbedSession {
                                hwnd,
                                tp_id: id.clone(),
                                dpi_fix: tp.dpi_fix,
                                last_dpi: win::window_dpi(hwnd),
                                root_pid: root_pid.unwrap_or(0),
                                pids: Vec::new(),
                                host: None,
                                capture: true,
                            },
                        );
                    });
                    ensure_event_hook(&app);
                    spawn_session_watcher(app, key, root_pid.unwrap_or(0), hwnd);
                    return Ok(EmbedResult { attached: true, reason: String::new(), root_pid, capture: true });
                }
                Err(e) => {
                    return Ok(EmbedResult {
                        attached: false,
                        reason: format!(
                            "「{}」L3 画面捕获不可用（{}）→ 保持独立窗口运行。",
                            id, e
                        ),
                        root_pid,
                        capture: false,
                    });
                }
            }
        }
        crate::shell::compat_probe::CompatTier::L4 => {
            // 批次C-5：L4 智能让位——绝不嵌入。反作弊窗口强行剥样式/重父级
            // 可能触发检测误判；独占全屏重排会破坏渲染契约。应用保持独立窗口，
            // 让位语义由运行时看护承担（fullscreen → 桌面层收起；anticheat →
            // kbdhook 停用 + 横幅声明）。
            let hint = compat.hint.as_deref().unwrap_or("fullscreen");
            return Ok(EmbedResult {
                attached: false,
                reason: format!(
                    "「{}」判定为 L4 层级（让位归因：{}）→ 保持独立窗口运行，Variable 桌面层将智能让位。",
                    id, hint
                ),
                root_pid,
                capture: false,
            });
        }
        // L1（标准窗口）→ 走下方重父级嵌入主路径
        _ => {}
    }

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

    with_registry(|map| {
        map.insert(
            key.clone(),
            EmbedSession {
                hwnd,
                tp_id: id.clone(),
                dpi_fix: tp.dpi_fix,
                last_dpi: win::window_dpi(hwnd),
                root_pid: root_pid.unwrap_or(0),
                pids: Vec::new(),
                host: None,
                capture: false,
            },
        );
    });

    // 批次C-1：确保 WinEventHook 常驻监护已启动（首个嵌入会话时初始化，全局一份）
    ensure_event_hook(&app);

    // 批次W-3 长期监护：每 2s 核对该会话窗口存活（事件线程，随会话结束退出）。
    // 窗口消失 → 进程树仍在 = Orphaned（应用回到自身窗口）；树全灭 = 退出，
    // 采样退出码（0 = 正常退出，非 0 = 异常终止）。绝不强杀，只如实上报。
    spawn_session_watcher(app, key, root_pid.unwrap_or(0), hwnd);

    Ok(EmbedResult { attached: true, reason: String::new(), root_pid, capture: false })
}

/// 批次W-3 + C-1：单会话监护线程（2s 轮询兜底；事件驱动主通道见 ensure_event_hook）。
/// - 每拍刷新该会话的 pid 树缓存（Chrome 类多进程动态扩展）；
/// - 会话 hwnd 从注册中心现读（C-1 重嵌更新 hwnd 后监护自动跟随）；
/// - 窗口消失先留一拍宽限（WinEventHook 可能正在自动重嵌），仍未恢复才上报；
/// - 会话被 embed_close 移除或状态上报后退出。绝不强杀进程。
#[cfg(windows)]
fn spawn_session_watcher(app: tauri::AppHandle, key: String, root_pid: u32, _hwnd: isize) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        // 会话已被显式关闭（embed_close / 脱离 / 退出清场）→ 前端已处理，收工
        if !with_registry(|m| m.contains_key(&key)) {
            return;
        }
        // 批次C-1：刷新 pid 树缓存（动态合并新后代进程；也是资源护栏的兜底轮询）
        let tree = if root_pid != 0 { win::pid_tree(root_pid) } else { Vec::new() };
        with_registry(|m| {
            if let Some(e) = m.get_mut(&key) {
                e.pids = tree.clone();
            }
        });
        let cur_hwnd = with_registry(|m| m.get(&key).map(|e| e.hwnd));
        let Some(cur_hwnd) = cur_hwnd else { return };
        if unsafe { IsWindow(hwnd_from_isize(cur_hwnd)) }.as_bool() {
            continue; // Running
        }
        // 批次C-1 宽限一拍：WinEventHook 可能正把重建的新窗口重嵌进本会话
        std::thread::sleep(std::time::Duration::from_secs(2));
        let Some(e) = with_registry(|m| m.get(&key).map(|e| (e.hwnd, e.root_pid, e.host))) else {
            return;
        };
        if unsafe { IsWindow(hwnd_from_isize(e.0)) }.as_bool() {
            continue; // 已自动重嵌（C-1），继续监护
        }
        // 窗口消失 → 分类：进程树仍有存活者 = Orphaned
        let mut orphaned = false;
        for p in win::pid_tree(e.1) {
            if p == 0 {
                continue;
            }
            if let Ok(h) = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, p) } {
                let mut code = 0u32;
                unsafe {
                    let _ = GetExitCodeProcess(h, &mut code);
                    let _ = CloseHandle(h);
                }
                if code == 259 {
                    // STILL_ACTIVE
                    orphaned = true;
                    break;
                }
            }
        }
        // 会话移除（占位卡只展示一次；重新打开走新会话）
        with_registry(|m| {
            m.remove(&key);
        });
        // 第十二轮大检查：L2 宿主收场——第三方窗口先退出时，宿主空壳必须
        // 销毁。此前只有 embed_close 链路会销毁宿主，而本路径先移除了注册
        // 条目，前端随后的 embed_close 查不到会话，空壳宿主（WS_POPUP
        // 900×600）永久留在桌面。DestroyWindow 有线程亲和性（宿主在主线程
        // 创建）→ 必须经 run_on_main_thread；unwrap_child 清 CHILD_OF 映射
        // （子窗口已死时 SetParent 等调用无害失败）。
        if let Some(host) = e.2 {
            crate::shell::container::win::unwrap_child(host);
            let _ = app.run_on_main_thread(move || unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(
                    windows::Win32::Foundation::HWND(host as *mut core::ffi::c_void),
                );
            });
        }
        // 尽力采样根进程退出码（进程对象可能已被回收 → code 缺省）
        let mut code: Option<u32> = None;
        if !orphaned && e.1 != 0 {
            if let Ok(h) = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, e.1) } {
                let mut c = 0u32;
                unsafe {
                    let _ = GetExitCodeProcess(h, &mut c);
                    let _ = CloseHandle(h);
                }
                if c != 259 {
                    code = Some(c);
                }
            }
        }
        use tauri::Emitter;
        let _ = app.emit(
            "embed://state",
            serde_json::json!({
                "embedId": key,
                "state": if orphaned { "orphaned" } else { "exited" },
                "code": code,
            }),
        );
        return;
    });
}

// ---------- 批次C-1：WinEventHook 常驻监护（事件驱动，资源护栏主通道） ----------

/// AppHandle 全局引用（钩子线程发事件用；embed_launch 首次调用时登记）。
#[cfg(windows)]
static HOOK_APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// 已广播给前端、等待 embed_adopt 收编的窗口句柄（去抖：同一 hwnd 只广播一次）。
#[cfg(windows)]
static PENDING_ADOPT: std::sync::LazyLock<Mutex<std::collections::HashSet<isize>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

#[cfg(windows)]
fn pending_adopt_insert(h: isize) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;
    let mut g = PENDING_ADOPT.lock().unwrap_or_else(|e| e.into_inner());
    // 第十一轮大检查：清理已销毁的等待句柄——此前集合只增不减（弹窗未被
    // 收编就关闭时条目永久驻留），且 Windows 会回收句柄值，陈旧条目会
    // 静默压制未来同句柄值的合法弹窗广播。集合很小，IsWindow 便宜。
    g.retain(|&pending| unsafe { IsWindow(hwnd_from_isize(pending)) }.as_bool());
    g.insert(h)
}

#[cfg(windows)]
fn pending_adopt_remove(h: isize) {
    let mut g = PENDING_ADOPT.lock().unwrap_or_else(|e| e.into_inner());
    g.remove(&h);
}

/// 批次C-1：启动全局 WinEventHook 消息泵线程（幂等；只在首个嵌入会话时初始化）。
/// EVENT_OBJECT_SHOW 事件 → 过滤 pid 树 → ① 会话 hwnd 已死 = 窗口重建自动重嵌；
/// ② 同树新顶层主窗口（Chrome 设置页等）→ 广播 embed://popup 由前端开新占位窗收编。
#[cfg(windows)]
fn ensure_event_hook(app: &tauri::AppHandle) {
    use windows::Win32::Foundation::{HMODULE, HWND};
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, MSG, OBJID_WINDOW, TranslateMessage, WINEVENT_OUTOFCONTEXT,
    };

    let _ = HOOK_APP.set(app.clone());
    // 幂等：泵线程仅启动一次
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    const EVENT_OBJECT_SHOW: u32 = 0x8002;

    unsafe extern "system" fn on_show(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        id_object: i32,
        id_child: i32,
        _thread: u32,
        _time: u32,
    ) {
        // 只关心顶层主窗口对象（控件级 SHOW 直接丢弃——资源护栏）
        if id_object != OBJID_WINDOW.0 || id_child != 0 {
            return;
        }
        let h = hwnd.0 as isize;
        // 顶层 + 可见 + 有标题栏（主窗口特征；无标题栏自绘窗口属 L2/L3 路由，不在此收编）
        if !win::is_visible_top_level(hwnd) {
            return;
        }
        if !unsafe { has_caption(hwnd) } {
            return;
        }
        let Some(pid) = win::window_pid(hwnd) else { return };
        // 快速路径：pid 必须命中某个会话的 pid 树缓存，否则立即返回
        let hit = with_registry(|m| {
            let dead_session: Option<(String, String, bool)> = m
                .iter()
                .find(|(_, e)| e.hwnd != h && e.pids.contains(&pid))
                .map(|(k, e)| (k.clone(), e.tp_id.clone(), unsafe {
                    !windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd_from_isize(e.hwnd)).as_bool()
                }));
            dead_session
        });
        let Some((key, tp_id, was_dead)) = hit else { return };
        // 已登记句柄不重复收编；同 hwnd 去抖
        let already = with_registry(|m| m.values().any(|e| e.hwnd == h));
        if already || !pending_adopt_insert(h) {
            return;
        }
        let Some(app) = HOOK_APP.get() else { return };
        use tauri::Emitter;
        if was_dead {
            // ① 窗口重建：同会话原地重嵌（不占位、不换 embed_id）
            if reembed_into_session(&key, h) {
                pending_adopt_remove(h);
                let _ = app.emit(
                    "embed://state",
                    serde_json::json!({ "embedId": key, "state": "running", "code": null }),
                );
            }
        } else {
            // ② 同树新主窗口：广播 popup，前端开新占位窗后调 embed_adopt 收编
            let _ = app.emit(
                "embed://popup",
                serde_json::json!({ "origin": key, "tpId": tp_id, "hwnd": h, "rootPid": pid }),
            );
        }
    }

    unsafe fn has_caption(hwnd: HWND) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_STYLE, WS_CAPTION};
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        style & WS_CAPTION.0 != 0
    }

    std::thread::spawn(move || unsafe {
        // 进程内钩子 + 本线程消息泵（WINEVENT_OUTOFCONTEXT 要求 pump）
        let hook = SetWinEventHook(
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_SHOW,
            HMODULE::default(),
            Some(on_show),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
        if hook.is_invalid() {
            return; // 钩子不可用 → 兜底轮询（W-3 监护线程）继续工作
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

/// 批次C-1：剥边框 → 重父化为桌面子窗口 → 占位边界（embed_launch / 重嵌 / 收编共用）。
#[cfg(windows)]
fn restyle_and_reparent(new_hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetParent, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED,
        SWP_NOZORDER, WS_CAPTION, WS_CHILD, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };
    let Some(desktop) = HOOK_APP.get().and_then(desktop_hwnd) else { return false };
    unsafe {
        let h = hwnd_from_isize(new_hwnd);
        let style = GetWindowLongPtrW(h, GWL_STYLE) as isize;
        let new_style = ((style as u32)
            & !(WS_CAPTION.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0 | WS_SYSMENU.0))
            | WS_CHILD.0;
        SetWindowLongPtrW(h, GWL_STYLE, new_style as isize);
        SetParent(h, hwnd_from(desktop));
        SetWindowPos(h, HWND::default(), 240, 140, 900, 600, SWP_FRAMECHANGED | SWP_NOZORDER);
    }
    true
}

/// 批次C-1：把重建的新窗口重嵌进既有会话（剥边框 → SetParent → 更新 hwnd/DPI）。
#[cfg(windows)]
fn reembed_into_session(key: &str, new_hwnd: isize) -> bool {
    if !restyle_and_reparent(new_hwnd) {
        return false;
    }
    with_registry(|m| {
        match m.get_mut(key) {
            Some(e) => {
                e.hwnd = new_hwnd;
                e.last_dpi = win::window_dpi(new_hwnd);
                true
            }
            None => false,
        }
    })
}

/// 批次C-1：收编同进程树新弹出的主窗口为独立嵌入会话
/// （WinEventHook 广播 embed://popup → 前端开新占位窗后调用）。
/// 窗口必须仍然有效且未被登记；剥边框 → SetParent → 注册 → 监护。
#[tauri::command]
#[cfg(windows)]
pub async fn embed_adopt(
    _st: tauri::State<'_, crate::state::AppState>,
    app: tauri::AppHandle,
    tp_id: String,
    hwnd: isize,
    root_pid: u32,
    embed_id: String,
) -> CmdResult<bool> {
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;
    pending_adopt_remove(hwnd);
    if !unsafe { IsWindow(hwnd_from_isize(hwnd)) }.as_bool() {
        return Ok(false);
    }
    let already = with_registry(|m| m.values().any(|e| e.hwnd == hwnd) || m.contains_key(&embed_id));
    if already {
        return Ok(false);
    }
    let dpi_fix = crate::shell::launcher::registry_snapshot(&_st)
        .iter()
        .find(|a| a.id == tp_id)
        .map(|a| a.dpi_fix)
        .unwrap_or(false);
    if !restyle_and_reparent(hwnd) {
        return Ok(false);
    }
    with_registry(|m| {
        m.insert(
            embed_id.clone(),
            EmbedSession {
                hwnd,
                tp_id: tp_id.clone(),
                dpi_fix,
                last_dpi: win::window_dpi(hwnd),
                root_pid,
                pids: win::pid_tree(root_pid),
                host: None,
                capture: false,
            },
        );
    });
    ensure_event_hook(&app);
    spawn_session_watcher(app, embed_id, root_pid, hwnd);
    Ok(true)
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn embed_adopt(
    _st: tauri::State<'_, crate::state::AppState>,
    _app: tauri::AppHandle,
    _tp_id: String,
    _hwnd: isize,
    _root_pid: u32,
    _embed_id: String,
) -> CmdResult<bool> {
    Ok(false)
}

/// 脱离指定会话（恢复独立顶层窗口；应用不退出）。返回该会话是否存在。
#[cfg(windows)]
fn detach_by_id(embed_id: &str) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetParent, SetWindowLongPtrW, GWL_STYLE, WS_CHILD, WS_POPUP,
    };
    let Some(e) = with_registry(|map| map.remove(embed_id)) else {
        return false;
    };
    unsafe {
        let h = hwnd_from(e.hwnd);
        let style = GetWindowLongPtrW(h, GWL_STYLE) as isize;
        SetWindowLongPtrW(h, GWL_STYLE, ((style as u32 & !WS_CHILD.0) | WS_POPUP.0) as isize);
        SetParent(h, HWND::default());
    }
    true
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn embed_launch(
    _st: tauri::State<'_, crate::state::AppState>,
    _app: tauri::AppHandle,
    _id: String,
    _embed_id: Option<String>,
    _arg: Option<String>,
) -> CmdResult<EmbedResult> {
    Ok(EmbedResult { attached: false, reason: "仅 Windows 支持 / Windows only".into(), root_pid: None, capture: false })
}

/// 批次C-2：手动框选窗口（捕获失败占位卡的兜底动作）。
/// 先等当前按下的左键释放（去抖），再在 timeout_ms 内轮询左键按下；
/// 按下瞬间取光标下顶层根窗口（WindowFromPoint → GA_ROOT）。未选中返回 None。
#[tauri::command(async)]
#[cfg(windows)]
pub fn embed_pick_window(timeout_ms: Option<u64>) -> CmdResult<Option<isize>> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetAncestor, GetCursorPos, WindowFromPoint, GA_ROOT,
    };
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_millis(timeout_ms.unwrap_or(5_000).min(15_000));
    let pressed = || unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) & i16::MIN != 0 };
    // 阶段1：等本次 UI 点击释放（去抖，≤2s）
    let debounce = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while pressed() && std::time::Instant::now() < debounce {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // 阶段2：等待下一次左键按下 → 取光标下根窗口
    while std::time::Instant::now() < deadline {
        if pressed() {
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
                let w = WindowFromPoint(pt);
                let root = GetAncestor(w, GA_ROOT);
                return Ok(Some(root.0 as isize));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    Ok(None)
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn embed_pick_window(_timeout_ms: Option<u64>) -> CmdResult<Option<isize>> {
    Ok(None)
}

/// 更新嵌入窗口边界（物理像素；随虚拟窗口移动/缩放由前端按 embed_id 上报）。
/// 批次W-2：跨屏拖动时检测显示器实际 DPI 变化 → 转发 WM_DPICHANGED
/// （dpiFix 例外登记的应用跳过——按主屏渲染，如实标注轻微模糊）。
#[tauri::command(async)]
#[cfg(windows)]
pub fn embed_bounds(embed_id: Option<String>, x: i32, y: i32, w: i32, h: i32) -> CmdResult<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOZORDER};
    let key = norm_id(embed_id);
    let cur = with_registry(|map| map.get(&key).map(|e| (e.hwnd, e.dpi_fix, e.last_dpi)));
    let Some((hw, dpi_fix, last_dpi)) = cur else {
        return Ok(());
    };
    unsafe {
        SetWindowPos(hwnd_from(hw), HWND::default(), x, y, w.max(1), h.max(1), SWP_NOZORDER);
    }
    let new_dpi = win::window_dpi(hw);
    if !dpi_fix && last_dpi != 0 && new_dpi != last_dpi {
        win::forward_dpichanged(hw, new_dpi, last_dpi);
    }
    with_registry(|map| {
        if let Some(e) = map.get_mut(&key) {
            e.last_dpi = new_dpi;
        }
    });
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn embed_bounds(_embed_id: Option<String>, _x: i32, _y: i32, _w: i32, _h: i32) -> CmdResult<()> {
    Ok(())
}

/// 显示/隐藏嵌入窗口（最小化=隐藏，恢复=显示）。
#[tauri::command(async)]
#[cfg(windows)]
pub fn embed_visible(embed_id: Option<String>, visible: bool) -> CmdResult<()> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOW};
    let key = norm_id(embed_id);
    let target = with_registry(|map| {
        map.get(&key).map(|e| e.host.unwrap_or(e.hwnd))
    });
    if let Some(h) = target {
        unsafe {
            ShowWindow(hwnd_from(h), if visible { SW_SHOW } else { SW_HIDE });
        }
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn embed_visible(_embed_id: Option<String>, _visible: bool) -> CmdResult<()> {
    Ok(())
}

/// 关闭指定嵌入会话（WM_CLOSE，应用自行退出），并清除嵌入状态。
#[tauri::command(async)]
#[cfg(windows)]
pub fn embed_close(embed_id: Option<String>) -> CmdResult<()> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    let key = norm_id(embed_id);
    if let Some(e) = with_registry(|map| map.remove(&key)) {
        // 批次C-4：L3 会话先停采集并还原屏外窗口
        if e.capture {
            crate::shell::capture::win::stop_capture(&key);
            crate::shell::capture::win::restore_window(e.hwnd);
        }
        // 批次C-3：L2 会话向宿主发 WM_CLOSE（宿主转发子窗口并脱离自毁）
        let target = e.host.unwrap_or(e.hwnd);
        unsafe {
            PostMessageW(hwnd_from(target), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn embed_close(_embed_id: Option<String>) -> CmdResult<()> {
    Ok(())
}

/// W-1 退出会话：对全部嵌入会话发 WM_CLOSE（应用自行退出），随后由后台线程
/// 在 30s 内核对——仍未退出的窗口**脱离重父化留在桌面**（绝不强杀进程）。
/// 立即返回，不阻塞退出流程。
#[tauri::command(async)]
#[cfg(windows)]
pub fn embed_close_all(app: tauri::AppHandle) -> CmdResult<usize> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    let sessions = with_registry(|map| {
        map.drain().map(|(k, v)| (k, v)).collect::<Vec<_>>()
    });
    let n = sessions.len();
    for (key, e) in &sessions {
        // 批次C-4：L3 先停采集并还原屏外窗口（与 embed_close 同语义：采集线程
        // 立即停转；30s 超时未退的窗口归还桌面可见区，而非留在 -32000 屏外）
        if e.capture {
            crate::shell::capture::win::stop_capture(key);
            crate::shell::capture::win::restore_window(e.hwnd);
        }
        // 批次C-3：L2 会话发宿主 WM_CLOSE（转发链路：宿主→子窗口→脱离自毁）
        let target = e.host.unwrap_or(e.hwnd);
        unsafe {
            PostMessageW(hwnd_from(target), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
    // 30s 超时核对：仍在的窗口脱离回桌面（留在桌面，不终止进程）
    if !sessions.is_empty() {
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                // 仍在的窗口 = IsWindow 为真（已销毁 = 应用自行退出 → 无需处理）
                let remaining: Vec<(Option<isize>, isize)> = sessions
                    .iter()
                    .map(|(_, e)| (e.host, e.hwnd))
                    .filter(|(host, hwnd)| {
                        let t = host.unwrap_or(*hwnd);
                        unsafe {
                            windows::Win32::UI::WindowsAndMessaging::IsWindow(hwnd_from_isize(t))
                                .as_bool()
                        }
                    })
                    .collect();
                if remaining.is_empty() || std::time::Instant::now() >= deadline {
                    for (host, hwnd) in &remaining {
                        match host {
                            // L2：宿主仍存活 → 脱离子窗口并销毁宿主。
                            // 第十二轮大检查：DestroyWindow 有线程亲和性——
                            // 本线程不是宿主创建线程（主线程），直接调用会
                            // 静默失败、宿主照旧泄漏；必须经 run_on_main_thread。
                            Some(h) => {
                                crate::shell::container::win::unwrap_child(*h);
                                let h2 = *h;
                                let _ = app.run_on_main_thread(move || unsafe {
                                    let _ =
                                        windows::Win32::UI::WindowsAndMessaging::DestroyWindow(
                                            windows::Win32::Foundation::HWND(
                                                h2 as *mut core::ffi::c_void,
                                            ),
                                        );
                                });
                            }
                            None => detach_child(*hwnd),
                        }
                    }
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        });
    }
    Ok(n)
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn embed_close_all() -> CmdResult<usize> {
    Ok(0)
}

/// 把仍是 WS_CHILD 的窗口脱离回桌面（30s 超时兜底；不杀进程）。
#[cfg(windows)]
fn detach_child(hwnd: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, IsWindow, SetParent, SetWindowLongPtrW, GWL_STYLE, WS_CHILD, WS_POPUP,
    };
    let h = hwnd_from_isize(hwnd);
    if !unsafe { IsWindow(h) }.as_bool() {
        return;
    }
    unsafe {
        let style = GetWindowLongPtrW(h, GWL_STYLE) as isize;
        if style as u32 & WS_CHILD.0 != 0 {
            SetWindowLongPtrW(h, GWL_STYLE, ((style as u32 & !WS_CHILD.0) | WS_POPUP.0) as isize);
            SetParent(h, HWND::default());
        }
    }
}

/// 让指定嵌入窗口获得键盘焦点（点击/聚焦虚拟窗口时调用；W-1 焦点仲裁：
/// Z 序顶窗口 = 焦点移交对象）。
#[tauri::command(async)]
#[cfg(windows)]
pub fn embed_focus(embed_id: Option<String>) -> CmdResult<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    let key = norm_id(embed_id);
    let hwnd = with_registry(|map| map.get(&key).map(|e| e.hwnd));
    if let Some(h) = hwnd {
        unsafe {
            let _ = SetFocus(hwnd_from(h));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub fn embed_focus(_embed_id: Option<String>) -> CmdResult<()> {
    Ok(())
}

/// 批次C-4：L3 输入转发 —— 归一化坐标(0..1) + 事件 → 客户区物理坐标
/// PostMessage 直注真实窗口（屏外窗口天然不泄漏光标、不抢焦点）。
/// kind: move | down | up | dbl | wheel | key | char
#[tauri::command(async)]
#[cfg(windows)]
pub fn embed_input(
    embed_id: Option<String>,
    kind: String,
    x: f64,
    y: f64,
    button: Option<String>,
    key: Option<u32>,
    delta: Option<f64>,
) -> CmdResult<()> {
    let key_id = norm_id(embed_id);
    let hwnd = with_registry(|map| map.get(&key_id).map(|e| e.hwnd));
    if let Some(h) = hwnd {
        let _ = crate::shell::capture::win::forward_input(
            h,
            &kind,
            x,
            y,
            button.as_deref().unwrap_or("left"),
            key.unwrap_or(0),
            delta.unwrap_or(0.0),
        );
    }
    Ok(())
}

#[tauri::command(async)]
#[cfg(not(windows))]
pub fn embed_input(
    _embed_id: Option<String>,
    _kind: String,
    _x: f64,
    _y: f64,
    _button: Option<String>,
    _key: Option<u32>,
    _delta: Option<f64>,
) -> CmdResult<()> {
    Ok(())
}

/// 当前全部嵌入会话的子窗口句柄（privacy_shield 防截屏打标用；W-1 多嵌入并发）。
#[cfg(windows)]
pub fn current_embed_hwnds() -> Vec<isize> {
    with_registry(|map| map.values().map(|e| e.hwnd).collect())
}

/// D-3 看门狗用：枚举全部顶层可见窗口 → (hwnd, pid, 进程完整映像路径)。
/// 复用既有 EnumWindows 通道（collect_handles），只补 pid 与映像路径。
#[cfg(windows)]
pub fn watch_scan_windows() -> Vec<(isize, u32, String)> {
    let mut out: Vec<(isize, u32, String)> = Vec::new();
    win::collect_handles(&mut |h, _| {
        let pid = win::window_pid(hwnd_from_isize(h)).unwrap_or(0);
        let image = if pid > 4 {
            win::process_image(pid).unwrap_or_default()
        } else {
            String::new()
        };
        out.push((h, pid, image));
        true
    });
    out
}

// ---------- Steam 主动收编（实机需求：从 Variable 启动 = Steam 收进 Variable 运行） ----------
//
// steam_launch（ecosystem.rs）之后启动本看护：轮询查找 Steam 家族
// （steam.exe / steamwebhelper.exe）的可见顶层主窗口 → emit `embed://popup`
// → 前端开 VWM 占位窗 → embed_adopt 重父化收编。Steam 已嵌入（主窗已是
// WS_CHILD，不出现在顶层枚举里）→ 扫不到候选，静默退出；之后游戏窗口由
// WinEventHook 同树 popup 通道自动收编（会话 pid 树含游戏进程）。
// 90s 超时（Steam 冷启动/登录中）→ 退出，逃逸窗口由 D-3 看门狗兜底。

/// Steam 家族进程映像名（basename 小写匹配）。
const STEAM_IMAGES: &[&str] = &["steam.exe", "steamwebhelper.exe"];

/// 纯函数：候选顶层窗口（hwnd, pid, 完整映像路径）里挑 Steam 主窗。
/// 规则：basename ∈ Steam 家族（大小写/路径分隔符不敏感）；嵌入式主窗
/// 本就不在顶层枚举里（WS_CHILD），无需额外排除。
pub fn pick_steam_window(cands: &[(isize, u32, String)]) -> Option<(isize, u32)> {
    for (h, pid, img) in cands {
        let name = img.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
        if STEAM_IMAGES.contains(&name.as_str()) {
            return Some((*h, *pid));
        }
    }
    None
}

/// Steam 会话 root pid：从窗口归属进程沿父链上行找 steam.exe
/// （主窗属 steamwebhelper，游戏进程是 steam.exe 后代——root 必须锚在
/// steam.exe 上，pid 树才能同时覆盖二者）。找不到（异常谱系）回落原 pid。
#[cfg(windows)]
fn steam_root_pid(pid: u32) -> u32 {
    use std::collections::HashMap;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return pid;
    };
    let mut rows: HashMap<u32, (u32, String)> = HashMap::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snap, &mut entry) }.is_ok() {
        loop {
            let name = String::from_utf16_lossy(&entry.szExeFile)
                .trim_end_matches('\0')
                .to_lowercase();
            rows.insert(entry.th32ProcessID, (entry.th32ParentProcessID, name));
            if unsafe { Process32NextW(snap, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { windows::Win32::Foundation::CloseHandle(snap) };
    let mut p = pid;
    for _ in 0..16 {
        // 防环：父链回到自身/查无此行即止
        match rows.get(&p) {
            Some((_, name)) if name == "steam.exe" => return p,
            Some((ppid, _)) if *ppid != 0 && *ppid != p => p = *ppid,
            _ => break,
        }
    }
    pid
}

/// 主窗特征过滤：带标题栏（CEF 无边框工具窗/气泡不收编，与 WinEventHook
/// on_show 的 has_caption 口径一致）。
#[cfg(windows)]
fn has_caption_style(hwnd: isize) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_STYLE, WS_CAPTION};
    let style = unsafe { GetWindowLongPtrW(hwnd_from_isize(hwnd), GWL_STYLE) } as u32;
    style & WS_CAPTION.0 != 0
}

/// steam_launch 后启动的 Steam 主窗看护（见模块注释）。幂等安全：每次
/// steam_launch 一个看护线程；已嵌入时扫不到候选，90s 后自然退出。
#[cfg(windows)]
pub fn spawn_steam_adopt_watcher(app: tauri::AppHandle) {
    use tauri::Emitter;
    std::thread::Builder::new()
        .name("steam-adopt".into())
        .spawn(move || {
            // ShellExecute 异步拉起 Steam：先等一拍再开扫
            std::thread::sleep(std::time::Duration::from_millis(1500));
            for _ in 0..90 {
                let cands: Vec<(isize, u32, String)> = watch_scan_windows()
                    .into_iter()
                    .filter(|(h, _, _)| has_caption_style(*h))
                    .collect();
                if let Some((hwnd, pid)) = pick_steam_window(&cands) {
                    let root = steam_root_pid(pid);
                    eprintln!(
                        "[steam-adopt] Steam main window found hwnd={hwnd} pid={pid} root={root} -> popup"
                    );
                    let _ = app.emit(
                        "embed://popup",
                        serde_json::json!({
                            "origin": "steam-launch",
                            "tpId": "steam",
                            "hwnd": hwnd,
                            "rootPid": root,
                        }),
                    );
                    return;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            eprintln!(
                "[steam-adopt] no Steam main window in 90s (cold start / login?) — watchdog remains as fallback"
            );
        })
        .ok();
}

#[cfg(not(windows))]
pub fn spawn_steam_adopt_watcher(_app: tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::{with_registry, norm_id};

    /// W-1：注册中心语义——多槽位并发、同槽位替换、按 id 移除、旧入口 "0" 兼容。
    #[test]
    fn registry_multi_embed_semantics() {
        // 旧单嵌入口缺省映射 "0"
        assert_eq!(norm_id(None), "0");
        assert_eq!(norm_id(Some("abc".into())), "abc");

        // 两个并发槽位互不干扰
        with_registry(|m| {
            m.insert("0".into(), super::EmbedSession { hwnd: 111, tp_id: "a".into(), dpi_fix: false, last_dpi: 0, root_pid: 0, pids: Vec::new(), host: None, capture: false });
            m.insert("vwm-tp-x1".into(), super::EmbedSession { hwnd: 222, tp_id: "b".into(), dpi_fix: false, last_dpi: 0, root_pid: 0, pids: Vec::new(), host: None, capture: false });
        });
        let (h0, h1) = with_registry(|m| {
            (m.get("0").map(|e| e.hwnd), m.get("vwm-tp-x1").map(|e| e.hwnd))
        });
        assert_eq!(h0, Some(111));
        assert_eq!(h1, Some(222));

        // 同槽位重嵌 = 替换（不留旧会话）
        with_registry(|m| {
            m.insert(
                "vwm-tp-x1".into(),
                super::EmbedSession { hwnd: 333, tp_id: "b".into(), dpi_fix: true, last_dpi: 144, root_pid: 0, pids: Vec::new(), host: None, capture: false },
            );
        });
        let n = with_registry(|m| m.len());
        assert_eq!(n, 2);

        // 按 id 移除（embed_close 语义）
        let removed = with_registry(|m| m.remove("vwm-tp-x1").map(|e| e.hwnd));
        assert_eq!(removed, Some(333));
        let gone = with_registry(|m| m.contains_key("vwm-tp-x1"));
        assert!(!gone);

        // 收尾清场
        with_registry(|m| m.clear());
    }

    /// Steam 主动收编：家族窗口匹配（basename 小写 + 任意路径/盘符），
    /// 非家族窗口不命中，空候选返回 None。
    #[test]
    fn pick_steam_window_matches_family_only() {
        let cands = vec![
            (101, 10, "C:\\Windows\\explorer.exe".into()),
            (102, 20, "C:\\Program Files (x86)\\Steam\\steamwebhelper.exe".into()),
            (103, 30, "D:\\Games\\SomeGame\\game.exe".into()),
        ];
        assert_eq!(super::pick_steam_window(&cands), Some((102, 20)));

        // 大写盘符路径 + steam.exe 本体
        let cands2 = vec![(201, 40, "E:\\Steam\\STEAM.EXE".into())];
        assert_eq!(super::pick_steam_window(&cands2), Some((201, 40)));

        // 正斜杠路径（shell_execute 语义兼容）与无家族窗口
        let none = vec![
            (301, 50, "C:/Windows/System32/notepad.exe".into()),
            (302, 51, String::new()),
        ];
        assert_eq!(super::pick_steam_window(&none), None);
        assert_eq!(super::pick_steam_window(&[]), None);
    }
}
