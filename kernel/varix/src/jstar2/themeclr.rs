//! F629 主题派生指针配色 · 完整设计（STAR I 主册 J-C 组）。
//!
//! **判据（主册原文）**：深浅两主题派生对拍（描边对比度 ≥3:1）；存为
//! 新方案纪律；与 F151/F116 令牌同源（换色重派生即时）；派生方案可再
//! 编辑链路；与 F626 边界用例。
//!
//! **派生算法（确定性纯函数——「换色重派生即时」的机制基础）**：
//! 1. 轮廓环识别：与透明像素相邻的不透明像素（1px 环）视为描边候选；
//! 2. 主体再着色：非环像素 → 中性色（低饱和、明度按主题取值——浅色
//!    主题出深主体、深色主题出浅主体）；
//! 3. 描边着色：环像素 → 主题强调色，并做对比度闭环——描边对主题底
//!    色对比度 < 3:1 时沿明度轴外推直至达标（判据线 ×100 定点 ≥300）；
//! 4. 产物存为新方案（origin=Derived{令牌指纹}，不覆盖现有——判据
//!    「存为新方案纪律」），入 F628 库房与其余方案平权（可再编辑 =
//!    F625 工坊/F626 重染都能打开它，边界用例验证）。
//!
//! **与 F626 边界（主册原文）**：F626 用户手动染任意方案（Recolored）、
//! F629 系统按令牌派生（Derived）——两条路都通向方案库；派生是赠品
//! 不是绑定（删除派生方案不影响主题）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{
    contrast_x100, CursorFrame, CursorSchemeModel, OriginKind, PixBuf, Rgb, fnv1a64,
};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 令牌模型（F151 令牌的指针域接缝——显式注入口）
// ---------------------------------------------------------------------------

/// 主题令牌投影（F151/F116 的显式参数注入口：不反向依赖未落地令牌
/// 模块——J 域只消费四个值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeTokens {
    /// 主题明暗（浅/深）。
    pub dark: bool,
    /// 强调色（F116 口径的当前强调色）。
    pub accent: Rgb,
    /// 主题底色（窗口/桌面基底，对比度的对边）。
    pub background: Rgb,
    /// 主体中性色（派生的指针主体基色）。
    pub neutral: Rgb,
}

impl ThemeTokens {
    /// 令牌指纹（origin 明细——令牌换色即换指纹，重派生可对账）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::with_capacity(16);
        buf.push(self.dark as u8);
        buf.extend_from_slice(&[self.accent.r, self.accent.g, self.accent.b]);
        buf.extend_from_slice(&[self.background.r, self.background.g, self.background.b]);
        buf.extend_from_slice(&[self.neutral.r, self.neutral.g, self.neutral.b]);
        fnv1a64(&buf)
    }
}

// ---------------------------------------------------------------------------
// 派生主算法
// ---------------------------------------------------------------------------

/// 明度外推：沿 HSL 明度轴把 `c` 对 `bg` 的对比度推到 ≥ target_x100。
/// 色相/饱和度全程保持（只动 L）；方向 = 背景亮度的反侧；从当前 L
/// 向端点逐档扫描（步长 25，最多 40 档达全轴），取首个达标档——
/// 到端点仍不达标（极端背景）时返回最近端点色（尽力而为 + 如实）。
pub fn push_contrast(c: Rgb, bg: Rgb, target_x100: i64) -> Rgb {
    if contrast_x100(c, bg) >= target_x100 {
        return c;
    }
    let (h, s, l0) = c.to_hsl();
    let bg_lum = crate::jstar2::jbase::relative_luminance_m(bg);
    let cur_lum = crate::jstar2::jbase::relative_luminance_m(c);
    // 朝远离背景的方向扫（暗背景 → 变亮；亮背景 → 变暗）。
    let dir: i64 = if cur_lum >= bg_lum { 1 } else { -1 };
    let mut best = c;
    for i in 1..=40i64 {
        let l = (l0 + dir * i * 25).clamp(0, 1000);
        let cand = Rgb::from_hsl(h, s, l);
        best = cand;
        if contrast_x100(cand, bg) >= target_x100 {
            return cand;
        }
        if l == 0 || l == 1000 {
            break; // 已到端点
        }
    }
    best
}

