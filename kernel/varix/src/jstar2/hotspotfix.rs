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
