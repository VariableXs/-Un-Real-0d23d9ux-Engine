//! F204 滚动行为统一 · 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F204「滚动行为统一」。
//!
//! **验收标准（主册第一句）**：滚轮、触控板、滚动条、键盘四条通路的
//! 参数集中在一处配置（单一定义点）；120fps 滚动无文字模糊；顶/底
//! 回弹动画时长实测 320ms±10ms。
//!
//! **设计要点**：
//! - [`ScrollTuning`] 单一定义点：全部参数（滚轮行/齿 1-10 默认 3、
//!   行高、惯性时长、滚动条悬停宽、回弹过冲、回弹时长）只在此结构
//!   声明一次，并整体旋钮化（`declare_knobs`/`from_knobreg`）——
//!   四通路读同一份参数，一处调全系统生效；
//! - [`normalize`] 四通路输入归一器：滚轮齿数/触控板像素/滚动条比例/
//!   键盘行数 → 统一速度域（像素位移 + 可选绝对目标）；
//! - 触控板惯性收尾走 F124「弹性」档（`Curve::Spring`，320ms）：
//!   逐帧按弹性进度增量释放位移，收尾带 F124 原生回弹形状；
//! - [`EdgeBounce`] 顶/底回弹状态机：越界位移吸收为过冲（钳 8px），
//!   松手后按 F124 强调曲线 320ms 收回（±10ms 窗即动画时长窗）；
//! - 120fps 无文字模糊：滚动偏移按 1/4 像素定点积分（Q4），渲染前
//!   `snap_qpx` 吸附整数像素——亚像素位移只在渲染边界取整一次；
//! - 列表虚拟化 `visible_range`：每帧只处理可视 ± 缓冲行，
//!   帧成本模型入自检（≪8.33ms）。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检面）、
//! `crate::checks::{push_str, push_usize}`（位置提示文案）、
//! `crate::h1star::h1base::{Curve, MotionPolicy}`（F124 弹性/强调曲线）、
//! `crate::star::sbase::{KnobReg, RingLog}`（旋钮化/帧账本）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::{push_str, push_usize, CheckSet};
use crate::h1star::h1base::{Curve, MotionPolicy, Rect};
use crate::star::sbase::{KnobReg, RingLog};

// ---------------------------------------------------------------------------
// 规格常量（全部主册 F204 原文数值；ScrollTuning 是它们的唯一集合点）
// ---------------------------------------------------------------------------

/// 滚轮默认行/齿——主册 F204：「滚轮默认 3 行/齿」。
pub const WHEEL_LINES_DEF: i64 = 3;
/// 滚轮可调下界——主册 F204：「可调 1-10」。
pub const WHEEL_LINES_MIN: i64 = 1;
/// 滚轮可调上界——主册 F204：「可调 1-10」。
pub const WHEEL_LINES_MAX: i64 = 10;

/// 缺省行高（px，行↔像素换算基准；旋钮可调，实装定值）。
pub const LINE_HEIGHT_DEF_PX: i32 = 20;

/// 触控板惯性收尾时长——主册 F204：「触控板惯性曲线用 F124『弹性』档
/// （320ms 收尾）」。
pub const INERTIA_MS: u32 = 320;

/// 滚动条常态宽（px，实装定值）。
pub const BAR_W_DEF_PX: i32 = 4;
/// 滚动条悬停加粗——主册 F204：「滚动条悬停加粗至 8px」。
pub const BAR_HOVER_PX: i32 = 8;

/// 顶/底回弹过冲——主册 F204：「列表滚到顶/底有 8px 回弹」。
pub const OVERSCROLL_PX: i32 = 8;
/// 回弹动画时长——主册 F204：「回弹动画时长实测 320ms±10ms」。
pub const BOUNCE_MS: u32 = 320;

/// 120fps 帧预算（8.33ms 取整）。
pub const FRAME_120_NS: u64 = 8_000_000;

/// 1/4 像素定点（Q4）——滚动积分粒度。
pub const Q4: i32 = 4;

// ---------------------------------------------------------------------------
// ScrollTuning —— 四通路参数的单一定义点
// ---------------------------------------------------------------------------

/// 滚动调参集合：四条通路读这里，别处不得私设参数（一处一事实）。
#[derive(Clone, Copy, Debug)]
pub struct ScrollTuning {
    /// 滚轮行/齿（1-10）。
    pub wheel_lines: i64,
    /// 行高（px）。
    pub line_height_px: i32,
    /// 触控板惯性收尾（ms，F124 弹性档）。
    pub inertia_ms: u32,
    /// 滚动条悬停宽（px）。
    pub bar_hover_px: i32,
    /// 顶/底回弹过冲（px）。
    pub overscroll_px: i32,
    /// 回弹时长（ms）。
    pub bounce_ms: u32,
}

impl ScrollTuning {
    pub const fn from_consts() -> ScrollTuning {
        ScrollTuning {
            wheel_lines: WHEEL_LINES_DEF,
            line_height_px: LINE_HEIGHT_DEF_PX,
            inertia_ms: INERTIA_MS,
            bar_hover_px: BAR_HOVER_PX,
            overscroll_px: OVERSCROLL_PX,
            bounce_ms: BOUNCE_MS,
        }
    }

    /// 设滚轮行/齿（钳制入 1-10 档）。
    pub fn set_wheel_lines(&mut self, v: i64) {
        self.wheel_lines = v.clamp(WHEEL_LINES_MIN, WHEEL_LINES_MAX);
    }

