//! 深化层三 · F143 星徽与品牌资产包（2026-09-26 深化批次三）。
//!
//! 补深资产管理工程面（主册 G-D-18）：资产命名规范校验器（kebab +
//! 尺寸后缀 + 扩展名白名单）、颜色解析与明暗变体派生（#RRGGBB →
//! 亮度 → 提亮/压暗 20%）、壁纸分辨率矩阵（4K 原生硬线）、使用场景
//! 矩阵查询（允许/禁止白名单裁决）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 资产命名规范：name[-<size>].<ext>，kebab 主体，扩展名白名单
// ---------------------------------------------------------------------------

pub const LOGO_EXTS: [&str; 3] = ["svg", "png", "webp"];
pub const LOGO_SIZES: [u32; 5] = [16, 32, 48, 128, 512];

/// 校验返回 Err(原因)；Ok(解析出的尺寸，无后缀为 None)。
pub fn check_asset_name(name: &str) -> Result<Option<u32>, &'static str> {
    let dot = name.rfind('.').ok_or("缺扩展名")?;
    let (stem, ext) = (&name[..dot], &name[dot + 1..]);
    if !LOGO_EXTS.contains(&ext) {
        return Err("扩展名不在白名单");
    }
    if stem.is_empty() || stem.starts_with('-') || stem.ends_with('-') {
        return Err("主体空或边界中划线");
    }
    // 主体 kebab：小写/数字/中划线。
    if !stem.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return Err("主体含非法字符：仅小写/数字/中划线");
    }
    // 尺寸后缀（可选）：-<digits> 必须在尺寸阶梯内。
    if let Some(dash) = stem.rfind('-') {
        let tail = &stem[dash + 1..];
        if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
            let size: u32 = tail.parse().map_err(|_| "尺寸后缀越界")?;
            if !LOGO_SIZES.contains(&size) {
                return Err("尺寸不在五档阶梯");
            }
            return Ok(Some(size));
        }
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// 颜色解析与明暗变体：#RRGGBB → 亮度 → 提亮/压暗 20%
// ---------------------------------------------------------------------------

pub fn parse_hex(s: &str) -> Result<[u8; 3], &'static str> {
    let Some(hex) = s.strip_prefix('#') else {
        return Err("缺 # 前缀");
    };
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("非 RRGGBB 形态");
    }
    let byte = |i: usize| -> u8 {
        let hi = (hex.as_bytes()[i] as char).to_digit(16).unwrap_or(0) as u8;
        let lo = (hex.as_bytes()[i + 1] as char).to_digit(16).unwrap_or(0) as u8;
        hi * 16 + lo
    };
    Ok([byte(0), byte(2), byte(4)])
}

/// 感知亮度（简易 Rec.601 整数口径——品牌检查够用）。
pub fn perceived_luma(rgb: [u8; 3]) -> u32 {
    (rgb[0] as u32 * 299 + rgb[1] as u32 * 587 + rgb[2] as u32 * 114) / 1000
}

/// 派生变体：亮色素材在深底用（提亮 20%，向 255 收敛）；暗色素材压暗 20%。
pub fn lighten(rgb: [u8; 3]) -> [u8; 3] {
    fn mix(c: u8) -> u8 {
        let v = c as u32 + (255 - c as u32) * 20 / 100;
        v.min(255) as u8
    }
    [mix(rgb[0]), mix(rgb[1]), mix(rgb[2])]
}

pub fn darken(rgb: [u8; 3]) -> [u8; 3] {
    fn mix(c: u8) -> u8 {
        (c as u32 * 80 / 100) as u8
    }
    [mix(rgb[0]), mix(rgb[1]), mix(rgb[2])]
}

/// 亮度一致性：品牌色在明暗两套主题下的变体亮度方向必须相反
/// （亮变体更亮、暗变体更暗——变体派生的自证）。
pub fn variant_direction_ok(base: [u8; 3]) -> bool {
    let l0 = perceived_luma(base);
    let l_up = perceived_luma(lighten(base));
    let l_dn = perceived_luma(darken(base));
    l_up > l0 && l_dn < l0
}

// ---------------------------------------------------------------------------
// 壁纸分辨率矩阵：4K 原生硬线（3840×2160 必须在列）
// ---------------------------------------------------------------------------

pub fn wallpaper_4k_native(resolutions: &[(u32, u32)]) -> bool {
    resolutions.contains(&(3840, 2160))
}

/// 全部条目宽高比均在 1.2~2.5 之间（防奇形资产混入品牌包）。
pub fn wallpaper_aspect_ok(resolutions: &[(u32, u32)]) -> bool {
    resolutions.iter().all(|(w, h)| {
        *w > 0 && *h > 0 && {
            let r = (*w as u64 * 100) / (*h as u64);
            (120..=250).contains(&r)
        }
    })
}

