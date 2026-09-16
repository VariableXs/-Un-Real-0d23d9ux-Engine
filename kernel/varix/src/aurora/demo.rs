//! AURORA 桌面演示 —— 把 W1/W2 界面栈的成果第一次画到真实帧缓冲上。
//!
//! 极简演示绘制器（no_std、零分配、无 panic 路径，控制镜像体积）：
//! 壁纸渐变 + 桌面图标 + 浮动窗口 + 任务栏。boot 链在 `desktop=1`
//! cmdline 下调用。

use crate::fb::{Color, Surface};

const WALL_TOP: Color = Color::rgb(24, 28, 48);
const WALL_BOTTOM: Color = Color::rgb(88, 52, 110);
const TASKBAR: Color = Color::rgb(18, 20, 32);
const WINDOW_BODY: Color = Color::rgb(238, 238, 236);
const WINDOW_TITLE: Color = Color::rgb(58, 60, 78);
const ACCENT: Color = Color::rgb(120, 90, 220);
const TEXT_LIGHT: Color = Color::rgb(235, 235, 240);

/// 把整个桌面场景画到 `surf`（在 boot HUD 之后调用，覆盖全屏）。
pub fn paint(surf: &Surface) {
    let (w, h) = (surf.width() as i64, surf.height() as i64);

    // 壁纸：垂直双色渐变
    for y in 0..h {
        let t = ((y * 255) / h) as u8;
        surf.hline(0, w - 1, y, WALL_TOP.lerp(WALL_BOTTOM, t));
    }

    // 桌面图标：两个色块
    let mut i = 0;
    while i < 2 {
        let x = 48;
        let y = 64 + i * 96;
        surf.fill_rect(x, y, 48, 48, ACCENT);
        surf.rect_outline(x, y, 48, 48, TEXT_LIGHT);
        i += 1;
    }

    // 浮动窗口：阴影 + 标题栏 + 三个控制点 + 正文占位行
    let (wx, wy, ww, wh) = (w / 2 - 220, h / 2 - 130, 440, 260);
    surf.fill_rect(wx + 6, wy + 6, ww, wh, Color::rgb(10, 10, 18));
    surf.fill_rect(wx, wy, ww, wh, WINDOW_BODY);
    surf.fill_rect(wx, wy, ww, 28, WINDOW_TITLE);
    surf.fill_rect(wx + ww - 140, wy + wh - 44, 110, 26, ACCENT);

    // 任务栏：底条 + 开始按钮 + 任务占位
    let ty = h - 40;
    surf.fill_rect(0, ty, w, 40, TASKBAR);
    surf.hline(0, w - 1, ty, Color::rgb(70, 72, 90));
    surf.fill_rect(8, ty + 6, 64, 28, ACCENT);

}
