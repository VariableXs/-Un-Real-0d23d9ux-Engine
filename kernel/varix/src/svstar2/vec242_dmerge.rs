//! VE-F0642 脏区合并算法（VE-D 域 · 目标 400 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0642`
//!
//! **判据（锚点原文逐条）**：
//! - **相交合并与相邻合并**（相邻判定带容差，容差 < 边距膨胀值防缝隙）；
//! - **收益成式**：合并后面积 对比 分开绘制 + 边距重复成本（不许拍脑袋）；
//! - **条带优先**：水平/垂直对齐矩形优先条带合并（局部性最优）；
//! - **上限信号**：区域数超限或合并后覆盖率超阈 → 建议升档全帧（发 F0644）；
//! - **判据**：收益为负不合并保留碎片；容差与膨胀表对齐。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::Rect;

/// 两矩形是否相交
pub fn intersects(a: Rect, b: Rect) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// 相邻判定（带容差 gap：间距 ≤ gap 视为相邻；gap 须小于边距膨胀值）
pub fn adjacent(a: Rect, b: Rect, gap: u32) -> bool {
    // 水平相邻：垂直区间重叠，水平间距 ≤ gap
    let v_ovl = a.1 < b.3 && b.1 < a.3;
    let h_adj = if a.2 <= b.0 { b.0 - a.2 <= gap } else if b.2 <= a.0 { a.0 - b.2 <= gap } else { true };
    let h_ovl = a.0 < b.2 && b.0 < a.2;
    let v_adj = if a.3 <= b.1 { b.1 - a.3 <= gap } else if b.3 <= a.1 { a.1 - b.3 <= gap } else { true };
    (v_ovl && h_adj) || (h_ovl && v_adj)
}

/// 并集矩形
pub fn union(a: Rect, b: Rect) -> Rect {
    (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), b.3.max(a.3))
}

pub fn area(r: Rect) -> u64 {
    (r.2.saturating_sub(r.0) as u64) * (r.3.saturating_sub(r.1) as u64)
}

/// 条带判定：水平或垂直对齐（同一行/列带内，另一维有重叠）
pub fn stripe_aligned(a: Rect, b: Rect) -> bool {
    let v_ovl = a.1 < b.3 && b.1 < a.3;
    let h_ovl = a.0 < b.2 && b.0 < a.2;
    let same_band_v = (a.1 == b.1 && a.3 == b.3) || v_ovl;
    let same_band_h = (a.0 == b.0 && a.2 == b.2) || h_ovl;
    (a.1 < b.3 && b.1 < a.3 && a.2 <= b.0 || b.2 <= a.0) && same_band_v
        || (a.0 < b.2 && b.0 < a.2 && a.3 <= b.1 || b.3 <= a.1) && same_band_h
}

/// 合并收益模型（成式）：
/// 合并收益 = 分开绘制面积和 − 合并后面积（中间多绘区域为负收益）
/// 再叠加边距膨胀重复成本（合并后需重绘的边距带只算一次）
pub fn merge_gain(a: Rect, b: Rect, margin: u32) -> i64 {
    let sep = (area(a) + area(b)) as i64;
    let m = union(a, b);
    let merged = area(m) as i64;
    // 边距带重复成本：合并方案需要重绘合并矩形的边距带（一次），分开方案是两份
    let band_a = margin_band_area(a, margin) as i64;
    let band_b = margin_band_area(b, margin) as i64;
    let band_m = margin_band_area(m, margin) as i64;
    // 分开成本 = sep + band_a + band_b；合并成本 = merged + band_m
    (sep + band_a + band_b) - (merged + band_m)
}

/// 边距膨胀带面积（矩形外扩 margin 的环带）
pub fn margin_band_area(r: Rect, margin: u32) -> u64 {
    let outer = (
        r.0.saturating_sub(margin),
        r.1.saturating_sub(margin),
        r.2.saturating_add(margin),
        r.3.saturating_add(margin),
    );
    area(outer) - area(r)
}

/// 合并上限信号
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MergeCap {
    /// 区域数上限（超限建议全帧）
    pub region_max: u32,
    /// 合并后覆盖率上限
    pub cover_max: f32,
    pub frame_area: u64,
}

pub const DEFAULT_MERGE_CAP: MergeCap = MergeCap {
    region_max: 64,
    cover_max: 0.5,
    frame_area: 1_000_000,
};

impl MergeCap {
    /// 上限信号：命中任一 → 建议升档全帧（信号发 F0644）
    pub fn exceeded(&self, regions: &[Rect]) -> bool {
        if regions.len() as u32 > self.region_max {
            return true;
        }
        let total: u64 = regions.iter().map(|&r| area(r)).sum();
        total as f32 / self.frame_area as f32 > self.cover_max
    }
}

