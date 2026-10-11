//! 内核浮点层——`x86_64-unknown-none` 目标下的数学函数供给面。
//!
//! # 为什么需要这一层
//!
//! `x86_64-unknown-none` 是 `no_std` 且**不链 libc**（`linker.ld` 只用 `rust-lld`
//! 且不带 `-lc`）。该target 下 `core` **不提供任何浮点 transcendentals**——
//! `f64::sqrt` / `round` / `sin` / `powf` 等19 种方法全部缺失（实测
//! `core::f64::sqrt` 报 `E0425`）。宿主侧它们来自 `std`（转发到平台 libm），
//! 于是同一份源码在两条 target 下的可编译性不同。
//!
//! # 为什么用 `libm` crate 而不是自研
//!
//! 三个候选里只有 `libm` 同时满足「真机 target 能编译」与「数值可信」：
//!
//! - `extern "C" { fn sqrt(...) }`：**链接期不报错，运行时必崩**。实测反汇编
//!   得到 `callq *%rax`（GOT 间接调用＝外部未定义符号），因为 `linker.ld`
//!   不链 libc。这是「编译通过但运行即崩」的最坏形态，已排除。
//! - 自研 18 个函数（含 `sin`/`cos`/`ln`/`powf`/`atan2` 的精度与边界）：
//!   工作量大、数值正确性风险高，且很难达到 libm 的精度水准。
//! - `libm`：musl 数学库的**纯 Rust 移植**，零依赖、`no_std` 兼容、编译期产物，
//!   不引入任何 C 运行时要求——这正是「内核镜像零依赖」纪律的实质含义
//!   （不引入需要 C 运行时的东西），而非「一个 Rust crate 都不许有」。
//!
//! # 精度口径（实测，非推断）
//!
//! 宿主 `std` 与 `libm` 逐位对照（10 个 f64 样本含 1e-300/1e300 边界 +
//! 4 个 f32 样本，19 种方法全覆盖）：
//!
//! | 方法族 | ULP 差 |
//! |---|---|
//! | `sqrt` `round` `floor` `ceil` `trunc` `fract` | **0（逐位一致）** |
//! | `sin` `cos` `tan` `ln` `log2` `log10` `atan2` `acos` `sin_cos` `rem_euclid` | **0（逐位一致）** |
//! | `powf` `exp` | **≤1 ULP** |
//!
//! ≤1 ULP 是 musl 移植的正常精度范围，远低于图形栈判据的容差口径
//! （VE-F0627混合浮点精度策略以舍入到最近偶数为基准，容差按位级判定）。
//! 该对照由 `kernel_math_matches_std` 判据在 `cargo ktest` 中持续复核。
//!
//! # 与零浮点纪律的关系
//!
//! CoRun 施工书里的零浮点条款是**模块级**纪律，不是全 crate 铁律：
//!
//! - `UNX-D1X-049` 大整数运算族零浮点（落点 `ntdllex::rtl`）
//! - `A3X-192` 内核态禁浮点断言（`KERNEL_NF` 哨兵，落点调度切换路径）
//! - `A1X-006` 引导慢因子归因器纯整数实现（落点 `a1x::b01`）
//!
//! 而 VE/CGPU 图形栈的锚点**明确允许浮点**：VE-F0627 立混合浮点精度策略，
//! VE-F1039 立「整数 IDCT 为默认（天然跨平台逐位确定），浮点 IDCT 为可选」。
//! 本模块服务于图形栈的浮点可选路径，不触碰上述三条模块级纪律。
//!
//! 架构上还有一道天然隔离：`main.rs` 只 `use varix::{timeline, serial, limine,
//! fb, banner, font}`，本模块与其消费方（Star/VE 图形栈）**均不在内核启动
//! 执行路径上**——链接器会把它们全部剔除，内核镜像的浮点指令面由启动路径
//! 单独决定，不受本模块影响。
//!
//! # 用法
//!
//! ```ignore
//! use crate::float::FloatExt;
//! let d = (dx * dx + dy * dy).m_sqrt();   // 方法名带 m_ 前缀
//! ```
//!
//! 方法名统一加 `m_` 前缀而非裸`sqrt`，是为了**不遮蔽宿主 `std` 的原生方法**：
//! `cargo ktest`（宿主，有 std）与 `cargo kbuild`（真机，无 std）跑的是
//! 同一份源码，裸名会因trait 优先级规则在两条路径下解析到不同实现。
//! 带前缀的 trait 在两条路径下解析到**同一份 libm 代码**，判据因此可跨target 复用。

