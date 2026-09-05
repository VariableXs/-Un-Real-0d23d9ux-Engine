//! L3 shell — envtoggle.rs（批次E-18 环境切换/退出键）：
//! - 双击 Esc（500ms 内两次按下）= 环境 ↔ Windows 切换：
//!   Variable 可见 → 隐藏（露出真实桌面/壁纸）；隐藏 → 恢复并聚焦。
//!   切换在 Rust 侧直接完成 —— 环境隐藏后 webview 不处理事件，
//!   "再次双击切回"必须不依赖前端监听。
//! - Delete + Backspace 同时按住 = 真正退出：emit `sys://quit-request`
//!   到桌面窗口走完整保存冲刷流程（先 show 保证 webview 活跃可处理）。
//!
//! 实现：GetAsyncKeyState 轮询线程（40ms；与焦点无关，全局键盘状态）。
//! 低级键盘钩子方案已实测被系统静默忽略（回调零派发），不用。

#[cfg(windows)]
use tauri::Manager;

static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

const VK_ESCAPE: i32 = 0x1B;
const VK_DELETE: i32 = 0x2E;
const VK_BACK: i32 = 0x08;

pub fn spawn_env_monitor(app: tauri::AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(poll_loop);
}

#[cfg(windows)]
fn poll_loop() {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    let mut prev_esc = false;
    let mut esc_last_down: Option<std::time::Instant> = None;
    let mut quit_fired = false;
    let mut dbl_fired = false;

    loop {
        std::thread::sleep(std::time::Duration::from_millis(30));
        let esc = unsafe { GetAsyncKeyState(VK_ESCAPE) as u16 & 0x8000 != 0 };
        let del = unsafe { GetAsyncKeyState(VK_DELETE) as u16 & 0x8000 != 0 };
        let back = unsafe { GetAsyncKeyState(VK_BACK) as u16 & 0x8000 != 0 };

        // ---- 双击 Esc → 切换环境/Windows ----
        if esc && !prev_esc {
            let now = std::time::Instant::now();
            match esc_last_down {
                Some(t) if now.duration_since(t) <= std::time::Duration::from_millis(500) => {
                    if !dbl_fired {
                        dbl_fired = true;
                        eprintln!("[env] double-Esc -> toggle environment");
                        if let Some(a) = APP.get() {
                            if let Some(w) = a.get_webview_window("desktop") {
                                match w.is_visible() {
                                    Ok(true) => {
                                        let _ = w.hide();
                                    }
                                    _ => {
                                        let _ = w.show();
                                        let _ = w.unminimize();
                                        let _ = w.set_focus();
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {
                    dbl_fired = false;
                    esc_last_down = Some(now);
                }
            }
        }
        if !esc {
            // 松开后允许下一次双击（保留时间戳用于间隔判定）
            if dbl_fired {
                esc_last_down = None;
            }
        }
        prev_esc = esc;

        // ---- Delete + Backspace 同按 → 真正退出（保存冲刷后关闭） ----
        if del && back {
            if !quit_fired {
                quit_fired = true;
                eprintln!("[env] Del+Backspace -> real quit");
                if let Some(a) = APP.get() {
                    if let Some(w) = a.get_webview_window("desktop") {
                        let _ = w.show();
                        let _ = w.unminimize();
                        let _ = w.set_focus();
                    }
                    use tauri::Emitter;
                    let _ = a.emit_to("desktop", "sys://quit-request", ());
                }
            }
        } else {
            quit_fired = false;
        }
    }
}

#[cfg(not(windows))]
fn poll_loop() {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
