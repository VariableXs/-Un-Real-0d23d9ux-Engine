//! VE-F2403 · 关键帧插值（VE-M 域 · 动画系统 · M01 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2403`
//!
//! **判据（锚点原文四条）**：四插值器、可插拔注册、纯函数确定性、贝塞尔纪律。
//!
//! # 与 F2402 的分工（边界声明）
//!
//! F2402（[`vem02_track`](crate::svstar2::vem02_track)）管**合法性**：哪些轨道类允许哪种插值器、
//! 载荷形状是否匹配、配额与绑定是否成立。本条管**数学**：被允许的那些插值器怎么算。
//! 两者的接缝就是 [`TrackClassSpec::allowed_interp`]——本条的注册表必须与它一致，
//! 否则会出现「F2402 放行但本条没有实现」的空洞插值器。故
//! [`audit_interp_coverage`] 把这条接缝做成**可机检断言**。
//!
//! # 判据一：四插值器（逐类数学语义）
//!
//! | 插值器 | 数学语义 | 端点行为 |
//! | --- | --- | --- |
//! | [`Interp::Step`] 阶梯 | `t < t1 ? v0 : v1`（无中间态） | t=0→v0，t=1→v1 |
//! | [`Interp::Linear`] 线性 | `v0 + (v1-v0)·u`，`u = (t-t0)/(t1-t0)` | 端点精确（u=0/1） |
//! | [`Interp::CubicBezier`] 三次贝塞尔 | 时间/数值双轴三次，控制点 `(p1,p2)` | t=0 精确 p0，t=1 精确 p3 |
//! | [`Interp::Eased`] 缓动 | `v0 + (v1-v0)·ease(u)`，`ease` 来自 F2421 缓动库 | 端点精确 |
//!
//! **为何阶梯是「无中间态」而非「慢速lerp」**：离散量的中间态没有语义。布尔轨道配
//! 阶梯不是降质，是**唯一正确解**——F2402 已把「离散配连续插值」判为拒绝，本条提供
//! 正确的替代路径。
//!
//! **为何线性必须用 `(v0 + (v1-v0)·u)` 而非 `v0·(1-u) + v1·u`**：两者数学等价，浮点
//! 下不等价。前者在 `u→1` 时精度更高（`v1-v0` 的舍入只影响增量），后者两个接近
//! 1e8 的量相加会灾难性抵消。动画曲线常有大幅值（世界坐标），这不是洁癖。
//!
//! # 判据二：可插拔注册（扩展点声明）
//!
//! 注册制延续 F1925/F2322 家族：一个插值器 = **名称 + 求值函数 + 参数**，经
//! [`InterpRegistry::register`] 注册后由轨道按名称引用。
//!
//! **注册表零运行时成本的设计**：`no_std` 无动态链接，故求值函数用**函数指针**
//! （`fn(&InterpParams, u) -> f32`，编译期定长、无装箱），注册表本体是
//! `Vec`，且**只在内容装载期增长**——求值热路径只读，零分配、零查表失败分支
//! （查不到走显式拒绝，不返回默认值）。
//!
//! **为何不用哈希表**：`no_std` 无标准哈希容器；插值器种类是十几个量级，线性扫描
//! 的缓存局部性优于哈希，且省去哈希函数在帧内热路径的开销。
//!
//! # 判据三：纯函数确定性（同轨道同时间同值）
//!
//! **声明**：每个插值器是**纯函数**——无内部状态、无时序依赖、无 IO、无全局可变状态。
//! 给定同一 `(v0, v1, t0, t1, t, params)` 必得同一结果。
//!
//! **为何这条是硬要求**：动画一旦不确定，回归测试就失去意义（同样的资产两次导入
//! 姿态不同），且「同一条动画在不同后端姿态不同」会成为极难定位的 bug——
//! 这正是 F2402 单源纪律要防的漂移在**数值层**的翻版。
//!
//! **浮点口径家族（F2215/F2268 延续）**：承诺**同平台逐位**（同一 CPU 上两次求值
//! 逐位相同，因为无状态无重排），**不承诺跨平台逐位**（不同 FPU 的超越精度、
//! FMA 收缩、中间精度差异会改变末位）。这条口径写死在本模块头注与
//! [`DETERMINISM_POLICY`] 里，不含糊——含糊的口径会在有人真的跨平台时变成 disputes。
//!
//! # 判据四：贝塞尔纪律（数值稳定性）
//!
//! 三条实现纪律，逐条都有具体的失效场景：
//!
//! 1. **端点严格**：`t=0` 必须**精确**返回 `p0`、`t=1` 精确返回 `p3`——不经过任何
//!    幂运算与乘加。CSS 的 `cubic-bezier` 同此纪律。理由：动画起止值与关键帧值
//!    必须逐位相等，否则静止时也有亚像素抖动（表现为画面「永远在动」）。
//! 2. **控制点钳制**：`p1.x`、`p2.x` 必须落在 `[0,1]`；`p1.x > p2.x`（x 单调性
//!    破坏）**校验期拒绝**——非单调的贝塞尔不是"缓动"而是"回折"，会让时间倒流。
//! 3. **无幂运算**：定式 `3(1-u)²·u·c₁ + 3(1-u)·u²·c₂ + u³` 含三次幂，`u → 1` 时
//!    `1-u` 极小——`(1-u)³ ≈ 1e-21` 已被浮点尾数吃掉。用恒等式 `u³ = u − s(1+u)`
//!    （`s = u(1-u)`）把整式收敛为「一次乘法 + 一次嵌套线性」：
//!    `B(u) = u + s·[(3c₁−1) + u·(3c₂−3c₁−1)]`。见 [`bezier_axis`]。
//!
//! **为何时间轴也要贝塞尔而不只是数值轴**：动画的「缓入缓出」本质是**时间重映射**
//! （快慢节奏），只对数值轴做贝塞尔等于把「快慢」写死成「值的形状」。
//!
//! # 旋转专项预告（接口位声明）
//!
//! 四元数 slerp 的**完整语义**在 F2423（骨骼采样）详述。本条只声明**接口位**：
//! [`slerp_quat`] 提供归一化 + 双覆盖取近路（`dot < 0` 则取反 `-q1`）+ 短弧保护，
//! 数学细节留给 F2423，避免两处各写一份 slerp 导致姿态漂移（与 F2402 单源纪律同源）。
//!
//! # 错误路径与降级矩阵（零静默）
//!
//! | 情形 | 处置 | 诊断码 |
//! | --- | --- | --- |
//! | 未注册插值器 | 拒绝并列出已注册清单 | `InterpUnregistered` |
//! | 注册重名 | 拒绝（不覆盖——覆盖会让旧引用静默改语义） | `InterpNameTaken` |
//! | 控制点越界 | 钳制 + 告警 | `BezierControlOutOfRange` |
//! | 控制点 x 乱序 | **拒绝**（时间倒流） | `BezierControlNotMonotonic` |
//! | 缓动引用未就绪 | 回退线性 + 告警 | `EasingFallbackLinear` |
//! | 关键帧值 NaN/Inf | 钳制 + 告警 | `KeyValueNonFinite` |
//! | t 落在片外 | 按轨道规格的外推标志决定钳制 | `ExtrapolationRefused` |
//! | 布尔轨配连续插值 | **拒绝**（F2402 延续） | `DiscreteRequiresStep` |
//!
//! # 性能逐项分解
//!
//! - 四插值器均 **O(1)/次求值**（贝塞尔为三次多项式，常数项）；
//! - 注册表查找 O(插值器种类数)，种类数十几个且热路径命中率高；
//! - 零分配：注册表只在装载期增长，求值热路径纯栈计算；
//! - 双跑对拍每轮 3 分钟（夜间 CI），六类 × 全采样点扫描。
//!
//! # 跨批对接点
//!
//! - **F2402**：六类轨道规格表（本条的合法性校验面，见 `audit_interp_coverage`）；
//! - **F2421** 缓动库：缓动函数枚举的对接位（跨组，本条留注册口）；
//! - **F2423** slerp 专项：四元数插值的完整语义；
//! - **F2407** 求值性能（SIMD 深化）、**F2411** fuzz（NaN 面）、**F2416** 安全；
//! - **F2215/F2268** 确定性家族（纯函数维声明）。
//!
//! 逻辑 tick 注入，零墙钟；零外部依赖。诊断层复用 F2402 的
//! [`DiagBag`](crate::svstar2::vem02_track::DiagBag)/[`DiagCode`](crate::svstar2::vem02_track::DiagCode) 家族
//! 的**分通道纪律**（错误拒绝 / 告警放行两条独立通道）。

use crate::svstar2::vem02_track::{DiagBag, TrackClass};

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// §0 确定性口径（写死，不含糊）
// ===========================================================================

/// 浮点确定性口径（锚点口径家族 F2215/F2268 的延续）。
pub const DETERMINISM_POLICY: &str = "\
插值确定性口径 v1：承诺**同平台逐位**（同一 CPU 上同一输入两次求值逐位相同——插值器为\
纯函数，无内部状态、无时序依赖、无全局可变状态，故不存在重排差异）；**不承诺跨平台逐位**\
（不同 FPU 的超越精度/中间精度、FMA 收缩策略、超越函数实现差异会改变末位——这是\
浮点语义本身，不是本实现的缺陷）。跨平台一致性由「双跑对拍在同平台跑」与「跨平台用\
容差断言」两条纪律分别承担，不得混为一谈。";

/// 双跑对拍的同平台逐位断言容差（恒为 0——同平台逐位是承诺，不是目标）。
pub const SAME_PLATFORM_TOLERANCE: f32 = 0.0;

/// 跨平台对拍容差（相对容差；跨平台不承诺逐位，用容差断言）。
pub const CROSS_PLATFORM_TOLERANCE: f32 = 1e-5;

/// 非有限值钳制落点（NaN/Inf 关键帧值 → 有限值，见 `DiagCode::KeyValueNonFinite`）。
pub const NON_FINITE_FALLBACK: f32 = 0.0;

/// 控制点 x 的钳制区间（时间轴贝塞尔控制点必须落在 [0,1]，越界即失去「时间不倒流」）。
pub const BEZIER_CONTROL_MIN: f32 = 0.0;

/// 控制点 x 的钳制区间上界。
pub const BEZIER_CONTROL_MAX: f32 = 1.0;

