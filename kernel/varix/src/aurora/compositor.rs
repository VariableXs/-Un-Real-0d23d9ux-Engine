//! AURORA-1000 AI-05 · 合成器（A101~A125，W1）
//!
//! 全场景合成器：图层注册表（固定 `[Layer; 16]`，z 序、可见性、脏矩形）、
//! 按 z 序 src-over 逐层合成到目标画布、混合模式（normal/Add/Multiply 纯函数
//! 逐像素）、3x3 盒模糊（分离两趟）、矩形阴影羽化（距离衰减）、透明度渐变
//! （alpha ramp）、动画插值合成（layer offset 按时间 0..255 插值后合成）、
//! vsync 帧对齐（帧号取模、掉帧判定）、损伤追踪（脏区并集）、合成一次完成
//! 标志（整帧提交语义）。
//!
//! 纯 `no_std` 实现：只依赖 `core`，固定容量数组 + `usize` 计数；画布/图层以
//! `&[u8]`/`&mut [u8]` 传入。无 `unsafe`、无宏、无泛型魔法。

use crate::checks::{push_str, push_usize, CheckSet};

// ---------------------------------------------------------------------------
// 容量与像素格式（RGBA8，4 字节/像素，行主序）
// ---------------------------------------------------------------------------

pub const MAX_LAYERS: usize = 16;
pub const LAYER_MAX_W: usize = 16;
pub const LAYER_MAX_H: usize = 16;
pub const LAYER_PX: usize = LAYER_MAX_W * LAYER_MAX_H; // 256
pub const LAYER_BYTES: usize = LAYER_PX * 4; // 1024

pub const MAX_CANVAS_W: usize = 64;
pub const MAX_CANVAS_H: usize = 64;

pub const MAX_TEXTURES: usize = 8;
pub const TEX_W: usize = 16;
pub const TEX_H: usize = 16;
pub const TEX_PX: usize = TEX_W * TEX_H;
pub const TEX_BYTES: usize = TEX_PX * 4; // 1024

pub const OFF_W: usize = 32;
pub const OFF_H: usize = 32;
pub const OFF_BYTES: usize = OFF_W * OFF_H * 4;

/// 像素索引（行主序，不做越界检查——调用方负责边界）。
pub fn px_index(w: usize, x: usize, y: usize) -> usize {
    (y * w + x) * 4
}

// ---------------------------------------------------------------------------
// 基础类型
// ---------------------------------------------------------------------------

/// 混合模式枚举（A103 混合模式库）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    Normal,
    Add,
    Multiply,
}

impl BlendMode {
    pub fn name(self) -> &'static str {
        match self {
            BlendMode::Normal => "normal",
            BlendMode::Add => "add",
            BlendMode::Multiply => "multiply",
        }
    }
}

/// 矩形（用于裁剪 / 脏区 / 阴影 / 多屏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// 图层：几何 + 属性 + 固定像素缓冲（A101 图层树管理）。
#[derive(Clone, Copy)]
pub struct Layer {
    pub id: u16,
    pub visible: bool,
    pub z: u8,
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
    pub alpha: u8,
    pub blend: BlendMode,
    /// 动画偏移：在 t=255 时施加（t=0 时不偏移）。A107/A115。
    pub anim_dx: i16,
    pub anim_dy: i16,
    /// 固定 RGBA 像素缓冲（仅前 w*h*4 字节有效）。
    pub data: [u8; LAYER_BYTES],
}

impl Layer {
    pub const fn zero() -> Layer {
        Layer {
            id: 0,
            visible: false,
            z: 0,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            alpha: 0,
            blend: BlendMode::Normal,
            anim_dx: 0,
            anim_dy: 0,
            data: [0u8; LAYER_BYTES],
        }
    }
}

/// 图层注册表：固定容量 `[Layer; 16]`，z 序、可见性、脏矩形。
#[derive(Clone, Copy)]
pub struct LayerRegistry {
    layers: [Layer; MAX_LAYERS],
    count: usize,
    dirty_union: Option<Rect>,
}

