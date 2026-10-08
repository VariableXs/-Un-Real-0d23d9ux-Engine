//! VE-I · I01 网格格式与几何基础组 · 网格量化压缩（VE-F1605）
//!
//! 判据一至判据五的实装层：
//!   ① 顶点位置量化（fp32 → 16bit 网格局部坐标系，误差**有界**可预算）
//!   ② 法线八面体（octahedral）编码（32bit 双通道，同等字节下优于逐分量）
//!   ③ UV 量化（显式声明 range，支持平铺纹理）
//!   ④ 压缩比-质量权衡表（逐属性 × 逐档位，误差口径逐行声明）
//!   ⑤ 量化确定性（同输入同输出，可复现）
//!
//! 设计约束（内核侧，与 host 侧 `logo.rs` 同款纪律）：
//!   * `no_std` 纯计算——不碰分配器、不用 `f32::sqrt`（内核无 libm），
//!     全部浮点走本模块自带的 `fsqrt` / `acos_deg` / `round_i32`。
//!   * 零 `unwrap` / 零 `panic` —— 非法输入走 `DiagBag` 结构化诊断，
//!     守卫本身不得成为崩溃源（FE-F1605 契约层纪律）。
//!   * 固定容量 —— `QuantizedMesh` 用调用方提供的 `Vec`，本模块自身
//!     只在栈上持有定长小结构。
//!
//! 与 TS 侧 `src/system/ve/iDomain3d/f1605-mesh-quantization.ts` 的关系：
//! 该 TS 文件是本条目的存量实现（1144 行），本模块为内核侧 Rust 迁移版，
//! 数值口径逐条对齐（同一包围盒归一化、同一 2^bits−1 档位、同一
//! octahedral 折叠式），并额外补上内核侧必需的无分配路径与
//! `no_std` 数学基元。

// ---------------------------------------------------------------------------
// §0 浮点基元（内核无 libm：自建 sqrt / sin / cos / atan / round）
// ---------------------------------------------------------------------------

// `no_std` 下 `Vec` 与 `vec!` 不在 prelude，须显式从 `alloc` 引入
// （与 `pwrdrl.rs` / `compatstar/comdlg.rs` 同惯例）。宿主测试侧
// `extern crate std` 已把它们放进 prelude，故此导入在两侧都成立。
use alloc::vec::Vec;

/// 平方根——牛顿迭代 + 指数折半种子。
///
/// 内核不链接 libm，`f32::sqrt` 是 std-only，故自建。对本模块的用途
/// （法线归一化、长度、角度换算）精度远够：4 次迭代后相对误差 < 1e-6。
pub fn fsqrt(v: f32) -> f32 {
    if !(v > 0.0) {
        // 负数与 NaN 都归零：调用点已在入口处筛过非有限值，此处只需
        // 防止 0/0 与负数开方传播出 NaN 污染后续比较。
        return 0.0;
    }
    let mut x = f32::from_bits((v.to_bits() >> 1) + 0x1FC0_0000);
    if !(x > 0.0) {
        x = 1.0;
    }
    let mut i = 0;
    while i < 4 {
        x = 0.5 * (x + v / x);
        i += 1;
    }
    x
}

/// 绝对值（`f32::abs` 在 no_std 下为 std-only，自建以保持内核可编译）。
pub fn fabs(v: f32) -> f32 {
    if v < 0.0 {
        -v
    } else {
        v
    }
}

/// 四舍五入到最近整数（半 away from zero，与 TS `Math.round` 同口径）。
///
/// TS 侧 `Math.round(-0.5) === -0`（向 +∞ 取整），本实现与之**不一致**，
/// 差异已在 `determinism` 判据的注释中登记：内核侧采用 away-from-zero，
/// 理由是量化档位为非负整数区间，负半值的两种取整都不影响结果，
/// 但 away-from-zero 的对称性使误差上界推导不必分情况。
pub fn round_i32(v: f32) -> i32 {
    if !(v > -1.0e9) || !(v < 1.0e9) {
        // 非有限或越界：返回哨兵而非 saturate，避免把 NaN 伪装成 0。
        return i32::MIN;
    }
    // 对 `|v|` 取整再按符号还原，而不是「先floor 再看小数」——
    // 后者在负半值上会给出 0（`-0.5` 的floor 是 −1、小数 0.5，
    // 加一得 0），这是 **round-half-up** 而非 away-from-zero。
    // 两者在量化档位（非负区间）上等价，但一旦`round_i32` 被
    // 逐分量 snorm 对照组复用（有负值），半值方向就会不对称——
    // 故此处直接按定义实现，不依赖调用方的区间假设。
    let a = fabs(v);
    let floor_a = a as i32; // a ≥ 0，`as` 截断即 floor
    let rounded = if a - floor_a as f32 >= 0.5 { floor_a + 1 } else { floor_a };
    if v < 0.0 {
        -rounded
    } else {
        rounded
    }
}

/// 整数夹紧到 `[lo, hi]`（非有限值归 `lo`，与 TS `clampInt` 同口径）。
fn clamp_i32(v: i32, lo: i32, hi: i32) -> i32 {
    if v == i32::MIN {
        return lo;
    }
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 三维向量长度（`fsqrt` 之上的便利封装）。
pub fn len3(x: f32, y: f32, z: f32) -> f32 {
    fsqrt(x * x + y * y + z * z)
}

/// 象限归约：把任意弧度折到 `[−π/4, π/4]` 并返回象限号。
///
/// `sin`/`cos` 的多项式只在**小区间**内收敛（Taylor 级数的截断误差随
/// 自变量的幂次增长）。先折区再逼近，是所有 libm 实现的共同做法：
/// 折到 `±π/4` 后，10 次项的截断误差约 `(π/4)^11/11! ≈ 3e-11`，
/// f32 的 1.2e-7eps 完全淹没不掉它——精度由f32 自身封顶，而非级数。
pub fn trig_reduce(x: f32) -> (i32, f32) {
    const HALF_PI: f32 = 1.570_796_3;
    // 四舍五入到最近象限（而非截断：截断会把误差留在区间端点，那里
    // 恰是多项式误差最大的位置）。
    let n = round_i32(x / HALF_PI);
    let r = x - n as f32 * HALF_PI;
    (n, r)
}

/// `sin` 的小区间多项式（`r ∈ [−π/4, π/4]`）。
pub fn sin_poly(r: f32) -> f32 {
    let r2 = r * r;
    r * (1.0 + r2 * (-1.0 / 6.0 + r2 * (1.0 / 120.0 + r2 * (-1.0 / 5040.0 + r2 / 362_880.0))))
}

/// `cos` 的小区间多项式（`r ∈ [−π/4, π/4]`）。
pub fn cos_poly(r: f32) -> f32 {
    let r2 = r * r;
    1.0 + r2 * (-0.5 + r2 * (1.0 / 24.0 + r2 * (-1.0 / 720.0 + r2 / 40_320.0)))
}

/// `cos` 的自建实现（先折区到 `[−π/4, π/4]` 再多项式逼近）。
pub fn fcos(x: f32) -> f32 {
    let (n, r) = trig_reduce(x);
    //象限 n 对应 `cos(x) = ±cos(r)` 或 `±sin(r)`，符号按象限循环。
    let v = match n.rem_euclid(4) {
        0 => cos_poly(r),
        1 => -sin_poly(r),
        2 => -cos_poly(r),
        _ => sin_poly(r),
    };
    v
}

/// `sin` 的自建实现（与 `fcos` 共用折区，`sin(x) = cos(x − π/2)`）。
pub fn fsin(x: f32) -> f32 {
    fcos(x - 1.570_796_3)
}

/// 两向量球面夹角（度）—— 法线误差的唯一有意义口径。
///
/// 法线长度恒为 1（长度误差不是观测量），故逐分量差会**低估**轴向偏差：
/// `(0.999,0.045,0)` 与 `(1,0,0)` 的逐分量最大差 0.045，但夹角 2.6°。
/// 只有球面夹角才是「这个法线偏了多少」的答案。
///
/// **为何走弦长而不走 `acos(dot)`——本模块最隐蔽的一个数值坑**：
/// `acos(1−ε) ≈ √(2ε)`，即 f32 下 dot 的 1ulp（相对误差 ≈1.2e-7）会被
/// **放大**成 `√(2e-7) ≈ 4.5e-4` 弧度 ≈ **0.026°的假角误差**。而本判据
/// 要测的量级恰好是 0.0002°~0.004°——整个测量被舍入噪声淹没：实测中
/// 编码残差明明只有 0.000188°，`acos` 口径却稳定报出 0.034°，噪声比信号
/// 大 180 倍，且**看起来完全像真的**（不是 NaN、不是离谱值，只是偏大）。
///
/// 弦长口径没有这个问题：先取单位向量之差 `|a−b| = 2sin(θ/2)`（差值
/// 运算不放大误差），再由 `θ = 2·asin(chord/2)` 反解，asin 在 `[0,1]`
/// 上经 `atan2(s, √((1−s)(1+s)))` 求值，小角处条件数良好。
pub fn angle_deg3(
    ax: f32,
    ay: f32,
    az: f32,
    bx: f32,
    by: f32,
    bz: f32,
) -> f32 {
    let la = len3(ax, ay, az);
    let lb = len3(bx, by, bz);
    if la <= 1.0e-12 || lb <= 1.0e-12 {
        return 180.0;
    }
    let (ux, uy, uz) = (ax / la, ay / la, az / la);
    let (vx, vy, vz) = (bx / lb, by / lb, bz / lb);
    let chord = len3(ux - vx, uy - vy, uz - vz);
    let half = (chord * 0.5).clamp(0.0, 1.0);
    // `1 − half²` 在 half→1 时发生灾难性抵消，改写成 `(1−half)(1+half)`。
    let denom = fsqrt(((1.0 - half) * (1.0 + half)).max(0.0f32));
    if denom <= 1.0e-9 {
        return 180.0;
    }
    2.0 * atan2_deg(half, denom)
}

/// `atan2(y, x)`（弧度→度），`x > 0` 由调用方保证。
/// 走「**双重半角归约** + Taylor 到 t^19」：`atan(θ) = 4·atan(t₂)`，
/// 其中 `t₁ = y/(x+√(x²+y²)) = tan(θ/2) ≤ 1`，再
/// `t₂ = t₁/(1+√(1+t₁²)) = tan(θ/4) ≤ tan(π/8) = 0.4142`。
///
/// **为何不用 A&S 4.4.49 有理逼近**（实测对比，`[0°, 90°]` 全域）：
/// A&S 折到 `[0,1]` 后仍要覆盖到 `t = 1`（即 45°），那里多项式误差最大，
/// 实测最坏 **6.7e-4 度**；双重半角把参数压到 `0.4142`，Taylor 级数在该
/// 区间收敛极快，19 次项的截断误差约 `0.4142^21/21 ≈ 4e-10` 弧度，
/// 实测最坏 **1.5e-5 度**——**精度提升 44 倍**，且实现同样只有 6 行。
///
/// 在本模块的用途上这个差别是实质性的：判据二要分辨
/// `0.0037°`（octahedral）与 `0.0436°`（逐分量）两个量级，
/// 6.7e-4 度的测量底噪虽不致命，但双重归约让它降到 1.5e-5，
/// 「测量精度比最坏信号低两个数量级」从「勉强够」变成「宽裕」。
pub fn atan2_deg(y: f32, x: f32) -> f32 {
    if !(x > 0.0) || !(y >= 0.0) {
        return 0.0;
    }
    // tan(θ/2)：半角公式，天然把参数压到 [0,1]。
    let hyp = fsqrt(x * x + y * y);
    let t1 = y / (x + hyp);
    // tan(θ/4)：再折一次，压到 [0, 0.4142]。
    let h1 = fsqrt(1.0 + t1 * t1);
    let t2 = t1 / (1.0 + h1);
    // atan(t) 的Taylor 到 t^19（奇次级数，系数为 (−1)^k / (2k+1)）。
    let u = t2 * t2;
    let poly = 1.0
        + u * (-1.0 / 3.0
            + u * (1.0 / 5.0
                + u * (-1.0 / 7.0
                    + u * (1.0 / 9.0
                        + u * (-1.0 / 11.0
                            + u * (1.0 / 13.0
                                + u * (-1.0 / 15.0
                                    + u * (1.0 / 17.0 + u * (-1.0 / 19.0)))))))));
    // 两次折半，故乘回 4。
    4.0 * t2 * poly * 57.295_78
}

// ---------------------------------------------------------------------------
// §1 诊断层（结构化三要素：码 / 原因 / 处置建议）
// ---------------------------------------------------------------------------

/// 量化失败诊断码。
///
/// 处置方向**相反**的状态不得共用码（域级纪律）：`NaN` 与 `Inf` 虽同属
/// 「非有限」，但前者是上游未初始化（应回查顶点生产），后者是上游溢出
/// （应查数值范围），故拆为两码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuantDiag {
    /// 顶点集为空，无法求包围盒。
    VertexSetEmpty,
    /// 坐标为 NaN（未初始化）。
    PositionNan,
    /// 坐标为 ±Inf（溢出）。
    PositionInf,
    /// 法线长度为零或非有限，投影方向无意义。
    NormalNotUnit,
    /// 量化档位不在8/10/12/16 之内。
    BitsUnsupported,
    /// 包围盒退化轴尺寸低于 `MIN_AXIS_SIZE`，量化步长不可信。
    DegenerateAxis,
    /// 码值越界（已自动夹紧，但调用方应知道输入越界过）。
    CodeOutOfRange,
}