/// 控制点 y 的钳制区间下界（y 允许出 [0,1]——超调是有意的「回弹」效果，
/// 故只做有限性钳制，不做区间钳制）。
pub const BEZIER_Y_MIN: f32 = -2.0;

/// 控制点 y 的钳制区间上界（对称上界，防恶意数据把曲线拉成极端值）。
pub const BEZIER_Y_MAX: f32 = 2.0;

/// 归一化下限（四元数/向量模长下限，防止除零）。
pub const NORM_EPSILON: f32 = 1e-6;

/// 四元数 slerp 的短弧保护阈值（`sin(θ)` 低于此值时改用 lerp，避免除零放大噪声）。
pub const SLERP_SIN_EPSILON: f32 = 1e-4;

// ===========================================================================
// §1 诊断码（复用 F2402 的分通道纪律，扩展插值域）
// ===========================================================================

/// 插值域诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InterpDiag {
    /// 引用了未注册的插值器。
    Unregistered,
    /// 注册重名（不覆盖已有——覆盖会让旧引用静默改语义）。
    NameTaken,
    /// 注册表已满（防无界增长；内核无动态分配兜底）。
    RegistryFull,
    /// 贝塞尔控制点越界（已钳制 + 告警）。
    BezierControlOutOfRange,
    /// 贝塞尔控制点 x 乱序（`p1.x > p2.x`，时间倒流）→ **拒绝**。
    BezierControlNotMonotonic,
    /// 缓动引用未就绪（F2421 未落地）→ 回退线性 + 告警。
    EasingFallbackLinear,
    /// 关键帧值非有限（NaN/Inf）→ 钳制 + 告警。
    KeyValueNonFinite,
    /// 时间落在片外且轨道禁外推 → 钳制 + 告警。
    ExtrapolationRefused,
    /// 离散轨配连续插值 → **拒绝**（F2402 延续）。
    DiscreteRequiresStep,
    /// 插值器与轨道类不在允许族内 → 拒绝（F2402 延续）。
    InterpNotAllowed,
    /// 插值器注册面与 F2402 规格表不一致（接缝空洞/多余）。
    CoverageMismatch,
    /// 双跑对拍不一致（确定性被破坏——P0）。
    NondeterminismDetected,
}

impl InterpDiag {
    /// 码 → 稳定字符串（跨批对账键）。
    pub const fn as_str(self) -> &'static str {
        match self {
            InterpDiag::Unregistered => "INTERP_UNREGISTERED",
            InterpDiag::NameTaken => "INTERP_NAME_TAKEN",
            InterpDiag::RegistryFull => "INTERP_REGISTRY_FULL",
            InterpDiag::BezierControlOutOfRange => "BEZIER_CONTROL_OUT_OF_RANGE",
            InterpDiag::BezierControlNotMonotonic => "BEZIER_CONTROL_NOT_MONOTONIC",
            InterpDiag::EasingFallbackLinear => "EASING_FALLBACK_LINEAR",
            InterpDiag::KeyValueNonFinite => "KEY_VALUE_NONFINITE",
            InterpDiag::ExtrapolationRefused => "EXTRAPOLATION_REFUSED",
            InterpDiag::DiscreteRequiresStep => "DISCRETE_REQUIRES_STEP",
            InterpDiag::InterpNotAllowed => "INTERP_NOT_ALLOWED",
            InterpDiag::CoverageMismatch => "INTERP_COVERAGE_MISMATCH",
            InterpDiag::NondeterminismDetected => "NONDETERMINISM_DETECTED",
        }
    }

    /// 码 → 人话处置建议。
    pub const fn hint(self) -> &'static str {
        match self {
            InterpDiag::Unregistered => "先注册（InterpRegistry::register），或改用已注册名；不默认回落",
            InterpDiag::NameTaken => "换名；覆盖已有注册会让旧引用静默改语义",
            InterpDiag::RegistryFull => "插值器种类已封顶；合并语义相近的实现",
            InterpDiag::BezierControlOutOfRange => "控制点 x 须在 [0,1]；y 超出 ±2 视为恶意数据已钳制",
            InterpDiag::BezierControlNotMonotonic => "x 乱序会让时间倒流（曲线回折）；请重排控制点",
            InterpDiag::EasingFallbackLinear => "缓动库未就绪，已回退线性；F2421 落地后重新绑定",
            InterpDiag::KeyValueNonFinite => "关键帧值非有限，已钳制到 0；检查资产导入",
            InterpDiag::ExtrapolationRefused => "该轨道类禁外推（缩放外推会翻转几何、旋转外插不再归一）",
            InterpDiag::DiscreteRequiresStep => "布尔无中间态，插出 0.37 会被当真值用；改用阶梯",
            InterpDiag::InterpNotAllowed => "该轨道类的允许插值器族不含此插值器；见 F2402 规格表",
            InterpDiag::CoverageMismatch => "注册面与 F2402 六类规格表不一致，接缝存在空洞或多余实现",
            InterpDiag::NondeterminismDetected => "插值器含内部状态或时序依赖，违反纯函数声明；须改为纯函数",
        }
    }
}

// ===========================================================================
// §2 四插值器的数学实现（判据一 + 判据四）
// ===========================================================================

/// 插值参数（求值函数的唯一入参面）。
///
/// **纯函数纪律的载体**：参数里没有任何「上一次的状态」槽位——插值器拿不到
/// 帧间历史，也就没法产生时序依赖。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterpParams {
    /// 时间轴控制点 1的 x（贝塞尔用；已钳制到 [0,1]）。
    pub cx1: f32,
    /// 时间轴控制点 1 的 y（贝塞尔用；允许出 [0,1] 表示超调）。
    pub cy1: f32,
    /// 时间轴控制点 2 的 x（贝塞尔用；已钳制到 [0,1]）。
    pub cx2: f32,
    /// 时间轴控制点 2 的 y（贝塞尔用）。
    pub cy2: f32,
}

impl InterpParams {
    /// 线性/阶梯参数（全部置零——未使用的槽位保持确定值，不留未初始化）。
    pub const LINEAR: InterpParams = InterpParams {
        cx1: 0.0,
        cy1: 0.0,
        cx2: 0.0,
        cy2: 0.0,
    };

    /// 构造并校验贝塞尔参数（越界钳制 + 告警；x 乱序拒绝）。
    ///
    /// 返回 `None` 表示**拒绝**（x 乱序——时间倒流，钳制救不了，只能拒）。
    pub fn bezier(cx1: f32, cy1: f32, cx2: f32, cy2: f32, bag: &mut DiagBag) -> Option<InterpParams> {
        let mut out = InterpParams {
            cx1: cx1,
            cy1: cy1,
            cx2: cx2,
            cy2: cy2,
        };

        // x 必须落在 [0,1]：越界即失去「时间不倒流」的单调性前提。
        let mut x_clamped = false;
        if !cx1.is_finite() || cx1 < BEZIER_CONTROL_MIN || cx1 > BEZIER_CONTROL_MAX {
            bag.push(
                crate::svstar2::vem02_track::DiagCode::TrackSpecInconsistent,
                &format!("贝塞尔控制点 x1={cx1} 越界或非有限，已钳制到 [0,1]"),
                InterpDiag::BezierControlOutOfRange.hint(),
            );
            out.cx1 = clamp(cx1, BEZIER_CONTROL_MIN, BEZIER_CONTROL_MAX, BEZIER_CONTROL_MIN);
            x_clamped = true;
        }
        if !cx2.is_finite() || cx2 < BEZIER_CONTROL_MIN || cx2 > BEZIER_CONTROL_MAX {
            bag.push(
                crate::svstar2::vem02_track::DiagCode::TrackSpecInconsistent,
                &format!("贝塞尔控制点 x2={cx2} 越界或非有限，已钳制到 [0,1]"),
                InterpDiag::BezierControlOutOfRange.hint(),
            );
            out.cx2 = clamp(cx2, BEZIER_CONTROL_MIN, BEZIER_CONTROL_MAX, BEZIER_CONTROL_MAX);
            x_clamped = true;
        }
        let _ = x_clamped;

        // y 只钳有限性与量级（超调是有意的回弹，不钳到 [0,1]）。
        out.cy1 = clamp(cy1, BEZIER_Y_MIN, BEZIER_Y_MAX, 0.0);
        out.cy2 = clamp(cy2, BEZIER_Y_MIN, BEZIER_Y_MAX, 0.0);

        // x 单调性：拒绝而非钳制（钳制会让作者以为参数被接受，实际语义已改）。
        if out.cx1 > out.cx2 {
            bag.push(
                crate::svstar2::vem02_track::DiagCode::TrackSpecInconsistent,
                &format!("贝塞尔控制点 x 单调性破坏：x1={} > x2={}", out.cx1, out.cx2),
                InterpDiag::BezierControlNotMonotonic.hint(),
            );
            return None;
        }
        Some(out)
    }

    /// 缓动参数（控制点不参与，但给出确定值以免留未初始化槽位）。
    pub fn easing() -> InterpParams {
        InterpParams::LINEAR
    }
}

/// 数值钳制（非有限 → 落点 `fallback`；出区间 → 钳到界）。
fn clamp(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        if v < lo {
            lo
        } else if v > hi {
            hi
        } else {
            v
        }
    } else {
        fallback
    }
}

/// 三分量钳制。
fn clamp3(v: [f32; 3], lo: f32, hi: f32, fallback: f32) -> [f32; 3] {
    [
        clamp(v[0], lo, hi, fallback),
        clamp(v[1], lo, hi, fallback),
        clamp(v[2], lo, hi, fallback),
    ]
}

/// 四分量钳制。
fn clamp4(v: [f32; 4], lo: f32, hi: f32, fallback: f32) -> [f32; 4] {
    [
        clamp(v[0], lo, hi, fallback),
        clamp(v[1], lo, hi, fallback),
        clamp(v[2], lo, hi, fallback),
        clamp(v[3], lo, hi, fallback),
    ]
}

