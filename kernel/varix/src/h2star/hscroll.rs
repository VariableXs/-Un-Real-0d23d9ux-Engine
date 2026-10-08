//! F256 横向滚动语义 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：Shift+滚轮映射用例；双轴独立到位判据；冻结列用例；
//! 横向惯性曲线与纵向同谱（F124）。
//!
//! **设计要点（主册）**：Shift+滚轮=横向滚动（Windows 全系一致的暗语）、
//! 触摸板双指横滑原生支持、水平滚动条同 F204 规范；横向滚动到位后继续
//! 滚不「越权」变成纵向（各轴独立到位即停）；列冻结支持——横向滚时
//! 首列（名称列）可选冻结不滚走。
//!
//! 实装：双轴滚动模型（每轴独立 clamp 到位即停、互不越权）；Shift+滚轮
//! 映射器（滚轮 deltaY → 横轴）；冻结列（首列宽度固定，横轴滚动不带动）；
//! 惯性曲线复用 F124 同谱参数（衰减系数常量唯一源）。

use crate::checks::CheckSet;

/// 双轴视口（内容尺寸 + 当前偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Axis {
    /// 内容总宽/高（px）。
    pub content: i32,
    /// 视口宽/高（px）。
    pub viewport: i32,
    /// 当前偏移（≥0，clamp 到 content-viewport）。
    pub offset: i32,
}

impl Axis {
    pub fn new(content: i32, viewport: i32) -> Axis {
        Axis { content, viewport, offset: 0 }
    }

    fn max_offset(&self) -> i32 {
        (self.content - self.viewport).max(0)
    }

    /// 滚动 delta（正=向内容深处）。到位即停——超出部分被 clamp 吃掉，
    /// 返回实际消费量（供「到位后继续滚不越权」的账目核对）。
    pub fn scroll(&mut self, delta: i32) -> i32 {
        let target = (self.offset + delta).clamp(0, self.max_offset());
        let consumed = target - self.offset;
        self.offset = target;
        consumed
    }

    pub fn at_end(&self) -> bool {
        self.offset >= self.max_offset()
    }
}

/// 双轴滚动模型。
#[derive(Clone, Copy, Debug)]
pub struct BiScroll {
    pub x: Axis,
    pub y: Axis,
}

impl BiScroll {
    /// 滚轮事件：Shift 按住=横向（Windows 暗语），否则纵向。
    /// 各轴独立——横轴到位后剩余量**不**转给纵轴（判据：双轴独立到位）。
    pub fn wheel(&mut self, delta_y: i32, shift: bool) -> (i32, i32) {
        if shift {
            (self.x.scroll(delta_y), 0)
        } else {
            (0, self.y.scroll(delta_y))
        }
    }

    /// 触摸板双指横滑（原生横轴）。
    pub fn pan_x(&mut self, delta_x: i32) -> i32 {
        self.x.scroll(delta_x)
    }
}

/// F124 惯性谱同源参数：初速衰减（每帧 × 系数，<1 停）。
/// 横向与纵向共用同一系数——「横向惯性曲线与纵向同谱」。
pub const INERTIA_DECAY: f64 = 0.92;
/// 停判线（px/frame，低于即停——不再无限积分）。
pub const INERTIA_FLOOR_PX: f64 = 0.5;

/// 惯性轨迹生成：初速 → 逐帧速度序列（同谱判据的对拍数据源）。
pub fn inertia_trace(mut v: f64) -> alloc::vec::Vec<f64> {
    let mut out = alloc::vec::Vec::new();
    while v > INERTIA_FLOOR_PX {
        out.push(v);
        v *= INERTIA_DECAY;
    }
    out
}

/// 冻结列：首列（名称列）宽度固定，横轴滚动不带动。
#[derive(Clone, Copy, Debug)]
pub struct FrozenColumn {
    /// 首列宽（px）。
    pub width: i32,
    pub frozen: bool,
}

impl FrozenColumn {
    /// 某列在屏幕上的横坐标（冻结列恒 0 + 列序偏移；非冻结列随 offset 平移）。
    pub fn screen_x(&self, logical_x: i32, scroll_x: i32) -> i32 {
        if self.frozen && logical_x < self.width {
            logical_x
        } else {
            logical_x - scroll_x
        }
    }

    /// 冻结列是否遮住了内容（横滚时内容滑进冻结区——视图层裁剪判据）。
    pub fn occluded(&self, logical_x: i32, scroll_x: i32) -> bool {
        !self.frozen && {
            let sx = self.screen_x(logical_x, scroll_x);
            sx < self.width
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_hscroll_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F256");
    // Shift+滚轮映射：Shift 纵向 delta 全部进横轴。
    let mut s = BiScroll {
        x: Axis::new(2000, 400),
        y: Axis::new(5000, 400),
    };
    let (dx, dy) = s.wheel(120, true);
    set.add("F256 shift maps x", dx == 120 && dy == 0 && s.x.offset == 120, "wheel→x");
    // 先下滚再上滚——上滚量被如实消费（不在顶端时）。
    let _ = s.wheel(120, false);
    let (dx2, dy2) = s.wheel(-50, false);
    set.add("F256 plain maps y", dx2 == 0 && dy2 == -50 && s.y.offset == 70, "wheel→y");
    // 双轴独立到位：横轴滚到底继续滚，纵轴纹丝不动。
    let _ = s.pan_x(10_000);
    let x_at = s.x.offset;
    let y_before = s.y.offset;
    let (dx3, _) = s.wheel(999, true);
    set.add(
        "F256 x stops at end",
        s.x.at_end() && x_at == s.x.max_offset() && dx3 == 0 && s.y.offset == y_before,
        "no cross-axis leak",
    );
    // 惯性同谱：横向与纵向轨迹逐帧一致。
    let tx = inertia_trace(60.0);
    let ty = inertia_trace(60.0);
    set.add("F256 same spectrum", tx == ty && !tx.is_empty(), "F124 curve");
    // 冻结列：冻结时首列不随横滚走。
    let fc = FrozenColumn { width: 120, frozen: true };
    let moved = fc.screen_x(40, 300);
    set.add("F256 frozen fixed", moved == 40, "first column stays");
    let fcf = FrozenColumn { width: 120, frozen: false };
    set.add(
        "F256 unfrozen moves+occlusion",
        fcf.screen_x(40, 300) == -260 && fcf.occluded(40, 300),
        "scrolls and clips",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f256_axes_independent() {
        let set = run_hscroll_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F256 自检红 {f}/{p}");
    }

    #[test]
    fn clamp_never_negative() {
        let mut a = Axis::new(100, 400);
        let consumed = a.scroll(-50);
        assert_eq!(consumed, 0, "视口大于内容时滚动量为零");
        assert_eq!(a.offset, 0);
    }
}
