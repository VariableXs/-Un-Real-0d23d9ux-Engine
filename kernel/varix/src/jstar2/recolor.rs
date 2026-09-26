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
