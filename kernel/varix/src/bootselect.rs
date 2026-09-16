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

    // 「配置已重置」角标（任务4：boot-select.json 整体损坏时）
    if cfg_reset_badge_on() {
        draw_cfg_reset_badge(surf);
    }

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

// ---------------------------------------------------------------------------
// 「配置已重置」角标（任务4 容错第 3 层的视觉面）
// ---------------------------------------------------------------------------

use core::sync::atomic::{AtomicBool, Ordering};

static CFG_RESET_BADGE: AtomicBool = AtomicBool::new(false);

/// 共享分区 boot-select.json 整体损坏（`bootcfg::CfgSource::Reset`）时置位：
/// 菜单右上角逐帧画角标，如实告知用户配置未生效、已回内置默认。
pub fn set_cfg_reset_badge(on: bool) {
    CFG_RESET_BADGE.store(on, Ordering::Relaxed);
}

fn cfg_reset_badge_on() -> bool {
    CFG_RESET_BADGE.load(Ordering::Relaxed)
}

const BADGE_BOX: Color = Color::rgb(0xFF, 0xB4, 0x3C);
const BADGE_INK: Color = Color::rgb(0x1C, 0x14, 0x08);

/// 右上角角标：amber 底 + 深字，与菜单主题区分（警示语义）。
fn draw_cfg_reset_badge(surf: &Surface) {
    let text = "CONFIG RESET: DEFAULTS APPLIED";
    let tw = crate::font::text_width_scaled(text, 1) as i64;
    let pad = 10i64;
    let bw = tw + pad * 2;
    let bh = 26i64;
    let x = surf.width() as i64 - bw - 16;
    surf.fill_rect(x, 16, bw, bh, BADGE_BOX);
    crate::font::draw_text(surf, x + pad, 16 + (bh - 16) / 2, text, BADGE_INK);
}

/// 键事件来源：目标态走 PS/2 轮询；宿主测试注入脚本化按键序列。
type KeySource<'a> = &'a mut dyn FnMut() -> Option<crate::ps2::Key>;

/// 倒计时轮询循环（任务1）：每秒切成 `POLL_SLICES` 片，片间轮询按键。
/// - ↑/↓：移动选中并立即重画（不重置倒计时——总案口径：倒计时照走）；
/// - Enter：立即返回当前选中；
/// - 归零：返回**默认项**（任务5 键盘拔除演练定版：↑↓ 只是预选，
///   未经 Enter 确认的选择不生效——倒计时自动执行默认拒绝原则，
///   切 Windows/固件这类重动作必须显式 Enter）。
///
/// 返回最终选中下标。
pub fn run_countdown(surf: &Surface, timeout_secs: u32, tsc_hz: u64) -> usize {
    // 目标态键源：PS/2 轮询。
    let mut hw = || crate::ps2::poll_key();
    run_countdown_with(surf, timeout_secs, tsc_hz, &mut hw)
}

/// 可注入键源的循环体（宿主测试与目标共用同一逻辑）。
/// 每帧先重绘背板再画菜单——清屏策略单一来源，杜绝倒计时/选中残影（任务3）。
pub fn run_countdown_with(
    surf: &Surface,
    timeout_secs: u32,
    tsc_hz: u64,
    keys: KeySource,
) -> usize {
    /// 每秒轮询片数：50 片 × 20ms，按键响应 ≤20ms。
    const POLL_SLICES: u32 = 50;
    let opts = crate::bootopt::options();
    let mut sel = default_index(opts.default_entry);
    let mut remaining = timeout_secs;
    let slice_ticks = tsc_hz / POLL_SLICES as u64;
    draw_frame(surf, remaining, sel);
    loop {
        if remaining == 0 {
            // 归零执行默认项（未确认的 ↑↓ 预选不生效——键盘拔除/无人
            // 操作时系统回落配置默认，见任务5 演练矩阵场景2）。
            return default_index(opts.default_entry);
        }
        for _ in 0..POLL_SLICES {
            // 消费本轮已积累的按键（一片内可能有多键），↑/↓ 立即重画。
            while let Some(k) = keys() {
                match k {
                    crate::ps2::Key::Up => {
                        sel = sel.saturating_sub(1);
                        draw_frame(surf, remaining, sel);
                    }
                    crate::ps2::Key::Down => {
                        sel = (sel + 1).min(crate::bootselect::ENTRIES.len() - 1);
                        draw_frame(surf, remaining, sel);
                    }
                    crate::ps2::Key::Enter => return sel,
                }
            }
            wait_ticks(slice_ticks);
        }
        remaining -= 1;
        draw_frame(surf, remaining, sel);
    }
}

