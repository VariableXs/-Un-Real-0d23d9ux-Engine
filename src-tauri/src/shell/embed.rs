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

/// R5 隔离底线：把桌面窗口重新提到 always_on_top（tp_launch 启动第三方时
/// 会撤销置顶让回退独立窗浮出；嵌入成功后必须收回，否则 Windows 任务栏/
/// shell 会在兼容期露出）。幂等，失败静默（无桌面窗时无意义）。
#[cfg(windows)]
fn reassert_desktop_topmost(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("desktop") {
        let _ = w.set_always_on_top(true);
    }
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EmbedResult {    /// 是否成功嵌入（false = 已回退为独立窗口运行）
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
    /// 单例启动器：接受**启动前就存在**的主窗（缺省按 exe 家族自动判定）。
    #[serde(default)]
    adopt_existing: Option<bool>,
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
pub(crate) mod win {
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
    /// 保留为无清单直跑档的兜底入口；环境内主路径由 tauri.conf 清单声明承担。
    #[allow(dead_code)]
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
        collect_handles_ex(keep, false)
    }

    /// collect_handles 变体：`include_hidden=true` 时不过滤 IsWindowVisible。
    /// R3-B11 单例重开探测 / 兜底收编必须用它——Steam 关闭走 WM_CLOSE 会
    /// 退托盘隐藏主窗（IsWindowVisible=false 但进程存活），按可见性过滤
    /// 会让「单例重开」探测与兜底候选双双落空（60s 空转 / 兜底无候选）。
    pub fn collect_handles_ex(
        keep: &mut dyn FnMut(isize, &str) -> bool,
        include_hidden: bool,
    ) -> Vec<isize> {
        struct Ctx<'a> {
            keep: &'a mut dyn FnMut(isize, &str) -> bool,
            out: Vec<isize>,
            include_hidden: bool,
        }
        unsafe extern "system" fn probe(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let ctx = unsafe { &mut *(lparam.0 as *mut Ctx) };
            unsafe {
                if !ctx.include_hidden && !IsWindowVisible(hwnd).as_bool() {
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
        let mut ctx = Ctx { keep, out: Vec::new(), include_hidden };
        unsafe {
            let _ = EnumWindows(Some(probe), LPARAM(&mut ctx as *mut Ctx as isize));
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
    /// `singleton_relaunch`（单例重开 fast-path）：启动前家族已有带标题栏主窗
    /// 时置 true —— 启动进程树已空（单例转发进程退出，如 steam.exe 二次启动
    /// 转交旧实例后即退）即提前返回 None，让调用方立即走兜底收编既有主窗，
    /// 不再空转满超时。短暂空窗期（前 ~600ms）不早退，避开启动瞬时抖动。
    pub fn wait_new_window(
        exe_name: &str,
        root_pid: Option<u32>,
        before: &[isize],
        title_regex: Option<&str>,
        timeout_ms: u64,
        singleton_relaunch: bool,
    ) -> Option<isize> {
        let suffix = exe_name.to_lowercase();
        let re = title_regex.and_then(|p| regex::Regex::new(p).ok());
        let steps = ((timeout_ms / 200).max(1)) as usize;
        for step in 0..steps {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let tree: Vec<u32> = root_pid.map(|r| pid_tree(r)).unwrap_or_default();
            // 单例重开：转发进程已退出（树空）→ 不可能有"新"窗口了，立即兜底
            if singleton_relaunch
                && root_pid.is_some()
                && step >= 3
                && tree.is_empty()
            {
                return None;
            }
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

/// 单例/家族应用映像族（basename 小写）：登记 exe + 已知伴生进程映像。
/// Steam：主窗属 steamwebhelper.exe（CEF），按 exe 名匹配必须带上家族，
/// 否则单例二次启动（无新进程树）时永远"未能捕获"。
#[cfg(windows)]
fn family_images(exe_name: &str) -> Vec<String> {
    let lower = exe_name.to_lowercase();
    let mut out = vec![lower.clone()];
    match lower.as_str() {
        "steam.exe" => out.push("steamwebhelper.exe".into()),
        "steamwebhelper.exe" => out.push("steam.exe".into()),
        // Wallpaper Engine 2.8+：UI 主窗属独立进程 wallpaperui.exe（CEF），
        // 登记/启动项通常指向 wallpaper64/32.exe —— 按 exe 名匹配必须带上
        // 家族，否则 UI 窗口 pid 树匹配落空 → embed-fail / 永久逃逸（实机）。
        "wallpaper64.exe" | "wallpaper32.exe" => out.push("wallpaperui.exe".into()),
        "wallpaperui.exe" => {
            out.push("wallpaper64.exe".into());
            out.push("wallpaper32.exe".into());
        }
        _ => {}
    }
    out
}

/// 启动第三方应用并把它的主窗口嵌入 Variable 桌面窗口（环境内打开）。
/// `embed_id` = 前端 VWM 虚拟窗口实例 id（占位窗口创建时分配；缺省 "0" 兼容旧单嵌）。
/// 无法嵌入时如实返回 attached=false（应用已按独立窗口方式启动）。
#[tauri::command(async)]
#[cfg(windows)]
pub async fn embed_launch(
    st: tauri::State<'_, crate::state::AppState>,
    app: tauri::AppHandle,
    id: String,
    embed_id: Option<String>,
    arg: Option<String>,
) -> CmdResult<EmbedResult> {
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
    // R4-B2 修复：登记项/桌面图标为 Steam 快捷方式（.url 内容指向 steam://）
    // 时，绝不能走 ShellExecute 通道 —— Windows 会按协议关联从宿主侧拉起
    // steam.exe，root_pid 与窗口匹配（exe 名是 xxx.url）全部落空，收编看护
    // 永不触发。改为转交 steam_open_url（CEF 兼容态 + 收编看护），前端收到
    // "steam:handoff" 后关闭本占位窗，由 embed://popup 流程开真正的占位窗。
    if target.to_lowercase().ends_with(".url") {
        if let Some(url) = crate::system::steam_probe(&tp.target.clone().unwrap_or_else(|| tp.path.clone())) {
            crate::shell::ecosystem::steam_open_url(&app, &url)?;
            return Ok(EmbedResult {
                attached: false,
                reason: "steam:handoff".into(),
                root_pid: None,
                ..EmbedResult::default()
            });
        }
    }
    let exe_name = std::path::Path::new(&target)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| AppError::validation("登记项路径无效 / invalid path"))?;
    // 通道③兜底：Steam 家族 embed_launch 未接入时，强制拉起主动收编看护
    // （steam-adopt 线程找主窗 → embed://popup → 前端开占位窗收编），
    // 杜绝"进程起来了但滞留宿主桌面/托盘"的逃逸态。
    let steam_family = matches!(exe_name.as_str(), "steam.exe" | "steamwebhelper.exe");

    // 2) 记录启动前已存在的该应用窗口（避免把旧窗口误嵌）
    let before = win::collect_handles(&mut |_h, img| img.to_lowercase().ends_with(&exe_name));

    // 2b) 单例 fast-path 探测（修复「Steam 二次启动卡 60s / 打不开」）：
    //     分离后 / 托盘退出后再点图标时，旧实例仍存活，新 steam.exe 只把
    //     参数转交旧实例便退出——主窗属旧实例的 steamwebhelper.exe，既不
    //     在 before（按登记 exe 过滤），映像名也不以 steam.exe 结尾，
    //     wait_new_window 必然空转满超时才走兜底。探测「启动前家族已有
    //     带标题栏主窗」即判定为单例重开：本次等待压到 8s，并允许
    //     wait_new_window 在启动进程树已空（转发进程已退出）时提前返回，
    //     立即走兜底收编既有主窗（秒级）。冷启动（无既有窗）行为不变。
    //     R3-B11 补充：探测必须包含隐藏窗（include_hidden）——红钮 WM_CLOSE
    //     后 Steam 退托盘隐藏主窗，且分离还原前窗口可能仍在 -32000 屏外，
    //     两者 IsWindowVisible 都是 false，按可见性过滤会让探测恒空
    //     （第二次启动仍 单例重开=false 空转 60s 的根因）。
    let family_imgs = family_images(&exe_name);
    let fam_before = win::collect_handles_ex(&mut |h, img| {
        let name = img.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
        family_imgs.iter().any(|m| *m == name) && has_caption_style(h)
    }, true);
    let singleton_relaunch = !fam_before.is_empty();

    // 3) 启动（复用既有通道；本模块不含进程创建代码）。root_pid 用于
    //    按子进程树匹配窗口——启动器型软件（如 Wallpaper Engine）真正的
    //    主窗口属于它派生的子进程，按 exe 名匹配不到。
    let root_pid = crate::shell::launcher::tp_launch_inner(&st, &app, id.clone(), arg.as_deref())?;

    // 4) 等待新主窗口（批次C-2 自适应超时 + 标题正则兜底；超时后应用保持
    //    独立窗口运行，不终止进程，并落地失败证据包）
    let Some(_desktop) = desktop_hwnd(&app) else {
        return Err(AppError::io("桌面窗口不存在 / no desktop window"));
    };
    let mut hints = load_hints(&st);
    let mut hint = hints.get(&id).cloned().unwrap_or_default();
    let timeout_ms = hint.capture_timeout_ms.unwrap_or(30_000);
    // 单例重开：等待上限压到 8s（不改 hints 自适应，只影响本次）
    let wait_ms = if singleton_relaunch { timeout_ms.min(8_000) } else { timeout_ms };
    let started = std::time::Instant::now();
    crate::shell::applog::log(
        "launch",
        format!(
            "embed_launch {id}: 已启动 {exe_name} root_pid={root_pid:?}，等待主窗（最长 {wait_ms}ms，单例重开={singleton_relaunch}）"
        ),
    );
    // 等窗口轮询（最长 30s）是纯阻塞操作：移入阻塞线程池执行，
    // 避免占死 tokio async worker 拖慢其它 IPC。
    let exe_c = exe_name.clone();
    let title_c = hint.title_regex.clone();
    let hwnd = tauri::async_runtime::spawn_blocking(move || {
        win::wait_new_window(
            &exe_c,
            root_pid,
            &before,
            title_c.as_deref(),
            wait_ms,
            singleton_relaunch,
        )
    })
    .await
    .map_err(|e| AppError::io(format!("等待窗口线程异常 / wait thread error: {e}")))?;
    let Some(hwnd) = hwnd
    else {
        let elapsed = started.elapsed().as_millis() as u64;
        crate::shell::applog::log(
            "launch",
            format!("embed_launch {id}: 等待主窗超时（{elapsed}ms）→ 走兜底收编（root_pid={root_pid:?}）"),
        );
        write_fail_evidence(&st, &id, root_pid.unwrap_or(0), &exe_name);
        // 自适应：超时 → 下次放宽到 60s（快启动命中后由下方收紧）。
        // 修复：此前只在 capture_timeout_ms 未设置时放宽 —— 一旦被快启动
        // 收紧成 Some(5000)，慢启动应用（Steam 冷启动 >5s）每次必然超时，
        // 且永不恢复（连续 embed-fail 的根因）。现在只要当前窗口 <60s 就放宽。
        if hint.capture_timeout_ms.unwrap_or(30_000) < 60_000 {
            hint.capture_timeout_ms = Some(60_000);
            hints.insert(id.clone(), hint);
            save_hints(&st, &hints);
        }
        // 实机需求（兜底收编）：应用窗口没按"标准主窗"出现也要进 Variable ——
        // 三档候选（优先级递降）：
        //   1) 同家族可见带标题栏主窗（Steam/微信等单例二次启动只唤起既有实例；
        //      Steam 家族含 steamwebhelper.exe —— 主窗属它，按 exe 名匹配不到）；
        //   2) 启动进程树内任意可见窗口（冷启动主窗迟迟不带标题栏/非常规框架）；
        //   3) 同家族任意可见窗口（CEF 无边框窗）。
        // 命中即收编（attach_by_tier 同一裁判：CEF 合成管道照走 L3 采画面），
        // 绝不把应用丢回 Windows 桌面。
        let family = family_images(&exe_name);
        let family_pair = family.len() > 1;
        let tree = match root_pid {
            Some(r) if r != 0 => win::pid_tree(r),
            _ => Vec::new(),
        };
        let tree_c = tree.clone();
        let fam_c = family.clone();
        let buckets = tauri::async_runtime::spawn_blocking(move || {
            let mut fam_cap: Vec<isize> = Vec::new();
            let mut tree_win: Vec<isize> = Vec::new();
            let mut fam_any: Vec<isize> = Vec::new();
            // R3-B11：与 fam_before 同口径 —— include_hidden（托盘隐藏 /
            // 屏外的家族主窗也是合法收编对象，attach 前有还原防黑帧兜底）
            win::collect_handles_ex(&mut |h, img| {
                let name = img.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase();
                let is_fam = fam_c.iter().any(|m| *m == name);
                let in_tree = !tree_c.is_empty() && {
                    win::window_pid(hwnd_from_isize(h)).map(|p| tree_c.contains(&p)).unwrap_or(false)
                };
                if is_fam {
                    if has_caption_style(h) {
                        fam_cap.push(h);
                    } else {
                        fam_any.push(h);
                    }
                }
                if in_tree {
                    tree_win.push(h);
                }
                false
            }, true);
            (fam_cap, tree_win, fam_any)
        })
        .await
        .unwrap_or((Vec::new(), Vec::new(), Vec::new()));
        let (fam_cap, tree_win, fam_any) = buckets;
        crate::shell::applog::log(
            "launch",
            format!(
                "embed_launch {id} 兜底候选: 家族带标题栏={:?} 树内窗口={:?} 家族其它={:?}",
                fam_cap, tree_win, fam_any
            ),
        );
        if let Some(&old_hwnd) = fam_cap.first().or_else(|| tree_win.first()).or_else(|| fam_any.first()) {
            let pid = win::window_pid(hwnd_from_isize(old_hwnd)).unwrap_or(0);
            // Steam 家族 root 必须锚在 steam.exe（主窗属 steamwebhelper，
            // 游戏进程是其后代——pid 树监护才能同时覆盖）
            let anchor = if family_pair { steam_root_pid(pid) } else { pid };
            crate::shell::applog::log(
                "launch",
                format!("embed_launch {id}: 兜底收编既有窗口 hwnd={old_hwnd} pid={pid} root={anchor}"),
            );
            return match attach_by_tier(
                &app,
                &st,
                key,
                id.clone(),
                old_hwnd,
                anchor,
                tp.dpi_fix,
                Some(&target),
            ) {
                Attach::Ok { capture } => Ok(EmbedResult {
                    attached: true,
                    reason: String::new(),
                    root_pid,
                    capture,
                }),
                Attach::Skip { reason } => {
                    crate::shell::applog::log("launch", format!("embed_launch {id}: 兜底收编被拒：{reason}"));
                    if steam_family { spawn_steam_adopt_watcher(app.clone()); }
                    Ok(EmbedResult { attached: false, reason, root_pid, capture: false })
                }
            };
        }
        crate::shell::applog::log(
            "launch",
            format!("embed_launch {id}: 兜底无候选 → 如实回退独立窗口（应用继续运行，可占位卡「框选窗口」收编）"),
        );
        if steam_family { spawn_steam_adopt_watcher(app.clone()); }
        return Ok(EmbedResult {
            attached: false,
            reason: "未能捕获应用窗口（启动较慢或无标准窗口）。应用已在系统桌面独立运行，未受影响；可在占位卡上「框选窗口」手动收编。".into(),
            root_pid,
            capture: false,
        });
    };
    // 自适应：快启动（<5s 命中）→ 下次收紧到 5s，减少慢启动错觉等待
    let elapsed = started.elapsed().as_millis() as u64;
    crate::shell::applog::log(
        "launch",
        format!("embed_launch {id}: 命中主窗 hwnd={hwnd}（耗时 {elapsed}ms，超时窗 {timeout_ms}ms）"),
    );
    if elapsed < 5_000 && hint.capture_timeout_ms.unwrap_or(30_000) != 5_000 {
        hint.capture_timeout_ms = Some(5_000);
        hints.insert(id.clone(), hint);
        save_hints(&st, &hints);
    }

    // 5) 分级接入（L1 重父化 / L2 容器包裹 / L3 画面捕获）——
    //    与 embed_adopt（WinEventHook 弹窗 / Steam 主动收编）共用同一裁判
    //    attach_by_tier，杜绝两条入口口径分裂（Steam 客户端纯黑窗口的根因）。
    match attach_by_tier(
        &app,
        &st,
        key,
        id.clone(),
        hwnd,
        root_pid.unwrap_or(0),
        tp.dpi_fix,
        Some(&target),
    ) {
        Attach::Ok { capture } => {
            crate::shell::applog::log(
                "embed",
                format!("embed_launch {id}: 接入成功 hwnd={hwnd}（capture={capture}）"),
            );
            // R5 隔离底线：嵌入成功后重新断言桌面置顶（tp_launch 撤销过），
            // 嵌入窗已成为桌面子窗，独立 shell 界面不得重新露出。
            reassert_desktop_topmost(&app);
            Ok(EmbedResult { attached: true, reason: String::new(), root_pid, capture })
        }
        Attach::Skip { reason } => {
            crate::shell::applog::log("embed", format!("embed_launch {id}: 未接入：{reason}"));
            if steam_family { spawn_steam_adopt_watcher(app.clone()); }
            Ok(EmbedResult { attached: false, reason, root_pid, capture: false })
        }
    }
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
        // R5 隔离轮补：orphaned（首窗被应用自毁重建、进程树仍存活）时，
        // 会话已移除 → WinEventHook 重嵌通道（需注册会话）失效，Steam 冷启动
        // 重建的主窗将永远逃逸（r5 实机复现）。派生重收看护兜底。
        if orphaned {
            spawn_readopt_watcher(app.clone(), e.1, key.clone());
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

/// R5：orphaned 后的重收看护——轮询同 root 进程树的新可见主窗（带标题栏），
/// 命中即广播 `embed://popup` 走前端既有收编流程（开占位窗 → embed_adopt）。
/// 45s 未现（应用彻底退出/纯托盘化）→ 交给 D-3 看门狗，线程自然退出。
#[cfg(windows)]
fn spawn_readopt_watcher(app: tauri::AppHandle, root_pid: u32, tp_label: String) {
    use tauri::Emitter;
    std::thread::Builder::new()
        .name("embed-readopt".into())
        .spawn(move || {
            for _ in 0..45 {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let tree: std::collections::HashSet<u32> =
                    win::pid_tree(root_pid).into_iter().collect();
                if tree.is_empty() {
                    return; // 进程树已消失 → 应用真退出，无需重收
                }
                for (h, pid, _img) in watch_scan_windows() {
                    if tree.contains(&pid) && has_caption_style(h) {
                        crate::shell::applog::log(
                            "embed",
                            format!("readopt {tp_label}: 重收重建主窗 hwnd={h} → 广播 embed://popup"),
                        );
                        let _ = app.emit(
                            "embed://popup",
                            serde_json::json!({
                                "origin": "readopt",
                                "tpId": tp_label,
                                "hwnd": h,
                                "rootPid": root_pid,
                            }),
                        );
                        return;
                    }
                }
            }
            crate::shell::applog::log(
                "embed",
                format!("readopt {tp_label}: 45s 未现新主窗 → 交给 D-3 看门狗兜底"),
            );
        })
        .ok();
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
            if reembed_into_session(app, &key, h) {
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

/// 批次C-1：剥边框 → 重父化为桌面子窗口 → 占位边界（L1 路径；重嵌/收编共用）。
/// 桌面句柄经 `app` 现取（HoOK_APP 是进程级 OnceLock，收编路径首调时可能尚未登记
/// —— 依赖它会让「首个会话来自弹窗收编」的链路静默失败）。
#[cfg(windows)]
fn restyle_and_reparent(app: &tauri::AppHandle, new_hwnd: isize) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetParent, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED,
        SWP_NOZORDER, WS_CAPTION, WS_CHILD, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };
    let Some(desktop) = desktop_hwnd(app) else { return false };
    unsafe {
        let h = hwnd_from_isize(new_hwnd);
        let style = GetWindowLongPtrW(h, GWL_STYLE) as isize;
        let new_style = ((style as u32)
            & !(WS_CAPTION.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0 | WS_SYSMENU.0))
            | WS_CHILD.0;
        SetWindowLongPtrW(h, GWL_STYLE, new_style as isize);
        let _ = SetParent(h, hwnd_from(desktop));
        let _ = SetWindowPos(h, HWND::default(), 240, 140, 900, 600, SWP_FRAMECHANGED | SWP_NOZORDER);
    }
    true
}

/// 批次C-1：把重建的新窗口重嵌进既有会话（剥边框 → SetParent → 更新 hwnd/DPI）。
#[cfg(windows)]
fn reembed_into_session(app: &tauri::AppHandle, key: &str, new_hwnd: isize) -> bool {
    if !restyle_and_reparent(app, new_hwnd) {
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

/// 分层接入结果。
#[cfg(windows)]
enum Attach {
    /// 已接入 Variable（capture = true 表示 L3 画面捕获会话）。
    Ok { capture: bool },
    /// 该层级不接入 → 调用方按「独立窗口运行」如实处理并回传 reason。
    Skip { reason: String },
}

/// 按兼容层级把**已定位的**原生窗口接入 Variable —— `embed_launch` 与
/// `embed_adopt` 共用同一裁判。
///
/// 这是 Steam 黑屏的根因修复：`embed_adopt`（WinEventHook popup / Steam 主动收编
/// 走的那条）此前**无条件**走 L1 重父化，绕过了 compat_probe。Steam 客户端主窗属
/// steamwebhelper.exe，是 CEF（DirectComposition，无 DWM 重定向表面）窗口 ——
/// SetParent 之后父窗口客户区里没有任何可合成位图，于是呈现为一大片纯黑且不吃
/// 输入。现在两条入口共用同一分级：合成管道窗口一律走 L3（采画面 + 转发输入）。
///
/// 绝不强杀进程：Skip 只表示「不接入」，应用继续以独立窗口运行。
#[cfg(windows)]
fn attach_by_tier(
    app: &tauri::AppHandle,
    st: &crate::state::AppState,
    key: String,
    tp_id: String,
    hwnd: isize,
    root_pid: u32,
    dpi_fix: bool,
    target: Option<&str>,
) -> Attach {
    use crate::shell::compat_probe::CompatTier;

    // 批次C-6：分级探测（改样式前采样；结果持久化 apps.json，用户覆盖最高优先）
    let compat = crate::shell::compat_probe::probe_and_persist(st, &tp_id, hwnd, target);
    crate::shell::applog::log(
        "embed",
        format!(
            "attach {tp_id}: hwnd={hwnd} root_pid={root_pid} → 层级 {}（{}）",
            compat.effective().as_str(),
            compat.evidence.get("reason").and_then(|v| v.as_str()).unwrap_or("?")
        ),
    );

    // 登记会话 + 启动监护（三种层级共用的收尾动作）
    let register = |host: Option<isize>, capture: bool| {
        with_registry(|map| {
            map.insert(
                key.clone(),
                EmbedSession {
                    hwnd,
                    tp_id: tp_id.clone(),
                    dpi_fix,
                    last_dpi: win::window_dpi(hwnd),
                    root_pid,
                    pids: if root_pid != 0 { win::pid_tree(root_pid) } else { Vec::new() },
                    host,
                    capture,
                },
            );
        });
        // 批次C-1：确保 WinEventHook 常驻监护已启动（首个嵌入会话时初始化，全局一份）
        ensure_event_hook(app);
        // 批次W-3 长期监护：每 2s 核对该会话窗口存活（事件线程，随会话结束退出）
        spawn_session_watcher(app.clone(), key.clone(), root_pid, hwnd);
    };

    match compat.effective() {
        // Native = 独立窗口如实降级
        CompatTier::Native => Attach::Skip {
            reason: format!(
                "「{}」判定为 Native 层级（{}）→ 保持独立窗口运行。",
                tp_id,
                compat.evidence.get("reason").and_then(|v| v.as_str()).unwrap_or("?")
            ),
        },
        // 批次C-5：L4 智能让位——绝不嵌入（反作弊误判 / 独占全屏重排会让渲染契约崩）
        CompatTier::L4 => {
            let hint = compat.hint.as_deref().unwrap_or("fullscreen");
            Attach::Skip {
                reason: format!(
                    "「{}」判定为 L4 层级（让位归因：{}）→ 保持独立窗口运行，Variable 桌面层将智能让位。",
                    tp_id, hint
                ),
            }
        }
        // 批次C-3 L2 容器包裹：自绘/非标框架窗口不剥样式，包进 Variable 原生宿主窗口
        CompatTier::L2 => {
            let Some(host) = crate::shell::container::win::create_host_on_main_thread(app) else {
                return Attach::Skip {
                    reason: "容器宿主窗口创建失败（L2 包裹不可用）。应用保持独立窗口运行。".into(),
                };
            };
            // 第十四轮大检查：命令已 async 化（线程池运行），宿主窗口消息泵在主线程
            // → wrap 的 SetParent/尺寸同步必须经主线程调度。
            if !crate::shell::container::win::wrap_child_on_main_thread(app, host, hwnd) {
                return Attach::Skip { reason: "容器包裹失败（L2）。应用保持独立窗口运行。".into() };
            }
            register(Some(host), false);
            Attach::Ok { capture: false }
        }
        // 批次C-4 L3 画面捕获：真实窗口屏外隐藏 + WGC 采集 → 前端合成；
        // 输入经 embed_input PostMessage 直注。失败降级独立窗口 + 如实原因。
        CompatTier::L3 => {
            // R3-B11 终局结论（实机三轮实验）：Chromium 窗口与 L3 藏窗捕获
            // 根本冲突 —— 窗口一旦藏 -32000 / 被完全遮挡，occlusion 检测即
            // 停渲染（黑帧/冻结帧），重新显示也不会自动恢复（需 resize 或
            // 交互唤醒）。历史「CEF 一律 L3、L1 重父化呈现纯黑」是渲染挂起
            // 的误判：唤醒后的 Steam CEF 主窗 reparent 进桌面画面/输入全通。
            // Steam 家族（steam.exe / steamwebhelper.exe，含 embed_adopt 的
            // 无 target 路径）一律 **L1 真实嵌入优先**，失败才回退 L3 冻结帧。
            // Wallpaper Engine 2.8+ 的 UI 主窗（wallpaperui.exe，CEF）同根
            // 冲突同策略（实机：L3 藏窗捕获必停渲染黑帧，L1 嵌入画面/输入全通）。
            // 其它 CEF 应用待逐个实机验证后再扩大（避免旧坑）。
            let is_cef_l1_family = {
                let img = win::window_pid(hwnd_from_isize(hwnd))
                    .and_then(|p| win::process_image(p))
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();
                let base = img.rsplit(['\\', '/']).next().unwrap_or("");
                base == "steam.exe" || base == "steamwebhelper.exe" || base == "wallpaperui.exe"
            };
            // R5 补：单例重开/托盘旧窗路径的藏窗态必须先唤醒再重父化——
            // 此前 Steam 家族分支在 needs_reveal 检查之前 return，上会话
            // 被 hide 到 -32000 的托盘旧窗未唤醒直接嵌入 → Chromium 停渲染
            // 黑帧（r5 实机复现）。还原+抖动唤醒与下方 R3-B11 治理同构。
            if is_cef_l1_family && crate::shell::capture::win::needs_reveal(hwnd) {
                crate::shell::capture::win::restore_window(hwnd);
                let woke = crate::shell::capture::win::jiggle_window(hwnd);
                crate::shell::applog::log(
                    "capture",
                    format!(
                        "attach {tp_id}: CEF 家族托盘旧窗处于停渲染态 → 已还原+抖动唤醒（woke={woke}）再 L1 嵌入"
                    ),
                );
                std::thread::sleep(std::time::Duration::from_millis(1200));
            }
            if is_cef_l1_family && restyle_and_reparent(app, hwnd) {
                register(None, false);
                crate::shell::applog::log(
                    "embed",
                    format!(
                        "attach {tp_id}: L3→L1 真实嵌入（CEF 家族，藏窗捕获对 Chromium 必停渲染）hwnd={hwnd}（capture=false）"
                    ),
                );
                return Attach::Ok { capture: false };
            }
            // R3-B11 黑帧治理：窗口藏在 -32000 屏外（上会话 hide 后被单例
            // 唤起/重收编）或完全隐藏（Steam 关登录窗后 CEF 主窗退托盘）时，
            // Chromium occlusion 检测已停渲染，且实测重新显示后渲染管线
            // 不会自动恢复（resize/交互才触发）——藏回 -32000 又会立刻再停。
            // 因此对处于停渲染态的窗口：还原 → 尺寸抖动唤醒 → 等渲染恢复 →
            // **降级 L1 真实嵌入**（可见即渲染，画面与输入均为真实通路），
            // 不再走"唤醒后藏回"的必黑路径。冷启动（窗口活跃渲染中）零改动。
            if crate::shell::capture::win::needs_reveal(hwnd) {
                crate::shell::capture::win::restore_window(hwnd);
                let woke = crate::shell::capture::win::jiggle_window(hwnd);
                crate::shell::applog::log(
                    "capture",
                    format!(
                        "attach {tp_id}: 窗口隐藏/屏外 → 已还原+抖动唤醒（woke={woke}），降级 L1 真实嵌入（L3 藏窗对 Chromium 必停渲染）"
                    ),
                );
                std::thread::sleep(std::time::Duration::from_millis(1200));
                if restyle_and_reparent(app, hwnd) {
                    register(None, false);
                    crate::shell::applog::log(
                        "embed",
                        format!("attach {tp_id}: L3→L1 降级嵌入成功 hwnd={hwnd}（capture=false）"),
                    );
                    return Attach::Ok { capture: false };
                }
                crate::shell::applog::log(
                    "capture",
                    format!("attach {tp_id}: L1 降级失败 → 回退 L3 捕获（可能黑帧/冻结）"),
                );
            }
            match crate::shell::capture::win::start_capture(app.clone(), key.clone(), hwnd) {
                Ok(()) => {
                    crate::shell::applog::log("capture", format!("attach {tp_id}: L3 画面捕获已启动 hwnd={hwnd}"));
                    let _ = crate::shell::capture::win::hide_offscreen(hwnd);
                    register(None, true);
                    Attach::Ok { capture: true }
                }
                Err(e) => {
                    crate::shell::applog::log(
                        "capture",
                        format!("attach {tp_id}: L3 画面捕获启动失败（{e}）→ 保持独立窗口"),
                    );
                    Attach::Skip {
                        reason: format!("「{}」L3 画面捕获不可用（{}）→ 保持独立窗口运行。", tp_id, e),
                    }
                }
            }
        }
        // L1（标准窗口）→ 重父级嵌入主路径
        _ => {
            if !restyle_and_reparent(app, hwnd) {
                return Attach::Skip {
                    reason: "重父级失败（桌面窗口不存在）。应用保持独立窗口运行。".into(),
                };
            }
            register(None, false);
            Attach::Ok { capture: false }
        }
    }
}

/// 批次C-1：收编同进程树新弹出的主窗口为独立嵌入会话
/// （WinEventHook 广播 embed://popup / Steam 主动收编看护 → 前端开新占位窗后调用）。
/// 窗口必须仍然有效且未被登记；随后走 `attach_by_tier` **分级**接入 ——
/// 与 embed_launch 同一裁判（CEF/Chromium/Electron/DirectComposition 窗口
/// 一律 L3 采画面，绝不做会呈现纯黑的 L1 重父化）。
#[tauri::command]
#[cfg(windows)]
pub async fn embed_adopt(
    st: tauri::State<'_, crate::state::AppState>,
    app: tauri::AppHandle,
    tp_id: String,
    hwnd: isize,
    root_pid: u32,
    embed_id: String,
) -> CmdResult<bool> {
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;
    crate::shell::applog::log("adopt", format!("embed_adopt {tp_id}: hwnd={hwnd} root_pid={root_pid} embed_id={embed_id}"));
    pending_adopt_remove(hwnd);
    if !unsafe { IsWindow(hwnd_from_isize(hwnd)) }.as_bool() {
        return Ok(false);
    }
    let already = with_registry(|m| m.values().any(|e| e.hwnd == hwnd) || m.contains_key(&embed_id));
    if already {
        return Ok(false);
    }
    let dpi_fix = crate::shell::launcher::registry_snapshot(&st)
        .iter()
        .find(|a| a.id == tp_id)
        .map(|a| a.dpi_fix)
        .unwrap_or(false);
    match attach_by_tier(&app, &st, embed_id.clone(), tp_id.clone(), hwnd, root_pid, dpi_fix, None) {
        Attach::Ok { .. } => {
            // R5 隔离底线：嵌入成功后重新断言桌面置顶（tp_launch 撤销过），
            // 防止 Windows 任务栏/shell 在兼容期露出（r5 实机复现）。
            reassert_desktop_topmost(&app);
            Ok(true)
        }
        // Native / L4 / L2 包裹失败 / L3 采集不可用 → 不接入：应用保持独立窗口运行。
        // 前端据 false 关闭刚开的占位窗（不伪造成功）。
        Attach::Skip { reason } => {
            crate::shell::applog::log("adopt", format!("embed_adopt {tp_id}: 未接入：{reason}"));
            eprintln!("[embed-adopt] skip {tp_id}: {reason}");
            Ok(false)
        }
    }
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
        let _ = SetParent(h, HWND::default());
    }
    true
}

#[cfg(not(windows))]
#[tauri::command(async)]
pub async fn embed_launch(
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
pub async fn embed_pick_window(timeout_ms: Option<u64>) -> CmdResult<Option<isize>> {
    // 键盘去抖 + 轮询等待（最长 15s）为纯阻塞操作：整体移入阻塞线程池
    tauri::async_runtime::spawn_blocking(move || embed_pick_window_blocking(timeout_ms))
        .await
        .map_err(|e| AppError::io(format!("框选线程异常 / pick thread error: {e}")))?
}

#[cfg(windows)]
fn embed_pick_window_blocking(timeout_ms: Option<u64>) -> CmdResult<Option<isize>> {
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
        let _ = SetWindowPos(hwnd_from(hw), HWND::default(), x, y, w.max(1), h.max(1), SWP_NOZORDER);
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
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOW};
    let key = norm_id(embed_id);
    let target = with_registry(|map| {
        map.get(&key).map(|e| e.host.unwrap_or(e.hwnd))
    });
    if let Some(h) = target {
        unsafe {
            let _ = ShowWindow(hwnd_from(h), if visible { SW_SHOW } else { SW_HIDE });
        }
        // R4-B7（首轮 B-4）：收编子窗隐藏/最小化后 WebView2 合成层可能失效白屏，
        // 立即对桌面 WebView 强制同步重绘（root=Tauri Window 顶层）。
        if !visible {
            force_webview_repaint(h);
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
            let _ = PostMessageW(hwnd_from(target), WM_CLOSE, WPARAM(0), LPARAM(0));
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
            let _ = PostMessageW(hwnd_from(target), WM_CLOSE, WPARAM(0), LPARAM(0));
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
            let _ = SetParent(h, HWND::default());
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

// ---------- R4 修复：桌面 WebView 提升与强制重绘（首轮 B-3 / B-4） ----------

/// R4-B5：定位桌面 WebView2 子窗 —— Tauri Window 的直接子窗中，
/// 不在嵌入注册表且类名为 Chrome_*/WebView* 的那个
/// （被收编 Edge 与 WebView2 同族类名，用「非嵌入」区分）。
#[cfg(windows)]
fn desktop_webview_child(top: isize) -> Option<isize> {
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetClassNameW, GetWindow, GA_ROOT, GW_CHILD, GW_HWNDNEXT};
    let root = unsafe { GetAncestor(hwnd_from(top), GA_ROOT) };
    let embeds = current_embed_hwnds();
    let mut child = unsafe { GetWindow(root, GW_CHILD) }.ok();
    let mut buf = [0u16; 64];
    while let Some(h) = child {
        let hval = h.0 as isize;
        if !embeds.contains(&hval) {
            let n = unsafe { GetClassNameW(h, &mut buf) };
            let cls = String::from_utf16_lossy(&buf[..n as usize]);
            if cls.starts_with("Chrome_") || cls.contains("WebView") {
                return Some(hval);
            }
        }
        child = unsafe { GetWindow(h, GW_HWNDNEXT) }.ok();
    }
    None
}

/// R4-B7：对桌面 WebView2 子窗强制同步重绘（修复 WebView2 合成层失效白屏）。
#[cfg(windows)]
fn force_webview_repaint(top: isize) {
    use windows::Win32::Graphics::Gdi::{
        RedrawWindow, HRGN, RDW_ALLCHILDREN, RDW_FRAME, RDW_INVALIDATE, RDW_UPDATENOW,
    };
    let Some(wv) = desktop_webview_child(top) else { return };
    unsafe {
        let _ = RedrawWindow(
            hwnd_from(wv),
            None,
            HRGN::default(),
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_FRAME | RDW_UPDATENOW,
        );
    }
}

/// R4-B6（首轮 B-3）：收编子窗置顶盖满全屏时，把桌面 WebView 提回
/// 全部嵌入子窗之上（不激活，不抢焦点）——用户按 Win 键即可回到桌面壳。
#[tauri::command(async)]
#[cfg(windows)]
pub fn desktop_raise(app: tauri::AppHandle) -> CmdResult<()> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW,
    };
    let Some(top) = desktop_hwnd(&app) else { return Ok(()) };
    let Some(wv) = desktop_webview_child(top) else { return Ok(()) };
    unsafe {
        let _ = SetWindowPos(
            hwnd_from(wv),
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
    Ok(())
}

#[tauri::command(async)]
#[cfg(not(windows))]
pub fn desktop_raise(_app: tauri::AppHandle) -> CmdResult<()> {
    Ok(())
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
pub(crate) fn has_caption_style(hwnd: isize) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, GWL_STYLE, WS_CAPTION};
    let style = unsafe { GetWindowLongPtrW(hwnd_from_isize(hwnd), GWL_STYLE) } as u32;
    style & WS_CAPTION.0 != 0
}

/// D-3 看门狗「可收编主窗」闸门（R6 实机根因修复）：CEF 家族会开出大量
/// 无标题工具窗 / "Menu" 弹出窗 / 截图覆盖层——它们不是应用主窗，收编
/// 只会得到一个黑框并抢占真正主窗的收编次序（Steam 黑屏实机根因）。
/// 口径：必须有标题栏样式 + 非工具窗 + 客户区 ≥160×100。
#[cfg(windows)]
pub(crate) fn is_adoptable_main_window(hwnd: isize) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, GetWindowRect, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };
    if !has_caption_style(hwnd) {
        return false;
    }
    let ex = unsafe { GetWindowLongPtrW(hwnd_from_isize(hwnd), GWL_EXSTYLE) } as u32;
    if ex & WS_EX_TOOLWINDOW.0 != 0 {
        return false;
    }
    let mut rc = windows::Win32::Foundation::RECT::default();
    if unsafe { GetWindowRect(hwnd_from_isize(hwnd), &mut rc) }.is_err() {
        return false;
    }
    (rc.right - rc.left) >= 160 && (rc.bottom - rc.top) >= 100
}

#[cfg(not(windows))]
pub(crate) fn is_adoptable_main_window(_hwnd: isize) -> bool {
    true
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
                    crate::shell::applog::log(
                        "steam",
                        format!("steam-adopt: 找到主窗 hwnd={hwnd} pid={pid} root={root} → 广播 embed://popup"),
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
            crate::shell::applog::log(
                "steam",
                "steam-adopt: 90s 内未找到 Steam 主窗（冷启动/登录中？）→ 交给 D-3 看门狗兜底",
            );
        })
        .ok();
}

#[cfg(not(windows))]
pub fn spawn_steam_adopt_watcher(_app: tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::{with_registry, norm_id};

    /// 单例兜底收编：家族映像族 —— Steam 主 exe 必须带上 steamwebhelper
    /// （主窗属它），反向登记同理；普通 exe 族内只有自己。
    #[cfg(windows)]
    #[test]
    fn family_images_covers_steam_pair() {
        let f = super::family_images("steam.exe");
        assert!(f.contains(&"steam.exe".to_string()));
        assert!(f.contains(&"steamwebhelper.exe".to_string()));
        let fb = super::family_images("steamwebhelper.exe");
        assert!(fb.contains(&"steam.exe".to_string()));
        assert_eq!(super::family_images("notepad.exe"), vec!["notepad.exe".to_string()]);
        // 大小写不敏感
        assert!(super::family_images("STEAM.EXE").contains(&"steamwebhelper.exe".to_string()));
    }

    /// Wallpaper Engine 2.8+：UI 主窗属 wallpaperui.exe —— 登记 wallpaper64/32
    /// 时必须互为家族，反向同理（实机回归：UI 窗 pid 树落空 → embed-fail）。
    #[test]
    fn family_images_covers_wallpaperui() {
        let f = super::family_images("wallpaper64.exe");
        assert!(f.contains(&"wallpaperui.exe".to_string()));
        let f32 = super::family_images("wallpaper32.exe");
        assert!(f32.contains(&"wallpaperui.exe".to_string()));
        let fu = super::family_images("wallpaperui.exe");
        assert!(fu.contains(&"wallpaper64.exe".to_string()));
        assert!(fu.contains(&"wallpaper32.exe".to_string()));
    }

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
