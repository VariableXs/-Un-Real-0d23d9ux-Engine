//! m600theme — VARIX-M600 AI-10 主题艺术域 (F226~F250)
//!
//! 主题语言规范/材质设计系统/色彩科学中台/动效曲线博物馆/字体排印学/
//! 图标插画工坊/壁纸策展人/季节叙事引擎/节气美学/昼夜光影剧场/
//! 高对比作品模式/玻璃拟态调优室/新拟物实验舱/极简主义模式/复古计算致敬/
//! 蒸汽波试验田/用户主题市场/主题迁移器/对比度自动校正/深浅色和谐引擎/
//! 材质性能预算/主题差异器/设计令牌字典/视觉回归走廊/美学年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。
//! 登记类接口自带去重或容量上限拒绝；自检断言遵守"末态读取"禁令。

use crate::checks::CheckSet;

// ===========================================================================
// F226 — 主题语言规范：令牌名 = 小写字母开头 + 小写/数字/'-'
// ===========================================================================

pub const TOKEN_NAME_MAX: usize = 32;

pub fn design_token_ok(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > TOKEN_NAME_MAX {
        return false;
    }
    if !name[0].is_ascii_lowercase() {
        return false;
    }
    name.iter().all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

// ===========================================================================
// F227 — 材质设计系统：层级 → 投影不透明度
// ===========================================================================

pub const MATERIAL_ELEVATION_MAX: u32 = 12;

/// opacity = 200 + elevation*50，层级超出上限先钳位。
pub fn material_shadow_opacity(elevation: u32) -> u32 {
    let e = if elevation > MATERIAL_ELEVATION_MAX { MATERIAL_ELEVATION_MAX } else { elevation };
    200 + e * 50
}

// ===========================================================================
// F228 — 色彩科学中台：RGB565 打包 / 亮度 permille / 对比度
// ===========================================================================

pub fn rgb565_pack(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
}

pub fn rgb565_channel5(c: u16) -> u8 {
    ((c >> 11) & 0x1F) as u8
}

/// 感知亮度 permille：Rec.601 权重 299/587/114 归一到 0..1000。
pub fn luma_permille(r: u8, g: u8, b: u8) -> u32 {
    (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 255
}

/// 对比度（×1000 定点）：(hi+50)*1000/(lo+50)，1000 = 1:1。
pub fn contrast_ratio_permille(l1: u32, l2: u32) -> u32 {
    let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (hi + 50) * 1000 / (lo + 50)
}

// ===========================================================================
// F229 — 动效曲线博物馆：三次缓入 / 三次缓出（permille 定点）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShowpieceCurve {
    EaseInCubic,
    EaseOutCubic,
}

pub fn showpiece_sample(curve: ShowpieceCurve, t_permille: u32) -> u32 {
    let t = if t_permille > 1000 { 1000 } else { t_permille };
    match curve {
        ShowpieceCurve::EaseInCubic => t * t * t / 1_000_000,
        ShowpieceCurve::EaseOutCubic => 1000 - (1000 - t) * (1000 - t) * (1000 - t) / 1_000_000,
    }
}

// ===========================================================================
// F230 — 字体排印学：1.25 倍字阶坡道
// ===========================================================================

pub const TYPE_RAMP_BASE_PX: u32 = 16;
pub const TYPE_RAMP_RATIO: u32 = 1250;

pub fn type_ramp_px(step: u32) -> u32 {
    let mut size = TYPE_RAMP_BASE_PX;
    let mut i = 0u32;
    while i < step {
        size = size * TYPE_RAMP_RATIO / 1000;
        i += 1;
    }
    size
}

// ===========================================================================
// F231 — 图标插画工坊：网格对齐 + 格数
// ===========================================================================

pub const ICON_GRID_PX: u32 = 8;

pub fn icon_grid_ok(size_px: u32) -> bool {
    size_px >= ICON_GRID_PX && size_px % ICON_GRID_PX == 0
}

pub fn icon_cells(size_px: u32) -> u32 {
    size_px / ICON_GRID_PX
}

// ===========================================================================
// F232 — 壁纸策展人：按内容哈希收藏（去重 + 容量上限）
// ===========================================================================

pub const GALLERY_CAP: usize = 12;

#[derive(Clone, Copy, Debug)]
pub struct WallpaperGallery {
    pub hashes: [u64; GALLERY_CAP],
    pub count: usize,
}

impl WallpaperGallery {
    pub const fn new() -> WallpaperGallery {
        WallpaperGallery { hashes: [0; GALLERY_CAP], count: 0 }
    }

    /// 收藏：零哈希视为非法，重复哈希去重，满员拒绝。
    pub fn curate(&mut self, hash: u64) -> bool {
        if hash == 0 || self.count >= GALLERY_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.hashes[i] == hash {
                return false;
            }
            i += 1;
        }
        self.hashes[self.count] = hash;
        self.count += 1;
        true
    }
}

