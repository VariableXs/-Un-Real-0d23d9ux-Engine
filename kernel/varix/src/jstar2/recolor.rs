//! F626 指针颜色重染 · 完整设计（STAR I 主册 J-C 组）。
//!
//! **判据（主册原文）**：三滑杆效果实测；强调色一键染色对拍；非破坏
//! 副本纪律；动画帧同步（ΔE<1 判据）；存档与回退；与 F629 分工（手动
//! 染 vs 令牌派生）边界用例。
//!
//! **重染语义**：
//! - 三滑杆 = 色相偏移（-180..+180°）/饱和度缩放（0..2000‰）/明度缩放
//!   （0..2000‰）——逐像素 HSL 域定点变换，纯函数（同入同出）；
//! - 强调色一键 = 把方案主色（不透明像素 HSL 众数色相）对齐到强调色
//!   色相：全帧统一 hue 偏移，饱和/明度保持像素个性——「染成一家人」
//!   不是「染成一块板」；
//! - 非破坏：重染产出副本（名字「«原名»·重染」+ origin=Recolored{原
//!   方案名}），原件分毫未动；存档与回退 = 库房删副本即回原件；
//! - 动画帧同步：变换是逐像素纯函数 → 原本同色的帧间像素重染后仍同色
//!   ——ΔE76 < 1（×100 定点 < 100）逐帧对拍验证（判据数值原文）；
//! - 与 F629 边界：F626 是用户手动染**任意**方案（origin=Recolored），
//!   F629 是系统按**令牌**派生（origin=Derived）——两条路都通向方案库
//!   （F628），边界用例验证两 origin 互不冒认。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{delta_e76_x100, CursorFrame, CursorSchemeModel, OriginKind, PointerState, Rgb};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 变换模型
// ---------------------------------------------------------------------------

/// 三滑杆参数（千分位定点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecolorParams {
    /// 色相偏移（度，-180..180）。
    pub hue_shift_deg: i32,
    /// 饱和度缩放（‰，0..2000；1000 = 不变）。
    pub sat_scale_m: i64,
    /// 明度缩放（‰，0..2000；1000 = 不变）。
    pub light_scale_m: i64,
}

impl Default for RecolorParams {
    fn default() -> Self {
        RecolorParams { hue_shift_deg: 0, sat_scale_m: 1000, light_scale_m: 1000 }
    }
}

impl RecolorParams {
    pub fn is_identity(&self) -> bool {
        self.hue_shift_deg == 0 && self.sat_scale_m == 1000 && self.light_scale_m == 1000
    }
}

/// 单像素 HSL 域变换（纯函数）。
pub fn transform_pixel(p: [u8; 4], prm: &RecolorParams) -> [u8; 4] {
    if p[3] == 0 {
        return p; // 全透明像素不参与色彩域（诚实：染不染都看不见）
    }
    let c = Rgb::new(p[0], p[1], p[2]);
    let (h, s, l) = c.to_hsl();
    let h2 = ((h as i64 + prm.hue_shift_deg as i64).rem_euclid(360)) as u32;
    let s2 = (s * prm.sat_scale_m / 1000).clamp(0, 1000);
    let l2 = (l * prm.light_scale_m / 1000).clamp(0, 1000);
    let out = Rgb::from_hsl(h2, s2, l2);
    [out.r, out.g, out.b, p[3]]
}

/// 整帧变换。
pub fn transform_frame(f: &CursorFrame, prm: &RecolorParams) -> CursorFrame {
    let mut px = f.px.clone();
    for chunk in px.chunks_exact_mut(4) {
        let t = transform_pixel([chunk[0], chunk[1], chunk[2], chunk[3]], prm);
        chunk[0] = t[0];
        chunk[1] = t[1];
        chunk[2] = t[2];
        chunk[3] = t[3];
    }
    CursorFrame { w: f.w, h: f.h, hot_x: f.hot_x, hot_y: f.hot_y, delay_ms: f.delay_ms, px }
}

/// 方案主色（不透明像素色相众数，30° 桶直方图）→ 返回主桶中心色相。
/// 无不透明像素时返回 None（一键染色诚实降级为恒等）。
pub fn dominant_hue(m: &CursorSchemeModel) -> Option<i64> {
    let mut hist = [0u64; 12];
    let mut total = 0u64;
    for e in &m.entries {
        for f in &e.frames {
            for chunk in f.px.chunks_exact(4) {
                if chunk[3] < 128 {
                    continue;
                }
                let (h, s, _l) = Rgb::new(chunk[0], chunk[1], chunk[2]).to_hsl();
                if s < 100 {
                    continue; // 近灰像素不参与色相众数（无「主色」语义）
                }
                hist[(h as usize / 30).min(11)] += 1;
                total += 1;
            }
        }
    }
    if total == 0 {
        return None;
    }
    let best = hist
        .iter()
        .enumerate()
        .max_by_key(|(_, n)| **n)
        .map(|(i, _)| i as i64 * 30 + 15)
        .unwrap_or(0);
    Some(best % 360)
}

