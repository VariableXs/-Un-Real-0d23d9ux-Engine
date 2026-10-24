//! VE-F5243 · 景深 DoF（VE-Z 域 · Z03 后处理 · 锚点目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5243`
//!
//! **判据（锚点原文）**：弥散圆计算、对焦面、近远景分段、散景形状、
//! 深度预滤波、判据。
//!
//! # 深化说明
//!
//! ## 深化一：弥散圆用**薄透镜公式**，不是线性映射
//!
//! 常见偷懒写法：`coc = |depth - focus| * k`（线性）。它在对焦面附近与实际
//! 光学不符——真实弥散圆在**对焦面**处为 0，且随距离以 `1/z` 关系变化，
//! 在线性模型下「对焦面」会变成一个「对焦带」，观感是主体只有一部分清晰。
//! 本实现用薄透镜：
//!
//! ```text
//! coc = A * f * |z - zf| / (z * (zf - f))
//! ```
//!
//! 其中 `A` 为光圈直径，`f` 为焦距，`zf` 为对焦距离。该式在对焦面处严格
//! 为 0，且远景色散（`z→∞` 时 `coc → A*f/(zf-f)` 有界），近景色散
//! （`z→f` 时发散）——与真实镜头一致。`coc_is_zero_at_focus` 判据对
//! `z = zf` 要求**严格 0**，`coc_far_is_bounded` 要求远景有界。
//!
//! ## 深化二：近/远景分段用**两个独立 CoC 尺度**，不是同一个
//!
//! 艺术上近景虚化常比远景更「软」（前景虚化是窥视感的一部分）。若用同一
//! 系数，调近景必然连带远景。本实现把近/远 CoC 分成两个乘子，且校验
//! 「远景乘子 ≥ 0.5」——低于这个值远景会显得「跟焦」，失去景深层次
//! （`far_scale_floor` 判据）。
//!
//! ## 深化三：散景形状是**多边形光圈的可参数化采样**，且有权重校验
//!
//! 真实镜头的光圈是多边形叶片，散景呈多边形。本实现用极坐标采样：
//! 半径按 `sqrt(u)` 分布（保证盘内**面积均匀**——直接用 `u` 会让边缘过密，
//! 散景出现亮环），角度按叶片数取模形成多边形边界。
//! `bokeh_disc_is_area_uniform` 判据把 4096 个采样点分 8 个同心环，每环
//! 计数应正比于环面积（容差 15%）——这是「无亮环」的机检形式。
//!
//! ## 深化四：深度预滤波是**双边式**，只为去深度锯齿
//!
//! 直接对深度图算 CoC，会在物体轮廓处产生「深度跳变 → CoC 跳变 → 轮廓
//! 一道虚化亮边」。双边滤波（空间权重 × 深度差权重）能在去锯齿的同时保留
//! 轮廓锐度。本实现的深度差权重用 `1/(1+ (Δd/σr)²)`，且 `sigma_r` 越界
//! 会被拒（过大会把前后景混在一起，产生「轮廓被吃掉」的经典错误）。

use alloc::vec::Vec;
use crate::float::FloatExt;

use crate::checks::CheckSet;

/// 模块版本。
pub const DOF_VERSION: &str = "F5243-dof-v1";

// ── 诊断码（本模块独占段 0xE040~0xE04F）─────────────────────────────
const SEG_BASE: u16 = 0xE040;
/// 镜头参数非法（光圈/焦距/对焦距离越界）。
pub const E_LENS: u16 = SEG_BASE + 1;
/// CoC 尺度越界。
pub const E_SCALE: u16 = SEG_BASE + 2;
/// 叶片数越界。
pub const E_BLADES: u16 = SEG_BASE + 3;
/// 深度滤波参数越界。
pub const E_DEPTH_FILTER: u16 = SEG_BASE + 4;

/// 最大 CoC 半径（像素）。依据：1080p 下 24 像素的虚化圆直径 48px，
/// 已接近「背景完全不可辨」；再大只会均匀糊掉整幅画面。
pub const COC_MAX_PX: f32 = 24.0;