impl LayerRegistry {
    pub const fn new() -> LayerRegistry {
        LayerRegistry {
            layers: [Layer::zero(); MAX_LAYERS],
            count: 0,
            dirty_union: None,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, index: usize) -> Option<Layer> {
        if index < self.count {
            Some(self.layers[index])
        } else {
            None
        }
    }

    pub fn dirty_union(&self) -> Option<Rect> {
        self.dirty_union
    }
}

// ---------------------------------------------------------------------------
// A101 图层树管理
// ---------------------------------------------------------------------------

/// 构造一个填满纯色的图层（便于测试与自检）。
pub fn make_layer(
    id: u16,
    x: i16,
    y: i16,
    w: u16,
    h: u16,
    color: [u8; 4],
    alpha: u8,
    blend: BlendMode,
) -> Layer {
    let mut l = Layer::zero();
    l.id = id;
    l.visible = true;
    l.x = x;
    l.y = y;
    l.w = w;
    l.h = h;
    l.alpha = alpha;
    l.blend = blend;
    let pw = (w as usize).min(LAYER_MAX_W);
    let ph = (h as usize).min(LAYER_MAX_H);
    for yy in 0..ph {
        for xx in 0..pw {
            let i = (yy * pw + xx) * 4;
            l.data[i] = color[0];
            l.data[i + 1] = color[1];
            l.data[i + 2] = color[2];
            l.data[i + 3] = color[3];
        }
    }
    l
}

/// 注册一个图层；满则失败（A101）。
pub fn layer_tree_add(reg: &mut LayerRegistry, layer: Layer) -> bool {
    if reg.count >= MAX_LAYERS {
        return false;
    }
    reg.layers[reg.count] = layer;
    reg.count += 1;
    true
}

/// 按 z 序升序稳定排序（选择排序，无分配）。
pub fn layer_tree_sort(reg: &mut LayerRegistry) {
    for i in 0..reg.count {
        let mut best = i;
        for j in (i + 1)..reg.count {
            if reg.layers[j].z < reg.layers[best].z {
                best = j;
            }
        }
        if best != i {
            let tmp = reg.layers[i];
            reg.layers[i] = reg.layers[best];
            reg.layers[best] = tmp;
        }
    }
}

/// 切换可见性；返回是否找到该图层。
pub fn layer_tree_set_visible(reg: &mut LayerRegistry, id: u16, visible: bool) -> bool {
    for i in 0..reg.count {
        if reg.layers[i].id == id {
            reg.layers[i].visible = visible;
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// A102 窗口纹理缓存
// ---------------------------------------------------------------------------

/// 窗口纹理：固定 RGBA 缓冲。
#[derive(Clone, Copy)]
pub struct Texture {
    pub id: u16,
    pub w: u16,
    pub h: u16,
    pub data: [u8; TEX_BYTES],
}

/// 纹理缓存：固定 `[Option<Texture>; 8]`。
#[derive(Clone, Copy)]
pub struct TextureCache {
    slots: [Option<Texture>; MAX_TEXTURES],
}

impl TextureCache {
    pub const fn new() -> TextureCache {
        TextureCache {
            slots: [None; MAX_TEXTURES],
        }
    }

    pub fn len(&self) -> usize {
        let mut n = 0usize;
        for i in 0..MAX_TEXTURES {
            if self.slots[i].is_some() {
                n += 1;
            }
        }
        n
    }
}

/// 存入纹理（按 id 找空位或覆盖同 id）；越界数据被截断。A102。
pub fn texture_cache_store(
    c: &mut TextureCache,
    id: u16,
    w: u16,
    h: u16,
    data: &[u8],
) -> bool {
    if (w as usize) * (h as usize) * 4 > TEX_BYTES {
        return false;
    }
    // 覆盖同 id
    for i in 0..MAX_TEXTURES {
        if let Some(t) = c.slots[i] {
            if t.id == id {
                let mut tex = t;
                tex.w = w;
                tex.h = h;
                let n = data.len().min(TEX_BYTES);
                tex.data[..n].copy_from_slice(&data[..n]);
                c.slots[i] = Some(tex);
                return true;
            }
        }
    }
    // 否则找空位
    for i in 0..MAX_TEXTURES {
        if c.slots[i].is_none() {
            let mut tex = Texture {
                id,
                w,
                h,
                data: [0u8; TEX_BYTES],
            };
            let n = data.len().min(TEX_BYTES);
            tex.data[..n].copy_from_slice(&data[..n]);
            c.slots[i] = Some(tex);
            return true;
        }
    }
    false
}

/// 取出纹理（按 id）。
pub fn texture_cache_get(c: &TextureCache, id: u16) -> Option<Texture> {
    for i in 0..MAX_TEXTURES {
        if let Some(t) = c.slots[i] {
            if t.id == id {
                return Some(t);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// A103 混合模式库（逐像素纯函数）
// ---------------------------------------------------------------------------

/// 单像素 src-over（straight alpha）。
fn blend_normal(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let n = src[3] as u32;
    let mut o = [0u8; 4];
    for c in 0..3 {
        let v = (src[c] as u32 * n + dst[c] as u32 * (255 - n)) / 255;
        o[c] = v.min(255) as u8;
    }
    let a = n + (dst[3] as u32) * (255 - n) / 255;
    o[3] = a.min(255) as u8;
    o
}

/// 单像素加色混合（按 alpha 加权）。
fn blend_add(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let n = src[3] as u32;
    let mut o = [0u8; 4];
    for c in 0..3 {
        let v = dst[c] as u32 + (src[c] as u32) * n / 255;
        o[c] = v.min(255) as u8;
    }
    let a = (dst[3] as u32 + n).min(255);
    o[3] = a as u8;
    o
}

/// 单像素正片叠底（按 Alpha 加权）。
fn blend_multiply(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let n = src[3] as u32;
    let mut o = [0u8; 4];
    for c in 0..3 {
        let m = dst[c] as u32 * src[c] as u32 / 255;
        let v = m * n + dst[c] as u32 * (255 - n);
        o[c] = (v / 255).min(255) as u8;
    }
    let a = (n + (dst[3] as u32) * (255 - n) / 255).min(255);
    o[3] = a as u8;
    o
}

/// 混合模式库入口（A103）。
pub fn blend_mode_apply(mode: BlendMode, dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    match mode {
        BlendMode::Normal => blend_normal(dst, src),
        BlendMode::Add => blend_add(dst, src),
        BlendMode::Multiply => blend_multiply(dst, src),
    }
}

// ---------------------------------------------------------------------------
// A104 高斯模糊（3x3 近似，分离两趟：水平 1x3 + 垂直 3x1）
// ---------------------------------------------------------------------------

/// 带边界钳制的采样。
fn sample(buf: &[u8], w: usize, h: usize, x: usize, y: usize, c: usize) -> u8 {
    let x = x.min(w - 1);
    let y = y.min(h - 1);
    buf[(y * w + x) * 4 + c]
}

/// 3x3 盒模糊（分离两趟），返回写入字节数，失败返回 0（A104）。
pub fn gauss_blur_3x3(src: &[u8], w: usize, h: usize, tmp: &mut [u8], out: &mut [u8]) -> usize {
    let need = w * h * 4;
    if w == 0 || h == 0 || src.len() < need || tmp.len() < need || out.len() < need {
        return 0;
    }
    // 水平 1x3
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let a = sample(src, w, h, x.wrapping_sub(1), y, c) as u32;
                let b = sample(src, w, h, x, y, c) as u32;
                let d = sample(src, w, h, x + 1, y, c) as u32;
                tmp[(y * w + x) * 4 + c] = (((a + b + d) / 3) as u8);
            }
        }
    }
    // 垂直 3x1
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let a = sample(tmp, w, h, x, y.wrapping_sub(1), c) as u32;
                let b = sample(tmp, w, h, x, y, c) as u32;
                let d = sample(tmp, w, h, x, y + 1, c) as u32;
                out[(y * w + x) * 4 + c] = (((a + b + d) / 3) as u8);
            }
        }
    }
    need
}

// ---------------------------------------------------------------------------
// A105 投影与阴影（矩形阴影羽化，切比雪夫距离衰减）
// ---------------------------------------------------------------------------

/// 在画布上绘制矩形投影：阴影落在矩形之外，按到矩形边界的距离衰减（A105）。
/// 返回受影响像素数（近似）。
pub fn draw_drop_shadow(
    dst: &mut [u8],
    w: usize,
    h: usize,
    rx: usize,
    ry: usize,
    rw: usize,
    rh: usize,
    radius: usize,
    color: [u8; 4],
    intensity: u8,
) -> usize {
    if w == 0 || h == 0 || radius == 0 || dst.len() < w * h * 4 {
        return 0;
    }
    let rx0 = rx as i32;
    let ry0 = ry as i32;
    let rx1 = rx0 + rw as i32;
    let ry1 = ry0 + rh as i32;
    let rad = radius as i32;
    let mut touched = 0usize;
    for y in 0..h {
        for x in 0..w {
            // 阴影片内部不绘制（投影像在物体之下）。
            if (x as i32) >= rx0 && (x as i32) < rx1 && (y as i32) >= ry0 && (y as i32) < ry1 {
                continue;
            }
            let px = x as i32;
            let py = y as i32;
            let dx = if px < rx0 { rx0 - px } else if px >= rx1 { px - rx1 } else { 0 };
            let dy = if py < ry0 { ry0 - py } else if py >= ry1 { py - ry1 } else { 0 };
            let dist = dx.max(dy);
            if dist >= rad {
                continue;
            }
            let sa = (intensity as i32) * (rad - dist) / rad;
            let sa = (sa as u8).min(255);
            let i = (y * w + x) * 4;
            for c in 0..3 {
                let v = dst[i + c] as u32 * (255 - sa as u32)
                    + color[c] as u32 * sa as u32;
                dst[i + c] = (v / 255).min(255) as u8;
            }
            dst[i + 3] = (dst[i + 3] as u32 + sa as u32).min(255) as u8;
            touched += 1;
        }
    }
    touched
}

// ---------------------------------------------------------------------------
// 核心合成（内部）
// ---------------------------------------------------------------------------

/// 按 z 序合成可见图层到画布；`clip` 限制写入区域；`t` 为动画时间 0..255。
fn composite_region(
    canvas: &mut [u8],
    cw: usize,
    ch: usize,
    reg: &LayerRegistry,
    clip: Option<Rect>,
    t: u8,
) -> usize {
    let need = cw * ch * 4;
    if cw == 0 || ch == 0 || canvas.len() < need {
        return 0;
    }
    // z 序索引（升序，仅可见图层）。
    let mut order: [usize; MAX_LAYERS] = [0; MAX_LAYERS];
    let mut n = 0usize;
    for i in 0..reg.count {
        if reg.layers[i].visible {
            order[n] = i;
            n += 1;
        }
    }
    for a in 0..n {
        for b in (a + 1)..n {
            if reg.layers[order[b]].z < reg.layers[order[a]].z {
                let tmp = order[a];
                order[a] = order[b];
                order[b] = tmp;
            }
        }
    }

    let mut written = 0usize;
    for k in 0..n {
        let l = reg.layers[order[k]];
        let lalpha = l.alpha as u32;
        let ox = ((l.anim_dx as i32) * (t as i32) / 255) as i16;
        let oy = ((l.anim_dy as i32) * (t as i32) / 255) as i16;
        let pw = (l.w as usize).min(LAYER_MAX_W);
        let ph = (l.h as usize).min(LAYER_MAX_H);
        for ly in 0..ph {
            for lx in 0..pw {
                let cx = l.x as i32 + ox as i32 + lx as i32;
                let cy = l.y as i32 + oy as i32 + ly as i32;
                if cx < 0 || cy < 0 || cx >= cw as i32 || cy >= ch as i32 {
                    continue;
                }
                let cxu = cx as usize;
                let cyu = cy as usize;
                if let Some(r) = clip {
                    if cxu < r.x || cxu >= r.x + r.w || cyu < r.y || cyu >= r.y + r.h {
                        continue;
                    }
                }
                let idx = (cyu * cw + cxu) * 4;
                let si = (ly * pw + lx) * 4;
                let sa = ((l.data[si + 3] as u32) * lalpha / 255) as u8;
                let src = [l.data[si], l.data[si + 1], l.data[si + 2], sa];
                let dst = [canvas[idx], canvas[idx + 1], canvas[idx + 2], canvas[idx + 3]];
                let out = blend_mode_apply(l.blend, dst, src);
                canvas[idx] = out[0];
                canvas[idx + 1] = out[1];
                canvas[idx + 2] = out[2];
                canvas[idx + 3] = out[3];
                written += 1;
            }
        }
    }
    written
}

// ---------------------------------------------------------------------------
// A106 透明度合成（按 z 序 src-over 逐层合成）
// ---------------------------------------------------------------------------

/// 整画布 src-over 合成（alpha ramp 由 layer.alpha 提供）。A106。
pub fn composite_src_over(canvas: &mut [u8], cw: usize, ch: usize, reg: &LayerRegistry) -> usize {
    composite_region(canvas, cw, ch, reg, None, 0)
}

// ---------------------------------------------------------------------------
// A107 动画属性合成（对 layer offset 按时间 0..255 插值后合成）
// ---------------------------------------------------------------------------

/// 动画插值合成：layer 偏移随 t 从 0 到 (anim_dx, anim_dy) 线性插值。A107。
pub fn composite_animated(
    canvas: &mut [u8],
    cw: usize,
    ch: usize,
    reg: &LayerRegistry,
    t: u8,
) -> usize {
    composite_region(canvas, cw, ch, reg, None, t)
}

// ---------------------------------------------------------------------------
// A108 vsync 对齐（帧号取模、掉帧判定）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VsyncVerdict {
    Present,
    Skip,
    Dropped,
}

/// 帧时钟：按 interval（每 N 帧一次呈现）对齐；未呈现的呈现帧计为掉帧。A108。
#[derive(Clone, Copy, Debug)]
pub struct FrameClock {
    pub frame: u64,
    pub interval: u32,
    pub dropped: u64,
    pub presented: u64,
}

impl FrameClock {
    pub const fn new(interval: u32) -> FrameClock {
        FrameClock {
            frame: 0,
            interval: if interval < 1 { 1 } else { interval },
            dropped: 0,
            presented: 0,
        }
    }
}

/// 推进一帧：rendered=true 表示本帧实际渲染完成。返回呈现判定。A108。
pub fn vsync_tick(clk: &mut FrameClock, rendered: bool) -> VsyncVerdict {
    let f = clk.frame;
    clk.frame += 1;
    let on_present = (f % clk.interval as u64) == 0;
    if on_present {
        if rendered {
            clk.presented += 1;
            VsyncVerdict::Present
        } else {
            clk.dropped += 1;
            VsyncVerdict::Dropped
        }
    } else {
        VsyncVerdict::Skip
    }
}

// ---------------------------------------------------------------------------
// A109 部分重绘（损伤追踪：脏区并集）
// ---------------------------------------------------------------------------

/// 仅合成脏矩形区域（A109）。
pub fn composite_dirty(
    canvas: &mut [u8],
    cw: usize,
    ch: usize,
    reg: &LayerRegistry,
    dirty: &Rect,
) -> usize {
    composite_region(canvas, cw, ch, reg, Some(*dirty), 0)
}

/// 计算两个脏区的并集（A109 损伤追踪）。
pub fn dirty_union(a: Option<Rect>, b: Rect) -> Rect {
    match a {
        None => b,
        Some(x) => {
            let x0 = x.x.min(b.x);
            let y0 = x.y.min(b.y);
            let x1 = (x.x + x.w).max(b.x + b.w);
            let y1 = (x.y + x.h).max(b.y + b.h);
            Rect {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            }
        }
    }
}

/// 把图层标记脏并并入注册表脏区并集（A109）。
pub fn mark_layer_dirty(reg: &mut LayerRegistry, id: u16) -> bool {
    for i in 0..reg.count {
        if reg.layers[i].id == id {
            let r = Rect {
                x: reg.layers[i].x.max(0) as usize,
                y: reg.layers[i].y.max(0) as usize,
                w: reg.layers[i].w as usize,
                h: reg.layers[i].h as usize,
            };
            reg.dirty_union = Some(dirty_union(reg.dirty_union, r));
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// A110 图层裁剪优化（图层与父裁剪区求交）
// ---------------------------------------------------------------------------

/// 图层在画布中的包围盒与 `bounds` 求交（A110）。
pub fn layer_clip(l: &Layer, bounds: &Rect) -> Rect {
    let lx0 = if l.x < 0 { 0 } else { l.x as usize };
    let ly0 = if l.y < 0 { 0 } else { l.y as usize };
    let lx1 = lx0 + l.w as usize;
    let ly1 = ly0 + l.h as usize;
    let x0 = lx0.max(bounds.x);
    let y0 = ly0.max(bounds.y);
    let x1 = lx1.min(bounds.x + bounds.w);
    let y1 = ly1.min(bounds.y + bounds.h);
    if x1 <= x0 || y1 <= y0 {
        Rect { x: 0, y: 0, w: 0, h: 0 }
    } else {
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }
}

/// 裁剪到 `clip` 矩形合成（A110）。
pub fn composite_clipped(
    canvas: &mut [u8],
    cw: usize,
    ch: usize,
    reg: &LayerRegistry,
    clip: &Rect,
) -> usize {
    composite_region(canvas, cw, ch, reg, Some(*clip), 0)
}

// ---------------------------------------------------------------------------
// A111 合成快照
// ---------------------------------------------------------------------------

/// 把当前帧复制到快照缓冲（A111）。返回复制字节数。
pub fn composite_snapshot(src: &[u8], out: &mut [u8]) -> usize {
    let n = src.len().min(out.len());
    out[..n].copy_from_slice(&src[..n]);
    n
}

// ---------------------------------------------------------------------------
// A112 离屏合成缓冲
// ---------------------------------------------------------------------------

/// 离屏合成缓冲：固定 RGBA 缓冲。A112。
#[derive(Clone, Copy)]
pub struct Offscreen {
    pub data: [u8; OFF_BYTES],
    pub w: u16,
    pub h: u16,
}

impl Offscreen {
    pub const fn new(w: u16, h: u16) -> Offscreen {
        Offscreen {
            data: [0u8; OFF_BYTES],
            w: if w > OFF_W as u16 { OFF_W as u16 } else { w },
            h: if h > OFF_H as u16 { OFF_H as u16 } else { h },
        }
    }
}

/// 在离屏缓冲上合成（A112）。返回写入像素数。
pub fn offscreen_compose(o: &mut Offscreen, reg: &LayerRegistry) -> usize {
    composite_region(&mut o.data, o.w as usize, o.h as usize, reg, None, 0)
}

// ---------------------------------------------------------------------------
// A113 合成性能剖析
// ---------------------------------------------------------------------------

/// 合成性能剖析累加器（A113）。
#[derive(Clone, Copy, Debug)]
pub struct ComposeProfile {
    pub pixels: u32,
    pub frames: u32,
    pub last_micros: u32,
    pub max_micros: u32,
}

impl ComposeProfile {
    pub const fn new() -> ComposeProfile {
        ComposeProfile {
            pixels: 0,
            frames: 0,
            last_micros: 0,
            max_micros: 0,
        }
    }
}

/// 记录一次合成（像素数 + 耗时微秒）（A113）。
pub fn profile_record(p: &mut ComposeProfile, pixels: usize, micros: u32) {
    p.pixels = p.pixels.saturating_add(pixels as u32);
    p.frames = p.frames.saturating_add(1);
    p.last_micros = micros;
    if micros > p.max_micros {
        p.max_micros = micros;
    }
}

// ---------------------------------------------------------------------------
// A114 合成降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeLevel {
    Full,
    Reduced,
    Minimal,
}

/// 根据平均耗时与预算选择降级档（A114）。
pub fn degrade_for_budget(avg_micros: u32, budget_micros: u32) -> DegradeLevel {
    let budget = budget_micros.max(1);
    if avg_micros * 2 <= budget {
        DegradeLevel::Full
    } else if avg_micros <= budget {
        DegradeLevel::Reduced
    } else {
        DegradeLevel::Minimal
    }
}

// ---------------------------------------------------------------------------
// A115 合成与动效协作（动画偏移插值）
// ---------------------------------------------------------------------------

/// 线性插值：t=0 → from，t=255 → to（A115）。
pub fn motion_evaluate(t: u8, from: i16, to: i16) -> i16 {
    let delta = to as i32 - from as i32;
    (from as i32 + delta * t as i32 / 255) as i16
}

/// 设置图层动画目标偏移（A115）。
pub fn motion_set_offset(l: &mut Layer, dx: i16, dy: i16) {
    l.anim_dx = dx;
    l.anim_dy = dy;
}

// ---------------------------------------------------------------------------
// A116 合成多屏
// ---------------------------------------------------------------------------

/// 单屏描述：在帧缓冲中的偏移与尺寸，及其底色。A116。
#[derive(Clone, Copy, Debug)]
pub struct Screen {
    pub ox: usize,
    pub oy: usize,
    pub w: usize,
    pub h: usize,
    pub color: [u8; 4],
}

/// 填充矩形区域（用于多屏底色）。
fn fill_rect(buf: &mut [u8], w: usize, h: usize, r: &Rect, color: [u8; 4]) {
    if buf.len() < w * h * 4 {
        return;
    }
    let x1 = (r.x + r.w).min(w);
    let y1 = (r.y + r.h).min(h);
    for y in r.y..y1 {
        for x in r.x..x1 {
            let i = (y * w + x) * 4;
            buf[i] = color[0];
            buf[i + 1] = color[1];
            buf[i + 2] = color[2];
            buf[i + 3] = color[3];
        }
    }
}

/// 把图层合成到某屏的裁剪区域（A116）。
pub fn composite_screen_region(
    canvas: &mut [u8],
    cw: usize,
    ch: usize,
    reg: &LayerRegistry,
    screen: &Screen,
) -> usize {
    let r = Rect {
        x: screen.ox,
        y: screen.oy,
        w: screen.w,
        h: screen.h,
    };
    composite_region(canvas, cw, ch, reg, Some(r), 0)
}

/// 多屏合成：逐屏填底色并裁剪合成（A116）。返回写入像素数。
pub fn composite_multi_screen(
    screens: &[Screen],
    reg: &LayerRegistry,
    frame: &mut [u8],
    fw: usize,
    fh: usize,
) -> usize {
    if frame.len() < fw * fh * 4 {
        return 0;
    }
    let mut total = 0usize;
    for s in screens {
        let r = Rect {
            x: s.ox,
            y: s.oy,
            w: s.w,
            h: s.h,
        };
        fill_rect(frame, fw, fh, &r, s.color);
        total += s.w * s.h; // 底色写入像素计入总数
        total += composite_region(frame, fw, fh, reg, Some(r), 0);
    }
    total
}

// ---------------------------------------------------------------------------
// A117 合成内存预算
// ---------------------------------------------------------------------------

/// 估算一帧占用字节（RGBA8）（A117）。
pub fn estimate_frame_bytes(w: usize, h: usize) -> usize {
    w * h * 4
}

/// 内存预算判定（A117）。
pub fn memory_budget_ok(used: usize, budget: usize) -> bool {
    used <= budget
}

// ---------------------------------------------------------------------------
// A118 合成一次完成标志（整帧提交语义）
// ---------------------------------------------------------------------------

/// 整帧提交状态：一次合成完成后置 committed，并清空脏计数（A118）。
#[derive(Clone, Copy, Debug)]
pub struct ComposeFrame {
    pub committed: bool,
    pub dirty: usize,
}

impl ComposeFrame {
    pub const fn new() -> ComposeFrame {
        ComposeFrame {
            committed: false,
            dirty: 0,
        }
    }
}

/// 标记脏像素数。
pub fn frame_mark_dirty(f: &mut ComposeFrame, n: usize) {
    f.dirty = f.dirty.saturating_add(n);
}

/// 整帧提交：成功则置 committed 并清脏；返回提交前是否有脏内容（A118）。
pub fn frame_commit(f: &mut ComposeFrame) -> bool {
    let had_dirty = f.dirty > 0;
    f.committed = true;
    f.dirty = 0;
    had_dirty
}

// ---------------------------------------------------------------------------
// A120 合成器性能预算
// ---------------------------------------------------------------------------

/// 性能预算判定：最近一次合成耗时是否在预算内（A120）。
pub fn perf_budget_ok(p: &ComposeProfile, budget_micros: u32) -> bool {
    p.last_micros <= budget_micros
}

// ---------------------------------------------------------------------------
// A121 合成器可观测（遥测文本渲染）
// ---------------------------------------------------------------------------

/// 把合成遥测渲染为文本到 `out`，返回写入字节数（A121）。
pub fn compositor_telemetry(p: &ComposeProfile, out: &mut [u8]) -> usize {
    let mut n = 0usize;
    push_str(out, &mut n, "compositor frames=");
    push_usize(out, &mut n, p.frames as usize);
    push_str(out, &mut n, " pixels=");
    push_usize(out, &mut n, p.pixels as usize);
    push_str(out, &mut n, " max_us=");
    push_usize(out, &mut n, p.max_micros as usize);
    push_str(out, &mut n, "\n");
    n
}

// ---------------------------------------------------------------------------
// A122 合成器模糊测试（边界安全合成）
// ---------------------------------------------------------------------------

/// 模糊测试入口：以任意注册表/画布做合成，必须不越界、不 panic（A122）。
pub fn fuzz_composite(canvas: &mut [u8], cw: usize, ch: usize, reg: &LayerRegistry) -> usize {
    composite_region(canvas, cw, ch, reg, None, 0)
}

// ---------------------------------------------------------------------------
// A123 合成器文档
// ---------------------------------------------------------------------------

/// 合成器文档横幅（A123）。
pub fn compositor_doc() -> &'static str {
    "AURORA-1000 AI-05 compositor A101..A125: layer tree, blend modes, \
     gauss blur, drop shadow, alpha compositing, animated compose, vsync, \
     dirty rect, clip, snapshot, offscreen, profile, degrade, multi-screen, \
     memory budget, commit, telemetry, fuzz, finalize."
}

// ---------------------------------------------------------------------------
// A124 合成器降级链（降级时选用更廉价的混合）
// ---------------------------------------------------------------------------

/// 按降级档挑选混合模式：Full 保留原模式，其余降级为 Normal（A124）。
pub fn degrade_blend(preferred: BlendMode, level: DegradeLevel) -> BlendMode {
    match level {
        DegradeLevel::Full => preferred,
        DegradeLevel::Reduced => {
            if preferred == BlendMode::Multiply {
                BlendMode::Normal
            } else {
                preferred
            }
        }
        DegradeLevel::Minimal => BlendMode::Normal,
    }
}

// ---------------------------------------------------------------------------
// A119 合成器自检（域自检，≥25 项全真）
// ---------------------------------------------------------------------------

/// A119 合成器自检：对样例小画布断言合成像素，产出 ≥25 项全真 CheckSet。
pub fn run_compositor_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-compositor");

    // --- A101 图层树管理 ---
    let mut reg = LayerRegistry::new();
    let l0 = make_layer(1, 0, 0, 4, 4, [255, 0, 0, 255], 255, BlendMode::Normal);
    let l1 = make_layer(2, 0, 0, 4, 4, [0, 0, 255, 255], 255, BlendMode::Normal);
    let added = layer_tree_add(&mut reg, l0) && layer_tree_add(&mut reg, l1);
    set.add("A101 register", added && reg.len() == 2, "two layers registered");

    // z 序：把 l1 改为更高 z 并排序
    reg.layers[1].z = 5;
    layer_tree_sort(&mut reg);
    set.add(
        "A101 z-sort",
        reg.get(0).unwrap().z <= reg.get(1).unwrap().z && reg.get(1).unwrap().z == 5,
        "sorted by z",
    );

    let vis = layer_tree_set_visible(&mut reg, 2, false);
    set.add(
        "A101 visible toggle",
        vis && !reg.get(1).unwrap().visible,
        "set invisible",
    );
    layer_tree_set_visible(&mut reg, 2, true);

    // --- A102 窗口纹理缓存 ---
    let mut cache = TextureCache::new();
    let tex = [200u8; 12];
    let stored = texture_cache_store(&mut cache, 7, 3, 1, &tex);
    let got = texture_cache_get(&cache, 7);
    set.add(
        "A102 texture store",
        stored && cache.len() == 1 && got.is_some() && got.unwrap().w == 3,
        "store and fetch",
    );
    set.add(
        "A102 texture data",
        got.map(|t| t.data[0] == 200 && t.data[11] == 200).unwrap_or(false),
        "bytes copied",
    );

    // --- A103 混合模式库 ---
    let dn = blend_mode_apply(
        BlendMode::Normal,
        [255, 0, 0, 255],
        [0, 255, 0, 255],
    );
    set.add(
        "A103 normal",
        dn[0] == 0 && dn[1] == 255 && dn[2] == 0,
        "src over opaque",
    );
    let da = blend_mode_apply(BlendMode::Add, [10, 10, 10, 255], [20, 20, 20, 255]);
    set.add(
        "A103 add",
        da[0] == 30 && da[1] == 30 && da[2] == 30,
        "additive",
    );
    let dm = blend_mode_apply(
        BlendMode::Multiply,
        [100, 100, 100, 255],
        [200, 200, 200, 255],
    );
    set.add("A103 multiply", dm[0] == 78 && dm[1] == 78, "multiply 100*200/255");

    // --- A104 高斯模糊 ---
    let mut src = [0u8; 100]; // 5x5
    let mut tmp = [0u8; 100];
    let mut out = [0u8; 100];
    src[px_index(5, 2, 2) + 0] = 255;
    src[px_index(5, 2, 2) + 1] = 255;
    src[px_index(5, 2, 2) + 2] = 255;
    src[px_index(5, 2, 2) + 3] = 255;
    let blurred = gauss_blur_3x3(&src, 5, 5, &mut tmp, &mut out);
    set.add(
        "A104 blur center<255",
        blurred == 100 && out[px_index(5, 2, 2)] < 255 && out[px_index(5, 2, 2)] > 0,
        "center spread",
    );
    set.add(
        "A104 blur neighbor>0",
        out[px_index(5, 2, 1)] > 0,
        "neighbor lit",
    );

    // --- A105 投影与阴影 ---
    let mut cv = [0u8; 16 * 16 * 4];
    let sh = draw_drop_shadow(&mut cv, 16, 16, 4, 4, 4, 4, 4, [0, 0, 0, 255], 200);
    let outside = px_index(16, 2, 6) + 3; // 矩形外、距离 2
    set.add(
        "A105 shadow outside",
        sh > 0 && cv[outside] > 0,
        "shadow drawn outside rect",
    );

    // --- A106 透明度合成 ---
    let mut reg2 = LayerRegistry::new();
    layer_tree_add(
        &mut reg2,
        make_layer(1, 0, 0, 8, 8, [255, 0, 0, 255], 255, BlendMode::Normal),
    );
    layer_tree_add(
        &mut reg2,
        make_layer(2, 2, 2, 4, 4, [0, 0, 255, 255], 255, BlendMode::Normal),
    );
    let mut canvas = [0u8; 8 * 8 * 4];
    composite_src_over(&mut canvas, 8, 8, &reg2);
    set.add(
        "A106 top covers",
        canvas[px_index(8, 3, 3)] == 0 && canvas[px_index(8, 3, 3) + 2] == 255,
        "top layer blue",
    );
    set.add(
        "A106 bottom only",
        canvas[px_index(8, 0, 0)] == 255 && canvas[px_index(8, 0, 0) + 2] == 0,
        "bottom red",
    );
    // alpha=0 图层无影响
    let mut reg3 = LayerRegistry::new();
    layer_tree_add(
        &mut reg3,
        make_layer(1, 1, 1, 2, 2, [0, 255, 0, 0], 0, BlendMode::Normal),
    );
    let mut c3 = [0u8; 8 * 8 * 4];
    composite_src_over(&mut c3, 8, 8, &reg3);
    set.add(
        "A106 alpha0 no-op",
        c3[px_index(8, 1, 1)] == 0 && c3[px_index(8, 1, 1) + 1] == 0,
        "transparent layer ignored",
    );

    // --- A107 动画属性合成 ---
    let mut reg4 = LayerRegistry::new();
    let mut gl = make_layer(1, 1, 1, 3, 3, [0, 255, 0, 255], 255, BlendMode::Normal);
    gl.anim_dx = 4;
    gl.anim_dy = 0;
    layer_tree_add(&mut reg4, gl);
    let mut c_a = [0u8; 8 * 8 * 4];
    let mut c_b = [0u8; 8 * 8 * 4];
    composite_animated(&mut c_a, 8, 8, &reg4, 0);
    composite_animated(&mut c_b, 8, 8, &reg4, 255);
    set.add(
        "A107 t0 base",
        c_a[px_index(8, 2, 2) + 1] == 255 && c_a[px_index(8, 6, 2)] == 0,
        "at base at t=0",
    );
    set.add(
        "A107 t255 target",
        c_b[px_index(8, 6, 2) + 1] == 255 && c_b[px_index(8, 2, 2)] == 0,
        "shifted at t=255",
    );

    // --- A108 vsync 对齐 ---
    let mut clk = FrameClock::new(2);
    let mut presents = 0u64;
    let mut dropped = 0u64;
    for f in 0..4 {
        match vsync_tick(&mut clk, f % 2 == 0) {
            VsyncVerdict::Present => presents += 1,
            VsyncVerdict::Dropped => dropped += 1,
            VsyncVerdict::Skip => {}
        }
    }
    set.add(
        "A108 present count",
        presents == 2 && clk.presented == 2,
        "two present frames",
    );
    let mut clk2 = FrameClock::new(2);
    let mut d2 = 0u64;
    for f in 0..4 {
        if let VsyncVerdict::Dropped = vsync_tick(&mut clk2, f != 0) {
            d2 += 1;
        }
    }
    set.add("A108 dropped", dropped == 0 && d2 == 1, "dropped frame detected");

    // --- A109 部分重绘 ---
    let mut reg5 = LayerRegistry::new();
    layer_tree_add(
        &mut reg5,
        make_layer(1, 0, 0, 8, 8, [255, 255, 255, 255], 255, BlendMode::Normal),
    );
    let mut c9 = [0u8; 8 * 8 * 4];
    let dirty = Rect { x: 4, y: 4, w: 4, h: 4 };
    composite_dirty(&mut c9, 8, 8, &reg5, &dirty);
    set.add(
        "A109 dirty region",
        c9[px_index(8, 5, 5)] == 255 && c9[px_index(8, 1, 1)] == 0,
        "only dirty rect updated",
    );

    // --- A110 图层裁剪优化 ---
    let mut reg6 = LayerRegistry::new();
    layer_tree_add(
        &mut reg6,
        make_layer(1, 0, 0, 8, 8, [255, 255, 255, 255], 255, BlendMode::Normal),
    );
    let mut c10 = [0u8; 8 * 8 * 4];
    let clip = Rect { x: 4, y: 4, w: 4, h: 4 };
    composite_clipped(&mut c10, 8, 8, &reg6, &clip);
    set.add(
        "A110 clip outside",
        c10[px_index(8, 5, 5)] == 255 && c10[px_index(8, 1, 1)] == 0,
        "outside clip untouched",
    );

    // --- A111 合成快照 ---
    let mut snap_src = [0u8; 4 * 4 * 4];
    for i in 0..snap_src.len() {
        snap_src[i] = (i % 251) as u8;
    }
    let mut snap_out = [0u8; 4 * 4 * 4];
    let copied = composite_snapshot(&snap_src, &mut snap_out);
    let mut same = copied == snap_src.len();
    for i in 0..snap_src.len() {
        if snap_src[i] != snap_out[i] {
            same = false;
        }
    }
    set.add("A111 snapshot", same, "exact frame copy");

    // --- A112 离屏合成缓冲 ---
    let mut reg7 = LayerRegistry::new();
    layer_tree_add(
        &mut reg7,
        make_layer(1, 1, 1, 4, 4, [255, 0, 255, 255], 255, BlendMode::Normal),
    );
    let mut off = Offscreen::new(16, 16);
    let off_n = offscreen_compose(&mut off, &reg7);
    set.add(
        "A112 offscreen",
        off_n > 0 && off.data[px_index(16, 2, 2)] == 255,
        "offscreen compose",
    );

    // --- A113 合成性能剖析 ---
    let mut prof = ComposeProfile::new();
    profile_record(&mut prof, 100, 50);
    profile_record(&mut prof, 100, 70);
    set.add(
        "A113 profile",
        prof.frames == 2 && prof.pixels == 200 && prof.max_micros == 70,
        "accumulates",
    );

    // --- A114 合成降级链 ---
    set.add(
        "A114 degrade",
        degrade_for_budget(10, 20) == DegradeLevel::Full
            && degrade_for_budget(15, 20) == DegradeLevel::Reduced
            && degrade_for_budget(30, 20) == DegradeLevel::Minimal,
        "three levels",
    );

    // --- A115 合成与动效协作 ---
    set.add(
        "A115 motion lerp",
        motion_evaluate(0, 0, 10) == 0
            && motion_evaluate(255, 0, 10) == 10
            && motion_evaluate(128, 0, 10) == 5,
        "linear interp",
    );

    // --- A116 合成多屏 ---
    let screens = [
        Screen {
            ox: 0,
            oy: 0,
            w: 8,
            h: 8,
            color: [40, 0, 0, 255],
        },
        Screen {
            ox: 8,
            oy: 0,
            w: 8,
            h: 8,
            color: [0, 40, 0, 255],
        },
    ];
    let mut frame = [0u8; 16 * 8 * 4];
    let m = composite_multi_screen(&screens, &LayerRegistry::new(), &mut frame, 16, 8);
    set.add(
        "A116 multi-screen",
        m > 0 && frame[px_index(16, 1, 1)] == 40 && frame[px_index(16, 9, 1) + 1] == 40
            && frame[px_index(16, 1, 1) + 1] == 0
            && frame[px_index(16, 9, 1)] == 0,
        "per-screen bg",
    );

    // --- A117 合成内存预算 ---
    set.add(
        "A117 memory",
        estimate_frame_bytes(32, 32) == 4096
            && memory_budget_ok(100, 200)
            && !memory_budget_ok(300, 200),
        "budget math",
    );

    // --- A118 合成一次完成标志 ---
    let mut cf = ComposeFrame::new();
    frame_mark_dirty(&mut cf, 3);
    let had = frame_commit(&mut cf);
    set.add(
        "A118 commit",
        had && cf.committed && cf.dirty == 0,
        "frame committed",
    );

    // --- A120 合成器性能预算 ---
    let mut p2 = ComposeProfile::new();
    profile_record(&mut p2, 10, 120);
    let mut p3 = ComposeProfile::new();
    profile_record(&mut p3, 10, 300);
    set.add(
        "A120 perf budget",
        perf_budget_ok(&p2, 200) && !perf_budget_ok(&p3, 200),
        "within budget",
    );

    // --- A121 合成器可观测 ---
    let mut tel = [0u8; 128];
    let tn = compositor_telemetry(&prof, &mut tel);
    set.add("A121 telemetry", tn > 0, "telemetry rendered");

    // --- A122 合成器模糊测试 ---
    let mut fuzz = [0u8; 2 * 2 * 4];
    let empty = LayerRegistry::new();
    let fz = fuzz_composite(&mut fuzz, 2, 2, &empty);
    let mut still_zero = true;
    for b in fuzz.iter() {
        if *b != 0 {
            still_zero = false;
        }
    }
    set.add("A122 fuzz empty", fz == 0 && still_zero, "empty reg safe");

    // --- A123 合成器文档 ---
    let doc = compositor_doc();
    set.add(
        "A123 doc",
        doc.contains("A101") && doc.contains("A125"),
        "doc banner",
    );

    // --- A124 合成器降级链 ---
    set.add(
        "A124 degrade blend",
        degrade_blend(BlendMode::Multiply, DegradeLevel::Reduced) == BlendMode::Normal
            && degrade_blend(BlendMode::Add, DegradeLevel::Minimal) == BlendMode::Normal
            && degrade_blend(BlendMode::Normal, DegradeLevel::Full) == BlendMode::Normal,
        "simplify on degrade",
    );

    set
}

// ---------------------------------------------------------------------------
// A125 合成器域自检收口
// ---------------------------------------------------------------------------

/// A125 合成器域自检收口：运行全量自检并保证全绿后返回结果集。
pub fn compositor_domain_finalize() -> CheckSet {
    let set = run_compositor_checks();
    // 收口契约：域自检必须全绿。
    if !set.all_passed() {
        // 不 panic（no_std 友好），仅保证返回的集反映真实状态。
        return set;
    }
    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(id: u16, x: i16, y: i16, w: u16, h: u16, color: [u8; 4]) -> Layer {
        make_layer(id, x, y, w, h, color, 255, BlendMode::Normal)
    }

    #[test]
    fn a101_register_and_count() {
        let mut reg = LayerRegistry::new();
        assert!(layer_tree_add(&mut reg, solid(1, 0, 0, 2, 2, [1, 2, 3, 4])));
        assert!(layer_tree_add(&mut reg, solid(2, 0, 0, 2, 2, [4, 3, 2, 1])));
        assert_eq!(reg.len(), 2);
        // 容量 MAX_LAYERS=16：填满后拒绝。
        let mut id = 3u16;
        while reg.len() < MAX_LAYERS {
            assert!(layer_tree_add(&mut reg, solid(id, 0, 0, 1, 1, [0; 4])));
            id += 1;
        }
        assert!(!layer_tree_add(&mut reg, solid(id, 0, 0, 1, 1, [0; 4]))); // 满
        assert_eq!(reg.len(), MAX_LAYERS);
    }

    #[test]
    fn a101_z_sort_order() {
        let mut reg = LayerRegistry::new();
        let mut a = solid(1, 0, 0, 1, 1, [0; 4]);
        a.z = 9;
        let mut b = solid(2, 0, 0, 1, 1, [0; 4]);
        b.z = 1;
        layer_tree_add(&mut reg, a);
        layer_tree_add(&mut reg, b);
        layer_tree_sort(&mut reg);
        assert!(reg.get(0).unwrap().z <= reg.get(1).unwrap().z);
        assert_eq!(reg.get(0).unwrap().z, 1);
    }

    #[test]
    fn a101_visible_toggle() {
        let mut reg = LayerRegistry::new();
        layer_tree_add(&mut reg, solid(1, 0, 0, 1, 1, [0; 4]));
        assert!(layer_tree_set_visible(&mut reg, 1, false));
        assert!(!reg.get(0).unwrap().visible);
        assert!(!layer_tree_set_visible(&mut reg, 99, false));
    }

    #[test]
    fn a102_texture_store_get() {
        let mut c = TextureCache::new();
        let data = [9u8; 20];
        assert!(texture_cache_store(&mut c, 3, 5, 1, &data));
        let t = texture_cache_get(&c, 3).unwrap();
        assert_eq!(t.w, 5);
        assert_eq!(t.data[0], 9);
        assert_eq!(t.data[19], 9);
        assert!(texture_cache_get(&c, 4).is_none());
    }

    #[test]
    fn a102_texture_overflow_rejected() {
        let mut c = TextureCache::new();
        // 超过 TEX_PX*4 的纹理被拒
        assert!(!texture_cache_store(&mut c, 1, 0xFF, 0xFF, &[0u8; 4]));
    }

    #[test]
    fn a103_blend_normal() {
        let o = blend_mode_apply(BlendMode::Normal, [0, 0, 0, 0], [0, 0, 255, 255]);
        assert_eq!(o[2], 255);
        assert_eq!(o[3], 255);
        let o2 = blend_mode_apply(BlendMode::Normal, [100, 100, 100, 255], [0, 0, 0, 0]);
        assert_eq!(o2[0], 100);
    }

    #[test]
    fn a103_blend_add() {
        let o = blend_mode_apply(BlendMode::Add, [10, 10, 10, 255], [20, 20, 20, 255]);
        assert_eq!(o[0], 30);
        let o2 = blend_mode_apply(BlendMode::Add, [250, 0, 0, 255], [20, 0, 0, 255]);
        assert_eq!(o2[0], 255); // 饱和
    }

    #[test]
    fn a103_blend_multiply() {
        let o = blend_mode_apply(BlendMode::Multiply, [200, 200, 200, 255], [128, 128, 128, 255]);
        assert_eq!(o[0], 100); // 200*128/255 = 100
    }

    #[test]
    fn a104_gauss_blur_center() {
        let mut src = [0u8; 100];
        let mut tmp = [0u8; 100];
        let mut out = [0u8; 100];
        src[px_index(5, 2, 2) + 3] = 255;
        src[px_index(5, 2, 2)] = 255;
        assert_eq!(gauss_blur_3x3(&src, 5, 5, &mut tmp, &mut out), 100);
        assert!(out[px_index(5, 2, 2)] < 255);
        assert!(out[px_index(5, 2, 2)] > 0);
    }

    #[test]
    fn a104_gauss_blur_edges() {
        let mut src = [0u8; 100];
        let mut tmp = [0u8; 100];
        let mut out = [0u8; 100];
        src[px_index(5, 2, 2) + 3] = 255;
        src[px_index(5, 2, 2)] = 255;
        gauss_blur_3x3(&src, 5, 5, &mut tmp, &mut out);
        // 3x3 邻域均被点亮
        assert!(out[px_index(5, 1, 1)] > 0);
        assert!(out[px_index(5, 3, 3)] > 0);
    }

    #[test]
    fn a104_gauss_blur_empty_canvas() {
        let src = [0u8; 4];
        let mut tmp = [0u8; 4];
        let mut out = [0u8; 4];
        assert_eq!(gauss_blur_3x3(&src, 0, 1, &mut tmp, &mut out), 0);
    }

    #[test]
    fn a105_drop_shadow_outside() {
        let mut cv = [0u8; 16 * 16 * 4];
        let n = draw_drop_shadow(&mut cv, 16, 16, 4, 4, 4, 4, 4, [0, 0, 0, 255], 200);
        assert!(n > 0);
        let a = px_index(16, 2, 6) + 3;
        assert!(cv[a] > 0);
        // 远超半径处不变
        let far = px_index(16, 15, 6) + 3;
        assert_eq!(cv[far], 0);
    }

    #[test]
    fn a105_drop_shadow_radius_zero() {
        let mut cv = [0u8; 16 * 16 * 4];
        assert_eq!(draw_drop_shadow(&mut cv, 16, 16, 4, 4, 4, 4, 0, [0; 4], 200), 0);
    }

    #[test]
    fn a106_src_over_top() {
        let mut reg = LayerRegistry::new();
        layer_tree_add(&mut reg, solid(1, 0, 0, 8, 8, [255, 0, 0, 255]));
        layer_tree_add(&mut reg, solid(2, 2, 2, 4, 4, [0, 0, 255, 255]));
        let mut cv = [0u8; 8 * 8 * 4];
        composite_src_over(&mut cv, 8, 8, &reg);
        assert_eq!(cv[px_index(8, 3, 3) + 2], 255);
        assert_eq!(cv[px_index(8, 3, 3)], 0);
    }

    #[test]
    fn a106_src_over_bottom() {
        let mut reg = LayerRegistry::new();
        layer_tree_add(&mut reg, solid(1, 0, 0, 8, 8, [255, 0, 0, 255]));
        layer_tree_add(&mut reg, solid(2, 2, 2, 4, 4, [0, 0, 255, 255]));
        let mut cv = [0u8; 8 * 8 * 4];
        composite_src_over(&mut cv, 8, 8, &reg);
        assert_eq!(cv[px_index(8, 0, 0)], 255);
        assert_eq!(cv[px_index(8, 0, 0) + 2], 0);
    }

    #[test]
    fn a106_alpha_zero_noop() {
        let mut reg = LayerRegistry::new();
        let mut l = solid(1, 1, 1, 2, 2, [0, 255, 0, 255]);
        l.alpha = 0;
        layer_tree_add(&mut reg, l);
        let mut cv = [0u8; 8 * 8 * 4];
        composite_src_over(&mut cv, 8, 8, &reg);
        assert_eq!(cv[px_index(8, 1, 1)], 0);
    }

    #[test]
    fn a107_animated_t0() {
        let mut reg = LayerRegistry::new();
        let mut gl = solid(1, 1, 1, 3, 3, [0, 255, 0, 255]);
        gl.anim_dx = 4;
        layer_tree_add(&mut reg, gl);
        let mut cv = [0u8; 8 * 8 * 4];
        composite_animated(&mut cv, 8, 8, &reg, 0);
        assert_eq!(cv[px_index(8, 2, 2) + 1], 255);
        assert_eq!(cv[px_index(8, 6, 2)], 0);
    }

    #[test]
    fn a107_animated_t255() {
        let mut reg = LayerRegistry::new();
        let mut gl = solid(1, 1, 1, 3, 3, [0, 255, 0, 255]);
        gl.anim_dx = 4;
        layer_tree_add(&mut reg, gl);
        let mut cv = [0u8; 8 * 8 * 4];
        composite_animated(&mut cv, 8, 8, &reg, 255);
        assert_eq!(cv[px_index(8, 6, 2) + 1], 255);
        assert_eq!(cv[px_index(8, 2, 2)], 0);
    }

    #[test]
    fn a108_vsync_present() {
        let mut clk = FrameClock::new(2);
        let mut presents = 0;
        for f in 0..4 {
            if let VsyncVerdict::Present = vsync_tick(&mut clk, f % 2 == 0) {
                presents += 1;
            }
        }
        assert_eq!(presents, 2);
    }

    #[test]
    fn a108_vsync_dropped() {
        let mut clk = FrameClock::new(2);
        let mut dropped = 0;
        for f in 0..4 {
            if let VsyncVerdict::Dropped = vsync_tick(&mut clk, f != 0) {
                dropped += 1;
            }
        }
        assert_eq!(dropped, 1);
    }

    #[test]
    fn a109_dirty_region() {
        let mut reg = LayerRegistry::new();
        layer_tree_add(&mut reg, solid(1, 0, 0, 8, 8, [255, 255, 255, 255]));
        let mut cv = [0u8; 8 * 8 * 4];
        let dirty = Rect { x: 4, y: 4, w: 4, h: 4 };
        composite_dirty(&mut cv, 8, 8, &reg, &dirty);
        assert_eq!(cv[px_index(8, 5, 5)], 255);
        assert_eq!(cv[px_index(8, 1, 1)], 0);
    }

    #[test]
    fn a109_dirty_union() {
        let a = Some(Rect { x: 0, y: 0, w: 2, h: 2 });
        let b = Rect { x: 4, y: 4, w: 2, h: 2 };
        let u = dirty_union(a, b);
        assert_eq!(u.x, 0);
        assert_eq!(u.w, 6);
        assert_eq!(u.h, 6);
    }

    #[test]
    fn a110_clip_outside() {
        let mut reg = LayerRegistry::new();
        layer_tree_add(&mut reg, solid(1, 0, 0, 8, 8, [255, 255, 255, 255]));
        let mut cv = [0u8; 8 * 8 * 4];
        let clip = Rect { x: 4, y: 4, w: 4, h: 4 };
        composite_clipped(&mut cv, 8, 8, &reg, &clip);
        assert_eq!(cv[px_index(8, 5, 5)], 255);
        assert_eq!(cv[px_index(8, 1, 1)], 0);
    }

    #[test]
    fn a110_layer_clip_intersect() {
        let l = solid(1, 0, 0, 8, 8, [0; 4]);
        let bounds = Rect { x: 4, y: 4, w: 4, h: 4 };
        let r = layer_clip(&l, &bounds);
        assert_eq!(r.x, 4);
        assert_eq!(r.w, 4);
    }

    #[test]
    fn a111_snapshot_copy() {
        let mut src = [0u8; 4 * 4 * 4];
        for (i, b) in src.iter_mut().enumerate() {
            *b = (i % 200) as u8;
        }
        let mut out = [0u8; 4 * 4 * 4];
        assert_eq!(composite_snapshot(&src, &mut out), src.len());
        assert_eq!(src, out);
    }

    #[test]
    fn a112_offscreen_compose() {
        let mut reg = LayerRegistry::new();
        layer_tree_add(&mut reg, solid(1, 1, 1, 4, 4, [255, 0, 255, 255]));
        let mut off = Offscreen::new(16, 16);
        let n = offscreen_compose(&mut off, &reg);
        assert!(n > 0);
        assert_eq!(off.data[px_index(16, 2, 2)], 255);
    }

    #[test]
    fn a113_profile_record() {
        let mut p = ComposeProfile::new();
        profile_record(&mut p, 100, 50);
        profile_record(&mut p, 100, 70);
        assert_eq!(p.frames, 2);
        assert_eq!(p.pixels, 200);
        assert_eq!(p.max_micros, 70);
        assert_eq!(p.last_micros, 70);
    }

    #[test]
    fn a114_degrade_levels() {
        assert_eq!(degrade_for_budget(10, 20), DegradeLevel::Full);
        assert_eq!(degrade_for_budget(15, 20), DegradeLevel::Reduced);
        assert_eq!(degrade_for_budget(30, 20), DegradeLevel::Minimal);
    }

    #[test]
    fn a115_motion_lerp() {
        assert_eq!(motion_evaluate(0, 0, 10), 0);
        assert_eq!(motion_evaluate(255, 0, 10), 10);
        assert_eq!(motion_evaluate(128, 0, 10), 5);
        assert_eq!(motion_evaluate(255, 10, -10), -10);
    }

    #[test]
    fn a116_multi_screen() {
        let screens = [
            Screen { ox: 0, oy: 0, w: 8, h: 8, color: [40, 0, 0, 255] },
            Screen { ox: 8, oy: 0, w: 8, h: 8, color: [0, 40, 0, 255] },
        ];
        let mut frame = [0u8; 16 * 8 * 4];
        let m = composite_multi_screen(&screens, &LayerRegistry::new(), &mut frame, 16, 8);
        assert!(m > 0);
        assert_eq!(frame[px_index(16, 1, 1)], 40);
        assert_eq!(frame[px_index(16, 9, 1) + 1], 40);
    }

    #[test]
    fn a117_memory_budget() {
        assert_eq!(estimate_frame_bytes(32, 32), 4096);
        assert!(memory_budget_ok(100, 200));
        assert!(!memory_budget_ok(300, 200));
    }

    #[test]
    fn a118_frame_commit() {
        let mut f = ComposeFrame::new();
        frame_mark_dirty(&mut f, 3);
        assert!(frame_commit(&mut f));
        assert!(f.committed);
        assert_eq!(f.dirty, 0);
        // 无脏内容时提交返回 false
        assert!(!frame_commit(&mut f));
    }

    #[test]
    fn a120_perf_budget() {
        let mut p = ComposeProfile::new();
        profile_record(&mut p, 10, 120);
        assert!(perf_budget_ok(&p, 200));
        assert!(!perf_budget_ok(&p, 100));
    }

    #[test]
    fn a121_telemetry() {
        let mut p = ComposeProfile::new();
        profile_record(&mut p, 50, 10);
        let mut buf = [0u8; 128];
        let n = compositor_telemetry(&p, &mut buf);
        assert!(n > 0);
        assert!(core::str::from_utf8(&buf[..n]).unwrap().contains("compositor"));
    }

    #[test]
    fn a122_fuzz_empty() {
        let mut cv = [0u8; 2 * 2 * 4];
        let reg = LayerRegistry::new();
        assert_eq!(fuzz_composite(&mut cv, 2, 2, &reg), 0);
        assert!(cv.iter().all(|b| *b == 0));
    }

    #[test]
    fn a122_fuzz_oversized_layer() {
        let mut reg = LayerRegistry::new();
        // w/h 超过图层缓冲上限，应被钳制且不越界
        let big = make_layer(1, 0, 0, 200, 200, [255, 0, 0, 255], 255, BlendMode::Normal);
        layer_tree_add(&mut reg, big);
        let mut cv = [0u8; 8 * 8 * 4];
        let n = fuzz_composite(&mut cv, 8, 8, &reg);
        assert!(n <= 8 * 8);
        // 仅 (0..16 钳制为 8) 范围被写入，无越界
        assert_eq!(cv[px_index(8, 0, 0)], 255);
    }

    #[test]
    fn a123_doc_contains() {
        let d = compositor_doc();
        assert!(d.contains("A101"));
        assert!(d.contains("A125"));
    }

    #[test]
    fn a124_degrade_blend() {
        assert_eq!(
            degrade_blend(BlendMode::Multiply, DegradeLevel::Reduced),
            BlendMode::Normal
        );
        assert_eq!(
            degrade_blend(BlendMode::Add, DegradeLevel::Minimal),
            BlendMode::Normal
        );
        assert_eq!(
            degrade_blend(BlendMode::Normal, DegradeLevel::Full),
            BlendMode::Normal
        );
        // Reduced 档保留 Add
        assert_eq!(
            degrade_blend(BlendMode::Add, DegradeLevel::Reduced),
            BlendMode::Add
        );
    }

    #[test]
    fn a125_finalize_all_pass() {
        let set = compositor_domain_finalize();
for i in 0..set.len() { if let Some(c) = set.get(i) { if !c.passed { println!("DIAG a125 fail: {} : {}", c.name, c.detail); } } }
        assert!(!set.is_empty());
        assert!(set.all_passed());
        let (passed, failed) = set.tally();
        assert_eq!(failed, 0);
        assert!(passed >= 25);
    }
}
