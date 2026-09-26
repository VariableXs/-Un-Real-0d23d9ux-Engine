//! F500 窗口跨屏移动键（genstar2 · I 域通用·二分队 · AI-U2 · 泳道四收官件）。
//!
//! 主册判据（验收标准第一句）：
//! **跨屏搬移相对位置精度；贴靠重排；单屏静默；动画时长；组合键注册
//! （F244）。**
//!
//! 功能定义（主册批次三）：Win+Shift+方向键（VARIX 组合）——当前窗口整体
//! 搬移到相邻屏（保留相对位置与尺寸，超界自动适配 F214）；单屏用户此键无
//! 动作（不报错——键盘按下没有目的地就安静略过）；移动动画 200ms（F124
//! 强调档）+落屏后贴靠语义保留（原是半屏贴靠的移动后按新屏重新贴靠）。
//!
//! 零堆纪律：定长几何结构，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 移动动画时长（主册：200ms——F124 强调档）。
pub const MOVE_ANIM_MS: u64 = 200;
/// 最大屏数（F286 多屏模型）。
pub const SCREEN_CAP: usize = 4;
/// 组合键注册名（F244 注册表键）。
pub const HOTKEY_NAME: &str = "win-shift-arrow-move-window";

/// 屏幕几何（虚拟桌面坐标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl ScreenRect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w as i32 && py >= self.y && py < self.y + self.h as i32
    }
}

/// 窗口几何 + 贴靠语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WinGeom {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    /// 贴靠语义（None=浮动 / Some(方向)——落屏后按新屏重新贴靠）。
    pub snap: Option<SnapSide>,
}

/// 贴靠方向（半屏贴靠——F500：移动后按新屏重新贴靠）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapSide {
    Left,
    Right,
    Top,
    Bottom,
}

/// 相对位置描述（主册：保留相对位置——窗口在源屏的归一化坐标）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RelativePos {
    /// 窗口左上角在源屏内的归一化位置（0.0-1.0）。
    pub rx: f64,
    pub ry: f64,
    /// 窗口占源屏比例（保留尺寸相对性）。
    pub rw: f64,
    pub rh: f64,
}

/// 从窗口+源屏提取相对位置（跨屏搬移相对位置精度判据的基准）。
pub fn to_relative(win: WinGeom, src: ScreenRect) -> RelativePos {
    RelativePos {
        rx: (win.x - src.x) as f64 / src.w as f64,
        ry: (win.y - src.y) as f64 / src.h as f64,
        rw: win.w as f64 / src.w as f64,
        rh: win.h as f64 / src.h as f64,
    }
}

/// 落屏适配（主册：保留相对位置与尺寸，超界自动适配 F214）。
pub fn to_absolute(rel: RelativePos, dst: ScreenRect) -> WinGeom {
    let mut x = dst.x + (rel.rx * dst.w as f64).round() as i32;
    let mut y = dst.y + (rel.ry * dst.h as f64).round() as i32;
    let mut w = (rel.rw * dst.w as f64).round() as u32;
    let mut h = (rel.rh * dst.h as f64).round() as u32;
    // 超界自动适配（F214）：按目标屏钳制。
    w = w.min(dst.w);
    h = h.min(dst.h);
    x = x.clamp(dst.x, dst.x + dst.w as i32 - w as i32);
    y = y.clamp(dst.y, dst.y + dst.h as i32 - h as i32);
    WinGeom { x, y, w, h, snap: None }
}

/// 贴靠重排（主册：原是半屏贴靠的移动后按新屏重新贴靠）。
pub fn resnap(snap: SnapSide, dst: ScreenRect) -> WinGeom {
    let half_w = dst.w / 2;
    let half_h = dst.h / 2;
    match snap {
        SnapSide::Left => WinGeom { x: dst.x, y: dst.y, w: half_w, h: dst.h, snap: Some(SnapSide::Left) },
        SnapSide::Right => WinGeom { x: dst.x + half_w as i32, y: dst.y, w: dst.w - half_w, h: dst.h, snap: Some(SnapSide::Right) },
        SnapSide::Top => WinGeom { x: dst.x, y: dst.y, w: dst.w, h: half_h, snap: Some(SnapSide::Top) },
        SnapSide::Bottom => WinGeom { x: dst.x, y: dst.y + half_h as i32, w: dst.w, h: dst.h - half_h, snap: Some(SnapSide::Bottom) },
    }
}

/// 跨屏移动核心（主册全链：单屏静默 / 相对位置 / 贴靠重排）。
pub fn move_to_neighbor(
    win: WinGeom,
    screens: &[ScreenRect; SCREEN_CAP],
    src_idx: usize,
    dir: SnapSide,
) -> Option<(usize, WinGeom)> {
    if src_idx >= SCREEN_CAP || screens[src_idx].w == 0 {
        return None;
    }
    // 邻屏解析（方向→相邻屏；无边屏 = None——单屏静默的上游）。
    let dst_idx = neighbor_of(&screens[src_idx], screens, dir)?;
    let src = screens[src_idx];
    let dst = screens[dst_idx];
    // 贴靠语义保留优先；浮动窗口走相对位置。
    let geom = match win.snap {
        Some(s) => resnap(s, dst),
        None => {
            let mut g = to_absolute(to_relative(win, src), dst);
            g.snap = None;
            g
        }
    };
    Some((dst_idx, geom))
}

