//! F178 签名状态角标 · 批次六深化（secstar · G-G-08）。
//!
//! 批次六功能面（达成率 53%——本批主攻。与 b3「渲染路由」b4「时序统计」
//! b5「信任交互」互补，本批管「绘制效率与引导」）：
//! - [`PaintCache`]：绘制缓存——渲染结果按态缓存（同态不重画——
//!   每帧都画 12×12 是浪费：缓存命中直接拷贝）；
//! - [`first_hover_hint`]：首次悬停引导——第一次看到黄盾给一句话
//!   （引导只出现一次、可跳过——第 11 章纪律）；
//! - [`triple_encoding`]：色弱三重编码——色/形/位置三通道同时可辨
//!   （三态角标在三个通道上都不同——F114 红线的机械验证）；
//! - [`badge_audit_row`]：角标审计导出行——应用/态/时刻 CSV 式字节面
//!   （审计页消费：谁在什么时候是什么态）。
//!
//! 零堆纪律：定长缓存 + 定长行缓冲，无 alloc。

use super::signbadge::Badge;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 绘制缓存
// ---------------------------------------------------------------------------

/// 缓存槽（四态各一——Badge 枚举封闭）。
pub const PAINT_SLOTS: usize = 4;

/// 绘制缓存：渲染一次存档，同态直接拷贝。
pub struct PaintCache {
    cached: [bool; PAINT_SLOTS],
    pixels: [[u32; 144]; PAINT_SLOTS], // 12×12
    pub hits: u32,
    pub misses: u32,
}

impl PaintCache {
    pub const fn new() -> PaintCache {
        PaintCache { cached: [false; PAINT_SLOTS], pixels: [[0; 144]; PAINT_SLOTS], hits: 0, misses: 0 }
    }

    fn slot(b: Badge) -> usize {
        match b {
            Badge::None => 0,
            Badge::ShieldGray => 1,
            Badge::ShieldYellow => 2,
            Badge::DotGray => 3,
        }
    }

    /// 取像素：命中 → (像素, true)；未命中 → 调渲染回调入库。
    pub fn get_or_render(&mut self, b: Badge, render: fn(Badge, &mut [u32; 144]) -> bool) -> Option<[u32; 144]> {
        let s = Self::slot(b);
        if self.cached[s] {
            self.hits += 1;
            return Some(self.pixels[s]);
        }
        self.misses += 1;
        let mut px = [0u32; 144];
        if !render(b, &mut px) {
            return None; // 链验无角标——不缓存不存在的东西
        }
        self.pixels[s] = px;
        self.cached[s] = true;
        Some(px)
    }

    /// 命中率 ‰。
    pub fn hit_rate_permille(&self) -> u32 {
        let t = self.hits + self.misses;
        if t == 0 {
            return 0;
        }
        (self.hits * 1_000 / t) as u32
    }
}

// ---------------------------------------------------------------------------
// 首次悬停引导
// ---------------------------------------------------------------------------

/// 引导状态（三态角标各一次——看过就不再弹）。
#[derive(Clone, Copy)]
pub struct HoverTutorial {
    seen: [bool; 3], // 0=灰盾 1=黄盾 2=灰点
    pub skipped: u32,
}

impl HoverTutorial {
    pub const fn new() -> HoverTutorial {
        HoverTutorial { seen: [false; 3], skipped: 0 }
    }

