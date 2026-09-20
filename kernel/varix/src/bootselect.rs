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
use crate::inputsvc::MouseDelta;

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

/// 鼠标来源：每次调用取走一个累计位移包（`None` = 本片没有鼠标数据）。
/// 宿主测试与无鼠标机器传恒 `None` 的源，复用同一份循环体。
type MouseSource<'a> = &'a mut dyn FnMut() -> Option<MouseDelta>;

/// 屏幕指针（纯几何，不碰端口——宿主可完整单测）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pointer {
    pub x: i64,
    pub y: i64,
    /// 是否画出来：收到第一个位移包之前不画，没插鼠标时屏幕保持原样。
    pub visible: bool,
}

impl Pointer {
    /// 初始指针落在屏幕中心，且不可见。
    pub fn centered(surf: &Surface) -> Pointer {
        Pointer {
            x: (surf.width() as i64) / 2,
            y: (surf.height() as i64) / 2,
            visible: false,
        }
    }

    /// 累加一个位移包并夹在屏幕内。
    ///
    /// PS/2 的 `dy` 为正表示**向上**，而屏幕 y 轴向下，故 y 取反。
    pub fn apply(&mut self, d: &MouseDelta, surf: &Surface) {
        let w = surf.width() as i64;
        let h = surf.height() as i64;
        self.x = (self.x + d.dx as i64).clamp(0, w - 1);
        self.y = (self.y - d.dy as i64).clamp(0, h - 1);
        self.visible = true;
    }
}

/// 第 i 张卡片的矩形 `(x, y, w, h)`。
///
/// 与 [`draw`] 共用同一个 [`metrics`]——「看到的高亮框」和「点得中的区域」
/// 必须逐像素同源，否则会出现"看着在第一张、点了进第二张"。
pub fn card_rect(surf: &Surface, i: usize) -> (i64, i64, i64, i64) {
    let (mx, my, mw, ch, gap) = metrics(surf);
    (mx, my + i as i64 * (ch + gap), mw, ch)
}

/// 命中测试：指针落在第几张卡片上，不在任何卡片上返回 `None`。
///
/// 外扩 2px 与选中项的描边对齐（`draw` 里描边画在 `mx-2`），让反色卡片
/// 和它那圈描边一样点得中——不做"看得见却点不到"的手感陷阱。
pub fn hit_card(surf: &Surface, px: i64, py: i64) -> Option<usize> {
    for i in 0..ENTRIES.len() {
        let (x, y, w, h) = card_rect(surf, i);
        if px >= x - 2 && px < x + w + 2 && py >= y - 2 && py < y + h + 2 {
            return Some(i);
        }
    }
    None
}

/// 指针形状：实心箭头（尖朝左上）+ 1px 深色描边。
///
/// 先画大一圈的暗三角再叠亮芯，等效描边且不引入新原语；深浅两种背板
/// （卡片高亮是亮蓝、背板是深空）上都保持可辨。
pub fn draw_cursor(surf: &Surface, p: &Pointer) {
    if !p.visible {
        return;
    }
    const OUT: Color = Color::rgb(0x0B, 0x0E, 0x14);
    const IN: Color = Color::rgb(0xF2, 0xF5, 0xFA);
    for r in 0..16i64 {
        surf.fill_rect(p.x, p.y + r, r + 1, 1, OUT);
    }
    for r in 0..16i64 {
        let w = r.saturating_sub(2);
        if w > 0 {
            surf.fill_rect(p.x + 1, p.y + r, w, 1, IN);
        }
    }
}

/// 倒计时轮询循环（任务1）：每秒切成 `POLL_SLICES` 片，片间轮询按键。
/// - ↑/↓：移动选中并立即重画（不重置倒计时——总案口径：倒计时照走）；
/// - Enter：立即返回当前选中；
/// - 归零：返回**默认项**（任务5 键盘拔除演练定版：↑↓ 只是预选，
///   未经 Enter 确认的选择不生效——倒计时自动执行默认拒绝原则，
///   切 Windows/固件这类重动作必须显式 Enter）。
///
/// 返回最终选中下标。
///
/// 鼠标与键盘**权力对等**（需求 1/6）：移动 = 悬停改高亮（等价 ↑↓ 预选，
/// 不确认），左键单击卡片 = 确认进入（等价 Enter）。倒计时语义一字未改。
#[cfg(target_os = "none")]
pub fn run_countdown(surf: &Surface, timeout_secs: u32, tsc_hz: u64) -> usize {
    // 键鼠共用一个端口泵：AUX 标志位分流，两边不会互吃字节。
    let mut hw = || crate::inputsvc::target::menu_poll_key();
    let mut mw = || crate::inputsvc::target::menu_poll_mouse();
    run_countdown_with_input(surf, timeout_secs, tsc_hz, &mut hw, &mut mw)
}

/// 宿主态：没有端口，键鼠源恒空（宿主测试走注入键源）。
#[cfg(not(target_os = "none"))]
pub fn run_countdown(surf: &Surface, timeout_secs: u32, tsc_hz: u64) -> usize {
    let mut hw = || crate::ps2::poll_key();
    let mut mw = || None::<MouseDelta>;
    run_countdown_with_input(surf, timeout_secs, tsc_hz, &mut hw, &mut mw)
}

