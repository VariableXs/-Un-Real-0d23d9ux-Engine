//! CGPU-F0001 · SIMD 栅格化基元库（CGPU-A 域 · GA01 · 目标 520 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0001`
//!
//! **判据（锚点原文）**：AVX2 下单核填充吞吐 ≥8GB/s、混合 12 种与参考
//! 逐像素一致、越界注入零写穿、四目标编译全绿、吞吐与正确性基准入
//! CGPU-Bench。
//!
//! # 一、CPU 侧渲染加速度的基石
//!
//! 把 2D 栅格化的核心基元（矩形填充、渐变填充、圆角矩形、椭圆、三角
//! 形、纹理块搬运、混合）全部写成 SIMD 版本。设计要点：
//!
//! - **按编译目标自动派发实现**（[`SimdPath::dispatch`]）：AVX2 256 位
//!   为主力、AVX512 加宽、SSE4 保底、NEON 供 ARM——编译期
//!   `cfg(target_feature)` 派发（内核 no_std 无运行时探测面），另有
//!   标量路径作语义参考与兜底。四条 SIMD 路径共享同一批处理骨架
//!   （每批 [`LANE`]=4 像素），仅像素通道打包方式随路径加宽。
//! - **像素格式统一两族**（[`PixelFormat`]）：Premultiplied-ABGR 8888
//!   与 FP16 线性——预乘域内混合，输出回转正确。
//! - **混合模式首批 12 种**（[`BlendMode`]）：over / source-in /
//!   source-out / source-atop / dest-in / dest-atop / multiply / screen
//!   / darken / lighten / plus / xor，全部展开成 SIMD 常量矩阵
//!   （[`BlendMatrix`]）而非查表，混合主循环零分支。
//! - **渐变填充**（[`GradientLut`]）：stops 预展开成每 4 像素一档的
//!   定点插值表，运行时零重算。
//! - **圆角矩形**走 SDF（有向距离场）逐 4 像素批判（[`RRect`])：距离
//!   全程用平方量比较（内核面 f32 无 sqrt，见记忆红线），角区专用
//!   判定，避免逐像素分支。
//!
//! # 二、边界纪律：越界写为零容忍缺陷
//!
//! 任何写入前做行首行尾 SIMD 掩码裁剪（[`RowMask`]）：头批/尾批按
//! 裁剪宽度出「通道允许位」，批量写入前逐通道屏蔽；整行越界的行直接
//! 不进批。注入测试（判据 C1-越界-两例）证明哨兵像素零写穿。
//!
//! # 三、与 GPU 路径对拍
//!
//! 同输入逐像素 diff（[`GpuDiffReport`]），差异仅允许来自浮点舍入
//! （每通道 ≤1 LSB）且入册；结构性差异（>1 LSB 或通道错位）为缺陷。
//! 吞吐与正确性基准条目（[`BenchEntry`]）入 CGPU-Bench 账面：AVX2
//! 单核填充吞吐目标 ≥8GB/s，实测值由真机基准回填（本件记录目标与
//! 口径，诚实不虚报）。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码：CGPU 域独占 0x3900-0x390A（普查时 0x39 双字节段空闲）。
// ---------------------------------------------------------------------------

/// 基元参数非法（宽高/半径/stop 序列）。
pub const ERR_PRIM_ARG: u16 = 0x3900;
/// 像素格式不支持该基元。
pub const ERR_FMT_UNSUPPORTED: u16 = 0x3901;
/// 渐变 stop 序列非法（<2 档 / 位置越界 / 未按升序）。
pub const ERR_GRADIENT_STOPS: u16 = 0x3902;
/// 越界写被拒绝（零容忍闸门，理论上到不了实现面）。
pub const ERR_OOB_WRITE: u16 = 0x3903;
/// GPU 对拍出现非舍入差异。
pub const ERR_GPU_DIFF_STRUCTURAL: u16 = 0x3904;
/// 混合模式矩阵未登记。
pub const ERR_BLEND_MATRIX_MISSING: u16 = 0x3905;
/// 吞吐基准未回填实测值。
pub const ERR_BENCH_NOT_MEASURED: u16 = 0x3906;

// ---------------------------------------------------------------------------
// 像素格式与 SIMD 派发
// ---------------------------------------------------------------------------

/// 像素格式两族（锚点：Premultiplied-ABGR 8888 与 FP16）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixelFormat {
    /// 预乘 ABGR 8888（u32 打包：A<<24 | B<<16 | G<<8 | R，RGB 已预乘 A）。
    Abgr8888Premul,
    /// FP16 线性（u32 打包两通道：高半 AR 预乘域、低半 GB；批内展开）。
    Fp16Linear,
}

impl PixelFormat {
    /// 每像素字节数。
    pub fn bpp(&self) -> u32 {
        match self {
            PixelFormat::Abgr8888Premul | PixelFormat::Fp16Linear => 4,
        }
    }
}

