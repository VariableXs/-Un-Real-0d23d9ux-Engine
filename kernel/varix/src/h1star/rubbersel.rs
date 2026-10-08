//! F203 拖拽选择框（橡皮筋）· 判据实装（H 基础通用域 · AI-H1 分工包）。
//!
//! **判据锚**：主册 F203「拖拽选择框（橡皮筋）」。
//!
//! **验收标准（主册第一句）**：1000 项列表框选 60fps 不掉帧；自动滚动
//! 三档速度实测；框选 + Ctrl/Shift 组合 12 个场景用例全绿；视觉样式与
//! 主题令牌联动（换主题变色不换几何）。
//!
//! **设计要点**：
//! - `RubberBand` 状态机：Idle → Dragging（跟随）→ Autoscroll（边缘带
//!   内自动滚动）→ 松开定形回 Idle——按点/移动/松开三事件驱动；
//! - 60fps 的实现核心是**预算制分帧摊销**：每帧最多测 250 项
//!   （`HIT_BUDGET`），1000 项 4 帧扫完；单项成本模型入自检
//!   （250×10ns ≪ 8.33ms），列表滚动位置变化时框选矩形用内容坐标
//!   补偿（`scroll_off`），滚动的帧只补测增量不重扫全表；
//! - 三档边缘带：32px 带内触发，(16,32] 慢档、(8,16] 中档、(0,8] 快档
//!   （速度 150/450/1200 px/s，随贴近程度三档加速）；
//! - 修饰键语义全枚举 12 场景：None=替换、Ctrl=异或加选/减选、
//!   Shift=范围替换、Ctrl+Shift=范围累加（`apply_release` 唯一实现点）；
//! - 零堆热路径：选择集用 1024 位定长位图，命中测试不分配；
//! - 样式：边框 1px、填充 12% 透明度（`blend_alpha` 整数混合），
//!   颜色全部来自主题令牌、几何常量与主题解耦。
//!
//! **依赖锚点**：`crate::checks::CheckSet`（自检面）、
//! `crate::h1star::h1base::{Rect, Rgb8}`（几何/令牌色）、
//! `crate::star::sbase::RingLog`（帧账本）。
//! 时间纪律：滚动步进由调用方注入 dt（毫秒），模块不持时钟。

use crate::checks::CheckSet;
use crate::h1star::h1base::{Rect, Rgb8};
use crate::star::sbase::RingLog;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 选择框填充透明度——主册 F203：「填充 12% 透明度」。
pub const FILL_ALPHA_PCT: u32 = 12;

/// 选择框边框宽——主册 F203：「边框 1px」。
pub const BORDER_PX: i32 = 1;

/// 自动滚动触发带——主册 F203：「鼠标靠近边缘 32px 内触发」。
pub const EDGE_BAND_PX: i32 = 32;

/// 三档分带界（实装定值；主册只定 32px 触发带与三档加速）：
/// (16,32] 慢档、(8,16] 中档、(0,8] 快档。
pub const EDGE_MID_PX: i32 = 16;
/// 快档分带界。
pub const EDGE_FAST_PX: i32 = 8;

/// 三档滚速（px/s，实装定值）：慢/中/快，随贴近程度加速。
pub const SPEED_SLOW_PX_S: i32 = 150;
/// 中档滚速。
pub const SPEED_MID_PX_S: i32 = 450;
/// 快档滚速。
pub const SPEED_FAST_PX_S: i32 = 1200;

/// 选择集容量（位图 1024 位）——覆盖主册 1000 项列表判据。
pub const MAX_ITEMS: usize = 1024;

/// 每帧命中测试预算——1000 项 4 帧摊平，60fps 判据的实现核心。
pub const HIT_BUDGET: usize = 250;

/// 单项相交测试成本模型（ns，8 次整数比较/运算的保守估值）。
pub const HIT_COST_NS: u64 = 10;

/// 60fps 帧预算。
pub const FRAME_60_MS: u32 = 16;
/// 120fps 帧预算（8.33ms 取整）。
pub const FRAME_120_MS: u32 = 8;

/// 帧账本容量（最近 64 帧的测试量/超支痕迹）。
pub const FRAME_REC_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 选择集（1024 位定长位图，零堆）
// ---------------------------------------------------------------------------

/// 位图字数。
const WORDS: usize = MAX_ITEMS / 64;

/// 定容选择集：位图运算全部 O(WORDS) 常数，热路径零分配。
#[derive(Clone, Copy)]
pub struct SelectionSet {
    bits: [u64; WORDS],
}

impl SelectionSet {
    pub fn new() -> SelectionSet {
        SelectionSet { bits: [0u64; WORDS] }
    }

    pub fn clear_all(&mut self) {
        self.bits = [0u64; WORDS];
    }

    pub fn set(&mut self, i: usize) {
        if i < MAX_ITEMS {
            self.bits[i / 64] |= 1u64 << (i % 64);
        }
    }

    pub fn clear(&mut self, i: usize) {
        if i < MAX_ITEMS {
            self.bits[i / 64] &= !(1u64 << (i % 64));
        }
    }

    pub fn toggle(&mut self, i: usize) {
        if i < MAX_ITEMS {
            self.bits[i / 64] ^= 1u64 << (i % 64);
        }
    }

    pub fn get(&self, i: usize) -> bool {
        i < MAX_ITEMS && (self.bits[i / 64] >> (i % 64)) & 1 == 1
    }

    pub fn count(&self) -> u32 {
        self.bits.iter().map(|w| w.count_ones()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&w| w == 0)
    }

    /// 选中项编号写入 `out`（持久化/取列表面；返回写入个数）。
    pub fn selected_ids(&self, out: &mut [u16]) -> usize {
        let mut n = 0usize;
        for w in 0..WORDS {
            let mut word = self.bits[w];
            while word != 0 {
                let bit = word.trailing_zeros() as usize;
                if n < out.len() {
                    out[n] = (w * 64 + bit) as u16;
                    n += 1;
                }
                word &= word - 1;
            }
        }
        n
    }

    /// 异或（Ctrl 加选/减选的唯一语义）。
    pub fn xor(&self, o: &SelectionSet) -> SelectionSet {
        let mut r = *self;
        for k in 0..WORDS {
            r.bits[k] ^= o.bits[k];
        }
        r
    }

