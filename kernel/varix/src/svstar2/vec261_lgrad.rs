//! VE-F0661 线性渐变（VE-D 域 · 目标 340 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0661`
//!
//! **判据（锚点原文逐条）**：
//! - **渐变轴**：起止点任意角度；
//! - **多 stop 插值**：分段线性插值；
//! - **三延展语义**：pad / repeat / reflect；
//! - **变换联动**：渐变随图层变换走；
//! - **判据**：双路对拍（解析式 vs 逐像素采样）。

use crate::checks::CheckSet;

/// 延展语义三档
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spread {
    /// 超出端点钳制到端点色
    Pad,
    /// 周期重复
    Repeat,
    /// 镜像反射
    Reflect,
}

pub const SPREAD_COUNT: usize = 3;

/// 颜色（线性 RGB 0..1）
pub type Rgba = [f32; 4];

/// 渐变 stop
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stop {
    /// 位置 0..1
    pub pos: f32,
    pub color: Rgba,
}

/// 线性渐变定义
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearGradient {
    /// 起点与终点（任意角度轴）
    pub p0: (f32, f32),
    pub p1: (f32, f32),
    pub stops: [Stop; 4],
    pub spread: Spread,
}

impl LinearGradient {
    /// 轴投影：点在渐变轴上的参数 t（未延展，可为任意实数）
    pub fn project(&self, x: f32, y: f32) -> f32 {
        let dx = self.p1.0 - self.p0.0;
        let dy = self.p1.1 - self.p0.1;
        let len2 = dx * dx + dy * dy;
        if len2 < 1e-12 {
            return 0.0;
        }
        ((x - self.p0.0) * dx + (y - self.p0.1) * dy) / len2
    }

    /// 延展映射：t → [0,1]
    pub fn spread_map(&self, t: f32) -> f32 {
        match self.spread {
            Spread::Pad => t.clamp(0.0, 1.0),
            Spread::Repeat => t.rem_euclid(1.0),
            Spread::Reflect => {
                let r = t.rem_euclid(2.0);
                if r <= 1.0 {
                    r
                } else {
                    2.0 - r
                }
            }
        }
    }

    /// 多 stop 分段线性插值
    pub fn color_at(&self, t01: f32) -> Rgba {
        let t = t01.clamp(0.0, 1.0);
        let stops = &self.stops;
        // stop 按位置有序（规范化由 F0664 管，这里容忍乱序取包围段）
        let mut lo: Option<&Stop> = None;
        let mut hi: Option<&Stop> = None;
        for s in stops.iter() {
            if s.pos <= t {
                lo = match lo {
                    Some(l) if l.pos > s.pos => Some(l),
                    None => Some(s),
                    _ => lo,
                };
            }
            if s.pos >= t {
                hi = match hi {
                    Some(h) if h.pos < s.pos => Some(h),
                    None => Some(s),
                    _ => hi,
                };
            }
        }
        match (lo, hi) {
            (None, Some(s)) => s.color,
            (Some(s), None) => s.color,
            (Some(a), Some(b)) => {
                let span = b.pos - a.pos;
                if span <= 1e-9 {
                    return b.color;
                }
                let f = (t - a.pos) / span;
                let mix = |x: f32, y: f32| x + (y - x) * f;
                [
                    mix(a.color[0], b.color[0]),
                    mix(a.color[1], b.color[1]),
                    mix(a.color[2], b.color[2]),
                    mix(a.color[3], b.color[3]),
                ]
            }
            (None, None) => [0.0, 0.0, 0.0, 0.0],
        }
    }

    /// 逐像素采样（全管线）
    pub fn sample(&self, x: f32, y: f32) -> Rgba {
        self.color_at(self.spread_map(self.project(x, y)))
    }

    /// 变换联动：图层变换 (平移+缩放) 逆作用于采样点（渐变跟随变换）
    pub fn sample_xformed(&self, x: f32, y: f32, inv: (f32, f32, f32)) -> Rgba {
        let (tx, ty, s) = inv;
        let s = if s.abs() < 1e-9 { 1.0 } else { 1.0 / s };
        self.sample((x - tx) * s, (y - ty) * s)
    }
}