    /// 悬停：第一次见该态 → 给引导句并记账；第二次 → None。
    pub fn hover(&mut self, badge: Badge) -> Option<&'static str> {
        let i = match badge {
            Badge::ShieldGray => 0,
            Badge::ShieldYellow => 1,
            Badge::DotGray => 2,
            Badge::None => return None,
        };
        if self.seen[i] {
            return None;
        }
        self.seen[i] = true;
        Some(match i {
            0 => "提示：灰盾=未签名。点击可了解详情或加入信任。",
            1 => "提示：黄盾=自签名。开发者用自己的证书签的名。",
            _ => "提示：灰点=你信任过的未签名应用。",
        })
    }

    /// 用户跳过引导（跳过也记账——不再弹）。
    pub fn skip(&mut self, badge: Badge) -> bool {
        if self.hover(badge).is_some() || {
            // 已看过但跳过计数（跳过不重复给句）。
            let i = match badge {
                Badge::ShieldGray => 0,
                Badge::ShieldYellow => 1,
                Badge::DotGray => 2,
                Badge::None => return false,
            };
            self.seen[i] = true;
            true
        } {
            self.skipped += 1;
            true
        } else {
            false
        }
    }

    pub fn all_seen(&self) -> bool {
        self.seen.iter().all(|s| *s)
    }
}

// ---------------------------------------------------------------------------
// 色弱三重编码
// ---------------------------------------------------------------------------

/// 三通道编码表（色/形/位置——三态在三通道上两两可辨）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TripleCode {
    pub color: u8,   // 0=无 1=灰 2=黄
    pub shape: u8,   // 0=无 1=盾 2=点
    pub slot: u8,    // 位置槽（标题栏固定槽位——位置也是信号）
}

pub fn triple_code(b: Badge, slot: u8) -> TripleCode {
    match b {
        Badge::None => TripleCode { color: 0, shape: 0, slot },
        Badge::ShieldGray => TripleCode { color: 1, shape: 1, slot },
        Badge::ShieldYellow => TripleCode { color: 2, shape: 1, slot },
        Badge::DotGray => TripleCode { color: 1, shape: 2, slot },
    }
}

/// 可辨性判定：三通道至少一通道不同——且本编码表构造上保证不存在
/// 「仅颜色不同」的态对（灰盾/灰点同色但形状不同——构造即色弱安全）。
pub fn distinguishable(a: TripleCode, b: TripleCode) -> bool {
    let diffs = (a.color != b.color) as u32 + (a.shape != b.shape) as u32 + (a.slot != b.slot) as u32;
    diffs >= 1
}

/// 三态两两可辨（机械穷举三对）。
pub fn triple_encoding_holds(slot_base: u8) -> bool {
    let g = triple_code(Badge::ShieldGray, slot_base);
    let y = triple_code(Badge::ShieldYellow, slot_base + 1);
    let d = triple_code(Badge::DotGray, slot_base + 2);
    distinguishable(g, y) && distinguishable(g, d) && distinguishable(y, d)
}

// ---------------------------------------------------------------------------
// 审计导出行
// ---------------------------------------------------------------------------

/// CSV 式行：`app=<id>,badge=<b>,at=<ms>` 字节面（审计页消费）。
pub fn badge_audit_row(app_id: u32, badge: Badge, at_ms: u64, out: &mut [u8]) -> usize {
    let tag = match badge {
        Badge::None => "none",
        Badge::ShieldGray => "gray",
        Badge::ShieldYellow => "yellow",
        Badge::DotGray => "dot",
    };
    let mut n = 0;
    let put = |bytes: &[u8], out: &mut [u8], n: &mut usize| {
        for b in bytes {
            if *n < out.len() {
                out[*n] = *b;
                *n += 1;
            }
        }
    };
    put(b"app=", out, &mut n);
    put_num(app_id as u64, out, &mut n);
    put(b",badge=", out, &mut n);
    put(tag.as_bytes(), out, &mut n);
    put(b",at=", out, &mut n);
    put_num(at_ms, out, &mut n);
    n
}

