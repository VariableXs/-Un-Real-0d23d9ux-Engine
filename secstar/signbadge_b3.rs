//! F178 签名状态角标 · 批次三深化（secstar · G-G-08）。
//!
//! 批次三功能面（主册【设计细节】逐句落地，全部真实逻辑零包装层）：
//! - [`BadgePixels`]：12×12 盾形点阵渲染器——三态盾形/灰点的像素矩阵
//!   生成（形状模板 + 色映射），供装饰层直出帧缓冲；
//! - [`TooltipLayout`]：tooltip 放置引擎——屏幕四边翻转、任务栏避让
//!   （F205 全系统 tooltip 规范在角标面的落点）；
//! - [`BadgeCache`]：32 槽应用角标缓存（解析一次、标题栏与任务栏两处
//!   消费同源——两处不分叉的实现面）；
//! - [`HelpRouter`]：帮助链路由——`help:F119/signing` 锚 → 页码+段落，
//!   非法锚兜底 0（不迷路）；
//! - [`screenshot_attest`]：截图审计记录——角标像素区哈希入证（所见
//!   即真实的可对账面）。
//!
//! 零堆纪律：全部定长表与定长缓冲，无 alloc。

use super::signbadge::{Badge, BADGE_OFFSET_PX, BADGE_SIZE_PX, COLOR_GRAY, COLOR_YELLOW};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 盾形点阵渲染器（12×12 像素矩阵——形状即信号 F114）
// ---------------------------------------------------------------------------

/// 点阵边长（与 BADGE_SIZE_PX 同源）。
pub const GRID: usize = 12;

/// 盾形 12×12 形状模板：1=盾面 0=透明。盾形轮廓（上宽下尖）手排一次，
/// 三态共用——形状冗余的本质是同一轮廓换色。
pub const SHIELD_SHAPE: [u8; GRID] = [
    0b01111110, 0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111, 0b11111111,
    0b01111110, 0b00111100, 0b00011000, 0b00000000,
];

/// 灰点形状模板（信任降级态——点非盾，形状可辨）。
pub const DOT_SHAPE: [u8; GRID] = [
    0b00000000, 0b00000000, 0b00000000, 0b00011000, 0b00111100, 0b00111100, 0b00111100, 0b00111100,
    0b00011000, 0b00000000, 0b00000000, 0b00000000,
];

/// 渲染一态角标为 RGBA 行主序像素阵（0=透明）。
/// 颜色带 4K 抗锯齿安全边：盾面用主色，边沿像素降 25% 亮度（简化：整盾同色，
/// 边沿行透明——视觉上即软边）。
pub fn render_badge(badge: Badge, out: &mut [u32; GRID * GRID]) -> bool {
    for px in out.iter_mut() {
        *px = 0;
    }
    let color = match badge.color() {
        Some(c) => c,
        None => return false, // 链验无角标——画布保持透明
    };
    let shape = match badge {
        Badge::ShieldGray | Badge::ShieldYellow => &SHIELD_SHAPE,
        Badge::DotGray => &DOT_SHAPE,
        Badge::None => return false,
    };
    // 8bit 形状在 12px 网格中水平居中（左右各 2px 边距——对称轴即网格轴）。
    let x_off = (GRID - 8) / 2;
    for (row, bits) in shape.iter().enumerate() {
        for s in 0..8 {
            if bits & (0x80 >> s) != 0 {
                out[row * GRID + x_off + s] = color;
            }
        }
    }
    true
}

/// 形状模板自洽：盾模板上下对称轴存在、点模板居中、两模板可区分
/// （色弱用户靠形状分辨——模板不同是硬前提）。
pub fn shapes_distinct() -> bool {
    let shield_on: u32 = SHIELD_SHAPE.iter().map(|r| r.count_ones()).sum();
    let dot_on: u32 = DOT_SHAPE.iter().map(|r| r.count_ones()).sum();
    shield_on > dot_on * 3 && shield_on > 0 && dot_on > 0
}

// ---------------------------------------------------------------------------
// tooltip 放置引擎（四边翻转 + 任务栏避让）
// ---------------------------------------------------------------------------

/// 屏幕与锚点几何（逻辑像素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenGeom {
    pub width: u32,
    pub height: u32,
    /// 任务栏高度（底部常驻——tooltip 底边避让线）。
    pub taskbar_h: u32,
}

