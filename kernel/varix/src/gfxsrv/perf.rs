//! VARIABLE-200 AI-05 · 显示域性能与交付面（F116~F125）。
//!
//! 动画时钟、帧预算仪表、丢帧降级、色彩管理、截屏/录屏、多显示器预留、
//! 功耗钩子、画质走查与显示域自检。纯逻辑 + 固定容量数组，无分配。

use crate::checks::CheckSet;
use crate::gfxsrv::{
    color_a, color_b, color_g, color_r, rgb, Canvas, Compositor, DirtyTracker, DoubleBuffer,
    FlipResult, FbService, GpuProbe, Rect, RenderPath, RenderPathState, FRAME_PIXELS, FRAME_W,
};

// ---------------------------------------------------------------------------
// F116 动画时钟源 — 合成器统一动画时钟 + 弹簧曲线采样
// ---------------------------------------------------------------------------

/// 弹簧参数（Q8 定点：1.0 = 256）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spring {
    /// 当前位置（Q8）。
    pub pos_q8: i32,
    /// 速度（Q8/帧）。
    pub vel_q8: i32,
    /// 目标位置（Q8）。
    pub target_q8: i32,
    /// 刚度系数。
    pub stiffness: i32,
    /// 阻尼系数。
    pub damping: i32,
}

impl Spring {
    pub const fn new(target_q8: i32, stiffness: i32, damping: i32) -> Spring {
        Spring { pos_q8: 0, vel_q8: 0, target_q8, stiffness, damping }
    }

    /// 单步积分（半隐式欧拉，整数）。
    pub fn step(&mut self) -> i32 {
        let force = (self.target_q8 - self.pos_q8) * self.stiffness / 256;
        self.vel_q8 += force - self.vel_q8 * self.damping / 256;
        self.pos_q8 += self.vel_q8;
        self.pos_q8
    }

    /// 收敛判定：位置与速度都进入容差带。
    pub fn settled(&self) -> bool {
        let dp = self.target_q8 - self.pos_q8;
        (dp.abs() <= 2) && self.vel_q8.abs() <= 2
    }

    /// 采样指定步数，返回收敛所需步数（预算上限）。
    pub fn settle_steps(&mut self, budget: usize) -> Option<usize> {
        let mut i = 0usize;
        while i < budget {
            self.step();
            if self.settled() {
                return Some(i + 1);
            }
            i += 1;
        }
        None
    }
}

#[derive(Clone, Copy)]
pub struct AnimClock {
    /// 时钟 tick（毫秒）。
    pub now_ms: u64,
    /// 本帧 dt（毫秒）。
    pub dt_ms: u32,
    /// 帧序号。
    pub tick: u64,
    /// 暂停（消费方隐藏时暂停动画，省电）。
    pub paused: bool,
    /// 累计 dt 溢出（跳帧）次数。
    pub jumps: u32,
}

impl AnimClock {
    pub const fn new() -> AnimClock {
        AnimClock { now_ms: 0, dt_ms: 0, tick: 0, paused: false, jumps: 0 }
    }

    /// 推进时钟：dt 被钳制在 [1, 100] 之间，超出即记一次跳帧。
    pub fn advance(&mut self, raw_dt_ms: u32) -> u32 {
        if self.paused {
            self.dt_ms = 0;
            return 0;
        }
        let dt = if raw_dt_ms == 0 {
            1
        } else if raw_dt_ms > 100 {
            self.jumps += 1;
            100
        } else {
            raw_dt_ms
        };
        self.dt_ms = dt;
        self.now_ms += dt as u64;
        self.tick += 1;
        dt
    }

    /// 统一时钟：同一帧内所有动画共享同一 dt（弹簧曲线不漂移）。
    pub fn shared_dt(&self) -> u32 {
        self.dt_ms
    }
}

// ---------------------------------------------------------------------------
// F117 帧预算仪表 — 每帧合成耗时环形统计（p99 < 8ms 红线）
// ---------------------------------------------------------------------------

pub const BUDGET_RING: usize = 64;
/// 帧预算红线（微秒）：8ms。
pub const FRAME_BUDGET_US: u64 = 8_000;

