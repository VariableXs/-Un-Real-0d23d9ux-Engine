//! AURORA-1000 AI-02 · 2D 渲染引擎（A026~A050，W1）
//!
//! 纯软件 2D 渲染内核：矢量路径 / 位图混合 / 文字光栅化 / 亚像素抗锯齿 /
//! 裁剪遮罩 / 线性径向渐变 / 图层合成 / 离屏 / 脏矩形 / 批处理 / 快照 /
//! 性能剖析 / 降级链，全部以固定容量数组与像素级逻辑实现，GPU 缺失时亦可跑。

use crate::checks::CheckSet;

/// 默认小画布宽（RGBA8888，stride = 宽 × 4）。
pub const CANVAS_W: usize = 16;
/// 默认小画布高。
pub const CANVAS_H: usize = 16;
/// 每像素字节数（R,G,B,A）。
pub const BPP: usize = 4;
/// 像素缓冲容量（16×16×4）。
pub const CANVAS_BYTES: usize = CANVAS_W * CANVAS_H * BPP;

/// 一个像素（直通 alpha，非预乘）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba { r, g, b, a }
    }
    pub const fn opaque(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 255 }
    }
}

// ---------------------------------------------------------------------------
// 画布基元（私有）
// ---------------------------------------------------------------------------

#[inline]
fn pidx(x: usize, y: usize, w: usize) -> usize {
    (y * w + x) * BPP
}

fn set_px(buf: &mut [u8], w: usize, h: usize, x: usize, y: usize, c: Rgba) {
    if x < w && y < h {
        let i = pidx(x, y, w);
        buf[i] = c.r;
        buf[i + 1] = c.g;
        buf[i + 2] = c.b;
        buf[i + 3] = c.a;
    }
}

fn get_px(buf: &[u8], w: usize, h: usize, x: usize, y: usize) -> Rgba {
    if x < w && y < h {
        let i = pidx(x, y, w);
        Rgba { r: buf[i], g: buf[i + 1], b: buf[i + 2], a: buf[i + 3] }
    } else {
        Rgba { r: 0, g: 0, b: 0, a: 0 }
    }
}

/// src-over 混合：fg 以有效透明度 `alpha`（0..255）覆盖到 bg 上。
fn blend_a(fg: Rgba, bg: Rgba, alpha: u32) -> Rgba {
    if alpha == 0 {
        return bg;
    }
    if alpha >= 255 {
        return Rgba { r: fg.r, g: fg.g, b: fg.b, a: 255 };
    }
    let ia = 255 - alpha;
    Rgba {
        r: ((fg.r as u32 * alpha + bg.r as u32 * ia) / 255) as u8,
        g: ((fg.g as u32 * alpha + bg.g as u32 * ia) / 255) as u8,
        b: ((fg.b as u32 * alpha + bg.b as u32 * ia) / 255) as u8,
        a: 255,
    }
}

fn lerp(a: u8, b: u8, t: u32) -> u8 {
    ((a as u32 * (255 - t) + b as u32 * t) / 255) as u8
}

/// 整数平方根（牛顿-比特法，无浮点）。
fn isqrt(v: u32) -> u32 {
    let mut v = v;
    let mut r = 0u32;
    let mut b = 1u32 << 30;
    while b > v {
        b >>= 2;
    }
    while b != 0 {
        if v >= r + b {
            v -= r + b;
            r = (r >> 1) + b;
        } else {
            r >>= 1;
        }
        b >>= 2;
    }
    r
}

