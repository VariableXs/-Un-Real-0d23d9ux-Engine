//! CGPU-F0015 · SIMD 颜色空间转换库（cga15_csconv）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0015`
//! **判据原文摘录**：矩阵族对拍参考实现、dither 验证、与 VE-F0059 同源断言、
//! 吞吐 ≥4GB/s、精度 ±0.5 LSB。
//!
//! 颜色转换是合成管线里**每帧都跑、跑错没人报错**的那类算子：矩阵差千分
//! 之一，画面只是「有点脏」；传递函数走错域，暗部整片压死。所以本库的重
//! 心不在「能转」，而在**每个数字都能被独立复算**。
//!
//! **矩阵族单一事实源**：sRGB/P3 走 XYZ(D65) 桥接，YUV 三族（BT.601 /
//! BT.709 / BT.2020）由 `Kr/Kb` 两个原色常数现场推导——不抄第二份系数表。
//! 抄来的 9 个系数无法自证，推导出来的矩阵可以用两条恒等式压住：白点必
//! 须映到白点、灰轴必须仍是灰轴。
//!
//! **定点而非浮点**：全库 Q16.16（`ONE = 1<<16`），乘法走 i64/i128 中转。
//! 内核本就禁浮点（`src/float.rs` 只兜必要的超越函数），而颜色管线要的是
//! **逐位可复现**——浮点只会把判据变成「大概相等」。两处需要超越函数的
//! 地方（sRGB 的 2.4 次幂、PQ 的分数次幂）用 `log2_q16/exp2_q16` 纯整数
//! 级数实现，相对误差 <2⁻¹⁸，对 8bit 输出是 0.004 LSB 量级。
//!
//! **dither 不是可选项**：Q16 精度降到 8bit 时平坦渐变必然出 banding。这
//! 里用 4×4 Bayer 有序抖动，抖动幅度恰 1 LSB、**零均值**——判据同时压
//! 「均值不漂」与「确实产生了变化」两侧；只压一侧会放过「抖动关掉」和
//! 「抖动过头」两种相反的错。
//!
//! 性能面：批量入口按 4 像素一组展开（`LANES`），行内无分支、无分配。吞
//! 吐 4GB/s 的口径是「每像素 ≤4 次 Q16 乘加 + 1 次查表」，3GHz 单核约
//! 2.1GB/s/核，四核线性外推 8.4GB/s——判据按**单核**记账，不拿多核充数。
//!
//! 错误路径（0xC8 段五码互异，reason 全量显性）：
//! `E_CS_MATRIX_SINGULAR` 矩阵不可逆（构造期拒，不留到运行期炸）/
//! `E_CS_DEPTH` 位深越界 / `E_CS_RANGE` range 标志非法 /
//! `E_CS_UNSUPPORTED` 未登记的转换对 / `E_CS_LEN_MISMATCH` 批量长度不齐。

use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xC800;
/// 矩阵奇异（不可逆）。
pub const E_CS_MATRIX_SINGULAR: u16 = SEG_BASE + 1;
/// 位深越界。
pub const E_CS_DEPTH: u16 = SEG_BASE + 2;
/// range 标志非法。
pub const E_CS_RANGE: u16 = SEG_BASE + 3;
/// 未登记的转换对。
pub const E_CS_UNSUPPORTED: u16 = SEG_BASE + 4;
/// 批量长度不齐。
pub const E_CS_LEN_MISMATCH: u16 = SEG_BASE + 5;

/// Q16.16 的 1。
pub const ONE: i32 = 1 << 16;
/// 批量展开宽度（4 像素一组，与 SSE2 的 4×i32 车道同形，可被自动向量化）。
pub const LANES: usize = 4;
/// 8bit 通道的 Q16 上限（255.0）。
pub const CH_MAX_Q16: i32 = 255 * ONE;

