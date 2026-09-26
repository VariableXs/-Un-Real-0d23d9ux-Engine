//! NOVA-4K · src/design/tokens.css → 内核原生设计令牌移植（A-1 单一事实源同步）。
//!
//! 目标：src 前端的全部视觉令牌在内核里 1:1 在位，合成器/桌面自绘直接取用，
//! 4K（3840×2160）下按 dp 缩放引擎等比放大，画质无损（矢量令牌 + 高分辨率资产）。
//! 纯逻辑 + 固定容量：no_std / 仅 core，无分配、无 unsafe。
//!
//! 同步纪律：本文件与 src/design/tokens.css 数值一一对应；改 src 令牌必须同步改这里。

use crate::checks::CheckSet;

// ===========================================================================
// 1. 字阶（12/13/15/17/20/24/32，1.25 比例）—— design px
// ===========================================================================
pub const FS_12: u32 = 12;
pub const FS_13: u32 = 13;
pub const FS_15: u32 = 15;
pub const FS_17: u32 = 17;
pub const FS_20: u32 = 20;
pub const FS_24: u32 = 24;
pub const FS_32: u32 = 32;

// ===========================================================================
// 2. 间距（4px 基数 8 档）
// ===========================================================================
pub const SP_1: u32 = 4;
pub const SP_2: u32 = 8;
pub const SP_3: u32 = 12;
pub const SP_4: u32 = 16;
pub const SP_5: u32 = 24;
pub const SP_6: u32 = 32;
pub const SP_7: u32 = 48;
pub const SP_8: u32 = 64;

// ===========================================================================
// 3. 圆角（窗口 16 / 卡片 12 / 控件 8 / 小件 4）
// ===========================================================================
pub const R_WINDOW: u32 = 16;
pub const R_CARD: u32 = 12;
pub const R_CONTROL: u32 = 8;
pub const R_CHIP: u32 = 4;

// ===========================================================================
// 4. 阴影 elevation 0-5（贴合壁纸层，非纯黑）—— (blur_px, alpha_permille, dy_px)
//    y 偏移与模糊半径来自 tokens.css 的 elev-1..5；色彩恒 oklch(0.15 0.02 260)。
// ===========================================================================
pub const ELEV_SHADOW_HUE_MILLI: u32 = 260_000;
pub const ELEV_SHADOW_L_MILLI: u32 = 150;
pub const ELEV_SHADOW_C_MILLI: u32 = 20;
pub const ELEV: [(u32, u32, u32); 6] = [
    (0, 0, 0),      // elev-0: none
    (3, 180, 1),    // elev-1: 0 1px 3px / .18
    (8, 220, 2),    // elev-2: 0 2px 8px / .22
    (18, 280, 6),   // elev-3: 0 6px 18px / .28
    (32, 340, 12),  // elev-4: 0 12px 32px / .34
    (64, 400, 24),  // elev-5: 0 24px 64px / .40
];

// ===========================================================================
// 5. 动效：曲线控制点（cubic-bezier 千分比）+ 六档时长（ms，dur-scale=1 基准）
// ===========================================================================
/// cubic-bezier(x1,y1,x2,y2)，分量千分比（1000 = 1.0）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bezier(pub i32, pub i32, pub i32, pub i32);

pub const EASE_STANDARD: Bezier = Bezier(200, 0, 100, 1000); // cubic-bezier(0.2, 0, 0.1, 1)
pub const EASE_EMPHASIZED: Bezier = Bezier(300, 0, 50, 1000); // cubic-bezier(0.3, 0, 0.05, 1)
pub const EASE_SPRING: Bezier = Bezier(340, 1400, 640, 1000); // cubic-bezier(0.34, 1.4, 0.64, 1)

pub const DUR_1: u32 = 80;
pub const DUR_2: u32 = 120;
pub const DUR_3: u32 = 170;
pub const DUR_4: u32 = 200;
pub const DUR_5: u32 = 240;
pub const DUR_6: u32 = 320;

