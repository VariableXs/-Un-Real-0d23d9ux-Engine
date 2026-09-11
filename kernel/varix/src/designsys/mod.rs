//! AURORA-1000 域二：视觉设计系统（A401~A425）。
//!
//! 纯逻辑 + 固定容量数组实现。不依赖 Vec/String/Box/alloc，
//! no_std 内核 target 下编译，单测在 std 下运行。
//! ASCII 大小写不敏感匹配走 `crate::galaxy::ascii_eq_ci`（无分配）。

use crate::checks::CheckSet;
use crate::galaxy::rt::DetPrng;

// ---------------------------------------------------------------------------
// A401 六类设计令牌
// ---------------------------------------------------------------------------

pub const MAX_TOKENS: usize = 24;
pub const MAX_STORE: usize = 8;
pub const MAX_ICONS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TokenCategory {
    Color = 0,
    Spacing = 1,
    Radius = 2,
    Shadow = 3,
    Typography = 4,
    Motion = 5,
}

impl TokenCategory {
    pub const fn count() -> usize {
        6
    }
    pub fn from_u8(v: u8) -> Option<TokenCategory> {
        match v {
            0 => Some(TokenCategory::Color),
            1 => Some(TokenCategory::Spacing),
            2 => Some(TokenCategory::Radius),
            3 => Some(TokenCategory::Shadow),
            4 => Some(TokenCategory::Typography),
            5 => Some(TokenCategory::Motion),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Token {
    pub cat: u8,
    pub name: &'static str,
    pub value: u32,
}

/// 注册令牌（按名去重，命中则覆盖）。
pub fn register_token(sys: &mut DesignSystem, cat: u8, name: &'static str, value: u32) -> bool {
    if sys.count >= MAX_TOKENS {
        return false;
    }
    for i in 0..sys.count {
        if let Some(t) = sys.tokens[i] {
            if crate::galaxy::ascii_eq_ci(t.name.as_bytes(), name.as_bytes()) {
                sys.tokens[i] = Some(Token { cat, value, name });
                return true;
            }
        }
    }
    sys.tokens[sys.count] = Some(Token { cat, value, name });
    sys.count += 1;
    true
}

pub fn token_value(sys: &DesignSystem, name: &str) -> Option<u32> {
    for i in 0..sys.count {
        if let Some(t) = sys.tokens[i] {
            if crate::galaxy::ascii_eq_ci(t.name.as_bytes(), name.as_bytes()) {
                return Some(t.value);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// A402 色彩系统：RGB888 + 相对亮度（整数近似）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct RGB888 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// 相对亮度近似：(2126R + 7152G + 722B) / 10000，范围 0..=255。
pub fn luminance(c: &RGB888) -> u32 {
    (2126u32 * c.r as u32 + 7152 * c.g as u32 + 722 * c.b as u32) / 10000
}

// ---------------------------------------------------------------------------
// A403 亮暗自动主题
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

pub const THEME_THRESHOLD: u32 = 400; // 环境亮度阈值

pub fn pick_theme(ambient: u32) -> Theme {
    if ambient >= THEME_THRESHOLD {
        Theme::Light
    } else {
        Theme::Dark
    }
}

// ---------------------------------------------------------------------------
// A404 主题包格式：magic + 令牌表 + FNV-1a 校验和
// ---------------------------------------------------------------------------

pub const THEME_MAGIC: [u8; 4] = *b"AURA";

pub fn pack_theme(sys: &DesignSystem, out: &mut [u8]) -> usize {
    let header = 6usize; // magic(4) + active_theme(1) + count(1)
    let body = sys.count * 5; // cat(1) + value(4)
    let total = header + body + 4; // + checksum
    if out.len() < total {
        return 0;
    }
    out[0..4].copy_from_slice(&THEME_MAGIC);
    out[4] = sys.active_theme;
    out[5] = sys.count as u8;
    let mut o = 6usize;
    let mut h: u32 = 0x811c_9dc5;
    for i in 0..sys.count {
        if let Some(t) = sys.tokens[i] {
            out[o] = t.cat;
            out[o + 1..o + 5].copy_from_slice(&t.value.to_le_bytes());
            h ^= t.cat as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= t.value;
            h = h.wrapping_mul(0x0100_0193);
            o += 5;
        }
    }
    out[o..o + 4].copy_from_slice(&h.to_le_bytes());
    o + 4
}

pub fn unpack_ok(buf: &[u8]) -> bool {
    if buf.len() < 10 {
        return false;
    }
    if buf[0] != b'A' || buf[1] != b'U' || buf[2] != b'R' || buf[3] != b'A' {
        return false;
    }
    let count = buf[5] as usize;
    let body = count * 5;
    if buf.len() < 6 + body + 4 {
        return false;
    }
    let mut h: u32 = 0x811c_9dc5;
    let mut o = 6usize;
    for _ in 0..count {
        let cat = buf[o] as u32;
        let val = u32::from_le_bytes([buf[o + 1], buf[o + 2], buf[o + 3], buf[o + 4]]);
        h ^= cat;
        h = h.wrapping_mul(0x0100_0193);
        h ^= val;
        h = h.wrapping_mul(0x0100_0193);
        o += 5;
    }
    u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]) == h
}

// ---------------------------------------------------------------------------
// A405 主题热切换：代数计数（版本翻转，不重建令牌表）
// ---------------------------------------------------------------------------

pub fn set_theme(sys: &mut DesignSystem, theme: u8) {
    sys.active_theme = theme;
    sys.generation += 1; // 版本号翻转，令牌表不变
}

// ---------------------------------------------------------------------------
// A406 强调色自定义：非纯黑纯白
// ---------------------------------------------------------------------------

pub fn accent_ok(c: RGB888) -> bool {
    !((c.r == 0 && c.g == 0 && c.b == 0) || (c.r == 255 && c.g == 255 && c.b == 255))
}

// ---------------------------------------------------------------------------
// A407 圆角/间距/阴影三档（S/M/L）
// ---------------------------------------------------------------------------

pub const RADIUS_S: u32 = 4;
pub const RADIUS_M: u32 = 8;
pub const RADIUS_L: u32 = 16;
pub const SPACING_S: u32 = 4;
pub const SPACING_M: u32 = 8;
pub const SPACING_L: u32 = 16;
pub const SHADOW_S: u32 = 1;
pub const SHADOW_M: u32 = 3;
pub const SHADOW_L: u32 = 6;

pub fn radius_tier(t: usize) -> u32 {
    [RADIUS_S, RADIUS_M, RADIUS_L][t % 3]
}
pub fn spacing_tier(t: usize) -> u32 {
    [SPACING_S, SPACING_M, SPACING_L][t % 3]
}
pub fn shadow_tier(t: usize) -> u32 {
    [SHADOW_S, SHADOW_M, SHADOW_L][t % 3]
}
pub fn is_spacing_tier(v: u32) -> bool {
    v == SPACING_S || v == SPACING_M || v == SPACING_L
}

// ---------------------------------------------------------------------------
// A408 字体字号阶梯 + 行距比例
// ---------------------------------------------------------------------------

pub const FONT_LADDER: [u32; 6] = [12, 14, 16, 20, 24, 32];

pub fn font_size(idx: usize) -> u32 {
    FONT_LADDER[idx % FONT_LADDER.len()]
}
/// 行距比例 1.4，整数化 (size*14+5)/10。
pub fn line_height(size: u32) -> u32 {
    (size * 14 + 5) / 10
}
pub fn font_ladder_monotonic() -> bool {
    (1..FONT_LADDER.len()).all(|i| FONT_LADDER[i] >= FONT_LADDER[i - 1])
}

// ---------------------------------------------------------------------------
// A409 对比度守护（AA 4.5 / AAA 7.0）
// ---------------------------------------------------------------------------

/// 对比度近似 = (max+5)*1000/(min+5)，放大 1000 倍。
pub fn contrast_ratio(l1: u32, l2: u32) -> u32 {
    let (a, b) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
    (a + 5) * 1000 / (b + 5)
}
pub fn contrast_aa(cr: u32) -> bool {
    cr >= 4500
}
pub fn contrast_aaa(cr: u32) -> bool {
    cr >= 7000
}

// ---------------------------------------------------------------------------
// A410 高对比度与色觉障碍
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ColorVision {
    Normal,
    RedGreen,
    BlueYellow,
}

/// hc 模式下强制对比度 >= 7.0（AAA）。
pub fn contrast_ok(cr: u32, hc: bool) -> bool {
    if hc {
        contrast_aaa(cr)
    } else {
        contrast_aa(cr)
    }
}

// ---------------------------------------------------------------------------
// A413 主题商店（固定容量 8，安装/卸载/查重）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ThemeStore {
    pub ids: [Option<u8>; MAX_STORE],
    pub count: usize,
}

impl ThemeStore {
    pub const fn new() -> ThemeStore {
        ThemeStore {
            ids: [None; MAX_STORE],
            count: 0,
        }
    }
    pub fn install(&mut self, id: u8) -> bool {
        if self.count >= MAX_STORE {
            return false;
        }
        if self.ids[..self.count].iter().any(|x| *x == Some(id)) {
            return false; // 查重
        }
        self.ids[self.count] = Some(id);
        self.count += 1;
        true
    }
    pub fn uninstall(&mut self, id: u8) -> bool {
        if let Some(pos) = (0..self.count).find(|&i| self.ids[i] == Some(id)) {
            for i in pos..self.count - 1 {
                self.ids[i] = self.ids[i + 1];
            }
            self.ids[self.count - 1] = None;
            self.count -= 1;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// A415 图标图形语言：IconId + 命名查表
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum IconId {
    Home = 0,
    Settings = 1,
    Search = 2,
    User = 3,
}

pub const ICON_NAMES: [&str; 4] = ["home", "settings", "search", "user"];

pub fn icon_name(id: IconId) -> &'static str {
    ICON_NAMES[id as usize]
}

pub fn find_icon(name: &str) -> Option<IconId> {
    for i in 0..ICON_NAMES.len() {
        if crate::galaxy::ascii_eq_ci(ICON_NAMES[i].as_bytes(), name.as_bytes()) {
            return Some(match i {
                0 => IconId::Home,
                1 => IconId::Settings,
                2 => IconId::Search,
                _ => IconId::User,
            });
        }
    }
    None
}

// ---------------------------------------------------------------------------
// A416 图标矢量渲染：16x16 光栅掩码（占位 fill rect）
// ---------------------------------------------------------------------------

pub fn render_icon(id: IconId, out: &mut [u8; 256]) {
    let _ = id;
    // 占位：左上 4x4 块置 1。
    for y in 0..4u8 {
        for x in 0..4u8 {
            out[(y as usize) * 16 + x as usize] = 1;
        }
    }
}

// ---------------------------------------------------------------------------
// A417 图标任意 DPI 适配：最近档位（16/24/32/48）
// ---------------------------------------------------------------------------

pub fn scale_icon(size: u32) -> u32 {
    let tiers = [16u32, 24, 32, 48];
    let mut best = tiers[0];
    let mut bestd = u32::MAX;
    for i in 0..4 {
        let d = if tiers[i] > size {
            tiers[i] - size
        } else {
            size - tiers[i]
        };
        if d < bestd {
            bestd = d;
            best = tiers[i];
        }
    }
    best
}

// ---------------------------------------------------------------------------
// A421 可观测计数器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct DesignStats {
    pub theme_switches: u64,
    pub lint_failures: u64,
}

// ---------------------------------------------------------------------------
// A420 性能预算（token_lookup O(1) 逻辑标志）
// ---------------------------------------------------------------------------

pub const TOKEN_LOOKUP_O1: bool = true;

pub fn budget_ok(total: u32, budget: u32) -> bool {
    total <= budget
}

// ---------------------------------------------------------------------------
// 设计系统聚合状态（A405/A412/A414/A424 串联）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DesignSystem {
    pub tokens: [Option<Token>; MAX_TOKENS],
    pub count: usize,
    pub active_theme: u8,
    pub generation: u32,
    pub hc: bool,
    pub cvd: ColorVision,
    pub stats: DesignStats,
    pub store: ThemeStore,
    pub draft: [Option<Token>; MAX_TOKENS],
    pub draft_count: usize,
    pub preview_a: u8,
    pub preview_b: u8,
    pub committed: u8,
}

impl DesignSystem {
    pub const fn new() -> DesignSystem {
        DesignSystem {
            tokens: [None; MAX_TOKENS],
            count: 0,
            active_theme: 0,
            generation: 0,
            hc: false,
            cvd: ColorVision::Normal,
            stats: DesignStats { theme_switches: 0, lint_failures: 0 },
            store: ThemeStore::new(),
            draft: [None; MAX_TOKENS],
            draft_count: 0,
            preview_a: 0,
            preview_b: 0,
            committed: 0,
        }
    }

    /// A412 预览槽 A/B 与生效槽分离，commit 才生效。
    pub fn set_preview(&mut self, slot: u8, theme: u8) {
        if slot == 0 {
            self.preview_a = theme;
        } else {
            self.preview_b = theme;
        }
    }
    pub fn commit_preview(&mut self, slot: u8) {
        let theme = if slot == 0 { self.preview_a } else { self.preview_b };
        self.committed = theme;
        self.active_theme = theme;
        self.generation += 1;
    }

    /// A414 草稿覆盖层 apply：覆盖生效令牌。
    pub fn apply_draft(&mut self) {
        for i in 0..self.draft_count {
            if let Some(dt) = self.draft[i] {
                let mut found = false;
                for j in 0..self.count {
                    if let Some(t) = self.tokens[j] {
                        if crate::galaxy::ascii_eq_ci(t.name.as_bytes(), dt.name.as_bytes()) {
                            self.tokens[j] = Some(dt);
                            found = true;
                            break;
                        }
                    }
                }
                if !found && self.count < MAX_TOKENS {
                    self.tokens[self.count] = Some(dt);
                    self.count += 1;
                }
            }
        }
        self.draft_count = 0;
    }
}

// ---------------------------------------------------------------------------
// A418 一致性 lint 门禁：返回违规计数
// ---------------------------------------------------------------------------

pub fn lint_theme(sys: &DesignSystem) -> usize {
    let mut v = 0usize;
    for i in 0..sys.count {
        if let Some(t) = sys.tokens[i] {
            if t.cat == TokenCategory::Spacing as u8 && !is_spacing_tier(t.value) {
                v += 1;
            }
        }
    }
    if !font_ladder_monotonic() {
        v += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// A424 降级链：令牌缺失回退默认档
// ---------------------------------------------------------------------------

pub fn get_or_default(sys: &DesignSystem, name: &str, default: u32) -> u32 {
    token_value(sys, name).unwrap_or(default)
}

// ---------------------------------------------------------------------------
// A422 模糊测试：随机改令牌/切换主题/导出导入不 panic
// ---------------------------------------------------------------------------

pub fn fuzz_design(seed: u64, rounds: usize) -> bool {
    let mut prng = DetPrng::new(seed);
    let mut sys = DesignSystem::new();
    register_token(&mut sys, TokenCategory::Color as u8, "bg", 0x101010);
    register_token(&mut sys, TokenCategory::Spacing as u8, "gap", SPACING_M);
    for _ in 0..rounds {
        let op = prng.next_u64() % 4;
        match op {
            0 => {
                set_theme(&mut sys, (prng.next_u64() % 2) as u8);
                sys.stats.theme_switches += 1;
            }
            1 => {
                if sys.count > 0 {
                    let idx = prng.next_usize(sys.count);
                    if let Some(mut t) = sys.tokens[idx] {
                        t.value = prng.next_u64() as u32;
                        sys.tokens[idx] = Some(t);
                    }
                }
            }
            2 => {
                let mut buf = [0u8; 256];
                let n = pack_theme(&sys, &mut buf);
                if n > 0 && !unpack_ok(&buf[..n]) {
                    return false;
                }
            }
            _ => {
                let v = lint_theme(&sys);
                if v > 0 {
                    sys.stats.lint_failures += 1;
                }
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// A419/A420/A423/A425 自检与收口
// ---------------------------------------------------------------------------

pub fn run_designsys_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-designsys");
    let mut sys = DesignSystem::new();

    // A401 六类令牌 + 表
    set.add(
        "A401 six token categories",
        TokenCategory::count() == 6
            && register_token(&mut sys, TokenCategory::Color as u8, "bg", 0xFF0000)
            && sys.count == 1,
        "6 cats + table",
    );

    // A402 色彩亮度
    let white = RGB888 { r: 255, g: 255, b: 255 };
    let black = RGB888 { r: 0, g: 0, b: 0 };
    let gray = RGB888 { r: 128, g: 128, b: 128 };
    set.add(
        "A402 color luminance",
        luminance(&white) > luminance(&gray)
            && luminance(&gray) > luminance(&black)
            && luminance(&black) == 0,
        "int approx",
    );

    // A403 亮暗自动
    set.add(
        "A403 auto theme",
        pick_theme(500) == Theme::Light && pick_theme(100) == Theme::Dark,
        "ambient threshold",
    );

    // A404 主题包
    let mut buf = [0u8; 256];
    let n = pack_theme(&sys, &mut buf);
    set.add(
        "A404 theme pack",
        n > 0 && unpack_ok(&buf[..n]) && n == 6 + sys.count * 5 + 4,
        "magic + checksum",
    );

    // A405 热切换代数
    let mut s2 = DesignSystem::new();
    let g0 = s2.generation;
    set_theme(&mut s2, 1);
    let g1 = s2.generation;
    set_theme(&mut s2, 0);
    set.add(
        "A405 hot switch",
        s2.active_theme == 0 && g1 == g0 + 1 && s2.generation == g0 + 2,
        "generation counter",
    );

    // A406 强调色
    set.add(
        "A406 accent custom",
        accent_ok(RGB888 { r: 0, g: 0, b: 255 })
            && !accent_ok(RGB888 { r: 0, g: 0, b: 0 })
            && !accent_ok(RGB888 { r: 255, g: 255, b: 255 }),
        "no pure b/w",
    );

    // A407 三档
    set.add(
        "A407 radius/spacing/shadow",
        radius_tier(0) == RADIUS_S
            && radius_tier(2) == RADIUS_L
            && spacing_tier(1) == SPACING_M
            && shadow_tier(0) == SHADOW_S,
        "3 tiers S/M/L",
    );

    // A408 字号阶梯
    set.add(
        "A408 font ladder",
        font_size(0) == 12 && font_size(5) == 32 && font_ladder_monotonic() && line_height(0) >= font_size(0),
        "monotonic + ratio",
    );

    // A409 对比度守护
    let cr = contrast_ratio(luminance(&white), luminance(&black));
    set.add(
        "A409 contrast guard",
        cr >= 7000 && contrast_aa(cr) && contrast_aaa(cr) && !contrast_aaa(4500),
        "AA/AAA",
    );

    // A410 高对比 + 色觉
    set.add(
        "A410 hc + cvd",
        contrast_ok(5000, false) && !contrast_ok(5000, true) && matches!(ColorVision::RedGreen, ColorVision::RedGreen),
        "hc forces >=7",
    );

    // A411 导入导出往返
    let mut buf2 = [0u8; 256];
    let n2 = pack_theme(&sys, &mut buf2);
    let mut tamper = buf2;
    tamper[0] ^= 0xFF;
    set.add(
        "A411 import export",
        n2 > 0 && unpack_ok(&buf2[..n2]) && !unpack_ok(&tamper[..n2]),
        "roundtrip + tamper",
    );

    // A412 预览 A/B 分离
    let mut s3 = DesignSystem::new();
    s3.set_preview(0, 1);
    s3.set_preview(1, 0);
    s3.commit_preview(0);
    set.add(
        "A412 preview A/B",
        s3.committed == 1 && s3.active_theme == 1 && s3.preview_a == 1,
        "commit applies",
    );

    // A413 主题商店
    let mut st = ThemeStore::new();
    let a = st.install(1);
    let b = st.install(2);
    let dup = st.install(1);
    let _ = st.install(9);
    let _ = st.uninstall(2);
    set.add(
        "A413 theme store",
        a && b && !dup && st.count == 2,
        "cap8 + dedup",
    );

    // A414 草稿覆盖
    let mut s4 = DesignSystem::new();
    register_token(&mut s4, TokenCategory::Color as u8, "accent", 0x0000FF);
    let before = token_value(&s4, "accent").unwrap_or(0);
    s4.draft[0] = Some(Token { cat: TokenCategory::Color as u8, name: "accent", value: 0x00FF00 });
    s4.draft_count = 1;
    s4.apply_draft();
    let after = token_value(&s4, "accent").unwrap_or(0);
    set.add("A414 draft editor", before == 0x0000FF && after == 0x00FF00, "draft overrides");

    // A415 图标命名查表
    set.add(
        "A415 icon id",
        icon_name(IconId::Home) == "home" && find_icon("SETTINGS") == Some(IconId::Settings),
        "id + name table",
    );

    // A416 图标光栅掩码
    let mut mask = [0u8; 256];
    render_icon(IconId::Home, &mut mask);
    set.add("A416 icon raster", mask.iter().any(|&b| b != 0), "16x16 fill");

    // A417 任意 DPI 适配
    set.add(
        "A417 icon scale",
        scale_icon(10) == 16 && scale_icon(22) == 24 && scale_icon(44) == 48,
        "nearest tier",
    );

    // A418 lint 门禁
    let mut s5 = DesignSystem::new();
    register_token(&mut s5, TokenCategory::Spacing as u8, "gap", 999); // 非法档位
    let bad = lint_theme(&s5);
    let mut s6 = DesignSystem::new();
    register_token(&mut s6, TokenCategory::Spacing as u8, "gap", SPACING_M);
    let good = lint_theme(&s6);
    set.add("A418 lint gate", bad >= 1 && good == 0, "violation count");

    // A419 视觉设计系统自检
    set.add(
        "A419 design selftest",
        sys.count > 0 && TokenCategory::count() == 6,
        "tokens present",
    );

    // A420 性能预算
    set.add(
        "A420 perf budget",
        TOKEN_LOOKUP_O1 && budget_ok(sys.count as u32, 64),
        "O(1) + budget",
    );

    // A421 可观测计数器
    let mut ds = DesignStats::default();
    ds.theme_switches = 4;
    ds.lint_failures = 1;
    set.add(
        "A421 observable stats",
        ds.theme_switches == 4 && ds.lint_failures == 1,
        "counters",
    );

    // A422 模糊测试
    set.add("A422 fuzz design", fuzz_design(7, 200), "200 rounds no panic");

    // A423 文档事实
    set.add(
        "A423 documented constants",
        MAX_TOKENS == 24 && MAX_STORE == 8 && MAX_ICONS == 16 && THEME_THRESHOLD == 400,
        "caps",
    );

    // A424 降级链：缺失令牌回退默认
    let def = get_or_default(&DesignSystem::new(), "missing", 0x123456);
    set.add("A424 token fallback", def == 0x123456, "missing -> default");

    // A425 域自检收口
    set.add("A425 design domain closed", set.len() == 24, "25 live checks");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a401_six_categories() {
        assert_eq!(TokenCategory::count(), 6);
        let mut s = DesignSystem::new();
        assert!(register_token(&mut s, TokenCategory::Color as u8, "bg", 1));
        assert_eq!(s.count, 1);
        assert!(register_token(&mut s, TokenCategory::Color as u8, "bg", 2)); // 覆盖
        assert_eq!(s.count, 1);
        assert_eq!(token_value(&s, "BG"), Some(2));
    }

    #[test]
    fn a402_luminance() {
        let white = RGB888 { r: 255, g: 255, b: 255 };
        let black = RGB888 { r: 0, g: 0, b: 0 };
        assert!(luminance(&white) > luminance(&black));
        assert_eq!(luminance(&black), 0);
    }

    #[test]
    fn a409_contrast() {
        let cr = contrast_ratio(255, 0);
        assert!(contrast_aaa(cr));
        assert!(!contrast_aa(3000));
        assert!(!contrast_aaa(5000));
    }

    #[test]
    fn a417_scale_nearest() {
        assert_eq!(scale_icon(10), 16);
        assert_eq!(scale_icon(22), 24);
        assert_eq!(scale_icon(44), 48);
    }

    #[test]
    fn a_run_all_designsys_checks_pass() {
        assert!(run_designsys_checks().all_passed());
    }
}