    /// 整体旋钮化：六个参数全部注册进旋钮表（声明幂等）。
    pub fn declare_knobs(&self, reg: &mut KnobReg) {
        reg.declare("f204.wheel_lines", WHEEL_LINES_DEF, WHEEL_LINES_MIN, WHEEL_LINES_MAX, "行", "滚轮行/齿（F204 可调 1-10）");
        reg.declare("f204.line_height_px", LINE_HEIGHT_DEF_PX as i64, 8, 64, "px", "行高（F204 行↔px 换算基准）");
        reg.declare("f204.inertia_ms", INERTIA_MS as i64, 160, 640, "ms", "触控板惯性收尾（F204 弹性 320ms）");
        reg.declare("f204.bar_hover_px", BAR_HOVER_PX as i64, 4, 16, "px", "滚动条悬停加粗（F204 8px）");
        reg.declare("f204.overscroll_px", OVERSCROLL_PX as i64, 0, 24, "px", "顶/底回弹过冲（F204 8px）");
        reg.declare("f204.bounce_ms", BOUNCE_MS as i64, 160, 640, "ms", "回弹时长（F204 320ms±10ms）");
    }

    /// 从旋钮表读回（缺省回退常量——旋钮表未装载时四通路仍有合法参数）。
    pub fn from_knobreg(reg: &KnobReg) -> ScrollTuning {
        ScrollTuning {
            wheel_lines: reg.get_or("f204.wheel_lines", WHEEL_LINES_DEF),
            line_height_px: reg.get_or("f204.line_height_px", LINE_HEIGHT_DEF_PX as i64) as i32,
            inertia_ms: reg.get_or("f204.inertia_ms", INERTIA_MS as i64) as u32,
            bar_hover_px: reg.get_or("f204.bar_hover_px", BAR_HOVER_PX as i64) as i32,
            overscroll_px: reg.get_or("f204.overscroll_px", OVERSCROLL_PX as i64) as i32,
            bounce_ms: reg.get_or("f204.bounce_ms", BOUNCE_MS as i64) as u32,
        }
    }
}

impl Default for ScrollTuning {
    fn default() -> Self {
        Self::from_consts()
    }
}

// ---------------------------------------------------------------------------
// 四通路输入归一器
// ---------------------------------------------------------------------------

/// 四通路输入（主册：滚轮、触控板、滚动条、键盘）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollInput {
    /// 滚轮齿数（含方向）。
    WheelNotches(i32),
    /// 触控板像素位移（含方向，直通）。
    TrackpadPx(i32),
    /// 滚动条目标比例（×1000，0..=1000 → 绝对目标）。
    BarRatio(i32),
    /// 键盘行数（方向键 ±1、PageUp/Down ±页）。
    KeyLines(i32),
}

/// 归一结果：增量位移；滚动条通路给绝对目标（target_px）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollMotion {
    pub delta_px: i32,
    pub target_px: Option<i32>,
}

/// 归一到统一速度域（像素）：四通路各自换算、同一出口。
pub fn normalize(t: &ScrollTuning, input: ScrollInput, content_px: i32) -> ScrollMotion {
    match input {
        ScrollInput::WheelNotches(n) => {
            ScrollMotion { delta_px: n * t.wheel_lines as i32 * t.line_height_px, target_px: None }
        }
        ScrollInput::TrackpadPx(p) => ScrollMotion { delta_px: p, target_px: None },
        ScrollInput::BarRatio(r) => {
            let r = r.clamp(0, 1000);
            let tgt = (r as i64 * content_px.max(0) as i64 / 1000) as i32;
            ScrollMotion { delta_px: 0, target_px: Some(tgt) }
        }
        ScrollInput::KeyLines(n) => {
            ScrollMotion { delta_px: n * t.line_height_px, target_px: None }
        }
    }
}

// ---------------------------------------------------------------------------
// 惯性滚动模型（F124 弹性档 320ms 收尾）
// ---------------------------------------------------------------------------

/// 惯性滚动：以初速起步，按 F124 弹性曲线的进度增量释放总位移，
/// 320ms 收尾（曲线自带 overshoot→回弹形状）。
pub struct InertiaScroll {
    total_px: i64,
    start_ms: u64,
    released_px: i64,
}

impl InertiaScroll {
    /// 初速 v0（px/s）→ 320ms 内的总位移（匀速等效）。
    pub fn start(v0_px_s: i32, now_ms: u64) -> InertiaScroll {
        InertiaScroll {
            total_px: v0_px_s as i64 * INERTIA_MS as i64 / 1000,
            start_ms: now_ms,
            released_px: 0,
        }
    }

    /// 逐帧释放（调用方每帧注入当前时刻）。返回本帧位移。
    pub fn frame(&mut self, now_ms: u64) -> i32 {
        let pol = MotionPolicy::normal();
        let t = now_ms.saturating_sub(self.start_ms).min(INERTIA_MS as u64) as u32;
        let p = pol.progress(Curve::Spring, t, 0) as i64;
        let target = self.total_px * p / 1000;
        let d = target - self.released_px;
        self.released_px = target;
        d as i32
    }

    /// 是否已收尾。
    pub fn done(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.start_ms) >= INERTIA_MS as u64
    }

    /// 已释放累计位移。
    pub fn released(&self) -> i64 {
        self.released_px
    }

    /// 总位移。
    pub fn total(&self) -> i64 {
        self.total_px
    }
}

// ---------------------------------------------------------------------------
// 顶/底回弹状态机（8px 过冲 + 强调曲线 320ms 收回）
// ---------------------------------------------------------------------------

/// 回弹相位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BouncePhase {
    Rest,
    Overshot,
    Rebounding,
}

/// 顶/底回弹状态机：越界滚动吸收为过冲（最多 8px），settle 按
/// F124 强调曲线 320ms 收回 0。
pub struct EdgeBounce {
    overshoot_px: i32,
    dir: i32,
    phase: BouncePhase,
    rb_start: u64,
    rb_from: i32,
}

impl EdgeBounce {
    pub fn new() -> EdgeBounce {
        EdgeBounce { overshoot_px: 0, dir: 0, phase: BouncePhase::Rest, rb_start: 0, rb_from: 0 }
    }

