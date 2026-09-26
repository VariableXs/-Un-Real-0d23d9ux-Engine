//! F501/F502/F503 桌面图标文字与网格（ustar3 · I 域 · AI-U3 分工包）。
//!
//! ── F501 桌面图标文字可读性 ─────────────────────────────────────────────
//! 主册判据（验收标准第一句）：
//! **五档亮度壁纸×两字色自动选择用例；投影参数实测；选中底衬；4K 渲染
//! 精度；与 F297 压暗联动一致性。**
//! 功能定义要点：文字双层渲染（柔和投影+微描边，投影透明度 40%、模糊
//! 2px——4K 管线 F068 精度）；深浅壁纸自动选字色（壁纸亮度分析选白/黑
//! 字——F297 压暗协同）；选中态文字底衬（半透明胶囊底）。无感标准：白
//! 壁纸、黑壁纸、花壁纸（密林/星空）上图标名字都看得清。
//! 依赖锚点：F068（4K 渲染管线精度）、F297（壁纸压暗联动）。
//!
//! ── F502 图标文字两行封顶 ───────────────────────────────────────────────
//! 主册判据（验收标准第一句）：
//! **两行/8 字换行参数；省略触发与全文可达三路；居中对齐；中英混排换行
//! （不在英文单词中间断）；Tooltip 联动。**
//! 功能定义要点：最多两行、每行约 8 个全角字符、超出第二行尾部省略号
//! （…）；省略的全文看 Tooltip（F205）/重命名（F260）/属性（F264）三条
//! 路可达；文字块整体水平居中于图标。无感标准：长文件名在桌面上体面地
//! 截断（「项目总结报告最终版…」）；省略号永远表示「还有内容」而不是
//! 显示 bug。
//! 依赖锚点：F205（Tooltip）、F260（重命名）、F264（属性）。
//!
//! ── F503 图标网格密度 ───────────────────────────────────────────────────
//! 主册判据（验收标准第一句）：
//! **三档格距；重排最近格吸附与相对位置保持；自定义 8px 步进；与 F084
//! 网格/自动排列 F401 兼容；预览即时性。**
//! 功能定义要点：桌面网格密度三档（宽松/标准/紧凑——格距 96/80/64px）：
//! 设置页选择+即时预览；密度改变时图标按最近格吸附重排（相对位置尽量
//! 保持）；自定义间隔进阶入口（行距列距独立调，8px 步进）。无感标准：
//! 切换时图标「搬家」但邻居关系不变（不乱序）；即时看效果不用确定。
//! 依赖锚点：F084（网格对齐）、F401（自动排列）。
//!
//! 零堆纪律：本文件逻辑路径零堆——无 String/Vec/Box/format!/to_string/
//! to_vec，定长数组 + &str 字面量 + core 运算（no_std 兼容纯逻辑
//! 判据实装层）。#[cfg(test)] 测试代码可用 std。

use crate::checks::CheckSet;

// ===========================================================================
// F501 桌面图标文字可读性
// ===========================================================================

// ---------------------------------------------------------------------------
// 常量（一处一事实，数字判据精确成常量）
// ---------------------------------------------------------------------------

/// 壁纸亮度五档分界（0-255 亮度分为五档：[0,52) / [52,103) / [103,154) /
/// [154,205) / [205,255]。主册：五档亮度壁纸）。
pub const BRIGHTNESS_TIER_BOUNDS: [u8; 4] = [52, 103, 154, 205];
/// 中间档对比度最大化分界：背景亮度 ≥128 时黑字对比更大，否则白字。
pub const MID_LUMINANCE: u8 = 128;
/// 投影透明度 40%（主册：投影透明度 40%）。
pub const SHADOW_ALPHA_PERMILLE: u32 = 400;
/// 投影模糊 2px（主册：模糊 2px——4K 管线 F068 精度）。
pub const SHADOW_BLUR_PX: u32 = 2;
/// 投影偏移（柔和投影：右下 1px 微移）。
pub const SHADOW_OFFSET_PX: i32 = 1;
/// 微描边宽度 1px（主册：微描边）。
pub const OUTLINE_WIDTH_PX: u32 = 1;
/// 选中底衬胶囊透明度（半透明胶囊底）。
pub const CAPSULE_ALPHA_PERMILLE: u32 = 350;
/// 选中底衬胶囊基色亮度（深灰底，白纸黑纸壁纸上均抬高对比）。
pub const CAPSULE_BASE_LUM: u8 = 40;
/// 可读性对比裕量：|字色亮度 - 有效背景亮度| ≥ 64 视为「看得清」。
pub const READABILITY_MARGIN: u8 = 64;
/// F297 压暗档位（‰）：0/200/400/600 四档，压暗后重新选字色须一致。
pub const DIM_LEVELS_PERMILLE: [u32; 4] = [0, 200, 400, 600];
/// 4K 基准渲染宽度（F068：渲染坐标以 4K 为基准缩放）。
pub const BASE_4K_WIDTH_PX: u32 = 3840;
/// 缩放档位（‰）：125% / 150% / 200% DPI。
pub const DPI_125_PERMILLE: u32 = 1250;
pub const DPI_150_PERMILLE: u32 = 1500;
pub const DPI_200_PERMILLE: u32 = 2000;

/// 图标文字色（两字色自动选择）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextColor {
    White,
    Black,
}

impl TextColor {
    /// 字色亮度（纯白 255 / 纯黑 0）。
    pub fn luminance(self) -> u8 {
        match self {
            TextColor::White => 255,
            TextColor::Black => 0,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            TextColor::White => "white",
            TextColor::Black => "black",
        }
    }
    /// 「偏白程度」标量（F297 压暗联动单调性检查用）。
    fn whiteness(self) -> u32 {
        match self {
            TextColor::White => 1,
            TextColor::Black => 0,
        }
    }
}

/// 壁纸亮度 → 五档（0=最暗 … 4=最亮）。
pub fn brightness_tier(lum: u8) -> u8 {
    let mut tier = 0u8;
    while (tier as usize) < BRIGHTNESS_TIER_BOUNDS.len() && lum >= BRIGHTNESS_TIER_BOUNDS[tier as usize] {
        tier += 1;
    }
    tier
}

/// 字色选择纯函数（对比度最大化规则）：|白-背景| ≥ |黑-背景| 取白，
/// 即背景 ≥ 128 取黑字、< 128 取白字；对五档壁纸逐档成立。
pub fn select_text_color(bg_lum: u8) -> TextColor {
    if bg_lum >= MID_LUMINANCE {
        TextColor::Black
    } else {
        TextColor::White
    }
}

/// 五档亮度壁纸 × 两字色用例表（主册：五档亮度壁纸×两字色自动选择用例）。
/// (壁纸名, 代表亮度, 期望字色)。覆盖黑壁纸/星空/密林/灰雾/白壁纸。
pub const FIVE_TIER_WALLPAPERS: [(&'static str, u8, TextColor); 5] = [
    ("black-wall", 8, TextColor::White),
    ("starfield", 30, TextColor::White),
    ("dense-forest", 70, TextColor::White),
    ("mist-gray", 140, TextColor::Black),
    ("white-wall", 245, TextColor::Black),
];

/// 投影参数（主册：投影参数实测——透明度 40%、模糊 2px）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShadowParams {
    pub alpha_permille: u32,
    pub blur_px: u32,
    pub offset_x_px: i32,
    pub offset_y_px: i32,
}

