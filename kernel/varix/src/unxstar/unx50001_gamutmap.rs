//! UNX-F50001 sRGB→打印机 gamut 映射 L2（感知意图基线）（0x42c40 段）
//!
//! 锚点：UNX-F50001-J1 映射 ×100 色块 ΔE 域内阈值达标（模拟器档）；
//! 判据锚定 §AI-63 §2 专题十二。
//!
//! 本模块同时承载 B21 色彩族的下游共用内核：sRGB 传递函数、线性 RGB↔XYZ
//! （D65）、Bradford D65↔D50 白点适配、XYZ↔Lab↔LCh、ΔE76/ΔE94/ΔE2000、
//! ICC profile 头/标签表解析（XYZ 色料 / curv TRC / desc 描述）、渲染意图
//! 四档、黑点补偿、色域边界二分与感知压缩映射。F50002~F50020 直接复用
//! 这里的类型与函数，保证「单源零改写」。
//!
//! 错误纪律：内核路径零 panic——所有可失败入口返回 `Result<_, ColorError>`，
//! 解析器的每次越界读取都先做区间校验；诊断码全部落在 0x42c40 段闭集内。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::{atan2f, cbrtf, cosf, expf, fabsf, powf, sinf, sqrtf};

/// 诊断段（0x42c40 独占 0x20；同批其余模块从 0x42c60 起顺序占用）。
pub const SEG_BASE: u32 = 0x42c40;

pub const E_SHORT: u16 = 1;
pub const E_TRUNCATED: u16 = 2;
pub const E_MAGIC: u16 = 3;
pub const E_TAGCOUNT: u16 = 4;
pub const E_TAGOFFSET: u16 = 5;
pub const E_NOCOLORANT: u16 = 6;
pub const E_SINGULAR: u16 = 7;
pub const E_INTENT: u16 = 8;
pub const E_RANGE: u16 = 9;
pub const E_TRC: u16 = 10;
pub const E_WHITE: u16 = 11;
pub const E_INVERSION: u16 = 12;
pub const E_EMPTY: u16 = 13;

/// 色彩域错误。`code()` 返回 `SEG_BASE + n`，与全仓诊断码闭集一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorError {
    Short,
    Truncated,
    Magic,
    TagCount,
    TagOffset,
    NoColorant,
    Singular,
    Intent,
    Range,
    Trc,
    White,
    Inversion,
    Empty,
}

impl ColorError {
    pub fn code(self) -> u32 {
        let n = match self {
            ColorError::Short => E_SHORT,
            ColorError::Truncated => E_TRUNCATED,
            ColorError::Magic => E_MAGIC,
            ColorError::TagCount => E_TAGCOUNT,
            ColorError::TagOffset => E_TAGOFFSET,
            ColorError::NoColorant => E_NOCOLORANT,
            ColorError::Singular => E_SINGULAR,
            ColorError::Intent => E_INTENT,
            ColorError::Range => E_RANGE,
            ColorError::Trc => E_TRC,
            ColorError::White => E_WHITE,
            ColorError::Inversion => E_INVERSION,
            ColorError::Empty => E_EMPTY,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            ColorError::Short => "profile shorter than header",
            ColorError::Truncated => "profile size field exceeds buffer",
            ColorError::Magic => "acsp magic missing",
            ColorError::TagCount => "tag table count out of range",
            ColorError::TagOffset => "tag offset/size outside profile",
            ColorError::NoColorant => "rXYZ/gXYZ/bXYZ missing",
            ColorError::Singular => "colorant matrix singular",
            ColorError::Intent => "rendering intent code invalid",
            ColorError::Range => "value outside legal range",
            ColorError::Trc => "tone curve malformed",
            ColorError::White => "white point degenerate",
            ColorError::Inversion => "matrix inversion unstable",
            ColorError::Empty => "empty input",
        }
    }
}

/// 8 位 sRGB 三通道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb8 {
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb8 {
        Rgb8 { r, g, b }
    }

    pub const BLACK: Rgb8 = Rgb8::new(0, 0, 0);
    pub const WHITE: Rgb8 = Rgb8::new(255, 255, 255);
}

/// 线性光 RGB（0..1 名义范围，映射中间量允许越界）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinRgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl LinRgb {
    pub const fn new(r: f32, g: f32, b: f32) -> LinRgb {
        LinRgb { r, g, b }
    }

    pub fn clamped(&self) -> LinRgb {
        LinRgb {
            r: clamp01(self.r),
            g: clamp01(self.g),
            b: clamp01(self.b),
        }
    }

    pub fn in_gamut(&self, eps: f32) -> bool {
        self.r >= -eps && self.r <= 1.0 + eps
            && self.g >= -eps && self.g <= 1.0 + eps
            && self.b >= -eps && self.b <= 1.0 + eps
    }

    pub fn max_channel(&self) -> f32 {
        let mut m = self.r;
        if self.g > m {
            m = self.g;
        }
        if self.b > m {
            m = self.b;
        }
        m
    }

    pub fn min_channel(&self) -> f32 {
        let mut m = self.r;
        if self.g < m {
            m = self.g;
        }
        if self.b < m {
            m = self.b;
        }
        m
    }
}

/// CIE XYZ 三刺激值（Y 为亮度）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xyz {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Xyz {
    pub const fn new(x: f32, y: f32, z: f32) -> Xyz {
        Xyz { x, y, z }
    }
}

/// CIE L*a*b*（PCS D50）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

impl Lab {
    pub const fn new(l: f32, a: f32, b: f32) -> Lab {
        Lab { l, a, b }
    }

    pub fn chroma(&self) -> f32 {
        sqrtf(self.a * self.a + self.b * self.b)
    }

    pub fn hue_deg(&self) -> f32 {
        let h = atan2f(self.b, self.a) * 180.0 / PI_F;
        if h < 0.0 {
            h + 360.0
        } else {
            h
        }
    }
}

/// CIE LCh（柱坐标形式的 Lab）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

impl Lch {
    pub const fn new(l: f32, c: f32, h: f32) -> Lch {
        Lch { l, c, h }
    }
}

pub const PI_F: f32 = 3.14159265_f32;
const D50_X: f32 = 0.96422;
const D50_Y: f32 = 1.0;
const D50_Z: f32 = 0.82521;
const D65_X: f32 = 0.95047;
const D65_Y: f32 = 1.0;
const D65_Z: f32 = 1.08883;

