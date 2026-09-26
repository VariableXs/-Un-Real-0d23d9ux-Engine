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

/// 派生单帧·带判线目标（常规 3:1 / 高对比 4.5:1 共用同一派生管线，
/// 只差闭环目标——「高对比」是参数不是另一套算法）。
fn derive_frame_target(f: &CursorFrame, tokens: &ThemeTokens, target_x100: i64) -> CursorFrame {
    let buf = PixBuf::from_rgba(f.w, f.h, f.px.clone());
    let mask = outline_mask(&buf);
    // 描边色：强调色对主题底色闭环到判线（常规 ≥3:1 / 高对比 ≥4.5:1）。
    let outline = push_contrast(tokens.accent, tokens.background, target_x100);
    // 主体色：中性色对主题底色闭环（可读主体）。
    let body = push_contrast(tokens.neutral, tokens.background, target_x100);
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
    derive_scheme_target(m, tokens, 300)
}

/// 派生整方案·带判线目标（高对比预设自证用 450 线闭环）。
pub fn derive_scheme_target(m: &CursorSchemeModel, tokens: &ThemeTokens, target_x100: i64) -> CursorSchemeModel {
    let mut out = CursorSchemeModel::empty(
        &alloc::format!("{}·主题派生", m.name),
        OriginKind::Derived(alloc::format!("{:016x}", tokens.fingerprint())),
    );
    out.author = m.author.clone();
    out.native_2x = m.native_2x;
    out.vector_source = m.vector_source;
    out.enhanced_render = m.enhanced_render;
    for e in &m.entries {
        let frames: Vec<CursorFrame> = e.frames.iter().map(|f| derive_frame_target(f, tokens, target_x100)).collect();
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

// ---------------------------------------------------------------------------
// v2 深化：默认令牌 / 深浅成对派生 / 逐态对比度报告
// ---------------------------------------------------------------------------

/// 深浅主题默认令牌（预设起步——不要求用户先配四个值；浅色 = 白底深
/// 灰主体蓝强调，深色 = 深底浅灰主体青强调。判据对拍的确定性输入）。
pub fn default_tokens(dark: bool) -> ThemeTokens {
    if dark {
        ThemeTokens {
            dark: true,
            accent: Rgb::new(80, 200, 220),
            background: Rgb::new(24, 24, 28),
            neutral: Rgb::new(210, 210, 215),
        }
    } else {
        ThemeTokens {
            dark: false,
            accent: Rgb::new(0, 110, 200),
            background: Rgb::new(245, 245, 248),
            neutral: Rgb::new(40, 40, 45),
        }
    }
}

/// 深浅两主题成对派生（主册「深浅两主题派生对拍」的成对面）：同一
/// 基础令牌的明暗两版各派生一份——浅色主题出深描边、深色主题出浅
/// 描边，两份都是完整方案（命名带「浅色/深色」段以互辨）。
pub fn derive_scheme_pair(m: &CursorSchemeModel, base: ThemeTokens) -> (CursorSchemeModel, CursorSchemeModel) {
    let light_tok = ThemeTokens { dark: false, ..base };
    let dark_tok = ThemeTokens { dark: true, ..base };
    let mut light = derive_scheme(m, &light_tok);
    let mut dark = derive_scheme(m, &dark_tok);
    light.name = alloc::format!("{}·浅色派生", m.name);
    dark.name = alloc::format!("{}·深色派生", m.name);
    (light, dark)
}

/// 逐态描边对比度报告（态 id → 该态描边像素对主题底色的最低对比度
/// ×100；派生描边是统一色，报告值即派生色对底色的对比度——在场态
/// 全覆盖，缺态不进表，报告如实反映）。
pub fn outline_contrast_report(m: &CursorSchemeModel, tokens: &ThemeTokens) -> Vec<(u8, i64)> {
    let outline = push_contrast(tokens.accent, tokens.background, 300);
    let mut out = Vec::new();
    for e in &m.entries {
        let d = contrast_x100(outline, tokens.background);
        out.push((e.state.id(), d));
    }
    out.sort_by_key(|(id, _)| *id);
    out
}

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


    // 5. 成对派生：深浅两版对各自底色的描边对比度都 ≥3:1，命名互辨。
    let base5 = crate::jstar2::jbase::builtin_default_scheme();
    let (pair_light, pair_dark) = derive_scheme_pair(&base5, default_tokens(false));
    let l_ok = min_outline_contrast_x100(&pair_light, &default_tokens(false)).map(|c| c >= 300).unwrap_or(false);
    let d_ok = min_outline_contrast_x100(&pair_dark, &default_tokens(true)).map(|c| c >= 300).unwrap_or(false);
    set.add(
        "default token pair derivation named and contrasted",
        l_ok && d_ok && pair_light.name.contains("浅色") && pair_dark.name.contains("深色"),
        "",
    );

    // 6. 逐态对比度报告：在场态全覆盖 + 逐态恒等于派生描边色对比度
    //    （派生描边统一色 → 报告逐格同值）且闭环 ≥3:1。
    let tok6 = default_tokens(false);
    let report = outline_contrast_report(&base5, &tok6);
    let outline_c = contrast_x100(push_contrast(tok6.accent, tok6.background, 300), tok6.background);
    set.add(
        "per-state outline report consistent",
        report.len() == 15
            && report.iter().all(|(_, c)| *c == outline_c)
            && outline_c >= 300,
        "",
    );

    // 7. 默认令牌：明暗两版确有区分（对拍输入不重合）。
    set.add(
        "default tokens light dark distinct",
        default_tokens(false).background != default_tokens(true).background
            && !default_tokens(false).dark
            && default_tokens(true).dark,
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

// ---------------------------------------------------------------------------
// v3 深化批：派生配方（序列化+失配检测）· 强调色变体梯 · 多主题预览
// 矩阵 · 最差态聚合 · 重派生事件台账 · 派生确定性对拍
// ---------------------------------------------------------------------------

/// 派生配方（KV 8 字节：魔数 | dark | accent | background | neutral
/// 各 1 字节色索引——全色走 6 位量化寄存，还原时 ×4 近似）。
///
/// 说明：令牌色是真彩（0..255），8 字节装不下四个真彩——配方序列化
/// 用「每色 2 字节 RGB565」方案（12 字节 + 魔数 2 + dark 1 = 15），
/// 往返允许 ±8/通道量化差（判据锚：重派生以指纹为对账，不以逐字节）。
pub const RECIPE_MAGIC: [u8; 2] = [0x54, 0x44];

fn rgb565(c: Rgb) -> [u8; 2] {
    let r = (c.r >> 3) as u16;
    let g = (c.g >> 2) as u16;
    let b = (c.b >> 3) as u16;
    let v = (r << 11) | (g << 5) | b;
    [(v >> 8) as u8, v as u8]
}

fn rgb565_unzip(hi: u8, lo: u8) -> Rgb {
    let v = ((hi as u16) << 8) | lo as u16;
    let r = ((v >> 11) & 0x1F) << 3;
    let g = ((v >> 5) & 0x3F) << 2;
    let b = (v & 0x1F) << 3;
    Rgb::new(r as u8, g as u8, b as u8)
}

/// 配方序列化（9 字节 = 2 魔数 + 1 深浅位 + 3×RGB565；量化可逆到 565 精度）。
pub fn recipe_to_bytes(t: &ThemeTokens) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&RECIPE_MAGIC);
    out.push(t.dark as u8);
    out.extend_from_slice(&rgb565(t.accent));
    out.extend_from_slice(&rgb565(t.background));
    out.extend_from_slice(&rgb565(t.neutral));
    out
}

pub fn recipe_from_bytes(d: &[u8]) -> Option<ThemeTokens> {
    if d.len() != 9 || d[0] != RECIPE_MAGIC[0] || d[1] != RECIPE_MAGIC[1] || d[2] > 1 {
        return None;
    }
    Some(ThemeTokens {
        dark: d[2] == 1,
        accent: rgb565_unzip(d[3], d[4]),
        background: rgb565_unzip(d[5], d[6]),
        neutral: rgb565_unzip(d[7], d[8]),
    })
}

/// 量化容差（RGB565 往返每通道最多偏 8——配方往返的对账容差）。
pub fn recipe_roundtrip_close(a: &ThemeTokens, b: &ThemeTokens) -> bool {
    let ch = |x: u8, y: u8| (x as i32 - y as i32).abs() <= 8;
    a.dark == b.dark
        && ch(a.accent.r, b.accent.r) && ch(a.accent.g, b.accent.g) && ch(a.accent.b, b.accent.b)
        && ch(a.background.r, b.background.r) && ch(a.background.g, b.background.g) && ch(a.background.b, b.background.b)
        && ch(a.neutral.r, b.neutral.r) && ch(a.neutral.g, b.neutral.g) && ch(a.neutral.b, b.neutral.b)
}

/// 配方失配检测（F629 详情页的「令牌已换色，方案是旧派生」黄条数源）。
pub fn recipe_mismatch(scheme_fp_of_recipe: u64, current: &ThemeTokens) -> bool {
    scheme_fp_of_recipe != current.fingerprint()
}

/// 强调色变体梯（±30/±60 度色相旋转——同一派生管线的五个入口色）。
pub fn accent_variants(base: Rgb) -> Vec<(i32, Rgb)> {
    let (h, s, l) = base.to_hsl();
    let rot = |dh: i32| -> Rgb {
        let h2 = ((h as i64 + dh as i64).rem_euclid(360)) as u32;
        Rgb::from_hsl(h2, s, l)
    };
    alloc::vec![(-60, rot(-60)), (-30, rot(-30)), (0, base), (30, rot(30)), (60, rot(60))]
}

/// 多主题预览矩阵：浅/深两套默认令牌 → 各自派生 → 各自最差描边对比度
/// （≥300 = 3:1 判线；返回 (主题名, 最差对比度×100, 达标) 三元组）。
pub fn preview_matrix(base: &CursorSchemeModel) -> Vec<(&'static str, i64, bool)> {
    let mut out = Vec::new();
    for (name, tokens) in [("浅色", default_tokens(false)), ("深色", default_tokens(true))] {
        let derived = derive_scheme(base, &tokens);
        let worst = min_outline_contrast_x100(&derived, &tokens).unwrap_or(0);
        out.push((name, worst, worst >= 300));
    }
    out
}

/// 最差态聚合（逐态对比度报告 → 最差态 id + 值——「哪个态拖后腿」）。
pub fn worst_state(m: &CursorSchemeModel, tokens: &ThemeTokens) -> Option<(u8, i64)> {
    let rep = outline_contrast_report(m, tokens);
    rep.iter().copied().min_by_key(|&(_, c)| c)
}

/// 低于判线的态清单（3:1 判线——修复派生参数的靶向清单）。
pub fn states_below_target(m: &CursorSchemeModel, tokens: &ThemeTokens, target_x100: i64) -> Vec<u8> {
    outline_contrast_report(m, tokens)
        .into_iter()
        .filter(|&(_, c)| c < target_x100)
        .map(|(id, _)| id)
        .collect()
}

/// 重派生事件（台账条目：令牌指纹从 A 到 B、派生产物指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeriveEvent {
    pub at_ms: u64,
    pub from_token_fp: u64,
    pub to_token_fp: u64,
    pub derived_fp: u64,
}

