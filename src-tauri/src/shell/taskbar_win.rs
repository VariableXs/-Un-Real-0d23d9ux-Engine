//! shell/taskbar_win.rs —— M4：任务栏独立原生顶层窗 + 彻底隐藏 Windows 痕迹。
//!
//! 三件事（与《嵌入子系统改造-待完成清单》M4 对应）：
//! 1. **`taskbar` 原生窗**：全屏透明 TOPMOST+TOOLWINDOW 顶层窗，加载 taskbar.html
//!    渲染与桌面版像素级一致的 Taskbar/StartMenu 组件。它不在桌面 WebView 内，
//!    因此不受「被拥有窗口恒在宿主之上」约束 —— 提到 TOPMOST 带顶即可压过任何
//!    被收编的第三方窗口（M4 验收：任务栏不被第三方窗口遮住）。
//! 2. **光标守护**（150ms 轮询）：驱动收起/呼出状态机 ——
//!    第三方收编数 > 0 → 收起（SW_HIDE）；退出软件 → 重新出现；收起态光标进
//!    底边热区 → 临时呼出并重排到 TOPMOST 带顶；点击穿透由前端上报的交互矩形
//!    （hitmap）精确控制 set_ignore_cursor_events，保证透明区点击直达第三方窗口。
//! 3. **Windows 痕迹清除**：Shell_TrayWnd / Shell_SecondaryTrayWnd 隐藏 +
//!    SPI_SETWORKAREA 全屏（最大化窗口＝真全屏），看门狗自愈（explorer 重启后
//!    重新隐藏）；退出时 Drop guard + RunEvent::Exit 双保险恢复原状。
//!
//! 绝不强杀、绝不改用户进程：本模块只动自己的窗口与系统 shell 窗口的可见性。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::Manager;

use crate::error::CmdResult;

/// 项目 windows crate（0.58）的 HWND 构造。tauri 自带的 hwnd() 返回其依赖
/// 版本的 HWND，跨版本不可直接传参 —— 统一经 isize 中转（与 embed.rs 一致）。
#[cfg(windows)]
fn hwnd_from_isize(v: isize) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(v as *mut core::ffi::c_void)
}

/// tauri WebviewWindow → 本项目 windows crate 的 HWND。
#[cfg(windows)]
fn native_hwnd(win: &tauri::WebviewWindow) -> Option<windows::Win32::Foundation::HWND> {
    win.hwnd().ok().map(|h| hwnd_from_isize(h.0 as isize))
}

/// 任务栏窗 label（与 capabilities/default.json 的 windows 列表一致）。
pub const TASKBAR_LABEL: &str = "taskbar";

/// 收起态底边呼出热区（物理 px）：光标贴屏幕底边 3px 内触发呼出。
const HOTZONE_PHYS: i32 = 3;
/// 光标离开任务栏交互区后收回呼出态的宽限（ms）。
const SUMMON_GRACE_MS: u128 = 1200;
/// 收起切换的滑出动画窗口（ms）：先广播 taskbar://state=collapsed（前端播放
/// CSS 滑出过渡），延迟后再 SW_HIDE，避免窗口瞬间消失的生硬感。
const TASKBAR_HIDE_DELAY_MS: u64 = 240;
/// 悬停激活阈值（ms）：光标稳定落在交互区超过该时长，才把窗口带到前台。
/// 背景：未激活窗口的 WebView2 会吞掉物理鼠标点击（down 可达而 up 被截断，
/// click 不合成，React onClick 永不触发——A6 实测 ev 0~1 条不完整）；
/// 窗口在前台后点击链路 100% 可达。快速滑过（<250ms）不触发，不抢输入焦点。
const HOVER_FOCUS_MS: u64 = 250;

// ---------------------------------------------------------------------------
// 前端 → 后端共享状态
// ---------------------------------------------------------------------------