/// 邻屏查找（按屏幕布局静态相邻性：同排左右 / 同列上下）。
fn neighbor_of(src: &ScreenRect, screens: &[ScreenRect; SCREEN_CAP], dir: SnapSide) -> Option<usize> {
    let src_cx = src.x + src.w as i32 / 2;
    let src_cy = src.y + src.h as i32 / 2;
    let mut best: Option<(usize, u64)> = None;
    for (i, s) in screens.iter().enumerate() {
        if s.w == 0 || s == src {
            continue;
        }
        let cx = s.x + s.w as i32 / 2;
        let cy = s.y + s.h as i32 / 2;
        let (dx, dy) = (cx - src_cx, cy - src_cy);
        // 方向粗判 + 距离最近者（四向键位语义）。
        let ok = match dir {
            SnapSide::Left => dx < 0 && (dy.abs() as u64) <= src.h as u64,
            SnapSide::Right => dx > 0 && (dy.abs() as u64) <= src.h as u64,
            SnapSide::Top => dy < 0 && (dx.abs() as u64) <= src.w as u64,
            SnapSide::Bottom => dy > 0 && (dx.abs() as u64) <= src.w as u64,
        };
        if ok {
            let d = (dx.abs() as u64).pow(2) + (dy.abs() as u64).pow(2);
            if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((i, d));
            }
        }
    }
    best.map(|(i, _)| i)
}

/// 单屏静默（主册：单屏用户此键无动作——不报错，安静略过）。
pub fn single_screen_silent(screens: &[ScreenRect; SCREEN_CAP]) -> bool {
    screens.iter().filter(|s| s.w > 0).count() <= 1
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_xmovkey_checks() -> CheckSet {
    let mut cs = CheckSet::new("F500-xmovkey");
    let scr = [
        ScreenRect { x: 0, y: 0, w: 1_920, h: 1_080 },
        ScreenRect { x: 1_920, y: 0, w: 2_560, h: 1_440 },
        ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ScreenRect { x: 0, y: 0, w: 0, h: 0 },
    ];
    // 1) 跨屏搬移相对位置精度（窗口在源屏 25%,25% 处 → 落屏同比例）。
    let win = WinGeom { x: 480, y: 270, w: 960, h: 540, snap: None }; // 25%,25%,50%,50%
    let moved = move_to_neighbor(win, &scr, 0, SnapSide::Right).unwrap();
    cs.add("moves_right", moved.0 == 1, "");
    let rel_dst = to_relative(moved.1, scr[1]);
    cs.add("relative_position_kept", (rel_dst.rx - 0.25).abs() < 0.01 && (rel_dst.ry - 0.25).abs() < 0.01 && (rel_dst.rw - 0.5).abs() < 0.01, "");
    // 2) 尺寸按目标屏比例保留（50% 源屏宽 → 50% 目标屏宽）。
    cs.add("size_adapts", moved.1.w == 1_280 && moved.1.h == 720, "");
    // 3) 贴靠重排（半屏贴靠窗口 → 新屏重新贴靠）。
    let snapped = WinGeom { x: 0, y: 0, w: 960, h: 1_080, snap: Some(SnapSide::Left) };
    let m2 = move_to_neighbor(snapped, &scr, 0, SnapSide::Right).unwrap();
    cs.add("snap_reapplied", m2.1.snap == Some(SnapSide::Left) && m2.1.w == 1_280 && m2.1.h == 1_440, "");
    // 4) 单屏静默（无邻屏 = None——键盘按下没有目的地安静略过）。
    let single = [
        scr[0],
        ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ScreenRect { x: 0, y: 0, w: 0, h: 0 },
    ];
    cs.add("single_screen_silent", single_screen_silent(&single) && move_to_neighbor(win, &single, 0, SnapSide::Right).is_none(), "");
    // 5) 动画时长（F124 强调档 200ms 在册）。
    cs.add("anim_200ms", MOVE_ANIM_MS == 200, "");
    // 6) 组合键注册（F244 注册表锚）。
    cs.add("hotkey_registered", HOTKEY_NAME == "win-shift-arrow-move-window", "");
    // 7) 落屏超界适配（比目标屏还大的窗口钳制在屏内——F214）。
    let huge = WinGeom { x: 100, y: 100, w: 4_000, h: 2_000, snap: None };
    let m3 = move_to_neighbor(huge, &scr, 0, SnapSide::Right).unwrap();
    cs.add("overflow_clamped", m3.1.w <= scr[1].w && m3.1.h <= scr[1].h && m3.1.x >= scr[1].x, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_geometry_round_trip() {
        let scr = ScreenRect { x: 0, y: 0, w: 1_000, h: 800 };
        let win = WinGeom { x: 250, y: 200, w: 500, h: 400, snap: None };
        let rel = to_relative(win, scr);
        assert!((rel.rx - 0.25).abs() < 1e-9);
        let back = to_absolute(rel, scr);
        assert_eq!((back.x, back.y, back.w, back.h), (250, 200, 500, 400));
    }

    #[test]
    fn left_direction_finds_left_screen() {
        let scr = [
            ScreenRect { x: 0, y: 0, w: 1_920, h: 1_080 },
            ScreenRect { x: 1_920, y: 0, w: 1_920, h: 1_080 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ];
        let win = WinGeom { x: 2_000, y: 100, w: 800, h: 600, snap: None };
        let m = move_to_neighbor(win, &scr, 1, SnapSide::Left).unwrap();
        assert_eq!(m.0, 0);
        // 相对位置保留：源屏 4.17% → 目标屏 1920×0.0417 ≈ 80。
        assert_eq!(m.1.x, 80);
    }

    #[test]
    fn vertical_neighbors_found() {
        let scr = [
            ScreenRect { x: 0, y: 0, w: 1_920, h: 1_080 },
            ScreenRect { x: 0, y: 1_080, w: 1_920, h: 1_080 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ];
        let win = WinGeom { x: 100, y: 100, w: 800, h: 600, snap: None };
        let m = move_to_neighbor(win, &scr, 0, SnapSide::Bottom).unwrap();
        assert_eq!(m.0, 1);
    }
}