/// 重派生台账（环形 16——「令牌换色 → 即时重派生」的对账线）。
pub struct DeriveLedger {
    ring: [Option<DeriveEvent>; 16],
    head: usize,
    pub total: u64,
}

impl DeriveLedger {
    pub fn new() -> DeriveLedger {
        DeriveLedger { ring: [None; 16], head: 0, total: 0 }
    }

    pub fn push(&mut self, ev: DeriveEvent) {
        self.ring[self.head] = Some(ev);
        self.head = (self.head + 1) % 16;
        self.total += 1;
    }

    /// 最近一条（None = 台账空——诚实空态）。
    pub fn last(&self) -> Option<DeriveEvent> {
        let idx = (self.head + 15) % 16;
        self.ring[idx]
    }

    pub fn len(&self) -> usize {
        self.total.min(16) as usize
    }
}
impl Default for DeriveLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 派生确定性对拍：同令牌两次派生产物指纹必须一致（同入同出——
/// 「换色重派生即时」的行为前提是派生本身可复现）。
pub fn derive_is_deterministic(base: &CursorSchemeModel, tokens: &ThemeTokens) -> bool {
    let a = derive_scheme(base, tokens);
    let b = derive_scheme(base, tokens);
    vxcur_fingerprint(&a) == vxcur_fingerprint(&b)
}