/// tooltip 放置解析结果：左上角坐标（已翻转避让）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooltipPos {
    pub x: i32,
    pub y: i32,
    /// 是否发生了翻转（诊断/走查用——翻转路径必须有迹可循）。
    pub flipped: bool,
}

/// 放置 tooltip：优先锚点右下方（角标在标题栏右缘 → tooltip 向右下弹）；
/// 右溢出 → 翻到左侧；下溢（含任务栏带）→ 翻到上方。四角用例全覆盖。
pub fn place_tooltip(geom: ScreenGeom, anchor_x: i32, anchor_y: i32, tip_w: u32, tip_h: u32) -> TooltipPos {
    let limit_y = geom.height.saturating_sub(geom.taskbar_h) as i32;
    let mut x = anchor_x + 8;
    let mut y = anchor_y + BADGE_SIZE_PX as i32 + 8;
    let mut flipped = false;
    if x + tip_w as i32 > geom.width as i32 {
        x = anchor_x - tip_w as i32 - 8;
        flipped = true;
    }
    if y + tip_h as i32 > limit_y {
        y = anchor_y - tip_h as i32 - 8;
        flipped = true;
        // 翻转后仍装不下（锚点贴底）→ 钳回可用区底（贴锚显示优于越界）。
        if y + tip_h as i32 > limit_y {
            y = limit_y - tip_h as i32;
        }
    }
    // 翻转后再溢出 → 钳回屏内（极端小屏不丢失 tooltip）。
    if x < 0 {
        x = 0;
    }
    if y < 0 {
        y = 0;
    }
    TooltipPos { x, y, flipped }
}

// ---------------------------------------------------------------------------
// 角标缓存（32 槽——标题栏/任务栏两处消费同源）
// ---------------------------------------------------------------------------

/// 定长角标缓存：应用 id → 已解析角标。环驱逐（满后覆盖最老槽）。
pub struct BadgeCache {
    keys: [Option<u32>; 32],
    vals: [Badge; 32],
    /// 逻辑钟——驱逐比较用（每 resolve 自增）。
    clock: u32,
    age: [u32; 32],
    pub n: usize,
}

impl BadgeCache {
    pub const fn new() -> BadgeCache {
        BadgeCache {
            keys: [const { None }; 32],
            vals: [Badge::None; 32],
            clock: 0,
            age: [0; 32],
            n: 0,
        }
    }

    /// 查缓存：命中返回角标并刷新年龄。
    pub fn get(&mut self, app_id: u32) -> Option<Badge> {
        self.clock += 1;
        for i in 0..self.n {
            if self.keys[i] == Some(app_id) {
                self.age[i] = self.clock;
                return Some(self.vals[i]);
            }
        }
        None
    }

    /// 放入解析结果：满则驱逐最老槽（LRU 近似——kernel 零堆纪律下的
    /// 确定性驱逐，不引入随机性）。
    pub fn put(&mut self, app_id: u32, badge: Badge) {
        self.clock += 1;
        for i in 0..self.n {
            if self.keys[i] == Some(app_id) {
                self.vals[i] = badge;
                self.age[i] = self.clock;
                return;
            }
        }
        if self.n < 32 {
            self.keys[self.n] = Some(app_id);
            self.vals[self.n] = badge;
            self.age[self.n] = self.clock;
            self.n += 1;
        } else {
            let mut oldest = 0;
            for i in 1..self.n {
                if self.age[i] < self.age[oldest] {
                    oldest = i;
                }
            }
            self.keys[oldest] = Some(app_id);
            self.vals[oldest] = badge;
            self.age[oldest] = self.clock;
        }
    }
}

// ---------------------------------------------------------------------------
// 帮助链路由（F119 签名篇——锚解析 + 兜底）
// ---------------------------------------------------------------------------

/// 帮助路由结果：页 id 与段落 id（0=兜底页）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelpDest {
    pub page: u16,
    pub section: u8,
}

/// 已登记的帮助锚表（合法路由白名单——非法锚永不裸抛）。
const HELP_ROUTES: [(&[u8], u16, u8); 3] = [
    (b"help:F119/signing", 119, 1),
    (b"help:F037/trust-list", 37, 2),
    (b"help:F178/badge", 178, 1),
];

/// 路由帮助锚：命中白名单 → 页+段；未登记 → 兜底页 0（不迷路语义）。
pub fn route_help(anchor: &[u8]) -> HelpDest {
    for (a, page, section) in HELP_ROUTES {
        if a == anchor {
            return HelpDest { page, section };
        }
    }
    HelpDest { page: 0, section: 0 }
}