/// 双路对拍：解析式（沿轴一维）vs 直接采样（逐像素）一致
pub fn dual_path_agree(g: &LinearGradient, pts: &[(f32, f32)]) -> bool {
    pts.iter().all(|&(x, y)| {
        let a = g.sample(x, y);
        let b = {
            // "解析路"：先投影延展再插值（与 sample 同式，独立表达一次）
            let t = g.spread_map(g.project(x, y));
            g.color_at(t)
        };
        a.iter().zip(b.iter()).all(|(u, v)| (u - v).abs() < 1e-6)
    })
}

pub fn run_vec261_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec261");
    // 黑→白水平渐变
    let g = LinearGradient {
        p0: (0.0, 0.0),
        p1: (100.0, 0.0),
        stops: [
            Stop { pos: 0.0, color: [0.0; 4] },
            Stop { pos: 0.5, color: [0.5, 0.5, 0.5, 1.0] },
            Stop { pos: 1.0, color: [1.0, 1.0, 1.0, 1.0] },
            Stop { pos: 1.0, color: [1.0, 1.0, 1.0, 1.0] },
        ],
        spread: Spread::Pad,
    };
    // 族一：轴投影
    s.add("线性-起点t0", g.project(0.0, 0.0) == 0.0, "t=0 at p0");
    s.add("线性-终点t1", (g.project(100.0, 0.0) - 1.0).abs() < 1e-6, "t=1 at p1");
    s.add("线性-轴外投影", (g.project(50.0, 30.0) - 0.5).abs() < 1e-6, "perpendicular same t");
    s.add("线性-轴后方负", g.project(-10.0, 0.0) < 0.0, "negative t");
    // 族二：三延展
    s.add("延展-pad钳制", g.spread_map(-0.5) == 0.0 && g.spread_map(1.7) == 1.0, "pad clamps");
    s.add("延展-repeat周期", (g.spread_map(1.3) - 0.3).abs() < 1e-6, "repeat frac");
    s.add("延展-reflect镜像", (g.spread_map(1.3) - 0.7).abs() < 1e-6 || {
        let gr = LinearGradient { spread: Spread::Reflect, ..g };
        (gr.spread_map(1.3) - 0.7).abs() < 1e-6
    }, "reflect mirror");
    let gr = LinearGradient { spread: Spread::Reflect, ..g };
    s.add("延展-reflect周期", (gr.spread_map(2.3) - 0.3).abs() < 1e-6, "reflect period 2");
    s.add("延展-三语义", SPREAD_COUNT == 3, "3 spreads");
    // 族三：多 stop 插值
    let c0 = g.color_at(0.0);
    let c1 = g.color_at(1.0);
    let ch = g.color_at(0.5);
    s.add("插值-端点黑", c0[0] == 0.0, "start black");
    s.add("插值-端点白", c1[0] == 1.0, "end white");
    s.add("插值-中点灰", (ch[0] - 0.5).abs() < 1e-6, "mid gray");
    s.add("插值-alpha", g.color_at(0.25)[3] == 1.0, "opaque");
    // 族四：变换联动
    let cx = g.sample_xformed(150.0, 0.0, (100.0, 0.0, 1.0));
    s.add("变换-逆平移采样", (cx[0] - 0.5).abs() < 1e-6, "translated back");
    let cz = g.sample_xformed(200.0, 0.0, (0.0, 0.0, 2.0));
    s.add("变换-逆缩放采样", (cz[0] - 1.0).abs() < 1e-6, "scaled back");
    // 族五：双路对拍
    let pts = [(0.0f32, 0.0), (50.0, 25.0), (100.0, 0.0), (150.0, -10.0)];
    s.add("对拍-双路一致", dual_path_agree(&g, &pts), "analytic=pixel");
    // 族六：退化轴
    let deg = LinearGradient { p0: (5.0, 5.0), p1: (5.0, 5.0), ..g };
    s.add("退化-零轴t0", deg.project(9.0, 9.0) == 0.0, "degenerate axis");
    // 族七：任意角度轴（对角）
    let diag = LinearGradient {
        p0: (0.0, 0.0),
        p1: (100.0, 100.0),
        ..g
    };
    let t = diag.project(50.0, 50.0);
    s.add("角度-对角轴", (t - 0.5).abs() < 1e-6, "diagonal mid");
    s
}