/// 开始菜单/弹出层打开中（任务栏窗前端上报）：呼出态钉住，不自动收回。
static MENU_OPEN: AtomicBool = AtomicBool::new(false);
/// 任务栏窗交互矩形集合（**逻辑 px**，前端上报；匹配时 ×DPR 换算物理）。
/// 元素来自任务栏树内 pointer-events != none 的可见元素（含弹出层）。
static HITMAP: Mutex<Vec<(f32, f32, f32, f32)>> = Mutex::new(Vec::new());
/// 桌面窗启动仪式完成 → 任务栏窗可以显示（避免盖住 BootScreen）。
static DESKTOP_READY: AtomicBool = AtomicBool::new(false);
/// 光标守护自己维护的当前显示态（避免每轮重复 SetWindowPos）。
static SHOWN: AtomicBool = AtomicBool::new(false);
/// 当前 ignore_cursor_events 值（值变化才调用，减少 COM 往返）。
static IGNORED: AtomicBool = AtomicBool::new(true);
/// 光标连续落在交互区的起点（悬停激活计时；离开即清零）。
static HOVER_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

/// 任务栏窗上报交互矩形（逻辑 px）与菜单开合。由 entries/taskbar 宿主调用。
pub fn taskbar_report_hitmap(
    rects: Vec<(f32, f32, f32, f32)>,
    menu_open: bool,
) {
    // 首次收到非空上报 → 前端 JS 存活铁证（只打一次，避免刷屏）
    static FIRST_REPORT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
    if FIRST_REPORT.swap(false, Ordering::Relaxed) {
        crate::shell::applog::log(
            "taskbar",
            format!("任务栏窗前端首个 hitmap 上报到达（{:.0} rects，menu_open={menu_open}）——前端 JS 存活", rects.len()),
        );
    }
    if let Ok(mut g) = HITMAP.lock() {
        *g = rects;
    }
    MENU_OPEN.store(menu_open, Ordering::Relaxed);
}

/// 桌面窗启动仪式完成时调用（App.tsx boot 完成处 emit，后端 listen）。
pub fn taskbar_desktop_ready() {
    DESKTOP_READY.store(true, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// 任务栏窗创建
// ---------------------------------------------------------------------------

/// 在 setup 里创建任务栏窗（不可见，等 desktop ready / 状态机首轮再显示）。
pub fn spawn_taskbar_window(app: &tauri::AppHandle) {
    use tauri::WebviewUrl;
    if app.get_webview_window(TASKBAR_LABEL).is_some() {
        return; // 单实例内幂等
    }
    let built = tauri::webview::WebviewWindowBuilder::new(
        app,
        TASKBAR_LABEL,
        WebviewUrl::App("taskbar.html".into()),
    )
    .title("Variable Taskbar")
    .transparent(true)
    .decorations(false)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .shadow(false)
    .focused(false)
    .visible(false)
    .build();

    let win = match built {
        Ok(w) => w,
        Err(e) => {
            crate::shell::applog::log(
                "taskbar",
                format!("任务栏窗创建失败（降级为无独立任务栏运行）: {e}"),
            );
            return;
        }
    };

    // 全屏物理矩形（主屏）：窗体恒全屏、透明，条/菜单由 DOM 决定视觉。
    #[cfg(windows)]
    {
        let sw = unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN,
        ) };
        let sh = unsafe { windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
            windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
        ) };
        let _ = win.set_position(tauri::PhysicalPosition::new(0, 0));
        let _ = win.set_size(tauri::PhysicalSize::new(sw.max(1) as u32, sh.max(1) as u32));
        apply_toolwindow(&win);
        let _ = win.set_ignore_cursor_events(true);
        IGNORED.store(true, Ordering::Relaxed);
    }
    crate::shell::applog::log("taskbar", "任务栏窗已创建（TOPMOST+TOOLWINDOW+透明，等待 desktop ready）");
}

/// 补钉 WS_EX_TOOLWINDOW（Alt+Tab / Flip3D 不可见；skip_taskbar 的双保险）。
#[cfg(windows)]
fn apply_toolwindow(win: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };
    if let Some(h) = native_hwnd(win) {
        let ex = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32;
        if ex & WS_EX_TOOLWINDOW.0 == 0 {
            unsafe {
                SetWindowLongPtrW(h, GWL_EXSTYLE, (ex | WS_EX_TOOLWINDOW.0) as isize);
            }
        }
    }
}

#[cfg(not(windows))]
fn apply_toolwindow(_win: &tauri::WebviewWindow) {}

// ---------------------------------------------------------------------------
// 光标守护：收起 / 呼出 / 点击穿透状态机
// ---------------------------------------------------------------------------