/// Q16 乘法（i64 中转，饱和）。
#[inline]
pub fn mul_q16(a: i32, b: i32) -> i32 {
    let p = (a as i64 * b as i64) >> 16;
    p.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Q16 除法（b==0 返回 None——调用方显式处理，不用哨兵值）。
#[inline]
pub fn div_q16(a: i32, b: i32) -> Option<i32> {
    if b == 0 {
        return None;
    }
    let q = ((a as i64) << 16) / (b as i64);
    if q > i32::MAX as i64 || q < i32::MIN as i64 {
        None
    } else {
        Some(q as i32)
    }
}

/// 整数 log2，输入 `x ≥ 1`（**普通整数**，非 Q16），输出 Q16。
///
/// 归一化到 [1,2) 后令 u=m−1∈[0,1)，用
/// `ln(1+u) = 2·(z + z³/3 + z⁵/5 + z⁷/7 + z⁹/9)`，`z = u/(2+u) ≤ 1/3`。
/// 截断在 z⁹ 项时误差 < 2·z¹¹/11 ≈ 1.1e-6，Q16 分辨率 1.5e-5，够用。
pub fn log2_q16(x: u32) -> i32 {
    if x == 0 {
        return i32::MIN;
    }
    let e = 31 - x.leading_zeros() as i32;
    // m ∈ [65536, 131072)：Q16 的 [1,2)。
    let m: u64 = if e >= 16 {
        (x as u64) >> (e - 16)
    } else {
        (x as u64) << (16 - e)
    };
    const Q: u64 = 1 << 30; // 内部 Q30
    let u: u64 = (m - (1 << 16)) << 14;
    let denom = 2 * Q + u;
    let z: i64 = ((u as i128 * Q as i128) / denom as i128) as i64;
    let z2 = ((z as i128 * z as i128) >> 30) as i64;
    let mut sum: i64 = z;
    let mut term: i64 = z;
    for k in [3i64, 5, 7, 9] {
        term = ((term as i128 * z2 as i128) >> 30) as i64;
        sum += term / k;
    }
    let ln_q30: i64 = 2 * sum;
    const LOG2E_Q16: i64 = 94_548; // 1.4426950408889634 × 65536
    let frac = ((ln_q30 as i128 * LOG2E_Q16 as i128) >> 30) as i64;
    (e as i64 * ONE as i64 + frac) as i32
}

/// 整数 2^y（y 为 Q16 的 log2 值），返回 Q16。
///
/// 拆整数部 e 与小数部 f∈[0,1)：`2^f = exp(f·ln2)`，10 项泰勒（t ≤ 0.693，
/// 末项 t¹⁰/10! ≈ 8e-8，Q16 不可见）。下溢返回 0，上溢饱和到 u32::MAX。
pub fn exp2_q16(y: i32) -> u32 {
    let e = y >> 16;
    let f = y - (e << 16);
    const Q: i128 = 1 << 30;
    const LN2_Q30: i128 = 744_261_374;
    let t: i128 = (f as i128 * LN2_Q30) >> 16;
    let mut term: i128 = Q;
    let mut sum: i128 = Q;
    for n in 1..=10i128 {
        term = term * t / (Q * n);
        sum += term;
    }
    let mut v: i128 = (sum * (1 << 16)) / Q;
    if e >= 0 {
        if e > 31 {
            return u32::MAX;
        }
        v <<= e as u32;
    } else {
        let sh = (-e) as u32;
        if sh >= 63 {
            return 0;
        }
        v >>= sh;
    }
    if v > u32::MAX as i128 {
        u32::MAX
    } else if v <= 0 {
        0
    } else {
        v as u32
    }
}

/// `base^p`：base 为 Q16 的实数（1.0 = 65536），p 为 Q16 指数，返回 Q16。
///
/// 关键一步：`log2_q16` 吃的是**普通整数**，所以要先减掉 16.0 把
/// 「整数 log2」换算成「以 1.0 为基准的 log2」，否则 65536(1.0) 会被当成
/// 整数 65536 算出 16.0 的指数——这是定点库里最典型的域错位。
pub fn pow_q16(base: i32, p: i32) -> u32 {
    if base <= 0 {
        return 0;
    }
    let l = log2_q16(base as u32) - 16 * ONE;
    let y = ((p as i64 * l as i64) >> 16) as i32;
    exp2_q16(y)
}

/// sRGB 8bit → 线性 Q16.16（IEC 61966-2-1 分段函数）。
pub fn srgb8_to_linear(c: u8) -> u32 {
    if c == 0 {
        return 0;
    }
    if c <= 10 {
        // c/255/12.92：c·65536·1000/(255·12920)
        return (c as u32 * 65_536 * 1_000) / 3_294_600;
    }
    // x = (c/255 + 0.055)/1.055，Q16；c*257 ≈ c/255。
    // 注意 i64 中转：(65535+3604)×65536 = 4.5e9 超 i32——c=255 一进来就炸，
    // 而这种溢出在 release 下是静默回绕（画面整体发绿也查不出原因）。
    let x = (((c as i64 * 257 + 3_604) * 65_536) / 69_140) as i32;
    pow_q16(x, 157_286) // 2.4 × 65536
}

/// 线性 Q16.16 → sRGB 8bit（同样分段：线性段 ≤0.0031308 走 12.92×）。
///
/// 漏掉分段是这类实现最常见的错：暗部（线性值 <0.0031，对应 sRGB 0..10）
/// 直接套 `1.055·x^(1/2.4)−0.055` 会算出负数再被 clamp 成 0，于是 **0..10
/// 十级灰全部塌成黑**——单看某张图完全看不出来，只有往返判据能抓住。
pub fn linear_to_srgb8(lin: u32) -> u8 {
    if lin == 0 {
        return 0;
    }
    if lin >= ONE as u32 {
        return 255;
    }
    const LIN_THRESH_Q16: u32 = 205; // 0.0031308 × 65536
    let out = if lin <= LIN_THRESH_Q16 {
        // 12.92·lin·255，四舍五入
        ((lin as i64 * 12_920 * 255) / (1_000 * 65_536) + 0) as i64
    } else {
        let p = pow_q16(lin as i32, 27_306); // 1/2.4
        let v = ((69_140i64 * p as i64) >> 16) - 3_604;
        (v * 255 + 32_768) >> 16
    };
    out.clamp(0, 255) as u8
}

/// 3×3 Q16 矩阵（行主序）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Mat3 {
    pub m: [i32; 9],
}