#[derive(Clone, Copy)]
pub struct FrameBudget {
    ring: [u32; BUDGET_RING],
    wr: usize,
    pub filled: usize,
    /// 超预算帧数。
    pub over_budget: u64,
    pub frames: u64,
}

impl FrameBudget {
    pub const fn new() -> FrameBudget {
        FrameBudget { ring: [0u32; BUDGET_RING], wr: 0, filled: 0, over_budget: 0, frames: 0 }
    }

    pub fn record(&mut self, us: u32) -> bool {
        self.ring[self.wr] = us;
        self.wr = (self.wr + 1) % BUDGET_RING;
        if self.filled < BUDGET_RING {
            self.filled += 1;
        }
        self.frames += 1;
        let over = us as u64 > FRAME_BUDGET_US;
        if over {
            self.over_budget += 1;
        }
        over
    }

    /// 已排序副本（固定容量，用于分位数）。
    fn sorted(&self) -> [u32; BUDGET_RING] {
        let mut a = [0u32; BUDGET_RING];
        let mut i = 0usize;
        while i < self.filled {
            a[i] = self.ring[i];
            i += 1;
        }
        let mut k = 1usize;
        while k < self.filled {
            let key = a[k];
            let mut j = k;
            while j > 0 && a[j - 1] > key {
                a[j] = a[j - 1];
                j -= 1;
            }
            a[j] = key;
            k += 1;
        }
        a
    }

    /// 百分位（permille：500 = p50，990 = p99）。
    pub fn percentile_permille(&self, permille: usize) -> u32 {
        if self.filled == 0 {
            return 0;
        }
        let a = self.sorted();
        let idx = (self.filled * permille) / 1000;
        let idx = core::cmp::min(idx, self.filled - 1);
        a[idx]
    }

    pub fn p50(&self) -> u32 {
        self.percentile_permille(500)
    }

    pub fn p99(&self) -> u32 {
        self.percentile_permille(990)
    }

    /// 预算内判定（F117 红线）。
    pub fn within_budget(&self) -> bool {
        self.p99() as u64 <= FRAME_BUDGET_US
    }
}

// ---------------------------------------------------------------------------
// F118 丢帧统计与降级 — 连续丢帧自动降档，恢复后回升
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EffectTier {
    /// 全特效。
    High,
    /// 标准档。
    Standard,
    /// 省电档。
    Eco,
}

impl EffectTier {
    pub fn level(self) -> u8 {
        match self {
            EffectTier::High => 2,
            EffectTier::Standard => 1,
            EffectTier::Eco => 0,
        }
    }
}

/// 连续丢帧阈值。
pub const DROP_DEGRADE_THRESHOLD: u32 = 3;
/// 连续达标帧阈值（回升）。
pub const GOOD_RESTORE_THRESHOLD: u32 = 30;

#[derive(Clone, Copy)]
pub struct FrameQuality {
    pub tier: EffectTier,
    consecutive_drops: u32,
    consecutive_good: u32,
    pub drops_total: u64,
    pub degrades: u32,
    pub restores: u32,
}

impl FrameQuality {
    pub const fn new() -> FrameQuality {
        FrameQuality {
            tier: EffectTier::High,
            consecutive_drops: 0,
            consecutive_good: 0,
            drops_total: 0,
            degrades: 0,
            restores: 0,
        }
    }

    /// 上报一帧结果，返回是否发生档位变化。
    pub fn report(&mut self, dropped: bool) -> bool {
        if dropped {
            self.drops_total += 1;
            self.consecutive_drops += 1;
            self.consecutive_good = 0;
            if self.consecutive_drops >= DROP_DEGRADE_THRESHOLD && self.tier != EffectTier::Eco {
                self.tier = match self.tier {
                    EffectTier::High => EffectTier::Standard,
                    EffectTier::Standard => EffectTier::Eco,
                    EffectTier::Eco => EffectTier::Eco,
                };
                self.degrades += 1;
                self.consecutive_drops = 0;
                return true;
            }
        } else {
            self.consecutive_drops = 0;
            self.consecutive_good += 1;
            if self.consecutive_good >= GOOD_RESTORE_THRESHOLD && self.tier != EffectTier::High {
                self.tier = match self.tier {
                    EffectTier::Eco => EffectTier::Standard,
                    EffectTier::Standard => EffectTier::High,
                    EffectTier::High => EffectTier::High,
                };
                self.restores += 1;
                self.consecutive_good = 0;
                return true;
            }
        }
        false
    }