    /// 过冲偏移（带方向：顶边过冲为负、底边为正——渲染叠加用）。
    pub fn offset_signed(&self) -> i32 {
        self.overshoot_px * self.dir
    }

    pub fn phase(&self) -> BouncePhase {
        self.phase
    }

    /// 一步滚动：`at_start`/`at_end` 为当前是否已在顶/底。
    /// 返回实际生效的内容滚动量——越界部分被过冲吸收，不直接生效（回 0）；
    /// 反向滚动先把过冲吃回来，吃回的部分如实生效。
    pub fn on_scroll(&mut self, delta_px: i32, at_start: bool, at_end: bool) -> i32 {
        if self.overshoot_px > 0 {
            if delta_px == 0 || delta_px.signum() == self.dir {
                // 同向：过冲已满（≤8px），继续同向滚动全部吸收。
                return 0;
            }
            // 反向：先把过冲吃回来。
            let back = delta_px.abs().min(self.overshoot_px);
            self.overshoot_px -= back;
            if self.overshoot_px == 0 {
                self.phase = BouncePhase::Rest;
                self.dir = 0;
            }
            return delta_px.signum() * back;
        }
        if (at_start && delta_px < 0) || (at_end && delta_px > 0) {
            let dir = if delta_px < 0 { -1 } else { 1 };
            let room = (OVERSCROLL_PX - self.overshoot_px).max(0);
            let over = delta_px.abs().min(room);
            self.overshoot_px += over;
            self.dir = dir;
            self.phase = BouncePhase::Overshot;
            return 0;
        }
        delta_px
    }

    /// 每帧调用的收尾步：过冲态进入回弹，回弹态按强调曲线收回 0。
    /// 返回当前过冲偏移（带方向）。
    pub fn settle(&mut self, now_ms: u64) -> i32 {
        match self.phase {
            BouncePhase::Overshot => {
                self.phase = BouncePhase::Rebounding;
                self.rb_start = now_ms;
                self.rb_from = self.overshoot_px;
                self.offset_signed()
            }
            BouncePhase::Rebounding => {
                let pol = MotionPolicy::normal();
                let t = now_ms.saturating_sub(self.rb_start).min(BOUNCE_MS as u64) as u32;
                let p = pol.progress(Curve::Emphasis, t, 0) as i32;
                self.overshoot_px = self.rb_from * (1000 - p) / 1000;
                if p >= 1000 || self.overshoot_px == 0 {
                    self.overshoot_px = 0;
                    self.phase = BouncePhase::Rest;
                    self.dir = 0;
                }
                self.offset_signed()
            }
            BouncePhase::Rest => 0,
        }
    }
}

impl Default for EdgeBounce {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 渲染辅助：像素吸附 / 虚拟化裁剪 / 滚动条 / 位置提示
// ---------------------------------------------------------------------------

/// Q4 定点偏移 → 整数像素（四舍五入）。120fps 无文字模糊的落点：
/// 积分带亚像素精度，渲染恒为整数像素。
pub fn snap_qpx(v_q4: i32) -> i32 {
    (v_q4 + Q4 / 2) >> 2
}

/// 可视项范围（列表虚拟化：只处理可视 ± 1 行缓冲）。
pub fn visible_range(scroll_px: i32, viewport_px: i32, item_h: i32, total: usize) -> (usize, usize) {
    if item_h <= 0 || total == 0 {
        return (0, 0);
    }
    let first = (scroll_px.max(0) / item_h) as usize;
    let visible = (viewport_px.max(0) / item_h) as usize + 2;
    let lo = first.min(total);
    let hi = (first + visible).min(total);
    (lo, hi)
}

/// 滚动条宽度：常态 4px，悬停/拖动加粗至 8px（主册判据）。
pub fn bar_width(hover: bool, dragging: bool) -> i32 {
    if hover || dragging {
        BAR_HOVER_PX
    } else {
        BAR_W_DEF_PX
    }
}

/// 位置提示文案「第 N 项/共 M 项」写入 `out`（拖动滚动条时显示，
/// 主册 F204；N 从 1 计）。返回字节数。
pub fn position_hint(top_item: usize, total: usize, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    push_str(out, &mut n, "第 ");
    push_usize(out, &mut n, top_item.saturating_add(1));
    push_str(out, &mut n, " 项/共 ");
    push_usize(out, &mut n, total);
    push_str(out, &mut n, " 项");
    n
}

// ---------------------------------------------------------------------------
// 帧账本（120fps 判据的入账面）
// ---------------------------------------------------------------------------

/// 单帧账目。
#[derive(Clone, Copy, Debug)]
pub struct FrameRec {
    /// 本帧渲染吸附后的整数偏移（无亚像素 → 无模糊的证明字段）。
    pub snap_px: i32,
    /// 本帧处理成本（ns，成本模型）。
    pub cost_ns: u64,
}

/// 滚动帧账本：最近 64 帧 + 超预算计数。
pub struct ScrollPerf {
    ring: RingLog<FrameRec, 64>,
    pub frames: u32,
    pub over_budget: u32,
}

impl ScrollPerf {
    pub fn new() -> ScrollPerf {
        ScrollPerf { ring: RingLog::new(), frames: 0, over_budget: 0 }
    }

    /// 记一帧（成本由调用方按 `visible_range` 处理面用成本模型算出）。
    pub fn record_frame(&mut self, snap_px: i32, cost_ns: u64) {
        if cost_ns > FRAME_120_NS {
            self.over_budget += 1;
        }
        self.ring.push(FrameRec { snap_px, cost_ns });
        self.frames += 1;
    }