/// 强调色一键染色参数：hue 偏移 = 强调色色相 − 主色色相（取最短弧）。
pub fn accent_dye_params(m: &CursorSchemeModel, accent: Rgb) -> RecolorParams {
    let Some(dominant) = dominant_hue(m) else {
        return RecolorParams::default();
    };
    let (ah, _, _) = accent.to_hsl();
    let mut shift = ah as i64 - dominant;
    if shift > 180 {
        shift -= 360;
    }
    if shift < -180 {
        shift += 360;
    }
    RecolorParams { hue_shift_deg: shift as i32, sat_scale_m: 1000, light_scale_m: 1000 }
}

// ---------------------------------------------------------------------------
// v2 深化：预设面板 / 批量重染 / 色相直方图 / 参数钳制
// ---------------------------------------------------------------------------

/// 色相直方图（12 桶 × 30°，桶序 = 0..360；不透明且近彩像素计数——
/// 方案详情页「色相分布」的数据面，与 `dominant_hue` 同一采样口径）。
pub fn hue_histogram(m: &CursorSchemeModel) -> [u64; 12] {
    let mut hist = [0u64; 12];
    for e in &m.entries {
        for f in &e.frames {
            for chunk in f.px.chunks_exact(4) {
                if chunk[3] < 128 {
                    continue;
                }
                let (h, s, _l) = Rgb::new(chunk[0], chunk[1], chunk[2]).to_hsl();
                if s < 100 {
                    continue; // 近灰像素不参与（与 dominant_hue 同口径）
                }
                hist[(h as usize / 30).min(11)] += 1;
            }
        }
    }
    hist
}

/// 预设面板（滑杆组合的命名包装——「暖色/冷色/去饱和/黑白/原味」，
/// 预设即判据的参数化：每格参数有名字、可复现、可微调起步）。
pub const RECOLOR_PRESETS: [(&str, RecolorParams); 5] = [
    ("原味", RecolorParams { hue_shift_deg: 0, sat_scale_m: 1000, light_scale_m: 1000 }),
    ("暖色", RecolorParams { hue_shift_deg: -25, sat_scale_m: 1100, light_scale_m: 1050 }),
    ("冷色", RecolorParams { hue_shift_deg: 160, sat_scale_m: 1050, light_scale_m: 1000 }),
    ("去饱和", RecolorParams { hue_shift_deg: 0, sat_scale_m: 400, light_scale_m: 1000 }),
    ("黑白", RecolorParams { hue_shift_deg: 0, sat_scale_m: 0, light_scale_m: 1100 }),
];

/// 批量重染：一次产出全部预设的副本（预览面板一次出图——用户不用
/// 手动切五次滑杆；命名「原名·预设名·重染」如实标注来源预设）。
pub fn batch_recolor(m: &CursorSchemeModel) -> Vec<RecolorOutcome> {
    RECOLOR_PRESETS
        .iter()
        .map(|(name, prm)| {
            let mut out = recolor(m, prm);
            out.copy.name = alloc::format!("{}·{}·重染", m.name, name);
            out
        })
        .collect()
}

impl RecolorParams {
    /// 参数钳制（滑杆脏数据/外部包参数的诚实归位：色相取最短环、
    /// 缩放钳 0..2000‰——不崩溃不产生未定义变换）。
    pub fn clamped(&self) -> RecolorParams {
        RecolorParams {
            hue_shift_deg: self.hue_shift_deg.clamp(-180, 180),
            sat_scale_m: self.sat_scale_m.clamp(0, 2000),
            light_scale_m: self.light_scale_m.clamp(0, 2000),
        }
    }
}

// ---------------------------------------------------------------------------
// 重染主入口（非破坏副本）
// ---------------------------------------------------------------------------

/// 重染结果（副本 + 边界标记）。
pub struct RecolorOutcome {
    pub copy: CursorSchemeModel,
}

/// 执行重染：产出副本（原件分毫未动——由调用方持有原件保证，本函数
/// 借用不修改）。
///
/// 命名纪律：`«原名»·重染`；origin=Recolored{原名}——与 F629 的
/// Derived、F630 的 Imported 互斥可辨（分工边界的机器面）。
pub fn recolor(m: &CursorSchemeModel, prm: &RecolorParams) -> RecolorOutcome {
    let mut copy = CursorSchemeModel::empty(
        &alloc::format!("{}·重染", m.name),
        OriginKind::Recolored(m.name.clone()),
    );
    let identity = prm.is_identity();
    copy.author = m.author.clone();
    copy.native_2x = m.native_2x;
    copy.vector_source = m.vector_source;
    copy.enhanced_render = m.enhanced_render;
    for e in &m.entries {
        let frames: Vec<CursorFrame> = if identity {
            // 恒等参数短路：HSL 定点往返有 ±1 量化——恒等重染的语义是
            // 「分毫未动」，直接克隆帧（对拍判据：逐像素零差）。
            e.frames.clone()
        } else {
            e.frames.iter().map(|f| transform_frame(f, prm)).collect()
        };
        copy.set_state(e.state, frames);
    }
    RecolorOutcome { copy }
}