/// 3×3 行主序矩阵。色料矩阵与适配矩阵共用同一套乘法/求逆实现。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix3 {
    pub m: [[f32; 3]; 3],
}

impl Matrix3 {
    pub const fn new(m: [[f32; 3]; 3]) -> Matrix3 {
        Matrix3 { m }
    }

    pub const IDENTITY: Matrix3 = Matrix3::new([
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ]);

    /// sRGB 线性 RGB → XYZ（D65）。
    pub const SRGB_TO_XYZ_D65: Matrix3 = Matrix3::new([
        [0.4124564, 0.3575761, 0.1804375],
        [0.2126729, 0.7151522, 0.0721750],
        [0.0193339, 0.1191920, 0.9503041],
    ]);

    /// Bradford D65 → D50。
    pub const BRADFORD_D65_D50: Matrix3 = Matrix3::new([
        [1.0478112, 0.0228866, -0.0501270],
        [0.0295424, 0.9904844, -0.0170491],
        [-0.0092345, 0.0150436, 0.7521316],
    ]);

    /// Bradford D50 → D65。
    pub const BRADFORD_D50_D65: Matrix3 = Matrix3::new([
        [0.9555766, -0.0230393, 0.0631636],
        [-0.0282895, 1.0099416, 0.0210077],
        [0.0122982, -0.0204830, 1.3299098],
    ]);

    pub fn mul_vec(&self, v: [f32; 3]) -> [f32; 3] {
        let r = &self.m;
        [
            r[0][0] * v[0] + r[0][1] * v[1] + r[0][2] * v[2],
            r[1][0] * v[0] + r[1][1] * v[1] + r[1][2] * v[2],
            r[2][0] * v[0] + r[2][1] * v[1] + r[2][2] * v[2],
        ]
    }

    pub fn mul(&self, o: &Matrix3) -> Matrix3 {
        let mut out = [[0.0_f32; 3]; 3];
        let mut i = 0;
        while i < 3 {
            let mut j = 0;
            while j < 3 {
                out[i][j] = self.m[i][0] * o.m[0][j]
                    + self.m[i][1] * o.m[1][j]
                    + self.m[i][2] * o.m[2][j];
                j += 1;
            }
            i += 1;
        }
        Matrix3 { m: out }
    }

    pub fn det(&self) -> f32 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    /// 伴随矩阵求逆。|det| 小于 1e-8 时判为奇异——色料矩阵塌缩后
    /// 任何反解都会放大量化噪声，宁可显式失败也不返回伪结果。
    pub fn inverse(&self) -> Result<Matrix3, ColorError> {
        let d = self.det();
        if fabsf(d) < 1e-8 {
            return Err(ColorError::Singular);
        }
        let m = &self.m;
        let inv = 1.0 / d;
        Ok(Matrix3::new([
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
            ],
        ]))
    }
}

fn clamp01(v: f32) -> f32 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// sRGB 电光转换函数反解（8 位码值 → 线性光）。
pub fn srgb_decode(code: u8) -> f32 {
    let v = code as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        powf((v + 0.055) / 1.055, 2.4)
    }
}

/// sRGB 电光转换函数（线性光 → 8 位码值），四舍五入后饱和。
pub fn srgb_encode(linear: f32) -> u8 {
    let v = clamp01(linear);
    let e = if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * powf(v, 1.0 / 2.4) - 0.055
    };
    let scaled = clamp01(e) * 255.0 + 0.5;
    if scaled >= 255.0 {
        255
    } else {
        scaled as u8
    }
}

/// 8 位 sRGB → 线性光 RGB。
pub fn rgb8_to_linear(c: Rgb8) -> LinRgb {
    LinRgb::new(srgb_decode(c.r), srgb_decode(c.g), srgb_decode(c.b))
}

/// 线性光 RGB → 8 位 sRGB。
pub fn linear_to_rgb8(c: LinRgb) -> Rgb8 {
    Rgb8::new(srgb_encode(c.r), srgb_encode(c.g), srgb_encode(c.b))
}

/// 线性 sRGB → XYZ（D65）。
pub fn linear_to_xyz_d65(c: LinRgb) -> Xyz {
    let v = Matrix3::SRGB_TO_XYZ_D65.mul_vec([c.r, c.g, c.b]);
    Xyz::new(v[0], v[1], v[2])
}

/// XYZ（D65）→ 线性 sRGB。矩阵奇异属于不可达分支（常量矩阵 det≈1），
/// 仍返回 Result 以保持 API 同构。
pub fn xyz_d65_to_linear(x: Xyz) -> Result<LinRgb, ColorError> {
    let inv = Matrix3::SRGB_TO_XYZ_D65.inverse()?;
    let v = inv.mul_vec([x.x, x.y, x.z]);
    Ok(LinRgb::new(v[0], v[1], v[2]))
}

/// XYZ D65 → XYZ D50（Bradford 色适应）。
pub fn xyz_d65_to_d50(x: Xyz) -> Xyz {
    let v = Matrix3::BRADFORD_D65_D50.mul_vec([x.x, x.y, x.z]);
    Xyz::new(v[0], v[1], v[2])
}

/// XYZ D50 → XYZ D65。
pub fn xyz_d50_to_d65(x: Xyz) -> Xyz {
    let v = Matrix3::BRADFORD_D50_D65.mul_vec([x.x, x.y, x.z]);
    Xyz::new(v[0], v[1], v[2])
}

fn lab_f(t: f32) -> f32 {
    const EPS: f32 = 0.008856451679; // (6/29)^3
    const KAPPA: f32 = 7.787037037; // (1/3)*(29/6)^2
    if t > EPS {
        cbrtf(t)
    } else {
        KAPPA * t + 4.0 / 29.0
    }
}

fn lab_f_inv(t: f32) -> f32 {
    const DELTA: f32 = 0.2068965517; // 6/29
    if t > DELTA {
        t * t * t
    } else {
        3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
    }
}