pub const SHADOW_PARAMS: ShadowParams = ShadowParams {
    alpha_permille: SHADOW_ALPHA_PERMILLE,
    blur_px: SHADOW_BLUR_PX,
    offset_x_px: SHADOW_OFFSET_PX,
    offset_y_px: SHADOW_OFFSET_PX,
};

/// 微描边参数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OutlineParams {
    pub width_px: u32,
}

pub const OUTLINE_PARAMS: OutlineParams = OutlineParams { width_px: OUTLINE_WIDTH_PX };

/// 选中态文字底衬（半透明胶囊底）：圆角 = 文字块半高（胶囊几何），
/// 水平/垂直内边距随文字高度走。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapsuleStyle {
    pub alpha_permille: u32,
    pub corner_radius_px: u32,
    pub pad_h_px: u32,
    pub pad_v_px: u32,
}

/// 由文字块高度推导胶囊几何（radius = h/2 即两端的半圆）。
pub fn capsule_for_text_height(text_h_px: u32) -> CapsuleStyle {
    CapsuleStyle {
        alpha_permille: CAPSULE_ALPHA_PERMILLE,
        corner_radius_px: text_h_px / 2,
        pad_h_px: text_h_px / 2,
        pad_v_px: text_h_px / 4,
    }
}

/// 透明度合成（permille 加权）：胶囊叠在壁纸上后文字背后的有效背景亮度。
pub fn alpha_blend(fg_lum: u8, bg_lum: u8, alpha_permille: u32) -> u8 {
    let a = alpha_permille.min(1000) as u32;
    ((fg_lum as u32 * a + bg_lum as u32 * (1000 - a)) / 1000) as u8
}

/// F068 4K 精度：以 4K 基准的参数按 DPI(‰) 缩放，四舍五入取整。
/// 规则：物理像素 = round(逻辑像素 × dpi / 1000)。
pub fn scale_px(px: u32, dpi_permille: u32) -> u32 {
    (px * dpi_permille + 500) / 1000
}

/// 4K 换算达标规则：125%/150%/200% DPI 下投影模糊与描边宽度换算后
/// 仍 ≥ 主册规定值（2px 投影模糊不许缩水成 1px）。
pub fn scaled_params_meet_spec(dpi_permille: u32) -> bool {
    scale_px(SHADOW_BLUR_PX, dpi_permille) >= SHADOW_BLUR_PX
        && scale_px(OUTLINE_WIDTH_PX, dpi_permille) >= OUTLINE_WIDTH_PX
}

/// F297 联动：壁纸压暗后的有效亮度 = 原亮度 × (1000 - 压暗‰) / 1000。
pub fn dimmed_brightness(lum: u8, dim_permille: u32) -> u8 {
    let d = dim_permille.min(1000);
    ((lum as u32 * (1000 - d)) / 1000) as u8
}

/// F297 联动合成函数：压暗后按有效亮度重新选字色（与直接在压暗后的
/// 壁纸上选色完全一致——联动一致性）。
pub fn select_after_dim(lum: u8, dim_permille: u32) -> TextColor {
    select_text_color(dimmed_brightness(lum, dim_permille))
}

// ---------------------------------------------------------------------------
// F501 域自检
// ---------------------------------------------------------------------------

/// F501 域自检。
pub fn run_f501_checks() -> CheckSet {
    let mut cs = CheckSet::new("F501-deskicon-readable");
    // 1) 五档分界常量把 0..=255 恰好切成五段（边界值核对）。
    let bounds_ok = brightness_tier(0) == 0
        && brightness_tier(51) == 0
        && brightness_tier(52) == 1
        && brightness_tier(102) == 1
        && brightness_tier(103) == 2
        && brightness_tier(153) == 2
        && brightness_tier(154) == 3
        && brightness_tier(204) == 3
        && brightness_tier(205) == 4
        && brightness_tier(255) == 4;
    cs.add("brightness_tier_boundaries", bounds_ok, "五档分界 52/103/154/205");
    // 2) 全亮度域枚举：每个 0..=255 都落在合法档位（0..=4），且单调不减。
    let mut all_valid = true;
    let mut monotonic = true;
    let mut prev = 0u8;
    for lum in 0..=255u32 {
        let t = brightness_tier(lum as u8);
        if t > 4 {
            all_valid = false;
        }
        if t < prev {
            monotonic = false;
        }
        prev = t;
    }
    cs.add("tier_classify_all_luminances", all_valid && monotonic, "");
    // 3) 五档壁纸 × 两字色用例表逐条断言（判据：自动选择用例）。
    let mut table_ok = true;
    for &(name, lum, expect) in FIVE_TIER_WALLPAPERS.iter() {
        let _ = name;
        if select_text_color(lum) != expect {
            table_ok = false;
        }
    }
    cs.add("text_color_five_tiers_table", table_ok, "");
    // 4) 中间档对比度最大化规则：127→白、128→黑（|255-bg| vs |0-bg|）。
    cs.add(
        "text_color_contrast_max_mid",
        select_text_color(127) == TextColor::White && select_text_color(128) == TextColor::Black,
        "",
    );
    // 5) 投影参数实测：透明度 400‰（40%）、模糊 2px（主册逐字）。
    cs.add(
        "shadow_params_spec",
        SHADOW_PARAMS.alpha_permille == 400 && SHADOW_PARAMS.blur_px == 2 && SHADOW_PARAMS.offset_x_px == 1 && SHADOW_PARAMS.offset_y_px == 1,
        "",
    );
    // 6) 微描边 1px。
    cs.add("outline_width_spec", OUTLINE_PARAMS.width_px == 1, "");
    // 7) 选中底衬胶囊几何：圆角 = 半高（胶囊半圆），透明度在 (0,1000) 开区间。
    let cap = capsule_for_text_height(24);
    cs.add(
        "capsule_geometry_halfround",
        cap.corner_radius_px == 12 && cap.alpha_permille == CAPSULE_ALPHA_PERMILLE && CAPSULE_ALPHA_PERMILLE > 0 && CAPSULE_ALPHA_PERMILLE < 1000,
        "",
    );
    // 8) 透明度合成数学：0‰=背景、1000‰=前景、350‰ 加权正确。
    let blend_ok = alpha_blend(40, 245, 0) == 245
        && alpha_blend(40, 245, 1000) == 40
        && alpha_blend(40, 245, 350) == 173;
    cs.add("alpha_blend_math", blend_ok, "");
    // 9) 4K 精度取整规则：2px 在 125%/150%/200% 下 → 3/3/4（round half up）。
    cs.add(
        "dpi_scale_rounding",
        scale_px(SHADOW_BLUR_PX, DPI_125_PERMILLE) == 3
            && scale_px(SHADOW_BLUR_PX, DPI_150_PERMILLE) == 3
            && scale_px(SHADOW_BLUR_PX, DPI_200_PERMILLE) == 4,
        "",
    );
    // 10) 三档 DPI 下投影/描边换算后仍达标（F068 精度判据）。
    cs.add(
        "dpi_scaled_params_meet_spec",
        scaled_params_meet_spec(DPI_125_PERMILLE)
            && scaled_params_meet_spec(DPI_150_PERMILLE)
            && scaled_params_meet_spec(DPI_200_PERMILLE),
        "",
    );
    // 11) F297 压暗联动一致性：任意壁纸亮度 × 压暗档位扫描，字色只能
    //     从黑→白单向翻转（压暗只会更暗，不会反向）。
    let mut dim_consistent = true;
    for lum in [0u8, 40, 90, 128, 160, 200, 255] {
        let mut prev_w = select_after_dim(lum, 0).whiteness();
        for &dim in DIM_LEVELS_PERMILLE.iter() {
            let w = select_after_dim(lum, dim).whiteness();
            if w < prev_w {
                dim_consistent = false;
            }
            // 压暗后重新选色 = 直接对有效亮度选色（合成一致）。
            if select_after_dim(lum, dim) != select_text_color(dimmed_brightness(lum, dim)) {
                dim_consistent = false;
            }
            prev_w = w;
        }
    }
    cs.add("f297_dim_reselect_consistency", dim_consistent, "");
    // 12) 可读性硬标准：五档壁纸（黑/星空/密林/灰雾/白）上字色对比
    //     裕量 ≥ 64；叠加选中胶囊后依旧 ≥ 64（无感标准全通过）。
    let mut readable = true;
    for &(_, lum, color) in FIVE_TIER_WALLPAPERS.iter() {
        let bare = (color.luminance() as i32 - lum as i32).abs() as u8;
        let eff = alpha_blend(CAPSULE_BASE_LUM, lum, CAPSULE_ALPHA_PERMILLE);
        let capped = (color.luminance() as i32 - eff as i32).abs() as u8;
        if bare < READABILITY_MARGIN || capped < READABILITY_MARGIN {
            readable = false;
        }
    }
    cs.add("five_wallpapers_readable", readable, "");
    cs
}