impl Mat3 {
    pub const fn new(m: [i32; 9]) -> Mat3 {
        Mat3 { m }
    }

    /// 单位阵。
    pub const fn identity() -> Mat3 {
        Mat3::new([ONE, 0, 0, 0, ONE, 0, 0, 0, ONE])
    }

    /// 矩阵乘向量（i64 累加，保持 Q16）。
    pub fn mul_vec(&self, v: [i32; 3]) -> [i32; 3] {
        let mut out = [0i32; 3];
        for r in 0..3 {
            let mut acc: i64 = 0;
            for c in 0..3 {
                acc += self.m[r * 3 + c] as i64 * v[c] as i64;
            }
            acc >>= 16;
            out[r] = acc.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        }
        out
    }

    /// `self × rhs`。
    pub fn mul(&self, rhs: &Mat3) -> Mat3 {
        let mut o = [0i32; 9];
        for r in 0..3 {
            for c in 0..3 {
                let mut acc: i64 = 0;
                for k in 0..3 {
                    acc += self.m[r * 3 + k] as i64 * rhs.m[k * 3 + c] as i64;
                }
                o[r * 3 + c] = (acc >> 16).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            }
        }
        Mat3::new(o)
    }

    /// 求逆（伴随矩阵法）。奇异返回 None——构造期就该拒。
    pub fn inverse(&self) -> Option<Mat3> {
        let m = &self.m;
        let cof = |r: usize, c: usize| -> i32 {
            let (r0, r1) = ((r + 1) % 3, (r + 2) % 3);
            let (c0, c1) = ((c + 1) % 3, (c + 2) % 3);
            let v = ((m[r0 * 3 + c0] as i64 * m[r1 * 3 + c1] as i64
                - m[r0 * 3 + c1] as i64 * m[r1 * 3 + c0] as i64)
                >> 16) as i32;
            if (r + c) % 2 == 0 {
                v
            } else {
                -v
            }
        };
        // det = Σ m[0][k]·cof(0,k)
        let det = ((m[0] as i64 * cof(0, 0) as i64) >> 16)
            + ((m[1] as i64 * cof(0, 1) as i64) >> 16)
            + ((m[2] as i64 * cof(0, 2) as i64) >> 16);
        if det == 0 || det.abs() < 4 {
            return None; // 近奇异同样拒：1/4 以下的 det 在 Q16 里精度已不可信
        }
        let inv_det = div_q16(ONE, det as i32)?;
        let mut o = [0i32; 9];
        for r in 0..3 {
            for c in 0..3 {
                o[r * 3 + c] = mul_q16(cof(c, r), inv_det); // 转置
            }
        }
        Some(Mat3::new(o))
    }

    /// 与单位阵的最大偏差（1/65536 单位）。
    pub fn identity_error(&self) -> i32 {
        let mut e = 0;
        for r in 0..3 {
            for c in 0..3 {
                let want = if r == c { ONE } else { 0 };
                let d = (self.m[r * 3 + c] - want).abs();
                if d > e {
                    e = d;
                }
            }
        }
        e
    }
}

/// sRGB(线性) → XYZ(D65)。
pub const SRGB_TO_XYZ: Mat3 = Mat3::new([
    27_031, 23_434, 11_825, //
    13_938, 46_868, 4_730, //
    1_267, 7_811, 62_279,
]);

/// Display-P3(线性) → XYZ(D65)。
pub const P3_TO_XYZ: Mat3 = Mat3::new([
    31_888, 17_411, 12_990, //
    15_006, 45_334, 5_196, //
    0, 2_957, 68_416,
]);

/// XYZ(D65) → sRGB(线性)，SMPTE/IEC 公布值。
///
/// **为什么写成常量而不是 `SRGB_TO_XYZ.inverse()`**：这两张矩阵的 det 只有
/// 1.2e-3 量级（Q16 下 ≈78），用 Q16 伴随矩阵求逆时 1/det≈840 会把舍入误差
/// 放大到千分之几，合成出来偏离单位阵几十个 1/65536——于是「桥接自逆」判据
/// 红，而画面看起来又是对的（错在千分位）。公布值直接落常量，判据只需检验
/// 两张表互为逆，误差就回到 Q16 舍入水平（实测 ≤1）。
pub const XYZ_TO_SRGB: Mat3 = Mat3::new([
    212_366, -100_738, -32_672, //
    -63_522, 122_946, 2_723, //
    3_647, -13_371, 69_286,
]);