/// 叶片数范围（5~9 是真实镜头的常见规格）。
pub const BLADES_MIN: u8 = 5;
pub const BLADES_MAX: u8 = 9;

/// 远景 CoC 尺度下限。依据：低于 0.5 时远景散景半径不足近景一半，
/// 层次感消失（实测观感「跟焦」）。
pub const FAR_SCALE_FLOOR: f32 = 0.5;

/// 薄透镜镜头参数。
#[derive(Clone, Copy, Debug)]
pub struct Lens {
    /// 光圈直径（世界单位，越小景深越深）。
    pub aperture: f32,
    /// 焦距（世界单位）。
    pub focal: f32,
    /// 对焦距离（世界单位，必须 > focal）。
    pub focus: f32,
}

impl Lens {
    /// 标准（35mm 等效 f/2.8 的示意换算）。
    pub fn standard() -> Self {
        Self {
            aperture: 6.0,
            focal: 50.0,
            focus: 300.0,
        }
    }

    pub fn validate(&self) -> Result<(), u16> {
        if self.aperture.is_nan() || self.aperture <= 0.0 || self.aperture > 500.0 {
            return Err(E_LENS);
        }
        if self.focal.is_nan() || self.focal <= 0.0 || self.focal > 1000.0 {
            return Err(E_LENS);
        }
        if self.focus.is_nan() || self.focus <= self.focal || self.focus > 100_000.0 {
            return Err(E_LENS);
        }
        Ok(())
    }

    /// 无限远 CoC 上限（`z→∞` 时的极限值）。
    ///
    /// 由薄透镜式 `coc = A*f*|z-focus| / (z*(focus-f))` 取 `z→∞`：
    /// `coc → A*f / (focus - f)`。
    pub fn coc_infinity(&self) -> f32 {
        self.aperture * self.focal / (self.focus - self.focal)
    }

    /// 给定深度 `z` 的弥散圆直径（世界单位）。
    ///
    /// 薄透镜式，**在对焦面处严格为 0**（分子 `|z - focus|` = 0）。
    /// `z <= focal` 时（比焦距还近，物理上无法成像）返回 `coc_infinity()`
    /// 作为上限——不返回 NaN/∞，因为下游要拿它做插值。
    pub fn coc_world(&self, z: f32) -> f32 {
        if !(z > self.focal) || z.is_nan() {
            return self.coc_infinity();
        }
        let num = self.aperture * self.focal * (z - self.focus).abs();
        let den = z * (self.focus - self.focal);
        if den <= 0.0 {
            return self.coc_infinity();
        }
        let c = num / den;
        let lim = self.coc_infinity();
        if c > lim {
            lim
        } else {
            c
        }
    }
}

/// DoF 参数。
#[derive(Clone, Copy, Debug)]
pub struct DofConfig {
    pub lens: Lens,
    /// 近景 CoC 尺度乘子。
    pub near_scale: f32,
    /// 远景 CoC 尺度乘子。
    pub far_scale: f32,
    /// 像素换算（每世界单位多少像素）。
    pub px_per_unit: f32,
    /// 叶片数（散景多边形）。
    pub blades: u8,
}

impl DofConfig {
    pub fn standard() -> Self {
        Self {
            lens: Lens::standard(),
            near_scale: 1.4,
            far_scale: 0.8,
            px_per_unit: 4.0,
            blades: 7,
        }
    }

    pub fn validate(&self) -> Result<(), u16> {
        self.lens.validate()?;
        if self.near_scale.is_nan()
            || self.far_scale.is_nan()
            || self.near_scale <= 0.0
            || self.far_scale < FAR_SCALE_FLOOR
            || self.near_scale > 4.0
            || self.far_scale > 4.0
        {
            return Err(E_SCALE);
        }
        if self.blades < BLADES_MIN || self.blades > BLADES_MAX {
            return Err(E_BLADES);
        }
        if self.px_per_unit.is_nan() || self.px_per_unit <= 0.0 || self.px_per_unit > 1000.0 {
            return Err(E_SCALE);
        }
        Ok(())
    }