#[cfg(test)]
mod f501_tests {
    use super::*;

    #[test]
    fn five_tier_case_table_exact() {
        // 判据「五档亮度壁纸×两字色自动选择用例」：用例表逐条精确匹配。
        for &(name, lum, expect) in FIVE_TIER_WALLPAPERS.iter() {
            assert!(brightness_tier(lum) <= 4, "壁纸 {} 亮度 {} 档位应在五档内", name, lum);
            assert_eq!(select_text_color(lum), expect, "壁纸 {} 亮度 {} 应选 {:?} 字", name, lum, expect);
        }
    }

    #[test]
    fn brightness_boundaries_exact() {
        // 五档边界值：52/103/154/205 恰为档位切换点。
        assert_eq!(brightness_tier(51), 0, "亮度 51 属最暗档");
        assert_eq!(brightness_tier(52), 1, "亮度 52 跨入第二档");
        assert_eq!(brightness_tier(103), 2, "亮度 103 跨入中间档");
        assert_eq!(brightness_tier(154), 3, "亮度 154 跨入第四档");
        assert_eq!(brightness_tier(205), 4, "亮度 205 跨入最亮档");
        assert_eq!(brightness_tier(255), 4, "纯白仍在最亮档");
    }

    #[test]
    fn shadow_alpha_40pct_blur_2px() {
        // 判据「投影参数实测」：透明度 40% = 400‰，模糊 2px。
        assert_eq!(SHADOW_ALPHA_PERMILLE, 400, "投影透明度 40%");
        assert_eq!(SHADOW_BLUR_PX, 2, "投影模糊 2px");
        assert_eq!(SHADOW_PARAMS, ShadowParams { alpha_permille: 400, blur_px: 2, offset_x_px: 1, offset_y_px: 1 });
    }

    #[test]
    fn dpi_4k_scaling_meets_spec() {
        // 判据「4K 渲染精度」：125%/150%/200% 下 2px 投影模糊换算后不缩水。
        assert_eq!(scale_px(2, 1250), 3);
        assert_eq!(scale_px(2, 1500), 3);
        assert_eq!(scale_px(2, 2000), 4);
        assert!(scaled_params_meet_spec(DPI_125_PERMILLE), "125% 下参数仍达标");
        assert!(scaled_params_meet_spec(DPI_200_PERMILLE), "200% 下参数仍达标");
    }

    #[test]
    fn dim_consistency_sweep() {
        // 判据「与 F297 压暗联动一致性」：压暗加深字色只向白字单向翻转。
        for lum in [0u8, 64, 128, 192, 255] {
            let mut prev_w = 0u32;
            for &dim in DIM_LEVELS_PERMILLE.iter() {
                let w = select_after_dim(lum, dim).whiteness();
                assert!(w >= prev_w, "亮度 {} 压暗 {}‰ 后字色不得从白翻回黑", lum, dim);
                prev_w = w;
            }
        }
        // 全黑壁纸压暗前后都是白字；纯白壁纸压暗到 600‰ 后翻成白字。
        assert_eq!(select_after_dim(0, 0), TextColor::White);
        assert_eq!(select_after_dim(255, 600), TextColor::White, "白壁纸压暗 60% 后有效亮度 102 < 128 应翻白字");
    }

    #[test]
    fn capsule_round_geometry_and_contrast() {
        // 判据「选中底衬」：半透明胶囊，圆角 = 文字块半高。
        let cap = capsule_for_text_height(28);
        assert_eq!(cap.corner_radius_px, 14, "圆角 = 半高即胶囊");
        assert_eq!(cap.pad_h_px, 14);
        // 白壁纸 245 上黑字 + 深灰胶囊：有效背景 173，黑字对比 173 ≥ 64。
        let eff = alpha_blend(CAPSULE_BASE_LUM, 245, CAPSULE_ALPHA_PERMILLE);
        assert_eq!(eff, 173);
        assert!((TextColor::Black.luminance() as i32 - eff as i32).abs() >= READABILITY_MARGIN as i32, "选中态胶囊上文字仍可读");
    }

    #[test]
    fn all_wallpapers_readable_margin() {
        // 无感标准：白壁纸、黑壁纸、花壁纸（密林/星空）上图标名字都看得清。
        for &(_, lum, color) in FIVE_TIER_WALLPAPERS.iter() {
            let margin = (color.luminance() as i32 - lum as i32).abs();
            assert!(margin >= READABILITY_MARGIN as i32, "亮度 {} 上选 {:?} 字对比裕量 {} 应 ≥ 64", lum, color, margin);
        }
    }
}

// ===========================================================================
// F502 图标文字两行封顶
// ===========================================================================

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 最多两行（主册：两行封顶）。
pub const MAX_LINES: usize = 2;
/// 每行约 8 个全角字符（主册：8 字换行参数）。
pub const LINE_CAP_FULLWIDTH: u32 = 8;
/// 行宽上限（半角单位）= 8 全角 × 2 = 16。
pub const LINE_CAP_UNITS: u32 = LINE_CAP_FULLWIDTH * 2;
/// 单行码点缓冲上限（全半角混排最坏情形：16 个半角码点）。
pub const MAX_LINE_CPS: usize = 16;
/// 尾部省略号 …（U+2026，全角宽度 2 单位）。
pub const ELLIPSIS: u32 = 0x2026;
pub const ELLIPSIS_UNITS: u32 = 2;