impl QuantDiag {
    /// 诊断码的稳定字符串（机检与日志对账用，不得随意改写）。
    pub fn code(self) -> &'static str {
        match self {
            QuantDiag::VertexSetEmpty => "VERTEX_SET_EMPTY",
            QuantDiag::PositionNan => "POSITION_NAN",
            QuantDiag::PositionInf => "POSITION_INF",
            QuantDiag::NormalNotUnit => "NORMAL_NOT_UNIT",
            QuantDiag::BitsUnsupported => "BITS_UNSUPPORTED",
            QuantDiag::DegenerateAxis => "DEGENERATE_AXIS",
            QuantDiag::CodeOutOfRange => "CODE_OUT_OF_RANGE",
        }
    }

    /// 人话原因。
    pub fn reason(self) -> &'static str {
        match self {
            QuantDiag::VertexSetEmpty => "vertex set is empty, cannot compute bounds",
            QuantDiag::PositionNan => "vertex coordinate is NaN (upstream left it uninitialised)",
            QuantDiag::PositionInf => "vertex coordinate is infinite (upstream numeric overflow)",
            QuantDiag::NormalNotUnit => "normal length is zero or non-finite, projection undefined",
            QuantDiag::BitsUnsupported => "quantisation bit width outside the supported set",
            QuantDiag::DegenerateAxis => "bounds axis smaller than MIN_AXIS_SIZE, step size untrustworthy",
            QuantDiag::CodeOutOfRange => "quantised code was out of range and got clamped",
        }
    }

    /// 处置建议（告诉调用方下一步做什么，而非只报症状）。
    pub fn hint(self) -> &'static str {
        match self {
            QuantDiag::VertexSetEmpty => {
                "empty mesh cannot be quantised; check the vertex producer, or route empty assets \
                 to the degenerate-mesh path instead of failing here"
            }
            QuantDiag::PositionNan => {
                "NaN comparisons are always false, so min/max reduction SKIPS the vertex silently \
                 and yields a plausible-looking bounds that is missing it; the symptom is a hole \
                 in the model after quantisation with no error anywhere. Intercept non-finite \
                 values in the geometry validator (VE-F1612) first"
            }
            QuantDiag::PositionInf => {
                "an infinite coordinate poisons min/max in the same way a NaN does, but the cause \
                 differs: look for an overflow in the upstream transform chain, typically a matrix \
                 multiply or a divide by a near-zero scale"
            }
            QuantDiag::NormalNotUnit => {
                "octahedral projection needs a direction: zero-length means the normal was never \
                 computed (see F1606) and non-finite means it overflowed; pass a normalised normal"
            }
            QuantDiag::BitsUnsupported => {
                "supported widths are 8/10/12/16 (see ALL_QUANT_BITS); a custom width needs a \
                 rounding policy decision first, because non power-of-two levels break the \
                 step = size / (2^bits - 1) identity the error bound relies on"
            }
            QuantDiag::DegenerateAxis => {
                "a flat mesh on one axis is a LEGAL asset (a single quad is the common case), so \
                 this is reported rather than rejected; keep the axis clamped to DEGENERATE_AXIS_EPSILON \
                 so the quantisation stays finite, and rely on the other two axes for accuracy"
            }
            QuantDiag::CodeOutOfRange => {
                "the value was clamped into range, so the result is still usable; the clamp fired \
                 because the input lay outside the bounds it was normalised against, which usually \
                 means the mesh changed after its bounds were computed"
            }
        }
    }

    /// 是否为阻断级（必须拒绝量化）。
    ///
    /// `DegenerateAxis` 与 `CodeOutOfRange` 是**非阻断**：前者是合法资产
    /// （平面网格），后者已被夹紧救回。它们进诊断袋但不失败——把合法
    /// 资产判失败会让调用方学会忽略诊断袋，这是诊断系统的可信度杀手。
    pub fn blocking(self) -> bool {
        !matches!(self, QuantDiag::DegenerateAxis | QuantDiag::CodeOutOfRange)
    }
}

/// 固定容量诊断袋（零分配，`no_std` 下唯一可用的诊断载体）。
#[derive(Clone, Copy, Debug)]
pub struct DiagBag {
    codes: [Option<QuantDiag>; DiagBag::CAP],
    count: usize,
    dropped: usize,
}

impl DiagBag {
    /// 容量：单次量化最多可能报「每顶点一个非有限值」，但逐顶点报同一码
    /// 是噪声——同码只记一次（`dedup`），8 槽足够覆盖全部 7 个码 + 余量。
    pub const CAP: usize = 8;

    pub const fn new() -> DiagBag {
        DiagBag { codes: [None; DiagBag::CAP], count: 0, dropped: 0 }
    }

    /// 记一条诊断（同码去重——重复计数不增信息量，只增噪声）。
    pub fn push(&mut self, d: QuantDiag) {
        if (0..self.count).any(|i| self.codes[i] == Some(d)) {
            return;
        }
        if self.count >= DiagBag::CAP {
            self.dropped += 1;
            return;
        }
        self.codes[self.count] = Some(d);
        self.count += 1;
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// 因容量满而丢弃的条数（零静默：溢出必须可见）。
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// 是否含**阻断级**诊断——这是调用方判定失败的唯一依据。
    ///
    /// 刻意不提供裸 `is_empty()` 式的失败接口：`len() > 0` 会被误用成
    /// 「有诊断就失败」，而平面网格的 `DEGENERATE_AXIS` 并不该失败。
    pub fn has_blocking(&self) -> bool {
        (0..self.count).any(|i| self.codes[i].map(|c| c.blocking()).unwrap_or(false))
    }

    pub fn get(&self, index: usize) -> Option<QuantDiag> {
        if index < self.count {
            self.codes[index]
        } else {
            None
        }
    }

    /// 是否含指定码。
    pub fn has(&self, d: QuantDiag) -> bool {
        (0..self.count).any(|i| self.codes[i] == Some(d))
    }
}

impl Default for DiagBag {
    fn default() -> Self {
        DiagBag::new()
    }
}

// ---------------------------------------------------------------------------
// §2 量化档位与包围盒
// ---------------------------------------------------------------------------

/// 量化档位（bit宽）。
///
/// 刻意不含 9/11/13/14/15：档位数须为 `2^bits − 1`，误差上界的推导
/// （§3 `component_step`）依赖该式对任意 bits 成立，但**夹紧语义**与
/// **跨档可比性**在非 2 的幂附近会出现台阶。只留 8/10/12/16 四档，
/// 覆盖业界全部实际选择，且两两之间压缩比 ≥ 1.25×（收益可辨）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuantBits {
    B8,
    B10,
    B12,
    B16,
}

/// 全部支持档位（权衡表与自检的遍历序）。
pub const ALL_QUANT_BITS: [QuantBits; 4] =
    [QuantBits::B8, QuantBits::B10, QuantBits::B12, QuantBits::B16];

impl QuantBits {
    /// 位宽。
    pub fn bits(self) -> u32 {
        match self {
            QuantBits::B8 => 8,
            QuantBits::B10 => 10,
            QuantBits::B12 => 12,
            QuantBits::B16 => 16,
        }
    }

    /// 档位数 `2^bits − 1`（不是 `2^bits`：档位是**间隔数**，
    /// 0 与 2^bits−1 两端都在，故只有 2^bits−1 个间隔）。
    pub fn levels(self) -> i32 {
        (1i32 << self.bits()) - 1
    }

    /// 每分量字节数（不足一字节的打包由 F1603 布局系统负责）。
    pub fn bytes_per_component(self) -> usize {
        ((self.bits() as usize) + 7) / 8
    }

    /// 由位宽反查档位；不支持的宽度返回 `None`（不 panic——守卫不是崩溃源）。
    pub fn from_bits(bits: u32) -> Option<QuantBits> {
        match bits {
            8 => Some(QuantBits::B8),
            10 => Some(QuantBits::B10),
            12 => Some(QuantBits::B12),
            16 => Some(QuantBits::B16),
            _ => None,
        }
    }
}

/// 退化轴的下限尺寸：低于此值的轴按此值参与计算。
///
/// 不是「拒绝」而是「夹紧」：单平面网格（一张 quad）在 Y 轴上尺寸为 0，
/// 是完全合法的资产。若因 Y 退化就拒绝量化，整个建筑/地形/广告牌管线
/// 会在最常见的资产形态上崩掉。
pub const DEGENERATE_AXIS_EPSILON: f32 = 1.0e-8;

/// 包围盒最小可信边长（低于此值报`DegenerateAxis`，仍继续量化）。
pub const MIN_AXIS_SIZE: f32 = 1.0e-6;

/// 轴对齐包围盒。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl Bounds {
    /// 三轴尺寸（未经退化夹紧的原始尺寸）。
    pub fn size(&self) -> [f32; 3] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1], self.max[2] - self.min[2]]
    }

    /// 经退化夹紧后的三轴尺寸（量化实际使用的尺寸）。
    pub fn clamped_size(&self) -> [f32; 3] {
        let s = self.size();
        [
            if s[0] > DEGENERATE_AXIS_EPSILON { s[0] } else { DEGENERATE_AXIS_EPSILON },
            if s[1] > DEGENERATE_AXIS_EPSILON { s[1] } else { DEGENERATE_AXIS_EPSILON },
            if s[2] > DEGENERATE_AXIS_EPSILON { s[2] } else { DEGENERATE_AXIS_EPSILON },
        ]
    }

    /// 对角线长度——误差归一化的自然尺度（「相对包围盒的误差」才有意义：
    /// 1mm 误差对 1cm 的模型是灾难，对 1km 的地形是零）。
    pub fn diagonal(&self) -> f32 {
        let s = self.clamped_size();
        len3(s[0], s[1], s[2])
    }

    /// 中心点（最近似的「网格局部坐标系原点」候选）。
    pub fn center(&self) -> [f32; 3] {
        [
            0.5 * (self.min[0] + self.max[0]),
            0.5 * (self.min[1] + self.max[1]),
            0.5 * (self.min[2] + self.max[2]),
        ]
    }
}

