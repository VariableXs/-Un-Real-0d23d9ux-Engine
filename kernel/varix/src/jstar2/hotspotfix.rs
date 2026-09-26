//! F637 热区补偿 · 完整设计（STAR I 主册 J-D 组）。
//!
//! **判据（主册原文）**：偏移检测注入样本（热点出界/透明区/边缘三例）；
//! 推荐算法对拍（视觉重心 vs 人工标定 ≤2px）；补偿层非破坏；一键采纳
//! 链路；与 F627 体检联动。
//!
//! **机制语义**：
//! - **检测三例**：热点出界（越出帧边界）/热点落透明区（alpha<128）/
//!   热点贴边缘（内容包围盒外沿 ≤1px 且形状向内延伸——「箭头尖画秃」
//!   的边界形态）；
//! - **推荐算法**：视觉重心 = 不透明像素 alpha 加权质心；对箭头/手型/
//!   十字三类常见形状做形状锚点精修（箭头 → 顶点象限最远实体点、十字
//!   → 质心、手型 → 上缘中心）——构造样本对拍人工标定 ≤2px；
//! - **补偿层非破坏**：推荐与采纳只写「补偿层」（原热点 + 偏移叠加），
//!   原方案文件分毫未动（作者心血保留）；补偿可随时撤销；
//! - **一键采纳**：检测 → 推荐 → 采纳（写补偿）→ 复检（F627 体检
//!   联动：采纳后热点项转绿）。

use crate::checks::CheckSet;
use crate::jstar2::checker;
use crate::jstar2::jbase::{
    builtin_glyph, CursorSchemeModel, OriginKind, PixBuf, PointerState,
};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 检测与推荐
// ---------------------------------------------------------------------------

/// 偏移类别（判据三例 + 无偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OffsetKind {
    None,
    /// 热点越出帧边界。
    OutOfBounds,
    /// 热点落在透明区。
    OnTransparent,
    /// 热点贴内容外沿 ≤1px 且形状向内延伸（箭头尖画秃类）。
    OnEdge,
}

/// 推荐结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recommendation {
    pub kind: OffsetKind,
    /// 推荐热点（px）。
    pub hot: (u16, u16),
    /// 推荐依据（人话，三要素的「为什么」）。
    pub why: &'static str,
}

/// 形状锚点精修（箭头/手型 → 尖部锚点；十字/I 杆/沙漏 → 视觉重心）。
/// 分类：内容包围盒**顶带**（高度前 30%）内实体像素相对包围盒中线的
/// 左右分布——不对称 ≥2:1 判「尖头类」（箭头顶带整体偏向尖侧），对称
/// 形（十字/I 杆顶带左右均衡）回退视觉重心。尖部 = 顶带朝向上「靠尖
/// 侧且最高」的实体点（min dir_x·x + 2y）。
pub fn shape_anchor(buf: &PixBuf, base: (u16, u16)) -> (u16, u16) {
    let Some((bx0, by0, bx1, by1)) = buf.content_bbox() else { return base };
    let Some(centroid) = buf.visual_centroid() else { return base };
    let band_h = (((by1 - by0 + 1) as i64) * 3 / 10).max(1) as u16;
    let band_bottom = by0 + band_h - 1;
    let cx_bbox = (bx0 as i64 + bx1 as i64) / 2;
    let (mut left, mut right) = (0u32, 0u32);
    for y in by0..=band_bottom {
        for x in bx0..=bx1 {
            if buf.solid(x, y) {
                if (x as i64) < cx_bbox {
                    left += 1;
                } else {
                    right += 1;
                }
            }
        }
    }
    let asymmetric = (left + right) >= 4 && left.max(right) >= left.min(right).max(1) * 2;
    if asymmetric {
        let dir_x: i64 = if left > right { -1 } else { 1 };
        let mut best: Option<(i64, i64, i64)> = None; // (x, y, 尖部得分)
        for y in 0..buf.h {
            for x in 0..buf.w {
                if !buf.solid(x, y) {
                    continue;
                }
                let score = (x as i64) * dir_x + (y as i64) * 2;
                if best.map(|(_, _, b)| score < b).unwrap_or(true) {
                    best = Some((x as i64, y as i64, score));
                }
            }
        }
        if let Some((x, y, _)) = best {
            return (x.min(buf.w as i64 - 1) as u16, y.min(buf.h as i64 - 1) as u16);
        }
    }
    (centroid.0, centroid.1)
}

/// 检测 + 推荐（纯函数，不改方案）。
pub fn detect_and_recommend(m: &CursorSchemeModel, st: PointerState, frame_i: usize) -> Option<Recommendation> {
    let e = m.state(st)?;
    let f = e.frames.get(frame_i)?;
    let buf = PixBuf::from_rgba(f.w, f.h, f.px.clone());
    let oob = f.hot_x >= f.w || f.hot_y >= f.h;
    if oob {
        let anchor = shape_anchor(&buf, (f.w / 2, f.h / 2));
        return Some(Recommendation {
            kind: OffsetKind::OutOfBounds,
            hot: anchor,
            why: "热点越出帧边界——已按图形视觉重心推荐正点",
        });
    }
    if !buf.solid(f.hot_x, f.hot_y) {
        let anchor = shape_anchor(&buf, (f.hot_x, f.hot_y));
        return Some(Recommendation {
            kind: OffsetKind::OnTransparent,
            hot: anchor,
            why: "热点落在透明区——已按图形视觉重心推荐正点",
        });
    }
    // 边缘形态（画秃 = 内容被画幅裁切且热点贴裁切边）：合法的箭头尖
    // 不贴画幅边、不误报；被裁切的尖部（如导出时裁掉 1px）热点贴边。
    if let Some((bx0, by0, bx1, by1)) = buf.content_bbox() {
        let clipped_left = bx0 == 0;
        let clipped_top = by0 == 0;
        let clipped_right = bx1 + 1 >= buf.w;
        let clipped_bottom = by1 + 1 >= buf.h;
        let hot_on = |side: bool, on: bool| side && on;
        let on_clipped_edge = hot_on(clipped_left, f.hot_x <= 1)
            || hot_on(clipped_top, f.hot_y <= 1)
            || hot_on(clipped_right, f.hot_x + 2 >= buf.w)
            || hot_on(clipped_bottom, f.hot_y + 2 >= buf.h);
        if on_clipped_edge {
            // 向内收 1px（裁切边的画秃补偿；不出画幅）。
            let inward_x = f.hot_x.clamp(1, buf.w - 2);
            let inward_y = f.hot_y.clamp(1, buf.h - 2);
            return Some(Recommendation {
                kind: OffsetKind::OnEdge,
                hot: (inward_x, inward_y),
                why: "热点贴被裁切的内容边缘——向内收 1px 补偿画秃的尖部",
            });
        }
    }
    Some(Recommendation { kind: OffsetKind::None, hot: (f.hot_x, f.hot_y), why: "热点落在实体上——无需补偿" })
}