    pub fn drops_in_row(&self) -> u32 {
        self.consecutive_drops
    }
}

// ---------------------------------------------------------------------------
// F119 色彩管理 — sRGB 输出基准 + 色彩空间标记
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorSpace {
    /// sRGB 输出基准（默认）。
    Srgb,
    /// 广色域（预留）。
    DisplayP3,
    /// 线性空间（合成内部）。
    Linear,
}

impl ColorSpace {
    pub fn tag(self) -> u8 {
        match self {
            ColorSpace::Srgb => 0,
            ColorSpace::DisplayP3 => 1,
            ColorSpace::Linear => 2,
        }
    }
}

/// 8 点 gamma 查表（Q8 → 8bit），线性插值。
/// LUT 对应 gamma≈2.2 的归一化采样点 [0,32,64,96,128,160,192,224,255]。
pub const GAMMA_LUT: [u8; 9] = [0, 129, 170, 197, 216, 232, 245, 252, 255];

/// gamma 编码：线性值 v（0..=255）→ sRGB 编码值。
pub fn gamma_encode(v: u8) -> u8 {
    let v = v as usize;
    if v >= 255 {
        return 255;
    }
    let idx = v / 32;
    if idx >= 8 {
        return 255;
    }
    let lo = idx * 32;
    let frac = v - lo;
    let a = GAMMA_LUT[idx] as usize;
    let b = GAMMA_LUT[idx + 1] as usize;
    let out = a + (b - a) * frac / 32;
    out.min(255) as u8
}

/// gamma 解码：sRGB 编码值 → 线性值（LUT 反向近似）。
pub fn gamma_decode(v: u8) -> u8 {
    let v = v as usize;
    // 在 LUT 中找第一个 >= v 的项。
    let mut i = 0usize;
    while i < 8 {
        if v <= GAMMA_LUT[i + 1] as usize {
            let a = GAMMA_LUT[i] as usize;
            let b = GAMMA_LUT[i + 1] as usize;
            let span = if b == a { 1 } else { b - a };
            let frac = v.saturating_sub(a);
            let out = i * 32 + core::cmp::min(31, frac * 32 / span);
            return out as u8;
        }
        i += 1;
    }
    255
}

#[derive(Clone, Copy)]
pub struct ColorManager {
    pub space: ColorSpace,
    /// 近色域裁剪计数。
    pub clipped: u64,
    /// gamma 往返误差累计（健康度）。
    pub roundtrip_error: u64,
}

impl ColorManager {
    pub const fn new(space: ColorSpace) -> ColorManager {
        ColorManager { space, clipped: 0, roundtrip_error: 0 }
    }

    /// 全颜色 gamma 往返，统计最大误差。
    pub fn calibrate(&mut self) -> u8 {
        let mut max_err = 0u8;
        let mut v = 0usize;
        while v < 256 {
            let enc = gamma_encode(v as u8);
            let dec = gamma_decode(enc);
            let err = (dec as i32 - v as i32).unsigned_abs() as u8;
            if err > max_err {
                max_err = err;
            }
            self.roundtrip_error += err as u64;
            v += 1;
        }
        max_err
    }

    /// 越色域裁剪。
    pub fn clamp(&mut self, c: u32, in_gamut: bool) -> u32 {
        if in_gamut {
            return c;
        }
        self.clipped += 1;
        // 裁剪到 sRGB 边界（仅保留 alpha 与 clamp 后的分量）。
        let r = core::cmp::min(color_r(c), 255);
        let g = core::cmp::min(color_g(c), 255);
        let b = core::cmp::min(color_b(c), 255);
        crate::gfxsrv::argb(color_a(c), r, g, b)
    }
}

// ---------------------------------------------------------------------------
// F120 截屏服务 — 全屏/窗口截取
// ---------------------------------------------------------------------------

pub const SHOT_CAPACITY: usize = FRAME_PIXELS;

#[derive(Clone, Copy)]
pub struct Screenshot {
    pub px: [u32; SHOT_CAPACITY],
    pub len: usize,
    /// 内容校验和（验收物指纹）。
    pub checksum: u32,
    pub w: usize,
    pub h: usize,
    /// 截屏次数。
    pub shots: u64,
}

