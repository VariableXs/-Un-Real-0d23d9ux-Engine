//! 动态壁纸系统（#401~#410，AI-05 域五）。
//!
//! 三层图层架构：壁纸层（视频/WebGL/Canvas/GIF）→ 遮罩层（颜色+透明度+模糊）
//! → UI 层。全部为确定性纯逻辑：壁纸源管理、尺寸适配计算、遮罩合成参数、
//! 预设实时壁纸枚举、命令解析——零 AI。
//! 三端等价：本域只产出纯数据（图层栈/适配矩阵/遮罩参数），
//! 由壳A/壳B/壳C 各自渲染，Windows / Variable / VARIX 行为一致。

use crate::checks::CheckSet;

// ───────────────────────── F401/F402/F403 壁纸源 ─────────────────────────

/// 壁纸源类型：纯色/渐变（F401）、静态图片（F402）、动态视频/GIF（F403）、
/// 创意工坊链接（F404）、WebGL 实时壁纸（F408）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WallpaperKind {
    Solid,
    Gradient,
    Image,
    Video,
    Live,
}

impl WallpaperKind {
    pub fn key(self) -> &'static str {
        match self {
            WallpaperKind::Solid => "solid",
            WallpaperKind::Gradient => "gradient",
            WallpaperKind::Image => "image",
            WallpaperKind::Video => "video",
            WallpaperKind::Live => "live",
        }
    }
}

/// 一张壁纸配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wallpaper {
    pub kind: WallpaperKind,
    /// 内容：纯色=色值 / 渐变=css / 图片视频=路径 / 实时=预设名 / 链接=工坊 URL。
    pub value: String,
}

impl Wallpaper {
    pub fn solid(color: &str) -> Wallpaper {
        Wallpaper { kind: WallpaperKind::Solid, value: color.into() }
    }
    pub fn gradient(css: &str) -> Wallpaper {
        Wallpaper { kind: WallpaperKind::Gradient, value: css.into() }
    }
    pub fn image(path: &str) -> Wallpaper {
        Wallpaper { kind: WallpaperKind::Image, value: path.into() }
    }
    pub fn video(path: &str) -> Wallpaper {
        Wallpaper { kind: WallpaperKind::Video, value: path.into() }
    }
    pub fn live(preset: &str) -> Wallpaper {
        Wallpaper { kind: WallpaperKind::Live, value: preset.into() }
    }
}

/// 支持的文件扩展名白名单（按源类型）。
pub fn ext_allowed(kind: WallpaperKind, ext: &str) -> bool {
    let e = ext.trim_start_matches('.').to_lowercase();
    match kind {
        WallpaperKind::Image => matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp"),
        WallpaperKind::Video => matches!(e.as_str(), "mp4" | "webm" | "gif"),
        _ => false,
    }
}

// ───────────────────────── F404 Wallpaper Engine 创意工坊 ─────────────────────────

/// 创意工坊链接解析：形如 `steam://url/WorkshopFilePage/?id=123456789`，
/// 或裸 id。返回 (workshop_id, 下载占位路径)。
pub fn parse_workshop_url(input: &str) -> Option<(String, String)> {
    let s = input.trim();
    let id = if let Some(p) = s.find("id=") {
        let rest = &s[p + 3..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if num.is_empty() {
            return None;
        }
        num
    } else if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
        s.to_string()
    } else {
        return None;
    };
    Some((id.clone(), format!("workshop/{id}.mp4")))
}

// ───────────────────────── F405 尺寸适配 ─────────────────────────

/// 适配方式：用户显式选择（壁纸太小时用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Cover,   // 裁切铺满（默认）
    Tile,    // 平铺
    Center,  // 居中
    Stretch, // 拉伸
    Blur,    // 模糊放大
}