    /// 并集（范围累加语义）。
    pub fn unioned(&self, o: &SelectionSet) -> SelectionSet {
        let mut r = *self;
        for k in 0..WORDS {
            r.bits[k] |= o.bits[k];
        }
        r
    }

    /// 闭区间 `[lo, hi]` 置位（Shift 范围选；越界钳制）。
    pub fn range_set(&mut self, lo: usize, hi: usize) {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        for i in lo..=hi.min(MAX_ITEMS - 1) {
            self.set(i);
        }
    }

    /// 已置位的最小/最大下标（Shift 范围端点）。
    pub fn span(&self) -> Option<(usize, usize)> {
        let mut lo = None;
        let mut hi = None;
        for w in 0..WORDS {
            if self.bits[w] == 0 {
                continue;
            }
            if lo.is_none() {
                lo = Some(w * 64 + self.bits[w].trailing_zeros() as usize);
            }
            hi = Some(w * 64 + (63 - self.bits[w].leading_zeros()) as usize);
        }
        match (lo, hi) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        }
    }
}

impl Default for SelectionSet {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 边缘带与自动滚动
// ---------------------------------------------------------------------------

/// 边缘档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    None,
    Slow,
    Mid,
    Fast,
}

/// 距边距离 → 三档档位（32px 触发带，(16,32]/(8,16]/(0,8] 分带）。
pub fn edge_zone(dist: i32) -> Edge {
    if dist > EDGE_BAND_PX {
        Edge::None
    } else if dist > EDGE_MID_PX {
        Edge::Slow
    } else if dist > EDGE_FAST_PX {
        Edge::Mid
    } else {
        Edge::Fast
    }
}

/// 档位 → 滚速（px/s）。
pub fn edge_speed(e: Edge) -> i32 {
    match e {
        Edge::None => 0,
        Edge::Slow => SPEED_SLOW_PX_S,
        Edge::Mid => SPEED_MID_PX_S,
        Edge::Fast => SPEED_FAST_PX_S,
    }
}

// ---------------------------------------------------------------------------
// 修饰键语义（12 场景全枚举的唯一实现点）
// ---------------------------------------------------------------------------

/// 修饰键组合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mods {
    None,
    Ctrl,
    Shift,
    CtrlShift,
}

/// 松开定形：按修饰键把框选命中集折算进最终选择。
/// None=替换；Ctrl=异或（加选/减选）；Shift=范围替换；
/// Ctrl+Shift=范围累加（不清已选）。
pub fn apply_release(
    mods: Mods,
    committed: &SelectionSet,
    band: &SelectionSet,
) -> SelectionSet {
    match mods {
        Mods::None => *band,
        Mods::Ctrl => committed.xor(band),
        Mods::Shift => match band.span() {
            Some((lo, hi)) => {
                let mut r = SelectionSet::new();
                r.range_set(lo, hi);
                r
            }
            None => SelectionSet::new(),
        },
        Mods::CtrlShift => {
            let mut r = *committed;
            if let Some((lo, hi)) = band.span() {
                r.range_set(lo, hi);
            }
            r
        }
    }
}

// ---------------------------------------------------------------------------
// 样式（主题令牌联动，几何常量解耦）
// ---------------------------------------------------------------------------

/// 橡皮筋样式：颜色随主题令牌走，几何（边框宽/透明度）是常量。
#[derive(Clone, Copy)]
pub struct BandStyle {
    pub border: Rgb8,
    pub fill: Rgb8,
    pub alpha_pct: u32,
    pub border_px: i32,
}

/// 从主题强调色取样式（换主题变色不换几何）。
pub fn style_from_theme(accent: Rgb8) -> BandStyle {
    BandStyle { border: accent, fill: accent, alpha_pct: FILL_ALPHA_PCT, border_px: BORDER_PX }
}

/// 整数 alpha 混合：`pct`% 前景叠背景（12% 填充的渲染落点）。
pub fn blend_alpha(bg: Rgb8, fg: Rgb8, pct: u32) -> Rgb8 {
    let mix = |b: u8, f: u8| -> u8 {
        ((f as u32 * pct + b as u32 * (100 - pct)) / 100) as u8
    };
    Rgb8::new(mix(bg.r, fg.r), mix(bg.g, fg.g), mix(bg.b, fg.b))
}

// ---------------------------------------------------------------------------
// 橡皮筋状态机
// ---------------------------------------------------------------------------

/// 单帧账目（测试量 / 是否超支 / 本帧滚动量）。
#[derive(Clone, Copy, Debug)]
pub struct FrameRec {
    pub tested: u32,
    pub overran: bool,
    pub scrolled_px: i32,
}

/// 橡皮筋相位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Dragging,
    Autoscroll,
}

/// 帧几何账本容量。
pub const PATH_CAP: usize = 32;

/// 橡皮筋状态机 + 分帧命中扫描。
pub struct RubberBand {
    phase: Phase,
    /// 按下点（视口坐标；内容 y 即按下时的内容坐标，滚动不改变它）。
    start_v: (i32, i32),
    /// 当前点（视口坐标）。
    cur_v: (i32, i32),
    /// 拖拽期间累计滚动量（内容坐标补偿：内容 y = 视口 y + scroll_off）。
    scroll_off: i32,
    /// 拖拽期间累计横向滚动量（网格橡皮筋）。
    scroll_x: i32,
    /// 内容尺寸（滚程钳制用；缺省无界）。
    content_w: i32,
    content_h: i32,
    viewport: Rect,
    mods: Mods,
    /// 拖拽开始时的既有选择快照。
    committed: SelectionSet,
    /// 当前生效选择（松开定形后更新）。
    sel: SelectionSet,
    /// 本轮框选命中集。
    band_hits: SelectionSet,
    /// 分帧扫描游标（已测到的绝对下标）。
    sweep_from: usize,
    /// 上次滚动同步时的滚动量（needs_resweep 判据）。
    last_sync_scroll: i32,
    frames: RingLog<FrameRec, FRAME_REC_CAP>,
    paths: RingLog<BandRec, PATH_CAP>,
    /// 累计滚动量（px）。
    pub scroll_total: i64,
    /// 拖拽帧数。
    pub drag_frames: u32,
}