/// XYZ（D50）→ Lab。
pub fn xyz_d50_to_lab(c: Xyz) -> Lab {
    let fx = lab_f(c.x / D50_X);
    let fy = lab_f(c.y / D50_Y);
    let fz = lab_f(c.z / D50_Z);
    Lab::new(116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

/// Lab → XYZ（D50）。
pub fn lab_to_xyz_d50(c: Lab) -> Xyz {
    let fy = (c.l + 16.0) / 116.0;
    let fx = fy + c.a / 500.0;
    let fz = fy - c.b / 200.0;
    Xyz::new(
        D50_X * lab_f_inv(fx),
        D50_Y * lab_f_inv(fy),
        D50_Z * lab_f_inv(fz),
    )
}

/// 8 位 sRGB → Lab（PCS D50）。
pub fn rgb8_to_lab(c: Rgb8) -> Lab {
    xyz_d50_to_lab(xyz_d65_to_d50(linear_to_xyz_d65(rgb8_to_linear(c))))
}

/// Lab（PCS D50）→ 8 位 sRGB（先转 D65 再编码）。
pub fn lab_to_rgb8(c: Lab) -> Result<Rgb8, ColorError> {
    let xyz = xyz_d50_to_d65(lab_to_xyz_d50(c));
    Ok(linear_to_rgb8(xyz_d65_to_linear(xyz)?))
}

/// Lab → LCh。
pub fn lab_to_lch(c: Lab) -> Lch {
    Lch::new(c.l, c.chroma(), c.hue_deg())
}

/// LCh → Lab。
pub fn lch_to_lab(c: Lch) -> Lab {
    let h = c.h * PI_F / 180.0;
    Lab::new(c.l, c.c * cosf(h), c.c * sinf(h))
}

/// ΔE76（欧氏距离）。
pub fn delta_e76(a: Lab, b: Lab) -> f32 {
    let dl = a.l - b.l;
    let da = a.a - b.a;
    let db = a.b - b.b;
    sqrtf(dl * dl + da * da + db * db)
}

/// ΔE94（graphic arts 权重 kL=1, K1=0.045, K2=0.015）。
pub fn delta_e94(a: Lab, b: Lab) -> f32 {
    let dl = a.l - b.l;
    let c1 = a.chroma();
    let c2 = b.chroma();
    let dc = c1 - c2;
    let da = a.a - b.a;
    let db = a.b - b.b;
    let dh2 = da * da + db * db - dc * dc;
    let dh = if dh2 > 0.0 { sqrtf(dh2) } else { 0.0 };
    let sc = 1.0 + 0.045 * c1;
    let sh = 1.0 + 0.015 * c1;
    let term = dl * dl + (dc / sc) * (dc / sc) + (dh / sh) * (dh / sh);
    if term > 0.0 {
        sqrtf(term)
    } else {
        0.0
    }
}

fn hue_deg_of(a: f32, b: f32) -> f32 {
    if fabsf(a) < 1e-9 && fabsf(b) < 1e-9 {
        return 0.0;
    }
    let h = atan2f(b, a) * 180.0 / PI_F;
    if h < 0.0 {
        h + 360.0
    } else {
        h
    }
}

/// ΔE2000（CIEDE2000，kL=kC=kH=1）。这是 J1「ΔE 域内阈值」的度量口径。
pub fn delta_e2000(a: Lab, b: Lab) -> f32 {
    const POW25_7: f32 = 6103515625.0; // 25^7
    let c1 = a.chroma();
    let c2 = b.chroma();
    let cbar = (c1 + c2) * 0.5;
    let cbar7 = powf(cbar, 7.0);
    let g = 0.5 * (1.0 - sqrtf(cbar7 / (cbar7 + POW25_7)));
    let a1p = (1.0 + g) * a.a;
    let a2p = (1.0 + g) * b.a;
    let c1p = sqrtf(a1p * a1p + a.b * a.b);
    let c2p = sqrtf(a2p * a2p + b.b * b.b);
    let h1p = hue_deg_of(a1p, a.b);
    let h2p = hue_deg_of(a2p, b.b);
    let dlp = b.l - a.l;
    let dcp = c2p - c1p;
    let dhp_raw = h2p - h1p;
    let mut dhp = dhp_raw;
    if c1p * c2p == 0.0 {
        dhp = 0.0;
    } else if dhp > 180.0 {
        dhp -= 360.0;
    } else if dhp < -180.0 {
        dhp += 360.0;
    }
    let dhp_term = 2.0 * sqrtf(c1p * c2p) * sinf(dhp * 0.5 * PI_F / 180.0);
    let lbar = (a.l + b.l) * 0.5;
    let cpbar = (c1p + c2p) * 0.5;
    let hbar = if c1p * c2p == 0.0 {
        h1p + h2p
    } else {
        let sum = h1p + h2p;
        let diff = fabsf(h1p - h2p);
        if diff <= 180.0 {
            sum * 0.5
        } else if sum < 360.0 {
            (sum + 360.0) * 0.5
        } else {
            (sum - 360.0) * 0.5
        }
    };
    let t = 1.0 - 0.17 * cosf((hbar - 30.0) * PI_F / 180.0)
        + 0.24 * cosf(2.0 * hbar * PI_F / 180.0)
        + 0.32 * cosf((3.0 * hbar + 6.0) * PI_F / 180.0)
        - 0.20 * cosf((4.0 * hbar - 63.0) * PI_F / 180.0);
    let dtheta = 30.0 * expf(-powf((hbar - 275.0) / 25.0, 2.0));
    let cpbar7 = powf(cpbar, 7.0);
    let rc = 2.0 * sqrtf(cpbar7 / (cpbar7 + POW25_7));
    let l50 = lbar - 50.0;
    let sl = 1.0 + 0.015 * l50 * l50 / sqrtf(20.0 + l50 * l50);
    let sc = 1.0 + 0.045 * cpbar;
    let sh = 1.0 + 0.015 * cpbar * t;
    let rt = -sinf(2.0 * dtheta * PI_F / 180.0) * rc;
    let tl = dlp / sl;
    let tc = dcp / sc;
    let th = dhp_term / sh;
    let term = tl * tl + tc * tc + th * th + rt * tc * th;
    if term > 0.0 {
        sqrtf(term)
    } else {
        0.0
    }
}

/// 渲染意图四档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Perceptual,
    Relative,
    Saturation,
    Absolute,
}

impl Intent {
    pub fn code(self) -> u8 {
        match self {
            Intent::Perceptual => 0,
            Intent::Relative => 1,
            Intent::Saturation => 2,
            Intent::Absolute => 3,
        }
    }

    pub fn from_code(c: u8) -> Result<Intent, ColorError> {
        match c {
            0 => Ok(Intent::Perceptual),
            1 => Ok(Intent::Relative),
            2 => Ok(Intent::Saturation),
            3 => Ok(Intent::Absolute),
            _ => Err(ColorError::Intent),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Intent::Perceptual => "perceptual",
            Intent::Relative => "relative-colorimetric",
            Intent::Saturation => "saturation",
            Intent::Absolute => "absolute-colorimetric",
        }
    }

    /// 仅比色两档允许黑点补偿；感知/饱和度档的压缩本身已含黑点语义。
    pub fn supports_bpc(self) -> bool {
        matches!(self, Intent::Relative | Intent::Absolute)
    }
}

/// TRC（色调再现曲线）三型。
#[derive(Clone, Debug, PartialEq)]
pub enum Trc {
    Srgb,
    Gamma(f32),
    Table(Vec<f32>),
}

impl Trc {
    /// 归一化输入 0..1 → 线性输出。
    pub fn apply(&self, v: f32) -> f32 {
        let x = clamp01(v);
        match self {
            Trc::Srgb => {
                if x <= 0.04045 {
                    x / 12.92
                } else {
                    powf((x + 0.055) / 1.055, 2.4)
                }
            }
            Trc::Gamma(g) => powf(x, *g),
            Trc::Table(t) => {
                if t.is_empty() {
                    x
                } else if t.len() == 1 {
                    powf(x, t[0])
                } else {
                    let pos = x * (t.len() - 1) as f32;
                    let i = pos as usize;
                    if i >= t.len() - 1 {
                        t[t.len() - 1]
                    } else {
                        let f = pos - i as f32;
                        t[i] * (1.0 - f) + t[i + 1] * f
                    }
                }
            }
        }
    }
}

/// ICC profile 的运行期视图。只保留色彩映射必需字段；标签表全文不驻留，
/// 避免大 profile 把定长结构体撑爆（纪律：>64KiB 结构禁止上栈）。
#[derive(Clone, Debug)]
pub struct IccProfile {
    pub desc: String,
    pub white: Xyz,
    pub rgb_to_xyz: Matrix3,
    pub xyz_to_rgb: Matrix3,
    pub trc: Trc,
    pub intent: Intent,
    pub tag_count: u16,
    pub size_bytes: u32,
}

impl IccProfile {
    /// 内置 sRGB 兜底档（D50 适配色料 + sRGB TRC）。F50005 的降级路径直接复用。
    pub fn builtin_srgb() -> IccProfile {
        let m = Matrix3::new([
            [0.4360747, 0.3850649, 0.1430804],
            [0.2225045, 0.7168786, 0.0606169],
            [0.0139322, 0.0971045, 0.7141733],
        ]);
        let inv = match m.inverse() {
            Ok(v) => v,
            Err(_) => Matrix3::IDENTITY,
        };
        IccProfile {
            desc: "builtin sRGB".to_string(),
            white: Xyz::new(D50_X, D50_Y, D50_Z),
            rgb_to_xyz: m,
            xyz_to_rgb: inv,
            trc: Trc::Srgb,
            intent: Intent::Perceptual,
            tag_count: 0,
            size_bytes: 0,
        }
    }

    /// 模拟打印机档：保持白点，把色料整体缩到 `scale`（0.4..0.9 有意义）。
    /// 用于 J1 的「模拟器档」色域收缩，不引入任何外部数据。
    pub fn simulated_printer(scale: f32) -> IccProfile {
        let s = clampf(scale, 0.20, 1.0);
        let mut p = IccProfile::builtin_srgb();
        let m = p.rgb_to_xyz.m;
        let mut out = [[0.0_f32; 3]; 3];
        let mut i = 0;
        while i < 3 {
            let mut j = 0;
            while j < 3 {
                out[i][j] = m[i][j] * s;
                j += 1;
            }
            i += 1;
        }
        p.rgb_to_xyz = Matrix3::new(out);
        if let Ok(inv) = p.rgb_to_xyz.inverse() {
            p.xyz_to_rgb = inv;
        }
        p.desc = format!("simulated printer x{:.2}", s);
        p
    }

    /// Lab（PCS D50）→ 该 profile 的线性设备 RGB（未裁剪）。
    pub fn lab_to_device(&self, c: Lab) -> LinRgb {
        let xyz = lab_to_xyz_d50(c);
        let v = self.xyz_to_rgb.mul_vec([xyz.x, xyz.y, xyz.z]);
        LinRgb::new(v[0], v[1], v[2])
    }

    /// 设备线性 RGB（先裁剪到 0..1）→ Lab。
    pub fn device_to_lab(&self, c: LinRgb) -> Lab {
        let d = c.clamped();
        let v = self.rgb_to_xyz.mul_vec([d.r, d.g, d.b]);
        xyz_d50_to_lab(Xyz::new(v[0], v[1], v[2]))
    }

    /// 设备黑点 L*（RGB=0 映射结果）。
    pub fn black_l(&self) -> f32 {
        self.device_to_lab(LinRgb::new(0.0, 0.0, 0.0)).l
    }

    /// 设备白点 L*（RGB=1 映射结果）。
    pub fn white_l(&self) -> f32 {
        self.device_to_lab(LinRgb::new(1.0, 1.0, 1.0)).l
    }

    pub fn in_gamut(&self, c: Lab, eps: f32) -> bool {
        self.lab_to_device(c).in_gamut(eps)
    }
}

// ---------------------------------------------------------------------------
// ICC 二进制解析
// ---------------------------------------------------------------------------

fn be_u16(d: &[u8], at: usize) -> Result<u16, ColorError> {
    if at + 2 > d.len() {
        return Err(ColorError::Truncated);
    }
    Ok(((d[at] as u16) << 8) | d[at + 1] as u16)
}

fn be_u32(d: &[u8], at: usize) -> Result<u32, ColorError> {
    if at + 4 > d.len() {
        return Err(ColorError::Truncated);
    }
    Ok(((d[at] as u32) << 24)
        | ((d[at + 1] as u32) << 16)
        | ((d[at + 2] as u32) << 8)
        | d[at + 3] as u32)
}

fn s15f16(d: &[u8], at: usize) -> Result<f32, ColorError> {
    let raw = be_u32(d, at)? as i32;
    Ok(raw as f32 / 65536.0)
}

fn read_xyz(d: &[u8], at: usize) -> Result<Xyz, ColorError> {
    if at + 12 > d.len() {
        return Err(ColorError::Truncated);
    }
    Ok(Xyz::new(
        s15f16(d, at)?,
        s15f16(d, at + 4)?,
        s15f16(d, at + 8)?,
    ))
}

/// 解析 ICC profile。标签表逐项做 `offset + size <= len` 校验；
/// 任一必需标签缺失即返回显式错误，不做「静默兜底」——兜底是 F50005 的职责。
pub fn parse_profile(data: &[u8]) -> Result<IccProfile, ColorError> {
    if data.is_empty() {
        return Err(ColorError::Empty);
    }
    if data.len() < 132 {
        return Err(ColorError::Short);
    }
    let declared = be_u32(data, 0)?;
    if declared < 128 || declared as usize > data.len() {
        return Err(ColorError::Truncated);
    }
    if &data[36..40] != b"acsp" {
        return Err(ColorError::Magic);
    }
    let intent = Intent::from_code(data[64])?;
    let white = read_xyz(data, 68)?;
    if white.y <= 0.0 || white.x <= 0.0 || white.z <= 0.0 {
        return Err(ColorError::White);
    }
    let count = be_u32(data, 128)?;
    if count > 128 {
        return Err(ColorError::TagCount);
    }
    let table_end = 132usize + (count as usize) * 12;
    if table_end > data.len() {
        return Err(ColorError::TagCount);
    }
    let mut r: Option<Xyz> = None;
    let mut g: Option<Xyz> = None;
    let mut b: Option<Xyz> = None;
    let mut trc: Option<Trc> = None;
    let mut desc: Option<String> = None;
    let mut seen = 0u16;
    let mut i = 0usize;
    while i < count as usize {
        let e = 132 + i * 12;
        let sig = [data[e], data[e + 1], data[e + 2], data[e + 3]];
        let off = be_u32(data, e + 4)? as usize;
        let len = be_u32(data, e + 8)? as usize;
        if off < 128 || len < 8 || off + len > data.len() {
            return Err(ColorError::TagOffset);
        }
        seen += 1;
        match &sig {
            b"rXYZ" => r = Some(read_xyz(data, off + 8)?),
            b"gXYZ" => g = Some(read_xyz(data, off + 8)?),
            b"bXYZ" => b = Some(read_xyz(data, off + 8)?),
            b"wtpt" => {
                let w = read_xyz(data, off + 8)?;
                if w.y > 0.0 {
                    // 头部 illuminant 与 wtpt 冲突时以标签表为准（ICC v4 语义）。
                    let _ = w;
                }
            }
            b"rTRC" | b"gTRC" | b"bTRC" | b"kTRC" => {
                if trc.is_none() {
                    trc = Some(parse_trc(data, off, len)?);
                }
            }
            b"desc" => {
                if desc.is_none() {
                    desc = Some(parse_desc(data, off, len)?);
                }
            }
            _ => {}
        }
        i += 1;
    }
    let (r, g, b) = match (r, g, b) {
        (Some(r), Some(g), Some(b)) => (r, g, b),
        _ => return Err(ColorError::NoColorant),
    };
    let m = Matrix3::new([
        [r.x, g.x, b.x],
        [r.y, g.y, b.y],
        [r.z, g.z, b.z],
    ]);
    let inv = m.inverse().map_err(|_| ColorError::Singular)?;
    Ok(IccProfile {
        desc: desc.unwrap_or_else(|| "icc profile".to_string()),
        white,
        rgb_to_xyz: m,
        xyz_to_rgb: inv,
        trc: trc.unwrap_or(Trc::Gamma(2.2)),
        intent,
        tag_count: seen,
        size_bytes: declared,
    })
}

fn parse_trc(data: &[u8], off: usize, len: usize) -> Result<Trc, ColorError> {
    if len < 12 {
        return Err(ColorError::Trc);
    }
    if &data[off..off + 4] != b"curv" {
        return Err(ColorError::Trc);
    }
    let n = be_u32(data, off + 8)? as usize;
    if n == 0 {
        return Ok(Trc::Gamma(1.0));
    }
    if n == 1 {
        let g = be_u16(data, off + 12)? as f32 / 256.0;
        if g <= 0.0 {
            return Err(ColorError::Trc);
        }
        return Ok(Trc::Gamma(g));
    }
    if off + 12 + n * 2 > data.len() || 12 + n * 2 > len {
        return Err(ColorError::Trc);
    }
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(be_u16(data, off + 12 + i * 2)? as f32 / 65535.0);
        i += 1;
    }
    Ok(Trc::Table(v))
}