/// 帧同步验证（ΔE<1 判据的度量面）：对原本「帧间同色」的像素位置，
/// 重染后帧间色差 ΔE76 ×100 必须 < 100（即 ΔE < 1）。
/// 实现口径：帧 i 与帧 i+1 逐像素取 ΔE（尺寸一致时）；全部 < 100 视为
/// 帧同步（变换逐像素纯函数 → 构造上成立，验证是对判据的实测兑现）。
pub fn frames_color_sync_x100(a: &CursorFrame, b: &CursorFrame) -> Option<i64> {
    if a.w != b.w || a.h != b.h {
        return None;
    }
    let mut worst: i64 = 0;
    for (ca, cb) in a.px.chunks_exact(4).zip(b.px.chunks_exact(4)) {
        if ca[3] < 128 || cb[3] < 128 {
            continue;
        }
        let d = delta_e76_x100(
            Rgb::new(ca[0], ca[1], ca[2]),
            Rgb::new(cb[0], cb[1], cb[2]),
        );
        if d > worst {
            worst = d;
        }
    }
    Some(worst)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F626 自检。
pub fn run_recolor_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F626");
    let base = builtin_default_scheme();

    // 1. 三滑杆效果实测：纯 hue 偏移 180 → 饱和像素色相反转；饱和 0 → 全灰。
    let hue = recolor(&base, &RecolorParams { hue_shift_deg: 180, ..Default::default() });
    let f0_base = &base.state(PointerState::Normal).unwrap().frames[0];
    let f0_hue = &hue.copy.state(PointerState::Normal).unwrap().frames[0];
    let mut hue_ok = true;
    for (c0, ch) in f0_base.px.chunks_exact(4).zip(f0_hue.px.chunks_exact(4)) {
        if c0[3] < 128 {
            continue;
        }
        let (h0, s0, _) = Rgb::new(c0[0], c0[1], c0[2]).to_hsl();
        let (h2, _, _) = Rgb::new(ch[0], ch[1], ch[2]).to_hsl();
        let dh = (h2 as i64 - h0 as i64).rem_euclid(360);
        if s0 > 100 && (dh < 170 || dh > 190) {
            hue_ok = false;
        }
    }
    set.add("hue slider 180 shifts all saturated pixels", hue_ok, "");

    let grey = recolor(&base, &RecolorParams { hue_shift_deg: 0, sat_scale_m: 0, light_scale_m: 1000 });
    let mut all_grey = true;
    for chunk in grey.copy.state(PointerState::Link).unwrap().frames[0].px.chunks_exact(4) {
        if chunk[3] < 128 {
            continue;
        }
        let (_, s, _) = Rgb::new(chunk[0], chunk[1], chunk[2]).to_hsl();
        if s > 10 {
            all_grey = false;
        }
    }
    set.add("saturation slider 0 greys everything", all_grey, "");

    // 2. 恒等参数逐像素零差。
    let id = recolor(&base, &RecolorParams::default());
    let a = id.copy.state(PointerState::Normal).unwrap().frames[0].buf();
    let b = base.state(PointerState::Normal).unwrap().frames[0].buf();
    set.add("identity transform zero diff", a.diff_pixels(&b) == Some(0), "");

    // 3. 强调色一键染色：主色色相对齐强调色（众数桶对拍）。
    let accent = Rgb::new(0, 200, 80); // 绿色系强调
    let prm = accent_dye_params(&base, accent);
    let dyed = recolor(&base, &prm);
    let new_dom = dominant_hue(&dyed.copy).unwrap_or(999);
    let (accent_h, _, _) = accent.to_hsl();
    // 最短角距（双向 rem_euclid 取小——色环上 351° 等价于 −9°）。
    let dh = (new_dom as i64 - accent_h as i64).rem_euclid(360)
        .min((accent_h as i64 - new_dom as i64).rem_euclid(360));
    set.add("accent dye aligns dominant hue", dh <= 30, "");

    // 4. 非破坏副本纪律：原件指纹分毫未动 + 副本 origin/命名。
    let fp_before = crate::jstar2::jbase::vxcur_fingerprint(&base);
    let _ = recolor(&base, &RecolorParams { hue_shift_deg: 90, ..Default::default() });
    set.add(
        "non destructive original untouched + naming",
        crate::jstar2::jbase::vxcur_fingerprint(&base) == fp_before
            && dyed.copy.name == "VARIX 默认指针·重染"
            && dyed.copy.origin == OriginKind::Recolored(String::from("VARIX 默认指针")),
        "",
    );

    // 5. 存档与回退：库房存原件+副本，删副本即回原件（库房侧机制验证）。
    use crate::jstar2::library::{AddOutcome, LibraryView, SchemeLibrary};
    let mut lib = SchemeLibrary::new(0);
    let mut original = base.clone();
    original.name = String::from("母本");
    assert!(matches!(lib.add(original), AddOutcome::Added(_)));
    let mut copy = dyed.copy.clone();
    copy.name = String::from("母本·重染");
    let _ = lib.add(copy);
    let had_copy = lib.get("母本·重染").is_some();
    lib.remove("母本·重染");
    set.add(
        "archive & rollback via library",
        had_copy && lib.get("母本").is_some() && lib.view(LibraryView::All).len() == 1,
        "",
    );

    // 6. 动画帧同步 ΔE<1：构造同色两帧动画 → 重染后帧间 ΔE ×100 < 100。
    use crate::jstar2::jbase::builtin_glyph;
    let mut m = CursorSchemeModel::empty("动画", OriginKind::Created);
    let f1 = builtin_glyph(PointerState::Busy);
    let mut f2 = f1.clone();
    f2.delay_ms = 120;
    m.set_state(PointerState::Busy, alloc::vec![f1.clone(), f2.clone()]);
    let out = recolor(&m, &RecolorParams { hue_shift_deg: 40, ..Default::default() });
    let r1 = &out.copy.state(PointerState::Busy).unwrap().frames[0];
    let r2 = &out.copy.state(PointerState::Busy).unwrap().frames[1];
    match frames_color_sync_x100(r1, r2) {
        Some(d) => set.add("animation frames sync dE<1", d < 100, ""),
        None => set.add("animation frames sync dE<1", false, "size mismatch"),
    }

    // 7. F629 边界：F626 产物 origin=Recolored ≠ F629 产物 origin=Derived。
    let boundary_ok = matches!(dyed.copy.origin, OriginKind::Recolored(_))
        && !matches!(dyed.copy.origin, OriginKind::Derived(_));
    set.add("boundary with F629 origin kinds", boundary_ok, "");

    // 8. 预设面板 + 批量重染：五预设一次出全套，命名带预设名，
    //    origin 一律 Recolored，五产物两两互异。
    let batch = batch_recolor(&base);
    let mut all_ok = batch.len() == 5;
    let mut sigs: Vec<u64> = Vec::new();
    for (i, (name, _)) in RECOLOR_PRESETS.iter().enumerate() {
        all_ok &= batch[i].copy.name.contains(name)
            && batch[i].copy.name.contains("重染")
            && matches!(batch[i].copy.origin, OriginKind::Recolored(_));
        sigs.push(crate::jstar2::jbase::vxcur_fingerprint(&batch[i].copy));
    }
    let distinct = {
        let mut uniq = sigs.clone();
        uniq.sort();
        uniq.dedup();
        uniq.len() == 5
    };
    set.add(
        "batch recolor full preset set with distinct outputs",
        all_ok && distinct,
        "",
    );

    // 9. 色相直方图：12 桶全露出（详情页色相分布的数据面）。
    let hist = hue_histogram(&base);
    set.add(
        "hue histogram twelve buckets",
        hist.len() == 12 && hist.iter().sum::<u64>() > 0,
        "",
    );

    // 10. 参数越界钳制（滑杆脏数据诚实处理）。
    let dirty = RecolorParams { hue_shift_deg: 900, sat_scale_m: -50, light_scale_m: 99999 };
    let clean = dirty.clamped();
    set.add(
        "params clamped honestly",
        clean.hue_shift_deg == 180 && clean.sat_scale_m == 0 && clean.light_scale_m == 2000,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jstar2::jbase::builtin_glyph;

    #[test]
    fn hue_shift_shortest_arc() {
        // 主色 350°、强调色 10° → 偏移 +20（不绕远 −340）。
        let mut m = CursorSchemeModel::empty("t", OriginKind::Created);
        let mut f = builtin_glyph(PointerState::Normal);
        // 全部实体像素刷成品红（h≈350）。
        for c in f.px.chunks_exact_mut(4) {
            if c[3] > 0 {
                let rgb = Rgb::from_hsl(350, 900, 500);
                c[0] = rgb.r;
                c[1] = rgb.g;
                c[2] = rgb.b;
                c[3] = 255;
            }
        }
        m.set_state(PointerState::Normal, alloc::vec![f]);
        let prm = accent_dye_params(&m, Rgb::from_hsl(10, 900, 500));
        // 强调色 10° 经 HSL 定点往返取整为 9°；主色按 30° 桶众数中心
        // （345）取最短弧：9−345 → +24。
        assert_eq!(prm.hue_shift_deg, 24);
    }

    #[test]
    fn dominant_hue_ignores_greys() {
        // 全灰方案 → 无主色 → 染色参数恒等（诚实降级）。
        let mut m = CursorSchemeModel::empty("grey", OriginKind::Created);
        let mut f = builtin_glyph(PointerState::Move);
        for c in f.px.chunks_exact_mut(4) {
            if c[3] > 0 {
                c[0] = 120;
                c[1] = 120;
                c[2] = 120;
                c[3] = 255;
            }
        }
        m.set_state(PointerState::Move, alloc::vec![f]);
        let prm = accent_dye_params(&m, Rgb::new(255, 0, 0));
        assert!(prm.is_identity());
    }

    #[test]
    fn light_scale_bounds_clamped() {
        let mut f = builtin_glyph(PointerState::Text);
        let out = transform_frame(&f, &RecolorParams { hue_shift_deg: 0, sat_scale_m: 1000, light_scale_m: 2000 });
        // 明度 ×2 逐像素 ≤ 1000 定点 → RGB 通道不越 255（clamp 生效）。
        for chunk in out.px.chunks_exact(4) {
            let (_, _, l) = Rgb::new(chunk[0], chunk[1], chunk[2]).to_hsl();
            assert!(l <= 1000);
        }
        let _ = &mut f;
    }

    #[test]
    fn transparent_pixels_untouched() {
        let mut f = builtin_glyph(PointerState::Normal);
        for c in f.px.chunks_exact_mut(4) {
            c[3] = 0;
        }
        let out = transform_frame(&f, &RecolorParams { hue_shift_deg: 123, ..Default::default() });
        assert_eq!(f.px, out.px, "全透明帧不参与色彩域");
    }

    #[test]
    fn frames_sync_none_on_size_mismatch() {
        let a = builtin_glyph(PointerState::Normal);
        let mut b = builtin_glyph(PointerState::Text);
        b.w = 31;
        assert!(frames_color_sync_x100(&a, &b).is_none());
    }
}

// ---------------------------------------------------------------------------
// v3 深化批：色相域替换 · 渐变映射 · 亮度/对比曲线 · 着色混合 ·
// 重染历史栈 · 预设 KV · 前后差异度量 · 批量进度模型
// ---------------------------------------------------------------------------

use crate::jstar2::jbase::ALL_STATES;

/// 色相域替换参数：只对落在 [hue_lo, hue_hi)（度，自动处理 360 环绕）
/// 的像素做 hue_shift + 可选明度补偿（色相羽化 = 边界 ±feather 度内
/// 线性减权——硬边换色会撕出锯齿环）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HueRangeReplace {
    pub hue_lo: u32,
    pub hue_hi: u32,
    pub feather_deg: u32,
    pub hue_shift_deg: i32,
    pub light_scale_m: i64,
}