/// 依据壁纸比例 vs 屏幕比例计算适配决策：
/// 相同=直接铺满；壁纸更宽=左右裁切；壁纸更高=上下裁切；壁纸更小=走用户 FitMode。
pub fn fit_decision(
    wallpaper_w: u32,
    wallpaper_h: u32,
    screen_w: u32,
    screen_h: u32,
    user_mode: FitMode,
) -> (FitMode, &'static str) {
    if screen_w == 0 || screen_h == 0 || wallpaper_w == 0 || wallpaper_h == 0 {
        return (FitMode::Center, "degenerate");
    }
    let wr = wallpaper_w as f64 / wallpaper_h as f64;
    let sr = screen_w as f64 / screen_h as f64;
    let wpix = wallpaper_w as u64 * wallpaper_h as u64;
    let spix = screen_w as u64 * screen_h as u64;
    if wpix < spix {
        return (user_mode, "undersized");
    }
    if (wr - sr).abs() < 0.01 {
        (FitMode::Cover, "same-ratio")
    } else if wr > sr {
        (FitMode::Cover, "crops-sides")
    } else {
        (FitMode::Cover, "crops-top-bottom")
    }
}

// ───────────────────────── F406/F407 遮罩层 ─────────────────────────

/// 遮罩参数：颜色 + 透明度(0~100) + 模糊(0~20px)。
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    pub color: String,
    pub opacity: u32,
    pub blur_px: u32,
}

impl Default for Mask {
    fn default() -> Self {
        Mask { color: "#000000".into(), opacity: 30, blur_px: 0 }
    }
}

impl Mask {
    /// 钳位构造：透明度 0~100、模糊 0~20，越界收敛。
    pub fn clamped(color: &str, opacity: i64, blur: i64) -> Mask {
        Mask {
            color: color.into(),
            opacity: opacity.clamp(0, 100) as u32,
            blur_px: blur.clamp(0, 20) as u32,
        }
    }

    /// 合成为 CSS 片段（UI 层之下）。
    pub fn css(&self) -> String {
        format!(
            "background:{};opacity:{};backdrop-filter:blur({}px)",
            self.color,
            self.opacity as f64 / 100.0,
            self.blur_px
        )
    }
}

/// 区域遮罩（F407）：画布区遮罩重（默认 70%），面板区遮罩轻（默认 30%）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionMasks {
    pub canvas_opacity: u32,
    pub panel_opacity: u32,
}

impl Default for RegionMasks {
    fn default() -> Self {
        RegionMasks { canvas_opacity: 70, panel_opacity: 30 }
    }
}

impl RegionMasks {
    /// 覆盖设置（钳位 0~100）。
    pub fn set(&mut self, canvas: i64, panel: i64) {
        self.canvas_opacity = canvas.clamp(0, 100) as u32;
        self.panel_opacity = panel.clamp(0, 100) as u32;
    }

    /// 某屏幕坐标命中的遮罩透明度（画布区在中间大块，其余算面板区）。
    pub fn opacity_at(&self, x: u32, y: u32, screen_w: u32, screen_h: u32) -> u32 {
        let in_canvas = x >= screen_w / 6
            && x <= screen_w * 5 / 6
            && y >= screen_h / 6
            && y <= screen_h * 5 / 6;
        if in_canvas {
            self.canvas_opacity
        } else {
            self.panel_opacity
        }
    }
}

// ───────────────────────── F408 内置实时壁纸 ─────────────────────────

/// 八种 WebGL 实时预设。
pub const LIVE_WALLPAPERS: [&str; 8] = [
    "starfield", // 星空
    "aurora",    // 极光
    "deepsea",   // 深海
    "fire",      // 火焰
    "matrix",    // 矩阵
    "fluid",     // 流体
    "nebula",    // 星云
    "rain",      // 雨天
];

pub fn is_live_preset(name: &str) -> bool {
    LIVE_WALLPAPERS.contains(&name)
}

// ───────────────────────── F409 图层叠加 ─────────────────────────