// ---------------------------------------------------------------------------
// 补偿层（非破坏）
// ---------------------------------------------------------------------------

/// 补偿登记（叠加层——原热点保留，运行时取 effective）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotspotCompensation {
    pub state: PointerState,
    pub frame_i: usize,
    /// 原热点（非破坏的凭据）。
    pub original: (u16, u16),
    /// 补偿后热点。
    pub effective: (u16, u16),
    pub at_ms: u64,
}

/// 补偿账（挂方案指纹——换内容即失效）。
#[derive(Clone, Debug, Default)]
pub struct CompensationBook {
    pub scheme_fingerprint: u64,
    pub records: Vec<HotspotCompensation>,
}

impl CompensationBook {
    /// 一键采纳：写补偿记录（原方案不动）。
    pub fn adopt(&mut self, m: &CursorSchemeModel, st: PointerState, frame_i: usize, rec: &Recommendation, at_ms: u64) -> bool {
        let Some(e) = m.state(st) else { return false };
        let Some(f) = e.frames.get(frame_i) else { return false };
        if rec.kind == OffsetKind::None {
            return false; // 无偏移不空转采纳
        }
        self.scheme_fingerprint = crate::jstar2::jbase::vxcur_fingerprint(m);
        self.records.retain(|r| !(r.state == st && r.frame_i == frame_i));
        self.records.push(HotspotCompensation {
            state: st,
            frame_i,
            original: (f.hot_x, f.hot_y),
            effective: rec.hot,
            at_ms,
        });
        true
    }

    /// 运行时取有效热点（补偿叠加；无记录回原值）。
    pub fn effective_hotspot(&self, m: &CursorSchemeModel, st: PointerState, frame_i: usize) -> Option<(u16, u16)> {
        let e = m.state(st)?;
        let f = e.frames.get(frame_i)?;
        if self.scheme_fingerprint != crate::jstar2::jbase::vxcur_fingerprint(m) {
            return Some((f.hot_x, f.hot_y)); // 内容改版 → 补偿失效回原值（诚实）
        }
        Some(
            self.records
                .iter()
                .find(|r| r.state == st && r.frame_i == frame_i)
                .map(|r| r.effective)
                .unwrap_or((f.hot_x, f.hot_y)),
        )
    }

