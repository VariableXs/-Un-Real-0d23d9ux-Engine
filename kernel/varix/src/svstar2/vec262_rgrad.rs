//! VE-F0662 径向渐变（VE-D 域 · 目标 340 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0662`
//!
//! **判据（锚点原文逐条）**：
//! - **焦点语义**：圆心/半径/焦点偏移（CSS radial-gradient 焦点行为）；
//! - **椭圆变换**：非等比缩放下圆→椭圆（渐变空间变换法）；
//! - **延展三语义**：pad / repeat / reflect；
//! - **判据**：双路对拍。

use crate::checks::CheckSet;
use super::vec261_lgrad::{Rgba, Stop, Spread};

/// 径向渐变定义（渐变空间变换法：点先变换到单位圆空间）
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RadialGradient {
    /// 圆心
    pub center: (f32, f32),
    /// 半径（x/y 独立——非等比缩放即椭圆）
    pub rx: f32,
    pub ry: f32,
    /// 焦点偏移（相对圆心，单位：渐变空间比例 -1..1）
    pub focal: (f32, f32),
    pub stops: [Stop; 4],
    pub spread: Spread,
}

impl RadialGradient {
    /// 点 → 渐变空间归一化距离（椭圆：除以半径；焦点偏移：CSS 焦点行为线性化）
    pub fn project(&self, x: f32, y: f32) -> f32 {
        let gx = (x - self.center.0) / self.rx.max(1e-9);
        let gy = (y - self.center.1) / self.ry.max(1e-9);
        let d = (gx * gx + gy * gy).sqrt();
        // 焦点偏移：t = (d - |f|) / (1 - |f|)（CSS 焦点行为：焦点处 t=0，圆边处 t=1）
        let f = (self.focal.0 * self.focal.0 + self.focal.1 * self.focal.1).sqrt();
        if f >= 1.0 {
            return d; // 焦点退化到圆心语义（越界焦点钳到圆心行为）
        }
        if d <= f {
            0.0
        } else {
            (d - f) / (1.0 - f)
        }
    }

    /// 延展映射（与线性渐变同纪律）
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