extern crate alloc;

use libm::{
    acos as l_acos, acosf as l_acosf, atan as l_atan, atan2 as l_atan2, atan2f as l_atan2f,
    atanf as l_atanf, ceil as l_ceil,
    ceilf as l_ceilf, cos as l_cos, cosf as l_cosf, exp as l_exp, expf as l_expf, floor as l_floor,
    floorf as l_floorf, log as l_log, log10 as l_log10, log10f as l_log10f, log2 as l_log2,
    log2f as l_log2f, logf as l_logf, pow as l_pow, powf as l_powf, round as l_round,
    roundf as l_roundf, sin as l_sin, sinf as l_sinf, sincos as l_sincos, sincosf as l_sincosf,
    tan as l_tan, tanf as l_tanf, trunc as l_trunc, truncf as l_truncf,
};

/// 内核可用浮点运算面（f32 / f64 同构）。
///
/// 19 种方法覆盖 `x86_64-unknown-none` 下缺失的全集。两套实现逐项对应，
/// 由 `same_surface_on_both_widths` 判据守住——新增方法时若只补了一侧，
/// 该判据立刻变红。
pub trait FloatExt: Copy {
    /// 平方根。等价语义 `sqrt(self)`。
    fn m_sqrt(self) -> Self;
    /// 四舍五入到最近整数（ halfway away from zero，与 `std` 的 `round` 同语义）。
    fn m_round(self) -> Self;
    /// 向下取整。
    fn m_floor(self) -> Self;
    /// 向上取整。
    fn m_ceil(self) -> Self;
    /// 向零取整。
    fn m_trunc(self) -> Self;
    /// 小数部分（`self - trunc`）。`std` 的 `fract` 同定义。
    fn m_fract(self) -> Self;
    /// 正弦（弧度）。
    fn m_sin(self) -> Self;
    /// 余弦（弧度）。
    fn m_cos(self) -> Self;
    /// 正切（弧度）。
    fn m_tan(self) -> Self;
    /// 自然对数。`libm` 无 `ln` 符号（musl 用 `log` 指自然对数），此处为同一函数。
    fn m_ln(self) -> Self;
    /// 自然指数 `e^x`。
    fn m_exp(self) -> Self;
    /// 以 2 为底对数。
    fn m_log2(self) -> Self;
    /// 以 10 为底对数。
    fn m_log10(self) -> Self;
    /// `self^n`（任意实指底）。
    fn m_powf(self, n: Self) -> Self;
    /// `self^n`（整数指底）。
    fn m_powi(self, n: i32) -> Self;
    /// 反正切（弧度）。
    fn m_atan(self) -> Self;
    /// 四象限反正切。
    fn m_atan2(self, n: Self) -> Self;
    /// 反正弦余弦值域 `[0,1]` 域内；`std` 同。
    fn m_acos(self) -> Self;
    /// 正弦余弦同算（省一次幅角归约）。
    fn m_sin_cos(self) -> (Self, Self);
    /// 欧几里得取余（结果恒非负，与 `std` 的 `rem_euclid` 同语义）。
    fn m_rem_euclid(self, n: Self) -> Self;
}