// ===========================================================================
// F233 — 季节叙事引擎：月份 → 叙事季节
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NarrativeSeason {
    Spring,
    Summer,
    Autumn,
    Winter,
}

pub fn narrative_season_of(month: u8) -> NarrativeSeason {
    match month {
        3..=5 => NarrativeSeason::Spring,
        6..=8 => NarrativeSeason::Summer,
        9..=11 => NarrativeSeason::Autumn,
        _ => NarrativeSeason::Winter,
    }
}

// ===========================================================================
// F234 — 节气美学：每月 2 气，16 日换气
// ===========================================================================

pub const SOLAR_TERMS_PER_MONTH: u32 = 2;

/// 节气序号 0..24：上半月取偶位，16 日起取奇位；非法月份返回 0。
pub fn solar_term_index(month: u8, day: u8) -> u32 {
    let m = month as u32;
    if m == 0 || m > 12 {
        0
    } else {
        (m - 1) * SOLAR_TERMS_PER_MONTH + if day >= 16 { 1 } else { 0 }
    }
}

// ===========================================================================
// F235 — 昼夜光影剧场：19 点入夜、6 点天亮，亮度二态
// ===========================================================================

pub const LIGHTING_NIGHT_FROM: u32 = 19;
pub const LIGHTING_DAYBREAK: u32 = 6;
pub const LIGHTING_NIGHT_BRIGHTNESS: u32 = 200;
pub const LIGHTING_DAY_BRIGHTNESS: u32 = 800;

pub fn lighting_is_night(hour: u32) -> bool {
    hour >= LIGHTING_NIGHT_FROM || hour < LIGHTING_DAYBREAK
}

pub fn lighting_brightness(hour: u32) -> u32 {
    if lighting_is_night(hour) {
        LIGHTING_NIGHT_BRIGHTNESS
    } else {
        LIGHTING_DAY_BRIGHTNESS
    }
}

// ===========================================================================
// F236 — 高对比作品模式：对比不足即建议开启，开启后不透明度拉满
// ===========================================================================

pub const HC_TRIGGER_CONTRAST: u32 = 4500;

pub fn hc_recommended(contrast_permille: u32) -> bool {
    contrast_permille < HC_TRIGGER_CONTRAST
}

pub fn hc_opacity(enabled: bool, normal_opacity: u32) -> u32 {
    if enabled {
        1000
    } else {
        normal_opacity
    }
}

// ===========================================================================
// F237 — 玻璃拟态调优室：模糊半径 × 透明度预算 + 染色
// ===========================================================================

pub const GLASS_BLUR_BUDGET: u32 = 6000;

/// 预算单位 = blur_px * alpha_permille。
pub fn glass_fit(blur_px: u32, alpha_permille: u32) -> bool {
    blur_px * alpha_permille <= GLASS_BLUR_BUDGET
}

pub fn glass_tint(base_permille: u32, alpha_permille: u32) -> u32 {
    base_permille * alpha_permille / 1000
}

// ===========================================================================
// F238 — 新拟物实验舱：双影（亮影上、暗影下）
// ===========================================================================

pub const NEU_ELEVATION_MAX: u32 = 8;

/// 返回 (亮影偏移, 暗影偏移)。
pub fn neu_shadow_pair(elevation: u32) -> (u32, u32) {
    (elevation + 2, elevation + 1)
}

