//! 引导选择页（双域总案·阶段0 第1步）— 开机菜单渲染：三个启动项卡片
//! + 倒计时行，默认项在倒计时归零后自动选定。
//!
//! 本模块只负责「画出来 + 数完倒计时」两件确定的事：
//! - 绘制全部走 `Surface`/`font` 既有原语，宿主测试用 `Vec` 背板（同 fb/font 范式）；
//! - 倒计时文案复用 `bootopt::countdown_line`（策略与显示单一来源）。
//!
//! 刻意不在这里做的：键盘上下键选择（等 input 域轮询接线后接 `selected`
//! 参数）、UEFI BootNext 写入与链式引导（引导器侧职责）。选中项之外的
//! 入口当前如实记录「chainload 未接线」并继续引导 varix——不假装能切。

use crate::fb::{Color, Surface};

/// 菜单项：id 与 bootopt 的 default_entry 词表一致。
pub struct Entry {
    pub id: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
}

/// 三个启动项。顺序即屏幕顺序，默认项由 bootopt 决定。
pub const ENTRIES: [Entry; 3] = [
    Entry {
        id: "varix",
        title: "VARIX + VARIABLE",
        subtitle: "Private Desktop Environment",
    },
    Entry {
        id: "windows",
        title: "WINDOWS",
        subtitle: "Full native Windows session",
    },
    Entry {
        id: "uefi",
        title: "FIRMWARE SETUP",
        subtitle: "UEFI configuration",
    },
];

/// 主题色（与 banner 背板同族：深空底、亮字、选中高亮）。
const INK_TITLE: Color = Color::rgb(0xF2, 0xF5, 0xFA);
const INK_SUB: Color = Color::rgb(0x9A, 0xA6, 0xB8);
const INK_DIM: Color = Color::rgb(0x6B, 0x74, 0x86);
const HL_TITLE: Color = Color::rgb(0x10, 0x14, 0x1C);
const HL_BOX: Color = Color::rgb(0x53, 0xB1, 0xFF);
const BOX_DIM: Color = Color::rgb(0x2A, 0x33, 0x44);

/// 默认项在菜单里的下标；bootopt 词表外的值一律落回 varix。
pub fn default_index(default_entry: &str) -> usize {
    ENTRIES
        .iter()
        .position(|e| e.id == default_entry)
        .unwrap_or(0)
}

/// 菜单几何：居中卡片列。返回 (x, y0, w, card_h, gap)。
fn metrics(surf: &Surface) -> (i64, i64, i64, i64, i64) {
    let w = surf.width() as i64;
    let h = surf.height() as i64;
    let card_w = (w * 5 / 8).clamp(280, 720);
    let card_h = 72i64;
    let gap = 16i64;
    let total = ENTRIES.len() as i64 * card_h + (ENTRIES.len() as i64 - 1) * gap;
    let x = (w - card_w) / 2;
    let y0 = (h - total) / 2 - 24;
    (x, y0, card_w, card_h, gap)
}

/// 画整个选择页：标题、三项卡片（选中项反色高亮）、底部倒计时行。
/// 返回菜单底边 y（调用方在其下排 HUD）。
pub fn draw(surf: &Surface, remaining: u32, selected: usize) -> i64 {
    let w = surf.width() as i64;
    let (mx, my, mw, ch, gap) = metrics(surf);

    // 标题行
    let title = "SELECT AN OPERATING SYSTEM";
    let tw = crate::font::text_width_scaled(title, 2);
    crate::font::draw_text_scaled(surf, (w - tw) / 2, my - 72, title, INK_TITLE, 2);

    // 三张卡片
    for (i, e) in ENTRIES.iter().enumerate() {
        let cy = my + i as i64 * (ch + gap);
        let is_sel = i == selected;
        surf.fill_rect(mx, cy, mw, ch, if is_sel { HL_BOX } else { BOX_DIM });
        if is_sel {
            surf.rect_outline(mx - 2, cy - 2, mw + 4, ch + 4, HL_BOX);
        }
        let pad = 20i64;
        crate::font::draw_text_scaled(
            surf,
            mx + pad,
            cy + 12,
            e.title,
            if is_sel { HL_TITLE } else { INK_TITLE },
            2,
        );
        crate::font::draw_text(surf, mx + pad, cy + ch - 22, e.subtitle, INK_SUB);
    }

    // 倒计时行（bootopt 单一来源）
    let mut buf = [0u8; 48];
    let n = crate::bootopt::options().countdown_line(remaining, &mut buf);
    let line = core::str::from_utf8(&buf[..n]).unwrap_or("");
    let lw = crate::font::text_width_scaled(line, 1);
    crate::font::draw_text_scaled(
        surf,
        (w - lw) / 2,
        my + ENTRIES.len() as i64 * (ch + gap) + 8,
        line,
        INK_DIM,
        1,
    );

    my + ENTRIES.len() as i64 * (ch + gap) + 24
}

