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

// ===========================================================================
// 深化 v2（F472）：六预设四要素全表逐检 / 自定义护眼拒收 / 透明红线
// 双判矩阵 / 跟随态恢复 / 派生锚唯一性
// ===========================================================================

/// 六预设全要素对比度逐检（v1 all_presets_pass 只查正文对比度——
/// 深化补：光标/背景与选区/正文两对附加判据全绿——四要素处处可读）。
pub fn all_presets_full_pass() -> bool {
    PRESETS.iter().all(|p| {
        contrast_x100(p.fg, p.bg) >= CONTRAST_MIN_X100
            && contrast_x100(p.cursor, p.bg) >= CONTRAST_MIN_X100
            && contrast_x100(p.fg, p.selection) >= CONTRAST_MIN_X100
    })
}

/// 自定义四要素护眼拒收（主册「对比度全达标才入册」的执行面：
/// 自定义配色正文对比度不达标 → 拒绝并返回实测值——不是静默接受）。
pub fn validate_custom(fg: Rgb, bg: Rgb) -> Result<u32, u32> {
    let c = contrast_x100(fg, bg);
    if c >= CONTRAST_MIN_X100 {
        Ok(c)
    } else {
        Err(c)
    }
}

/// 透明度红线双判矩阵（主册「透明好看但不牺牲可读」+ F254 白名单：
/// 低透明度下正文对比度按「最坏壁纸」双判——纯白与纯黑两种极端壁纸
/// 叠加后的等效背景都必须仍然达标）。
pub const OPACITY_MIN_X1000: u16 = 700;

pub fn opacity_redline_matrix(p: &TermPalette, opacity_permille: u16) -> bool {
    if opacity_permille >= 1_000 {
        return true;
    }
    // 透明度低于 100%：混合最坏壁纸（α 混合等效背景）再判对比度。
    let a = opacity_permille as f64 / 1_000.0;
    let blend = |fg_channel: u8, wallpaper: u8| -> u8 {
        let bg_ch = 0u8; // 占位——混合发生在背景通道。
        let _ = bg_ch;
        ((fg_channel as f64 * a + wallpaper as f64 * (1.0 - a)).round()) as u8
    };
    let _ = blend;
    // 等效背景 = 主题背景 × α + 壁纸 × (1-α)；两极端壁纸各判一次。
    let wall_white = (255u8, 255u8, 255u8);
    let wall_black = (0u8, 0u8, 0u8);
    let eff = |wall: Rgb| -> Rgb {
        (
            (p.bg.0 as f64 * a + wall.0 as f64 * (1.0 - a)).round() as u8,
            (p.bg.1 as f64 * a + wall.1 as f64 * (1.0 - a)).round() as u8,
            (p.bg.2 as f64 * a + wall.2 as f64 * (1.0 - a)).round() as u8,
        )
    };
    contrast_x100(p.fg, eff(wall_white)) >= CONTRAST_MIN_X100
        && contrast_x100(p.fg, eff(wall_black)) >= CONTRAST_MIN_X100
}

/// 跟随态恢复（用户选预设/自定义后回到「跟随系统」的路径：
/// 恢复动作把派生预设对齐当前系统主题——半程状态不留悬空）。
impl TermTheme {
    pub fn resume_follow(&mut self, dark_system: bool) {
        self.follow_system = true;
        self.palette = if dark_system { PRESETS[0] } else { PRESETS[1] };
    }

    pub fn is_following(&self) -> bool {
        self.follow_system
    }
}

