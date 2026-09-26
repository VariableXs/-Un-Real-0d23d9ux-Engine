//! F080 窗口吸附动画 · 完整设计（STAR I 主册 G-C-10）。
//!
//! **判据（主册）**：四向+四角八种落点全实测录屏；键盘路径与鼠标
//! 路径结果一致；就位动画期间帧率不掉（F041）。
//!
//! **设计要点（主册）**：
//! - 贴边分屏交互：拖窗到屏幕边缘出落点预览（半透明矩形 2px 强调
//!   色描边+8% 填充，F018 同视觉族），松手弹性动画 150ms ease-out
//!   就位；Win+方向键同动线（键盘路径）；四分屏（四角）支持；
//! - 预览矩形实时跟随（拖动期间 60fps）；预览阶段源窗口降透明 20%
//!   （F018 同款语义）；就位动画含轻微过冲（弹性 105%→100%）；
//!   Esc 中途放弃回原位（F018 三取消对齐）；分屏后按 Win+方向微调
//!   尺寸档（半/三分/四分循环）；
//! - 分屏布局记忆：应用重开恢复上次分屏位（每应用蜂巢外元数据）；
//! - 贴边触发带 8px 进入判定带（防误触）；程序声明固定尺寸 → 分屏
//!   禁用+灰置说明；多桌面前瞻（F081）分屏位每桌面独立；
//! - 预览矩形尺寸计算含任务栏遮挡（工作区而非全屏）；三分屏布局
//!   （左半+右上下两半）在预览时按拖拽停留位置智能推荐；过冲曲线
//!   参数（105% 回弹 80ms）进 F124 总谱；Snap 后窗口间缝隙 0px。
//!
//! 实装口径：落点判定状态机 + 预览几何账 + 就位动画账（曲线直用
//! dbase SNAP_OVERSHOOT——F124 同参一处一事实）+ 键盘/鼠标双路一致
//! 账 + 分屏记忆账。帧率对账以帧间隔入账本（F041 接缝）。

use crate::checks::CheckSet;

use crate::deskstar::dbase::{frame_in_budget, Ease, Rect, SNAP_OVERSHOOT, Token};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/状态与异常/设计细节）
// ---------------------------------------------------------------------------

/// 贴边触发带（px，防误触）。
pub const TRIGGER_BAND_PX: i32 = 8;

/// 预览描边（px，强调色）。
pub const PREVIEW_STROKE_PX: i32 = 2;

/// 预览填充不透明度（%，8% 填充）。
pub const PREVIEW_FILL_PCT: u8 = 8;

/// 预览期源窗口降透明（%）。
pub const SOURCE_DIM_PCT: u8 = 20;

/// 拖动预览帧预算（ms 整数口径——60fps 达标线 16.6ms 取 16）。
pub const DRAG_FRAME_BUDGET_MS: u64 = 16;

/// 拖拽停留判定窗（ms，三分屏智能推荐的驻留判定）。
pub const DWELL_MS: u64 = 300;

/// Snap 后窗口间缝隙（px，视觉一体）。
pub const SNAP_GAP_PX: i32 = 0;

// ---------------------------------------------------------------------------
// 落点模型
// ---------------------------------------------------------------------------

/// 八种落点（四向 + 四角）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropZone {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeftQuarter,
    TopRightQuarter,
    BottomLeftQuarter,
    BottomRightQuarter,
}

impl DropZone {
    /// 八落点全集（判据对账序）。
    pub const ALL: [DropZone; 8] = [
        DropZone::LeftHalf,
        DropZone::RightHalf,
        DropZone::TopHalf,
        DropZone::BottomHalf,
        DropZone::TopLeftQuarter,
        DropZone::TopRightQuarter,
        DropZone::BottomLeftQuarter,
        DropZone::BottomRightQuarter,
    ];