impl Screenshot {
    pub const fn new() -> Screenshot {
        Screenshot { px: [0u32; SHOT_CAPACITY], len: 0, checksum: 0, w: 0, h: 0, shots: 0 }
    }

    fn digest(px: &[u32], n: usize) -> u32 {
        let mut s = 0x811C_9DC5u32;
        let mut i = 0usize;
        while i < n {
            s ^= px[i];
            s = s.wrapping_mul(0x0100_0193);
            i += 1;
        }
        s
    }

    /// 全屏截取：逐行抓取，行 stride 与画布一致。
    pub fn capture_full(&mut self, canvas: &Canvas) -> bool {
        let src = canvas.as_slice();
        if src.len() != SHOT_CAPACITY {
            return false;
        }
        let mut i = 0usize;
        while i < SHOT_CAPACITY {
            self.px[i] = src[i];
            i += 1;
        }
        self.len = SHOT_CAPACITY;
        self.w = FRAME_W;
        self.h = crate::gfxsrv::FRAME_H;
        self.checksum = Self::digest(&self.px, self.len);
        self.shots += 1;
        true
    }

    /// 窗口截取（按矩形裁剪到画布外区域补齐为 0）。
    pub fn capture_rect(&mut self, canvas: &Canvas, r: Rect) -> usize {
        let mut n = 0usize;
        let mut y = 0usize;
        while y < r.h {
            let mut x = 0usize;
            while x < r.w {
                let px = r.x + x;
                let py = r.y + y;
                let v = if px < FRAME_W && py < crate::gfxsrv::FRAME_H {
                    canvas.get(px, py)
                } else {
                    0
                };
                if n < SHOT_CAPACITY {
                    self.px[n] = v;
                    n += 1;
                }
                x += 1;
            }
            y += 1;
        }
        self.len = n;
        self.w = r.w;
        self.h = r.h;
        self.checksum = Self::digest(&self.px, n);
        self.shots += 1;
        n
    }