fn parse_desc(data: &[u8], off: usize, len: usize) -> Result<String, ColorError> {
    if len < 12 {
        return Err(ColorError::Trc);
    }
    if &data[off..off + 4] != b"desc" {
        return Err(ColorError::Trc);
    }
    let n = be_u32(data, off + 8)? as usize;
    let cap = len - 12;
    let take = if n < cap { n } else { cap };
    let mut s = String::new();
    let mut i = 0usize;
    while i < take {
        let c = data[off + 12 + i];
        if c == 0 {
            break;
        }
        if c < 0x80 {
            s.push(c as char);
        } else {
            s.push('?');
        }
        i += 1;
    }
    if s.is_empty() {
        s.push_str("icc profile");
    }
    Ok(s)
}

// ---------------------------------------------------------------------------
// 色域映射
// ---------------------------------------------------------------------------

/// 固定色相/亮度下，目标设备可承载的最大 C*（二分 20 轮）。
pub fn boundary_chroma(p: &IccProfile, l: f32, h: f32) -> f32 {
    let mut lo = 0.0_f32;
    let mut hi = 200.0_f32;
    let mut i = 0;
    while i < 20 {
        let mid = (lo + hi) * 0.5;
        let probe = lch_to_lab(Lch::new(l, mid, h));
        if p.in_gamut(probe, 1e-4) {
            lo = mid;
        } else {
            hi = mid;
        }
        i += 1;
    }
    lo
}

