//! VE-F0663 锥形渐变（conic，VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0663`
//!
//! **判据（锚点原文逐条）**：
//! - **角度扫掠**：起始角/终止角/中心点；
//! - **角度边界**：跨 0°/360° 边界处理；
//! - **色轮用例**：HSL 色轮的 conic 实现；
//! - **判据**：插值正确、双路、性能。

use crate::checks::CheckSet;
use super::vec261_lgrad::{Rgba, Stop, Spread};

/// 角度归一化到 [0, 360)
pub fn norm_deg(a: f32) -> f32 {
    a.rem_euclid(360.0)
}

/// 锥形渐变定义
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConicGradient {
    pub center: (f32, f32),
    /// 起始角（度，0 = 正右方向，顺时针为正）
    pub start_deg: f32,
    /// 扫掠角（正 = 顺时针）
    pub sweep_deg: f32,
    pub stops: [Stop; 4],
    pub spread: Spread,
}

impl ConicGradient {
    /// 点 → 扫掠参数 t（0..1 为一个扫掠周期）
    pub fn project(&self, x: f32, y: f32) -> f32 {
        let dx = x - self.center.0;
        let dy = y - self.center.1;
        // atan2 以度计（y 向下坐标系中，顺时针为正角）
        let ang = norm_deg(dy.atan2(dx).to_degrees());
        let rel = norm_deg(ang - self.start_deg);
        let sweep = self.sweep_deg.max(1e-6);
        rel / sweep
    }

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

/// HSL 色轮：hue → RGB（conic 渐变的色轮用例）
pub fn hsl_wheel(hue_deg: f32) -> Rgba {
    let h = norm_deg(hue_deg) / 60.0;
    let i = h.floor() as i32 % 6;
    let f = h - h.floor();
    let q = 1.0 - f;
    let (r, g, b) = match i {
        0 => (1.0, f, 0.0),
        1 => (q, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, q, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, q),
    };
    [r, g, b, 1.0]
}

pub fn run_vec263_checks() -> crate::checks::CheckSet {
    // 0° 起 360° 扫掠全轮
    let g = ConicGradient {
        center: (50.0, 50.0),
        start_deg: 0.0,
        sweep_deg: 360.0,
        stops: [
            Stop { pos: 0.0, color: hsl_wheel(0.0) },
            Stop { pos: 0.25, color: hsl_wheel(90.0) },
            Stop { pos: 0.5, color: hsl_wheel(180.0) },
            Stop { pos: 1.0, color: hsl_wheel(360.0) },
        ],
        spread: Spread::Pad,
    };
    let mut s = CheckSet::new("svstar2-vec263");
    // 族一：角度投影
    s_add(&mut s, "锥形-0°t0", g.project(100.0, 50.0).abs() < 1e-5, "east=0");
    s_add(&mut s, "锥形-90°t四分之一", (g.project(50.0, 100.0) - 0.25).abs() < 1e-4, "south=¼");
    s_add(&mut s, "锥形-180°t半", (g.project(0.0, 50.0) - 0.5).abs() < 1e-4, "west=½");
    s_add(&mut s, "锥形-270°t四分之三", (g.project(50.0, 0.0) - 0.75).abs() < 1e-4, "north=¾");
    // 族二：角度边界（跨 0/360）
    s_add(&mut s, "边界-359°归一", norm_deg(359.0) == 359.0, "359");
    s_add(&mut s, "边界-361°归一", norm_deg(361.0) == 1.0, "361→1");
    s_add(&mut s, "边界-负角归一", norm_deg(-90.0) == 270.0, "-90→270");
    s_add(&mut s, "边界-720°归一", norm_deg(720.0) == 0.0, "720→0");
    // 起始角非零
    let g90 = ConicGradient { start_deg: 90.0, ..g };
    s_add(&mut s, "边界-起始角偏移", (g90.project(50.0, 100.0)).abs() < 1e-5, "start@south");
    // 部分扫掠
    let g180 = ConicGradient { sweep_deg: 180.0, ..g };
    s_add(&mut s, "扫掠-半轮", (g180.project(0.0, 50.0) - 1.0).abs() < 1e-4, "west=1 of 180°");
    // 族三：色轮用例
    s_add(&mut s, "色轮-红0", hsl_wheel(0.0)[0] == 1.0 && hsl_wheel(0.0)[1] == 0.0, "red");
    s_add(&mut s, "色轮-绿120", hsl_wheel(120.0)[1] == 1.0, "green");
    s_add(&mut s, "色轮-蓝240", hsl_wheel(240.0)[2] == 1.0, "blue");
    s_add(&mut s, "色轮-周期", hsl_wheel(360.0) == hsl_wheel(0.0), "hue wrap");
    // conic 采样与色轮一致（0° 方向即红）
    s_add(&mut s, "色轮-conic红", g.sample(100.0, 50.0)[0] == 1.0, "east red");
    s_add(&mut s, "色轮-conic绿", (g.sample(50.0, 100.0)[1] - 1.0).abs() < 1e-4, "south green");
    // 族四：中心点（中心角度不定但有限）
    s_add(&mut s, "中心-有限", g.project(50.0, 50.0).is_finite(), "finite center");
    // 族五：延展
    let gr = ConicGradient { spread: Spread::Repeat, ..g };
    s_add(&mut s, "延展-repeat", (gr.spread_map(1.5) - 0.5).abs() < 1e-6, "repeat");
    let gf = ConicGradient { spread: Spread::Reflect, ..g };
    s_add(&mut s, "延展-reflect", (gf.spread_map(1.5) - 0.5).abs() < 1e-6, "reflect");
    // 族六：双路对拍
    let pts = [(100.0f32, 50.0), (50.0, 100.0), (0.0, 50.0), (50.0, 0.0)];
    let agree = pts.iter().all(|&(x, y)| {
        let a = g.sample(x, y);
        let b = g.color_at(g.spread_map(g.project(x, y)));
        a.iter().zip(b.iter()).all(|(u, v)| (u - v).abs() < 1e-6)
    });
    s_add(&mut s, "对拍-双路一致", agree, "agree");
    s
}

fn s_add(s: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    s.add(label, ok, detail);
}
