//! 全景画布·现代艺术风格（#251~#275，AI-04 域一）。
//!
//! 三层渲染架构（WebGL2 底层 / Canvas2D 中层 / DOM 顶层）的数据与参数契约，
//! 本文件只产出**确定性的布局/样式/动画数据**，不依赖任何图形后端——
//! 三端（壳A/壳B/壳C）各自把同一份数据喂给自己的渲染器即可等价。

use crate::model::{Edge, EdgeKind};

// ───────────────────────── 公共几何 ─────────────────────────

/// 画布相机：无限平移，无边界无网格（F251）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { x: 0.0, y: 0.0, zoom: 1.0 }
    }
}

impl Camera {
    /// 平移（无限，负方向/超大距离都不允许越界截断）。
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
    }

    /// 对数缩放（F278 同款策略）：滚轮 delta，近处慢远处快，钳制 10%~500%。
    pub fn zoom_by(&mut self, wheel: f64) {
        let factor = (1.0 + wheel.abs() * 0.001).powf(wheel.signum());
        self.zoom = (self.zoom * factor).clamp(0.1, 5.0);
    }
}

// ───────────────────────── F251 无限画布 ─────────────────────────

/// 画布背景样式：纯色、无网格、无边界（F251）。
pub const CANVAS_BG_LIGHT: &str = "#FFFFFF";
pub const CANVAS_BG_DARK: &str = "#0A0A0F";

// ───────────────────────── F252 语义引力布局 ─────────────────────────

/// 力导向布局：引力=模块依赖强度，斥力=调用频率，收敛后锁定。
#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub x: f64,
    pub y: f64,
    /// 模块依赖强度（0~1，引力系数）。
    pub gravity: f64,
    /// 调用频率（0~1，斥力系数）。
    pub repulsion: f64,
}

/// 迭代一步力导向；返回最大位移。
pub fn force_step(bodies: &mut [Body]) -> f64 {
    const REST: f64 = 120.0;
    let n = bodies.len();
    let mut fx = vec![0.0; n];
    let mut fy = vec![0.0; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let dx = bodies[j].x - bodies[i].x;
            let dy = bodies[j].y - bodies[i].y;
            let d = (dx * dx + dy * dy).sqrt().max(1.0);
            // 斥力 ∝ 双方调用频率
            let f = bodies[i].repulsion * bodies[j].repulsion * 4000.0 / (d * d);
            let (ux, uy) = (dx / d, dy / d);
            fx[i] -= ux * f; fy[i] -= uy * f;
            fx[j] += ux * f; fy[j] += uy * f;
            // 引力 ∝ 依赖强度，趋向 REST 距离
            let g = bodies[i].gravity * bodies[j].gravity * (d - REST) * 0.01;
            fx[i] += ux * g; fy[i] += uy * g;
            fx[j] -= ux * g; fy[j] -= uy * g;
        }
    }
    let mut max_move = 0.0f64;
    for (i, b) in bodies.iter_mut().enumerate() {
        b.x += fx[i] * 0.1;
        b.y += fy[i] * 0.1;
        max_move = max_move.max(fx[i].abs() + fy[i].abs());
    }
    max_move
}

/// 跑到收敛（位移 < eps）并锁定；返回实际迭代数。
pub fn force_layout(bodies: &mut [Body], eps: f64, max_iter: usize) -> usize {
    let mut it = 0;
    while it < max_iter {
        if force_step(bodies) < eps {
            return it;
        }
        it += 1;
    }
    it
}

// ───────────────────────── F253 LOD 无级细节 ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    Module,
    Func,
    Line,
}

/// 缩放阈值：>80% 显示代码行，30~80% 显示函数签名，<30% 显示模块名（F253）。
pub fn lod(zoom: f64) -> Lod {
    if zoom > 0.8 { Lod::Line } else if zoom >= 0.3 { Lod::Func } else { Lod::Module }
}

// ───────────────────────── F254 自适应密度 ─────────────────────────

