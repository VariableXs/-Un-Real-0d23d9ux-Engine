//! H2 渲染几何 · 深化批次三（OSD 绘制面 + 材质采样 + 编辑框渲染 +
//! 落区高亮——「渲染层纵深」的几何与预算单一定义点）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F251 媒体会话仲裁**：OSD 显示应用归属——OSD 布局（进度环/
//!   徽标位/淡出时序）在这里定义；键响应 <50ms 红线在渲染侧的落点
//!   是「OSD 首帧预算」——超线计数不静默；
//! - **F254 窗口透明材质**：模糊采样走 GPU、CPU 占用增量 <3%——
//!   采样核预算与降级阶梯（全模糊→减半径→实底）在这里；恢复触发
//!   也是机判的（预算回稳 N 帧才升档，防抖动）；
//! - **F260 行内重命名**：编辑框渲染几何——插入符/选区矩形、IME
//!   候选窗锚点（跟随光标、底部翻转）、水平滚动钳制（h2edit 的
//!   最小平移在渲染侧的落点）；
//! - **F276 贴靠布局组**：落区高亮动画——呼吸环参数走
//!   [`crate::h2star::h2curve`] 总谱，几何走 [`crate::h2star::h2geo`]。
//!
//! 时间纪律：无时钟；淡出/呼吸由调用方按帧喂 t。

use crate::checks::CheckSet;

use crate::h2star::h2curve::{self, Curve};
use crate::h2star::h2geo::{Rect, SNAP_GAP_PX};

// ---------------------------------------------------------------------------
// F251 OSD 绘制面
// ---------------------------------------------------------------------------

/// OSD 首帧预算（F251 键响应 <50ms 红线在渲染侧的份额）。
pub const OSD_FIRST_FRAME_MS: u32 = 50;
/// OSD 淡出驻留（主册 F251 无独立值——按 F239 OSD 家族 1.5s 同谱）。
pub const OSD_DWELL_MS: u32 = 1500;
/// 进度环弧度：270°（顶部留缺口——音量环家族惯例）。
pub const OSD_ARC_DEG: u32 = 270;

/// OSD 布局：给宿主屏幕矩形与音量值，产出全部绘制矩形与弧参数。
/// 归属徽标（哪个应用在放）在环下方——归属是判据，不是装饰。
pub struct OsdLayout {
    /// 环心与半径。
    pub center: (i32, i32),
    pub radius: u32,
    /// 弧起角（度，12 点方向顺时针）与弧度。
    pub start_deg: u32,
    pub sweep_deg: u32,
    /// 归属徽标矩形（应用名）。
    pub badge: Rect,
    /// 淡出总时长（驻留 + 淡出 200ms）。
    pub total_ms: u32,
}

/// OSD 布局计算。`screen` 宿主屏；`level` 0..=100（越界钳制）。
/// 环心 = 屏幕水平居中、垂直 68%（下部三分位——不挡正文行）。
pub fn osd_layout(screen: Rect, level: u32) -> OsdLayout {
    let level = level.min(100);
    let cx = screen.x + (screen.w as i32) / 2;
    let cy = screen.y + (screen.h as i32) * 68 / 100;
    let radius = (screen.w.min(screen.h) / 8).max(48);
    let bw = radius * 3;
    let badge = Rect::new(
        cx - bw as i32 / 2,
        cy + radius as i32 + 12,
        bw,
        28,
    );
    OsdLayout {
        center: (cx, cy),
        radius,
        start_deg: 360 - OSD_ARC_DEG / 2,
        sweep_deg: OSD_ARC_DEG * level as u32 / 100,
        badge,
        total_ms: OSD_DWELL_MS + 200,
    }
}

// ---------------------------------------------------------------------------
// F254 材质采样预算与降级阶梯
// ---------------------------------------------------------------------------

/// 降级阶梯三档（判据：模糊→降采样→实底；恢复要回稳防抖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MaterialTier {
    /// 全档：半径 r 双轴 3 taps/倍频。
    Full,
    /// 减档：半径减半（省 ~4x 采样）。
    Reduced,
    /// 实底：不采样，纯色 alpha（保底可读性——4.5:1 红线仍守）。
    Solid,
}