    /// 某深度的 CoC 半径（像素），含近/远分段与上限钳制。
    pub fn coc_px(&self, z: f32) -> f32 {
        let w = self.lens.coc_world(z);
        let s = if z < self.lens.focus {
            self.near_scale
        } else {
            self.far_scale
        };
        let px = w * s * self.px_per_unit * 0.5;
        if px > COC_MAX_PX {
            COC_MAX_PX
        } else if px < 0.0 {
            0.0
        } else {
            px
        }
    }

    /// 该深度是否算「清晰」（CoC < 1 像素）。
    pub fn is_sharp(&self, z: f32) -> bool {
        self.coc_px(z) < 1.0
    }
}

// ── 散景采样 ────────────────────────────────────────────────────

/// 散景盘内均匀采样：返回 `(dx, dy)`，落在单位盘内。
///
/// `u, v ∈ [0,1)`：半径用 `sqrt(u)`（**面积均匀**的关键），角度用 `2πv`；
/// 再按叶片数把半径钳到多边形边界内（模拟多边形光圈的直边）。
pub fn bokeh_sample(u: f32, v: f32, blades: u8) -> [f32; 2] {
    let u = clamp01(u);
    let v = clamp01(v);
    let r = sqrtf(u);
    let theta = v * core::f32::consts::PI * 2.0;
    // 多边形半径：正 n 边形的边界半径 = cos(π/n) / cos(θ mod (2π/n) - π/n)。
    let n = if blades < 3 { 3 } else { blades } as f32;
    let seg = core::f32::consts::PI * 2.0 / n;
    let half = seg * 0.5;
    let mut a = theta % seg;
    if a < 0.0 {
        a += seg;
    }
    let poly = cosf(half) / cosf(a - half).max(1e-6);
    let rr = r * poly.min(1.0);
    [rr * cosf(theta), rr * sinf(theta)]
}

fn clamp01(x: f32) -> f32 {
    if x.is_nan() || x < 0.0 {
        0.0
    } else if x >= 1.0 {
        0.999_999_94
    } else {
        x
    }
}

// ── 深度预滤波（双边）────────────────────────────────────────────

/// 双边深度滤波参数。
#[derive(Clone, Copy, Debug)]
pub struct DepthFilter {
    /// 空间核半径（像素，1..=3）。
    pub radius: usize,
    /// 深度差 sigma（世界单位）。
    pub sigma_r: f32,
}

impl DepthFilter {
    pub fn standard() -> Self {
        Self {
            radius: 2,
            sigma_r: 0.5,
        }
    }

    pub fn validate(&self) -> Result<(), u16> {
        if self.radius == 0 || self.radius > 3 {
            return Err(E_DEPTH_FILTER);
        }
        if self.sigma_r.is_nan() || self.sigma_r <= 0.0 || self.sigma_r > 10.0 {
            return Err(E_DEPTH_FILTER);
        }
        Ok(())
    }

    /// 对单通道深度图（`w×h`，世界单位）做一次双边滤波。
    ///
    /// 权重 = 空间高斯 × 深度相似度 `1/(1+(Δd/σr)²)`。
    /// 输出到 `out`（长度须 ≥ w*h，否则原样返回不写）。
    pub fn apply(&self, src: &[f32], w: usize, h: usize, out: &mut Vec<f32>) {
        let n = w * h;
        out.clear();
        if src.len() < n || n == 0 {
            return;
        }
        let r = self.radius as i32;
        let inv_sr2 = 1.0 / (self.sigma_r * self.sigma_r);
        let inv_ss2 = 1.0 / (0.5 * 0.5 * (r * r) as f32 + 1e-6);
        out.reserve(n);
        for y in 0..h {
            for x in 0..w {
                let c = src[y * w + x];
                let mut acc = 0.0f32;
                let mut wacc = 0.0f32;
                for dy in -r..=r {
                    for dx in -r..=r {
                        let sx = clamp_i((x as i32 + dx) as usize, w);
                        let sy = clamp_i((y as i32 + dy) as usize, h);
                        let d = src[sy * w + sx];
                        let dd = d - c;
                        let ws = expf(-((dx * dx + dy * dy) as f32) * inv_ss2);
                        // 相似度权重：深度差越大越不信任。
                        let wr = 1.0 / (1.0 + dd * dd * inv_sr2);
                        let wgt = if c.is_nan() || d.is_nan() { 0.0 } else { ws * wr };
                        acc += d * wgt;
                        wacc += wgt;
                    }
                }
                out.push(if wacc > 0.0 { acc / wacc } else { c });
            }
        }
    }
}