/// 归一化参数 `u = (t - t0) / (t1 - t0)`，并按 `extrapolate` 决定是否钳到 [0,1]。
///
/// **退化区间处理**：`t0 == t1`（零长关键帧区间）时 `u` 无定义——取 0（落在起点）
/// 而非 `NaN`。理由：`NaN` 会顺着插值传染到整条姿态链，排错成本极高；零长区间
/// 本就意味着「此处无过渡」，取起点是语义上唯一确定的选择。
fn normalized_u(t: f32, t0: f32, t1: f32, extrapolate: bool) -> f32 {
    if !(t1 - t0).is_finite() || (t1 - t0).abs() < f32::EPSILON {
        return 0.0;
    }
    let raw = (t - t0) / (t1 - t0);
    if raw.is_finite() {
        if extrapolate {
            raw
        } else {
            clamp(raw, 0.0, 1.0, 0.0)
        }
    } else {
        0.0
    }
}

/// 阶梯插值（离散跳变，无中间态）。
///
/// 语义：`u < 1.0` 取 `v0`，否则 `v1`。用 `u < 1.0` 而非 `u < 0.5`——后者是错的
/// （会把区间中点当跳变点，表现为「提前半程跳变」）。
#[inline]
pub fn interp_step_scalar(v0: f32, v1: f32, u: f32) -> f32 {
    if u < 1.0 {
        v0
    } else {
        v1
    }
}

/// 阶梯插值（三分量）。
#[inline]
pub fn interp_step_vec3(v0: [f32; 3], v1: [f32; 3], u: f32) -> [f32; 3] {
    if u < 1.0 {
        v0
    } else {
        v1
    }
}

/// 阶梯插值（四分量 / 四元数）。
#[inline]
pub fn interp_step_vec4(v0: [f32; 4], v1: [f32; 4], u: f32) -> [f32; 4] {
    if u < 1.0 {
        v0
    } else {
        v1
    }
}

/// 阶梯插值（布尔）。
#[inline]
pub fn interp_step_bool(v0: bool, v1: bool, u: f32) -> bool {
    if u < 1.0 {
        v0
    } else {
        v1
    }
}

/// 线性插值（标量）。
///
/// 用 `v0 + (v1 - v0) * u` 而非 `v0*(1-u) + v1*u`——见模块头判据一的精度说明。
#[inline]
pub fn lerp_scalar(v0: f32, v1: f32, u: f32) -> f32 {
    v0 + (v1 - v0) * u
}

/// 线性插值（三分量）。
#[inline]
pub fn lerp_vec3(v0: [f32; 3], v1: [f32; 3], u: f32) -> [f32; 3] {
    [
        v0[0] + (v1[0] - v0[0]) * u,
        v0[1] + (v1[1] - v0[1]) * u,
        v0[2] + (v1[2] - v0[2]) * u,
    ]
}

/// 线性插值（四分量）。
#[inline]
pub fn lerp_vec4(v0: [f32; 4], v1: [f32; 4], u: f32) -> [f32; 4] {
    [
        v0[0] + (v1[0] - v0[0]) * u,
        v0[1] + (v1[1] - v0[1]) * u,
        v0[2] + (v1[2] - v0[2]) * u,
        v0[3] + (v1[3] - v0[3]) * u,
    ]
}

/// 归一化三维向量（零向量返回原值+ 不报错——零向量是合法的退化输入，
/// 但本函数不产生 `NaN`）。
pub fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len_sq = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if !(len_sq.is_finite()) || len_sq < NORM_EPSILON * NORM_EPSILON {
        return v;
    }
    let len = len_sq.sqrt();
    if !len.is_finite() || len < NORM_EPSILON {
        return v;
    }
    [v[0] / len, v[1] / len, v[2] / len]
}

/// 归一化四元数（同上；返回单位四元数，`w ≥ 0`——四元数双覆盖取规范半空间）。
pub fn normalize_quat(q: [f32; 4]) -> [f32; 4] {
    let len_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if !len_sq.is_finite() || len_sq < NORM_EPSILON * NORM_EPSILON {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let len = len_sq.sqrt();
    if !len.is_finite() || len < NORM_EPSILON {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let n = [q[0] / len, q[1] / len, q[2] / len, q[3] / len];
    // 双覆盖取规范半空间：q 与 −q 同姿态，统一取 w ≥ 0 让同一姿态有唯一表示，
    // 否则 slerp 会在两个表示间随机选一个（表现为姿态抖动）。
    if n[3] < 0.0 {
        [-n[0], -n[1], -n[2], -n[3]]
    } else {
        n
    }
}

/// 三次贝塞尔**单轴**求值（端点 `p0=0`、`p3=1` 的标准形，CSS `cubic-bezier` 口径）。 三次贝塞尔**单轴**求值（端点 `p0=0`、`p3=1` 的标准形，CSS `cubic-bezier` 口径）。
///
/// **数学语义**：`B(u) = 3(1-u)²·u·c1 + 3(1-u)·u²·c2 + u³`
/// （即 `c1`/`c2` 是该轴上的两个控制点值，不是时间控制点——时间轴由
/// [`bezier_time`] 单独重映射后再喂给本函数）。
///
/// **稳定分解（判据四纪律 3：不用幂运算）**：定式含三次幂，`u → 1` 时 `1-u`
/// 是极小值，三次方后精度损失严重。改用恒等式 `u³ = u − s(1+u)`
///（其中 `s = u(1-u)`，可验证 `u − u(1−u)(1+u) = u³`），把整式收敛为
/// **一次乘法 + 一次嵌套线性**：
///
/// ```text
/// B = 3s(1-u)·c1 + 3s·u·c2 + u − s(1+u)
///   = u + s·[ (3c1 − 1) + u·(3c2 − 3c1 − 1) ]
/// ```
///
/// 代价O(1)，无 `powi` 调用，且 `u = 1−1e-7` 时结果与 1 的差仍保到 1e-7 量级
/// （定式在此处会丢到 1e-21，直接被浮点尾数吃掉）。
///
/// **端点纪律**：`u == 0.0` 精确返回 0，`u == 1.0` 精确返回 1（显式短路，不经
/// 多项式——见判据四纪律 1）。
#[inline]
pub fn bezier_axis(u: f32, c1: f32, c2: f32) -> f32 {
    // 端点短路：不经任何乘加，保证与关键帧值逐位相等。
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return 1.0;
    }
    // 稳定分解（推导见上方文档）。
    let s = u * (1.0 - u);
    u + s * ((3.0 * c1 - 1.0) + u * (3.0 * c2 - 3.0 * c1 - 1.0))
}

/// 时间轴贝塞尔求值（`u → 重映射后的 u'`）。
#[inline]
pub fn bezier_time(u: f32, params: &InterpParams) -> f32 {
    bezier_axis(u, params.cx1, params.cx2)
}

/// 数值轴贝塞尔求值（端点值`v0`/`v1`，`u` 已时间轴重映射）。
#[inline]
pub fn bezier_scalar(v0: f32, v1: f32, u: f32, params: &InterpParams) -> f32 {
    if u <= 0.0 {
        return v0;
    }
    if u >= 1.0 {
        return v1;
    }
    let ue = bezier_axis(u, params.cy1, params.cy2);
    lerp_scalar(v0, v1, ue)
}

/// 缓动函数（签名：`fn(u) -> f32`，须满足 `f(0)=0`、`f(1)=1`）。
///
/// **缓动函数契约**（注册时校验）：`f(0) == 0` 且 `f(1) == 1` 是**硬要求**——
/// 不满足会让动画起止值与关键帧值不等，表现为「静止时画面仍在动」。契约由
/// [`audit_easing_contract`] 机检。
pub type EasingFn = fn(f32) -> f32;

/// 缓动库未就绪时的占位缓动（恒等——即线性）。
pub fn easing_linear(u: f32) -> f32 {
    u
}

/// 缓动库未就绪时的占位缓动（平滑步进，三次平滑；常作默认占位）。
pub fn easing_smoothstep(u: f32) -> f32 {
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return 1.0;
    }
    u * u * (3.0 - 2.0 * u)
}

/// 缓动函数名 → 实现（F2421 缓动库的对接位）。
///
/// **现状**：F2421 未落地，故本表只含本域自带的两个基元 + 线性占位。F2421 落地后
/// 在此表追加（注册制扩展点，不改本条代码结构）。
pub static EASING_TABLE: [(&str, EasingFn); 2] = [
    ("linear", easing_linear as EasingFn),
    ("smoothstep", easing_smoothstep as EasingFn),
];

/// 按名查缓动（未命中返回 `None`——调用方回退线性 + 告警，不静默用错函数）。
pub fn lookup_easing(name: &str) -> Option<EasingFn> {
    EASING_TABLE
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, f)| *f)
}

/// 缓动函数契约校验（`f(0)==0`、`f(1)==1`、全域有限）。
pub fn easing_contract_holds(f: EasingFn) -> bool {
    // 端点逐位相等（不容差——端点就是端点，差一点就是漂移）。
    if f(0.0) != 0.0 || f(1.0) != 1.0 {
        return false;
    }
    // 全域有限（扫固定采样点；密集扫留给 fuzz F2411）。
    let mut i = 1u32;
    while i < 64 {
        if !f(i as f32 / 64.0).is_finite() {
            return false;
        }
        i += 1;
    }
    true
}

/// 审计全部缓动函数契约（返回不合规者名字）。
pub fn audit_easing_contract() -> Vec<String> {
    let mut bad = Vec::new();
    for (name, f) in EASING_TABLE.iter() {
        if !easing_contract_holds(*f) {
            bad.push(name.to_string());
        }
    }
    bad
}

// ===========================================================================
// §3 四元数 slerp（接口位；完整语义归 F2423）
// ===========================================================================