/// 当前收编会话数（第三方窗口被 Variable 拥有中）。
fn embedded_count() -> usize {
    crate::shell::embed::embedded_count()
}

/// 主屏物理尺寸。
#[cfg(windows)]
fn screen_size() -> (i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN),
        )
    }
}

#[cfg(not(windows))]
fn screen_size() -> (i32, i32) {
    (1920, 1080)
}

/// 光标是否落在任一交互矩形内（rects 为逻辑 px，物理换算 ×DPR）。
#[cfg(windows)]
fn cursor_in_hitmap(cur: (i32, i32), dpr: f32) -> bool {
    if MENU_OPEN.load(Ordering::Relaxed) {
        return true; // 菜单/弹层打开中：全窗交互（含 backdrop 点击关闭）
    }
    let rects = match HITMAP.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let (fx, fy) = (cur.0 as f32 / dpr.max(0.01), cur.1 as f32 / dpr.max(0.01));
    rects
        .iter()
        .any(|&(x, y, w, h)| fx >= x && fx <= x + w && fy >= y && fy <= y + h)
}

#[cfg(not(windows))]
fn cursor_in_hitmap(_cur: (i32, i32), _dpr: f32) -> bool {
    false
}

/// 启动光标守护线程。
pub fn spawn_taskbar_cursor_watcher(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("taskbar-cursor".into())
        .spawn(move || cursor_loop(app))
        .ok();
}

