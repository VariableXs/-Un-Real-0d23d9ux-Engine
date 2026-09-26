//! VARIABLE-200 AI-05 · 图形显示服务域（F101~F125，W3）。
//!
//! 给 AURORA 一块不输 Windows 的画布：帧缓冲独占、双缓冲合成、脏区、
//! VSync、分层合成优化、软合成保底、GPU 探测、字体/图标/令牌管线、
//! DPI 与分辨率热切换、帧预算与丢帧降级、截屏录屏、多显示器与功耗钩子。
//!
//! 纪律：纯逻辑 + 固定容量数组；无 `Vec`/`String`/`Box`/`alloc`/外部 crate。

use crate::checks::CheckSet;

pub mod perf;
pub mod text;

/// VARIX-M500 AI-06 合成与视觉深化（F126~F150）。
pub mod vision;

// ---------------------------------------------------------------------------
// 画布基础：32×24 演示画布（逻辑等价，容量受控便于自检）
// ---------------------------------------------------------------------------

pub const FRAME_W: usize = 32;
pub const FRAME_H: usize = 24;
pub const FRAME_PIXELS: usize = FRAME_W * FRAME_H;

/// 像素格式 0xAARRGGBB。
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub const fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

pub const fn color_a(c: u32) -> u8 {
    (c >> 24) as u8
}
pub const fn color_r(c: u32) -> u8 {
    (c >> 16) as u8
}
pub const fn color_g(c: u32) -> u8 {
    (c >> 8) as u8
}
pub const fn color_b(c: u32) -> u8 {
    c as u8
}

/// `src` 以 `alpha`（0..=255）覆盖到 `dst` 之上（整数混合，无浮点）。
pub fn blend_over(dst: u32, src: u32, alpha: u8) -> u32 {
    let a = ((color_a(src) as u32 * alpha as u32) / 255) as u8;
    if a == 255 {
        return (src & 0x00FF_FFFF) | 0xFF00_0000;
    }
    if a == 0 {
        return dst;
    }
    let ia = 255u32 - a as u32;
    let r = (color_r(src) as u32 * a as u32 + color_r(dst) as u32 * ia) / 255;
    let g = (color_g(src) as u32 * a as u32 + color_g(dst) as u32 * ia) / 255;
    let b = (color_b(src) as u32 * a as u32 + color_b(dst) as u32 * ia) / 255;
    argb(255, r as u8, g as u8, b as u8)
}

#[derive(Clone, Copy)]
pub struct Canvas {
    px: [u32; FRAME_PIXELS],
}

impl Canvas {
    pub const fn new() -> Canvas {
        Canvas { px: [0u32; FRAME_PIXELS] }
    }

    pub fn fill(&mut self, c: u32) {
        let mut i = 0usize;
        while i < FRAME_PIXELS {
            self.px[i] = c;
            i += 1;
        }
    }

    pub fn get(&self, x: usize, y: usize) -> u32 {
        if x < FRAME_W && y < FRAME_H {
            self.px[y * FRAME_W + x]
        } else {
            0
        }
    }

    pub fn put(&mut self, x: usize, y: usize, c: u32) -> bool {
        if x < FRAME_W && y < FRAME_H {
            self.px[y * FRAME_W + x] = c;
            true
        } else {
            false
        }
    }

    pub fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, c: u32) -> usize {
        let mut n = 0usize;
        let mut j = 0usize;
        while j < h {
            let mut i = 0usize;
            while i < w {
                if self.put(x + i, y + j, c) {
                    n += 1;
                }
                i += 1;
            }
            j += 1;
        }
        n
    }

    pub fn as_slice(&self) -> &[u32] {
        &self.px
    }
}

// ---------------------------------------------------------------------------
// F101 帧缓冲服务 — 独占管理与画布租借
// ---------------------------------------------------------------------------

pub const CANVAS_MAX: usize = 4;

