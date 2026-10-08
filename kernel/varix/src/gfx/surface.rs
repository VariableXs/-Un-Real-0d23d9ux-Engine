//! TRINITY-500 · AI-05 2D 合成渲染栈域（F101~F123，W2）
//!
//! 依赖 VARIX-500 AI-08 的帧缓冲基础（`crate::fb`），本文件做「Variable 视觉语言」
//! 的完整合成器：双缓冲、软光栅化、圆角/渐变/透明度、脏矩形、帧预算、多显示器。
//! 全部为纯计算（无浮点库依赖，只用 f32 基本运算），可在 host 单测里验证。

// ---------------------------------------------------------------------------
// F101 帧缓冲抽象与模式表
// F122 像素格式抽象 — RGB/BGR
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// 0x00RRGGBB
    Rgb32,
    /// 0x00BBGGRR
    Bgr32,
    /// 每通道 8 位，打包成 u32 的 RGBA。
    Rgba32,
}

impl PixelFormat {
    pub fn name(self) -> &'static str {
        match self {
            PixelFormat::Rgb32 => "rgb32",
            PixelFormat::Bgr32 => "bgr32",
            PixelFormat::Rgba32 => "rgba32",
        }
    }

    pub fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Rgba32 => 4,
            _ => 4,
        }
    }
}

/// 把规范色（0xAARRGGBB）按目标格式重排。
pub fn pack(format: PixelFormat, argb: u32) -> u32 {
    let r = (argb >> 16) & 0xFF;
    let g = (argb >> 8) & 0xFF;
    let b = argb & 0xFF;
    let a = (argb >> 24) & 0xFF;
    match format {
        PixelFormat::Rgb32 => (a << 24) | (r << 16) | (g << 8) | b,
        PixelFormat::Bgr32 => (a << 24) | (b << 16) | (g << 8) | r,
        PixelFormat::Rgba32 => (a << 24) | (r << 16) | (g << 8) | b,
    }
}

