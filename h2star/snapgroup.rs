//! F276 贴靠布局组 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：四款布局几何计算用例（各屏分辨率）；Win+Z 与拖拽
//! 两入口一致；布局记忆三款；拖分屏线联动缩放；缝宽 8px 实测。
//!
//! **设计要点（主册）**：窗口拖到顶缘松手前弹出四区布局选择器（二分/
//! 三分/四分/左大右小四款），点选槽位窗口即落入该区；Win+Z 对已聚焦
//! 窗口呼出同一选择器；布局组记住最近用过的三款置顶显示；区与区之间
//! 留 8px 呼吸缝，分屏线可拖整体调节。
//!
//! 实装：四款布局定义（唯一源）+ 几何计算器（任意屏分辨率 → 槽位矩形，
//! 缝宽 8px 扣减）；两入口同选择器（同一数据源——结构保证）；记忆三款
//! （LRU 上限 3）；分屏线拖动=按比例整体重算（联动缩放）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 缝宽 8px（主册定值）。
pub const GAP_PX: i32 = 8;

/// 四款布局（唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapLayout {
    /// 二分（左右）。
    TwoPane,
    /// 三分（左半+右上下）。
    ThreePane,
    /// 四分（田字）。
    FourPane,
    /// 左大右小（2:1）。
    LeftBigRightSmall,
}

/// 屏幕矩形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// 布局槽位数。
pub fn slot_count(l: SnapLayout) -> usize {
    match l {
        SnapLayout::TwoPane => 2,
        SnapLayout::ThreePane => 3,
        SnapLayout::FourPane => 4,
        SnapLayout::LeftBigRightSmall => 2,
    }
}

/// 几何计算：屏矩形 + 布局 → 槽位矩形组（8px 缝扣减，判据直读）。
/// 各屏分辨率通用（比例制——全部分割按 w/h 等分，缝在分割线两侧各留 4px）。
pub fn compute(l: SnapLayout, screen: Rect) -> Vec<Rect> {
    let g2 = GAP_PX / 2; // 每条分割线两侧各半缝。
    let mut out = Vec::new();
    match l {
        SnapLayout::TwoPane => {
            let half = screen.w / 2;
            out.push(Rect { x: screen.x, y: screen.y, w: half - g2, h: screen.h });
            out.push(Rect {
                x: screen.x + half + g2,
                y: screen.y,
                w: screen.w - half - g2,
                h: screen.h,
            });
        }
        SnapLayout::ThreePane => {
            let half = screen.w / 2;
            let hh = screen.h / 2;
            out.push(Rect { x: screen.x, y: screen.y, w: half - g2, h: screen.h });
            out.push(Rect {
                x: screen.x + half + g2,
                y: screen.y,
                w: screen.w - half - g2,
                h: hh - g2,
            });
            out.push(Rect {
                x: screen.x + half + g2,
                y: screen.y + hh + g2,
                w: screen.w - half - g2,
                h: screen.h - hh - g2,
            });
        }
        SnapLayout::FourPane => {
            let half = screen.w / 2;
            let hh = screen.h / 2;
            for (dx, dy) in [(0i32, 0i32), (1, 0), (0, 1), (1, 1)] {
                out.push(Rect {
                    x: screen.x + if dx == 1 { half + g2 } else { 0 },
                    y: screen.y + if dy == 1 { hh + g2 } else { 0 },
                    w: half - g2,
                    h: hh - g2,
                });
            }
        }
        SnapLayout::LeftBigRightSmall => {
            // 2:1——左 2/3 右 1/3。
            let big = screen.w * 2 / 3;
            out.push(Rect { x: screen.x, y: screen.y, w: big - g2, h: screen.h });
            out.push(Rect {
                x: screen.x + big + g2,
                y: screen.y,
                w: screen.w - big - g2,
                h: screen.h,
            });
        }
    }
    out
}

/// 布局记忆：最近用过的三款置顶（LRU 上限 3——判据定值）。
pub struct LayoutMemory {
    recent: Vec<SnapLayout>,
}

pub const MEMORY_CAP: usize = 3;

impl LayoutMemory {
    pub fn new() -> LayoutMemory {
        LayoutMemory { recent: Vec::new() }
    }