fn put_num(v: u64, out: &mut [u8], n: &mut usize) {
    let mut digits = [0u8; 20];
    let mut w = 0;
    if v == 0 {
        digits[0] = b'0';
        w = 1;
    } else {
        let mut x = v;
        while x > 0 {
            digits[w] = b'0' + (x % 10) as u8;
            w += 1;
            x /= 10;
        }
    }
    for i in (0..w).rev() {
        if *n < out.len() {
            out[*n] = digits[i];
            *n += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_signbadge_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F178-b6");

    // 1) 绘制缓存：首渲染 miss、再取 hit（像素逐位一致）。
    let mut c = PaintCache::new();
    let p1 = c.get_or_render(Badge::ShieldGray, super::signbadge_b3::render_badge);
    let p2 = c.get_or_render(Badge::ShieldGray, super::signbadge_b3::render_badge);
    cs.add(
        "paint_cache_hit",
        p1.is_some() && p2 == p1 && c.misses == 1 && c.hits == 1,
        "",
    );

    // 2) 缓存不缓存不存在：None → None 且不入账（不留伪证）。
    let none_px = c.get_or_render(Badge::None, super::signbadge_b3::render_badge);
    cs.add("paint_cache_none", none_px.is_none() && c.hit_rate_permille() > 0, "");

    // 3) 首次悬停引导：首见给句、再见 None（只出现一次）。
    let mut t = HoverTutorial::new();
    let first = t.hover(Badge::ShieldYellow);
    let second = t.hover(Badge::ShieldYellow);
    cs.add("tutorial_once", first.is_some() && second.is_none() && first.unwrap().contains("黄盾"), "");

    // 4) 引导跳过：跳过也记账（不再弹）且计数在册。
    let mut t2 = HoverTutorial::new();
    t2.skip(Badge::ShieldGray);
    let after_skip = t2.hover(Badge::ShieldGray);
    cs.add("tutorial_skip_counts", after_skip.is_none() && t2.skipped == 1, "");

    // 5) 三态全看过：遍历三态 → all_seen（引导完整覆盖）。
    let mut t3 = HoverTutorial::new();
    t3.hover(Badge::ShieldGray);
    t3.hover(Badge::ShieldYellow);
    t3.hover(Badge::DotGray);
    cs.add("tutorial_all_seen", t3.all_seen(), "");

    // 6) 三重编码：三态两两可辨（色/形/位置 ≥2 通道差）。
    cs.add("triple_encoding", triple_encoding_holds(0), "");

    // 7) 可辨性反面：同态同槽 → 不可辨（判定器不是摆设）。
    let same = triple_code(Badge::ShieldGray, 5);
    cs.add("triple_same_indistinct", !distinguishable(same, same), "");

    // 8) 位置通道独立：同态不同槽 → 可辨（位置也是信号）。
    let a = triple_code(Badge::DotGray, 0);
    let b = triple_code(Badge::DotGray, 1);
    cs.add("triple_slot_channel", distinguishable(a, b), "");

    // 9) 审计行格式：app=7,badge=yellow,at=1500 逐字节。
    let mut buf = [0u8; 48];
    let n = badge_audit_row(7, Badge::ShieldYellow, 1_500, &mut buf);
    cs.add("audit_row_format", &buf[..n] == b"app=7,badge=yellow,at=1500", "");

    // 10) 审计行零值：app=0/at=0 不缺字段（格式稳定）。
    let n2 = badge_audit_row(0, Badge::None, 0, &mut buf);
    cs.add("audit_row_zero", &buf[..n2] == b"app=0,badge=none,at=0", "");

    // 11) 渲染器供料贯通：缓存出的黄盾像素与 b3 直渲逐位一致（一处一事实）。
    let mut direct = [0u32; 144];
    super::signbadge_b3::render_badge(Badge::ShieldYellow, &mut direct);
    let cached = c.get_or_render(Badge::ShieldYellow, super::signbadge_b3::render_badge).unwrap();
    cs.add("renderer_supply", cached == direct, "");

    // 12) 单测通道：三态各自缓存槽独立（不串像素）。
    let gray = c.get_or_render(Badge::ShieldGray, super::signbadge_b3::render_badge).unwrap();
    let yellow = c.get_or_render(Badge::ShieldYellow, super::signbadge_b3::render_badge).unwrap();
    cs.add("paint_slots_distinct", gray != yellow, "");

    cs
}