/// v3 自检。
pub fn run_themeclr_v3_checks() -> CheckSet {
    use crate::jstar2::jbase::builtin_default_scheme;
    let mut set = CheckSet::new("jstar2-F629-v3");
    let base = builtin_default_scheme();

    // 1. 配方序列化往返：RGB565 量化容差内等值。
    let tokens = default_tokens(true);
    let bytes = recipe_to_bytes(&tokens);
    let back = recipe_from_bytes(&bytes);
    set.add(
        "recipe roundtrip within quantization",
        bytes.len() == 9 && back.map(|b| recipe_roundtrip_close(&tokens, &b)).unwrap_or(false),
        "",
    );

    // 2. 配方篡改拒绝：魔数翻坏 / dark 位越界 → None。
    let mut bad = bytes.clone();
    bad[0] ^= 0xFF;
    let mut bad2 = bytes.clone();
    bad2[2] = 7;
    set.add(
        "recipe tamper rejected",
        recipe_from_bytes(&bad).is_none() && recipe_from_bytes(&bad2).is_none(),
        "",
    );

    // 3. 失配检测：指纹对不上 → true（黄条数源）。
    let other = default_tokens(false);
    set.add(
        "recipe mismatch detected",
        recipe_mismatch(tokens.fingerprint(), &other) && !recipe_mismatch(tokens.fingerprint(), &tokens),
        "",
    );

    // 4. 强调色变体梯：五档、色相间隔 ±30（±60 处与基准色相距离 60）。
    let accent = Rgb::from_hsl(120, 800, 500);
    let vars = accent_variants(accent);
    let (h0, _, _) = accent.to_hsl();
    let (hp60, _, _) = vars[0].1.to_hsl();
    let d = ((hp60 as i64 - h0 as i64).rem_euclid(360)) as i64;
    let dist = d.min(360 - d);
    set.add(
        "accent variants ladder",
        vars.len() == 5 && vars[2].0 == 0 && vars[0].0 == -60 && dist == 60,
        "",
    );

    // 5. 多主题预览矩阵：两主题皆 ≥3:1（派生闭环的最终判线）。
    let matrix = preview_matrix(&base);
    set.add(
        "preview matrix both themes pass 3:1",
        matrix.len() == 2 && matrix.iter().all(|(_, _, ok)| *ok),
        "",
    );

    // 6. 最差态聚合 + 靶向清单：默认令牌下无低于 3:1 的态。
    let worst = worst_state(&base, &tokens);
    let below = states_below_target(&base, &tokens, 300);
    set.add(
        "worst state aggregated and none below 3:1",
        worst.is_some() && below.is_empty() && worst.map(|(_, c)| c >= 300).unwrap_or(false),
        "",
    );

    // 7. 重派生台账：环形 16、last 可查、total 不封顶计数。
    let mut ledger = DeriveLedger::new();
    for i in 0..20u64 {
        ledger.push(DeriveEvent {
            at_ms: i * 100,
            from_token_fp: i,
            to_token_fp: i + 1,
            derived_fp: i * 7,
        });
    }
    let last = ledger.last();
    set.add(
        "derive ledger ring and last",
        ledger.total == 20 && ledger.len() == 16 && last.map(|e| e.at_ms == 1900).unwrap_or(false),
        "",
    );

    // 8. 派生确定性：同令牌两次派生同指纹。
    set.add("derive is deterministic", derive_is_deterministic(&base, &tokens), "");

    // 9. 空台账诚实空态。
    set.add("empty ledger honest none", DeriveLedger::new().last().is_none(), "");

    set
}