#[cfg_attr(not(windows), allow(unused_variables))]
fn cursor_loop(app: tauri::AppHandle) {
    let mut summon_since: Option<Instant> = None;
    let mut last_emit_mode = String::new();
    let mut hide_deadline: Option<Instant> = None;
    loop {
        std::thread::sleep(Duration::from_millis(150));
        if !DESKTOP_READY.load(Ordering::Relaxed) {
            continue; // 启动仪式未完，任务栏窗保持隐藏
        }
        let Some(win) = app.get_webview_window(TASKBAR_LABEL) else { continue };
        let embedded = embedded_count();
        let menu_open = MENU_OPEN.load(Ordering::Relaxed);

        #[cfg(windows)]
        let (_sw, sh) = screen_size();
        #[cfg(not(windows))]
        let (sw, sh) = screen_size();

        #[cfg(windows)]
        let cursor = {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
            let mut p = POINT::default();
            let ok = unsafe { GetCursorPos(&mut p) }.is_ok();
            if ok { (p.x, p.y) } else { (-1, -1) }
        };
        #[cfg(not(windows))]
        let cursor = (-1, -1);

        // 状态机：SHOWN（无收编或菜单钉住）/ COLLAPSED（收编中）/ SUMMONED（呼出）
        let dpr = win.scale_factor().unwrap_or(1.0) as f32;
        let in_hit = cursor.0 >= 0 && cursor_in_hitmap(cursor, dpr);
        let in_hotzone = cursor.1 >= 0 && cursor.1 >= sh - HOTZONE_PHYS;
        let in_summon_band = cursor.1 >= 0
            && cursor.1 >= sh - (54.0 * dpr) as i32; // 条带高度附近保持呼出

        let mode = if menu_open || embedded == 0 {
            "shown"
        } else if in_hotzone || (summon_since.is_some() && in_summon_band) {
            "summoned"
        } else {
            "collapsed"
        };

        // 呼出宽限计时（光标仍在热区 = 呼出意图持续，不参与超时回落）
        match mode {
            "summoned" => {
                if summon_since.is_none() {
                    summon_since = Some(Instant::now());
                    crate::shell::applog::log("taskbar", "底边热区 → 临时呼出任务栏");
                }
            }
            "collapsed" => summon_since = None,
            _ => {}
        }
        // 呼出超时回落：仅当光标离开热区且未落任务栏交互区超过宽限期
        let mode = if mode == "summoned" {
            if let Some(t) = summon_since {
                if !in_hit && !in_hotzone && t.elapsed().as_millis() > SUMMON_GRACE_MS {
                    summon_since = None;
                    "collapsed"
                } else {
                    "summoned"
                }
            } else {
                "summoned"
            }
        } else {
            mode
        };

        // 显示 / 隐藏（值变化才动作；收起先播滑出动画，延迟到点再 SW_HIDE）
        let want_shown = mode != "collapsed";
        let mut ready_to_apply = want_shown;
        if !want_shown {
            match hide_deadline {
                None => hide_deadline = Some(Instant::now() + Duration::from_millis(TASKBAR_HIDE_DELAY_MS)),
                Some(t) => ready_to_apply = Instant::now() >= t,
            }
        }
        if want_shown != SHOWN.load(Ordering::Relaxed) && ready_to_apply {
            #[cfg(windows)]
            apply_shown(&win, want_shown);
            #[cfg(not(windows))]
            let _ = &win;
            SHOWN.store(want_shown, Ordering::Relaxed);
            hide_deadline = None;
            if mode == "summoned" {
                crate::shell::applog::log("taskbar", "任务栏窗已呼出并重排到 TOPMOST 带顶");
            }
        }

        // 点击穿透：仅显示态需要管理
        let want_ignore = want_shown && !in_hit;
        if want_ignore != IGNORED.load(Ordering::Relaxed) {
            let _ = win.set_ignore_cursor_events(want_ignore);
            IGNORED.store(want_ignore, Ordering::Relaxed);
        }

        // 悬停激活：未激活窗口的 WebView2 会吞物理点击（down 可达 up 被截断，
        // click 不合成 → onClick 永不触发）。光标在交互区稳定悬停超过阈值时
        // 把窗口带到前台，为即将到来的点击做激活预备；快速滑过不触发。
        // 幂等：每轮先查前台，已在前台则零开销跳过（窗口意外失焦后自动补激活）。
        if want_shown && in_hit {
            let due = match HOVER_SINCE.lock() {
                Ok(mut h) => h
                    .get_or_insert_with(Instant::now)
                    .elapsed()
                    .as_millis()
                    >= HOVER_FOCUS_MS as u128,
                Err(_) => false,
            };
            if due {
                #[cfg(windows)]
                {
                    let need_fg = native_hwnd(&win)
                        .map(|h| unsafe {
                            windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0
                                as isize
                                != h.0 as isize
                        })
                        .unwrap_or(false);
                    if need_fg {
                        match win.set_focus() {
                            Ok(()) => {
                                crate::shell::applog::log(
                                    "taskbar",
                                    "光标悬停交互区 → 任务栏窗带到前台（保障点击可达）",
                                );
                            }
                            Err(e) => {
                                crate::shell::applog::log(
                                    "taskbar",
                                    format!("悬停激活 set_focus 失败: {e}"),
                                );
                            }
                        }
                    }
                }
            }
        } else if let Ok(mut h) = HOVER_SINCE.lock() {
            *h = None;
        }

        // 模式广播（前端做滑入/滑出动画）
        if mode != last_emit_mode {
            last_emit_mode = mode.to_string();
            let _ = tauri::Emitter::emit_to(
                &app,
                TASKBAR_LABEL,
                "taskbar://state",
                serde_json::json!({ "mode": mode, "embedded": embedded }),
            );
        }
    }
}