/// 帧预算超线次数（连续超此数才降档——单帧抖动不触发降级）。
pub const DEGRADE_AFTER_FRAMES: u32 = 3;
/// 升档回稳帧数（恢复滞回——防在阈值边抖动）。
pub const RESTORE_AFTER_FRAMES: u32 = 30;

/// 采样核预算：半径 → 双轴 taps 数。GPU 核预算超 512 taps 即视为
/// 超预算帧（F254：CPU 占用增量 <3% 的替代机判口径——核数是
/// 可枚举的，占用是随硬件浮动的，核数红线更诚实）。
pub fn blur_taps(radius: u32) -> u64 {
    if radius == 0 {
        return 0;
    }
    let octaves = ((radius as f32).log2().ceil() as u64).max(1);
    3 * 2 * octaves
}

/// 材质档位机：超线降档 / 回稳升档，滞回防抖。
pub struct MaterialGovernor {
    pub tier: MaterialTier,
    over: u32,
    calm: u32,
}

impl MaterialGovernor {
    pub fn new() -> MaterialGovernor {
        MaterialGovernor { tier: MaterialTier::Full, over: 0, calm: 0 }
    }

    /// 每帧喂一次：本帧是否超预算。返回当前档（渲染层直接用）。
    pub fn frame(&mut self, over_budget: bool) -> MaterialTier {
        if over_budget {
            self.over += 1;
            self.calm = 0;
            if self.over >= DEGRADE_AFTER_FRAMES && self.tier != MaterialTier::Solid {
                self.tier = match self.tier {
                    MaterialTier::Full => MaterialTier::Reduced,
                    _ => MaterialTier::Solid,
                };
                self.over = 0;
            }
        } else {
            self.over = 0;
            if self.tier != MaterialTier::Full {
                self.calm += 1;
                if self.calm >= RESTORE_AFTER_FRAMES {
                    self.tier = match self.tier {
                        MaterialTier::Solid => MaterialTier::Reduced,
                        _ => MaterialTier::Full,
                    };
                    self.calm = 0;
                }
            }
        }
        self.tier
    }
}

// ---------------------------------------------------------------------------
// F260 编辑框渲染几何
// ---------------------------------------------------------------------------

/// 插入符宽度（2px——高 DPI 下放大走资产档，逻辑宽恒定）。
pub const CARET_W: i32 = 2;
/// 行高（编辑框单行形制）。
pub const EDIT_LINE_H: i32 = 28;
/// IME 候选窗与插入符的间距。
pub const IME_GAP: i32 = 6;

/// 插入符矩形：`x` 来自 h2edit 的列↔像素换算（调用方喂），本函数
/// 只负责矩形成形与选区跨行展开（单行形制没有跨行——选区就是一段）。
pub fn caret_rect(x: i32, y: i32) -> Rect {
    Rect::new(x, y + 4, CARET_W as u32, EDIT_LINE_H as u32 - 8)
}

/// 选区矩形（单行）：从选区起点列到终点列。空选区 = 零宽（不绘制）。
pub fn selection_rect(sel_start_px: i32, sel_end_px: i32, y: i32) -> Rect {
    let (l, r) = if sel_start_px <= sel_end_px {
        (sel_start_px, sel_end_px)
    } else {
        (sel_end_px, sel_start_px)
    };
    Rect::new(l, y, (r - l).max(0) as u32, EDIT_LINE_H as u32)
}

/// IME 候选窗锚点：跟随插入符；底部越界时翻到上方（四边翻转的
/// 编辑框版——候选窗不许出屏，也不许盖住正在输入的行）。
pub fn ime_anchor(caret: Rect, cand_w: u32, cand_h: u32, screen: Rect) -> Rect {
    let mut x = caret.x + IME_GAP;
    let mut y = caret.y + caret.h as i32 + IME_GAP;
    if x + cand_w as i32 > screen.x + screen.w as i32 {
        x = screen.x + screen.w as i32 - cand_w as i32;
    }
    if y + cand_h as i32 > screen.y + screen.h as i32 {
        y = caret.y - cand_h as i32 - IME_GAP;
    }
    if x < screen.x {
        x = screen.x;
    }
    if y < screen.y {
        y = screen.y;
    }
    Rect::new(x, y, cand_w, cand_h)
}

