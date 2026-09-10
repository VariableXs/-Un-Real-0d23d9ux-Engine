//! L3 shell — container.rs（批次C-3 L2 容器包裹引擎）：
//! - 适用：自绘/非标框架窗口（探测器 C-6 判定 L2：无标准非客户区/自绘边框）。
//! - Variable 创建自己的 WS_POPUP 宿主窗口（NativeWindow，非 webview）→
//!   把第三方窗口 SetParent 进宿主客户区（**不剥其样式**，只加 WS_CHILD）→
//!   宿主窗口作为 VWM 嵌入对象（embed_bounds/embed_visible/embed_focus 全部代理）。
//! - 拖拽/缩放代理：宿主 WM_SIZE → SetWindowPos 同步第三方窗口到客户区。
//! - 键盘焦点代理：宿主 WM_SETFOCUS → SetFocus(第三方子窗口)。
//! - 宿主关闭 = WM_CLOSE 链路转发给第三方（不杀进程）。
//! - 宿主窗口在主线程创建（SetParent/DestroyWindow 的线程亲和性），
//!   命令线程经 run_on_main_thread + channel 等待结果。

#[cfg(windows)]
pub mod win {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::COLOR_WINDOW;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetDesktopWindow,
        GetWindowLongPtrW, RegisterClassW, SetParent, SetWindowLongPtrW, SetWindowPos,
        CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, GWL_STYLE, SWP_NOZORDER,
        WINDOW_EX_STYLE, WM_CLOSE, WM_SETFOCUS, WM_SIZE, WNDCLASSW, WS_CHILD, WS_POPUP,
        WS_VISIBLE,
    };

    /// 宿主窗口类名（进程内唯一）。
    pub const HOST_CLASS: &str = "VariableContainerHost";

    /// host hwnd → 第三方子窗口 hwnd（窗口过程与命令两侧共用）。
    static CHILD_OF: Mutex<Option<HashMap<isize, isize>>> = Mutex::new(None);

    fn child_of_insert(host: isize, child: isize) {
        let mut g = CHILD_OF.lock().unwrap_or_else(|e| e.into_inner());
        g.get_or_insert_with(HashMap::new).insert(host, child);
    }

    pub fn child_of(host: isize) -> Option<isize> {
        let g = CHILD_OF.lock().unwrap_or_else(|e| e.into_inner());
        g.as_ref().and_then(|m| m.get(&host).copied())
    }

    pub fn child_of_remove(host: isize) -> Option<isize> {
        let mut g = CHILD_OF.lock().unwrap_or_else(|e| e.into_inner());
        g.as_mut().and_then(|m| m.remove(&host))
    }

    /// 宿主窗口过程：WM_SIZE 同步子窗口到客户区；WM_SETFOCUS 移交键盘焦点；
    /// WM_CLOSE 转发给子窗口后自毁（链路关闭，不杀进程）。
    unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        let key = hwnd.0 as isize;
        match msg {
            WM_SIZE => {
                if let Some(child) = child_of(key) {
                    let w = (lp.0 & 0xFFFF) as i32;
                    let h = ((lp.0 >> 16) & 0xFFFF) as i32;
                    if w > 0 && h > 0 {
                        let _ = SetWindowPos(
                            HWND(child as *mut core::ffi::c_void),
                            HWND::default(),
                            0,
                            0,
                            w,
                            h,
                            SWP_NOZORDER,
                        );
                    }
                }
                LRESULT(0)
            }
            WM_SETFOCUS => {
                if let Some(child) = child_of(key) {
                    let _ = SetFocus(HWND(child as *mut core::ffi::c_void));
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let child = child_of(key);
                if let Some(c) = child {
                    // WM_CLOSE 链路：应用自行决定退出（不杀进程）
                    windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        HWND(c as *mut core::ffi::c_void),
                        WM_CLOSE,
                        WPARAM(0),
                        LPARAM(0),
                    );
                    // 脱离子窗口（恢复顶层），随后自毁宿主
                    let ch = HWND(c as *mut core::ffi::c_void);
                    let style = GetWindowLongPtrW(ch, GWL_STYLE) as isize;
                    SetWindowLongPtrW(
                        ch,
                        GWL_STYLE,
                        ((style as u32 & !WS_CHILD.0) | WS_POPUP.0) as isize,
                    );
                    let _ = SetParent(ch, GetDesktopWindow());
                }
                child_of_remove(key);
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    /// 注册宿主窗口类（幂等）。
    unsafe fn ensure_class() -> bool {
        static DONE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        if DONE.set(()).is_ok() {
            let name: Vec<u16> = HOST_CLASS.encode_utf16().chain([0]).collect();
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(host_proc),
                hInstance: windows::Win32::Foundation::HINSTANCE(
                    GetModuleHandleW(None).ok().unwrap_or_default().0,
                ),
                hCursor: windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                    None,
                    windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
                )
                .unwrap_or_default(),
                hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(COLOR_WINDOW.0 as *mut _),
                lpszClassName: windows::core::PCWSTR(name.as_ptr()),
                ..Default::default()
            };
            let atom = RegisterClassW(&wc);
            return atom != 0;
        }
        true
    }

    /// 在主线程创建宿主窗口（Tauri run_on_main_thread），返回 host hwnd。
    /// 调用方（命令线程）阻塞等待 ≤5s。
    pub fn create_host_on_main_thread(app: &tauri::AppHandle) -> Option<isize> {
        use std::sync::mpsc;
        let (tx, rx) = mpsc::channel::<Option<isize>>();
        let _ = app.run_on_main_thread(move || {
            let created = unsafe {
                if !ensure_class() {
                    None
                } else {
                    let name: Vec<u16> = HOST_CLASS.encode_utf16().chain([0]).collect();
                    let h = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        windows::core::PCWSTR(name.as_ptr()),
                        windows::core::w!(""),
                        WS_POPUP | WS_VISIBLE,
                        CW_USEDEFAULT,
                        CW_USEDEFAULT,
                        900,
                        600,
                        GetDesktopWindow(),
                        None,
                        GetModuleHandleW(None).ok().unwrap_or_default(),
                        None,
                    );
                    h.ok().map(|w| w.0 as isize)
                }
            };
            let _ = tx.send(created);
        });
        rx.recv_timeout(std::time::Duration::from_secs(5)).ok().flatten()
    }

    /// L2 包裹：把第三方窗口 SetParent 进宿主客户区（不剥样式，仅加 WS_CHILD）。
    /// 返回是否成功。
    pub fn wrap_child(host: isize, child: isize) -> bool {
        let h = HWND(host as *mut core::ffi::c_void);
        let c = HWND(child as *mut core::ffi::c_void);
        unsafe {
            let style = GetWindowLongPtrW(c, GWL_STYLE) as isize;
            SetWindowLongPtrW(c, GWL_STYLE, (style as u32 | WS_CHILD.0) as isize);
            SetParent(c, h);
            child_of_insert(host, child);
            // 立即同步一次客户区尺寸
            let mut rc = windows::Win32::Foundation::RECT::default();
            let _ = GetClientRect(h, &mut rc);
            let _ = SetWindowPos(
                c,
                HWND::default(),
                0,
                0,
                rc.right - rc.left,
                rc.bottom - rc.top,
                SWP_NOZORDER,
            );
        }
        true
    }

    /// 脱离：恢复第三方窗口为顶层（不杀进程）；宿主自毁。
    pub fn unwrap_child(host: isize) {
        if let Some(child) = child_of_remove(host) {
            unsafe {
                let c = HWND(child as *mut core::ffi::c_void);
                let style = GetWindowLongPtrW(c, GWL_STYLE) as isize;
                SetWindowLongPtrW(
                    c,
                    GWL_STYLE,
                    ((style as u32 & !WS_CHILD.0) | WS_POPUP.0) as isize,
                );
                let _ = SetParent(c, GetDesktopWindow());
            }
        }
    }

    #[cfg(test)]
    mod tests {
        /// 宿主→子窗口映射语义：插入/查询/移除（进程内映射，跨线程共享）。
        #[test]
        fn child_of_map_semantics() {
            // 未插入时查询为空
            assert_eq!(super::child_of(424242), None);
            super::child_of_insert(424242, 1001);
            assert_eq!(super::child_of(424242), Some(1001));
            // 覆盖语义：同宿主重包 = 替换
            super::child_of_insert(424242, 1002);
            assert_eq!(super::child_of(424242), Some(1002));
            // 移除返回旧值；再次查询为空
            assert_eq!(super::child_of_remove(424242), Some(1002));
            assert_eq!(super::child_of(424242), None);
        }
    }
}