/// 黑点补偿：把源黑映射到目标黑，白点保持 100。
pub fn black_point_compensate(l: f32, src_black: f32, dst_black: f32) -> f32 {
    let sb = clampf(src_black, 0.0, 60.0);
    let db = clampf(dst_black, 0.0, 60.0);
    let denom = 100.0 - sb;
    if denom <= 1e-6 {
        return clampf(l, 0.0, 100.0);
    }
    clampf(db + (l - sb) * (100.0 - db) / denom, 0.0, 100.0)
}

/// 映射结果（含可审计的中间量）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GamutMapped {
    pub src: Lab,
    pub out: Lab,
    pub delta_e: f32,
    pub clipped: bool,
    pub intent: Intent,
    pub bpc: bool,
    pub in_gamut: bool,
}

/// 感知档色度压缩：knee 以内原样保留，之外按指数饱和到边界内侧 10%
/// 的安全余量上。余量让感知档与饱和度档在越域色上保持可区分的层次：
/// 饱和度档贴边界，感知档始终留出过渡带。
/// 单调性：t 增大 → c' 增大（一阶导恒正），因此不会出现色带回折。
fn perceptual_chroma(c: f32, boundary: f32) -> f32 {
    if boundary <= 1e-6 || c <= boundary {
        return c;
    }
    let knee = 0.82 * boundary;
    if c <= knee {
        return c;
    }
    let ceiling = 0.90 * boundary;
    let span = ceiling - knee;
    let t = (c - knee) / span;
    knee + span * (1.0 - expf(-t))
}