/// 描边环识别掩码（1px：不透明且 4-邻至少一个透明）。
fn outline_mask(buf: &PixBuf) -> Vec<bool> {
    let n = buf.w as usize * buf.h as usize;
    let mut mask = alloc::vec![false; n];
    for y in 0..buf.h {
        for x in 0..buf.w {
            if !buf.solid(x, y) {
                continue;
            }
            let neighbor_transparent = [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dy)| {
                    let nx = x as i64 + dx;
                    let ny = y as i64 + dy;
                    nx < 0
                        || ny < 0
                        || nx >= buf.w as i64
                        || ny >= buf.h as i64
                        || !buf.solid(nx as u16, ny as u16)
                });
            if neighbor_transparent {
                mask[y as usize * buf.w as usize + x as usize] = true;
            }
        }
    }
    mask
}

/// 派生单帧。
fn derive_frame(f: &CursorFrame, tokens: &ThemeTokens) -> CursorFrame {
    let buf = PixBuf::from_rgba(f.w, f.h, f.px.clone());
    let mask = outline_mask(&buf);
    // 描边色：强调色对主题底色闭环到 ≥3:1（判据线）。
    let outline = push_contrast(tokens.accent, tokens.background, 300);
    // 主体色：中性色对主题底色闭环（可读主体）。
    let body = push_contrast(tokens.neutral, tokens.background, 300);
    let mut px = f.px.clone();
    for (i, chunk) in px.chunks_exact_mut(4).enumerate() {
        if chunk[3] == 0 {
            continue;
        }
        let c = if mask[i] { outline } else { body };
        chunk[0] = c.r;
        chunk[1] = c.g;
        chunk[2] = c.b;
        // alpha 保持。
    }
    CursorFrame { w: f.w, h: f.h, hot_x: f.hot_x, hot_y: f.hot_y, delay_ms: f.delay_ms, px }
}

/// 派生整方案（存为新方案纪律：origin=Derived{令牌指纹}，命名
/// 「«原名»·主题派生」）。
pub fn derive_scheme(m: &CursorSchemeModel, tokens: &ThemeTokens) -> CursorSchemeModel {
    let mut out = CursorSchemeModel::empty(
        &alloc::format!("{}·主题派生", m.name),
        OriginKind::Derived(alloc::format!("{:016x}", tokens.fingerprint())),
    );
    out.author = m.author.clone();
    out.native_2x = m.native_2x;
    out.vector_source = m.vector_source;
    out.enhanced_render = m.enhanced_render;
    for e in &m.entries {
        let frames: Vec<CursorFrame> = e.frames.iter().map(|f| derive_frame(f, tokens)).collect();
        out.set_state(e.state, frames);
    }
    out
}