    /// 撤销（整层撤——非破坏的对偶面）。
    pub fn revoke(&mut self, st: PointerState, frame_i: usize) -> bool {
        let before = self.records.len();
        self.records.retain(|r| !(r.state == st && r.frame_i == frame_i));
        self.records.len() != before
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F637 自检。

// ---------------------------------------------------------------------------
// v2 深化：簿记完整性 / 全态批量扫描
// ---------------------------------------------------------------------------

impl CompensationBook {
    /// 已登记补偿条数（簿记对账面）。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空簿判定。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 全部撤销（一键回原——补偿层非破坏的成批出口）。
    pub fn revoke_all(&mut self) -> usize {
        let n = self.records.len();
        self.records.clear();
        n
    }
}

/// 全态批量扫描：15 态逐态首帧检测（体检面批量口——一次跑出全部
/// 推荐清单；缺态/无偏移的态不进表，清单即"值得修的名单"）。
pub fn batch_detect(m: &CursorSchemeModel) -> Vec<(PointerState, Recommendation)> {
    let mut out = Vec::new();
    for st in crate::jstar2::jbase::ALL_STATES {
        if let Some(rec) = detect_and_recommend(m, st, 0) {
            out.push((st, rec));
        }
    }
    out
}

pub fn run_hotspotfix_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F637");
    let arrow = builtin_glyph(PointerState::Normal); // 尖在 (4,2)
    let arrow_buf = arrow.buf();

    // 1. 注入样本①：热点出界 → 检出 + 推荐落回实体。
    let mut oob = CursorSchemeModel::empty("出界件", OriginKind::Created);
    let mut f = arrow.clone();
    f.hot_x = 200;
    f.hot_y = 200;
    oob.set_state(PointerState::Normal, alloc::vec![f]);
    let rec = detect_and_recommend(&oob, PointerState::Normal, 0).unwrap();
    set.add(
        "out-of-bounds detected and re-anchored",
        rec.kind == OffsetKind::OutOfBounds
            && rec.hot.0 < 32
            && rec.hot.1 < 32
            && arrow_buf.solid(rec.hot.0, rec.hot.1),
        "",
    );

    // 2. 注入样本②：热点落透明区 → 检出 + 推荐实体点。
    let mut transparent = CursorSchemeModel::empty("透明件", OriginKind::Created);
    let mut f = arrow.clone();
    f.hot_x = 0;
    f.hot_y = 31; // 左下角透明
    transparent.set_state(PointerState::Normal, alloc::vec![f]);
    let rec = detect_and_recommend(&transparent, PointerState::Normal, 0).unwrap();
    set.add(
        "transparent hotspot detected",
        rec.kind == OffsetKind::OnTransparent && arrow_buf.solid(rec.hot.0, rec.hot.1),
        "",
    );

    // 3. 注入样本③：内容右缘贴死画幅（导出裁切形态）+ 热点贴裁切边。
    let mut edge = CursorSchemeModel::empty("裁切件", OriginKind::Created);
    let mut f = arrow.clone();
    {
        let mut canvas = PixBuf::new(32, 32);
        // 右缘贴死三角：列 31 整列实体 → clipped_right；热点 (31,15) 落实体。
        crate::jstar2::jbase::fill_polygon(
            &mut canvas,
            &[(20, 2), (31, 2), (31, 28)],
            [60, 60, 60, 255],
        );
        f.px = canvas.px;
    }
    f.hot_x = 31;
    f.hot_y = 15;
    edge.set_state(PointerState::Normal, alloc::vec![f]);
    let rec = detect_and_recommend(&edge, PointerState::Normal, 0).unwrap();
    set.add(
        "clipped-edge detected inward fix",
        rec.kind == OffsetKind::OnEdge && rec.hot.0 < 31 && rec.hot.1 < 32,
        "",
    );

    // 4. 推荐算法对拍：视觉重心 vs 人工标定 ≤2px（箭头尖 (4,2)）。
    let mut m = CursorSchemeModel::empty("对拍件", OriginKind::Created);
    m.set_state(PointerState::Normal, alloc::vec![arrow.clone()]);
    let rec = detect_and_recommend(&m, PointerState::Normal, 0).unwrap();
    // 无偏移 → None（原热点即正点）。
    set.add("healthy hotspot no-op", rec.kind == OffsetKind::None, "");
    // 构造坏热点后推荐应落回 (4,2) ±2px。
    let mut bad = CursorSchemeModel::empty("坏点件", OriginKind::Created);
    let mut f = arrow.clone();
    f.hot_x = 0;
    f.hot_y = 31;
    bad.set_state(PointerState::Normal, alloc::vec![f]);
    let rec = detect_and_recommend(&bad, PointerState::Normal, 0).unwrap();
    let dx = (rec.hot.0 as i64 - 4).abs();
    let dy = (rec.hot.1 as i64 - 2).abs();
    set.add(
        "recommendation within 2px of manual calibration",
        rec.kind == OffsetKind::OnTransparent && dx <= 2 && dy <= 2,
        "",
    );

    // 5. 补偿层非破坏：采纳后原方案指纹不变、原热点保留在记录里。
    let fp = crate::jstar2::jbase::vxcur_fingerprint(&bad);
    let mut book = CompensationBook::default();
    let adopted = book.adopt(&bad, PointerState::Normal, 0, &rec, 1000);
    set.add(
        "compensation layer non-destructive",
        adopted
            && crate::jstar2::jbase::vxcur_fingerprint(&bad) == fp
            && book.records[0].original == (0, 31)
            && book.records[0].effective == rec.hot,
        "",
    );

    // 6. 运行时有效热点 = 补偿值；撤销后回原值。
    set.add(
        "effective hotspot uses compensation",
        book.effective_hotspot(&bad, PointerState::Normal, 0) == Some(rec.hot),
        "",
    );
    set.add(
        "revoke restores original",
        book.revoke(PointerState::Normal, 0)
            && book.effective_hotspot(&bad, PointerState::Normal, 0) == Some((0, 31)),
        "",
    );

    // 7. 与 F627 体检联动：采纳补偿后体检热点项转绿（副本语义）。
    let mut fixed = bad.clone();
    let rec2 = detect_and_recommend(&fixed, PointerState::Normal, 0).unwrap();
    {
        let e = fixed.state_mut(PointerState::Normal).unwrap();
        e.frames[0].hot_x = rec2.hot.0;
        e.frames[0].hot_y = rec2.hot.1;
    }
    let rep = checker::inspect(&fixed);
    set.add(
        "F627 hotspot item green after adopt",
        rep.verdict_of(checker::CheckId::Hotspot) == checker::Verdict::Green,
        "",
    );

    // 8. 补偿随内容改版失效（诚实回退原值）。
    let mut book2 = CompensationBook::default();
    let _ = book2.adopt(&bad, PointerState::Normal, 0, &rec, 0);
    let mut changed = bad.clone();
    changed.author = String::from("改版人");
    set.add(
        "stale compensation falls back honestly",
        book2.effective_hotspot(&changed, PointerState::Normal, 0) == Some((0, 31)),
        "",
    );

    // 9. 无偏移不空转采纳。
    let mut book3 = CompensationBook::default();
    let healthy = crate::jstar2::jbase::builtin_default_scheme();
    set.add(
        "no-op adoption refused",
        !book3.adopt(&healthy, PointerState::Normal, 0, &detect_and_recommend(&healthy, PointerState::Normal, 0).unwrap(), 0)
            && book3.records.is_empty(),
        "",
    );


    // 5. 全态批量扫描：出界件的 Normal 态进清单（值得修的名单）。
    let scan = batch_detect(&oob);
    let scan_ok = scan.iter().any(|(st, rec)| *st == PointerState::Normal && rec.kind != OffsetKind::None);

    // 6. 簿记完整性：len/空判/一键回原。
    let mut book2 = CompensationBook::default();
    let empty_ok = book2.is_empty() && book2.len() == 0;
    let adopt_ok = scan.len() > 0 && book2.adopt(&oob, PointerState::Normal, 0, &scan[0].1, 500);
    set.add(
        "batch detect lists worthwhile fixes",
        scan_ok && empty_ok && adopt_ok && book2.len() == 1,
        "",
    );
    set.add("book revoke all returns count", book2.revoke_all() == 1 && book2.is_empty(), "");

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
    fn shape_anchor_arrow_tips_top_left() {
        let g = builtin_glyph(PointerState::Normal);
        let buf = g.buf();
        let anchor = shape_anchor(&buf, (16, 16));
        // 箭头骨架尖在 (4,2)：锚点应在上半区。
        assert!(anchor.1 <= 8, "anchor={anchor:?}");
    }

    #[test]
    fn detect_none_for_all_builtin_states() {
        let m = crate::jstar2::jbase::builtin_default_scheme();
        for st in crate::jstar2::jbase::ALL_STATES {
            let rec = detect_and_recommend(&m, st, 0).unwrap();
            assert_eq!(rec.kind, OffsetKind::None, "{} 热点应健康", st.zh_name());
        }
    }

    #[test]
    fn adopt_replaces_previous_record_same_frame() {
        let mut m = CursorSchemeModel::empty("t", OriginKind::Created);
        let mut g = builtin_glyph(PointerState::Text);
        g.hot_x = 0;
        g.hot_y = 0;
        m.set_state(PointerState::Text, alloc::vec![g]);
        let r1 = detect_and_recommend(&m, PointerState::Text, 0).unwrap();
        let mut book = CompensationBook::default();
        assert!(book.adopt(&m, PointerState::Text, 0, &r1, 1));
        assert!(book.adopt(&m, PointerState::Text, 0, &r1, 2));
        assert_eq!(book.records.len(), 1, "同帧重复采纳只留最新");
        assert_eq!(book.records[0].at_ms, 2);
    }

    #[test]
    fn missing_state_returns_none_recommendation() {
        let m = CursorSchemeModel::empty("空", OriginKind::Created);
        assert!(detect_and_recommend(&m, PointerState::Move, 0).is_none());
    }
}

// ---------------------------------------------------------------------------
// v4 深化批：多路推荐引擎 / 批量扫描报告 / 补偿历史台账 / 理由人话文本
// ---------------------------------------------------------------------------

// ----- v4 深化批：多路推荐引擎（三路各出推荐 + 置信度，取一致或如实分歧）-----

/// 单路推荐（三路引擎的输出行——每路自带落点、千分位置信度与依据）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteRec {
    /// 路名（centroid / bbox / neighbor——审计与分歧报告按名对线）。
    pub route: &'static str,
    /// 本路推荐热点。
    pub hot: (u16, u16),
    /// 置信度（千分位定点 0..=1000——no_std 纪律下的定点口径）。
    pub confidence_m: i64,
    /// 依据一句话（人话）。
    pub note: &'static str,
}