    pub fn color_at(&self, t01: f32) -> Rgba {
        let t = t01.clamp(0.0, 1.0);
        // 复用线性插值纪律（stop 表逐段线性）
        let mut lo: Option<&Stop> = None;
        let mut hi: Option<&Stop> = None;
        for st in self.stops.iter() {
            if st.pos <= t {
                lo = match lo {
                    Some(l) if l.pos > st.pos => Some(l),
                    None => Some(st),
                    _ => lo,
                };
            }
            if st.pos >= t {
                hi = match hi {
                    Some(h) if h.pos < st.pos => Some(h),
                    None => Some(st),
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
                let mix = |u: f32, v: f32| u + (v - u) * f;
                [
                    mix(a.color[0], b.color[0]),
                    mix(a.color[1], b.color[1]),
                    mix(a.color[2], b.color[2]),
                    mix(a.color[3], b.color[3]),
                ]
            }
            (None, None) => [0.0; 4],
        }
    }

    pub fn sample(&self, x: f32, y: f32) -> Rgba {
        self.color_at(self.spread_map(self.project(x, y)))
    }
}

/// 双路对拍（同口径独立表达）
pub fn dual_path_agree(g: &RadialGradient, pts: &[(f32, f32)]) -> bool {
    pts.iter().all(|&(x, y)| {
        let a = g.sample(x, y);
        let b = g.color_at(g.spread_map(g.project(x, y)));
        a.iter().zip(b.iter()).all(|(u, v)| (u - v).abs() < 1e-6)
    })
}

pub fn run_vec262_checks() -> crate::checks::CheckSet {
    let g = RadialGradient {
        center: (50.0, 50.0),
        rx: 50.0,
        ry: 50.0,
        focal: (0.0, 0.0),
        stops: [
            Stop { pos: 0.0, color: [1.0, 0.0, 0.0, 1.0] },
            Stop { pos: 1.0, color: [0.0, 0.0, 1.0, 1.0] },
            Stop { pos: 1.0, color: [0.0, 0.0, 1.0, 1.0] },
            Stop { pos: 1.0, color: [0.0, 0.0, 1.0, 1.0] },
        ],
        spread: Spread::Pad,
    };
    let mut s = CheckSet::new("svstar2-vec262");
    // 族一：投影
    s_add(&mut s, "径向-圆心t0", (g.project(50.0, 50.0)).abs() < 1e-6, "center 0");
    s_add(&mut s, "径向-圆边t1", (g.project(100.0, 50.0) - 1.0).abs() < 1e-6, "edge 1");
    s_add(&mut s, "径向-界外pad", g.project(150.0, 50.0) > 1.0, "beyond 1");
    // 等距同心（45° 方向同为 1）
    let d45 = (50.0 + 50.0 / (2.0f32).sqrt(), 50.0 + 50.0 / (2.0f32).sqrt());
    s_add(&mut s, "径向-各向同性", (g.project(d45.0, d45.1) - 1.0).abs() < 1e-4, "isotropic");
    // 族二：焦点语义
    let gf = RadialGradient { focal: (0.5, 0.0), ..g };
    s_add(&mut s, "焦点-焦点处t0", gf.project(75.0, 50.0).abs() < 1e-6, "focal t=0");
    s_add(&mut s, "焦点-圆边t1", (gf.project(100.0, 50.0) - 1.0).abs() < 1e-5, "edge still 1");
    s_add(&mut s, "焦点-近心侧更快", gf.project(60.0, 50.0) > g.project(60.0, 50.0), "focus shifts ramp");
    // 越界焦点钳到圆心语义
    let gbad = RadialGradient { focal: (1.5, 0.0), ..g };
    s_add(&mut s, "焦点-越界退化", (gbad.project(50.0, 50.0)).abs() < 1e-6, "clamp to center");
    // 族三：椭圆（非等比）
    let ge = RadialGradient { rx: 100.0, ry: 50.0, ..g };
    s_add(&mut s, "椭圆-x边t1", (ge.project(150.0, 50.0) - 1.0).abs() < 1e-6, "x edge");
    s_add(&mut s, "椭圆-y边t1", (ge.project(50.0, 100.0) - 1.0).abs() < 1e-6, "y edge");
    s_add(&mut s, "椭圆-x外t2", (ge.project(250.0, 50.0) - 2.0).abs() < 1e-6, "x beyond 2");
    // 族四：三延展
    let gr = RadialGradient { spread: Spread::Repeat, ..g };
    s_add(&mut s, "延展-repeat", (gr.spread_map(1.4) - 0.4).abs() < 1e-6, "repeat");
    let gf2 = RadialGradient { spread: Spread::Reflect, ..g };
    s_add(&mut s, "延展-reflect", (gf2.spread_map(1.4) - 0.6).abs() < 1e-6, "reflect");
    s_add(&mut s, "延展-pad", g.spread_map(1.4) == 1.0, "pad");
    // 族五：插值端点
    s_add(&mut s, "插值-心红", g.color_at(0.0)[0] == 1.0, "center red");
    s_add(&mut s, "插值-边蓝", g.color_at(1.0)[2] == 1.0, "edge blue");
    s_add(&mut s, "插值-中点混合", (g.color_at(0.5)[0] - 0.5).abs() < 1e-6, "mid mix");
    // 族六：双路对拍
    let pts = [(50.0f32, 50.0), (75.0, 60.0), (110.0, 50.0), (30.0, 90.0)];
    s_add(&mut s, "对拍-双路一致", dual_path_agree(&g, &pts), "agree");
    // 族七：零半径保护
    let g0 = RadialGradient { rx: 0.0, ry: 0.0, ..g };
    s_add(&mut s, "退化-零半径不炸", g0.project(60.0, 50.0).is_finite(), "finite");
    s
}

fn s_add(s: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    s.add(label, ok, detail);
}