#[derive(Clone, Copy)]
pub struct FbService {
    /// 物理帧缓冲（前缓冲）。
    pub front: Canvas,
    /// 已租出的画布数。
    pub rented: usize,
    /// 帧序号。
    pub frame: u64,
    /// 帧缓冲是否被独占（禁止他人直写）。
    pub exclusive: bool,
}

impl FbService {
    pub const fn new() -> FbService {
        FbService { front: Canvas::new(), rented: 0, frame: 0, exclusive: false }
    }

    /// 独占帧缓冲：此后只接受经合成器的提交。
    pub fn claim(&mut self, presenter_pid: u32) -> bool {
        if presenter_pid == 0 {
            return false;
        }
        self.exclusive = true;
        true
    }

    pub fn release(&mut self) {
        self.exclusive = false;
    }

    pub fn rent_canvas(&mut self) -> Option<usize> {
        if self.rented >= CANVAS_MAX {
            return None;
        }
        self.rented += 1;
        Some(self.rented - 1)
    }

    pub fn return_canvas(&mut self) -> bool {
        if self.rented == 0 {
            return false;
        }
        self.rented -= 1;
        true
    }

    /// 直接写入前缓冲必须已独占，否则拒绝（防撕裂/防越权）。
    pub fn present_direct(&mut self, x: usize, y: usize, c: u32) -> bool {
        if !self.exclusive {
            return false;
        }
        self.front.put(x, y, c)
    }
}

// ---------------------------------------------------------------------------
// F102 双缓冲管线 — 后备缓冲 + 页翻转
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlipResult {
    /// 成功翻转。
    Flipped,
    /// 未在 VSync 窗口内（可选等待）。
    OutsideVsync,
    /// 后备缓冲未就绪（未提交任何内容）。
    NotReady,
}

#[derive(Clone, Copy)]
pub struct DoubleBuffer {
    pub back: Canvas,
    pub front: Canvas,
    /// 后备缓冲已写入（可翻转）。
    pub dirty_submitted: bool,
    pub flips: u64,
    /// 当前是否处于 VSync 窗口。
    pub in_vsync: bool,
}

impl DoubleBuffer {
    pub const fn new() -> DoubleBuffer {
        DoubleBuffer {
            back: Canvas::new(),
            front: Canvas::new(),
            dirty_submitted: false,
            flips: 0,
            in_vsync: true,
        }
    }

    pub fn paint_back(&mut self, x: usize, y: usize, c: u32) -> bool {
        let ok = self.back.put(x, y, c);
        if ok {
            self.dirty_submitted = true;
        }
        ok
    }

    /// 翻转：后备 → 前缓冲。
    pub fn flip(&mut self) -> FlipResult {
        if !self.dirty_submitted {
            return FlipResult::NotReady;
        }
        if !self.in_vsync {
            return FlipResult::OutsideVsync;
        }
        self.front = self.back;
        self.flips += 1;
        self.dirty_submitted = false;
        FlipResult::Flipped
    }

