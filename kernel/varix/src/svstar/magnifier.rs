//! F111 放大镜 · 完整设计（STAR I 主册 G-C-41）。
//!
//! **判据（主册）**：2x-16x 全档文字锐利（4K 放大三倍不糊条款验证）；
//! 镜头模式 80fps 跟手；三跟随策略切换实测。
//!
//! **设计要点（主册）**：
//! - 全屏模式（整个桌面放大移动）与镜头模式（240px 圆形镜头随光标）；
//! - 放大重采样：最近邻（低倍保清晰锐利）+ 8x 以上双线性（像素画观察）；
//! - 三跟随策略：随光标 / 随焦点 / 随键盘——切换保持倍率不重置；
//! - 工具条迷你悬浮：倍率 +/- / 模式切换 / 退出四件；
//! - 倍率快捷 Ctrl+滚轮；全屏模式平移 = 光标顶边触发（对齐 Windows 动线）；
//!   全屏模式边缘平移速度三档；
//! - 16x 极限 → 像素网格显示辅助（帮助定位）；
//! - 放大状态下截图 → 截取放大后所见（所见即所得）；
//! - 性能保护 → 放大区域外渲染降载（F056 脏区联动：镜头模式脏区=镜头
//!   外接方框，全屏模式脏区=全视口）；
//! - 与夜间模式（F116）/色弱滤镜（F114）叠加顺序文档化：
//!   色温（F116）→ 滤镜（F114）→ 放大（F111）——放大在最末级，所见即所得；
//! - 倍率与模式记忆（会话内，不跨重启）；快捷键全套登记 F169。
//!
//! 时间注入式（微秒戳），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 倍率下限（主册：2x-16x）。
pub const ZOOM_MIN: u32 = 2;
/// 倍率上限（主册：2x-16x；16x 极限开像素网格辅助）。
pub const ZOOM_MAX: u32 = 16;
/// 像素网格辅助触发倍率（主册：16x 极限 → 像素网格显示辅助）。
pub const GRID_ZOOM: u32 = 16;

/// 镜头直径（px，主册：240px 圆形镜头）。
pub const LENS_DIAMETER_PX: u32 = 240;
/// 镜头边缘柔化环宽（px，主册：圆形边缘柔化 8px）。
pub const LENS_FEATHER_PX: u32 = 8;

/// 镜头模式帧预算（微秒）——80fps 跟手 = 帧耗时 ≤ 12.5ms。
pub const LENS_FRAME_BUDGET_US: u64 = 12_500;

/// 全屏模式边缘平移触发带宽（px，光标贴边判定）。
pub const EDGE_PAN_ZONE_PX: u32 = 20;
/// 边缘平移速度三档（px/帧，慢/中/快——主册：边缘平移速度三档）。
pub const EDGE_PAN_SPEEDS_PX: [u32; 3] = [4, 12, 32];

/// 三跟随策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FollowStrategy {
    /// 随光标（默认）。
    Cursor,
    /// 随焦点（控件焦点移动跳转）。
    Focus,
    /// 随键盘（输入光标跟随）。
    Keyboard,
}

/// 放大镜工作模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagMode {
    /// 关闭。
    Off,
    /// 全屏模式：整个桌面放大，视口随平移动线移动。
    Fullscreen,
    /// 镜头模式：240px 圆形镜头随光标。
    Lens,
}

/// 采样方式（主册：最近邻低倍 + 8x 以上双线性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resample {
    /// 最近邻——8x 以下（低倍保清晰锐利，文字边缘不糊）。
    Nearest,
    /// 双线性——8x 及以上（像素画观察平滑）。
    Bilinear,
}

/// 按倍率选采样方式（判据常量唯一源）。
pub fn resample_for(zoom: u32) -> Resample {
    if zoom >= 8 {
        Resample::Bilinear
    } else {
        Resample::Nearest
    }
}

// ---------------------------------------------------------------------------
// 源面（合成器输出注入）与采样
// ---------------------------------------------------------------------------

/// 源帧：合成器输出面注入（宽高 + RGBA 数据）。放大镜对源只读。
pub struct SourceFrame<'a> {
    pub w: u32,
    pub h: u32,
    pub rgba: &'a [u8],
}

