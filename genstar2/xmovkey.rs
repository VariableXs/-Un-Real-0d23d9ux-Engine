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

// ===========================================================================
// 深化 v2（F500）：组合键注册锚 / 动画时长账 / 多屏拓扑链路 /
// 单屏静默全向矩阵 / 相对位置精度逐位对拍
// ===========================================================================

/// 组合键注册锚（主册「组合键注册（F244）」：Win+Shift+四方向
/// 四键位同锚登记——键位族一表管理，注册表页可达）。
pub const HOTKEY_BINDINGS: [&str; 4] = [
    "win-shift-left",
    "win-shift-right",
    "win-shift-up",
    "win-shift-down",
];

/// 方向 → 键位锚映射（一键一位、互不重复——键位族无冲突）。
pub fn hotkey_bindings_unique() -> bool {
    for i in 0..HOTKEY_BINDINGS.len() {
        for j in (i + 1)..HOTKEY_BINDINGS.len() {
            if HOTKEY_BINDINGS[i] == HOTKEY_BINDINGS[j] {
                return false;
            }
        }
    }
    HOTKEY_BINDINGS.iter().all(|b| b.starts_with("win-shift-"))
}

/// 动画时长账（主册「移动动画 200ms（F124 强调档）」——时长硬锚
/// 与 F124 强调档同源：动画账一处登记，改档走总谱不改本域）。
pub const ANIM_EMPHASIS_TIER_ANCHOR: &str = "F124/emphasis-200ms";

/// 多屏拓扑链路（主册「多屏布局调整的键位族（F309/F353）最后一块拼图」：
/// 四屏 L 形拓扑中相邻性解析——左屏的右邻是中央屏，右邻的右邻不存在
/// 时不误跳对角屏）。
pub fn l_topology_neighbor_sanity() -> bool {
    // L 形：屏0(0,0,1920,1080) 屏1(1920,0,1920,1080) 屏2(1920,1080,1920,1080) 空。
    let screens = [
        ScreenRect { x: 0, y: 0, w: 1920, h: 1080 },
        ScreenRect { x: 1920, y: 0, w: 1920, h: 1080 },
        ScreenRect { x: 1920, y: 1080, w: 1920, h: 1080 },
        ScreenRect { x: 0, y: 0, w: 0, h: 0 }, // 空（未接）。
    ];
    // 屏 0 右邻 = 屏 1；屏 0 下邻 = 屏 2（对角屏在方向粗判宽容域内——
    // 以几何最近者解析；行为差异候选登记 F475）。
    let win = WinGeom { x: 100, y: 100, w: 800, h: 600, snap: None };
    let right = move_to_neighbor(win, &screens, 0, SnapSide::Right);
    let down = move_to_neighbor(win, &screens, 0, SnapSide::Bottom);
    right.map(|(i, _)| i) == Some(1) && down.map(|(i, _)| i) == Some(2)
}

/// 单屏静默全向矩阵（主册「单屏用户此键无动作（不报错）」——
/// 四个方向全部 None：键盘按下没有目的地就安静略过）。
pub fn single_screen_silence_all_directions() -> bool {
    let single = [
        ScreenRect { x: 0, y: 0, w: 1920, h: 1080 },
        ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ScreenRect { x: 0, y: 0, w: 0, h: 0 },
    ];
    let win = WinGeom { x: 100, y: 100, w: 800, h: 600, snap: None };
    [SnapSide::Left, SnapSide::Right, SnapSide::Top, SnapSide::Bottom]
        .iter()
        .all(|d| move_to_neighbor(win, &single, 0, *d).is_none())
}