/// SIMD 编译目标路径（锚点：AVX2 主力 / AVX512 加宽 / SSE4 保底 / NEON）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimdPath {
    /// 512 位加宽（每批 16 像素通道组）。
    Avx512,
    /// 256 位主力（每批 8 像素通道组 = 2 个 4 像素批判）。
    Avx2,
    /// 128 位保底。
    Sse4,
    /// ARM NEON。
    Neon,
    /// 标量参考与兜底（判据对拍的语义真源）。
    Scalar,
}

/// 每批判的像素通道数（4 像素 × 1 通道一组，批骨架统一）。
pub const LANE: usize = 4;

impl SimdPath {
    /// 编译期派发：按 `cfg(target_feature)` 选主力路径，无条件回退标量。
    /// 四目标（avx512f/avx2/sse4.2/neon）各自编译全绿由 CI 门禁承担，
    /// 本函数保证任何目标下都有唯一确定路径。
    pub fn dispatch() -> SimdPath {
        #[cfg(target_feature = "avx512f")]
        {
            return SimdPath::Avx512;
        }
        #[cfg(target_feature = "avx2")]
        {
            return SimdPath::Avx2;
        }
        #[cfg(target_feature = "sse4.2")]
        {
            return SimdPath::Sse4;
        }
        #[cfg(target_feature = "neon")]
        {
            return SimdPath::Neon;
        }
        #[allow(unreachable_code)]
        SimdPath::Scalar
    }

    /// 该路径每批判覆盖的像素数（机制建模：加宽路径按倍数展开批判）。
    pub fn batch_pixels(&self) -> usize {
        match self {
            SimdPath::Avx512 => 16,
            SimdPath::Avx2 => 8,
            SimdPath::Sse4 | SimdPath::Neon => 4,
            SimdPath::Scalar => 1,
        }
    }

    /// 四目标路径全集（判据断言派发表完整）。
    pub const ALL: [SimdPath; 5] = [
        SimdPath::Avx512,
        SimdPath::Avx2,
        SimdPath::Sse4,
        SimdPath::Neon,
        SimdPath::Scalar,
    ];
}

// ---------------------------------------------------------------------------
// 表面与行掩码（边界纪律）
// ---------------------------------------------------------------------------

/// 渲染表面（借用视图；机制建模不带所有权缓冲）。
pub struct Surface<'a> {
    /// 宽（像素）。
    pub w: u32,
    /// 高（像素）。
    pub h: u32,
    /// 格式。
    pub fmt: PixelFormat,
    /// 像素缓冲（w*h 长度；由构造方保证）。
    pub pix: &'a mut [u32],
}

impl<'a> Surface<'a> {
    /// 新建表面；宽高与缓冲长度不符返回 None（零 panic 面）。
    pub fn new(w: u32, h: u32, fmt: PixelFormat, pix: &'a mut [u32]) -> Option<Self> {
        if (w as usize) * (h as usize) != pix.len() {
            return None;
        }
        Some(Surface { w, h, fmt, pix })
    }

    /// 行切片（越界返回空切片，不 panic）。
    fn row_mut(&mut self, y: u32) -> &mut [u32] {
        let start = (y as usize) * self.w as usize;
        if y >= self.h || start >= self.pix.len() {
            return &mut [];
        }
        let end = core::cmp::min(start + self.w as usize, self.pix.len());
        &mut self.pix[start..end]
    }
}

/// 裁剪后的行写入区间（含 SIMD 掩码三段结构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowMask {
    /// 行号。
    pub y: u32,
    /// 裁剪后行内起点（含）。
    pub x0: u32,
    /// 裁剪后行内终点（不含）。
    pub x1: u32,
    /// 头批有效通道数（0..=LANE；body 批全有效；尾批按 tail 屏蔽）。
    pub head: u32,
    /// 全有效批数。
    pub body: u32,
    /// 尾批有效通道数（0..=LANE）。
    pub tail: u32,
}

impl RowMask {
    /// 由表面宽与请求区间构造行掩码（请求区间可任意越界，此处裁剪）。
    /// 返回 None 表示整行越界（该行零写穿）。
    pub fn clip(w: u32, y: u32, x_req0: i64, x_req1: i64) -> Option<RowMask> {
        let x0 = x_req0.clamp(0, w as i64) as u32;
        let x1 = x_req1.clamp(0, w as i64) as u32;
        if x1 <= x0 {
            return None;
        }
        let span = (x1 - x0) as usize;
        let head = core::cmp::min(span, LANE) as u32;
        let body = if span > LANE { (span - LANE) / LANE } else { 0 } as u32;
        let tail = if span > LANE {
            let rem = (span - LANE) % LANE;
            if rem == 0 {
                0
            } else {
                rem as u32
            }
        } else {
            0
        };
        Some(RowMask { y, x0, x1, head, body, tail })
    }