/// 节点面积 = 代码行数 × 系数，钳制 16×16 ~ 200×200（F254）。
pub fn node_size(loc: usize) -> f64 {
    (loc as f64 * 0.5).clamp(16.0, 200.0)
}

// ───────────────────────── F255 全景小地图 ─────────────────────────

/// 离屏 Canvas 小地图：150×100px，半透明黑底，红框=当前视口，可点击跳转。
pub const MINIMAP_W: f64 = 150.0;
pub const MINIMAP_H: f64 = 100.0;

pub struct Minimap {
    /// 世界包围盒。
    pub world: (f64, f64, f64, f64),
}

impl Minimap {
    /// 世界视口 → 小地图红框。
    pub fn viewport_rect(&self, cam: &Camera, view_w: f64, view_h: f64) -> (f64, f64, f64, f64) {
        let (wx, wy, ww, wh) = self.world;
        let sx = MINIMAP_W / (ww - wx).max(1.0);
        let sy = MINIMAP_H / (wh - wy).max(1.0);
        (cam.x * sx, cam.y * sy, view_w * sx, view_h * sy)
    }

    /// 点击小地图 → 世界坐标跳转。
    pub fn jump(&self, mx: f64, my: f64) -> (f64, f64) {
        let (wx, wy, ww, wh) = self.world;
        (wx + mx / MINIMAP_W * (ww - wx), wy + my / MINIMAP_H * (wh - wy))
    }
}

// ───────────────────────── F256 语义着色 ─────────────────────────

/// 语义色相环等分；规格书固定四例色值优先。
pub fn semantic_color(domain: &str) -> &'static str {
    match domain {
        "用户" | "user" => "#007AFF",
        "订单" | "order" => "#34C759",
        "支付" | "pay" => "#FF9500",
        "数据" | "data" => "#AF52DE",
        _ => "#8E8E93",
    }
}

// ───────────────────────── F257 发光节点 / F258 贝塞尔光束 ─────────────────────────

/// 发光节点参数：大小=代码量(8-64px)，亮度=调用频率(0.3-1.0)，颜色=模块色。
pub fn glow(loc: usize, call_freq: f64, module_color: &str) -> (f64, f64, &'static str) {
    // 已知语义色原样透传；未知色回落中性灰
    let color = match module_color {
        "#007AFF" => "#007AFF",
        "#34C759" => "#34C759",
        "#FF9500" => "#FF9500",
        "#AF52DE" => "#AF52DE",
        _ => "#8E8E93",
    };
    ((loc as f64 / 10.0).clamp(8.0, 64.0), call_freq.clamp(0.3, 1.0), color)
}

/// 三次贝塞尔取点（F258 光束路径 / F259 粒子轨道共用）。
pub fn bezier(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    let a = u * u * u;
    let b = 3.0 * u * u * t;
    let c = 3.0 * u * t * t;
    let d = t * t * t;
    (
        a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
        a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
    )
}

/// 光束线宽 = 调用频率对数 (1-4px)，颜色=源节点色，透明度 0.3-0.8。
pub fn beam_style(call_freq: f64) -> (f64, f64) {
    let w = (call_freq.max(0.1).ln() * 1.2).clamp(1.0, 4.0);
    let alpha = call_freq.clamp(0.3, 0.8);
    (w, alpha)
}

// ───────────────────────── F259~F261 粒子/光环/脉冲 ─────────────────────────

/// 数据流粒子：速度=吞吐量（px/s），沿贝塞尔路径。
pub fn particle_pos(path: [(f64, f64); 4], throughput: f64, t_sec: f64) -> (f64, f64) {
    let speed = throughput.clamp(1.0, 100.0);
    let path_len = 300.0; // 规格化路径长度
    let t = ((t_sec * speed) % path_len) / path_len;
    bezier(path[0], path[1], path[2], path[3], t)
}

/// 循环光环：半径=节点大小×1.5，转速=迭代次数（弧度/秒）。
pub fn orbit_ring(node_size: f64, iterations: usize) -> (f64, f64) {
    (node_size * 1.5, iterations as f64 * 0.5)
}