/// 角度距离（0..180，环绕感知）。
fn hue_dist(a: u32, b: u32) -> i64 {
    let d = (a as i64 - b as i64).rem_euclid(360);
    d.min(360 - d)
}

/// 像素是否入域（含羽化权重：域内 1000，羽化带线性衰减，域外 0）。
pub fn hue_range_weight(p: [u8; 4], r: &HueRangeReplace) -> i64 {
    if p[3] == 0 {
        return 0;
    }
    let (h, _, _) = Rgb::new(p[0], p[1], p[2]).to_hsl();
    // 域内判定（环绕）：与域中点的距离 ≤ 半宽。
    let half = ((r.hue_hi + 360 - r.hue_lo) % 360) as i64 / 2;
    let mid = (r.hue_lo as i64 + half) % 360;
    let d = hue_dist(h as u32, mid as u32);
    let core = half.max(1);
    if d <= core - r.feather_deg as i64 {
        return 1000;
    }
    if d <= core {
        let into = core - d;
        1000 * into / r.feather_deg.max(1) as i64
    } else {
        0
    }
}

/// 色相域替换整帧变换（羽化权重与目标变换线性插值——软边不撕）。
pub fn transform_frame_hue_range(f: &CursorFrame, r: &HueRangeReplace) -> CursorFrame {
    let base = RecolorParams { hue_shift_deg: r.hue_shift_deg, sat_scale_m: 1000, light_scale_m: r.light_scale_m };
    let mut px = f.px.clone();
    for chunk in px.chunks_exact_mut(4) {
        let w = hue_range_weight(chunk.try_into().unwrap(), r);
        if w == 0 {
            continue;
        }
        let transformed = transform_pixel(chunk.try_into().unwrap(), &base);
        let mix = |a: u8, b: u8| -> u8 { ((a as i64 * w + b as i64 * (1000 - w)) / 1000).clamp(0, 255) as u8 };
        chunk[0] = mix(transformed[0], chunk[0]);
        chunk[1] = mix(transformed[1], chunk[1]);
        chunk[2] = mix(transformed[2], chunk[2]);
    }
    CursorFrame::from_buf(f.hot_x, f.hot_y, f.delay_ms, crate::jstar2::jbase::PixBuf::from_rgba(f.w, f.h, px))
}

