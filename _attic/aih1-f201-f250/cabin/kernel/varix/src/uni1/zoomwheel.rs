//! F430 Ctrl+滚轮视图缩放 · 完整设计（STAR I 主册 G-I-30）。
//!
//! **判据（主册）**：三场景缩放用例；五档图标切换阈值；锚点缩放准确性
//! （放大后鼠标下的内容仍在鼠标下）；边界行为；记忆联动。＋通12。
//!
//! 设计：视图缩放核——三场景（图标五档/文本字号四档/图片锚点缩放）
//! 统一档位步进模型；五档图标（超大/大/中/小/列表）阈值切换；锚点
//! 缩放数学（鼠标点为不动点：content_offset' = mouse - (mouse-offset)*k'/k）；
//! 边界停住 + 微弹提示账；档位记忆（F219 联动）。

use crate::checks::CheckSet;

/// 图标视图五档。
pub const ICON_MODES: [&str; 5] = ["超大图标", "大图标", "中图标", "小图标", "列表"];

/// 图片缩放倍率边界（0.1x-8x）。
pub const ZOOM_MIN_PERMILLE: u64 = 100;
pub const ZOOM_MAX_PERMILLE: u64 = 8_000;
/// 每格滚轮步进（25%）。
pub const ZOOM_STEP_PERMILLE: u64 = 250;

/// 档位步进核（图标五档 / 文本字号四档共用同一模型——三场景判据）。
pub struct StepZoom {
    pub mode: usize,
    /// 档位数（图标 5 / 文本 4）。
    pub levels: usize,
}

impl StepZoom {
    pub fn new(levels: usize) -> StepZoom {
        StepZoom { mode: 0, levels: levels.max(1) }
    }

    /// Ctrl+滚轮：+1 向大档、-1 向小档；边界停住（返回 false = 已在边界）。
    pub fn wheel(&mut self, dir: i32) -> bool {
        let next = self.mode as i32 - dir; // 滚轮向上（dir=+1）→ 更大（index 减）
        if next < 0 || next as usize >= self.levels {
            return false;
        }
        self.mode = next as usize;
        true
    }
}

/// 图片锚点缩放核。
pub struct AnchorZoom {
    /// 当前倍率（千分比）。
    pub permille: u64,
    /// 内容偏移（内容原点相对视口的位置）。
    pub offset: (i64, i64),
    /// 边界微弹提示账。
    pub bounce_hints: u64,
}

impl AnchorZoom {
    pub fn new() -> AnchorZoom {
        AnchorZoom { permille: 1_000, offset: (0, 0), bounce_hints: 0 }
    }

    /// 锚点缩放：以鼠标点（视口坐标）为不动点。
    /// 数学：new_offset = mouse - (mouse - old_offset) * new/old。
    /// 边界语义：越界请求先记微弹提示，再钳到边界值——末段半步贴边
    /// （步长与边界不对齐时也能真正到达 min/max）；已贴边再越界则停住。
    pub fn wheel_at(&mut self, mouse: (i64, i64), dir: i32) -> bool {
        let old = self.permille as i64;
        let raw = old + dir as i64 * ZOOM_STEP_PERMILLE as i64;
        let (min, max) = (ZOOM_MIN_PERMILLE as i64, ZOOM_MAX_PERMILLE as i64);
        let new = if raw < min || raw > max {
            self.bounce_hints += 1;
            let clamped = raw.clamp(min, max);
            if clamped == old {
                return false; // 已贴边界：停住（微弹提示已记账）
            }
            clamped // 末段半步贴边
        } else {
            raw
        };
        self.permille = new as u64;
        self.offset = (
            mouse.0 - (mouse.0 - self.offset.0) * new / old,
            mouse.1 - (mouse.1 - self.offset.1) * new / old,
        );
        true
    }