/// 字符显示宽度（半角单位）：全角 2、半角 1。
/// 全角判定覆盖 CJK 统一表意、CJK 标点、假名、兼容表意、全角形式与
/// 省略号（…按 CJK 惯例计全角）。
pub fn char_display_width(cp: u32) -> u32 {
    let fullwidth = matches!(cp,
        0x1100..=0x115F
        | 0x2018..=0x2019
        | 0x201C..=0x201D
        | 0x2026
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF01..=0xFF60
        | 0xFFE0..=0xFFE6
    );
    if fullwidth {
        2
    } else {
        1
    }
}

/// 英文单词字符（ASCII 字母数字 + 下划线）：换行不许在单词中间断。
pub fn is_word_char(cp: u32) -> bool {
    matches!(cp, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A | 0x5F)
}

/// 两行排版结果（定长缓冲，零堆）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TextLayout {
    line_cps: [[u32; MAX_LINE_CPS]; MAX_LINES],
    line_len: [usize; MAX_LINES],
    /// 内容超出两行、第二行尾部已加省略号。
    pub truncated: bool,
}

impl TextLayout {
    pub const fn new() -> Self {
        TextLayout {
            line_cps: [[0; MAX_LINE_CPS]; MAX_LINES],
            line_len: [0; MAX_LINES],
            truncated: false,
        }
    }

    /// 某一行的码点切片。
    pub fn line(&self, idx: usize) -> &[u32] {
        &self.line_cps[idx][..self.line_len[idx]]
    }

    /// 某行的显示宽度（半角单位）。
    pub fn line_units(&self, idx: usize) -> u32 {
        let mut u = 0u32;
        for &cp in self.line(idx) {
            u += char_display_width(cp);
        }
        u
    }

    /// 第二行尾部是否挂着省略号。
    pub fn ends_with_ellipsis(&self) -> bool {
        self.truncated && self.line_len[MAX_LINES - 1] > 0 && self.line_cps[MAX_LINES - 1][self.line_len[MAX_LINES - 1] - 1] == ELLIPSIS
    }

    /// 文字块总宽（半角单位，取两行最宽者——居中计算用）。
    pub fn block_units(&self) -> u32 {
        let mut m = 0u32;
        for idx in 0..MAX_LINES {
            let u = self.line_units(idx);
            if u > m {
                m = u;
            }
        }
        m
    }
}

/// 单行填充：从 cps[i] 起尽量装满一行。
/// 返回 (行码点数, 下一个未消费的下标)。
/// - 非末行：溢出时优先回退到最近的词界（空格/全角字之后），
///   无词界（整行连续单词）才硬断——「不在英文单词中间断」的兜底。
/// - 末行：溢出即截断，从尾部腾出省略号宽度后追加 …。
fn fill_line(cps: &[u32], mut i: usize, last_line: bool, buf: &mut [u32; MAX_LINE_CPS]) -> (usize, usize) {
    let mut units = 0u32;
    let mut len = 0usize;
    let mut last_break: Option<usize> = None;
    while i < cps.len() {
        let cp = cps[i];
        let w = char_display_width(cp);
        if units + w <= LINE_CAP_UNITS {
            if !is_word_char(cp) {
                // 词界：空格、标点、全角字之后都可断行。
                last_break = Some(len + 1);
            }
            buf[len] = cp;
            len += 1;
            units += w;
            i += 1;
            continue;
        }
        // 溢出。
        if last_line {
            // 尾部省略号：从行尾腾出 ELLIPSIS_UNITS 再挂 …（被让出的
            // 字符连同未消费部分一起隐藏——省略号表示「还有内容」）。
            while units + ELLIPSIS_UNITS > LINE_CAP_UNITS && len > 0 {
                len -= 1;
                units -= char_display_width(buf[len]);
            }
            buf[len] = ELLIPSIS;
            len += 1;
            return (len, i);
        }
        if let Some(b) = last_break {
            // 词界回退：b 之后的字符退回流。
            let give = len - b;
            i -= give;
            return (b, i);
        }
        // 无词界（整行连续单词）→ 硬断兜底。
        return (len, i);
    }
    (len, i)
}

/// 换行引擎：最多两行、每行 8 全角封顶、第二行超限尾部省略号、
/// 中英混排优先词界断行。
pub fn layout(cps: &[u32]) -> TextLayout {
    let mut out = TextLayout::new();
    let (l1, i) = fill_line(cps, 0, false, &mut out.line_cps[0]);
    out.line_len[0] = l1;
    if i >= cps.len() {
        out.truncated = false;
        return out;
    }
    let (l2, i2) = fill_line(cps, i, true, &mut out.line_cps[1]);
    out.line_len[1] = l2;
    // 末行截断时 i2 停在第一个被省略的字符上（含换行时被让出的尾部字符）。
    out.truncated = i2 < cps.len();
    out
}

/// 全文可达三路（主册：Tooltip F205 / 重命名 F260 / 属性 F264）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FullTextPath {
    Tooltip,
    Rename,
    Properties,
}

impl FullTextPath {
    pub fn name(self) -> &'static str {
        match self {
            FullTextPath::Tooltip => "tooltip-f205",
            FullTextPath::Rename => "rename-f260",
            FullTextPath::Properties => "properties-f264",
        }
    }
}

/// 三路可达路径表（省略后的全文看这三条路，逐路可达）。
pub const FULLTEXT_PATHS: [FullTextPath; 3] = [FullTextPath::Tooltip, FullTextPath::Rename, FullTextPath::Properties];

/// 文字块水平居中：偏移 = (容器宽 - 文字宽) / 2（文字过宽时贴 0）。
pub fn center_offset_px(text_px: u32, container_px: u32) -> u32 {
    container_px.saturating_sub(text_px) / 2
}

// ---------------------------------------------------------------------------
// F502 域自检
// ---------------------------------------------------------------------------