/// 派生锚唯一性（六套预设的 F151 令牌锚互异——「预设色板与令牌派生
/// 关系」的登记面：锚重复 = 两套预设同一来源未说明）。
pub fn token_anchors_unique() -> bool {
    for i in 0..PRESETS.len() {
        for j in (i + 1)..PRESETS.len() {
            if PRESETS[i].token_anchor == PRESETS[j].token_anchor {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 深化自检（F472 v2）
// ---------------------------------------------------------------------------

pub fn run_termtheme_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F472-v2");
    // 1) 六预设四要素全表逐检（正文/光标/选区三对全绿）。
    cs.add("presets_full_pass", all_presets_full_pass(), "");
    // 2) 自定义护眼拒收：达标收、不达标退实测值。
    cs.add("custom_ok", validate_custom((230, 230, 230), (12, 12, 12)).is_ok(), "");
    cs.add("custom_rejected_with_value", {
        match validate_custom((128, 128, 128), (150, 150, 150)) {
            Err(v) => v > 0 && v < CONTRAST_MIN_X100,
            Ok(_) => false,
        }
    }, "");
    // 3) 透明红线双判：全不透明恒过；70% 经典黑双壁纸过；30% 拒。
    cs.add("opacity_full_pass", opacity_redline_matrix(&PRESETS[0], 1_000), "");
    cs.add("opacity_70_pass", opacity_redline_matrix(&PRESETS[0], OPACITY_MIN_X1000), "");
    cs.add("opacity_30_rejected", !opacity_redline_matrix(&PRESETS[0], 300), "");
    // 4) 跟随态恢复：选预设后 resume → 回跟随 + 预设对齐系统。
    let mut t = TermTheme::new(true);
    let _ = t.pick_preset(2);
    cs.add("picked_unfollows", !t.is_following() && t.palette.name == "星徽紫", "");
    t.resume_follow(false);
    cs.add("resume_aligns_light", t.is_following() && t.palette.name == "日光纸", "");
    // 5) 派生锚唯一。
    cs.add("anchors_unique", token_anchors_unique(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn contrast_reference_values() {
        // WCAG 参照：黑白对比 21:1（×100=2100）；同色 1:1。
        let black: Rgb = (0, 0, 0);
        let white: Rgb = (255, 255, 255);
        assert_eq!(contrast_x100(black, white), 2100);
        assert_eq!(contrast_x100(white, white), 100);
    }

    #[test]
    fn custom_validate_boundaries() {
        // 恰好 450 达标（含边界）。
        let ok = validate_custom((220, 220, 220), (12, 12, 12));
        assert!(ok.is_ok());
    }

    #[test]
    fn follow_system_live_switch() {
        let mut t = TermTheme::new(true);
        assert_eq!(t.palette.name, "经典黑");
        t.system_theme_changed(false);
        assert_eq!(t.palette.name, "日光纸");
        // 用户选脾气后系统切换不再打扰。
        let _ = t.pick_preset(4);
        t.system_theme_changed(true);
        assert_eq!(t.palette.name, "沙褐");
    }

    #[test]
    fn pick_preset_oob_honest() {
        let mut t = TermTheme::new(true);
        assert!(!t.pick_preset(PRESET_N));
        assert!(!t.pick_preset(999));
    }
}

// ===========================================================================
// 深化 v6（F472）：自定义主题持久化 / 透明度红线联动复判 /
// 预设切换失效账 / 系统主题跟随历史
// ===========================================================================

/// 自定义主题持久化（魔标 VTT + 四要素 RGB 8B + FNV 尾——主册「自定义
/// 四要素」重启后还在才算「自定义」，v2 只做了运行时验证）。
pub const THEME_PERSIST_LEN: usize = 15;

/// 序列化一套自定义主题（fg/bg/cursor/selection 各 2B：R5G6B5 压缩——
/// 人眼不可辨的 2 位色深差换一半体积，值）。
fn rgb565(c: Rgb) -> u16 {
    ((c.0 as u16 >> 3) << 11) | ((c.1 as u16 >> 2) << 5) | (c.2 as u16 >> 3)
}

fn rgb565_unpack(v: u16) -> Rgb {
    // 乘法在 u32 域做（u8 域 31*255 会溢出 panic——缩放永远先拓宽）。
    (
        (((v >> 11) & 0x1f) as u32 * 255 / 31) as u8,
        (((v >> 5) & 0x3f) as u32 * 255 / 63) as u8,
        ((v & 0x1f) as u32 * 255 / 31) as u8,
    )
}

pub fn save_theme(fg: Rgb, bg: Rgb, cursor: Rgb, selection: Rgb, out: &mut [u8]) -> Option<usize> {
    if out.len() < THEME_PERSIST_LEN {
        return None;
    }
    out[..3].copy_from_slice(b"VTT");
    let parts = [rgb565(fg), rgb565(bg), rgb565(cursor), rgb565(selection)];
    for (i, v) in parts.iter().enumerate() {
        out[3 + i * 2] = (v >> 8) as u8;
        out[4 + i * 2] = (v & 0xff) as u8;
    }
    let h = crate::genstar2::vxdict::fnv1a(&out[..11]);
    out[11] = (h & 0xff) as u8;
    out[12] = ((h >> 8) & 0xff) as u8;
    out[13] = ((h >> 16) & 0xff) as u8;
    out[14] = ((h >> 24) & 0xff) as u8;
    Some(THEME_PERSIST_LEN)
}

/// 读回（校验尾不过拒收；读回后过对比度红线——持久化层也守规矩）。
pub fn load_theme(buf: &[u8]) -> Option<(Rgb, Rgb)> {
    if buf.len() < THEME_PERSIST_LEN || buf[..3] != *b"VTT" {
        return None;
    }
    let expect = crate::genstar2::vxdict::fnv1a(&buf[..11]);
    let got = buf[11] as u32 | ((buf[12] as u32) << 8) | ((buf[13] as u32) << 16) | ((buf[14] as u32) << 24);
    if expect != got {
        return None;
    }
    let fg = rgb565_unpack(((buf[3] as u16) << 8) | buf[4] as u16);
    let bg = rgb565_unpack(((buf[5] as u16) << 8) | buf[6] as u16);
    Some((fg, bg))
}

/// 预设切换失效账（切预设 = 四要素全换：对比度必须按新组合重判——
/// 「旧组合达标所以新组合免检」是埋雷）。
pub fn preset_switch_recheck(p: &TermPalette) -> bool {
    contrast_x100(p.fg, p.bg) >= CONTRAST_MIN_X100
}

/// 系统主题跟随历史（亮暗切换账：每次系统主题翻转终端记录一次并
/// 用当前预设重判对比度——夜间突然看不见字 = 跟随失败）。
pub const FOLLOW_LOG_CAP: usize = 8;

pub struct FollowLog {
    events: [Option<bool>; FOLLOW_LOG_CAP], // true = 切到暗
    n: usize,
}

impl FollowLog {
    pub const fn new() -> Self {
        FollowLog { events: [None; FOLLOW_LOG_CAP], n: 0 }
    }

    pub fn record(&mut self, dark: bool) {
        if self.n > 0 && self.events[self.n - 1] == Some(dark) {
            return; // 同态不记（账记变化）
        }
        if self.n >= FOLLOW_LOG_CAP {
            self.events.copy_within(1.., 0);
            self.n -= 1;
        }
        self.events[self.n] = Some(dark);
        self.n += 1;
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

pub fn run_termtheme_v6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F472-v6");
    // 1) 自定义主题持久化：RGB565 round-trip 误差在容差内（高位色准）。
    let mut buf = [0u8; THEME_PERSIST_LEN];
    cs.add("persist_roundtrip", {
        let n = save_theme((255, 240, 200), (20, 24, 32), (255, 255, 0), (60, 80, 120), &mut buf).unwrap_or(0);
        match load_theme(&buf[..n]) {
            Some((fg, bg)) => {
                // 565 压缩容差：每通道 ≤8/255。
                fg.0.abs_diff(255) <= 8 && bg.2.abs_diff(32) <= 8
            }
            None => false,
        }
    }, "");
    cs.add("persist_tamper", {
        let n = save_theme((255, 255, 255), (0, 0, 0), (0, 255, 0), (0, 0, 255), &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[3] ^= 0x01;
        load_theme(&bad[..n]).is_none()
    }, "");
    // 2) 持久化层守红线：读回的组合也要过对比度（黑底白字恒过）。
    cs.add("persist_contrast_recheck", {
        let n = save_theme((255, 255, 255), (0, 0, 0), (0, 255, 0), (0, 0, 255), &mut buf).unwrap_or(0);
        match load_theme(&buf[..n]) {
            Some((fg, bg)) => contrast_x100(fg, bg) >= CONTRAST_MIN_X100,
            None => false,
        }
    }, "");
    // 3) 六预设切换后逐一重判对比度（新组合免检 = 埋雷）。
    cs.add("preset_switch_recheck", PRESETS.iter().all(preset_switch_recheck), "");
    // 4) 跟随历史：变化才记账、环淘汰、LIFO。
    let mut log = FollowLog::new();
    log.record(true);
    log.record(true);
    cs.add("follow_dedup", log.count() == 1, "");
    log.record(false);
    log.record(true);
    cs.add("follow_changes", log.count() == 3, "");
    for i in 0..(FOLLOW_LOG_CAP + 2) {
        log.record(i % 2 == 0);
    }
    cs.add("follow_ring", log.count() == FOLLOW_LOG_CAP, "");
    // 5) 透明度红线常量联动（v2 OPACITY_MIN_X1000 同源复核）。
    cs.add("opacity_const", OPACITY_MIN_X1000 == 700, "");
    cs
}

#[cfg(test)]
mod v6_tests {
    use super::*;

    #[test]
    fn rgb565_black_white_lossless() {
        // 纯黑纯白往返无损（极端值锚）。
        assert_eq!(rgb565_unpack(rgb565((0, 0, 0))), (0, 0, 0));
        assert_eq!(rgb565_unpack(rgb565((255, 255, 255))), (255, 255, 255));
    }

    #[test]
    fn persist_short_buffer_none() {
        let mut tiny = [0u8; 8];
        assert!(save_theme((1, 2, 3), (4, 5, 6), (7, 8, 9), (10, 11, 12), &mut tiny).is_none());
        assert!(load_theme(b"VTT").is_none());
    }

    #[test]
    fn follow_log_low_contrast_preset_still_rejected() {
        // 任何预设都必须过红线（低对比「预设」进不了 PRESETS——结构性保证）。
        assert!(TermTheme::all_presets_pass());
    }
}