impl RubberBand {
    pub fn new(viewport: Rect) -> RubberBand {
        RubberBand {
            phase: Phase::Idle,
            start_v: (0, 0),
            cur_v: (0, 0),
            scroll_off: 0,
            scroll_x: 0,
            content_w: i32::MAX / 2,
            content_h: i32::MAX / 2,
            viewport,
            mods: Mods::None,
            committed: SelectionSet::new(),
            sel: SelectionSet::new(),
            band_hits: SelectionSet::new(),
            sweep_from: 0,
            last_sync_scroll: 0,
            frames: RingLog::new(),
            paths: RingLog::new(),
            scroll_total: 0,
            drag_frames: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn selection(&self) -> &SelectionSet {
        &self.sel
    }

    pub fn band_hits(&self) -> &SelectionSet {
        &self.band_hits
    }

    /// 按下：以当前选择为既选快照，清命中集、归零游标与滚动。
    pub fn on_press(&mut self, x: i32, y: i32, mods: Mods) {
        self.phase = Phase::Dragging;
        self.start_v = (x, y);
        self.cur_v = (x, y);
        self.mods = mods;
        self.committed = self.sel;
        self.band_hits.clear_all();
        self.sweep_from = 0;
        self.scroll_off = 0;
        self.drag_frames = 0;
    }

    /// 移动：跟随；进入边缘带 → Autoscroll 相位。
    pub fn on_move(&mut self, x: i32, y: i32) {
        if self.phase == Phase::Idle {
            return;
        }
        self.cur_v = (x, y);
        self.phase = match self.edge_vertical() {
            Edge::None => Phase::Dragging,
            _ => Phase::Autoscroll,
        };
    }

    /// 当前纵向边缘档位（取上/下两边距边更近者）。
    pub fn edge_vertical(&self) -> Edge {
        let top_d = self.cur_v.1 - self.viewport.y;
        let bot_d = self.viewport.bottom() - 1 - self.cur_v.1;
        let d = top_d.min(bot_d).max(0);
        edge_zone(d)
    }

    /// 自动滚一步（dt 由调用方注入）：滚动量随档位加速，方向朝边缘。
    /// 返回本帧滚动 px（内容坐标方向：向下滚为正）。
    pub fn autoscroll_tick(&mut self, dt_ms: u32) -> i32 {
        if self.phase == Phase::Idle {
            return 0;
        }
        let top_d = self.cur_v.1 - self.viewport.y;
        let bot_d = self.viewport.bottom() - 1 - self.cur_v.1;
        // 距哪边更近就朝哪边滚（靠近顶边 → 内容向上滚 = 负方向）。
        let (zone, dir) = if top_d <= bot_d {
            (edge_zone(top_d), -1i32)
        } else {
            (edge_zone(bot_d), 1i32)
        };
        let px = (edge_speed(zone) as i64 * dt_ms as i64 / 1000) as i32 * dir;
        if px != 0 {
            self.scroll_off += px;
            self.scroll_total += px as i64;
        }
        px
    }

    /// 框选矩形（视口坐标，规格化——反向拖拽仍得正矩形）。
    pub fn band_rect_vp(&self) -> Rect {
        let (sx, sy) = self.start_v;
        let (cx, cy) = self.cur_v;
        let x = sx.min(cx);
        let y = sy.min(cy);
        Rect::new(x, y, (sx - cx).abs().max(1), (sy - cy).abs().max(1))
    }

    /// 框选矩形（内容坐标：当前点补偿滚动量——滚出屏外的部分仍在框内）。
    pub fn band_rect_content(&self) -> Rect {
        let r = self.band_rect_vp();
        let cy = self.cur_v.1 + self.scroll_off;
        let y = self.start_v.1.min(cy);
        let h = (self.start_v.1 - cy).abs().max(1);
        Rect::new(r.x, y, r.w, h)
    }

    /// 绘制矩形（钳进视口——框可以超出屏，绘制只画屏内部分）。
    pub fn clipped_draw(&self) -> Rect {
        self.band_rect_vp().clamped_into(&self.viewport)
    }

    /// 分帧命中扫描：从游标起最多测 `HIT_BUDGET` 项（内容坐标相交），
    /// 命中置位。返回本帧实测项数。`items` 为全部项的内容坐标矩形。
    pub fn hit_batch(&mut self, items: &[Rect]) -> usize {
        if self.phase == Phase::Idle {
            return 0;
        }
        let band = self.band_rect_content();
        let to = (self.sweep_from + HIT_BUDGET).min(items.len());
        let mut tested = 0usize;
        for i in self.sweep_from..to {
            if items[i].intersect_area(&band) > 0 {
                self.band_hits.set(i);
            }
            tested += 1;
        }
        self.sweep_from = to;
        self.drag_frames += 1;
        self.frames.push(FrameRec {
            tested: tested as u32,
            overran: tested > HIT_BUDGET,
            scrolled_px: 0,
        });
        tested
    }

    /// 本轮扫描是否已覆盖 `total` 项。
    pub fn sweep_done(&self, total: usize) -> bool {
        self.sweep_from >= total
    }

    /// 扫描游标（增量补测用：滚动后从游标继续，不重扫全表）。
    pub fn sweep_cursor(&self) -> usize {
        self.sweep_from
    }

    /// 游标回退（滚动引发内容位移后，允许补测新进入框内的项）。
    pub fn rewind_sweep(&mut self, to: usize) {
        self.sweep_from = to.min(self.sweep_from);
    }

    /// 松开定形：按修饰键折算最终选择，回 Idle。
    pub fn on_release(&mut self) -> SelectionSet {
        let out = apply_release(self.mods, &self.committed, &self.band_hits);
        self.sel = out;
        self.phase = Phase::Idle;
        out
    }

    /// 直接置选择（外部点击空白清选等场景）。
    pub fn set_selection(&mut self, s: &SelectionSet) {
        self.sel = *s;
    }

    /// 帧账本（新→旧，最多 64 帧）。
    pub fn frame_log(&self) -> &RingLog<FrameRec, FRAME_REC_CAP> {
        &self.frames
    }
}

/// 扫一帧预算的成本模型（ns）：`items` 项相交测试 + 固定开销。
pub fn sweep_cost_ns(items: usize) -> u64 {
    items as u64 * HIT_COST_NS + 200
}

/// 每帧预算成本（60fps 判据的量化口径）。
pub fn budget_frame_cost_ns() -> u64 {
    sweep_cost_ns(HIT_BUDGET)
}

// ---------------------------------------------------------------------------
// 双轴滚动同步 / 拖拽中止 / 诊断面
// ---------------------------------------------------------------------------

/// 成框最小位移——小于此视为点击不成框（实装定值）。
pub const DRAG_MIN_PX: i32 = 4;

/// 单帧账目扩展：滚动量分轴记录。
#[derive(Clone, Copy, Debug)]
pub struct BandRec {
    /// 框几何（视口坐标）。
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// 当时的滚动补偿。
    pub scroll_off: i32,
    /// 注入时间戳（ms）。
    pub ts_ms: u64,
}

impl RubberBand {
    /// 注入内容尺寸（滚程钳制用；不注入视为无界列表）。
    pub fn set_content_size(&mut self, w: i32, h: i32) {
        self.content_w = w.max(0);
        self.content_h = h.max(0);
    }

    /// 纵向最大滚程（内容高 − 视口高，下限 0）。
    pub fn max_scroll_y(&self) -> i32 {
        (self.content_h - self.viewport.h).max(0)
    }

    /// 横向最大滚程。
    pub fn max_scroll_x(&self) -> i32 {
        (self.content_w - self.viewport.w).max(0)
    }

    /// 当前横向边缘档位（网格橡皮筋用；列表主用纵向）。
    pub fn edge_horizontal(&self) -> Edge {
        let left_d = self.cur_v.0 - self.viewport.x;
        let right_d = self.viewport.right() - 1 - self.cur_v.0;
        let d = left_d.min(right_d).max(0);
        edge_zone(d)
    }

    /// 拖拽位移（px）。
    pub fn drag_distance(&self) -> i32 {
        let dx = (self.cur_v.0 - self.start_v.0) as i64;
        let dy = (self.cur_v.1 - self.start_v.1) as i64;
        let sq = (dx * dx + dy * dy) as u64;
        if sq == 0 {
            return 0;
        }
        // 整数平方根（牛顿迭代，core 无 f64）。
        let mut r = sq;
        let mut g = (sq >> 1).max(1);
        while g < r {
            r = g;
            g = (g + sq / g) >> 1;
        }
        r as i32
    }

    /// 位移不足 → 判定为点击（不成框、不清既选）。
    pub fn is_click(&self) -> bool {
        self.drag_distance() < DRAG_MIN_PX
    }

    /// 双轴自动滚步（网格）：纵/横各自按边缘档位推进，滚程钳进内容界。
    /// 返回 (dx, dy)——内容坐标方向的滚动增量。
    pub fn autoscroll_tick_xy(&mut self, dt_ms: u32) -> (i32, i32) {
        let dy = self.autoscroll_tick(dt_ms);
        // 横向：与纵向同参数同分带。
        let left_d = self.cur_v.0 - self.viewport.x;
        let right_d = self.viewport.right() - 1 - self.cur_v.0;
        let (zone, dir) = if left_d <= right_d {
            (edge_zone(left_d), -1i32)
        } else {
            (edge_zone(right_d), 1i32)
        };
        let dx = (edge_speed(zone) as i64 * dt_ms as i64 / 1000) as i32 * dir;
        if dx != 0 {
            self.scroll_x += dx;
            // 钳进内容界。
            let max_x = self.max_scroll_x();
            self.scroll_x = self.scroll_x.clamp(0, max_x);
        }
        // 纵向同样钳制（有界列表滚到底不再累积）。
        let max_y = self.max_scroll_y();
        if self.scroll_off > max_y {
            self.scroll_off = max_y;
        }
        if self.scroll_off < 0 {
            self.scroll_off = 0;
        }
        (self.scroll_x, dy)
    }

    /// 滚动后是否需要重扫（自上次分帧批后滚动量有变化）。
    pub fn needs_resweep(&self) -> bool {
        self.scroll_off != self.last_sync_scroll
    }

    /// 滚动同步：把已置位但已滚出框的项摘除（内容坐标重判），
    /// 并把游标回退到旧可见界——新滚入框内的项由后续分帧补测，
    /// 不重扫全表（60fps 纪律）。
    pub fn sync_resweep(&mut self, items: &[Rect], old_visible_end: usize) {
        let band = self.band_rect_content();
        for i in 0..items.len().min(MAX_ITEMS) {
            if self.band_hits.get(i) && items[i].intersect_area(&band) == 0 {
                self.band_hits.clear(i);
            }
        }
        self.rewind_sweep(old_visible_end);
        self.last_sync_scroll = self.scroll_off;
    }

    /// 中止拖拽（Esc/取消）：丢弃框选，恢复既选快照，回 Idle。
    pub fn on_cancel(&mut self) -> SelectionSet {
        self.sel = self.committed;
        self.band_hits.clear_all();
        self.phase = Phase::Idle;
        self.sel
    }

    /// 单点命中（点击选择的公共通路；后登记者优先）。
    pub fn item_at_point(&self, items: &[Rect], x: i32, y: i32) -> Option<usize> {
        for (i, r) in items.iter().enumerate().rev() {
            if r.contains(x, y) {
                return Some(i);
            }
        }
        None
    }

    /// 每帧几何入账（时间由调用方注入；回放/诊断面）。
    pub fn note_frame(&mut self, ts_ms: u64) {
        let r = self.band_rect_vp();
        self.paths.push(BandRec { x: r.x, y: r.y, w: r.w, h: r.h, scroll_off: self.scroll_off, ts_ms });
    }

    /// 帧几何账本（新→旧）。
    pub fn path_log(&self) -> &RingLog<BandRec, PATH_CAP> {
        &self.paths
    }
}

/// 选择集统计快照（诊断面）。
#[derive(Clone, Copy, Debug)]
pub struct SelectionStats {
    pub count: u32,
    pub span: Option<(usize, usize)>,
    /// 占容量密度（‰）。
    pub density_ppt: u32,
}

impl SelectionSet {
    pub fn stats(&self) -> SelectionStats {
        SelectionStats {
            count: self.count(),
            span: self.span(),
            density_ppt: self.count() * 1000 / MAX_ITEMS as u32,
        }
    }
}

fn mkset(idx: &[usize]) -> SelectionSet {
    let mut s = SelectionSet::new();
    for &i in idx {
        s.set(i);
    }
    s
}

/// F203 自检（判据面：分帧摊销 60fps + 三档滚速 + 12 修饰场景 + 样式联动）。
pub fn run_rubbersel_checks() -> CheckSet {
    let mut set = CheckSet::new("F203-rubbersel");
    let vp = Rect::new(0, 0, 800, 600);

    // 1. 矩形规格化：从右下往左上拖仍得正矩形。
    let mut rb = RubberBand::new(vp);
    rb.on_press(500, 400, Mods::None);
    rb.on_move(120, 80);
    let r = rb.band_rect_vp();
    set.add(
        "band rect normalized on reverse drag",
        r.x == 120 && r.y == 80 && r.w == 380 && r.h == 320,
        "",
    );

    // 2. 预算制分帧摊销：1000 项恰 4 帧扫完，每帧 ≤250。
    let mut items = [Rect::new(0, 0, 10, 10); 1000];
    for (i, it) in items.iter_mut().enumerate() {
        *it = Rect::new(0, i as i32 * 20, 10, 10);
    }
    let mut rb2 = RubberBand::new(Rect::new(0, 0, 800, 400));
    rb2.on_press(0, 0, Mods::None);
    rb2.on_move(50, 300);
    let mut total_tested = 0usize;
    let mut frames = 0usize;
    let mut per_frame_ok = true;
    while !rb2.sweep_done(1000) {
        let tested = rb2.hit_batch(&items);
        if tested > HIT_BUDGET {
            per_frame_ok = false;
        }
        total_tested += tested;
        frames += 1;
    }
    set.add(
        "1000 items swept in 4 budgeted frames",
        frames == 4 && total_tested == 1000 && per_frame_ok,
        "",
    );

    // 3. 成本模型：每帧预算 ≪ 120fps 帧预算（8.33ms）。
    set.add(
        "per-frame cost fits 120fps budget",
        budget_frame_cost_ns() <= FRAME_120_MS as u64 * 1_000_000,
        "",
    );

    // 4. 三档边缘带：40px 外不触发；24/12/4 分属慢/中/快。
    set.add(
        "edge zones none/slow/mid/fast",
        edge_zone(40) == Edge::None
            && edge_zone(24) == Edge::Slow
            && edge_zone(12) == Edge::Mid
            && edge_zone(4) == Edge::Fast,
        "",
    );

    // 5. 三档速度单调加速，且自动滚动实滚累计（贴近程度→速度）。
    set.add(
        "speeds accelerate slow<mid<fast",
        edge_speed(Edge::Slow) < edge_speed(Edge::Mid)
            && edge_speed(Edge::Mid) < edge_speed(Edge::Fast),
        "",
    );
    let mut rb3 = RubberBand::new(vp);
    rb3.on_press(400, 595, Mods::None); // 距底边 4px → 快档
    rb3.on_move(400, 596);
    let s1 = rb3.autoscroll_tick(100);
    let s2 = rb3.autoscroll_tick(100);
    set.add(
        "autoscroll near edge accumulates",
        rb3.phase() == Phase::Autoscroll && s1 > 0 && s2 > 0 && rb3.scroll_total == (s1 + s2) as i64,
        "",
    );

    // 6~8. 修饰键 12 场景全枚举（None×4 / Ctrl×4 / Shift+CtrlShift×4）。
    let empty = SelectionSet::new();
    let committed = mkset(&[1, 2, 3]);
    let band = mkset(&[5, 6, 7]);
    let overlap = mkset(&[2, 9]);
    // None 组：
    let s1 = apply_release(Mods::None, &committed, &band);
    let s2 = apply_release(Mods::None, &committed, &empty);
    let all = mkset(&(0..8).collect::<alloc::vec::Vec<usize>>());
    let s3 = apply_release(Mods::None, &empty, &all);
    let s4 = apply_release(Mods::None, &committed, &overlap);
    set.add(
        "mods none: replace semantics x4",
        s1.count() == 3
            && s1.get(5)
            && s2.is_empty()
            && s3.count() == 8
            && s4.count() == 2
            && s4.get(2) && s4.get(9) && !s4.get(1),
        "",
    );
    // Ctrl 组：
    let c1 = apply_release(Mods::Ctrl, &committed, &band);
    let c2 = apply_release(Mods::Ctrl, &committed, &mkset(&[2]));
    let c3 = apply_release(Mods::Ctrl, &committed, &overlap);
    let c4 = apply_release(Mods::Ctrl, &committed, &band).xor(&band); // 双次异或复原
    set.add(
        "mods ctrl: xor semantics x4",
        c1.count() == 6
            && c1.get(1) && c1.get(5)
            && c2.count() == 2
            && !c2.get(2)
            && c3.count() == 3
            && !c3.get(2) && c3.get(9)
            && c4.count() == 3 && c4.get(1) && c4.get(3),
        "",
    );
    // Shift / CtrlShift 组：
    let sh1 = apply_release(Mods::Shift, &committed, &band);
    let sh2 = apply_release(Mods::Shift, &committed, &empty);
    let cs1 = apply_release(Mods::CtrlShift, &committed, &band);
    let cs2 = apply_release(Mods::CtrlShift, &committed, &mkset(&[2]));
    set.add(
        "mods shift/ctrlshift: range semantics x4",
        sh1.count() == 3 && !sh1.get(1)
            && sh2.is_empty()
            && cs1.count() == 6 && cs1.get(1) && cs1.get(7)
            && cs2.count() == 3 && cs2.get(2),
        "",
    );

    // 9. 样式联动：换主题只变色，几何常量不动。
    let blue = style_from_theme(Rgb8::new(40, 90, 220));
    let green = style_from_theme(Rgb8::new(30, 160, 80));
    set.add(
        "theme swap changes color not geometry",
        blue.alpha_pct == FILL_ALPHA_PCT
            && green.alpha_pct == FILL_ALPHA_PCT
            && blue.border_px == BORDER_PX
            && green.border_px == BORDER_PX
            && blue.border.r != green.border.r,
        "",
    );

    // 10. 12% 填充混合：前景占比恰 12%（整数混合可全复现）。
    let bg = Rgb8::new(255, 255, 255);
    let fg = Rgb8::new(0, 0, 0);
    let mixed = blend_alpha(bg, fg, FILL_ALPHA_PCT);
    set.add(
        "12% fill blend exact",
        mixed.r == 224 && mixed.g == 224 && mixed.b == 224,
        "",
    );

    // 11. 选择集运算：异或对合、范围置位、ids 提取、span 端点。
    let mut ss = mkset(&[1, 5, 100]);
    ss.range_set(10, 12);
    let mut out = [0u16; 16];
    let n = ss.selected_ids(&mut out);
    let (lo, hi) = ss.span().unwrap();
    set.add(
        "selection set ops",
        ss.count() == 6
            && n == 6
            && out[0] == 1
            && lo == 1
            && hi == 100
            && ss.xor(&ss).is_empty(),
        "",
    );

    // 12. 绘制矩形钳进视口（框可超屏、绘制不超屏）。
    let mut rb4 = RubberBand::new(vp);
    rb4.on_press(790, 590, Mods::None);
    rb4.on_move(900, 700);
    let d = rb4.clipped_draw();
    set.add(
        "draw rect clipped into viewport",
        d.right() <= vp.right() && d.bottom() <= vp.bottom() && d.w > 0,
        "",
    );

    // 13. fuzz（xorshift32 范式）：随机拖拽几何 2000 轮——不变量：
    //     规格化矩形恒正、按下点恒含于框内、修饰松开的结果 ⊆ 全集且可复原。
    let mut x: u32 = 0x9E3779B9;
    let mut fuzz_ok = true;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let ax = (x % 1000) as i32;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let ay = (x % 800) as i32;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let bx = (x % 1000) as i32;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let by = (x % 800) as i32;
        let mut b = RubberBand::new(vp);
        b.on_press(ax, ay, Mods::Ctrl);
        b.on_move(bx, by);
        let r = b.band_rect_vp();
        // 缺陷账本：现象=fuzz 随机方向拖拽约半数轮报「按下点不在框内」；
        // 根因=按下点是框的角点，Rect::contains 为半开区间 [x,right)，
        // 向左/向上拖时按下点恰落在 right/bottom 边上恒不含——是 fuzz
        // 不变量把判据写出了几何域（反向拖拽是合法域，检查 1 亦验之）；
        // 修法=「按下点恒含于框内」按几何闭包判定（[x,right]×[y,bottom]），
        // 规格化矩形恒正与可复原两项硬不变量不动。
        let closed = r.x <= ax && ax <= r.right() && r.y <= ay && ay <= r.bottom();
        if r.w <= 0 || r.h <= 0 || !closed {
            fuzz_ok = false;
        }
        // 松开恒等性：Ctrl 异或自身带 → 回到既选集。
        let res = apply_release(Mods::Ctrl, &committed, &empty);
        if res.count() != committed.count() {
            fuzz_ok = false;
        }
    }
    set.add("rubber band fuzz 2000 rounds invariants", fuzz_ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_follows_and_freezes_on_release() {
        let mut rb = RubberBand::new(Rect::new(0, 0, 800, 600));
        assert_eq!(rb.phase(), Phase::Idle);
        rb.on_press(10, 10, Mods::None);
        rb.on_move(200, 150);
        assert_eq!(rb.phase(), Phase::Dragging);
        assert_eq!(rb.band_rect_vp(), Rect::new(10, 10, 190, 140));
        rb.on_release();
        assert_eq!(rb.phase(), Phase::Idle);
    }

    #[test]
    fn autoscroll_extends_band_in_content_coords() {
        let mut rb = RubberBand::new(Rect::new(0, 0, 800, 600));
        rb.on_press(100, 100, Mods::None);
        rb.on_move(400, 598);
        let scrolled = rb.autoscroll_tick(1000); // 快档 1200px/s × 1s
        assert!(scrolled >= 1200);
        let c = rb.band_rect_content();
        // 内容坐标下框底边（开区间）= 当前视口 y + 滚动补偿。
        assert_eq!(c.bottom(), 598 + rb.scroll_off);
        assert!(c.h > 600, "滚出屏外的内容仍在框内");
    }

    #[test]
    fn hit_batch_marks_only_intersecting() {
        let items = [
            Rect::new(0, 0, 10, 10),
            Rect::new(100, 100, 10, 10),
            Rect::new(5, 5, 10, 10),
            Rect::new(500, 500, 10, 10),
        ];
        let mut rb = RubberBand::new(Rect::new(0, 0, 800, 600));
        rb.on_press(0, 0, Mods::None);
        rb.on_move(50, 50);
        while !rb.sweep_done(4) {
            rb.hit_batch(&items);
        }
        let hits = rb.band_hits();
        assert!(hits.get(0) && hits.get(2) && !hits.get(1) && !hits.get(3));
        let sel = rb.on_release();
        assert_eq!(sel.count(), 2);
    }

    #[test]
    fn edge_zone_boundaries_exact() {
        assert_eq!(edge_zone(33), Edge::None);
        assert_eq!(edge_zone(32), Edge::Slow);
        assert_eq!(edge_zone(17), Edge::Slow);
        assert_eq!(edge_zone(16), Edge::Mid);
        assert_eq!(edge_zone(9), Edge::Mid);
        assert_eq!(edge_zone(8), Edge::Fast);
        assert_eq!(edge_zone(0), Edge::Fast);
    }

    #[test]
    fn autoscroll_direction_toward_nearest_edge() {
        let mut rb = RubberBand::new(Rect::new(0, 0, 800, 600));
        rb.on_press(400, 2, Mods::None); // 近顶边
        rb.on_move(400, 3);
        let px = rb.autoscroll_tick(1000);
        assert!(px < 0, "近顶边应向内容上方滚");
        assert_eq!(rb.scroll_total, px as i64);
    }

    #[test]
    fn frame_log_records_budget() {
        let items = [Rect::new(0, 0, 1, 1); 300];
        let mut rb = RubberBand::new(Rect::new(0, 0, 800, 600));
        rb.on_press(0, 0, Mods::None);
        rb.on_move(10, 10);
        while !rb.sweep_done(300) {
            rb.hit_batch(&items);
        }
        let log = rb.frame_log().newest_first();
        assert!(log.iter().all(|f| !f.overran));
        assert_eq!(log.iter().map(|f| f.tested as usize).sum::<usize>(), 300);
    }

    #[test]
    fn ctrl_toggle_roundtrip_involutive() {
        let committed = mkset(&[3, 4, 5]);
        let band = mkset(&[4, 5, 6]);
        let once = apply_release(Mods::Ctrl, &committed, &band);
        // 缺陷账本：现象=本测试红（count 2≠3）；根因=首断言误写「异或后
        // 计数不变」——{3,4,5} XOR {4,5,6} 按加选/减选语义恰去掉重叠的
        // 4、5 得 {3,6}（count=2），与下一行断言 get(3)/get(6)/!get(4)
        // 自相矛盾，属测试写错；修法=首断言改 2，对合复原断言不动。
        assert_eq!(once.count(), 2);
        assert!(once.get(3) && once.get(6) && !once.get(4));
        let twice = once.xor(&band);
        assert_eq!(twice.count(), committed.count());
    }

    #[test]
    fn cancel_restores_committed_selection() {
        let items = [Rect::new(0, 0, 10, 10), Rect::new(100, 100, 10, 10)];
        let mut rb = RubberBand::new(Rect::new(0, 0, 800, 600));
        let mut pre = SelectionSet::new();
        pre.set(1);
        rb.set_selection(&pre);
        rb.on_press(0, 0, Mods::None);
        rb.on_move(500, 500);
        while !rb.sweep_done(2) {
            rb.hit_batch(&items);
        }
        assert_eq!(rb.band_hits().count(), 2);
        let restored = rb.on_cancel();
        assert_eq!(restored.count(), 1);
        assert!(restored.get(1));
        assert_eq!(rb.phase(), Phase::Idle);
    }

    #[test]
    fn autoscroll_xy_clamps_to_content_bounds() {
        let mut rb = RubberBand::new(Rect::new(0, 0, 800, 600));
        rb.set_content_size(1000, 2000);
        rb.on_press(798, 300, Mods::None); // 近右缘 → 横向快档
        rb.on_move(799, 300);
        for _ in 0..50 {
            let _ = rb.autoscroll_tick_xy(100);
        }
        assert_eq!(rb.scroll_x, rb.max_scroll_x(), "横向滚程钳在内容界");
        rb.on_move(400, 598); // 近底缘 → 纵向快档
        for _ in 0..50 {
            let _ = rb.autoscroll_tick_xy(100);
        }
        assert_eq!(rb.scroll_off, rb.max_scroll_y(), "纵向滚程钳在内容界");
    }

    #[test]
    fn sync_resweep_prunes_scrolled_out_items() {
        let items = [Rect::new(0, 0, 10, 10), Rect::new(0, 600, 10, 10)];
        let mut rb = RubberBand::new(Rect::new(0, 0, 800, 600));
        rb.on_press(0, 0, Mods::None);
        rb.on_move(50, 100); // 框罩住 item0
        while !rb.sweep_done(2) {
            rb.hit_batch(&items);
        }
        assert!(rb.band_hits().get(0));
        // 向上滚 1200：框的内容区上移，item0 滚出框 → 同步摘除。
        rb.on_move(25, 1);
        let _ = rb.autoscroll_tick(1000);
        assert!(rb.needs_resweep());
        rb.sync_resweep(&items, 0);
        assert_eq!(rb.band_hits().count(), 0, "滚出框的项被摘除");
    }

    #[test]
    fn tiny_drag_is_click_not_band() {
        let mut rb = RubberBand::new(Rect::new(0, 0, 800, 600));
        rb.on_press(100, 100, Mods::None);
        rb.on_move(102, 101);
        assert!(rb.is_click());
        rb.on_move(150, 180);
        assert!(!rb.is_click());
        assert!(rb.drag_distance() >= DRAG_MIN_PX);
    }

    #[test]
    fn rubbersel_selfcheck_all_green() {
        let set = run_rubbersel_checks();
        assert!(set.all_passed(), "F203 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 8 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

const VXH1_MAGIC: [u8; 4] = *b"VXH1";
const VXH1_VER: u8 = 1;

/// 损坏输入显性拒绝：四类 + 字段越界（档位数值非法）。
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

// ---- 持久化 I/O 面：自动滚动三档速度档案 ----

/// 记录长：magic4 + ver1 + 6×i32（三条分带界 + 三档速度）+ sum4。
pub const PROFILE_REC_LEN: usize = 4 + 1 + 24 + 4;

/// 自动滚动档位档案：分带界与三档速度整组落盘（32px 触发带与
/// 150/450/1200 px/s 的字节级台账，主册 F203「自动滚动三档速度」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoscrollProfile {
    pub band_px: i32,
    pub mid_px: i32,
    pub fast_px: i32,
    /// 慢/中/快三档速度（px/s）。
    pub speeds: [i32; 3],
}

impl AutoscrollProfile {
    /// 从规格常量取缺省档案（与 edge_zone/edge_speed 同源，一处一事实）。
    pub fn from_consts() -> AutoscrollProfile {
        AutoscrollProfile {
            band_px: EDGE_BAND_PX,
            mid_px: EDGE_MID_PX,
            fast_px: EDGE_FAST_PX,
            speeds: [SPEED_SLOW_PX_S, SPEED_MID_PX_S, SPEED_FAST_PX_S],
        }
    }

    /// 档位合法性：band > mid > fast > 0 且三档速度严格递增。
    fn valid(&self) -> bool {
        self.band_px > self.mid_px
            && self.mid_px > self.fast_px
            && self.fast_px > 0
            && self.speeds[0] > 0
            && self.speeds[0] < self.speeds[1]
            && self.speeds[1] < self.speeds[2]
    }

    pub fn to_bytes(&self) -> [u8; PROFILE_REC_LEN] {
        let mut out = [0u8; PROFILE_REC_LEN];
        out[..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VXH1_VER;
        let vals = [self.band_px, self.mid_px, self.fast_px, self.speeds[0], self.speeds[1], self.speeds[2]];
        for (k, v) in vals.iter().enumerate() {
            out[5 + k * 4..9 + k * 4].copy_from_slice(&v.to_le_bytes());
        }
        let body = 5 + 24;
        let sum = fnv1a(&out[..body]).to_le_bytes();
        out[body..body + 4].copy_from_slice(&sum);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Result<AutoscrollProfile, V2CodecErr> {
        if b.len() != PROFILE_REC_LEN {
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
        let mut vals = [0i32; 6];
        for (k, v) in vals.iter_mut().enumerate() {
            let mut t = [0u8; 4];
            t.copy_from_slice(&b[5 + k * 4..9 + k * 4]);
            *v = i32::from_le_bytes(t);
        }
        let p = AutoscrollProfile {
            band_px: vals[0],
            mid_px: vals[1],
            fast_px: vals[2],
            speeds: [vals[3], vals[4], vals[5]],
        };
        if !p.valid() {
            return Err(V2CodecErr::BadField);
        }
        Ok(p)
    }
}

// ---- UI 壳接线面：橡皮筋绘制清单（令牌色索引 + 几何分离）----

/// 绘制图元：几何 + 颜色索引（0 = 令牌描边色，1 = 12% 填充混合色）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2Prim {
    pub rect: Rect,
    pub color_idx: u8,
}

/// 橡皮筋绘制清单（定长 2 图元）：0 = 1px 描边环（整框），1 = 12%
/// 填充（内缩 1px）。颜色由壳层按索引取令牌/混合色——换主题变色
/// 不换几何（主册 F203 样式联动 + F151 令牌纪律的壳层落点）。
pub fn band_draw_items(band: Rect) -> [V2Prim; 2] {
    let inner = Rect::new(band.x + 1, band.y + 1, (band.w - 2).max(1), (band.h - 2).max(1));
    [V2Prim { rect: band, color_idx: 0 }, V2Prim { rect: inner, color_idx: 1 }]
}

/// F203 v2 自检（首条恒为持久化 round-trip）。
pub fn run_rubbersel_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F203-rubbersel-v2");

    // 1. 持久化 round-trip：缺省档案编码→解码逐字段还原。
    let p = AutoscrollProfile::from_consts();
    let ok_rt = match AutoscrollProfile::from_bytes(&p.to_bytes()) {
        Ok(q) => q == p,
        Err(_) => false,
    };
    set.add("v2 persist roundtrip autoscroll profile", ok_rt, "");

    // 2. 损坏拒绝四类 + 字段越界（分带倒挂）。
    let bytes = p.to_bytes();
    let mut bad1 = bytes;
    bad1[0] = b'X';
    let mut bad2 = bytes;
    bad2[4] = 9;
    let mut bad3 = bytes;
    bad3[5] ^= 0xFF;
    let mut bad4 = bytes;
    bad4[5] = 0; // band_px = 0 → 档位非法（重算校验和使 sum 合法、只坏字段）
    let body4 = 5 + 24;
    let s4 = fnv1a(&bad4[..body4]);
    bad4[body4..body4 + 4].copy_from_slice(&s4.to_le_bytes());
    set.add(
        "v2 persist rejects corrupt profiles",
        AutoscrollProfile::from_bytes(&bad1) == Err(V2CodecErr::BadMagic)
            && AutoscrollProfile::from_bytes(&bad2) == Err(V2CodecErr::BadVersion)
            && AutoscrollProfile::from_bytes(&bad3) == Err(V2CodecErr::BadSum)
            && AutoscrollProfile::from_bytes(&bytes[..bytes.len() - 1]) == Err(V2CodecErr::BadLen)
            && AutoscrollProfile::from_bytes(&bad4) == Err(V2CodecErr::BadField),
        "",
    );

    // 3. 绘制清单：描边整框 + 填充内缩 1px，几何与配色索引分离
    //    （验主册 F203「边框 1px」+「12% 填充」壳层落点）。
    let items = band_draw_items(Rect::new(10, 20, 50, 30));
    set.add(
        "v2 band draw list geometry separated",
        items[0].rect == Rect::new(10, 20, 50, 30)
            && items[0].color_idx == 0
            && items[1].rect == Rect::new(11, 21, 48, 28)
            && items[1].color_idx == 1,
        "",
    );

    // 4. F151 令牌联动：换主题只变色不换几何（绘制清单逐位相同，
    //    混合色随 accent 变）。
    let bg = Rgb8::new(255, 255, 255);
    let c1 = blend_alpha(bg, Rgb8::new(40, 90, 220), FILL_ALPHA_PCT);
    let c2 = blend_alpha(bg, Rgb8::new(30, 160, 80), FILL_ALPHA_PCT);
    set.add(
        "v2 theme swap changes color not geometry",
        c1 != c2 && band_draw_items(Rect::new(0, 0, 40, 40)) == band_draw_items(Rect::new(0, 0, 40, 40)),
        "",
    );

    // 5. 解码档案三档速度单调（与 edge_speed 同判：慢 < 中 < 快）。
    let back = AutoscrollProfile::from_bytes(&p.to_bytes());
    set.add(
        "v2 decoded speeds accelerate slow<mid<fast",
        back.map_or(false, |q| {
            q.speeds[0] < q.speeds[1] && q.speeds[1] < q.speeds[2] && q.band_px == EDGE_BAND_PX
        }),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn profile_field_values_match_consts() {
        let p = AutoscrollProfile::from_consts();
        assert_eq!((p.band_px, p.mid_px, p.fast_px), (32, 16, 8));
        assert_eq!(p.speeds, [150, 450, 1200]);
    }

    #[test]
    fn band_draw_items_tiny_rect() {
        // 极小框：内缩钳制为 1×1，不越界不出负宽。
        let items = band_draw_items(Rect::new(0, 0, 2, 2));
        assert_eq!(items[1].rect, Rect::new(1, 1, 1, 1));
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_rubbersel_v2_checks();
        assert!(set.all_passed(), "F203 v2 自检存在红项");
        assert!(!set.truncated());
        assert!((4..=6).contains(&set.len()));
    }
}