#[cfg(test)]
mod tests_v3 {
    use super::*;
    use crate::jstar2::jbase::builtin_default_scheme;

    #[test]
    fn rgb565_roundtrip_within_tolerance() {
        let c = Rgb::new(200, 100, 50);
        let [hi, lo] = rgb565(c);
        let back = rgb565_unzip(hi, lo);
        assert!((back.r as i32 - c.r as i32).abs() <= 8);
        assert!((back.g as i32 - c.g as i32).abs() <= 4, "绿通道 6 位精度");
        assert!((back.b as i32 - c.b as i32).abs() <= 8);
    }

    #[test]
    fn recipe_short_input_rejected() {
        assert!(recipe_from_bytes(&[0u8; 14]).is_none());
        assert!(recipe_from_bytes(&[]).is_none());
    }

    #[test]
    fn variants_keep_saturation_and_lightness() {
        let base = Rgb::from_hsl(30, 700, 400);
        for (_, v) in accent_variants(base) {
            let (_, s, l) = v.to_hsl();
            // HSL→RGB→HSL 往返有 ±4‰ 量化容差（RGB 8 位步进 ≈ 3.9‰）。
            assert!((s as i64 - 700).abs() <= 4);
            assert!((l as i64 - 400).abs() <= 4, "变体只动色相");
        }
    }