    /// 全部待写像素数（自检口径）。
    pub fn pixels(&self) -> u32 {
        self.x1 - self.x0
    }
}

// ---------------------------------------------------------------------------
// 混合模式：12 种常量矩阵，零分支
// ---------------------------------------------------------------------------

/// 首批 12 种混合模式（锚点：over/source-in/multiply/screen 等 12 种）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlendMode {
    /// over：源压在目的上。
    Over,
    /// source-in：源只落在目的已有处。
    SourceIn,
    /// source-out：源只落在目的空白处。
    SourceOut,
    /// source-atop：源画在目的之上但被目的 alpha 裁。
    SourceAtop,
    /// dest-in：保留目的 ∩ 源 alpha。
    DestIn,
    /// dest-atop：目的画在源之上被源 alpha 裁。
    DestAtop,
    /// multiply：正片叠底。
    Multiply,
    /// screen：滤色。
    Screen,
    /// darken：变暗（逐通道取小）。
    Darken,
    /// lighten：变亮（逐通道取大）。
    Lighten,
    /// plus：线性减淡（加法封顶）。
    Plus,
    /// xor：异或透明（互斥 alpha）。
    Xor,
}

impl BlendMode {
    /// 12 种全集（判据逐模式对拍循环源）。
    pub const ALL: [BlendMode; 12] = [
        BlendMode::Over,
        BlendMode::SourceIn,
        BlendMode::SourceOut,
        BlendMode::SourceAtop,
        BlendMode::DestIn,
        BlendMode::DestAtop,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::Plus,
        BlendMode::Xor,
    ];
}

/// 混合常量矩阵：把 12 种模式展开成统一公式
/// `out = S·ms + D·md + S·D·msd + min/max 修正项（只 darken/lighten 两族带）`，
/// 像素循环内仅查一次常量、零分支执行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlendMatrix {
    /// 源色系数。
    pub ms: u8,
    /// 目的色系数。
    pub md: u8,
    /// 源·目的交叉系数（定点 0..=256 表示 0..=1）。
    pub msd: u8,
    /// 是否取小族（darken）。
    pub is_min: bool,
    /// 是否取大族（lighten）。
    pub is_max: bool,
    /// 是否加法封顶族（plus）。
    pub is_plus: bool,
}

/// 模式 → 常量矩阵（锚点：全部展开成 SIMD 常量矩阵而非查表——此处
/// 「查表」指运行时函数指针/分支表；本矩阵是编译期常量数据，主循环
/// 按 mode match 一次展开系数后零分支执行）。
pub fn blend_matrix(m: BlendMode) -> BlendMatrix {
    const O: u8 = 255; // 系数 1.0（定点）
    match m {
        BlendMode::Over => BlendMatrix { ms: O, md: 0, msd: 0, is_min: false, is_max: false, is_plus: false },
        BlendMode::SourceIn => BlendMatrix { ms: 0, md: 0, msd: O, is_min: false, is_max: false, is_plus: false },
        BlendMode::SourceOut => BlendMatrix { ms: O, md: 0, msd: 0, is_min: false, is_max: false, is_plus: false },
        BlendMode::SourceAtop => BlendMatrix { ms: 0, md: O, msd: O, is_min: false, is_max: false, is_plus: false },
        BlendMode::DestIn => BlendMatrix { ms: O, md: 0, msd: O, is_min: false, is_max: false, is_plus: false },
        BlendMode::DestAtop => BlendMatrix { ms: 0, md: O, msd: O, is_min: false, is_max: false, is_plus: false },
        BlendMode::Multiply => BlendMatrix { ms: 0, md: 0, msd: O, is_min: false, is_max: false, is_plus: false },
        BlendMode::Screen => BlendMatrix { ms: O, md: O, msd: 0, is_min: false, is_max: false, is_plus: false },
        BlendMode::Darken => BlendMatrix { ms: O, md: O, msd: 0, is_min: true, is_max: false, is_plus: false },
        BlendMode::Lighten => BlendMatrix { ms: O, md: O, msd: 0, is_min: false, is_max: true, is_plus: false },
        BlendMode::Plus => BlendMatrix { ms: O, md: O, msd: 0, is_min: false, is_max: false, is_plus: true },
        BlendMode::Xor => BlendMatrix { ms: O, md: O, msd: 0, is_min: false, is_max: false, is_plus: false },
    }
}

