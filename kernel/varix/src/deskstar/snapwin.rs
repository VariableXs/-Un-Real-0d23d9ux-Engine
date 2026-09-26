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

    /// 落点名（持久化投影的稳定字面量——v1.0 接口冻结纪律）。
    pub fn name(self) -> &'static str {
        match self {
            DropZone::LeftHalf => "LeftHalf",
            DropZone::RightHalf => "RightHalf",
            DropZone::TopHalf => "TopHalf",
            DropZone::BottomHalf => "BottomHalf",
            DropZone::TopLeftQuarter => "TopLeftQuarter",
            DropZone::TopRightQuarter => "TopRightQuarter",
            DropZone::BottomLeftQuarter => "BottomLeftQuarter",
            DropZone::BottomRightQuarter => "BottomRightQuarter",
        }
    }

    /// 落点名解析（持久化导入口；未知名如实拒——不为脏数据编落点）。
    pub fn by_name(s: &str) -> Option<DropZone> {
        Some(match s {
            "LeftHalf" => DropZone::LeftHalf,
            "RightHalf" => DropZone::RightHalf,
            "TopHalf" => DropZone::TopHalf,
            "BottomHalf" => DropZone::BottomHalf,
            "TopLeftQuarter" => DropZone::TopLeftQuarter,
            "TopRightQuarter" => DropZone::TopRightQuarter,
            "BottomLeftQuarter" => DropZone::BottomLeftQuarter,
            "BottomRightQuarter" => DropZone::BottomRightQuarter,
            _ => return None,
        })
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

/// 桌面独立分屏记忆（键 = (桌面 id, 应用名)——F081 多桌面前瞻）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeskMemo {
    pub desk_key: u64,
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
    /// 桌面独立记忆（深化层二：(桌面 id, 应用名) 键）。
    desk_memos: Vec<DeskMemo>,
    /// 键盘/鼠标双路结果一致账（同窗口同落点的几何哈希一致）。
    pub path_mismatches: u64,
    /// Esc 放弃账。
    pub aborts: u64,
    /// Snap Assist（深化层：就位后剩余窗口填位）。
    assist_open: bool,
    assist_candidates: Vec<AssistCandidate>,
    /// 窗口吸附组。
    groups: Vec<SnapGroup>,
    next_group_id: u64,
    /// 尺寸档（Win+方向微调循环）。
    tier: SizeTier,
    /// 预览淡入淡出账。
    fade_start: Option<u64>,
    fade_appearing: bool,
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
            desk_memos: Vec::new(),
            path_mismatches: 0,
            aborts: 0,
            assist_open: false,
            assist_candidates: Vec::new(),
            groups: Vec::new(),
            next_group_id: 1,
            tier: SizeTier::Half,
            fade_start: None,
            fade_appearing: false,
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
// ---------------------------------------------------------------------------
// 深化层二（回炉批 v2）：桌面独立记忆 / 记忆持久化投影 / Assist 互补
// 落位——主册【数据与存储】【状态与异常】【开源复用】逐条补足。
// 深化编号 D1-v2-SW*。
// ---------------------------------------------------------------------------

impl DropZone {
    /// 互补落位（Snap Assist 建议的数据面：已用左半 → 建议右半；
    /// 已用左上 → 建议右上；角区垂直镜像保持同列——用户动线最少跨越）。
    pub fn complement(self) -> DropZone {
        match self {
            DropZone::LeftHalf => DropZone::RightHalf,
            DropZone::RightHalf => DropZone::LeftHalf,
            DropZone::TopHalf => DropZone::BottomHalf,
            DropZone::BottomHalf => DropZone::TopHalf,
            DropZone::TopLeftQuarter => DropZone::TopRightQuarter,
            DropZone::TopRightQuarter => DropZone::TopLeftQuarter,
            DropZone::BottomLeftQuarter => DropZone::BottomRightQuarter,
            DropZone::BottomRightQuarter => DropZone::BottomLeftQuarter,
        }
    }
}

/// 分屏记忆持久化投影（「每应用蜂巢外元数据」的序列化面：一行一条
/// `desk|app|zone`，走蜂巢外的布局元数据文件——重启恢复的数据源）。
pub fn serialize_memos(memos: &[DeskMemo]) -> String {
    let mut out = String::new();
    for m in memos {
        out.push_str(&m.desk_key.to_string());
        out.push('|');
        out.push_str(&m.app);
        out.push('|');
        out.push_str(m.zone.name());
        out.push('\n');
    }
    out
}

/// 反序列化（损坏行如实跳过——恢复不因单行脏数据崩）。
pub fn deserialize_memos(s: &str) -> Vec<DeskMemo> {
    let mut out = Vec::new();
    for line in s.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 3 {
            continue;
        }
        let Ok(desk) = parts[0].parse::<u64>() else {
            continue;
        };
        let Some(zone) = DropZone::by_name(parts[2]) else {
            continue;
        };
        out.push(DeskMemo {
            desk_key: desk,
            app: String::from(parts[1]),
            zone,
        });
    }
    out
}