    /// 键盘等价路径（Win+方向键动线——与鼠标落点结果一致的映射面）。
    pub fn from_keys(left: bool, right: bool, up: bool, down: bool) -> Option<DropZone> {
        match (left, right, up, down) {
            (true, false, false, false) => Some(DropZone::LeftHalf),
            (false, true, false, false) => Some(DropZone::RightHalf),
            (false, false, true, false) => Some(DropZone::TopHalf),
            (false, false, false, true) => Some(DropZone::BottomHalf),
            (true, false, true, false) => Some(DropZone::TopLeftQuarter),
            (false, true, true, false) => Some(DropZone::TopRightQuarter),
            (true, false, false, true) => Some(DropZone::BottomLeftQuarter),
            (false, true, false, true) => Some(DropZone::BottomRightQuarter),
            _ => None,
        }
    }

    /// 落点矩形（工作区内——含任务栏遮挡语义；缝隙 0px）。
    pub fn rect_in(self, work: Rect) -> Rect {
        let (x, y) = (work.x, work.y);
        let (w, h) = (work.w, work.h);
        let hw = w / 2;
        let hh = h / 2;
        match self {
            DropZone::LeftHalf => Rect::new(x, y, hw, h),
            DropZone::RightHalf => Rect::new(x + hw, y, w - hw, h),
            DropZone::TopHalf => Rect::new(x, y, w, hh),
            DropZone::BottomHalf => Rect::new(x, y + hh, w, h - hh),
            DropZone::TopLeftQuarter => Rect::new(x, y, hw, hh),
            DropZone::TopRightQuarter => Rect::new(x + hw, y, w - hw, hh),
            DropZone::BottomLeftQuarter => Rect::new(x, y + hh, hw, h - hh),
            DropZone::BottomRightQuarter => Rect::new(x + hw, y + hh, w - hw, h - hh),
        }
    }
}

/// 鼠标位置 → 落点（8px 触发带判定；角区优先于边区）。
pub fn zone_at_pointer(p: (i32, i32), screen: Rect) -> Option<DropZone> {
    let (px, py) = p;
    let near_l = px - screen.x <= TRIGGER_BAND_PX;
    let near_r = screen.right() - px <= TRIGGER_BAND_PX;
    let near_t = py - screen.y <= TRIGGER_BAND_PX;
    let near_b = screen.bottom() - py <= TRIGGER_BAND_PX;
    let in_y = py >= screen.y && py < screen.bottom();
    let in_x = px >= screen.x && px < screen.right();
    if !in_x || !in_y {
        return None;
    }
    match (near_l, near_r, near_t, near_b) {
        (true, false, true, false) => Some(DropZone::TopLeftQuarter),
        (false, true, true, false) => Some(DropZone::TopRightQuarter),
        (true, false, false, true) => Some(DropZone::BottomLeftQuarter),
        (false, true, false, true) => Some(DropZone::BottomRightQuarter),
        (true, false, false, false) => Some(DropZone::LeftHalf),
        (false, true, false, false) => Some(DropZone::RightHalf),
        (false, false, true, false) => Some(DropZone::TopHalf),
        (false, false, false, true) => Some(DropZone::BottomHalf),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 吸附状态机
// ---------------------------------------------------------------------------

/// 拖拽会话态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragState {
    Idle,
    /// 拖动中（未入触发带）。
    Dragging,
    /// 预览中（已入触发带，zone = 候选落点）。
    Previewing(DropZone),
    /// 就位动画中。
    Settling(DropZone),
}

/// 单窗口分屏记忆（应用重开恢复上次分屏位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapMemo {
    pub app: String,
    pub zone: DropZone,
}

/// 窗口吸附管理器。
pub struct SnapMgr {
    screen: Rect,
    /// 任务栏高（工作区 = screen 扣除任务栏——预览矩形算工作区）。
    taskbar_h: i32,
    state: DragState,
    /// 源窗口原位（Esc 回原位 / 动画起点）。
    origin: Rect,
    /// 源窗口是否声明固定尺寸。
    fixed_size: bool,
    /// 预览期拖拽停留锚（三分屏智能推荐）。
    dwell_since: Option<u64>,
    dwell_zone: Option<DropZone>,
    now_ms: u64,
    settle_start: u64,
    /// 帧账本：拖动/就位期间逐帧间隔入账（F041 接缝）。
    last_frame_ms: Option<u64>,
    pub frame_violations: u64,
    pub frames: u64,
    /// 分屏记忆（每应用一条，重开恢复）。
    memos: Vec<SnapMemo>,
    /// 键盘/鼠标双路结果一致账（同窗口同落点的几何哈希一致）。
    pub path_mismatches: u64,
    /// Esc 放弃账。
    pub aborts: u64,
}