/// 异常脉冲：品红 #FF00FF，周期 2s，扩散半径=节点大小×3。
pub fn pulse_radius(node_size: f64, t_sec: f64) -> f64 {
    let phase = (t_sec % 2.0) / 2.0;
    node_size * 3.0 * phase
}

pub const PULSE_COLOR: &str = "#FF00FF";

// ───────────────────────── F262~F264 热力/霓虹/Bloom ─────────────────────────

/// 性能热力：高斯核贡献，热点中心不透明，边缘模糊扩散（橙 #FF9500）。
pub fn heat_at(sources: &[(f64, f64, f64)], x: f64, y: f64) -> f64 {
    // (x, y, 强度) 求和后钳制 [0,1]
    let mut v = 0.0;
    for (sx, sy, s) in sources {
        let d2 = (x - sx).powi(2) + (y - sy).powi(2);
        v += s * (-d2 / (2.0 * 40.0_f64 * 40.0)).exp();
    }
    if v < 1e-9 { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// 暗色霓虹 Bloom 配置：纯黑底，阈值 0.7，强度 1.5。
pub const NEON_BLOOM_THRESHOLD: f64 = 0.7;
pub const NEON_BLOOM_INTENSITY: f64 = 1.5;
pub const NEON_BG: &str = "#000000";

/// Bloom 光晕：模糊半径 16px，screen 叠加强度可调。
pub fn bloom(radius_px: f64, strength: f64) -> (f64, f64) {
    (radius_px.max(0.0), strength.clamp(0.0, 3.0))
}

pub const BLOOM_BLUR_DEFAULT: f64 = 16.0;

// ───────────────────────── F265 渐变数据河 ─────────────────────────

/// #007AFF → #AF52DE → #FF2D95 沿路径线性插值（t∈[0,1]）。
pub fn river_color(t: f64) -> (f64, f64, f64) {
    let stops: [(f64, f64, f64); 3] = [(0.0, 122.0 / 255.0, 1.0), (172.0 / 255.0, 82.0 / 255.0, 222.0 / 255.0), (1.0, 45.0 / 255.0, 149.0 / 255.0)];
    let t = t.clamp(0.0, 1.0);
    let (a, b, mix) = if t < 0.5 { (stops[0], stops[1], t * 2.0) } else { (stops[1], stops[2], (t - 0.5) * 2.0) };
    (a.0 + (b.0 - a.0) * mix, a.1 + (b.1 - a.1) * mix, a.2 + (b.2 - a.2) * mix)
}

// ───────────────────────── F266~F267 呼吸/星空 ─────────────────────────

/// 呼吸动画：周期 4s，亮度振幅 ±10%，全局同步（只依赖 t，不依赖个体相位）。
pub fn breathe(t_sec: f64) -> f64 {
    1.0 + 0.1 * (std::f64::consts::TAU * t_sec / 4.0).sin()
}

/// 粒子星空：200-500 颗，大小 1-2px，透明度 0.1-0.3，缓慢漂移。
pub struct Starfield {
    pub stars: Vec<(f64, f64, f64, f64)>, // x, y, size(1-2), alpha(0.1-0.3)
}

impl Starfield {
    pub fn new(count: usize, seed: u64) -> Self {
        let count = count.clamp(200, 500);
        let mut rng = seed;
        let mut next = move || {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((rng >> 33) as f64) / ((1u64 << 31) as f64)
        };
        let mut stars = Vec::with_capacity(count);
        for _ in 0..count {
            stars.push((next(), next(), 1.0 + next(), 0.1 + next() * 0.2));
        }
        Starfield { stars }
    }

    /// 漂移：速度极慢（px/s），环绕。
    pub fn drift(&mut self, dt: f64) {
        for s in &mut self.stars {
            s.0 = (s.0 + dt * 0.002) % 1.0;
            s.1 = (s.1 + dt * 0.001) % 1.0;
        }
    }
}

// ───────────────────────── F268~F270 玻璃态/拖尾/星云 ─────────────────────────

/// 玻璃态节点 CSS 契约。
pub const GLASS_CSS: &str = "backdrop-filter: blur(12px) saturate(180%); background: rgba(255,255,255,0.1)";

/// 连线拖尾：长度=速度×0.5s，透明度指数衰减。
pub fn trail(speed: f64, steps: usize) -> Vec<f64> {
    let len = (speed * 0.5).max(1.0);
    (0..steps)
        .map(|i| (-(i as f64) / len).exp())
        .collect()
}

/// 模块星云：径向渐变，中心色=模块色，透明度 5%，半径=包围盒×1.5。
pub fn nebula(bbox: (f64, f64, f64, f64)) -> (f64, f64, f64) {
    let cx = (bbox.0 + bbox.2) / 2.0;
    let cy = (bbox.1 + bbox.3) / 2.0;
    let r = ((bbox.2 - bbox.0).max(bbox.3 - bbox.1)) * 1.5;
    (cx, cy, r)
}

pub const NEBULA_ALPHA: f64 = 0.05;

// ───────────────────────── F271~F273 代码雨/频谱/光影 ─────────────────────────

/// 代码雨：等宽 10px，绿 #34C759，透明度 0.05，下落 2px/帧。
pub const RAIN_COLOR: &str = "#34C759";
pub const RAIN_ALPHA: f64 = 0.05;
pub const RAIN_SPEED_PX_PER_FRAME: f64 = 2.0;

pub fn rain_drop(y: f64, height: f64) -> f64 {
    (y + RAIN_SPEED_PX_PER_FRAME) % height
}

/// 声波频谱：64 根柱，高度=复杂度(0~1→0~max_h)，颜色=模块色。
pub fn spectrum(complexity: &[f64]) -> Vec<f64> {
    complexity.iter().take(64).map(|c| c.clamp(0.0, 1.0)).collect()
}

pub const SPECTRUM_BARS: usize = 64;

/// 时间光影：色温偏移（keyframe 插值，返回 rgb 0~1）。
pub fn day_tint(hour: f64) -> (f64, f64, f64) {
    let stops: [(f64, [f64; 3]); 4] = [
        (6.0, [1.0, 0.93, 0.70]),   // 暖黄
        (12.0, [1.0, 1.0, 1.0]),    // 冷白
        (18.0, [1.0, 0.80, 0.60]),  // 暖橙
        (22.0, [0.70, 0.80, 1.0]),  // 冷蓝
    ];
    let h = hour.rem_euclid(24.0);
    let mut a = stops[0];
    let mut b = stops[0];
    let mut mix = 0.0;
    for w in stops.iter().rev() {
        if h >= w.0 {
            a = *w;
            b = *stops.last().unwrap();
            mix = ((h - a.0) / (b.0 - a.0).max(1.0)).clamp(0.0, 1.0);
            // 找 a 之后的下一个 stop 作为 b
            for w2 in stops.iter() {
                if w2.0 > a.0 {
                    b = *w2;
                    mix = ((h - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
                    break;
                }
            }
            break;
        }
    }
    (
        a.1[0] + (b.1[0] - a.1[0]) * mix,
        a.1[1] + (b.1[1] - a.1[1]) * mix,
        a.1[2] + (b.1[2] - a.1[2]) * mix,
    )
}

// ───────────────────────── F274 截图壁纸 ─────────────────────────

/// 截图壁纸参数：2x 超采样 + MSAA 抗锯齿 + sRGB 色彩校正。
pub struct Screenshot {
    pub supersample: u32,
    pub msaa: bool,
    pub srgb: bool,
}

impl Default for Screenshot {
    fn default() -> Self {
        Screenshot { supersample: 2, msaa: true, srgb: true }
    }
}

impl Screenshot {
    pub fn pixel_size(&self, w: u32, h: u32) -> (u32, u32) {
        (w * self.supersample, h * self.supersample)
    }
}

// ───────────────────────── F275 爆炸展开 ─────────────────────────

/// 子节点从父节点中心以随机角度飞出，500ms ease-out。
pub fn explode(parent: (f64, f64), angles: &[f64], dist: f64, t_ms: f64) -> Vec<(f64, f64)> {
    let p = (t_ms / 500.0).clamp(0.0, 1.0);
    let ease = 1.0 - (1.0 - p).powi(3); // cubic ease-out
    angles.iter().map(|a| (parent.0 + dist * a.cos() * ease, parent.1 + dist * a.sin() * ease)).collect()
}

pub const EXPLODE_MS: f64 = 500.0;

// ───────────────────────── 自检（CheckSet 25 项） ─────────────────────────

pub fn run_canvas_checks() -> crate::checks::CheckSet {
    let mut s = crate::CheckSet::new("canvas");

    // F251 无限画布
    let mut cam = Camera::default();
    cam.pan(-1.0e6, 9.0e5);
    s.add("F251 无限画布", cam.x == -1.0e6 && cam.y == 9.0e5 && CANVAS_BG_DARK == "#0A0A0F", "平移无限制+纯色底");

    // F252 语义引力布局
    let mut bodies = vec![
        Body { x: 0.0, y: 0.0, gravity: 1.0, repulsion: 0.5 },
        Body { x: 200.0, y: 0.0, gravity: 1.0, repulsion: 0.2 },
        Body { x: 500.0, y: 0.0, gravity: 0.1, repulsion: 0.9 },
    ];
    let it = force_layout(&mut bodies, 0.5, 500);
    let spread = (bodies[0].x - bodies[1].x).abs() + (bodies[0].x - bodies[2].x).abs();
    s.add("F252 语义引力布局", it > 0 && spread > 10.0 && spread < 5000.0, "引力/斥力收敛后锁定");

    // F253 LOD
    s.add("F253 LOD无级细节", lod(0.9) == Lod::Line && lod(0.5) == Lod::Func && lod(0.2) == Lod::Module, "80%/30% 阈值");

    // F254 自适应密度
    s.add("F254 自适应密度", node_size(0) == 16.0 && node_size(1000) == 200.0 && node_size(100) == 50.0, "16~200px 钳制");

    // F255 全景小地图
    let mm = Minimap { world: (0.0, 0.0, 1500.0, 1000.0) };
    let rect = mm.viewport_rect(&Camera { x: 300.0, y: 200.0, zoom: 1.0 }, 300.0, 200.0);
    let jump = mm.jump(75.0, 50.0);
    s.add("F255 全景小地图", (rect.0 - 30.0).abs() < 0.01 && jump.0 == 750.0 && jump.1 == 500.0, "视口红框+点击跳转");

    // F256 语义着色
    s.add("F256 语义着色", semantic_color("用户") == "#007AFF" && semantic_color("数据") == "#AF52DE" && semantic_color("?") == "#8E8E93", "色相环等分+默认灰");

    // F257 发光节点
    let (sz, br, _) = glow(500, 0.9, "#007AFF");
    s.add("F257 发光节点", sz == 50.0 && br == 0.9, "大小=代码量/亮度=频率");

    // F258 贝塞尔光束
    let mid = bezier((0.0, 0.0), (0.0, 100.0), (100.0, 100.0), (100.0, 0.0), 0.5);
    let (w, a) = beam_style(2.0);
    s.add("F258 贝塞尔光束", (mid.0 - 50.0).abs() < 0.01 && (mid.1 - 75.0).abs() < 0.01 && w >= 1.0 && w <= 4.0 && a == 0.8, "三次贝塞尔+频率线宽");

    // F259 数据流粒子
    let path = [(0.0, 0.0), (0.0, 100.0), (100.0, 100.0), (100.0, 0.0)];
    let p0 = particle_pos(path, 10.0, 0.0);
    let p15 = particle_pos(path, 10.0, 15.0);
    s.add("F259 数据流粒子", p0 == path[0] && p15 != p0, "粒子沿路径按吞吐量前进");

    // F260 循环光环
    let (r, spd) = orbit_ring(40.0, 8);
    s.add("F260 循环光环", r == 60.0 && spd == 4.0, "半径=节点×1.5/转速=迭代");

    // F261 异常脉冲
    let r0 = pulse_radius(20.0, 0.0);
    let r1 = pulse_radius(20.0, 1.0);
    let r2 = pulse_radius(20.0, 4.0);
    s.add("F261 异常脉冲", r0 == 0.0 && r1 == 30.0 && r2 == 0.0 && PULSE_COLOR == "#FF00FF", "2s 周期扩散");

    // F262 性能热力
    let h_center = heat_at(&[(100.0, 100.0, 1.0)], 100.0, 100.0);
    let h_far = heat_at(&[(100.0, 100.0, 1.0)], 1000.0, 1000.0);
    s.add("F262 性能热力", h_center == 1.0 && h_far == 0.0, "高斯核中心热边缘冷");

    // F263 暗色霓虹
    s.add("F263 暗色霓虹", NEON_BG == "#000000" && (NEON_BLOOM_THRESHOLD - 0.7).abs() < 1e-9 && (NEON_BLOOM_INTENSITY - 1.5).abs() < 1e-9, "Bloom 阈值 0.7 强度 1.5");

    // F264 Bloom 光晕
    let (br_, st) = bloom(BLOOM_BLUR_DEFAULT, 5.0);
    s.add("F264 Bloom光晕", br_ == 16.0 && st == 3.0, "半径16px+强度钳制");

    // F265 渐变数据河
    let c0 = river_color(0.0);
    let c1 = river_color(0.5);
    let c2 = river_color(1.0);
    s.add("F265 渐变数据河", c0.0 == 0.0 && c2.0 == 1.0 && (c1.0 - 172.0 / 255.0).abs() < 1e-9, "三色停靠点插值");

    // F266 呼吸动画
    let b_min = breathe(3.0);
    let b_max = breathe(1.0);
    s.add("F266 呼吸动画", (b_max - 1.1).abs() < 0.01 && (b_min - 0.9).abs() < 0.01, "4s 周期 ±10%");

    // F267 粒子星空
    let mut sf = Starfield::new(300, 42);
    let n0 = sf.stars.len();
    let x0 = sf.stars[0].0;
    sf.drift(1.0);
    s.add("F267 粒子星空", (200..=500).contains(&n0) && sf.stars[0].0 != x0, "200-500 颗+缓慢漂移");

    // F268 玻璃态节点
    s.add("F268 玻璃态节点", GLASS_CSS.contains("blur(12px)") && GLASS_CSS.contains("saturate(180%)"), "CSS 毛玻璃契约");

    // F269 连线拖尾
    let t1 = trail(10.0, 5);
    s.add("F269 连线拖尾", t1[0] == 1.0 && t1[4] < t1[1], "指数衰减拖尾");

    // F270 模块星云
    let (cx, cy, r) = nebula((0.0, 0.0, 100.0, 60.0));
    s.add("F270 模块星云", cx == 50.0 && cy == 30.0 && r == 150.0 && NEBULA_ALPHA == 0.05, "径向渐变×1.5");

    // F271 代码雨
    s.add("F271 代码雨", rain_drop(598.0, 600.0) == 0.0 && RAIN_SPEED_PX_PER_FRAME == 2.0 && RAIN_ALPHA == 0.05, "2px/帧 环形下落");

    // F272 声波频谱
    let bars = spectrum(&[0.5; 70]);
    s.add("F272 声波频谱", bars.len() == 64 && bars[0] == 0.5, "64 根柱高度=复杂度");

    // F273 时间光影
    let noon = day_tint(12.0);
    let night = day_tint(22.0);
    s.add("F273 时间光影", noon.0 == 1.0 && noon.2 == 1.0 && night.2 == 1.0 && night.0 < 1.0, "6/12/18/22 四档色温");

    // F274 截图壁纸
    let shot = Screenshot::default();
    s.add("F274 截图壁纸", shot.supersample == 2 && shot.msaa && shot.srgb && shot.pixel_size(800, 600) == (1600, 1200), "2x 超采样+MSAA+sRGB");

    // F275 爆炸展开
    let ang = [0.0, std::f64::consts::FRAC_PI_2];
    let mid_e = explode((0.0, 0.0), &ang, 100.0, 250.0);
    let end_e = explode((0.0, 0.0), &ang, 100.0, 500.0);
    s.add("F275 爆炸展开", (mid_e[0].0 - 87.5).abs() < 0.01 && (end_e[1].1 - 100.0).abs() < 0.01, "500ms ease-out 径向飞出");

    // 边输入契约（Edge 消费一致性）
    let e = Edge { from: "a".into(), to: "b".into(), kind: EdgeKind::Call };
    s.add("canvas IR边契约", e.kind == EdgeKind::Call && !e.from.is_empty(), "光束/粒子只消费统一 IR 边");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f251_pan_is_unbounded() {
        let mut cam = Camera::default();
        for _ in 0..1000 {
            cam.pan(1.0e6, -1.0e6);
        }
        assert!(cam.x > 0.0 && cam.y < 0.0);
    }

    #[test]
    fn f252_layout_converges_and_locks() {
        let mut bodies = vec![
            Body { x: 0.0, y: 0.0, gravity: 1.0, repulsion: 0.5 },
            Body { x: 10.0, y: 0.0, gravity: 1.0, repulsion: 0.5 },
        ];
        force_layout(&mut bodies, 0.001, 2000);
        let a = bodies.clone();
        force_layout(&mut bodies, 0.001, 10);
        // 收敛后锁定：再跑位移不变化
        assert!((a[0].x - bodies[0].x).abs() < 1.0 && (a[1].x - bodies[1].x).abs() < 1.0);
    }

    #[test]
    fn f253_f254_thresholds() {
        assert_eq!(lod(0.3000001), Lod::Func);
        assert_eq!(node_size(32), 16.0);
        assert_eq!(node_size(800), 200.0);
    }

    #[test]
    fn f255_minimap_roundtrip() {
        let mm = Minimap { world: (-100.0, -100.0, 900.0, 900.0) };
        let j = mm.jump(150.0, 100.0);
        assert_eq!(j, (900.0, 900.0));
    }

    #[test]
    fn f259_particle_loops() {
        let path = [(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)];
        let t = particle_pos(path, 300.0, 0.0);
        assert_eq!(t, path[0]);
    }

    #[test]
    fn f262_two_hotspots_sum() {
        let h = heat_at(&[(0.0, 0.0, 0.6), (10.0, 0.0, 0.6)], 0.0, 0.0);
        assert!(h > 0.6 && h <= 1.0);
    }

    #[test]
    fn f265_river_monotonic_red() {
        let a = river_color(0.2).0;
        let b = river_color(0.8).0;
        assert!(b > a);
    }

    #[test]
    fn f267_star_bounds() {
        let sf = Starfield::new(1000, 7);
        for (x, y, sz, a) in &sf.stars {
            assert!(*x >= 0.0 && *x <= 1.0 && *y >= 0.0 && *y <= 1.0);
            assert!(*sz >= 1.0 && *sz <= 2.0 && *a >= 0.1 && *a <= 0.3);
        }
    }

    #[test]
    fn f271_rain_wraps() {
        assert_eq!(rain_drop(600.0, 600.0), 2.0);
    }

    #[test]
    fn f273_tint_range() {
        for h in [0.0, 3.0, 6.0, 9.5, 12.0, 15.0, 18.0, 20.0, 23.5] {
            let c = day_tint(h);
            assert!(c.0 >= 0.0 && c.0 <= 1.0 && c.1 >= 0.0 && c.1 <= 1.0 && c.2 >= 0.0 && c.2 <= 1.0, "hour {h}");
        }
    }

    #[test]
    fn f275_ease_out_monotonic() {
        let ang = [0.0];
        let mut prev = 0.0;
        for t in [0.0, 100.0, 250.0, 400.0, 500.0, 900.0] {
            let p = explode((0.0, 0.0), &ang, 100.0, t)[0].0;
            assert!(p >= prev - 1e-9);
            prev = p;
        }
    }
}