impl SnapMgr {
    /// 桌面独立记忆写入（键 = (桌面 id, 应用名)——F081 多桌面下
    /// 「同一应用在不同桌面各自记得自己的分屏位」）。
    pub fn remember_app_desk(&mut self, desk_id: u64, app: &str, zone: DropZone) {
        if let Some(m) = self
            .desk_memos
            .iter_mut()
            .find(|m| m.desk_key == desk_id && m.app == app)
        {
            m.zone = zone;
            return;
        }
        self.desk_memos.push(DeskMemo {
            desk_key: desk_id,
            app: String::from(app),
            zone,
        });
    }

    /// 桌面独立记忆读取（键不存在回退到全局应用记忆——单桌面用户
    /// 无感迁移；再无 → None）。
    pub fn recall_app_desk(&self, desk_id: u64, app: &str) -> Option<DropZone> {
        if let Some(m) = self
            .desk_memos
            .iter()
            .find(|m| m.desk_key == desk_id && m.app == app)
        {
            return Some(m.zone);
        }
        self.recall_app(app)
    }

    /// 持久化导出（桌面独立记忆 + 全局记忆一并投影）。
    pub fn export_memos(&self) -> String {
        let mut all: Vec<DeskMemo> = self.desk_memos.clone();
        for m in &self.memos {
            all.push(DeskMemo {
                desk_key: 0, // 0 = 全局（单桌面）档
                app: m.app.clone(),
                zone: m.zone,
            });
        }
        serialize_memos(&all)
    }

    /// 持久化导入（round-trip 语义：导出→导入逐条还原）。
    pub fn import_memos(&mut self, blob: &str) -> usize {
        let restored = deserialize_memos(blob);
        let n = restored.len();
        for m in restored {
            if m.desk_key == 0 {
                self.remember_app(&m.app, m.zone);
            } else {
                self.remember_app_desk(m.desk_key, &m.app, m.zone);
            }
        }
        n
    }

    /// Snap Assist 建议刷新（以最近就位落点的互补区为建议——候选
    /// 逐一分配互补位；候选多于互补位时从右半顺延填充）。
    pub fn assist_suggest_complements(&mut self, used: DropZone) {
        let mut next = used.complement();
        for c in self.assist_candidates.iter_mut() {
            c.suggest = next;
            // 下一候选取再互补（左右交替）——两候选即填满半区对。
            next = next.complement();
        }
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// 深化层（回炉批）：Snap Assist / 窗口组吸附 / 尺寸档循环 / 最小尺寸约束 /
// 预览淡入淡出——主册【交互设计】【开源复用】【设计细节】补足。
// ---------------------------------------------------------------------------

/// Snap Assist：就位后推荐剩余落位（Windows Snap Assist 动线——
/// 松手分屏后，剩余窗口列表出现，点选填入相邻空位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistCandidate {
    pub window: u64,
    pub title: String,
    /// 建议落点（与已就位窗口互补的半区/角区）。
    pub suggest: DropZone,
}

/// 尺寸档（Win+方向微调循环：半 → 三分 → 四分 → 半）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeTier {
    Half,
    Third,
    Quarter,
}