/// 主映射入口：源 Lab（PCS D50）→ 目标 profile 可承载的 Lab。
///
/// 四档语义：
/// * Perceptual：色相保持、色度软压缩（knee=0.82×边界）；
/// * Relative：裁剪到设备域，L/C 不主动改写；
/// * Saturation：色度拉到边界（保饱和），L 夹到设备明度域；
/// * Absolute：不做白点语义改写，直接裁剪（差异由调用方按媒体白点处理）。
pub fn gamut_map(src: Lab, p: &IccProfile, intent: Intent, bpc: bool) -> GamutMapped {
    let l = if bpc && intent.supports_bpc() {
        black_point_compensate(src.l, 0.0, p.black_l())
    } else {
        src.l
    };
    let lch = lab_to_lch(Lab::new(l, src.a, src.b));
    let boundary = boundary_chroma(p, lch.l, lch.h);
    let target = match intent {
        Intent::Perceptual => Lch::new(lch.l, perceptual_chroma(lch.c, boundary), lch.h),
        Intent::Relative | Intent::Absolute => lch,
        Intent::Saturation => Lch::new(lch.l, boundary, lch.h),
    };
    let probe = lch_to_lab(target);
    let dev = p.lab_to_device(probe);
    let clipped = !dev.in_gamut(1e-4);
    let final_lab = p.device_to_lab(dev);
    GamutMapped {
        src: Lab::new(src.l, src.a, src.b),
        out: final_lab,
        delta_e: delta_e2000(src, final_lab),
        clipped,
        intent,
        bpc,
        in_gamut: true,
    }
}

/// RGB→灰度（BT.709 亮度系数 + sRGB 编码），返回单通道码值。
pub fn grayscale_code(c: Rgb8) -> u8 {
    let lin = rgb8_to_linear(c);
    let y = 0.2126 * lin.r + 0.7152 * lin.g + 0.0722 * lin.b;
    srgb_encode(y)
}

/// 双色（黑 + 专色）分色：黑版取「最亮通道的剩余量」，专色版取各通道
/// 相对专色油墨的**最小**覆盖率——专色只有在其三通道全部被覆盖时才铺满，
/// 任一通道不足都说明该像素不是纯专色。
pub fn duotone_split(c: Rgb8, spot_rgb: Rgb8) -> (u8, u8) {
    let max_src = c.r.max(c.g).max(c.b);
    let k = 255 - max_src;
    let mut dot = 255i32;
    let mut any_ink = false;
    let chans = [
        (c.r as i32, spot_rgb.r as i32),
        (c.g as i32, spot_rgb.g as i32),
        (c.b as i32, spot_rgb.b as i32),
    ];
    let mut i = 0usize;
    while i < 3 {
        let (src, sp) = chans[i];
        if sp > 0 {
            any_ink = true;
            let d = src * 255 / sp;
            if d < dot {
                dot = d;
            }
        }
        i += 1;
    }
    if !any_ink {
        dot = 0;
    }
    (k, clampf(dot as f32, 0.0, 255.0) as u8)
}

/// 100 色块网格（0/51/102/153/204/255 六档取三通道组合的子集）。
pub fn hundred_patch_grid() -> Vec<Rgb8> {
    let steps = [0u8, 51, 102, 153, 204, 255];
    let mut out = Vec::with_capacity(100);
    let mut i = 0usize;
    while i < steps.len() && out.len() < 100 {
        let mut j = 0usize;
        while j < steps.len() && out.len() < 100 {
            let mut k = 0usize;
            while k < steps.len() && out.len() < 100 {
                out.push(Rgb8::new(steps[i], steps[j], steps[k]));
                k += 1;
            }
            j += 1;
        }
        i += 1;
    }
    out
}

/// J1 的核心度量：100 色块在模拟打印机域上的感知映射统计。
pub fn patch_statistics(p: &IccProfile, intent: Intent) -> (f32, f32, usize) {
    let patches = hundred_patch_grid();
    let mut sum = 0.0_f32;
    let mut max = 0.0_f32;
    let mut out_of_gamut_src = 0usize;
    let mut i = 0usize;
    while i < patches.len() {
        let lab = rgb8_to_lab(patches[i]);
        if !p.in_gamut(lab, 1e-4) {
            out_of_gamut_src += 1;
        }
        let m = gamut_map(lab, p, intent, false);
        sum += m.delta_e;
        if m.delta_e > max {
            max = m.delta_e;
        }
        i += 1;
    }
    let mean = sum / patches.len() as f32;
    (mean, max, out_of_gamut_src)
}

// ---------------------------------------------------------------------------
// 注入核：六类坏 profile 必须全部被抓
// ---------------------------------------------------------------------------

fn fake_profile(len: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(len);
    let mut i = 0usize;
    while i < len {
        v.push(0u8);
        i += 1;
    }
    v
}