    /// 锚点准确性：缩放前后鼠标点下的内容坐标不变。
    /// 内容坐标 = (mouse - offset) / scale = (mouse - offset) * 1000 / permille。
    pub fn content_under_mouse(&self, mouse: (i64, i64)) -> (i64, i64) {
        (
            (mouse.0 - self.offset.0) * 1_000 / self.permille as i64,
            (mouse.1 - self.offset.1) * 1_000 / self.permille as i64,
        )
    }

    /// 记忆联动快照（F219）。
    pub fn snapshot(&self) -> (u64, (i64, i64)) {
        (self.permille, self.offset)
    }
}

pub fn run_zoomwheel_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F430");
    set.add(
        "f430-five-modes",
        ICON_MODES == ["超大图标", "大图标", "中图标", "小图标", "列表"],
        "",
    );
    // 场景一：图标五档切换 + 边界停住。
    let mut iz = StepZoom { mode: 2, levels: ICON_MODES.len() };
    set.add(
        "f430-icon-cycle",
        iz.wheel(1) && iz.mode == 1 && iz.wheel(-1) && iz.mode == 2,
        "",
    );
    iz.mode = 0;
    set.add("f430-icon-bound-top", !iz.wheel(1) && iz.mode == 0, "");
    iz.mode = 4;
    set.add("f430-icon-bound-bottom", !iz.wheel(-1) && iz.mode == 4, "");
    // 场景二：文本字号四档（同一模型不同档集——档位步进一致）。
    // 小(0)/标准(1)/大(2)/特大(3)：标准起两步向大字端（dir=-1，index+1）→ 特大。
    let mut fz = StepZoom { mode: 1, levels: 4 };
    set.add(
        "f430-text-four-steps",
        fz.wheel(-1) && fz.wheel(-1) && fz.mode == 3 && !fz.wheel(-1),
        "",
    );
    set.add("f430-text-bound-small", fz.wheel(1) && fz.wheel(1) && fz.wheel(1) && fz.wheel(1) == false && fz.mode == 0, "");
    // 场景三：锚点缩放数学。
    let mut z = AnchorZoom::new();
    // 视口 1000×800；内容 2000×1600 初始 1x；鼠标 (500,400) 指向内容中心。
    z.offset = (0, 0);
    let before = z.content_under_mouse((500, 400));
    set.add("f430-anchor-before", before == (500, 400), "");
    // 放大 1.25x：offset' = 500 - 500*1250/1000 = -125。
    set.add("f430-wheel-ok", z.wheel_at((500, 400), 1), "");
    set.add("f430-anchor-after", z.permille == 1_250 && z.offset == (-125, -100), "");
    let after = z.content_under_mouse((500, 400));
    set.add(
        "f430-anchor-invariant",
        after == before,
        "",
    );
    // 边界行为：连续放大到 8x → 停住 + 微弹。
    let mut b = AnchorZoom::new();
    let mut hits = 0;
    for _ in 0..40 {
        if !b.wheel_at((0, 0), 1) {
            hits += 1;
        }
    }
    set.add(
        "f430-boundary-bounce",
        b.permille == ZOOM_MAX_PERMILLE && hits >= 1 && b.bounce_hints == hits,
        "",
    );
    // 缩小边界同理。
    let mut s = AnchorZoom::new();
    for _ in 0..10 {
        let _ = s.wheel_at((0, 0), -1);
    }
    set.add("f430-boundary-min", s.permille == ZOOM_MIN_PERMILLE && s.bounce_hints >= 1, "");
    // 记忆联动快照。
    set.add("f430-memory-snapshot", z.snapshot() == (1_250, (-125, -100)), "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_math_asymmetric_mouse() {
        let mut z = AnchorZoom::new();
        z.offset = (100, 50);
        let m = (300, 250);
        let before = z.content_under_mouse(m);
        let _ = z.wheel_at(m, -1); // 缩小到 0.75x
        let after = z.content_under_mouse(m);
        assert_eq!(before, after, "锚点不动点判据（非中心鼠标位）");
        assert_eq!(z.permille, 750);
    }
}