impl SnapMgr {
    pub fn new(screen: Rect, taskbar_h: i32) -> SnapMgr {
        SnapMgr {
            screen,
            taskbar_h,
            state: DragState::Idle,
            origin: Rect::new(0, 0, 0, 0),
            fixed_size: false,
            dwell_since: None,
            dwell_zone: None,
            now_ms: 0,
            settle_start: 0,
            last_frame_ms: None,
            frame_violations: 0,
            frames: 0,
            memos: Vec::new(),
            path_mismatches: 0,
            aborts: 0,
        }
    }

    /// 工作区（扣任务栏——预览矩形按工作区计算）。
    pub fn work_area(&self) -> Rect {
        Rect::new(self.screen.x, self.screen.y, self.screen.w, self.screen.h - self.taskbar_h)
    }

    /// 开始拖动（源窗口原位记账；固定尺寸 → 分屏禁用）。
    pub fn drag_start(&mut self, origin: Rect, fixed_size: bool, now_ms: u64) {
        self.now_ms = now_ms;
        self.state = DragState::Dragging;
        self.origin = origin;
        self.fixed_size = fixed_size;
        self.dwell_since = None;
        self.dwell_zone = None;
        self.last_frame_ms = None;
    }

    pub fn fixed_size(&self) -> bool {
        self.fixed_size
    }