/// 读 TSC（与 timeline 同一来源）。
fn now() -> u64 {
    crate::timeline::read_tsc()
}

/// 忙等 `hz` 个 TSC tick ≈ 1 秒（引导期单核、中断未开，忙等即正确）。
fn wait_ticks(ticks: u64) {
    let start = now();
    while now().wrapping_sub(start) < ticks {
        core::hint::spin_loop();
    }
}

/// 倒计时主循环：从 `timeout_secs` 每秒重画一次直到归零。
/// 返回最终选中下标（默认项；键盘接线前没有别的途径改变它）。
pub fn run_countdown(surf: &Surface, timeout_secs: u32, tsc_hz: u64) -> usize {
    let opts = crate::bootopt::options();
    let sel = default_index(opts.default_entry);
    let mut remaining = timeout_secs;
    loop {
        draw(surf, remaining, sel);
        if remaining == 0 {
            return sel;
        }
        wait_ticks(tsc_hz);
        remaining -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fb::PixelFormat;
    use std::vec::Vec;

    fn surface(w: u32, h: u32) -> (Surface, Vec<u8>) {
        let mut v: Vec<u8> = std::vec![0u8; (w * h * 4) as usize];
        let s = unsafe { Surface::from_raw(v.as_mut_ptr(), w, h, w * 4, PixelFormat::Bgr32) };
        (s, v)
    }

    /// 高亮色像素计数：用于断言选中卡片确实被画成 HL_BOX。
    fn count_px(s: &Surface, c: Color) -> u64 {
        let mut n = 0u64;
        for y in 0..s.height() as i64 {
            for x in 0..s.width() as i64 {
                if s.get_px(x, y) == Some(PixelFormat::Bgr32.pack(c)) {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn default_index_maps_bootopt_vocab() {
        assert_eq!(default_index("varix"), 0);
        assert_eq!(default_index("windows"), 1);
        assert_eq!(default_index("uefi"), 2);
        assert_eq!(default_index("other"), 0); // 词表外落回 varix
        assert_eq!(default_index(""), 0);
    }

    #[test]
    fn draw_renders_three_cards_and_countdown() {
        let (s, _b) = surface(800, 600);
        let bottom = draw(&s, 5, 0);
        assert!(bottom > 0 && bottom < 600);
        // 选中卡片：HL_BOX 填充必须有大量像素
        assert!(count_px(&s, HL_BOX) > 10_000);
        // 未选中卡片底色也存在
        assert!(count_px(&s, BOX_DIM) > 10_000);
        // 倒计时文字为亮色，标题文字存在
        assert!(count_px(&s, INK_TITLE) > 0);
        assert!(count_px(&s, INK_DIM) > 0);
    }

    #[test]
    fn draw_selection_moves_highlight() {
        let (s1, _b1) = surface(800, 600);
        let (s2, _b2) = surface(800, 600);
        draw(&s1, 3, 0);
        draw(&s2, 3, 1);
        // 两帧画面对比：高亮填充首像素出现在不同卡片行（卡片 2 标题更短，
        // 文字覆盖后高亮面积略有差异，故只断言位置移动而非面积相等）。
        fn first_hl_y(s: &Surface) -> i64 {
            for y in 0..s.height() as i64 {
                for x in 0..s.width() as i64 {
                    if s.get_px(x, y) == Some(PixelFormat::Bgr32.pack(HL_BOX)) {
                        return y;
                    }
                }
            }
            -1
        }
        let y1 = first_hl_y(&s1);
        let y2 = first_hl_y(&s2);
        assert!(y1 >= 0 && y2 > y1, "选中高亮应随 selected 下移: {y1} -> {y2}");
    }

    #[test]
    fn countdown_zero_still_renders_final_frame() {
        let (s, _b) = surface(800, 600);
        let bottom = draw(&s, 0, 0);
        assert!(bottom > 0);
        assert!(count_px(&s, HL_BOX) > 10_000);
    }

    #[test]
    fn metrics_fit_small_and_large_screens() {
        for (w, h) in [(640u32, 480u32), (1024, 768), (1920, 1080), (2560, 1440)] {
            let (s, _b) = surface(w, h);
            let (mx, my, mw, ch, gap) = metrics(&s);
            assert!(mx > 0 && mw > 0 && ch > 0 && gap > 0);
            let bottom = my + ENTRIES.len() as i64 * (ch + gap);
            assert!(bottom < h as i64, "{}x{} 菜单越界", w, h);
            assert!(mx + mw <= w as i64);
        }
    }
}