/// 一轮贪心合并：相交/相邻（容差内）且收益为正才合并；条带对优先
pub fn merge_round(regions: &[Rect], gap: u32, margin: u32) -> (Vec<Rect>, u32) {
    let mut rs: Vec<Rect> = regions.to_vec();
    let mut merges = 0u32;
    let mut changed = true;
    while changed && rs.len() >= 2 {
        changed = false;
        'outer: for i in 0..rs.len() {
            for j in (i + 1)..rs.len() {
                let (a, b) = (rs[i], rs[j]);
                let mergeable = intersects(a, b) || adjacent(a, b, gap) || stripe_aligned(a, b);
                if mergeable && merge_gain(a, b, margin) > 0 {
                    let m = union(a, b);
                    rs.remove(j);
                    rs.remove(i);
                    rs.push(m);
                    merges += 1;
                    changed = true;
                    break 'outer;
                }
            }
        }
    }
    (rs, merges)
}

/// 容差对齐校验：合并容差必须小于边距膨胀值（防合并后缝隙重绘）
pub fn gap_aligned(gap: u32, margin: u32) -> bool {
    gap < margin
}

pub fn run_vec242_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec242");
    // 族一：相交合并
    s.add("合并-相交判定", intersects((0, 0, 10, 10), (5, 5, 15, 15)), "overlap");
    s.add("合并-相离不交", !intersects((0, 0, 10, 10), (20, 20, 30, 30)), "apart");
    let (out, n) = merge_round(&[(0, 0, 10, 10), (5, 5, 15, 15)], 0, 0);
    s.add("合并-相交归一", out.len() == 1 && n == 1, "merged to 1");
    s.add("合并-并集框", out[0] == (0, 0, 15, 15), "union rect");
    // 族二：相邻合并带容差
    s.add("合并-间距3容差4相邻", adjacent((0, 0, 10, 10), (13, 0, 20, 10), 4), "gap ≤ tol");
    s.add("合并-间距3容差2不相邻", !adjacent((0, 0, 10, 10), (13, 0, 20, 10), 2), "gap > tol");
    s.add("合并-贴邻", adjacent((0, 0, 10, 10), (10, 0, 20, 10), 0), "touching");
    // 族三：收益成式
    // 相交对收益必为正（合并面积 < 分开面积和）
    s.add("收益-相交必正", merge_gain((0, 0, 10, 10), (5, 5, 15, 15), 0) > 0, "overlap gain");
    // 远距对收益为负 → 保留碎片
    let (out2, n2) = merge_round(&[(0, 0, 10, 10), (500, 500, 510, 510)], 2, 1);
    s.add("收益-负不合并", out2.len() == 2 && n2 == 0, "negative keep");
    s.add("收益-远距为负", merge_gain((0, 0, 10, 10), (500, 500, 510, 510), 1) < 0, "gain negative");
    // 族四：条带优先
    s.add("条带-水平对齐", stripe_aligned((0, 0, 10, 10), (10, 0, 20, 10)), "h stripe");
    s.add("条带-错位非条带", !stripe_aligned((0, 0, 10, 10), (10, 5, 20, 15)), "offset not stripe");
    let (out3, n3) = merge_round(&[(0, 0, 10, 10), (10, 0, 20, 10), (40, 40, 50, 50)], 1, 1);
    s.add("条带-条带并远区保留", out3.iter().any(|&r| r == (0, 0, 20, 10)) && out3.len() == 2 && n3 >= 1, "stripe merged");
    // 族五：上限信号
    let cap = DEFAULT_MERGE_CAP;
    let many: Vec<Rect> = (0..70).map(|i| (i * 3, 0, i * 3 + 2, 2)).collect();
    s.add("上限-区域数超限", cap.exceeded(&many), "count exceeded");
    let big = [(0u32, 0u32, 800u32, 800u32)];
    s.add("上限-覆盖率超阈", cap.exceeded(&big), "cover exceeded");
    let small = [(0u32, 0u32, 100u32, 100u32)];
    s.add("上限-正常不发", !cap.exceeded(&small), "ok");
    // 族六：容差对齐
    s.add("容差-gap<margin", gap_aligned(2, 4), "aligned");
    s.add("容差-失配检出", !gap_aligned(4, 4), "gap ≥ margin rejected");
    // 族七：边距带
    s.add("边距-零边距零带", margin_band_area((0, 0, 10, 10), 0) == 0, "no margin no band");
    s.add("边距-带面积", margin_band_area((10, 10, 20, 20), 2) == 144 - 100, "band 44");
    s
}
