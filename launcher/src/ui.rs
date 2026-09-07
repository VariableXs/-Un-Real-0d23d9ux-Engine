//! L-1 极简进度窗：探测 → 挂载 → 启动 → 接管 四阶段真实进度。
//! 纯 Win32，无 Tauri 依赖（引导器 exe 目标 ~3MB）。

#![cfg(windows)]

use std::sync::{Arc, Mutex};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, SetBkMode, SetTextColor, TextOutW, HDC, PAINTSTRUCT,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, GetWindowLongPtrW,
    PostQuitMessage, RegisterClassW, SetTimer, SetWindowLongPtrW, ShowWindow, TranslateMessage,
    CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWLP_USERDATA, HMENU, SW_SHOW, WINDOW_EX_STYLE,
    WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

pub const STAGES: [&str; 4] = ["探测硬件", "挂载磁盘", "启动 VM", "引擎接管"];

#[derive(Default, Clone)]
pub struct UiState {
    pub stage: usize,          // 0..3 当前阶段
    pub detail: String,        // 阶段内细节
    pub error: Option<String>, // 失败文案（含诊断指引）
}

pub type SharedUi = Arc<Mutex<UiState>>;

pub struct ProgressWindow {
    pub hwnd: isize,
}

impl ProgressWindow {
    pub fn new(state: SharedUi) -> Result<Self, String> {
        unsafe {
            let hinstance = GetModuleHandleW(None).map_err(|_| "GetModuleHandleW 失败")?;
            let class_name = w!("VariableLauncherProgress");
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: HINSTANCE(hinstance.0),
                lpszClassName: class_name,
                ..Default::default()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("Variable 启动中"),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                480,
                240,
                None,
                HMENU::default(),
                HINSTANCE(hinstance.0),
                None,
            )
            .map_err(|_| "CreateWindowExW 失败")?;
            let boxed = Box::into_raw(Box::new(state));
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, boxed as isize);
            let _ = SetTimer(hwnd, 1, 120, None);
            let _ = ShowWindow(hwnd, SW_SHOW);
            Ok(Self { hwnd: hwnd.0 as isize })
        }
    }

    pub fn run_loop(&self) {
        unsafe {
            let mut msg = std::mem::zeroed();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
            let state = unsafe { (ptr as *const Mutex<UiState>).as_ref() }
                .and_then(|m| m.lock().ok())
                .map(|g| g.clone())
                .unwrap_or_default();
            let mut ps = PAINTSTRUCT::default();
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            paint_text(hdc, &state);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            unsafe {
                let _ = InvalidateRect(hwnd, None, true);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w, l) },
    }
}

fn draw_line(hdc: HDC, x: i32, y: i32, text: &str, color: COLORREF) {
    unsafe {
        let _ = SetTextColor(hdc, color);
        let wide: Vec<u16> = text.encode_utf16().collect();
        let _ = TextOutW(hdc, x, y, &wide);
    }
}

fn paint_text(hdc: HDC, state: &UiState) {
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
    }
    draw_line(hdc, 24, 20, "Variable 便携系统", COLORREF(0x00_10_10_10));
    let mut y = 56;
    for (i, name) in STAGES.iter().enumerate() {
        let mark = if state.error.is_some() && i == state.stage {
            "x"
        } else if i < state.stage {
            "OK"
        } else if i == state.stage {
            ">"
        } else {
            "-"
        };
        let mut line = format!("{mark}  {name}");
        if i == state.stage && !state.detail.is_empty() {
            line.push_str(&format!(" — {}", state.detail));
        }
        draw_line(hdc, 24, y, &line, COLORREF(0x00_44_44_44));
        y += 30;
    }
    if let Some(err) = &state.error {
        for (i, seg) in err.lines().take(4).enumerate() {
            draw_line(hdc, 24, y + 8 + (i as i32) * 22, seg, COLORREF(0x00_22_22_cc));
        }
    }
}