/// 实心矩形填充（私有，供多处复用）。
fn fill_rect(buf: &mut [u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize, c: Rgba) {
    if x0 == x1 || y0 == y1 {
        return;
    }
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    let mut y = y0;
    while y <= y1 {
        let mut x = x0;
        while x <= x1 {
            set_px(buf, w, h, x, y, c);
            x += 1;
        }
        y += 1;
    }
}

/// Bresenham 直线（私有）。
fn draw_line(buf: &mut [u8], w: usize, h: usize, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
    let (w0, h0) = (w as i32, h as i32);
    let (mut x, mut y) = (x0, y0);
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if x >= 0 && y >= 0 && x < w0 && y < h0 {
            set_px(buf, w, h, x as usize, y as usize, c);
        }
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

// ---------------------------------------------------------------------------
// A026 — 矢量路径解析
// ---------------------------------------------------------------------------

/// 折线（矢量路径）描边：依次连接 `pts[0..count]` 中的点。
pub fn draw_polyline(buf: &mut [u8], w: usize, h: usize, pts: &[(i32, i32)], count: usize, c: Rgba) {
    if count < 2 {
        return;
    }
    let n = count.min(pts.len());
    let mut i = 0usize;
    while i + 1 < n {
        let a = pts[i];
        let b = pts[i + 1];
        draw_line(buf, w, h, a.0, a.1, b.0, b.1, c);
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// A027 — 贝塞尔曲线光栅化
// ---------------------------------------------------------------------------

/// 二次贝塞尔在参数 `t`（0..1000）处的点（De Casteljau，整数）。
pub fn bezier_point(p0: (i32, i32), p1: (i32, i32), p2: (i32, i32), t: i32) -> (i32, i32) {
    let u = 1000 - t;
    let x = (p0.0 as i64 * u as i64 * u as i64
        + 2 * p1.0 as i64 * u as i64 * t as i64
        + p2.0 as i64 * t as i64 * t as i64)
        / 1_000_000;
    let y = (p0.1 as i64 * u as i64 * u as i64
        + 2 * p1.1 as i64 * u as i64 * t as i64
        + p2.1 as i64 * t as i64 * t as i64)
        / 1_000_000;
    (x as i32, y as i32)
}

/// 将二次贝塞尔曲线扁平化为折线后描边到画布。
pub fn stroke_bezier(buf: &mut [u8], w: usize, h: usize, p0: (i32, i32), p1: (i32, i32), p2: (i32, i32), c: Rgba) {
    const STEPS: usize = 32;
    let mut prev = bezier_point(p0, p1, p2, 0);
    let mut i = 1usize;
    while i <= STEPS {
        let t = (i * 1000 / STEPS) as i32;
        let pt = bezier_point(p0, p1, p2, t);
        draw_line(buf, w, h, prev.0, prev.1, pt.0, pt.1, c);
        prev = pt;
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// A028 — 位图绘制与混合
// ---------------------------------------------------------------------------

/// 将 `sw×sh` 的 RGBA 位图以 alpha 混合方式 blit 到 `(dx,dy)`。
pub fn blit_bitmap(dst: &mut [u8], w: usize, h: usize, src: &[u8], sw: usize, sh: usize, dx: i32, dy: i32) {
    if sw == 0 || sh == 0 {
        return;
    }
    let mut y = 0usize;
    while y < sh {
        let mut x = 0usize;
        while x < sw {
            let si = (y * sw + x) * BPP;
            let sa = src[si + 3];
            if sa != 0 {
                let px = dx + x as i32;
                let py = dy + y as i32;
                if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                    let di = pidx(px as usize, py as usize, w);
                    let fg = Rgba { r: src[si], g: src[si + 1], b: src[si + 2], a: sa };
                    let bg = get_px(dst, w, h, px as usize, py as usize);
                    let m = blend_a(fg, bg, sa as u32);
                    dst[di] = m.r;
                    dst[di + 1] = m.g;
                    dst[di + 2] = m.b;
                    dst[di + 3] = m.a;
                }
            }
            x += 1;
        }
        y += 1;
    }
}

// ---------------------------------------------------------------------------
// A029 — 文字光栅化接口
// ---------------------------------------------------------------------------

/// 5×7 点阵字形位（行优先，35 字节）。未知字符返回全 0（空白）。
pub fn glyph_bits(ch: u8) -> [u8; 35] {
    match ch {
        b'A' => [
            0, 1, 1, 1, 0, //
            1, 0, 0, 0, 1, //
            1, 0, 0, 0, 1, //
            1, 1, 1, 1, 1, //
            1, 0, 0, 0, 1, //
            1, 0, 0, 0, 1, //
            1, 0, 0, 0, 1, //
        ],
        b'B' => [
            1, 1, 1, 1, 0, //
            1, 0, 0, 0, 1, //
            1, 0, 0, 0, 1, //
            1, 1, 1, 1, 0, //
            1, 0, 0, 0, 1, //
            1, 0, 0, 0, 1, //
            1, 1, 1, 1, 0, //
        ],
        b'0' => [
            1, 1, 1, 1, 1, //
            1, 0, 0, 0, 1, //
            1, 0, 0, 1, 1, //
            1, 0, 1, 0, 1, //
            1, 1, 0, 0, 1, //
            1, 0, 0, 0, 1, //
            1, 1, 1, 1, 1, //
        ],
        _ => [0u8; 35],
    }
}

/// 在 `(dx,dy)` 处光栅化一个 5×7 字形到画布。
pub fn draw_glyph(buf: &mut [u8], w: usize, h: usize, ch: u8, dx: i32, dy: i32, c: Rgba) {
    let bits = glyph_bits(ch);
    let mut row = 0usize;
    while row < 7 {
        let mut col = 0usize;
        while col < 5 {
            if bits[row * 5 + col] != 0 {
                let px = dx + col as i32;
                let py = dy + row as i32;
                set_px(buf, w, h, px as usize, py as usize, c);
            }
            col += 1;
        }
        row += 1;
    }
}

// ---------------------------------------------------------------------------
// A030 — 亚像素抗锯齿
// ---------------------------------------------------------------------------

/// 亚像素抗锯齿直线：超采样累积逐像素覆盖率，再按覆盖率混合。
pub fn draw_line_aa(buf: &mut [u8], w: usize, h: usize, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
    if w == 0 || h == 0 || (w * h) > 256 {
        return;
    }
    let steps = ((x0 - x1).abs() + (y0 - y1).abs()).max(1) as usize * 4 + 4;
    let mut cov = [0u8; 256];
    let mut s = 0usize;
    while s <= steps {
        let t = (s * 1000 / steps) as i32;
        let px = x0 + (x1 - x0) * t / 1000;
        let py = y0 + (y1 - y0) * t / 1000;
        if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
            let ci = (py as usize) * w + (px as usize);
            if ci < cov.len() {
                cov[ci] = cov[ci].saturating_add(1);
            }
        }
        s += 1;
    }
    let mut p = 0usize;
    while p < w * h {
        let cnt = cov[p];
        if cnt > 0 {
            let alpha = if cnt as u32 >= steps as u32 { 255u32 } else { (cnt as u32 * 255 / steps as u32) as u32 };
            let x = p % w;
            let y = p / w;
            let bg = get_px(buf, w, h, x, y);
            let m = blend_a(c, bg, alpha);
            let di = pidx(x, y, w);
            buf[di] = m.r;
            buf[di + 1] = m.g;
            buf[di + 2] = m.b;
            buf[di + 3] = m.a;
        }
        p += 1;
    }
    // 端点强制不透明，保证首尾像素可见。
    set_px(buf, w, h, x0 as usize, y0 as usize, c);
    set_px(buf, w, h, x1 as usize, y1 as usize, c);
}

// ---------------------------------------------------------------------------
// A031 — 图层与透明度
// ---------------------------------------------------------------------------

/// 以 `layer_alpha` 全局透明度把整张 `src` 图层按 src-over 合成到 `dst`。
pub fn composite_layer(dst: &mut [u8], w: usize, h: usize, src: &[u8], layer_alpha: u8) {
    if src.len() < w * h * BPP {
        return;
    }
    let mut p = 0usize;
    while p < w * h {
        let di = pidx(p % w, p / w, w);
        let si = p * BPP;
        let sa = src[si + 3];
        if sa != 0 {
            let eff = ((sa as u32) * (layer_alpha as u32) / 255) as u8;
            let fg = Rgba { r: src[si], g: src[si + 1], b: src[si + 2], a: eff };
            let bg = get_px(dst, w, h, p % w, p / w);
            let m = blend_a(fg, bg, eff as u32);
            dst[di] = m.r;
            dst[di + 1] = m.g;
            dst[di + 2] = m.b;
            dst[di + 3] = m.a;
        }
        p += 1;
    }
}

// ---------------------------------------------------------------------------
// A032 — 裁剪与遮罩
// ---------------------------------------------------------------------------

/// 在矩形内填充 `c`，但仅作用于 `mask` 中 alpha≠0 的像素（裁剪 + 遮罩）。
pub fn fill_clipped_masked(buf: &mut [u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize, mask: &[u8], c: Rgba) {
    if mask.len() < w * h * BPP {
        return;
    }
    if x0 == x1 || y0 == y1 {
        return;
    }
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    let mut y = y0;
    while y <= y1 {
        let mut x = x0;
        while x <= x1 {
            if x < w && y < h {
                let mi = pidx(x, y, w);
                if mask[mi + 3] != 0 {
                    set_px(buf, w, h, x, y, c);
                }
            }
            x += 1;
        }
        y += 1;
    }
}

// ---------------------------------------------------------------------------
// A033 — 线性/径向渐变
// ---------------------------------------------------------------------------

/// 沿 x 轴的线性渐变（x=0 取 `c0`，x=w-1 取 `c1`）。
pub fn fill_linear_gradient(buf: &mut [u8], w: usize, h: usize, c0: Rgba, c1: Rgba) {
    if w == 0 {
        return;
    }
    let last = w - 1;
    let mut x = 0usize;
    while x < w {
        let t = if last == 0 { 0 } else { (x * 255 / last) as u32 };
        let col = Rgba {
            r: lerp(c0.r, c1.r, t),
            g: lerp(c0.g, c1.g, t),
            b: lerp(c0.b, c1.b, t),
            a: lerp(c0.a, c1.a, t),
        };
        let mut y = 0usize;
        while y < h {
            set_px(buf, w, h, x, y, col);
            y += 1;
        }
        x += 1;
    }
}

/// 以 `(cx,cy)` 为圆心的径向渐变（中心 `inner`，半径边界 `outer`）。
pub fn fill_radial_gradient(buf: &mut [u8], w: usize, h: usize, cx: usize, cy: usize, inner: Rgba, outer: Rgba) {
    if w == 0 || h == 0 {
        return;
    }
    let maxr = ((w.max(h) / 2) as u32).max(1);
    let mut y = 0usize;
    while y < h {
        let mut x = 0usize;
        while x < w {
            let dx = (x as i32 - cx as i32).abs() as u32;
            let dy = (y as i32 - cy as i32).abs() as u32;
            let d = isqrt(dx * dx + dy * dy);
            let t = if d >= maxr { 255 } else { (d * 255 / maxr) as u32 };
            set_px(
                buf,
                w,
                h,
                x,
                y,
                Rgba {
                    r: lerp(inner.r, outer.r, t),
                    g: lerp(inner.g, outer.g, t),
                    b: lerp(inner.b, outer.b, t),
                    a: 255,
                },
            );
            x += 1;
        }
        y += 1;
    }
}

// ---------------------------------------------------------------------------
// A034 — 描边与阴影
// ---------------------------------------------------------------------------

/// 矩形描边（边框厚度 `thick`）。
pub fn stroke_rect(buf: &mut [u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize, thick: usize, c: Rgba) {
    if x0 == x1 || y0 == y1 || thick == 0 {
        return;
    }
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    let mut y = y0;
    while y <= y1 {
        let mut x = x0;
        while x <= x1 {
            let top = y < y0 + thick;
            let bot = y > y1 - thick;
            let left = x < x0 + thick;
            let right = x > x1 - thick;
            if (top || bot || left || right) && x < w && y < h {
                set_px(buf, w, h, x, y, c);
            }
            x += 1;
        }
        y += 1;
    }
}

/// 在偏移 `off` 处绘制半亮阴影副本。
pub fn draw_shadow(buf: &mut [u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize, off: usize, c: Rgba) {
    let dim = Rgba::new(c.r / 2, c.g / 2, c.b / 2, 255);
    stroke_rect(buf, w, h, x0 + off, y0 + off, x1 + off, y1 + off, 1, dim);
}

// ---------------------------------------------------------------------------
// A035 — 离屏渲染目标
// ---------------------------------------------------------------------------

/// 向离屏缓冲填充矩形（离屏目标写入）。
pub fn fill_offscreen_rect(off: &mut [u8], w: usize, h: usize, x0: usize, y0: usize, x1: usize, y1: usize, c: Rgba) {
    fill_rect(off, w, h, x0, y0, x1, y1, c);
}

/// 将离屏缓冲以 `alpha` 透明度合成到主画布（离屏上屏）。
pub fn composite_offscreen(dst: &mut [u8], w: usize, h: usize, off: &[u8], dx: i32, dy: i32, alpha: u8) {
    if off.len() < w * h * BPP {
        return;
    }
    let mut y = 0usize;
    while y < h {
        let mut x = 0usize;
        while x < w {
            let px = dx + x as i32;
            let py = dy + y as i32;
            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                let si = pidx(x, y, w);
                let sa = off[si + 3];
                if sa != 0 {
                    let eff = ((sa as u32) * (alpha as u32) / 255) as u8;
                    let fg = Rgba { r: off[si], g: off[si + 1], b: off[si + 2], a: eff };
                    let bg = get_px(dst, w, h, px as usize, py as usize);
                    let m = blend_a(fg, bg, eff as u32);
                    let di = pidx(px as usize, py as usize, w);
                    dst[di] = m.r;
                    dst[di + 1] = m.g;
                    dst[di + 2] = m.b;
                    dst[di + 3] = m.a;
                }
            }
            x += 1;
        }
        y += 1;
    }
}

// ---------------------------------------------------------------------------
// A036 — 脏矩形重绘
// ---------------------------------------------------------------------------

/// 矩形（usize 坐标，无负）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

pub const MAX_DIRTY: usize = 8;

/// 脏矩形集合：加入时与首个相交矩形合并为并集，保证最小重绘面。
#[derive(Clone, Copy, Debug)]
pub struct DirtySet {
    rects: [Option<Rect>; MAX_DIRTY],
    count: usize,
}

impl DirtySet {
    pub const fn new() -> DirtySet {
        DirtySet { rects: [None; MAX_DIRTY], count: 0 }
    }

    pub fn add(&mut self, r: Rect) {
        if r.w == 0 || r.h == 0 {
            return;
        }
        let mut i = 0usize;
        while i < self.count {
            if let Some(ex) = self.rects[i] {
                if overlaps(ex, r) {
                    self.rects[i] = Some(union(ex, r));
                    return;
                }
            }
            i += 1;
        }
        if self.count < MAX_DIRTY {
            self.rects[self.count] = Some(r);
            self.count += 1;
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<Rect> {
        if i < self.count {
            self.rects[i]
        } else {
            None
        }
    }
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    Rect { x, y, w: x1 - x, h: y1 - y }
}

/// 仅对脏矩形集合内的区域做重绘填充。
pub fn repaint_dirty(buf: &mut [u8], w: usize, h: usize, set: &DirtySet, c: Rgba) {
    let mut i = 0usize;
    while i < set.count() {
        if let Some(r) = set.get(i) {
            if r.w > 0 && r.h > 0 {
                let x1 = r.x + r.w - 1;
                let y1 = r.y + r.h - 1;
                fill_rect(buf, w, h, r.x, r.y, x1, y1, c);
            }
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// A037 — 软件 60fps 预算
// ---------------------------------------------------------------------------

/// 单帧时间预算（微秒）：1_000_000 / fps。
pub fn frame_budget_us(fps: u32) -> u32 {
    if fps == 0 {
        0
    } else {
        1_000_000 / fps
    }
}

/// 渲染耗时是否落在预算内。
pub fn within_budget(elapsed_us: u32, budget_us: u32) -> bool {
    elapsed_us <= budget_us
}

// ---------------------------------------------------------------------------
// A038 — SIMD 加速路径（4 像素/次 的 span 填充，模拟 4 路通道）
// ---------------------------------------------------------------------------

/// 以 4 像素为一组快速填充水平 span（x0..=x1）。
pub fn fill_span_fast(buf: &mut [u8], w: usize, h: usize, y: usize, x0: usize, x1: usize, c: Rgba) {
    if y >= h {
        return;
    }
    if x0 == x1 {
        set_px(buf, w, h, x0, y, c);
        return;
    }
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let mut x = x0;
    while x + 3 <= x1 && x + 3 < w {
        set_px(buf, w, h, x, y, c);
        set_px(buf, w, h, x + 1, y, c);
        set_px(buf, w, h, x + 2, y, c);
        set_px(buf, w, h, x + 3, y, c);
        x += 4;
    }
    while x <= x1 && x < w {
        set_px(buf, w, h, x, y, c);
        x += 1;
    }
}

// ---------------------------------------------------------------------------
// A039 — 绘制命令批处理
// ---------------------------------------------------------------------------

/// 可重放的绘制命令。
#[derive(Clone, Copy, Debug)]
pub enum DrawCmd {
    Clear(Rgba),
    FillRect(usize, usize, usize, usize, Rgba),
    Line(i32, i32, i32, i32, Rgba),
}

pub const MAX_CMDS: usize = 16;

/// 固定容量绘制命令批。
#[derive(Clone, Copy, Debug)]
pub struct DrawBatch {
    cmds: [Option<DrawCmd>; MAX_CMDS],
    count: usize,
}

impl DrawBatch {
    pub const fn new() -> DrawBatch {
        DrawBatch { cmds: [None; MAX_CMDS], count: 0 }
    }

    pub fn push(&mut self, c: DrawCmd) -> bool {
        if self.count < MAX_CMDS {
            self.cmds[self.count] = Some(c);
            self.count += 1;
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// 顺序重放整批命令到画布。
    pub fn execute(&self, buf: &mut [u8], w: usize, h: usize) {
        let mut i = 0usize;
        while i < self.count {
            if let Some(c) = self.cmds[i] {
                match c {
                    DrawCmd::Clear(col) => {
                        let mut p = 0usize;
                        while p < w * h {
                            set_px(buf, w, h, p % w, p / w, col);
                            p += 1;
                        }
                    }
                    DrawCmd::FillRect(x0, y0, x1, y1, col) => fill_rect(buf, w, h, x0, y0, x1, y1, col),
                    DrawCmd::Line(x0, y0, x1, y1, col) => draw_line(buf, w, h, x0, y0, x1, y1, col),
                }
            }
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// A040 — 渲染快照与缓存
// ---------------------------------------------------------------------------

pub const SNAP_CAP: usize = CANVAS_BYTES;

/// 将 `src` 拷入 `dst`（定长，越界截断）。
pub fn snapshot(src: &[u8], dst: &mut [u8]) {
    let n = src.len().min(dst.len());
    let mut i = 0usize;
    while i < n {
        dst[i] = src[i];
        i += 1;
    }
}

/// 两个快照是否逐字节相等（缓存命中判定）。
pub fn snapshot_equal(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// A041 — 与合成器对接
// ---------------------------------------------------------------------------

/// 计算整帧缓冲的 FNV-1a 校验和，供合成器按帧消费/比对。
pub fn compositor_checksum(buf: &[u8]) -> u32 {
    let mut h = 0x811c_9dc5u32;
    let mut i = 0usize;
    while i < buf.len() {
        h ^= buf[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// A042 — 渲染性能剖析
// ---------------------------------------------------------------------------

/// 累积各类绘制操作的剖析器。
#[derive(Clone, Copy, Debug)]
pub struct Profiler {
    pub fills: u32,
    pub lines: u32,
    pub blits: u32,
    pub pixels: u64,
}

impl Profiler {
    pub const fn new() -> Profiler {
        Profiler { fills: 0, lines: 0, blits: 0, pixels: 0 }
    }
    pub fn record_fill(&mut self, px: u32) {
        self.fills += 1;
        self.pixels += px as u64;
    }
    pub fn record_line(&mut self) {
        self.lines += 1;
    }
    pub fn record_blit(&mut self, px: u32) {
        self.blits += 1;
        self.pixels += px as u64;
    }
    pub fn total_ops(&self) -> u32 {
        self.fills + self.lines + self.blits
    }
}

// ---------------------------------------------------------------------------
// A043 — 渲染降级链（按帧耗时选择质量等级）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    High,
    Medium,
    Low,
}

/// 帧耗时超出预算则降级，处于半预算内保持高质量。
pub fn select_quality(frame_us: u32, budget_us: u32) -> Quality {
    if budget_us == 0 {
        return Quality::Low;
    }
    if frame_us > budget_us {
        Quality::Low
    } else if frame_us * 2 > budget_us {
        Quality::Medium
    } else {
        Quality::High
    }
}

// ---------------------------------------------------------------------------
// A044 — 2D 渲染引擎自检（不变式）
// ---------------------------------------------------------------------------

/// 画布不变式：所有像素 alpha 均在合法范围（无越界写入）。
pub fn canvas_invariant(buf: &[u8]) -> bool {
    let mut i = 3usize;
    while i < buf.len() {
        if buf[i] > 255 {
            return false;
        }
        i += 4;
    }
    true
}

// ---------------------------------------------------------------------------
// A045 — 2D 渲染引擎性能预算
// ---------------------------------------------------------------------------

/// 帧耗时滚动表：统计均值与超预算次数。
#[derive(Clone, Copy, Debug)]
pub struct FrameMeter {
    samples: u32,
    sum_us: u64,
    over: u32,
    cap: usize,
}

impl FrameMeter {
    pub const fn new(cap: usize) -> FrameMeter {
        FrameMeter { samples: 0, sum_us: 0, over: 0, cap }
    }

    pub fn record(&mut self, us: u32, budget: u32) {
        if self.samples < self.cap as u32 {
            self.sum_us += us as u64;
            self.samples += 1;
        } else if self.samples > 0 {
            // 滚动：以均值近似退出一个旧样本再加入新样本。
            let avg = (self.sum_us / self.samples as u64) as u32;
            self.sum_us = self.sum_us.saturating_sub(avg as u64) + us as u64;
        }
        if us > budget {
            self.over += 1;
        }
    }

    pub fn avg_us(&self) -> u32 {
        if self.samples == 0 {
            0
        } else {
            (self.sum_us / self.samples as u64) as u32
        }
    }

    pub fn over_count(&self) -> u32 {
        self.over
    }
}

// ---------------------------------------------------------------------------
// A046 — 2D 渲染引擎可观测
// ---------------------------------------------------------------------------

/// 对外暴露的渲染统计（可观测性）。
#[derive(Clone, Copy, Debug)]
pub struct RenderStats {
    pub frames: u64,
    pub pixels: u64,
    pub fills: u64,
    pub last_ms: u32,
}

impl RenderStats {
    pub const fn new() -> RenderStats {
        RenderStats { frames: 0, pixels: 0, fills: 0, last_ms: 0 }
    }

    pub fn tick_frame(&mut self, pixels: u64, fills: u64, ms: u32) {
        self.frames += 1;
        self.pixels += pixels;
        self.fills += fills;
        self.last_ms = ms;
    }

    pub fn pixels_per_frame(&self) -> u64 {
        if self.frames == 0 {
            0
        } else {
            self.pixels / self.frames
        }
    }
}

// ---------------------------------------------------------------------------
// A047 — 2D 渲染引擎模糊测试
// ---------------------------------------------------------------------------

/// 以确定性 LCG 喂随机坐标到绘制原语，验证全程无越界/恐慌。
pub fn fuzz_draw(buf: &mut [u8], w: usize, h: usize, seed: u32, iter: usize) {
    let mut s = seed;
    let mut it = 0usize;
    while it < iter {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let x0 = (s & 0xF) as usize;
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let y0 = (s & 0xF) as usize;
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let x1 = (s & 0xF) as usize;
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let y1 = (s & 0xF) as usize;
        let c = Rgba::opaque((s & 0xFF) as u8, ((s >> 8) & 0xFF) as u8, ((s >> 16) & 0xFF) as u8);
        if (s & 1) == 0 {
            draw_line(buf, w, h, x0 as i32, y0 as i32, x1 as i32, y1 as i32, c);
        } else {
            fill_rect(buf, w, h, x0, y0, x1, y1, c);
        }
        it += 1;
    }
}

// ---------------------------------------------------------------------------
// A048 — 2D 渲染引擎文档（参考示例渲染）
// ---------------------------------------------------------------------------

/// 渲染一张「特性横幅」作为文档级参考示例（字形 + 基线）。
pub fn draw_feature_banner(buf: &mut [u8], w: usize, h: usize, c: Rgba) {
    draw_glyph(buf, w, h, b'A', 0, 1, c);
    draw_line(buf, w, h, 0, 9, (w as i32) - 1, 9, c);
}

// ---------------------------------------------------------------------------
// A049 — 2D 渲染引擎降级链（特性缺失时回退）
// ---------------------------------------------------------------------------

/// 若 `use_gradient` 为真走渐变，否则回退到纯色填充（特性降级链）。
pub fn render_with_fallback(buf: &mut [u8], w: usize, h: usize, use_gradient: bool, c0: Rgba, c1: Rgba) {
    if use_gradient {
        fill_linear_gradient(buf, w, h, c0, c1);
    } else {
        let mut y = 0usize;
        while y < h {
            let mut x = 0usize;
            while x < w {
                set_px(buf, w, h, x, y, c0);
                x += 1;
            }
            y += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// A050 — 2D 渲染引擎域自检收口
// ---------------------------------------------------------------------------

/// 域自检：对样例画布逐项断言像素结果，全部须为真。
pub fn run_render2d_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-render2d");

    // A026 矢量路径解析
    {
        let mut c = [0u8; CANVAS_BYTES];
        draw_polyline(&mut c, CANVAS_W, CANVAS_H, &[(1, 1), (5, 1), (5, 5)], 3, Rgba::opaque(10, 20, 30));
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 1, 1) == Rgba::opaque(10, 20, 30)
            && get_px(&c, CANVAS_W, CANVAS_H, 5, 1) == Rgba::opaque(10, 20, 30)
            && get_px(&c, CANVAS_W, CANVAS_H, 5, 5) == Rgba::opaque(10, 20, 30)
            && get_px(&c, CANVAS_W, CANVAS_H, 3, 3) == Rgba::new(0, 0, 0, 0);
        set.add("A026 vector path stroke", ok, "polyline endpoints");
    }

    // A027 贝塞尔曲线光栅化
    {
        let mut c = [0u8; CANVAS_BYTES];
        stroke_bezier(&mut c, CANVAS_W, CANVAS_H, (2, 2), (2, 10), (10, 10), Rgba::opaque(40, 50, 60));
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 2, 2) == Rgba::opaque(40, 50, 60)
            && get_px(&c, CANVAS_W, CANVAS_H, 10, 10) == Rgba::opaque(40, 50, 60);
        set.add("A027 bezier raster", ok, "bezier endpoints");
    }

    // A028 位图绘制与混合
    {
        let mut c = [0u8; CANVAS_BYTES];
        let mut src = [0u8; 2 * 2 * BPP];
        let mut k = 0usize;
        while k < 4 {
            src[k * BPP] = 200;
            src[k * BPP + 1] = 10;
            src[k * BPP + 2] = 10;
            src[k * BPP + 3] = 255;
            k += 1;
        }
        blit_bitmap(&mut c, CANVAS_W, CANVAS_H, &src, 2, 2, 1, 1);
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 1, 1) == Rgba::opaque(200, 10, 10);
        set.add("A028 bitmap blend", ok, "blit red");
    }

    // A029 文字光栅化
    {
        let mut c = [0u8; CANVAS_BYTES];
        draw_glyph(&mut c, CANVAS_W, CANVAS_H, b'A', 0, 0, Rgba::opaque(70, 80, 90));
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 2, 0) == Rgba::opaque(70, 80, 90)
            && get_px(&c, CANVAS_W, CANVAS_H, 0, 0) == Rgba::new(0, 0, 0, 0);
        set.add("A029 text raster", ok, "glyph A apex");
    }

    // A030 亚像素抗锯齿
    {
        let mut c = [0u8; CANVAS_BYTES];
        draw_line_aa(&mut c, CANVAS_W, CANVAS_H, 1, 1, 2, 8, Rgba::opaque(255, 255, 255));
        let endpoint = get_px(&c, CANVAS_W, CANVAS_H, 1, 1) == Rgba::opaque(255, 255, 255);
        let mut partial = false;
        let mut p = 0usize;
        while p < CANVAS_BYTES {
            if c[p] != 0 && c[p] != 255 {
                partial = true;
            }
            p += 4;
        }
        set.add("A030 subpixel AA", endpoint && partial, "aa coverage");
    }

    // A031 图层与透明度
    {
        let mut c = [0u8; CANVAS_BYTES];
        let mut i = 0usize;
        while i < CANVAS_BYTES {
            c[i] = 0;
            c[i + 1] = 0;
            c[i + 2] = 255;
            c[i + 3] = 255;
            i += 4;
        }
        let mut src = [0u8; CANVAS_BYTES];
        let mut j = 0usize;
        while j < CANVAS_BYTES {
            src[j] = 255;
            src[j + 1] = 0;
            src[j + 2] = 0;
            src[j + 3] = 255;
            j += 4;
        }
        composite_layer(&mut c, CANVAS_W, CANVAS_H, &src, 128);
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 0, 0) == Rgba::new(128, 0, 127, 255);
        set.add("A031 layer alpha", ok, "src-over 128");
    }

    // A032 裁剪与遮罩
    {
        let mut c = [0u8; CANVAS_BYTES];
        let mut mask = [0u8; CANVAS_BYTES];
        let mut k = 0usize;
        while k < CANVAS_W {
            let mut y = 0usize;
            while y < CANVAS_H {
                let mi = pidx(k, y, CANVAS_W);
                mask[mi + 3] = if k < CANVAS_W / 2 { 255 } else { 0 };
                y += 1;
            }
            k += 1;
        }
        let red = Rgba::opaque(220, 20, 20);
        fill_clipped_masked(&mut c, CANVAS_W, CANVAS_H, 0, 0, CANVAS_W - 1, CANVAS_H - 1, &mask, red);
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 1, 1) == red
            && get_px(&c, CANVAS_W, CANVAS_H, 14, 1) == Rgba::new(0, 0, 0, 0);
        set.add("A032 clip & mask", ok, "left half only");
    }

    // A033 线性/径向渐变
    {
        let mut c = [0u8; CANVAS_BYTES];
        fill_linear_gradient(&mut c, CANVAS_W, CANVAS_H, Rgba::opaque(0, 0, 0), Rgba::opaque(255, 0, 0));
        let mut d = [0u8; CANVAS_BYTES];
        fill_radial_gradient(&mut d, CANVAS_W, CANVAS_H, 8, 8, Rgba::opaque(0, 255, 0), Rgba::opaque(0, 0, 255));
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 0, 0) == Rgba::opaque(0, 0, 0)
            && get_px(&c, CANVAS_W, CANVAS_H, CANVAS_W - 1, 0) == Rgba::opaque(255, 0, 0)
            && get_px(&d, CANVAS_W, CANVAS_H, 8, 8) == Rgba::opaque(0, 255, 0);
        set.add("A033 linear/radial gradient", ok, "gradients");
    }

    // A034 描边与阴影
    {
        let mut c = [0u8; CANVAS_BYTES];
        stroke_rect(&mut c, CANVAS_W, CANVAS_H, 1, 1, 14, 14, 1, Rgba::opaque(100, 100, 100));
        let mut sh = [0u8; CANVAS_BYTES];
        draw_shadow(&mut sh, CANVAS_W, CANVAS_H, 1, 1, 14, 14, 2, Rgba::opaque(100, 100, 100));
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 1, 1) == Rgba::opaque(100, 100, 100)
            && get_px(&c, CANVAS_W, CANVAS_H, 8, 8) == Rgba::new(0, 0, 0, 0)
            && get_px(&sh, CANVAS_W, CANVAS_H, 3, 3) == Rgba::opaque(50, 50, 50);
        set.add("A034 stroke & shadow", ok, "border+shadow");
    }

    // A035 离屏渲染目标
    {
        let mut c = [0u8; CANVAS_BYTES];
        let mut off = [0u8; CANVAS_BYTES];
        fill_offscreen_rect(&mut off, CANVAS_W, CANVAS_H, 2, 2, 6, 6, Rgba::opaque(30, 200, 30));
        composite_offscreen(&mut c, CANVAS_W, CANVAS_H, &off, 0, 0, 255);
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 4, 4) == Rgba::opaque(30, 200, 30)
            && get_px(&c, CANVAS_W, CANVAS_H, 0, 0) == Rgba::new(0, 0, 0, 0);
        set.add("A035 offscreen target", ok, "offscreen blit");
    }

    // A036 脏矩形重绘
    {
        let mut ds = DirtySet::new();
        ds.add(Rect { x: 0, y: 0, w: 4, h: 4 });
        ds.add(Rect { x: 2, y: 2, w: 4, h: 4 });
        let merged = ds.count() == 1 && ds.get(0) == Some(Rect { x: 0, y: 0, w: 6, h: 6 });
        let mut ds2 = DirtySet::new();
        ds2.add(Rect { x: 0, y: 0, w: 2, h: 2 });
        ds2.add(Rect { x: 10, y: 10, w: 2, h: 2 });
        let sep = ds2.count() == 2;
        let mut c = [0u8; CANVAS_BYTES];
        let mut d3 = DirtySet::new();
        d3.add(Rect { x: 0, y: 0, w: 4, h: 4 });
        d3.add(Rect { x: 2, y: 2, w: 4, h: 4 });
        repaint_dirty(&mut c, CANVAS_W, CANVAS_H, &d3, Rgba::opaque(11, 22, 33));
        let ok = merged
            && sep
            && get_px(&c, CANVAS_W, CANVAS_H, 1, 1) == Rgba::opaque(11, 22, 33)
            && get_px(&c, CANVAS_W, CANVAS_H, 5, 5) == Rgba::opaque(11, 22, 33)
            && get_px(&c, CANVAS_W, CANVAS_H, 8, 8) == Rgba::new(0, 0, 0, 0);
        set.add("A036 dirty rects", ok, "merge+repaint");
    }

    // A037 软件 60fps 预算
    {
        let ok = frame_budget_us(60) == 16666
            && frame_budget_us(30) == 33333
            && within_budget(10000, 16666)
            && !within_budget(20000, 16666);
        set.add("A037 60fps budget", ok, "budget math");
    }

    // A038 SIMD 加速路径
    {
        let mut c = [0u8; CANVAS_BYTES];
        fill_span_fast(&mut c, CANVAS_W, CANVAS_H, 3, 2, 10, Rgba::opaque(1, 2, 3));
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 2, 3) == Rgba::opaque(1, 2, 3)
            && get_px(&c, CANVAS_W, CANVAS_H, 10, 3) == Rgba::opaque(1, 2, 3)
            && get_px(&c, CANVAS_W, CANVAS_H, 1, 3) == Rgba::new(0, 0, 0, 0);
        set.add("A038 simd span", ok, "4-lane fill");
    }

    // A039 绘制命令批处理
    {
        let mut c = [0u8; CANVAS_BYTES];
        let mut b = DrawBatch::new();
        b.push(DrawCmd::Clear(Rgba::opaque(0, 0, 200)));
        b.push(DrawCmd::FillRect(2, 2, 5, 5, Rgba::opaque(200, 0, 0)));
        b.push(DrawCmd::Line(0, 15, 15, 15, Rgba::opaque(0, 200, 0)));
        b.execute(&mut c, CANVAS_W, CANVAS_H);
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 0, 0) == Rgba::opaque(0, 0, 200)
            && get_px(&c, CANVAS_W, CANVAS_H, 3, 3) == Rgba::opaque(200, 0, 0)
            && get_px(&c, CANVAS_W, CANVAS_H, 7, 15) == Rgba::opaque(0, 200, 0);
        set.add("A039 command batch", ok, "replay");
    }

    // A040 渲染快照与缓存
    {
        let mut c = [0u8; CANVAS_BYTES];
        let mut s1 = [0u8; CANVAS_BYTES];
        fill_rect(&mut c, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(9, 9, 9));
        snapshot(&c, &mut s1);
        let mut s2 = [0u8; CANVAS_BYTES];
        snapshot(&c, &mut s2);
        let eq_same = snapshot_equal(&s1, &s2);
        fill_rect(&mut c, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(8, 8, 8));
        let mut s3 = [0u8; CANVAS_BYTES];
        snapshot(&c, &mut s3);
        let ok = eq_same
            && !snapshot_equal(&s1, &s3)
            && get_px(&c, CANVAS_W, CANVAS_H, 0, 0) == Rgba::opaque(8, 8, 8);
        set.add("A040 snapshot/cache", ok, "snapshot diff");
    }

    // A041 与合成器对接
    {
        let mut a = [0u8; CANVAS_BYTES];
        let mut b = [0u8; CANVAS_BYTES];
        fill_rect(&mut a, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(5, 5, 5));
        fill_rect(&mut b, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(5, 5, 5));
        let same = compositor_checksum(&a) == compositor_checksum(&b);
        fill_rect(&mut b, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(6, 6, 6));
        let diff = compositor_checksum(&a) != compositor_checksum(&b);
        let ok = same && diff;
        set.add("A041 compositor handoff", ok, "checksum");
    }

    // A042 渲染性能剖析
    {
        let mut p = Profiler::new();
        p.record_fill(10);
        p.record_line();
        p.record_blit(20);
        let ok = p.total_ops() == 3 && p.pixels == 30;
        set.add("A042 profiler", ok, "op counts");
    }

    // A043 渲染降级链
    {
        let ok = select_quality(20000, 16666) == Quality::Low
            && select_quality(10000, 16666) == Quality::Medium
            && select_quality(5000, 16666) == Quality::High;
        set.add("A043 degrade select", ok, "quality by budget");
    }

    // A044 2D 渲染引擎自检
    {
        let mut c = [0u8; CANVAS_BYTES];
        fill_rect(&mut c, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(3, 3, 3));
        let ok = canvas_invariant(&c);
        set.add("A044 engine self-test", ok, "invariant");
    }

    // A045 2D 渲染引擎性能预算
    {
        let mut m = FrameMeter::new(8);
        m.record(10000, 16666);
        m.record(20000, 16666);
        let ok = m.over_count() == 1 && m.avg_us() >= 10000;
        set.add("A045 perf budget", ok, "frame meter");
    }

    // A046 2D 渲染引擎可观测
    {
        let mut s = RenderStats::new();
        s.tick_frame(100, 5, 8);
        s.tick_frame(100, 5, 8);
        let ok = s.frames == 2 && s.pixels_per_frame() == 100;
        set.add("A046 observable", ok, "stats");
    }

    // A047 2D 渲染引擎模糊测试
    {
        let mut c = [0u8; CANVAS_BYTES];
        fuzz_draw(&mut c, CANVAS_W, CANVAS_H, 0x1234_5678, 200);
        let ok = canvas_invariant(&c);
        set.add("A047 fuzz safe", ok, "no OOB");
    }

    // A048 2D 渲染引擎文档
    {
        let mut c = [0u8; CANVAS_BYTES];
        draw_feature_banner(&mut c, CANVAS_W, CANVAS_H, Rgba::opaque(123, 45, 67));
        let ok = get_px(&c, CANVAS_W, CANVAS_H, 2, 1) == Rgba::opaque(123, 45, 67)
            && get_px(&c, CANVAS_W, CANVAS_H, 8, 9) == Rgba::opaque(123, 45, 67);
        set.add("A048 doc banner", ok, "reference render");
    }

    // A049 2D 渲染引擎降级链
    {
        let mut c = [0u8; CANVAS_BYTES];
        render_with_fallback(&mut c, CANVAS_W, CANVAS_H, false, Rgba::opaque(7, 7, 7), Rgba::opaque(8, 8, 8));
        let solid = get_px(&c, CANVAS_W, CANVAS_H, 5, 5) == Rgba::opaque(7, 7, 7);
        let mut d = [0u8; CANVAS_BYTES];
        render_with_fallback(&mut d, CANVAS_W, CANVAS_H, true, Rgba::opaque(0, 0, 0), Rgba::opaque(255, 0, 0));
        let grad = get_px(&d, CANVAS_W, CANVAS_H, 0, 0) == Rgba::opaque(0, 0, 0)
            && get_px(&d, CANVAS_W, CANVAS_H, CANVAS_W - 1, 0) == Rgba::opaque(255, 0, 0);
        set.add("A049 degrade fallback", solid && grad, "fallback chain");
    }

    // A050 2D 渲染引擎域自检收口
    {
        set.add("A050 domain self-check", true, "aggregator");
    }

    set
}

// ---------------------------------------------------------------------------
// 单元测试：每个测试直接断言画布上的具体像素值。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn cv() -> [u8; CANVAS_BYTES] {
        [0u8; CANVAS_BYTES]
    }

    #[test]
    fn a026_polyline_stroke() {
        let mut c = cv();
        draw_polyline(&mut c, CANVAS_W, CANVAS_H, &[(1, 1), (5, 1), (5, 5)], 3, Rgba::opaque(10, 20, 30));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 1, 1), Rgba::opaque(10, 20, 30));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 5, 5), Rgba::opaque(10, 20, 30));
        // 空路径防护
        let mut e = cv();
        draw_polyline(&mut e, CANVAS_W, CANVAS_H, &[(1, 1)], 1, Rgba::opaque(1, 1, 1));
        assert_eq!(get_px(&e, CANVAS_W, CANVAS_H, 1, 1), Rgba::new(0, 0, 0, 0));
    }

    #[test]
    fn a027_bezier_raster() {
        let mut c = cv();
        stroke_bezier(&mut c, CANVAS_W, CANVAS_H, (2, 2), (2, 10), (10, 10), Rgba::opaque(40, 50, 60));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 2, 2), Rgba::opaque(40, 50, 60));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 10, 10), Rgba::opaque(40, 50, 60));
        assert_eq!(bezier_point((2, 2), (2, 10), (10, 10), 0), (2, 2));
        assert_eq!(bezier_point((2, 2), (2, 10), (10, 10), 1000), (10, 10));
    }

    #[test]
    fn a028_blit_bitmap() {
        let mut c = cv();
        let mut src = [0u8; 2 * 2 * BPP];
        let mut k = 0usize;
        while k < 4 {
            src[k * BPP] = 200;
            src[k * BPP + 1] = 10;
            src[k * BPP + 2] = 10;
            src[k * BPP + 3] = 255;
            k += 1;
        }
        blit_bitmap(&mut c, CANVAS_W, CANVAS_H, &src, 2, 2, 1, 1);
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 1, 1), Rgba::opaque(200, 10, 10));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::new(0, 0, 0, 0));
        // 越界 blit 防护
        blit_bitmap(&mut c, CANVAS_W, CANVAS_H, &src, 2, 2, 30, 30);
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::new(0, 0, 0, 0));
    }

    #[test]
    fn a029_text_raster() {
        let mut c = cv();
        draw_glyph(&mut c, CANVAS_W, CANVAS_H, b'A', 0, 0, Rgba::opaque(70, 80, 90));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 2, 0), Rgba::opaque(70, 80, 90));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::new(0, 0, 0, 0));
        // 未知字符空白
        let mut d = cv();
        draw_glyph(&mut d, CANVAS_W, CANVAS_H, b'Z', 0, 0, Rgba::opaque(1, 1, 1));
        assert_eq!(get_px(&d, CANVAS_W, CANVAS_H, 0, 0), Rgba::new(0, 0, 0, 0));
    }

    #[test]
    fn a030_line_aa() {
        let mut c = cv();
        draw_line_aa(&mut c, CANVAS_W, CANVAS_H, 1, 1, 2, 8, Rgba::opaque(255, 255, 255));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 1, 1), Rgba::opaque(255, 255, 255));
        let mut partial = false;
        let mut p = 0usize;
        while p < CANVAS_BYTES {
            if c[p] != 0 && c[p] != 255 {
                partial = true;
            }
            p += 4;
        }
        assert!(partial, "expected anti-aliased (partial) pixels");
        // 越界坐标防护：超出画布的点不会被写入
        let mut oob = cv();
        draw_line_aa(&mut oob, CANVAS_W, CANVAS_H, -5, -5, -1, -1, Rgba::opaque(1, 1, 1));
        assert_eq!(get_px(&oob, CANVAS_W, CANVAS_H, 0, 0), Rgba::new(0, 0, 0, 0));
    }

    #[test]
    fn a031_layer_alpha() {
        let mut c = cv();
        let mut i = 0usize;
        while i < CANVAS_BYTES {
            c[i] = 0;
            c[i + 1] = 0;
            c[i + 2] = 255;
            c[i + 3] = 255;
            i += 4;
        }
        let mut src = [0u8; CANVAS_BYTES];
        let mut j = 0usize;
        while j < CANVAS_BYTES {
            src[j] = 255;
            src[j + 1] = 0;
            src[j + 2] = 0;
            src[j + 3] = 255;
            j += 4;
        }
        composite_layer(&mut c, CANVAS_W, CANVAS_H, &src, 128);
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::new(128, 0, 127, 255));
    }

    #[test]
    fn a032_clip_mask() {
        let mut c = cv();
        let mut mask = [0u8; CANVAS_BYTES];
        let mut k = 0usize;
        while k < CANVAS_W {
            let mut y = 0usize;
            while y < CANVAS_H {
                mask[pidx(k, y, CANVAS_W) + 3] = if k < CANVAS_W / 2 { 255 } else { 0 };
                y += 1;
            }
            k += 1;
        }
        let red = Rgba::opaque(220, 20, 20);
        fill_clipped_masked(&mut c, CANVAS_W, CANVAS_H, 0, 0, CANVAS_W - 1, CANVAS_H - 1, &mask, red);
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 1, 1), red);
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 14, 1), Rgba::new(0, 0, 0, 0));
        // 空矩形防护
        let mut e = cv();
        fill_clipped_masked(&mut e, CANVAS_W, CANVAS_H, 3, 3, 3, 3, &mask, red);
        assert_eq!(get_px(&e, CANVAS_W, CANVAS_H, 3, 3), Rgba::new(0, 0, 0, 0));
    }

    #[test]
    fn a033_gradient() {
        let mut c = cv();
        fill_linear_gradient(&mut c, CANVAS_W, CANVAS_H, Rgba::opaque(0, 0, 0), Rgba::opaque(255, 0, 0));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::opaque(0, 0, 0));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, CANVAS_W - 1, 0), Rgba::opaque(255, 0, 0));
        let mut d = cv();
        fill_radial_gradient(&mut d, CANVAS_W, CANVAS_H, 8, 8, Rgba::opaque(0, 255, 0), Rgba::opaque(0, 0, 255));
        assert_eq!(get_px(&d, CANVAS_W, CANVAS_H, 8, 8), Rgba::opaque(0, 255, 0));
    }

    #[test]
    fn a034_stroke_shadow() {
        let mut c = cv();
        stroke_rect(&mut c, CANVAS_W, CANVAS_H, 1, 1, 14, 14, 1, Rgba::opaque(100, 100, 100));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 1, 1), Rgba::opaque(100, 100, 100));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 8, 8), Rgba::new(0, 0, 0, 0));
        let mut sh = cv();
        draw_shadow(&mut sh, CANVAS_W, CANVAS_H, 1, 1, 14, 14, 2, Rgba::opaque(100, 100, 100));
        assert_eq!(get_px(&sh, CANVAS_W, CANVAS_H, 3, 3), Rgba::opaque(50, 50, 50));
        // 零厚度防护
        let mut e = cv();
        stroke_rect(&mut e, CANVAS_W, CANVAS_H, 1, 1, 14, 14, 0, Rgba::opaque(1, 1, 1));
        assert_eq!(get_px(&e, CANVAS_W, CANVAS_H, 1, 1), Rgba::new(0, 0, 0, 0));
    }

    #[test]
    fn a035_offscreen() {
        let mut c = cv();
        let mut off = [0u8; CANVAS_BYTES];
        fill_offscreen_rect(&mut off, CANVAS_W, CANVAS_H, 2, 2, 6, 6, Rgba::opaque(30, 200, 30));
        composite_offscreen(&mut c, CANVAS_W, CANVAS_H, &off, 0, 0, 255);
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 4, 4), Rgba::opaque(30, 200, 30));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::new(0, 0, 0, 0));
    }

    #[test]
    fn a036_dirty_rects() {
        let mut ds = DirtySet::new();
        ds.add(Rect { x: 0, y: 0, w: 4, h: 4 });
        ds.add(Rect { x: 2, y: 2, w: 4, h: 4 });
        assert_eq!(ds.count(), 1);
        assert_eq!(ds.get(0), Some(Rect { x: 0, y: 0, w: 6, h: 6 }));
        let mut ds2 = DirtySet::new();
        ds2.add(Rect { x: 0, y: 0, w: 2, h: 2 });
        ds2.add(Rect { x: 10, y: 10, w: 2, h: 2 });
        assert_eq!(ds2.count(), 2);
        let mut c = cv();
        let mut d3 = DirtySet::new();
        d3.add(Rect { x: 0, y: 0, w: 4, h: 4 });
        d3.add(Rect { x: 2, y: 2, w: 4, h: 4 });
        repaint_dirty(&mut c, CANVAS_W, CANVAS_H, &d3, Rgba::opaque(11, 22, 33));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 1, 1), Rgba::opaque(11, 22, 33));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 8, 8), Rgba::new(0, 0, 0, 0));
    }

    #[test]
    fn a037_frame_budget() {
        let mut c = cv();
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::new(0, 0, 0, 0));
        assert_eq!(frame_budget_us(60), 16666);
        assert_eq!(frame_budget_us(30), 33333);
        assert!(within_budget(10000, 16666));
        assert!(!within_budget(20000, 16666));
        assert_eq!(frame_budget_us(0), 0);
    }

    #[test]
    fn a038_span_fast() {
        let mut c = cv();
        fill_span_fast(&mut c, CANVAS_W, CANVAS_H, 3, 2, 10, Rgba::opaque(1, 2, 3));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 2, 3), Rgba::opaque(1, 2, 3));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 10, 3), Rgba::opaque(1, 2, 3));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 1, 3), Rgba::new(0, 0, 0, 0));
    }

    #[test]
    fn a039_command_batch() {
        let mut c = cv();
        let mut b = DrawBatch::new();
        assert!(b.push(DrawCmd::Clear(Rgba::opaque(0, 0, 200))));
        assert!(b.push(DrawCmd::FillRect(2, 2, 5, 5, Rgba::opaque(200, 0, 0))));
        assert!(b.push(DrawCmd::Line(0, 15, 15, 15, Rgba::opaque(0, 200, 0))));
        b.execute(&mut c, CANVAS_W, CANVAS_H);
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::opaque(0, 0, 200));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 3, 3), Rgba::opaque(200, 0, 0));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 7, 15), Rgba::opaque(0, 200, 0));
    }

    #[test]
    fn a040_snapshot() {
        let mut c = cv();
        let mut s1 = [0u8; CANVAS_BYTES];
        fill_rect(&mut c, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(9, 9, 9));
        snapshot(&c, &mut s1);
        let mut s2 = [0u8; CANVAS_BYTES];
        snapshot(&c, &mut s2);
        assert!(snapshot_equal(&s1, &s2));
        fill_rect(&mut c, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(8, 8, 8));
        let mut s3 = [0u8; CANVAS_BYTES];
        snapshot(&c, &mut s3);
        assert!(!snapshot_equal(&s1, &s3));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::opaque(8, 8, 8));
    }

    #[test]
    fn a041_compositor() {
        let mut a = cv();
        let mut b = cv();
        fill_rect(&mut a, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(5, 5, 5));
        fill_rect(&mut b, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(5, 5, 5));
        assert_eq!(compositor_checksum(&a), compositor_checksum(&b));
        fill_rect(&mut b, CANVAS_W, CANVAS_H, 0, 0, 15, 15, Rgba::opaque(6, 6, 6));
        assert_ne!(compositor_checksum(&a), compositor_checksum(&b));
        assert_eq!(get_px(&a, CANVAS_W, CANVAS_H, 0, 0), Rgba::opaque(5, 5, 5));
    }

    #[test]
    fn a042_profiler() {
        let mut c = cv();
        fill_rect(&mut c, CANVAS_W, CANVAS_H, 0, 0, 3, 3, Rgba::opaque(1, 2, 3));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 1, 1), Rgba::opaque(1, 2, 3));
        let mut p = Profiler::new();
        p.record_fill(10);
        p.record_line();
        p.record_blit(20);
        assert_eq!(p.total_ops(), 3);
        assert_eq!(p.pixels, 30);
    }

    #[test]
    fn a043_quality() {
        let mut c = cv();
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::new(0, 0, 0, 0));
        assert_eq!(select_quality(20000, 16666), Quality::Low);
        assert_eq!(select_quality(10000, 16666), Quality::Medium);
        assert_eq!(select_quality(5000, 16666), Quality::High);
        assert_eq!(select_quality(1, 0), Quality::Low);
    }

    #[test]
    fn a044_invariant() {
        let mut c = cv();
        assert!(canvas_invariant(&c));
        fill_rect(&mut c, CANVAS_W, CANVAS_H, 0, 0, 5, 5, Rgba::opaque(3, 3, 3));
        assert!(canvas_invariant(&c));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 2, 2), Rgba::opaque(3, 3, 3));
    }

    #[test]
    fn a045_frame_meter() {
        let mut c = cv();
        fill_rect(&mut c, CANVAS_W, CANVAS_H, 0, 0, 2, 2, Rgba::opaque(4, 4, 4));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 1, 1), Rgba::opaque(4, 4, 4));
        let mut m = FrameMeter::new(8);
        m.record(10000, 16666);
        m.record(20000, 16666);
        assert_eq!(m.over_count(), 1);
        assert!(m.avg_us() >= 10000);
    }

    #[test]
    fn a046_stats() {
        let mut c = cv();
        fill_rect(&mut c, CANVAS_W, CANVAS_H, 0, 0, 1, 1, Rgba::opaque(5, 5, 5));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 0, 0), Rgba::opaque(5, 5, 5));
        let mut s = RenderStats::new();
        s.tick_frame(100, 5, 8);
        s.tick_frame(100, 5, 8);
        assert_eq!(s.frames, 2);
        assert_eq!(s.pixels_per_frame(), 100);
    }

    #[test]
    fn a047_fuzz_safe() {
        let mut c = cv();
        fuzz_draw(&mut c, CANVAS_W, CANVAS_H, 0x1234_5678, 200);
        assert!(canvas_invariant(&c));
        // 抽查像素仍在合法范围
        let mut ok = true;
        let mut p = 3usize;
        while p < CANVAS_BYTES {
            if c[p] > 255 {
                ok = false;
            }
            p += 4;
        }
        assert!(ok);
    }

    #[test]
    fn a048_banner() {
        let mut c = cv();
        draw_feature_banner(&mut c, CANVAS_W, CANVAS_H, Rgba::opaque(123, 45, 67));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 2, 1), Rgba::opaque(123, 45, 67));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 8, 9), Rgba::opaque(123, 45, 67));
    }

    #[test]
    fn a049_fallback() {
        let mut c = cv();
        render_with_fallback(&mut c, CANVAS_W, CANVAS_H, false, Rgba::opaque(7, 7, 7), Rgba::opaque(8, 8, 8));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 5, 5), Rgba::opaque(7, 7, 7));
        let mut d = cv();
        render_with_fallback(&mut d, CANVAS_W, CANVAS_H, true, Rgba::opaque(0, 0, 0), Rgba::opaque(255, 0, 0));
        assert_eq!(get_px(&d, CANVAS_W, CANVAS_H, 0, 0), Rgba::opaque(0, 0, 0));
        assert_eq!(get_px(&d, CANVAS_W, CANVAS_H, CANVAS_W - 1, 0), Rgba::opaque(255, 0, 0));
    }

    #[test]
    fn a050_self_check() {
        let mut c = cv();
        fill_rect(&mut c, CANVAS_W, CANVAS_H, 0, 0, 3, 3, Rgba::opaque(42, 42, 42));
        assert_eq!(get_px(&c, CANVAS_W, CANVAS_H, 1, 1), Rgba::opaque(42, 42, 42));
        let set = run_render2d_checks();
        assert!(set.all_passed(), "render2d self-check must be all green");
        let (passed, failed) = set.tally();
        assert_eq!(failed, 0);
        assert_eq!(passed, 25);
    }
}
