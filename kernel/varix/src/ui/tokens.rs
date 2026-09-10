//! TRINITY-500 · AI-08 动效与视觉令牌域（F180~F189，W2）
//!
//! 与 Tauri 版 `tokens.css` 同源：材质、主题跟随、色温迁移、裸色值门禁、
//! 128px 图标管线、水印与 Logo 层。动效部分见 [`crate::ui::motion`]。

use crate::gfx::surface::Oklch;

// ---------------------------------------------------------------------------
// F180 玻璃/亚克力/金属/织物材质
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Glass,
    Acrylic,
    Metal,
    Fabric,
}

impl Material {
    pub fn name(self) -> &'static str {
        match self {
            Material::Glass => "glass",
            Material::Acrylic => "acrylic",
            Material::Metal => "metal",
            Material::Fabric => "fabric",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MaterialParams {
    /// 背景模糊半径（px）；0 = 不模糊。
    pub blur_radius: u32,
    /// 底色叠加强度（permille）。
    pub tint_permille: u16,
    /// 高光强度（permille）。
    pub specular_permille: u16,
    /// 表面粗糙度（permille）——影响颗粒强度。
    pub roughness_permille: u16,
    /// 该材质需要的合成遍数（性能预算用）。
    pub passes: u8,
}

impl Material {
    pub fn params(self) -> MaterialParams {
        match self {
            Material::Glass => MaterialParams { blur_radius: 24, tint_permille: 90, specular_permille: 160, roughness_permille: 30, passes: 2 },
            Material::Acrylic => MaterialParams { blur_radius: 16, tint_permille: 140, specular_permille: 120, roughness_permille: 60, passes: 2 },
            Material::Metal => MaterialParams { blur_radius: 0, tint_permille: 40, specular_permille: 320, roughness_permille: 15, passes: 1 },
            Material::Fabric => MaterialParams { blur_radius: 0, tint_permille: 60, specular_permille: 20, roughness_permille: 260, passes: 1 },
        }
    }

    /// 无 GPU 时金属/织物完全不受影响（不模糊）；玻璃/亚克力必须降级。
    pub fn degrades_without_gpu(self) -> bool {
        matches!(self, Material::Glass | Material::Acrylic)
    }
}

// ---------------------------------------------------------------------------
// F181 主题跟随壁纸
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

/// 壁纸平均亮度（permille）→ 明暗主题。阈值 500‰，迟滞 ±40‰ 防抖。
pub fn theme_from_wallpaper(luma_permille: u16, current: Option<ThemeMode>) -> ThemeMode {
    match current {
        Some(ThemeMode::Dark) if luma_permille < 540 => ThemeMode::Dark,
        Some(ThemeMode::Light) if luma_permille >= 460 => ThemeMode::Light,
        _ => {
            if luma_permille >= 500 {
                ThemeMode::Light
            } else {
                ThemeMode::Dark
            }
        }
    }
}

/// 从壁纸主色提取强调色：保持原色相，统一到 Variable 的彩度/亮度带。
pub fn accent_from_wallpaper(hue_deg: f32, theme: ThemeMode) -> u32 {
    let (l, c) = match theme {
        ThemeMode::Dark => (0.72f32, 0.14f32),
        ThemeMode::Light => (0.58f32, 0.16f32),
    };
    crate::gfx::surface::oklch_to_argb(Oklch { l, c, h: hue_deg })
}

// ---------------------------------------------------------------------------
// F182 昼夜色温迁移
// ---------------------------------------------------------------------------

pub const KELVIN_MIN: u32 = 2700;
pub const KELVIN_MAX: u32 = 6500;

/// 一天 24 小时的色温曲线：正午最冷（6500K），午夜最暖（2700K）。
pub fn kelvin_at_hour(hour: u32) -> u32 {
    let h = hour % 24;
    // 距正午的小时距离（绕环取近路）：0 → 6500K，12 → 2700K
    let raw = if h > 12 { h - 12 } else { 12 - h };
    let d = if raw > 12 { 24 - raw } else { raw };
    KELVIN_MAX - ((KELVIN_MAX - KELVIN_MIN) * d) / 12
}

/// 色温 → 暖色叠加（0x00RRGGBB）与强度（permille）。
pub fn warmth_tint(kelvin: u32) -> (u32, u16) {
    let k = kelvin.clamp(KELVIN_MIN, KELVIN_MAX);
    let warm = ((KELVIN_MAX - k) * 1000) / (KELVIN_MAX - KELVIN_MIN);
    if warm == 0 {
        return (0x0000_0000, 0);
    }
    // 暖 = 提红压蓝，强度随色温线性。
    let r = 255u32;
    let b = 255 - (255 * warm as u32) / 2000;
    ((r << 16) | b, ((warm / 10).min(120)) as u16)
}

// ---------------------------------------------------------------------------
// F186 视觉回归令牌 — 裸色值门禁（内核对齐 audit）
// ---------------------------------------------------------------------------

/// CSS/TS 侧：出现 `#rrggbb` 且不是 `var(--token)` 的，算一次违规。
pub fn css_bare_colors(lines: &[&str]) -> usize {
    let mut count = 0usize;
    for line in lines.iter() {
        let b = line.as_bytes();
        let mut i = 0usize;
        while i + 6 < b.len() {
            if b[i] == b'#' && is_hex6(&b[i + 1..i + 7]) {
                // 排除已经在 var() 声明里的令牌定义（令牌集中定义不算违规）
                if !line.trim_start().starts_with("--") {
                    count += 1;
                }
                i += 7;
            } else {
                i += 1;
            }
        }
    }
    count
}

fn is_hex6(s: &[u8]) -> bool {
    s.len() >= 6 && s[..6].iter().all(|c| c.is_ascii_hexdigit())
}

/// Rust 侧：不在令牌白名单里的 `0xRRGGBB` 字面量算违规。
pub fn rust_bare_colors(tokens: &[u32], src: &str) -> usize {
    let b = src.as_bytes();
    let mut count = 0usize;
    let mut i = 0usize;
    while i + 7 < b.len() {
        if b[i] == b'0' && (b[i + 1] == b'x' || b[i + 1] == b'X') && is_hex6(&b[i + 2..i + 8]) {
            let v = parse_hex6(&b[i + 2..i + 8]);
            if !tokens.contains(&v) {
                count += 1;
            }
            i += 8;
        } else {
            i += 1;
        }
    }
    count
}

fn parse_hex6(s: &[u8]) -> u32 {
    let mut v = 0u32;
    for &c in s[..6].iter() {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => 0,
        };
        v = (v << 4) | d as u32;
    }
    v
}

// ---------------------------------------------------------------------------
// F187 128px 图标管线平移
// ---------------------------------------------------------------------------

pub const ICON_MASTER: u32 = 128;
pub const ICON_SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconStage {
    pub size: u32,
    /// 该档是否由母版直接降采样（256 是放大，需单独标注）。
    pub upscale: bool,
}

/// 图标管线：母版 128px，其余档全部由母版派生；256 档为放大，如实标注。
pub fn icon_pipeline(out: &mut [IconStage]) -> usize {
    let mut n = 0usize;
    for s in ICON_SIZES.iter() {
        if n >= out.len() {
            break;
        }
        out[n] = IconStage { size: *s, upscale: *s > ICON_MASTER };
        n += 1;
    }
    n
}

/// 图标契约：请求尺寸必须是整数倍或母版本身，否则拒绝（禁止非整数缩放）。
pub fn icon_contract_ok(requested: u32, master: u32) -> bool {
    if requested == master {
        return true;
    }
    if requested == 0 || master == 0 {
        return false;
    }
    if requested < master {
        master % requested == 0
    } else {
        requested % master == 0
    }
}

// ---------------------------------------------------------------------------
// F188 水印与品牌标识
// F189 Logo 层
// ---------------------------------------------------------------------------

/// 水印贴在右下角，尺寸 = 画布短边的 6%（下限 24px）。
pub fn watermark_rect(canvas: (i32, i32), margin: i32) -> (i32, i32, i32, i32) {
    let (w, h) = canvas;
    let size = ((if w < h { w } else { h }) * 6 / 100).max(24);
    (w - margin - size, h - margin - size, size, size)
}

/// Logo 层居中，尺寸 = 画布短边 × permille。
pub fn logo_rect(canvas: (i32, i32), scale_permille: u32) -> (i32, i32, i32, i32) {
    let (w, h) = canvas;
    let size = ((if w < h { w } else { h }) as i64 * scale_permille as i64 / 1000) as i32;
    ((w - size) / 2, (h - size) / 2, size, size)
}

/// 水印与 Logo 不得重叠（品牌纪律）：水印在右下、Logo 居中。
pub fn brand_layers_disjoint(canvas: (i32, i32), margin: i32, scale_permille: u32) -> bool {
    let (wx, wy, ww, wh) = watermark_rect(canvas, margin);
    let (lx, ly, ls, _) = logo_rect(canvas, scale_permille);
    let (lx2, ly2) = (lx + ls, ly + ls);
    wx + ww <= lx || lx2 <= wx || wy + wh <= ly || ly2 <= wy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f180_material_params() {
        assert!(Material::Glass.params().blur_radius > Material::Metal.params().blur_radius);
        assert!(Material::Glass.degrades_without_gpu());
        assert!(!Material::Metal.degrades_without_gpu());
        assert_eq!(Material::Fabric.params().roughness_permille, 260);
    }

    #[test]
    fn f181_theme_hysteresis() {
        assert_eq!(theme_from_wallpaper(600, None), ThemeMode::Light);
        assert_eq!(theme_from_wallpaper(400, None), ThemeMode::Dark);
        // 迟滞：暗主题下 520 仍保持暗
        assert_eq!(theme_from_wallpaper(520, Some(ThemeMode::Dark)), ThemeMode::Dark);
        assert_eq!(theme_from_wallpaper(520, Some(ThemeMode::Light)), ThemeMode::Light);
        assert_ne!(accent_from_wallpaper(30.0, ThemeMode::Dark), accent_from_wallpaper(30.0, ThemeMode::Light));
    }

    #[test]
    fn f182_kelvin_curve() {
        assert_eq!(kelvin_at_hour(12), KELVIN_MAX);
        assert_eq!(kelvin_at_hour(0), KELVIN_MIN);
        assert!(kelvin_at_hour(6) > kelvin_at_hour(0));
        assert!(kelvin_at_hour(18) > kelvin_at_hour(23));
        let (tint, strength) = warmth_tint(KELVIN_MIN);
        assert!(strength > 0 && tint & 0xFF < 255);
        assert_eq!(warmth_tint(KELVIN_MAX).1, 0);
    }

    #[test]
    fn f186_bare_color_gate() {
        let css = ["  color: #ff0000;", "  color: var(--fg);", "  --brand: #112233;"];
        assert_eq!(css_bare_colors(&css), 1, "token definitions are allowed");
        let src = "let a = 0x00FF00; let b = 0x112233; let c = 0x00FF00;";
        assert_eq!(rust_bare_colors(&[0x00FF00], src), 1, "whitelisted tokens are allowed");
    }

    #[test]
    fn f187_icon_pipeline() {
        let mut out = [IconStage { size: 0, upscale: false }; 8];
        assert_eq!(icon_pipeline(&mut out), ICON_SIZES.len());
        assert!(out[6].upscale, "256 is upscaled from the 128 master");
        assert!(!out[0].upscale);
        assert!(icon_contract_ok(64, 128));
        assert!(!icon_contract_ok(48, 128));
        assert!(icon_contract_ok(256, 128));
    }

    #[test]
    fn f188_brand_layers() {
        let (x, y, w, h) = watermark_rect((1920, 1080), 24);
        assert!(x > 1920 / 2 && y > 1080 / 2 && w == h);
        let (lx, ly, ls, _) = logo_rect((1920, 1080), 200);
        assert_eq!(ls, 216);
        assert!(lx + ls > 1920 / 2 - 200 && ly < 1080 / 2);
        assert!(brand_layers_disjoint((1920, 1080), 24, 200));
    }
}