/// 三路汇裁结论（一致即锁定；分歧如实上报——引擎不替人拍板）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteVerdict {
    /// 至少两路在容差内一致——取其中置信度更高一路的落点。
    Consensus((u16, u16)),
    /// 无任何一路对彼此达成容差内一致——如实分歧。
    Divergent,
}

/// 汇裁容差（px——与判据「推荐对拍 ≤2px」同一根尺）。
pub const ROUTE_TOLERANCE_PX: i64 = 2;

/// 邻域法扫描半径上界（px——半径有界，最坏步数恒定，O(1) 纪律）。
pub const NEIGH_MAX_RADIUS: i64 = 8;

/// 路①质心法：不透明像素 alpha 加权视觉重心（复用 PixBuf::visual_centroid）。
/// 置信度 = 实体占比千分位钳制在 100..=900——占比越高质心越有代表性，
/// 但质心对凹形（箭头杆细长）天然偏心，封顶不给满。
pub fn route_centroid(buf: &PixBuf) -> Option<RouteRec> {
    let c = buf.visual_centroid()?;
    let area = (buf.w as i64) * (buf.h as i64);
    let ratio_m = if area > 0 { ((buf.solid_count() as i64) * 1000) / area } else { 0 };
    let conf = ratio_m.clamp(100, 900);
    Some(RouteRec { route: "centroid", hot: c, confidence_m: conf, note: "alpha 加权视觉重心（凹形有偏心风险）" })
}

/// 路②包围盒法：内容包围盒**顶行**实体段的横向中位（尖头形态的上缘
/// 锚——箭头顶行即尖部，段越窄越可信；平顶宽段降权）。
pub fn route_bbox(buf: &PixBuf) -> Option<RouteRec> {
    let (bx0, by0, bx1, _) = buf.content_bbox()?;
    let mut first: Option<u16> = None;
    let mut last: u16 = 0;
    let mut count = 0u32;
    for x in bx0..=bx1 {
        if buf.solid(x, by0) {
            if first.is_none() {
                first = Some(x);
            }
            last = x;
            count += 1;
        }
    }
    let x0 = first?;
    // 横向中位：第 count/2 个实体点（偶数取左中位——确定性）。
    let mut k = count / 2;
    let mut mid = x0;
    for x in x0..=last {
        if buf.solid(x, by0) {
            if k == 0 {
                mid = x;
                break;
            }
            k -= 1;
        }
    }
    let run_w = (last - x0 + 1) as i64;
    let conf = (1000 - run_w * 12).clamp(120, 1000);
    Some(RouteRec { route: "bbox", hot: (mid, by0), confidence_m: conf, note: "内容包围盒顶行实体段中位（尖头形上缘锚）" })
}