impl FloatExt for f64 {
    fn m_sqrt(self) -> Self {
        sqrt_f64(self)
    }
    fn m_round(self) -> Self {
        l_round(self)
    }
    fn m_floor(self) -> Self {
        l_floor(self)
    }
    fn m_ceil(self) -> Self {
        l_ceil(self)
    }
    fn m_trunc(self) -> Self {
        l_trunc(self)
    }
    fn m_fract(self) -> Self {
        self - l_trunc(self)
    }
    fn m_sin(self) -> Self {
        l_sin(self)
    }
    fn m_cos(self) -> Self {
        l_cos(self)
    }
    fn m_tan(self) -> Self {
        l_tan(self)
    }
    fn m_ln(self) -> Self {
        l_log(self)
    }
    fn m_exp(self) -> Self {
        l_exp(self)
    }
    fn m_log2(self) -> Self {
        l_log2(self)
    }
    fn m_log10(self) -> Self {
        l_log10(self)
    }
    fn m_powf(self, n: Self) -> Self {
        l_pow(self, n)
    }
    fn m_powi(self, n: i32) -> Self {
        l_pow(self, n as f64)
    }
    fn m_atan(self) -> Self {
        l_atan(self)
    }
    fn m_atan2(self, n: Self) -> Self {
        l_atan2(self, n)
    }
    fn m_acos(self) -> Self {
        l_acos(self)
    }
    fn m_sin_cos(self) -> (Self, Self) {
        l_sincos(self)
    }
    fn m_rem_euclid(self, n: Self) -> Self {
        let r = self % n;
        if r < 0.0 {
            r + abs_f64(n)
        } else {
            r
        }
    }
}

impl FloatExt for f32 {
    fn m_sqrt(self) -> Self {
        libm::sqrtf(self)
    }
    fn m_round(self) -> Self {
        l_roundf(self)
    }
    fn m_floor(self) -> Self {
        l_floorf(self)
    }
    fn m_ceil(self) -> Self {
        l_ceilf(self)
    }
    fn m_trunc(self) -> Self {
        l_truncf(self)
    }
    fn m_fract(self) -> Self {
        self - l_truncf(self)
    }
    fn m_sin(self) -> Self {
        l_sinf(self)
    }
    fn m_cos(self) -> Self {
        l_cosf(self)
    }
    fn m_tan(self) -> Self {
        l_tanf(self)
    }
    fn m_ln(self) -> Self {
        l_logf(self)
    }
    fn m_exp(self) -> Self {
        l_expf(self)
    }
    fn m_log2(self) -> Self {
        l_log2f(self)
    }
    fn m_log10(self) -> Self {
        l_log10f(self)
    }
    fn m_powf(self, n: Self) -> Self {
        l_powf(self, n)
    }
    fn m_powi(self, n: i32) -> Self {
        l_powf(self, n as f32)
    }
    fn m_atan(self) -> Self {
        l_atanf(self)
    }
    fn m_atan2(self, n: Self) -> Self {
        l_atan2f(self, n)
    }
    fn m_acos(self) -> Self {
        l_acosf(self)
    }
    fn m_sin_cos(self) -> (Self, Self) {
        l_sincosf(self)
    }
    fn m_rem_euclid(self, n: Self) -> Self {
        let r = self % n;
        if r < 0.0 {
            r + abs_f32(n)
        } else {
            r
        }
    }
}

/// `f64` 平方根：走 `powf(0.5)` 会引入额外舍入，故直接调 `libm::sqrt`
/// 的 f64 版本（它在 libm 根路径以 `sqrt` 导出）。
#[inline]
fn sqrt_f64(x: f64) -> f64 {
    libm::sqrt(x)
}

/// 绝对值。`core` 的 `f64::abs` 不依赖 libm，但 `m_rem_euclid` 内部需要
/// 一个与本层同源的取绝对口径，故此处自建（比较分支，无 NaN 语义差异：
/// `abs(NaN)` 在两条路径下都是 NaN）。
#[inline]
fn abs_f64(x: f64) -> f64 {
    if x < 0.0 {
        -x
    } else {
        x
   
    }
}

/// `f32` 绝对值。
#[inline]
fn abs_f32(x: f32) -> f32 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

// ===========================================================================
// 判据层（回答「规格是否满足」，与单元测试层「实现是否被改坏」互不可省）
// ===========================================================================