/// 渐变映射：按像素明度（0..1000）在 dark→light 两色间插值（保 alpha
/// ——映射不碰透明通道的形状信息）。
pub fn gradient_map_frame(f: &CursorFrame, dark: Rgb, light: Rgb) -> CursorFrame {
    let mut px = f.px.clone();
    for chunk in px.chunks_exact_mut(4) {
        if chunk[3] == 0 {
            continue;
        }
        let (_, _, l) = Rgb::new(chunk[0], chunk[1], chunk[2]).to_hsl();
        let mix = |a: u8, b: u8| -> u8 { ((a as i64 * (1000 - l) + b as i64 * l) / 1000).clamp(0, 255) as u8 };
        chunk[0] = mix(dark.r, light.r);
        chunk[1] = mix(dark.g, light.g);
        chunk[2] = mix(dark.b, light.b);
    }
    CursorFrame::from_buf(f.hot_x, f.hot_y, f.delay_ms, crate::jstar2::jbase::PixBuf::from_rgba(f.w, f.h, px))
}

/// 亮度/对比曲线（对比 0..2000‰，锚点 128 定点；亮度 ±500‰ 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrightContrast {
    pub brightness_m: i64,
    pub contrast_m: i64,
}

impl Default for BrightContrast {
    fn default() -> Self {
        BrightContrast { brightness_m: 0, contrast_m: 1000 }
    }
}

