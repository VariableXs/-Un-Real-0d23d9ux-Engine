//! F430 Ctrl+滚轮视图缩放 · 完整设计（STAR I 主册 G-I-30）。
//!
//! **判据（主册）**：三场景缩放用例；五档图标切换阈值；锚点缩放准确性
//! （放大后鼠标下的内容仍在鼠标下）；边界行为；记忆联动。＋通12。
//!
//! 设计：视图缩放核——三场景（图标五档/文本字号四档/图片锚点缩放）
//! 统一档位步进模型；五档图标（超大/大/中/小/列表）阈值切换；锚点
//! 缩放数学（鼠标点为不动点：content_offset' = mouse - (mouse-offset)*k'/k）；
//! 边界停住 + 微弹提示账；档位记忆（F219 联动）。
//!
//! v5 纵深：触屏双指捏合（同一锚点数学、同一边界语义）；双击 100%
//! 复位（锚点保持——内容不跳）；人话百分比读数；按宽适配；快照
//! round-trip（记忆联动的另一半）。

use crate::checks::CheckSet;

/// 图标视图五档。
pub const ICON_MODES: [&str; 5] = ["超大图标", "大图标", "中图标", "小图标", "列表"];

/// 图片缩放倍率边界（0.1x-8x）。
pub const ZOOM_MIN_PERMILLE: u64 = 100;
pub const ZOOM_MAX_PERMILLE: u64 = 8_000;
/// 每格滚轮步进（25%）。
pub const ZOOM_STEP_PERMILLE: u64 = 250;
/// 复位倍率（100%）。
pub const ZOOM_RESET_PERMILLE: u64 = 1_000;

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
    /// 捏合步数账（触屏双指）。
    pub pinch_steps: u64,
}

impl AnchorZoom {
    pub fn new() -> AnchorZoom {
        AnchorZoom { permille: 1_000, offset: (0, 0), bounce_hints: 0, pinch_steps: 0 }
    }

    /// 通用缩放到目标倍率（锚点数学唯一实现——滚轮/捏合/复位/适配共用）。
    /// 边界：越界钳到边界值（末段半步贴边）；已贴边 → 停住。
    fn zoom_to(&mut self, target: i64, mouse: (i64, i64)) -> bool {
        let old = self.permille as i64;
        let (min, max) = (ZOOM_MIN_PERMILLE as i64, ZOOM_MAX_PERMILLE as i64);
        let new = target.clamp(min, max);
        if new == old {
            return false;
        }
        self.permille = new as u64;
        self.offset = (
            mouse.0 - (mouse.0 - self.offset.0) * new / old,
            mouse.1 - (mouse.1 - self.offset.1) * new / old,
        );
        true
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

    /// 触屏双指捏合：ratio_permille = 新/旧（如 2_000 = 放大一倍）。
    /// 同一锚点数学、同一边界语义（滚轮能到的捏合也能到，反之亦然）。
    pub fn pinch(&mut self, ratio_permille: u64, mouse: (i64, i64)) -> bool {
        let old = self.permille as i64;
        let target = old * ratio_permille as i64 / 1_000;
        if target < ZOOM_MIN_PERMILLE as i64 || target > ZOOM_MAX_PERMILLE as i64 {
            self.bounce_hints += 1;
        }
        let moved = self.zoom_to(target, mouse);
        if moved {
            self.pinch_steps += 1;
        }
        moved
    }

    /// 双击复位：100% 且鼠标下内容不动（锚点保持——画面不跳）。
    pub fn reset_at(&mut self, mouse: (i64, i64)) -> bool {
        self.zoom_to(ZOOM_RESET_PERMILLE as i64, mouse)
    }

    /// 按宽适配：viewport_w / content_w → 目标倍率（钳制入界）。
    pub fn fit_width(&mut self, viewport_w: i64, content_w: i64, mouse: (i64, i64)) -> u64 {
        if content_w <= 0 {
            return self.permille; // 非法输入不动现状（诚实）
        }
        let target = viewport_w * 1_000 / content_w;
        let _ = self.zoom_to(target, mouse);
        self.permille
    }

    /// 锚点准确性：缩放前后鼠标点下的内容坐标不变。
    /// 内容坐标 = (mouse - offset) / scale = (mouse - offset) * 1000 / permille。
    pub fn content_under_mouse(&self, mouse: (i64, i64)) -> (i64, i64) {
        (
            (mouse.0 - self.offset.0) * 1_000 / self.permille as i64,
            (mouse.1 - self.offset.1) * 1_000 / self.permille as i64,
        )
    }

    /// 人话读数：千分比 → 百分比整数。
    pub fn percent_label(&self) -> u64 {
        self.permille / 10
    }

    /// 记忆联动快照（F219）。
    pub fn snapshot(&self) -> (u64, (i64, i64)) {
        (self.permille, self.offset)
    }

    /// 快照恢复（round-trip 的另一半——只收自己产出的快照）。
    pub fn apply_snapshot(&mut self, permille: u64, offset: (i64, i64)) {
        self.permille = permille.clamp(ZOOM_MIN_PERMILLE, ZOOM_MAX_PERMILLE);
        self.offset = offset;
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
    // v5：触屏捏合——同一锚点数学（不动点判据与滚轮一致）。
    let mut pz = AnchorZoom::new();
    pz.offset = (0, 0);
    let pb = pz.content_under_mouse((400, 300));
    set.add(
        "f430-pinch-anchor",
        pz.pinch(2_000, (400, 300)) && pz.permille == 2_000 && pz.pinch_steps == 1 && pz.content_under_mouse((400, 300)) == pb,
        "",
    );
    // v5：双击复位——100% 且锚点保持。
    set.add(
        "f430-reset-anchor",
        pz.reset_at((400, 300)) && pz.permille == ZOOM_RESET_PERMILLE && pz.content_under_mouse((400, 300)) == pb,
        "",
    );
    // v5：人话读数（千分比 → 百分比）。
    set.add(
        "f430-percent-label",
        pz.pinch(1_250, (0, 0)) && pz.percent_label() == 125,
        "",
    );
    // v5：按宽适配——500px 视口看 2000px 宽内容 → 25%。
    let mut f = AnchorZoom::new();
    set.add("f430-fit-width", f.fit_width(500, 2_000, (0, 0)) == 250, "");
    // v5：快照 round-trip（记忆联动另一半）。
    let snap = f.snapshot();
    let mut g = AnchorZoom::new();
    g.apply_snapshot(snap.0, snap.1);
    set.add("f430-snapshot-roundtrip", g.snapshot() == snap && g.permille == 250, "");
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

    #[test]
    fn pinch_at_boundary_bounces() {
        let mut z = AnchorZoom::new();
        z.permille = ZOOM_MAX_PERMILLE;
        assert!(!z.pinch(2_000, (0, 0)), "已在 8x 再捏合放大 → 停住");
        assert_eq!(z.bounce_hints, 1, "微弹记账");
        assert_eq!(z.pinch_steps, 0);
    }

    #[test]
    fn fit_width_invalid_content_holds() {
        let mut z = AnchorZoom::new();
        assert_eq!(z.fit_width(500, 0, (0, 0)), 1_000, "内容宽 0 → 不动现状");
    }

    #[test]
    fn snapshot_clamped_on_apply() {
        let mut z = AnchorZoom::new();
        z.apply_snapshot(99_999, (0, 0)); // 越界快照钳入界
        assert_eq!(z.permille, ZOOM_MAX_PERMILLE);
    }
}
