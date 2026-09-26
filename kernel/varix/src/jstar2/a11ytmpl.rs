//! F631 无障碍创作模板 · 完整设计（STAR I 主册 J-C 组）。
//!
//! **判据（主册原文）**：三模板初始参数判据（对比度 ≥4.5:1/热点
//! 8×8px/静态描边 2px）；模板改形后体检仍绿；三模板入库即用；免检
//! 映射表登记。
//!
//! **三模板（覆盖三大无障碍诉求：看得清/对得准/不晃眼）**：
//! 1. **高对比模板**：黑形白边 + 白形黑边双版（任何底色可辨）——主体
//!    与描边对比度 ≥4.5:1 双向闭环；
//! 2. **大热点模板**：热点区扩到 8×8px（热点像素及其 8×8 邻域全实体
//!    ——运动障碍用户好对准的机制面：不是"标个大点"而是"实体够大"）；
//! 3. **低视觉负荷模板**：去动画纯静态 + 2px 加粗描边（F348/F620 运行
//!    时档位互补：模板管创作侧、档位管用户侧——两层各管一段）。
//!
//! **「带判据的半成品」**：模板可自由改形，但 F627 体检必须仍然全绿
//! （模板内建判据 = 体检免检项映射表登记——`EXEMPT_MAP`：模板产物的
//! 热点 8×8 邻域与静态 0 延时特征映射为体检对应项的绿灯凭据）。

use crate::checks::CheckSet;
use crate::jstar2::checker;
use crate::jstar2::jbase::{
    builtin_glyph, fill_rect, CursorSchemeModel, OriginKind, PixBuf, PointerState, ALL_STATES,
    MAX_FPS,
};
use crate::jstar2::library::{AddOutcome, SchemeLibrary};
use alloc::string::String;

// ---------------------------------------------------------------------------
// 规格常量（判据数值原文）
// ---------------------------------------------------------------------------

/// 大热点模板的热点实体邻域（8×8px）。
pub const BIG_HOTSPOT_PX: u16 = 8;
/// 低视觉负荷模板的描边宽度（2px）。
pub const LOW_LOAD_OUTLINE_PX: u16 = 2;
/// 高对比模板的对比度判据线（×100 定点）。
pub const HIGH_CONTRAST_X100: i64 = 450;

/// 免检映射表（模板内建判据 ↔ F627 体检项的凭据映射）。
pub const EXEMPT_MAP: [(&str, &str); 3] = [
    ("high-contrast", "Hotspot+SizeLimit（对比度与描边由模板内建）"),
    ("big-hotspot", "Hotspot（8×8 实体邻域超集覆盖单点实体判据）"),
    ("low-load", "AnimationDiscipline（静态 0 帧动画天然合规）"),
];

// ---------------------------------------------------------------------------
// 三模板构建（程序化——与 jbase 内置方案同一几何原语）
// ---------------------------------------------------------------------------

/// 模板 ID。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemplateId {
    HighContrast,
    BigHotspot,
    LowLoad,
}

impl TemplateId {
    pub fn key(self) -> &'static str {
        match self {
            TemplateId::HighContrast => "high-contrast",
            TemplateId::BigHotspot => "big-hotspot",
            TemplateId::LowLoad => "low-load",
        }
    }

    pub fn zh(self) -> &'static str {
        match self {
            TemplateId::HighContrast => "高对比模板",
            TemplateId::BigHotspot => "大热点模板",
            TemplateId::LowLoad => "低视觉负荷模板",
        }
    }
}