pub fn transform_pixel_bc(p: [u8; 4], bc: &BrightContrast) -> [u8; 4] {
    let f = |v: u8| -> u8 {
        let x = v as i64 - 128;
        let scaled = x * bc.contrast_m / 1000 + 128 + bc.brightness_m / 2;
        scaled.clamp(0, 255) as u8
    };
    [f(p[0]), f(p[1]), f(p[2]), p[3]]
}

/// 着色混合：向 tint 色按 amount_m（0..1000）靠拢（保 alpha——
/// 单色调主题派生的底层算子）。
pub fn tint_frame(f: &CursorFrame, tint: Rgb, amount_m: i64) -> CursorFrame {
    let a = amount_m.clamp(0, 1000);
    let mut px = f.px.clone();
    for chunk in px.chunks_exact_mut(4) {
        if chunk[3] == 0 {
            continue;
        }
        let mix = |t: u8, b: u8| -> u8 { ((t as i64 * a + b as i64 * (1000 - a)) / 1000).clamp(0, 255) as u8 };
        chunk[0] = mix(tint.r, chunk[0]);
        chunk[1] = mix(tint.g, chunk[1]);
        chunk[2] = mix(tint.b, chunk[2]);
    }
    CursorFrame::from_buf(f.hot_x, f.hot_y, f.delay_ms, crate::jstar2::jbase::PixBuf::from_rgba(f.w, f.h, px))
}

/// 重染历史栈：参数序列 + 游标（应用即推进，undo 回退，redo 前进——
/// 第十二·补「用户内容操作要有 undo 链」）。
pub struct RecolorHistory {
    steps: Vec<RecolorParams>,
    cursor: usize,
}

impl RecolorHistory {
    pub fn new() -> RecolorHistory {
        RecolorHistory { steps: alloc::vec![RecolorParams::default()], cursor: 0 }
    }

    /// 应用新参数（截断游标之后的重做分支——新操作作废旧未来）。
    pub fn apply(&mut self, prm: RecolorParams) {
        if prm.is_identity() {
            return; // 恒等参数不进栈（无意义步骤不入链）
        }
        self.steps.truncate(self.cursor + 1);
        self.steps.push(prm);
        self.cursor += 1;
    }

    pub fn undo(&mut self) -> Option<RecolorParams> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        Some(self.steps[self.cursor])
    }

    pub fn redo(&mut self) -> Option<RecolorParams> {
        if self.cursor + 1 >= self.steps.len() {
            return None;
        }
        self.cursor += 1;
        Some(self.steps[self.cursor])
    }

    pub fn current(&self) -> RecolorParams {
        self.steps[self.cursor]
    }

    pub fn depth(&self) -> usize {
        self.steps.len() - 1
    }
}
impl Default for RecolorHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// 预设 KV 序列化（12 字节：魔数 | hue i16 | sat i16 | light i16，大端）。
pub fn params_to_kv(p: &RecolorParams) -> [u8; 8] {
    let hue = p.hue_shift_deg.clamp(-180, 180) as i16;
    let sat = p.sat_scale_m.clamp(0, 2000) as u16;
    let light = p.light_scale_m.clamp(0, 2000) as u16;
    [
        0x52, 0x43,
        (hue >> 8) as u8, hue as u8,
        (sat >> 8) as u8, sat as u8,
        (light >> 8) as u8, light as u8,
    ]
}

pub fn params_from_kv(kv: &[u8]) -> Option<RecolorParams> {
    if kv.len() != 8 || kv[0] != 0x52 || kv[1] != 0x43 {
        return None;
    }
    let hue = i16::from_be_bytes([kv[2], kv[3]]);
    if !(-180..=180).contains(&hue) {
        return None;
    }
    Some(RecolorParams {
        hue_shift_deg: hue as i32,
        sat_scale_m: u16::from_be_bytes([kv[4], kv[5]]) as i64,
        light_scale_m: u16::from_be_bytes([kv[6], kv[7]]) as i64,
    })
}