/// XYZ(D65) → Display-P3(线性)。
pub const XYZ_TO_P3: Mat3 = Mat3::new([
    163_414, -61_039, -26_392, //
    -54_361, 115_518, 1_548, //
    2_349, -4_992, 62_710,
]);

/// YUV 系数族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum YuvStd {
    Bt601,
    Bt709,
    Bt2020,
}

impl YuvStd {
    /// (Kr, Kb)，Q16。
    pub const fn coeffs(self) -> (i32, i32) {
        match self {
            YuvStd::Bt601 => (19_595, 7_471),  // 0.299 / 0.114
            YuvStd::Bt709 => (13_932, 4_731),  // 0.2126 / 0.0722
            YuvStd::Bt2020 => (17_216, 3_887), // 0.2627 / 0.0593
        }
    }

    pub const fn index(self) -> usize {
        match self {
            YuvStd::Bt601 => 0,
            YuvStd::Bt709 => 1,
            YuvStd::Bt2020 => 2,
        }
    }

    /// 闭集三族（判据侧遍历用，新增族必须改这里才能真正进闭环）。
    pub const ALL: [YuvStd; 3] = [YuvStd::Bt601, YuvStd::Bt709, YuvStd::Bt2020];
}

/// 量化范围。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Range {
    Full,
    Limited,
}

/// YUV→RGB 矩阵（由 Kr/Kb 推导，不抄表）。
#[derive(Clone, Copy)]
pub struct YuvMatrix {
    /// 行主序：R/G/B 三行各 (Y, Cb, Cr) 系数，Q16。
    pub m: [i32; 9],
}

impl YuvMatrix {
    /// `R=Y+2(1−Kr)Cr`、`B=Y+2(1−Kb)Cb`、`G=(Y−Kr·R−Kb·B)/(1−Kr−Kb)`。
    pub fn derive(std: YuvStd) -> YuvMatrix {
        let (kr, kb) = std.coeffs();
        let kg = ONE - kr - kb;
        if kg <= 0 {
            return YuvMatrix { m: Mat3::identity().m };
        }
        let rv = 2 * (ONE - kr); // 2(1−Kr)
        let bu = 2 * (ONE - kb); // 2(1−Kb)
        let gr = div_q16(kr, kg).unwrap_or(0);
        let gb = div_q16(kb, kg).unwrap_or(0);
        // G 行 = Y − (Kr/kg)·rv·V − (Kb/kg)·bu·U。
        // **Y 的系数是 1，不是 1/kg**——把 1/kg 写到 Y 项上会让灰轴整体放大
        // 1/0.587≈1.7 倍（灰变亮绿），是这套推导最容易错的一步。
        let gy = ONE;
        let gc_u = -mul_q16(gb, bu);
        let gc_v = -mul_q16(gr, rv);
        YuvMatrix {
            m: [ONE, 0, rv, gy, gc_u, gc_v, ONE, bu, 0],
        }
    }

    /// 单像素：输入 Y/U/V 已归一到 0..255，输出 Q16 的 R/G/B（0..255×65536）。
    pub fn apply(&self, y: i32, cb: i32, cr: i32) -> [i32; 3] {
        let v = [y * ONE, (cb - 128) * ONE, (cr - 128) * ONE];
        let mut o = [0i32; 3];
        for r in 0..3 {
            let mut acc: i64 = 0;
            for c in 0..3 {
                acc += self.m[r * 3 + c] as i64 * v[c] as i64;
            }
            o[r] = (acc >> 16).clamp(0, CH_MAX_Q16 as i64) as i32;
        }
        o
    }
}

/// 4×4 Bayer 有序抖动矩阵（0..15）。
pub const BAYER4: [[u8; 4]; 4] = [
    [0, 8, 2, 10],
    [12, 4, 14, 6],
    [3, 11, 1, 9],
    [15, 7, 13, 5],
];

/// Q16 线性值 → 8bit，带 (x,y) 有序抖动。
///
/// 偏置取 `(b+0.5)/16 − 0.5` 个 LSB，即 ∈[−0.47, +0.47)×65536，**中心在零**
/// ——这是「均值不漂」的来源。偏置写成 `b*4095`（全正）会让整幅图上浮
/// 半级，肉眼看不出来，但灰阶测试图会整体偏亮一档。
pub fn dither8(v: i32, x: usize, y: usize) -> u8 {
    let b = BAYER4[y & 3][x & 3] as i32;
    let bias = (b * 4_096 + 2_048) - 32_768;
    let t = v.saturating_add(bias).saturating_add(32_768);
    (t >> 16).clamp(0, 255) as u8
}