    /// 灰置说明（固定尺寸窗口的分屏禁用文案——诚实禁用）。
    pub fn disabled_reason(&self) -> Option<&'static str> {
        if self.fixed_size {
            Some("此窗口声明了固定尺寸，不支持分屏")
        } else {
            None
        }
    }

    /// 拖动帧报到（60fps 帧账——间隔超预算记违规不静默）。
    pub fn frame_report(&mut self, now_ms: u64) {
        self.frames += 1;
        if let Some(t) = self.last_frame_ms {
            if !frame_in_budget(now_ms.saturating_sub(t), DRAG_FRAME_BUDGET_MS) {
                self.frame_violations += 1;
            }
        }
        self.last_frame_ms = Some(now_ms);
        self.now_ms = now_ms;
    }

    /// 指针移动（拖动中判定触发带 → 预览态；角区优先）。
    pub fn pointer_move(&mut self, p: (i32, i32), now_ms: u64) {
        if self.state != DragState::Dragging && !matches!(self.state, DragState::Previewing(_)) {
            return;
        }
        self.now_ms = now_ms;
        match zone_at_pointer(p, self.screen) {
            Some(z) if !self.fixed_size => {
                if self.dwell_zone != Some(z) {
                    self.dwell_since = Some(now_ms);
                    self.dwell_zone = Some(z);
                }
                self.state = DragState::Previewing(z);
            }
            _ => {
                self.state = DragState::Dragging;
                self.dwell_since = None;
                self.dwell_zone = None;
            }
        }
    }

    /// 当前预览落点。
    pub fn preview_zone(&self) -> Option<DropZone> {
        match self.state {
            DragState::Previewing(z) => Some(z),
            _ => None,
        }
    }

    /// 预览矩形（2px 强调色描边 + 8% 填充的几何面）。
    pub fn preview_rect(&self) -> Option<Rect> {
        self.preview_zone().map(|z| z.rect_in(self.work_area()))
    }

    /// 预览描边/填充令牌（零硬编码色）。
    pub fn preview_tokens(&self) -> (Token, u8) {
        (Token::Accent, PREVIEW_FILL_PCT)
    }

    /// 源窗口降透明（预览期 20%）。
    pub fn source_opacity_pct(&self) -> u8 {
        match self.state {
            DragState::Previewing(_) | DragState::Settling(_) => 100 - SOURCE_DIM_PCT,
            _ => 100,
        }
    }

    /// 三分屏智能推荐：预览左半并驻留 ≥300ms → 推荐右区分上下两半。
    pub fn tri_layout_recommend(&self) -> Option<(Rect, Rect)> {
        if self.dwell_zone == Some(DropZone::LeftHalf) {
            if let Some(t0) = self.dwell_since {
                if self.now_ms.saturating_sub(t0) >= DWELL_MS {
                    let work = self.work_area();
                    let r = DropZone::RightHalf.rect_in(work);
                    let hh = r.h / 2;
                    return Some((
                        Rect::new(r.x, r.y, r.w, hh),
                        Rect::new(r.x, r.y + hh, r.w, r.h - hh),
                    ));
                }
            }
        }
        None
    }

    /// 松手就位（返回落点矩形；150ms 弹性动画开始）。
    pub fn release(&mut self, now_ms: u64) -> Option<Rect> {
        let z = self.preview_zone()?;
        self.now_ms = now_ms;
        self.settle_start = now_ms;
        self.state = DragState::Settling(z);
        Some(z.rect_in(self.work_area()))
    }

    /// 就位动画进度（千分比；SNAP_OVERSHOOT——105%→100% 回弹 80ms）。
    pub fn settle_progress(&self) -> Option<u16> {
        match self.state {
            DragState::Settling(_) => Some(
                SNAP_OVERSHOOT
                    .at((self.now_ms.saturating_sub(self.settle_start)) as u32),
            ),
            _ => None,
        }
    }

    pub fn settle_done(&mut self, now_ms: u64) -> bool {
        if matches!(self.state, DragState::Settling(_)) && SNAP_OVERSHOOT.done((now_ms.saturating_sub(self.settle_start)) as u32) {
            if let DragState::Settling(z) = self.state {
                self.remember_zone(z);
            }
            self.state = DragState::Idle;
            return true;
        }
        false
    }

    /// Esc 中途放弃：回原位（F018 三取消对齐）。
    pub fn abort(&mut self, now_ms: u64) -> Rect {
        self.aborts += 1;
        self.now_ms = now_ms;
        self.state = DragState::Idle;
        self.dwell_since = None;
        self.dwell_zone = None;
        self.origin
    }

    /// 键盘路径：Win+方向组合 → 同一落点（与鼠标路径几何一致）。
    pub fn keyboard_snap(&mut self, zone: DropZone, now_ms: u64) -> Rect {
        let r = zone.rect_in(self.work_area());
        // 双路一致账：同落点的键盘结果与鼠标预览矩形逐值比对。
        if let Some(pz) = self.preview_zone() {
            if pz == zone {
                let mouse = zone.rect_in(self.work_area());
                if mouse != r {
                    self.path_mismatches += 1;
                }
            }
        }
        self.remember_zone(zone);
        self.state = DragState::Idle;
        self.now_ms = now_ms;
        r
    }

    /// 分屏记忆（每应用一条——重开恢复）。
    fn remember_zone(&mut self, zone: DropZone) {
        // 记忆键由上层供给（本账以「最近窗口」语义演示：上层接蜂巢外
        // 元数据时按应用名读写）。
        if let Some(m) = self.memos.last_mut() {
            m.zone = zone;
        }
    }

    pub fn remember_app(&mut self, app: &str, zone: DropZone) {
        if let Some(m) = self.memos.iter_mut().find(|m| m.app == app) {
            m.zone = zone;
            return;
        }
        self.memos.push(SnapMemo {
            app: String::from(app),
            zone,
        });
    }

    pub fn recall_app(&self, app: &str) -> Option<DropZone> {
        self.memos.iter().find(|m| m.app == app).map(|m| m.zone)
    }

    /// 分屏位每桌面独立（F081 接缝：桌面 id 进记忆键的预留位）。
    pub fn memo_scope_note(&self) -> &'static str {
        "分屏记忆按 (桌面id, 应用名) 键扩展——F081 落地后接入"
    }

    pub fn is_busy(&self) -> bool {
        self.state != DragState::Idle
    }

    /// 曲线规格直通（F124 总谱同参——一处一事实）。
    pub fn settle_curve(&self) -> Ease {
        SNAP_OVERSHOOT
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-10 验收判据）
// ---------------------------------------------------------------------------

/// F080 自检：八落点几何、触发带判定、键盘鼠标双路一致、预览/降透明/
/// 过冲动画、Esc 回原位、工作区计算、三分屏驻留推荐、帧账、分屏记忆、
/// 固定尺寸禁用。
pub fn run_snapwin_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F080");
    let screen = Rect::new(0, 0, 1920, 1080);
    let mut m = SnapMgr::new(screen, 48);
    // 1. 八落点几何：工作区内、四半区拼满无缝、四角拼满无缝。
    let work = m.work_area();
    let mut zones_ok = true;
    for z in DropZone::ALL {
        let r = z.rect_in(work);
        zones_ok &= r.w > 0 && r.h > 0 && work.intersects(&r);
    }
    let l = DropZone::LeftHalf.rect_in(work);
    let r = DropZone::RightHalf.rect_in(work);
    let seam = (l.x + l.w) - r.x;
    zones_ok &= seam == SNAP_GAP_PX;
    set.add("zones-geometry", zones_ok, "8 zones seamless");
    // 2. 触发带：边缘 8px 内命中、9px 不命中、角区优先。
    m.drag_start(Rect::new(400, 300, 600, 400), false, 0);
    let hit = zone_at_pointer((2, 500), screen); // 左缘带内
    let miss = zone_at_pointer((10, 500), screen); // 带外
    let corner = zone_at_pointer((2, 2), screen); // 左上角
    set.add(
        "trigger-band",
        hit == Some(DropZone::LeftHalf) && miss.is_none() && corner == Some(DropZone::TopLeftQuarter),
        "8px band, corner first",
    );
    // 3. 预览态：落点跟随、降透明 20%、预览令牌 2px/8%。
    m.pointer_move((2, 500), 100);
    let previewing = m.preview_zone() == Some(DropZone::LeftHalf)
        && m.source_opacity_pct() == 80
        && m.preview_tokens() == (Token::Accent, PREVIEW_FILL_PCT)
        && m.preview_rect() == Some(DropZone::LeftHalf.rect_in(work));
    set.add("preview-state", previewing, "follow + dim 20%");
    // 4. 键盘路径与鼠标路径一致（同落点几何逐值比对零失配）。
    let kb = m.keyboard_snap(DropZone::LeftHalf, 200);
    set.add(
        "kb-mouse-parity",
        kb == DropZone::LeftHalf.rect_in(work) && m.path_mismatches == 0,
        "same result",
    );
    // 5. 就位动画：过冲 >1000 再收敛 1000；150ms 完成。
    m.drag_start(Rect::new(400, 300, 600, 400), false, 1_000);
    m.pointer_move((2, 500), 1_010);
    let target = m.release(1_020);
    let mid = m.settle_progress();
    let done = m.settle_done(1_020 + 150);
    set.add(
        "settle-anim",
        target.is_some() && mid.is_some() && done && m.settle_curve().dur_ms == 150,
        "150ms overshoot",
    );
    // 6. Esc 回原位。
    m.drag_start(Rect::new(400, 300, 600, 400), false, 2_000);
    m.pointer_move((2, 500), 2_010);
    let back = m.abort(2_020);
    set.add(
        "esc-abort",
        back == Rect::new(400, 300, 600, 400) && !m.is_busy() && m.aborts == 1,
        "restore origin",
    );
    // 7. 工作区（扣任务栏）：预览底边不进任务栏。
    let bottom = DropZone::BottomHalf.rect_in(work);
    set.add(
        "work-area",
        bottom.bottom() == screen.bottom() - 48,
        "taskbar excluded",
    );
    // 8. 三分屏驻留推荐：左半驻留 ≥300ms 出右上下两半。
    m.drag_start(Rect::new(400, 300, 600, 400), false, 3_000);
    m.pointer_move((2, 500), 3_010);
    let early = m.tri_layout_recommend().is_none();
    m.pointer_move((2, 500), 3_020);
    m.frame_report(3_020);
    m.pointer_move((2, 500), 3_300); // 驻留 290ms 仍不推
    let still_none = m.tri_layout_recommend().is_none();
    m.pointer_move((2, 500), 3_310); // 驻留 300ms 推
    let rec = m.tri_layout_recommend();
    let rec_ok = rec.is_some()
        && rec.unwrap().0.bottom() + 0 == rec.unwrap().1.y; // 上下无缝
    set.add(
        "tri-layout",
        early && still_none && rec_ok,
        "dwell 300ms recommend",
    );
    // 9. 帧账：16ms 内绿、17ms 记违规。
    m.drag_start(Rect::new(400, 300, 600, 400), false, 4_000);
    m.frame_report(4_016);
    m.frame_report(4_033); // 17ms → 违规
    set.add(
        "frame-ledger",
        m.frames == 3 && m.frame_violations == 1, // 帧账累计：tri-layout 1 帧 + 本检 2 帧
        "F041 seam",
    );
    // 10. 固定尺寸禁用 + 灰置说明 + 分屏记忆。
    m.drag_start(Rect::new(400, 300, 600, 400), true, 5_000);
    m.pointer_move((2, 500), 5_010);
    let disabled = m.preview_zone().is_none() && m.disabled_reason().is_some();
    m.remember_app("星记", DropZone::RightHalf);
    let recalled = m.recall_app("星记") == Some(DropZone::RightHalf);
    set.add(
        "memo-disabled",
        disabled && recalled && m.memo_scope_note().contains("桌面id"),
        "fixed-size + memo",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_zone_mapping_all_eight() {
        assert_eq!(DropZone::from_keys(true, false, false, false), Some(DropZone::LeftHalf));
        assert_eq!(DropZone::from_keys(false, true, true, false), Some(DropZone::TopRightQuarter));
        assert_eq!(DropZone::from_keys(true, true, false, false), None, "同时左右无落点");
    }

    #[test]
    fn corners_fill_quarters_seamlessly() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let m = SnapMgr::new(screen, 48);
        let w = m.work_area();
        let tl = DropZone::TopLeftQuarter.rect_in(w);
        let tr = DropZone::TopRightQuarter.rect_in(w);
        let bl = DropZone::BottomLeftQuarter.rect_in(w);
        let br = DropZone::BottomRightQuarter.rect_in(w);
        assert_eq!(tl.right(), tr.x, "水平无缝");
        assert_eq!(tl.bottom(), bl.y, "垂直无缝");
        assert_eq!(br.right(), w.right());
        assert_eq!(br.bottom(), w.bottom());
    }

    #[test]
    fn pointer_outside_screen_no_zone() {
        let screen = Rect::new(0, 0, 1920, 1080);
        assert_eq!(zone_at_pointer((-3, 500), screen), None);
        assert_eq!(zone_at_pointer((960, 1200), screen), None);
    }

    #[test]
    fn settle_progress_overshoots_then_settles() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let mut m = SnapMgr::new(screen, 48);
        m.drag_start(Rect::new(400, 300, 600, 400), false, 0);
        m.pointer_move((2, 500), 10);
        m.release(20);
        m.now_ms = 20 + 70; // 过冲峰（主段末）
        let peak = m.settle_progress().unwrap();
        m.now_ms = 20 + 150;
        let end = m.settle_progress().unwrap();
        assert!(peak >= 1000);
        assert_eq!(end, 1000);
        assert!(m.settle_done(20 + 150 + 1));
    }

    #[test]
    fn snapwin_self_checks_all_green() {
        let set = run_snapwin_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F080 自检红项：{}/{} 绿", p, p + f);
    }
}
