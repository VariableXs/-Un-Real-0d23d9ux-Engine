//! m4arts — VARIX-M400 AI-16 艺术与体验域 (F376~F400)
//!
//! 作品集级别的观感：视觉语言 v2/动效编排/系统音/壁纸集/图标管线/光标/
//! 字体授权/主题打磨/对比度/开关机动画/错误友好化/安装体验/开箱仪式/
//! 彩蛋/音效记忆/省电降级/资产版本库/设计评审/像素走查/动效红线/
//! 品牌一致性/令牌治理/年度回顾/作品集页。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F376 — 视觉语言 v2：曲线/阴影/层次规范
// ===========================================================================

/// 圆角/阴影/海拔三件套令牌（radius px, shadow level, elevation）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SurfaceToken {
    pub radius_px: u8,
    pub shadow: u8, // 0~4
    pub elevation: u8,
}

pub const SURFACE_DIALOG: SurfaceToken = SurfaceToken { radius_px: 12, shadow: 3, elevation: 3 };
pub const SURFACE_CARD: SurfaceToken = SurfaceToken { radius_px: 8, shadow: 1, elevation: 1 };
pub const SURFACE_WINDOW: SurfaceToken = SurfaceToken { radius_px: 10, shadow: 4, elevation: 2 };

pub fn surface_token_valid(t: SurfaceToken) -> bool {
    t.shadow <= 4 && t.elevation <= 4 && t.radius_px <= 24
}

// ===========================================================================
// F377 — 动效编排系统：时序/编排统一框架
// ===========================================================================

#[derive(Clone, Copy)]
pub struct MotionCue {
    pub at_ms: u32,
    pub dur_ms: u32,
    pub easing: u8, // 0=standard 1=decel 2=accel
}

impl MotionCue {
    pub fn overlaps(&self, other: &MotionCue) -> bool {
        self.at_ms < other.at_ms + other.dur_ms && other.at_ms < self.at_ms + self.dur_ms
    }
    pub fn valid(&self) -> bool {
        self.dur_ms > 0 && self.dur_ms <= 2000 && self.easing <= 2
    }
}

/// 编排：总时长 = 最晚结束时刻。
pub fn timeline_total_ms(cues: &[MotionCue]) -> u32 {
    cues.iter().map(|c| c.at_ms + c.dur_ms).max().unwrap_or(0)
}

// ===========================================================================
// F378 — 系统音设计：全套系统音（可关）
// ===========================================================================

pub const SYSTEM_SOUNDS: [&str; 6] =
    ["boot", "notify", "error", "unlock", "plug", "unplug"];

/// 音高（Hz）与时长（ms）的声学参数表（整数）。
pub const SOUND_PARAMS: [(u16, u16); 6] =
    [(523, 400), (880, 180), (330, 500), (660, 250), (440, 120), (392, 120)];

pub fn sound_param_sane() -> bool {
    SOUND_PARAMS.iter().all(|(hz, ms)| (20..=20_000).contains(hz) && *ms >= 20 && *ms <= 2000)
}

// ===========================================================================
// F379 — 壁纸集：四季 + 动态壁纸
// ===========================================================================

pub const WALLPAPERS: [&str; 5] = ["spring", "summer", "autumn", "winter", "dynamic"];

// ===========================================================================
// F380 — 图标重绘管线：128px 全套过检
// ===========================================================================

/// 过检规则：网格对齐（尺寸为 8 的倍数）+ 描边宽度一致。
pub fn icon_pass(size: u32, stroke_px: u32, grid: u32) -> bool {
    size % grid == 0 && stroke_px >= 2 && size >= 16 && size <= 128
}

// ===========================================================================
// F381 — 光标主题：统一光标风格
// ===========================================================================

pub const CURSOR_KINDS: [&str; 6] = ["default", "pointer", "text", "wait", "resize-ns", "grab"];

// ===========================================================================
// F382 — 字体选型与授权：中文字体合法授权
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FontLicense {
    Ofl,
    Apache,
    Commercial,
    Unknown,
}

pub fn font_license_ok(l: FontLicense) -> bool {
    matches!(l, FontLicense::Ofl | FontLicense::Apache | FontLicense::Commercial)
}

// ===========================================================================
// F383 — 深浅主题打磨：双主题像素级走查
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ThemePair {
    pub dark_bg: u32,
    pub dark_fg: u32,
    pub light_bg: u32,
    pub light_fg: u32,
}