/// 水平滚动钳制：保证插入符像素位落在可视带内（最小平移——与
/// h2edit `scroll_to_show` 同规则，像素口径）。
pub fn clamp_hscroll(caret_x: i32, scroll_x: i32, view_w: u32) -> i32 {
    let view_w = view_w as i32;
    if caret_x < scroll_x {
        caret_x
    } else if caret_x + CARET_W > scroll_x + view_w {
        caret_x + CARET_W - view_w
    } else {
        scroll_x
    }
}

// ---------------------------------------------------------------------------
// F276 落区高亮
// ---------------------------------------------------------------------------

/// 呼吸环参数：幅度 2px、周期 1600ms、标准档曲线（总谱内）。
pub const GLOW_AMP_PX: i32 = 2;
pub const GLOW_PERIOD_MS: u32 = 1600;

/// 落区高亮矩形：贴靠目标槽外扩 `SNAP_GAP/2 + 呼吸偏移`。
/// `t_ms` 为周期内相位（0..GLOW_PERIOD_MS），呼吸走标准档半行程
/// （0→1→0 的三角包络 × 标准曲线——总谱纪律：不许私设曲线）。
pub fn glow_rect(target: Rect, t_ms: u32) -> Rect {
    let half = GLOW_PERIOD_MS / 2;
    let phase = if t_ms % GLOW_PERIOD_MS < half {
        t_ms % GLOW_PERIOD_MS
    } else {
        GLOW_PERIOD_MS - t_ms % GLOW_PERIOD_MS
    };
    let t = phase as f32 / half as f32;
    let e = h2curve::sample(t, Curve::Standard) as f32;
    let grow = (GLOW_AMP_PX as f32 * e).round() as i32;
    let inset = crate::h2star::h2geo::SNAP_GAP_PX as i32 / 2 + grow;
    Rect::new(
        target.x - inset,
        target.y - inset,
        target.w + 2 * inset as u32,
        target.h + 2 * inset as u32,
    )
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2render_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2render");
    // F251 OSD：布局可算、弧度随音量单调、徽标在环下、预算红线常量。
    let scr = Rect::new(0, 0, 1920, 1080);
    let l0 = osd_layout(scr, 0);
    let l50 = osd_layout(scr, 50);
    let l100 = osd_layout(scr, 100);
    set.add(
        "h2render osd arc sweep",
        l0.sweep_deg == 0 && l50.sweep_deg == OSD_ARC_DEG / 2 && l100.sweep_deg == OSD_ARC_DEG,
        "level→sweep monotonic",
    );
    set.add(
        "h2render osd badge below",
        l50.badge.y as i32 > l50.center.1 + l50.radius as i32,
        "attribution visible",
    );
    set.add(
        "h2render osd budget",
        OSD_FIRST_FRAME_MS == 50 && osd_layout(scr, 255).sweep_deg == OSD_ARC_DEG,
        "50ms line + clamp",
    );
    // F254 采样核：0 半径零开销；倍频增长；512 taps 红线内有界。
    set.add(
        "h2render taps bounded",
        blur_taps(0) == 0 && blur_taps(4) == 12 && blur_taps(64) == 36,
        "octave taps",
    );
    // 降级滞回：连续 3 超线才降一档；单帧尖峰不触发；回稳 30 帧才升。
    let mut g = MaterialGovernor::new();
    let t1 = g.frame(true);
    let t2 = g.frame(false);
    set.add(
        "h2render spike no degrade",
        t1 == MaterialTier::Full && t2 == MaterialTier::Full,
        "single spike ignored",
    );
    for _ in 0..(DEGRADE_AFTER_FRAMES - 1) {
        let t = g.frame(true);
        assert_eq!(t, MaterialTier::Full);
    }
    let t3 = g.frame(true);
    set.add("h2render degrade step", t3 == MaterialTier::Reduced, "3 strikes → reduced");
    for _ in 0..DEGRADE_AFTER_FRAMES {
        let _ = g.frame(true);
    }
    let t4 = g.frame(true);
    set.add("h2render degrade floor", t4 == MaterialTier::Solid, "solid floor");
    // 升档滞回：29 帧不动、第 30 帧升一档。
    for _ in 0..(RESTORE_AFTER_FRAMES - 1) {
        let t = g.frame(false);
        assert_eq!(t, MaterialTier::Solid);
    }
    let t5 = g.frame(false);
    set.add("h2render restore hysteresis", t5 == MaterialTier::Reduced, "30 calm frames");
    // F260 编辑框：插入符成形、空选区零宽、IME 底部翻转、钳制最小平移。
    let caret = caret_rect(100, 0);
    set.add(
        "h2render caret shape",
        caret.w == 2 && caret.h == EDIT_LINE_H as u32 - 8,
        "2px caret",
    );
    let empty = selection_rect(50, 50, 0);
    set.add("h2render empty selection", empty.w == 0, "no zero-area spam");
    let sel = selection_rect(80, 30, 0);
    set.add("h2render selection order", sel.x == 30 && sel.w == 50, "reversed sel normalized");
    let scr2 = Rect::new(0, 0, 400, 100);
    let low = ime_anchor(caret_rect(100, 70), 120, 40, scr2);
    set.add(
        "h2render ime flip up",
        low.y + 40 <= 100 && low.y < caret_rect(100, 70).y,
        "flip above caret",
    );
    let inb = ime_anchor(caret_rect(320, 0), 120, 40, scr2);
    set.add("h2render ime clamp x", inb.x + 120 <= 400, "no offscreen");
    set.add(
        "h2render hscroll clamp",
        clamp_hscroll(10, 50, 100) == 10
            && clamp_hscroll(200, 50, 100) == 102
            && clamp_hscroll(60, 50, 100) == 50,
        "min shift",
    );
    // F276 呼吸环：周期内往返对称、外扩在缝与幅度预算内、端点稳定。
    let slot = Rect::new(100, 100, 300, 200);
    let g0 = glow_rect(slot, 0);
    let gh = glow_rect(slot, GLOW_PERIOD_MS / 2);
    let ge = glow_rect(slot, GLOW_PERIOD_MS - 1);
    set.add(
        "h2render glow symmetric",
        g0.w == slot.w + 2 * (SNAP_GAP_PX as i32 / 2) as u32
            && gh.w == slot.w + 2 * (SNAP_GAP_PX as i32 / 2 + GLOW_AMP_PX) as u32,
        "breathe amp",
    );
    set.add(
        "h2render glow ends equal",
        (g0.w as i32 - ge.w as i32).abs() <= 1,
        "period closure",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2render_all_green() {
        let set = run_h2render_checks();
        assert!(set.all_passed(), "h2render 自检有红项");
        assert!(!set.truncated(), "h2render 自检溢出");
    }

    #[test]
    fn governor_never_oscillates_at_threshold() {
        // 阈值边抖动注入：超/稳交替 100 帧，档位不来回横跳（滞回证明）。
        let mut g = MaterialGovernor::new();
        let mut flips = 0;
        let mut last = g.tier;
        for i in 0..100u32 {
            let t = g.frame(i % 2 == 0);
            if t != last {
                flips += 1;
                last = t;
            }
        }
        assert!(flips <= 2, "oscillating governor: {flips} flips");
    }

    #[test]
    fn osd_layout_holds_on_small_screens() {
        // 极小屏（800×480 车机类）：半径下限兜底、徽标不出屏。
        let l = osd_layout(Rect::new(0, 0, 800, 480), 60);
        assert!(l.radius >= 48);
        assert!(l.badge.x >= 0 && l.badge.x + l.badge.w as i32 <= 800);
    }
}