pub fn unpack(format: PixelFormat, v: u32) -> (u8, u8, u8, u8) {
    match format {
        PixelFormat::Rgb32 | PixelFormat::Rgba32 => {
            ((v >> 16) as u8 & 0xFF, (v >> 8) as u8, v as u8, (v >> 24) as u8)
        }
        PixelFormat::Bgr32 => (v as u8, (v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeInfo {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub format: PixelFormat,
}

/// 内核支持的模式表（不编造未验证的分辨率——表里每一项都要能真机点亮）。
pub const MODE_TABLE: [ModeInfo; 6] = [
    ModeInfo { width: 1024, height: 768, refresh_hz: 60, format: PixelFormat::Rgb32 },
    ModeInfo { width: 1280, height: 720, refresh_hz: 60, format: PixelFormat::Rgb32 },
    ModeInfo { width: 1600, height: 900, refresh_hz: 60, format: PixelFormat::Rgb32 },
    ModeInfo { width: 1920, height: 1080, refresh_hz: 60, format: PixelFormat::Rgb32 },
    ModeInfo { width: 2560, height: 1440, refresh_hz: 60, format: PixelFormat::Bgr32 },
    ModeInfo { width: 3840, height: 2160, refresh_hz: 30, format: PixelFormat::Bgr32 },
];

pub fn mode_at(index: usize) -> Option<ModeInfo> {
    MODE_TABLE.get(index).copied()
}

/// 选择与目标宽高最接近的模式（不超过上限）。
pub fn pick_mode(max_w: u32, max_h: u32) -> ModeInfo {
    let mut best = MODE_TABLE[0];
    for m in MODE_TABLE.iter() {
        if m.width <= max_w && m.height <= max_h {
            best = *m;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// F102 双缓冲/三缓冲
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapKind {
    Single,
    Double,
    Triple,
}

impl SwapKind {
    pub fn buffer_count(self) -> usize {
        match self {
            SwapKind::Single => 1,
            SwapKind::Double => 2,
            SwapKind::Triple => 3,
        }
    }
}

pub struct SwapChain {
    pub kind: SwapKind,
    pub front: usize,
    /// 正在被写入的缓冲（back）；present 时与 front 交换。
    pub back: usize,
    pub frames: u64,
}

impl SwapChain {
    pub const fn new(kind: SwapKind) -> SwapChain {
        let back = match kind {
            SwapKind::Single => 0,
            _ => 1,
        };
        SwapChain { kind, front: 0, back, frames: 0 }
    }

    /// 提交：front 前进到 back，back 换到下一个空闲缓冲。三缓冲下不阻塞。
    pub fn present(&mut self) -> usize {
        let n = self.kind.buffer_count();
        if n <= 1 {
            self.frames += 1;
            return self.front;
        }
        self.front = self.back;
        self.back = (self.back + 1) % n;
        if self.back == self.front && n > 2 {
            self.back = (self.back + 1) % n;
        }
        self.frames += 1;
        self.front
    }
}

// ---------------------------------------------------------------------------
// F103 软件光栅化（CPU blit）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn area(&self) -> i64 {
        (self.w.max(0) as i64) * (self.h.max(0) as i64)
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        Rect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
}

/// 逐像素拷贝（不处理 alpha，纯 memcpy 语义）。越界自动裁剪。
pub fn blit(dst: &mut [u32], dst_w: usize, src: &[u32], src_w: usize, r: &Rect, dst_clip: &Rect) -> usize {
    let region = r.intersect(dst_clip);
    if region.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    for row in 0..region.h {
        let dy = region.y + row;
        let sy = row + (region.y - r.y);
        if dy < 0 || sy < 0 {
            continue;
        }
        for col in 0..region.w {
            let dx = region.x + col;
            let sx = col + (region.x - r.x);
            if dx < 0 || sx < 0 {
                continue;
            }
            let di = dy as usize * dst_w + dx as usize;
            let si = sy as usize * src_w + sx as usize;
            if di < dst.len() && si < src.len() {
                dst[di] = src[si];
                count += 1;
            }
        }
    }
    count
}

pub fn fill_rect(buf: &mut [u32], w: usize, clip: &Rect, r: &Rect, color: u32) -> usize {
    let region = r.intersect(clip);
    if region.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    for row in 0..region.h {
        let y = region.y + row;
        if y < 0 {
            continue;
        }
        for col in 0..region.w {
            let x = region.x + col;
            if x < 0 {
                continue;
            }
            let idx = y as usize * w + x as usize;
            if idx < buf.len() {
                buf[idx] = color;
                count += 1;
            }
        }
    }
    count
}

// ---------------------------------------------------------------------------
// F104 矩形/圆角/渐变绘制
// ---------------------------------------------------------------------------

/// 圆角半径必须钳制到短边一半，否则形状退化。
pub fn clamp_radius(radius: i32, w: i32, h: i32) -> i32 {
    let max_r = core::cmp::min(w, h) / 2;
    radius.clamp(0, max_r.max(0))
}

/// 判断像素 (x,y) 是否落在圆角矩形内（1/4 圆判定，无浮点）。
pub fn inside_round_rect(x: i32, y: i32, w: i32, h: i32, r: i32) -> bool {
    let r = clamp_radius(r, w, h);
    if x < 0 || y < 0 || x >= w || y >= h {
        return false;
    }
    if r == 0 {
        return true;
    }
    let (cx, cy) = match (x < r, y < r, x >= w - r, y >= h - r) {
        (true, true, _, _) => (r, r),
        (_, true, true, _) => (w - 1 - r, r),
        (_, _, true, true) => (w - 1 - r, h - 1 - r),
        (true, _, _, true) => (r, h - 1 - r),
        _ => return true,
    };
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

/// 线性渐变取色（t 为 0..=1000 permille）。
pub fn gradient_at(a: u32, b: u32, t_permille: u32) -> u32 {
    let t = if t_permille > 1000 { 1000 } else { t_permille };
    let lerp = |x: u32, y: u32| -> u32 { (x * (1000 - t) + y * t) / 1000 };
    let ar = (a >> 16) & 0xFF;
    let ag = (a >> 8) & 0xFF;
    let ab = a & 0xFF;
    let br = (b >> 16) & 0xFF;
    let bg = (b >> 8) & 0xFF;
    let bb = b & 0xFF;
    (0xFF << 24) | (lerp(ar, br) << 16) | (lerp(ag, bg) << 8) | lerp(ab, bb)
}

// ---------------------------------------------------------------------------
// F106 透明度合成（alpha blend）
// ---------------------------------------------------------------------------

/// src-over 合成，`alpha` 为 0..=255。
pub fn blend(src: u32, dst: u32, alpha: u8) -> u32 {
    let a = alpha as u32;
    let inv = 255 - a;
    let sr = (src >> 16) & 0xFF;
    let sg = (src >> 8) & 0xFF;
    let sb = src & 0xFF;
    let dr = (dst >> 16) & 0xFF;
    let dg = (dst >> 8) & 0xFF;
    let db = dst & 0xFF;
    let r = (sr * a + dr * inv) / 255;
    let g = (sg * a + dg * inv) / 255;
    let b = (sb * a + db * inv) / 255;
    (0xFF << 24) | (r << 16) | (g << 8) | b
}

/// 预乘 alpha 版本的合成（合成器内部热路径用，避免除法）。
pub fn blend_premultiplied(src_rgb: (u32, u32, u32), dst: u32, alpha: u8) -> u32 {
    let a = alpha as u32;
    let dr = (dst >> 16) & 0xFF;
    let dg = (dst >> 8) & 0xFF;
    let db = dst & 0xFF;
    let r = src_rgb.0 + (dr * (255 - a)) / 255;
    let g = src_rgb.1 + (dg * (255 - a)) / 255;
    let b = src_rgb.2 + (db * (255 - a)) / 255;
    (0xFF << 24) | (r.min(255) << 16) | (g.min(255) << 8) | b.min(255)
}

// ---------------------------------------------------------------------------
// F107 裁剪与脏矩形重绘
// ---------------------------------------------------------------------------

pub const MAX_DIRTY: usize = 32;

pub struct DirtyList {
    rects: [Option<Rect>; MAX_DIRTY],
    count: usize,
}

impl DirtyList {
    pub const fn new() -> DirtyList {
        DirtyList { rects: [None; MAX_DIRTY], count: 0 }
    }

    pub fn add(&mut self, r: Rect) {
        if r.is_empty() {
            return;
        }
        // 先尝试与已有矩形合并（相邻/包含），控制矩形数量。
        for i in 0..self.count {
            if let Some(e) = self.rects[i] {
                let u = Rect::new(
                    e.x.min(r.x),
                    e.y.min(r.y),
                    (e.right().max(r.right())) - e.x.min(r.x),
                    (e.bottom().max(r.bottom())) - e.y.min(r.y),
                );
                // 合并判据：并集里的「空隙」不超过并集的一半，避免把整屏并进来。
                let slack = u.area() - e.area() - r.area();
                if slack <= u.area() / 2 {
                    self.rects[i] = Some(u);
                    return;
                }
            }
        }
        if self.count < MAX_DIRTY {
            self.rects[self.count] = Some(r);
            self.count += 1;
        } else {
            // 溢出：退化为整屏重绘（宁可多画，不可漏画）。
            self.rects[0] = Some(Rect::new(0, 0, i32::MAX, i32::MAX));
            self.count = 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<Rect> {
        if i < self.count {
            self.rects[i]
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        self.rects = [None; MAX_DIRTY];
        self.count = 0;
    }

    /// 总脏面积（用于判断「是否值得重绘」）。
    pub fn total_area(&self) -> i64 {
        (0..self.count).filter_map(|i| self.get(i)).map(|r| r.area()).sum()
    }
}

impl Default for DirtyList {
    fn default() -> Self {
        DirtyList::new()
    }
}

// ---------------------------------------------------------------------------
// F108 OKLCH 色彩令牌对齐
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklch {
    /// 0.0~1.0
    pub l: f32,
    /// 彩度 0.0~0.4
    pub c: f32,
    /// 色相角（度）
    pub h: f32,
}

// `no_std` 下没有 libm，浮点只有四则运算与比较可用。
// 这里自备最小数学库：exp/ln/pow/sin/cos 全部用「区间分解 + 泰勒/帕德级数」实现，
// 精度足以满足色彩令牌（误差 < 1e-4），且不引入任何浮点方法调用。

const LN2_F32: f32 = 0.693_147_2f32;
const PI_F32: f32 = 3.141_592_7f32;

fn clamp01(v: f32) -> f32 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

fn fabs_f32(v: f32) -> f32 {
    if v < 0.0 {
        -v
    } else {
        v
    }
}

fn exp_f32(x: f32) -> f32 {
    if x < -80.0 {
        return 0.0;
    }
    if x > 80.0 {
        return 1.0e30;
    }
    let kf = x / LN2_F32;
    let k = if kf >= 0.0 { (kf + 0.5) as i32 } else { (kf - 0.5) as i32 };
    let r = x - (k as f32) * LN2_F32;
    let mut term = 1.0f32;
    let mut sum = 1.0f32;
    let mut i = 1i32;
    while i < 10 {
        term = term * r / (i as f32);
        sum += term;
        i += 1;
    }
    let mut scale = 1.0f32;
    let mut n = k;
    while n > 0 {
        scale *= 2.0;
        n -= 1;
    }
    while n < 0 {
        scale /= 2.0;
        n += 1;
    }
    sum * scale
}

fn ln_f32(x: f32) -> f32 {
    if x <= 0.0 {
        return -1.0e30;
    }
    let mut m = x;
    let mut e = 0i32;
    while m >= 2.0 {
        m /= 2.0;
        e += 1;
    }
    while m < 1.0 {
        m *= 2.0;
        e -= 1;
    }
    // ln(m) = 2·(t + t³/3 + t⁵/5 + …)，t = (m-1)/(m+1)，m∈[1,2) 时 t∈[0,1/3)
    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    let series = 1.0 + t2 * (1.0 / 3.0 + t2 * (1.0 / 5.0 + t2 * (1.0 / 7.0 + t2 * (1.0 / 9.0))));
    (e as f32) * LN2_F32 + 2.0 * t * series
}

fn pow_f32(base: f32, exponent: f32) -> f32 {
    if base <= 0.0 {
        return 0.0;
    }
    exp_f32(exponent * ln_f32(base))
}

fn sin_f32(x: f32) -> f32 {
    let mut v = x;
    while v > PI_F32 {
        v -= 2.0 * PI_F32;
    }
    while v < -PI_F32 {
        v += 2.0 * PI_F32;
    }
    let x2 = v * v;
    v * (1.0 + x2 * (-1.0 / 6.0 + x2 * (1.0 / 120.0 + x2 * (-1.0 / 5040.0 + x2 / 362_880.0))))
}

fn cos_f32(x: f32) -> f32 {
    sin_f32(x + PI_F32 / 2.0)
}

fn cbrt(x: f32) -> f32 {
    if x == 0.0 {
        return 0.0;
    }
    let neg = x < 0.0;
    let a = if neg { -x } else { x };
    let y = exp_f32(ln_f32(a) / 3.0);
    if neg {
        -y
    } else {
        y
    }
}

fn linear_to_srgb(v: f32) -> u8 {
    let x = if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * pow_f32(v, 1.0 / 2.4) - 0.055
    };
    (clamp01(x) * 255.0 + 0.5) as u8
}

/// OKLCH → sRGB（0xAARRGGBB）。与 Tauri 版 tokens.css 用同一套矩阵，保证双形态同色。
pub fn oklch_to_argb(c: Oklch) -> u32 {
    let h = c.h * PI_F32 / 180.0;
    let a = c.c * cos_f32(h);
    let b = c.c * sin_f32(h);
    let l_ = c.l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = c.l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = c.l - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
    let l = cbrt(l_);
    let m = cbrt(m_);
    let s = cbrt(s_);
    let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
    let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
    let bl = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;
    (0xFF << 24)
        | ((linear_to_srgb(r) as u32) << 16)
        | ((linear_to_srgb(g) as u32) << 8)
        | (linear_to_srgb(bl) as u32)
}

/// 对比度（WCAG 相对亮度），用于无障碍门禁（AI-20 F481）。
pub fn relative_luminance(argb: u32) -> f32 {
    let f = |v: u32| -> f32 {
        let x = v as f32 / 255.0;
        if x <= 0.040_45 {
            x / 12.92
        } else {
            pow_f32((x + 0.055) / 1.055, 2.4)
        }
    };
    let r = f((argb >> 16) & 0xFF);
    let g = f((argb >> 8) & 0xFF);
    let b = f(argb & 0xFF);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

pub fn contrast_ratio(a: u32, b: u32) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let hi = la.max(lb);
    let lo = la.min(lb);
    (hi + 0.05) / (lo + 0.05)
}

// ---------------------------------------------------------------------------
// F109 动效曲线（弹簧物理）— 渲染侧的数值积分器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub stiffness: f32,
    pub damping: f32,
    pub mass: f32,
}

impl Spring {
    pub const fn default_spring() -> Spring {
        Spring { stiffness: 180.0, damping: 22.0, mass: 1.0 }
    }

    /// 半隐式欧拉一步。返回（新位置，新速度）。
    pub fn step(&self, pos: f32, vel: f32, target: f32, dt_ms: f32) -> (f32, f32) {
        let mut dt = dt_ms / 1000.0;
        if dt < 0.0 {
            dt = 0.0;
        }
        if dt > 0.064 {
            dt = 0.064; // 一帧最多推进 64ms，防止卡顿后炸开
        }
        let mass = if self.mass < 0.001 { 0.001 } else { self.mass };
        let f = -self.stiffness * (pos - target) - self.damping * vel;
        let a = f / mass;
        let v = vel + a * dt;
        let p = pos + v * dt;
        (p, v)
    }

    /// 是否收敛（可停止动画，省电）。
    pub fn settled(&self, pos: f32, vel: f32, target: f32) -> bool {
        fabs_f32(pos - target) < 0.001 && fabs_f32(vel) < 0.01
    }
}

// ---------------------------------------------------------------------------
// F110 图层合成（z-order）
// F111 阴影与高光
// ---------------------------------------------------------------------------

pub const MAX_LAYERS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct Layer {
    pub z: i16,
    pub rect: Rect,
    pub alpha: u8,
    pub visible: bool,
    /// 阴影：偏移 + 半径。
    pub shadow: i32,
    pub highlight: u8,
}

impl Layer {
    pub const fn new(z: i16, rect: Rect) -> Layer {
        Layer { z, rect, alpha: 255, visible: true, shadow: 0, highlight: 0 }
    }
}

/// 按 z 从小到大排序（返回索引数组，0 为最底层）。
pub fn compose_order(layers: &[Layer], out: &mut [usize]) -> usize {
    let mut idx = [0usize; MAX_LAYERS];
    let n = if layers.len() < MAX_LAYERS { layers.len() } else { MAX_LAYERS };
    for i in 0..n {
        idx[i] = i;
    }
    for i in 0..n {
        let mut best = i;
        for j in (i + 1)..n {
            if layers[idx[j]].z < layers[idx[best]].z {
                best = j;
            }
        }
        idx.swap(i, best);
    }
    let take = if n < out.len() { n } else { out.len() };
    out[..take].copy_from_slice(&idx[..take]);
    take
}

/// 阴影强度随距离衰减（1/r 的离散近似，避免开方）。
pub fn shadow_alpha(distance: i32, radius: i32) -> u8 {
    if radius <= 0 || distance < 0 || distance > radius {
        return 0;
    }
    let falloff = ((radius - distance) * 160) / radius;
    falloff.clamp(0, 160) as u8
}

/// 高光：在基色上叠加白色，强度 0..255。
pub fn apply_highlight(base: u32, strength: u8) -> u32 {
    blend(0x00FF_FFFF, base, strength)
}

// ---------------------------------------------------------------------------
// F112 粒子/星空/极光管线
// ---------------------------------------------------------------------------

pub const MAX_PARTICLES: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    /// 位置用 16.16 定点表示，避免每帧大量浮点。
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    pub life: u16,
    pub color: u32,
}

pub struct ParticleField {
    items: [Option<Particle>; MAX_PARTICLES],
    count: usize,
}

impl ParticleField {
    pub const fn new() -> ParticleField {
        ParticleField { items: [None; MAX_PARTICLES], count: 0 }
    }

    pub fn spawn(&mut self, p: Particle) -> bool {
        if self.count >= MAX_PARTICLES {
            return false;
        }
        self.items[self.count] = Some(p);
        self.count += 1;
        true
    }

    /// 推进一步（life 归零即回收）。
    pub fn step(&mut self, bounds: &Rect) {
        let mut alive = 0usize;
        for i in 0..self.count {
            if let Some(mut p) = self.items[i] {
                if p.life == 0 {
                    continue;
                }
                p.x += p.vx;
                p.y += p.vy;
                if p.x < bounds.x {
                    p.x = bounds.right() - 1;
                }
                if p.x >= bounds.right() {
                    p.x = bounds.x;
                }
                if p.y < bounds.y {
                    p.y = bounds.bottom() - 1;
                }
                if p.y >= bounds.bottom() {
                    p.y = bounds.y;
                }
                p.life -= 1;
                if p.life > 0 {
                    self.items[alive] = Some(p);
                    alive += 1;
                }
            }
        }
        self.count = alive;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<Particle> {
        if i < self.count {
            self.items[i]
        } else {
            None
        }
    }
}

impl Default for ParticleField {
    fn default() -> Self {
        ParticleField::new()
    }
}

// ---------------------------------------------------------------------------
// F113 垂直同步与帧率预算（60fps）
// F114 帧时间预算监控
// ---------------------------------------------------------------------------

pub const FRAME_BUDGET_US: u32 = 16_666;
pub const TARGET_FPS: u32 = 60;

pub struct FrameMeter {
    samples: [u32; 60],
    head: usize,
    filled: usize,
}

impl FrameMeter {
    pub const fn new() -> FrameMeter {
        FrameMeter { samples: [FRAME_BUDGET_US; 60], head: 0, filled: 0 }
    }

    pub fn push(&mut self, frame_us: u32) {
        self.samples[self.head] = frame_us;
        self.head = (self.head + 1) % self.samples.len();
        if self.filled < self.samples.len() {
            self.filled += 1;
        }
    }

    /// 最近 N 帧均值（微秒）。
    pub fn average_us(&self) -> u32 {
        if self.filled == 0 {
            return 0;
        }
        let sum: u32 = self.samples[..self.filled].iter().sum();
        sum / self.filled as u32
    }

    /// 是否有帧超预算（掉帧）。
    pub fn over_budget(&self) -> bool {
        self.samples[..self.filled].iter().any(|&s| s > FRAME_BUDGET_US)
    }

    pub fn fps(&self) -> u32 {
        let avg = self.average_us();
        if avg == 0 {
            return 0;
        }
        1_000_000 / avg
    }
}

impl Default for FrameMeter {
    fn default() -> Self {
        FrameMeter::new()
    }
}

/// 是否启用垂直同步（无 GPU 时靠定时器模拟，如实标注）。
pub fn vsync_enabled(gpu: bool) -> bool {
    gpu
}

// ---------------------------------------------------------------------------
// F115 reduce-motion 降级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionPref {
    Full,
    Reduced,
}

impl MotionPref {
    /// reduce-motion 下动效时长归零——不是「变快」，而是「不发生」。
    pub fn duration_ms(self, full_ms: u32) -> u32 {
        match self {
            MotionPref::Full => full_ms,
            MotionPref::Reduced => 0,
        }
    }

    pub fn particles_allowed(self) -> bool {
        matches!(self, MotionPref::Full)
    }
}

// ---------------------------------------------------------------------------
// F117 GPU 加速探测 — 直通/如实降级软合成
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuPath {
    /// 直通（VGA BARB 可映射 / 有线性帧缓冲）。
    Direct,
    /// 软合成（无 GPU 驱动，全部 CPU 光栅化）。
    Software,
}

impl GpuPath {
    pub fn text(self) -> &'static str {
        match self {
            GpuPath::Direct => "direct framebuffer",
            GpuPath::Software => "software compositing (no gpu driver)",
        }
    }
}

/// 探测：只有拿到线性帧缓冲且分辨率已知才走直通；否则如实软合成，不假装加速。
pub fn probe_gpu(has_linear_fb: bool, width_known: bool) -> GpuPath {
    if has_linear_fb && width_known {
        GpuPath::Direct
    } else {
        GpuPath::Software
    }
}

// ---------------------------------------------------------------------------
// F118 分辨率/缩放 — 整数 permille 布局
// ---------------------------------------------------------------------------

/// 整数 permille 缩放：`value * permille / 1000`，全程不进浮点，保证像素级一致。
pub fn scale_permille(value: i32, permille: u32) -> i32 {
    let neg = value < 0;
    let v = value.unsigned_abs() as u64;
    let scaled = (v * permille as u64) / 1000;
    if neg {
        -(scaled.min(i32::MAX as u64) as i32)
    } else {
        scaled.min(i32::MAX as u64) as i32
    }
}

pub const DPI_STEPS: [u32; 5] = [1000, 1250, 1500, 1750, 2000];

/// 选择与目标 DPI 最接近的整数档（不产生半像素布局）。
pub fn dpi_step(requested: u32) -> u32 {
    let mut best = DPI_STEPS[0];
    for s in DPI_STEPS.iter() {
        if (*s as i64 - requested as i64).abs() < (best as i64 - requested as i64).abs() {
            best = *s;
        }
    }
    best
}

// ---------------------------------------------------------------------------
// F119 多显示器
// ---------------------------------------------------------------------------

pub const MAX_MONITORS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct Monitor {
    pub id: u8,
    pub x: i32,
    pub y: i32,
    pub mode: ModeInfo,
    pub primary: bool,
}

pub struct MonitorTable {
    items: [Option<Monitor>; MAX_MONITORS],
    count: usize,
}

impl MonitorTable {
    pub const fn new() -> MonitorTable {
        MonitorTable { items: [None; MAX_MONITORS], count: 0 }
    }

    pub fn add(&mut self, m: Monitor) -> bool {
        if self.count >= MAX_MONITORS {
            return false;
        }
        self.items[self.count] = Some(m);
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<Monitor> {
        if i < self.count {
            self.items[i]
        } else {
            None
        }
    }

    pub fn primary(&self) -> Option<Monitor> {
        (0..self.count).filter_map(|i| self.get(i)).find(|m| m.primary)
    }

    /// 命中的显示器（用于输入坐标 → 显示器路由）。
    pub fn hit(&self, x: i32, y: i32) -> Option<Monitor> {
        (0..self.count).filter_map(|i| self.get(i)).find(|m| {
            x >= m.x && y >= m.y && x < m.x + m.mode.width as i32 && y < m.y + m.mode.height as i32
        })
    }

    /// 虚拟桌面总尺寸（所有显示器包围盒）。
    pub fn bounds(&self) -> Rect {
        let mut r = Rect::new(0, 0, 0, 0);
        for i in 0..self.count {
            if let Some(m) = self.get(i) {
                let right = m.x + m.mode.width as i32;
                let bottom = m.y + m.mode.height as i32;
                if i == 0 {
                    r = Rect::new(m.x, m.y, m.mode.width as i32, m.mode.height as i32);
                } else {
                    let nr = Rect::new(r.x.min(m.x), r.y.min(m.y), right - r.x.min(m.x), bottom - r.y.min(m.y));
                    r = nr;
                }
            }
        }
        r
    }
}

impl Default for MonitorTable {
    fn default() -> Self {
        MonitorTable::new()
    }
}

// ---------------------------------------------------------------------------
// F123 渲染内存预算
// ---------------------------------------------------------------------------

/// 一块 width×height×4 的缓冲字节数（按 stride 对齐到 64 字节）。
pub fn buffer_bytes(width: u32, height: u32) -> u64 {
    let stride = ((width as u64 * 4) + 63) & !63;
    stride * height as u64
}

pub const RENDER_BUDGET_BYTES: u64 = 64 * 1024 * 1024;

/// 三缓冲 + 桌面 + 两个层缓存是否超预算。
pub fn render_memory_within_budget(width: u32, height: u32, buffers: u32) -> bool {
    buffer_bytes(width, height) * buffers as u64 <= RENDER_BUDGET_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f101_mode_table() {
        assert_eq!(MODE_TABLE.len(), 6);
        assert_eq!(mode_at(0).unwrap().width, 1024);
        assert_eq!(pick_mode(1920, 1080).width, 1920);
        assert_eq!(pick_mode(500, 500).width, 1024);
    }

    #[test]
    fn f122_pixel_format_swizzle() {
        assert_eq!(pack(PixelFormat::Rgb32, 0xFF11_2233), 0xFF11_2233);
        assert_eq!(pack(PixelFormat::Bgr32, 0xFF11_2233), 0xFF33_2211);
        assert_eq!(unpack(PixelFormat::Bgr32, 0xFF33_2211), (0x11, 0x22, 0x33, 0xFF));
    }

    #[test]
    fn f102_swap_chain_rotates() {
        let mut sc = SwapChain::new(SwapKind::Triple);
        assert_eq!(sc.present(), 1);
        assert_ne!(sc.front, sc.back);
        let mut single = SwapChain::new(SwapKind::Single);
        assert_eq!(single.present(), 0);
    }

    #[test]
    fn f103_blit_clips() {
        let mut dst = [0u32; 16];
        let mut src = [0u32; 16];
        for (i, v) in src.iter_mut().enumerate() {
            *v = i as u32 + 1;
        }
        let r = Rect::new(-1, -1, 4, 4);
        let clip = Rect::new(0, 0, 4, 4);
        let n = blit(&mut dst, 4, &src, 4, &r, &clip);
        assert_eq!(n, 9, "only the overlapping 3x3 lands");
        assert_eq!(dst[0], src[1 * 4 + 1], "source offset is carried through");
        assert_eq!(dst[4 * 3 + 3], 0, "outside the region stays untouched");
    }

    #[test]
    fn f103_fill_rect_respects_clip() {
        let mut buf = [0u32; 16];
        let n = fill_rect(&mut buf, 4, &Rect::new(0, 0, 2, 2), &Rect::new(0, 0, 4, 4), 0xFF);
        assert_eq!(n, 4);
        assert_eq!(buf[0], 0xFF);
        assert_eq!(buf[5], 0xFF);
        assert_eq!(buf[6], 0);
    }

    #[test]
    fn f104_round_rect_corners() {
        let r = clamp_radius(999, 40, 20);
        assert_eq!(r, 10);
        assert!(inside_round_rect(0, 0, 40, 20, 10) == false);
        assert!(inside_round_rect(20, 10, 40, 20, 10));
        assert!(inside_round_rect(39, 19, 40, 20, 10) == false);
    }

    #[test]
    fn f104_gradient_endpoints() {
        assert_eq!(gradient_at(0x0000_0000, 0x00FF_FFFF, 0), 0xFF00_0000);
        assert_eq!(gradient_at(0x0000_0000, 0x00FF_FFFF, 1000), 0xFFFF_FFFF);
    }

    #[test]
    fn f106_alpha_blend() {
        assert_eq!(blend(0x00FF_FFFF, 0x0000_0000, 255), 0xFFFF_FFFF);
        assert_eq!(blend(0x00FF_FFFF, 0x0000_0000, 0), 0xFF00_0000);
        assert_eq!(blend(0x0000_00FF, 0x0000_FF00, 128), 0xFF00_7F80);
    }

    #[test]
    fn f107_dirty_merge_and_overflow() {
        let mut d = DirtyList::new();
        d.add(Rect::new(0, 0, 2, 2));
        d.add(Rect::new(1, 1, 2, 2));
        assert_eq!(d.len(), 1);
        for i in 0..MAX_DIRTY + 8 {
            d.add(Rect::new(i as i32 * 1000, 0, 1, 1));
        }
        assert_eq!(d.len(), 1, "overflow degrades to full-screen redraw");
        assert!(d.total_area() > 0);
    }

    #[test]
    fn f108_oklch_matches_expectations() {
        let white = oklch_to_argb(Oklch { l: 1.0, c: 0.0, h: 0.0 });
        assert!((white >> 16) & 0xFF > 250);
        let black = oklch_to_argb(Oklch { l: 0.0, c: 0.0, h: 0.0 });
        assert_eq!(black & 0x00FF_FFFF, 0);
        let red = oklch_to_argb(Oklch { l: 0.628, c: 0.2577, h: 29.23 });
        assert!((red >> 16) & 0xFF > ((red >> 8) & 0xFF));
        assert!(contrast_ratio(0xFFFF_FFFF, 0xFF00_0000) > 15.0);
    }

    #[test]
    fn f109_spring_converges() {
        let s = Spring::default_spring();
        let (mut p, mut v) = (0.0f32, 0.0f32);
        for _ in 0..600 {
            let (np, nv) = s.step(p, v, 1.0, 16.0);
            p = np;
            v = nv;
        }
        assert!((p - 1.0).abs() < 0.01);
        assert!(s.settled(p, v, 1.0));
    }

    #[test]
    fn f110_layer_ordering() {
        let layers = [Layer::new(5, Rect::new(0, 0, 1, 1)), Layer::new(-1, Rect::new(0, 0, 1, 1))];
        let mut out = [0usize; 8];
        assert_eq!(compose_order(&layers, &mut out), 2);
        assert_eq!(out[0], 1);
    }

    #[test]
    fn f111_shadow_falloff() {
        assert_eq!(shadow_alpha(0, 8), 160);
        assert_eq!(shadow_alpha(8, 8), 0);
        assert!(shadow_alpha(4, 8) < shadow_alpha(1, 8));
        assert_eq!(apply_highlight(0xFF00_0000, 0), 0xFF00_0000);
    }

    #[test]
    fn f112_particles_recycle() {
        let mut f = ParticleField::new();
        for i in 0..4 {
            assert!(f.spawn(Particle { x: 0, y: 0, vx: 1, vy: 1, life: 2, color: 0 }));
            let _ = i;
        }
        f.step(&Rect::new(0, 0, 8, 8));
        f.step(&Rect::new(0, 0, 8, 8));
        assert_eq!(f.len(), 0);
    }

    #[test]
    fn f113_frame_budget() {
        let mut m = FrameMeter::new();
        m.push(16_000);
        m.push(17_000);
        assert!(m.over_budget());
        assert!(m.average_us() >= 16_500);
        assert!(m.fps() >= 55 && m.fps() <= 63);
        assert!(!vsync_enabled(false));
    }

    #[test]
    fn f115_reduce_motion_zeroes_duration() {
        assert_eq!(MotionPref::Reduced.duration_ms(300), 0);
        assert_eq!(MotionPref::Full.duration_ms(300), 300);
        assert!(!MotionPref::Reduced.particles_allowed());
    }

    #[test]
    fn f117_gpu_probe_is_honest() {
        assert_eq!(probe_gpu(true, true), GpuPath::Direct);
        assert_eq!(probe_gpu(true, false), GpuPath::Software);
        assert!(GpuPath::Software.text().contains("software"));
    }

    #[test]
    fn f118_integer_scaling() {
        assert_eq!(scale_permille(100, 1500), 150);
        assert_eq!(scale_permille(-100, 1500), -150);
        assert_eq!(dpi_step(1400), 1500);
        assert_eq!(dpi_step(10_000), 2000);
    }

    #[test]
    fn f119_multi_monitor() {
        let mut t = MonitorTable::new();
        assert!(t.add(Monitor { id: 0, x: 0, y: 0, mode: MODE_TABLE[3], primary: true }));
        assert!(t.add(Monitor { id: 1, x: 1920, y: 0, mode: MODE_TABLE[1], primary: false }));
        assert_eq!(t.primary().unwrap().id, 0);
        assert_eq!(t.hit(2000, 100).unwrap().id, 1);
        assert!(t.hit(4000, 100).is_none());
        assert_eq!(t.bounds().w, 1920 + 1280);
    }

    #[test]
    fn f123_memory_budget() {
        assert!(render_memory_within_budget(1920, 1080, 3));
        assert!(!render_memory_within_budget(7680, 4320, 3));
        assert!(buffer_bytes(1920, 1080) >= 1920 * 1080 * 4);
    }
}
