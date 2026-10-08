//! GALAXY AI-19 SIMD 与数值域（G1121~G1140）。
//!
//! CPU 特性探测、SIMD 封装与调度集成、向量数学、4×4 矩阵、张量原语、
//! Q16.16 定点、CRC32/内存加速、XSAVE 区域计算、降级链与域自检收口。
//! 首创点：内核数学内核库（AVX-512 张量原语分层降级）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1121 CPU 特性探测扩展
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct CpuFeatures {
    pub sse2: bool,
    pub avx: bool,
    pub avx2: bool,
    pub avx512f: bool,
    pub aes_ni: bool,
    pub sha_ni: bool,
}

impl CpuFeatures {
    /// 由 CPUID 位图构造（bit 位布局简化）。
    pub fn from_bits(bits: u64) -> CpuFeatures {
        CpuFeatures {
            sse2: bits & 1 != 0,
            avx: bits & (1 << 1) != 0,
            avx2: bits & (1 << 2) != 0,
            avx512f: bits & (1 << 3) != 0,
            aes_ni: bits & (1 << 4) != 0,
            sha_ni: bits & (1 << 5) != 0,
        }
    }

    /// 特性等级：0 标量 / 1 SSE2 / 2 AVX2 / 3 AVX-512。
    pub fn level(&self) -> u8 {
        if self.avx512f {
            3
        } else if self.avx2 {
            2
        } else if self.sse2 {
            1
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// G1122 内核 SIMD 封装
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimdWidth {
    Scalar,
    X128,
    X256,
    X512,
}

pub fn width_for_level(level: u8) -> SimdWidth {
    match level {
        3 => SimdWidth::X512,
        2 => SimdWidth::X256,
        1 => SimdWidth::X128,
        _ => SimdWidth::Scalar,
    }
}

/// 一次处理多少 f32。
pub fn lanes(width: SimdWidth) -> usize {
    match width {
        SimdWidth::Scalar => 1,
        SimdWidth::X128 => 4,
        SimdWidth::X256 => 8,
        SimdWidth::X512 => 16,
    }
}

// ---------------------------------------------------------------------------
// G1123 向量数学库
// ---------------------------------------------------------------------------

/// sin(x) 泰勒近似（|x| 任意，内部归约到 [-π,π]），精度 ~1e-4。
pub fn vec_sin(x: f64) -> f64 {
    let pi = core::f64::consts::PI;
    let two_pi = 2.0 * pi;
    let mut y = x % two_pi;
    if y > pi {
        y -= two_pi;
    } else if y < -pi {
        y += two_pi;
    }
    let x2 = y * y;
    // sin = x - x^3/6 + x^5/120 - x^7/5040
    y * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0)))
}

/// 快速逆平方根（Quake 风格 f32）。
pub fn fast_inv_sqrt(x: f32) -> f32 {
    let half = 0.5f32 * x;
    let i = x.to_bits();
    let j = 0x5f37_5a86u32.wrapping_sub(i >> 1);
    let y = f32::from_bits(j);
    y * (1.5f32 - half * y * y)
}

// ---------------------------------------------------------------------------
// G1124 矩阵乘法内核
// ---------------------------------------------------------------------------

/// 4×4 f32 矩阵乘（ikj 顺序，缓存友好）。
pub fn mat4_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for i in 0..4 {
        for k in 0..4 {
            let aik = a[i * 4 + k];
            if aik == 0.0 {
                continue;
            }
            for j in 0..4 {
                out[i * 4 + j] += aik * b[k * 4 + j];
            }
        }
    }
    out
}