/// M-75 动效时长缩放：全局乘数（千分比，1000 = 1x）。reduceMotion 恒走 DUR_1。
pub const MOTION_SPEED_DEFAULT_MILLI: u32 = 1000;
pub const MOTION_SPEED_HALF_MILLI: u32 = 500;
pub const MOTION_SPEED_ONE5_MILLI: u32 = 1500;

pub fn dur_scaled(base_ms: u32, speed_milli: u32) -> u32 {
    (base_ms * speed_milli + 500) / 1000
}

// ===========================================================================
// 6. Fluent 控件规格（Z-02 --ctl-*）
// ===========================================================================
pub const CTL_BTN: u32 = 32;
pub const CTL_BTN_MINW: u32 = 120;
pub const CTL_INPUT: u32 = 32;
pub const CTL_MENU_ITEM: u32 = 36;
pub const CTL_CHECKBOX: u32 = 20;
pub const CTL_TOUCH: u32 = 44; // 触控命中红线
pub const CTL_DIVIDER: u32 = 1;
pub const CTL_PAD_BTN: u32 = 4;

// V-74 紧凑密度档（间距 -20%、菜单行高 36→31）
pub const CTL_MENU_ITEM_COMPACT: u32 = 31;

// 悬停延迟三档（M-71）
pub const HOVER_DELAY_FAST: u32 = 200;
pub const HOVER_DELAY_STD: u32 = 400;
pub const HOVER_DELAY_SLOW: u32 = 600;

// 图标档位（V-78 / U-09）
pub const ICON_UI: u32 = 20;
pub const ICON_TIERS: [u32; 3] = [16, 20, 24];

// ===========================================================================
// 7. OKLCH 色彩令牌（语义层三主题）—— 分量毫单位（0.90 → 900，262° → 262000）
// ===========================================================================
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Oklch {
    /// L 0-1000（0=黑 1=白）
    pub l: u32,
    /// C 色度 0-1000
    pub c: u32,
    /// H 色相，毫度（262.0° → 262000）
    pub h: u32,
    /// 透明度 0-1000（1000 = 不透明）
    pub a: u32,
}