/// 三层图层栈（z 顺序）：壁纸(0) → 遮罩(1) → UI(2)。
#[derive(Debug, Clone, PartialEq)]
pub struct LayerStack {
    pub wallpaper: Wallpaper,
    pub mask: Mask,
    /// UI 层是否在最上（恒真；留位以显式表达互不干扰）。
    pub ui_on_top: bool,
}

impl LayerStack {
    pub fn new(wallpaper: Wallpaper, mask: Mask) -> LayerStack {
        LayerStack { wallpaper, mask, ui_on_top: true }
    }

    /// z-index 表：壁纸 0 / 遮罩 1 / UI 2。
    pub fn z_order(&self) -> [u32; 3] {
        [0, 1, 2]
    }

    /// 互换壁纸不改变遮罩与 UI（互不干扰验证）。
    pub fn swap_wallpaper(&mut self, w: Wallpaper) {
        self.wallpaper = w;
        self.ui_on_top = true;
    }
}

// ───────────────────────── F410 /wallpaper 命令 ─────────────────────────

/// /wallpaper 命令解析结果。
#[derive(Debug, Clone, PartialEq)]
pub enum WallCmd {
    Set(Wallpaper),
    Overlay { opacity: u32 },
    Blur { px: u32 },
    Clear,
}

/// 解析 `/wallpaper <路径>` / `/wallpaper overlay 50` / `/wallpaper blur 10` / `/wallpaper clear`。
/// 按扩展名自动判定图片/视频源；`solid:#xxx` / `live:xxx` 亦可。
pub fn parse_wall_cmd(args: &str) -> Option<WallCmd> {
    let args = args.trim();
    if args.is_empty() {
        return None;
    }
    let mut it = args.split_whitespace();
    let head = it.next()?;
    let rest = it.next();
    match head {
        "overlay" => {
            let v = rest?.parse::<i64>().ok()?;
            Some(WallCmd::Overlay { opacity: v.clamp(0, 100) as u32 })
        }
        "blur" => {
            let v = rest?.parse::<i64>().ok()?;
            Some(WallCmd::Blur { px: v.clamp(0, 20) as u32 })
        }
        "clear" => Some(WallCmd::Clear),
        path => {
            let wp = if let Some(c) = path.strip_prefix("solid:") {
                Wallpaper::solid(c)
            } else if let Some(n) = path.strip_prefix("live:") {
                if !is_live_preset(n) {
                    return None;
                }
                Wallpaper::live(n)
            } else {
                let ext = path.rsplit('.').next()?;
                if ext_allowed(WallpaperKind::Video, ext) {
                    Wallpaper::video(path)
                } else if ext_allowed(WallpaperKind::Image, ext) {
                    Wallpaper::image(path)
                } else {
                    return None;
                }
            };
            Some(WallCmd::Set(wp))
        }
    }
}

/// 把命令应用到图层栈（UI 层不变）。
pub fn apply_wall_cmd(stack: &mut LayerStack, cmd: &WallCmd) -> bool {
    match cmd {
        WallCmd::Set(w) => {
            stack.swap_wallpaper(w.clone());
            true
        }
        WallCmd::Overlay { opacity } => {
            stack.mask.opacity = *opacity;
            true
        }
        WallCmd::Blur { px } => {
            stack.mask.blur_px = *px;
            true
        }
        WallCmd::Clear => {
            stack.wallpaper = Wallpaper::solid("#0A0A0F");
            stack.mask = Mask::default();
            true
        }
    }
}

// ───────────────────────── 域自检 ─────────────────────────