    /// 撕裂检测：翻转后前缓冲内容必须与后备逐像素一致。
    pub fn tear_free(&self) -> bool {
        let mut i = 0usize;
        while i < FRAME_PIXELS {
            if self.front.as_slice()[i] != self.back.as_slice()[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// F103 脏区跟踪 — 只重绘变化区域，静止画面零拷贝
// ---------------------------------------------------------------------------

pub const DIRTY_MAX: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    pub const fn new(x: usize, y: usize, w: usize, h: usize) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x
            && o.y >= self.y
            && o.x + o.w <= self.x + self.w
            && o.y + o.h <= self.y + self.h
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let x = core::cmp::min(self.x, o.x);
        let y = core::cmp::min(self.y, o.y);
        let x2 = core::cmp::max(self.x + self.w, o.x + o.w);
        let y2 = core::cmp::max(self.y + self.h, o.y + o.h);
        Rect::new(x, y, x2 - x, y2 - y)
    }

    pub fn area(&self) -> usize {
        self.w * self.h
    }
}

#[derive(Clone, Copy)]
pub struct DirtyTracker {
    rects: [Rect; DIRTY_MAX],
    count: usize,
    /// 因溢出而合并的次数。
    pub merges: u64,
    /// 提交的脏区总数。
    pub submitted: u64,
    /// 被跳过的重绘面积（静止画面零拷贝的度量）。
    pub skipped_pixels: u64,
}

impl DirtyTracker {
    pub const fn new() -> DirtyTracker {
        DirtyTracker {
            rects: [Rect::new(0, 0, 0, 0); DIRTY_MAX],
            count: 0,
            merges: 0,
            submitted: 0,
            skipped_pixels: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, i: usize) -> Option<Rect> {
        if i < self.count {
            Some(self.rects[i])
        } else {
            None
        }
    }

    /// 标记脏区：已包含则忽略；溢满则与最后一项合并。
    pub fn mark(&mut self, r: Rect) -> bool {
        self.submitted += 1;
        if r.area() == 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.rects[i].contains(&r) {
                self.skipped_pixels += r.area() as u64;
                return false;
            }
            i += 1;
        }
        if self.count >= DIRTY_MAX {
            let last = self.count - 1;
            self.rects[last] = self.rects[last].union(&r);
            self.merges += 1;
            return false;
        }
        self.rects[self.count] = r;
        self.count += 1;
        true
    }

    /// 本帧需要重绘的总像素。
    pub fn dirty_area(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            n += self.rects[i].area();
            i += 1;
        }
        n
    }

    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// 静止帧判定：零脏区即零拷贝。
    pub fn idle(&self) -> bool {
        self.count == 0
    }
}

// ---------------------------------------------------------------------------
// F104 VSync 同步 — 垂直同步等待与翻转时机
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Vsync {
    /// 扫描线周期（tick）。
    pub period: u64,
    /// 上次 VSync 的 tick。
    pub last_tick: u64,
    /// 累计等待次数。
    pub waits: u64,
    /// 累计错过（超时未等到）。
    pub misses: u64,
    /// 允许的最长等待。
    pub max_wait: u64,
}

impl Vsync {
    pub const fn new(period: u64, max_wait: u64) -> Vsync {
        Vsync { period, last_tick: 0, waits: 0, misses: 0, max_wait }
    }

    /// 等到下一个 VSync：返回 (是否等到, 实际 tick)。
    pub fn wait(&mut self, now: u64) -> (bool, u64) {
        self.waits += 1;
        let next = self.last_tick + self.period;
        if next <= now {
            self.misses += 1;
            self.last_tick = now;
            return (false, now);
        }
        if next - now > self.max_wait {
            self.misses += 1;
            self.last_tick = now;
            return (false, now);
        }
        self.last_tick = next;
        (true, next)
    }
}

// ---------------------------------------------------------------------------
// F105 合成器内核服务 — 多画布分层合成 + Z 序
// ---------------------------------------------------------------------------

pub const LAYER_MAX: usize = 4;
pub const TILE_W: usize = 8;
pub const TILE_H: usize = 8;
pub const TILE_PIXELS: usize = TILE_W * TILE_H;

#[derive(Clone, Copy)]
pub struct Layer {
    /// Z 序：数值大者在上。
    pub z: i32,
    pub x: usize,
    pub y: usize,
    /// 8×8 贴片。
    pub tile: [u32; TILE_PIXELS],
    pub visible: bool,
    /// 不透明层可走直通快路。
    pub opaque: bool,
    pub alpha: u8,
    /// 组件 id（AURORA 组件树对应）。
    pub comp_id: u32,
}

impl Layer {
    pub const fn new() -> Layer {
        Layer {
            z: 0,
            x: 0,
            y: 0,
            tile: [0u32; TILE_PIXELS],
            visible: false,
            opaque: false,
            alpha: 255,
            comp_id: 0,
        }
    }

    pub fn fill_tile(&mut self, c: u32) {
        let mut i = 0usize;
        while i < TILE_PIXELS {
            self.tile[i] = c;
            i += 1;
        }
    }