/// 仅 1..=8 层级允许新拟物。
pub fn neu_enabled(elevation: u32) -> bool {
    elevation > 0 && elevation <= NEU_ELEVATION_MAX
}

// ===========================================================================
// F239 — 极简主义模式：装饰位超集即不合格，裁剪到必需位
// ===========================================================================

pub const MINIMAL_ESSENTIAL_MASK: u32 = 0b0011;

pub fn minimal_ok(decor_mask: u32) -> bool {
    decor_mask & !MINIMAL_ESSENTIAL_MASK == 0
}

pub fn minimal_strip(decor_mask: u32) -> u32 {
    decor_mask & MINIMAL_ESSENTIAL_MASK
}

// ===========================================================================
// F240 — 复古计算致敬：四色调色板按亮度分桶 + 扫描线
// ===========================================================================

pub const RETRO_PALETTE_SIZE: u32 = 4;

/// 亮度 0..1000 → 色板桶 0..3。
pub fn retro_index(luma_permille: u32) -> u32 {
    let l = if luma_permille > 1000 { 1000 } else { luma_permille };
    l * RETRO_PALETTE_SIZE / 1001
}

pub fn retro_scanline(y: u32) -> bool {
    y % 2 == 0
}

// ===========================================================================
// F241 — 蒸汽波试验田：色相 +130° 漂移 + 3 行错位毛刺
// ===========================================================================

pub const VAPOR_HUE_SHIFT: u32 = 130;

pub fn vapor_hue(hue_deg: u32) -> u32 {
    (hue_deg + VAPOR_HUE_SHIFT) % 360
}

pub fn vapor_glitch_row(y: u32) -> u32 {
    (y / 3) % 2
}

// ===========================================================================
// F242 — 用户主题市场：上架（去重 + 容量）+ 评分榜首
// ===========================================================================

pub const BAZAAR_CAP: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketListing {
    pub token_hash: u64,
    pub rating_permille: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ThemeBazaar {
    pub listings: [MarketListing; BAZAAR_CAP],
    pub count: usize,
}

impl ThemeBazaar {
    pub const fn new() -> ThemeBazaar {
        ThemeBazaar { listings: [MarketListing { token_hash: 0, rating_permille: 0 }; BAZAAR_CAP], count: 0 }
    }

    /// 上架：零哈希非法、重复哈希去重、满员拒绝。
    pub fn publish(&mut self, token_hash: u64, rating_permille: u32) -> bool {
        if token_hash == 0 || self.count >= BAZAAR_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.listings[i].token_hash == token_hash {
                return false;
            }
            i += 1;
        }
        self.listings[self.count] = MarketListing { token_hash, rating_permille };
        self.count += 1;
        true
    }

    /// 评分最高的上架作品哈希；空市返回 0。
    pub fn best(&self) -> u64 {
        let mut best_hash = 0u64;
        let mut best_rating = 0u32;
        let mut i = 0usize;
        while i < self.count {
            if self.listings[i].rating_permille > best_rating {
                best_rating = self.listings[i].rating_permille;
                best_hash = self.listings[i].token_hash;
            }
            i += 1;
        }
        best_hash
    }
}

// ===========================================================================
// F243 — 主题迁移器：6 个字段位掩码迁移完成度
// ===========================================================================

pub const MIGRATION_FIELD_COUNT: u32 = 6;
pub const MIGRATION_REQUIRED_MASK: u32 = (1 << MIGRATION_FIELD_COUNT) - 1;

pub fn migration_done(migrated_mask: u32) -> bool {
    migrated_mask & MIGRATION_REQUIRED_MASK == MIGRATION_REQUIRED_MASK
}

pub fn migration_bit(field: usize) -> Option<u32> {
    if (field as u32) < MIGRATION_FIELD_COUNT {
        Some(1u32 << field)
    } else {
        None
    }
}

// ===========================================================================
// F244 — 对比度自动校正：前景亮度步进抬升到对比达标
// ===========================================================================

pub const CONTRAST_TARGET: u32 = 4500;
pub const CONTRAST_STEP: u32 = 10;