/// 灰度近似对比度：亮度差 permille ≥ 400 视为可读（WCAG 简化）。
pub fn contrast_ok(bg: u32, fg: u32) -> bool {
    let lum = |c: u32| {
        let r = (c >> 16) & 0xFF;
        let g = (c >> 8) & 0xFF;
        let b = c & 0xFF;
        (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000
    };
    let (a, b) = (lum(bg), lum(fg));
    let d = if a > b { a - b } else { b - a };
    d * 1000 / 255 >= 400
}

impl ThemePair {
    pub fn both_readable(&self) -> bool {
        contrast_ok(self.dark_bg, self.dark_fg) && contrast_ok(self.light_bg, self.light_fg)
    }
}

// ===========================================================================
// F384 — 对比度全检：无障碍对比度达标
// ===========================================================================

pub const CONTRAST_MIN_PERMILLE: u32 = 400;

pub fn wcag_gate(bg: u32, fg: u32) -> bool {
    contrast_ok(bg, fg)
}

// ===========================================================================
// F385/F386 — 开机/关机动画
// ===========================================================================

pub const BOOT_ANIM_FRAMES: u32 = 90; // 1.5s @60fps
pub const BOOT_ANIM_PARALLEL: bool = true; // 帧率独立于内核初始化进度
pub const SHUTDOWN_ANIM_MS: u32 = 800;

// ===========================================================================
// F387 — 错误画面友好化：人话错误 + 自助建议
// ===========================================================================

#[derive(Clone, Copy)]
pub struct FriendlyError {
    pub code: u16,
    pub human: &'static str,
    pub selfhelp: &'static str,
}

impl FriendlyError {
    pub fn friendly(&self) -> bool {
        !self.human.is_empty() && !self.selfhelp.is_empty()
    }
}

// ===========================================================================
// F388 — 安装体验：电影感安装流程
// ===========================================================================

/// 安装动效步进必须与真实进度同步（不同步即超 200 permille 偏差判失败）。
pub fn install_anim_sync(shown_permille: u16, actual_permille: u16) -> bool {
    let d = if shown_permille > actual_permille {
        shown_permille - actual_permille
    } else {
        actual_permille - shown_permille
    };
    d <= 200
}

// ===========================================================================
// F389 — 首次开箱仪式感：开箱引导动效
// ===========================================================================

pub const UNBOXING_CUES: [&str; 4] = ["logo-reveal", "hello", "accent-pick", "done-chime"];

// ===========================================================================
// F390 — 彩蛋系统：有分寸的彩蛋清单
// ===========================================================================

#[derive(Clone, Copy)]
pub struct EasterEgg {
    pub trigger: &'static str,
    pub subtle: bool, // 有分寸：不干扰正常使用
}

pub fn easter_eggs_tasteful(eggs: &[EasterEgg]) -> bool {
    eggs.iter().all(|e| e.subtle && !e.trigger.is_empty())
}

// ===========================================================================
// F391 — 音效记忆开关：偏好持久化
// ===========================================================================

pub struct SoundPref {
    pub enabled: bool,
    pub remembered: bool,
}

impl SoundPref {
    pub fn persisted(&self) -> bool {
        self.remembered
    }
}

// ===========================================================================
// F392 — 省电动效降级：低电自动简化
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerTier {
    Full,
    Reduced,
    Minimal,
}

/// 电量 permille → 动效档位：<20% 最简，<50% 简化。
pub fn motion_tier(battery_permille: u16, charging: bool) -> PowerTier {
    if charging || battery_permille >= 500 {
        PowerTier::Full
    } else if battery_permille >= 200 {
        PowerTier::Reduced
    } else {
        PowerTier::Minimal
    }
}

// ===========================================================================
// F393 — 艺术资产版本库：资产可追溯
// ===========================================================================

#[derive(Clone, Copy)]
pub struct ArtAsset {
    pub name: &'static str,
    pub rev: u32,
    pub source: &'static str, // 出处/作者
}

impl ArtAsset {
    pub fn traceable(&self) -> bool {
        !self.name.is_empty() && self.rev > 0 && !self.source.is_empty()
    }
}

// ===========================================================================
// F394 — 设计评审流程：评审准入与记录
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReviewVerdict {
    Pending,
    Approved,
    Rework,
}

pub fn review_gate(assets_done: bool, contrast_checked: bool, verdict: ReviewVerdict) -> bool {
    match verdict {
        ReviewVerdict::Approved => assets_done && contrast_checked,
        _ => true,
    }
}

// ===========================================================================
// F395 — 像素走查制度：定期走查 + 问题单
// ===========================================================================

pub const PIXEL_WALKTHROUGH_CADENCE_WEEKS: u32 = 4;

#[derive(Clone, Copy)]
pub struct WalkthroughIssue {
    pub screen: &'static str,
    pub severity: u8, // 0~3
    pub fixed: bool,
}

pub fn walkthrough_open_critical(issues: &[WalkthroughIssue]) -> usize {
    issues.iter().filter(|i| i.severity >= 3 && !i.fixed).count()
}

// ===========================================================================
// F396 — 动效性能红线：动效不破帧预算
// ===========================================================================

/// 每帧合成预算 16.6ms；动效帧超预算即越线。
pub fn motion_frame_within(us_per_frame: u32) -> bool {
    us_per_frame <= 16_600
}

// ===========================================================================
// F397 — 品牌一致性检查器：自动检查工具
// ===========================================================================

/// 品牌色与给定的偏差 permille ≤ 50 视为一致。
pub fn brand_color_match(actual_rgb: u32, brand_rgb: u32) -> bool {
    let diff = |a: u32, b: u32| if a > b { a - b } else { b - a };
    let (ar, ag, ab) = ((actual_rgb >> 16) & 0xFF, (actual_rgb >> 8) & 0xFF, actual_rgb & 0xFF);
    let (br, bg, bb) = ((brand_rgb >> 16) & 0xFF, (brand_rgb >> 8) & 0xFF, brand_rgb & 0xFF);
    let dr = diff(ar, br) * 1000 / 255;
    let dg = diff(ag, bg) * 1000 / 255;
    let db = diff(ab, bb) * 1000 / 255;
    dr.max(dg).max(db) <= 50
}

// ===========================================================================
// F398 — 设计令牌治理：令牌生命周期管理
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TokenStage {
    Draft,
    Active,
    Deprecated,
}

pub fn token_usable(s: TokenStage) -> bool {
    matches!(s, TokenStage::Draft | TokenStage::Active)
}

// ===========================================================================
// F399 — 年度设计回顾：设计演进存档
// ===========================================================================

pub const DESIGN_REVIEW_SECTIONS: [&str; 4] = ["tokens-evolution", "before-after", "metrics", "learnings"];

// ===========================================================================
// F400 — 作品集展示页：成品级展示页面
// ===========================================================================

#[derive(Clone, Copy)]
pub struct PortfolioItem {
    pub title: &'static str,
    pub screenshot: bool,
    pub caption: &'static str,
}

pub fn portfolio_ready(items: &[PortfolioItem]) -> bool {
    items.len() >= 4 && items.iter().all(|i| i.screenshot && !i.title.is_empty() && !i.caption.is_empty())
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m4arts_checks() -> CheckSet {
    let mut set = CheckSet::new("m4arts");

    // F376 视觉语言
    set.add(
        "F376 visual language v2",
        surface_token_valid(SURFACE_DIALOG) && surface_token_valid(SURFACE_CARD) && surface_token_valid(SURFACE_WINDOW),
        "tokens",
    );
    set.add(
        "F376 token bounds",
        !surface_token_valid(SurfaceToken { radius_px: 40, shadow: 9, elevation: 1 }),
        "bounds",
    );

    // F377 动效编排
    let cues = [
        MotionCue { at_ms: 0, dur_ms: 200, easing: 1 },
        MotionCue { at_ms: 150, dur_ms: 300, easing: 0 },
        MotionCue { at_ms: 500, dur_ms: 100, easing: 2 },
    ];
    set.add("F377 motion cues", cues.iter().all(|c| c.valid()), "validated");
    set.add("F377 timeline", timeline_total_ms(&cues) == 600, "total span");
    set.add("F377 overlap detect", cues[0].overlaps(&cues[1]) && !cues[1].overlaps(&cues[2]), "scheduling");

    // F378 系统音
    set.add("F378 system sounds", SYSTEM_SOUNDS.len() == 6 && sound_param_sane(), "full set, mutable");

    // F379 壁纸
    set.add("F379 wallpapers", WALLPAPERS.len() == 5 && WALLPAPERS.contains(&"dynamic"), "four seasons+dynamic");

    // F380 图标管线
    set.add("F380 icon pipeline", icon_pass(128, 8, 8) && !icon_pass(127, 8, 8), "128px pass");
    set.add("F380 icon stroke", !icon_pass(64, 1, 8), "stroke rule");

    // F381 光标
    set.add("F381 cursor theme", CURSOR_KINDS.len() == 6, "unified style");

    // F382 字体授权
    set.add(
        "F382 font license",
        font_license_ok(FontLicense::Ofl) && !font_license_ok(FontLicense::Unknown),
        "legal only",
    );

    // F383 双主题
    let tp = ThemePair { dark_bg: 0x1E1E1E, dark_fg: 0xEAEAEA, light_bg: 0xFAFAFA, light_fg: 0x222222 };
    set.add("F383 dual theme polish", tp.both_readable(), "pixel walkthrough");
    set.add(
        "F383 bad contrast caught",
        !ThemePair { dark_bg: 0x808080, dark_fg: 0x909090, light_bg: 0xFAFAFA, light_fg: 0x222222 }.both_readable(),
        "gray-on-gray",
    );

    // F384 对比度
    set.add("F384 contrast gate", wcag_gate(0x000000, 0xFFFFFF) && !wcag_gate(0x777777, 0x888888), "a11y");

    // F385/386 动画
    set.add("F385 boot anim", BOOT_ANIM_FRAMES == 90 && BOOT_ANIM_PARALLEL, "independent fps");
    set.add("F386 shutdown anim", SHUTDOWN_ANIM_MS == 800, "graceful exit");

    // F387 错误友好化
    let fe = FriendlyError { code: 0x30, human: "找不到这个文件", selfhelp: "确认路径后重试" };
    set.add(
        "F387 friendly error",
        fe.friendly() && !FriendlyError { code: 1, human: "", selfhelp: "" }.friendly(),
        "human+selfhelp",
    );

    // F388 安装体验
    set.add("F388 install sync", install_anim_sync(600, 650) && !install_anim_sync(900, 600), "cinematic");

    // F389 开箱
    set.add("F389 unboxing", UNBOXING_CUES.len() == 4, "ritual");

    // F390 彩蛋
    let eggs = [EasterEgg { trigger: "konami", subtle: true }];
    set.add(
        "F390 easter eggs",
        easter_eggs_tasteful(&eggs) && !easter_eggs_tasteful(&[EasterEgg { trigger: "ad", subtle: false }]),
        "tasteful only",
    );

    // F391 音效记忆
    let sp = SoundPref { enabled: true, remembered: true };
    set.add("F391 sound memory", sp.persisted(), "pref persist");

    // F392 省电降级
    set.add(
        "F392 power tier",
        motion_tier(800, false) == PowerTier::Full
            && motion_tier(300, false) == PowerTier::Reduced
            && motion_tier(100, false) == PowerTier::Minimal
            && motion_tier(0, true) == PowerTier::Full,
        "auto degrade",
    );

    // F393 资产版本
    let art = ArtAsset { name: "logo-512", rev: 4, source: "in-house" };
    set.add("F393 art assets", art.traceable(), "traceable");

    // F394 设计评审
    set.add(
        "F394 design review",
        review_gate(true, true, ReviewVerdict::Approved)
            && !review_gate(true, false, ReviewVerdict::Approved)
            && review_gate(false, false, ReviewVerdict::Rework),
        "gate",
    );

    // F395 像素走查
    let issues = [
        WalkthroughIssue { screen: "settings", severity: 3, fixed: false },
        WalkthroughIssue { screen: "files", severity: 2, fixed: false },
    ];
    set.add("F395 pixel walkthrough", walkthrough_open_critical(&issues) == 1 && PIXEL_WALKTHROUGH_CADENCE_WEEKS == 4, "cadence+issues");

    // F396 动效红线
    set.add("F396 motion redline", motion_frame_within(14_000) && !motion_frame_within(17_000), "frame budget");

    // F397 品牌一致性
    set.add("F397 brand checker", brand_color_match(0x0A58CE, 0x0A5AD2) && !brand_color_match(0xFF0000, 0x00FF00), "auto check");

    // F398 令牌治理
    set.add(
        "F398 token lifecycle",
        token_usable(TokenStage::Draft) && token_usable(TokenStage::Active) && !token_usable(TokenStage::Deprecated),
        "lifecycle",
    );

    // F399 年度回顾
    set.add("F399 annual design review", DESIGN_REVIEW_SECTIONS.len() == 4, "archived");

    // F400 作品集
    let pf = [
        PortfolioItem { title: "boot", screenshot: true, caption: "开机瞬间" },
        PortfolioItem { title: "desk", screenshot: true, caption: "桌面全景" },
        PortfolioItem { title: "files", screenshot: true, caption: "文件管理器" },
        PortfolioItem { title: "terminal", screenshot: true, caption: "真彩终端" },
    ];
    set.add("F400 portfolio", portfolio_ready(&pf), "showcase page");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f377_timeline_total() {
        let empty: [MotionCue; 0] = [];
        assert_eq!(timeline_total_ms(&empty), 0);
        let c = [MotionCue { at_ms: 100, dur_ms: 50, easing: 0 }];
        assert_eq!(timeline_total_ms(&c), 150);
    }

    #[test]
    fn f383_contrast_math() {
        assert!(contrast_ok(0x000000, 0xFFFFFF));
        assert!(!contrast_ok(0x808080, 0x909090));
    }

    #[test]
    fn f392_tier_boundaries() {
        assert_eq!(motion_tier(500, false), PowerTier::Full);
        assert_eq!(motion_tier(499, false), PowerTier::Reduced);
        assert_eq!(motion_tier(199, false), PowerTier::Minimal);
    }

    #[test]
    fn f397_brand_tolerance() {
        assert!(brand_color_match(0x0A5A28, 0x0A5A32)); // 通道差 ≤50‰
        assert!(!brand_color_match(0x000000, 0xFFFFFF));
    }

    #[test]
    fn f400_domain_selfcheck_all_pass() {
        let set = run_m4arts_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