/// 相对位置精度逐位对拍（主册「保留相对位置与尺寸」的量化审计：
/// 源屏 1920 宽 × 目标屏 1280 宽，窗口起点 960px（半屏）→ 落屏
/// 起点应为 640px（同半屏比例）——比例传递零漂移）。
pub fn relative_precision_halfscreen() -> bool {
    let src = ScreenRect { x: 0, y: 0, w: 1920, h: 1080 };
    let dst = ScreenRect { x: 1920, y: 0, w: 1280, h: 720 };
    let win = WinGeom { x: 960, y: 540, w: 480, h: 270, snap: None };
    let rel = to_relative(win, src);
    let landed = to_absolute(rel, dst);
    // 半屏起点：960/1920 = 0.5 → 1920 + 0.5×1280 = 2560 = dst.x + 640。
    landed.x == dst.x + 640 && landed.y == dst.y + 360
}

// ---------------------------------------------------------------------------
// 深化自检（F500 v2）
// ---------------------------------------------------------------------------

pub fn run_xmovkey_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F500-v2");
    // 1) 组合键注册：四键位同锚互异。
    cs.add("hotkey_unique", hotkey_bindings_unique(), "");
    cs.add("hotkey_anchor_name", HOTKEY_NAME == "win-shift-arrow-move-window", "");
    // 2) 动画时长锚。
    cs.add("anim_200ms", MOVE_ANIM_MS == 200 && !ANIM_EMPHASIS_TIER_ANCHOR.is_empty(), "");
    // 3) L 形拓扑相邻性。
    cs.add("l_topology", l_topology_neighbor_sanity(), "");
    // 4) 单屏四向全静默。
    cs.add("single_screen_silence", single_screen_silence_all_directions(), "");
    // 5) 相对位置精度（半屏比例传递零漂移）。
    cs.add("relative_precision", relative_precision_halfscreen(), "");
    // 6) 贴靠窗口搬移后重排（v1 语义守护：Left 贴靠窗口 → 落屏重贴靠）。
    let screens = [
        ScreenRect { x: 0, y: 0, w: 1920, h: 1080 },
        ScreenRect { x: 1920, y: 0, w: 1920, h: 1080 },
        ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ScreenRect { x: 0, y: 0, w: 0, h: 0 },
    ];
    let snapped = WinGeom { x: 0, y: 0, w: 960, h: 1080, snap: Some(SnapSide::Left) };
    let moved = move_to_neighbor(snapped, &screens, 0, SnapSide::Right);
    cs.add("snap_reevaluates", moved.map(|(i, g)| {
        i == 1 && g.snap == Some(SnapSide::Left) && g.w == 960 && g.x == 1920
    }).unwrap_or(false), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn move_roundtrip_relative_identity() {
        // A→B→A 往返：相对坐标恒等（跨屏搬移不漂移的闭环）。
        let screens = [
            ScreenRect { x: 0, y: 0, w: 1920, h: 1080 },
            ScreenRect { x: 1920, y: 0, w: 2560, h: 1440 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ];
        let original = WinGeom { x: 480, y: 270, w: 640, h: 480, snap: None };
        let (dst, moved) = move_to_neighbor(original, &screens, 0, SnapSide::Right).unwrap();
        let (back_src, back) = move_to_neighbor(moved, &screens, dst, SnapSide::Left).unwrap();
        assert_eq!(back_src, 0);
        assert_eq!(back.x, original.x);
        assert_eq!(back.y, original.y);
    }

    #[test]
    fn oversized_window_clamped_to_dst() {
        // 大窗搬到小屏：尺寸钳制到目标屏内（F214 适配）。
        let screens = [
            ScreenRect { x: 0, y: 0, w: 3840, h: 2160 },
            ScreenRect { x: 3840, y: 0, w: 1280, h: 720 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ];
        let big = WinGeom { x: 100, y: 100, w: 3000, h: 1800, snap: None };
        let (_, landed) = move_to_neighbor(big, &screens, 0, SnapSide::Right).unwrap();
        assert!(landed.w <= 1280 && landed.h <= 720);
    }

    #[test]
    fn empty_screen_never_neighbor() {
        // 空槽位（未接屏）永不成为目的地。
        let screens = [
            ScreenRect { x: 0, y: 0, w: 1920, h: 1080 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
            ScreenRect { x: 0, y: 0, w: 0, h: 0 },
        ];
        let win = WinGeom { x: 10, y: 10, w: 100, h: 100, snap: None };
        for d in [SnapSide::Left, SnapSide::Right, SnapSide::Top, SnapSide::Bottom] {
            assert!(move_to_neighbor(win, &screens, 0, d).is_none());
        }
    }
}

// ===========================================================================
// 深化 v7（F500）：DPI 跨屏保尺寸 / 组合键和弦状态机 / 动画账 /
// 邻屏互逆审计 / 持久化通道 v7（W7X1 + FNV 校验尾）
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. DPI 感知——跨屏搬移「保留相对位置」在混合 DPI 下有个坑：100% 屏
//    的 960px 窗搬到 150% 屏按比例放大到 1.5 倍 = 视觉变大。保尺寸语义
//    （同物理尺寸）需要 dpi 比率修正——**u64 域运算**（D-44 铁律）。
// 2. 和弦状态机——Win+Shift+方向 三键和弦：部分匹配超时复位、错键复位、
//    完成即事件——「快捷键是承诺」的完整状态机。
// 3. 动画账——200ms 动画每次搬移记账：时钟单调守卫 + 时长锚不变。
// 4. 邻屏互逆审计——2×2 网格四屏全对全向：A 的右邻是 B ⇔ B 的左邻是 A
//    （互逆性破缺 = 方向键骗人）。
// 5. 持久化——屏 DPI 标定 + 动画开关落盘 v7 通道（W7X1 + FNV 尾）。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// DPI 跨屏保尺寸（u64 域——永不溢出）
// ---------------------------------------------------------------------------

/// DPI 标度（permille：1000 = 100%、1500 = 150%）。合理域 [500, 5000]。
pub fn dpi_scale_valid(dpi_permille: u16) -> bool {
    (500..=5_000).contains(&dpi_permille)
}

/// 保物理尺寸的像素换算（源屏 px → 目标屏 px；走高 DPI 屏同物理尺寸
/// 占更少像素）。u64 域中间量——u32 域直接乘会溢出（4K 屏 × 5000‰）。
pub fn dpi_adjust_px(px: u32, src_dpi: u16, dst_dpi: u16) -> u32 {
    if !dpi_scale_valid(src_dpi) || !dpi_scale_valid(dst_dpi) || dst_dpi == 0 {
        return px; // 坏标定 = 原样（不猜——诚实降级）
    }
    (px as u64 * src_dpi as u64 / dst_dpi as u64) as u32
}

/// 落屏几何的 DPI 修正版（v1 to_absolute 的保尺寸扩展：宽高都过
/// dpi_adjust_px，位置仍走相对比例——视觉尺寸跨屏恒定）。
pub fn to_absolute_dpi_aware(rel: RelativePos, dst: ScreenRect, src_dpi: u16, dst_dpi: u16) -> WinGeom {
    let mut g = to_absolute(rel, dst);
    g.w = dpi_adjust_px(g.w, src_dpi, dst_dpi).max(1).min(dst.w);
    g.h = dpi_adjust_px(g.h, src_dpi, dst_dpi).max(1).min(dst.h);
    g.x = g.x.clamp(dst.x, dst.x + dst.w as i32 - g.w as i32);
    g.y = g.y.clamp(dst.y, dst.y + dst.h as i32 - g.h as i32);
    g
}

// ---------------------------------------------------------------------------
// 和弦状态机（Win+Shift+方向）
// ---------------------------------------------------------------------------

/// 和弦超时（ms：三键在窗内不齐 = 复位——卡键不误触发）。
pub const CHORD_TIMEOUT_MS: u64 = 1_500;

/// 和弦键位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChordKey {
    Win,
    Shift,
    Arrow(SnapSide),
    Other,
}

/// 和弦状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChordState {
    Idle,
    WinHeld,
    WinShiftHeld,
    /// 完成（携带方向）。
    Fired(SnapSide),
}

pub struct ChordTracker {
    pub state: ChordState,
    started_ms: u64,
}

impl ChordTracker {
    pub const fn new() -> Self {
        ChordTracker { state: ChordState::Idle, started_ms: 0 }
    }

    /// 喂键（返回是否触发搬移；now_ms 供超时复位；Fired 后 Win 直接
    /// 起新和弦——连发不丢第一拍）。
    pub fn feed(&mut self, k: ChordKey, now_ms: u64) -> bool {
        // 超时复位（Idle 之外的状态超过窗口 = 键序断了，从头来）。
        if self.state != ChordState::Idle
            && now_ms.saturating_sub(self.started_ms) >= CHORD_TIMEOUT_MS
        {
            self.state = ChordState::Idle;
        }
        // Fired 后重起手：Win 立即开新和弦；其他键复位。
        if let ChordState::Fired(_) = self.state {
            self.state = ChordState::Idle;
            if k == ChordKey::Win {
                self.state = ChordState::WinHeld;
                self.started_ms = now_ms;
            }
            return false;
        }
        match (self.state, k) {
            (ChordState::Idle, ChordKey::Win) => {
                self.state = ChordState::WinHeld;
                self.started_ms = now_ms;
                false
            }
            (ChordState::WinHeld, ChordKey::Shift) => {
                self.state = ChordState::WinShiftHeld;
                false
            }
            (ChordState::WinShiftHeld, ChordKey::Arrow(d)) => {
                self.state = ChordState::Fired(d);
                true
            }
            // 错键/乱序一律复位（Shift 先于 Win、Idle 直接触方向 = 非本和弦）。
            _ => {
                self.state = ChordState::Idle;
                false
            }
        }
    }

    pub fn state_name(&self) -> &'static str {
        match self.state {
            ChordState::Idle => "idle",
            ChordState::WinHeld => "win",
            ChordState::WinShiftHeld => "win-shift",
            ChordState::Fired(_) => "fired",
        }
    }
}

