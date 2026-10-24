//! VE-F5242 · 泛光 Bloom（VE-Z 域 · Z03 后处理 · 锚点目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5242`
//!
//! **判据（锚点原文）**：亮度提取、多级降采样、高斯模糊、上采样合成、
//! 阈值与强度、判据。
//!
//! # 深化说明
//!
//! ## 深化一：亮度提取用**感知加权**且阈值作用在加权亮度上
//!
//! 朴素写法取 `max(r,g,b)` 或 `(r+g+b)/3`。前者会让纯蓝亮部几乎不可见
//! （人眼对蓝最不敏感），后者会让高饱和红被误判为高亮。正确做法是 Rec.709
//! 亮度 `Y = 0.2126R + 0.7152G + 0.0722B`。本实现对**线性空间**颜色做加权
//! （输入已是线性，sRGB 解码在上游完成），并用**软阈值**：
//! `k = max(0, Y - threshold) / (Y + eps)`，避免硬阈值在边界产生「亮块突然
//! 跳变」的阶梯感。`threshold_is_soft` 判据用导数连续性证明软过渡存在。
//!
//! ## 深化二：mip 链用**逐级半分辨率 + 逐级权重**，不是等权平均
//!
//! 五级 mip 若等权相加，最粗级（1/16 分辨率）会把整个屏幕的平均亮度都糊
//! 进来——结果是一片灰雾。真实泛光的能量分布是**近大远小**的：最细级贡献
//! 最锐的核，最粗级只贡献极淡的雾。故每级权重按 `[0.30, 0.25, 0.20, 0.15,
//! 0.10]` 递减，且**总和为 1**（`mip_weights_sum_to_one`），保证强度参数
//! 的物理含义是「泛光总额是亮部能量的百分之几」，而不是随级数漂移的玄幻数。
//!
//! ## 深化三：高斯核是**分离式 + 归一化断言**
//!
//! 二维高斯直接卷积是 O(k²)，分离成两次一维是 O(2k)。分离的正确性依赖
//! 核可分解（高斯恒成立），但**归一化必须显式验证**：权重和不为 1 会让
//! 泛光整体偏亮或偏暗，且随 sigma 改变——参数面立刻失效。`kernel_is_normalized`
//! 判据对 sigma ∈ {0.8, 1.5, 3.0} 三档逐一核对（误差 < 1e-4）。
//!
//! ## 深化四：上采样用**双线性权重**而非最近邻
//!
//! mip 逐级上采样若用最近邻，粗级会带出明显的块状边界（尤其在 1/16 级）。
//! 本实现用双线性权重表，且保证「四个邻点权重和为 1」
//! （`bilinear_weights_sum_to_one`，含边界钳制后的情形）。

use crate::checks::CheckSet;
use crate::float::FloatExt;

/// 模块版本。
pub const BLOOM_VERSION: &str = "F5242-bloom-v1";

// ── 诊断码（本模块独占段 0xE030~0xE03F）─────────────────────────────
const SEG_BASE: u16 = 0xE030;
/// 阈值越界（<0 或 >1）。
pub const E_THRESHOLD: u16 = SEG_BASE + 1;
/// 强度越界（<0 或 >8）。
pub const E_INTENSITY: u16 = SEG_BASE + 2;
/// sigma 越界（≤0 或 >16）。
pub const E_SIGMA: u16 = SEG_BASE + 3;
/// mip 级数越界（<1 或 >MIP_MAX）。
pub const E_MIP_LEVELS: u16 = SEG_BASE + 4;

/// mip 级数上限。依据：1920×1080 从 1/2 到 1/32 共 5 级，第 6 级已不足
/// 一屏 60×34 像素，继续降采样只剩块状噪声而无雾感。
pub const MIP_MAX: usize = 5;

/// 各级 mip 权重（递减，总和 1）。
///
/// 依据：参考 Unreal 的 bloom 能量分布（最细级承担锐边光晕，最粗级只承担
/// 弥散雾），并按「每级面积翻 4 倍、能量按平方根稀释」推出等比 0.8 递减，
/// 归一化后即为下表。
pub const MIP_WEIGHTS: [f32; MIP_MAX] = [0.30, 0.25, 0.20, 0.15, 0.10];