/// F502 域自检。
pub fn run_f502_checks() -> CheckSet {
    let mut cs = CheckSet::new("F502-icon-text-clamp");
    // 1) 宽度函数：全角 2、半角 1、省略号全角。
    cs.add(
        "display_width_cjk_fullwidth",
        char_display_width(0x4E2D) == 2 && char_display_width(0x6587) == 2 && char_display_width(ELLIPSIS) == 2,
        "",
    );
    cs.add(
        "display_width_ascii_halfwidth",
        char_display_width(b'A' as u32) == 1 && char_display_width(b'0' as u32) == 1 && char_display_width(b' ' as u32) == 1,
        "",
    );
    // 2) 换行参数常量：两行封顶、每行 8 全角（= 16 半角单位）。
    cs.add(
        "line_cap_constants",
        MAX_LINES == 2 && LINE_CAP_FULLWIDTH == 8 && LINE_CAP_UNITS == 16,
        "",
    );
    // 3) 恰好 16 全角 = 两行各 8，不触发省略。
    let exact: [u32; 16] = [
        0x9879, 0x76EE, 0x603B, 0x7ED3, 0x62A5, 0x544A, 0x6700, 0x7EC8, 0x7248, 0x5B58, 0x6863, 0x5B8C, 0x6574, 0x65E0, 0x7F3A, 0x75D5,
    ];
    let lay = layout(&exact);
    cs.add(
        "wrap_two_lines_no_truncate",
        !lay.truncated && lay.line(0).len() == 8 && lay.line(1).len() == 8 && !lay.ends_with_ellipsis(),
        "",
    );
    // 4) 20 全角 → 8 + 7…，省略号挂第二行尾。
    let mut twenty = [0u32; 20];
    for (k, c) in twenty.iter_mut().enumerate() {
        *c = 0x9879 + k as u32;
    }
    let lay2 = layout(&twenty);
    cs.add(
        "wrap_truncate_with_ellipsis",
        lay2.truncated && lay2.line(0).len() == 8 && lay2.line(1).len() == 8 && lay2.ends_with_ellipsis(),
        "",
    );
    // 5) 省略号永远表示「还有内容」：truncated ⇔ 行宽封顶下确有字符被隐藏。
    let mut ell_ok = true;
    for n in 0..=20u32 {
        let mut buf = [0u32; 20];
        for (k, c) in buf.iter_mut().enumerate().take(n as usize) {
            *c = 0x9879 + k as u32;
        }
        let lay = layout(&buf[..n as usize]);
        if lay.truncated != lay.ends_with_ellipsis() {
            ell_ok = false;
        }
        // 不截断时绝不能出现省略号（显示 bug 即此处）。
        if !lay.truncated && lay.line(1).contains(&ELLIPSIS) {
            ell_ok = false;
        }
    }
    cs.add("ellipsis_means_more_content", ell_ok, "");
    // 6) 行宽永不超 16 半角单位（两行逐行核对）。
    let mut cap_ok = true;
    for n in 0..=24u32 {
        let mut buf = [0u32; 24];
        for (k, c) in buf.iter_mut().enumerate().take(n as usize) {
            *c = 0x9879 + k as u32;
        }
        let lay = layout(&buf[..n as usize]);
        for idx in 0..MAX_LINES {
            if lay.line_units(idx) > LINE_CAP_UNITS {
                cap_ok = false;
            }
        }
    }
    cs.add("line_units_never_exceed_cap", cap_ok, "");
    // 7) 中英混排不在英文单词中间断：两行交界处不得同为单词字符
    //    （存在词界时）。
    let mixed: [u32; 12] = [
        b'W' as u32, b'e' as u32, b'b' as u32, 0x7CFB, 0x7EDF, 0x8BBE, 0x7F6E, 0x5927, 0x5168, 0x624B, 0x518C, 0x94BB,
    ];
    let lay3 = layout(&mixed);
    let boundary_ok = match (lay3.line(0).last(), lay3.line(1).first()) {
        (Some(&a), Some(&b)) => !(is_word_char(a) && is_word_char(b)),
        _ => true,
    };
    cs.add("mixed_text_no_midword_break", boundary_ok && lay3.line_units(0) <= LINE_CAP_UNITS, "");
    // 8) 纯长词无断点 → 硬断兜底：20 个半角字母 → 16 + 4，不丢内容不挂省略。
    let long_word: [u32; 20] = [
        b'a' as u32, b'b' as u32, b'c' as u32, b'd' as u32, b'e' as u32, b'f' as u32, b'g' as u32, b'h' as u32, b'i' as u32, b'j' as u32,
        b'k' as u32, b'l' as u32, b'm' as u32, b'n' as u32, b'o' as u32, b'p' as u32, b'q' as u32, b'r' as u32, b's' as u32, b't' as u32,
    ];
    let lay4 = layout(&long_word);
    cs.add(
        "long_word_hard_break_unavoidable",
        !lay4.truncated && lay4.line(0).len() == 16 && lay4.line(1).len() == 4,
        "",
    );
    // 9) 文字块整体水平居中：offset×2 + 文字宽 = 容器宽（整除情形）。
    cs.add(
        "text_block_centered",
        center_offset_px(64, 96) == 16 && center_offset_px(96, 96) == 0 && center_offset_px(120, 96) == 0,
        "",
    );
    // 10) 全文可达三路：Tooltip/重命名/属性逐路在册可达（F205/F260/F264）。
    cs.add(
        "fulltext_three_paths",
        FULLTEXT_PATHS.len() == 3
            && FULLTEXT_PATHS[0] == FullTextPath::Tooltip
            && FULLTEXT_PATHS[1] == FullTextPath::Rename
            && FULLTEXT_PATHS[2] == FullTextPath::Properties,
        "",
    );
    // 11) 主册体面截断样例：长文件名 17 全角 → 「8 字…」两行收场。
    let mut long_name = [0u32; 17];
    for (k, c) in long_name.iter_mut().enumerate() {
        *c = 0x9879 + k as u32;
    }
    let lay5 = layout(&long_name);
    cs.add(
        "long_filename_tidy_truncation",
        lay5.truncated && lay5.ends_with_ellipsis() && lay5.line_units(0) == LINE_CAP_UNITS && lay5.line_units(1) == LINE_CAP_UNITS,
        "",
    );
    // 12) 边界：空串/单字符/恰好一行——单行收场、无第二行内容。
    let empty = layout(&[]);
    let one = layout(&[0x9879]);
    cs.add(
        "empty_and_short_edges",
        !empty.truncated && empty.line(0).is_empty() && empty.line(1).is_empty()
            && !one.truncated && one.line(0).len() == 1 && one.line(1).is_empty(),
        "",
    );
    cs
}

#[cfg(test)]
mod f502_tests {
    use super::*;

    #[test]
    fn exactly_eight_fullwidth_per_line() {
        // 判据「两行/8 字换行参数」：每行恰 8 全角封顶。
        let mut cps = [0u32; 17];
        for (k, c) in cps.iter_mut().enumerate() {
            *c = 0x9879 + k as u32;
        }
        let lay = layout(&cps);
        assert_eq!(lay.line_units(0), 16, "第一行 8 全角 = 16 单位");
        assert_eq!(lay.line(0).len(), 8, "第一行 8 字");
        assert!(lay.line(1).len() <= 8, "第二行不超 8 字");
    }

    #[test]
    fn ellipsis_present_iff_truncated() {
        // 判据「省略触发」：省略号 ⇔ 确有内容被截；未截绝不出现省略号。
        for n in 0..=18u32 {
            let mut buf = [0u32; 18];
            for (k, c) in buf.iter_mut().enumerate().take(n as usize) {
                *c = 0x9879 + k as u32;
            }
            let lay = layout(&buf[..n as usize]);
            assert_eq!(lay.truncated, lay.ends_with_ellipsis(), "n={} 截断状态与省略号须一致", n);
            if n <= 16 {
                assert!(!lay.truncated, "16 全角以内不应截断（n={}）", n);
            } else {
                assert!(lay.truncated, "超过 16 全角必须截断（n={}）", n);
            }
        }
    }

    #[test]
    fn english_word_kept_whole() {
        // 判据「中英混排换行（不在英文单词中间断）」。
        // 「系统Webster设置大全」：Webster 一词不许拆到两行。
        let mixed: [u32; 14] = [
            0x7CFB, 0x7EDF, b'W' as u32, b'e' as u32, b'b' as u32, b's' as u32, b't' as u32, b'e' as u32, b'r' as u32, 0x8BBE, 0x7F6E,
            0x5927, 0x5168, 0x5F55,
        ];
        let lay = layout(&mixed);
        if let (Some(&a), Some(&b)) = (lay.line(0).last(), lay.line(1).first()) {
            assert!(!(is_word_char(a) && is_word_char(b)), "两行交界不得把英文单词劈开：行尾 {:x} 行首 {:x}", a, b);
        }
        // 全部内容仍可达（不截断情形下两行拼回原文）。
        assert!(!lay.truncated, "14 码点混排应两行装下");
    }