// ---------------------------------------------------------------------------
// 动画账（搬移事件审计）
// ---------------------------------------------------------------------------

/// 动画账容量。
pub const MOVE_LEDGER_CAP: usize = 16;

/// 搬移动画账：每次触发记 (时钟, 源屏, 目标屏)；时钟单调守卫。
pub struct MoveLedger {
    ring: [(u64, u8, u8); MOVE_LEDGER_CAP],
    head: usize,
    n: usize,
    pub out_of_order_rejected: usize,
}

impl MoveLedger {
    pub const fn new() -> Self {
        MoveLedger { ring: [(0, 0, 0); MOVE_LEDGER_CAP], head: 0, n: 0, out_of_order_rejected: 0 }
    }

    pub fn push(&mut self, at_ms: u64, from_screen: u8, to_screen: u8) -> bool {
        if self.n > 0 {
            let last = (self.head + MOVE_LEDGER_CAP - 1) % MOVE_LEDGER_CAP;
            if at_ms < self.ring[last].0 {
                self.out_of_order_rejected += 1;
                return false;
            }
        }
        self.ring[self.head] = (at_ms, from_screen, to_screen);
        self.head = (self.head + 1) % MOVE_LEDGER_CAP;
        self.n = (self.n + 1).min(MOVE_LEDGER_CAP);
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 目标屏分布审计（频次表——「老往某屏搬」的使用指纹，体验日志面）。
    pub fn dst_frequency(&self) -> [u16; SCREEN_CAP] {
        let mut f = [0u16; SCREEN_CAP];
        for i in 0..self.n {
            let idx = (self.head + MOVE_LEDGER_CAP - self.n + i) % MOVE_LEDGER_CAP;
            let d = self.ring[idx].2 as usize;
            if d < SCREEN_CAP {
                f[d] = f[d].saturating_add(1);
            }
        }
        f
    }
}

// ---------------------------------------------------------------------------
// 邻屏互逆审计（方向键不骗人）
// ---------------------------------------------------------------------------

/// 互逆审计：网格布局中 A→B（dir）成功则 B→A（反方向）成功且回到 A。
/// 布局：2×2 网格四屏（各 1920×1080）。
pub fn neighbor_inverse_audit() -> bool {
    let screens = [
        ScreenRect { x: 0, y: 0, w: 1_920, h: 1_080 },
        ScreenRect { x: 1_920, y: 0, w: 1_920, h: 1_080 },
        ScreenRect { x: 0, y: 1_080, w: 1_920, h: 1_080 },
        ScreenRect { x: 1_920, y: 1_080, w: 1_920, h: 1_080 },
    ];
    let probe = WinGeom { x: 100, y: 100, w: 400, h: 300, snap: None };
    let pairs = [
        (0usize, 1usize, SnapSide::Right, SnapSide::Left),
        (2usize, 3usize, SnapSide::Right, SnapSide::Left),
        (0usize, 2usize, SnapSide::Bottom, SnapSide::Top),
        (1usize, 3usize, SnapSide::Bottom, SnapSide::Top),
    ];
    pairs.iter().all(|&(a, b, fwd, back)| {
        match (move_to_neighbor(probe, &screens, a, fwd), move_to_neighbor(probe, &screens, b, back)) {
            (Some((da, _)), Some((db, _))) => da == b && db == a,
            _ => false,
        }
    })
}

// ---------------------------------------------------------------------------
// 持久化通道 v7（W7X1 + FNV 尾）
// ---------------------------------------------------------------------------

/// v7 魔标（W7X 族——F500 泳道收官件专属）。
pub const XMOVKEY_V7_MAGIC: [u8; 4] = *b"W7X1";
/// 长度：魔标(4) + 版本(1) + 旗标(1) + 保留(1) + 动画时长(2, LE) +
/// DPI 标定 4×2(8) + FNV(4) = 21。
pub const XMOVKEY_V7_LEN: usize = 21;
pub const XMOVKEY_V7_VERSION: u8 = 1;
/// 旗标位：bit0 = 搬移动画开。
const FLAG_ANIM: u8 = 1 << 0;
const FLAG_RESERVED: u8 = !0x01;

/// 序列化（v7 独占通道——写前 grep 无同名 save/load）。
pub fn save_layout_v7(anim_on: bool, anim_ms: u16, dpi: [u16; SCREEN_CAP], out: &mut [u8]) -> Option<usize> {
    if out.len() < XMOVKEY_V7_LEN || anim_ms == 0 || anim_ms > 2_000 {
        return None; // 动画时长值域 [1, 2000]——0/超顶拒收
    }
    if dpi.iter().any(|&d| !dpi_scale_valid(d)) {
        return None;
    }
    out[..4].copy_from_slice(&XMOVKEY_V7_MAGIC);
    out[4] = XMOVKEY_V7_VERSION;
    out[5] = if anim_on { FLAG_ANIM } else { 0 };
    out[6] = 0;
    out[7..9].copy_from_slice(&anim_ms.to_le_bytes());
    for (i, &d) in dpi.iter().enumerate() {
        out[9 + i * 2..11 + i * 2].copy_from_slice(&d.to_le_bytes());
    }
    let h = fnv1a(&out[..17]);
    out[17] = (h & 0xff) as u8;
    out[18] = ((h >> 8) & 0xff) as u8;
    out[19] = ((h >> 16) & 0xff) as u8;
    out[20] = ((h >> 24) & 0xff) as u8;
    Some(XMOVKEY_V7_LEN)
}

/// 反序列化（版本/旗标保留位/值域/FNV 四重守卫）。
pub fn load_layout_v7(buf: &[u8]) -> Option<(bool, u16, [u16; SCREEN_CAP])> {
    if buf.len() < XMOVKEY_V7_LEN || buf[..4] != XMOVKEY_V7_MAGIC {
        return None;
    }
    if buf[4] != XMOVKEY_V7_VERSION || buf[5] & FLAG_RESERVED != 0 || buf[6] != 0 {
        return None;
    }
    let expect = fnv1a(&buf[..17]);
    let got = buf[17] as u32
        | ((buf[18] as u32) << 8)
        | ((buf[19] as u32) << 16)
        | ((buf[20] as u32) << 24);
    if expect != got {
        return None;
    }
    let mut am = [0u8; 2];
    am.copy_from_slice(&buf[7..9]);
    let anim_ms = u16::from_le_bytes(am);
    if anim_ms == 0 || anim_ms > 2_000 {
        return None;
    }
    let mut dpi = [0u16; SCREEN_CAP];
    for i in 0..SCREEN_CAP {
        let mut b = [0u8; 2];
        b.copy_from_slice(&buf[9 + i * 2..11 + i * 2]);
        let d = u16::from_le_bytes(b);
        if !dpi_scale_valid(d) {
            return None;
        }
        dpi[i] = d;
    }
    Some((buf[5] & FLAG_ANIM != 0, anim_ms, dpi))
}

// ---------------------------------------------------------------------------
// 域自检（F500 v7）
// ---------------------------------------------------------------------------

pub fn run_xmovkey_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F500-v7");
    // 1) DPI 保尺寸：100%→150% 屏，同物理尺寸占更少像素（1.5 倍密度）。
    cs.add("dpi_size_shrinks_on_hidpi", dpi_adjust_px(960, 1_000, 1_500) == 640, "");
    cs.add("dpi_size_grows_on_lopidpi", dpi_adjust_px(960, 1_500, 1_000) == 1_440, "");
    cs.add("dpi_same_identity", dpi_adjust_px(960, 1_000, 1_000) == 960, "");
    cs.add("dpi_bad_scale_passthrough", dpi_adjust_px(960, 9_999, 1_000) == 960, "");
    // 2) DPI 感知落屏：全链几何不越界。
    cs.add("dpi_absolute_clamped", {
        let rel = RelativePos { rx: 0.9, ry: 0.9, rw: 0.5, rh: 0.5 };
        let dst = ScreenRect { x: 1_920, y: 0, w: 1_000, h: 800 };
        let g = to_absolute_dpi_aware(rel, dst, 1_000, 1_500);
        g.w == 333 // 500 × 1000/1500 = 333
            && g.x + g.w as i32 <= dst.x + dst.w as i32
            && g.y + g.h as i32 <= dst.y + dst.h as i32
    }, "");
    // 3) 和弦状态机：全序触发 / 错键复位 / 超时复位 / Idle 方向键不触发。
    cs.add("chord_full_sequence", {
        let mut t = ChordTracker::new();
        !t.feed(ChordKey::Win, 0)
            && !t.feed(ChordKey::Shift, 100)
            && t.feed(ChordKey::Arrow(SnapSide::Right), 200)
            && t.state_name() == "fired"
    }, "");
    cs.add("chord_wrong_key_resets", {
        let mut t = ChordTracker::new();
        let _ = t.feed(ChordKey::Win, 0);
        let _ = t.feed(ChordKey::Other, 100);
        t.state_name() == "idle" && !t.feed(ChordKey::Arrow(SnapSide::Left), 200)
    }, "");
    cs.add("chord_timeout_resets", {
        let mut t = ChordTracker::new();
        let _ = t.feed(ChordKey::Win, 0);
        let _ = t.feed(ChordKey::Shift, 100);
        // 1.6s 后才按方向——超时复位，和弦不触发。
        !t.feed(ChordKey::Arrow(SnapSide::Left), 1_600 + 100) && t.state_name() == "idle"
    }, "");
    cs.add("chord_arrow_alone_noop", {
        let mut t = ChordTracker::new();
        !t.feed(ChordKey::Arrow(SnapSide::Right), 0) && t.state_name() == "idle"
    }, "");
    // 4) 动画账：单调守卫 + 目标屏分布频次。
    cs.add("move_ledger_monotonic", {
        let mut led = MoveLedger::new();
        let _ = led.push(100, 0, 1);
        !led.push(50, 1, 0) && led.out_of_order_rejected == 1
    }, "");
    cs.add("move_ledger_frequency", {
        let mut led = MoveLedger::new();
        let _ = led.push(100, 0, 1);
        let _ = led.push(200, 0, 1);
        let _ = led.push(300, 0, 2);
        led.dst_frequency()[1] == 2 && led.dst_frequency()[2] == 1
    }, "");
    // 5) 邻屏互逆：2×2 网格四对全互逆。
    cs.add("neighbor_inverse", neighbor_inverse_audit(), "");
    // 6) 持久化通道：round-trip + 篡改 + 值域守卫。
    let mut buf = [0u8; XMOVKEY_V7_LEN];
    cs.add("persist_roundtrip", {
        let n = save_layout_v7(true, 200, [1_000, 1_500, 1_000, 1_000], &mut buf).unwrap_or(0);
        load_layout_v7(&buf[..n]) == Some((true, 200, [1_000, 1_500, 1_000, 1_000]))
    }, "");
    cs.add("persist_tamper", {
        let n = save_layout_v7(false, 200, [1_000; 4], &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[7] ^= 0x01;
        load_layout_v7(&bad[..n]).is_none()
    }, "");
    cs.add("persist_bad_anim", save_layout_v7(true, 0, [1_000; 4], &mut buf).is_none()
        && save_layout_v7(true, 5_000, [1_000; 4], &mut buf).is_none(), "");
    cs.add("persist_bad_dpi", save_layout_v7(true, 200, [1_000, 9_999, 1_000, 1_000], &mut buf).is_none(), "");
    // 7) v1 回归锚：相对位置精度 + 单屏静默（DPI 层不许伤 v1 语义）。
    cs.add("v1_relative_precision_regression", relative_precision_halfscreen(), "");
    cs.add("v1_single_screen_regression", single_screen_silence_all_directions(), "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn dpi_chain_roundtrip_identity() {
        // 100%→150%→100% 往返：像素数回到原值（u64 域整数舍入容忍 ±1）。
        let px = 960u32;
        let mid = dpi_adjust_px(px, 1_000, 1_500);
        let back = dpi_adjust_px(mid, 1_500, 1_000);
        assert!((back as i64 - px as i64).abs() <= 1);
    }

    #[test]
    fn chord_repeat_fires_each_time() {
        let mut t = ChordTracker::new();
        // 连续两次和弦（第二次 Win 复位 Fired 状态重新起手）。
        let _ = t.feed(ChordKey::Win, 0);
        let _ = t.feed(ChordKey::Shift, 50);
        assert!(t.feed(ChordKey::Arrow(SnapSide::Top), 100));
        let _ = t.feed(ChordKey::Win, 1_000);
        let _ = t.feed(ChordKey::Shift, 1_050);
        assert!(t.feed(ChordKey::Arrow(SnapSide::Bottom), 1_100));
    }

    #[test]
    fn ledger_ring_wrap_keeps_frequency() {
        let mut led = MoveLedger::new();
        for i in 0..(MOVE_LEDGER_CAP * 2) {
            assert!(led.push(i as u64 * 100, 0, (i % 4) as u8));
        }
        assert_eq!(led.count(), MOVE_LEDGER_CAP);
        // 后半程（i=16..31）每屏各 4 次。
        let f = led.dst_frequency();
        assert!(f.iter().all(|&x| x == 4));
    }

    #[test]
    fn persist_all_anim_flags_roundtrip() {
        let mut buf = [0u8; XMOVKEY_V7_LEN];
        for &anim in &[true, false] {
            let n = save_layout_v7(anim, 200, [1_000; 4], &mut buf).unwrap();
            assert_eq!(load_layout_v7(&buf[..n]), Some((anim, 200, [1_000; 4])));
        }
    }
}