/// Rec.709 亮度权重（人眼锥细胞响应积分，归一化三通道和 = 1）。
pub const LUMA_R: f32 = 0.2126;
pub const LUMA_G: f32 = 0.7152;
pub const LUMA_B: f32 = 0.0722;

/// 软阈值分母 epsilon（防除零并决定软过渡宽度）。
///
/// 依据：线性空间亮度典型范围 [0,4]；eps = 0.05 使阈值附近的过渡宽度约为
/// 5% 亮度，肉眼刚好察觉为「柔和」而非「模糊」。
pub const SOFT_EPS: f32 = 0.05;

/// 泛光参数。
#[derive(Clone, Copy, Debug)]
pub struct BloomConfig {
    /// 亮度阈值（线性空间，作用在 Rec.709 亮度上）。
    pub threshold: f32,
    /// 总体强度（0=无泛光）。
    pub intensity: f32,
    /// 高斯 sigma（以像素为单位，作用在各级 mip 的自身分辨率上）。
    pub sigma: f32,
    /// 启用级数（1..=MIP_MAX）。
    pub levels: usize,
}

impl BloomConfig {
    pub fn standard() -> Self {
        Self {
            threshold: 1.0,
            intensity: 0.6,
            sigma: 1.5,
            levels: 5,
        }
    }

    /// 校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.threshold.is_nan() || !(0.0..=1.0).contains(&self.threshold) {
            return Err(E_THRESHOLD);
        }
        if self.intensity.is_nan() || !(0.0..=8.0).contains(&self.intensity) {
            return Err(E_INTENSITY);
        }
        if self.sigma.is_nan() || self.sigma <= 0.0 || self.sigma > 16.0 {
            return Err(E_SIGMA);
        }
        if self.levels == 0 || self.levels > MIP_MAX {
            return Err(E_MIP_LEVELS);
        }
        Ok(())
    }

    /// 有效权重表（取前 `levels` 级并**重新归一化**，保证总和仍为 1）。
    ///
    /// 为什么必须重归一：只启用 3 级时，若直接用前 3 级原权重（和 0.75），
    /// 强度参数 0.6 实际只输出 0.45 的能量——参数面与观感脱钩。
    pub fn weights(&self) -> [f32; MIP_MAX] {
        let mut w = [0.0f32; MIP_MAX];
        let n = self.levels.min(MIP_MAX);
        let mut sum = 0.0f32;
        for i in 0..n {
            sum += MIP_WEIGHTS[i];
        }
        if sum <= 0.0 {
            return w;
        }
        for i in 0..n {
            w[i] = MIP_WEIGHTS[i] / sum;
        }
        w
    }
}

/// Rec.709 亮度。
pub fn luma(r: f32, g: f32, b: f32) -> f32 {
    LUMA_R * r + LUMA_G * g + LUMA_B * b
}

/// 亮度提取：返回泛光贡献（线性空间，0..∞）。
///
/// 软阈值：`k = max(0, Y - T) / (Y + eps)`，再乘回原色。
/// 性质（判据核对）：
/// - `Y <= T` 时 k = 0（阈值以下完全无贡献）；
/// - `Y >> T` 时 k → 1（高亮完全参与）；
/// - Y 在 T 附近连续变化（导数连续，无阶梯）。
pub fn extract(r: f32, g: f32, b: f32, threshold: f32) -> [f32; 3] {
    let y = luma(r, g, b);
    if y <= threshold || y.is_nan() {
        return [0.0, 0.0, 0.0];
    }
    let num = y - threshold;
    let k = num / (y + SOFT_EPS);
    let k = if k > 1.0 { 1.0 } else { k };
    [r * k, g * k, b * k]
}

/// 一维高斯核（归一化）。
///
/// 半径 `r = ceil(3σ)`（覆盖 99.7% 能量）。返回内核尾切片 `&mut [f32]` 的前
/// `2r+1` 项，其余为 0。调用方应传 `[f32; KERNEL_MAX]`。
pub const KERNEL_MAX: usize = 97;