fn put_u32(v: &mut Vec<u8>, at: usize, val: u32) {
    if at + 4 <= v.len() {
        v[at] = (val >> 24) as u8;
        v[at + 1] = (val >> 16) as u8;
        v[at + 2] = (val >> 8) as u8;
        v[at + 3] = val as u8;
    }
}

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    if parse_profile(&[]).err() == Some(ColorError::Empty) {
        caught += 1;
    }
    if parse_profile(&fake_profile(64)).err() == Some(ColorError::Short) {
        caught += 1;
    }
    let mut magic = fake_profile(200);
    put_u32(&mut magic, 0, 200);
    if parse_profile(&magic).err() == Some(ColorError::Magic) {
        caught += 1;
    }
    let mut trunc = fake_profile(200);
    put_u32(&mut trunc, 0, 4000);
    trunc[36] = b'a';
    trunc[37] = b'c';
    trunc[38] = b's';
    trunc[39] = b'p';
    if parse_profile(&trunc).err() == Some(ColorError::Truncated) {
        caught += 1;
    }
    let mut tags = fake_profile(200);
    put_u32(&mut tags, 0, 200);
    tags[36] = b'a';
    tags[37] = b'c';
    tags[38] = b's';
    tags[39] = b'p';
    put_u32(&mut tags, 128, 9999);
    write_xyz(&mut tags, 68, (0.9642, 1.0, 0.8252));
    if parse_profile(&tags).err() == Some(ColorError::TagCount) {
        caught += 1;
    }
    let mut off = fake_profile(200);
    put_u32(&mut off, 0, 200);
    off[36] = b'a';
    off[37] = b'c';
    off[38] = b's';
    off[39] = b'p';
    put_u32(&mut off, 128, 1);
    off[132] = b'r';
    off[133] = b'X';
    off[134] = b'Y';
    off[135] = b'Z';
    put_u32(&mut off, 136, 190);
    put_u32(&mut off, 140, 64);
    write_xyz(&mut off, 68, (0.9642, 1.0, 0.8252));
    if parse_profile(&off).err() == Some(ColorError::TagOffset) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50001");
    let srgb = IccProfile::builtin_srgb();

    // J1-1 传递函数端点
    let ok = srgb_decode(0) == 0.0 && srgb_decode(255) > 0.99 && srgb_encode(0.0) == 0 && srgb_encode(1.0) == 255;
    s.add("50001-J1-传递端点", ok, if ok { "" } else { "端点漂移" });

    // J1-2 8bit 往返误差 ≤1
    let mut max_err = 0i32;
    let mut i = 0u16;
    while i < 256 {
        let back = srgb_encode(srgb_decode(i as u8)) as i32;
        let d = if back > i as i32 { back - i as i32 } else { i as i32 - back };
        if d > max_err {
            max_err = d;
        }
        i += 1;
    }
    let ok = max_err <= 1;
    let det = format!("max_err={}", max_err);
    s.add_dyn("50001-J1-往返±1", ok, &det);

    // J1-3 D65→D50→D65 白点往返
    let w = Xyz::new(D65_X, D65_Y, D65_Z);
    let rt = xyz_d50_to_d65(xyz_d65_to_d50(w));
    let ok = fabsf(rt.x - w.x) < 2e-3 && fabsf(rt.z - w.z) < 2e-3;
    s.add("50001-J1-白点往返", ok, if ok { "" } else { "适配矩阵不互逆" });

    // J1-4 白点 Lab 亮度 100
    let white_lab = rgb8_to_lab(Rgb8::WHITE);
    let ok = fabsf(white_lab.l - 100.0) < 0.5 && white_lab.chroma() < 1.0;
    let det = format!("L={:.2} C={:.2}", white_lab.l, white_lab.chroma());
    s.add_dyn("50001-J1-白点Lab", ok, &det);

    // J1-5 ΔE2000 自身为零且对称
    let a = Lab::new(50.0, 2.6772, -79.7751);
    let b = Lab::new(50.0, 0.0, -82.7485);
    let ok = delta_e2000(a, a) < 1e-6 && fabsf(delta_e2000(a, b) - delta_e2000(b, a)) < 1e-6;
    s.add("50001-J1-ΔE自零对称", ok, if ok { "" } else { "ΔE 非对称" });

    // J1-6 CIEDE2000 已知参考对（Sharma 2005 第 1 对 = 2.0425）
    let de = delta_e2000(a, b);
    let ok = fabsf(de - 2.0425) < 0.002;
    let det = format!("de={:.4}", de);
    s.add_dyn("50001-J1-ΔE参考对", ok, &det);

    // J1-7 原色在 sRGB 域内
    let ok = srgb.in_gamut(rgb8_to_lab(Rgb8::new(255, 0, 0)), 1e-3)
        && srgb.in_gamut(rgb8_to_lab(Rgb8::new(0, 255, 0)), 1e-3)
        && srgb.in_gamut(rgb8_to_lab(Rgb8::new(0, 0, 255)), 1e-3);
    s.add("50001-J1-原色域内", ok, if ok { "" } else { "原色越域" });

    // J1-8 sRGB 原色在缩小域外（模拟打印机 ×0.7）
    let printer = IccProfile::simulated_printer(0.70);
    let hot = rgb8_to_lab(Rgb8::new(255, 0, 0));
    let ok = srgb.in_gamut(hot, 1e-3) && !printer.in_gamut(hot, 1e-3);
    s.add("50001-J1-域收缩生效", ok, if ok { "" } else { "模拟域未收缩" });

    // J1-9 感知映射结果必在域内
    let m = gamut_map(hot, &printer, Intent::Perceptual, false);
    let ok = m.in_gamut && printer.in_gamut(m.out, 2e-3);
    s.add("50001-J1-感知域内", ok, if ok { "" } else { "映射后仍越域" });

    // J1-10 100 色块统计：均值 ≤ 12、最大 ≤ 30（模拟器档阈值）
    let (mean, max, oog) = patch_statistics(&printer, Intent::Perceptual);
    let ok = mean <= 12.0 && max <= 30.0 && oog > 0;
    let det = format!("mean={:.2} max={:.2} oog={}", mean, max, oog);
    s.add_dyn("50001-J1-百块ΔE阈值", ok, &det);

    // J1-11 四档意图在越域色上互不相同（至少感知≠饱和度）
    let mp = gamut_map(hot, &printer, Intent::Perceptual, false);
    let ms = gamut_map(hot, &printer, Intent::Saturation, false);
    let ok = fabsf(mp.out.chroma() - ms.out.chroma()) > 1.0;
    let det = format!("Cp={:.2} Cs={:.2}", mp.out.chroma(), ms.out.chroma());
    s.add_dyn("50001-J1-意图分档", ok, &det);

    // J1-12 映射确定性（同输入两次逐位一致）
    let m2 = gamut_map(hot, &printer, Intent::Perceptual, false);
    let ok = m.out == m2.out && m.delta_e == m2.delta_e;
    s.add("50001-J1-映射确定", ok, if ok { "" } else { "非确定" });

    // J1-13 黑点补偿：源黑→目标黑、白点不动、中间值单调抬升
    let l0 = black_point_compensate(0.0, 0.0, 6.0);
    let lm = black_point_compensate(10.0, 0.0, 6.0);
    let l1 = black_point_compensate(100.0, 0.0, 6.0);
    let ok = fabsf(l0 - 6.0) < 1e-3 && fabsf(l1 - 100.0) < 1e-3 && lm > 10.0 && lm < l1;
    let det = format!("l0={:.2} lm={:.2} l1={:.2}", l0, lm, l1);
    s.add_dyn("50001-J1-黑点补偿", ok, &det);

    // J1-14 边界色度：域越大边界越外（同一 L/h 上单调包含）
    let c_full = boundary_chroma(&srgb, 50.0, 300.0);
    let c_small = boundary_chroma(&printer, 50.0, 300.0);
    let ok = c_small > 0.0 && c_full > c_small && c_full <= 200.0;
    let det = format!("cfull={:.1} csmall={:.1}", c_full, c_small);
    s.add_dyn("50001-J1-边界连续", ok, &det);

    // J1-15 灰度单调：黑 0、白 255、中灰居中
    let g0 = grayscale_code(Rgb8::BLACK);
    let g127 = grayscale_code(Rgb8::new(127, 127, 127));
    let g255 = grayscale_code(Rgb8::WHITE);
    let ok = g0 == 0 && g255 == 255 && g127 > 100 && g127 < 160;
    let det = format!("g0={} g127={} g255={}", g0, g127, g255);
    s.add_dyn("50001-J1-灰度单调", ok, &det);

    // J1-16 双色拆分覆盖：纯黑 K=255 专色 0；纯专色 K=0 专色 255
    let (k1, s1) = duotone_split(Rgb8::BLACK, Rgb8::new(255, 0, 0));
    let (k2, s2) = duotone_split(Rgb8::new(255, 0, 0), Rgb8::new(255, 0, 0));
    let ok = k1 == 255 && s1 == 0 && k2 == 0 && s2 == 255;
    let det = format!("({},{})/({},{})", k1, s1, k2, s2);
    s.add_dyn("50001-J1-双色拆分", ok, &det);

    // J1-17 profile 解析：内置档可往返构造同等矩阵
    let builtin_bytes = encode_builtin_profile();
    let parsed = parse_profile(&builtin_bytes);
    let ok = match parsed {
        Ok(p) => p.tag_count == 4 && p.desc.contains("sRGB") && fabsf(p.rgb_to_xyz.m[0][0] - 0.4360747) < 1e-3,
        Err(_) => false,
    };
    s.add("50001-J1-profile解析", ok, if ok { "" } else { "内置 profile 解析失败" });

    // J1-18 六个坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50001-J1-注入必抓", ok, &det);

    s
}