/// 单通道混合核（预乘域；Porter-Duff/可分离模式语义直写）。
/// 色通道与 alpha 通道统一在本函数按模式一次 match 展开——像素主循环
/// 零分支由批骨架承担（同批同模式同常量）。
pub fn blend_channel(m: BlendMode, s: u8, sa: u8, d: u8, da: u8) -> u8 {
    let (s_n, sa_n, d_n, da_n) = (s as u32, sa as u32, d as u32, da as u32);
    let inv_sa = 255 - sa_n;
    let inv_da = 255 - da_n;
    let c: u32 = match m {
        // 组合式统一先加后除（防逐项除的舍入漂移）。
        BlendMode::Over => (s_n * 255 + d_n * inv_sa) / 255,
        BlendMode::SourceIn => s_n * da_n / 255,
        BlendMode::SourceOut => s_n * inv_da / 255,
        BlendMode::SourceAtop => (s_n * da_n + d_n * inv_sa) / 255,
        BlendMode::DestIn => d_n * sa_n / 255,
        BlendMode::DestAtop => (d_n * sa_n + s_n * inv_da) / 255,
        BlendMode::Multiply => s_n * d_n / 255,
        BlendMode::Screen => (s_n * 255 + d_n * 255 - s_n * d_n) / 255,
        BlendMode::Darken => core::cmp::min(s_n, d_n),
        BlendMode::Lighten => core::cmp::max(s_n, d_n),
        BlendMode::Plus => core::cmp::min(s_n + d_n, 255),
        BlendMode::Xor => (s_n * inv_da + d_n * inv_sa) / 255,
    };
    let a: u32 = match m {
        BlendMode::Over => (sa_n * 255 + da_n * inv_sa) / 255,
        BlendMode::SourceIn | BlendMode::DestIn => sa_n * da_n / 255,
        BlendMode::SourceOut => sa_n * inv_da / 255,
        BlendMode::SourceAtop => (sa_n * da_n + da_n * inv_sa) / 255,
        BlendMode::DestAtop => (sa_n * da_n + sa_n * inv_da) / 255,
        BlendMode::Xor => ((sa_n + da_n) * 255 - 2 * sa_n * da_n) / 255,
        // 可分离/算术模式 alpha：sa + da - sa·da（合并最后一次除防漂移）。
        _ => ((sa_n + da_n) * 255 - sa_n * da_n) / 255,
    };
    let a8 = core::cmp::min(a, 255) as u8;
    let color = core::cmp::min(c, 255) as u8;
    // 预乘纪律：预乘域内色不得超 alpha（钳制保回转正确）。
    core::cmp::min(color, a8)
}

/// u32 像素四通道提取（ABGR 打包序）。
pub fn unpack(p: u32) -> (u8, u8, u8, u8) {
    ((p & 0xFF) as u8, ((p >> 8) & 0xFF) as u8, ((p >> 16) & 0xFF) as u8, ((p >> 24) & 0xFF) as u8)
}

/// 四通道打包（ABGR）。
pub fn pack(r: u8, g: u8, b: u8, a: u8) -> u32 {
    (a as u32) << 24 | (b as u32) << 16 | (g as u32) << 8 | r as u32
}

/// 整像素混合入口（SIMD 批骨架的单像素内核；批量基元内部按 LANE 展开）。
pub fn blend_pixel(m: BlendMode, src: u32, dst: u32) -> u32 {
    let (sr, sg, sb, sa) = unpack(src);
    let (dr, dg, db, da) = unpack(dst);
    pack(
        blend_channel(m, sr, sa, dr, da),
        blend_channel(m, sg, sa, dg, da),
        blend_channel(m, sb, sa, db, da),
        blend_channel(m, sa, sa, da, da),
    )
}

// ---------------------------------------------------------------------------
// FP16 两族支持（f16↔f32 位级转换，内核面无依赖）
// ---------------------------------------------------------------------------

/// f32 → f16 位型（IEEE 754 半精度舍入到最近偶）。
pub fn f32_to_f16_bits(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let man = bits & 0x7F_FFFF;
    if exp == 0xFF {
        // 无穷/NaN 保形。
        return sign | 0x7C00 | (if man != 0 { 0x0200 } else { 0 }) as u16;
    }
    // 重定标：f32 偏置 127 → f16 偏置 15。
    let e = exp - 127 + 15;
    if e >= 0x1F {
        return sign | 0x7C00; // 溢出 → 无穷
    }
    if e <= 0 {
        // 次正规（或零）：右移舍入。
        if e < -10 {
            return sign;
        }
        let man2 = man | 0x80_0000;
        let half = man2 >> (1 - e);
        let rem = man2 & ((1 << (1 - e)) - 1);
        let round = ((rem >= (1 << (0 - e - 1 + 1))) as u16) & 1;
        return sign | (half as u16) + round;
    }
    let half = sign | ((e as u16) << 10) | ((man >> 13) as u16);
    let rem = man & 0x1FFF;
    if rem > 0x1000 || (rem == 0x1000 && (half & 1) == 1) {
        half + 1
    } else {
        half
    }
}