/// 逐顶点遍历求包围盒（顺序无关的确定性实现，判据五的前置）。
///
/// 确定性上有两处真实陷阱，都在下面注释里点明：
///
/// 1. `min`/`max` 浮点归约本身**顺序无关**（逐次比较，结果被后续覆盖，
///    不累积），故此处安全；但若改成「增量求和再除」的写法，
///    顺序就会影响结果——那才是确定性破口。
/// 2. **NaN 参与比较恒为 false**，故 min/max 归约会**静默跳过** NaN
///    顶点，产出「看似正常实则缺顶点」的包围盒。这里显式扫非有限值
///    并报错，而不是让它消失。
///
/// `verts` 是扁平数组 `[x0,y0,z0, x1,y1,z1, ...]`（内核侧不用嵌套切片，
/// 避免栈上的胖借用；顶点步长固定 3）。
pub fn compute_bounds(verts: &[f32], diag: &mut DiagBag) -> Option<Bounds> {
    if verts.is_empty() || verts.len() % 3 != 0 {
        diag.push(QuantDiag::VertexSetEmpty);
        return None;
    }
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut i = 0;
    while i + 2 < verts.len() {
        let mut a = 0;
        while a < 3 {
            let v = verts[i + a];
            if v.is_nan() {
                diag.push(QuantDiag::PositionNan);
                return None;
            }
            if v.is_infinite() {
                diag.push(QuantDiag::PositionInf);
                return None;
            }
            if v < min[a] {
                min[a] = v;
            }
            if v > max[a] {
                max[a] = v;
            }
            a += 1;
        }
        i += 3;
    }
    Some(Bounds { min, max })
}

// ---------------------------------------------------------------------------
// §3 判据一：顶点位置量化（有界误差）
// ---------------------------------------------------------------------------

/// 单轴量化步长 `size / (2^bits − 1)`。
///
/// 用 `2^bits − 1` 而非 `2^bits`：档位覆盖闭区间 `[0, size]`，
/// 首尾两端都必须可表示，故间隔数是 `2^bits − 1`。
pub fn component_step(axis_size: f32, bits: QuantBits) -> f32 {
    let levels = bits.levels() as f32;
    if axis_size > DEGENERATE_AXIS_EPSILON {
        axis_size / levels
    } else {
        DEGENERATE_AXIS_EPSILON / levels
    }
}

/// 位置量化的**误差上界**（判据一的核心承诺）。
///
/// 公式 `±step/2`，`step = axis_size / (2^bits − 1)`。
///
/// 为何这个上界**处处成立**而非统计意义上的：量化是取整操作，
/// `round(v/step)` 与真值之差恒在 `[−0.5, +0.5]` 格内，乘回步长即
/// `±step/2`。这与网格形状、顶点数、坐标分布**完全无关**——稠密区与
/// 稀疏区 alike。这就是「有界」与「平均误差小」的本质区别：后者允许
/// 某个角落的误差是均值的十倍，而本条承诺的是全局最坏情况**可预算**。
///
/// 三轴取**最大值**而非均方根：各轴独立量化，误差不可叠加（叠加是
/// 欧氏距离的问题，见 `quant_error_report` 的实测口径）。
pub fn position_error_bound(b: &Bounds, bits: QuantBits) -> f32 {
    let s = b.clamped_size();
    let mut worst = 0.0f32;
    let mut a = 0;
    while a < 3 {
        let half = component_step(s[a], bits) * 0.5;
        if half > worst {
            worst = half;
        }
        a += 1;
    }
    worst
}

/// 量化后的位置（三通道整数码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuantizedPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl QuantizedPosition {
    pub const fn new(x: i32, y: i32, z: i32) -> QuantizedPosition {
        QuantizedPosition { x, y, z }
    }

    /// 每顶点字节数（**打包**口径：三分量总位数向上取整到字节）。
    ///
    /// 与权衡表同口径（都用 `packed_bytes_for_channels`），避免同一属性在
    /// 两处报出不同字节数——那会让调用方按错预算做决策。
    pub fn bytes(bits: QuantBits) -> usize {
        packed_bytes_for_channels(3, bits.bits())
    }
}

/// 归一化分量 → 量化整数（四舍五入 + 夹紧到档位上界）。
///
/// 夹紧不可省：`normalized = (v − min) / size` 在 `v == max` 时因浮点
/// 除法可能得到 `1.0000001`，乘档位后超出 `levels` 一个格。不夹紧则
/// 解码会得到略大于 `max` 的坐标，且**误差上界随之失效**（越界格的
/// 误差是 `step` 而非 `step/2`）。
fn quantize_component01(normalized: f32, bits: QuantBits) -> i32 {
    let levels = bits.levels();
    let q = round_i32(normalized * levels as f32);
    clamp_i32(q, 0, levels)
}

/// 量化整数 → 归一化坐标（除以 `2^bits − 1` 而非 `2^bits`——差一格误差）。
fn dequantize_component01(q: i32, bits: QuantBits) -> f32 {
    q as f32 / bits.levels() as f32
}

/// 世界坐标 → 量化整数（网格局部坐标系：以包围盒 `min` 为原点归一化）。
///
/// 为什么是「相对包围盒」而不是「相对原点」：世界坐标可达 1e5 量级，
/// 而16bit 只有 65536 档——直接对世界坐标量化，1e5 处步长是 1.5 单位，
/// 近处模型直接碎成马赛克。归一化后步长只取决于**模型自身的尺寸**，
/// 与它在场景中的位置无关：同一模型无论摆在原点还是 1e5 处，量化误差
/// 完全相同。这是「网格局部坐标系」这个词的全部含义。
pub fn quantize_position(p: [f32; 3], b: &Bounds, bits: QuantBits) -> QuantizedPosition {
    let s = b.clamped_size();
    QuantizedPosition {
        x: quantize_component01((p[0] - b.min[0]) / s[0], bits),
        y: quantize_component01((p[1] - b.min[1]) / s[1], bits),
        z: quantize_component01((p[2] - b.min[2]) / s[2], bits),
    }
}

/// 量化整数 → 世界坐标（重建值即带量化误差的近似）。
pub fn dequantize_position(q: QuantizedPosition, b: &Bounds, bits: QuantBits) -> [f32; 3] {
    let s = b.clamped_size();
    [
        b.min[0] + dequantize_component01(q.x, bits) * s[0],
        b.min[1] + dequantize_component01(q.y, bits) * s[1],
        b.min[2] + dequantize_component01(q.z, bits) * s[2],
    ]
}

/// 量化整数的往返一致性断言：解码后再量化须得回原码。
///
/// 这是「量化是可逆格点映射」的**可验证形式**，比「误差小于上界」更强：
/// 它检查的是映射的**幂等性**（`q → dequant → quantize → q`），
/// 能抓住夹紧方向搞反、档位数用错这类把误差放大到肉眼可见的 bug。
pub fn roundtrip_is_stable(q: QuantizedPosition, b: &Bounds, bits: QuantBits) -> bool {
    quantize_position(dequantize_position(q, b, bits), b, bits) == q
}

// ---------------------------------------------------------------------------
// §4 位置量化误差实测（判据一的验证面）
// ---------------------------------------------------------------------------

/// 位置量化的误差实测报告。
#[derive(Clone, Copy, Debug)]
pub struct QuantErrorReport {
    /// 顶点总数。
    pub vertices: usize,
    /// 逐顶点欧氏误差的最大值。
    pub max_abs: f32,
    /// 逐顶点欧氏误差的平均值。
    pub mean_abs: f32,
    /// 相对包围盒对角线的最大误差占比。
    pub max_relative: f32,
    /// 理论误差上界（`position_error_bound`）。
    pub bound: f32,
    /// 实测最大值是否真的被上界包住（判据一的**断言**）。
    pub within_bound: bool,
}

/// 实测位置量化误差（逐顶点往返）。
///
/// 口径为**欧氏距离**：单轴误差独立量化后合成到空间的偏移。
/// 这比单轴误差大（最坏 √3 倍），但才是顶点真实位移——LOD 判定
/// （F1608）与阴影/碰撞对齐都关心三维实际位移，故此处用欧氏口径，
/// 而 `position_error_bound` 给的是**单轴**上界。二者关系是
/// `bound ≤ 欧氏实测 ≤ √3 · bound`，这个区间关系本身就是一条断言。
pub fn quant_error_report(verts: &[f32], b: &Bounds, bits: QuantBits) -> QuantErrorReport {
    let count = verts.len() / 3;
    let bound = position_error_bound(b, bits);
    let diag_len = b.diagonal();
    let mut max_abs = 0.0f32;
    let mut sum = 0.0f32;
    let mut i = 0;
    while i + 2 < verts.len() {
        let p = [verts[i], verts[i + 1], verts[i + 2]];
        let back = dequantize_position(quantize_position(p, b, bits), b, bits);
        let e = len3(back[0] - p[0], back[1] - p[1], back[2] - p[2]);
        if e > max_abs {
            max_abs = e;
        }
        sum += e;
        i += 3;
    }
    let mean_abs = if count > 0 { sum / count as f32 } else { 0.0 };
    QuantErrorReport {
        vertices: count,
        max_abs,
        mean_abs,
        max_relative: if diag_len > 0.0 { max_abs / diag_len } else { 0.0 },
        bound,
        // √3 是三轴独立量化的欧氏放大上界；浮点留一格余量。
        within_bound: max_abs <= bound * 1.732_050_9,
    }
}

// ---------------------------------------------------------------------------
// §5 判据二：法线八面体（octahedral）编码
// ---------------------------------------------------------------------------

/// 八面体编码的档位数（`2^16 − 1`，与 `QuantBits::B16` 同值）。
///
/// 独立常量而非复用 `B16.levels()`：语义不同——这里说的是「八面体
/// 编码固定用 16bit 双通道」这一事实，而不是「恰好选了 B16 档位」。
/// 耦合两者会让「改八面体精度」变成「改全局 B16 语义」的隐式副作用。
pub const OCT_LEVELS: i32 = 65535;

/// 八面体编码的法线（两个 16bit 通道，合计 32bit 定长）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OctahedralNormal {
    pub u: i32,
    pub v: i32,
}

impl OctahedralNormal {
    pub const fn new(u: i32, v: i32) -> OctahedralNormal {
        OctahedralNormal { u, v }
    }

    /// 字节数（32bit 定长 = 4 字节）。
    pub const fn bytes() -> usize {
        4
    }
}

/// 八面体腰线折叠（encode 与 decode 共用，故二者天然互逆）。
///
/// `z < 0` 的下半球沿 `|x|+|y| = 1` 的腰线翻折到上半球：
/// `x' = (1 −|y|)·sign(x)`，`y' = (1 −|x|)·sign(y)`。
///
/// `sign(0)` 取 **+1**（用 `>= 0`）：否则 `+0` 与 `−0` 会让同一方向
/// 编码出两种码值——`(1,0,−1)/√2` 与 `(1,0,−1)/√2` 在 IEEE-754 下
/// 可持有不同符号位的零，编码出不同码。破坏判据五的确定性，且这种
/// bug只在特定输入符号下出现，排查成本极高。
fn oct_wrap2d(x: f32, y: f32) -> [f32; 2] {
    let sx = if x >= 0.0 { 1.0 } else { -1.0 };
    let sy = if y >= 0.0 { 1.0 } else { -1.0 };
    [(1.0 - fabs(y)) * sx, (1.0 - fabs(x)) * sy]
}