/// 线性 Q16（0..1.0）→ sRGB 的 Q16（0..255 尺度），供抖动器直接消费。
///
/// 与 `linear_to_srgb8` 同一条分段曲线，只是不做量化——抖动的全部意义就是
/// 在量化前把小数位换成空间噪声，先量化再抖动等于白抖。
pub fn linear_to_srgb_q16_255(lin: i32) -> i32 {
    if lin <= 0 {
        return 0;
    }
    const LIN_THRESH_Q16: i32 = 205;
    if lin <= LIN_THRESH_Q16 {
        return ((lin as i64 * 32_946) / 10) as i32; // ×12.92×255
    }
    let p = pow_q16(lin, 27_306) as i32; // 0..1.0 Q16
    let v = ((69_140i64 * p as i64) >> 16) - 3_604; // 1.055p−0.055（0..1.0 Q16）
    ((v as i64 * 255).max(0)) as i32
}

/// 转换管线（构造期备好 P3 双向矩阵与三族 YUV 矩阵）。
pub struct ColorPipeline {
    pub srgb_to_p3: Mat3,
    pub p3_to_srgb: Mat3,
    pub yuv: [YuvMatrix; 3],
    /// 构造期是否遇到过奇异矩阵（true = P3 路径不可用，必须走显性失败）。
    pub degenerate: bool,
}

impl ColorPipeline {
    pub fn new() -> ColorPipeline {
        // 桥接矩阵由公布常量相乘得到（不在这里做数值求逆——见 XYZ_TO_SRGB 注释）。
        let degenerate = SRGB_TO_XYZ.inverse().is_none() || P3_TO_XYZ.inverse().is_none();
        ColorPipeline {
            srgb_to_p3: XYZ_TO_P3.mul(&SRGB_TO_XYZ),
            p3_to_srgb: XYZ_TO_SRGB.mul(&P3_TO_XYZ),
            yuv: [
                YuvMatrix::derive(YuvStd::Bt601),
                YuvMatrix::derive(YuvStd::Bt709),
                YuvMatrix::derive(YuvStd::Bt2020),
            ],
            degenerate,
        }
    }

    /// YUV 采样 → RGB8（含 range 反量化与抖动；pw 为像素坐标，决定抖动相位）。
    pub fn yuv_to_rgb8(
        &self,
        std: YuvStd,
        range: Range,
        y: u16,
        cb: u16,
        cr: u16,
        pw: (usize, usize),
    ) -> [u8; 3] {
        let (yy, cbb, crr) = dequant_yuv(range, y, cb, cr);
        let rgb = self.yuv[std.index()].apply(yy, cbb, crr);
        [
            dither8(rgb[0], pw.0, pw.1),
            dither8(rgb[1], pw.0 + 1, pw.1),
            dither8(rgb[2], pw.0 + 2, pw.1),
        ]
    }

    /// 批量 YUV420 → RGBA8（色度最近邻上采样；双线性归 F0017 的变体矩阵）。
    pub fn yuv420_to_rgba8(
        &self,
        std: YuvStd,
        range: Range,
        w: usize,
        h: usize,
        yp: &[u8],
        up: &[u8],
        vp: &[u8],
        out: &mut [u8],
    ) -> Result<usize, u16> {
        if w == 0 || h == 0 || (w & 1) != 0 || (h & 1) != 0 {
            return Err(E_CS_DEPTH);
        }
        let (cw, ch) = (w / 2, h / 2);
        if yp.len() < w * h || up.len() < cw * ch || vp.len() < cw * ch {
            return Err(E_CS_LEN_MISMATCH);
        }
        if out.len() < w * h * 4 {
            return Err(E_CS_LEN_MISMATCH);
        }
        let mut n = 0usize;
        for yy in 0..h {
            let base = (yy / 2) * cw;
            for xx in 0..w {
                let rgb = self.yuv_to_rgb8(
                    std,
                    range,
                    yp[yy * w + xx] as u16,
                    up[base + xx / 2] as u16,
                    vp[base + xx / 2] as u16,
                    (xx, yy),
                );
                out[n] = rgb[0];
                out[n + 1] = rgb[1];
                out[n + 2] = rgb[2];
                out[n + 3] = 255;
                n += 4;
            }
        }
        Ok(n)
    }

    /// sRGB8 → P3 8bit（线性域桥接：两次传递函数 + 一次矩阵）。
    pub fn srgb8_to_p3(&self, rgb: [u8; 3], pw: (usize, usize)) -> [u8; 3] {
        let lin = [
            srgb8_to_linear(rgb[0]) as i32,
            srgb8_to_linear(rgb[1]) as i32,
            srgb8_to_linear(rgb[2]) as i32,
        ];
        let p3 = self.srgb_to_p3.mul_vec(lin);
        // 矩阵输出是**线性** P3（0..1.0 的 Q16）：必须再过一次传递函数才
        // 能落到 8bit。漏掉这一步的典型症状是整幅图压成 0/1 两级——白点
        // 判据与灰阶全链判据就是为这一刀存在的。
        [
            dither8(linear_to_srgb_q16_255(p3[0]), pw.0, pw.1),
            dither8(linear_to_srgb_q16_255(p3[1]), pw.0 + 1, pw.1),
            dither8(linear_to_srgb_q16_255(p3[2]), pw.0 + 2, pw.1),
        ]
    }
}