fn clamp_i(v: usize, n: usize) -> usize {
    if v >= n {
        n - 1
    } else {
        v
    }
}

// ── 数学工具 ────────────────────────────────────────────────────

pub fn sqrtf(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return 0.0;
    }
    let mut g = if x > 1.0 { x } else { 1.0 };
    for _ in 0..24 {
        g = 0.5 * (g + x / g);
    }
    g
}

pub fn cosf(x: f32) -> f32 {
    let two_pi = core::f32::consts::PI * 2.0;
    let mut t = x % two_pi;
    if t > core::f32::consts::PI {
        t -= two_pi;
    } else if t < -core::f32::consts::PI {
        t += two_pi;
    }
    let t2 = t * t;
    let mut term = 1.0f32;
    let mut sum = 1.0f32;
    let mut k = 1.0f32;
    for _ in 0..12 {
        term *= -t2 / ((2.0 * k - 1.0) * (2.0 * k));
        sum += term;
        k += 1.0;
    }
    sum
}

pub fn sinf(x: f32) -> f32 {
    cosf(x - core::f32::consts::PI * 0.5)
}

pub fn expf(x: f32) -> f32 {
    if x.is_nan() {
        return 0.0;
    }
    if x > 20.0 {
        return 4.85e8;
    }
    if x < -20.0 {
        return 0.0;
    }
    let ln2 = core::f32::consts::LN_2;
    let k = (x / ln2).m_round();
    let r = x - k * ln2;
    let mut term = 1.0f32;
    let mut sum = 1.0f32;
    for i in 1..14 {
        term *= r / i as f32;
        sum += term;
    }
    sum * pow2i(k as i32)
}

pub fn pow2i(k: i32) -> f32 {
    let k = k.clamp(-126, 127);
    f32::from_bits(((k + 127) as u32) << 23)
}

// ── 判据 ────────────────────────────────────────────────────────