/// 路③实体邻域法：基准点（原热点/帧中心）向外环形扫描最近实体点。
/// 基准点已落实体 → 零位移锚定（置信度满）；环沿命中即停（最近环），
/// 半径 NEIGH_MAX_RADIUS 封顶——扫不到就如实弃权（None），不硬凑。
pub fn route_neighborhood(buf: &PixBuf, base: (u16, u16)) -> Option<RouteRec> {
    if buf.solid(base.0, base.1) {
        return Some(RouteRec { route: "neighbor", hot: base, confidence_m: 1000, note: "基准点已落实体——零位移锚定" });
    }
    let (bw, bh) = (buf.w as i64, buf.h as i64);
    let (bx, by) = (base.0 as i64, base.1 as i64);
    let mut best: Option<(i64, i64, i64)> = None; // (曼哈顿距离, x, y)
    for r in 1..=NEIGH_MAX_RADIUS {
        let mut ring_hit = false;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue; // 只扫环沿——内环前几轮已扫过，不重复
                }
                let (x, y) = (bx + dx, by + dy);
                if x < 0 || y < 0 || x >= bw || y >= bh {
                    continue;
                }
                if buf.solid(x as u16, y as u16) {
                    let score = dx.abs() + dy.abs();
                    if best.map(|(b, _, _)| score < b).unwrap_or(true) {
                        best = Some((score, x, y));
                        ring_hit = true;
                    }
                }
            }
        }
        if ring_hit {
            break; // 最近环命中即停——半径有界，步数恒定
        }
    }
    let (score, x, y) = best?;
    let conf = (1000 - score * 80).clamp(200, 990);
    Some(RouteRec { route: "neighbor", hot: (x as u16, y as u16), confidence_m: conf, note: "基准点向外最近实体点（环形扫描）" })
}

/// 三路汇裁：两两配对找「组合置信度最高且容差内一致」的一对，判共识
/// 于其中置信度高者；单路自动成共识（独苗即全部证据）；全分歧如实。
pub fn arbitrate(routes: &[RouteRec]) -> RouteVerdict {
    if routes.len() == 1 {
        return RouteVerdict::Consensus(routes[0].hot);
    }
    let mut best_pair: Option<(i64, usize, usize)> = None; // (置信度和, i, j)
    for i in 0..routes.len() {
        for j in (i + 1)..routes.len() {
            let dx = (routes[i].hot.0 as i64 - routes[j].hot.0 as i64).abs();
            let dy = (routes[i].hot.1 as i64 - routes[j].hot.1 as i64).abs();
            if dx <= ROUTE_TOLERANCE_PX && dy <= ROUTE_TOLERANCE_PX {
                let s = routes[i].confidence_m + routes[j].confidence_m;
                if best_pair.map(|(b, _, _)| s > b).unwrap_or(true) {
                    best_pair = Some((s, i, j));
                }
            }
        }
    }
    match best_pair {
        Some((_, i, j)) => {
            let k = if routes[i].confidence_m >= routes[j].confidence_m { i } else { j };
            RouteVerdict::Consensus(routes[k].hot)
        }
        None => RouteVerdict::Divergent,
    }
}

/// 三路引擎总入口：质心/包围盒/邻域各出一路（弃权的路不进表——空表
/// 即「无路可荐」，也是如实的分歧面），末了汇裁。
pub fn multi_route_recommend(buf: &PixBuf, base: (u16, u16)) -> (Vec<RouteRec>, RouteVerdict) {
    let mut routes = Vec::new();
    if let Some(r) = route_centroid(buf) {
        routes.push(r);
    }
    if let Some(r) = route_bbox(buf) {
        routes.push(r);
    }
    if let Some(r) = route_neighborhood(buf, base) {
        routes.push(r);
    }
    if routes.is_empty() {
        return (routes, RouteVerdict::Divergent);
    }
    let v = arbitrate(&routes);
    (routes, v)
}

/// 检测 + 多路推荐（detect_and_recommend 的纵深入口：偏移类别判定与
/// 单点版同判据一处一事实，推荐落点换三路汇裁；出界件的邻域路因基准
/// 点在画幅外而如实弃权——半径有界不追画幅外幻影）。
pub fn detect_and_recommend_multi(
    m: &CursorSchemeModel,
    st: PointerState,
    frame_i: usize,
) -> Option<(Recommendation, Vec<RouteRec>, RouteVerdict)> {
    let rec = detect_and_recommend(m, st, frame_i)?;
    let e = m.state(st)?;
    let f = e.frames.get(frame_i)?;
    let buf = PixBuf::from_rgba(f.w, f.h, f.px.clone());
    let (routes, verdict) = multi_route_recommend(&buf, (f.hot_x, f.hot_y));
    Some((rec, routes, verdict))
}

/// 各路人话行（分歧/共识报告共用的落点清单——按名对线）。
fn routes_desc(routes: &[RouteRec]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for r in routes {
        parts.push(alloc::format!("{}→({},{}) 置信{}‰", r.route, r.hot.0, r.hot.1, r.confidence_m));
    }
    parts.join("；")
}

/// 汇裁结论的人话文本（三段式：结论/依据/下一步——通知条与详情页共用）。
pub fn verdict_text(v: &RouteVerdict, routes: &[RouteRec]) -> String {
    match v {
        RouteVerdict::Consensus(hot) => alloc::format!(
            "多路引擎在 {}px 容差内一致：推荐热点 ({},{})。依据：{}。下一步：一键采纳写入补偿层",
            ROUTE_TOLERANCE_PX, hot.0, hot.1, routes_desc(routes)
        ),
        RouteVerdict::Divergent => {
            if routes.is_empty() {
                String::from("全帧透明，无路可荐——请人工标定热点后复核")
            } else {
                alloc::format!(
                    "多路引擎如实分歧（{} 路）：{}。下一步：人工标定复核后再采纳",
                    routes.len(),
                    routes_desc(routes)
                )
            }
        }
    }
}

/// 推荐理由人话文本（判据三要素「为什么」的扩写——含位移量：
/// 采纳前让作者看见热点将被挪多远）。
pub fn explain(rec: &Recommendation, from: (u16, u16)) -> String {
    let manhattan = (rec.hot.0 as i64 - from.0 as i64).abs() + (rec.hot.1 as i64 - from.1 as i64).abs();
    alloc::format!("{}：热点 ({},{}) → ({},{})，位移 {}px", rec.why, from.0, from.1, rec.hot.0, rec.hot.1, manhattan)
}

// ----- v4 深化批：批量补偿扫描报告（体检面批量口的报告层） -----