    pub fn recent(&self) -> alloc::vec::Vec<FrameRec> {
        self.ring.newest_first()
    }
}

impl Default for ScrollPerf {
    fn default() -> Self {
        Self::new()
    }
}

/// 每帧成本模型（ns）：可视项处理 + 固定开销。
pub fn frame_cost_ns(visible_items: usize) -> u64 {
    visible_items as u64 * 40 + 200
}

// ---------------------------------------------------------------------------
// 滚轮平滑驱动 / 滚动条拖拽映射 / 键盘滚动语义 / 参数体检
// ---------------------------------------------------------------------------

/// 滚轮平滑收尾时长——F124 进入曲线 120ms（实装定值）。
pub const SMOOTH_ENTER_MS: u64 = 120;

/// 滚轮平滑滚动驱动：滚一齿 → 目标位移累加，帧间按 F124 进入曲线
/// 推进到位。连滚不断档（每次 nudge 从当前位点重启动曲线）。
/// 位置以 Q4 定点积分，渲染端 [`snap_qpx`] 吸附——无亚像素模糊。
pub struct SmoothWheel {
    pos_q4: i64,
    from_q4: i64,
    target_q4: i64,
    start_ms: u64,
    active: bool,
}

impl SmoothWheel {
    pub fn new() -> SmoothWheel {
        SmoothWheel { pos_q4: 0, from_q4: 0, target_q4: 0, start_ms: 0, active: false }
    }

    /// 滚一齿（delta_px 为本次齿的像素位移）。
    pub fn nudge(&mut self, delta_px: i32, now: u64) {
        self.from_q4 = self.pos_q4;
        self.start_ms = now;
        self.active = true;
        self.target_q4 += delta_px as i64 * Q4 as i64;
    }