/// 暗底场景下抬升前景亮度直至对比 ≥ 4500‰（已达标则原样返回）。
pub fn auto_contrast_boost(fg_luma: u32, bg_luma: u32) -> u32 {
    let mut fg = fg_luma;
    while fg < 1000 && contrast_ratio_permille(fg, bg_luma) < CONTRAST_TARGET {
        fg += CONTRAST_STEP;
    }
    fg
}

// ===========================================================================
// F245 — 深浅色和谐引擎：暗色令牌亮度 ≈ 反转的浅色令牌
// ===========================================================================

pub const TWIN_TOLERANCE: u32 = 50;

pub fn twin_harmony_ok(dark_luma: u32, light_luma: u32) -> bool {
    let inverted = 1000 - if light_luma > 1000 { 1000 } else { light_luma };
    let diff = if inverted > dark_luma { inverted - dark_luma } else { dark_luma - inverted };
    diff <= TWIN_TOLERANCE
}

// ===========================================================================
// F246 — 材质性能预算：定额发放，绝不透支
// ===========================================================================

pub const MATERIAL_BUDGET_UNITS: u32 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialBudget {
    pub budget: u32,
    pub consumed: u32,
}

impl MaterialBudget {
    pub const fn new(budget: u32) -> MaterialBudget {
        MaterialBudget { budget, consumed: 0 }
    }

    pub fn remaining(&self) -> u32 {
        self.budget.saturating_sub(self.consumed)
    }

    /// 申请 n 单位材质额度，返回实际批准数。
    pub fn take(&mut self, want: u32) -> u32 {
        let remaining = self.remaining();
        let granted = if want > remaining { remaining } else { want };
        self.consumed += granted;
        granted
    }
}

// ===========================================================================
// F247 — 主题差异器：逐令牌比对，长度不等视为不可比
// ===========================================================================

pub const THEME_DIFF_INCOMPARABLE: u32 = u32::MAX;

pub fn theme_diff_count(a: &[u32], b: &[u32]) -> u32 {
    if a.len() != b.len() {
        return THEME_DIFF_INCOMPARABLE;
    }
    let mut diffs = 0u32;
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            diffs += 1;
        }
        i += 1;
    }
    diffs
}

// ===========================================================================
// F248 — 设计令牌字典：令牌登记（去重 + 容量上限）
// ===========================================================================

pub const LEXICON_CAP: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct TokenLexicon {
    pub ids: [u16; LEXICON_CAP],
    pub count: usize,
}

impl TokenLexicon {
    pub const fn new() -> TokenLexicon {
        TokenLexicon { ids: [0; LEXICON_CAP], count: 0 }
    }