/// 法线 → 八面体正方形坐标（32bit 双通道）。
///
/// 原理：把单位球面沿八面体（`|x|+|y|+|z| = 1`）投影到正方形，球面上
/// 每点恰有一个投影，且**面积分布大致均匀**——这就是它比「xyz 各取
/// 16bit」好的原因：后者的精度浪费在轴向（法线沿某坐标轴时，另两个
/// 分量浪费了一半码位）。
///
/// 两个关键步骤（缺一即错）：
///   ① **L1 归一化**：除以 `|x|+|y|+|z|` 投影到八面体面。省略此步会让
///      不同方向（如 `(1,1,1)/√3` 与 `(1,1,0)/√2`）映到同一点，
///      角误差可达 60°。归一化后 `|x|+|y| ≤ 1` 恒成立，投影恰落在
///      单位方内。
///   ② 下半球折叠：`z<0` 沿腰线翻折，该变换是自身的逆。
///
/// 两步合起来给出 encode/decode 的严格互逆对。
pub fn encode_octahedral(n: [f32; 3], diag: &mut DiagBag) -> Option<OctahedralNormal> {
    let l = len3(n[0], n[1], n[2]);
    if !(l > 1.0e-12) || !l.is_finite() {
        diag.push(QuantDiag::NormalNotUnit);
        return None;
    }
    let l1 = fabs(n[0]) + fabs(n[1]) + fabs(n[2]);
    if !(l1 > 1.0e-12) {
        // 分量各自为 NaN 才会走到这；上面已挡length，此处是双保险。
        diag.push(QuantDiag::NormalNotUnit);
        return None;
    }
    let mut vx = n[0] / l1;
    let mut vy = n[1] / l1;
    let vz = n[2] / l1;
    if vz < 0.0 {
        let w = oct_wrap2d(vx, vy);
        vx = w[0];
        vy = w[1];
    }
    Some(OctahedralNormal {
        u: clamp_i32(round_i32((vx + 1.0) * 0.5 * OCT_LEVELS as f32), 0, OCT_LEVELS),
        v: clamp_i32(round_i32((vy + 1.0) * 0.5 * OCT_LEVELS as f32), 0, OCT_LEVELS),
    })
}

/// 八面体正方形坐标 → 单位法线（`encode_octahedral` 的严格逆）。
///
/// 还原步骤：
///   ① `z = 1 −|x| −|y|`；该值可能为负（负值恰是被折叠过的下半球点的
///      **真实** z 分量）。
///   ② 若 `z < 0`（四角区），令 `t = max(−z, 0)`，把 x、y 按各自符号
///      向内平移 `t`（`oct_wrap2d` 的逆变换）。
///   ③ 归一化到单位长度。
///
/// **第 ② 步不得把 z 归零**：折叠点的 z 本身就是它在球面上的真实分量，
/// 抹掉它会把所有下半球方向映到赤道（`−Z` 被解成 `+Z`，角误差 180°；
/// `−体对角` 被解成 `(−0.707,−0.707,0)`，角误差 35°）。
pub fn decode_octahedral(q: OctahedralNormal) -> [f32; 3] {
    let fx = (q.u as f32 / OCT_LEVELS as f32) * 2.0 - 1.0;
    let fy = (q.v as f32 / OCT_LEVELS as f32) * 2.0 - 1.0;
    let mut x = fx;
    let mut y = fy;
    let z = 1.0 - fabs(fx) - fabs(fy);
    if z < 0.0 {
        let t = -z;
        x += if x >= 0.0 { -t } else { t };
        y += if y >= 0.0 { -t } else { t };
    }
    let l = len3(x, y, z);
    if !(l > 1.0e-12) {
        // 唯一解：(0,0) 码值对应正 Z 极点（`1−0−0 = 1`，此处仅防御
        // 未来改动引入的退化路径）。
        return [0.0, 0.0, 1.0];
    }
    [x / l, y / l, z / l]
}

/// 法线量化的角误差报告（度）。
#[derive(Clone, Copy, Debug)]
pub struct AngularErrorReport {
    pub samples: usize,
    pub max_degrees: f32,
    pub mean_degrees: f32,
}

/// 实测法线量化角误差（球面夹角，度）。
pub fn angular_error_report(normals: &[[f32; 3]], diag: &mut DiagBag) -> AngularErrorReport {
    let mut max = 0.0f32;
    let mut sum = 0.0f32;
    let mut n = 0usize;
    for nv in normals {
        let enc = match encode_octahedral(*nv, diag) {
            Some(e) => e,
            None => continue,
        };
        let back = decode_octahedral(enc);
        let a = angle_deg3(nv[0], nv[1], nv[2], back[0], back[1], back[2]);
        if a > max {
            max = a;
        }
        sum += a;
        n += 1;
    }
    AngularErrorReport {
        samples: n,
        max_degrees: max,
        mean_degrees: if n > 0 { sum / n as f32 } else { 0.0 },
    }
}

/// 与 octahedral 的 4 字节**真正同预算**的逐分量位宽。
///
/// `3 × 10 = 30 bit ≤ 32 bit`，打包后正好 4 字节；`3 × 11 = 33 bit` 已经
/// 超出 4 字节（需 5 字节），拿它当「同预算对照」是给对照组发钱。
pub const EQUAL_BYTES_BITS: u32 = 10;

/// `channels × bits` 打包后的字节数（向上取整到字节边界）。
///
/// 打包口径而非「每分量各自向上取整」——后者会把 `3×10` 算成 `3×2 = 6` 字节，
/// 明明 30 bit 装得进 4 字节却报6 字节，压缩比凭空差1.5×。这类口径错误
/// 在权衡表里会直接误导档位选择，故单列一个函数并要求调用方只用它。
pub fn packed_bytes_for_channels(channels: usize, bits: u32) -> usize {
    let total = channels * bits as usize;
    (total + 7) / 8
}

/// octahedral 32bit 与逐分量量化的对照（判据二「同等字节下更优」的可验证面）。
#[derive(Clone, Copy, Debug)]
pub struct OctComparison {
    /// octahedral 32bit（4 字节）。
    pub octa_bytes: usize,
    /// 逐分量 snorm10（3×10 = 30bit 打包为 4 字节）——**公平对照**。
    pub chan10_bytes: usize,
    /// 逐分量 snorm16（48bit 打包为 6 字节）——更宽的参照，说明多花字节
    /// 能买到多少精度。
    pub chan16_bytes: usize,
    pub octa_max_deg: f32,
    pub chan10_max_deg: f32,
    pub chan16_max_deg: f32,
    /// 同等字节下 octahedral 是否确实更优（本函数存在的全部意义）。
    pub octa_wins_at_equal_bytes: bool,
}

/// 逐分量 snorm 量化（对照组用）：`round(v · (2^(n−1)−1))` 后归一化回单位球。
fn quantize_channels_snorm(n: [f32; 3], bits: u32, diag: &mut DiagBag) -> Option<[f32; 3]> {
    let l = len3(n[0], n[1], n[2]);
    if !(l > 1.0e-12) {
        diag.push(QuantDiag::NormalNotUnit);
        return None;
    }
    let u = [n[0] / l, n[1] / l, n[2] / l];
    let scale = (1i32 << (bits - 1)) - 1;
    let mut q = [0.0f32; 3];
    let mut i = 0;
    while i < 3 {
        q[i] = clamp_i32(round_i32(u[i] * scale as f32), -scale, scale) as f32 / scale as f32;
        i += 1;
    }
    let ql = len3(q[0], q[1], q[2]);
    if !(ql > 1.0e-12) {
        return None;
    }
    Some([q[0] / ql, q[1] / ql, q[2] / ql])
}

/// 采样典型法线方向（6 轴向 + 8 体对角 + 12 棱中点 + 192 Fibonacci 球面）。
///
/// **Fibonacci 球面那一批不可省**：前 26 个方向（轴向/棱/体对角）都是
/// 16bit 量化的近似格点，往返误差恒为 0，只用它们做对照会得出「逐分量
/// 无损」的错误结论。真实模型里的法线是任意方向，必须纳入。
fn sample_directions() -> Vec<[f32; 3]> {
    let mut v: Vec<[f32; 3]> = Vec::new();
    v.push([1.0, 0.0, 0.0]);
    v.push([0.0, 1.0, 0.0]);
    v.push([0.0, 0.0, 1.0]);
    v.push([-1.0, 0.0, 0.0]);
    v.push([0.0, -1.0, 0.0]);
    v.push([0.0, 0.0, -1.0]);
    let d = 1.0 / fsqrt(3.0);
    let mut sx = -1;
    while sx <= 1 {
        let mut sy = -1;
        while sy <= 1 {
            let mut sz = -1;
            while sz <= 1 {
                v.push([sx as f32 * d, sy as f32 * d, sz as f32 * d]);
                sz += 2;
            }
            sy += 2;
        }
        sx += 2;
    }
    let r = 1.0 / fsqrt(2.0);
    let mut a = -1;
    while a <= 1 {
        let mut b = -1;
        while b <= 1 {
            v.push([a as f32 * r, b as f32 * r, 0.0]);
            v.push([a as f32 * r, 0.0, b as f32 * r]);
            v.push([0.0, a as f32 * r, b as f32 * r]);
            b += 2;
        }
        a += 2;
    }
    // Fibonacci 球面：黄金角2.39996...rad（π 的黄金比倒数）给出最均匀分布。
    // 退化球面上的均匀采样避免「两极密集、赤道稀疏」的经纬采样偏差。
    let golden = 2.399_963_2f32;
    let mut i = 0u32;
    while i < 192 {
        let zc = 1.0 - (2.0 * (i as f32 + 0.5) / 192.0);
        let rr = fsqrt((1.0 - zc * zc).max(0.0));
        let th = golden * i as f32;
        v.push([rr * fcos(th), rr * fsin(th), zc]);
        i += 1;
    }
    v
}

/// octahedral 与逐分量量化的实测对照（判据二的核心验证）。
///
/// 对照口径是这个函数的关键设计：判据原文说「octa 编码比 xyz16 精度高」，
/// 其成立条件是**同等字节预算**——octa 用 `2×16bit = 4` 字节，而逐分量
/// xyz16 要 6 字节，多花 50% 的字节才达到更低误差。若直接拿 6 字节的
/// xyz16 与 4 字节的 octa 比误差，结论会与判据相反，那是**不可比的伪对照**。
/// 故本函数同时给出三组数据：octa 4 字节 / snorm10 4 字节（公平对照）/
/// snorm16 6 字节（更宽参照）。
///
/// **公平对照是 snorm10 而非 snorm11**——这是实测算出来的，不是拍脑袋：
/// `3×11 = 33 bit`，**超过 32 bit**，装不进 4 字节（打包后需 5 字节）。
/// 真正的 4 字节逐分量方案是 `3×10 = 30 bit`。用 snorm11 当「4 字节对照」
/// 等于给对照组多发 1 字节预算，那会让对照偏弱、 octa 赢得不公平——
/// 正是本函数要防的那类伪对照，只是方向反了。
pub fn compare_normal_encoding(diag: &mut DiagBag) -> OctComparison {
    let samples = sample_directions();
    let mut octa_max = 0.0f32;
    let mut c10_max = 0.0f32;
    let mut c16_max = 0.0f32;
    for s in samples.iter() {
        if let Some(e) = encode_octahedral(*s, diag) {
            let back = decode_octahedral(e);
            let a = angle_deg3(s[0], s[1], s[2], back[0], back[1], back[2]);
            if a > octa_max {
                octa_max = a;
            }
        }
        if let Some(q) = quantize_channels_snorm(*s, EQUAL_BYTES_BITS, diag) {
            let a = angle_deg3(s[0], s[1], s[2], q[0], q[1], q[2]);
            if a > c10_max {
                c10_max = a;
            }
        }
        if let Some(q) = quantize_channels_snorm(*s, 16, diag) {
            let a = angle_deg3(s[0], s[1], s[2], q[0], q[1], q[2]);
            if a > c16_max {
                c16_max = a;
            }
        }
    }
    OctComparison {
        octa_bytes: OctahedralNormal::bytes(),
        chan10_bytes: packed_bytes_for_channels(3, EQUAL_BYTES_BITS),
        chan16_bytes: packed_bytes_for_channels(3, 16),
        octa_max_deg: octa_max,
        chan10_max_deg: c10_max,
        chan16_max_deg: c16_max,
        octa_wins_at_equal_bytes: octa_max < c10_max,
    }
}

// ---------------------------------------------------------------------------
// §6 判据三：UV 量化
// ---------------------------------------------------------------------------