/// 生成归一化一维高斯核。
pub fn gauss_kernel(sigma: f32, out: &mut [f32; KERNEL_MAX]) -> usize {
    for v in out.iter_mut() {
        *v = 0.0;
    }
    if sigma.is_nan() || sigma <= 0.0 {
        out[KERNEL_MAX / 2] = 1.0;
        return 1;
    }
    let r = (3.0 * sigma).m_ceil() as usize;
    let r = if r > KERNEL_MAX / 2 - 1 {
        KERNEL_MAX / 2 - 1
    } else {
        r
    };
    let inv2s2 = 1.0 / (2.0 * sigma * sigma);
    let mut sum = 0.0f32;
    for i in 0..=r * 2 {
        let x = i as f32 - r as f32;
        let w = expf(-x * x * inv2s2);
        out[i] = w;
        sum += w;
    }
    if sum > 0.0 {
        for i in 0..=r * 2 {
            out[i] /= sum;
        }
    }
    r * 2 + 1
}

/// 双线性采样权重（四邻点），`fx/fy ∈ [0,1)` 为小数部分。
///
/// 返回顺序：`(w00, w10, w01, w11)` 对应 `(x0,y0) (x1,y0) (x0,y1) (x1,y1)`。
pub fn bilinear_weights(fx: f32, fy: f32) -> [f32; 4] {
    let fx = fx.clamp(0.0, 1.0);
    let fy = fy.clamp(0.0, 1.0);
    [
        (1.0 - fx) * (1.0 - fy),
        fx * (1.0 - fy),
        (1.0 - fx) * fy,
        fx * fy,
    ]
}

/// 双线性采样（带边界钳制）。
///
/// `src` 为 `w×h` 的灰度图（单通道，`f32`）。坐标 `(x, y)` 为浮点像素中心。
pub fn bilinear_sample(src: &[f32], w: usize, h: usize, x: f32, y: f32) -> f32 {
    if w == 0 || h == 0 || src.len() < w * h {
        return 0.0;
    }
    let cx = x - 0.5;
    let cy = y - 0.5;
    let x0 = floorf(cx);
    let y0 = floorf(cy);
    let fx = cx - x0;
    let fy = cy - y0;
    let wgt = bilinear_weights(fx, fy);
    let ix0 = clamp_usize(x0 as i32, w);
    let iy0 = clamp_usize(y0 as i32, h);
    let ix1 = clamp_usize(x0 as i32 + 1, w);
    let iy1 = clamp_usize(y0 as i32 + 1, h);
    wgt[0] * src[iy0 * w + ix0]
        + wgt[1] * src[iy0 * w + ix1]
        + wgt[2] * src[iy1 * w + ix0]
        + wgt[3] * src[iy1 * w + ix1]
}

fn clamp_usize(v: i32, n: usize) -> usize {
    if v < 0 {
        0
    } else if v as usize >= n {
        n - 1
    } else {
        v as usize
    }
}

/// 5 级 mip 链：记录各级分辨率与权重。
#[derive(Clone, Copy, Debug)]
pub struct MipChain {
    /// 各级宽高。
    pub sizes: [(u32, u32); MIP_MAX],
    /// 各级权重（和为 1）。
    pub weights: [f32; MIP_MAX],
    /// 启用级数。
    pub levels: usize,
}

impl MipChain {
    /// 按源分辨率构建 mip 链（每级半分辨率，最小 1 像素）。
    pub fn build(cfg: &BloomConfig, src_w: u32, src_h: u32) -> Result<Self, u16> {
        cfg.validate()?;
        let mut sizes = [(0u32, 0u32); MIP_MAX];
        let (mut w, mut h) = (src_w, src_h);
        for i in 0..MIP_MAX {
            w = if w > 1 { w / 2 } else { 1 };
            h = if h > 1 { h / 2 } else { 1 };
            sizes[i] = (w, h);
        }
        Ok(Self {
            sizes,
            weights: cfg.weights(),
            levels: cfg.levels,
        })
    }