    pub fn define(&mut self, id: u16) -> bool {
        if self.count >= LEXICON_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.ids[i] == id {
                return false;
            }
            i += 1;
        }
        self.ids[self.count] = id;
        self.count += 1;
        true
    }

    pub fn defined(&self, id: u16) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.ids[i] == id {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ===========================================================================
// F249 — 视觉回归走廊：三视口金样哈希逐像素比对
// ===========================================================================

pub const VISUAL_VIEWPORTS: usize = 3;

pub fn visual_golden_hashes() -> [u64; VISUAL_VIEWPORTS] {
    [0xA11CE, 0xB0B, 0xC0DE]
}

pub fn visual_corridor_match(actual: &[u64; VISUAL_VIEWPORTS]) -> bool {
    let golden = visual_golden_hashes();
    let mut i = 0usize;
    while i < VISUAL_VIEWPORTS {
        if actual[i] != golden[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 首个失配视口（全对返回 None）。
pub fn visual_first_mismatch(actual: &[u64; VISUAL_VIEWPORTS]) -> Option<usize> {
    let golden = visual_golden_hashes();
    let mut i = 0usize;
    while i < VISUAL_VIEWPORTS {
        if actual[i] != golden[i] {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ===========================================================================
// F250 — 美学年报：章节完备性
// ===========================================================================

pub const M600_THEME_REPORT_SECTIONS: [&str; 5] =
    ["tokens", "materials", "motion", "color", "regression"];

pub fn m600_theme_report_complete(filled: u32) -> bool {
    filled >= M600_THEME_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600theme_checks() -> CheckSet {
    let mut set = CheckSet::new("m600theme");

    // F226 主题语言规范
    set.add(
        "F226 token grammar",
        design_token_ok(b"color-primary") && design_token_ok(b"a-1") && !design_token_ok(b"-color")
            && !design_token_ok(b"Color") && !design_token_ok(b""),
        "lowercase head, charset strict",
    );
    set.add("F226 token length", !design_token_ok(&[b'a'; TOKEN_NAME_MAX + 1]), "over 32 rejected");

    // F227 材质设计系统
    set.add(
        "F227 material shadow",
        material_shadow_opacity(0) == 200 && material_shadow_opacity(4) == 400
            && material_shadow_opacity(12) == 800,
        "linear ramp",
    );
    set.add("F227 material clamp", material_shadow_opacity(20) == 800, "elevation clamped");

    // F228 色彩科学中台
    set.add(
        "F228 rgb565 pack",
        rgb565_pack(255, 255, 255) == 0xFFFF && rgb565_pack(0, 0, 0) == 0
            && rgb565_channel5(0xF800) == 31,
        "565 bit layout",
    );
    set.add(
        "F228 luma weights",
        luma_permille(255, 0, 0) == 299 && luma_permille(0, 255, 0) == 587
            && luma_permille(0, 0, 255) == 114 && luma_permille(255, 255, 255) == 1000,
        "rec601 normalized",
    );
    set.add(
        "F228 contrast scale",
        contrast_ratio_permille(1000, 0) == 21000 && contrast_ratio_permille(500, 500) == 1000,
        "x1000 fixed point",
    );

    // F229 动效曲线博物馆
    set.add(
        "F229 motion midpoint",
        showpiece_sample(ShowpieceCurve::EaseInCubic, 500) == 125
            && showpiece_sample(ShowpieceCurve::EaseOutCubic, 500) == 875,
        "cubic split 125/875",
    );
    set.add(
        "F229 motion endpoints",
        showpiece_sample(ShowpieceCurve::EaseInCubic, 1000) == 1000
            && showpiece_sample(ShowpieceCurve::EaseOutCubic, 0) == 0,
        "curves anchored",
    );

    // F230 字体排印学
    set.add(
        "F230 type ramp",
        type_ramp_px(0) == 16 && type_ramp_px(1) == 20 && type_ramp_px(2) == 25 && type_ramp_px(3) == 31,
        "1.25 steps with floor",
    );

    // F231 图标插画工坊
    set.add(
        "F231 icon grid",
        icon_grid_ok(24) && !icon_grid_ok(20) && !icon_grid_ok(4),
        "multiple of 8, at least 8",
    );
    set.add("F231 icon cells", icon_cells(24) == 3, "size / grid");

    // F232 壁纸策展人
    let mut gallery = WallpaperGallery::new();
    let w1 = gallery.curate(11);
    let w2 = gallery.curate(11);
    let w3 = gallery.curate(0);
    let wcount = gallery.count;
    set.add("F232 wallpaper dedup", w1 && !w2 && !w3 && wcount == 1, "zero + dup rejected");
    let mut wfull = WallpaperGallery::new();
    let mut wh = 1u64;
    let mut wgot = 0usize;
    while wh <= 12 {
        if wfull.curate(wh) {
            wgot += 1;
        }
        wh += 1;
    }
    let woverflow = wfull.curate(99);
    set.add("F232 wallpaper capacity", wgot == 12 && !woverflow, "12 slots cap");

    // F233 季节叙事引擎
    set.add(
        "F233 narrative season",
        narrative_season_of(4) == NarrativeSeason::Spring && narrative_season_of(7) == NarrativeSeason::Summer
            && narrative_season_of(10) == NarrativeSeason::Autumn
            && narrative_season_of(12) == NarrativeSeason::Winter
            && narrative_season_of(1) == NarrativeSeason::Winter,
        "month bands",
    );

    // F234 节气美学
    set.add(
        "F234 solar term index",
        solar_term_index(1, 5) == 0 && solar_term_index(1, 20) == 1
            && solar_term_index(12, 30) == 23 && solar_term_index(12, 1) == 22,
        "two terms per month",
    );
    set.add(
        "F234 solar term guard",
        solar_term_index(0, 5) == 0 && solar_term_index(13, 5) == 0,
        "bad month pinned to 0",
    );

    // F235 昼夜光影剧场
    set.add(
        "F235 day night",
        lighting_is_night(23) && lighting_is_night(5) && !lighting_is_night(6)
            && lighting_is_night(19),
        "19:00 in, 06:00 out",
    );
    set.add(
        "F235 brightness states",
        lighting_brightness(23) == 200 && lighting_brightness(12) == 800,
        "two-state dimming",
    );

    // F236 高对比作品模式
    let low_contrast = contrast_ratio_permille(200, 100);
    let high_contrast = contrast_ratio_permille(1000, 0);
    set.add(
        "F236 hc recommend",
        hc_recommended(low_contrast) && !hc_recommended(high_contrast),
        "below 4500‰ suggests",
    );
    set.add(
        "F236 hc opacity",
        hc_opacity(true, 300) == 1000 && hc_opacity(false, 300) == 300,
        "forced full opacity",
    );

    // F237 玻璃拟态调优室
    set.add(
        "F237 glass budget",
        glass_fit(6, 1000) && !glass_fit(7, 1000) && glass_fit(10, 500),
        "radius*alpha <= 6000",
    );
    set.add("F237 glass tint", glass_tint(800, 500) == 400, "alpha scaling");

    // F238 新拟物实验舱
    set.add(
        "F238 neu shadows",
        neu_shadow_pair(4) == (6, 5),
        "light +2, dark +1",
    );
    set.add(
        "F238 neu gate",
        !neu_enabled(0) && neu_enabled(8) && !neu_enabled(9),
        "1..=8 only",
    );

    // F239 极简主义模式
    set.add(
        "F239 minimal gate",
        minimal_ok(0b0010) && minimal_ok(0) && !minimal_ok(0b0111),
        "essential bits only",
    );
    set.add("F239 minimal strip", minimal_strip(0xFF) == 0b0011, "decor stripped");

    // F240 复古计算致敬
    set.add(
        "F240 retro buckets",
        retro_index(0) == 0 && retro_index(500) == 1 && retro_index(750) == 2 && retro_index(1000) == 3,
        "luma to 4 buckets",
    );
    set.add("F240 retro scanline", retro_scanline(4) && !retro_scanline(5), "even rows lit");

    // F241 蒸汽波试验田
    set.add(
        "F241 vapor hue",
        vapor_hue(0) == 130 && vapor_hue(300) == 70 && vapor_hue(230) == 0,
        "+130° wrap",
    );
    set.add("F241 vapor glitch", vapor_glitch_row(3) == 1 && vapor_glitch_row(6) == 0, "every 3rd row toggles");

    // F242 用户主题市场
    let mut bazaar = ThemeBazaar::new();
    let p1 = bazaar.publish(1, 800);
    let p2 = bazaar.publish(1, 900);
    let p3 = bazaar.publish(2, 950);
    let pcount = bazaar.count;
    let top = bazaar.best();
    set.add(
        "F242 bazaar publish",
        p1 && !p2 && p3 && pcount == 2 && top == 2,
        "dedup + best rated",
    );

    // F243 主题迁移器
    set.add(
        "F243 migration done",
        migration_done(0b111111) && !migration_done(0b011111) && migration_done(0b1111111),
        "all 6 fields required",
    );
    set.add(
        "F243 migration bits",
        migration_bit(5) == Some(32) && migration_bit(6).is_none(),
        "bit per field",
    );

    // F244 对比度自动校正
    let boosted = auto_contrast_boost(200, 100);
    let boosted_contrast = contrast_ratio_permille(boosted, 100);
    set.add(
        "F244 contrast boost",
        boosted == 630 && boosted_contrast == 4533,
        "step up to 4500‰",
    );
    let untouched = auto_contrast_boost(800, 100);
    set.add("F244 contrast skip", untouched == 800, "already passing stays");

    // F245 深浅色和谐引擎
    set.add(
        "F245 twin harmony",
        twin_harmony_ok(200, 800) && twin_harmony_ok(250, 800) && !twin_harmony_ok(300, 800),
        "inverted within 50",
    );

    // F246 材质性能预算
    let mut budget = MaterialBudget::new(100);
    let first_grant = budget.take(60);
    let mid_remaining = budget.remaining();
    let second_grant = budget.take(60);
    set.add(
        "F246 budget grant",
        first_grant == 60 && mid_remaining == 40 && second_grant == 40,
        "partial grant on overdraft",
    );
    set.add("F246 budget exhausted", budget.remaining() == 0, "never negative");

    // F247 主题差异器
    set.add(
        "F247 theme diff",
        theme_diff_count(&[1, 2, 3], &[1, 9, 3]) == 1 && theme_diff_count(&[1, 2], &[1, 2]) == 0,
        "count mismatches",
    );
    set.add(
        "F247 theme diff incomparable",
        theme_diff_count(&[1, 2, 3], &[1, 2]) == THEME_DIFF_INCOMPARABLE,
        "length mismatch",
    );

    // F248 设计令牌字典
    let mut lexicon = TokenLexicon::new();
    let d1 = lexicon.define(5);
    let d2 = lexicon.define(5);
    let dcount = lexicon.count;
    let lookup = lexicon.defined(5) && !lexicon.defined(6);
    set.add(
        "F248 lexicon define",
        d1 && !d2 && dcount == 1 && lookup,
        "dup rejected, lookup works",
    );
    let mut lfull = TokenLexicon::new();
    let mut lid = 1u16;
    let mut lgot = 0usize;
    while lid <= 32 {
        if lfull.define(lid) {
            lgot += 1;
        }
        lid += 1;
    }
    let loverflow = lfull.define(99);
    set.add("F248 lexicon capacity", lgot == 32 && !loverflow, "32 tokens cap");

    // F249 视觉回归走廊
    let golden = visual_golden_hashes();
    let mut drifted = visual_golden_hashes();
    drifted[2] = 0xC0DF;
    let mismatch = visual_first_mismatch(&drifted);
    set.add(
        "F249 visual corridor",
        visual_corridor_match(&golden) && !visual_corridor_match(&drifted),
        "hash equality per viewport",
    );
    set.add("F249 visual mismatch", mismatch == Some(2), "first divergent viewport");

    // F250 美学年报
    set.add(
        "F250 theme report",
        M600_THEME_REPORT_SECTIONS.len() == 5 && m600_theme_report_complete(5)
            && !m600_theme_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f228_color_science() {
        assert_eq!(rgb565_pack(255, 0, 0), 0xF800);
        assert_eq!(rgb565_pack(0, 255, 0), 0x07E0);
        assert_eq!(rgb565_pack(0, 0, 255), 0x001F);
        assert_eq!(luma_permille(128, 128, 128), 501); // 128*1000/255 = 501（整除截断）
    }

    #[test]
    fn f230_type_ramp_growth() {
        // 严格递增：16 < 20 < 25 < 31 < 39（31*1250/1000 = 38.75 → 38）
        assert!(type_ramp_px(0) < type_ramp_px(1));
        assert!(type_ramp_px(1) < type_ramp_px(2));
        assert!(type_ramp_px(2) < type_ramp_px(3));
        assert_eq!(type_ramp_px(4), 38);
    }

    #[test]
    fn f244_contrast_boost_boundaries() {
        // 边界：620 差一点（4466），630 达标（4533）
        assert_eq!(contrast_ratio_permille(620, 100), 4466);
        assert_eq!(contrast_ratio_permille(630, 100), 4533);
        assert_eq!(auto_contrast_boost(630, 100), 630);
    }

    #[test]
    fn f246_never_overdraft() {
        let mut b = MaterialBudget::new(50);
        assert_eq!(b.take(80), 50);
        assert_eq!(b.take(1), 0);
        assert_eq!(b.remaining(), 0);
    }

    #[test]
    fn f249_corridor_diff_detection() {
        let mut a = visual_golden_hashes();
        a[0] = 0xA11CF;
        assert_eq!(visual_first_mismatch(&a), Some(0));
        assert!(!visual_corridor_match(&a));
        assert!(visual_corridor_match(&visual_golden_hashes()));
    }

    #[test]
    fn m600theme_selfcheck_all_pass() {
        let set = run_m600theme_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