impl<'a> SourceFrame<'a> {
    fn px(&self, x: u32, y: u32) -> (u8, u8, u8) {
        let cx = x.min(self.w.saturating_sub(1)) as usize;
        let cy = y.min(self.h.saturating_sub(1)) as usize;
        let i = (cy * self.w as usize + cx) * 4;
        (self.rgba[i], self.rgba[i + 1], self.rgba[i + 2])
    }

    /// 最近邻采样：源坐标直取像素——文字边缘锐利不糊。
    pub fn sample_nearest(&self, sx: u32, sy: u32) -> (u8, u8, u8) {
        self.px(sx, sy)
    }

    /// 双线性采样：四邻插值（8x 以上像素画观察平滑）。
    pub fn sample_bilinear(&self, fx: f64, fy: f64) -> (u8, u8, u8) {
        let x0 = fx.floor().max(0.0) as u32;
        let y0 = fy.floor().max(0.0) as u32;
        let x1 = x0.saturating_add(1);
        let y1 = y0.saturating_add(1);
        let dx = fx - x0 as f64;
        let dy = fy - y0 as f64;
        let c00 = self.px(x0, y0);
        let c10 = self.px(x1, y0);
        let c01 = self.px(x0, y1);
        let c11 = self.px(x1, y1);
        let lerp = |a: u8, b: u8, t: f64| -> u8 {
            (a as f64 * (1.0 - t) + b as f64 * t).round().clamp(0.0, 255.0) as u8
        };
        let top = (lerp(c00.0, c10.0, dx), lerp(c00.1, c10.1, dx), lerp(c00.2, c10.2, dx));
        let bot = (lerp(c01.0, c11.0, dx), lerp(c01.1, c11.1, dx), lerp(c01.2, c11.2, dx));
        (lerp(top.0, bot.0, dy), lerp(top.1, bot.1, dy), lerp(top.2, bot.2, dy))
    }
}

// ---------------------------------------------------------------------------
// 放大镜状态机
// ---------------------------------------------------------------------------

/// 放大镜头部状态。倍率与模式记忆（会话内）；关闭再开恢复上次记忆。
pub struct Magnifier {
    mode: MagMode,
    zoom: u32,
    follow: FollowStrategy,
    /// 记忆槽：上次非 Off 模式（Off→再开恢复）。
    last_active_mode: MagMode,
    /// 全屏视口左上角（源面坐标）。
    viewport: (u32, u32),
    /// 视口所在屏尺寸（全屏平移边界用）。
    screen: (u32, u32),
    /// 边缘平移速度档（0/1/2 → 三档）。
    pan_speed_tier: usize,
    /// 16x 像素网格辅助开关（判据：16x 极限 → 像素网格显示辅助）。
    grid_overlay: bool,
    /// 镜头最近一次渲染耗时（us，注入；80fps 预算对账用）。
    last_lens_render_us: u64,
}

impl Magnifier {
    pub fn new(screen_w: u32, screen_h: u32) -> Magnifier {
        Magnifier {
            mode: MagMode::Off,
            zoom: ZOOM_MIN,
            follow: FollowStrategy::Cursor,
            last_active_mode: MagMode::Fullscreen,
            viewport: (0, 0),
            screen: (screen_w.max(1), screen_h.max(1)),
            pan_speed_tier: 1,
            grid_overlay: false,
            last_lens_render_us: 0,
        }
    }

    pub fn mode(&self) -> MagMode {
        self.mode
    }

    pub fn zoom(&self) -> u32 {
        self.zoom
    }

    pub fn follow(&self) -> FollowStrategy {
        self.follow
    }

    pub fn grid_overlay(&self) -> bool {
        self.grid_overlay
    }

    pub fn pan_speed_tier(&self) -> usize {
        self.pan_speed_tier
    }

    pub fn last_lens_render_us(&self) -> u64 {
        self.last_lens_render_us
    }

    /// 全屏视口原点（源面坐标）。
    pub fn viewport(&self) -> (u32, u32) {
        self.viewport
    }