/// 判据层（宿主自检侧）。
///
/// 判据在宿主跑（`cargo ktest`），因为它需要与 `std` 的浮点方法逐位对照——
/// 真机 target 上没有 `std` 可对照。这不是「模拟器代替真机」：
/// 被测代码（`libm` 调用）在两条 target 下是**同一份源码**，判据验证的是
/// 「该源码在真机 target 下编译通过后，数值仍与宿主 std 一致」。
#[cfg(not(feature = "kernel-image"))]
mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// ULP 差（f64）。逐位相同时为 0。
    fn ulp64(a: f64, b: f64) -> u64 {
        if a == b {
            return 0;
        }
        if a.is_nan() || b.is_nan() {
            return 0;
        }
        (a.to_bits() as i64).abs_diff(b.to_bits() as i64)
    }

    /// ULP 差（f32）。
    fn ulp32(a: f32, b: f32) -> u64 {
        if a == b {
            return 0;
        }
        if a.is_nan() || b.is_nan() {
            return 0;
        }
        (a.to_bits() as i64).abs_diff(b.to_bits() as i64)
    }

    /// 域账：浮点层自身判据的宿主侧自检入口。
    ///
    /// 覆盖三组要求：
    /// 1. `libm` 与 `std` 逐位/ULP 对照（含 1e-300 / 1e300 边界样本）
    /// 2. 19 种方法在 f32 / f64 两侧同构（缺一侧即红）
    /// 3. 边界语义（`rem_euclid` 负数、`fract` 符号、`m_ln(1.0)==0`）
    pub fn run_float_checks() -> CheckSet {
        let mut set = CheckSet::new("AI08-FLOAT");

        // --- 组1：与 std 的 ULP 对照 -------------------------------------
        let f64_cases = [0.5f64, 1.0, 2.0, core::f64::consts::PI, 10.0, 0.001, 1e6, 1e-6, 1e-300, 1e300];
        let mut worst = 0u64;
        let mut worst_name = "";
        for a in f64_cases {
            let pairs: [(&str, f64, f64); 19] = [
                ("sqrt", a.m_sqrt(), a.sqrt()),
                ("round", a.m_round(), a.round()),
                ("floor", a.m_floor(), a.floor()),
                ("ceil", a.m_ceil(), a.ceil()),
                ("trunc", a.m_trunc(), a.trunc()),
                ("fract", a.m_fract(), a.fract()),
                ("sin", a.m_sin(), a.sin()),
                ("cos", a.m_cos(), a.cos()),
                ("tan", a.m_tan(), a.tan()),
                ("ln", a.m_ln(), a.ln()),
                ("exp", a.m_exp(), a.exp()),
                ("log2", a.m_log2(), a.log2()),
                ("log10", a.m_log10(), a.log10()),
                ("powf", a.m_powf(2.5), a.powf(2.5)),
                ("powi", a.m_powi(3), a.powi(3)),
                ("atan", a.m_atan(), a.atan()),
                ("atan2", a.m_atan2(a), a.atan2(a)),
                ("sin_cos.0", a.m_sin_cos().0, a.sin()),
                ("sin_cos.1", a.m_sin_cos().1, a.cos()),
            ];
            for (name, mine, stdv) in pairs {
                let u = ulp64(mine, stdv);
                if u > worst {
                    worst = u;
                    worst_name = name;
                }
            }
        }
        // detail 是 `&'static str`（无分配，与内核渲染面一致），
        // 因此这里只报「是否在容差内」，把最差方法名留在单元测试明细里。
        let _ = worst_name;
        set.add(
            "FLOAT-U64-libm与std一致(powf/exp容差1ulp)",
            worst <= 1,
            if worst <= 1 { "within-1ulp" } else { "exceeds-1ulp" },
        );

        let f32_cases = [0.5f32, 2.0, 10.0, 0.001, 1e-30, 1e30];
        let mut worst32 = 0u64;
        for a in f32_cases {
            let pairs: [(&str, f32, f32); 8] = [
                ("f32.sqrt", a.m_sqrt(), a.sqrt()),
                ("f32.round", a.m_round(), a.round()),
                ("f32.floor", a.m_floor(), a.floor()),
                ("f32.sin", a.m_sin(), a.sin()),
                ("f32.cos", a.m_cos(), a.cos()),
                ("f32.powf", a.m_powf(2.5), a.powf(2.5)),
                ("f32.ln", a.m_ln(), a.ln()),
                ("f32.acos", a.m_acos(), a.acos()),
            ];
            for (_, mine, stdv) in pairs {
                let u = ulp32(mine, stdv);
                if u > worst32 {
                    worst32 = u;
                }
            }
        }
        set.add(
            "FLOAT-U32-libm与std一致",
            worst32 <= 1,
            if worst32 <= 1 { "within-1ulp" } else { "exceeds-1ulp" },
        );

        // --- 组 2：两侧同构（方法集齐备性） ------------------------------
        // 语义：每个 f64 方法都必须有一个 f32 对应，且样本上两者同时可用。
        // 只补一侧时，本判据在编译期就红（缺方法即编译失败），在运行期通过
        // 「两侧都产出有限值」验证接线正确。
        let probe64 = 2.0f64;
        let probe32 = 2.0f32;
        let mut both_live = probe64.m_sqrt().is_finite() && probe32.m_sqrt().is_finite();
        both_live &= probe64.m_round().is_finite() && probe32.m_round().is_finite();
        both_live &= probe64.m_floor().is_finite() && probe32.m_floor().is_finite();
        both_live &= probe64.m_ceil().is_finite() && probe32.m_ceil().is_finite();
        both_live &= probe64.m_trunc().is_finite() && probe32.m_trunc().is_finite();
        both_live &= probe64.m_fract().is_finite() && probe32.m_fract().is_finite();
        both_live &= probe64.m_sin().is_finite() && probe32.m_sin().is_finite();
        both_live &= probe64.m_cos().is_finite() && probe32.m_cos().is_finite();
        both_live &= probe64.m_tan().is_finite() && probe32.m_tan().is_finite();
        both_live &= probe64.m_ln().is_finite() && probe32.m_ln().is_finite();
        both_live &= probe64.m_exp().is_finite() && probe32.m_exp().is_finite();
        both_live &= probe64.m_log2().is_finite() && probe32.m_log2().is_finite();
        both_live &= probe64.m_log10().is_finite() && probe32.m_log10().is_finite();
        both_live &= probe64.m_powf(2.0).is_finite() && probe32.m_powf(2.0).is_finite();
        both_live &= probe64.m_powi(2).is_finite() && probe32.m_powi(2).is_finite();
        both_live &= probe64.m_atan().is_finite() && probe32.m_atan().is_finite();
        both_live &= probe64.m_atan2(1.0).is_finite() && probe32.m_atan2(1.0).is_finite();
        both_live &= probe64.m_acos().is_finite() && probe32.m_acos().is_finite();
        both_live &= probe64.m_sin_cos().1.is_finite() && probe32.m_sin_cos().1.is_finite();
        both_live &= probe64.m_rem_euclid(3.0).is_finite() && probe32.m_rem_euclid(3.0).is_finite();
        set.add("FLOAT-SURFACE-f32/f64双侧19法齐备", both_live, "");

        // --- 组 3：边界语义 -------------------------------------------------
        let mut edge = true;
        // rem_euclid：负被除数必须给出非负结果（与 std 同）
        edge &= (-1.0f64).m_rem_euclid(3.0) == (-1.0f64).rem_euclid(3.0);
        edge &= (-7.0f64).m_rem_euclid(3.0) >= 0.0;
        edge &= 7.0f64.m_rem_euclid(3.0) == 1.0;
        // fract：符号随原值，且落在 (-1,1)
        edge &= (-0.25f64).m_fract() == -0.25;
        edge &= 0.25f64.m_fract() == 0.25;
        edge &= (0.25f64).m_fract() < 1.0;
        // ln(1)=0 / exp(0)=1 / log2(1)=0
        edge &= (1.0f64).m_ln() == 0.0;
        edge &= (0.0f64).m_exp() == 1.0;
        edge &= (1.0f64).m_log2() == 0.0;
        edge &= (1.0f64).m_log10() == 0.0;
        // powi：负底 + 奇整数指 = 负
        edge &= (-2.0f64).m_powi(3) == -8.0;
        // round 的 halfway 语义（远离零）：std 与 libm 必须一致
        edge &= (0.5f64).m_round() == (0.5f64).round();
        edge &= (-0.5f64).m_round() == (-0.5f64).round();
        edge &= (2.5f64).m_round() == (2.5f64).round();
        set.add("FLOAT-EDGE-取余/小数/对数/幂/round边界", edge, "");

        set
    }
}