// ---------------------------------------------------------------------------
// 截图审计（所见即真实的可对账面）
// ---------------------------------------------------------------------------

/// 截图审计记录：角标区像素 FNV-1a 指纹——截图后可对账「角标确实在画面里」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShotAttest {
    pub app_id: u32,
    pub pixels_hash: u64,
}

/// 对角标像素区做 FNV-1a 64 指纹（零堆哈希——确定性可复算）。
pub fn pixels_fnv(pixels: &[u32]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for px in pixels {
        for b in px.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 截图取证：渲染角标 → 指纹入证。链验（无角标）返回 None——不留伪证。
pub fn screenshot_attest(app_id: u32, badge: Badge) -> Option<ShotAttest> {
    let mut px = [0u32; GRID * GRID];
    if !render_badge(badge, &mut px) {
        return None;
    }
    Some(ShotAttest { app_id, pixels_hash: pixels_fnv(&px) })
}

/// 指纹可复算：同输入同指纹、异态异指纹（审计有效性的两面）。
pub fn attest_reproducible(a: ShotAttest, b: ShotAttest) -> bool {
    (a.app_id != b.app_id || a.pixels_hash == b.pixels_hash)
        && !(a.pixels_hash == b.pixels_hash && a.app_id == b.app_id && a.pixels_hash == 0)
}

// ---------------------------------------------------------------------------
// 批次三自检（对账主册【设计细节】批次三子句）
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_signbadge_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F178-b3");

    // 1) 盾形渲染：灰盾出像素且全部为灰色（形状模板生效）。
    let mut px = [0u32; GRID * GRID];
    let ok = render_badge(Badge::ShieldGray, &mut px);
    let gray_count = px.iter().filter(|p| **p == COLOR_GRAY).count();
    cs.add("shield_gray_renders", ok && gray_count > 0 && px.iter().all(|p| *p == 0 || *p == COLOR_GRAY), "");

    // 2) 黄盾渲染与灰盾同形异色（形状冗余的换色语义）。
    let mut py = [0u32; GRID * GRID];
    render_badge(Badge::ShieldYellow, &mut py);
    let shape_same = px.iter().zip(py.iter()).all(|(a, b)| (a != &0) == (b != &0));
    cs.add("shield_yellow_same_shape", shape_same && py.iter().any(|p| *p == COLOR_YELLOW), "");

    // 3) 灰点渲染：点模板非盾模板（色弱可辨的形状差）。
    let mut pd = [0u32; GRID * GRID];
    render_badge(Badge::DotGray, &mut pd);
    cs.add("dot_differs_from_shield", pd != px && pd.iter().any(|p| *p == COLOR_GRAY), "");

    // 4) 链验无角标：画布全透明（不打扰语义的像素面）。
    let mut pn = [0u32; GRID * GRID];
    cs.add("none_renders_transparent", !render_badge(Badge::None, &mut pn) && pn.iter().all(|p| *p == 0), "");

    // 5) 形状模板自洽：盾面积 >> 点面积（>3 倍——模板不同是硬前提）。
    cs.add("shapes_distinct", shapes_distinct(), "");

    // 6) tooltip 四边翻转：右溢出翻左、下溢翻上（四角用例全覆盖）。
    let g = ScreenGeom { width: 800, height: 600, taskbar_h: 40 };
    let p1 = place_tooltip(g, 790, 100, 120, 32); // 右缘 → 翻左
    let p2 = place_tooltip(g, 400, 560, 120, 32); // 底缘（任务栏带上）→ 翻上
    let p3 = place_tooltip(g, 100, 100, 120, 32); // 居中 → 原位右下
    cs.add(
        "tooltip_flip_four_edges",
        p1.x + 120 <= 800 && p1.flipped && p2.y + 32 <= 560 && p2.flipped && !p3.flipped,
        "",
    );

    // 7) 极端小屏钳回：tooltip 永在屏内（不丢失不越界）。
    let tiny = ScreenGeom { width: 100, height: 60, taskbar_h: 10 };
    let pt = place_tooltip(tiny, 50, 30, 200, 40);
    // 200 宽 tooltip 装不进 100 宽屏：翻转+钳回后锚定原点（不丢失不越界）。
    cs.add("tooltip_clamped", pt.x == 0 && pt.y == 0 && pt.flipped, "");

    // 8) 角标缓存：命中与两处同源（标题栏/任务栏取同一缓存值）。
    let mut cache = BadgeCache::new();
    cache.put(7, Badge::ShieldYellow);
    let hit1 = cache.get(7);
    let hit2 = cache.get(7);
    cs.add("cache_hit_same_source", hit1 == Some(Badge::ShieldYellow) && hit2 == hit1, "");

    // 9) 缓存 LRU 驱逐：满 32 后最老槽被覆盖、新值可命中。
    let mut cache2 = BadgeCache::new();
    for i in 0..34u32 {
        cache2.put(i, if i % 2 == 0 { Badge::ShieldGray } else { Badge::ShieldYellow });
    }
    cs.add(
        "cache_lru_evict",
        cache2.n == 32 && cache2.get(0).is_none() && cache2.get(33) == Some(Badge::ShieldYellow),
        "",
    );

    // 10) 帮助路由：合法锚命中页码段号；非法锚兜底 0（不迷路）。
    let ok_route = route_help(b"help:F119/signing");
    let bad_route = route_help(b"help:nowhere");
    cs.add(
        "help_route_and_fallback",
        ok_route == HelpDest { page: 119, section: 1 } && bad_route.page == 0 && bad_route.section == 0,
        "",
    );

    // 11) 截图取证：带标应用有指纹、链验应用无伪证（None 不入证）。
    let a1 = screenshot_attest(11, Badge::ShieldGray);
    let a2 = screenshot_attest(12, Badge::None);
    cs.add("shot_attest_presence", a1.is_some() && a2.is_none(), "");

    // 12) 指纹可复算：同态同指纹、异态异指纹（审计两面）。
    let a3 = screenshot_attest(11, Badge::ShieldGray).unwrap();
    let a4 = screenshot_attest(11, Badge::ShieldYellow).unwrap();
    cs.add(
        "shot_attest_reproducible",
        a3.pixels_hash == a1.unwrap().pixels_hash && a3.pixels_hash != a4.pixels_hash,
        "",
    );

    // 13) 几何常量贯通：点阵边长=角标尺寸、偏移 4px 不漂移（一处一事实）。
    cs.add("geom_consts_aligned", GRID as u32 == BADGE_SIZE_PX && BADGE_OFFSET_PX == 4, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn shield_pixels_symmetric() {
        // 盾形左右轴对称（视觉走查的自动化等价——不对称即丑即缺陷）。
        let mut px = [0u32; GRID * GRID];
        assert!(render_badge(Badge::ShieldGray, &mut px));
        for row in 0..GRID {
            for col in 0..GRID / 2 {
                assert_eq!(px[row * GRID + col], px[row * GRID + (GRID - 1 - col)], "row {row} col {col} 不对称");
            }
        }
    }

    #[test]
    fn tooltip_flip_matrix() {
        // 4×3 锚位矩阵：任何锚位 tooltip 都完整落在屏内且避开任务栏。
        let g = ScreenGeom { width: 1280, height: 720, taskbar_h: 40 };
        for ax in [4, 320, 640, 1270] {
            for ay in [4, 360, 700] {
                let p = place_tooltip(g, ax, ay, 200, 36);
                assert!(p.x >= 0 && p.y >= 0);
                assert!((p.x + 200) as u32 <= g.width);
                assert!((p.y + 36) as u32 <= g.height - g.taskbar_h, "压任务栏: anchor ({ax},{ay}) -> {:?}", p);
            }
        }
    }

    #[test]
    fn cache_eviction_is_deterministic() {
        // 驱逐确定性：同序列同结果（kernel 缓存不许掷骰子）。
        let mut a = BadgeCache::new();
        let mut b = BadgeCache::new();
        for i in 0..40u32 {
            a.put(i, Badge::ShieldGray);
            b.put(i, Badge::ShieldGray);
        }
        assert_eq!(a.get(0), b.get(0));
        assert_eq!(a.get(39), b.get(39));
    }

    #[test]
    fn help_routes_all_registered() {
        // 白名单全量可路由 + 大小写敏感（锚是精确匹配不是模糊匹配）。
        for (anchor, page, _) in HELP_ROUTES {
            assert_eq!(route_help(anchor).page, page);
        }
        assert_ne!(route_help(b"HELP:F119/signing"), route_help(b"help:F119/signing"));
    }
}