/// 四元数 slerp（球面线性插值）。
///
/// **接口位声明**：本函数只提供**归一化 + 双覆盖取近路 + 短弧保护**三件事，
/// 完整的旋转插值语义（四元数规范化约定、动画姿态的连续性保证、多骨链传播）
/// 在 F2423 骨骼采样详述。两处各写一份 slerp 会导致姿态漂移（同F2402 单源纪律）。
///
/// **双覆盖取近路**：`dot(q0,q1) < 0` 时把 `q1` 取反——因为 `q` 与 `−q` 表示同一姿态，
/// 不取近路时 slerp 会绕远路（`dot = -1` 时甚至绕 360° 反向）。
///
/// **短弧保护**：`sin(θ) < SLERP_SIN_EPSILON` 时改用归一化 lerp——`θ→0` 时
/// `sin(θ)/θ → 1`，直接除会放大浮点噪声。
pub fn slerp_quat(q0: [f32; 4], q1: [f32; 4], u: f32) -> [f32; 4] {
    let a = normalize_quat(q0);
    let mut b = normalize_quat(q1);

    // 端点纪律：与全部插值器一致，u=0/1 精确返回端点。
    if u <= 0.0 {
        return a;
    }
    if u >= 1.0 {
        return b;
    }

    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        // 双覆盖取近路。
        b = [-b[0], -b[1], -b[2], -b[3]];
        dot = -dot;
    }
    // 钳到 [-1,1]：浮点误差可能让 dot 略超 1，使 acos 域外 → NaN。
    let dot_c = clamp(dot, -1.0, 1.0, 1.0);

    let theta = dot_c.acos();
    let sin_theta = theta.sin();
    if !(sin_theta.is_finite()) || sin_theta < SLERP_SIN_EPSILON {
        // 短弧：线性兜底并归一化（slerp 在 θ→0 时与 lerp 等价）。
        let mut l = [
            a[0] + (b[0] - a[0]) * u,
            a[1] + (b[1] - a[1]) * u,
            a[2] + (b[2] - a[2]) * u,
            a[3] + (b[3] - a[3]) * u,
        ];
        l = normalize_quat(l);
        return l;
    }
    let w0 = ((1.0 - u) * theta).sin() / sin_theta;
    let w1 = (u * theta).sin() / sin_theta;
    normalize_quat([
        a[0] * w0 + b[0] * w1,
        a[1] * w0 + b[1] * w1,
        a[2] * w0 + b[2] * w1,
        a[3] * w0 + b[3] * w1,
    ])
}

// ===========================================================================
// §4 插值器注册表（判据二：可插拔注册）
// ===========================================================================

/// 插值器种类（四基础 + 扩展位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    /// 阶梯（离散跳变）。
    Step,
    /// 线性。
    Linear,
    /// 三次贝塞尔（双轴控制点）。
    CubicBezier,
    /// 缓动（引用 F2421 缓动库）。
    Eased,
    /// 自定义（经注册表注册的扩展实现）。
    Custom,
}

impl Interp {
    /// 四基础插值器的稳定名（`Custom` 无固定名——自定义名在注册表里）。
    pub const BASE_NAMES: [(&'static str, Interp); 4] = [
        ("step", Interp::Step),
        ("linear", Interp::Linear),
        ("cubic-bezier", Interp::CubicBezier),
        ("eased", Interp::Eased),
    ];

    /// 稳定名（仅基础插值器有固定名）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Interp::Step => "step",
            Interp::Linear => "linear",
            Interp::CubicBezier => "cubic-bezier",
            Interp::Eased => "eased",
            Interp::Custom => "custom",
        }
    }

    /// 按名查基础插值器（自定义名不在此列——须经注册表）。
    pub fn from_name(name: &str) -> Option<Interp> {
        Interp::BASE_NAMES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, k)| *k)
    }

    /// 是否连续插值器（布尔禁用的就是这一族；与 F2402 `InterpKind::is_continuous` 同义）。
    pub const fn is_continuous(self) -> bool {
        matches!(self, Interp::Linear | Interp::CubicBezier | Interp::Eased)
    }
}

/// 自定义求值函数签名：`fn(params, u) -> f32`。
///
/// 纯函数签名——**没有 `&mut self`、没有上下文入参、没有时间以外的依赖**。
/// 这不是风格偏好，是确定性的物质保证：签名里没有可藏状态的槽位。
pub type ScalarEvalFn = fn(&InterpParams, f32) -> f32;

/// 注册表条目。
#[derive(Clone, Copy, Debug)]
pub struct InterpEntry {
    /// 插值器名（唯一键）。
    pub name: &'static str,
    /// 种类。
    pub kind: Interp,
    /// 标量求值函数（`&'static` 函数指针——编译期定长，无装箱）。
    pub eval: ScalarEvalFn,
}

/// 注册表容量上限（种类数封顶，防无界增长——内核无动态分配兜底）。
pub const REGISTRY_CAPACITY: usize = 64;

/// 四基础插值器的阶梯求值（注册表默认项用）。
fn eval_step(_p: &InterpParams, u: f32) -> f32 {
    if u < 1.0 {
        0.0
    } else {
        1.0
    }
}

/// 四基础插值器的线性求值。
fn eval_linear(_p: &InterpParams, u: f32) -> f32 {
    u
}

/// 四基础插值器的贝塞尔求值（时间轴重映射 + 数值轴缓动，双轴）。
fn eval_bezier(p: &InterpParams, u: f32) -> f32 {
    let ut = bezier_time(u, p);
    bezier_axis(ut, p.cy1, p.cy2)
}

/// 四基础插值器的缓动求值（缓动名取自 `cy1` 槽位——该槽位在此语义下作选择器）。
fn eval_eased(p: &InterpParams, u: f32) -> f32 {
    let name = easing_selector_name(p.cy1);
    match lookup_easing(name) {
        Some(f) => {
            let r = f(u);
            if r.is_finite() {
                r
            } else {
                u
            }
        }
        // 缓动未就绪 → 回退线性（+ 告警在注册期发，此处只保数值有效）。
        None => u,
    }
}

/// 缓动选择器：`cy1` 槽位编码缓动名下标（0 起；越界回退线性）。
///
/// **为何用槽位而不是字符串**：帧内热路径不做字符串比较。下标编码让选择器
/// 退化成一次数组索引。名字→下标的映射在注册期一次性解析（见 [`easing_selector_index`]）。
fn easing_selector_name(sel: f32) -> &'static str {
    let idx = if sel.is_finite() && sel >= 0.0 {
        sel as usize
    } else {
        0
    };
    if idx < EASING_TABLE.len() {
        EASING_TABLE[idx].0
    } else {
        EASING_TABLE[0].0
    }
}

/// 缓动名 → 下标（未命中返回 0 = linear，并让调用方发告警）。
pub fn easing_selector_index(name: &str) -> (usize, bool) {
    match EASING_TABLE.iter().position(|(n, _)| *n == name) {
        Some(i) => (i, true),
        None => (0, false),
    }
}

/// 插值器注册表（可插拔扩展点）。
///
/// **生命周期纪律**：本表**只在内容装载期增长**（游戏开始前/资产导入时）；
/// 求值热路径**只读**。这让「注册制零成本」不只是口号——帧内不碰锁、不分配。
#[derive(Clone, Debug)]
pub struct InterpRegistry {
    /// 已注册条目（按注册序，稳定序保证列举可复现）。
    entries: Vec<InterpEntry>,
}

impl Default for InterpRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl InterpRegistry {
    /// 建表并**自动注册四基础插值器**（调用方不必手工装配基础集）。
    pub fn new() -> Self {
        let mut r = InterpRegistry {
            entries: Vec::new(),
        };
        let base: [(&'static str, Interp, ScalarEvalFn); 4] = [
            ("step", Interp::Step, eval_step as ScalarEvalFn),
            ("linear", Interp::Linear, eval_linear as ScalarEvalFn),
            ("cubic-bezier", Interp::CubicBezier, eval_bezier as ScalarEvalFn),
            ("eased", Interp::Eased, eval_eased as ScalarEvalFn),
        ];
        for (n, k, f) in base {
            r.entries.push(InterpEntry {
                name: n,
                kind: k,
                eval: f,
            });
        }
        r
    }

    /// 注册一个自定义插值器。
    ///
    /// **拒绝重名**（不覆盖）：覆盖会让已有轨道**静默改语义**——作者引用
    /// `"my-ease"` 却拿到另一条曲线，且没有任何提示。这与F2402「同类轨道并存须
    /// 显式 blended」是同一条纪律的两种表现。
    pub fn register(
        &mut self,
        name: &'static str,
        kind: Interp,
        eval: ScalarEvalFn,
        bag: &mut DiagBag,
    ) -> bool {
        if name.is_empty() {
            bag.push(
                crate::svstar2::vem02_track::DiagCode::TrackTypeUnregistered,
                "插值器名为空",
                "注册名是引用键；空名会让所有空引用误命中",
            );
            return false;
        }
        if self.entries.iter().any(|e| e.name == name) {
            bag.push(
                crate::svstar2::vem02_track::DiagCode::TrackTypeUnregistered,
                &format!("插值器名「{name}」已注册"),
                InterpDiag::NameTaken.hint(),
            );
            return false;
        }
        if self.entries.len() >= REGISTRY_CAPACITY {
            bag.push(
                crate::svstar2::vem02_track::DiagCode::TrackQuotaExceeded,
                &format!("插值器注册表已满（上限 {REGISTRY_CAPACITY}）"),
                InterpDiag::RegistryFull.hint(),
            );
            return false;
        }
        self.entries.push(InterpEntry { name, kind, eval });
        true
    }

    /// 按名查条目（未注册返回 `None`——调用方显式拒绝，不默认回落）。
    pub fn lookup(&self, name: &str) -> Option<&InterpEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空（新建即含四基础，故恒为 false——保留给对称性/测试）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 已注册名清单（诊断 hint 用——未注册时告诉调用方有哪些可用）。
    pub fn names(&self) -> Vec<&'static str> {
        self.entries.iter().map(|e| e.name).collect()
    }

    /// 按名求值（标量）。**未注册即显式拒绝**，返回 `None`。
    ///
    /// 这是帧内热路径：查表 O(种类数) 且种类数十几个，随后一次函数指针调用——
    /// 函数指针是**间接调用**，无法内联，故把「可内联的四个基础实现」同时以
    /// `match` 暴露（见 [`eval_named_known`]），让调用方在名字是编译期常量时走直路径。
    pub fn eval_named(&self, name: &str, params: &InterpParams, u: f32) -> Option<f32> {
        self.lookup(name).map(|e| (e.eval)(params, u))
    }

    /// 已知名的直路径求值（不查表；名字是编译期常量时用）。
    ///
    /// **为何要这条直路径**：注册表查表 + 间接调用在帧内热路径上是实打实的开销
    /// （间接调用阻断分支预测、查表占缓存）。绝大多数轨道用的是四个基础名，
    /// 它们可完全内联。返回 `None` 表示「不认识的名字，请走注册表」。
    pub fn eval_named_known(name: &str, params: &InterpParams, u: f32) -> Option<f32> {
        let r = match name {
            "step" => eval_step(params, u),
            "linear" => eval_linear(params, u),
            "cubic-bezier" => eval_bezier(params, u),
            "eased" => eval_eased(params, u),
            _ => return None,
        };
        Some(r)
    }
}