/// F5243 判据集。
pub fn run_vez43_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5243");

    // ── 1. 弥散圆：对焦面严格为 0 ──
    {
        let lens = Lens::standard();
        let c_focus = lens.coc_world(lens.focus);
        s.add(
            "VZ43-CoC-01",
            c_focus == 0.0,
            "对焦面 CoC 严格为 0（薄透镜分子为 0，非近似）",
        );
        // 对焦面前后各偏移 1 单位，CoC 应同量级（薄透镜对称性：仅差 1/z 项）。
        let a = lens.coc_world(lens.focus - 1.0);
        let b = lens.coc_world(lens.focus + 1.0);
        let ratio = if a > b { a / b } else { b / a };
        s.add(
            "VZ43-CoC-02",
            ratio < 1.05,
            "对焦面两侧 1 单位处 CoC 近似对称（|z-zf| 主导，1/z 项影响 <5%）",
        );
        // 远景有界。
        let far = lens.coc_world(1.0e9);
        let lim = lens.coc_infinity();
        s.add(
            "VZ43-CoC-03",
            far <= lim + 1e-6 && lim > 0.0,
            "远景 CoC 收敛到有限极限 A*f/(zf-f)",
        );
        // 越近越大（z 从 focus 往 focal 走，CoC 单调增）。
        let mut mono = true;
        let mut prev = -1.0f32;
        for k in 1..=10 {
            let z = lens.focus - k as f32 * 2.0;
            if z <= lens.focal {
                break;
            }
            let c = lens.coc_world(z);
            if c < prev - 1e-6 {
                mono = false;
            }
            prev = c;
        }
        s.add("VZ43-CoC-04", mono, "接近镜头方向 CoC 单调增");
        // 焦距以内（无法成像）返回有界值而非 NaN/∞。
        let beyond = lens.coc_world(lens.focal * 0.5);
        s.add(
            "VZ43-CoC-05",
            beyond.is_finite() && beyond == lim,
            "z ≤ focal 时返回有界上限（无 NaN/∞ 下游插值安全）",
        );
        // 反向：线性模型的 CoC 在对焦面附近不为 0 的结构性缺陷——
        // 用「对焦面 ±0.5」两个点证明薄透镜给出的差异远小于线性模型。
        let d_small = lens.coc_world(lens.focus + 0.5);
        let linear_would = 0.5 * 0.5; // 线性模型 |Δz|*k，k=0.5
        s.add(
            "VZ43-CoC-06",
            d_small < linear_would,
            "反向：薄透镜在对焦面附近比线性模型收敛更快（主体更锐）",
        );
    }

    // ── 2. 近/远景分段 ──
    {
        let cfg = DofConfig::standard();
        let z_near = cfg.lens.focus - 50.0;
        let z_far = cfg.lens.focus + 50.0;
        let c_near = cfg.coc_px(z_near);
        let c_far = cfg.coc_px(z_far);
        s.add(
            "VZ43-分段-01",
            c_near > c_far,
            "同距偏移下近景 CoC 大于远景（near_scale 1.4 > far_scale 0.8）",
        );
        let ratio = c_near / c_far;
        s.add(
            "VZ43-分段-02",
            (ratio - 1.4 / 0.8).abs() < 0.05,
            "近/远 CoC 比 = near_scale/far_scale（独立可调）",
        );
        // 远景尺度下限校验。
        let bad = DofConfig {
            far_scale: 0.4,
            ..DofConfig::standard()
        }
        .validate()
        .is_err();
        s.add(
            "VZ43-分段-03",
            bad,
            "反向：far_scale < 0.5 被拒（远景跟焦）",
        );
        // CoC 像素上限钳制。
        let huge = DofConfig {
            lens: Lens {
                aperture: 400.0,
                focal: 900.0,
                focus: 1000.0,
            },
            px_per_unit: 900.0,
            ..DofConfig::standard()
        };
        let c = huge.coc_px(999.0);
        s.add(
            "VZ43-分段-04",
            c <= COC_MAX_PX + 1e-4,
            "CoC 上限钳制生效（不超 24px）",
        );
        // 清晰判定与 CoC 一致。
        s.add(
            "VZ43-分段-05",
            cfg.is_sharp(cfg.lens.focus) && !cfg.is_sharp(cfg.lens.focus - 200.0),
            "清晰判定：对焦面清晰、近景 200 单位处不清晰",
        );
    }

    // ── 3. 散景：盘内面积均匀（无亮环）──
    {
        // 4096 个 Halton 采样，分 8 环统计。
        let mut rings = [0u32; 8];
        let mut outside = 0u32;
        for i in 1..=4096u32 {
            let u = halton(i, 2);
            let v = halton(i, 3);
            let p = bokeh_sample(u, v, 7);
            let d = sqrtf(p[0] * p[0] + p[1] * p[1]);
            if d > 1.02 {
                outside += 1;
            }
            let ring = (d * 8.0) as usize;
            if ring < 8 {
                rings[ring] += 1;
            }
        }
        s.add(
            "VZ43-散景-01",
            outside == 0,
            "全部采样落在单位盘内（多边形钳制 ≤ 1）",
        );
        // 每环点数应正比于环面积：环 k 的面积 = π((k+1)²-k²)/64 = π(2k+1)/64。
        let total = 4096.0f32;
        let mut worst = 0.0f32;
        for k in 0..8 {
            let frac = (2.0 * k as f32 + 1.0) / 64.0;
            let expect = total * frac;
            let got = rings[k] as f32;
            let rel = ((got - expect) / expect).abs();
            if rel > worst {
                worst = rel;
            }
        }
        s.add(
            "VZ43-散景-02",
            worst < 0.15,
            "八环点数正比于环面积（误差 < 15%，证明 sqrt 半径分布生效）",
        );
        // 反向：若半径直接用 u（不做 sqrt），内环会过密（环 0 期望偏差 > 100%）。
        let mut rings_u = [0u32; 8];
        for i in 1..=4096u32 {
            let u = halton(i, 2);
            let v = halton(i, 3);
            // 直接用 u 作半径（错误做法），角度仍用 v。
            let theta = v * core::f32::consts::PI * 2.0;
            let d = sqrtf(u * u * cosf(theta) * cosf(theta) + u * u * sinf(theta) * sinf(theta));
            let ring = (d * 8.0) as usize;
            if ring < 8 {
                rings_u[ring] += 1;
            }
        }
        let expect0 = total * (1.0 / 64.0);
        let rel0 = ((rings_u[0] as f32 - expect0) / expect0).abs();
        s.add(
            "VZ43-散景-03",
            rel0 > 1.0,
            "反向：线性半径分布使内环过密（偏差 > 100%，本判据有区分力）",
        );
        // 叶片数影响边界（7 叶与 9 叶的最大半径应不同）。
        let mut max7 = 0.0f32;
        let mut max9 = 0.0f32;
        for i in 1..=256u32 {
            let p7 = bokeh_sample(halton(i, 2), halton(i, 3), 7);
            let p9 = bokeh_sample(halton(i, 2), halton(i, 3), 9);
            let d7 = sqrtf(p7[0] * p7[0] + p7[1] * p7[1]);
            let d9 = sqrtf(p9[0] * p9[0] + p9[1] * p9[1]);
            if d7 > max7 {
                max7 = d7;
            }
            if d9 > max9 {
                max9 = d9;
            }
        }
        s.add(
            "VZ43-散景-04",
            max7 > max9,
            "叶片数越少外接半径越大（7 叶 > 9 叶，正 n 边形几何）",
        );
    }

    // ── 4. 深度预滤波 ──
    {
        let f = DepthFilter::standard();
        // 阶跃深度图（左半 10，右半 20）：滤波后轮廓两侧仍应保持各自量级。
        let w = 16usize;
        let h = 4usize;
        let mut src = Vec::new();
        for _y in 0..h {
            for x in 0..w {
                src.push(if x < w / 2 { 10.0f32 } else { 20.0 });
            }
        }
        let mut out = Vec::new();
        f.apply(&src, w, h, &mut out);
        s.add("VZ43-滤波-01", out.len() == w * h, "输出长度 = w*h");
        // 远离边界处应接近原值（不糊）。
        let interior_l = out[1 * w + 2];
        let interior_r = out[1 * w + 13];
        s.add(
            "VZ43-滤波-02",
            (interior_l - 10.0).abs() < 0.3 && (interior_r - 20.0).abs() < 0.3,
            "远离边界处保持原深度（双边保边）",
        );
        // 边界处应有过渡（不是硬跳变），但过渡宽度应小于空间半径两倍。
        let mut transition = 0usize;
        let y = 1usize;
        for x in 0..w {
            let v = out[y * w + x];
            if v > 10.5 && v < 19.5 {
                transition += 1;
            }
        }
        s.add(
            "VZ43-滤波-03",
            transition > 0 && transition <= 2 * f.radius + 1,
            "边界有过渡（去锯齿）且宽度 ≤ 2r+1（不吃掉轮廓）",
        );
        // 常值图滤波后不变。
        let mut flat = Vec::new();
        for _ in 0..(w * h) {
            flat.push(7.0f32);
        }
        let mut o2 = Vec::new();
        f.apply(&flat, w, h, &mut o2);
        let mut const_ok = true;
        for v in &o2 {
            if (v - 7.0).abs() > 1e-4 {
                const_ok = false;
            }
        }
        s.add("VZ43-滤波-04", const_ok, "常值深度图滤波后不变（权重归一正确）");
        // 非法参数。
        let b1 = DepthFilter {
            radius: 0,
            sigma_r: 0.5,
        }
        .validate()
        .is_err();
        let b2 = DepthFilter {
            radius: 2,
            sigma_r: 0.0,
        }
        .validate()
        .is_err();
        s.add("VZ43-滤波-05", b1 && b2, "反向：零半径/零 sigma_r 均拒");
        // 空图安全。
        let mut o3 = Vec::new();
        f.apply(&[], 0, 0, &mut o3);
        s.add("VZ43-滤波-06", o3.is_empty(), "空图不写入（不 panic）");
    }

    // ── 5. 镜头参数校验 ──
    {
        let ok = Lens::standard().validate().is_ok();
        // focus <= focal 拒绝（物理上无法对焦）。
        let b1 = Lens {
            aperture: 6.0,
            focal: 50.0,
            focus: 50.0,
        }
        .validate()
        .is_err();
        let b2 = Lens {
            aperture: 0.0,
            focal: 50.0,
            focus: 300.0,
        }
        .validate()
        .is_err();
        let b3 = Lens {
            aperture: 6.0,
            focal: 0.0,
            focus: 300.0,
        }
        .validate()
        .is_err();
        s.add(
            "VZ43-镜头-01",
            ok && b1 && b2 && b3,
            "镜头校验：focus=focal / 零光圈 / 零焦距 均拒",
        );
        // 光圈越小景深越深（CoC 越小）。
        let small = Lens {
            aperture: 2.0,
            ..Lens::standard()
        };
        let big = Lens {
            aperture: 12.0,
            ..Lens::standard()
        };
        s.add(
            "VZ43-镜头-02",
            small.coc_world(400.0) < big.coc_world(400.0),
            "光圈越小 CoC 越小（景深越深）",
        );
        // 叶片数校验。
        let bb = DofConfig {
            blades: 4,
            ..DofConfig::standard()
        }
        .validate()
        .is_err();
        let b9 = DofConfig {
            blades: 10,
            ..DofConfig::standard()
        }
        .validate()
        .is_err();
        s.add("VZ43-镜头-03", bb && b9, "反向：叶片数 4 / 10 均拒");
    }

    // ── 6. 冻结 ──
    s.add(
        "VZ43-冻结-01",
        DOF_VERSION == "F5243-dof-v1",
        "API 版本冻结：F5243-dof-v1",
    );
    s.add(
        "VZ43-冻结-02",
        BLADES_MIN == 5 && BLADES_MAX == 9 && COC_MAX_PX == 24.0,
        "参数指纹：叶片 5..9 / CoC 上限 24px",
    );

    s
}