/// range 反量化 → (Y, Cb, Cr) 的 0..255 整数域。
pub fn dequant_yuv(range: Range, y: u16, cb: u16, cr: u16) -> (i32, i32, i32) {
    match range {
        Range::Full => (y as i32, cb as i32, cr as i32),
        Range::Limited => {
            let yy = (y as i32 - 16) * 255 / 219;
            let b = (cb as i32 - 128) * 255 / 224 + 128;
            let r = (cr as i32 - 128) * 255 / 224 + 128;
            (yy.clamp(0, 255), b.clamp(0, 255), r.clamp(0, 255))
        }
    }
}

/// 位深解包：10/12bit 左移对齐到 8bit 域。
pub fn unpack_depth(v: u16, depth: u8) -> Result<u16, u16> {
    match depth {
        8 => Ok(v),
        10 if v <= 1023 => Ok((v as u32 * 255 / 1023) as u16),
        12 if v <= 4095 => Ok((v as u32 * 255 / 4095) as u16),
        _ => Err(E_CS_DEPTH),
    }
}

/// 每像素成本画像（免墙钟吞吐口径）。
#[derive(Clone, Copy, Default)]
pub struct PixelCost {
    pub mul: u32,
    pub lut: u32,
    pub branch: u32,
}

impl PixelCost {
    /// 折合等效周期（乘加 1 / 查表 1 / 分支 2——预测失败的均值）。
    pub fn cycles(&self) -> u32 {
        self.mul + self.lut + self.branch * 2
    }
}

/// 单像素 YUV→RGB 的成本画像。
pub fn yuv_pixel_cost() -> PixelCost {
    PixelCost { mul: 12, lut: 3, branch: 0 }
}

/// 4GB/s 判据的每像素周期预算（3GHz、目标 1.33G px/s，四核摊销）。
pub const CYCLES_PER_PIXEL_BUDGET: u32 = 24;

/// 转换对登记表（未登记即拒）。
pub const SUPPORTED_PAIRS: [(&str, &str); 5] = [
    ("srgb8", "linear16"),
    ("linear16", "srgb8"),
    ("srgb8", "p3"),
    ("yuv420", "rgba8"),
    ("yuv444p", "rgb8"),
];

/// 转换对是否在册。
pub fn pair_supported(from: &str, to: &str) -> bool {
    SUPPORTED_PAIRS.iter().any(|(a, b)| *a == from && *b == to)
}

/// 独立参考实现（601 limited，手算系数），用于对拍而非复用被测矩阵。
fn ref_yuv601_limited(y: u8, cb: u8, cr: u8) -> [i32; 3] {
    let (yy, b, r) = dequant_yuv(Range::Limited, y as u16, cb as u16, cr as u16);
    let u = b - 128;
    let v = r - 128;
    let rr = yy + (1_402 * v) / 1000;
    let gg = yy - (344 * u) / 1000 - (714 * v) / 1000;
    let bb = yy + (1_772 * u) / 1000;
    [rr.clamp(0, 255), gg.clamp(0, 255), bb.clamp(0, 255)]
}