/// 一帧 = 背板重绘 + 菜单绘制。清屏策略在此归口。
fn draw_frame(surf: &Surface, remaining: u32, sel: usize) {
    crate::banner::paint_backdrop(surf);
    draw(surf, remaining, sel);
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

    /// 任务4 容错第 3 层视觉面：角标置位时菜单右上角必须出现 amber 警示块。
    #[test]
    fn cfg_reset_badge_renders_when_flag_set() {
        let (s, _b) = surface(800, 600);
        set_cfg_reset_badge(true);
        draw(&s, 5, 0);
        set_cfg_reset_badge(false);
        // 角标底色（amber）与角标深色文字都必须出现
        assert!(count_px(&s, BADGE_BOX) > 500);
        assert!(count_px(&s, BADGE_INK) > 0);
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

    // ---- 任务1：键盘选择路径（注入键源，tsc_hz 调小让宿主忙等可忽略） ----

    /// 从按键序列构造键源。
    fn scripted(keys: &[crate::ps2::Key]) -> impl FnMut() -> Option<crate::ps2::Key> + '_ {
        let mut i = 0usize;
        move || {
            let r = keys.get(i).copied();
            i += 1;
            r
        }
    }

    const FAST_HZ: u64 = 100_000; // 宿主测试：slice 忙等 ≈ 微秒级

    #[test]
    fn enter_selects_current_highlight() {
        let (s, _b) = surface(800, 600);
        let mut keys = scripted(&[crate::ps2::Key::Down, crate::ps2::Key::Down, crate::ps2::Key::Enter]);
        let sel = run_countdown_with(&s, 5, FAST_HZ, &mut keys);
        assert_eq!(sel, 2, "两次 ↓ + Enter 应选中第三项");
    }

    #[test]
    fn no_keys_counts_down_to_default() {
        let (s, _b) = surface(800, 600);
        let mut none = || -> Option<crate::ps2::Key> { None };
        let sel = run_countdown_with(&s, 2, FAST_HZ, &mut none);
        assert_eq!(sel, 0, "无按键应倒计时归零走默认项");
    }

    #[test]
    fn up_clamps_at_top() {
        let (s, _b) = surface(800, 600);
        let mut keys = scripted(&[
            crate::ps2::Key::Up,
            crate::ps2::Key::Up,
            crate::ps2::Key::Enter,
        ]);
        let sel = run_countdown_with(&s, 5, FAST_HZ, &mut keys);
        assert_eq!(sel, 0, "↑ 在顶部应钳位");
    }

    #[test]
    fn down_clamps_at_bottom() {
        let (s, _b) = surface(800, 600);
        let mut keys = scripted(&[
            crate::ps2::Key::Down,
            crate::ps2::Key::Down,
            crate::ps2::Key::Down,
            crate::ps2::Key::Down,
            crate::ps2::Key::Down,
            crate::ps2::Key::Enter,
        ]);
        let sel = run_countdown_with(&s, 5, FAST_HZ, &mut keys);
        assert_eq!(sel, 2, "↓ 在底部应钳位");
    }

    #[test]
    fn up_then_enter_moves_back() {
        let (s, _b) = surface(800, 600);
        let mut keys = scripted(&[
            crate::ps2::Key::Down,
            crate::ps2::Key::Up,
            crate::ps2::Key::Enter,
        ]);
        let sel = run_countdown_with(&s, 5, FAST_HZ, &mut keys);
        assert_eq!(sel, 0);
    }

    #[test]
    fn keys_do_not_reset_countdown_expiry() {
        // 任务5 定版：键按了但从不 Enter（如键盘中途拔除）——倒计时
        // 归零回落**默认项**，未经确认的 ↑↓ 预选不生效（默认拒绝）。
        let (s, _b) = surface(800, 600);
        let mut one_down_then_none = {
            let mut fired = false;
            move || {
                if fired {
                    None
                } else {
                    fired = true;
                    Some(crate::ps2::Key::Down)
                }
            }
        };
        let sel = run_countdown_with(&s, 1, FAST_HZ, &mut one_down_then_none);
        assert_eq!(sel, 0, "归零应回落默认项而非未确认的预选项");
    }

    #[test]
    fn frame_repaint_erases_previous_highlight() {
        // 任务3：帧重绘策略必须抹掉上一帧的选中高亮（零残影）。
        let (s, _b) = surface(1280, 720);
        draw(&s, 5, 0);
        assert!(count_px(&s, HL_BOX) > 10_000);
        draw_frame(&s, 5, 1); // 下一帧选中下移
        // 第一张卡片区域的旧高亮必须消失：整屏不再有属于卡片 0 行的高亮色
        // （卡片 1 的高亮在其行内；断言卡片 0 行带内无高亮像素）。
        let (_, my, _, ch, _) = metrics(&s);
        let mut leaked = 0u64;
        for y in my..(my + ch) {
            for x in 0..s.width() as i64 {
                if s.get_px(x, y) == Some(PixelFormat::Bgr32.pack(HL_BOX)) {
                    leaked += 1;
                }
            }
        }
        assert_eq!(leaked, 0, "上一帧高亮残影未清除");
    }

    /// 多分辨率整页渲染归档（任务3）：VARIX_RENDER_MENU=1 cargo ktest -- bootselect::
    /// 产出 800×600 / 1280×720 / 1920×1080 的 PPM 到 docs/acceptance 归档目录。
    #[test]
    fn render_archive_multi_resolution() {
        if std::env::var("VARIX_RENDER_MENU").unwrap_or_default() != "1" {
            return;
        }
        let dir = "docs/acceptance/2026-09-16-任务3-选择页视觉收口";
        std::fs::create_dir_all(dir).unwrap();
        for (w, h) in [(800u32, 600u32), (1280, 720), (1920, 1080)] {
            let (s, buf) = surface(w, h);
            draw_frame(&s, 5, 0);
            let path = format!("{}/menu-{}x{}.ppm", dir, w, h);
            let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
            // Surface 背板为 BGR32：转成 RGB 字节序输出。
            for px in buf.chunks_exact(4) {
                out.push(px[2]);
                out.push(px[1]);
                out.push(px[0]);
            }
            std::fs::write(&path, out).unwrap();
        }
    }
}