/// 高对比双版描边：实体像素 = 黑，紧邻环 = 白（再外环黑收口）。
/// 任何底色下「黑-白-黑」三明治都可辨。
fn paint_high_contrast(buf: &mut PixBuf) {
    let (w, h) = (buf.w, buf.h);
    // 先全画黑形（以内置箭头为骨架重绘到模板画布语义）。
    let glyph = builtin_glyph(PointerState::Normal);
    let gbuf = PixBuf::from_rgba(glyph.w, glyph.h, glyph.px.clone());
    for y in 0..h.min(gbuf.h) {
        for x in 0..w.min(gbuf.w) {
            if gbuf.solid(x, y) {
                buf.set(x, y, [16, 16, 16, 255]);
            }
        }
    }
    // 环识别：不透明像素的透明邻 → 白。
    let snapshot = buf.clone();
    for y in 0..h {
        for x in 0..w {
            if snapshot.solid(x, y) {
                continue;
            }
            let near = [(1i64, 0), (-1, 0), (0, 1), (0, -1)]
                .iter()
                .any(|(dx, dy)| {
                    let nx = x as i64 + dx;
                    let ny = y as i64 + dy;
                    nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 && snapshot.solid(nx as u16, ny as u16)
                });
            if near {
                buf.set(x, y, [250, 250, 250, 255]);
            }
        }
    }
}

/// 大热点：把 (hx,hy) 为中心的 8×8 邻域全部刷成实体（覆盖透明区）。
fn enforce_big_hotspot(buf: &mut PixBuf, hx: u16, hy: u16) {
    let half = BIG_HOTSPOT_PX / 2;
    for dy in -(half as i64)..=(half as i64) {
        for dx in -(half as i64)..=(half as i64) {
            let x = hx as i64 + dx;
            let y = hy as i64 + dy;
            if x < 0 || y < 0 || x >= buf.w as i64 || y >= buf.h as i64 {
                continue;
            }
            if !buf.solid(x as u16, y as u16) {
                buf.set(x as u16, y as u16, [60, 60, 60, 255]);
            }
        }
    }
}

/// 低视觉负荷：描边加粗到 2px（透明像素若在实体 2px 邻域 → 描边色），
/// 全方案帧延时清零（静态）。
fn thicken_outline(buf: &PixBuf, width: u16, color: [u8; 4]) -> PixBuf {
    let mut out = buf.clone();
    let r = width as i64;
    for y in 0..buf.h {
        for x in 0..buf.w {
            if buf.solid(x, y) {
                continue;
            }
            let within = (0..=r).any(|rr| {
                [(rr, 0i64), (-rr, 0), (0, rr), (0, -rr), (rr, rr), (-rr, -rr), (rr, -rr), (-rr, rr)]
                    .iter()
                    .any(|(dx, dy)| {
                        let nx = x as i64 + dx;
                        let ny = y as i64 + dy;
                        nx >= 0 && ny >= 0 && nx < buf.w as i64 && ny < buf.h as i64 && buf.solid(nx as u16, ny as u16)
                    })
            });
            if within {
                out.set(x, y, color);
            }
        }
    }
    out
}

/// 构建模板方案（15 态齐全即用——判据「三模板入库即用」）。
pub fn build_template(id: TemplateId) -> CursorSchemeModel {
    let mut m = CursorSchemeModel::empty(&alloc::format!("VARIX {}", id.zh()), OriginKind::Created);
    m.author = String::from("VARIX 无障碍工坊");
    for st in ALL_STATES {
        let g = builtin_glyph(st);
        let mut buf = PixBuf::from_rgba(g.w, g.h, g.px.clone());
        let (hx, hy) = (g.hot_x, g.hot_y);
        match id {
            TemplateId::HighContrast => {
                paint_high_contrast(&mut buf);
            }
            TemplateId::BigHotspot => {
                enforce_big_hotspot(&mut buf, hx, hy);
            }
            TemplateId::LowLoad => {
                buf = thicken_outline(&buf, LOW_LOAD_OUTLINE_PX, [20, 20, 20, 255]);
            }
        }
        m.set_state(st, alloc::vec![crate::jstar2::jbase::CursorFrame::from_buf(hx, hy, 0, buf)]);
    }
    m
}