    /// 每帧推进（时间注入）。返回本帧后的整数像素位置（已吸附）。
    pub fn frame(&mut self, now: u64) -> i32 {
        if !self.active {
            return snap_qpx(self.pos_q4.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
        }
        let pol = MotionPolicy::normal();
        let t = now.saturating_sub(self.start_ms).min(SMOOTH_ENTER_MS) as u32;
        let p = pol.progress(Curve::Enter, t, 0) as i64;
        self.pos_q4 = self.from_q4 + (self.target_q4 - self.from_q4) * p / 1000;
        if now.saturating_sub(self.start_ms) >= SMOOTH_ENTER_MS {
            self.pos_q4 = self.target_q4;
            self.active = false;
        }
        snap_qpx(self.pos_q4.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
    }

    pub fn settled(&self, now: u64) -> bool {
        !self.active || now.saturating_sub(self.start_ms) >= SMOOTH_ENTER_MS
    }

    pub fn target_px(&self) -> i64 {
        self.target_q4 / Q4 as i64
    }
}

impl Default for SmoothWheel {
    fn default() -> Self {
        Self::new()
    }
}

/// 指针 y 在轨道内的滚动比例（×1000，越界钳制）。
pub fn bar_ratio_at(track: Rect, pointer_y: i32) -> i32 {
    if track.h <= 1 {
        return 0;
    }
    (((pointer_y - track.y) as i64 * 1000 / (track.h - 1) as i64).clamp(0, 1000)) as i32
}

/// 滚动条拖拽映射：抓取时保存「指针 − 滑块顶」的比例差，
/// 拖动全程差值不变——滑块不跳变、手感连续。
pub struct BarDrag {
    grab_offset_ratio: i64,
}

/// 开始拖拽：`thumb_y` 为滑块顶、`thumb_h` 滑块高、`pointer_y` 指针。
pub fn begin_bar_drag(track: Rect, thumb_y: i32, thumb_h: i32, pointer_y: i32) -> BarDrag {
    let _ = thumb_h;
    let p = bar_ratio_at(track, pointer_y) as i64;
    let t = bar_ratio_at(track, thumb_y) as i64;
    BarDrag { grab_offset_ratio: p - t }
}

impl BarDrag {
    /// 拖动中：指针 y → 当前滚动比例（×1000，钳 0..=1000）。
    pub fn update(&self, track: Rect, pointer_y: i32) -> i32 {
        (bar_ratio_at(track, pointer_y) as i64 - self.grab_offset_ratio).clamp(0, 1000) as i32
    }
}

/// 键盘滚动语义（方向键/翻页/Home/End）→ 统一输入映射。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyScroll {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
    Home,
    End,
}

/// 键盘语义落进四通路归一器（同一域、同一参数来源）。
pub fn key_scroll(t: &ScrollTuning, k: KeyScroll, viewport_px: i32) -> ScrollInput {
    let lh = t.line_height_px.max(1);
    match k {
        KeyScroll::LineUp => ScrollInput::KeyLines(-1),
        KeyScroll::LineDown => ScrollInput::KeyLines(1),
        KeyScroll::PageUp => {
            ScrollInput::KeyLines(-((viewport_px / lh).max(1) as i32))
        }
        KeyScroll::PageDown => {
            ScrollInput::KeyLines((viewport_px / lh).max(1) as i32)
        }
        KeyScroll::Home => ScrollInput::BarRatio(0),
        KeyScroll::End => ScrollInput::BarRatio(1000),
    }
}

impl ScrollTuning {
    /// 参数体检：逻辑一致性显性校验（拒绝静默修正；档位数值以旋钮表为准）。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.wheel_lines < WHEEL_LINES_MIN || self.wheel_lines > WHEEL_LINES_MAX {
            return Err("wheel lines out of 1..=10 (F204)");
        }
        if self.line_height_px <= 0 {
            return Err("line height must be positive");
        }
        if self.bar_hover_px < BAR_W_DEF_PX {
            return Err("hover width below resting width");
        }
        if self.overscroll_px < 0 {
            return Err("overscroll must be non-negative");
        }
        if self.bounce_ms == 0 {
            return Err("bounce duration must be positive");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F204 自检（判据面：单一定义点 + 四通路归一 + 弹性 320ms + 8px 回弹 + 无模糊）。
pub fn run_scrolluni_checks() -> CheckSet {
    let mut set = CheckSet::new("F204-scrolluni");
    let t = ScrollTuning::from_consts();

    // 1. 单一定义点：常量集合点字段与主册数值一致；旋钮化后可读回。
    let mut reg = KnobReg::new();
    t.declare_knobs(&mut reg);
    let back = ScrollTuning::from_knobreg(&reg);
    set.add(
        "single source of truth + knob roundtrip",
        t.wheel_lines == WHEEL_LINES_DEF
            && t.inertia_ms == INERTIA_MS
            && t.bar_hover_px == BAR_HOVER_PX
            && t.overscroll_px == OVERSCROLL_PX
            && t.bounce_ms == BOUNCE_MS
            && reg.version() >= 6
            && back.wheel_lines == t.wheel_lines
            && back.bounce_ms == t.bounce_ms,
        "",
    );

    // 2. 滚轮行/齿钳制 1-10（0→1、99→10）。
    let mut t2 = ScrollTuning::from_consts();
    t2.set_wheel_lines(0);
    let lo = t2.wheel_lines;
    t2.set_wheel_lines(99);
    set.add("wheel lines clamp 1..=10", lo == 1 && t2.wheel_lines == 10, "");

    // 3. 滚轮/键盘归一：1 齿 ×3 行 ×20px = 60px；键盘 3 行同值。
    let m1 = normalize(&t, ScrollInput::WheelNotches(1), 0);
    let m2 = normalize(&t, ScrollInput::KeyLines(3), 0);
    set.add(
        "wheel 3 lines x 20px = 60px; keys same domain",
        m1.delta_px == 60 && m2.delta_px == 60 && m1.target_px.is_none(),
        "",
    );

    // 4. 触控板直通；滚动条 500‰ × 2000px = 1000px 绝对目标。
    let m3 = normalize(&t, ScrollInput::TrackpadPx(-45), 0);
    let m4 = normalize(&t, ScrollInput::BarRatio(500), 2000);
    set.add(
        "trackpad passthrough; bar ratio to target",
        m3.delta_px == -45 && m4.target_px == Some(1000),
        "",
    );

    // 5. 惯性走 F124 弹性档：时长恰 320ms，逐帧积分收尾总量精确。
    let mut inertia = InertiaScroll::start(1000, 0);
    let dur = MotionPolicy::normal().duration_ms(Curve::Spring, 0);
    let mut ms = 0u32;
    while ms < 400 {
        let _ = inertia.frame(ms as u64);
        ms += 17;
    }
    set.add(
        "inertia spring 320ms, integral converges",
        dur == 320 && inertia.done(320) && inertia.released() == inertia.total(),
        "",
    );

    // 6. 惯性帧位移有界（弹性档最大步进斜率 ≈5.5×帧步 → 单帧 ≤ 总量 30%）。
    let mut inertia2 = InertiaScroll::start(1000, 0);
    let mut bounded = true;
    for k in 0..20u32 {
        let d = inertia2.frame(k as u64 * 17);
        if d.abs() > (inertia2.total() * 30 / 100).max(1) as i32 {
            bounded = false;
        }
    }
    set.add("inertia per-frame release bounded", bounded, "");

    // 7. 回弹：越界滚动全被吸收为过冲（内容不动，钳 8px），松手 320ms
    //    内收回 0（±10ms 窗：310ms 处已收完——独立状态机实测）。
    let mut eb = EdgeBounce::new();
    let got = eb.on_scroll(-50, true, false);
    let o0 = eb.settle(1_000);
    let o_mid = eb.settle(1_160);
    let o_end = eb.settle(1_320);
    let mut eb2 = EdgeBounce::new();
    let _ = eb2.on_scroll(-50, true, false);
    let _ = eb2.settle(1_000);
    let o_310 = eb2.settle(1_310);
    set.add(
        "overscroll capped 8px, rebound 320ms window",
        got == 0 && eb.offset_signed() == 0 && o0 != 0 && o_mid != 0 && o_end == 0 && o_310 == 0,
        "",
    );

    // 8. 位置提示格式：拖动滚动条显示「第 3 项/共 100 项」。
    let mut buf = [0u8; 48];
    let n = position_hint(2, 100, &mut buf);
    set.add(
        "position hint text format",
        &buf[..n] == "第 3 项/共 100 项".as_bytes(),
        "",
    );

    // 9. 像素吸附：Q4 定点 fuzz 2000 轮——吸附误差 ≤ 半像素（无亚像素残留）。
    let mut x: u32 = 0x9E3779B9;
    let mut snap_ok = true;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let v = (x % 400_001) as i32 - 200_000; // ±50000px 的 Q4 值
        let s = snap_qpx(v);
        if (s * Q4 - v).abs() > Q4 / 2 {
            snap_ok = false;
        }
        if s * Q4 % Q4 != 0 {
            snap_ok = false;
        }
    }
    set.add("pixel snap fuzz 2000 rounds <= half px", snap_ok, "");

    // 10. 虚拟化裁剪：视口 400px / 行高 20 → 可视 20 行 + 2 缓冲；滚程平移。
    let (a, b) = visible_range(0, 400, 20, 1000);
    let (c, d) = visible_range(2000, 400, 20, 1000);
    set.add(
        "virtualized visible range culls",
        (a, b) == (0, 22) && (c, d) == (100, 122),
        "",
    );

    // 11. 滚动条：常态 4px、悬停加粗至 8px、拖动中保持 8px。
    set.add(
        "scrollbar hover widens to 8px",
        bar_width(false, false) == BAR_W_DEF_PX
            && bar_width(true, false) == BAR_HOVER_PX
            && bar_width(false, true) == BAR_HOVER_PX,
        "",
    );

    // 12. 120fps 帧成本：虚拟化后每帧只碰 ~22 项，成本 ≪ 8.33ms。
    set.add(
        "frame cost far below 120fps budget",
        frame_cost_ns(22) < FRAME_120_NS,
        "",
    );

    // 13. 帧账本：超预算帧如实计数（9ms 两帧 → over_budget=2）。
    //     缺陷账本：现象=计数恒 0、检查项红；根因=检查项样本写错量纲
    //     （9_000 ns = 9µs，远低于主册 120fps 帧预算 8.33ms，本就不该
    //     计超支——实现 record_frame 按 FRAME_120_NS 判定是对的）；修法=
    //     样本改为 9_000_000 ns（9ms > 8ms 预算）与 1_000_000 ns（不超支
    //     对照），断言数值与判据不变。
    let mut perf = ScrollPerf::new();
    perf.record_frame(12, 9_000_000);
    perf.record_frame(13, 9_000_000);
    perf.record_frame(14, 1_000_000);
    set.add(
        "scroll perf ledger counts overruns",
        perf.frames == 3 && perf.over_budget == 2 && perf.recent().len() == 3,
        "",
    );

    // 14. 旋钮留痕：改滚轮行数入变更日志（参数面可审计）。
    reg.set("f204.wheel_lines", 5, 42);
    let log = reg.change_log();
    set.add(
        "knob change leaves audit trail",
        reg.get_or("f204.wheel_lines", 0) == 5 && log.len() == 1 && log[0].to == 5,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_all_four_paths() {
        let t = ScrollTuning::from_consts();
        assert_eq!(normalize(&t, ScrollInput::WheelNotches(-2), 0).delta_px, -120);
        assert_eq!(normalize(&t, ScrollInput::TrackpadPx(37), 0).delta_px, 37);
        assert_eq!(normalize(&t, ScrollInput::KeyLines(-1), 0).delta_px, -20);
        let bar = normalize(&t, ScrollInput::BarRatio(1001), 900);
        assert_eq!(bar.target_px, Some(900), "比例越界钳 1000‰");
    }

    #[test]
    fn inertia_frame_by_frame_sum() {
        let mut inertia = InertiaScroll::start(-800, 0);
        let mut sum = 0i64;
        for k in 0..25u32 {
            sum += inertia.frame(k as u64 * 17) as i64;
        }
        assert_eq!(sum, inertia.total(), "逐帧释放之和 = 总位移");
        assert_eq!(inertia.total(), -256, "-800px/s × 320ms");
        assert!(inertia.done(320));
    }

    #[test]
    fn bounce_full_cycle() {
        let mut eb = EdgeBounce::new();
        // 未在边界时滚动直通。
        assert_eq!(eb.on_scroll(30, false, false), 30);
        // 底边越界：内容不动（20px 全部越界），其中 8px 转为过冲。
        assert_eq!(eb.on_scroll(20, false, true), 0);
        assert_eq!(eb.offset_signed(), 8);
        assert_eq!(eb.on_scroll(50, false, true), 0, "过冲已满，同向滚动全部吸收");
        // 回弹全过程收回 0。
        let _ = eb.settle(0);
        let mut last = 8;
        for k in 1..=22u32 {
            last = eb.settle(k as u64 * 17);
        }
        assert_eq!(last, 0);
        assert_eq!(eb.phase(), BouncePhase::Rest);
        // 反向滚动把过冲吃回来。
        let mut eb2 = EdgeBounce::new();
        let _ = eb2.on_scroll(-6, true, false);
        assert_eq!(eb2.offset_signed(), -6);
        assert_eq!(eb2.on_scroll(4, true, false), 4);
        assert_eq!(eb2.offset_signed(), -2);
    }

    #[test]
    fn tuning_knob_drift_and_reset() {
        let mut reg = KnobReg::new();
        let t = ScrollTuning::from_consts();
        t.declare_knobs(&mut reg);
        assert_eq!(reg.set("f204.wheel_lines", 50, 1), Some(10), "越界钳到档位上界");
        let t2 = ScrollTuning::from_knobreg(&reg);
        assert_eq!(t2.wheel_lines, 10);
        assert_eq!(reg.reset("f204.wheel_lines", 2), Some(3));
        assert_eq!(ScrollTuning::from_knobreg(&reg).wheel_lines, 3);
    }

    #[test]
    fn snap_negative_values_round_consistently() {
        assert_eq!(snap_qpx(6), 2); // 1.5px → 2
        assert_eq!(snap_qpx(-6), -1); // -1.5px → -1（floor 系，半像素内守恒）
        assert_eq!(snap_qpx(0), 0);
        assert_eq!(snap_qpx(4), 1);
    }

    #[test]
    fn hint_writes_into_small_buffer() {
        let mut buf = [0u8; 12];
        let n = position_hint(9, 100_000, &mut buf);
        assert!(n <= buf.len());
        // 缓冲截断但不出界：恰写满 12 字节。
        assert_eq!(n, 12);
        assert_eq!(&buf[..n], &"第 10 项/共 ".as_bytes()[..12]);
    }

    #[test]
    fn smooth_wheel_converges_and_snaps() {
        let mut w = SmoothWheel::new();
        w.nudge(60, 0); // 3 行 × 20px
        let mut last = 0i32;
        for k in 0..30u32 {
            last = w.frame(k as u64 * 10);
        }
        assert_eq!(last, 60, "120ms 后精确到位并吸附整数像素");
        assert!(w.settled(300));
        // 连滚：从当前位点重启曲线继续累加目标。
        w.nudge(-20, 310);
        let mut fin = last;
        for k in 0..30u32 {
            fin = w.frame(310 + k as u64 * 10);
        }
        assert_eq!(fin, 40, "目标位 = 60 - 20");
        assert_eq!(w.target_px(), 40);
    }

    #[test]
    fn bar_drag_keeps_grab_offset() {
        let track = Rect::new(0, 0, 16, 1000);
        // 滑块顶在 30% 处，指针按在 35% → 差 5%。
        let thumb_y = 300;
        let drag = begin_bar_drag(track, thumb_y, 100, 350);
        assert_eq!(drag.update(track, 350), 300, "指针未动 → 比例不变（不跳变）");
        assert_eq!(drag.update(track, 450), 400);
        assert_eq!(drag.update(track, 10_000), 950, "越界钳满程（抓取差恒保持）");
        assert_eq!(drag.update(track, -5), 0);
        assert_eq!(bar_ratio_at(track, 500), 500);
    }

    #[test]
    fn key_scroll_maps_to_unified_domain() {
        let t = ScrollTuning::from_consts();
        assert_eq!(
            normalize(&t, key_scroll(&t, KeyScroll::LineDown, 400), 0).delta_px,
            20
        );
        assert_eq!(
            normalize(&t, key_scroll(&t, KeyScroll::PageDown, 400), 0).delta_px,
            400
        );
        assert_eq!(
            normalize(&t, key_scroll(&t, KeyScroll::Home, 400), 9000).target_px,
            Some(0)
        );
        assert_eq!(
            normalize(&t, key_scroll(&t, KeyScroll::End, 400), 9000).target_px,
            Some(9000)
        );
    }

    #[test]
    fn tuning_validate_rejects_bad_configs() {
        let mut t = ScrollTuning::from_consts();
        assert!(t.validate().is_ok());
        t.wheel_lines = 0;
        assert!(t.validate().is_err());
        let mut t2 = ScrollTuning::from_consts();
        t2.line_height_px = 0;
        assert!(t2.validate().is_err());
        let mut t3 = ScrollTuning::from_consts();
        t3.bar_hover_px = 2;
        assert!(t3.validate().is_err());
    }

    #[test]
    fn scrolluni_selfcheck_all_green() {
        let set = run_scrolluni_checks();
        assert!(set.all_passed(), "F204 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

const VXH1_MAGIC: [u8; 4] = *b"VXH1";
const VXH1_VER: u8 = 1;

/// 损坏输入显性拒绝：四类 + 字段越界（参数越过 F204 档位界）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2CodecErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadSum,
    BadField,
}

/// FNV-1a 32 位（校验和唯一实现点）。
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ---- 持久化 I/O 面：四通路参数表档案（单一定义点的落盘面）----

/// 记录长：magic4 + ver1 + 6×i32（wheel/行高/惯性/悬停宽/过冲/回弹）+ sum4。
pub const TUNING_REC_LEN: usize = 4 + 1 + 24 + 4;

/// 四通路参数档案：与 ScrollTuning 同字段的字节级台账——四条通路
/// 参数的持久化出口（解码后仍只此一份，通路不私设）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TuningRec {
    pub wheel_lines: i32,
    pub line_height_px: i32,
    pub inertia_ms: u32,
    pub bar_hover_px: i32,
    pub overscroll_px: i32,
    pub bounce_ms: u32,
}

impl TuningRec {
    /// 从运行态取档（ScrollTuning 字段公开，直取）。
    pub fn of(t: &ScrollTuning) -> TuningRec {
        TuningRec {
            wheel_lines: t.wheel_lines as i32,
            line_height_px: t.line_height_px,
            inertia_ms: t.inertia_ms,
            bar_hover_px: t.bar_hover_px,
            overscroll_px: t.overscroll_px,
            bounce_ms: t.bounce_ms,
        }
    }

    /// 还原进运行态（仍是四通路唯一参数来源）。
    pub fn into_tuning(self) -> ScrollTuning {
        ScrollTuning {
            wheel_lines: self.wheel_lines as i64,
            line_height_px: self.line_height_px,
            inertia_ms: self.inertia_ms,
            bar_hover_px: self.bar_hover_px,
            overscroll_px: self.overscroll_px,
            bounce_ms: self.bounce_ms,
        }
    }

    pub fn to_bytes(&self) -> [u8; TUNING_REC_LEN] {
        let mut out = [0u8; TUNING_REC_LEN];
        out[..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VXH1_VER;
        let vals = [
            self.wheel_lines,
            self.line_height_px,
            self.inertia_ms as i32,
            self.bar_hover_px,
            self.overscroll_px,
            self.bounce_ms as i32,
        ];
        for (k, v) in vals.iter().enumerate() {
            out[5 + k * 4..9 + k * 4].copy_from_slice(&v.to_le_bytes());
        }
        let body = 5 + 24;
        let sum = fnv1a(&out[..body]).to_le_bytes();
        out[body..body + 4].copy_from_slice(&sum);
        out
    }

    /// 解码：档位校验按主册 F204 判据（滚轮 1-10；行高/回弹为正）。
    pub fn from_bytes(b: &[u8]) -> Result<TuningRec, V2CodecErr> {
        if b.len() != TUNING_REC_LEN {
            return Err(V2CodecErr::BadLen);
        }
        let mut mg = [0u8; 4];
        mg.copy_from_slice(&b[..4]);
        if mg != VXH1_MAGIC {
            return Err(V2CodecErr::BadMagic);
        }
        if b[4] != VXH1_VER {
            return Err(V2CodecErr::BadVersion);
        }
        let body = 5 + 24;
        let mut sum = [0u8; 4];
        sum.copy_from_slice(&b[body..body + 4]);
        if fnv1a(&b[..body]) != u32::from_le_bytes(sum) {
            return Err(V2CodecErr::BadSum);
        }
        let mut v = [0i32; 6];
        for (k, slot) in v.iter_mut().enumerate() {
            let mut t = [0u8; 4];
            t.copy_from_slice(&b[5 + k * 4..9 + k * 4]);
            *slot = i32::from_le_bytes(t);
        }
        let rec = TuningRec {
            wheel_lines: v[0],
            line_height_px: v[1],
            inertia_ms: v[2] as u32,
            bar_hover_px: v[3],
            overscroll_px: v[4],
            bounce_ms: v[5] as u32,
        };
        let in_range = rec.wheel_lines >= WHEEL_LINES_MIN as i32
            && rec.wheel_lines <= WHEEL_LINES_MAX as i32
            && rec.line_height_px > 0
            && rec.bounce_ms > 0;
        if !in_range {
            return Err(V2CodecErr::BadField);
        }
        Ok(rec)
    }
}

// ---- UI 壳接线面：滚动条滑块几何 + 速度-模糊判定面 ----

/// 滚动条滑块矩形：thumb 高 = 轨高×视口/内容（最小 8px），位置随滚动
/// 量钳制。壳层命中测试与绘制共用此几何（一处一事实）。内容不超视口
/// （无滚程）时滑块占满轨道。
pub fn bar_thumb(track: Rect, scroll_px: i32, viewport_px: i32, content_px: i32) -> Rect {
    if content_px <= viewport_px || track.h <= 0 {
        return Rect::new(track.x, track.y, track.w.max(1), track.h.max(1));
    }
    let th = ((track.h as i64 * viewport_px as i64 / content_px as i64) as i32).clamp(8, track.h);
    let max_scroll = (content_px - viewport_px).max(1);
    let s = scroll_px.clamp(0, content_px - viewport_px);
    let ty = track.y + ((track.h - th) as i64 * s as i64 / max_scroll as i64) as i32;
    Rect::new(track.x, ty, track.w.max(1), th)
}

/// 速度 → 本帧 Q4 定点位移。120fps 无文字模糊判定面入口：配合
/// `snap_qpx` 每帧吸附整数像素，亚像素只活在积分域、渲染零残留。
pub fn frame_delta_q4(speed_px_s: i32, dt_ms: u32) -> i32 {
    (speed_px_s as i64 * dt_ms as i64 * Q4 as i64 / 1000) as i32
}

/// F204 v2 自检（首条恒为持久化 round-trip）。
pub fn run_scrolluni_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F204-scrolluni-v2");

    // 1. 持久化 round-trip：缺省四通路参数编码→解码→还原运行态逐字段同值。
    let t = ScrollTuning::from_consts();
    let rec = TuningRec::of(&t);
    let ok_rt = match TuningRec::from_bytes(&rec.to_bytes()) {
        Ok(r) => r == rec && r.into_tuning().wheel_lines == t.wheel_lines && r.bounce_ms == t.bounce_ms,
        Err(_) => false,
    };
    set.add("v2 persist roundtrip tuning rec", ok_rt, "");

    // 2. 损坏拒绝四类 + 字段越界（滚轮行数 0 违反 F204「可调 1-10」）。
    let bytes = rec.to_bytes();
    let mut bad1 = bytes;
    bad1[0] = b'X';
    let mut bad2 = bytes;
    bad2[4] = 9;
    let mut bad3 = bytes;
    bad3[5] ^= 0xFF;
    let mut bad4 = bytes;
    bad4[5] = 0; // wheel_lines = 0 → 越过 F204「可调 1-10」档（重算 sum 使只坏字段）
    let body4 = 5 + 24;
    let s4 = fnv1a(&bad4[..body4]);
    bad4[body4..body4 + 4].copy_from_slice(&s4.to_le_bytes());
    set.add(
        "v2 persist rejects corrupt tuning recs",
        TuningRec::from_bytes(&bad1) == Err(V2CodecErr::BadMagic)
            && TuningRec::from_bytes(&bad2) == Err(V2CodecErr::BadVersion)
            && TuningRec::from_bytes(&bad3) == Err(V2CodecErr::BadSum)
            && TuningRec::from_bytes(&bytes[..bytes.len() - 1]) == Err(V2CodecErr::BadLen)
            && TuningRec::from_bytes(&bad4) == Err(V2CodecErr::BadField),
        "",
    );

    // 3. 滑块几何：内容 2000/视口 400 → thumb 高 200；滚 0 与滚满分别
    //    贴轨道顶/底（验主册 F204 滚动条通路的壳层落点）。
    let track = Rect::new(790, 0, 10, 1000);
    let top = bar_thumb(track, 0, 400, 2000);
    let bottom = bar_thumb(track, 1600, 400, 2000);
    set.add(
        "v2 bar thumb geometry endpoints",
        top == Rect::new(790, 0, 10, 200) && bottom == Rect::new(790, 800, 10, 200),
        "",
    );

    // 4. 速度-模糊判定面：975px/s × 16ms 帧的 Q4 位移吸附误差 ≤ 半像素
    //    （验主册 F204「120fps 无文字模糊」的量化口径）。
    let d = frame_delta_q4(975, 16);
    set.add(
        "v2 frame delta snap error within half px",
        d == 62 && (snap_qpx(d) * Q4 - d).abs() <= Q4 / 2,
        "",
    );

    // 5. 四通路同源：解码还原的参数喂给归一器，与缺省参数产出同位移
    //    （验主册 F204「四通路参数单一定义点」档案面）。
    let back = TuningRec::from_bytes(&bytes);
    set.add(
        "v2 decoded tuning drives all four paths",
        back.map_or(false, |r| {
            let t2 = r.into_tuning();
            normalize(&t2, ScrollInput::WheelNotches(1), 0).delta_px == 60
                && normalize(&t2, ScrollInput::KeyLines(3), 0).delta_px == 60
        }),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn tuning_rec_codec_exact() {
        let rec = TuningRec::of(&ScrollTuning::from_consts());
        assert_eq!(rec.to_bytes().len(), TUNING_REC_LEN);
        assert_eq!(TuningRec::from_bytes(&rec.to_bytes()).unwrap(), rec);
    }

    #[test]
    fn bar_thumb_full_track_when_no_scroll_range() {
        let track = Rect::new(0, 0, 8, 100);
        assert_eq!(bar_thumb(track, 50, 400, 400), track, "无滚程 → 滑块占满");
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_scrolluni_v2_checks();
        assert!(set.all_passed(), "F204 v2 自检存在红项");
        assert!(!set.truncated());
        assert!((4..=6).contains(&set.len()));
    }
}