/// 前后差异度量：变化像素数 + 最大 ΔE（×100）——「染了多少、染得多深」
/// 的量化对账面（全域逐态扫描）。
pub struct DiffMetrics {
    pub changed_px: u64,
    pub max_de_x100: i64,
}

pub fn diff_metrics(before: &CursorSchemeModel, after: &CursorSchemeModel) -> DiffMetrics {
    let mut changed = 0u64;
    let mut max_de = 0i64;
    for st in ALL_STATES {
        let (Some(bs), Some(asf)) = (before.state(st), after.state(st)) else {
            continue;
        };
        for (bf, af) in bs.frames.iter().zip(asf.frames.iter()) {
        for (b, a) in bf.px.chunks_exact(4).zip(af.px.chunks_exact(4)) {
            if b != a {
                changed += 1;
                let de = crate::jstar2::jbase::delta_e76_x100(
                    Rgb::new(b[0], b[1], b[2]),
                    Rgb::new(a[0], a[1], a[2]),
                );
                if de > max_de {
                    max_de = de;
                }
            }
        }
        }
    }
    DiffMetrics { changed_px: changed, max_de_x100: max_de }
}

/// 批量进度模型（B-18xx UI 线程隔离判据的模型面：分片推进 + 取消旗）。
pub struct BatchProgress {
    pub total: usize,
    pub done: usize,
    pub cancelled: bool,
}

impl BatchProgress {
    pub fn new(total: usize) -> BatchProgress {
        BatchProgress { total, done: 0, cancelled: false }
    }

    /// 推进一步（返回是否应继续——取消旗升起即停）。
    pub fn step(&mut self) -> bool {
        if self.cancelled || self.done >= self.total {
            return false;
        }
        self.done += 1;
        !self.cancelled
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    /// 完成比（千分位）——诚实进度条的数源。
    pub fn permille(&self) -> i64 {
        if self.total == 0 {
            return 1000;
        }
        1000 * self.done as i64 / self.total as i64
    }
}

/// v3 自检。
pub fn run_recolor_v3_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F626-v3");

    // —— 色相域替换 ——
    let r = HueRangeReplace {
        hue_lo: 60,
        hue_hi: 180,
        feather_deg: 10,
        hue_shift_deg: 180,
        light_scale_m: 1000,
    };
    // 绿（hue≈120，域中点）在域内全权重；蓝（hue≈240）在域外零权重。
    let green = Rgb::from_hsl(120, 1000, 500);
    let blue = Rgb::from_hsl(240, 1000, 500);
    set.add(
        "hue range weights inside outside",
        hue_range_weight([green.r, green.g, green.b, 255], &r) == 1000
            && hue_range_weight([blue.r, blue.g, blue.b, 255], &r) == 0,
        "",
    );
    // 域边界羽化权重介于 0 与 1000 之间（hue 65 落在 lo=60 起 10° 羽化带内）。
    let edge = Rgb::from_hsl(((r.hue_lo + 5) % 360) as u32, 1000, 500);
    let w_edge = hue_range_weight([edge.r, edge.g, edge.b, 255], &r);
    set.add("hue range feather band partial", w_edge > 0 && w_edge < 1000, "");
    // 透明像素恒零权重（不碰形状）。
    set.add("hue range transparent weight zero", hue_range_weight([255, 255, 0, 0], &r) == 0, "");

    // —— 渐变映射 ——
    let grad_src = crate::jstar2::jbase::builtin_glyph(PointerState::Normal);
    let dark = Rgb::new(0, 0, 0);
    let light = Rgb::new(255, 255, 255);
    let mapped = gradient_map_frame(&grad_src, dark, light);
    let alpha_preserved = mapped
        .px
        .chunks_exact(4)
        .zip(grad_src.px.chunks_exact(4))
        .all(|(a, b)| a[3] == b[3]);
    set.add("gradient map preserves alpha plane", mapped.px.len() == grad_src.px.len() && alpha_preserved, "");

    // —— 亮度/对比 ——
    let bc = BrightContrast { brightness_m: 0, contrast_m: 2000 };
    set.add(
        "contrast pivots at 128",
        transform_pixel_bc([128, 128, 128, 255], &bc) == [128, 128, 128, 255]
            && transform_pixel_bc([138, 138, 138, 255], &bc) == [148, 148, 148, 255]
            && transform_pixel_bc([118, 118, 118, 255], &bc) == [108, 108, 108, 255],
        "",
    );
    let bc2 = BrightContrast { brightness_m: 200, contrast_m: 1000 };
    set.add(
        "brightness shifts and clamps",
        transform_pixel_bc([100, 100, 100, 255], &bc2) == [200, 200, 200, 255]
            && transform_pixel_bc([250, 250, 250, 255], &bc2) == [255, 255, 255, 255],
        "",
    );

