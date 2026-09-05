//! L3 shell — tray.rs（M5）
//! OS 级系统托盘：图标 + 菜单（显示桌面 / 四款独立软件 / 退出 Variable）。
//! - 左键点击托盘 = 聚焦桌面窗口（未运行软件窗口时它就是 Variable 本身）
//! - "退出 Variable" 走桌面窗口前端的保存冲刷流程（tray://quit 事件），不粗暴杀进程
//! - 菜单文案双语并列（Rust 端静态构建，不随前端语言热切换）

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const APPS: [(&str, &str); 4] = [
    ("app-write", "Variable Write"),
    ("app-mind", "Variable Mind"),
    ("app-code", "Variable Code"),
    ("app-fate", "Variable Fate"),
];

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示桌面 Show Desktop", true, None::<&str>)?;
    let sep0 = PredefinedMenuItem::separator(app)?;
    let w = MenuItem::with_id(app, "open-app-write", APPS[0].1, true, None::<&str>)?;
    let m = MenuItem::with_id(app, "open-app-mind", APPS[1].1, true, None::<&str>)?;
    let c = MenuItem::with_id(app, "open-app-code", APPS[2].1, true, None::<&str>)?;
    let f = MenuItem::with_id(app, "open-app-fate", APPS[3].1, true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Variable Exit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&show, &sep0, &w, &m, &c, &f, &sep1, &quit])?;

    let _tray = TrayIconBuilder::with_id("variable-tray")
        .icon(app.default_window_icon().expect("app icon").clone())
        .tooltip("Variable — Private Desktop Environment")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => focus_desktop(app),
            "quit" => quit_via_desktop(app),
            id => {
                if let Some((label, _)) = APPS.iter().find(|(l, _)| format!("open-{l}") == id) {
                    open_app_window(app, label);
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                focus_desktop(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn focus_desktop(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("desktop") {
        // 批次D（规格 4.3.4）：红灯"隐藏到托盘"后窗口不可见 —— 左键托盘先 show 再聚焦
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 打开（或聚焦）一款独立软件窗口。与前端 appWindows.ts 行为一致。
fn open_app_window(app: &AppHandle, label: &str) {
    let title = APPS
        .iter()
        .find(|(l, _)| *l == label)
        .map(|(_, t)| *t)
        .unwrap_or("Variable");
    open_window(app, label, title, WebviewUrl::App(format!("{label}.html").into()));
}

/// 系统窗口（M6）：explorer（文件管理器）/ recycle（回收站，复用 explorer.html）。
pub fn open_system_window(app: &AppHandle, label: &str) {
    let (title, url) = match label {
        "recycle" => ("Variable 回收站", WebviewUrl::App("explorer.html?view=recycle".into())),
        _ => ("Variable 文件管理器", WebviewUrl::App("explorer.html".into())),
    };
    open_window(app, label, title, url);
}

fn open_window(app: &AppHandle, label: &str, title: &str, url: WebviewUrl) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, label, url)
        .title(title)
        .inner_size(1280.0, 800.0)
        .min_inner_size(960.0, 640.0)
        .resizable(true)
        .decorations(false)
        // 批次0：与置顶桌面同层（topmost 组内按激活序），保证浮于覆盖桌面之上
        .always_on_top(true)
        .build();
}

/// 退出：通知桌面窗口走保存冲刷；桌面窗口不存在时兜底退出。
fn quit_via_desktop(app: &AppHandle) {
    match app.get_webview_window("desktop") {
        Some(w) => {
            let _ = w.unminimize();
            let _ = w.set_focus();
            let _ = w.emit("tray://quit", ());
        }
        None => app.exit(0),
    }
}