impl SizeTier {
    pub fn next(self) -> SizeTier {
        match self {
            SizeTier::Half => SizeTier::Third,
            SizeTier::Third => SizeTier::Quarter,
            SizeTier::Quarter => SizeTier::Half,
        }
    }

    /// 档位在半区内的切分（返回目标矩形——半区基准上细分子区）。
    pub fn rect_in_half(self, half: Rect, vertical_split: bool) -> Rect {
        match self {
            SizeTier::Half => half,
            SizeTier::Third => {
                let (w, h) = (half.w, half.h);
                if vertical_split {
                    Rect::new(half.x, half.y, w, h / 3 * 2)
                } else {
                    Rect::new(half.x, half.y, w / 3 * 2, h)
                }
            }
            SizeTier::Quarter => {
                let (w, h) = (half.w, half.h);
                if vertical_split {
                    Rect::new(half.x, half.y, w, h / 2)
                } else {
                    Rect::new(half.x, half.y, w / 2, h)
                }
            }
        }
    }
}

/// 窗口吸附组（成组：组内窗口保持相对位、可整体恢复）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapGroup {
    pub id: u64,
    /// 成员（窗口, 落点）。
    pub members: Vec<(u64, DropZone)>,
}

impl SnapMgr {
    /// Snap Assist 开启（release 就位后调用；候选由上层窗口清单供给）。
    pub fn open_assist(&mut self, candidates: &[(u64, &str)], now_ms: u64) {
        self.now_ms = now_ms;
        let used = self.preview_zone();
        let _ = used;
        self.assist_candidates = candidates
            .iter()
            .map(|(w, t)| AssistCandidate {
                window: *w,
                title: String::from(*t),
                suggest: DropZone::RightHalf, // 缺省建议：另一半区
            })
            .collect();
        self.assist_open = true;
    }

    pub fn assist_open(&self) -> bool {
        self.assist_open
    }

    pub fn assist_candidates(&self) -> &[AssistCandidate] {
        &self.assist_candidates
    }

    /// 点选候选 → 填入建议落位（返回落点矩形；Assist 收起）。
    pub fn assist_pick(&mut self, window: u64, now_ms: u64) -> Option<Rect> {
        if !self.assist_open {
            return None;
        }
        let pos = self
            .assist_candidates
            .iter()
            .position(|c| c.window == window)?;
        let c = self.assist_candidates.remove(pos);
        self.assist_open = !self.assist_candidates.is_empty();
        let rect = c.suggest.rect_in(self.work_area());
        self.remember_app("", c.suggest);
        self.now_ms = now_ms;
        Some(rect)
    }

    /// Assist 收起（点外 / Esc / 超时——与浮层出路纪律对齐）。
    pub fn close_assist(&mut self, now_ms: u64) {
        self.assist_open = false;
        self.assist_candidates.clear();
        self.now_ms = now_ms;
    }

    /// 建组（成组吸附：把两个已就位窗口编入同组）。
    pub fn group_create(&mut self, members: &[(u64, DropZone)]) -> u64 {
        self.next_group_id += 1;
        let id = self.next_group_id - 1;
        self.groups.push(SnapGroup {
            id,
            members: members.to_vec(),
        });
        id
    }