    #[test]
    fn long_word_hard_break_fallback() {
        // 无词界的纯长词：硬断兜底，内容不丢、不挂省略号。
        let word: [u32; 20] = [
            b'a' as u32, b'b' as u32, b'c' as u32, b'd' as u32, b'e' as u32, b'f' as u32, b'g' as u32, b'h' as u32, b'i' as u32, b'j' as u32,
            b'k' as u32, b'l' as u32, b'm' as u32, b'n' as u32, b'o' as u32, b'p' as u32, b'q' as u32, b'r' as u32, b's' as u32, b't' as u32,
        ];
        let lay = layout(&word);
        assert_eq!(lay.line(0).len(), 16, "第一行硬断在 16 半角");
        assert_eq!(lay.line(1).len(), 4, "剩余 4 字母进第二行");
        assert!(!lay.truncated, "20 半角两行装得下，不截断");
    }

    #[test]
    fn centering_math() {
        // 判据「居中对齐」：offset×2 + 文字宽 = 容器宽。
        assert_eq!(center_offset_px(64, 96), 16);
        assert_eq!(center_offset_px(0, 80), 40);
        assert_eq!(center_offset_px(80, 80), 0);
        // 文字过宽时贴 0（饱和，不下溢）。
        assert_eq!(center_offset_px(200, 96), 0);
    }

    #[test]
    fn three_fulltext_paths_reachable() {
        // 判据「省略触发与全文可达三路」：Tooltip(F205)/重命名(F260)/属性(F264)。
        assert_eq!(FULLTEXT_PATHS.len(), 3);
        assert_eq!(FULLTEXT_PATHS[0].name(), "tooltip-f205");
        assert_eq!(FULLTEXT_PATHS[1].name(), "rename-f260");
        assert_eq!(FULLTEXT_PATHS[2].name(), "properties-f264");
    }

    #[test]
    fn line_cap_never_exceeded() {
        // 判据「两行封顶」：任何输入行宽不超 8 全角，行数不超 2。
        for n in 0..=24u32 {
            let mut buf = [0u32; 24];
            for (k, c) in buf.iter_mut().enumerate().take(n as usize) {
                *c = 0x9879 + k as u32;
            }
            let lay = layout(&buf[..n as usize]);
            assert!(lay.line_units(0) <= LINE_CAP_UNITS, "n={} 第一行超宽", n);
            assert!(lay.line_units(1) <= LINE_CAP_UNITS, "n={} 第二行超宽", n);
        }
    }
}

// ===========================================================================
// F503 图标网格密度
// ===========================================================================

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 三档格距（主册：宽松/标准/紧凑——96/80/64px）。
pub const GRID_LOOSE_PX: u32 = 96;
pub const GRID_STD_PX: u32 = 80;
pub const GRID_COMPACT_PX: u32 = 64;
/// 自定义间隔步进（主册：8px 步进）。
pub const CUSTOM_STEP_PX: u32 = 8;
/// 自定义行距/列距允许范围（进阶入口钳制区间，均为 8 的倍数）。
pub const CUSTOM_MIN_PX: u32 = 32;
pub const CUSTOM_MAX_PX: u32 = 160;
/// 桌面图标上限（定长槽位，零堆）。
pub const MAX_DESKTOP_ICONS: usize = 64;
/// 行主序 key 编码跨度（row × KEY_SPAN + col）。
const KEY_SPAN: i64 = 1 << 20;

/// 网格密度三档。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum GridDensity {
    Compact,
    Standard,
    Loose,
}

impl GridDensity {
    pub fn cell_px(self) -> u32 {
        match self {
            GridDensity::Loose => GRID_LOOSE_PX,
            GridDensity::Standard => GRID_STD_PX,
            GridDensity::Compact => GRID_COMPACT_PX,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            GridDensity::Loose => "loose-96",
            GridDensity::Standard => "standard-80",
            GridDensity::Compact => "compact-64",
        }
    }
}

/// 8px 步进对齐：非法值四舍五入吸附到 8 的倍数（0 吸附到 8 防零格距）。
pub fn step8_align(v: u32) -> u32 {
    let aligned = (v + CUSTOM_STEP_PX / 2) / CUSTOM_STEP_PX * CUSTOM_STEP_PX;
    if aligned == 0 {
        CUSTOM_STEP_PX
    } else {
        aligned
    }
}

/// 自定义间隔进阶入口：行距/列距独立调，先钳制到 [32,160] 再 8px 对齐。
pub fn clamp_custom_spacing(v: u32) -> u32 {
    let c = if v < CUSTOM_MIN_PX {
        CUSTOM_MIN_PX
    } else if v > CUSTOM_MAX_PX {
        CUSTOM_MAX_PX
    } else {
        v
    };
    step8_align(c)
}

/// 图标格位（col/row 可为任意整数格）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GridPos {
    pub col: i32,
    pub row: i32,
}

impl GridPos {
    pub const fn new(col: i32, row: i32) -> Self {
        GridPos { col, row }
    }
    /// 行主序 key（同 row 比 col，严格全序——顺序保持判定用）。
    pub fn key(self) -> i64 {
        self.row as i64 * KEY_SPAN + self.col as i64
    }
    pub fn from_key(k: i64) -> Self {
        GridPos {
            row: k.div_euclid(KEY_SPAN) as i32,
            col: k.rem_euclid(KEY_SPAN) as i32,
        }
    }
    /// 格位 → 像素坐标（格距换算）。
    pub fn to_px(self, cell: u32) -> (i32, i32) {
        (self.col * cell as i32, self.row * cell as i32)
    }
}

/// 最近格吸附：像素坐标 → 最近格线（四舍五入，格心距最短）。
pub fn nearest_snap_px(pixels: i32, cell: u32) -> i32 {
    ((2 * pixels as i64 + cell as i64).div_euclid(2 * cell as i64)) as i32
}

/// 单图标换密度吸附：旧格位 → 像素 → 新格距下最近格。
pub fn snap_to_grid(p: GridPos, old_cell: u32, new_cell: u32) -> GridPos {
    let (px, py) = p.to_px(old_cell);
    GridPos::new(nearest_snap_px(px, new_cell), nearest_snap_px(py, new_cell))
}

/// 重排吸附算法（主册：重排最近格吸附与相对位置保持）：
/// 输入旧密度下的图标格位（按行主序给出），输出新密度下的格位数组。
/// 规则：逐个最近格吸附；撞格时按行主序向后找第一个空格——只向后推，
/// 保证输入顺序（邻居关系）不变、无重叠。
/// 返回写入 out 的图标数。
pub fn rearrange(old_cell: u32, new_cell: u32, icons: &[GridPos], out: &mut [GridPos; MAX_DESKTOP_ICONS]) -> usize {
    let n = icons.len().min(MAX_DESKTOP_ICONS);
    let mut count = 0usize;
    for idx in 0..n {
        let snapped = snap_to_grid(icons[idx], old_cell, new_cell);
        let mut k = snapped.key();
        loop {
            let mut conflict = false;
            for j in 0..count {
                if out[j].key() == k {
                    conflict = true;
                    break;
                }
            }
            if !conflict {
                break;
            }
            k += 1;
        }
        out[count] = GridPos::from_key(k);
        count += 1;
    }
    count
}