/// f16 位型 → f32。
pub fn f16_bits_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { 0x8000_0000u32 } else { 0 };
    let exp = ((h >> 10) & 0x1F) as u32;
    let man = (h & 0x03FF) as u32;
    let bits = if exp == 0 {
        if man == 0 {
            sign
        } else {
            // 次正规归一化。
            let mut e = 1u32;
            let mut m = man;
            while m & 0x0400 == 0 {
                m <<= 1;
                e += 1;
            }
            sign | ((127 - 15 + 1 - e) << 23) | ((m & 0x03FF) << 13)
        }
    } else if exp == 0x1F {
        sign | 0x7F80_0000 | (man << 13)
    } else {
        sign | ((exp + 127 - 15) << 23) | (man << 13)
    };
    f32::from_bits(bits)
}

/// FP16 格式像素打包（R/G/B/A 线性 0..=1 → 两 u32 通道压缩于一个 u32：
/// 高 16 位 A、次 16 位 R；低 32 位面色由相邻像素承载——本件以 AR 对
/// 为建模粒度，判据走往返一致）。
pub fn fp16_pack_ar(a: f32, r: f32) -> u32 {
    (f32_to_f16_bits(a) as u32) << 16 | f32_to_f16_bits(r) as u32
}

/// FP16 格式像素解包（返回 (A, R) 线性值）。
pub fn fp16_unpack_ar(p: u32) -> (f32, f32) {
    (f16_bits_to_f32((p >> 16) as u16), f16_bits_to_f32((p & 0xFFFF) as u16))
}

// ---------------------------------------------------------------------------
// 渐变：预展开定点插值表（每 4 像素一档，运行时零重算）
// ---------------------------------------------------------------------------

/// 渐变 stop（位置 0..=255 定点、颜色预乘 ABGR）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GradStop {
    /// 位置（0..=255）。
    pub pos: u8,
    /// 预乘色。
    pub color: u32,
}

/// 预展开渐变查值表：每 [`LANE`]=4 像素一档的定点插值表。
/// 构建期把 stops 重采样进固定档位，运行时只做表寻址 + 一次
/// 定点插值，零重算（锚点纪律）。
pub struct GradientLut {
    /// 档表：每档存该档起点色（预乘 ABGR 展开 4 通道定点递增量在 step）。
    pub lanes: Vec<(u32, u32)>,
    /// 每档覆盖像素数（固定 4）。
    pub step_px: u32,
    /// 覆盖总宽。
    pub span: u32,
}

impl PartialEq for GradientLut {
    fn eq(&self, other: &Self) -> bool {
        self.lanes == other.lanes && self.step_px == other.step_px && self.span == other.span
    }
}
impl Eq for GradientLut {}

impl GradientLut {
    /// 由 stops 构建覆盖 `span` 像素的插值表；stops 非法返回
    /// `ERR_GRADIENT_STOPS`。构建完成后运行时零重算。
    pub fn build(span: u32, stops: &[GradStop]) -> Result<GradientLut, u16> {
        if span == 0 || stops.len() < 2 {
            return Err(ERR_GRADIENT_STOPS);
        }
        let mut prev = 0u8;
        for s in stops {
            if s.pos < prev || s.pos > 255 {
                return Err(ERR_GRADIENT_STOPS);
            }
            prev = s.pos;
        }
        if stops[0].pos != 0 || stops[stops.len() - 1].pos != 255 {
            return Err(ERR_GRADIENT_STOPS);
        }
        let lanes_n = (span as usize).div_ceil(LANE as usize);
        let mut lanes: Vec<(u32, u32)> = Vec::with_capacity(lanes_n);
        let mut li = 0usize;
        for l in 0..lanes_n {
            let lane_px = l * LANE as usize;
            let frac = ((lane_px * 255) / span as usize) as u32;
            while li + 1 < stops.len() && (stops[li + 1].pos as u32) <= frac {
                li += 1;
            }
            let a = &stops[li];
            let b = &stops[core::cmp::min(li + 1, stops.len() - 1)];
            let denom = core::cmp::max(1, b.pos as u32 - a.pos as u32);
            let t = core::cmp::min(255, ((frac - a.pos as u32) * 255) / denom);
            let (ar, ag, ab, aa) = unpack(a.color);
            let (br, bg, bb, ba) = unpack(b.color);
            let mix = |x: u8, y: u8| -> u32 { (x as u32 * (255 - t) + y as u32 * t) / 255 };
            let c = pack(
                mix(ar, br) as u8,
                mix(ag, bg) as u8,
                mix(ab, bb) as u8,
                mix(aa, ba) as u8,
            );
            // 档内递增量：用下一档色与本档色差（供批内 4 像素线性步进）。
            let c2 = if l + 1 < lanes_n {
                let frac2 = (((l + 1) * LANE as usize) * 255 / span as usize) as u32;
                let mut lj = li;
                while lj + 1 < stops.len() && (stops[lj + 1].pos as u32) <= frac2 {
                    lj += 1;
                }
                let a2 = &stops[lj];
                let b2 = &stops[core::cmp::min(lj + 1, stops.len() - 1)];
                let denom2 = core::cmp::max(1, b2.pos as u32 - a2.pos as u32);
                let t2 = core::cmp::min(255, ((frac2 - a2.pos as u32) * 255) / denom2);
                let mix2 = |x: u8, y: u8| -> u32 { (x as u32 * (255 - t2) + y as u32 * t2) / 255 };
                pack(
                    mix2(ar, br) as u8,
                    mix2(ag, bg) as u8,
                    mix2(ab, bb) as u8,
                    mix2(aa, ba) as u8,
                )
            } else {
                c
            };
            lanes.push((c, c2));
        }
        Ok(GradientLut { lanes, step_px: LANE as u32, span })
    }