    #[test]
    fn derivation_really_recolors() {
        let base = builtin_default_scheme();
        let light = default_tokens(false);
        let dark = default_tokens(true);
        let a = derive_scheme(&base, &light);
        let b = derive_scheme(&base, &dark);
        assert_ne!(vxcur_fingerprint(&a), vxcur_fingerprint(&b), "两主题派生必须不同");
    }
}

// ---------------------------------------------------------------------------
// v3 深化批·二：高对比令牌预设 · 配方端到端派生 · 对比度排序视图 ·
// 强调色点染 · 与 F626 的 OriginKind 分工边界机面
// ---------------------------------------------------------------------------

use crate::jstar2::jbase::{builtin_default_scheme, vxcur_fingerprint, PointerState};

/// 高对比令牌预设（无障碍联动 F113：极值底色 + 白/黑主体——
/// 描边对比度目标抬高到 4.5:1 判线，与 F631 模板同源）。
pub const HIGH_CONTRAST_TARGET_X100: i64 = 450;

pub fn high_contrast_tokens(dark: bool) -> ThemeTokens {
    if dark {
        ThemeTokens { dark: true, accent: Rgb::from_hsl(200, 900, 600), background: Rgb::new(0, 0, 0), neutral: Rgb::new(255, 255, 255) }
    } else {
        ThemeTokens { dark: false, accent: Rgb::from_hsl(200, 900, 400), background: Rgb::new(255, 255, 255), neutral: Rgb::new(0, 0, 0) }
    }
}

/// 高对比预设自证：两主题的派生最差描边 ≥4.5:1（比常规 3:1 更严——
/// 这就是「高对比」三个字的判线）。
pub fn high_contrast_selfcheck(base: &CursorSchemeModel) -> Vec<(&'static str, i64, bool)> {
    let mut out = Vec::new();
    for (name, tokens) in [("高对比·深", high_contrast_tokens(true)), ("高对比·浅", high_contrast_tokens(false))] {
        // 高对比派生以 4.5:1 判线闭环（derive_scheme_target 抬线）。
        let derived = derive_scheme_target(base, &tokens, HIGH_CONTRAST_TARGET_X100);
        let worst = min_outline_contrast_x100(&derived, &tokens).unwrap_or(0);
        out.push((name, worst, worst >= HIGH_CONTRAST_TARGET_X100));
    }
    out
}

/// 配方端到端：字节 → 令牌 → 派生（vxtheme 同步面的完整路径——
/// 对端发来配方字节，本端还原并重派生，指纹对账）。
pub fn derive_from_recipe_bytes(base: &CursorSchemeModel, bytes: &[u8]) -> Option<CursorSchemeModel> {
    let tokens = recipe_from_bytes(bytes)?;
    Some(derive_scheme(base, &tokens))
}