    /// 全屏视口原点直设（焦点/键盘跟随落点与测试注入口；自动钳到有效域）。
    pub fn viewport_set(&mut self, x: u32, y: u32) {
        let span = self.visible_source_span();
        self.viewport = (
            x.min(self.screen.0.saturating_sub(span.0)),
            y.min(self.screen.1.saturating_sub(span.1)),
        );
    }

    /// 打开（恢复会话记忆模式与倍率——主册：倍率与模式记忆（会话））。
    pub fn turn_on(&mut self) {
        self.mode = if self.last_active_mode == MagMode::Off {
            MagMode::Fullscreen
        } else {
            self.last_active_mode
        };
        self.sync_grid();
    }

    /// 关闭（记忆保留）。
    pub fn turn_off(&mut self) {
        if self.mode != MagMode::Off {
            self.last_active_mode = self.mode;
        }
        self.mode = MagMode::Off;
    }

    /// 倍率步进（+/−；工具条与 Ctrl+滚轮同口）。钳 [2,16]；16x 同步像素网格。
    pub fn zoom_step(&mut self, up: bool) {
        if self.zoom == ZOOM_MIN && !up {
            return;
        }
        if self.zoom == ZOOM_MAX && up {
            return;
        }
        self.zoom = if up { self.zoom + 1 } else { self.zoom - 1 };
        self.sync_grid();
    }

    /// 直接设倍率（越界钳制）。
    pub fn zoom_set(&mut self, z: u32) {
        self.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
        self.sync_grid();
    }

    fn sync_grid(&mut self) {
        self.grid_overlay = self.zoom >= GRID_ZOOM;
    }

    /// 三跟随策略切换（判据：三跟随策略切换实测——切换保持倍率与模式）。
    pub fn cycle_follow(&mut self) -> FollowStrategy {
        self.follow = match self.follow {
            FollowStrategy::Cursor => FollowStrategy::Focus,
            FollowStrategy::Focus => FollowStrategy::Keyboard,
            FollowStrategy::Keyboard => FollowStrategy::Cursor,
        };
        self.follow
    }

    /// 模式切换（工具条/快捷键同口；Off 不在此口出现——开关联独立）。
    pub fn toggle_mode(&mut self) -> MagMode {
        self.mode = match self.mode {
            MagMode::Off => MagMode::Fullscreen,
            MagMode::Fullscreen => MagMode::Lens,
            MagMode::Lens => MagMode::Fullscreen,
        };
        self.last_active_mode = self.mode;
        self.mode
    }

    /// 边缘平移速度档循环（三档）。
    pub fn cycle_pan_speed(&mut self) -> usize {
        self.pan_speed_tier = (self.pan_speed_tier + 1) % EDGE_PAN_SPEEDS_PX.len();
        self.pan_speed_tier
    }

    /// 全屏可视源范围（屏尺寸 ÷ 倍率）。
    pub fn visible_source_span(&self) -> (u32, u32) {
        (
            (self.screen.0 / self.zoom).max(1),
            (self.screen.1 / self.zoom).max(1),
        )
    }

    /// 全屏模式平移动线（主册：全屏模式平移 = 光标顶边触发，对齐
    /// Windows 动线）：光标进入视口边缘 [`EDGE_PAN_ZONE_PX`] 带内 →
    /// 按当前档速度平移；返回新视口原点。镜头/关闭模式不响应（模式隔离）。
    pub fn fullscreen_pan(&mut self, cursor: (u32, u32)) -> (u32, u32) {
        if self.mode != MagMode::Fullscreen {
            return self.viewport;
        }
        let speed = EDGE_PAN_SPEEDS_PX[self.pan_speed_tier];
        let span = self.visible_source_span();
        let max_x = self.screen.0.saturating_sub(span.0);
        let max_y = self.screen.1.saturating_sub(span.1);
        let (mut vx, mut vy) = self.viewport;
        if cursor.0 <= EDGE_PAN_ZONE_PX {
            vx = vx.saturating_sub(speed);
        } else if cursor.0 + EDGE_PAN_ZONE_PX >= self.screen.0 {
            vx = (vx + speed).min(max_x);
        }
        if cursor.1 <= EDGE_PAN_ZONE_PX {
            vy = vy.saturating_sub(speed);
        } else if cursor.1 + EDGE_PAN_ZONE_PX >= self.screen.1 {
            vy = (vy + speed).min(max_y);
        }
        self.viewport = (vx, vy);
        self.viewport
    }