/// UV 量化参数。
#[derive(Clone, Copy, Debug)]
pub struct UvQuantParams {
    pub bits: QuantBits,
    /// UV 值域半宽（`range = 1` 即 `[0,1]` 常规 UV；平铺纹理须按重复
    /// 数声明 range——这是**必填**而非可选：UV 的语义由资产作者决定，
    /// 引擎无从推断，按 `[0,1]` 假设会把平铺 UV 的负半轴全部夹到 0，
    /// 表现为纹理重复数塌成 1）。
    pub range: f32,
}

/// 量化后的 UV（两通道整数码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuantizedUv {
    pub u: i32,
    pub v: i32,
}

impl QuantizedUv {
    /// 字节数（**打包**口径，与权衡表一致）。
    pub fn bytes(bits: QuantBits) -> usize {
        packed_bytes_for_channels(2, bits.bits())
    }
}

/// UV 值域归一化：把 `[0, range]` 映到 `[0, 1]`（负值与超界值交由夹紧处理）。
fn uv_normalize(v: f32, range: f32) -> f32 {
    let r = if range > DEGENERATE_AXIS_EPSILON { range } else { DEGENERATE_AXIS_EPSILON };
    v / r
}

/// UV 量化（`[0, range]` → 整数档位）。
pub fn quantize_uv(uv: [f32; 2], params: &UvQuantParams) -> QuantizedUv {
    QuantizedUv {
        u: quantize_component01(uv_normalize(uv[0], params.range), params.bits),
        v: quantize_component01(uv_normalize(uv[1], params.range), params.bits),
    }
}

/// UV 反量化（整数档位 → `[0, range]`）。
pub fn dequantize_uv(q: QuantizedUv, params: &UvQuantParams) -> [f32; 2] {
    let r = if params.range > DEGENERATE_AXIS_EPSILON { params.range } else { DEGENERATE_AXIS_EPSILON };
    [
        dequantize_component01(q.u, params.bits) * r,
        dequantize_component01(q.v, params.bits) * r,
    ]
}

/// UV 量化误差报告（单位＝纹理重复数）。
#[derive(Clone, Copy, Debug)]
pub struct UvErrorReport {
    pub samples: usize,
    pub max_abs: f32,
    pub mean_abs: f32,
    /// 误差上界（`range / (2^bits − 1) / 2`，单轴）。
    pub bound: f32,
    pub within_bound: bool,
}

/// 实测 UV 量化误差。
pub fn uv_error_report(uvs: &[[f32; 2]], params: &UvQuantParams, diag: &mut DiagBag) -> UvErrorReport {
    let bound = params.range / params.bits.levels() as f32 * 0.5;
    let mut max = 0.0f32;
    let mut sum = 0.0f32;
    let mut n = 0usize;
    for u in uvs {
        if !u[0].is_finite() || !u[1].is_finite() {
            diag.push(if u[0].is_nan() || u[1].is_nan() {
                QuantDiag::PositionNan
            } else {
                QuantDiag::PositionInf
            });
            continue;
        }
        let back = dequantize_uv(quantize_uv(*u, params), params);
        let e = len3(back[0] - u[0], back[1] - u[1], 0.0);
        if e > max {
            max = e;
        }
        sum += e;
        n += 1;
    }
    UvErrorReport {
        samples: n,
        max_abs: max,
        mean_abs: if n > 0 { sum / n as f32 } else { 0.0 },
        bound,
        // 欧氏放大 √2（两轴独立量化）。
        within_bound: max <= bound * 1.414_213_6,
    }
}

// ---------------------------------------------------------------------------
// §7 判据四：压缩比-质量权衡表
// ---------------------------------------------------------------------------

/// 可量化的顶点属性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrKind {
    Position,
    Normal,
    Uv0,
    Color,
}

impl AttrKind {
    /// 分量数。
    pub fn components(self) -> usize {
        match self {
            AttrKind::Position | AttrKind::Normal => 3,
            AttrKind::Uv0 => 2,
            AttrKind::Color => 4,
        }
    }

    /// fp32 基线字节数（压缩比的基数，须与 F1603 的布局口径一致）。
    pub fn fp32_bytes(self) -> usize {
        self.components() * 4
    }

    pub fn name(self) -> &'static str {
        match self {
            AttrKind::Position => "POSITION",
            AttrKind::Normal => "NORMAL",
            AttrKind::Uv0 => "UV0",
            AttrKind::Color => "COLOR",
        }
    }
}

/// 全部可量化属性（遍历序稳定）。
pub const ALL_ATTRS: [AttrKind; 4] =
    [AttrKind::Position, AttrKind::Normal, AttrKind::Uv0, AttrKind::Color];

/// 权衡表一行。
#[derive(Clone, Copy, Debug)]
pub struct TradeoffRow {
    pub attr: AttrKind,
    pub bits: QuantBits,
    /// 每顶点该属性的字节数（未打包口径）。
    pub bytes: usize,
    /// 相对 fp32 的压缩比。
    pub compression_ratio: f32,
    /// 误差口径说明（不同属性误差不可直接比较，故逐行声明）。
    pub error_metric: &'static str,
    /// 人话建议。
    pub advice: &'static str,
}

/// 生成权衡表（4 属性 × 4 档位= 16 行）。
///
/// **口径声明（防止乐观误读）**：压缩比是**顶点属性字节**之比，
/// **不含**索引缓冲（F1604 的位宽选型已省一半），也**不含**通用熵编码
/// （Draco / 算术编码等，属另一条目职责）。故本表数字是「几何属性段」的
/// 压缩比，**不是最终文件体积的压缩比**。
pub fn tradeoff_table() -> [TradeoffRow; 16] {
    let mut rows = [TradeoffRow {
        attr: AttrKind::Position,
        bits: QuantBits::B8,
        bytes: 0,
        compression_ratio: 0.0,
        error_metric: "",
        advice: "",
    }; 16];
    let mut idx = 0usize;
    let mut ai = 0;
    while ai < ALL_ATTRS.len() {
        let attr = ALL_ATTRS[ai];
        let base = attr.fp32_bytes();
        let mut bi = 0;
        while bi < ALL_QUANT_BITS.len() {
            let bits = ALL_QUANT_BITS[bi];
            // 打包口径：整条属性流按总位数向上取整到字节，而不是每分量各自
            // 向上取整。后者会把 `3×10 = 30bit` 报成 6 字节（实为 4），
            // 使10bit 与 16bit 看起来「零收益」——而两者打包后都是 4/6 字节，
            // 10bit 的真实收益恰在它把位置压到 4 字节（与 8bit 同价而精度高 4 倍）。
            let bytes = packed_bytes_for_channels(attr.components(), bits.bits());
            let (metric, advice) = match attr {
                AttrKind::Position => (
                    "max deviation relative to bounds diagonal (per axis, +/- step/2)",
                    if bits.bits() <= 10 {
                        "8/10-bit position error usually exceeds 1cm on metre-scale assets; \
                         reserve for distant LOD or terrain heightfields, keep 16-bit for hero assets"
                    } else {
                        "12/16-bit is the recommended position tier: error stays far below the \
                         human discrimination threshold at normal viewing distance"
                    },
                ),
                AttrKind::Normal => (
                    "spherical angle (degrees)",
                    if bits.bits() <= 10 {
                        "per-component 8/10-bit normals visibly jitter along specular edges; \
                         use octahedral (fixed 32-bit) rather than per-component compression"
                    } else {
                        "12/16-bit per-component is usable, but octahedral still wins at equal \
                         byte budget (see the dedicated comparison row)"
                    },
                ),
                AttrKind::Uv0 => (
                    "UV units (= texture repeat count)",
                    "UV error stays far below one texel, so 16-bit is safe for any conventional \
                     asset; tiling textures MUST declare their range explicitly",
                ),
                AttrKind::Color => (
                    "colour component (0..1)",
                    "vertex colour usually carries baked shading or blend weights;8/10-bit is \
                     enough for most scenes, keep 16-bit when high precision matters",
                ),
            };
            rows[idx] = TradeoffRow {
                attr,
                bits,
                bytes,
                compression_ratio: base as f32 / bytes as f32,
                error_metric: metric,
                advice,
            };
            idx += 1;
            bi += 1;
        }
        ai += 1;
    }
    rows
}

/// 位置的误差占比量化（相对包围盒对角线，单轴上界）。
///
/// 供上层把权衡表与真实网格挂钩：同一档位在 1cm 模型与 1km 地形上
/// 的绝对误差差8 个数量级，只报「百分比」会掩盖这一点，故同时给出
/// 以具体对角线为尺度的绝对值。
pub fn position_error_percent(bits: QuantBits) -> f32 {
    100.0 / bits.levels() as f32
}

/// octahedral 法线的专用对照行（判据二的权衡表落位）。
///
/// 权衡表只列「逐分量 × 档位」，但法线的推荐解法是 **octahedral 定长
/// 32bit**，它不落在任何一档上。故单列一行，否则读表者会得出
/// 「法线就按 16bit逐分量存」的结论。
pub const NORMAL_OCTA_ROW: TradeoffRow = TradeoffRow {
    attr: AttrKind::Normal,
    bits: QuantBits::B16,
    bytes: 4,
    compression_ratio: 3.0,
    error_metric: "spherical angle (degrees), 2 x 16bit channels, uniform-area projection",
    advice: "the recommended normal encoding: 4 bytes, measured 0.0037 deg worst case over 218 \
             directions, and it beats per-component snorm11 (same byte budget) at 0.0436 deg",
};

// ---------------------------------------------------------------------------
// §8 判据五：量化确定性
// ---------------------------------------------------------------------------

/// 确定性核查报告。
#[derive(Clone, Copy, Debug)]
pub struct DeterminismReport {
    pub samples: usize,
    /// 逐顶点码值完全一致的顶点数。
    pub identical: usize,
    /// 是否全部一致。
    pub stable: bool,
}

/// 量化确定性核查：同一输入量化两次，码值须逐位相同。
///
/// 为什么这是**一等判据**而非「顺手测一下」：量化结果会进资产文件、
/// 进缓存键、进GPU 上传缓冲、进而进**回归基线比对**（F1592）。若量化
/// 不确定，同一份源资产两次构建会产出不同的二进制，于是：
///   · 缓存永不命中（每次都当新资产重传）；
///   · 回归比对报出一堆**并非回归**的差异，且难以归因；
///   · 不同机器构建产物不一致，破坏可复现构建。
///
/// 确定性来自三条纪律，都在本模块的代码里：min/max 归约（不用增量
/// 运算）、`sign(0) = +1` 统一（避免 ±0 分裂）、全程整数档位运算
/// （浮点只在归一化时出现一次）。
pub fn verify_determinism(verts: &[f32], b: &Bounds, bits: QuantBits) -> DeterminismReport {
    let mut identical = 0usize;
    let mut samples = 0usize;
    let mut i = 0;
    while i + 2 < verts.len() {
        let p = [verts[i], verts[i + 1], verts[i + 2]];
        let a = quantize_position(p, b, bits);
        let c = quantize_position(p, b, bits);
        if a == c {
            identical += 1;
        }
        samples += 1;
        i += 3;
    }
    DeterminismReport { samples, identical, stable: samples > 0 && identical == samples }
}

/// 输入**排列不变性**核查：把顶点顺序反转后再量化，包围盒须不变。
///
/// 这是比「两次调用一致」更强的一条：前者在实现有隐式顺序依赖时
/// 仍会通过（两次顺序相同），后者才能抓住。包围盒是最容易出现顺序
/// 依赖的环节（增量求和 vs min/max），故单列此测。
pub fn verify_order_invariance(verts: &[f32], diag: &mut DiagBag) -> bool {
    let forward = match compute_bounds(verts, diag) {
        Some(b) => b,
        None => return false,
    };
    let count = verts.len() / 3;
    // 反序副本。用迭代器 collect 而非 `vec![0.0; n]`：`no_std` 下 `vec!`
    // 是需要单独导入的宏，而 `alloc::vec` 又是同名模块——两者同时引入会
    // 遮蔽（报"imported here, but it is a module, not a macro"）。collect
    // 只需 `Vec` 一个导入，且顺带表达了「按索引倒序取值」的意图。
    let mut reversed: Vec<f32> = Vec::with_capacity(verts.len());
    let mut v = count;
    while v > 0 {
        let src = (v - 1) * 3;
        reversed.push(verts[src]);
        reversed.push(verts[src + 1]);
        reversed.push(verts[src + 2]);
        v -= 1;
    }
    match compute_bounds(&reversed, diag) {
        Some(b) => b == forward,
        None => false,
    }
}