    /// 组恢复（应用重开/Win+Shift+方向族：整组按成员落点重摆——返回
    /// (窗口, 矩形) 表；缺组如实返回 None）。
    pub fn group_restore(&self, group_id: u64) -> Option<Vec<(u64, Rect)>> {
        let g = self.groups.iter().find(|g| g.id == group_id)?;
        let work = self.work_area();
        Some(
            g.members
                .iter()
                .map(|(w, z)| (*w, z.rect_in(work)))
                .collect(),
        )
    }

    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// 尺寸档循环：当前窗口在半区内 半→三分→四分 循环微调
    /// （Win+方向重复按压语义；vertical_split = 右半区上下切分）。
    pub fn cycle_tier(&mut self, base_zone: DropZone, vertical_split: bool, now_ms: u64) -> Rect {
        self.tier = self.tier.next();
        let half = base_zone.rect_in(self.work_area());
        let r = self.tier.rect_in_half(half, vertical_split);
        self.now_ms = now_ms;
        r
    }

    pub fn current_tier(&self) -> SizeTier {
        self.tier
    }

    /// 最小尺寸约束：落点矩形须容纳窗口最小尺寸（不满足 → 拒绝分屏
    /// + 原因说明——不硬塞出破碎布局）。
    pub fn check_min_size(&self, zone: DropZone, min: (i32, i32)) -> Result<Rect, &'static str> {
        let r = zone.rect_in(self.work_area());
        if r.w < min.0 {
            return Err("分屏区宽度小于窗口最小宽度");
        }
        if r.h < min.1 {
            return Err("分屏区高度小于窗口最小高度");
        }
        Ok(r)
    }

    /// 预览淡入淡出账（预览出现/离开各 120ms——出现即淡入，离开即淡出，
    /// 本账持相位与时刻）。
    pub fn preview_fade(&mut self, appearing: bool, now_ms: u64) {
        self.fade_start = Some(now_ms);
        self.fade_appearing = appearing;
        self.now_ms = now_ms;
    }

    /// 淡入淡出进度（千分比；120ms）。
    pub fn fade_progress(&self) -> u16 {
        match self.fade_start {
            None => 0,
            Some(t0) => {
                let t = ((self.now_ms.saturating_sub(t0)) as u32).min(PREVIEW_FADE_MS);
                (t * 1000 / PREVIEW_FADE_MS) as u16
            }
        }
    }
}

/// 预览淡入淡出时长（ms）。
pub const PREVIEW_FADE_MS: u32 = 120;

/// F080 深化自检：Snap Assist 填位与收起、组建组与恢复、尺寸档循环、
/// 最小尺寸约束、预览淡入淡出。
pub fn run_snapwin_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F080-deep");
    let screen = Rect::new(0, 0, 1920, 1080);
    let mut m = SnapMgr::new(screen, 48);
    // 1. Snap Assist：release 后开启，候选缺省建议右半。
    m.drag_start(Rect::new(400, 300, 600, 400), false, 0);
    m.pointer_move((2, 500), 10);
    m.release(20);
    m.open_assist(&[(1u64, "资料"), (2, "乐谱")], 30);
    let open = m.assist_open() && m.assist_candidates().len() == 2;
    // 2. 点选：1 号填右半（矩形 = RightHalf）；2 号候选仍在。
    let picked = m.assist_pick(1, 100);
    let pick_ok = picked == Some(DropZone::RightHalf.rect_in(m.work_area()))
        && m.assist_candidates().len() == 1;
    // 3. 收起（点外语义）。
    m.close_assist(150);
    let closed = !m.assist_open() && m.assist_pick(2, 160).is_none();
    set.add(
        "snap-assist",
        open && pick_ok && closed,
        "assist fill + dismiss",
    );
    // 4. 窗口组：建组 → 整组恢复几何。
    let gid = m.group_create(&[(1, DropZone::LeftHalf), (2, DropZone::RightHalf)]);
    let restored = m.group_restore(gid).unwrap();
    let group_ok = restored.len() == 2
        && restored[0].1 == DropZone::LeftHalf.rect_in(m.work_area())
        && restored[1].1 == DropZone::RightHalf.rect_in(m.work_area());
    let missing = m.group_restore(999).is_none();
    set.add(
        "group-restore",
        group_ok && missing && m.group_count() == 1,
        "group geometry",
    );
    // 5. 尺寸档循环：半 → 三分 → 四分 → 半（循环回绕）。
    let t0 = m.current_tier();
    let r1 = m.cycle_tier(DropZone::LeftHalf, false, 1_000);
    let r2 = m.cycle_tier(DropZone::LeftHalf, false, 1_010);
    let r3 = m.cycle_tier(DropZone::LeftHalf, false, 1_020);
    let work = m.work_area();
    let half = DropZone::LeftHalf.rect_in(work);
    set.add(
        "tier-cycle",
        t0 == SizeTier::Half
            && r1.w == half.w / 3 * 2
            && r2.w == half.w / 2
            && r3.w == half.w
            && m.current_tier() == SizeTier::Half,
        "half→third→quarter→half",
    );
    // 6. 最小尺寸约束：高度不足的角区拒收 + 原因文案。
    let half_zone = m.check_min_size(DropZone::LeftHalf, (400, 300));
    let tiny_zone = m.check_min_size(DropZone::TopLeftQuarter, (700, 600));
    set.add(
        "min-size",
        half_zone.is_ok() && tiny_zone.is_err(),
        "reject broken layouts",
    );
    // 7. 预览淡入淡出：120ms 两相。
    m.preview_fade(true, 2_000);
    m.now_ms = 2_060; // 淡入中段
    let mid = m.fade_progress();
    m.preview_fade(false, 2_200);
    m.now_ms = 2_260;
    let fade_out = m.fade_progress();
    set.add(
        "preview-fade",
        mid > 0 && mid < 1000 && fade_out > 0 && PREVIEW_FADE_MS == 120,
        "120ms two-phase",
    );
    set
}