/// 工作区可容纳图标数（容量模型：紧凑档必须比标准/宽松装得多）。
pub fn icons_fit(work_w_px: u32, work_h_px: u32, cell: u32) -> u32 {
    (work_w_px / cell) * (work_h_px / cell)
}

/// 密度设置状态机（预览即时性）：select 即生效——没有 pending 暂存、
/// 永远不进确认态（主册：即时看效果不用确定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DensityState {
    current: GridDensity,
    applied_gen: u32,
}

impl DensityState {
    pub const fn new() -> Self {
        DensityState {
            current: GridDensity::Standard,
            applied_gen: 0,
        }
    }
    /// 选择即生效（即时预览语义：改 current、世代 +1，无确认步骤）。
    pub fn select(&mut self, d: GridDensity) {
        self.current = d;
        self.applied_gen += 1;
    }
    pub fn current(&self) -> GridDensity {
        self.current
    }
    pub fn applied_generation(&self) -> u32 {
        self.applied_gen
    }
    /// 预览即时性判据：切换永不经过确认态。
    pub fn needs_confirmation(&self) -> bool {
        false
    }
}

/// 排列模式（F084 网格 / F401 自动排列兼容）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrangeMode {
    /// 锁定网格：图标保持在既有格位（F084）。
    Locked,
    /// 自由自动排列：按行主序重新铺位（F401）。
    Free,
}