/// 全态批量扫描报告：类别计数 + 首个待修项人话——体检页「值得修的
/// 名单」的汇总面（batch_detect 是清单本体，本报告是它的计数页眉）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanReport {
    /// 实际扫过的态数（有帧的态；缺态不虚计）。
    pub scanned: usize,
    /// 有偏移的态数（值得修的名单长度）。
    pub flagged: usize,
    pub oob: usize,
    pub transparent: usize,
    pub edge: usize,
    /// 首个待修项（人话行——态名 + 推荐依据）。
    pub first_issue: Option<String>,
}

impl ScanReport {
    /// 全绿判定（零 flag 即体检页该方案热点项无待修）。
    pub fn is_clean(&self) -> bool {
        self.flagged == 0
    }
}

/// 对方案 15 态逐态首帧扫描并出报告（判定复用 detect_and_recommend
/// ——一处一事实，不另立第二套偏移判定）。
pub fn batch_scan_report(m: &CursorSchemeModel) -> ScanReport {
    let mut rep = ScanReport { scanned: 0, flagged: 0, oob: 0, transparent: 0, edge: 0, first_issue: None };
    for st in crate::jstar2::jbase::ALL_STATES {
        if let Some(rec) = detect_and_recommend(m, st, 0) {
            rep.scanned += 1;
            if rec.kind != OffsetKind::None {
                rep.flagged += 1;
                match rec.kind {
                    OffsetKind::OutOfBounds => rep.oob += 1,
                    OffsetKind::OnTransparent => rep.transparent += 1,
                    OffsetKind::OnEdge => rep.edge += 1,
                    OffsetKind::None => {}
                }
                if rep.first_issue.is_none() {
                    rep.first_issue = Some(alloc::format!("{}态：{}", st.zh_name(), rec.why));
                }
            }
        }
    }
    rep
}

// ----- v4 深化批：补偿历史台账（环形——采纳/撤销动作留痕） -----

/// 补偿历史台账容量（环形——超容丢最旧并计数，F372 留痕口径同源）。
pub const COMP_HISTORY_CAP: usize = 32;

/// 台账动作类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryAction {
    Adopt,
    Revoke,
}

impl HistoryAction {
    pub fn zh(self) -> &'static str {
        match self {
            HistoryAction::Adopt => "采纳补偿",
            HistoryAction::Revoke => "撤销补偿",
        }
    }
}

/// 台账行（一次补偿动作的完整凭据：何时/何态何帧/动作/位移方向）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub at_ms: u64,
    pub state: PointerState,
    pub frame_i: usize,
    pub action: HistoryAction,
    /// 位移起点（采纳=原热点；撤销=补偿值）。
    pub from: (u16, u16),
    /// 位移终点（采纳=补偿值；撤销=原热点）。
    pub to: (u16, u16),
}

/// 补偿历史台账（环形；与 CompensationBook 分账：Book 记「当前生效的
/// 补偿」，History 记「动作流水」——撤销后 Book 空而 History 仍在）。
#[derive(Clone, Debug, Default)]
pub struct CompensationHistory {
    entries: Vec<HistoryEntry>,
    dropped: usize,
}

impl CompensationHistory {
    /// 追加一条（超容丢最旧并计数——容量纪律）。
    pub fn push(&mut self, e: HistoryEntry) {
        if self.entries.len() >= COMP_HISTORY_CAP {
            self.entries.remove(0);
            self.dropped += 1;
        }
        self.entries.push(e);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 超容丢弃计数（容量纪律的如实面）。
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// 最新一条。
    pub fn latest(&self) -> Option<&HistoryEntry> {
        self.entries.last()
    }

    /// 全部清空（返回清掉的条数）。
    pub fn clear(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        n
    }

    /// 按态+帧过滤（对账面——查某帧的补偿动作史）。
    pub fn entries_for(&self, st: PointerState, frame_i: usize) -> Vec<&HistoryEntry> {
        self.entries.iter().filter(|e| e.state == st && e.frame_i == frame_i).collect()
    }

    /// 确定性人话渲染（审计导出面——同台账同字节）。
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        for (i, e) in self.entries.iter().enumerate() {
            out.push_str(&alloc::format!(
                "{}. [ms={}] {}态 帧{} {}：({},{})→({},{})\n",
                i + 1,
                e.at_ms,
                e.state.zh_name(),
                e.frame_i,
                e.action.zh(),
                e.from.0,
                e.from.1,
                e.to.0,
                e.to.1
            ));
        }
        out
    }
}

/// 采纳 + 留痕（一键采纳链路的台账面——采纳成功才写史，失败不虚记）。
pub fn log_adopt(
    book: &mut CompensationBook,
    hist: &mut CompensationHistory,
    m: &CursorSchemeModel,
    st: PointerState,
    frame_i: usize,
    rec: &Recommendation,
    at_ms: u64,
) -> bool {
    let Some(from) = m.state(st).and_then(|e| e.frames.get(frame_i)).map(|f| (f.hot_x, f.hot_y)) else {
        return false;
    };
    if !book.adopt(m, st, frame_i, rec, at_ms) {
        return false;
    }
    hist.push(HistoryEntry { at_ms, state: st, frame_i, action: HistoryAction::Adopt, from, to: rec.hot });
    true
}

/// 撤销 + 留痕（撤销的位移方向：补偿值 → 原热点；无记录可撤时不虚记）。
pub fn log_revoke(
    book: &mut CompensationBook,
    hist: &mut CompensationHistory,
    st: PointerState,
    frame_i: usize,
    at_ms: u64,
) -> bool {
    let Some((from, to)) = book
        .records
        .iter()
        .find(|r| r.state == st && r.frame_i == frame_i)
        .map(|r| (r.effective, r.original))
    else {
        return false;
    };
    if !book.revoke(st, frame_i) {
        return false;
    }
    hist.push(HistoryEntry { at_ms, state: st, frame_i, action: HistoryAction::Revoke, from, to });
    true
}