/// #401~#410 自检（10 项）。
pub fn run_wallpaper_checks() -> CheckSet {
    let mut s = CheckSet::new("wallpaper");

    // F401 纯色/渐变
    let solid = Wallpaper::solid("#101010");
    let grad = Wallpaper::gradient("linear-gradient(#050510,#0D0221)");
    s.add(
        "F401 纯色/渐变",
        solid.kind == WallpaperKind::Solid && grad.kind == WallpaperKind::Gradient,
        "设置→外观→背景→颜色选择器",
    );

    // F402 静态图片
    s.add(
        "F402 静态图片",
        ext_allowed(WallpaperKind::Image, "png")
            && ext_allowed(WallpaperKind::Image, ".jpg")
            && ext_allowed(WallpaperKind::Image, "WEBP")
            && !ext_allowed(WallpaperKind::Image, "mp4"),
        "PNG/JPG/WebP 白名单",
    );

    // F403 动态壁纸上传
    let vid = Wallpaper::video("media/star.mp4");
    s.add(
        "F403 动态壁纸上传",
        vid.kind == WallpaperKind::Video
            && ext_allowed(WallpaperKind::Video, "mp4")
            && ext_allowed(WallpaperKind::Video, "webm")
            && ext_allowed(WallpaperKind::Video, "gif")
            && !ext_allowed(WallpaperKind::Video, "png"),
        "MP4/WebM/GIF",
    );

    // F404 Wallpaper Engine 创意工坊
    let w1 = parse_workshop_url("steam://url/WorkshopFilePage/?id=3210123456");
    let w2 = parse_workshop_url("777");
    let w3 = parse_workshop_url("not-a-link");
    s.add(
        "F404 Wallpaper Engine",
        w1.as_ref().map(|(i, _)| i.as_str()) == Some("3210123456")
            && w1.as_ref().map(|(_, p)| p.as_str()) == Some("workshop/3210123456.mp4")
            && w2.as_ref().map(|(i, _)| i.as_str()) == Some("777")
            && w3.is_none(),
        "链接/裸 id→自动下载占位",
    );

    // F405 尺寸适配
    let (m1, r1) = fit_decision(1920, 1080, 1920, 1080, FitMode::Tile);
    let (m2, r2) = fit_decision(3840, 1080, 1920, 1080, FitMode::Tile);
    let (m3, r3) = fit_decision(1080, 1920, 1920, 1080, FitMode::Tile);
    let (m4, r4) = fit_decision(800, 600, 1920, 1080, FitMode::Blur);
    s.add(
        "F405 尺寸适配",
        m1 == FitMode::Cover && r1 == "same-ratio"
            && m2 == FitMode::Cover && r2 == "crops-sides"
            && m3 == FitMode::Cover && r3 == "crops-top-bottom"
            && m4 == FitMode::Blur && r4 == "undersized",
        "同比例铺满/宽裁侧/高裁上下/小图走用户选择",
    );

    // F406 遮罩控制
    let mk = Mask::clamped("#000000", 150, 25);
    let mk2 = Mask::clamped("#000000", 50, 10);
    s.add(
        "F406 遮罩控制",
        mk.opacity == 100 && mk.blur_px == 20
            && mk2.opacity == 50 && mk2.blur_px == 10
            && mk2.css().contains("opacity:0.5") && mk2.css().contains("blur(10px)"),
        "透明度 0-100/模糊 0-20 钳位",
    );

    // F407 区域遮罩
    let mut rm = RegionMasks::default();
    let canvas_mid = rm.opacity_at(960, 540, 1920, 1080);
    let panel_edge = rm.opacity_at(10, 10, 1920, 1080);
    rm.set(60, 40);
    let after = rm.opacity_at(960, 540, 1920, 1080);
    rm.set(200, -1);
    s.add(
        "F407 区域遮罩",
        canvas_mid == 70 && panel_edge == 30 && after == 60
            && rm.canvas_opacity == 100 && rm.panel_opacity == 0,
        "画布 70%/面板 30%+可调+钳位",
    );

    // F408 内置实时壁纸
    let all_named = LIVE_WALLPAPERS.iter().all(|n| is_live_preset(n));
    s.add(
        "F408 内置实时壁纸",
        LIVE_WALLPAPERS.len() == 8 && all_named && !is_live_preset("nope"),
        "星空/极光/深海/火焰/矩阵/流体/星云/雨天",
    );

    // F409 图层叠加
    let mut ls = LayerStack::new(Wallpaper::image("bg.png"), Mask::default());
    let z = ls.z_order();
    let mask_before = ls.mask.clone();
    ls.swap_wallpaper(Wallpaper::live("aurora"));
    s.add(
        "F409 图层叠加",
        z == [0, 1, 2] && ls.ui_on_top && ls.wallpaper.value == "aurora" && ls.mask == mask_before,
        "壁纸→遮罩→UI 互不干扰",
    );

    // F410 命令控制
    let c1 = parse_wall_cmd("star.mp4");
    let c2 = parse_wall_cmd("bg.png");
    let c3 = parse_wall_cmd("live:matrix");
    let c4 = parse_wall_cmd("overlay 50");
    let c5 = parse_wall_cmd("blur 10");
    let c6 = parse_wall_cmd("overlay 999");
    let c7 = parse_wall_cmd("clear");
    let mut stack = LayerStack::new(Wallpaper::solid("#111"), Mask::default());
    let ok = c1.as_ref().map(|c| c.clone()) == Some(WallCmd::Set(Wallpaper::video("star.mp4")))
        && matches!(c2, Some(WallCmd::Set(Wallpaper { kind: WallpaperKind::Image, .. })))
        && c3.is_some()
        && c4 == Some(WallCmd::Overlay { opacity: 50 })
        && c5 == Some(WallCmd::Blur { px: 10 })
        && c6 == Some(WallCmd::Overlay { opacity: 100 })
        && c7 == Some(WallCmd::Clear)
        && parse_wall_cmd("").is_none()
        && parse_wall_cmd("evil.exe").is_none()
        && apply_wall_cmd(&mut stack, &WallCmd::Set(Wallpaper::live("aurora")))
        && apply_wall_cmd(&mut stack, &WallCmd::Overlay { opacity: 80 })
        && stack.wallpaper.value == "aurora"
        && stack.mask.opacity == 80;
    s.add("F410 命令控制", ok, "/wallpaper <路径>|overlay|blur|clear");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f404_workshop_edge() {
        assert!(parse_workshop_url("steam://x/?id=").is_none());
        assert!(parse_workshop_url("").is_none());
        assert!(parse_workshop_url("id=12abc").map(|(i, _)| i) == Some("12".into()));
    }

    #[test]
    fn f405_fit_degenerate() {
        assert_eq!(fit_decision(0, 0, 1920, 1080, FitMode::Tile).1, "degenerate");
        assert_eq!(fit_decision(1920, 1080, 0, 0, FitMode::Tile).1, "degenerate");
    }

    #[test]
    fn f407_region_layout() {
        let rm = RegionMasks::default();
        assert_eq!(rm.opacity_at(320, 180, 1920, 1080), 70);
        assert_eq!(rm.opacity_at(1600, 900, 1920, 1080), 70);
        assert_eq!(rm.opacity_at(100, 540, 1920, 1080), 30);
        assert_eq!(rm.opacity_at(960, 50, 1920, 1080), 30);
    }

    #[test]
    fn f410_cmd_rejects_bad() {
        assert!(parse_wall_cmd("live:notapreset").is_none());
        assert!(parse_wall_cmd("overlay abc").is_none());
        assert!(parse_wall_cmd("blur").is_none());
        assert!(parse_wall_cmd("evil.exe").is_none());
        let solid = parse_wall_cmd("solid:#0A0A0F");
        assert!(matches!(
            solid,
            Some(WallCmd::Set(Wallpaper { kind: WallpaperKind::Solid, .. }))
        ));
    }

    #[test]
    fn f410_clear_resets() {
        let mut stack =
            LayerStack::new(Wallpaper::video("x.mp4"), Mask::clamped("#000", 90, 15));
        assert!(apply_wall_cmd(&mut stack, &WallCmd::Clear));
        assert_eq!(stack.wallpaper, Wallpaper::solid("#0A0A0F"));
        assert_eq!(stack.mask, Mask::default());
    }
}