/// 可注入键源的循环体（纯键盘；宿主既有测试沿用此入口，行为零变化）。
pub fn run_countdown_with(
    surf: &Surface,
    timeout_secs: u32,
    tsc_hz: u64,
    keys: KeySource,
) -> usize {
    let mut no_mouse = || None::<MouseDelta>;
    run_countdown_with_input(surf, timeout_secs, tsc_hz, keys, &mut no_mouse)
}

/// 键鼠双源循环体（宿主测试与目标共用同一逻辑）。
///
/// 鼠标语义与键盘严格对齐，**不引入键盘没有的权力**：
/// - 移动 → 显现指针；悬停到某张卡 = 改高亮（等价 ↑↓ 预选，**不确认**）；
/// - 左键单击卡片 = 确认进入（等价 Enter）；点在卡片外 = 静默忽略；
/// - 倒计时归零一律走配置默认项（未确认的预选不生效，与键盘语义一致）。
///
/// 每帧先重绘背板再画菜单——清屏策略单一来源，杜绝倒计时/选中残影（任务3）。
pub fn run_countdown_with_input(
    surf: &Surface,
    timeout_secs: u32,
    tsc_hz: u64,
    keys: KeySource,
    mice: MouseSource<'_>,
) -> usize {
    /// 每秒轮询片数：50 片 × 20ms，键鼠响应 ≤20ms。
    const POLL_SLICES: u32 = 50;
    /// 左键位（PS/2 包字节 0 的 bit0）。
    const BTN_LEFT: u8 = 0x01;
    let opts = crate::bootopt::options();
    let mut sel = default_index(opts.default_entry);
    let mut remaining = timeout_secs;
    let slice_ticks = tsc_hz / POLL_SLICES as u64;
    let mut ptr = Pointer::centered(surf);
    draw_frame(surf, remaining, sel, &ptr);
    loop {
        if remaining == 0 {
            // 归零执行默认项（未确认的 ↑↓/悬停 预选不生效——键盘拔除/无人
            // 操作时系统回落配置默认，见任务5 演练矩阵场景2）。
            return default_index(opts.default_entry);
        }
        for _ in 0..POLL_SLICES {
            // 消费本轮已积累的按键（一片内可能有多键），↑/↓ 立即重画。
            while let Some(k) = keys() {
                match k {
                    crate::ps2::Key::Up => {
                        sel = sel.saturating_sub(1);
                        draw_frame(surf, remaining, sel, &ptr);
                    }
                    crate::ps2::Key::Down => {
                        sel = (sel + 1).min(crate::bootselect::ENTRIES.len() - 1);
                        draw_frame(surf, remaining, sel, &ptr);
                    }
                    crate::ps2::Key::Enter => return sel,
                    // 任务55 扩表：其余键与菜单无关，如实忽略。
                    _ => {}
                }
            }
            // 鼠标：一片只取一个累计包（多包已在服务里累加，位移不会丢）。
            if let Some(d) = mice() {
                ptr.apply(&d, surf);
                // 悬停即改高亮：只动选中，不确认。
                if let Some(i) = hit_card(surf, ptr.x, ptr.y) {
                    sel = i;
                }
                // 左键按下 = 确认进入；点在卡片外则静默忽略（不做半途动作）。
                if d.buttons & BTN_LEFT != 0 {
                    if let Some(i) = hit_card(surf, ptr.x, ptr.y) {
                        return i;
                    }
                }
                draw_frame(surf, remaining, sel, &ptr);
            }
            wait_ticks(slice_ticks);
        }
        remaining -= 1;
        draw_frame(surf, remaining, sel, &ptr);
    }
}