/// 补偿账有效性复检（F627 联动纵深：内容改版后逐条核对「补偿落点仍
/// 落实体」——指纹失配全数失效；返回仍有效条数，失效条供「建议重扫」）。
pub fn validate_book(book: &CompensationBook, m: &CursorSchemeModel) -> usize {
    if book.scheme_fingerprint != crate::jstar2::jbase::vxcur_fingerprint(m) {
        return 0;
    }
    let mut ok = 0usize;
    for r in &book.records {
        let valid = m
            .state(r.state)
            .and_then(|e| e.frames.get(r.frame_i))
            .map(|f| PixBuf::from_rgba(f.w, f.h, f.px.clone()).solid(r.effective.0, r.effective.1))
            .unwrap_or(false);
        if valid {
            ok += 1;
        }
    }
    ok
}

/// F637 v4 深化自检。
pub fn run_hotspotfix_v4_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F637-v4");
    let arrow = crate::jstar2::jbase::builtin_glyph(PointerState::Normal);

    // 坏点件：热点 (0,31) 落透明区（三路引擎的对拍底座）。
    let mut bad = CursorSchemeModel::empty("坏点件v4", OriginKind::Created);
    let mut f = arrow.clone();
    f.hot_x = 0;
    f.hot_y = 31;
    bad.set_state(PointerState::Normal, alloc::vec![f]);

    // 1. 多路汇裁：对称圆盘 + 中心基准 → 质心与邻域两路都指圆心，
    //    共识锁定圆心（包围盒路指上缘，不掺和——两路即成共识）。
    let mut disk = PixBuf::new(32, 32);
    crate::jstar2::jbase::fill_ellipse(&mut disk, 16, 16, 6, 6, [250, 250, 250, 255]);
    let (disk_routes, disk_verdict) = multi_route_recommend(&disk, (16, 16));
    set.add(
        "disk routes reach consensus at visual center",
        disk_routes.len() == 3 && disk_verdict == RouteVerdict::Consensus((16, 16)),
        "",
    );

    // 2. 真实坏点件三路各出推荐：全部落画幅内、置信度千分位合规；
    //    邻域路因基准点离实体超半径而如实弃权（半径有界不硬凑）。
    let (rec, routes, _verdict) = detect_and_recommend_multi(&bad, PointerState::Normal, 0).unwrap();
    set.add(
        "real scheme routes report permille confidence",
        routes.len() >= 2
            && routes.len() <= 3
            && routes.iter().all(|r| r.hot.0 < 32 && r.hot.1 < 32)
            && routes.iter().all(|r| r.confidence_m >= 0 && r.confidence_m <= 1000),
        "",
    );

    // 3. 邻域法几何：实体块旁的透明基准点吸附到最近实体（确定性）。
    let mut block = PixBuf::new(32, 32);
    crate::jstar2::jbase::fill_rect(&mut block, 10, 10, 20, 20, [60, 60, 60, 255]);
    let nb = route_neighborhood(&block, (5, 12)).unwrap();
    set.add("neighborhood snaps to nearest solid", nb.hot == (10, 12) && block.solid(nb.hot.0, nb.hot.1), "");

    // 4. 汇裁如实分歧：三路互异（合成样本——引擎不替人拍板）。
    let fake = alloc::vec![
        RouteRec { route: "a", hot: (0, 0), confidence_m: 900, note: "" },
        RouteRec { route: "b", hot: (10, 0), confidence_m: 800, note: "" },
        RouteRec { route: "c", hot: (20, 20), confidence_m: 700, note: "" },
    ];
    set.add("arbitrate reports honest divergence", arbitrate(&fake) == RouteVerdict::Divergent, "");

    // 5. 共识择优：容差内一对成立时取置信度高者的落点。
    let pair = alloc::vec![
        RouteRec { route: "a", hot: (5, 5), confidence_m: 900, note: "" },
        RouteRec { route: "b", hot: (6, 5), confidence_m: 800, note: "" },
        RouteRec { route: "c", hot: (30, 30), confidence_m: 950, note: "" },
    ];
    set.add("arbitrate picks higher confidence member", arbitrate(&pair) == RouteVerdict::Consensus((5, 5)), "");

    // 6. 批量扫描报告：坏点件只登记 Normal 态 → 扫 1 态、flag 1、透明类 1。
    let rep = batch_scan_report(&bad);
    set.add(
        "batch scan report counts kinds",
        rep.scanned == 1 && rep.flagged == 1 && rep.transparent == 1 && rep.first_issue.is_some(),
        "",
    );

    // 7. 健康件扫描全绿（15 态全扫、零 flag）。
    let healthy = crate::jstar2::jbase::builtin_default_scheme();
    let rep_ok = batch_scan_report(&healthy);
    set.add(
        "healthy scheme scans clean",
        rep_ok.scanned == crate::jstar2::jbase::ALL_STATES.len() && rep_ok.is_clean() && rep_ok.first_issue.is_none(),
        "",
    );

    // 8. 理由人话文本：含位移量（采纳前看得见热点挪多远）。
    let exp = explain(&rec, (0, 31));
    set.add("explain text carries displacement", exp.contains("位移") && exp.contains("(0,31)"), "");

    // 9. 历史台账环形：34 条只留 32、丢 2、最新一条是最后写入的。
    let mut hist = CompensationHistory::default();
    for i in 0..34u64 {
        hist.push(HistoryEntry { at_ms: i, state: PointerState::Normal, frame_i: 0, action: HistoryAction::Adopt, from: (0, 0), to: (1, 1) });
    }
    set.add(
        "history ring caps at 32 with drop count",
        hist.len() == COMP_HISTORY_CAP && hist.dropped() == 2 && hist.latest().map(|e| e.at_ms == 33).unwrap_or(false),
        "",
    );

    // 10. 采纳/撤销留痕链路：动作方向各就位（采纳 原→补偿，撤销 补偿→原）。
    let mut book = CompensationBook::default();
    let mut hist2 = CompensationHistory::default();
    let adopted = log_adopt(&mut book, &mut hist2, &bad, PointerState::Normal, 0, &rec, 100);
    let revoked = log_revoke(&mut book, &mut hist2, PointerState::Normal, 0, 200);
    set.add(
        "adopt and revoke write directional history",
        adopted
            && revoked
            && hist2.len() == 2
            && hist2.entries[0].action == HistoryAction::Adopt
            && hist2.entries[0].to == rec.hot
            && hist2.entries[1].action == HistoryAction::Revoke
            && hist2.entries[1].to == (0, 31)
            && hist2.entries_for(PointerState::Normal, 0).len() == 2,
        "",
    );

    // 11. 补偿账有效性复检：采纳后 1 条有效；内容改版（指纹变）后 0 条。
    let mut book3 = CompensationBook::default();
    let _ = book3.adopt(&bad, PointerState::Normal, 0, &rec, 300);
    let mut changed = bad.clone();
    changed.author = String::from("改版人");
    set.add(
        "validate book counts solid landings",
        validate_book(&book3, &bad) == 1 && validate_book(&book3, &changed) == 0,
        "",
    );

    // 12. 台账人话渲染确定性（同台账同字节）+ 含动作词。
    let r1 = hist2.render_text();
    let r2 = hist2.render_text();
    set.add(
        "history render deterministic",
        r1 == r2 && r1.contains("采纳补偿") && r1.contains("撤销补偿"),
        "",
    );

    // 13. 汇裁人话文本：分歧面点名列路、共识面给落点。
    let diverge_txt = verdict_text(&RouteVerdict::Divergent, &fake);
    let consensus_txt = verdict_text(&disk_verdict, &disk_routes);
    set.add(
        "verdict text names routes and hot",
        diverge_txt.contains("如实分歧") && diverge_txt.contains("人工标定")
            && consensus_txt.contains("(16,16)") && consensus_txt.contains("容差内一致"),
        "",
    );

    // 14. 全透明帧：三路全弃权 → 空表如实分歧 + 人话指人工标定。
    let empty_buf = PixBuf::new(32, 32);
    let (empty_routes, empty_verdict) = multi_route_recommend(&empty_buf, (16, 16));
    set.add(
        "empty buf abstains honestly",
        empty_routes.is_empty() && empty_verdict == RouteVerdict::Divergent
            && verdict_text(&empty_verdict, &empty_routes).contains("人工标定"),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// v4 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_v4 {
    use super::*;

    #[test]
    fn v4_disk_routes_consensus_at_center() {
        let mut cv = PixBuf::new(32, 32);
        crate::jstar2::jbase::fill_ellipse(&mut cv, 16, 16, 6, 6, [250, 250, 250, 255]);
        let (routes, verdict) = multi_route_recommend(&cv, (16, 16));
        assert_eq!(routes.len(), 3);
        assert_eq!(verdict, RouteVerdict::Consensus((16, 16)), "对称盘三路应锁定圆心");
    }

    #[test]
    fn v4_neighborhood_bounded_and_honest() {
        let empty = PixBuf::new(32, 32);
        assert!(route_neighborhood(&empty, (16, 16)).is_none(), "全透明帧扫不到实体应弃权");
        let mut cv = PixBuf::new(32, 32);
        crate::jstar2::jbase::fill_rect(&mut cv, 10, 10, 20, 20, [1, 2, 3, 255]);
        let r = route_neighborhood(&cv, (16, 16)).unwrap();
        assert_eq!(r.hot, (16, 16), "基准点已落实体 → 零位移");
        assert_eq!(r.confidence_m, 1000);
    }

    #[test]
    fn v4_history_ring_evicts_oldest() {
        let mut hist = CompensationHistory::default();
        for i in 0..(COMP_HISTORY_CAP + 3) as u64 {
            hist.push(HistoryEntry { at_ms: i, state: PointerState::Normal, frame_i: 0, action: HistoryAction::Adopt, from: (0, 0), to: (0, 0) });
        }
        assert_eq!(hist.len(), COMP_HISTORY_CAP);
        assert_eq!(hist.dropped(), 3);
        assert_eq!(hist.entries[0].at_ms, 3, "最旧三条被挤出环");
        assert_eq!(hist.clear(), COMP_HISTORY_CAP);
        assert!(hist.is_empty());
    }

    #[test]
    fn v4_scan_report_counts_oob() {
        let mut m = CursorSchemeModel::empty("出界v4", OriginKind::Created);
        let mut g = crate::jstar2::jbase::builtin_glyph(PointerState::Normal);
        g.hot_x = 200;
        g.hot_y = 200;
        m.set_state(PointerState::Normal, alloc::vec![g]);
        let rep = batch_scan_report(&m);
        assert_eq!(rep.scanned, 1);
        assert_eq!(rep.oob, 1);
        assert_eq!(rep.flagged, 1);
        let issue = rep.first_issue.unwrap();
        assert!(issue.contains("正常选择"), "首个待修项应带态名: {issue}");
    }

    #[test]
    fn v4_log_roundtrip_directions() {
        let mut m = CursorSchemeModel::empty("留痕件", OriginKind::Created);
        let mut g = crate::jstar2::jbase::builtin_glyph(PointerState::Normal);
        g.hot_x = 0;
        g.hot_y = 31;
        m.set_state(PointerState::Normal, alloc::vec![g]);
        let rec = detect_and_recommend(&m, PointerState::Normal, 0).unwrap();
        let mut book = CompensationBook::default();
        let mut hist = CompensationHistory::default();
        assert!(log_adopt(&mut book, &mut hist, &m, PointerState::Normal, 0, &rec, 10));
        assert!(log_revoke(&mut book, &mut hist, PointerState::Normal, 0, 20));
        assert!(!log_revoke(&mut book, &mut hist, PointerState::Normal, 0, 30), "无记录可撤不虚记");
        assert_eq!(hist.entries[0].from, (0, 31));
        assert_eq!(hist.entries[1].from, rec.hot);
        assert_eq!(hist.entries[1].to, (0, 31));
    }
}