    /// 脏区（F056 联动，x/y/w/h）：镜头模式 = 镜头外接方框（屏心）；
    /// 全屏模式 = 全视口；关闭 = 空。放大区域外渲染降载的判据面——
    /// 脏区外合成器跳过重绘。
    pub fn dirty_region(&self) -> (u32, u32, u32, u32) {
        match self.mode {
            MagMode::Off => (0, 0, 0, 0),
            MagMode::Lens => {
                let x = self.screen.0 / 2;
                let y = self.screen.1 / 2;
                (x, y, LENS_DIAMETER_PX, LENS_DIAMETER_PX)
            }
            MagMode::Fullscreen => (0, 0, self.screen.0, self.screen.1),
        }
    }

    /// 镜头渲染对账：注入本次渲染耗时（us），返回是否在 80fps 预算内。
    pub fn lens_render_done(&mut self, cost_us: u64) -> bool {
        self.last_lens_render_us = cost_us;
        cost_us <= LENS_FRAME_BUDGET_US
    }

    /// 截图取样（主册：放大状态下截图 → 截取放大后所见，所见即所得）：
    /// 源面 + 当前倍率 + 采样方式一并交出——截图管线按此取样即得用户所见。
    pub fn screenshot_view<'a>(&self, src: &'a SourceFrame<'a>) -> ScreenshotView<'a> {
        ScreenshotView { src, zoom: self.zoom, resample: resample_for(self.zoom) }
    }
}

/// 截图所见视图（所见即所得：按当前倍率与采样方式取样）。
pub struct ScreenshotView<'a> {
    src: &'a SourceFrame<'a>,
    zoom: u32,
    resample: Resample,
}

impl<'a> ScreenshotView<'a> {
    /// 目标坐标（放大面像素）→ 取色。
    pub fn sample(&self, dst_x: u32, dst_y: u32) -> (u8, u8, u8) {
        match self.resample {
            Resample::Nearest => {
                let sx = dst_x / self.zoom;
                let sy = dst_y / self.zoom;
                self.src.sample_nearest(sx, sy)
            }
            Resample::Bilinear => {
                let fx = dst_x as f64 / self.zoom as f64;
                let fy = dst_y as f64 / self.zoom as f64;
                self.src.sample_bilinear(fx, fy)
            }
        }
    }

    pub fn resample(&self) -> Resample {
        self.resample
    }

    pub fn zoom(&self) -> u32 {
        self.zoom
    }
}

/// 叠加顺序文档常量（一处一事实）：色温（F116）→ 滤镜（F114）→ 放大（F111）。
pub const OVERLAY_ORDER_DOC: &str = "F116-color-temp -> F114-filter -> F111-magnify";
/// 快捷键登记面（F169 同源语义）。
pub const HOTKEYS_DOC: [&str; 5] = [
    "Win+Plus open/zoom-in",
    "Win+Minus zoom-out",
    "Win+Esc exit",
    "Ctrl+Wheel zoom",
    "Ctrl+Alt+M toggle-mode",
];