/// 描边对比度抽检（判据「描边对比度 ≥3:1」的度量面）：全部描边像素
/// （环掩码）对主题底色的最低对比度 ×100。
pub fn min_outline_contrast_x100(m: &CursorSchemeModel, tokens: &ThemeTokens) -> Option<i64> {
    let mut worst: Option<i64> = None;
    for e in &m.entries {
        for f in &e.frames {
            let buf = PixBuf::from_rgba(f.w, f.h, f.px.clone());
            let mask = outline_mask(&buf);
            for (i, chunk) in buf.px.chunks_exact(4).enumerate() {
                if chunk[3] < 128 || !mask[i] {
                    continue;
                }
                let c = contrast_x100(
                    Rgb::new(chunk[0], chunk[1], chunk[2]),
                    tokens.background,
                );
                worst = Some(match worst {
                    Some(w) if w <= c => w,
                    _ => c,
                });
            }
        }
    }
    worst
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F629 自检。
pub fn run_themeclr_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F629");
    let base = builtin_default_scheme();

    let light = ThemeTokens {
        dark: false,
        accent: Rgb::new(0, 102, 204),
        background: Rgb::new(245, 245, 245),
        neutral: Rgb::new(60, 60, 60),
    };
    let dark = ThemeTokens {
        dark: true,
        accent: Rgb::new(80, 180, 255),
        background: Rgb::new(28, 28, 30),
        neutral: Rgb::new(220, 220, 220),
    };

    // 1. 深浅两主题派生：描边最低对比度都 ≥3:1（判据线 ×100 ≥ 300）。
    let dl = derive_scheme(&base, &light);
    let dd = derive_scheme(&base, &dark);
    let cl = min_outline_contrast_x100(&dl, &light).unwrap_or(0);
    let cd = min_outline_contrast_x100(&dd, &dark).unwrap_or(0);
    set.add(
        "light & dark derived outlines >= 3:1",
        cl >= 300 && cd >= 300,
        "",
    );

    // 2. 深浅产物方向对拍：浅主题描边比深主题描边更「深」（亮度更低）。
    let lum = |m: &CursorSchemeModel| -> i64 {
        let f = &m.state(crate::jstar2::jbase::PointerState::Normal).unwrap().frames[0];
        // 首个实体像素（描边环）的亮度——透明角的 RGB 是无意义值。
        for c in f.px.chunks_exact(4) {
            if c[3] >= 128 {
                return crate::jstar2::jbase::relative_luminance_m(Rgb::new(c[0], c[1], c[2]));
            }
        }
        0
    };
    set.add(
        "light theme derives dark outline, dark derives light",
        lum(&dl) < lum(&dd),
        "",
    );

    // 3. 存为新方案纪律：origin=Derived{令牌指纹}、命名带派生后缀、
    //    原方案分毫未动。
    let fp_base = crate::jstar2::jbase::vxcur_fingerprint(&base);
    let _ = derive_scheme(&base, &light);
    set.add(
        "derived saved as new scheme, base untouched",
        crate::jstar2::jbase::vxcur_fingerprint(&base) == fp_base
            && dl.name == "VARIX 默认指针·主题派生"
            && matches!(dl.origin, OriginKind::Derived(_)),
        "",
    );

    // 4. 令牌同源：换强调色 → 指纹变 → 重派生产物跟着变（即时性=纯函数）。
    let accent2 = ThemeTokens { accent: Rgb::new(220, 40, 40), ..light };
    let dl2 = derive_scheme(&base, &accent2);
    let d_of = |m: &CursorSchemeModel| -> i64 {
        let f = &m.state(crate::jstar2::jbase::PointerState::Normal).unwrap().frames[0];
        for c in f.px.chunks_exact(4) {
            if c[3] >= 128 {
                let (h, _, _) = Rgb::new(c[0], c[1], c[2]).to_hsl();
                return h as i64;
            }
        }
        0
    };
    set.add(
        "token change re-derives immediately",
        accent2.fingerprint() != light.fingerprint() && d_of(&dl) != d_of(&dl2),
        "",
    );

    // 5. 派生方案可再编辑链路：入库 → F626 重染可开 → F625/F628 平权。
    use crate::jstar2::library::{AddOutcome, SchemeLibrary};
    use crate::jstar2::recolor::{recolor, RecolorParams};
    let mut lib = SchemeLibrary::new(0);
    let mut derived = dl.clone();
    derived.name = String::from("派生件");
    assert!(matches!(lib.add(derived), AddOutcome::Added(_)));
    let in_lib = lib.get("派生件").unwrap().model.clone();
    let edited = recolor(&in_lib, &RecolorParams { hue_shift_deg: 30, ..Default::default() });
    set.add(
        "derived scheme re-editable via F626",
        matches!(edited.copy.origin, OriginKind::Recolored(_))
            && edited.copy.name == "派生件·重染"
            && lib.get("派生件").is_some(),
        "",
    );

    // 6. F626 边界：本域产物 Derived ≠ Recolored（同 F626 检查镜像）。
    set.add(
        "boundary derived-vs-recolored",
        matches!(dl.origin, OriginKind::Derived(_)) && !matches!(dl.origin, OriginKind::Recolored(_)),
        "",
    );

    // 7. 派生是赠品不是绑定：删派生方案不影响主题令牌与原方案。
    let mut lib2 = SchemeLibrary::new(0);
    let mut dd2 = dd.clone();
    dd2.name = String::from("深色派生");
    let _ = lib2.add(dd2);
    lib2.remove("深色派生");
    set.add(
        "deleting derivative leaves theme intact",
        lib2.get("深色派生").is_none()
            && crate::jstar2::jbase::vxcur_fingerprint(&base) == fp_base,
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
    use crate::jstar2::jbase::{builtin_glyph, PointerState};

    fn flat_scheme(color: Rgb) -> CursorSchemeModel {
        let mut m = CursorSchemeModel::empty("flat", OriginKind::Created);
        let mut f = builtin_glyph(PointerState::Normal);
        for c in f.px.chunks_exact_mut(4) {
            if c[3] > 0 {
                c[0] = color.r;
                c[1] = color.g;
                c[2] = color.b;
                c[3] = 255;
            }
        }
        m.set_state(PointerState::Normal, alloc::vec![f]);
        m
    }

    #[test]
    fn outline_mask_marks_border_ring_only() {
        // 8×8 实心方块 → 环 = 周长像素（28），内部（36）非环。
        let mut buf = PixBuf::new(8, 8);
        for y in 0..8u16 {
            for x in 0..8u16 {
                buf.set(x, y, [255, 0, 0, 255]);
            }
        }
        let mask = outline_mask(&buf);
        let ring = mask.iter().filter(|b| **b).count();
        assert_eq!(ring, 28);
    }

    #[test]
    fn push_contrast_reaches_target() {
        let bg = Rgb::new(240, 240, 240);
        let c = push_contrast(Rgb::new(200, 200, 200), bg, 300);
        assert!(contrast_x100(c, bg) >= 300, "got {}", contrast_x100(c, bg));
        // 深底同理。
        let bg2 = Rgb::new(20, 20, 20);
        let c2 = push_contrast(Rgb::new(60, 60, 60), bg2, 300);
        assert!(contrast_x100(c2, bg2) >= 300);
    }

    #[test]
    fn alpha_preserved_by_derive() {
        let m = flat_scheme(Rgb::new(255, 0, 0));
        let t = ThemeTokens {
            dark: false,
            accent: Rgb::new(0, 0, 255),
            background: Rgb::new(255, 255, 255),
            neutral: Rgb::new(0, 0, 0),
        };
        let out = derive_scheme(&m, &t);
        let f0 = &out.state(PointerState::Normal).unwrap().frames[0];
        let f_in = &m.state(PointerState::Normal).unwrap().frames[0];
        for (a, b) in f0.px.chunks_exact(4).zip(f_in.px.chunks_exact(4)) {
            assert_eq!(a[3], b[3], "alpha 必须逐像素保持");
        }
    }

    #[test]
    fn token_fingerprint_changes_with_accent() {
        let t1 = ThemeTokens {
            dark: false,
            accent: Rgb::new(0, 102, 204),
            background: Rgb::new(245, 245, 245),
            neutral: Rgb::new(60, 60, 60),
        };
        let t2 = ThemeTokens { accent: Rgb::new(0, 103, 204), ..t1 };
        assert_ne!(t1.fingerprint(), t2.fingerprint());
        assert_eq!(t1.fingerprint(), t1.fingerprint());
    }
}