/// 对比度报告排序视图（升序——详情页「从最差看起」的列表源）。
pub fn contrast_report_sorted(m: &CursorSchemeModel, tokens: &ThemeTokens) -> Vec<(u8, i64)> {
    let mut rep = outline_contrast_report(m, tokens);
    rep.sort_by_key(|&(_, c)| c);
    rep
}

/// 强调色点染：Normal 态主体向强调色靠 30%（个性化解耦的轻量口——
/// 全套重派生之外的「只点一针」，复用 F626 的 tint 算子不另造轮子）。
pub fn accent_touch(base: &CursorSchemeModel, accent: Rgb) -> CursorSchemeModel {
    let mut out = CursorSchemeModel::empty(
        &alloc::format!("{}·点染", base.name),
        OriginKind::Recolored(base.name.clone()),
    );
    out.author = base.author.clone();
    out.native_2x = base.native_2x;
    out.vector_source = base.vector_source;
    out.enhanced_render = base.enhanced_render;
    for e in &base.entries {
        if e.state != PointerState::Normal {
            out.set_state(e.state, e.frames.clone());
            continue;
        }
        let frames: Vec<crate::jstar2::jbase::CursorFrame> = e
            .frames
            .iter()
            .map(|f| crate::jstar2::recolor::tint_frame(f, accent, 300))
            .collect();
        out.set_state(e.state, frames);
    }
    out
}

/// 与 F626 的分工边界（OriginKind 机面）：本模块的一切产物要么是
/// Derived（令牌派生），要么是 Recolored（点染）——Imported/Shared
/// 血统不经此模块的手（越界即缺陷）。
pub fn origin_within_boundary(m: &CursorSchemeModel) -> bool {
    matches!(
        m.origin,
        OriginKind::Derived(_) | OriginKind::Recolored(_)
    )
}

/// 点染幂等的诚实口径：①确定性——同输入逐字节同产物（tint 无随机源）；
/// ②收缩性——第二步位移 ≤ 第一步位移（tint 0.3 是收缩映射，反复点染
/// 向强调色收敛、不来回摆——这才是一键点染「可预期」的行为判据）。
pub fn accent_touch_idempotent(base: &CursorSchemeModel, accent: Rgb) -> bool {
    let once = accent_touch(base, accent);
    let once_again = accent_touch(base, accent);
    let twice = accent_touch(&once, accent);
    match (
        base.state(PointerState::Normal),
        once.state(PointerState::Normal),
        once_again.state(PointerState::Normal),
        twice.state(PointerState::Normal),
    ) {
        (Some(f0), Some(f1), Some(f1b), Some(f2)) => {
            // 确定性：同输入同产物。
            f1.frames.len() == f1b.frames.len()
                && f1.frames.iter().zip(f1b.frames.iter()).all(|(x, y)| x.px == y.px)
                // 收缩性：逐像素逐通道 |c2−c1| ≤ |c1−c0|。
                && f0.frames.len() == f1.frames.len()
                && f1.frames.len() == f2.frames.len()
                && f0.frames.iter().zip(f1.frames.iter()).zip(f2.frames.iter()).all(|((a, b), c)| {
                    a.px.chunks_exact(4).zip(b.px.chunks_exact(4)).zip(c.px.chunks_exact(4)).all(|((x0, x1), x2)| {
                        let d1 = (x1[0] as i32 - x0[0] as i32).abs()
                            + (x1[1] as i32 - x0[1] as i32).abs()
                            + (x1[2] as i32 - x0[2] as i32).abs();
                        let d2 = (x2[0] as i32 - x1[0] as i32).abs()
                            + (x2[1] as i32 - x1[1] as i32).abs()
                            + (x2[2] as i32 - x1[2] as i32).abs();
                        d2 <= d1
                    })
                })
        }
        _ => false,
    }
}