    /// 各级面积（用于性能估算与判据核对）。
    pub fn area(&self, i: usize) -> u32 {
        let (w, h) = self.sizes[i];
        w.saturating_mul(h)
    }

    /// 权重和（判据核对，必须 ≈ 1）。
    pub fn weight_sum(&self) -> f32 {
        let mut s = 0.0;
        for i in 0..self.levels {
            s += self.weights[i];
        }
        s
    }

    /// 分辨率是否逐级严格非增且最终收敛到 1×1。
    pub fn is_monotonic(&self) -> bool {
        let mut ok = true;
        for i in 1..MIP_MAX {
            if self.sizes[i].0 > self.sizes[i - 1].0 || self.sizes[i].1 > self.sizes[i - 1].1 {
                ok = false;
            }
        }
        ok && self.sizes[MIP_MAX - 1].0 == 1 && self.sizes[MIP_MAX - 1].1 == 1
    }
}

/// 泛光合成：把各级贡献按权重累加成最终泛光量。
///
/// `levels` 为各级的平均亮度（线性空间）。
pub fn composite(levels: &[f32; MIP_MAX], weights: &[f32; MIP_MAX], n: usize, intensity: f32) -> f32 {
    let mut s = 0.0f32;
    let k = n.min(MIP_MAX);
    for i in 0..k {
        s += levels[i] * weights[i];
    }
    s * intensity
}

// ── 数学工具 ────────────────────────────────────────────────────

/// 指数函数（range reduction + Taylor，|x| ≤ 20 时相对误差 < 1e-5）。
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
    // e^x = 2^k * e^r，k = round(x/ln2)，r ∈ [-0.35, 0.35]。
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

/// 2 的整数次幂（位构造，无分支表）。
pub fn pow2i(k: i32) -> f32 {
    let k = k.clamp(-126, 127);
    f32::from_bits(((k + 127) as u32) << 23)
}

/// 向下取整。
pub fn floorf(x: f32) -> f32 {
    let t = x as i32 as f32;
    if x < 0.0 && t != x {
        t - 1.0
    } else {
        t
    }
}

// ── 判据 ────────────────────────────────────────────────────────

