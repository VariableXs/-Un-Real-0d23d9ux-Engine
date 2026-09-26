//! F472 终端主题跟随（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **跟随切换即时性；六预设对比度全检（≥4.5:1）；自定义四要素；透明度红线
//! 复判；预设色板与令牌派生关系。**
//!
//! 功能定义（主册批次三）：终端配色两套（深/浅）跟随系统主题即时切换，
//! 六款预设主题（经典黑/日光纸/星徽紫/森绿/沙褐/墨蓝——色板走 F151 令牌
//! 体系派生）；自定义主题入口（前景/背景/光标/选区四要素色）；透明度选项
//! 与 F254 材质白名单对齐（正文区对比度红线同判）。
//!
//! 零堆纪律：静态色板表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 对比度红线（主册：≥4.5:1——WCAG 同源）。
pub const CONTRAST_MIN_X100: u32 = 450;
/// 预设主题数（主册：六款）。
pub const PRESET_N: usize = 6;
/// 自定义四要素（主册：前景/背景/光标/选区）。
pub const CUSTOM_ELEMENTS: usize = 4;

/// RGB 颜色。
pub type Rgb = (u8, u8, u8);

/// 相对亮度（WCAG 公式；输入 0-255）。
pub fn rel_luminance(c: Rgb) -> f64 {
    fn ch(v: u8) -> f64 {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * ch(c.0) + 0.7152 * ch(c.1) + 0.0722 * ch(c.2)
}

/// 对比度（×100 整数，≥450 达标）。
pub fn contrast_x100(a: Rgb, b: Rgb) -> u32 {
    let (la, lb) = (rel_luminance(a), rel_luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    ((hi + 0.05) / (lo + 0.05) * 100.0).round() as u32
}

/// 一套终端主题（前景/背景/光标/选区四要素——预设与自定义同构）。
#[derive(Clone, Copy, Debug)]
pub struct TermPalette {
    pub name: &'static str,
    pub fg: Rgb,
    pub bg: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    /// 派生源令牌（主册：色板走 F151 令牌体系派生——每套登记派生锚）。
    pub token_anchor: &'static str,
}

/// 六预设（主册原文六款；对比度全部入册前全检）。
pub const PRESETS: [TermPalette; PRESET_N] = [
    TermPalette { name: "经典黑", fg: (220, 220, 220), bg: (12, 12, 12), cursor: (220, 220, 220), selection: (40, 60, 90), token_anchor: "F151/term-classic-dark" },
    TermPalette { name: "日光纸", fg: (40, 40, 40), bg: (250, 246, 237), cursor: (40, 40, 40), selection: (235, 220, 180), token_anchor: "F151/term-daylight" },
    TermPalette { name: "星徽紫", fg: (216, 205, 255), bg: (24, 16, 40), cursor: (230, 220, 255), selection: (70, 50, 110), token_anchor: "F151/term-starlight" },
    TermPalette { name: "森绿", fg: (200, 228, 205), bg: (14, 30, 20), cursor: (210, 240, 215), selection: (40, 70, 50), token_anchor: "F151/term-forest" },
    TermPalette { name: "沙褐", fg: (238, 224, 200), bg: (40, 30, 22), cursor: (245, 232, 205), selection: (80, 60, 42), token_anchor: "F151/term-sand" },
    TermPalette { name: "墨蓝", fg: (196, 214, 235), bg: (10, 18, 32), cursor: (210, 225, 245), selection: (35, 55, 85), token_anchor: "F151/term-ink" },
];

/// 终端主题管理器。
pub struct TermTheme {
    pub palette: TermPalette,
    /// 跟随系统主题（深/浅）——跟随时从系统主题取派生预设。
    pub follow_system: bool,
    /// 透明度 0-100%（正文区对比度红线复判用）。
    pub opacity_permille: u16,
}

/// 透明度红线复判（主册/F254：透明好看但不牺牲可读——有效背景 = 主题 bg
/// 与壁纸近似色按透明度混合后，对比度仍须达标；此处以最坏情况纯白/纯黑
/// 壁纸双判）。
pub fn opacity_redline_ok(p: &TermPalette, opacity_permille: u16) -> bool {
    if opacity_permille >= 1_000 {
        return contrast_x100(p.fg, p.bg) >= CONTRAST_MIN_X100;
    }
    let a = opacity_permille as f64 / 1_000.0;
    let mix = |c: u8, wall: u8| -> u8 { (c as f64 * a + wall as f64 * (1.0 - a)).round() as u8 };
    let bg = p.bg;
    let worst_white = (mix(bg.0, 255), mix(bg.1, 255), mix(bg.2, 255));
    let worst_black = (mix(bg.0, 0), mix(bg.1, 0), mix(bg.2, 0));
    contrast_x100(p.fg, worst_white) >= CONTRAST_MIN_X100
        && contrast_x100(p.fg, worst_black) >= CONTRAST_MIN_X100
}

impl TermTheme {
    pub fn new(dark_system: bool) -> Self {
        // 跟随系统：深系统 → 经典黑；浅系统 → 日光纸（派生预设）。
        TermTheme {
            palette: if dark_system { PRESETS[0] } else { PRESETS[1] },
            follow_system: true,
            opacity_permille: 1_000,
        }
    }

    /// 主题跟随即时切换（主册：跟随切换即时性）。
    pub fn system_theme_changed(&mut self, dark: bool) {
        if self.follow_system {
            self.palette = if dark { PRESETS[0] } else { PRESETS[1] };
        }
    }

    /// 选预设（跟随之解除——用户选了脾气）。
    pub fn pick_preset(&mut self, idx: usize) -> bool {
        if idx >= PRESET_N {
            return false;
        }
        self.palette = PRESETS[idx];
        self.follow_system = false;
        true
    }

    /// 自定义四要素（主册：前景/背景/光标/选区）。
    pub fn set_custom(&mut self, fg: Rgb, bg: Rgb, cursor: Rgb, selection: Rgb) {
        self.palette = TermPalette {
            name: "自定义",
            fg,
            bg,
            cursor,
            selection,
            token_anchor: "F151/term-custom",
        };
        self.follow_system = false;
    }

    /// 入册前全检：六预设正文对比度全达标（主册：对比度全检 ≥4.5:1）。
    pub fn all_presets_pass() -> bool {
        PRESETS.iter().all(|p| contrast_x100(p.fg, p.bg) >= CONTRAST_MIN_X100)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_termtheme_checks() -> CheckSet {
    let mut cs = CheckSet::new("F472-termtheme");
    // 1) 六预设对比度全检（主册：≥4.5:1 才入册）。
    cs.add("six_presets_contrast", PRESETS.len() == PRESET_N && TermTheme::all_presets_pass(), "");
    // 2) 跟随切换即时性。
    let mut t = TermTheme::new(true);
    cs.add("dark_default", t.palette.name == "经典黑", "");
    t.system_theme_changed(false);
    cs.add("follow_instant", t.palette.name == "日光纸", "");
    // 3) 选预设解除跟随（越界诚实拒绝）。
    cs.add("pick_preset_unfollows", t.pick_preset(2) && t.palette.name == "星徽紫" && !t.follow_system, "");
    cs.add("pick_oob_honest", !t.pick_preset(PRESET_N), "");
    // 4) 自定义四要素。
    t.set_custom((230, 230, 230), (20, 20, 20), (255, 255, 255), (60, 60, 90));
    cs.add("custom_four_elements", t.palette.name == "自定义" && t.palette.cursor == (255, 255, 255) && t.palette.selection == (60, 60, 90), "");
    // 5) 透明度红线复判（100% 全量达标；50% 低透明按最坏壁纸双判——
    //    经典黑在白壁纸混色下 fg/bg 对比跌破 4.5:1 → 红线如实拦下）。
    cs.add("opacity_full_ok", opacity_redline_ok(&PRESETS[0], 1_000), "");
    cs.add("opacity_50_redline", !opacity_redline_ok(&PRESETS[0], 500), "");
    //    低透明高对比主题（前景极亮）在合理透明度（90%）下通过；50% 时
    //    白壁纸混色把黑底抬到中灰——红线如实拦下（诚实复判）。
    let bright = TermPalette { name: "亮前", fg: (255, 255, 255), bg: (0, 0, 0), cursor: (255, 255, 255), selection: (30, 30, 30), token_anchor: "F151/term-test" };
    cs.add("opacity_90_pass", opacity_redline_ok(&bright, 900), "");
    cs.add("opacity_50_redline_worst_wall", !opacity_redline_ok(&bright, 500), "");
    // 6) 对比度公式抽检（同色 = 1:1；黑白 = 21:1）。
    let gray = (128, 128, 128);
    cs.add("contrast_math", contrast_x100(gray, gray) == 100 && contrast_x100((0, 0, 0), (255, 255, 255)) >= 2_000, "");
    // 7) 派生关系登记（每套预设带 F151 令牌锚）。
    cs.add("token_anchors", PRESETS.iter().all(|p| p.token_anchor.starts_with("F151/")), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_presets_each_name_unique() {
        let mut seen = 0;
        for (i, p) in PRESETS.iter().enumerate() {
            for (j, q) in PRESETS.iter().enumerate() {
                if i < j {
                    assert_ne!(p.name, q.name);
                }
            }
            seen += 1;
        }
        assert_eq!(seen, 6);
    }

    #[test]
    fn opacity_redline_blocks_low_contrast_blends() {
        // 前景与背景太接近的自定义主题 + 低透明 → 红线拦下。
        let bad = TermPalette { name: "坏例", fg: (120, 120, 120), bg: (100, 100, 100), cursor: (0, 0, 0), selection: (0, 0, 0), token_anchor: "x" };
        assert!(!opacity_redline_ok(&bad, 1_000));
        let good = PRESETS[0];
        assert!(opacity_redline_ok(&good, 1_000));
    }

    #[test]
    fn follow_reenables_only_explicitly() {
        let mut t = TermTheme::new(true);
        t.system_theme_changed(false);
        assert!(t.follow_system);
        t.pick_preset(3);
        t.system_theme_changed(true); // 不跟随了——用户预设不被覆盖
        assert_eq!(t.palette.name, "森绿");
    }
}