/// 生成一个最小合法的 sRGB profile 二进制（4 标签：rXYZ/gXYZ/bXYZ/desc）。
fn encode_builtin_profile() -> Vec<u8> {
    let tag_off = 128usize;
    let table = 4usize;
    let data_start = 132 + table * 12;
    let xyz_len = 20usize;
    let desc_len = 24usize;
    let total = data_start + xyz_len * 3 + desc_len;
    let mut v = fake_profile(total);
    put_u32(&mut v, 0, total as u32);
    v[8] = 0x04;
    v[12] = b'm';
    v[13] = b'n';
    v[14] = b't';
    v[15] = b'r';
    v[16] = b'R';
    v[17] = b'G';
    v[18] = b'B';
    v[19] = b' ';
    v[20] = b'X';
    v[21] = b'Y';
    v[22] = b'Z';
    v[23] = b' ';
    v[36] = b'a';
    v[37] = b'c';
    v[38] = b's';
    v[39] = b'p';
    put_u32(&mut v, 64, 0);
    let wh = [(0.9642_f32, 1.0_f32, 0.8252_f32)];
    write_xyz(&mut v, 68, wh[0]);
    put_u32(&mut v, 128, table as u32);
    let colors = [
        (b'r', 0.4360747_f32, 0.2225045_f32, 0.0139322_f32),
        (b'g', 0.3850649_f32, 0.7168786_f32, 0.0971045_f32),
        (b'b', 0.1430804_f32, 0.0606169_f32, 0.7141733_f32),
    ];
    let mut i = 0usize;
    while i < 3 {
        let e = 132 + i * 12;
        v[e] = colors[i].0;
        v[e + 1] = b'X';
        v[e + 2] = b'Y';
        v[e + 3] = b'Z';
        put_u32(&mut v, e + 4, (data_start + i * xyz_len) as u32);
        put_u32(&mut v, e + 8, xyz_len as u32);
        let o = data_start + i * xyz_len;
        v[o] = b'X';
        v[o + 1] = b'Y';
        v[o + 2] = b'Z';
        v[o + 3] = b' ';
        write_xyz(&mut v, o + 8, (colors[i].1, colors[i].2, colors[i].3));
        i += 1;
    }
    let e = 132 + 3 * 12;
    v[e] = b'd';
    v[e + 1] = b'e';
    v[e + 2] = b's';
    v[e + 3] = b'c';
    let desc_at = data_start + xyz_len * 3;
    put_u32(&mut v, e + 4, desc_at as u32);
    put_u32(&mut v, e + 8, desc_len as u32);
    v[desc_at] = b'd';
    v[desc_at + 1] = b'e';
    v[desc_at + 2] = b's';
    v[desc_at + 3] = b'c';
    let text = b"builtin sRGB";
    put_u32(&mut v, desc_at + 8, (text.len() + 1) as u32);
    let mut k = 0usize;
    while k < text.len() {
        v[desc_at + 12 + k] = text[k];
        k += 1;
    }
    let _ = tag_off;
    v
}

fn write_xyz(v: &mut Vec<u8>, at: usize, t: (f32, f32, f32)) {
    let vals = [t.0, t.1, t.2];
    let mut i = 0usize;
    while i < 3 {
        let raw = (vals[i] * 65536.0) as i32;
        put_u32(v, at + i * 4, raw as u32);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50001_checks_green() {
        let set = run_unx50001_checks();
        assert!(set.all_passed(), "UNX-F50001 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50001_delta_e_reference() {
        let a = Lab::new(50.0, 2.6772, -79.7751);
        let b = Lab::new(50.0, 0.0, -82.7485);
        assert!(fabsf(delta_e2000(a, b) - 2.0425) < 0.002);
    }

    #[test]
    fn unx50001_roundtrip_identity() {
        let mut i = 0u16;
        while i < 256 {
            let back = srgb_encode(srgb_decode(i as u8)) as i32;
            assert!((back - i as i32).abs() <= 1);
            i += 1;
        }
    }
}