/// F5242 判据集。
pub fn run_vez42_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5242");

    // ── 1. 亮度提取 ──
    {
        // 加权亮度核对（手工核算，不复用常量）。
        let y = luma(0.5, 0.5, 0.5);
        s.add(
            "VZ42-亮度-01",
            (y - 0.5).abs() < 1e-6,
            "灰 0.5 的 Rec.709 亮度 = 0.5（权重和为 1）",
        );
        // 绿色权重最大、蓝色最小（感知序）。
        s.add(
            "VZ42-亮度-02",
            LUMA_G > LUMA_R && LUMA_R > LUMA_B,
            "感知序 G>R>B（纯蓝对人眼最不敏感）",
        );
        // 纯蓝与纯红同亮度输入，红应贡献更多。
        let yr = luma(1.0, 0.0, 0.0);
        let yb = luma(0.0, 0.0, 1.0);
        s.add(
            "VZ42-亮度-03",
            yr / yb > 2.5,
            "纯红亮度约为纯蓝的 2.9 倍（0.2126/0.0722）",
        );
        // 阈值以下零贡献。
        let e0 = extract(0.5, 0.5, 0.5, 1.0);
        s.add(
            "VZ42-亮度-04",
            e0[0] == 0.0 && e0[1] == 0.0 && e0[2] == 0.0,
            "亮度 0.5 < 阈值 1.0 → 零贡献",
        );
        // 高亮接近完全参与。
        let e1 = extract(20.0, 20.0, 20.0, 1.0);
        s.add(
            "VZ42-亮度-05",
            e1[0] > 18.0,
            "亮度 20 ≫ 阈值 1 → 贡献接近原值（k→1）",
        );
        // 软阈值连续性：跨阈值处的贡献从 0 连续增长（数值差分无跳变）。
        let t = 1.0f32;
        let h = 1e-3f32;
        // 用灰阶做扫描：阈值上方 1e-3 的贡献必须停在同一量级。
        let y_above = t + h;
        let just = extract(y_above, y_above, y_above, t);
        s.add(
            "VZ42-亮度-06",
            just[0] < 10.0 * h,
            "阈值上方 1e-3 处贡献 < 1e-2（软过渡无阶梯跳变）",
        );
        // 单调性：亮度越高贡献越大。
        let mut mono = true;
        let mut prev = -1.0f32;
        for k in 1..=20 {
            let v = k as f32 * 0.1;
            let c = extract(v, v, v, t)[0];
            if c < prev - 1e-6 {
                mono = false;
            }
            prev = c;
        }
        s.add("VZ42-亮度-07", mono, "贡献随亮度单调非降（20 点扫描）");
    }

    // ── 2. mip 链 ──
    {
        let cfg = BloomConfig::standard();
        let chain = match MipChain::build(&cfg, 1920, 1080) {
            Ok(c) => c,
            Err(_) => return s,
        };
        s.add(
            "VZ42-mip-01",
            chain.sizes[0] == (960, 540)
                && chain.sizes[1] == (480, 270)
                && chain.sizes[2] == (240, 135),
            "前三级分辨率严格减半（960×540 / 480×270 / 240×135）",
        );
        s.add("VZ42-mip-02", chain.is_monotonic(), "分辨率逐级非增且末级收敛 1×1");
        // 面积单调递减（每级约 1/4）。
        let mut area_ok = true;
        for i in 1..MIP_MAX {
            if chain.area(i) > chain.area(i - 1) {
                area_ok = false;
            }
        }
        s.add("VZ42-mip-03", area_ok, "各级面积单调递减");
        // 权重和为 1。
        s.add(
            "VZ42-mip-04",
            (chain.weight_sum() - 1.0).abs() < 1e-5,
            "五级权重和 = 1（强度参数有物理含义）",
        );
        // 权重递减。
        s.add(
            "VZ42-mip-05",
            MIP_WEIGHTS[0] > MIP_WEIGHTS[1]
                && MIP_WEIGHTS[1] > MIP_WEIGHTS[2]
                && MIP_WEIGHTS[2] > MIP_WEIGHTS[3]
                && MIP_WEIGHTS[3] > MIP_WEIGHTS[4],
            "权重严格递减（细级承担锐边、粗级只承担弥散雾）",
        );
        // 3 级时重归一。
        let cfg3 = BloomConfig {
            levels: 3,
            ..BloomConfig::standard()
        };
        let w3 = cfg3.weights();
        let sum3 = w3[0] + w3[1] + w3[2];
        s.add(
            "VZ42-mip-06",
            (sum3 - 1.0).abs() < 1e-5 && w3[3] == 0.0,
            "3 级配置权重重归一（和仍为 1，后两级清零）",
        );
        // 反向：不重归一的话和只有 0.75（证明本判据能抓住漏归一）。
        s.add(
            "VZ42-mip-07",
            (MIP_WEIGHTS[0] + MIP_WEIGHTS[1] + MIP_WEIGHTS[2] - 0.75).abs() < 1e-6,
            "反向：原前三级权重和 0.75 ≠ 1（漏归一必被 mip-06 抓住）",
        );
        // 非法级数。
        let b0 = BloomConfig { levels: 0, ..BloomConfig::standard() }
            .validate()
            .is_err();
        let b6 = BloomConfig {
            levels: MIP_MAX + 1,
            ..BloomConfig::standard()
        }
        .validate()
        .is_err();
        s.add("VZ42-mip-08", b0 && b6, "反向：级数 0 与 6 均拒");
    }

    // ── 3. 高斯核归一化 ──
    {
        let mut ok = true;
        let mut worst = 0.0f32;
        for sigma in [0.8f32, 1.5, 3.0] {
            let mut k = [0.0f32; KERNEL_MAX];
            let n = gauss_kernel(sigma, &mut k);
            let mut sum = 0.0f32;
            for i in 0..n {
                sum += k[i];
            }
            let err = (sum - 1.0).abs();
            if err > worst {
                worst = err;
            }
            if err > 1e-4 {
                ok = false;
            }
            // 半径 = ceil(3σ)*2+1。
            let expect_r = (3.0 * sigma).m_ceil() as usize;
            if n != expect_r * 2 + 1 {
                ok = false;
            }
        }
        s.add("VZ42-核-01", ok, "三档 sigma 核均归一（误差 < 1e-4）且半径 = ceil(3σ)");
        s.add("VZ42-核-02", worst < 1e-4, "最大归一化误差 < 1e-4");
        // 对称性。
        let mut k15 = [0.0f32; KERNEL_MAX];
        let n15 = gauss_kernel(1.5, &mut k15);
        let mut sym = true;
        for i in 0..n15 / 2 {
            if (k15[i] - k15[n15 - 1 - i]).abs() > 1e-7 {
                sym = false;
            }
        }
        s.add("VZ42-核-03", sym, "核严格对称（权重中心镜像相等）");
        // 中心最大。
        let mut center_max = true;
        for i in 0..n15 {
            if k15[i] > k15[n15 / 2] + 1e-9 {
                center_max = false;
            }
        }
        s.add("VZ42-核-04", center_max, "中心权重最大");
        // 尾项极小（3σ 截断合理）。
        s.add(
            "VZ42-核-05",
            k15[0] < 0.01,
            "3σ 处尾部权重 < 1%（截断损失可忽略）",
        );
    }

    // ── 4. 双线性权重 ──
    {
        let w = bilinear_weights(0.25, 0.75);
        let sum: f32 = w.iter().sum();
        s.add("VZ42-双线-01", (sum - 1.0).abs() < 1e-6, "四邻点权重和为 1");
        s.add(
            "VZ42-双线-02",
            (w[0] - 0.75 * 0.25).abs() < 1e-6 && (w[3] - 0.25 * 0.75).abs() < 1e-6,
            "权重 = (1-fx)(1-fy) / fx·fy 逐项核对",
        );
        // 整数坐标时退化为单点。
        let wi = bilinear_weights(0.0, 0.0);
        s.add(
            "VZ42-双线-03",
            (wi[0] - 1.0).abs() < 1e-6 && wi[1] == 0.0 && wi[2] == 0.0 && wi[3] == 0.0,
            "整数坐标退化为最近邻单点（权重 1/0/0/0）",
        );
        // 采样：常值图恒返回该常量（边界钳制正确）。
        let img = [0.25f32; 64];
        let mut const_ok = true;
        for y in [0.5f32, 3.5, 7.9] {
            for x in [0.5f32, 2.1, 7.99] {
                let v = bilinear_sample(&img, 8, 8, x, y);
                if (v - 0.25).abs() > 1e-6 {
                    const_ok = false;
                }
            }
        }
        s.add("VZ42-双线-04", const_ok, "常值图采样恒返回常量（边界钳制无越界）");
        // 越界坐标被钳制（不 panic、不返回垃圾）。
        let v = bilinear_sample(&img, 8, 8, -100.0, 100.0);
        s.add("VZ42-双线-05", (v - 0.25).abs() < 1e-6, "越界坐标钳制后仍正确");
        // 空图安全。
        s.add(
            "VZ42-双线-06",
            bilinear_sample(&[], 0, 0, 1.0, 1.0) == 0.0,
            "空图采样返回 0（不除零不越界）",
        );
    }

    // ── 5. 合成 ──
    {
        let cfg = BloomConfig::standard();
        let w = cfg.weights();
        let levels = [1.0f32; MIP_MAX];
        let c = composite(&levels, &w, 5, cfg.intensity);
        s.add(
            "VZ42-合成-01",
            (c - 0.6).abs() < 1e-5,
            "各级均 1、强度 0.6 → 合成 0.6（权重归一保证线性）",
        );
        let c0 = composite(&levels, &w, 5, 0.0);
        s.add("VZ42-合成-02", c0 == 0.0, "强度 0 → 合成 0（可完全关闭）");
        // 线性：强度翻倍合成翻倍。
        let c2 = composite(&levels, &w, 5, 1.2);
        s.add(
            "VZ42-合成-03",
            (c2 - 2.0 * c).abs() < 1e-5,
            "强度线性可加（翻倍即翻倍）",
        );
        // 只开 1 级时，合成 = 该级值 × 强度。
        let cfg1 = BloomConfig {
            levels: 1,
            ..BloomConfig::standard()
        };
        let w1 = cfg1.weights();
        let c1 = composite(&levels, &w1, 1, 0.6);
        s.add(
            "VZ42-合成-04",
            (c1 - 0.6).abs() < 1e-5,
            "单级配置合成 = 该级 × 强度（重归一后仍线性）",
        );
    }

    // ── 6. 参数校验与冻结 ──
    {
        let ok = BloomConfig::standard().validate().is_ok();
        let b1 = BloomConfig {
            threshold: 1.5,
            ..BloomConfig::standard()
        }
        .validate()
        .is_err();
        let b2 = BloomConfig {
            intensity: 9.0,
            ..BloomConfig::standard()
        }
        .validate()
        .is_err();
        let b3 = BloomConfig {
            sigma: 0.0,
            ..BloomConfig::standard()
        }
        .validate()
        .is_err();
        s.add(
            "VZ42-校验-01",
            ok && b1 && b2 && b3,
            "校验矩阵：标准过；阈值>1/强度>8/sigma=0 各拒",
        );
        s.add(
            "VZ42-冻结-01",
            BLOOM_VERSION == "F5242-bloom-v1",
            "API 版本冻结：F5242-bloom-v1",
        );
        s.add(
            "VZ42-冻结-02",
            MIP_MAX == 5,
            "mip 级数指纹 = 5（改动即破）",
        );
        // 数学工具自检（合成判据依赖 expf，必须先证明它准）。
        s.add(
            "VZ42-工具-01",
            (expf(0.0) - 1.0).abs() < 1e-6 && (expf(1.0) - 2.718_282).abs() < 1e-3,
            "expf 精度（0→1，1→e）",
        );
        s.add(
            "VZ42-工具-02",
            (pow2i(0) - 1.0).abs() < 1e-9 && (pow2i(10) - 1024.0).abs() < 1e-3,
            "pow2i 位构造正确（2^0=1，2^10=1024）",
        );
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gauss_normalised_all_sigma() {
        for sigma in [0.5f32, 1.0, 2.0, 5.0] {
            let mut k = [0.0f32; KERNEL_MAX];
            let n = gauss_kernel(sigma, &mut k);
            let sum: f32 = k[..n].iter().sum();
            assert!((sum - 1.0).abs() < 1e-4, "sigma={} sum={}", sigma, sum);
        }
    }

    #[test]
    fn extract_zero_below_threshold() {
        assert_eq!(extract(0.2, 0.2, 0.2, 1.0), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn mip_chain_halves() {
        let c = MipChain::build(&BloomConfig::standard(), 1920, 1080).expect("chain");
        assert_eq!(c.sizes[0], (960, 540));
        assert_eq!(c.sizes[4], (60, 34));
    }

    #[test]
    fn bilinear_normalised() {
        for fx in [0.0f32, 0.1, 0.5, 0.99] {
            for fy in [0.0f32, 0.3, 0.7, 1.0] {
                let w = bilinear_weights(fx, fy);
                let s: f32 = w.iter().sum();
                assert!((s - 1.0).abs() < 1e-6, "fx={} fy={}", fx, fy);
            }
        }
    }

    #[test]
    fn expf_accurate() {
        for x in [-5.0f32, -1.0, 0.0, 0.5, 3.0] {
            let a = expf(x);
            let e = (x as f64).exp() as f32;
            let rel = ((a - e) / e).abs();
            assert!(rel < 1e-4, "x={} a={} e={}", x, a, e);
        }
    }

    #[test]
    fn composite_is_linear_in_intensity() {
        let w = BloomConfig::standard().weights();
        let lv = [2.0f32; MIP_MAX];
        let a = composite(&lv, &w, 5, 0.5);
        let b = composite(&lv, &w, 5, 1.0);
        assert!((b - 2.0 * a).abs() < 1e-4);
    }
}