/// 显示/隐藏任务栏窗；显示时重排到 TOPMOST 带顶（压过被拥有窗口的关键一步）。
/// 显示路径顺带补钉 WS_EX_TOOLWINDOW（tauri 对 transparent+decorations(false) 窗
/// 的 EXSTYLE 初始化时序在 build() 之后才完成，spawn 时钉的位会被覆盖）。
///
/// **必须用 `win.show()` / `win.hide()`**：窗口以 visible(false) 创建时，wry 的
/// WRY_WEBVIEW 子窗同样保持隐藏态；裸 SetWindowPos(SWP_SHOWWINDOW) 只显示外层
/// 壳窗，WebView 子窗仍是 WS_VISIBLE=0 → 窗口可见但内容全透明（实测踩坑）。
/// tauri 的 show()/hide() 会连 WebView2 一起显隐；TOPMOST 重排随后补上。
#[cfg(windows)]
fn apply_shown(win: &tauri::WebviewWindow, shown: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOP, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    if shown {
        let _ = win.show();
    } else {
        let _ = win.hide();
    }
    if let Some(h) = native_hwnd(win) {
        if shown {
            apply_toolwindow(win);
        }
        unsafe {
            let applied = SetWindowPos(
                h,
                if shown { HWND_TOPMOST } else { HWND_TOP },
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
            if let Err(e) = applied {
                crate::shell::applog::log(
                    "taskbar",
                    format!("SetWindowPos(shown={shown}) 失败: {e} hwnd={:?}", h.0 as isize),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Windows 任务栏隐藏 + 工作区全屏（痕迹清除）
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod traces {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, FindWindowW, GetWindowRect, IsWindowVisible, SetWindowPos,
        SystemParametersInfoW, HWND_BOTTOM, SPI_GETWORKAREA, SPI_SETWORKAREA,
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    };
    use windows::core::w;

    /// 原 Shell_TrayWnd 句柄（恢复用；explorer 重启后按类名现查）。
    pub static TRAY_HWND: std::sync::Mutex<usize> = std::sync::Mutex::new(0);
    /// 隐藏前的原工作区。
    pub static ORIG_WORKAREA: std::sync::Mutex<Option<RECT>> = std::sync::Mutex::new(None);
    /// 本模块已执行隐藏（防止重复 SPI 调用）。
    pub static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// 枚举 Shell_TrayWnd + 全部 Shell_SecondaryTrayWnd。
    pub fn find_tray_windows() -> Vec<windows::Win32::Foundation::HWND> {
        let mut out = Vec::new();
        unsafe {
            if let Ok(main) = FindWindowW(w!("Shell_TrayWnd"), None) {
                out.push(main);
            }
            let mut prev: Option<windows::Win32::Foundation::HWND> = None;
            loop {
                let h = FindWindowExW(None, prev.as_ref(), w!("Shell_SecondaryTrayWnd"), None);
                match h {
                    Ok(x) if !x.0.is_null() => {
                        out.push(x);
                        prev = Some(x);
                    }
                    _ => break,
                }
            }
        }
        out
    }

    pub fn hide_all() {
        for h in find_tray_windows() {
            unsafe {
                let visible = IsWindowVisible(h).as_bool();
                if visible {
                    let _ = SetWindowPos(
                        h, HWND_BOTTOM, 0, 0, 0, 0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
                    );
                }
            }
        }
        unsafe {
            if let Ok(main) = FindWindowW(w!("Shell_TrayWnd"), None) {
                if let Ok(mut g) = TRAY_HWND.lock() {
                    *g = main.0 as usize;
                }
            }
        }
        ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn show_all() {
        for h in find_tray_windows() {
            unsafe {
                // 不动 Z 序（SWP_NOZORDER）：只还原可见性，Windows 自己管理任务栏 TOPMOST。
                let _ = SetWindowPos(
                    h, HWND_BOTTOM, 0, 0, 0, 0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW,
                );
            }
        }
        ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
    }

    /// 工作区 → 全屏（最大化窗口＝真全屏）；返回是否首次设置。
    pub fn set_workarea_fullscreen() -> bool {
        unsafe {
            let mut orig = RECT::default();
            let got = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut orig as *mut RECT as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
            let first = ORIG_WORKAREA.lock().map(|mut g| {
                if got.is_err() {
                    return false;
                }
                if g.is_none() {
                    *g = Some(orig);
                    true
                } else {
                    false
                }
            }).unwrap_or(false);
            let sw = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN);
            let sh = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN);
            let full = RECT { left: 0, top: 0, right: sw, bottom: sh };
            let _ = SystemParametersInfoW(SPI_SETWORKAREA, 0, Some(&full as *const RECT as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
            first
        }
    }

    pub fn restore_workarea() {
        if let Ok(g) = ORIG_WORKAREA.lock() {
            if let Some(r) = *g {
                unsafe {
                    let _ = SystemParametersInfoW(
                        SPI_SETWORKAREA, 0, Some(&r as *const RECT as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
                    );
                }
            }
        }
    }

    /// 任务栏当前是否可见（看门狗自愈判定）。
    pub fn tray_visible() -> bool {
        unsafe {
            match FindWindowW(w!("Shell_TrayWnd"), None) {
                Ok(h) => IsWindowVisible(h).as_bool(),
                Err(_) => false,
            }
        }
    }

    /// rect 摘要（日志用）。
    pub fn rect_str(r: &RECT) -> String {
        format!("({},{},{},{})", r.left, r.top, r.right, r.bottom)
    }

    /// 供外部复核 rect（守门狗比对 workarea 是否被改写）。
    pub fn current_workarea() -> Option<RECT> {
        unsafe {
            let mut r = RECT::default();
            if SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut r as *mut RECT as _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_ok() {
                Some(r)
            } else {
                None
            }
        }
    }

    /// GetWindowRect 摘要（日志用）。
    pub fn tray_rect() -> Option<RECT> {
        unsafe {
            match FindWindowW(w!("Shell_TrayWnd"), None) {
                Ok(h) => {
                    let mut r = RECT::default();
                    GetWindowRect(h, &mut r).ok()?;
                    Some(r)
                }
                Err(_) => None,
            }
        }
    }
}

/// 执行痕迹清除：隐藏 Windows 任务栏 + 工作区全屏。重复调用幂等。
#[cfg(windows)]
pub fn hide_windows_traces() {
    let first = traces::set_workarea_fullscreen();
    traces::hide_all();
    if first {
        if let (Some(r), Some(w)) = (traces::tray_rect(), traces::current_workarea()) {
            crate::shell::applog::log(
                "taskbar",
                format!("Windows 任务栏已隐藏 rect={} · 工作区→全屏（原 {}）", traces::rect_str(&r), traces::rect_str(&w)),
            );
        }
    }
}

#[cfg(not(windows))]
pub fn hide_windows_traces() {}

/// 恢复 Windows 任务栏与工作区（退出路径）。
#[cfg(windows)]
pub fn restore_windows_traces() {
    traces::restore_workarea();
    traces::show_all();
    crate::shell::applog::log("taskbar", "Windows 任务栏与工作区已恢复");
}

#[cfg(not(windows))]
pub fn restore_windows_traces() {}

/// 自愈看门狗：explorer 重启后任务栏窗口重建（重新可见）→ 重新隐藏；
/// 工作区被系统/第三方改写 → 重设全屏。3s 轮询，随光标守护线程一起常驻。
#[cfg(windows)]
fn spawn_trace_watchdog() {
    std::thread::Builder::new()
        .name("taskbar-trace-watch".into())
        .spawn(|| loop {
            std::thread::sleep(Duration::from_secs(3));
            if !traces::ACTIVE.load(Ordering::Relaxed) {
                continue;
            }
            if traces::tray_visible() {
                crate::shell::applog::log("taskbar", "检测到 Windows 任务栏重新可见（explorer 重启？）→ 重新隐藏");
                hide_windows_traces();
            } else if let Some(w) = traces::current_workarea() {
                let sh = screen_size().1;
                if w.bottom < sh - 1 {
                    // 工作区不再全屏 → 重设
                    traces::set_workarea_fullscreen();
                }
            }
        })
        .ok();
}

#[cfg(not(windows))]
fn spawn_trace_watchdog() {}

/// setup 统一入口：创建窗 + 状态机 + 痕迹清除 + 自愈。
/// 恢复兜底由 lib.rs 的 TraceGuard（Drop）+ RunEvent::Exit 双保险承担。
pub fn init(app: &tauri::AppHandle) {
    spawn_taskbar_window(app);
    hide_windows_traces();
    spawn_trace_watchdog();
    spawn_taskbar_cursor_watcher(app.clone());
}

/// 桌面主窗销毁后关任务栏窗（否则 app 因任务栏窗存活而无法退出）。
pub fn close_taskbar_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window(TASKBAR_LABEL) {
        let _ = w.destroy();
    }
}

// ---------------------------------------------------------------------------
// Tauri 命令（任务栏窗前端调用）
// ---------------------------------------------------------------------------

/// 任务栏窗上报交互矩形（逻辑 px）+ 菜单开合。节流由前端负责（250ms）。
#[tauri::command]
pub fn taskbar_report_hitmap_cmd(
    rects: Vec<(f32, f32, f32, f32)>,
    menu_open: bool,
) -> CmdResult<()> {
    taskbar_report_hitmap(rects, menu_open);
    Ok(())
}

/// 桌面窗启动仪式完成 → 允许任务栏窗显示。
#[tauri::command]
pub fn taskbar_desktop_ready_cmd() -> CmdResult<()> {
    taskbar_desktop_ready();
    crate::shell::applog::log("taskbar", "desktop ready → 任务栏窗进入状态机管理");
    Ok(())
}