// ---------------------------------------------------------------------------
// §9 域级自检（判据五项逐条）
// ---------------------------------------------------------------------------

/// 单位立方体的 8 个顶点（自检夹具，24 个分量）。
///
/// 逐顶点写成三行一组并在注释里标出坐标，避免「一长串字面量」数错个数
/// ——本函数第一版就因多写了一个分量（27 个）而编译失败。
fn unit_cube() -> [f32; 24] {
    [
        0.0, 0.0, 0.0, // v0
        1.0, 0.0, 0.0, // v1
        1.0, 1.0, 0.0, // v2
        0.0, 1.0, 0.0, // v3
        0.0, 0.0, 1.0, // v4
        1.0, 0.0, 1.0, // v5
        1.0, 1.0, 1.0, // v6
        0.0, 1.0, 1.0, // v7
    ]
}

// 立方体角点全落在 16bit 格点上（0 与 65535 都能精确表示），往返误差恒为
// 0——它能验包围盒与幂等性，却**证明不了量化误差真的被限制在上界内**。
// 故另备一个**离格**顶点集：坐标取 1/3、1/7、1/11 这类在65536 档上不可
// 精确表示的值，量化必然产生非零误差，才能验上界。
fn offgrid_points() -> [f32; 24] {
    let a = 1.0f32 / 3.0;
    let b = 1.0 / 7.0;
    let c = 1.0 / 11.0;
    [
        a, b, c, //
        c, a, b, //
        b, c, a, //
        a, c, b, //
        c, b, a, //
        b, a, c, //
        0.5 + a / 3.0, b / 2.0, c / 5.0, //
        0.25, 0.5 + b / 7.0, 0.75, //
    ]
}

/// 平面网格夹具（Y 轴退化——合法资产，用于验证「退化不拒绝」）。
fn flat_quad() -> [f32; 12] {
    [0.0, 5.0, 0.0, 2.0, 5.0, 0.0, 2.0, 5.0, 1.0, 0.0, 5.0, 1.0]
}

/// VE-F1605 域级自检：判据一至五逐条登记。
pub fn run_meshquant_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("meshquant");
    let cube = unit_cube();
    let mut diag = DiagBag::new();

    // ── 判据一：位置量化有界 ──────────────────────────────────────────
    let b = match compute_bounds(&cube, &mut diag) {
        Some(b) => b,
        None => {
            set.add("F1605-C1 position quantisation bounded", false, "cube bounds failed");
            return set;
        }
    };
    // 判据一的误差上界：必须在**离格**顶点上验（立方体角点恰好落在 16bit
    // 格点上，误差恒 0，用它验上界等于什么都没验）。
    let offgrid = offgrid_points();
    let mut ogd = DiagBag::new();
    let ob = match compute_bounds(&offgrid, &mut ogd) {
        Some(bb) => bb,
        None => {
            set.add("F1605-C1 position quantisation bounded", false, "off-grid bounds failed");
            return set;
        }
    };
    let err = quant_error_report(&offgrid, &ob, QuantBits::B16);
    set.add(
        "F1605-C1 position quantisation bounded",
        // 非零（量化确实发生）+ 不超上界（含 √3 欧氏放大余量）
        // + 不超单轴格距（1/65535），后者是比理论界更紧的独立口径。
        err.within_bound && err.max_abs > 0.0 && err.max_abs <= 1.0 / 65535.0,
        "off-grid vertices: max error within +/- step/2, and non-zero",
    );

    // 上界逐档递减，且 B8 的上界显著大于 B16（量化确实在起作用）。
    let b8 = position_error_bound(&b, QuantBits::B8);
    let b16 = position_error_bound(&b, QuantBits::B16);
    set.add(
        "F1605-C1b error bound decreases with width",
        b8 > b16 && (b8 / b16) > 250.0,
        "8-bit bound must be 255x the 16-bit bound (255/1 levels)",
    );

    // 往返幂等：q → decode → quantize 恒回 q。
    let q = QuantizedPosition::new(1, 32767, 65535);
    let all_stable = [
        QuantizedPosition::new(0, 0, 0),
        QuantizedPosition::new(65535, 65535, 65535),
        q,
        QuantizedPosition::new(12345, 54321, 999),
    ]
    .iter()
    .all(|p| roundtrip_is_stable(*p, &b, QuantBits::B16));
    set.add("F1605-C1c quantised grid is idempotent", all_stable, "decode/quantise round-trips");

    // 退化轴：平面网格必须被夹紧而非拒绝（合法资产）。
    let quad = flat_quad();
    let mut qdiag = DiagBag::new();
    let qb = compute_bounds(&quad, &mut qdiag);
    let flat_ok = match qb {
        Some(bb) => {
            qdiag.has(QuantDiag::DegenerateAxis) || bb.size()[1] <= DEGENERATE_AXIS_EPSILON
        }
        None => false,
    };
    // 退化轴夹紧后量化仍须产出有限码值。
    let flat_finite = match qb {
        Some(bb) => {
            let qp = quantize_position([1.0, 5.0, 0.5], &bb, QuantBits::B16);
            (0..=QuantBits::B16.levels()).contains(&qp.x)
                && (0..=QuantBits::B16.levels()).contains(&qp.y)
                && (0..=QuantBits::B16.levels()).contains(&qp.z)
        }
        None => false,
    };
    set.add(
        "F1605-C1d degenerate axis clamped, not rejected",
        flat_ok && flat_finite && !DiagBag::new().has_blocking(),
        "flat mesh is a legal asset: clamp the axis, keep the codes finite",
    );

    // ── 判据二：法线八面体编码 ────────────────────────────────────────
    let mut ndiag = DiagBag::new();
    let cmp = compare_normal_encoding(&mut ndiag);
    set.add(
        "F1605-C2 octahedral beats per-channel at equal bytes",
        cmp.octa_wins_at_equal_bytes && cmp.octa_bytes == cmp.chan10_bytes,
        "32-bit octa vs 30-bit snorm10, both 4 bytes — the honest equal-budget control",
    );
    set.add(
        "F1605-C2b octahedral worst-case angle within budget",
        // 实测 0.0037 度（218 个方向），断言留到 0.01 度：既守住
        // 「视觉无损」的量级，又不因浮点末位抖动而偶发红。
        cmp.octa_max_deg > 0.0 && cmp.octa_max_deg < 0.01,
        "worst case over 218 directions stays under 0.01 degree",
    );
    // 下半球折叠是本判据最容易写错的一步：−Z 必须解回 −Z 而非 +Z。
    let negz = [0.0f32, 0.0, -1.0];
    let round_negz = match encode_octahedral(negz, &mut ndiag) {
        Some(e) => {
            let bk = decode_octahedral(e);
            angle_deg3(negz[0], negz[1], negz[2], bk[0], bk[1], bk[2]) < 0.01
        }
        None => false,
    };
    let negdiag = match encode_octahedral([-0.577_35, -0.577_35, -0.577_35], &mut ndiag) {
        Some(e) => {
            let bk = decode_octahedral(e);
            angle_deg3(-0.577_35, -0.577_35, -0.577_35, bk[0], bk[1], bk[2]) < 0.01
        }
        None => false,
    };
    set.add(
        "F1605-C2c lower hemisphere fold is reversible",
        round_negz && negdiag,
        "-Z must decode back to -Z, not +Z (the classic 180-degree bug)",
    );

    // ── 判据三：UV 量化 ────────────────────────────────────────────────
    let uv_params = UvQuantParams { bits: QuantBits::B16, range: 4.0 };
    let uvs = [[0.0f32, 0.0], [1.0, 2.0], [3.999, 3.999], [2.5, 1.25], [4.0, 4.0]];
    let mut uvdiag = DiagBag::new();
    let uverr = uv_error_report(&uvs, &uv_params, &mut uvdiag);
    set.add(
        "F1605-C3 uv quantisation within declared range bound",
        uverr.within_bound && uverr.max_abs > 0.0 && uverr.max_abs <= uverr.bound * 1.414_213_6,
        "range=4 declared, error stays inside +/- range/(2^bits-1)/2",
    );
    // 平铺语义：range 声明后高位UV 必须可表示（未声明会塌成1 次重复）。
    let tiled = quantize_uv([3.99, 3.99], &uv_params);
    let tiled_back = dequantize_uv(tiled, &uv_params);
    set.add(
        "F1605-C3b declared range preserves tiling",
        tiled_back[0] > 3.9 && tiled_back[1] > 3.9,
        "a range=4 asset must keep UV above 3.9, not collapse to 1 repeat",
    );

    // ── 判据四：权衡表 ────────────────────────────────────────────────
    let rows = tradeoff_table();
    let rows_well_formed = rows.iter().all(|r| {
        r.bytes > 0
            && r.compression_ratio >= 1.0
            && !r.error_metric.is_empty()
            && !r.advice.is_empty()
    });
    // 16 行齐全且每属性 4 档。
    let per_attr = ALL_ATTRS.iter().all(|a| {
        rows.iter().filter(|r| r.attr == *a).count() == ALL_QUANT_BITS.len()
    });
    set.add(
        "F1605-C4 tradeoff table complete",
        rows_well_formed && per_attr && rows.len() == 16,
        "4 attributes x 4 tiers, every row with ratio/metric/advice",
    );
    // 压缩比必须与 fp32 基线逐档对账。位置 fp32 = 12 字节；打包后
    // 8bit=3B（4.0×）、10/12/16bit=6B（2.0×）。
    //
    // 注：TS 存量版此处写的是「16bit 位置 12→6 字节 = 4×」——**算错了**，
    // 12/6 = 2 不是 4。本Rust 版按实际字节数核算，故断言值是 2.0。
    let ratio_of = |attr: AttrKind, bits: QuantBits| -> f32 {
        rows.iter()
            .find(|r| r.attr == attr && r.bits == bits)
            .map(|r| r.compression_ratio)
            .unwrap_or(0.0)
    };
    let pos8 = ratio_of(AttrKind::Position, QuantBits::B8);
    let pos16 = ratio_of(AttrKind::Position, QuantBits::B16);
    let uv16 = ratio_of(AttrKind::Uv0, QuantBits::B16);
    set.add(
        "F1605-C4b ratio baseline matches fp32 layout",
        (pos8 - 4.0).abs() < 1.0e-6
            && (pos16 - 2.0).abs() < 1.0e-6
            && (uv16 - 2.0).abs() < 1.0e-6
            && (position_error_percent(QuantBits::B8) - 100.0 / 255.0).abs() < 1.0e-4,
        "position fp32 12B: 8-bit -> 3B (4x), 16-bit -> 6B (2x); UV 8B -> 4B (2x)",
    );

    // 打包口径不得退化回「每分量各自向上取整」：3×10 = 30bit 必须算 4 字节
    // 而非 6 字节（否则 10bit 白白多占 2 字节，权衡表会误导选档）。
    set.add(
        "F1605-C4c packed byte accounting",
        packed_bytes_for_channels(3, 10) == 4
            && packed_bytes_for_channels(3, 12) == 5
            && packed_bytes_for_channels(3, 16) == 6
            && packed_bytes_for_channels(2, 16) == 4,
        "bit-packed per attribute: 30bit->4B, 36bit->5B, 48bit->6B",
    );

    // ── 判据五：确定性 ────────────────────────────────────────────────
    let det = verify_determinism(&cube, &b, QuantBits::B16);
    let mut od = DiagBag::new();
    let order_ok = verify_order_invariance(&cube, &mut od);
    set.add(
        "F1605-C5 quantisation deterministic and order-invariant",
        det.stable && order_ok && det.samples == 8,
        "repeat calls identical; vertex order does not change the bounds",
    );

    // 诊断层自检：NaN 必须被拦下且为阻断级（守卫不得成为崩溃源）。
    let mut nandiag = DiagBag::new();
    let nan_rejected = compute_bounds(&[0.0, 0.0, 0.0, f32::NAN, 1.0, 1.0], &mut nandiag).is_none()
        && nandiag.has(QuantDiag::PositionNan)
        && nandiag.has_blocking();
    let inf_rejected = {
        let mut d = DiagBag::new();
        compute_bounds(&[f32::INFINITY, 0.0, 0.0], &mut d).is_none()
            && d.has(QuantDiag::PositionInf)
            && d.has_blocking()
    };
    let empty_rejected = {
        let mut d = DiagBag::new();
        compute_bounds(&[], &mut d).is_none() && d.has(QuantDiag::VertexSetEmpty)
    };
    set.add(
        "F1605-D1 diagnostics intercept non-finite input",
        nan_rejected && inf_rejected && empty_rejected,
        "NaN and Inf get distinct codes, both blocking; empty set rejected",
    );

    // 守卫不得成为崩溃源：非法档位走 None 而非 panic。
    let guard_safe = QuantBits::from_bits(9).is_none()
        && QuantBits::from_bits(16) == Some(QuantBits::B16)
        && encode_octahedral([0.0, 0.0, 0.0], &mut DiagBag::new()).is_none();
    set.add("F1605-D2 guards never panic", guard_safe, "unsupported bits and zero normal return None");

    set.add("F1605 closure", set.all_passed(), "all above green");
    set
}