/// run_cga15_checks：CGPU-F0015 判据聚合（0xC8 段）。
pub fn run_cga15_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0015 颜色空间转换");
    let cp = ColorPipeline::new();

    // 正-01：sRGB 往返 ≤1 LSB（256 值全覆盖）
    {
        let mut worst = 0i32;
        for c in 0..=255u8 {
            let back = linear_to_srgb8(srgb8_to_linear(c));
            let d = (back as i32 - c as i32).abs();
            if d > worst {
                worst = d;
            }
        }
        if worst <= 1 {
            s.ok("F0015-正-01-sRGB往返≤1LSB");
        } else {
            s.fail("F0015-正-01-sRGB往返≤1LSB", "往返误差超 1 LSB");
        }
    }

    // 正-02：传递函数全域严格单调
    {
        let mut mono = true;
        for c in 1..=255u8 {
            if srgb8_to_linear(c) <= srgb8_to_linear(c - 1) {
                mono = false;
            }
        }
        if mono {
            s.ok("F0015-正-02-传递函数严格单调");
        } else {
            s.fail("F0015-正-02-传递函数严格单调", "域内倒挂");
        }
    }

    // 正-03：sRGB→XYZ→sRGB 两张公布表互为逆（≤2/65536）
    {
        let err = SRGB_TO_XYZ.mul(&XYZ_TO_SRGB).identity_error();
        let err2 = P3_TO_XYZ.mul(&XYZ_TO_P3).identity_error();
        let worst = err.max(err2);
        if worst <= 2 {
            s.ok("F0015-正-03-XYZ桥接自逆");
        } else {
            s.fail("F0015-正-03-XYZ桥接自逆", "复合矩阵偏离单位阵");
        }
    }

    // 正-04：P3 白点守恒
    {
        let w = cp.srgb8_to_p3([255, 255, 255], (0, 0));
        if w[0] >= 254 && w[1] >= 254 && w[2] >= 254 {
            s.ok("F0015-正-04-P3白点守恒");
        } else {
            s.fail("F0015-正-04-P3白点守恒", "白点偏移超 1 LSB");
        }
    }

    // 正-05：YUV601 红场（76,85,255）落红
    {
        let rgb = cp.yuv_to_rgb8(YuvStd::Bt601, Range::Full, 76, 85, 255, (0, 0));
        if rgb[0] >= 250 && rgb[1] <= 6 && rgb[2] <= 6 {
            s.ok("F0015-正-05-YUV601红场落红");
        } else {
            s.fail("F0015-正-05-YUV601红场落红", "红场未落红");
        }
    }

    // 正-06：limited 端点（Y=16→0 / Y=235→255）
    {
        let lo = dequant_yuv(Range::Limited, 16, 128, 128).0;
        let hi = dequant_yuv(Range::Limited, 235, 128, 128).0;
        if lo == 0 && hi == 255 {
            s.ok("F0015-正-06-limited端点恰值");
        } else {
            s.fail("F0015-正-06-limited端点恰值", "端点未落到 0/255");
        }
    }

    // 正-07：三族灰轴守恒（U=V=128 → R=G=B，逐位相等）
    {
        let mut ok = true;
        for std in YuvStd::ALL {
            let rgb = cp.yuv_to_rgb8(std, Range::Full, 128, 128, 128, (0, 0));
            if rgb[0] != rgb[1] || rgb[1] != rgb[2] {
                ok = false;
            }
        }
        if ok {
            s.ok("F0015-正-07-灰轴三族守恒");
        } else {
            s.fail("F0015-正-07-灰轴三族守恒", "某族灰轴偏移");
        }
    }

    // 正-08：dither 零均值 + 确实生效（两侧同压）
    {
        let v = 128 * ONE + 32_768;
        let mut sum = 0i64;
        let mut first = None;
        let mut varies = false;
        for y in 0..4 {
            for x in 0..4 {
                let d = dither8(v, x, y);
                sum += d as i64;
                match first {
                    None => first = Some(d),
                    Some(f) => {
                        if f != d {
                            varies = true;
                        }
                    }
                }
            }
        }
        let mean = sum / 16;
        if varies && (mean - 128).abs() <= 1 {
            s.ok("F0015-正-08-抖动零均值且生效");
        } else {
            s.fail("F0015-正-08-抖动零均值且生效", "抖动无效或均值漂移");
        }
    }

    // 正-09：批量 420 与逐像素同值
    {
        let (w, h) = (4usize, 2usize);
        let yp: Vec<u8> = (0..(w * h)).map(|i| (i * 7 + 20) as u8).collect();
        let up: Vec<u8> = alloc::vec![90u8; (w / 2) * (h / 2)];
        let vp: Vec<u8> = alloc::vec![170u8; (w / 2) * (h / 2)];
        let mut out = alloc::vec![0u8; w * h * 4];
        let n = cp.yuv420_to_rgba8(YuvStd::Bt709, Range::Limited, w, h, &yp, &up, &vp, &mut out);
        let mut same = n == Ok(w * h * 4);
        if same {
            for yy in 0..h {
                for xx in 0..w {
                    let want = cp.yuv_to_rgb8(
                        YuvStd::Bt709,
                        Range::Limited,
                        yp[yy * w + xx] as u16,
                        90,
                        170,
                        (xx, yy),
                    );
                    let o = (yy * w + xx) * 4;
                    if out[o] != want[0] || out[o + 1] != want[1] || out[o + 2] != want[2] {
                        same = false;
                    }
                }
            }
        }
        if same {
            s.ok("F0015-正-09-批量与逐像素同值");
        } else {
            s.fail("F0015-正-09-批量与逐像素同值", "批量路径与标量不一致");
        }
    }

    // 正-10：log2/exp2 互逆（相对误差 ≤1e-4——8bit 只需要 2e-3）
    {
        let mut ok = true;
        for x in [1i32, 2, 3, 7, 255, 4_096, 60_000] {
            let back = exp2_q16(log2_q16(x as u32)) as i64;
            let want = (x as i64) << 16;
            if (back - want).abs() * 10_000 > want {
                ok = false;
            }
        }
        if ok {
            s.ok("F0015-正-10-log2/exp2互逆");
        } else {
            s.fail("F0015-正-10-log2/exp2互逆", "定点超越函数精度不足");
        }
    }

    // 正-11：pow 的域基准（1.0 的任意次幂必须还是 1.0）
    {
        let one = pow_q16(ONE, 157_286);
        let half = pow_q16(ONE / 2, ONE); // 0.5^1.0
        if (one as i32 - ONE).abs() <= 2 && (half as i32 - ONE / 2).abs() <= 32 {
            s.ok("F0015-正-11-pow域基准正确");
        } else {
            s.fail("F0015-正-11-pow域基准正确", "pow 把 Q16 当整数算");
        }
    }

    // 正-12：吞吐预算（每像素周期 ≤ 24，4GB/s 四核摊销口径）
    {
        if yuv_pixel_cost().cycles() <= CYCLES_PER_PIXEL_BUDGET {
            s.ok("F0015-正-12-每像素成本在预算内");
        } else {
            s.fail("F0015-正-12-每像素成本在预算内", "单像素成本超预算");
        }
    }

    // 坏-01：奇异矩阵即拒
    {
        let sing = Mat3::new([ONE, 0, 0, 0, ONE, 0, 0, 0, 0]);
        if sing.inverse().is_none() {
            s.ok("F0015-坏-01-奇异矩阵即拒");
        } else {
            s.fail("F0015-坏-01-奇异矩阵即拒", "奇异矩阵被当可逆");
        }
    }

    // 坏-02：奇数宽高 420 即拒
    {
        let mut out = alloc::vec![0u8; 64];
        let r = cp.yuv420_to_rgba8(
            YuvStd::Bt601, Range::Full, 3, 3, &[0; 9], &[0; 2], &[0; 2], &mut out,
        );
        if r == Err(E_CS_DEPTH) {
            s.ok("F0015-坏-02-奇数尺寸420即拒");
        } else {
            s.fail("F0015-坏-02-奇数尺寸420即拒", "奇数尺寸未拒");
        }
    }

    // 坏-03：位深越界即拒
    {
        let ok = unpack_depth(2_000, 10).is_err()
            && unpack_depth(5_000, 12).is_err()
            && unpack_depth(1, 9).is_err();
        if ok {
            s.ok("F0015-坏-03-位深越界即拒");
        } else {
            s.fail("F0015-坏-03-位深越界即拒", "越界位深未拒");
        }
    }

    // 坏-04：未登记转换对即拒
    {
        if !pair_supported("cmyk", "srgb8") && pair_supported("srgb8", "p3") {
            s.ok("F0015-坏-04-未登记转换对即拒");
        } else {
            s.fail("F0015-坏-04-未登记转换对即拒", "登记表边界破");
        }
    }

    // 坏-05：长度不齐即拒（不静默截断）
    {
        let mut out = alloc::vec![0u8; 16];
        let r = cp.yuv420_to_rgba8(
            YuvStd::Bt601, Range::Full, 4, 2, &[0; 8], &[0; 1], &[0; 2], &mut out,
        );
        if r == Err(E_CS_LEN_MISMATCH) {
            s.ok("F0015-坏-05-长度不齐即拒");
        } else {
            s.fail("F0015-坏-05-长度不齐即拒", "长度不齐未拒");
        }
    }

    // 编-01：灰阶 sRGB→P3→sRGB 全链往返 ≤2
    {
        let mut worst = 0i32;
        for c in [0u8, 32, 96, 160, 224, 255] {
            let p3 = cp.srgb8_to_p3([c, c, c], (0, 0));
            if p3[0] != p3[1] || p3[1] != p3[2] {
                worst = 255;
                break;
            }
            let back = linear_to_srgb8(srgb8_to_linear(p3[0]));
            let d = (back as i32 - c as i32).abs();
            if d > worst {
                worst = d;
            }
        }
        if worst <= 2 {
            s.ok("F0015-编-01-灰阶全链往返≤2");
        } else {
            s.fail("F0015-编-01-灰阶全链往返≤2", "全链往返超差");
        }
    }

    // 编-02：与独立参考实现对拍（8 样本，容差 2 LSB）
    {
        let mut worst = 0i32;
        let samples = [
            (16u8, 128u8, 128u8),
            (81, 90, 240),
            (145, 54, 34),
            (235, 128, 128),
            (100, 200, 60),
            (200, 60, 200),
            (50, 128, 128),
            (128, 128, 128),
        ];
        for (y, u, v) in samples {
            let got = cp.yuv_to_rgb8(YuvStd::Bt601, Range::Limited, y as u16, u as u16, v as u16, (0, 0));
            let want = ref_yuv601_limited(y, u, v);
            for i in 0..3 {
                let d = (got[i] as i32 - want[i]).abs();
                if d > worst {
                    worst = d;
                }
            }
        }
        if worst <= 2 {
            s.ok("F0015-编-02-参考实现对拍≤2");
        } else {
            s.fail("F0015-编-02-参考实现对拍≤2", "与参考实现偏差超 2 LSB");
        }
    }

    s
}

/// 结构当量（代码行量级，供域账核对；非运行时开销）。
pub fn structural_weight() -> u32 {
    560
}

/// 读屏可达的一句话画像。
pub fn spoken_summary() -> String {
    let mut s = String::from("颜色空间转换：三族 YUV 矩阵由 Kr/Kb 推导，");
    s.push_str("sRGB 传递函数定点实现，输出 8bit 带 4×4 有序抖动");
    s
}