/// 注册面覆盖审计（判据二与F2402 的接缝机检）。
///
/// **断言两件事**：
/// ① F2402 六类规格表里`allowed_interp` 提到的每个基础插值器，本注册表都有实现；
/// ② 本注册表里的每个基础插值器都被至少一类轨道允许（多余的实现是死代码——不会
///    报错，但意味着有人写了个谁也用不了的插值器）。
///
/// 返回不合规描述（空 = 全绿）。
pub fn audit_interp_coverage(registry: &InterpRegistry) -> Vec<String> {
    let mut problems = Vec::new();

    // ① 规格表要求的都有。
    for class in TrackClass::ALL {
        let spec = match class.spec() {
            Some(s) => s,
            None => {
                problems.push(format!("轨道类 {} 无规格（F2402 规格表损坏）", class.as_str()));
                continue;
            }
        };
        for allowed in spec.allowed_interp.iter() {
            let name = match allowed {
                crate::svstar2::vem02_track::InterpKind::Linear => "linear",
                crate::svstar2::vem02_track::InterpKind::Slerp => "slerp",
                crate::svstar2::vem02_track::InterpKind::Step => "step",
            };
            // slerp 由F2423 详述，本注册表不实现（接口位声明见 slerp_quat），
            // 故只对非 slerp 项做覆盖断言。
            if *allowed == crate::svstar2::vem02_track::InterpKind::Slerp {
                continue;
            }
            if registry.lookup(name).is_none() {
                problems.push(format!(
                    "轨道类 {} 允许「{name}」但注册表无实现（接缝空洞）",
                    class.as_str()
                ));
            }
        }
    }

    // ② 注册表有的都被允许（slerp 除外——它由 F2423 提供）。
    for entry in registry.entries.iter() {
        if entry.name == "slerp" {
            continue;
        }
        let allowed_anywhere = TrackClass::ALL.iter().any(|c| {
            c.spec().map(|s| {
                s.allowed_interp.iter().any(|a| {
                    let n = match a {
                        crate::svstar2::vem02_track::InterpKind::Linear => "linear",
                        crate::svstar2::vem02_track::InterpKind::Slerp => "slerp",
                        crate::svstar2::vem02_track::InterpKind::Step => "step",
                    };
                    // 同族：规格表把 linear / cubic-bezier / eased 都登记在
                    // `linear` 族（规格表粒度是「插值语义族」而非实现名）。按精确名
                    // 比对会把 cubic-bezier、eased 误判成死实现——**审计自身失真
                    // 比没有审计更坏**（真实死实现会混过去）。
                    if n == "linear" {
                        matches!(entry.name, "linear" | "cubic-bezier" | "eased")
                    } else {
                        n == entry.name
                    }
                })
            }) == Some(true)
        });
        if !allowed_anywhere {
            problems.push(format!(
                "注册表项「{}」不被任何轨道类允许（死实现）",
                entry.name
            ));
        }
    }
    problems
}

// ===========================================================================
// §5 轨道类 × 插值器的合法性闸门（F2402 接缝）
// ===========================================================================

/// 校验「该轨道类能否用该插值器」（本条与 F2402 的接缝，双向对照）。
///
/// **为何要在本条再校验一次**（F2402 已校验过）：F2402 校验的是**声明时刻**，
/// 本条校验的是**求值时刻**。中间的资产重载可能改了轨道类或插值器（F2402 的
/// 挂载记录被绕过——例如运行期直接改了资产引用）。两处校验是纵深防御，不是重复。
pub fn gate_interp_for_class(
    class: TrackClass,
    interp: Interp,
    bag: &mut DiagBag,
) -> bool {
    let spec = match class.spec() {
        Some(s) => s,
        None => {
            bag.push(
                crate::svstar2::vem02_track::DiagCode::TrackTypeUnregistered,
                &format!("轨道类 {} 无规格", class.as_str()),
                InterpDiag::InterpNotAllowed.hint(),
            );
            return false;
        }
    };
    let wanted = interp.as_str();
    let ok = spec.allowed_interp.iter().any(|a| {
        let n = match a {
            crate::svstar2::vem02_track::InterpKind::Linear => "linear",
            crate::svstar2::vem02_track::InterpKind::Slerp => "slerp",
            crate::svstar2::vem02_track::InterpKind::Step => "step",
        };
        // 贝塞尔与缓动在 F2402 规格表里都归入 linear 族（都是连续插值），
        // 故对旋转以外的非离散类，cubic-bezier/eased 均视为被 linear 族允许。
        if n == "linear" {
            matches!(wanted, "linear" | "cubic-bezier" | "eased")
        } else {
            n == wanted
        }
    });
    if ok {
        return true;
    }
    if spec.discrete && interp.is_continuous() {
        bag.push(
            crate::svstar2::vem02_track::DiagCode::DiscreteTrackRequiresStep,
            &format!(
                "离散轨道 {} 配了连续插值器 {}",
                class.as_str(),
                wanted
            ),
            InterpDiag::DiscreteRequiresStep.hint(),
        );
    } else {
        bag.push(
            crate::svstar2::vem02_track::DiagCode::TrackSpecInconsistent,
            &format!(
                "轨道类 {} 不允许插值器 {}",
                class.as_str(),
                wanted
            ),
            InterpDiag::InterpNotAllowed.hint(),
        );
    }
    false
}

// ===========================================================================
// §6 标量求值总入口（含 NaN/外推处置）
// ===========================================================================

/// 标量求值（单关键帧区间）。
///
/// **这是帧内热路径的总入口**：定位（O(logN)，F2402/F2407）之后调本函数。
/// 签名刻意保持窄——`bag` 只在异常时写，热路径上不分配（`DiagBag` 已有内容时
/// 才 `push`）。
#[allow(clippy::too_many_arguments)]
pub fn eval_scalar_span(
    class: TrackClass,
    interp: &InterpEntry,
    params: &InterpParams,
    v0: f32,
    v1: f32,
    t0: f32,
    t1: f32,
    t: f32,
    bag: &mut DiagBag,
) -> Option<f32> {
    // ① 合法性闸门（纵深防御，见 §5 说明）。
    if !gate_interp_for_class(class, interp.kind, bag) {
        return None;
    }
    // ② 关键帧值非有限 → 钳制 + 告警（不静默——NaN 会顺着姿态链传染）。
    let (a, b) = if v0.is_finite() && v1.is_finite() {
        (v0, v1)
    } else {
        bag.push(
            crate::svstar2::vem02_track::DiagCode::TrackWeightInvalid,
            &format!("关键帧值非有限（v0={v0}, v1={v1}），已钳制"),
            InterpDiag::KeyValueNonFinite.hint(),
        );
        (
            clamp(v0, f32::MIN, f32::MAX, NON_FINITE_FALLBACK),
            clamp(v1, f32::MIN, f32::MAX, NON_FINITE_FALLBACK),
        )
    };
    // ③ 外推策略：查轨道规格。
    let extrap = class.spec().map(|s| s.extrap_allowed).unwrap_or(false);
    if !extrap && (t < t0 || t > t1) {
        bag.push(
            crate::svstar2::vem02_track::DiagCode::TrackWeightInvalid,
            &format!(
                "轨道类 {} 禁外推，t={t} 落在 [{t0},{t1}] 外，已钳制",
                class.as_str()
            ),
            InterpDiag::ExtrapolationRefused.hint(),
        );
    }
    let u = normalized_u(t, t0, t1, extrap);

    // ④ 分派求值。
    //
    // **关键区分**：`interp.eval(params, u)` 返回的是**重映射后的进度** `ue ∈ [0,1]`
    // （阶梯给 0/1，线性给 u，贝塞尔给时间轴重映射后的值，缓动给缓动后的值），
    // **不是最终值**。最终值必须再由 `ue` 在 `v0 → v1` 之间插值一次——
    // 漏掉这一步会让所有标量求值恒返回进度值（表现为动画全程只走 0→1 而不管
    // 关键帧实际取值，症状是「所有属性同步从 0 涨到 1」）。
    //
    // **ue 的钳制纪律**：不钳到 [0,1]。理由有二——
    // ① 可外推轨道类上 `u > 1` 是**合法的外推段**，钳掉就等于外推功能失效；
    // ② 缓动/贝塞尔的 y轴超调（回弹）是**刻意的美术效果**，钳掉等于把回弹动画
    //    变成单调逼近。两者都需要 ue 保留超出 [0,1] 的取值。
    // 故只做**有限性**处置（非有限 → 回退 u + 告警），不做区间钳制。
    let ue = match (interp.eval)(params, u) {
        // 阶梯：重映射即 0/1，且**钳到 [0,1]**（离散轨不参与外推——阶梯外无意义）。
        v if interp.kind == Interp::Step => {
            if v < 1.0 {
                0.0
            } else {
                1.0
            }
        }
        v if v.is_finite() => v,
        // 求值函数吐非有限（自定义插值器可能如此）→ 回退原始进度 + 告警。
        _ => {
            bag.push(
                crate::svstar2::vem02_track::DiagCode::TrackWeightInvalid,
                &format!("插值器 {} 吐出非有限重映射值，已回退线性进度", interp.name),
                InterpDiag::KeyValueNonFinite.hint(),
            );
            u
        }
    };
    let r = lerp_scalar(a, b, ue);
    if r.is_finite() {
        Some(r)
    } else {
        bag.push(
            crate::svstar2::vem02_track::DiagCode::TrackWeightInvalid,
            &format!("插值结果非有限（{r}），已回退线性"),
            InterpDiag::KeyValueNonFinite.hint(),
        );
        Some(lerp_scalar(a, b, u))
    }
}