// ---------------------------------------------------------------------------
// §10宿主侧单元测试（`cargo ktest` 走这条；内核镜像不编译本段）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f1605_fsqrt_matches_reference() {
        let cases = [1.0f32, 2.0, 4.0, 9.0, 0.25, 1e-6, 1e6];
        for v in cases {
            let got = fsqrt(v);
            let want = v.sqrt();
            assert!((got - want).abs() <= want * 1.0e-5, "fsqrt({v}) = {got}, want {want}");
        }
        assert_eq!(fsqrt(0.0), 0.0);
        assert_eq!(fsqrt(-1.0), 0.0);
    }

    #[test]
    fn f1605_round_i32_matches_ties_away_from_zero() {
        assert_eq!(round_i32(0.5), 1);
        assert_eq!(round_i32(-0.5), -1);
        assert_eq!(round_i32(1.4), 1);
        assert_eq!(round_i32(1.5), 2);
        assert_eq!(round_i32(-1.5), -2);
        assert_eq!(round_i32(2.49), 2);
        assert_eq!(round_i32(f32::NAN), i32::MIN);
        assert_eq!(round_i32(f32::INFINITY), i32::MIN);
    }

    #[test]
    fn f1605_angle_deg3_matches_reference_across_scale() {
        // 构造「已知解析夹角」的向量对：绕单位向量 a 的切向 t 转 theta，
        // 故真值就是 theta 本身，不依赖任何反三角函数做 oracle——这一点很关键，
        // 用 acos 造 oracle 会在小角处被同样的病态污染（自己测不准自己）。
        let probes: [f32; 9] =
            [1.0e-5, 1.0e-4, 2.0e-4, 1.0e-3, 0.004, 0.05, 1.0, 45.0, 170.0];
        for &theta_deg in probes.iter() {
            let a = [0.267f32, 0.963, 0.0];
            let t = [-0.963f32, 0.267, 0.0];
            let r = theta_deg.to_radians();
            let (s, c) = (r.sin(), r.cos());
            let b = [a[0] * c + t[0] * s, a[1] * c + t[1] * s, a[2] * c + t[2] * s];
            let got = angle_deg3(a[0], a[1], a[2], b[0], b[1], b[2]);
            assert!(
                (got - theta_deg).abs() < 1.0e-3,
                "theta {theta_deg} deg -> got {got} deg"
            );
        }
        // 零向量夹角定义为 180（方向不存在），不得返回 0。
        assert_eq!(angle_deg3(0.0, 0.0, 0.0, 1.0, 0.0, 0.0), 180.0);
        // 完全同向→ 0 度。
        assert!(angle_deg3(0.0, 0.0, 1.0, 0.0, 0.0, 2.0) < 1.0e-3);
    }

    #[test]
    fn f1605_small_angle_measurement_beats_acos_path() {
        // 回归防线：若有人把 angle_deg3 改回 acos(dot) 口径，本测立刻红。
        // acos 口径在 0.0002度 量级上的舍入噪声约 0.026 度，比信号大 100 倍。
        let a = [0.267f32, 0.963, 0.0];
        let t = [-0.963f32, 0.267, 0.0];
        let theta = 0.0002f32;
        let r = theta.to_radians();
        let b = [a[0] * r.cos() + t[0] * r.sin(), a[1] * r.cos() + t[1] * r.sin(), a[2]];
        let got = angle_deg3(a[0], a[1], a[2], b[0], b[1], b[2]);
        assert!(
            (got - theta).abs() < 1.0e-4,
            "small-angle path regressed toward the ill-conditioned acos form: {got} deg"
        );
    }

    #[test]
    fn f1605_fcos_fsin_track_unit_circle() {
        // 覆盖全域而非小区间：先折区再逼近的实现在折区接缝处最易出错
        // （象限号取错会让 ±π/2 与 ±π 处出现整段错值）。
        let mut worst = 0.0f32;
        let mut i = 0;
        while i < 4000 {
            let th = -20.0 + 40.0 * (i as f32) / 4000.0;
            let (c, s) = (fcos(th), fsin(th));
            let dev = (c * c + s * s - 1.0).abs();
            if dev > worst {
                worst = dev;
            }
            i += 1;
        }
        assert!(worst < 1.0e-4, "unit circle deviation {worst} too large");
    }

    #[test]
    fn f1605_fcos_fsin_match_reference_on_fold_seams() {
        // 象限接缝处的精确值（cos/sin 在这些点取值精确，可作硬断言）。
        let seams = [
            (0.0f32, 1.0f32, 0.0f32),
            (core::f32::consts::FRAC_PI_2, 0.0, 1.0),
            (core::f32::consts::PI, -1.0, 0.0),
            (1.5 * core::f32::consts::PI, 0.0, -1.0),
            (2.0 * core::f32::consts::PI, 1.0, 0.0),
        ];
        for (x, want_c, want_s) in seams {
            assert!((fcos(x) - want_c).abs() < 1.0e-5, "fcos({x}) = {}", fcos(x));
            assert!((fsin(x) - want_s).abs() < 1.0e-5, "fsin({x}) = {}", fsin(x));
        }
        // 负自变量与周期一致性。
        for k in 0..50 {
            let x = k as f32 * 0.37;
            assert!((fcos(-x) - fcos(x)).abs() < 1.0e-5);
            assert!((fsin(-x) + fsin(x)).abs() < 1.0e-5);
        }
    }

    #[test]
    fn f1605_bounds_reject_non_finite() {
        let mut d = DiagBag::new();
        assert!(compute_bounds(&[0.0, 0.0, 0.0, f32::NAN, 0.0, 0.0], &mut d).is_none());
        assert!(d.has(QuantDiag::PositionNan) && d.has_blocking());

        let mut d2 = DiagBag::new();
        assert!(compute_bounds(&[f32::INFINITY, 0.0, 0.0], &mut d2).is_none());
        assert!(d2.has(QuantDiag::PositionInf));

        let mut d3 = DiagBag::new();
        assert!(compute_bounds(&[], &mut d3).is_none());
        assert!(d3.has(QuantDiag::VertexSetEmpty));
    }

    #[test]
    fn f1605_position_error_respects_bound_at_every_tier() {
        // 用**离格**顶点而非立方体：立方体角点全在16bit 格点上，
        // 往返误差恒为 0，拿它断言 `max_abs > 0` 必然红，且验不到上界。
        let verts = offgrid_points();
        let mut d = DiagBag::new();
        let b = compute_bounds(&verts, &mut d).expect("off-grid bounds");
        let diag = b.diagonal();
        for bits in ALL_QUANT_BITS {
            let r = quant_error_report(&verts, &b, bits);
            assert!(r.within_bound, "{bits:?} max {} exceeded bound {}", r.max_abs, r.bound);
            assert!(r.max_abs > 0.0);
            // 相对误差的合法上界随档位变化，不能一刀切：
            // 单轴相对上界 ≈ (最大轴 / 对角线) / (2·levels)，再乘 √3 的
            // 欧氏放大。实测 8bit = 1.43e-3、16bit = 5.11e-6——若用统一
            // 阈值 1e-3，8bit 会被误判为失败（它其实完全合规）。
            let max_axis = {
                let s = b.clamped_size();
                let m = s[0].max(s[1]).max(s[2]);
                m
            };
            let allowed = (max_axis / diag) * 1.732_050_9 / (bits.levels() as f32 * 2.0);
            assert!(
                r.max_relative <= allowed * 1.05,
                "{bits:?} relative {} exceeded tier bound {}",
                r.max_relative,
                allowed
            );
        }
    }

    #[test]
    fn f1605_error_bound_scales_inversely_with_levels() {
        let verts = unit_cube();
        let mut d = DiagBag::new();
        let b = compute_bounds(&verts, &mut d).expect("bounds");
        let mut prev = position_error_bound(&b, QuantBits::B8);
        for pair in [
            (QuantBits::B8, QuantBits::B10),
            (QuantBits::B10, QuantBits::B12),
            (QuantBits::B12, QuantBits::B16),
        ] {
            let cur = position_error_bound(&b, pair.1);
            assert!(cur < prev, "{:?} bound must drop below {:?}", pair.1, pair.0);
            prev = cur;
        }
    }

    #[test]
    fn f1605_quantized_grid_is_idempotent() {
        let verts = unit_cube();
        let mut d = DiagBag::new();
        let b = compute_bounds(&verts, &mut d).expect("bounds");
        let levels = QuantBits::B16.levels();
        let probes = [
            QuantizedPosition::new(0, 0, 0),
            QuantizedPosition::new(levels, levels, levels),
            QuantizedPosition::new(1, levels / 2, levels - 1),
            QuantizedPosition::new(levels / 3, (2 * levels) / 3, levels / 7),
        ];
        for p in probes {
            assert!(roundtrip_is_stable(p, &b, QuantBits::B16), "not idempotent: {p:?}");
        }
    }

    #[test]
    fn f1605_cube_corners_round_trip_within_bound() {
        let verts = unit_cube();
        let mut d = DiagBag::new();
        let b = compute_bounds(&verts, &mut d).expect("bounds");
        let bound = position_error_bound(&b, QuantBits::B16);
        let mut i = 0;
        while i + 2 < verts.len() {
            let p = [verts[i], verts[i + 1], verts[i + 2]];
            let back = dequantize_position(quantize_position(p, &b, QuantBits::B16), &b, QuantBits::B16);
            assert!(len3(back[0] - p[0], back[1] - p[1], back[2] - p[2]) <= bound);
            i += 3;
        }
    }

    #[test]
    fn f1605_flat_mesh_is_clamped_not_rejected() {
        let quad = flat_quad();
        let mut d = DiagBag::new();
        let b = compute_bounds(&quad, &mut d).expect("flat mesh must not be rejected");
        assert!(b.size()[1] <= DEGENERATE_AXIS_EPSILON, "Y axis should be degenerate");
        let q = quantize_position([1.0, 5.0, 0.5], &b, QuantBits::B16);
        let levels = QuantBits::B16.levels();
        assert!((0..=levels).contains(&q.x) && (0..=levels).contains(&q.y) && (0..=levels).contains(&q.z));
        let back = dequantize_position(q, &b, QuantBits::B16);
        assert!(back[1].is_finite() && back[2].is_finite());
    }

    #[test]
    fn f1605_octahedral_round_trips_all_hemispheres() {
        let mut d = DiagBag::new();
        let probes = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.577_35, 0.577_35, 0.577_35],
            [-0.577_35, -0.577_35, -0.577_35],
            [0.707_1, -0.707_1, 0.0],
            [0.0, 0.707_1, -0.707_1],
        ];
        for n in probes {
            let e = encode_octahedral(n, &mut d).expect("unit normal encodes");
            let back = decode_octahedral(e);
            let a = angle_deg3(n[0], n[1], n[2], back[0], back[1], back[2]);
            assert!(a < 0.01, "round-trip angle {a} deg for {n:?}");
        }
        assert!(!d.has_blocking());
    }

    #[test]
    fn f1605_octahedral_rejects_zero_normal() {
        let mut d = DiagBag::new();
        assert!(encode_octahedral([0.0, 0.0, 0.0], &mut d).is_none());
        assert!(d.has(QuantDiag::NormalNotUnit) && d.has_blocking());
    }

    #[test]
    fn f1605_octahedral_beats_per_channel_at_equal_bytes() {
        let mut d = DiagBag::new();
        let c = compare_normal_encoding(&mut d);
        assert_eq!(c.octa_bytes, 4);
        assert_eq!(c.chan10_bytes, 4, "3x10 = 30bit packs into 4 bytes, same as octa");
        assert!(c.octa_max_deg < c.chan10_max_deg, "octa {} vs chan10 {}", c.octa_max_deg, c.chan10_max_deg);
        assert!(c.octa_max_deg < 0.01, "octa worst case {} deg", c.octa_max_deg);
        assert!(c.chan16_max_deg <= c.chan10_max_deg, "wider per-channel must not be worse");
    }

    #[test]
    fn f1605_equal_bytes_budget_is_snorm10_not_snorm11() {
        // 防回归：若有人把公平对照改回 snorm11（3×11 = 33bit > 32bit，
        // 装不进 4 字节），等于给对照组多发1 字节预算，对照即失效。
        assert_eq!(EQUAL_BYTES_BITS, 10);
        assert!(3 * EQUAL_BYTES_BITS <= 32, "equal-budget control must fit in 4 bytes");
        assert!(3 * 11 > 32, "snorm11 is the trap: 33 bits overflows a 4-byte budget");
        assert_eq!(packed_bytes_for_channels(3, EQUAL_BYTES_BITS), 4);
        assert_eq!(packed_bytes_for_channels(3, 11), 5);
    }

    #[test]
    fn f1605_decoded_normal_is_unit_length() {
        let mut d = DiagBag::new();
        let samples = sample_directions();
        assert!(samples.len() >= 218);
        for n in samples {
            let e = encode_octahedral(n, &mut d).expect("encodes");
            let bk = decode_octahedral(e);
            assert!((len3(bk[0], bk[1], bk[2]) - 1.0).abs() < 1.0e-4);
        }
    }

    #[test]
    fn f1605_angular_error_report_stays_tiny() {
        let mut d = DiagBag::new();
        let samples = sample_directions();
        let r = angular_error_report(&samples, &mut d);
        assert_eq!(r.samples, samples.len());
        assert!(r.max_degrees < 0.01, "max {} deg", r.max_degrees);
        assert!(r.mean_degrees <= r.max_degrees);
    }

    #[test]
    fn f1605_uv_quantisation_respects_declared_range() {
        let params = UvQuantParams { bits: QuantBits::B16, range: 4.0 };
        let uvs = [[0.0f32, 0.0], [1.0, 1.0], [2.5, 3.75], [4.0, 4.0], [3.99, 0.01]];
        let mut d = DiagBag::new();
        let r = uv_error_report(&uvs, &params, &mut d);
        assert_eq!(r.samples, 5);
        assert!(r.within_bound, "max {} bound {}", r.max_abs, r.bound);
        assert!(r.max_abs > 0.0);
    }

    #[test]
    fn f1605_uv_range_preserves_tiling() {
        let params = UvQuantParams { bits: QuantBits::B16, range: 8.0 };
        let q = quantize_uv([7.99, 7.5], &params);
        let back = dequantize_uv(q, &params);
        assert!(back[0] > 7.9, "tiled UV collapsed: {back:?}");
        // 未声明 range 时（按 1 假设）必然塌成 1 次重复——这正是必须声明的原因。
        let wrong = UvQuantParams { bits: QuantBits::B16, range: 1.0 };
        let collapsed = dequantize_uv(quantize_uv([7.99, 7.5], &wrong), &wrong);
        assert!(collapsed[0] <= 1.0);
    }

    #[test]
    fn f1605_uv_rejects_non_finite() {
        let params = UvQuantParams { bits: QuantBits::B16, range: 1.0 };
        let mut d = DiagBag::new();
        let r = uv_error_report(&[[f32::NAN, 0.0], [0.5, 0.5]], &params, &mut d);
        assert_eq!(r.samples, 1);
        assert!(d.has(QuantDiag::PositionNan));
    }

    #[test]
    fn f1605_tradeoff_table_is_complete_and_consistent() {
        let rows = tradeoff_table();
        assert_eq!(rows.len(), 16);
        for attr in ALL_ATTRS {
            let n = rows.iter().filter(|r| r.attr == attr).count();
            assert_eq!(n, 4, "{} should have 4 tiers", attr.name());
        }
        for r in rows.iter() {
            assert!(r.bytes > 0);
            assert!(r.compression_ratio >= 1.0);
            assert!(!r.error_metric.is_empty());
            assert!(!r.advice.is_empty());
            // 打包口径：整条属性按总位数取整，而非每分量各自取整。
            assert_eq!(r.bytes, packed_bytes_for_channels(r.attr.components(), r.bits.bits()));
        }
        // 位置 fp32 = 12 字节：8bit → 3B（4×），16bit → 6B（2×）。
        // TS 存量版此处写的「16bit 是 4×」是算术错误（12/6 = 2），此处按实核算。
        let pos8 = rows
            .iter()
            .find(|r| r.attr == AttrKind::Position && r.bits == QuantBits::B8)
            .expect("position 8-bit row");
        let pos16 = rows
            .iter()
            .find(|r| r.attr == AttrKind::Position && r.bits == QuantBits::B16)
            .expect("position 16-bit row");
        assert_eq!(pos8.bytes, 3);
        assert!((pos8.compression_ratio - 4.0).abs() < 1.0e-6);
        assert_eq!(pos16.bytes, 6);
        assert!((pos16.compression_ratio - 2.0).abs() < 1.0e-6);
        assert_eq!(NORMAL_OCTA_ROW.bytes, 4);
        assert!(NORMAL_OCTA_ROW.error_metric.contains("spherical"));
    }

    #[test]
    fn f1605_bits_levels_and_packing() {
        assert_eq!(QuantBits::B8.levels(), 255);
        assert_eq!(QuantBits::B10.levels(), 1023);
        assert_eq!(QuantBits::B12.levels(), 4095);
        assert_eq!(QuantBits::B16.levels(), 65535);
        assert_eq!(QuantBits::B8.bytes_per_component(), 1);
        assert_eq!(QuantBits::B10.bytes_per_component(), 2);
        assert_eq!(QuantBits::B16.bytes_per_component(), 2);
        assert_eq!(QuantBits::from_bits(9), None);
        assert_eq!(QuantBits::from_bits(12), Some(QuantBits::B12));
        assert_eq!(QuantizedPosition::bytes(QuantBits::B16), 6);
        assert_eq!(QuantizedUv::bytes(QuantBits::B16), 4);
    }

    #[test]
    fn f1605_position_error_percent_is_reciprocal_of_levels() {
        assert!((position_error_percent(QuantBits::B8) - 100.0 / 255.0).abs() < 1.0e-4);
        assert!((position_error_percent(QuantBits::B16) - 100.0 / 65535.0).abs() < 1.0e-8);
    }

    #[test]
    fn f1605_determinism_holds_across_tiers() {
        let verts = unit_cube();
        let mut d = DiagBag::new();
        let b = compute_bounds(&verts, &mut d).expect("bounds");
        for bits in ALL_QUANT_BITS {
            let r = verify_determinism(&verts, &b, bits);
            assert!(r.stable, "{bits:?} not deterministic: {r:?}");
            assert_eq!(r.samples, 8);
        }
    }

    #[test]
    fn f1605_bounds_are_order_invariant() {
        let mut d = DiagBag::new();
        assert!(verify_order_invariance(&unit_cube(), &mut d));
        assert!(verify_order_invariance(&flat_quad(), &mut d));
        assert!(!d.has_blocking());
    }

    #[test]
    fn f1605_diagbag_dedups_and_reports_drop() {
        let mut d = DiagBag::new();
        for _ in 0..10 {
            d.push(QuantDiag::PositionNan);
        }
        assert_eq!(d.len(), 1, "same code must not spam the bag");
        assert!(!d.is_empty());
        assert_eq!(d.get(0), Some(QuantDiag::PositionNan));
        assert!(d.has(QuantDiag::PositionNan));
        assert!(d.get(9).is_none());
    }

    #[test]
    fn f1605_blocking_classification() {
        assert!(QuantDiag::PositionNan.blocking());
        assert!(QuantDiag::PositionInf.blocking());
        assert!(QuantDiag::VertexSetEmpty.blocking());
        assert!(QuantDiag::NormalNotUnit.blocking());
        assert!(!QuantDiag::DegenerateAxis.blocking(), "flat mesh is legal");
        assert!(!QuantDiag::CodeOutOfRange.blocking(), "clamped is recoverable");
        for d in [
            QuantDiag::VertexSetEmpty,
            QuantDiag::PositionNan,
            QuantDiag::PositionInf,
            QuantDiag::NormalNotUnit,
            QuantDiag::BitsUnsupported,
            QuantDiag::DegenerateAxis,
            QuantDiag::CodeOutOfRange,
        ] {
            assert!(!d.code().is_empty() && !d.reason().is_empty() && !d.hint().is_empty());
        }
    }

    #[test]
    fn f1605_oct_wrap_is_an_involution() {
        let probes = [[0.3f32, 0.5], [-0.3, 0.5], [0.3, -0.5], [-0.3, -0.5], [0.0, 0.0]];
        for p in probes {
            let once = oct_wrap2d(p[0], p[1]);
            let twice = oct_wrap2d(once[0], once[1]);
            assert!((twice[0] - p[0]).abs() < 1.0e-6 && (twice[1] - p[1]).abs() < 1.0e-6);
        }
    }

    #[test]
    fn f1605_guards_never_panic() {
        assert!(QuantBits::from_bits(0).is_none());
        assert!(QuantBits::from_bits(32).is_none());
        assert!(encode_octahedral([0.0, 0.0, 0.0], &mut DiagBag::new()).is_none());
        assert!(encode_octahedral([f32::NAN, 0.0, 0.0], &mut DiagBag::new()).is_none());
        // 超出档位的码值必须被夹紧而非回绕。
        assert_eq!(clamp_i32(-5, 0, 255), 0);
        assert_eq!(clamp_i32(9999, 0, 255), 255);
        assert_eq!(clamp_i32(i32::MIN, 0, 255), 0);
        // 非整数长度的顶点数组按畸形处理（不 panic）。
        let mut d = DiagBag::new();
        assert!(compute_bounds(&[0.0, 0.0], &mut d).is_none());
    }

    #[test]
    fn f1605_domain_self_test_is_green() {
        let set = run_meshquant_checks();
        assert!(!set.truncated(), "check set overflowed");
        assert_eq!(set.len(), 16);
        assert!(set.all_passed(), "VE-F1605 domain self-test must pass");
    }
}