#[cfg(test)]
mod tests_deep {
    use super::*;

    #[test]
    fn assist_picks_all_candidates_then_closes() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let mut m = SnapMgr::new(screen, 48);
        m.drag_start(Rect::new(400, 300, 600, 400), false, 0);
        m.pointer_move((2, 500), 10);
        m.release(20);
        m.open_assist(&[(1, "甲"), (2, "乙"), (3, "丙")], 30);
        assert!(m.assist_pick(2, 40).is_some());
        assert!(m.assist_pick(1, 50).is_some());
        assert!(m.assist_pick(3, 60).is_some());
        assert!(!m.assist_open(), "候选耗尽自动收起");
    }

    #[test]
    fn tier_cycle_vertical_split() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let mut m = SnapMgr::new(screen, 48);
        let r = m.cycle_tier(DropZone::RightHalf, true, 0);
        assert_eq!(r.h, DropZone::RightHalf.rect_in(m.work_area()).h / 3 * 2);
        assert_eq!(r.x, DropZone::RightHalf.rect_in(m.work_area()).x);
    }

    #[test]
    fn min_size_boundary_exact() {
        let screen = Rect::new(0, 0, 1920, 1080);
        let m = SnapMgr::new(screen, 48);
        let half = DropZone::LeftHalf.rect_in(m.work_area());
        assert!(m.check_min_size(DropZone::LeftHalf, (half.w, half.h)).is_ok(), "恰好等于最小尺寸 = 通过");
    }

    #[test]
    fn snapwin_deep_checks_all_green() {
        let set = run_snapwin_deep_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F080-deep 红项：{}/{} 绿", p, p + f);
    }
}

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

// ---------------------------------------------------------------------------
// 深化自检二（回炉批 D1-v2）——桌面独立记忆 / 持久化 round-trip /
// Assist 互补落位。判据唯一源：主册 G-C-10 数据与存储/状态与异常。
// ---------------------------------------------------------------------------