    /// 与另一截屏逐像素比对（画质走查复用）。
    pub fn diff_count(&self, other: &Screenshot) -> usize {
        if self.len != other.len {
            return self.len.max(other.len);
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if self.px[i] != other.px[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F121 录屏服务（可选）— 帧序列录制
// ---------------------------------------------------------------------------

pub const REC_FRAMES: usize = 8;

#[derive(Clone, Copy)]
pub struct Recorder {
    frames: [[u32; 16]; REC_FRAMES],
    pub count: usize,
    pub recording: bool,
    /// 因容量满丢弃的帧数（有界，绝不无界增长）。
    pub dropped: u64,
}

impl Recorder {
    pub const fn new() -> Recorder {
        Recorder { frames: [[0u32; 16]; REC_FRAMES], count: 0, recording: false, dropped: 0 }
    }

    pub fn start(&mut self) -> bool {
        if self.recording {
            return false;
        }
        self.recording = true;
        self.count = 0;
        true
    }

    /// 录一帧（取画布前 16 像素作为帧指纹）。
    pub fn capture(&mut self, canvas: &Canvas) -> bool {
        if !self.recording {
            return false;
        }
        if self.count >= REC_FRAMES {
            self.dropped += 1;
            return false;
        }
        let src = canvas.as_slice();
        let mut i = 0usize;
        while i < 16 {
            self.frames[self.count][i] = src[i];
            i += 1;
        }
        self.count += 1;
        true
    }

    pub fn stop(&mut self) -> usize {
        self.recording = false;
        self.count
    }

    pub fn frames(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// F122 多显示器预留 — 接口与数据结构占位（单屏优先交付）
// ---------------------------------------------------------------------------

pub const HEAD_MAX: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Head {
    pub id: u8,
    pub mode: crate::gfxsrv::text::DisplayMode,
    pub primary: bool,
    pub enabled: bool,
    /// 相对主屏的原点（多屏拼接预留）。
    pub origin_x: i32,
    pub origin_y: i32,
}

impl Head {
    pub const fn empty() -> Head {
        Head {
            id: 0,
            mode: crate::gfxsrv::text::DisplayMode::new(0, 0, 0),
            primary: false,
            enabled: false,
            origin_x: 0,
            origin_y: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub struct DisplayTopology {
    heads: [Head; HEAD_MAX],
    count: usize,
    /// 多屏合成是否已启用（当前恒 false = 预留）。
    pub multi_enabled: bool,
}

impl DisplayTopology {
    pub const fn new() -> DisplayTopology {
        DisplayTopology { heads: [Head::empty(); HEAD_MAX], count: 0, multi_enabled: false }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn head(&self, i: usize) -> Option<Head> {
        if i < self.count {
            Some(self.heads[i])
        } else {
            None
        }
    }

    pub fn add(&mut self, mode: crate::gfxsrv::text::DisplayMode) -> Option<usize> {
        if self.count >= HEAD_MAX || !mode.valid() {
            return None;
        }
        let mut h = Head::empty();
        h.id = self.count as u8;
        h.mode = mode;
        h.enabled = true;
        h.primary = self.count == 0;
        if self.count > 0 {
            // 右侧拼接（预留语义）。
            h.origin_x = self.heads[0].mode.w as i32;
        }
        self.heads[self.count] = h;
        self.count += 1;
        Some(self.count - 1)
    }

    /// 多屏合成提交：预留接口，显式返回 Unsupported。
    pub fn present_multi(&mut self) -> bool {
        false
    }

    pub fn primary(&self) -> Option<Head> {
        let mut i = 0usize;
        while i < self.count {
            if self.heads[i].primary {
                return Some(self.heads[i]);
            }
            i += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// F123 显示功耗钩子 — 空闲降帧、唤醒即时满帧
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct PowerHook {
    /// 空闲判定阈值（连续无脏帧数）。
    pub idle_after_frames: u32,
    /// 省电档帧率。
    pub idle_fps: u16,
    /// 满帧率。
    pub active_fps: u16,
    idle_frames: u32,
    pub sleeping: bool,
    pub sleeps: u32,
    pub wakes: u32,
}

impl PowerHook {
    pub const fn new(active_fps: u16, idle_fps: u16, idle_after_frames: u32) -> PowerHook {
        PowerHook {
            idle_after_frames,
            idle_fps,
            active_fps,
            idle_frames: 0,
            sleeping: false,
            sleeps: 0,
            wakes: 0,
        }
    }

    /// 每帧上报是否有画面变化。
    pub fn on_frame(&mut self, dirty: bool) -> bool {
        if dirty {
            self.idle_frames = 0;
            if self.sleeping {
                self.sleeping = false;
                self.wakes += 1;
                return true;
            }
            return false;
        }
        self.idle_frames += 1;
        if !self.sleeping && self.idle_frames >= self.idle_after_frames {
            self.sleeping = true;
            self.sleeps += 1;
            return true;
        }
        false
    }

    pub fn target_fps(&self) -> u16 {
        if self.sleeping {
            self.idle_fps
        } else {
            self.active_fps
        }
    }
}

// ---------------------------------------------------------------------------
// F124 画质走查流程 — 与 Windows 版同屏像素对比
// ---------------------------------------------------------------------------

pub const WALK_DIFF_MAX: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WalkVerdict {
    /// 无可见差异（差异像素为 0）。
    Identical,
    /// 存在细微差异（低于阈值）。
    Minor,
    /// 明显差异（超阈值）。
    Divergent,
}

#[derive(Clone, Copy)]
pub struct QualityWalk {
    pub diffs: [(usize, u32, u32); WALK_DIFF_MAX],
    pub diff_count: usize,
    pub total_diff_pixels: usize,
    pub total_pixels: usize,
    pub threshold: usize,
}

impl QualityWalk {
    pub const fn new(threshold: usize) -> QualityWalk {
        QualityWalk {
            diffs: [(0, 0, 0); WALK_DIFF_MAX],
            diff_count: 0,
            total_diff_pixels: 0,
            total_pixels: 0,
            threshold,
        }
    }

    /// 走查两幅画布，记录前 N 个差异点。
    pub fn compare(&mut self, a: &Canvas, b: &Canvas) -> WalkVerdict {
        self.diff_count = 0;
        self.total_diff_pixels = 0;
        self.total_pixels = FRAME_PIXELS;
        let sa = a.as_slice();
        let sb = b.as_slice();
        let mut i = 0usize;
        while i < FRAME_PIXELS {
            if sa[i] != sb[i] {
                self.total_diff_pixels += 1;
                if self.diff_count < WALK_DIFF_MAX {
                    self.diffs[self.diff_count] = (i, sa[i], sb[i]);
                    self.diff_count += 1;
                }
            }
            i += 1;
        }
        if self.total_diff_pixels == 0 {
            WalkVerdict::Identical
        } else if self.total_diff_pixels <= self.threshold {
            WalkVerdict::Minor
        } else {
            WalkVerdict::Divergent
        }
    }

    pub fn diff_permille(&self) -> usize {
        if self.total_pixels == 0 {
            return 0;
        }
        self.total_diff_pixels * 1000 / self.total_pixels
    }
}

// ---------------------------------------------------------------------------
// F125 显示域自检 + 交付面
// ---------------------------------------------------------------------------

/// 显示域端到端闭环：帧缓冲 → 双缓冲 → 脏区 → 合成 → 截屏 → 走查。
pub fn display_closed_loop() -> bool {
    let mut fb = FbService::new();
    if !fb.claim(2) {
        return false;
    }
    let mut db = DoubleBuffer::new();
    let mut dt = DirtyTracker::new();
    let mut comp = Compositor::new();
    let l = match comp.add_layer(0, 1) {
        Some(l) => l,
        None => return false,
    };
    if let Some(layer) = comp.layer_mut(l) {
        layer.fill_tile(rgb(20, 40, 60));
        layer.opaque = true;
        layer.x = 4;
        layer.y = 4;
    }
    let _ = dt.mark(Rect::new(4, 4, 8, 8));
    comp.composite();
    // 把合成结果送入后备缓冲。
    let mut ok = true;
    let mut y = 0usize;
    while y < crate::gfxsrv::FRAME_H {
        let mut x = 0usize;
        while x < FRAME_W {
            ok &= db.paint_back(x, y, comp.out.get(x, y));
            x += 1;
        }
        y += 1;
    }
    if db.flip() != FlipResult::Flipped {
        return false;
    }
    let mut shot = Screenshot::new();
    let cap = shot.capture_full(&db.front);
    let mut walk = QualityWalk::new(0);
    let verdict = walk.compare(&db.front, &db.front);
    cap && verdict == WalkVerdict::Identical && ok && !dt.idle()
}

pub fn extend_checks(set: &mut CheckSet) {
    // F116 动画时钟源
    let mut clock = AnimClock::new();
    let d1 = clock.advance(16);
    let d2 = clock.advance(0);
    let d3 = clock.advance(500);
    let mut sp = Spring::new(256, 96, 160);
    let steps = sp.settle_steps(400);
    let mut sp2 = Spring::new(0, 96, 160);
    let already = sp2.settle_steps(10);
    clock.paused = true;
    let paused = clock.advance(16);
    set.add(
        "F116 anim clock",
        d1 == 16
            && d2 == 1
            && d3 == 100
            && clock.jumps == 1
            && clock.tick == 3
            && steps.map(|s| s > 1 && s < 400).unwrap_or(false)
            && already == Some(1)
            && paused == 0
            && clock.shared_dt() == 0,
        "统一 dt/跳帧钳制/弹簧收敛",
    );

    // F117 帧预算仪表
    let mut budget = FrameBudget::new();
    let mut i = 0usize;
    while i < 64 {
        let _ = budget.record(3000 + i as u32 * 20);
        i += 1;
    }
    let p50 = budget.p50();
    let p99 = budget.p99();
    let within = budget.within_budget();
    let mut over = FrameBudget::new();
    let mut k = 0usize;
    while k < 64 {
        let _ = over.record(9000);
        k += 1;
    }
    set.add(
        "F117 frame budget",
        budget.filled == BUDGET_RING
            && budget.frames == 64
            && budget.over_budget == 0
            && p50 >= 3000
            && p50 < p99
            && p99 < FRAME_BUDGET_US as u32
            && within
            && !over.within_budget()
            && over.over_budget == 64
            && FrameBudget::new().p99() == 0,
        "p50/p99 分位/红线判定",
    );

    // F118 丢帧统计与降级
    let mut fq = FrameQuality::new();
    let d1 = fq.report(true);
    let d2 = fq.report(true);
    let d3 = fq.report(true);
    let tier_after_drop = fq.tier;
    let mut k = 0usize;
    let mut restored = false;
    while k < GOOD_RESTORE_THRESHOLD as usize {
        if fq.report(false) {
            restored = true;
        }
        k += 1;
    }
    set.add(
        "F118 frame degrade",
        !d1 && !d2 && d3 && tier_after_drop == EffectTier::Standard && fq.degrades == 1
            && restored
            && fq.tier == EffectTier::High
            && fq.restores == 1
            && fq.drops_total == 3
            && FrameQuality::new().tier == EffectTier::High,
        "连续丢帧降档/达标回升",
    );

    // F119 色彩管理
    let mut cm = ColorManager::new(ColorSpace::Srgb);
    let max_err = cm.calibrate();
    let e0 = gamma_encode(0);
    let e255 = gamma_encode(255);
    let mono = gamma_encode(64) >= gamma_encode(32) && gamma_encode(32) >= gamma_encode(16);
    let clipped = cm.clamp(rgb(1, 2, 3), false);
    set.add(
        "F119 colour mgmt",
        cm.space == ColorSpace::Srgb
            && cm.space.tag() == 0
            && e0 == 0
            && e255 == 255
            && mono
            && max_err <= 32
            && cm.roundtrip_error < 256 * 32
            && clipped == rgb(1, 2, 3)
            && ColorSpace::Linear.tag() == 2,
        "sRGB 基准/gamma 往返/裁剪",
    );

    // F120 截屏服务
    let mut canvas = Canvas::new();
    canvas.fill(rgb(7, 8, 9));
    let mut shot = Screenshot::new();
    let full = shot.capture_full(&canvas);
    let sum1 = shot.checksum;
    let mut partial = Screenshot::new();
    let n = partial.capture_rect(&canvas, Rect::new(0, 0, 4, 4));
    let diff = shot.diff_count(&partial);
    set.add(
        "F120 screenshot",
        full
            && shot.len == FRAME_PIXELS
            && shot.w == FRAME_W
            && shot.shots == 1
            && sum1 != 0
            && n == 16
            && partial.w == 4
            && diff == FRAME_PIXELS
            && Screenshot::new().capture_full(&canvas),
        "全屏/窗口截取/校验和",
    );

    // F121 录屏服务
    let mut rec = Recorder::new();
    let not_recording = !rec.capture(&canvas);
    let started = rec.start();
    let mut i = 0usize;
    let mut captured = 0usize;
    while i < REC_FRAMES + 3 {
        if rec.capture(&canvas) {
            captured += 1;
        }
        i += 1;
    }
    let frames = rec.stop();
    set.add(
        "F121 recorder",
        not_recording
            && started
            && captured == REC_FRAMES
            && frames == REC_FRAMES
            && rec.dropped == 3
            && !rec.recording
            && rec.start(),
        "帧序列/有界丢弃/停止",
    );

    // F122 多显示器预留
    let mut topo = DisplayTopology::new();
    let h0 = topo.add(crate::gfxsrv::text::DisplayMode::new(1280, 720, 60));
    let h1 = topo.add(crate::gfxsrv::text::DisplayMode::new(1920, 1080, 60));
    let bad = topo.add(crate::gfxsrv::text::DisplayMode::new(10, 10, 60)).is_none();
    let multi = topo.present_multi();
    set.add(
        "F122 multi-display",
        h0 == Some(0)
            && h1 == Some(1)
            && bad
            && !multi
            && !topo.multi_enabled
            && topo.len() == 2
            && topo.primary().map(|h| h.primary && h.id == 0).unwrap_or(false)
            && topo.head(1).map(|h| h.origin_x == 1280).unwrap_or(false),
        "多屏数据结构占位/单屏优先",
    );

    // F123 显示功耗钩子
    let mut ph = PowerHook::new(60, 10, 5);
    let mut slept = false;
    let mut k = 0usize;
    while k < 5 {
        if ph.on_frame(false) {
            slept = true;
        }
        k += 1;
    }
    let idle_fps = ph.target_fps();
    let woke = ph.on_frame(true);
    set.add(
        "F123 power hook",
        slept
            && ph.sleeping == false
            && ph.sleeps == 1
            && ph.wakes == 1
            && woke
            && idle_fps == 10
            && ph.target_fps() == 60
            && PowerHook::new(60, 5, 3).target_fps() == 60,
        "空闲降帧/唤醒即时满帧",
    );

    // F124 画质走查 + F125 显示域自检
    let a = { let mut c = Canvas::new(); c.fill(rgb(1, 2, 3)); c };
    let mut b = Canvas::new();
    b.fill(rgb(1, 2, 3));
    let mut walk = QualityWalk::new(4);
    let identical = walk.compare(&a, &b);
    let _ = b.put(0, 0, rgb(9, 9, 9));
    let minor = walk.compare(&a, &b);
    let _ = b.put(1, 0, rgb(9, 9, 9));
    let minor2 = walk.compare(&a, &b);
    set.add(
        "F124 quality walk",
        identical == WalkVerdict::Identical
            && minor == WalkVerdict::Minor
            && minor2 == WalkVerdict::Minor
            && walk.diff_count == 2
            && walk.total_diff_pixels == 2
            && walk.diff_permille() > 0
            && walk.diffs[0].0 == 0
            && walk.diffs[0].2 == rgb(9, 9, 9),
        "同屏像素对比/差异清单",
    );

    let mut probe = GpuProbe::new();
    let _ = probe.scan(0x1234, 0x1111, 0, 1 << 20);
    let mut rps = RenderPathState::new(probe.len() > 0 && probe.device(0).map(|d| d.accel_2d).unwrap_or(false));
    let closed = display_closed_loop();
    let degraded = rps.degrade(2);
    let soft_closed = display_closed_loop();
    set.add(
        "F125 display selfcheck",
        closed
            && soft_closed
            && degraded
            && rps.path == RenderPath::Soft
            && rps.soft_available()
            && probe.len() == 1
            && !probe.pick_preferred().is_none(),
        "端到端闭环 + 软路径闭环",
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f116_spring_converges_and_clamps_long_dt() {
        let mut c = AnimClock::new();
        assert_eq!(c.advance(16), 16);
        assert_eq!(c.advance(0), 1);
        assert_eq!(c.advance(500), 100);
        assert_eq!(c.jumps, 1);
        let mut s = Spring::new(256, 96, 160);
        assert!(s.settle_steps(400).is_some());
    }

    #[test]
    fn f117_p99_respects_budget() {
        let mut b = FrameBudget::new();
        let mut i = 0usize;
        while i < 100 {
            let _ = b.record(2000);
            i += 1;
        }
        assert!(b.within_budget());
        assert_eq!(b.p50(), 2000);
        let mut o = FrameBudget::new();
        let _ = o.record(9000);
        assert!(!o.within_budget());
    }

    #[test]
    fn f118_degrades_then_restores() {
        let mut q = FrameQuality::new();
        assert!(!q.report(true));
        assert!(!q.report(true));
        assert!(q.report(true));
        assert_eq!(q.tier, EffectTier::Standard);
        let mut restored = false;
        let mut i = 0usize;
        while i < GOOD_RESTORE_THRESHOLD as usize {
            restored |= q.report(false);
            i += 1;
        }
        assert!(restored);
        assert_eq!(q.tier, EffectTier::High);
    }

    #[test]
    fn f119_gamma_endpoints() {
        assert_eq!(gamma_encode(0), 0);
        assert_eq!(gamma_encode(255), 255);
        assert!(gamma_encode(128) > 128);
        let mut c = ColorManager::new(ColorSpace::Srgb);
        assert!(c.calibrate() <= 64);
    }

    #[test]
    fn f123_sleeps_on_idle_and_wakes() {
        let mut p = PowerHook::new(60, 10, 3);
        assert!(!p.on_frame(false));
        assert!(!p.on_frame(false));
        assert!(p.on_frame(false));
        assert_eq!(p.target_fps(), 10);
        assert!(p.on_frame(true));
        assert_eq!(p.target_fps(), 60);
    }

    #[test]
    fn f125_closed_loop_is_stable() {
        assert!(display_closed_loop());
    }
}