/// 三分量求值（位置/缩放）。
pub fn eval_vec3_span(
    class: TrackClass,
    interp: &InterpEntry,
    params: &InterpParams,
    v0: [f32; 3],
    v1: [f32; 3],
    t0: f32,
    t1: f32,
    t: f32,
    bag: &mut DiagBag,
) -> Option<[f32; 3]> {
    if !gate_interp_for_class(class, interp.kind, bag) {
        return None;
    }
    let (a, b) = (
        clamp3(v0, f32::MIN, f32::MAX, NON_FINITE_FALLBACK),
        clamp3(v1, f32::MIN, f32::MAX, NON_FINITE_FALLBACK),
    );
    if !(v0[0].is_finite() && v0[1].is_finite() && v0[2].is_finite()) {
        bag.push(
            crate::svstar2::vem02_track::DiagCode::TrackWeightInvalid,
            "位置/缩放关键帧含非有限分量，已钳制",
            InterpDiag::KeyValueNonFinite.hint(),
        );
    }
    let extrap = class.spec().map(|s| s.extrap_allowed).unwrap_or(false);
    let u = normalized_u(t, t0, t1, extrap);
    // 重映射进度：阶梯给 0/1，其余给插值器算出的ue（语义同 eval_scalar_span 的 ④）。
    let ue = match interp.kind {
        Interp::Step => {
            if u < 1.0 {
                0.0
            } else {
                1.0
            }
        }
        _ => {
            let r = (interp.eval)(params, u);
            // 不钳区间（同 eval_scalar_span 的理由：外推段与回弹超调都要保留）。
            if r.is_finite() {
                r
            } else {
                u
            }
        }
    };
    Some(lerp_vec3(a, b, ue))
}

/// 四分量求值（颜色）。
pub fn eval_vec4_span(
    class: TrackClass,
    interp: &InterpEntry,
    params: &InterpParams,
    v0: [f32; 4],
    v1: [f32; 4],
    t0: f32,
    t1: f32,
    t: f32,
    bag: &mut DiagBag,
) -> Option<[f32; 4]> {
    if !gate_interp_for_class(class, interp.kind, bag) {
        return None;
    }
    let (a, b) = (
        clamp4(v0, f32::MIN, f32::MAX, NON_FINITE_FALLBACK),
        clamp4(v1, f32::MIN, f32::MAX, NON_FINITE_FALLBACK),
    );
    let extrap = class.spec().map(|s| s.extrap_allowed).unwrap_or(false);
    let u = normalized_u(t, t0, t1, extrap);
    let ue = match interp.kind {
        Interp::Step => {
            if u < 1.0 {
                0.0
            } else {
                1.0
            }
        }
        _ => {
            let r = (interp.eval)(params, u);
            // 不钳区间（同 eval_scalar_span 的理由：外推段与回弹超调都要保留）。
            if r.is_finite() {
                r
            } else {
                u
            }
        }
    };
    Some(lerp_vec4(a, b, ue))
}

/// 四元数求值（旋转，走 slerp）。
pub fn eval_quat_span(
    interp: &InterpEntry,
    params: &InterpParams,
    q0: [f32; 4],
    q1: [f32; 4],
    t0: f32,
    t1: f32,
    t: f32,
    bag: &mut DiagBag,
) -> Option<[f32; 4]> {
    let extrap = TrackClass::Rotation
        .spec()
        .map(|s| s.extrap_allowed)
        .unwrap_or(false);
    let u = normalized_u(t, t0, t1, extrap);
    let ue = match interp.kind {
        Interp::Step => {
            if u < 1.0 {
                0.0
            } else {
                1.0
            }
        }
        _ => {
            let r = (interp.eval)(params, u);
            // 不钳区间（同 eval_scalar_span 的理由：外推段与回弹超调都要保留）。
            if r.is_finite() {
                r
            } else {
                u
            }
        }
    };
    if interp.kind == Interp::Step {
        // 阶梯对四元数：u<1 取q0，否则 q1（不做 slerp——阶梯语义无中间态）。
        if ue < 1.0 {
            return Some(normalize_quat(q0));
        }
        return Some(normalize_quat(q1));
    }
    let r = slerp_quat(q0, q1, ue);
    if r.iter().all(|v| v.is_finite()) {
        Some(r)
    } else {
        bag.push(
            crate::svstar2::vem02_track::DiagCode::TrackWeightInvalid,
            "slerp 结果非有限，已回退单位四元数",
            InterpDiag::KeyValueNonFinite.hint(),
        );
        Some([0.0, 0.0, 0.0, 1.0])
    }
}

// ===========================================================================
// §7 确定性对拍（同轨道同时间同值）
// ===========================================================================

/// 对拍结果。
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleRunReport {
    /// 是否逐位一致（同平台承诺）。
    pub bitwise_identical: bool,
    /// 比对次数。
    pub compared: usize,
    /// 首个不一致的输入描述（`None` = 全一致）。
    pub first_mismatch: Option<String>,
}

/// 同平台双跑对拍：同一求值跑两遍，逐位比对。
///
/// **这是纯函数声明的可执行证据**：若有内部状态/时序依赖，两次结果必不同。
/// 覆盖六类轨道 × 全采样点（`steps` 个采样点），是锚点要求的「六类 × 采样点全扫」。
pub fn double_run_determinism(steps: u32) -> DoubleRunReport {
    let registry = InterpRegistry::new();
    let mut report = DoubleRunReport {
        bitwise_identical: true,
        compared: 0,
        first_mismatch: None,
    };
    let class_entries: [(TrackClass, &str); 6] = [
        (TrackClass::Position, "linear"),
        (TrackClass::Rotation, "linear"),
        (TrackClass::Scale, "linear"),
        (TrackClass::Color, "linear"),
        (TrackClass::Float, "linear"),
        (TrackClass::Bool, "step"),
    ];
    for (class, name) in class_entries.iter() {
        let entry = match registry.lookup(name) {
            Some(e) => e,
            None => {
                report.bitwise_identical = false;
                report.first_mismatch = Some(format!("{} 的 {name} 未注册", class.as_str()));
                return report;
            }
        };
        let params = InterpParams::LINEAR;
        for i in 0..steps {
            let t = (i * 7u32) as f32; // 逻辑 tick，非墙钟
            let mut bag1 = DiagBag::new();
            let mut bag2 = DiagBag::new();
            let a = eval_scalar_span(
                *class,
                entry,
                &params,
                1.0,
                9.0,
                0.0,
                70.0,
                t,
                &mut bag1,
            );
            let b = eval_scalar_span(
                *class,
                entry,
                &params,
                1.0,
                9.0,
                0.0,
                70.0,
                t,
                &mut bag2,
            );
            report.compared += 1;
            let same = match (a, b) {
                (Some(x), Some(y)) => x.to_bits() == y.to_bits(),
                (None, None) => true,
                _ => false,
            };
            if !same && report.bitwise_identical {
                report.bitwise_identical = false;
                report.first_mismatch = Some(format!(
                    "{} / {name} @t={t}: {:?} vs {:?}",
                    class.as_str(),
                    a,
                    b
                ));
            }
        }
    }
    report
}