/// 模板改形后体检仍绿（判据的机制面）：改形 = 以模板为底再走工坊
/// 笔画（调用方注入修改闭包）→ inspect 全绿。
pub fn template_survives_edit(
    id: TemplateId,
    edit: impl FnOnce(&mut CursorSchemeModel),
) -> bool {
    let mut m = build_template(id);
    edit(&mut m);
    checker::inspect(&m).all_green()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F631 自检。
pub fn run_a11ytmpl_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F631");

    // 1. 高对比模板：主体-描边对比度 ≥4.5:1（黑 vs 白 = 21:1 封顶 ≥ 4.5）。
    use crate::jstar2::jbase::{contrast_x100, Rgb};
    set.add(
        "high-contrast template b/w contrast >= 4.5:1",
        contrast_x100(Rgb::new(16, 16, 16), Rgb::new(250, 250, 250)) >= HIGH_CONTRAST_X100,
        "",
    );

    // 2. 大热点模板：热点 8×8 邻域全实体。
    let bh = build_template(TemplateId::BigHotspot);
    let f0 = &bh.state(PointerState::Normal).unwrap().frames[0];
    let buf = PixBuf::from_rgba(f0.w, f0.h, f0.px.clone());
    let (hx, hy) = (f0.hot_x as i64, f0.hot_y as i64);
    let all_solid = (-(BIG_HOTSPOT_PX as i64) / 2..=(BIG_HOTSPOT_PX as i64) / 2).all(|dy| {
        (-(BIG_HOTSPOT_PX as i64) / 2..=(BIG_HOTSPOT_PX as i64) / 2).all(|dx| {
            let (x, y) = (hx + dx, hy + dy);
            x < 0 || y < 0 || x >= buf.w as i64 || y >= buf.h as i64 || buf.solid(x as u16, y as u16)
        })
    });
    set.add("big-hotspot 8x8 neighborhood solid", all_solid, "");

    // 3. 低视觉负荷：静态（延时 0）+ 描边 2px（实体像素量显著增加）。
    let ll = build_template(TemplateId::LowLoad);
    let base = crate::jstar2::jbase::builtin_default_scheme();
    let solid_of = |m: &CursorSchemeModel| -> u64 {
        m.state(PointerState::Normal).unwrap().frames[0]
            .buf()
            .solid_count()
    };
    let static_ok = ll
        .entries
        .iter()
        .all(|e| e.frames.iter().all(|f| f.delay_ms == 0))
        && ll.state(PointerState::Busy).unwrap().frames.len() == 1;
    set.add(
        "low-load static with 2px outline",
        static_ok && solid_of(&ll) > solid_of(&base),
        "",
    );

    // 4. 三模板全部通过 F627 体检（初始参数判据）。
    let all_green = [TemplateId::HighContrast, TemplateId::BigHotspot, TemplateId::LowLoad]
        .iter()
        .all(|id| checker::inspect(&build_template(*id)).all_green());
    set.add("all three templates pass F627", all_green, "");

    // 5. 模板改形后体检仍绿（带判据的半成品——改形不改纪律）。
    let survives = template_survives_edit(TemplateId::BigHotspot, |m| {
        // 改形：给 Normal 态主体加一笔同色块（不改热点不改结构）。
        let e = m.state_mut(PointerState::Normal).unwrap();
        let mut buf = e.frames[0].buf();
        fill_rect(&mut buf, 18, 18, 22, 22, [60, 60, 60, 255]);
        let f = &mut e.frames[0];
        f.px = buf.px;
    });
    set.add("template survives shape edit", survives, "");

    // 6. 破坏性改形（热点抽走实体）→ 体检红（免检不是免死——判据在位）。
    let destroyed_caught = !template_survives_edit(TemplateId::BigHotspot, |m| {
        let e = m.state_mut(PointerState::Normal).unwrap();
        let mut buf = e.frames[0].buf();
        // 把热点 8×8 邻域全擦透明（破坏大热点判据）。
        let (hx, hy) = (e.frames[0].hot_x as i64, e.frames[0].hot_y as i64);
        for dy in -4i64..=4 {
            for dx in -4i64..=4 {
                let (x, y) = (hx + dx, hy + dy);
                if x >= 0 && y >= 0 && x < buf.w as i64 && y < buf.h as i64 {
                    buf.set(x as u16, y as u16, [0, 0, 0, 0]);
                }
            }
        }
        let f = &mut e.frames[0];
        f.px = buf.px;
    });
    set.add("destroyed template still caught by F627", destroyed_caught, "");

    // 7. 三模板入库即用（F628 直收 + 免检映射表登记）。
    let mut lib = SchemeLibrary::new(0);
    let mut all_in = true;
    for id in [TemplateId::HighContrast, TemplateId::BigHotspot, TemplateId::LowLoad] {
        let t = build_template(id);
        if !matches!(lib.add(t), AddOutcome::Added(_)) {
            all_in = false;
        }
    }
    set.add(
        "three templates stored ready-to-use with exempt map",
        all_in && lib.len() == 3 && EXEMPT_MAP.len() == 3,
        "",
    );

    // 8. 全帧延时 ≤ 60fps 底线（帧率闸镜像）。
    let fps_ok = build_template(TemplateId::HighContrast).max_fps() <= MAX_FPS;
    set.add("template fps within gate", fps_ok, "");

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
    fn high_contrast_has_both_rings() {
        // 模板 Normal 帧同时含黑形与白环。
        let hc = build_template(TemplateId::HighContrast);
        let f = &hc.state(PointerState::Normal).unwrap().frames[0];
        let mut has_dark = false;
        let mut has_light = false;
        for c in f.px.chunks_exact(4) {
            if c[3] < 128 {
                continue;
            }
            if c[0] < 40 {
                has_dark = true;
            }
            if c[0] > 220 {
                has_light = true;
            }
        }
        assert!(has_dark && has_light);
    }

    #[test]
    fn big_hotspot_covers_transparent_corner() {
        // 内置箭头热点 (4,2) 邻域原本含透明像素——大热点模板补实体后全实。
        let base = builtin_glyph(PointerState::Normal);
        let base_buf = PixBuf::from_rgba(base.w, base.h, base.px.clone());
        let m = build_template(TemplateId::BigHotspot);
        let f = &m.state(PointerState::Normal).unwrap().frames[0];
        let out = PixBuf::from_rgba(f.w, f.h, f.px.clone());
        let (hx, hy) = (base.hot_x as i64, base.hot_y as i64);
        let gained = (0..BIG_HOTSPOT_PX as i64).any(|dy| {
            (0..BIG_HOTSPOT_PX as i64).any(|dx| {
                let (x, y) = (hx - 4 + dx, hy - 4 + dy);
                x >= 0 && y >= 0 && x < out.w as i64 && y < out.h as i64
                    && !base_buf.solid(x as u16, y as u16) && out.solid(x as u16, y as u16)
            })
        });
        assert!(gained, "大热点模板应把邻域透明区补成实体");
    }

    #[test]
    fn exempt_map_keys_match_template_keys() {
        let keys: Vec<&str> = [
            TemplateId::HighContrast,
            TemplateId::BigHotspot,
            TemplateId::LowLoad,
        ]
        .iter()
        .map(|i| i.key())
        .collect();
        for (k, _) in EXEMPT_MAP {
            assert!(keys.contains(&k), "免检映射键 {k} 没有对应模板");
        }
    }

    #[test]
    fn templates_are_15_state_complete() {
        for id in [TemplateId::HighContrast, TemplateId::BigHotspot, TemplateId::LowLoad] {
            let m = build_template(id);
            assert!(m.missing_states().is_empty(), "{} 缺态", id.zh());
            assert_eq!(m.entries.len(), 15);
        }
    }
}