/// 一帧 = 背板重绘 + 菜单绘制 + 指针。清屏策略在此归口。
fn draw_frame(surf: &Surface, remaining: u32, sel: usize, ptr: &Pointer) {
    crate::banner::paint_backdrop(surf);
    draw(surf, remaining, sel);
    draw_cursor(surf, ptr);
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
        draw_frame(&s, 5, 1, &Pointer::centered(&s)); // 下一帧选中下移
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

    // ---- 鼠标选择路径（需求 1/6：任意鼠标和键盘） ----

    /// 从位移包序列构造鼠标源。
    fn scripted_mouse(moves: &[(i16, i16, u8)]) -> impl FnMut() -> Option<MouseDelta> + '_ {
        let mut i = 0usize;
        move || {
            let r = moves.get(i).copied();
            i += 1;
            r.map(|(dx, dy, buttons)| MouseDelta { dx, dy, buttons })
        }
    }

    /// 第 i 张卡片的中心点（复用同一套几何，避免测试自己另算一套坐标）。
    fn card_center(s: &Surface, i: usize) -> (i64, i64) {
        let (x, y, w, h) = card_rect(s, i);
        (x + w / 2, y + h / 2)
    }

    #[test]
    fn pointer_starts_centered_and_invisible() {
        let (s, _b) = surface(800, 600);
        let p = Pointer::centered(&s);
        assert!(!p.visible, "没动过鼠标时不该画指针（无鼠标机器画面保持原样）");
        assert_eq!(p.x, 400);
        assert_eq!(p.y, 300);
    }

    #[test]
    fn pointer_moves_invert_dy_and_clamps() {
        let (s, _b) = surface(800, 600);
        let mut p = Pointer::centered(&s);
        // PS/2 dy 为正 = 向上 = 屏幕 y 减小
        p.apply(&MouseDelta { dx: 10, dy: 20, buttons: 0 }, &s);
        assert_eq!((p.x, p.y), (410, 280));
        assert!(p.visible, "收到位移包后指针必须显现");
        // 越界钳位：往左上角反复推仍留在屏内
        for _ in 0..200 {
            p.apply(&MouseDelta { dx: -100, dy: -100, buttons: 0 }, &s);
        }
        assert_eq!((p.x, p.y), (0, 599));
    }

    #[test]
    fn hit_card_matches_drawn_geometry() {
        let (s, _b) = surface(800, 600);
        for i in 0..ENTRIES.len() {
            let (cx, cy) = card_center(&s, i);
            assert_eq!(hit_card(&s, cx, cy), Some(i), "卡片 {i} 中心必须命中自身");
        }
        // 卡片之间的间隙不应命中任何一张
        let (_, y0, _, ch, gap) = metrics(&s);
        assert_eq!(hit_card(&s, 400, y0 + ch + gap / 2), None, "卡片间隙不算命中");
        // 屏幕左上角（标题区）不算命中
        assert_eq!(hit_card(&s, 0, 0), None);
    }

    #[test]
    fn hover_selects_card_and_click_confirms() {
        let (s, _b) = surface(800, 600);
        let p = Pointer::centered(&s);
        let (cx2, cy2) = card_center(&s, 2);
        // 一步移到第三张卡中心 → 左键确认
        let script = [
            (cx2 as i16 - p.x as i16, -(cy2 as i16 - p.y as i16), 0),
            (0, 0, 1),
        ];
        let mut mice = scripted_mouse(&script);
        let mut no_keys = || -> Option<crate::ps2::Key> { None };
        let sel = run_countdown_with_input(&s, 5, FAST_HZ, &mut no_keys, &mut mice);
        assert_eq!(sel, 2, "悬停第三张卡 + 左键应进入第三项");
    }

    #[test]
    fn click_outside_cards_does_nothing() {
        let (s, _b) = surface(800, 600);
        // 指针推到左上角（不在任何卡片上）后按左键：不得误进入
        let mut mice = scripted_mouse(&[(-500, 500, 1), (-500, 500, 1)]);
        let mut no_keys = || -> Option<crate::ps2::Key> { None };
        let sel = run_countdown_with_input(&s, 1, FAST_HZ, &mut no_keys, &mut mice);
        assert_eq!(sel, 0, "卡片外点击不应触发进入，应回落默认项");
    }

    #[test]
    fn hover_without_click_falls_back_to_default() {
        // 与键盘语义严格对齐：只移动不点击 = 只改高亮，归零走默认项。
        let (s, _b) = surface(800, 600);
        let p = Pointer::centered(&s);
        let (cx2, cy2) = card_center(&s, 2);
        let script = [(cx2 as i16 - p.x as i16, -(cy2 as i16 - p.y as i16), 0)];
        let mut mice = scripted_mouse(&script);
        let mut no_keys = || -> Option<crate::ps2::Key> { None };
        let sel = run_countdown_with_input(&s, 1, FAST_HZ, &mut no_keys, &mut mice);
        assert_eq!(sel, 0, "只悬停不点击 = 未确认的预选，归零走默认项");
    }

    #[test]
    fn no_mouse_source_preserves_keyboard_behavior() {
        // 行为等价闸门：不接鼠标源时，逐字复现既有键盘路径的结果。
        let (s, _b) = surface(800, 600);
        let mut keys = scripted(&[crate::ps2::Key::Down, crate::ps2::Key::Enter]);
        let mut no_mouse = || -> Option<MouseDelta> { None };
        let sel = run_countdown_with_input(&s, 5, FAST_HZ, &mut keys, &mut no_mouse);
        assert_eq!(sel, 1, "无鼠标时键盘路径结果必须不变");
    }

    #[test]
    fn cursor_is_drawn_only_when_visible() {
        const OUT: Color = Color::rgb(0x0B, 0x0E, 0x14);
        let (s1, _b1) = surface(800, 600);
        draw_cursor(&s1, &Pointer::centered(&s1)); // 不可见
        assert_eq!(count_px(&s1, OUT), 0, "未动过鼠标时不该留下指针像素");
        let (s2, _b2) = surface(800, 600);
        let mut shown = Pointer::centered(&s2);
        shown.visible = true;
        draw_cursor(&s2, &shown);
        assert!(count_px(&s2, OUT) > 20, "指针可见时必须画出描边像素");
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
            draw_frame(&s, 5, 0, &Pointer::centered(&s));
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