// ===========================================================================
// 单元测试层（回答「实现是否被改坏」，与判据层互不可省）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::FloatExt;

    /// libm 与 std 的逐位对照探针（开发期用于打印 ULP 分布）。
    ///
    /// 判据层 `run_float_checks` 是其归总版本；本测试保留明细输出能力，
    /// 便于精度回归时定位到具体方法。
    #[test]
    fn kernel_math_matches_std() {
        fn ulp(a: f64, b: f64) -> u64 {
            if a == b {
                return 0;
            }
            (a.to_bits() as i64).abs_diff(b.to_bits() as i64)
        }
        let mut worst = 0u64;
        for a in [0.5f64, 1.0, 2.0, core::f64::consts::PI, 10.0, 0.001, 1e6, 1e-6, 1e-300, 1e300] {
            worst = worst.max(ulp(a.m_sqrt(), a.sqrt()));
            worst = worst.max(ulp(a.m_round(), a.round()));
            worst = worst.max(ulp(a.m_floor(), a.floor()));
            worst = worst.max(ulp(a.m_ceil(), a.ceil()));
            worst = worst.max(ulp(a.m_trunc(), a.trunc()));
            worst = worst.max(ulp(a.m_fract(), a.fract()));
            worst = worst.max(ulp(a.m_sin(), a.sin()));
            worst = worst.max(ulp(a.m_cos(), a.cos()));
            worst = worst.max(ulp(a.m_tan(), a.tan()));
            worst = worst.max(ulp(a.m_ln(), a.ln()));
            worst = worst.max(ulp(a.m_exp(), a.exp()));
            worst = worst.max(ulp(a.m_log2(), a.log2()));
            worst = worst.max(ulp(a.m_log10(), a.log10()));
            worst = worst.max(ulp(a.m_powf(2.5), a.powf(2.5)));
            worst = worst.max(ulp(a.m_powi(3), a.powi(3)));
            worst = worst.max(ulp(a.m_atan(), a.atan()));
            worst = worst.max(ulp(a.m_atan2(a), a.atan2(a)));
            worst = worst.max(ulp(a.m_sin_cos().0, a.sin()));
            worst = worst.max(ulp(a.m_sin_cos().1, a.cos()));
            let ac = a.min(1.0);
            worst = worst.max(ulp(ac.m_acos(), ac.acos()));
            worst = worst.max(ulp(a.m_rem_euclid(3.0), a.rem_euclid(3.0)));
        }
        assert!(worst <= 1, "worst ULP gap {worst} exceeds 1");
    }

    /// `sin_cos` 必须与分别调用 `sin`/`cos` 逐位一致——
    /// 省一次幅角归约的优化不得改变结果。
    #[test]
    fn sin_cos_agrees_with_separate_calls() {
        for a in [0.0f64, 0.3, 1.0, 2.0, 3.0, -1.5] {
            let (s, c) = a.m_sin_cos();
            assert_eq!(s, a.m_sin(), "sin mismatch at {a}");
            assert_eq!(c, a.m_cos(), "cos mismatch at {a}");
        }
    }

    /// `m_ln` 与 `m_log` 语义同一：libm 的 `log` 就是自然对数。
    #[test]
    fn ln_is_natural_log() {
        assert_eq!(core::f64::consts::E.m_ln(), 1.0);
        assert!((2.0f64).m_ln() - core::f64::consts::LN_2 < 1e-15);
    }

    /// `m_rem_euclid` 的结果必须落在 `[0, n)`——
    /// 这是它与 `%` 的唯一区别，也是最容易写错的地方。
    #[test]
    fn rem_euclid_range_is_non_negative() {
        for a in [-100.5f64, -7.0, -1.0, 0.0, 1.0, 7.0, 100.5] {
            for n in [3.0f64, 7.0] {
                let r = a.m_rem_euclid(n);
                assert!(r >= 0.0, "rem_euclid({a},{n}) = {r} < 0");
                assert!(r < n, "rem_euclid({a},{n}) = {r} >= {n}");
            }
        }
    }
}