    // —— 着色混合 ——
    let t0 = tint_frame(&grad_src, Rgb::new(255, 0, 0), 0);
    let t1 = tint_frame(&grad_src, Rgb::new(255, 0, 0), 1000);
    set.add(
        "tint amount zero and full",
        t0.px == grad_src.px && t1.px.chunks_exact(4).filter(|c| c[3] != 0).all(|c| c[0] == 255),
        "",
    );

    // —— 历史栈 ——
    let mut hist = RecolorHistory::new();
    hist.apply(RecolorParams { hue_shift_deg: 90, sat_scale_m: 1000, light_scale_m: 1000 });
    hist.apply(RecolorParams { hue_shift_deg: 0, sat_scale_m: 500, light_scale_m: 1000 });
    hist.apply(RecolorParams::default()); // 恒等不入栈
    let undo1 = hist.undo().unwrap();
    let undo2 = hist.undo().unwrap();
    let no_more = hist.undo().is_none();
    let redo1 = hist.redo().unwrap();
    set.add(
        "history undo redo chain sane",
        hist.depth() == 2
            && undo1.hue_shift_deg == 90
            && undo2.is_identity()
            && no_more
            && redo1.hue_shift_deg == 90
            && hist.current().sat_scale_m == 1000,
        "",
    );
    // 新操作截断重做分支（两步历史被一步新操作收窄）。
    hist.undo();
    hist.apply(RecolorParams { hue_shift_deg: 30, sat_scale_m: 1000, light_scale_m: 1000 });
    set.add("apply truncates redo branch", hist.redo().is_none() && hist.depth() == 1, "");

    // —— 预设 KV ——
    let p = RecolorParams { hue_shift_deg: -120, sat_scale_m: 1500, light_scale_m: 800 };
    let kv = params_to_kv(&p);
    let mut bad = kv;
    bad[0] = 0;
    set.add(
        "params kv roundtrip tamper reject",
        params_from_kv(&kv).map(|x| x == p).unwrap_or(false)
            && params_from_kv(&bad).is_none()
            && params_from_kv(&[0u8; 8]).is_none(),
        "",
    );

    // —— 前后差异度量 ——
    let scheme = crate::jstar2::jbase::builtin_default_scheme();
    let shifted = recolor(&scheme, &RecolorParams { hue_shift_deg: 180, sat_scale_m: 1000, light_scale_m: 1000 });
    let same = recolor(&scheme, &RecolorParams::default());
    let dm = diff_metrics(&scheme, &shifted.copy);
    let dm0 = diff_metrics(&scheme, &same.copy);
    set.add(
        "diff metrics quantify the dye",
        dm.changed_px > 0 && dm.max_de_x100 > 0 && dm0.changed_px == 0 && dm0.max_de_x100 == 0,
        "",
    );

    // —— 批量进度 ——
    let mut bp = BatchProgress::new(10);
    let mut ran = 0;
    while bp.step() {
        ran += 1;
        if ran == 6 {
            bp.cancel();
        }
    }
    set.add(
        "batch progress cancel honored",
        ran == 6 && bp.done == 6 && bp.permille() == 600,
        "",
    );
    let mut bp0 = BatchProgress::new(0);
    set.add("empty batch completes immediately", !bp0.step() && bp0.permille() == 1000, "");

    // —— 强调色染色确定性与收敛（同色同参；桶粒度 30° → 残余偏移 ≤15°）——
    let accent = Rgb::from_hsl(210, 900, 550);
    let once = accent_dye_params(&scheme, accent);
    let once_again = accent_dye_params(&scheme, accent);
    let dyed = recolor(&scheme, &once).copy;
    let twice_params = accent_dye_params(&dyed, accent);
    set.add(
        "accent dye idempotent for same accent",
        once == once_again && twice_params.hue_shift_deg.abs() <= 15,
        "",
    );

    set
}

#[cfg(test)]
mod tests_v3 {
    use super::*;

    #[test]
    fn hue_dist_wraps() {
        assert_eq!(hue_dist(10, 350), 20);
        assert_eq!(hue_dist(0, 180), 180);
        assert_eq!(hue_dist(0, 0), 0);
    }

    #[test]
    fn gradient_map_is_order_stable() {
        let f = crate::jstar2::jbase::builtin_glyph(PointerState::Normal);
        let a = gradient_map_frame(&f, Rgb::new(10, 20, 30), Rgb::new(200, 210, 220));
        let b = gradient_map_frame(&f, Rgb::new(10, 20, 30), Rgb::new(200, 210, 220));
        assert_eq!(a.px, b.px);
    }

    #[test]
    fn history_starts_identity() {
        let h = RecolorHistory::new();
        assert!(h.current().is_identity() && h.depth() == 0);
    }

    #[test]
    fn progress_runs_to_completion() {
        let mut bp = BatchProgress::new(4);
        let mut ran = 0;
        while bp.step() {
            ran += 1;
        }
        assert_eq!(ran, 4);
        assert_eq!(bp.permille(), 1000);
    }
}