// ---------------------------------------------------------------------------
// 使用场景矩阵：白名单裁决（未列入即禁止——条款清单式同构）
// ---------------------------------------------------------------------------

/// 场景位图：bit0 产品界面 / bit1 官网 / bit2 社区活动 / bit3 商品。
pub const SCENE_PRODUCT: u8 = 1;
pub const SCENE_WEBSITE: u8 = 2;
pub const SCENE_EVENT: u8 = 4;
pub const SCENE_MERCH: u8 = 8;

/// 资产的允许场景位图 → 查询裁决。
pub fn scene_allowed(granted_scenes: u8, query: u8) -> bool {
    query != 0 && granted_scenes & query == query
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F143F_TAG: &str = "stareco-F143-deep3";

pub fn run_f143_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F143F_TAG);

    // 命名
    set.add("f143f name plain", check_asset_name("logo.svg") == Ok(None), "无尺寸后缀通过");
    set.add("f143f name size", check_asset_name("logo-512.png") == Ok(Some(512)), "五档尺寸通过");
    set.add("f143f name bad size", check_asset_name("logo-100.png").is_err(), "非档尺寸拒绝");
    set.add("f143f name ext", check_asset_name("logo.gif").is_err(), "扩展名拒绝");
    set.add("f143f name case", check_asset_name("Logo.svg").is_err(), "大写拒绝");
    set.add("f143f name edge dash", check_asset_name("-logo.svg").is_err(), "边界中划线拒绝");
    set.add("f143f name noext", check_asset_name("logo").is_err(), "缺扩展名拒绝");

    // 颜色
    let brand = parse_hex("#3A7BD5").expect("ok");
    set.add("f143f hex ok", brand == [0x3A, 0x7B, 0xD5], "解析对拍");
    set.add("f143f hex short", parse_hex("#3A7BD").is_err(), "缺位拒绝");
    set.add("f143f hex nopfx", parse_hex("3A7BD5").is_err(), "缺 # 拒绝");
    set.add("f143f hex nonhex", parse_hex("#GG7BD5").is_err(), "非十六进制拒绝");
    set.add("f143f variant direction", variant_direction_ok(brand), "明暗变体方向自证");
    set.add("f143f luma white", perceived_luma([255, 255, 255]) == 255, "白 255");

    // 壁纸矩阵
    let walls = [(3840u32, 2160u32), (1920, 1080)];
    set.add("f143f 4k native", wallpaper_4k_native(&walls), "4K 原生在列");
    set.add("f143f 4k missing", !wallpaper_4k_native(&[(1920, 1080)]), "缺 4K 判红");
    set.add("f143f aspect ok", wallpaper_aspect_ok(&walls), "宽高比在带");
    set.add("f143f aspect odd", !wallpaper_aspect_ok(&[(1000, 100)]), "超宽比拒绝");
    set.add("f143f aspect zero", !wallpaper_aspect_ok(&[(0, 100)]), "零宽拒绝");

    // 场景矩阵
    let granted = SCENE_PRODUCT | SCENE_WEBSITE;
    set.add("f143f scene ok", scene_allowed(granted, SCENE_PRODUCT), "白名单内允许");
    set.add("f143f scene combo", scene_allowed(granted, SCENE_PRODUCT | SCENE_WEBSITE), "组合场景全许");
    set.add("f143f scene deny", !scene_allowed(granted, SCENE_MERCH), "未列入即禁止");
    set.add("f143f scene zero", !scene_allowed(granted, 0), "零查询拒绝");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn name_size_boundary() {
        // 全五档尺寸逐档通过。
        for s in LOGO_SIZES.iter() {
            let mut n = alloc::string::String::from("mark-");
            n.push_str(itoa_ascii(*s));
            n.push_str(".png");
            assert_eq!(check_asset_name(&n), Ok(Some(*s)));
        }
    }

    fn itoa_ascii(v: u32) -> &'static str {
        match v {
            16 => "16",
            32 => "32",
            48 => "48",
            128 => "128",
            512 => "512",
            _ => "",
        }
    }

    #[test]
    fn lighten_darken_roundtrip() {
        let c = [100u8, 150, 200];
        let up = lighten(c);
        let dn = darken(c);
        assert!(up[0] > c[0] && dn[0] < c[0]);
        // 白色提亮不动、压暗变灰。
        assert_eq!(lighten([255, 255, 255]), [255, 255, 255]);
        assert_eq!(darken([255, 255, 255]), [204, 204, 204]);
    }
}