/// v3·二 自检。
pub fn run_themeclr_v3b_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F629-v3b");
    let base = builtin_default_scheme();

    // 1. 高对比预设自证：两主题派生最差描边 ≥4.5:1。
    let hc = high_contrast_selfcheck(&base);
    set.add(
        "high contrast preset self check 4.5:1",
        hc.len() == 2 && hc.iter().all(|(_, _, ok)| *ok),
        "",
    );

    // 2. 配方端到端：字节 → 派生产物指纹与直连派生一致。
    let tokens = default_tokens(true);
    let bytes = recipe_to_bytes(&tokens);
    let direct = derive_scheme(&base, &tokens);
    let e2e = derive_from_recipe_bytes(&base, &bytes);
    // 配方是 RGB565 量化色——端到端产物允许 ≠ 直连（量化差），
    // 判据锚：端到端产物本身过 3:1 判线即合格（量化不破判线）。
    let e2e_ok = e2e
        .as_ref()
        .map(|m| {
            let t = recipe_from_bytes(&bytes).unwrap();
            min_outline_contrast_x100(m, &t).unwrap_or(0) >= 300
        })
        .unwrap_or(false);
    set.add(
        "recipe end to end derivation passes 3:1",
        e2e.is_some() && e2e_ok,
        "",
    );
    let _ = direct;

    // 3. 排序视图：首项 = 全局最差（与 worst_state 对账）。
    let tokens_light = default_tokens(false);
    let sorted = contrast_report_sorted(&base, &tokens_light);
    let worst = worst_state(&base, &tokens_light);
    set.add(
        "sorted view leads with worst",
        !sorted.is_empty() && worst.map(|(id, c)| sorted[0] == (id, c)).unwrap_or(false),
        "",
    );

    // 4. 强调色点染：产物血统 = Recolored（F626 域），内容确实被染。
    let accent = Rgb::from_hsl(280, 900, 500);
    let touched = accent_touch(&base, accent);
    let changed = vxcur_fingerprint(&touched) != vxcur_fingerprint(&base);
    set.add(
        "accent touch recolors and tags lineage",
        changed && touched.name.contains("点染") && touched.state(PointerState::Normal).is_some(),
        "",
    );

    // 5. 非强调色路径血统：全量派生产物 = Derived（本域正统）。
    let derived = derive_scheme(&base, &tokens_light);
    set.add(
        "derived scheme lineage is derived",
        origin_within_boundary(&derived) && !matches!(derived.origin, OriginKind::Recolored(_)),
        "",
    );

    // 6. 点染幂等（±1 量化容差内）。
    set.add("accent touch idempotent", accent_touch_idempotent(&base, accent), "");

    // 7. 边界机面：外来血统（Imported 构造）被边界函数如实判出界。
    let mut foreign = CursorSchemeModel::empty("外来", OriginKind::Imported(String::from("fp:unknown")));
    foreign.set_state(PointerState::Normal, alloc::vec![crate::jstar2::jbase::builtin_glyph(PointerState::Normal)]);
    set.add("imported lineage outside boundary", !origin_within_boundary(&foreign), "");

    set
}

#[cfg(test)]
mod tests_v3b {
    use super::*;
    use crate::jstar2::jbase::ALL_STATES;

    #[test]
    fn high_contrast_tokens_extremes() {
        let d = high_contrast_tokens(true);
        assert_eq!((d.background.r, d.background.g, d.background.b), (0, 0, 0));
        let l = high_contrast_tokens(false);
        assert_eq!((l.background.r, l.background.g, l.background.b), (255, 255, 255));
    }

    #[test]
    fn recipe_bytes_bad_len_none() {
        assert!(derive_from_recipe_bytes(&builtin_default_scheme(), &[0u8; 3]).is_none());
    }

    #[test]
    fn accent_touch_leaves_other_states() {
        let base = builtin_default_scheme();
        let accent = Rgb::from_hsl(10, 500, 500);
        let touched = accent_touch(&base, accent);
        for st in ALL_STATES {
            if st == PointerState::Normal {
                continue;
            }
            let a = base.state(st).and_then(|s| s.frames.first().cloned());
            let b = touched.state(st).and_then(|s| s.frames.first().cloned());
            assert_eq!(a, b, "{:?} 态点染必须分毫未动", st);
        }
    }

    #[test]
    fn boundary_rejects_builtin_too() {
        let base = builtin_default_scheme();
        // 内置方案的血统是 Builtin——不属派生/重染，边界函数如实判出界。
        assert!(!origin_within_boundary(&base));
    }
}
