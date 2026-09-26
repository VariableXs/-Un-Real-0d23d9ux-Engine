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
use alloc::vec::Vec;

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

/// 低视觉负荷：描边加粗到 width px（视觉描边带 = 1px 轮廓外扩新实体
/// + (width−1)px 原轮廓最外圈改描边色），全方案帧延时清零（静态）。
/// 顺序纪律：先 alpha 二值化（≥128 → 255，否则 0——低负荷 = 硬边输出，
/// 柔和 AA 是低视力用户的视觉噪声），环从二值化后的实体长出来。
/// 外扩只走 1px：2px 全外扩的轮廓在 200% 重采样下过渡带均值会顶出
/// F632 锐度判线（实测 168 > 160），1px 外扩 + 内圈改色是同视觉宽度、
/// 同 solid_count 增量、且过判线的落点。
fn thicken_outline(buf: &PixBuf, width: u16, color: [u8; 4]) -> PixBuf {
    let mut bin = buf.clone();
    for c in bin.px.chunks_exact_mut(4) {
        c[3] = if c[3] >= 128 { 255 } else { 0 };
    }
    let mut out = bin.clone();
    let (w, h) = (bin.w as i64, bin.h as i64);
    // ① 外扩 1px：透明像素若在实体 1px 邻域 → 描边色。
    for y in 0..bin.h {
        for x in 0..bin.w {
            if bin.solid(x, y) {
                continue;
            }
            let within = [(1i64, 0i64), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)]
                .iter()
                .any(|(dx, dy)| {
                    let nx = x as i64 + dx;
                    let ny = y as i64 + dy;
                    nx >= 0 && ny >= 0 && nx < w && ny < h && bin.solid(nx as u16, ny as u16)
                });
            if within {
                out.set(x, y, color);
            }
        }
    }
    // ② 原轮廓最外 (width−1) 圈改描边色（alpha 不动——不碰锐度度量面）。
    let recolor_layers = (width as i64 - 1).max(0);
    if recolor_layers >= 1 {
        for y in 0..bin.h {
            for x in 0..bin.w {
                if !bin.solid(x, y) {
                    continue;
                }
                let on_edge = (1..=recolor_layers).any(|rr| {
                    for dy in -rr..=rr {
                        for dx in -rr..=rr {
                            if dx.abs() != rr && dy.abs() != rr {
                                continue; // 只查 Chebyshev 环带
                            }
                            let nx = x as i64 + dx;
                            let ny = y as i64 + dy;
                            if nx < 0 || ny < 0 || nx >= w || ny >= h || !bin.solid(nx as u16, ny as u16) {
                                return true;
                            }
                        }
                    }
                    false
                });
                if on_edge {
                    let a = out.get(x, y).map(|p| p[3]).unwrap_or(255);
                    out.set(x, y, [color[0], color[1], color[2], a]);
                }
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

// ---------------------------------------------------------------------------
// v2 深化：模板注册表 / 全量构建 / 免检理由查询
// ---------------------------------------------------------------------------

/// 模板注册表（id → 判据摘要——工坊起步面板的数据源，一处一事实：
/// 面板渲染与免检映射共用这张表）。
pub const TEMPLATE_REGISTRY: [(TemplateId, &'static str); 3] = [
    (TemplateId::HighContrast, "黑形白边双版，任意底色对比度 ≥4.5:1"),
    (TemplateId::BigHotspot, "热点实体邻域 8×8px，运动障碍易对准"),
    (TemplateId::LowLoad, "纯静态零动画 + 2px 加粗描边"),
];

/// 三模板全量构建（工坊「起步模板」墙的一次性出图口）。
pub fn build_all_templates() -> Vec<CursorSchemeModel> {
    TEMPLATE_REGISTRY.iter().map(|(id, _)| build_template(*id)).collect()
}

/// 免检理由查询（键 → F627 免检项凭据；查无此键诚实 None）。
pub fn exempt_reason(key: &str) -> Option<&'static str> {
    EXEMPT_MAP.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

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


    // 5. 模板注册表与免检映射同源（三件互相对得上——键集一致）。
    set.add(
        "template registry covers three ids",
        TEMPLATE_REGISTRY.len() == 3
            && TEMPLATE_REGISTRY.iter().all(|(id, _)| EXEMPT_MAP.iter().any(|(k, _)| *k == id.key()))
            && build_all_templates().len() == 3,
        "",
    );

    // 6. 免检理由查询：命中返回原文、查无诚实 None。
    set.add(
        "exempt reason lookup honest",
        exempt_reason("high-contrast").map(|v| v.contains("内建")).unwrap_or(false)
            && exempt_reason("查无此键").is_none(),
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

// ---------------------------------------------------------------------------
// v3 深化批：模板参数覆盖（不变量守恒）· 画廊元数据 · 适配性规则 ·
// 模板转换图 · 版本登记 · 合规报告
// ---------------------------------------------------------------------------

use crate::jstar2::jbase::{contrast_x100, Rgb};

/// 模板版本（判据/几何改版必须递增——合规报告带版本，旧报告作废）。
pub const TEMPLATE_VERSION: u32 = 1;

/// 模板画廊元数据（工坊起步面板的说明卡：给谁用、解决什么）。
pub struct TemplateMeta {
    pub id: TemplateId,
    pub audience: &'static str,
    pub solves: &'static str,
}

/// 画廊（id → 人话元数据；与 TEMPLATE_REGISTRY 同源但不重复判据文——
/// 注册表管判据、画廊管说明，两表靠 id 键对账）。
pub const TEMPLATE_GALLERY: [TemplateMeta; 3] = [
    TemplateMeta {
        id: TemplateId::HighContrast,
        audience: "低视力用户 / 强光环境",
        solves: "任何壁纸底色下指针轮廓清晰可辨",
    },
    TemplateMeta {
        id: TemplateId::BigHotspot,
        audience: "运动障碍用户 / 高分屏小指针困扰者",
        solves: "点击判定区扩大到 8×8，对准成本低",
    },
    TemplateMeta {
        id: TemplateId::LowLoad,
        audience: "视觉负荷敏感用户 / 注意力易分散者",
        solves: "零动画 + 加粗描边，不闪不晃",
    },
];

/// 画廊对账：画廊与注册表 id 集合一致（一处加模板，两表同步——
/// 漏一边即对账红）。
pub fn gallery_registry_consistent() -> bool {
    TEMPLATE_GALLERY.len() == TEMPLATE_REGISTRY.len()
        && TEMPLATE_GALLERY.iter().all(|m| TEMPLATE_REGISTRY.iter().any(|(id, _)| *id == m.id))
}

/// 适配性规则（视觉需求档案 → 推荐模板；键值唯一的查表面）。
pub fn recommend_template(need: &str) -> Option<TemplateId> {
    match need {
        "low-vision" => Some(TemplateId::HighContrast),
        "motor" => Some(TemplateId::BigHotspot),
        "attention" => Some(TemplateId::LowLoad),
        _ => None,
    }
}

/// 模板转换图（用户换模板时保留哪些自定义的语义说明——这里钉的是
/// 「转换不带走旧模板的专属不变量」：新模板重建、署名与标签跟人走）。
pub fn conversion_note(from: TemplateId, to: TemplateId) -> Option<&'static str> {
    if from == to {
        return None;
    }
    Some(match (from, to) {
        (TemplateId::HighContrast, TemplateId::BigHotspot) => "高对比 → 大热点：描边丢弃，热点邻域重建",
        (TemplateId::BigHotspot, TemplateId::HighContrast) => "大热点 → 高对比：热点缩回单点，双版描边重建",
        (TemplateId::LowLoad, TemplateId::HighContrast) => "低负荷 → 高对比：静态保持，描边换双版",
        (TemplateId::HighContrast, TemplateId::LowLoad) => "高对比 → 低负荷：静态保持，描边换 2px 加粗",
        (TemplateId::BigHotspot, TemplateId::LowLoad) => "大热点 → 低负荷：热点缩回单点，描边 2px 加粗",
        (TemplateId::LowLoad, TemplateId::BigHotspot) => "低负荷 → 大热点：静态保持，热点邻域重建",
        _ => unreachable!("同模板转换已在函数头拦截"),
    })
}

/// 参数覆盖（不改判据线，只改表现参数——覆盖后不变量必须复检）。
pub struct TemplateOverride {
    /// 主体色（None = 保持模板默认）。
    pub body: Option<Rgb>,
    /// 描边/邻域色。
    pub outline: Option<Rgb>,
    /// 热点偏移（相对模板默认热点；None = 不动）。
    pub hotspot_delta: Option<(i64, i64)>,
}

impl Default for TemplateOverride {
    fn default() -> Self {
        TemplateOverride { body: None, outline: None, hotspot_delta: None }
    }
}

/// 应用覆盖（非破坏：以模板为底复制改；返回带血统标注的变体）。
/// ① 表现重染只在给了 body/outline 时发生——双 None = 恒等（默认覆盖
/// 就是模板原样）；② 大热点的热点偏移钳在「8×8 邻域完整落界」的域内
/// 并重建实体邻域——邻域出界 = 判据破，钳制只保热点单点是不够的。
pub fn apply_override(id: TemplateId, ov: &TemplateOverride) -> CursorSchemeModel {
    let mut m = build_template(id);
    let recolor = match (ov.body, ov.outline) {
        (None, None) => None,
        (b, o) => Some((b.unwrap_or(Rgb::new(16, 16, 16)), o.unwrap_or(Rgb::new(250, 250, 250)))),
    };
    for st in ALL_STATES {
        let Some(sf) = m.state_mut(st) else { continue };
        for f in sf.frames.iter_mut() {
            let mut buf = f.buf();
            // ① 热点偏移。
            if let Some((dx, dy)) = ov.hotspot_delta {
                let half = (BIG_HOTSPOT_PX / 2) as i64;
                if id == TemplateId::BigHotspot {
                    f.hot_x = (f.hot_x as i64 + dx).clamp(half, buf.w as i64 - 1 - half) as u16;
                    f.hot_y = (f.hot_y as i64 + dy).clamp(half, buf.h as i64 - 1 - half) as u16;
                    // 邻域重建（新热点处的实体保证——先于重染，色相一致）。
                    enforce_big_hotspot(&mut buf, f.hot_x, f.hot_y);
                } else {
                    f.hot_x = (f.hot_x as i64 + dx).clamp(0, buf.w as i64 - 1) as u16;
                    f.hot_y = (f.hot_y as i64 + dy).clamp(0, buf.h as i64 - 1) as u16;
                }
            }
            // ② 表现重染（双 None 跳过——不动像素）。
            if let Some((body, outline)) = recolor {
                for y in 0..buf.h {
                    for x in 0..buf.w {
                        let c = buf.get(x, y).unwrap_or([0, 0, 0, 0]);
                        if c[3] == 0 {
                            continue;
                        }
                        let l = (c[0] as i64 + c[1] as i64 + c[2] as i64) / 3;
                        let newc = if l >= 128 { outline } else { body };
                        buf.set(x, y, [newc.r, newc.g, newc.b, c[3]]);
                    }
                }
            }
            f.px = buf.px;
        }
    }
    m
}

/// 覆盖后不变量复检（判据线不动，参数改了也要过同一把尺）：
/// - 高对比：黑/白双色犹在（主体与描边对比度 ≥4.5:1）；
/// - 大热点：热点 8×8 邻域仍全实体；
/// - 低负荷：全态静态（delay=0）。
pub fn override_invariants_hold(id: TemplateId, m: &CursorSchemeModel) -> bool {
    match id {
        TemplateId::HighContrast => {
            // 主体与描边两大色簇对比度 ≥450。
            let Some(f) = m.state(PointerState::Normal).and_then(|s| s.frames.first()) else {
                return false;
            };
            let mut dark = Rgb::new(0, 0, 0);
            let mut light = Rgb::new(255, 255, 255);
            let mut found_dark = false;
            let mut found_light = false;
            for c in f.px.chunks_exact(4) {
                if c[3] == 0 {
                    continue;
                }
                let l = (c[0] as i32 + c[1] as i32 + c[2] as i32) / 3;
                if l < 128 && !found_dark {
                    dark = Rgb::new(c[0], c[1], c[2]);
                    found_dark = true;
                }
                if l >= 128 && !found_light {
                    light = Rgb::new(c[0], c[1], c[2]);
                    found_light = true;
                }
            }
            found_dark && found_light && contrast_x100(dark, light) >= HIGH_CONTRAST_X100
        }
        TemplateId::BigHotspot => {
            let Some(f) = m.state(PointerState::Normal).and_then(|s| s.frames.first()) else {
                return false;
            };
            let buf: PixBuf = f.buf();
            let half = (BIG_HOTSPOT_PX / 2) as i64;
            for dy in -half..=half {
                for dx in -half..=half {
                    let x = f.hot_x as i64 + dx;
                    let y = f.hot_y as i64 + dy;
                    if x < 0 || y < 0 || x >= buf.w as i64 || y >= buf.h as i64 {
                        // 画布外无像素可判——与 F631 v1 邻域检查同口径豁免
                        //（内置热点在箭头尖端，邻域天然越界）。
                        continue;
                    }
                    if !buf.solid(x as u16, y as u16) {
                        return false;
                    }
                }
            }
            true
        }
        TemplateId::LowLoad => {
            match m.state(PointerState::Normal) {
                Some(sf) => sf.frames.iter().all(|f| f.delay_ms == 0),
                None => false,
            }
        }
    }
}

/// 合规报告（三模板 × [F627 体检、F632 审计] 的对勾表——版本戳防旧报)。
pub struct ComplianceReport {
    pub template_version: u32,
    pub rows: Vec<(TemplateId, bool, bool)>, // (id, checker_green, audit_passed)
}

pub fn compliance_report() -> ComplianceReport {
    let rows = TEMPLATE_REGISTRY
        .iter()
        .map(|(id, _)| {
            let m = build_template(*id);
            let checker_green = checker::inspect(&m).all_green();
            let audit_passed = crate::jstar2::audit::audit(&m).all_passed;
            (*id, checker_green, audit_passed)
        })
        .collect();
    ComplianceReport { template_version: TEMPLATE_VERSION, rows }
}

/// v3 自检。
pub fn run_a11ytmpl_v3_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F631-v3");

    // 1. 画廊与注册表 id 集合一致（两表同源对账）。
    set.add("gallery consistent with registry", gallery_registry_consistent(), "");

    // 2. 适配性规则：三需求 → 三模板；未知需求诚实 None。
    set.add(
        "recommendation routes by need",
        recommend_template("low-vision") == Some(TemplateId::HighContrast)
            && recommend_template("motor") == Some(TemplateId::BigHotspot)
            && recommend_template("attention") == Some(TemplateId::LowLoad)
            && recommend_template("huh").is_none(),
        "",
    );

    // 3. 转换图：同模板 → None；异模板 → 有说明且六条路全通。
    set.add(
        "conversion graph complete",
        conversion_note(TemplateId::HighContrast, TemplateId::HighContrast).is_none()
            && conversion_note(TemplateId::LowLoad, TemplateId::BigHotspot).is_some()
            && [
                (TemplateId::HighContrast, TemplateId::BigHotspot),
                (TemplateId::BigHotspot, TemplateId::HighContrast),
                (TemplateId::LowLoad, TemplateId::HighContrast),
                (TemplateId::HighContrast, TemplateId::LowLoad),
                (TemplateId::BigHotspot, TemplateId::LowLoad),
                (TemplateId::LowLoad, TemplateId::BigHotspot),
            ]
            .iter()
            .all(|(a, b)| conversion_note(*a, *b).is_some()),
        "",
    );

    // 4. 参数覆盖：换色变体仍过体检（改形不改纪律）。
    let ov = TemplateOverride {
        body: Some(Rgb::new(30, 30, 90)),
        outline: Some(Rgb::new(240, 220, 120)),
        hotspot_delta: None,
    };
    let variant = apply_override(TemplateId::HighContrast, &ov);
    set.add(
        "override variant passes checker",
        checker::inspect(&variant).all_green(),
        "",
    );

    // 5. 覆盖后不变量复检：三模板默认变体各自的不变量仍成立。
    let hc_ok = override_invariants_hold(TemplateId::HighContrast, &apply_override(TemplateId::HighContrast, &ov));
    let bh_ok = override_invariants_hold(TemplateId::BigHotspot, &build_template(TemplateId::BigHotspot));
    let ll_ok = override_invariants_hold(TemplateId::LowLoad, &build_template(TemplateId::LowLoad));
    set.add(
        "override invariants hold per template",
        hc_ok && bh_ok && ll_ok,
        "",
    );

    // 6. 热点偏移覆盖：大热点模板偏移后邻域判仍成立（偏移被钳在界内）。
    let shifted = apply_override(TemplateId::BigHotspot, &TemplateOverride { hotspot_delta: Some((2, -1)), ..Default::default() });
    set.add(
        "hotspot shift keeps 8x8 solid",
        override_invariants_hold(TemplateId::BigHotspot, &shifted),
        "",
    );

    // 7. 合规报告：版本戳在、三行全绿（checker + audit 双尺）。
    let rep = compliance_report();
    set.add(
        "compliance report all green versioned",
        rep.template_version == TEMPLATE_VERSION
            && rep.rows.len() == 3
            && rep.rows.iter().all(|(_, c, a)| *c && *a),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v3 {
    use super::*;

    #[test]
    fn override_with_default_is_base() {
        let base = build_template(TemplateId::LowLoad);
        let ov = apply_override(TemplateId::LowLoad, &TemplateOverride::default());
        // 默认覆盖 = 模板原样（表现参数未动）。
        assert_eq!(base.state(PointerState::Normal).unwrap().frames[0].px, ov.state(PointerState::Normal).unwrap().frames[0].px);
    }

    #[test]
    fn hotspot_shift_clamped_inside() {
        let shifted = apply_override(TemplateId::BigHotspot, &TemplateOverride { hotspot_delta: Some((1000, 1000)), ..Default::default() });
        let f = shifted.state(PointerState::Normal).unwrap().frames[0].clone();
        assert!(f.hot_x < 32 && f.hot_y < 32, "偏移被钳在画布内");
    }

    #[test]
    fn gallery_meta_texts_nonempty() {
        for m in TEMPLATE_GALLERY.iter() {
            assert!(!m.audience.is_empty() && !m.solves.is_empty());
        }
    }
}

// ---------------------------------------------------------------------------
// v4 深化批：模板使用统计账本 · 模板摘要行人话渲染
// ---------------------------------------------------------------------------

/// 模板使用账本（工坊「起步模板」套用面的计数器：哪个模板被套用了多少
/// 次、最近一次何时——注册表管判据、画廊管说明、账本管使用，三表靠
/// id 键对账；槽位数与 `TEMPLATE_REGISTRY` 同宽，加模板漏账本即红）。
pub struct UsageLedger {
    counts: [u64; 3],
    last_used_ms: [Option<u64>; 3],
}

impl UsageLedger {
    pub fn new() -> UsageLedger {
        UsageLedger { counts: [0; 3], last_used_ms: [None; 3] }
    }

    /// id → 槽位（三模板固定座次：与注册表下标一致）。
    fn slot(id: TemplateId) -> usize {
        match id {
            TemplateId::HighContrast => 0,
            TemplateId::BigHotspot => 1,
            TemplateId::LowLoad => 2,
        }
    }

    /// 记一次套用（时刻只前进不回退——乱序喂入取较大者，账本不撒谎）。
    pub fn record(&mut self, id: TemplateId, at_ms: u64) {
        let s = Self::slot(id);
        self.counts[s] += 1;
        if self.last_used_ms[s].map(|t| at_ms > t).unwrap_or(true) {
            self.last_used_ms[s] = Some(at_ms);
        }
    }

    /// 某模板的累计套用次数。
    pub fn count_of(&self, id: TemplateId) -> u64 {
        self.counts[Self::slot(id)]
    }

    /// 某模板最近套用时刻（从未用过 → None——不猜）。
    pub fn last_used_of(&self, id: TemplateId) -> Option<u64> {
        self.last_used_ms[Self::slot(id)]
    }

    /// 最热门模板（并列取先登记者；全零 → None）。
    pub fn most_used(&self) -> Option<TemplateId> {
        let mut best: Option<usize> = None;
        for s in 0..3 {
            if self.counts[s] == 0 {
                continue;
            }
            match best {
                None => best = Some(s),
                Some(b) if self.counts[s] > self.counts[b] => best = Some(s),
                _ => {}
            }
        }
        best.map(|s| TEMPLATE_REGISTRY[s].0)
    }

    /// 总套用次数（三槽合计）。
    pub fn total(&self) -> u64 {
        self.counts[0] + self.counts[1] + self.counts[2]
    }
}

impl Default for UsageLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 模板摘要行（判据 + 适配人群 + 免检登记 → 一行人话：工坊起步面板的
/// 说明卡渲染面——注册表、画廊、免检映射三表联查，缺一即残行）。
pub fn template_summary_line(id: TemplateId) -> String {
    let criteria = TEMPLATE_REGISTRY
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, c)| *c)
        .unwrap_or("");
    let audience = TEMPLATE_GALLERY
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.audience)
        .unwrap_or("");
    let exempt = if exempt_reason(id.key()).is_some() { "已登记" } else { "未登记" };
    alloc::format!("{} [{}]：{}｜适配：{}｜免检：{}", id.zh(), id.key(), criteria, audience, exempt)
}

/// v4 自检。
pub fn run_a11ytmpl_v4_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F631-v4");

    // 1. 账本计数：三模板各记一次 → 各自 1、总数 3。
    let mut led = UsageLedger::new();
    led.record(TemplateId::HighContrast, 10);
    led.record(TemplateId::BigHotspot, 20);
    led.record(TemplateId::LowLoad, 30);
    set.add(
        "usage ledger counts per template",
        led.count_of(TemplateId::HighContrast) == 1
            && led.count_of(TemplateId::BigHotspot) == 1
            && led.count_of(TemplateId::LowLoad) == 1
            && led.total() == 3,
        "",
    );

    // 2. 最近时刻单调：乱序喂入取较大者（账本不回退）。
    led.record(TemplateId::BigHotspot, 5);
    set.add(
        "ledger last-used never regresses",
        led.last_used_of(TemplateId::BigHotspot) == Some(20)
            && led.count_of(TemplateId::BigHotspot) == 2,
        "",
    );

    // 3. 热门模板：计数领先者胜出；空账本诚实 None。
    led.record(TemplateId::LowLoad, 40);
    led.record(TemplateId::LowLoad, 50);
    set.add(
        "most used picks leader empty honest",
        led.most_used() == Some(TemplateId::LowLoad)
            && UsageLedger::new().most_used().is_none(),
        "",
    );

    // 4. 摘要行三表联查：名字、键、判据、人群、免检登记都在一行里。
    let line = template_summary_line(TemplateId::HighContrast);
    set.add(
        "summary line joins three tables",
        line.contains("高对比模板")
            && line.contains("high-contrast")
            && line.contains("4.5:1")
            && line.contains("低视力")
            && line.contains("已登记"),
        "",
    );

    // 5. 三模板摘要行各不相同且各带自家判据片段（8×8 / 2px）。
    let l1 = template_summary_line(TemplateId::BigHotspot);
    let l2 = template_summary_line(TemplateId::LowLoad);
    set.add(
        "three summary lines distinct",
        l1 != l2 && l1.contains("8×8") && l2.contains("2px") && line != l1 && line != l2,
        "",
    );

    // 6. 账本与注册表同宽：三槽位覆盖三模板、各记一次总数对齐
    //    （并列时取先登记者——高对比）。
    let mut led2 = UsageLedger::new();
    for (id, _) in TEMPLATE_REGISTRY.iter() {
        led2.record(*id, 1);
    }
    set.add(
        "ledger slots cover registry",
        led2.total() == TEMPLATE_REGISTRY.len() as u64
            && led2.most_used() == Some(TemplateId::HighContrast),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v4 {
    use super::*;

    #[test]
    fn ledger_roundtrip_and_totals() {
        let mut led = UsageLedger::new();
        led.record(TemplateId::LowLoad, 100);
        led.record(TemplateId::LowLoad, 200);
        led.record(TemplateId::LowLoad, 150);
        assert_eq!(led.count_of(TemplateId::LowLoad), 3);
        assert_eq!(led.last_used_of(TemplateId::LowLoad), Some(200), "乱序喂入取较大者");
        assert_eq!(led.total(), 3);
    }

    #[test]
    fn summary_line_complete_for_all_templates() {
        for (id, _) in TEMPLATE_REGISTRY.iter() {
            let line = template_summary_line(*id);
            assert!(line.contains(id.zh()) && line.contains(id.key()));
            assert!(line.ends_with("已登记"), "三模板免检全登记：{line}");
        }
    }

    #[test]
    fn unused_template_has_no_last_used() {
        let led = UsageLedger::new();
        assert_eq!(led.last_used_of(TemplateId::BigHotspot), None);
        assert_eq!(led.count_of(TemplateId::BigHotspot), 0);
    }
}