impl Oklch {
    pub const fn new(l: u32, c: u32, h: u32) -> Oklch {
        Oklch { l, c, h, a: 1000 }
    }
    pub const fn with_alpha(mut self, a: u32) -> Oklch {
        self.a = a;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    /// dark（默认）
    Dark,
    /// light / paper
    Light,
    /// high-contrast（F-7.1）
    HighContrast,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorRole {
    BgCanvas,
    BgSurface,
    BgRaised,
    TextPrimary,
    TextSecondary,
    Accent,
    AccentSoft,
    Success,
    Warn,
    Danger,
    Stroke,
    TooltipBg,
    TooltipFg,
}

/// 语义层（dark 默认；light / high-contrast 亮度层覆盖）。与 tokens.css 逐行对应。
pub const fn color(role: ColorRole, theme: Theme) -> Oklch {
    match theme {
        Theme::Dark => match role {
            ColorRole::BgCanvas => Oklch::new(140, 20, 262_000),
            ColorRole::BgSurface => Oklch::new(180, 25, 262_000).with_alpha(720),
            ColorRole::BgRaised => Oklch::new(200, 25, 262_000).with_alpha(920),
            ColorRole::TextPrimary => Oklch::new(900, 20, 262_000),
            ColorRole::TextSecondary => Oklch::new(650, 30, 262_000),
            ColorRole::Accent => Oklch::new(680, 90, 262_000),
            ColorRole::AccentSoft => Oklch::new(680, 90, 262_000).with_alpha(160),
            ColorRole::Success => Oklch::new(780, 120, 160_000),
            ColorRole::Warn => Oklch::new(820, 130, 85_000),
            ColorRole::Danger => Oklch::new(680, 150, 25_000),
            ColorRole::Stroke => Oklch::new(700, 20, 262_000).with_alpha(140),
            ColorRole::TooltipBg => Oklch::new(220, 20, 262_000).with_alpha(940),
            ColorRole::TooltipFg => Oklch::new(940, 10, 262_000),
        },
        Theme::Light => match role {
            ColorRole::BgCanvas => Oklch::new(930, 20, 90_000),
            ColorRole::BgSurface => Oklch::new(970, 15, 90_000).with_alpha(860),
            ColorRole::BgRaised => Oklch::new(980, 10, 90_000).with_alpha(940),
            ColorRole::TextPrimary => Oklch::new(280, 20, 90_000),
            ColorRole::TextSecondary => Oklch::new(520, 30, 90_000),
            ColorRole::Accent => Oklch::new(520, 80, 75_000),
            ColorRole::AccentSoft => Oklch::new(520, 80, 75_000).with_alpha(140),
            ColorRole::Success => Oklch::new(780, 120, 160_000),
            ColorRole::Warn => Oklch::new(820, 130, 85_000),
            ColorRole::Danger => Oklch::new(680, 150, 25_000),
            ColorRole::Stroke => Oklch::new(350, 20, 90_000).with_alpha(180),
            ColorRole::TooltipBg => Oklch::new(300, 20, 90_000).with_alpha(960),
            ColorRole::TooltipFg => Oklch::new(970, 10, 90_000),
        },
        Theme::HighContrast => match role {
            ColorRole::BgCanvas => Oklch::new(0, 0, 0),
            ColorRole::BgSurface => Oklch::new(0, 0, 0).with_alpha(940),
            ColorRole::BgRaised => Oklch::new(0, 0, 0),
            ColorRole::TextPrimary => Oklch::new(1000, 0, 0),
            ColorRole::TextSecondary => Oklch::new(850, 0, 0),
            ColorRole::Accent => Oklch::new(870, 160, 95_000),
            ColorRole::AccentSoft => Oklch::new(870, 160, 95_000).with_alpha(220),
            ColorRole::Success => Oklch::new(820, 200, 150_000),
            ColorRole::Warn => Oklch::new(850, 160, 90_000),
            ColorRole::Danger => Oklch::new(660, 210, 25_000),
            ColorRole::Stroke => Oklch::new(650, 0, 0),
            ColorRole::TooltipBg => Oklch::new(0, 0, 0),
            ColorRole::TooltipFg => Oklch::new(1000, 0, 0),
        },
    }
}

// ---- W2 视觉语言组扩展（F01626~F01650 / 色温档 / 选择色）----
pub const W2_ELEV_6: (u32, u32, u32) = (96, 480, 32); // blur/alpha/dy
pub const W2_BLUR: [u32; 3] = [8, 16, 28];
pub const W2_GLASS_ALPHA: u32 = 60; // oklch(1 0 0 / .06)
pub const W2_GLOW_ALPHA: u32 = 350; // 0 0 12px accent /.35
pub const W2_ROW_H: u32 = 40;
pub const W2_CONTROL_H: u32 = 32;
pub const W2_SCROLLBAR: u32 = 10;
pub const W2_LH_MILLI: u32 = 1600; // 行高 1.6
pub const W2_THEME_DUR: u32 = 240; // 明暗过渡
pub const W2_SELECTION: Oklch = Oklch::new(680, 90, 262_000).with_alpha(350);
pub const W2_WARM_DEEP: Oklch = Oklch::new(560, 80, 55_000); // 2700K
pub const W2_WARM: Oklch = Oklch::new(620, 80, 70_000); // 3800K
pub const W2_NEUTRAL: Oklch = Oklch::new(650, 60, 180_000); // 5000K
pub const W2_DAY: Oklch = Oklch::new(680, 60, 250_000); // 6500K

// ---- 电源剧场环境光五档（AI-02 veil，全亮=现状）----
pub const PX2_VEIL: [Oklch; 5] = [
    Oklch::new(1000, 0, 0).with_alpha(0),
    Oklch::new(150, 20, 260_000).with_alpha(120),
    Oklch::new(150, 20, 260_000).with_alpha(280),
    Oklch::new(150, 20, 260_000).with_alpha(480),
    Oklch::new(150, 20, 260_000).with_alpha(720),
];

// ===========================================================================
// 8. OKLCH → sRGB 转换（纯 core 数学：cbrt/sin/cos 用牛顿迭代与多项式逼近）
// ===========================================================================
/// f32 立方根（归一化 + 牛顿迭代，无 std 依赖）。
fn f_cbrt(x: f32) -> f32 {
    if x == 0.0 {
        return 0.0;
    }
    let neg = x < 0.0;
    let mut x = x.abs();
    // 归一化到 [1, 8)：每除 8 结果乘 2
    let mut e = 1.0f32;
    while x > 8.0 {
        x /= 8.0;
        e *= 2.0;
    }
    while x < 1.0 {
        x *= 8.0;
        e *= 0.5;
    }
    let mut y = 1.4 * x - 0.5; // [1,8) 区间良好初值
    let mut i = 0;
    while i < 12 {
        let y2 = y * y;
        let ny = (2.0 * y + x / y2) / 3.0;
        if ny == y {
            break;
        }
        y = ny;
        i += 1;
    }
    let r = y * e;
    if neg {
        -r
    } else {
        r
    }
}

/// 角度（度）正弦：先归约到 [-90°, 90°] 再泰勒展开（该区间 4 项误差 < 3e-4）。
fn f_sin_deg(deg: f32) -> f32 {
    let mut d = deg % 360.0;
    if d < 0.0 {
        d += 360.0;
    }
    // 象限归约：得到 [-90, 90] 区间的等价角
    let e = if d <= 90.0 {
        d
    } else if d <= 180.0 {
        180.0 - d
    } else if d <= 270.0 {
        -(d - 180.0)
    } else {
        d - 360.0
    };
    let r = e * core::f32::consts::PI / 180.0;
    let r2 = r * r;
    r * (1.0 - r2 / 6.0 * (1.0 - r2 / 20.0 * (1.0 - r2 / 42.0)))
}

fn f_cos_deg(deg: f32) -> f32 {
    f_sin_deg(deg + 90.0)
}

/// OKLCH → sRGB（输出 0-255 各分量 + alpha 0-255）。
pub fn oklch_to_rgba8(c: Oklch) -> (u8, u8, u8, u8) {
    // OKLCH → OKLab
    let hr = c.h as f32 / 1000.0;
    let l_ = c.l as f32 / 1000.0;
    let c_ = c.c as f32 / 1000.0;
    let aa = c_ * f_cos_deg(hr);
    let bb = c_ * f_sin_deg(hr);
    // OKLab → LMS'（Björn Ottosson 公式），再立方得 LMS
    let mm = l_ - 0.1055613458 * aa - 0.0638541728 * bb;
    let ss = l_ - 0.0894841775 * aa - 1.2914855480 * bb;
    let l_ = l_ + 0.3963377774 * aa + 0.2158037573 * bb;
    let l3 = l_ * l_ * l_;
    let m3 = mm * mm * mm;
    let s3 = ss * ss * ss;
    // LMS' → 线性 sRGB
    let r_lin = 4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3;
    let g_lin = -1.2684380046 * l3 + 2.6097574011 * m3 - 0.3413193965 * s3;
    let b_lin = -0.0041960863 * l3 - 0.7034186147 * m3 + 1.7076147010 * s3;
    // 线性 → sRGB gamma：x^(1/2.4) = x^(5/12) = cbrt(x)·cbrt(√√x)
    let enc = |x: f32| -> u8 {
        let v = if x <= 0.0031308 {
            12.92 * x
        } else {
            1.055 * f_cbrt(x) * f_cbrt(x.sqrt().sqrt()) - 0.055
        };
        let q = (v * 255.0 + 0.5) as i32;
        q.clamp(0, 255) as u8
    };
    let a8 = ((c.a as u32 * 255 + 500) / 1000) as u8;
    (enc(r_lin), enc(g_lin), enc(b_lin), a8)
}

// ===========================================================================
// 9. 4K 缩放引擎（dp = design px × scale，1920p 基准）
// ===========================================================================
/// 显示基准宽度：1920（tokens.css 全部按此口径测量）。
pub const BASE_W: u32 = 1920;
pub const BASE_H: u32 = 1080;

/// 由显示分辨率求缩放（毫单位，1000 = 1x）。4K = 2000（2x）。
/// 以 1920p 为基准取两向最小；低于基准的分辨率不再缩小（下限 1x），
/// 高于基准按比例放大并钳制 3x 上限，避免奇异分辨率爆炸。
pub const fn scale_milli(w: u32, h: u32) -> u32 {
    let sw = if w >= BASE_W { w * 1000 / BASE_W } else { 1000 };
    let sh = if h >= BASE_H { h * 1000 / BASE_H } else { 1000 };
    let s = if sw < sh { sw } else { sh };
    if s > 3000 {
        3000
    } else {
        s
    }
}

/// design px → 物理 px（四舍五入）。dp(1, 2000) = 2，4K 下 4px 基格不糊。
pub const fn dp(px: u32, scale_milli: u32) -> u32 {
    (px * scale_milli + 500) / 1000
}

// ===========================================================================
// 10. CheckSet 自检
// ===========================================================================
pub fn checks(cs: &mut CheckSet) {
    fn g(cs: &mut CheckSet, n: &str, ok: bool) {
        cs.check(n, ok);
    }

    g(cs, "nova4k-tokens-font-scale", FS_32 / FS_12 >= 2 && FS_20 > FS_17);
    g(cs, "nova4k-spacing-8", SP_8 == 64 && SP_1 == 4);
    g(cs, "nova4k-radius", R_WINDOW == 16 && R_CARD == 12 && R_CONTROL == 8 && R_CHIP == 4);
    g(cs, "nova4k-elev-6", ELEV.len() == 6 && ELEV[0].0 == 0 && ELEV[5].0 == 64);
    g(cs, "nova4k-bezier-3", EASE_STANDARD.0 == 200 && EASE_SPRING.1 == 1400);
    g(cs, "nova4k-dur-6", DUR_6 == 320 && dur_scaled(DUR_6, MOTION_SPEED_HALF_MILLI) == 160);
    g(cs, "nova4k-ctl", CTL_MENU_ITEM == 36 && CTL_TOUCH == 44 && CTL_MENU_ITEM_COMPACT == 31);
    g(cs, "nova4k-hover-3", HOVER_DELAY_STD == 400);
    g(cs, "nova4k-icon-tiers", ICON_TIERS == [16, 20, 24]);

    // 三主题语义色逐角色在位（dark/light/hc 同数量、数值不漂移）
    let roles = [
        ColorRole::BgCanvas, ColorRole::BgSurface, ColorRole::BgRaised,
        ColorRole::TextPrimary, ColorRole::TextSecondary, ColorRole::Accent,
        ColorRole::AccentSoft, ColorRole::Success, ColorRole::Warn,
        ColorRole::Danger, ColorRole::Stroke, ColorRole::TooltipBg, ColorRole::TooltipFg,
    ];
    g(cs, "nova4k-roles-13", roles.len() == 13);
    let mut theme_ok = true;
    for r in roles {
        for t in [Theme::Dark, Theme::Light, Theme::HighContrast] {
            let c = color(r, t);
            if c.l > 1000 || c.c > 1000 || c.a > 1000 || c.h > 359_999 {
                theme_ok = false;
            }
        }
    }
    g(cs, "nova4k-theme-range", theme_ok);
    // HC 纯黑画布、dark 画布深色
    g(cs, "nova4k-hc-black", color(ColorRole::BgCanvas, Theme::HighContrast).l == 0);
    g(cs, "nova4k-dark-deep", color(ColorRole::BgCanvas, Theme::Dark).l == 140);
    g(cs, "nova4k-light-paper", color(ColorRole::BgCanvas, Theme::Light).l == 930);

    // OKLCH→sRGB 转换正确性锚点
    let black = oklch_to_rgba8(Oklch::new(0, 0, 0));
    let white = oklch_to_rgba8(Oklch::new(1000, 0, 0));
    g(cs, "nova4k-conv-black", black.0 == 0 && black.1 == 0 && black.2 == 0);
    g(cs, "nova4k-conv-white", white.0 == 255 && white.1 == 255 && white.2 == 255);
    let accent = oklch_to_rgba8(color(ColorRole::Accent, Theme::Dark));
    // 参考实现锚点：oklch(0.68 0.09 262) ≈ rgb(122,152,208)，蓝主导
    g(cs, "nova4k-conv-accent", accent.3 == 255 && accent.0 > 100 && accent.2 > accent.1 && accent.1 > accent.0);
    // alpha 换算
    let half = oklch_to_rgba8(Oklch::new(500, 0, 0).with_alpha(500));
    g(cs, "nova4k-conv-alpha", half.3 == 127 || half.3 == 128);

    // 4K 缩放引擎
    g(cs, "nova4k-scale-1x", scale_milli(1920, 1080) == 1000);
    g(cs, "nova4k-scale-4k", scale_milli(3840, 2160) == 2000);
    g(cs, "nova4k-scale-1440p", scale_milli(2560, 1440) == 1333);
    g(cs, "nova4k-scale-floor", scale_milli(80, 60) == 1000);
    g(cs, "nova4k-dp-4k", dp(4, 2000) == 8 && dp(SP_1, 2000) == 8 && dp(R_WINDOW, 2000) == 32);
    g(cs, "nova4k-dp-round", dp(1, 1500) == 2);
    // W2 扩展与 veil
    g(cs, "nova4k-w2", W2_ROW_H == 40 && W2_CONTROL_H == 32 && W2_LH_MILLI == 1600);
    g(cs, "nova4k-veil-5", PX2_VEIL.len() == 5 && PX2_VEIL[0].a == 0 && PX2_VEIL[4].a == 720);
}

// ===========================================================================
// 11. 单元测试（宿主机 std 下运行）
// ===========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_values_match_tokens_css() {
        assert_eq!(FS_12, 12);
        assert_eq!(SP_8, 64);
        assert_eq!(R_WINDOW, 16);
        assert_eq!(DUR_3, 170);
        assert_eq!(dur_scaled(DUR_1, MOTION_SPEED_ONE5_MILLI), 120);
    }