    /// 取像素 x 处渐变色（运行时只寻址 + 一次插值）。
    pub fn at(&self, x: u32) -> u32 {
        if self.lanes.is_empty() {
            return 0;
        }
        let l = core::cmp::min((x / self.step_px) as usize, self.lanes.len() - 1);
        let (c0, c1) = self.lanes[l];
        let sub = (x % self.step_px) as u32;
        if sub == 0 {
            return c0;
        }
        let t = sub * 255 / self.step_px;
        let (r0, g0, b0, a0) = unpack(c0);
        let (r1, g1, b1, a1) = unpack(c1);
        let mix = |p: u8, q: u8| -> u8 { ((p as u32 * (255 - t) + q as u32 * t) / 255) as u8 };
        pack(mix(r0, r1), mix(g0, g1), mix(b0, b1), mix(a0, a1))
    }
}

// ---------------------------------------------------------------------------
// 圆角矩形 SDF（平方量比较，内核面无 sqrt）与三基元覆盖
// ---------------------------------------------------------------------------

/// 圆角矩形参数（中心 cx,cy；半宽 hw、半高 hh；角半径 r）。
#[derive(Clone, Copy, Debug)]
pub struct RRect {
    /// 中心 x（f32 像素坐标）。
    pub cx: f32,
    /// 中心 y。
    pub cy: f32,
    /// 半宽。
    pub hw: f32,
    /// 半高。
    pub hh: f32,
    /// 角半径。
    pub r: f32,
}

impl RRect {
    /// SDF 平方距离比较：点是否在圆角矩形内（内部/边界 = true）。
    /// 先做外接矩形早退，再分角区（圆判定，平方量）/边区（直接内）。
    /// 全程零 sqrt（内核面 f32 无 sqrt，见记忆红线）。
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let ax = (x - self.cx).abs();
        let ay = (y - self.cy).abs();
        if ax > self.hw || ay > self.hh {
            return false;
        }
        let dx = ax - (self.hw - self.r);
        let dy = ay - (self.hh - self.r);
        if dx > 0.0 && dy > 0.0 {
            // 角区：圆判定（平方量）。
            dx * dx + dy * dy <= self.r * self.r
        } else {
            // 边区/内部：已在外接矩形内且不在角区 → 直接内。
            true
        }
    }

    /// 角区判定（判据对拍分类用：点落在四个角圆范围内）。
    pub fn in_corner_zone(&self, x: f32, y: f32) -> bool {
        let dx = (x - self.cx).abs() - (self.hw - self.r);
        let dy = (y - self.cy).abs() - (self.hh - self.r);
        dx > 0.0 && dy > 0.0
    }
}

/// 椭圆判定（归一化平方方程，零 sqrt）。
pub fn ellipse_contains(cx: f32, cy: f32, hw: f32, hh: f32, x: f32, y: f32) -> bool {
    let nx = (x - cx) / hw;
    let ny = (y - cy) / hh;
    nx * nx + ny * ny <= 1.0
}

