//! L3 shell — xflow.rs（批次 C 规格 5.7 跨软件数据流协议）
//! 跨窗口拖拽的光标跟踪中继：HTML5 拖拽无法跨 WebView 窗口，
//! 源窗口调用 `drag_track` 后由后端轮询全局光标位置并广播事件：
//! - `xflow://drag-move`   {x, y}   物理屏幕坐标（~30ms 节流）
//! - `xflow://drag-drop`   {x, y}   左键释放点
//! - `xflow://drag-cancel` {}       Esc 取消 / 超时
//! 零网络、确定性：纯 Win32 轮询，无全局钩子、无进程注入。

use crate::error::CmdResult;
use serde::Serialize;

/// 单次轮询间隔（ms）：兼顾流畅与 CPU 占用。
const POLL_MS: u64 = 30;
/// 按键抖动窗口（ms）：invoke 到达时左键可能已松开，需先确认按下过。
const ARM_MS: u128 = 200;
/// 硬超时（ms）：防止后端线程在异常情况下永久循环。
const TIMEOUT_MS: u128 = 60_000;

#[derive(Serialize, Clone)]
pub struct DragTrackResult {
    /// 释放点物理屏幕坐标（cancelled 时为最后已知位置）
    pub x: i32,
    pub y: i32,
    /// true = Esc 取消或超时，前端不应执行放置
    pub cancelled: bool,
}

/// 阻塞式轮询循环（在 blocking 线程上运行）。
/// `poll` 返回 (x, y, ldown, esc)；拆出以便单元测试调度逻辑。
fn track_loop<F>(mut poll: F, emit: &dyn Fn(&str, DragTrackResult)) -> DragTrackResult
where
    F: FnMut() -> (i32, i32, bool, bool),
{
    let start = std::time::Instant::now();
    let mut armed = false;
    let mut last;
    loop {
        let (x, y, ldown, esc) = poll();
        last = DragTrackResult { x, y, cancelled: true };
        let elapsed = start.elapsed().as_millis();
        if esc {
            return last;
        }
        if ldown {
            armed = true;
        } else if armed || elapsed > ARM_MS {
            // 按下过且已松开 → 放置；从未按下且超出抖动窗口 → 视为取消
            return DragTrackResult { x, y, cancelled: !armed };
        }
        if elapsed > TIMEOUT_MS {
            return last;
        }
        if armed {
            emit("xflow://drag-move", DragTrackResult { x, y, cancelled: false });
        }
        std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
    }
}

/// 跟踪一次跨窗口拖拽：调用后轮询全局光标直到左键释放 / Esc / 超时。
/// 前端在自定义拖拽开始时 invoke，返回值即释放点；过程事件经 Event Bus 广播。
#[tauri::command]
pub async fn drag_track(app: tauri::AppHandle) -> CmdResult<DragTrackResult> {
    use tauri::Emitter;

    #[cfg(windows)]
    {
        let handle = app.clone();
        let res = tauri::async_runtime::spawn_blocking(move || {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON};
            use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

            track_loop(
                || {
                    let mut pt = POINT { x: 0, y: 0 };
                    let ok = unsafe { GetCursorPos(&mut pt) }.is_ok();
                    let ldown = ok && unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
                    let esc = unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) } < 0;
                    (pt.x, pt.y, ldown, esc)
                },
                &|event, r| {
                    let _ = handle.emit(event, r);
                },
            )
        })
        .await
        .map_err(|e| crate::error::AppError::new("XFLOW", format!("drag track join: {e}")))?;
        Ok(res)
    }

    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(DragTrackResult { x: 0, y: 0, cancelled: true })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 调度逻辑：按下过再松开 → drop（不取消）。
    #[test]
    fn track_loop_drop_after_press() {
        let seq = [
            (10, 10, false, false), // 尚未按下（抖动窗口内）
            (12, 14, true, false),  // 按下 → armed
            (30, 40, false, false), // 松开 → drop
        ];
        let mut i = 0;
        let events = std::cell::RefCell::new(Vec::new());
        let res = track_loop(
            || {
                let s = seq[i];
                i += 1;
                s
            },
            &|ev, r| events.borrow_mut().push((ev.to_string(), r.x, r.y)),
        );
        assert!(!res.cancelled);
        assert_eq!((res.x, res.y), (30, 40));
        let ev = events.borrow();
        assert!(ev.iter().any(|(e, _, _)| e == "xflow://drag-move"));
        assert!(!ev.iter().any(|(e, _, _)| e == "xflow://drag-idle"));
    }

    /// 从未按下且超出抖动窗口 → 取消。
    #[test]
    fn track_loop_cancel_without_press() {
        let mut n = 0u32;
        let res = track_loop(|| { n += 1; (5, 5, false, false) }, &|_, _| {});
        assert!(res.cancelled);
        assert!(n < 20); // ARM_MS/POLL_MS 轮内即返回
    }

    /// Esc 按下 → 立即取消。
    #[test]
    fn track_loop_esc() {
        let res = track_loop(|| (1, 2, true, true), &|_, _| {});
        assert!(res.cancelled);
    }
}