/// 排列命令：锁定模式原位保持；自由模式按行主序铺位（cols = 每行列数）。
/// 两模式结果都落在合法格点、互不重叠——排列命令与密度档位解耦一致。
pub fn arrange(icons: &[GridPos], cols: u32, mode: ArrangeMode, out: &mut [GridPos; MAX_DESKTOP_ICONS]) -> usize {
    let n = icons.len().min(MAX_DESKTOP_ICONS);
    match mode {
        ArrangeMode::Locked => {
            for i in 0..n {
                out[i] = icons[i];
            }
        }
        ArrangeMode::Free => {
            for i in 0..n {
                out[i] = GridPos::new((i as u32 % cols) as i32, (i as u32 / cols) as i32);
            }
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F503 域自检
// ---------------------------------------------------------------------------

/// F503 域自检。
pub fn run_f503_checks() -> CheckSet {
    let mut cs = CheckSet::new("F503-icon-grid-density");
    // 1) 三档格距常量：96/80/64（主册逐字）。
    cs.add(
        "density_triplet_constants",
        GRID_LOOSE_PX == 96 && GRID_STD_PX == 80 && GRID_COMPACT_PX == 64 && CUSTOM_STEP_PX == 8,
        "",
    );
    // 2) 档位序：紧凑 < 标准 < 宽松（枚举序与格距序一致）。
    cs.add(
        "density_ordering",
        GridDensity::Compact < GridDensity::Standard
            && GridDensity::Standard < GridDensity::Loose
            && GridDensity::Compact.cell_px() < GridDensity::Standard.cell_px()
            && GridDensity::Standard.cell_px() < GridDensity::Loose.cell_px(),
        "",
    );
    // 3) 8px 步进对齐：非法值吸附到 8 的倍数（60→64、67→64、68→72）。
    cs.add(
        "step8_align_snapping",
        step8_align(60) == 64 && step8_align(67) == 64 && step8_align(68) == 72 && step8_align(64) == 64 && step8_align(0) == 8,
        "",
    );
    // 4) 步进对齐幂等：对齐后再对齐不变（吸附是稳定不动点）。
    let mut idem = true;
    for v in [0u32, 3, 8, 33, 70, 127, 159, 200] {
        let a = step8_align(v);
        if step8_align(a) != a {
            idem = false;
        }
        if a % CUSTOM_STEP_PX != 0 {
            idem = false;
        }
    }
    cs.add("step8_align_idempotent", idem, "");
    // 5) 自定义间隔钳制：越界吸附回 [32,160] 且保持 8 的倍数。
    cs.add(
        "custom_spacing_clamped_range",
        clamp_custom_spacing(3) == 32 && clamp_custom_spacing(33) == 32 && clamp_custom_spacing(159) == 160 && clamp_custom_spacing(999) == 160,
        "",
    );
    // 6) 最近格吸附数学：96→80 密度下像素 288 → 格 4（288/80=3.6），
    //    像素 240 → 格 3（距离 0 < 40）。
    cs.add(
        "rearrange_nearest_snap",
        nearest_snap_px(288, 80) == 4 && nearest_snap_px(240, 80) == 3 && nearest_snap_px(96, 64) == 2,
        "",
    );
    // 7) 重排无重叠：换密度后所有格位互不相同。
    let icons = [
        GridPos::new(0, 0),
        GridPos::new(1, 0),
        GridPos::new(2, 0),
        GridPos::new(0, 1),
        GridPos::new(1, 1),
        GridPos::new(2, 1),
    ];
    let mut out = [GridPos::new(0, 0); MAX_DESKTOP_ICONS];
    let n = rearrange(GRID_LOOSE_PX, GRID_COMPACT_PX, &icons, &mut out);
    let mut no_overlap = n == icons.len();
    for i in 0..n {
        for j in (i + 1)..n {
            if out[i] == out[j] {
                no_overlap = false;
            }
        }
    }
    cs.add("rearrange_no_overlap", no_overlap, "");
    // 8) 顺序保持（不乱序）：行主序 key 严格递增——邻居关系不变。
    let mut order_ok = true;
    for i in 0..n.saturating_sub(1) {
        if out[i].key() >= out[i + 1].key() {
            order_ok = false;
        }
    }
    cs.add("rearrange_order_preserved", order_ok, "");
    // 9) 相对位置保持：三连排图标 96→80 密度后仍三连排（间距一致）。
    let trio = [GridPos::new(0, 0), GridPos::new(1, 0), GridPos::new(2, 0)];
    let mut out2 = [GridPos::new(0, 0); MAX_DESKTOP_ICONS];
    rearrange(GRID_LOOSE_PX, GRID_STD_PX, &trio, &mut out2);
    cs.add(
        "relative_position_kept",
        out2[0] == GridPos::new(0, 0) && out2[1] == GridPos::new(1, 0) && out2[2] == GridPos::new(2, 0),
        "",
    );
    // 10) 预览即时性：select 即生效、无确认态、世代推进（即时看效果不用确定）。
    let mut st = DensityState::new();
    let before = st.current();
    st.select(GridDensity::Compact);
    cs.add(
        "immediate_preview_no_confirm",
        before == GridDensity::Standard && st.current() == GridDensity::Compact && !st.needs_confirmation() && st.applied_generation() == 1,
        "",
    );
    // 11) F084/F401 兼容：锁定模式原位保持；自由模式行主序铺位且无重叠；
    //     两模式产出图标数一致。
    let mut locked_out = [GridPos::new(0, 0); MAX_DESKTOP_ICONS];
    let mut free_out = [GridPos::new(0, 0); MAX_DESKTOP_ICONS];
    let nl = arrange(&icons, 3, ArrangeMode::Locked, &mut locked_out);
    let nf = arrange(&icons, 3, ArrangeMode::Free, &mut free_out);
    let mut free_clean = nf == icons.len();
    for i in 0..nf {
        if free_out[i] != GridPos::new((i % 3) as i32, (i / 3) as i32) {
            free_clean = false;
        }
        for j in (i + 1)..nf {
            if free_out[i] == free_out[j] {
                free_clean = false;
            }
        }
    }
    let mut locked_kept = nl == icons.len();
    for i in 0..nl {
        if locked_out[i] != icons[i] {
            locked_kept = false;
        }
    }
    cs.add("f084_f401_arrange_compat", free_clean && locked_kept && nl == nf, "");
    // 12) 容量序：4K 工作区（3840×2160，留边 96px）紧凑 > 标准 > 宽松。
    let (w, h) = (3840 - 2 * 96, 2160 - 2 * 96);
    cs.add(
        "compact_fits_more_icons",
        icons_fit(w, h, GRID_COMPACT_PX) > icons_fit(w, h, GRID_STD_PX) && icons_fit(w, h, GRID_STD_PX) > icons_fit(w, h, GRID_LOOSE_PX),
        "",
    );
    cs
}

#[cfg(test)]
mod f503_tests {
    use super::*;

    #[test]
    fn grid_constants_match_criteria() {
        // 判据「三档格距」：宽松 96 / 标准 80 / 紧凑 64，自定义步进 8px。
        assert_eq!(GRID_LOOSE_PX, 96);
        assert_eq!(GRID_STD_PX, 80);
        assert_eq!(GRID_COMPACT_PX, 64);
        assert_eq!(CUSTOM_STEP_PX, 8);
        assert_eq!(GridDensity::Loose.cell_px(), 96);
        assert_eq!(GridDensity::Standard.cell_px(), 80);
        assert_eq!(GridDensity::Compact.cell_px(), 64);
    }

    #[test]
    fn step8_boundaries() {
        // 判据「自定义 8px 步进」：非法值吸附到 8 的倍数（边界值核对）。
        assert_eq!(step8_align(60), 64, "60 吸附到 64（距 64 为 4 < 距 56 为 4 取上）");
        assert_eq!(step8_align(61), 64);
        assert_eq!(step8_align(67), 64, "67 吸附到 64");
        assert_eq!(step8_align(68), 72, "68 吸附到 72");
        assert_eq!(step8_align(1), 8, "近零值吸附到最小步进 8");
        // 幂等性。
        for v in [0u32, 9, 33, 70, 159] {
            let a = step8_align(v);
            assert_eq!(step8_align(a), a, "步进对齐应为不动点：{}", v);
        }
    }

    #[test]
    fn rearrange_density_switch_neighbors_stay() {
        // 判据「切换时图标搬家但邻居关系不变」：96→64 密度后顺序不乱。
        let icons = [
            GridPos::new(0, 0),
            GridPos::new(1, 0),
            GridPos::new(2, 0),
            GridPos::new(3, 0),
        ];
        let mut out = [GridPos::new(0, 0); MAX_DESKTOP_ICONS];
        let n = rearrange(GRID_LOOSE_PX, GRID_COMPACT_PX, &icons, &mut out);
        assert_eq!(n, 4);
        for i in 0..n - 1 {
            assert!(out[i].key() < out[i + 1].key(), "图标 {} 顺序不得乱", i);
        }
        // 96px 间隔在 64 格距下吸附：96px → 格 2（1.5 四舍五入）。
        assert_eq!(out[1].col, 2, "96px 处图标吸附到格 2");
        assert_eq!(out[3].col, 5, "288px 处图标吸附到格 5（4.5 平局取近邻，确定性归一）");
    }

    #[test]
    fn overlap_resolved_by_push_forward() {
        // 撞格情形：两图标吸附到同一格时后者向后推一格，仍无重叠不乱序。
        // 96 密度下 (0,0) 与 (1,0) 换到 200px 超大格距：两者都吸附到格 0/1?——
        // 用 128 格距：96px → 0.75 → 格 1；192px → 1.5 → 格 2?（不会撞）。
        // 构造必撞：192px 与 199px 在 128 格距下都吸附到格 2? 不撞；
        // 直接用 160 格距：96→0.6→1、192→1.2→1 → 撞格 → 后者推到格 2。
        let icons = [GridPos::new(0, 0), GridPos::new(1, 0), GridPos::new(2, 0)];
        let mut out = [GridPos::new(0, 0); MAX_DESKTOP_ICONS];
        let n = rearrange(GRID_LOOSE_PX, 160, &icons, &mut out);
        assert_eq!(n, 3);
        assert_ne!(out[0], out[1], "撞格必须被推开");
        assert_ne!(out[1], out[2], "连锁推开亦不重叠");
        assert!(out[0].key() < out[1].key() && out[1].key() < out[2].key(), "推开后仍保持行主序");
    }

    #[test]
    fn free_arrange_reading_order() {
        // 判据「与自动排列 F401 兼容」：自由模式按行主序铺位。
        let junk = [
            GridPos::new(5, 3),
            GridPos::new(0, 7),
            GridPos::new(2, 1),
            GridPos::new(9, 9),
        ];
        let mut out = [GridPos::new(0, 0); MAX_DESKTOP_ICONS];
        let n = arrange(&junk, 2, ArrangeMode::Free, &mut out);
        assert_eq!(n, 4);
        assert_eq!(out[0], GridPos::new(0, 0));
        assert_eq!(out[1], GridPos::new(1, 0));
        assert_eq!(out[2], GridPos::new(0, 1));
        assert_eq!(out[3], GridPos::new(1, 1));
        // 锁定模式（F084）：原位保持。
        let mut lout = [GridPos::new(0, 0); MAX_DESKTOP_ICONS];
        arrange(&junk, 2, ArrangeMode::Locked, &mut lout);
        for i in 0..4 {
            assert_eq!(lout[i], junk[i], "锁定模式图标 {} 不得挪位", i);
        }
    }

    #[test]
    fn immediate_semantics_no_pending() {
        // 判据「预览即时性」：即时看效果不用确定——无确认态。
        let mut st = DensityState::new();
        assert_eq!(st.current(), GridDensity::Standard);
        st.select(GridDensity::Compact);
        assert_eq!(st.current(), GridDensity::Compact, "select 即生效");
        assert!(!st.needs_confirmation(), "永不进入确认态");
        assert_eq!(st.applied_generation(), 1, "每次选择世代 +1");
        st.select(GridDensity::Loose);
        assert_eq!(st.current(), GridDensity::Loose);
        assert_eq!(st.applied_generation(), 2);
    }

    #[test]
    fn compact_capacity_gain() {
        // 判据「三档格距」：密度越紧容量越大（4K 工作区模型）。
        let (w, h) = (3648u32, 1968u32); // 3840×2160 留边 96px
        let loose = icons_fit(w, h, GRID_LOOSE_PX);
        let std = icons_fit(w, h, GRID_STD_PX);
        let compact = icons_fit(w, h, GRID_COMPACT_PX);
        assert!(compact > std, "紧凑 {} 须多于标准 {}", compact, std);
        assert!(std > loose, "标准 {} 须多于宽松 {}", std, loose);
        // 容量数学：3648/64=57, 1968/64=30 → 1710。
        assert_eq!(compact, 57 * 30);
    }
}