    pub fn put_tile(&mut self, tx: usize, ty: usize, c: u32) -> bool {
        if tx < TILE_W && ty < TILE_H {
            self.tile[ty * TILE_W + tx] = c;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy)]
pub struct Compositor {
    layers: [Layer; LAYER_MAX],
    count: usize,
    /// 合成输出。
    pub out: Canvas,
    /// 直通快路命中的层数。
    pub fast_path_hits: u64,
    /// 逐像素混合的层数。
    pub blend_hits: u64,
    pub composites: u64,
}

impl Compositor {
    pub const fn new() -> Compositor {
        Compositor {
            layers: [Layer::new(); LAYER_MAX],
            count: 0,
            out: Canvas::new(),
            fast_path_hits: 0,
            blend_hits: 0,
            composites: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn layer_mut(&mut self, i: usize) -> Option<&mut Layer> {
        if i < self.count {
            Some(&mut self.layers[i])
        } else {
            None
        }
    }

    pub fn layer(&self, i: usize) -> Option<Layer> {
        if i < self.count {
            Some(self.layers[i])
        } else {
            None
        }
    }

    pub fn add_layer(&mut self, z: i32, comp_id: u32) -> Option<usize> {
        if self.count >= LAYER_MAX {
            return None;
        }
        let mut l = Layer::new();
        l.z = z;
        l.comp_id = comp_id;
        l.visible = true;
        self.layers[self.count] = l;
        self.count += 1;
        Some(self.count - 1)
    }

    /// Z 序索引：返回按 Z 升序（底 → 顶）的层下标。
    pub fn z_order(&self) -> [usize; LAYER_MAX] {
        let mut idx = [0usize; LAYER_MAX];
        let mut i = 0usize;
        while i < LAYER_MAX {
            idx[i] = i;
            i += 1;
        }
        // 简单插入排序（层数 ≤ 4）。
        let mut a = 1usize;
        while a < self.count {
            let key = idx[a];
            let mut b = a;
            while b > 0 && self.layers[idx[b - 1]].z > self.layers[key].z {
                idx[b] = idx[b - 1];
                b -= 1;
            }
            idx[b] = key;
            a += 1;
        }
        idx
    }

    /// F106 分层合成优化：不透明层直通（整片覆盖），透明层逐像素混合。
    pub fn composite(&mut self) {
        self.composites += 1;
        self.out.fill(0);
        let order = self.z_order();
        let mut i = 0usize;
        while i < self.count {
            let li = order[i];
            let l = self.layers[li];
            if l.visible {
                if l.opaque && l.alpha == 255 {
                    self.fast_path_hits += 1;
                    let mut ty = 0usize;
                    while ty < TILE_H {
                        let mut tx = 0usize;
                        while tx < TILE_W {
                            let _ = self.out.put(l.x + tx, l.y + ty, l.tile[ty * TILE_W + tx]);
                            tx += 1;
                        }
                        ty += 1;
                    }
                } else {
                    self.blend_hits += 1;
                    let mut ty = 0usize;
                    while ty < TILE_H {
                        let mut tx = 0usize;
                        while tx < TILE_W {
                            let px = l.x + tx;
                            let py = l.y + ty;
                            let dst = self.out.get(px, py);
                            let src = l.tile[ty * TILE_W + tx];
                            let _ = self.out.put(px, py, blend_over(dst, src, l.alpha));
                            tx += 1;
                        }
                        ty += 1;
                    }
                }
            }
            i += 1;
        }
    }

    /// 顶部像素（命中测试用）。
    pub fn hit_test(&self, x: usize, y: usize) -> Option<u32> {
        let order = self.z_order();
        let mut i = self.count;
        while i > 0 {
            i -= 1;
            let l = self.layers[order[i]];
            if !l.visible || l.alpha == 0 {
                continue;
            }
            if x >= l.x && x < l.x + TILE_W && y >= l.y && y < l.y + TILE_H {
                return Some(l.comp_id);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// F107 软合成保底 — 无 GPU 时纯 CPU 全路径可用
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderPath {
    /// 硬件加速。
    Hardware,
    /// 纯 CPU 软合成（降级纪律保底）。
    Soft,
}

#[derive(Clone, Copy)]
pub struct RenderPathState {
    pub path: RenderPath,
    /// 降级原因（0 = 无）。
    pub degraded_reason: u8,
    /// 降级/恢复次数。
    pub switches: u32,
}

impl RenderPathState {
    pub const fn new(hw_available: bool) -> RenderPathState {
        RenderPathState {
            path: if hw_available { RenderPath::Hardware } else { RenderPath::Soft },
            degraded_reason: if hw_available { 0 } else { 1 },
            switches: 0,
        }
    }

    pub fn degrade(&mut self, reason: u8) -> bool {
        if self.path == RenderPath::Soft {
            return false;
        }
        self.path = RenderPath::Soft;
        self.degraded_reason = reason;
        self.switches += 1;
        true
    }

    pub fn restore(&mut self, hw_available: bool) -> bool {
        if self.path == RenderPath::Hardware || !hw_available {
            return false;
        }
        self.path = RenderPath::Hardware;
        self.degraded_reason = 0;
        self.switches += 1;
        true
    }

    /// 软路径必须可用（保底纪律）。
    pub const fn soft_available(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// F108 GPU 探测 — PCI 扫描显示适配器
// ---------------------------------------------------------------------------

pub const GPU_MAX: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuVendor {
    QemuStdVga,
    BochsVbe,
    VirtioGpu,
    Unknown,
}

#[derive(Clone, Copy)]
pub struct GpuDevice {
    pub vendor: u16,
    pub device: u16,
    pub vendor_kind: GpuVendor,
    pub bar0: u64,
    pub mmio_len: usize,
    /// 2D 加速能力位。
    pub accel_2d: bool,
    pub preferred: bool,
}

impl GpuDevice {
    pub const fn empty() -> GpuDevice {
        GpuDevice {
            vendor: 0,
            device: 0,
            vendor_kind: GpuVendor::Unknown,
            bar0: 0,
            mmio_len: 0,
            accel_2d: false,
            preferred: false,
        }
    }
}

#[derive(Clone, Copy)]
pub struct GpuProbe {
    devices: [GpuDevice; GPU_MAX],
    count: usize,
}

impl GpuProbe {
    pub const fn new() -> GpuProbe {
        GpuProbe { devices: [GpuDevice::empty(); GPU_MAX], count: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn device(&self, i: usize) -> Option<GpuDevice> {
        if i < self.count {
            Some(self.devices[i])
        } else {
            None
        }
    }

    /// 识别已知显示适配器并登记。
    pub fn scan(&mut self, vendor: u16, device: u16, bar0: u64, mmio_len: usize) -> Option<usize> {
        if self.count >= GPU_MAX {
            return None;
        }
        let kind = match (vendor, device) {
            (0x1234, 0x1111) => GpuVendor::BochsVbe,
            (0x1AF4, 0x1050) => GpuVendor::VirtioGpu,
            (0x8086, _) => GpuVendor::Unknown,
            _ => GpuVendor::Unknown,
        };
        let mut d = GpuDevice::empty();
        d.vendor = vendor;
        d.device = device;
        d.vendor_kind = kind;
        d.bar0 = bar0;
        d.mmio_len = mmio_len;
        d.accel_2d = matches!(kind, GpuVendor::BochsVbe | GpuVendor::VirtioGpu);
        self.devices[self.count] = d;
        self.count += 1;
        Some(self.count - 1)
    }

    /// QEMU 标准 VGA 也是 1234:1111，优先 BOCHS/QEMU 显卡驱动（F108 纪律）。
    pub fn pick_preferred(&mut self) -> Option<usize> {
        let mut best: Option<usize> = None;
        let mut i = 0usize;
        while i < self.count {
            let rank = match self.devices[i].vendor_kind {
                GpuVendor::VirtioGpu => 3,
                GpuVendor::BochsVbe => 2,
                GpuVendor::Unknown => 1,
                GpuVendor::QemuStdVga => 0,
            };
            match best {
                None => best = Some(i),
                Some(b) => {
                    let cur = match self.devices[b].vendor_kind {
                        GpuVendor::VirtioGpu => 3,
                        GpuVendor::BochsVbe => 2,
                        GpuVendor::Unknown => 1,
                        GpuVendor::QemuStdVga => 0,
                    };
                    if rank > cur {
                        best = Some(i);
                    }
                }
            }
            i += 1;
        }
        if let Some(b) = best {
            self.devices[b].preferred = true;
        }
        best
    }
}

// ---------------------------------------------------------------------------
// F109 硬件加速接口预留 — 接口占位，不阻塞主线
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccelOp {
    Blit,
    FillRect,
    CompositeTile,
    ReadBack,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccelResult {
    /// 已执行（硬件路径可用）。
    Done,
    /// 不支持 → 调用方回退软合成。
    Unsupported,
    /// 提交失败（队列满/设备丢失）→ 回退。
    Failed,
}

/// 加速后端占位：默认全部 `Unsupported`，软合成接管，绝不阻塞主线。
#[derive(Clone, Copy)]
pub struct AccelStub {
    pub backend: RenderPath,
    pub submitted: u64,
    pub fallbacks: u64,
}

impl AccelStub {
    pub const fn new(backend: RenderPath) -> AccelStub {
        AccelStub { backend, submitted: 0, fallbacks: 0 }
    }

    pub fn submit(&mut self, op: AccelOp) -> AccelResult {
        self.submitted += 1;
        if self.backend != RenderPath::Hardware {
            self.fallbacks += 1;
            return AccelResult::Unsupported;
        }
        // 硬件后端尚未接入（预留）：显式回退，禁止静默丢失。
        let _ = op;
        self.fallbacks += 1;
        AccelResult::Unsupported
    }
}

// ---------------------------------------------------------------------------
// 域自检：F101~F125 共 25 项（F101~F109 在本文件，F110~F115/F116~F125 见子模块）
// ---------------------------------------------------------------------------

/// 自检里「取不到下标」的哨兵：下游访问器都做 `i < count` 边界检查，
/// 用它替代 `.unwrap()` 只会让该项自检判定失败，不会 panic 掉整个 checkup。
const IDX_NONE: usize = usize::MAX;

pub fn run_gfxsrv_checks() -> CheckSet {
    let mut set = CheckSet::new("gfxsrv");

    // F101 帧缓冲服务
    let mut fb = FbService::new();
    let claim = fb.claim(2);
    let direct = fb.present_direct(1, 1, rgb(1, 2, 3));
    let c0 = fb.rent_canvas();
    let _ = fb.rent_canvas();
    let _ = fb.rent_canvas();
    let _ = fb.rent_canvas();
    let over = fb.rent_canvas().is_none();
    let back = fb.return_canvas();
    fb.release();
    let after_release = !fb.present_direct(1, 1, rgb(9, 9, 9));
    set.add(
        "F101 framebuffer svc",
        claim
            && direct
            && fb.front.get(1, 1) == rgb(1, 2, 3)
            && c0 == Some(0)
            && over
            && back
            && fb.rented == 3
            && after_release
            && !fb.claim(0),
        "独占/租借上限/释放后拒写",
    );

    // F102 双缓冲管线
    let mut db = DoubleBuffer::new();
    let not_ready = db.flip() == FlipResult::NotReady;
    let _ = db.paint_back(0, 0, rgb(10, 20, 30));
    db.in_vsync = false;
    let outside = db.flip() == FlipResult::OutsideVsync;
    db.in_vsync = true;
    let flipped = db.flip() == FlipResult::Flipped;
    set.add(
        "F102 double buffer",
        not_ready
            && outside
            && flipped
            && db.flips == 1
            && db.front.get(0, 0) == rgb(10, 20, 30)
            && db.tear_free(),
        "后备缓冲/VSync 门控/页翻转",
    );

    // F103 脏区跟踪
    let mut dt = DirtyTracker::new();
    let a = dt.mark(Rect::new(0, 0, 4, 4));
    let contained = !dt.mark(Rect::new(1, 1, 2, 2));
    let b = dt.mark(Rect::new(10, 10, 4, 4));
    let area = dt.dirty_area();
    let idle_before = dt.idle();
    dt.clear();
    set.add(
        "F103 dirty tracking",
        a
            && contained
            && b
            && dt.submitted == 3
            && dt.skipped_pixels == 4
            && area == 32
            && !idle_before
            && dt.idle()
            && dt.len() == 0,
        "包含即忽略/面积统计/静止零拷贝",
    );

    // F103 溢出合并
    let mut dto = DirtyTracker::new();
    let mut i = 0usize;
    while i < DIRTY_MAX {
        let _ = dto.mark(Rect::new(i, 0, 1, 1));
        i += 1;
    }
    let merged = dto.mark(Rect::new(20, 20, 2, 2));
    set.add(
        "F103 dirty overflow",
        !merged && dto.merges == 1 && dto.len() == DIRTY_MAX,
        "溢满与末项合并",
    );

    // F104 VSync
    let mut vs = Vsync::new(100, 30);
    let (ok1, t1) = vs.wait(80);
    let (ok2, _t2) = vs.wait(105);
    let (ok3, _t3) = vs.wait(106);
    set.add(
        "F104 vsync",
        ok1 && t1 == 100 && !ok2 && !ok3 && vs.waits == 3 && vs.misses == 2,
        "等待窗口/超时计缺",
    );

    // F105 合成器 + Z 序
    let mut comp = Compositor::new();
    let bottom = comp.add_layer(0, 100).unwrap_or(IDX_NONE);
    let top = comp.add_layer(10, 200).unwrap_or(IDX_NONE);
    if let Some(l) = comp.layer_mut(bottom) {
        l.fill_tile(rgb(0, 0, 255));
        l.opaque = true;
    }
    if let Some(l) = comp.layer_mut(top) {
        l.fill_tile(argb(128, 255, 0, 0));
        l.x = 0;
        l.y = 0;
    }
    comp.composite();
    let px = comp.out.get(0, 0);
    let zok = comp.z_order()[0] == bottom && comp.z_order()[1] == top;
    set.add(
        "F105 compositor",
        comp.len() == 2
            && zok
            && color_a(px) == 255
            && color_r(px) > 0
            && color_b(px) > 0
            && color_g(px) == 0
            && comp.hit_test(0, 0) == Some(200),
        "分层合成/Z 序/命中测试",
    );

    // F106 分层合成优化
    let mut comp2 = Compositor::new();
    let op = comp2.add_layer(0, 1).unwrap_or(IDX_NONE);
    let tr = comp2.add_layer(1, 2).unwrap_or(IDX_NONE);
    if let Some(l) = comp2.layer_mut(op) {
        l.fill_tile(rgb(255, 255, 255));
        l.opaque = true;
    }
    if let Some(l) = comp2.layer_mut(tr) {
        l.fill_tile(argb(255, 0, 0, 0));
    }
    comp2.composite();
    set.add(
        "F106 layer optimisation",
        comp2.fast_path_hits == 1
            && comp2.blend_hits == 1
            && comp2.out.get(0, 0) == argb(255, 0, 0, 0)
            && comp2.composites == 1,
        "不透明直通/透明逐像素",
    );

    // F107 软合成保底
    let mut rps = RenderPathState::new(false);
    let soft_ok = rps.path == RenderPath::Soft && rps.soft_available();
    let restore_denied = !rps.restore(false);
    let mut rps2 = RenderPathState::new(true);
    let deg = rps2.degrade(7);
    let restore = rps2.restore(true);
    set.add(
        "F107 soft fallback",
        soft_ok
            && restore_denied
            && deg
            && restore
            && rps2.path == RenderPath::Hardware
            && rps2.switches == 2
            && rps2.degraded_reason == 0,
        "无 GPU 全路径可用/降级恢复",
    );

    // F108 GPU 探测
    let mut probe = GpuProbe::new();
    let _ = probe.scan(0x1234, 0x1111, 0xFD00_0000, 1 << 20);
    let _ = probe.scan(0x1AF4, 0x1050, 0xFE00_0000, 1 << 22);
    let _ = probe.scan(0x8086, 0x9999, 0, 0);
    let pref = probe.pick_preferred();
    set.add(
        "F108 gpu probe",
        probe.len() == 3
            && pref == Some(1)
            && probe.device(1).map(|d| d.vendor_kind == GpuVendor::VirtioGpu).unwrap_or(false)
            && probe.device(1).map(|d| d.preferred).unwrap_or(false)
            && probe.device(1).map(|d| d.accel_2d).unwrap_or(false),
        "适配器识别/优选 virtio-gpu",
    );

    // F109 硬件加速接口预留
    let mut accel = AccelStub::new(RenderPath::Hardware);
    let r1 = accel.submit(AccelOp::Blit);
    let _ = accel.submit(AccelOp::FillRect);
    let mut accel_soft = AccelStub::new(RenderPath::Soft);
    let r2 = accel_soft.submit(AccelOp::CompositeTile);
    set.add(
        "F109 accel placeholder",
        r1 == AccelResult::Unsupported
            && r2 == AccelResult::Unsupported
            && accel.fallbacks == 2
            && accel_soft.fallbacks == 1,
        "预留接口显式回退，不阻塞主线",
    );

    text::extend_checks(&mut set);
    perf::extend_checks(&mut set);

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f102_flip_requires_vsync() {
        let mut db = DoubleBuffer::new();
        assert_eq!(db.flip(), FlipResult::NotReady);
        let _ = db.paint_back(3, 3, rgb(1, 1, 1));
        db.in_vsync = false;
        assert_eq!(db.flip(), FlipResult::OutsideVsync);
        db.in_vsync = true;
        assert_eq!(db.flip(), FlipResult::Flipped);
        assert!(db.tear_free());
    }

    #[test]
    fn f103_rect_union_and_contains() {
        let a = Rect::new(0, 0, 4, 4);
        let b = Rect::new(2, 2, 4, 4);
        assert!(a.contains(&Rect::new(1, 1, 2, 2)));
        assert!(!a.contains(&b));
        let u = a.union(&b);
        assert_eq!(u, Rect::new(0, 0, 6, 6));
        assert_eq!(u.area(), 36);
    }

    #[test]
    fn f105_z_order_sorts_ascending() {
        let mut c = Compositor::new();
        let _ = c.add_layer(30, 1);
        let _ = c.add_layer(10, 2);
        let _ = c.add_layer(20, 3);
        let o = c.z_order();
        assert_eq!(o[0], 1);
        assert_eq!(o[1], 2);
        assert_eq!(o[2], 0);
    }

    #[test]
    fn f_blend_is_integer_exact() {
        assert_eq!(
            blend_over(rgb(0, 0, 0), argb(255, 255, 255, 255), 255),
            argb(255, 255, 255, 255)
        );
        assert_eq!(blend_over(rgb(10, 20, 30), argb(255, 0, 0, 0), 0), rgb(10, 20, 30));
    }

    #[test]
    fn f_run_gfxsrv_checks_pass() {
        let set = run_gfxsrv_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("gfxsrv self-test failed:\n{}", core::str::from_utf8(&buf[..n]).unwrap_or("<x>"));
        }
        assert!(set.len() >= 25, "gfxsrv domain must expose >= 25 checks");
    }
}