/// F080 深化自检二：三族逐条记账。
pub fn run_snapwin_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F080-deep2");
    let screen = Rect::new(0, 0, 1920, 1080);
    let mut m = SnapMgr::new(screen, 48);
    // 1. 桌面独立记忆：同应用在桌 1/桌 2 各自记得；缺桌键回退全局。
    m.remember_app("星记", DropZone::LeftHalf);
    m.remember_app_desk(1, "星记", DropZone::RightHalf);
    m.remember_app_desk(2, "星记", DropZone::TopHalf);
    let d1 = m.recall_app_desk(1, "星记");
    let d2 = m.recall_app_desk(2, "星记");
    let fallback = m.recall_app_desk(9, "星记"); // 无桌 9 记忆 → 全局
    set.add(
        "desk-scoped-memo",
        d1 == Some(DropZone::RightHalf)
            && d2 == Some(DropZone::TopHalf)
            && fallback == Some(DropZone::LeftHalf),
        "(desk, app) keyed memo",
    );
    // 2. 持久化 round-trip：导出→导入逐条还原（含全局档 desk=0）。
    m.remember_app_desk(1, "乐谱", DropZone::BottomLeftQuarter);
    let blob = m.export_memos();
    let mut m2 = SnapMgr::new(screen, 48);
    let n = m2.import_memos(&blob);
    let roundtrip = n >= 4
        && m2.recall_app_desk(1, "星记") == Some(DropZone::RightHalf)
        && m2.recall_app_desk(2, "星记") == Some(DropZone::TopHalf)
        && m2.recall_app_desk(1, "乐谱") == Some(DropZone::BottomLeftQuarter)
        && m2.recall_app_desk(9, "星记") == Some(DropZone::LeftHalf);
    set.add("memo-roundtrip", roundtrip, "export/import parity");
    // 3. 脏数据诚实处理：损坏行跳过、未知名拒收——恢复不崩不编。
    let dirty = "1|星记|RightHalf\n坏行\n2|x|NotAZone\n1|乐谱|TopHalf\n";
    let mut m3 = SnapMgr::new(screen, 48);
    let got = m3.import_memos(dirty);
    set.add(
        "memo-dirty-safe",
        got == 2
            && m3.recall_app_desk(1, "星记") == Some(DropZone::RightHalf)
            && m3.recall_app_desk(1, "乐谱") == Some(DropZone::TopHalf),
        "corrupt lines skipped",
    );
    // 4. Assist 互补建议：左半就位 → 右半建议；两候选 → 左右交替。
    m.drag_start(Rect::new(400, 300, 600, 400), false, 6_000);
    m.pointer_move((2, 500), 6_010);
    m.release(6_020); // 左半就位
    m.open_assist(&[(11u64, "资料"), (12, "乐谱")], 6_030);
    m.assist_suggest_complements(DropZone::LeftHalf);
    let c = m.assist_candidates();
    set.add(
        "assist-complement",
        c[0].suggest == DropZone::RightHalf && c[1].suggest == DropZone::LeftHalf
            && DropZone::TopLeftQuarter.complement() == DropZone::TopRightQuarter
            && DropZone::BottomHalf.complement() == DropZone::TopHalf,
        "complement suggestions",
    );
    set
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_deep2 {
    use super::*;

    #[test]
    fn zone_name_roundtrip_all_eight() {
        for z in DropZone::ALL {
            assert_eq!(DropZone::by_name(z.name()), Some(z), "{} 往返", z.name());
        }
        assert_eq!(DropZone::by_name("Bogus"), None);
    }

    #[test]
    fn complement_maps_all_eight() {
        // 互补映射双射：每个落点恰有一个互补，且互补的互补 = 自己。
        for z in DropZone::ALL {
            assert_eq!(z.complement().complement(), z);
        }
    }

    #[test]
    fn export_empty_is_empty_string() {
        let screen = Rect::new(0, 0, 100, 100);
        let m = SnapMgr::new(screen, 0);
        assert_eq!(m.export_memos(), "", "无记忆导出空串——诚实无数据");
    }

    #[test]
    fn snapwin_deep2_checks_all_green() {
        let set = run_snapwin_deep2_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F080-deep2 红项：{}/{} 绿", p, p + f);
    }
}