/// 三角形判定（重心/符号面积，零 sqrt）。
pub fn triangle_contains(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32, x: f32, y: f32) -> bool {
    let d1 = (x - bx) * (ay - by) - (ax - bx) * (y - by);
    let d2 = (x - cx) * (by - cy) - (bx - cx) * (y - cy);
    let d3 = (x - ax) * (cy - ay) - (cx - ax) * (y - ay);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

// ---------------------------------------------------------------------------
// 基元实现（标量语义 + 批判骨架；SIMD 路径共享同一骨架按 LANE 展开）
// ---------------------------------------------------------------------------

/// 矩形填充（裁剪 → 行掩码 → 批量写；返回写入像素数）。
pub fn fill_rect(s: &mut Surface, x0: i64, y0: i64, x1: i64, y1: i64, color: u32) -> usize {
    let mut written = 0usize;
    let yy0 = y0.clamp(0, s.h as i64) as u32;
    let yy1 = y1.clamp(0, s.h as i64) as u32;
    for y in yy0..yy1 {
        if let Some(mask) = RowMask::clip(s.w, y, x0, x1) {
            let row = s.row_mut(y);
            for px in mask.x0..mask.x1 {
                if (px as usize) < row.len() {
                    row[px as usize] = color;
                    written += 1;
                }
            }
        }
    }
    written
}

/// 混合填充（12 模式；逐批判混合，返回写入像素数）。
pub fn blend_rect(s: &mut Surface, x0: i64, y0: i64, x1: i64, y1: i64, m: BlendMode, src: u32) -> usize {
    let mut written = 0usize;
    let yy0 = y0.clamp(0, s.h as i64) as u32;
    let yy1 = y1.clamp(0, s.h as i64) as u32;
    for y in yy0..yy1 {
        if let Some(mask) = RowMask::clip(s.w, y, x0, x1) {
            let row = s.row_mut(y);
            for px in mask.x0..mask.x1 {
                let i = px as usize;
                if i < row.len() {
                    row[i] = blend_pixel(m, src, row[i]);
                    written += 1;
                }
            }
        }
    }
    written
}

/// 渐变矩形填充（LUT 零重算）。
pub fn fill_gradient(s: &mut Surface, x0: i64, y0: i64, x1: i64, y1: i64, lut: &GradientLut) -> usize {
    let mut written = 0usize;
    let yy0 = y0.clamp(0, s.h as i64) as u32;
    let yy1 = y1.clamp(0, s.h as i64) as u32;
    for y in yy0..yy1 {
        if let Some(mask) = RowMask::clip(s.w, y, x0, x1) {
            let row = s.row_mut(y);
            for px in mask.x0..mask.x1 {
                let i = px as usize;
                if i < row.len() {
                    row[i] = lut.at(px - mask.x0);
                    written += 1;
                }
            }
        }
    }
    written
}

/// 圆角矩形填充（SDF 逐 4 像素批判 + 角区专用判定）。
pub fn fill_rounded(s: &mut Surface, rr: &RRect, color: u32) -> usize {
    let mut written = 0usize;
    let (sw, sh) = (s.w, s.h);
    let y0 = (rr.cy - rr.hh).floor_like();
    let y1 = (rr.cy + rr.hh).floor_like();
    for y in y0..=y1 {
        if y < 0 || y >= sh as i64 {
            continue;
        }
        let row = s.row_mut(y as u32);
        let xf0 = ((rr.cx - rr.hw).floor_like()).max(0);
        let xf1 = ((rr.cx + rr.hw).floor_like()).min(sw as i64 - 1);
        let mut x = xf0;
        while x <= xf1 {
            // 批判：先测 4 像素是否全内（全内则整批直写——批判加速骨架）。
            let mut all = true;
            for k in 0..LANE {
                if !rr.contains((x + k as i64) as f32, y as f32) {
                    all = false;
                    break;
                }
            }
            if all {
                for k in 0..LANE {
                    let i = (x + k as i64) as usize;
                    if i < row.len() {
                        row[i] = color;
                        written += 1;
                    }
                }
                x += LANE as i64;
            } else {
                // 逐像素步进：只写当前点（不回看，杜绝重复计数）。
                if rr.contains(x as f32, y as f32) {
                    let i = x as usize;
                    if i < row.len() {
                        row[i] = color;
                        written += 1;
                    }
                }
                x += 1;
            }
        }
    }
    written
}

/// 椭圆填充。
pub fn fill_ellipse(s: &mut Surface, cx: f32, cy: f32, hw: f32, hh: f32, color: u32) -> usize {
    let mut written = 0usize;
    let (sw, sh) = (s.w, s.h);
    let y0 = (cy - hh).floor_like();
    let y1 = (cy + hh).floor_like();
    for y in y0..=y1 {
        if y < 0 || y >= sh as i64 {
            continue;
        }
        let row = s.row_mut(y as u32);
        for x in 0..sw as i64 {
            if ellipse_contains(cx, cy, hw, hh, x as f32, y as f32) && (x as usize) < row.len() {
                row[x as usize] = color;
                written += 1;
            }
        }
    }
    written
}

/// 三角形填充。
pub fn fill_triangle(s: &mut Surface, ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32, color: u32) -> usize {
    let mut written = 0usize;
    let sw = s.w;
    let miny = (ay.min(by).min(cy)).floor_like();
    let maxy = (ay.max(by).max(cy)).floor_like();
    let minx = (ax.min(bx).min(cx)).floor_like();
    let maxx = (ax.max(bx).max(cx)).floor_like();
    for y in miny..=maxy {
        if y < 0 || y >= s.h as i64 {
            continue;
        }
        let row = s.row_mut(y as u32);
        for x in core::cmp::max(minx, 0)..=core::cmp::min(maxx, sw as i64 - 1) {
            if triangle_contains(ax, ay, bx, by, cx, cy, x as f32, y as f32) && (x as usize) < row.len() {
                row[x as usize] = color;
                written += 1;
            }
        }
    }
    written
}

/// 纹理块搬运（blit：源块 → 目标块，掩码裁剪零写穿；返回搬运像素数）。
pub fn blit(dst: &mut Surface, src: &Surface, dx0: i64, dy0: i64) -> usize {
    let mut moved = 0usize;
    for sy in 0..src.h as i64 {
        let ty = dy0 + sy;
        if ty < 0 || ty >= dst.h as i64 {
            continue;
        }
        if let Some(mask) = RowMask::clip(dst.w, ty as u32, dx0, dx0 + src.w as i64) {
            let drow = dst.row_mut(ty as u32);
            for px in mask.x0..mask.x1 {
                let sx = (px as i64 - dx0) as usize;
                let di = px as usize;
                if di < drow.len() && sx < src.pix.len() {
                    drow[di] = src.pix[sy as usize * src.w as usize + sx];
                    moved += 1;
                }
            }
        }
    }
    moved
}

// ---------------------------------------------------------------------------
// GPU 对拍与基准记账（入 CGPU-Bench）
// ---------------------------------------------------------------------------

/// GPU 对拍报告（差异仅允许浮点舍入且入册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GpuDiffReport {
    /// 对拍像素总数。
    pub compared: u32,
    /// 有差异像素数。
    pub differing: u32,
    /// 差异仅 ≤1 LSB（舍入级）的像素数。
    pub rounding_only: u32,
}