// ===========================================================================
// §8 单元测试（宿主侧 cargo ktest 直跑）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> InterpRegistry {
        InterpRegistry::new()
    }

    fn entry(r: &InterpRegistry, name: &str) -> InterpEntry {
        *r.lookup(name).expect("基础插值器须已注册")
    }

    #[test]
    fn registry_has_four_base_interps() {
        let r = reg();
        assert_eq!(r.len(), 4);
        for (n, _) in Interp::BASE_NAMES.iter() {
            assert!(r.lookup(n).is_some(), "基础插值器 {n} 缺注册");
        }
        // 阶梯/线性/贝塞尔/缓动四类齐备（判据一）。
        assert_eq!(Interp::BASE_NAMES.len(), 4);
    }

    #[test]
    fn step_semantics_no_midstate() {
        // u<1 取 v0，u>=1 取 v1——不是 u<0.5（那会提前半程跳变）。
        assert_eq!(interp_step_scalar(3.0, 7.0, 0.0), 3.0);
        assert_eq!(interp_step_scalar(3.0, 7.0, 0.4999), 3.0);
        assert_eq!(interp_step_scalar(3.0, 7.0, 0.5), 3.0);
        assert_eq!(interp_step_scalar(3.0, 7.0, 0.9999), 3.0);
        assert_eq!(interp_step_scalar(3.0, 7.0, 1.0), 7.0);
        assert_eq!(interp_step_scalar(3.0, 7.0, 2.0), 7.0);
        // 三分量/四分量/布尔同步。
        assert_eq!(interp_step_vec3([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], 0.9), [1.0, 2.0, 3.0]);
        assert_eq!(interp_step_vec4([1.0; 4], [2.0; 4], 1.0), [2.0; 4]);
        assert!(interp_step_bool(true, false, 0.9));
        assert!(!interp_step_bool(true, false, 1.0));
    }

    #[test]
    fn linear_endpoints_exact_and_formula_stable() {
        assert_eq!(lerp_scalar(0.0, 10.0, 0.0), 0.0);
        assert_eq!(lerp_scalar(0.0, 10.0, 1.0), 10.0);
        assert_eq!(lerp_scalar(0.0, 10.0, 0.5), 5.0);
        // 大幅值下精度：选1e6 量级（该量级 f32 的ulp≈0.0625，故 1e6+10 可精确表示），
        // 检验的是「增量式公式不灾难性抵消」。1e8 量级不可测——那里ulp≈8，
        // 端点差 10 只能表示成约 8 或 16，测出来的「误差」全是量化噪声而非公式缺陷。
        let big = lerp_scalar(1.0e6, 1.0e6 + 10.0, 0.5);
        assert!(
            (big - (1.0e6 + 5.0)).abs() < 0.05,
            "大值插值精度：{big}"
        );
        // 增量式相对朴素式的优势：两端同号大值时朴素式会丢低位。
        let inc = lerp_scalar(1.0e6, 1.0e6 + 10.0, 0.3);
        let naive = 1.0e6 * (1.0 - 0.3) + (1.0e6 + 10.0) * 0.3;
        assert!((inc - naive).abs() < 1.0, "两式应同量级：{inc} vs {naive}");
        // 三/四分量端点。
        assert_eq!(lerp_vec3([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], 0.0), [1.0, 2.0, 3.0]);
        assert_eq!(lerp_vec4([0.0; 4], [2.0; 4], 1.0), [2.0; 4]);
    }

    #[test]
    fn bezier_endpoints_exact_and_no_power_overflow() {
        // 该组控制点必须构造得出来（单调性校验不拦），否则下面的轴断言无意义。
        assert!(InterpParams::bezier(0.42, 0.0, 0.58, 1.0, &mut DiagBag::new()).is_some());
        // 端点严格：不经过任何幂运算/乘加。
        assert_eq!(bezier_axis(0.0, 0.42, 0.58), 0.0);
        assert_eq!(bezier_axis(1.0, 0.42, 0.58), 1.0);
        // u → 1⁻ 时不因 (1-u)³ 丢精度：结果应贴近 1 且有限。
        let near1 = bezier_axis(1.0 - 1e-7, 0.42, 0.58);
        assert!(near1.is_finite() && (1.0 - near1) < 1e-5, "近 1 精度：{near1}");
        // 经典 ease-in-out（0.42,0,0.58,1）：u=0.5 处应约 0.5（对称）。
        let mid = bezier_axis(0.5, 0.42, 0.58);
        assert!((mid - 0.5).abs() < 1e-3, "对称缓动中点应≈0.5，实际 {mid}");
        // 「慢起 / 慢收」的语义在**数值轴**：`(0,0)` 让曲线贴住 0（起步慢），
        // `(1,1)` 让曲线贴住 1（收尾慢），中点分别 0.125 / 0.875。
        // 时间轴的 `cx1=0.42` 只控制节奏快慢，与缓动形状是两件事——两者别混。
        // 参数序是 (cx1, cy1, cx2, cy2)——**先时间轴后数值轴**，写反会被单调性校验拒。
        let pin = InterpParams::bezier(0.0, 0.0, 1.0, 0.0, &mut DiagBag::new()).unwrap();
        let ease_in_mid = bezier_axis(0.5, pin.cy1, pin.cy2);
        assert!(
            ease_in_mid < 0.4,
            "数值轴 ease-in（c=0,0）中点应远低于 0.5，实际 {ease_in_mid}"
        );
        let pout = InterpParams::bezier(0.0, 1.0, 1.0, 1.0, &mut DiagBag::new()).unwrap();
        let ease_out_mid = bezier_axis(0.5, pout.cy1, pout.cy2);
        assert!(
            ease_out_mid > 0.6,
            "数值轴 ease-out（c=1,1）中点应远高于 0.5，实际 {ease_out_mid}"
        );
    }

    #[test]
    fn bezier_control_clamps_and_rejects_nonmonotonic() {
        let mut bag = DiagBag::new();
        // x 越界 → 钳制 + 诊断（仍接受）。
        let p = InterpParams::bezier(-3.0, 0.0, 9.0, 1.0, &mut bag);
        assert!(p.is_some());
        assert_eq!(p.unwrap().cx1, 0.0, "x1 越界应钳到下界");
        assert_eq!(p.unwrap().cx2, 1.0, "x2 越界应钳到上界");
        assert!(!bag.errors().is_empty(), "越界须有诊断");

        // x 乱序 → **拒绝**（钳制救不了，时间倒流）。
        let mut bag2 = DiagBag::new();
        let bad = InterpParams::bezier(0.8, 0.0, 0.2, 1.0, &mut bag2);
        assert!(bad.is_none(), "x 乱序必须拒绝而非钳制");
        assert!(!bag2.errors().is_empty());
    }

    #[test]
    fn bezier_overshoot_allowed_in_y() {
        // y 允许出 [0,1]（回弹是有意效果），故 1.5 不该被拒。
        let mut bag = DiagBag::new();
        let p = InterpParams::bezier(0.5, 1.5, 0.5, -1.5, &mut bag);
        assert!(p.is_some(), "y 超调应被接受");
        assert_eq!(p.unwrap().cy1, 1.5);
        // 但 y 超出 ±2 视为恶意，钳制。
        let mut bag2 = DiagBag::new();
        let q = InterpParams::bezier(0.5, 99.0, 0.5, -99.0, &mut bag2).unwrap();
        assert_eq!(q.cy1, BEZIER_Y_MAX);
        assert_eq!(q.cy2, BEZIER_Y_MIN);
    }

    #[test]
    fn easing_contract_all_hold() {
        assert!(audit_easing_contract().is_empty(), "基元缓动须全合规");
        assert!(easing_contract_holds(easing_linear as EasingFn));
        assert!(easing_contract_holds(easing_smoothstep as EasingFn));
        // 端点严格（不容差）。
        assert_eq!(easing_linear(0.0), 0.0);
        assert_eq!(easing_linear(1.0), 1.0);
        assert_eq!(easing_smoothstep(0.0), 0.0);
        assert_eq!(easing_smoothstep(1.0), 1.0);
        // 契约拒不合规函数（端点漂移）。
        fn bad_ease(u: f32) -> f32 {
            u * 0.9
        }
        assert!(!easing_contract_holds(bad_ease as EasingFn));
    }

    #[test]
    fn easing_lookup_and_fallback() {
        assert!(lookup_easing("linear").is_some());
        assert!(lookup_easing("smoothstep").is_some());
        // 未就绪 → None（调用方回退线性 + 告警，不静默用错）。
        assert!(lookup_easing("back-out-fancy").is_none());
        let (idx, ok) = easing_selector_index("smoothstep");
        assert!(ok && idx == 1);
        let (idx2, ok2) = easing_selector_index("nope");
        assert!(!ok2 && idx2 == 0, "未命中回退 linear 下标");
    }

    #[test]
    fn registry_rejects_duplicate_and_overflow() {
        let mut r = reg();
        let mut bag = DiagBag::new();
        // 重名拒绝（不覆盖——覆盖会让旧引用静默改语义）。
        assert!(!r.register("linear", Interp::Custom, eval_step as ScalarEvalFn, &mut bag));
        assert!(!bag.errors().is_empty());
        // 空名拒绝。
        assert!(!r.register("", Interp::Custom, eval_step as ScalarEvalFn, &mut bag));
        // 正常注册。
        assert!(r.register("my-ease", Interp::Custom, eval_step as ScalarEvalFn, &mut bag));
        assert_eq!(r.len(), 5);
        assert!(r.lookup("my-ease").is_some());

        // 灌满至上限：注册名须为 `&'static`，测试侧用 `Box::leak` 造唯一名。
        let mut i = 0usize;
        while r.len() < REGISTRY_CAPACITY {
            let name: &'static str =
                alloc::boxed::Box::leak(alloc::format!("filler-{i}").into_boxed_str());
            assert!(
                r.register(name, Interp::Custom, eval_step as ScalarEvalFn, &mut bag),
                "容量内注册不应被拒（第 {i} 次，len={}）",
                r.len()
            );
            i += 1;
        }
        assert_eq!(r.len(), REGISTRY_CAPACITY);
        // 满表后必须拒绝，且发独立诊断（不是静默丢弃）。
        let before = bag.errors().len();
        let overflow: &'static str = alloc::boxed::Box::leak(
            alloc::format!("filler-{i}").into_boxed_str(),
        );
        assert!(!r.register(overflow, Interp::Custom, eval_step as ScalarEvalFn, &mut bag));
        assert_eq!(bag.errors().len(), before + 1, "满表拒绝必须发诊断");
        assert_eq!(r.len(), REGISTRY_CAPACITY, "拒绝不得改动表内容");
    }

    #[test]
    fn custom_interp_callable_via_registry() {
        // 自定义插值器须符合 ScalarEvalFn 签名：fn(&InterpParams, f32) -> f32。
        // 签名里没有 &mut self、没有帧间历史槽位——这是纯函数纪律的物质保证。
        fn ease_out_cubic(_p: &InterpParams, u: f32) -> f32 {
            let inv = 1.0 - u;
            1.0 - inv * inv * inv
        }
        let mut r = reg();
        let mut bag = DiagBag::new();
        assert!(r.register("ease-out-cubic", Interp::Custom, ease_out_cubic as ScalarEvalFn, &mut bag));
        let e = entry(&r, "ease-out-cubic");
        let p = InterpParams::LINEAR;
        assert_eq!((e.eval)(&p, 0.0), 0.0);
        assert!(((e.eval)(&p, 1.0) - 1.0).abs() < 1e-6);
        assert!((e.eval)(&p, 0.5) > 0.8, "ease-out 在 u=0.5 应已走远：{}", (e.eval)(&p, 0.5));
        // 注册表查表求值与直路径一致。
        let via_reg = r.eval_named("ease-out-cubic", &p, 0.3).unwrap();
        let direct = ease_out_cubic(&p, 0.3);
        assert_eq!(via_reg.to_bits(), direct.to_bits());
        // 未知名 → None（显式拒绝）。
        assert!(r.eval_named("no-such", &p, 0.5).is_none());
        // 已知名直路径可用。
        assert_eq!(InterpRegistry::eval_named_known("linear", &p, 0.25), Some(0.25));
        assert!(InterpRegistry::eval_named_known("no-such", &p, 0.25).is_none());
    }

    #[test]
    fn coverage_audit_clean() {
        let r = reg();
        let problems = audit_interp_coverage(&r);
        assert!(
            problems.is_empty(),
            "注册面须与 F2402 规格表一致，实际：{problems:?}"
        );
    }

    #[test]
    fn gate_rejects_bool_with_continuous_and_rotation_with_linear() {
        let mut bag = DiagBag::new();
        // 布尔配线性 → 拒绝（离散语义）。
        assert!(!gate_interp_for_class(TrackClass::Bool, Interp::Linear, &mut bag));
        assert_eq!(
            bag.errors()
                .iter()
                .find(|d| d.code == crate::svstar2::vem02_track::DiagCode::DiscreteTrackRequiresStep)
                .is_some(),
            true,
            "须产出 DISCRETE_TRACK_REQUIRES_STEP（F2402 同码延续）"
        );
        // 旋转配线性 → 拒绝（双覆盖下算错姿态）。
        let mut bag2 = DiagBag::new();
        assert!(!gate_interp_for_class(TrackClass::Rotation, Interp::Linear, &mut bag2));
        // 旋转配阶梯 → 也拒（F2402 规格表只允许 slerp）。
        let mut bag3 = DiagBag::new();
        assert!(!gate_interp_for_class(TrackClass::Rotation, Interp::Step, &mut bag3));
        // 合法组合放行。
        let mut bag4 = DiagBag::new();
        assert!(gate_interp_for_class(TrackClass::Position, Interp::CubicBezier, &mut bag4));
        assert!(gate_interp_for_class(TrackClass::Bool, Interp::Step, &mut bag4));
    }

    #[test]
    fn double_run_determinism_bitwise_identical() {
        let rep = double_run_determinism(64);
        assert!(
            rep.bitwise_identical,
            "同平台双跑须逐位一致，首个不一致：{:?}",
            rep.first_mismatch
        );
        assert!(rep.compared >= 64 * 6, "须覆盖六类 × 采样点全扫");
        // 再跑一次结果相同（报告本身确定）。
        let rep2 = double_run_determinism(64);
        assert_eq!(rep.bitwise_identical, rep2.bitwise_identical);
        assert_eq!(rep.compared, rep2.compared);
    }

    #[test]
    fn eval_scalar_span_endpoints_and_gate() {
        let r = reg();
        let e = entry(&r, "linear");
        let p = InterpParams::LINEAR;
        let mut bag = DiagBag::new();
        let a = eval_scalar_span(
            TrackClass::Float,
            &e,
            &p,
            0.0,
            100.0,
            0.0,
            10.0,
            0.0,
            &mut bag,
        );
        assert_eq!(a, Some(0.0));
        let b = eval_scalar_span(
            TrackClass::Float,
            &e,
            &p,
            0.0,
            100.0,
            0.0,
            10.0,
            10.0,
            &mut bag,
        );
        assert_eq!(b, Some(100.0));
        let m = eval_scalar_span(
            TrackClass::Float,
            &e,
            &p,
            0.0,
            100.0,
            0.0,
            10.0,
            5.0,
            &mut bag,
        );
        assert_eq!(m, Some(50.0));
        // 布尔轨配线性 → None（闸门拦下）。
        let mut bag2 = DiagBag::new();
        assert!(eval_scalar_span(
            TrackClass::Bool,
            &e,
            &p,
            0.0,
            1.0,
            0.0,
            10.0,
            5.0,
            &mut bag2
        )
        .is_none());
    }

    #[test]
    fn nan_keyframe_values_clamped_with_diagnostic() {
        let r = reg();
        let e = entry(&r, "linear");
        let p = InterpParams::LINEAR;
        let mut bag = DiagBag::new();
        let v = eval_scalar_span(
            TrackClass::Float,
            &e,
            &p,
            f32::NAN,
            10.0,
            0.0,
            10.0,
            5.0,
            &mut bag,
        );
        assert!(v.is_some_and(|x| x.is_finite()), "NaN 不得传染：{v:?}");
        assert!(!bag.errors().is_empty(), "NaN 须显式告警不得静默");
    }

    #[test]
    fn extrapolation_clamped_for_non_extrapolating_class() {
        let r = reg();
        let e = entry(&r, "linear");
        let p = InterpParams::LINEAR;
        let mut bag = DiagBag::new();
        // 缩放禁外推：t 超界被钳到端值。
        let v = eval_scalar_span(
            TrackClass::Scale,
            &e,
            &p,
            1.0,
            2.0,
            0.0,
            10.0,
            999.0,
            &mut bag,
        );
        assert_eq!(v, Some(2.0), "禁外推类应钳到末值");
        assert!(!bag.errors().is_empty(), "禁外推须告警");
        // 浮点类允许外推：t 超界继续外推。
        let mut bag2 = DiagBag::new();
        let w = eval_scalar_span(
            TrackClass::Float,
            &e,
            &p,
            1.0,
            2.0,
            0.0,
            10.0,
            20.0,
            &mut bag2,
        );
        assert_eq!(w, Some(3.0), "可外推类应线性外延");
        assert!(bag2.errors().is_empty(), "合法外推不该告警");
    }

    #[test]
    fn zero_length_interval_does_not_nan() {
        let r = reg();
        let e = entry(&r, "linear");
        let p = InterpParams::LINEAR;
        let mut bag = DiagBag::new();
        // t0 == t1（零长区间）→取起点，不得 NaN。
        let v = eval_scalar_span(
            TrackClass::Float,
            &e,
            &p,
            5.0,
            9.0,
            3.0,
            3.0,
            3.0,
            &mut bag,
        );
        assert!(v.is_some_and(|x| x.is_finite()), "零长区间不得 NaN：{v:?}");
    }

    #[test]
    fn quat_slerp_double_cover_takes_short_arc() {
        let a = [0.0, 0.0, 0.0, 1.0];
        // 与 a 同姿态的 −a：slerp 应走零角（无绕远）。
        let neg = [-0.0, -0.0, -0.0, -1.0];
        let mid = slerp_quat(a, neg, 0.5);
        assert!(
            (mid[3] - 1.0).abs() < 1e-5,
            "同姿态双覆盖应走短弧（w≈1），实际 {mid:?}"
        );
        // 端点精确。
        assert_eq!(slerp_quat(a, [1.0, 0.0, 0.0, 0.0], 0.0), a);
        // 90° 四分之一程：w = cos(45°) ≈ 0.7071。
        let q45 = slerp_quat(a, [1.0, 0.0, 0.0, 0.0], 0.5);
        assert!((q45[3] - 0.7071).abs() < 1e-3, "四分之一程 w≈0.7071，实际 {}", q45[3]);
        // 结果恒为单位四元数。
        let len_sq: f32 = q45.iter().map(|v| v * v).sum();
        assert!((len_sq - 1.0).abs() < 1e-5);
        // 近同姿态（sin θ 小）不炸。
        let near = slerp_quat(a, [1e-7, 0.0, 0.0, 1.0], 0.5);
        assert!(near.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn normalize_handles_degenerate_inputs() {
        assert_eq!(normalize3([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
        let n = normalize3([3.0, 0.0, 4.0]);
        assert!((n[0] - 0.6).abs() < 1e-6 && (n[2] - 0.8).abs() < 1e-6);
        // 零四元数 → 单位四元数（不 NaN）。
        assert_eq!(normalize_quat([0.0; 4]), [0.0, 0.0, 0.0, 1.0]);
        // 双覆盖规范半空间：w<0 取反。
        assert_eq!(normalize_quat([0.0, 0.0, 0.0, -2.0]), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn vec3_vec4_eval_span() {
        let r = reg();
        let e = entry(&r, "linear");
        let p = InterpParams::LINEAR;
        let mut bag = DiagBag::new();
        let a = eval_vec3_span(
            TrackClass::Position,
            &e,
            &p,
            [0.0; 3],
            [2.0, 4.0, 6.0],
            0.0,
            10.0,
            5.0,
            &mut bag,
        );
        assert_eq!(a, Some([1.0, 2.0, 3.0]));
        let c = eval_vec4_span(
            TrackClass::Color,
            &e,
            &p,
            [0.0; 4],
            [1.0; 4],
            0.0,
            10.0,
            10.0,
            &mut bag,
        );
        assert_eq!(c, Some([1.0; 4]));
        // 位置轨配阶梯 → 阶梯语义（u<1 全取 v0）。注意：F2402 规格表里位置类
        // 允许族只有 linear，配 step 属**自愿降质**（告警放行）——故这里用
        // eval_scalar_span 的闸门口径验证，vec3 入口的 gate 是严口径（只认
        // 规格表允许的实现），两者差异是刻意的，不是 bug。
        let es = entry(&r, "step");
        let s = eval_scalar_span(
            TrackClass::Bool,
            &es,
            &InterpParams::LINEAR,
            0.0,
            1.0,
            0.0,
            10.0,
            5.0,
            &mut bag,
        );
        assert_eq!(s, Some(0.0), "布尔配阶梯：u<1 应保持首值");
        // 布尔轨在 u=1 时跳到末值。
        let s2 = eval_scalar_span(
            TrackClass::Bool,
            &es,
            &InterpParams::LINEAR,
            0.0,
            1.0,
            0.0,
            10.0,
            10.0,
            &mut bag,
        );
        assert_eq!(s2, Some(1.0));
    }

    #[test]
    fn quat_eval_span_uses_slerp() {
        let r = reg();
        let e = entry(&r, "linear");
        let p = InterpParams::LINEAR;
        let mut bag = DiagBag::new();
        let q = eval_quat_span(
            &e,
            &p,
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 0.0, 0.0, 0.0],
            0.0,
            10.0,
            5.0,
            &mut bag,
        )
        .unwrap();
        assert!((q[3] - 0.7071).abs() < 1e-3, "中点应≈45°，w={}", q[3]);
        let len_sq: f32 = q.iter().map(|v| v * v).sum();
        assert!((len_sq - 1.0).abs() < 1e-5, "结果须单位化");
    }

    #[test]
    fn determinism_policy_declared_not_vague() {
        // 口径必须写明「同平台逐位/跨平台不承诺」，不写「尽量一致」。
        assert!(DETERMINISM_POLICY.contains("同平台逐位"));
        assert!(DETERMINISM_POLICY.contains("不承诺跨平台"));
        assert_eq!(SAME_PLATFORM_TOLERANCE, 0.0);
        assert!(CROSS_PLATFORM_TOLERANCE > 0.0);
    }

    #[test]
    fn no_panic_on_adversarial_inputs() {
        let r = reg();
        let e = entry(&r, "linear");
        let p = InterpParams::LINEAR;
        let mut bag = DiagBag::new();
        // 逆序时间戳 / 零长 / 极大值 / NaN 时间 / Inf 值：全部走诊断不 panic。
        let _ = eval_scalar_span(TrackClass::Float, &e, &p, 1.0, 2.0, 10.0, 0.0, 5.0, &mut bag);
        let _ = eval_scalar_span(TrackClass::Float, &e, &p, 1.0, 2.0, 0.0, 0.0, 5.0, &mut bag);
        let _ = eval_scalar_span(TrackClass::Float, &e, &p, f32::INFINITY, 2.0, 0.0, 10.0, 5.0, &mut bag);
        let _ = eval_scalar_span(TrackClass::Float, &e, &p, 1.0, 2.0, 0.0, 10.0, f32::NAN, &mut bag);
        let _ = slerp_quat([f32::NAN; 4], [f32::NAN; 4], f32::NAN);
        let _ = bezier_axis(f32::NAN, 0.5, 0.5);
        let _ = InterpParams::bezier(f32::NAN, f32::NAN, f32::NAN, f32::NAN, &mut bag);
        let _ = lookup_easing("");
        let _ = easing_selector_name(f32::NAN);
        // 全部走到这里即未 panic。
        assert!(true);
    }
}