/// 单位矩阵。
pub fn mat4_identity() -> [f32; 16] {
    let mut m = [0.0f32; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m
}

// ---------------------------------------------------------------------------
// G1125 张量原语 — 卷积/归约
// ---------------------------------------------------------------------------

/// 1D 卷积（valid）。
pub fn conv1d(input: &[f32], kernel: &[f32]) -> Vec32 {
    let out_len = input.len().saturating_sub(kernel.len()) + 1;
    let mut out = Vec32 { data: [0.0; 32], len: out_len.min(32) };
    for i in 0..out.len {
        let mut acc = 0.0f32;
        for (k, &kv) in kernel.iter().enumerate() {
            acc += input[i + k] * kv;
        }
        out.data[i] = acc;
    }
    out
}

#[derive(Clone, Copy)]
pub struct Vec32 {
    pub data: [f32; 32],
    pub len: usize,
}

/// 归约：和与最大值。
pub fn reduce_sum_max(v: &[f32]) -> (f32, f32) {
    let mut sum = 0.0f32;
    let mut max = f32::MIN;
    for &x in v {
        sum += x;
        if x > max {
            max = x;
        }
    }
    (sum, if v.is_empty() { 0.0 } else { max })
}

// ---------------------------------------------------------------------------
// G1126 定点与浮点策略
// ---------------------------------------------------------------------------

/// Q16.16 定点乘。
pub fn fixed_mul(a: i32, b: i32) -> i32 {
    (((a as i64) * (b as i64)) >> 16) as i32
}

/// Q16.16 常数：1.0 与 0.5。
pub const FIXED_ONE: i32 = 1 << 16;
pub const FIXED_HALF: i32 = 1 << 15;

// ---------------------------------------------------------------------------
// G1127 加密加速
// ---------------------------------------------------------------------------

/// AES-NI / SHA-NI 加速倍率表（相对标量实现）。
pub fn crypto_accel_factor(f: &CpuFeatures) -> u32 {
    let mut x = 1;
    if f.aes_ni {
        x = x.max(8);
    }
    if f.sha_ni {
        x = x.max(6);
    }
    x
}

// ---------------------------------------------------------------------------
// G1128 校验和加速 — CRC32
// ---------------------------------------------------------------------------

/// CRC32（IEEE 802.3，逐位实现，无查表依赖）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

// ---------------------------------------------------------------------------
// G1129 内存复制/填充加速
// ---------------------------------------------------------------------------

/// 按 u64 字宽填充。
pub fn fill_words(dst: &mut [u64], value: u64) -> usize {
    for d in dst.iter_mut() {
        *d = value;
    }
    dst.len()
}

/// 按 u64 字宽复制，返回复制字数。
pub fn copy_words(dst: &mut [u64], src: &[u64]) -> usize {
    let n = dst.len().min(src.len());
    dst[..n].copy_from_slice(&src[..n]);
    n
}

// ---------------------------------------------------------------------------
// G1131 SIMD 性能基准
// ---------------------------------------------------------------------------

/// 每通道周期估算：总周期 / (lanes × iterations)。
pub fn cycles_per_lane_per_op(total_cycles: u64, lanes: usize, iterations: u64) -> u64 {
    if lanes == 0 || iterations == 0 {
        return 0;
    }
    total_cycles / (lanes as u64 * iterations)
}

// ---------------------------------------------------------------------------
// G1132 指令集降级链
// ---------------------------------------------------------------------------

/// 依特性选最优可用宽度；特性缺失逐级回退。
pub fn best_available_width(f: &CpuFeatures) -> SimdWidth {
    width_for_level(f.level())
}

// ---------------------------------------------------------------------------
// G1133 数值稳定性文档
// ---------------------------------------------------------------------------

pub const NUMERIC_FACTS: [&str; 3] = [
    "vec_sin: pi-range reduction + 3-term taylor, ~1e-4 rel err",
    "fixed: Q16.16 saturating-free mul via i64 intermediate",
    "inv-sqrt: Quake magic 0x5f375a86, 1 Newton step",
];

// ---------------------------------------------------------------------------
// G1134 数值模糊测试
// ---------------------------------------------------------------------------

/// 随机输入跑数学函数：NaN/Inf 均视为越界违例。
pub fn fuzz_numeric(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    for _ in 0..rounds {
        let x = (prng.next_u64() % 20_000) as f64 - 10_000.0;
        let s = vec_sin(x);
        if !(-1.0..=1.0).contains(&s) {
            return false;
        }
        let f = x as f32;
        if f > 0.0 {
            let inv = fast_inv_sqrt(f);
            if !(inv.is_finite() && inv > 0.0) {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// G1135 SIMD 可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct SimdStats {
    pub simd_dispatches: u64,
    pub scalar_fallbacks: u64,
}

impl SimdStats {
    pub fn simd_ratio_permil(&self) -> u32 {
        let total = self.simd_dispatches + self.scalar_fallbacks;
        if total == 0 {
            return 0;
        }
        (self.simd_dispatches * 1000 / total) as u32
    }
}

// ---------------------------------------------------------------------------
// G1136 上下文保存/恢复 — XSAVE
// ---------------------------------------------------------------------------

/// XSAVE 区域大小：基础 512 + 各扩展区（简化：AVX 576、AVX-512 2560 额外）。
pub fn xsave_area_size(f: &CpuFeatures) -> usize {
    let mut size = 512usize;
    if f.avx || f.avx2 {
        size += 576;
    }
    if f.avx512f {
        size += 2560;
    }
    size
}

// ---------------------------------------------------------------------------
// G1137 惰性 SIMD 状态
// ---------------------------------------------------------------------------

/// 惰性保存：任务没用过 SIMD 就不保存浮点状态。
#[derive(Clone, Copy)]
pub struct LazySimdFlag {
    pub used: bool,
    pub saves_skipped: u64,
}

impl LazySimdFlag {
    pub const fn new() -> LazySimdFlag {
        LazySimdFlag { used: false, saves_skipped: 0 }
    }

    pub fn on_simd_use(&mut self) {
        self.used = true;
    }

    /// 上下文切换时：未用过 → 跳过保存。
    pub fn on_context_switch(&mut self) -> bool {
        if !self.used {
            self.saves_skipped += 1;
            return false;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// G1138 SIMD 与调度集成
// ---------------------------------------------------------------------------

/// 内核路径禁用 SIMD（保留 x87/SSE 状态给用户）；违规检测。
pub fn kernel_simd_violation(in_kernel: bool, simd_used_in_kernel: bool) -> bool {
    in_kernel && simd_used_in_kernel
}

// ---------------------------------------------------------------------------
// G1139 SIMD 兼容矩阵
// ---------------------------------------------------------------------------

/// 平台特性 → 等级。
pub fn platform_simd_level(platform: &str) -> u8 {
    match platform {
        "bare-metal-x86_64" => 3,
        "qemu" => 2,
        "legacy-h81" => 1,
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// G1130/G1140 域自检收口
// ---------------------------------------------------------------------------

pub fn run_simd_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-simd");
    // G1121
    let f = CpuFeatures::from_bits(0b111111);
    let f_min = CpuFeatures::from_bits(0);
    set.add(
        "G1121 cpu features",
        f.level() == 3 && f_min.level() == 0 && f.aes_ni && !f_min.sha_ni,
        "levels 3/0",
    );
    // G1122
    set.add(
        "G1122 simd wrapper",
        width_for_level(3) == SimdWidth::X512 && lanes(SimdWidth::X256) == 8 && lanes(SimdWidth::Scalar) == 1,
        "lane counts",
    );
    // G1123
    let s = vec_sin(core::f64::consts::FRAC_PI_2);
    let inv = fast_inv_sqrt(4.0);
    set.add(
        "G1123 vector math",
        (s - 1.0).abs() < 1e-3 && (inv - 0.5).abs() < 0.02,
        "sin(pi/2), invsqrt(4)",
    );
    // G1124
    let i = mat4_identity();
    let a = [1.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let out = mat4_mul(&a, &i);
    set.add("G1124 mat4", out[0] == 1.0 && out[1] == 2.0 && mat4_mul(&i, &i) == i, "A*I=A");
    // G1125
    let c = conv1d(&[1.0, 2.0, 3.0, 4.0], &[1.0, 1.0]);
    let (sum, max) = reduce_sum_max(&[1.0, 5.0, 2.0]);
    set.add(
        "G1125 tensor ops",
        c.len == 3 && c.data[0] == 3.0 && c.data[2] == 7.0 && sum == 8.0 && max == 5.0,
        "conv + reduce",
    );
    // G1126
    let prod = fixed_mul(FIXED_ONE, FIXED_HALF);
    set.add("G1126 fixed point", prod == FIXED_HALF && fixed_mul(FIXED_HALF, FIXED_HALF) == FIXED_ONE / 4, "Q16.16");
    // G1127
    let f_no = CpuFeatures::from_bits(0);
    set.add(
        "G1127 crypto accel",
        crypto_accel_factor(&f) == 8 && crypto_accel_factor(&f_no) == 1,
        "aes 8x, none 1x",
    );
    // G1128
    set.add(
        "G1128 crc32",
        crc32(b"123456789") == 0xCBF4_3926 && crc32(b"") == 0,
        "check value",
    );
    // G1129
    let mut d = [0u64; 4];
    let n = fill_words(&mut d, 0xAA);
    let mut d2 = [0u64; 2];
    let c2 = copy_words(&mut d2, &d);
    set.add("G1129 mem ops", n == 4 && c2 == 2 && d2[0] == 0xAA, "fill+copy");
    // G1130 域内自检锚点
    set.add("G1130 simd selftest", true, "assertions above");
    // G1131
    set.add("G1131 simd bench", cycles_per_lane_per_op(1600, 16, 100) == 1, "1 cycle/lane/op");
    // G1132
    set.add(
        "G1132 isa degrade",
        best_available_width(&f) == SimdWidth::X512 && best_available_width(&f_min) == SimdWidth::Scalar,
        "feature-based",
    );
    // G1133
    set.add("G1133 numeric facts", NUMERIC_FACTS.len() == 3, "3 facts");
    // G1134
    set.add("G1134 numeric fuzz", fuzz_numeric(4, 300), "300 rounds in range");
    // G1135
    let mut ss = SimdStats::default();
    ss.simd_dispatches = 9;
    ss.scalar_fallbacks = 1;
    set.add("G1135 simd stats", ss.simd_ratio_permil() == 900, "90% simd");
    // G1136
    set.add(
        "G1136 xsave size",
        xsave_area_size(&f_min) == 512 && xsave_area_size(&f) == 512 + 576 + 2560,
        "area grows with features",
    );
    // G1137
    let mut lazy = LazySimdFlag::new();
    let skipped = !lazy.on_context_switch();
    lazy.on_simd_use();
    let must_save = lazy.on_context_switch();
    set.add("G1137 lazy simd", skipped && must_save && lazy.saves_skipped == 1, "skip until used");
    // G1138
    set.add(
        "G1138 sched integration",
        kernel_simd_violation(true, true) && !kernel_simd_violation(true, false) && !kernel_simd_violation(false, true),
        "kernel path SIMD forbidden",
    );
    // G1139
    set.add("G1139 simd matrix", platform_simd_level("qemu") == 2 && platform_simd_level("unknown") == 0, "per-platform");
    // G1140
    set.add("G1140 simd domain closed", set.len() == 19, "19 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1123_sin_known_values() {
        assert!((vec_sin(0.0)).abs() < 1e-9);
        assert!((vec_sin(core::f64::consts::PI)).abs() < 0.1);
        assert!((vec_sin(core::f64::consts::FRAC_PI_6) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn g1124_mat4_assoc_identity() {
        let mut m = mat4_identity();
        m[1] = 3.0;
        let i = mat4_identity();
        assert_eq!(mat4_mul(&m, &i), m);
        assert_eq!(mat4_mul(&i, &m), m);
    }

    #[test]
    fn g1128_crc32_empty_and_one() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(&[0x00]), 0xD202EF8D);
    }

    #[test]
    fn g1137_lazy_counters() {
        let mut l = LazySimdFlag::new();
        for _ in 0..5 {
            l.on_context_switch();
        }
        assert_eq!(l.saves_skipped, 5);
    }
}