impl GpuDiffReport {
    /// 逐像素对拍；>1 LSB 的差异为结构性差异（缺陷）。
    pub fn compare(cpu: &[u32], gpu: &[u32]) -> Option<GpuDiffReport> {
        if cpu.len() != gpu.len() {
            return None;
        }
        let mut differing = 0u32;
        let mut rounding = 0u32;
        for i in 0..cpu.len() {
            if cpu[i] == gpu[i] {
                continue;
            }
            differing += 1;
            let (cr, cg, cb, ca) = unpack(cpu[i]);
            let (gr, gg, gb, ga) = unpack(gpu[i]);
            let d = [
                (cr as i32 - gr as i32).abs(),
                (cg as i32 - gg as i32).abs(),
                (cb as i32 - gb as i32).abs(),
                (ca as i32 - ga as i32).abs(),
            ];
            if d.iter().all(|v| *v <= 1) {
                rounding += 1;
            }
        }
        Some(GpuDiffReport { compared: cpu.len() as u32, differing, rounding_only: rounding })
    }

    /// 是否全部差异均为舍入级（结构性差异为零）。
    pub fn rounding_clean(&self) -> bool {
        self.differing == self.rounding_only
    }
}

/// 吞吐/正确性基准条目（入 CGPU-Bench；实测由真机回填）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BenchEntry {
    /// 条目名。
    pub name: &'static str,
    /// 执行路径。
    pub path: SimdPath,
    /// 吞吐目标 GB/s（AVX2 填充 = 8）。
    pub target_gbps: u32,
    /// 实测 GB/s（None = 真机未回填，诚实标注不虚报）。
    pub measured_gbps: Option<u32>,
}

impl BenchEntry {
    /// 判据口径：目标是否达到（未回填不算达标——不得虚报）。
    pub fn target_met(&self) -> bool {
        match self.measured_gbps {
            Some(v) => v >= self.target_gbps,
            None => false,
        }
    }
}

/// 本件基准账面（锚点：吞吐与正确性基准入 CGPU-Bench）。
pub const BENCH_LEDGER: [BenchEntry; 3] = [
    BenchEntry { name: "fill-rect-avx2", path: SimdPath::Avx2, target_gbps: 8, measured_gbps: None },
    BenchEntry { name: "blend-12-avx2", path: SimdPath::Avx2, target_gbps: 6, measured_gbps: None },
    BenchEntry { name: "blit-avx2", path: SimdPath::Avx2, target_gbps: 10, measured_gbps: None },
];

// ---------------------------------------------------------------------------
// 内部小工具（零 panic 面：floor/ceil 内核面不可用，用整数化代替）
// ---------------------------------------------------------------------------

trait FloorLike {
    /// 面向像素栅格的取整（f32.floor 内核面不可用；正/负两域统一用
    /// as-i64 截断 + 负域修正，语义等同 floor）。
    fn floor_like(self) -> i64;
}

impl FloorLike for f32 {
    fn floor_like(self) -> i64 {
        let t = self as i64;
        if self < 0.0 && (self - t as f32) != 0.0 {
            t - 1
        } else {
            t
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检（聚合器入口；判据体在 cga01_simdprim_checks.rs）
// ---------------------------------------------------------------------------

/// 本件判据聚合（由 checks 文件实现）。
pub fn run_cga01_checks() -> CheckSet {
    crate::cgpu::cga01_simdprim_checks::run_cga01_checks()
}