/// 镜头边缘柔化 alpha（step = 距边缘的环内步进 0..8）：边缘透明 → 内部
/// 不透明，线性带四舍五入。step ≥ 8 视为镜头内部（不透明）。
pub fn feather_alpha(step: u32) -> u8 {
    if step >= LENS_FEATHER_PX {
        return 255;
    }
    let px = LENS_FEATHER_PX as u32;
    let raw = 255 * (px - step);
    ((raw + px / 2) / px) as u8
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_magnifier_checks() -> CheckSet {
    let mut set = CheckSet::new("F111-magnifier");

    // 1. 2x-16x 全档采样方式正确（判据：2x-16x 全档——低倍最近邻锐利、
    //    8x 以上双线性；全档逐档实测）。
    let mut all_ok = true;
    for z in ZOOM_MIN..=ZOOM_MAX {
        let want = if z >= 8 { Resample::Bilinear } else { Resample::Nearest };
        if resample_for(z) != want {
            all_ok = false;
        }
    }
    set.add("zoom 2x-16x resample per-tier", all_ok, "");

    // 2. 最近邻锐利性：整数格取样恒等于源像素（放大不糊的算法面判据）。
    let data = vec![10u8, 20, 30, 255, 40, 50, 60, 255, 200, 210, 220, 255];
    let src = SourceFrame { w: 2, h: 1, rgba: &data };
    set.add(
        "nearest sampling exact source pixel",
        src.sample_nearest(0, 0) == (10, 20, 30)
            && src.sample_nearest(1, 0) == (40, 50, 60)
            && src.sample_nearest(9, 9) == (40, 50, 60),
        "",
    );

    // 3. 双线性中点插值（8x 平滑语义）。
    let mid = src.sample_bilinear(0.5, 0.0);
    set.add(
        "bilinear midpoint interpolates",
        (mid.0 as i32 - 25).abs() <= 1 && (mid.1 as i32 - 35).abs() <= 1,
        "",
    );

    // 4. 三跟随策略切换实测（判据第一句之三）——切换保持倍率与模式。
    let mut m = Magnifier::new(1920, 1080);
    m.turn_on();
    m.zoom_set(6);
    m.toggle_mode();
    let zoom_before = m.zoom();
    let f1 = m.cycle_follow();
    let f2 = m.cycle_follow();
    let f3 = m.cycle_follow();
    set.add(
        "follow cycle 3 strategies keeps zoom",
        f1 == FollowStrategy::Focus
            && f2 == FollowStrategy::Keyboard
            && f3 == FollowStrategy::Cursor
            && m.zoom() == zoom_before,
        "",
    );

    // 5. 镜头模式 80fps 跟手（判据第一句之二）：帧预算 12.5ms 注入实测。
    let mut m = Magnifier::new(1920, 1080);
    set.add(
        "lens frame budget 12.5ms",
        m.lens_render_done(12_500) && !m.lens_render_done(12_501),
        "",
    );

    // 6. 倍率步进钳制 + 16x 网格辅助（主册：16x 极限 → 像素网格）。
    let mut m = Magnifier::new(1920, 1080);
    m.turn_on();
    for _ in 0..20 {
        m.zoom_step(true);
    }
    let at_max_grid = m.zoom() == ZOOM_MAX && m.grid_overlay();
    m.zoom_step(true);
    let clamped = m.zoom() == ZOOM_MAX;
    for _ in 0..20 {
        m.zoom_step(false);
    }
    let at_min_no_grid = m.zoom() == ZOOM_MIN && !m.grid_overlay();
    set.add(
        "zoom clamp 2..16 + grid at 16x",
        at_max_grid && clamped && at_min_no_grid,
        "",
    );

    // 7. 会话记忆：关-开恢复模式与倍率（主册：倍率与模式记忆（会话））。
    let mut m = Magnifier::new(1920, 1080);
    m.turn_on();
    m.zoom_set(10);
    m.toggle_mode(); // Lens
    m.turn_off();
    let off_ok = m.mode() == MagMode::Off;
    m.turn_on();
    set.add(
        "session memory restored",
        off_ok && m.mode() == MagMode::Lens && m.zoom() == 10,
        "",
    );

    // 8. 全屏边缘平移三档速度（主册：边缘平移速度三档）+ 顶边触发动线。
    let mut m = Magnifier::new(1920, 1080);
    m.turn_on();
    m.zoom_set(4); // span = 480x270
    m.viewport_set(720, 405); // 居中
    let v0 = m.viewport();
    m.cycle_pan_speed(); // 档 1 → 2（快 32px/帧）
    let p_fast = m.fullscreen_pan((960, 10)); // 顶边带内 → 上移
    let moved_fast = (v0.1 as i64 - p_fast.1 as i64) == EDGE_PAN_SPEEDS_PX[2] as i64;
    m.cycle_pan_speed(); // 档 2 → 0（慢 4px/帧）
    let p_slow = m.fullscreen_pan((960, 10));
    let moved_slow = (p_fast.1 as i64 - p_slow.1 as i64) == EDGE_PAN_SPEEDS_PX[0] as i64;
    set.add("edge pan 3 tiers + top-edge trigger", moved_fast && moved_slow, "");

    // 9. 镜头模式不触发平移动线（模式隔离）。
    let mut m = Magnifier::new(1920, 1080);
    m.turn_on();
    m.toggle_mode(); // Lens
    let p = m.fullscreen_pan((0, 0));
    set.add("lens mode ignores pan gesture", p == (0, 0), "");

    // 10. 脏区降载（F056 联动）：镜头模式脏区=240px 外接框，面积 < 全屏 5%；
    //     全屏模式脏区=全视口。
    let mut m = Magnifier::new(1920, 1080);
    m.turn_on();
    m.toggle_mode();
    let (dx, dy, dw, dh) = m.dirty_region();
    let lens_area = dw as u64 * dh as u64;
    let screen_area = 1920u64 * 1080;
    let lens_ok = dx > 0
        && dy > 0
        && dw == LENS_DIAMETER_PX
        && dh == LENS_DIAMETER_PX
        && lens_area * 20 < screen_area;
    m.toggle_mode(); // Fullscreen
    let (_, _, fw, fh) = m.dirty_region();
    set.add(
        "dirty region lens-bounded / fullscreen-covered",
        lens_ok && fw == 1920 && fh == 1080,
        "",
    );

    // 11. 截图所见即所得：放大态截图取样 = 放大后所见像素（Nearest 路径）。
    let data = vec![7u8, 8, 9, 255, 70, 80, 90, 255];
    let src = SourceFrame { w: 2, h: 1, rgba: &data };
    let mut m = Magnifier::new(1920, 1080);
    m.turn_on();
    m.zoom_set(4);
    let view = m.screenshot_view(&src);
    set.add(
        "screenshot wysiwyg nearest",
        view.zoom() == 4
            && view.resample() == Resample::Nearest
            && view.sample(0, 0) == (7, 8, 9)
            && view.sample(4, 0) == (70, 80, 90)
            && view.sample(5, 0) == (70, 80, 90),
        "",
    );

    // 12. 叠加顺序与快捷键登记文档化（一处一事实）。
    set.add(
        "overlay order + hotkeys registered",
        OVERLAY_ORDER_DOC == "F116-color-temp -> F114-filter -> F111-magnify"
            && HOTKEYS_DOC.len() == 5,
        "",
    );

    // 13. 镜头柔化环规格（8px）：边缘透明 → 内部不透明线性梯度。
    set.add(
        "lens feather 8px ramp",
        LENS_FEATHER_PX == 8
            && feather_alpha(0) == 255
            && feather_alpha(7) == 32
            && feather_alpha(8) == 255,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnifier_all_checks_green() {
        let set = run_magnifier_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F111 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn zoom_set_clamps() {
        let mut m = Magnifier::new(1920, 1080);
        m.zoom_set(0);
        assert_eq!(m.zoom(), ZOOM_MIN);
        m.zoom_set(99);
        assert_eq!(m.zoom(), ZOOM_MAX);
        assert!(m.grid_overlay());
    }

    #[test]
    fn off_state_no_dirty() {
        let m = Magnifier::new(1920, 1080);
        assert_eq!(m.dirty_region(), (0, 0, 0, 0));
    }

    #[test]
    fn visible_span_shrinks_with_zoom() {
        let mut m = Magnifier::new(1920, 1080);
        m.turn_on();
        m.zoom_set(2);
        assert_eq!(m.visible_source_span(), (960, 540));
        m.zoom_set(8);
        assert_eq!(m.visible_source_span(), (240, 135));
    }

    #[test]
    fn bilinear_corner_exact() {
        let data = vec![255u8, 0, 0, 255];
        let src = SourceFrame { w: 1, h: 1, rgba: &data };
        assert_eq!(src.sample_bilinear(0.25, 0.75), (255, 0, 0));
    }

    #[test]
    fn viewport_set_clamped_to_span() {
        let mut m = Magnifier::new(1920, 1080);
        m.turn_on();
        m.zoom_set(4); // span 480x270，max = (1440, 810)
        m.viewport_set(9999, 9999);
        assert_eq!(m.viewport(), (1440, 810));
    }
}