    #[test]
    fn themes_match_css_layers() {
        // dark 默认画布 oklch(0.14 0.02 262)
        assert_eq!(color(ColorRole::BgCanvas, Theme::Dark), Oklch::new(140, 20, 262_000));
        // light 画布 oklch(0.93 0.02 90)
        assert_eq!(color(ColorRole::BgCanvas, Theme::Light).h, 90_000);
        // hc accent oklch(0.87 0.16 95)
        assert_eq!(
            color(ColorRole::Accent, Theme::HighContrast),
            Oklch::new(870, 160, 95_000)
        );
    }

    #[test]
    fn oklch_conversion_anchors() {
        let (r, g, b, a) = oklch_to_rgba8(Oklch::new(1000, 0, 0));
        assert_eq!((r, g, b, a), (255, 255, 255, 255));
        let (r, g, b, _) = oklch_to_rgba8(Oklch::new(0, 0, 0));
        assert_eq!((r, g, b), (0, 0, 0));
        // OKLab L=0.5 无色度 → LMS=0.125，sRGB 编码后 ≈99（参考实现逐位一致）
        let (r, g, b, _) = oklch_to_rgba8(Oklch::new(500, 0, 0));
        assert!((92..=106).contains(&r) && r == g && g == b, "gray got {r}");
        // 参考实现锚点：oklch(0.68 0.09 262) ≈ rgb(122,152,208)
        let (r, g, b, _) = oklch_to_rgba8(color(ColorRole::Accent, Theme::Dark));
        assert_eq!((r, g, b), (122, 152, 208));
    }

    #[test]
    fn scaling_engine_4k() {
        assert_eq!(scale_milli(3840, 2160), 2000);
        assert_eq!(dp(16, 2000), 32);
        // 紧凑档菜单行高 4K：31dp → 62px
        assert_eq!(dp(CTL_MENU_ITEM_COMPACT, 2000), 62);
    }
}