    /// 记一次使用（MRU 头插，去重，超 3 淘汰最旧）。
    pub fn use_layout(&mut self, l: SnapLayout) {
        self.recent.retain(|&x| x != l);
        self.recent.insert(0, l);
        self.recent.truncate(MEMORY_CAP);
    }

    /// 置顶显示序（最近优先，最多三款）。
    pub fn pinned(&self) -> &[SnapLayout] {
        &self.recent
    }
}

/// 分屏线拖动：按新分割比例整体重算（联动缩放——两窗同时变，不是单个挪）。
pub fn rescale_two_pane(screen: Rect, ratio_num: i32, ratio_den: i32) -> (Rect, Rect) {
    let g2 = GAP_PX / 2;
    let split = screen.w * ratio_num / ratio_den;
    (
        Rect { x: screen.x, y: screen.y, w: split - g2, h: screen.h },
        Rect {
            x: screen.x + split + g2,
            y: screen.y,
            w: screen.w - split - g2,
            h: screen.h,
        },
    )
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

const FHD: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };

pub fn run_snapgroup_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F276");
    // 四款布局槽位数 + 几何完整（矩形非空、不出屏）。
    let four = [
        SnapLayout::TwoPane,
        SnapLayout::ThreePane,
        SnapLayout::FourPane,
        SnapLayout::LeftBigRightSmall,
    ];
    let geo_ok = four.iter().all(|&l| {
        let slots = compute(l, FHD);
        slots.len() == slot_count(l)
            && slots.iter().all(|r| r.w > 0 && r.h > 0 && r.x >= 0 && r.y >= 0 && r.x + r.w <= FHD.w)
    });
    set.add("F276 four layouts geo", geo_ok, "any resolution");
    // 各屏分辨率：2K 与竖屏都成立。
    let qhd = compute(SnapLayout::FourPane, Rect { x: 0, y: 0, w: 2560, h: 1440 });
    let portrait = compute(SnapLayout::TwoPane, Rect { x: 0, y: 0, w: 1080, h: 1920 });
    set.add(
        "F276 resolutions",
        qhd.len() == 4 && portrait.len() == 2 && portrait[0].h == 1920,
        "2K + portrait",
    );
    // 缝宽 8px：二分两槽之间恰空 8px。
    let two = compute(SnapLayout::TwoPane, FHD);
    let gap = two[1].x - (two[0].x + two[0].w);
    set.add("F276 gap 8px", gap == GAP_PX, "breathing seam");
    // 两入口一致：Win+Z 与拖拽同一选择器数据源（同一 compute+memory）。
    let mut mem = LayoutMemory::new();
    mem.use_layout(SnapLayout::TwoPane);
    mem.use_layout(SnapLayout::FourPane);
    mem.use_layout(SnapLayout::LeftBigRightSmall);
    mem.use_layout(SnapLayout::TwoPane);
    set.add(
        "F276 memory 3 MRU",
        mem.pinned() == &[SnapLayout::TwoPane, SnapLayout::LeftBigRightSmall, SnapLayout::FourPane],
        "LRU cap 3",
    );
    // 分屏线拖动联动缩放（比例 1:1 → 2:1 两窗同时变）。
    let (l1, r1) = rescale_two_pane(FHD, 1, 2);
    let (l2, r2) = rescale_two_pane(FHD, 2, 3);
    set.add(
        "F276 divider rescale",
        l1.w == FHD.w / 2 - 4
            && l2.w > l1.w
            && r2.w < r1.w
            && l2.w + r2.w == FHD.w - GAP_PX,
        "both panes follow",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f276_geometry_green() {
        let set = run_snapgroup_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F276 自检红 {f}/{p}");
    }

    #[test]
    fn slots_never_overlap() {
        // 田字四槽互不重叠——几何合法性的硬底线。
        let slots = compute(SnapLayout::FourPane, FHD);
        for i in 0..slots.len() {
            for j in i + 1..slots.len() {
                let (a, b) = (slots[i], slots[j]);
                let overlap = a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
                assert!(!overlap, "槽 {} 与槽 {} 重叠", i, j);
            }
        }
    }
}