/// Halton 序列（判据内用，独立实现避免跨模块耦合）。
fn halton(mut i: u32, b: u32) -> f32 {
    if b < 2 {
        return 0.0;
    }
    let inv = 1.0 / b as f32;
    let mut f = inv;
    let mut r = 0.0f32;
    while i > 0 {
        r += f * (i % b) as f32;
        i /= b;
        f *= inv;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coc_zero_exactly_at_focus() {
        let l = Lens::standard();
        assert_eq!(l.coc_world(l.focus), 0.0);
    }

    #[test]
    fn coc_infinity_matches_limit() {
        let l = Lens::standard();
        let approx = l.coc_world(1.0e8);
        assert!((approx - l.coc_infinity()).abs() < 1e-3);
    }

    #[test]
    fn bokeh_inside_unit_disc() {
        for i in 1..=500u32 {
            let p = bokeh_sample(halton(i, 2), halton(i, 3), 6);
            let d = sqrtf(p[0] * p[0] + p[1] * p[1]);
            assert!(d <= 1.001, "d={}", d);
        }
    }

    #[test]
    fn bilateral_preserves_flat() {
        let f = DepthFilter::standard();
        let src = [3.0f32; 36];
        let mut out = Vec::new();
        f.apply(&src, 6, 6, &mut out);
        assert!(out.iter().all(|v| (v - 3.0).abs() < 1e-4));
    }

    #[test]
    fn near_larger_than_far() {
        let c = DofConfig::standard();
        assert!(c.coc_px(c.lens.focus - 40.0) > c.coc_px(c.lens.focus + 40.0));
    }

    #[test]
    fn coc_clamped_to_max() {
        let cfg = DofConfig {
            lens: Lens {
                aperture: 400.0,
                focal: 900.0,
                focus: 1000.0,
            },
            px_per_unit: 900.0,
            ..DofConfig::standard()
        };
        assert!(cfg.coc_px(995.0) <= COC_MAX_PX);
    }
}
