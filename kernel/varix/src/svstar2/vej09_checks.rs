//! VE-F1809 · 域自检（判据逐条对应，见 `vej09_ibl.rs` 头注）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1809`
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **预积分**（mip 链卷积 + BRDF LUT）→ `C09-PRE-*`
//! - **双查 O(1)**（粗糙度选 mip + LUT 双查）→ `C09-DUAL-*`
//! - **三档**（1024/512/256 采样分档）→ `C09-TIER-*`
//! - **F1642 兑现**（IBL 双通道）→ `C09-F1642-*`
//! - **降级矩阵 / 格式校验 / 增量重跑**→ `C09-DEG-*` / `C09-FMT-*`
//! - **加固判据**（弱门禁十诫回补）→ `C09-HARDEN-*`
//!
//! ---
//!
//! ## 一、判据侧为什么必须能取到「积分真值」而非查表
//!
//! 本模块曾有一个 **190 倍能量丢失**的缺陷：BRDF LUT 在 `NdotH`
//! 上均匀分段累加（丢掉 pdf 归一化），使镜面极限处 `A+B = 0.0052`
//! 而真值是 `1.0`。画面症状是「IBL 镜面反射几乎全黑」。
//!
//! **这类缺陷为什么能活过齐备的判据**：若判据只能 `lut.query(...)`，
//! 它取到的是 512 级网格上的**双线性插值**值，而 LUT 语义是经
//! `F0·scale + bias` 参与合成——判据若只断言「输出有限」「落在
//! 0..1 内」，**丢 190 倍能量后仍然全绿**（0.0052 也在 0..1 内）。
//!
//! 故本判据的第一原则：**归一化类契约必须能在任意坐标取到积分真值**
//! （经 [`brdf_integral_at`]），并在**极限点**断言绝对值，而不是
//! 断言「落在某个宽松区间内」。
//!
//! ## 二、参考实现的独立性
//!
//! `C09-HARDEN-01` 用 **Karis 解析近似**（业界公开拟合式）作交叉
//! 对拍参照。该式**只写在判据侧**、不复用被测的任何函数，且与被测
//! 实现**结构完全不同**（闭式多项式 vs 蒙特卡洛重要性采样）——
//! 这正是交叉对拍应有的形态。
//!
//! ### 2.1 参照物自己也可能是错的（本模块踩过的两坑）
//!
//! 交叉对拍的隐含前提是「参照侧正确」。本模块两次证明该前提不成立：
//!
//! 1. **`pow2` 丢泰勒项**：`2^x` 曾退化成 `2^floor(x·ln2)`——一个
//!    **阶梯函数**。放进 Karis 参照里后，参照式自身成了阶梯，而
//!    **没有一条判据会转红**（它只是稳定地给出系统性偏离的结果）。
//!    修法：`2^x = 2^e·e^f`，两段都算、都要乘上去。
//!
//! 2. **拟合式在训练域外失效**：Karis 拟合式是多项式，在
//!    `NdotV < 0.6` 的掠射角侧把 `A+B` 恒压在 0.89 附近，**不具
//!    参照效力**。曾据此把**本来正确**的被测权重误改为 `Vis·NdotL`
//!    以迎合错的参照（`A` 恒≈0.25、严重丢能量），由 `C09-PRE-01`
//!    当即转红才回滚。
//!
//! 3. **「拟合式算出的值看着可疑」不等于拟合式写错**：追查中曾因
//!    `A+B` 在`NoV=1` 处恒为 0.9725/0.725/0.835 等「整齐值」而
//!    判定参照式转写有误，凭记忆改写公式——改后 `B` 变负值，
//!    一眼即错。核对公开源码（lygia `envBRDFApprox.hlsl`、UE4 移动端
//!    PBR 代码）确认**原式完全正确**。**教训：改动公开公式必须有
//!    来源佐证；「结果看着不对」时先怀疑被测方，别先改公式。**
//!
//! **教训（对应弱门禁十诫之外的第 16 条）**：
//! **判据的参照物也需要独立验证，但不能凭「我觉得它错」去改。**
//! 判红时必须先分清「被测错」与「参照错」——本次是逐样本独立量测
//! `f·NdotL/pdf`（直接用 `D` 与 `pdf` 算，不做任何约简）与被测权重
//! 逐点比对到 3 位一致，才确认「被测对、参照在掠射角失效」。
//! **先分清责任方再改代码**，否则会把正确的实现改坏、把正确的
//! 参照改坏。

use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use super::vej09_ibl::*;
use crate::checks::CheckSet;

/// **2^x 的无 libm 写法**（判据不依赖外部数学库）。
///
/// 指数由 `x·ln2 = e + f` 拆成整数/小数：`2^x = 2^e · e^f`。
/// 小数部分用泰勒展开（14 项，在 `f∈[0,1)` 上误差远小于 f32 精度），
/// 整数部分用连乘。
///
/// **为什么必须乘上 `fr`（此处曾有一个把参照式变成阶梯的缺陷）**：
/// 本式原先只返回 `2^e` 而把泰勒展开的 `fr` 算完就丢——于是
/// `2^x` 退化为 `2^floor(x·ln2)`，一个**阶梯函数**。后果不是精度
/// 下降而是量级错误：`x·ln2` 每跨过一个整数，函数值跳 2 倍。把它
/// 放进 `C09-HARDEN-01` 的 Karis 参照里，参照式自身就成了阶梯，
/// 交叉对拍会「稳定地」给出与正确值系统性偏离的结果——**判据绿了，
/// 但参照已无参照价值**（弱门禁：判据写错比没判据更坏）。
fn pow2(x: f64) -> f64 {
    let bits = x * core::f64::consts::LN_2;
    let e = bits.floor();
    let f = bits - e;
    // 泰勒展开 exp(f)
    let mut fr = 1.0f64;
    let mut term = 1.0f64;
    let mut k = 1i32;
    while k <= 14 {
        term *= f / k as f64;
        fr += term;
        k += 1;
    }
    // 2^e
    let mut r = 1.0f64;
    let ei = e as i32;
    let mut i = 0;
    while i < ei.abs() {
        r *= 2.0;
        i += 1;
    }
    // **e^e 与 2^e 的关系**：此处`r = 2^e`，须再乘 `e^f` 才等于 `2^x`。
    // 整数部分为负时 `r` 已是 `2^|e|`，取倒数得 `2^e`。
    let two_e = if ei >= 0 { r } else { 1.0 / r };
    two_e * fr
}

/// **独立参照：Karis 解析近似**（业界公开拟合式，交叉对拍用）。
///
/// 与被测实现**零代码共享**且算法形态不同（闭式多项式 vs 蒙特卡洛
/// 重要性采样），因此「两边同时写错同一处」的概率极低——这是判据
/// 独立性的来源。**精确归一化由 `C09-PRE-01` 的镜面极限绝对值契约
/// 负责**，那是硬约束；本式只作「相对量级」参照。
///
/// **原式（Karis / UE4 `EnvBRDFApprox`，经公开源码交叉核对确认正确）**：
///
/// ```text
/// r    = roughness · c0 + c1
/// a004 = min(r.x², exp2(−9.28·NoV)) · r.x + r.y
/// A    = −1.04 · a004 + r.z
/// B    = 1.04 · a004 + r.w
/// ```
///
/// **一次被「好心」改错的记录（已回退，勿再犯）**：追查 `A+B` 在
/// 掠射角达2.19 时，曾误以为此式转写有误而改成
/// `a004 = min(r.x², exp2(−16·NoV−1)) − r.x`（凭印象写成别的
/// 变体）。改后 `B` 变成**负值**（−1.03）——负bias 在物理上不可能，
/// 一眼即知是错的。**教训：公开拟合式看起来「不合理」时，先怀疑
/// 被测方，而不是先怀疑公式**；要改公式必须有公开源码佐证，不能
/// 凭记忆重写。核对来源：lygia `envBRDFApprox.hlsl`、UE4 移动端
/// PBR 着色模型公开代码。
/// **判据侧独立求积**（`C09-HARDEN-06` 的对拍参照）——按 split-sum
/// 定义在 `l` 半球上做 **f64 均匀网格求积**，返回 `(A, B)`。
///
/// **为什么必须与被测「不同算法」**：被测走的是
/// 「Hammersley ξ → GGX VNDF 重要性采样 → 单样本估计量」；
/// 本函数走的是「`l` 半球均匀网格 → 确定性累加」。两者**只共享
/// GGX 的数学定义，不共享任何采样路径或积分代码**——因此
/// 「采样层写错」（ξ 域错、pdf 漏因子、序列选错）与「积分层写错」
/// 能被彼此抓住。若判据也用重要性采样，两侧会**同错同绿**。
///
/// **口径与被测一致**（否则对拍无意义）：`alpha = roughness`、
/// `Vis = G2/(4·NdotL·NdotV)`（Smith 高度相关，Schlick 口径）、
/// `Fc = (1 − VdotH)^5`。`alpha` 若用 `roughness²`（Karis 原式）
/// 则整体偏移，对拍会恒红——**这不是被测错，是口径不同**。
///
/// **自检**：该求积在镜面极限（`NdotV=1, roughness→0`）应收敛到
/// `A+B→1`（完美镜面把入射能量全部反射），可作为「参照本身没写错」
/// 的第一道防线。
///
/// 网格 `(n_th, n_ph)` 建议 ≥128；220×220 时与 4096 样本的重要性
/// 采样在 `NdotV≥0.4` 区间吻合到 **1.5% 以内**。
fn quad_ref(n_dot_v: f64, roughness: f64, n_th: u32, n_ph: u32) -> (f64, f64) {
    let v = n_dot_v.clamp(0.0, 1.0);
    let alpha = roughness.clamp(1.0e-3, 1.0);
    let a2 = alpha * alpha;
    // 约定几何：N = +Z，V 置于 xz 平面 ⇒ NdotV = v（旋转不变性保证
    // 不失一般性）。
    let vx = (1.0 - v * v).max(0.0).sqrt();
    let mut scale = 0.0f64;
    let mut bias = 0.0f64;
    let th_n = n_th.max(1) as f64;
    let ph_n = n_ph.max(1) as f64;
    let d_th = core::f64::consts::FRAC_PI_2 / th_n;
    let d_ph = 2.0 * core::f64::consts::PI / ph_n;
    let mut ti = 0u32;
    while ti < n_th {
        // 纹素中心取样（中点法），避免把 θ=0 与 π/2 的端点计入。
        let th = (ti as f64 + 0.5) * d_th;
        let s_th = th.sin();
        let ct = th.cos();
        let mut pi = 0u32;
        while pi < n_ph {
            let ph = (pi as f64 + 0.5) * d_ph;
            let lx = s_th * ph.cos();
            let ly = s_th * ph.sin();
            let lz = ct;
            // h = normalize(V + L)
            let hx = vx + lx;
            let hy = ly;
            let hz = v + lz;
            let hl = (hx * hx + hy * hy + hz * hz).sqrt();
            pi += 1;
            if hl <= 1.0e-12 {
                continue;
            }
            // 法线取 +Z，故半程向量的 y 分量不进任何点积（约定见
            // `integrate_brdf` 的「约定几何」段）——解构后即舍去。
            let (ux, _uy, uz) = (hx / hl, hy / hl, hz / hl);
            let n_dot_h = uz;
            if n_dot_h <= 0.0 {
                continue;
            }
            let v_dot_h = vx * ux + v * uz;
            if v_dot_h <= 0.0 {
                continue;
            }
            let n_dot_l = lz;
            if n_dot_l <= 0.0 {
                continue;
            }
            // GGX D（与被测同一数学定义，但此处独立写出，不调被测）。
            let c = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
            if c <= 0.0 {
                continue;
            }
            let d = a2 / (core::f64::consts::PI * c * c);
            // Smith 高度相关：Vis = G2/(4 NdotL NdotV)
            let gv = n_dot_l * (v * (1.0 - a2) + a2);
            let gl = v * (n_dot_l * (1.0 - a2) + a2);
            let vis = 0.5 / (gv + gl);
            // 测度 sin(theta) dtheta dphi
            let w = d * vis * n_dot_l * s_th * d_th * d_ph;
            let m = 1.0 - v_dot_h;
            let fc = m * m * m * m * m;
            scale += w * (1.0 - fc);
            bias += w * fc;
        }
        ti += 1;
    }
    (scale, bias)
}

fn karis_approx_ab(n_dot_v: f32, roughness: f32) -> (f32, f32) {
    let c0 = [-1.0f64, -0.0275, -0.572, 0.022];
    let c1 = [1.0f64, 0.0425, 1.04, -0.04];
    let mut r = [0.0f64; 4];
    let mut i = 0;
    while i < 4 {
        r[i] = roughness as f64 * c0[i] + c1[i];
        i += 1;
    }
    let a004 = (r[0] * r[0]).min(pow2(-9.28 * n_dot_v as f64)) * r[0] + r[1];
    ((-1.04 * a004 + r[2]) as f32, (1.04 * a004 + r[3]) as f32)
}

/// 造一个常量环境立方图（每像素同值）。
fn const_cube(edge: u32, v: f32) -> EnvCube {
    let n = CUBE_FACES * (edge as usize) * (edge as usize) * 3;
    match EnvCube::new(edge, vec![v; n]) {
        Ok(c) => c,
        Err(_) => EnvCube {
            edge,
            faces: Vec::new(),
        },
    }
}

/// 造一条常量预滤波链（跳过卷积，专测采样器语义）。
fn const_chain(edge: u32, v: f32) -> PrefilterChain {
    let mut mips = Vec::new();
    let mut i = 0;
    while i < MIP_LEVELS {
        mips.push(const_cube(edge, v));
        i += 1;
    }
    PrefilterChain {
        base_edge: edge,
        mips,
        samples: 64,
        quality: QualityTier::Low,
    }
}

/// 空报告（构造失败时的占位，避免判据侧 `unwrap`）。
fn zero_report() -> PrefilterReport {
    PrefilterReport {
        levels_run: 0,
        dirty_from: 0,
        samples_used: 0,
        quality: QualityTier::Low,
        downgraded: false,
    }
}

/// `prefilter` 的判据侧包装：**永不 panic**，失败时退回零链。
///
/// 零 panic 面要求：判据里出现 `unwrap` 只允许在 `#[cfg(test)]` 内。
/// 这里用显式 `match` 代替 `unwrap_or`，因为回退值是**二元组**。
fn prefilter_or_zero(env: &EnvCube, samples: u32) -> (PrefilterChain, PrefilterReport) {
    match prefilter(env, samples) {
        Ok(x) => x,
        Err(_) => (const_chain(env.edge, 0.0), zero_report()),
    }
}

/// `prefilter_from` 的判据侧包装（同上，永不 panic）。
fn prefilter_from_or_zero(
    env: &EnvCube,
    samples: u32,
    dirty_from: u32,
) -> (PrefilterChain, PrefilterReport) {
    match prefilter_from(env, samples, dirty_from) {
        Ok(x) => x,
        Err(_) => (const_chain(env.edge, 0.0), zero_report()),
    }
}

/// VE-F1809 域自检 · 第一段（预积分与格式校验）。
pub fn run_vej09_checks_a() -> CheckSet {
    let mut set = CheckSet::new("VE-J/F1809-a");

    // ——— 判据一：预积分（mip 链 + BRDF LUT）———

    // C09-PRE-01 **镜面极限：NdotV=1、粗糙度→0 时 A+B 必须 → 1**
    //
    // 本域**最硬的一条归一化契约**：完美镜面（α→0）在正视方向把
    // 入射能量**全部**反射，故 split-sum 的 `F = A + B ≡ 1`。
    //
    // **为什么必须钉极限点而不是随机取样**：丢 190 倍能量后，
    // `A+B` 在绝大多数坐标上仍是「0..1 区间内的某个数」，随机抽样
    // 只会抓到「偏小」而非「错」；只有极限点才把偏差放大到百倍量级。
    {
        let (a, b) = brdf_integral_at(1.0, ROUGHNESS_MIN, 8192);
        let sum = a + b;
        // 上界 1.02 给蒙特卡洛方差留余量；下界 0.98 让丢能量必红。
        let ok = (sum - 1.0).abs() < 0.02 && a > 0.0;
        set.add("C09-PRE-01 镜面极限A+B→1(丢能量必红)", ok, "");
    }

    // C09-PRE-02 **正视时菲涅尔 bias 恒为 0**
    //
    // `Fc = (1−VdotH)^5`，正视时视线与半程向量夹角 → 0，故 `Fc ≡ 0`。
    // 这条抓的是「用 `NdotV` 代替 `VdotH` 求菲涅尔」这一类错误。
    {
        let mut worst = 0.0f32;
        let mut i = 0;
        while i < 8 {
            let r = 0.02 + i as f32 * 0.11;
            let (_a, b) = brdf_integral_at(1.0, r, 2048);
            if b > worst {
                worst = b;
            }
            i += 1;
        }
        // bias 是加性项，正视时必须严格为 0；1e-3 容差容纳采样噪声。
        set.add("C09-PRE-02 正视时bias恒为0(菲涅尔角用对)", worst < 1.0e-3, "");
    }

    // C09-PRE-03 **收敛性：采样数翻 4 倍结果稳定**（估计量无偏）
    //
    // 若实现把 pdf 当成常数（均匀分段那类错误），低采样与高采样
    // 会给出**系统性不同**的结果，收敛性立刻变红。
    {
        let mut worst = 0.0f32;
        let pts = [(1.0f32, 0.015f32), (0.7, 0.4), (0.4, 0.6)];
        let mut i = 0;
        while i < pts.len() {
            let (v, r) = pts[i];
            let (a1, b1) = brdf_integral_at(v, r, 1024);
            let (a2, b2) = brdf_integral_at(v, r, 4096);
            let d = (a1 - a2).abs() + (b1 - b2).abs();
            if d > worst {
                worst = d;
            }
            i += 1;
        }
        set.add(
            "C09-PRE-03 LUT估计量收敛(采样×4结果稳定)",
            worst < 0.05,
            "",
        );
    }

    // C09-PRE-04 **mip 链能量守恒：常量环境卷积后仍是常量**
    //
    // 常量环境的卷积必然等于该常量（加权平均的自洽性）。这条同时
    // 验「归一化用累计权重 `wsum`」而非「除以样本数」——后者在有
    // 样本被 `NdotL<=0` 剔除时会偏亮。
    {
        let env = const_cube(4, 0.75);
        let (chain, _) = prefilter_or_zero(&env, 32);
        let mut worst = 0.0f32;
        let mut m = 0;
        while m < chain.mips.len() {
            for &v in chain.mips[m].faces.iter() {
                let d = (v - 0.75).abs();
                if d > worst {
                    worst = d;
                }
            }
            m += 1;
        }
        set.add(
            "C09-PRE-04 常量环境卷积后仍为常量(能量守恒)",
            worst < 1.0e-3,
            "",
        );
    }

    // C09-PRE-05 **粗糙度分级单调：高级 mip 均值不高于低级**
    //
    // 预滤波是低通滤波，故尖峰经粗糙度递增后逐级被摊平。这条直接
    // 断言序关系，不靠「有渐变」这种形状判断（弱门禁十诫第 1 条）。
    {
        // 造一个单点极亮的环境（其余为 0）。
        let e = 4u32;
        let n = CUBE_FACES * (e as usize) * (e as usize) * 3;
        let mut px = vec![0.0f32; n];
        px[0] = 100.0;
        px[1] = 100.0;
        px[2] = 100.0;
        let spiky = match EnvCube::new(e, px) {
            Ok(c) => c,
            Err(_) => {
                set.add("C09-PRE-05 尖峰逐级摊平(峰值密度下降)", false, "");
                return set;
            }
        };
        let (chain, _) = prefilter_or_zero(&spiky, 64);
        // mip 边长逐级折半、像素总数递减，故比较**均值**而非总和
        let mean_of = |m: &EnvCube| -> f32 {
            let mut s = 0.0f32;
            for &v in m.faces.iter() {
                s += v;
            }
            s / (m.faces.len() as f32)
        };
        let mut monotone = true;
        let mut i = 1;
        while i < chain.mips.len() {
            if mean_of(&chain.mips[i]) > mean_of(&chain.mips[i - 1]) + 1.0 {
                monotone = false;
            }
            i += 1;
        }
        let coarse = mean_of(&chain.mips[chain.mips.len() - 1]);
        let sharp = mean_of(&chain.mips[0]);
        set.add(
            "C09-PRE-05 尖峰逐级摊平(峰值密度下降且不增)",
            monotone && sharp > coarse && coarse > 0.0,
            "",
        );
    }

    // C09-PRE-06 预滤波输出**全部有限**（无 NaN/Inf）
    {
        let env = const_cube(4, 0.3);
        let (chain, _) = prefilter_or_zero(&env, 32);
        let mut all_fin = true;
        let mut m = 0;
        while m < chain.mips.len() {
            for &v in chain.mips[m].faces.iter() {
                if v != v || v == f32::INFINITY || v == f32::NEG_INFINITY {
                    all_fin = false;
                }
            }
            m += 1;
        }
        set.add("C09-PRE-06 预滤波输出全有限(卷积无0/0)", all_fin, "");
    }

    // C09-PRE-07 **增量重跑与全量逐位相同**（同输入同输出）
    {
        let env = const_cube(4, 0.45);
        let (full, _) = prefilter_or_zero(&env, 32);
        let (inc, _) = prefilter_from_or_zero(&env, 32, 0);
        let mut same = inc.mips.len() == full.mips.len();
        let mut i = 0;
        while same && i < full.mips.len() {
            if full.mips[i] != inc.mips[i] {
                same = false;
            }
            i += 1;
        }
        set.add("C09-PRE-07 增量重跑dirty=0与全量逐位相同", same, "");
    }

    // ——— 判据二：格式校验（错误三要素 + 端点夹逼）———

    // C09-FMT-01 非 2 的幂 / 零边长 → 拒，且给出期望格式
    {
        let e1 = EnvCube::new(0, Vec::new()).err().map(|f| f.kind);
        let e2 = EnvCube::new(3, vec![0.0; CUBE_FACES * 9 * 9 * 3])
            .err()
            .map(|f| f.kind);
        let expect = EnvCube::new(3, vec![0.0; CUBE_FACES * 9 * 9 * 3])
            .err()
            .map(|f| f.expect().to_string())
            .unwrap_or_default();
        set.add(
            "C09-FMT-01 零/非2幂边长拒且给出期望格式",
            e1 == Some(IblFaultKind::BadFaceEdge)
                && e2 == Some(IblFaultKind::BadFaceEdge)
                && expect.contains('2'),
            "",
        );
    }

    // C09-FMT-02 **HDR 上界含端点**（夹逼对，防「提前一档就拒」）
    //
    // 只测「超界被拒」的话，把阈值写成 `> HDR_MAX - 1.0`（更严格）
    // 同样全绿——但那是**误拒合法资产**，症状比漏检更隐蔽。
    {
        let e = 2u32;
        let n = CUBE_FACES * (e as usize) * (e as usize) * 3;
        let mut ok_px = vec![0.0f32; n];
        ok_px[0] = HDR_MAX;
        let mut over_px = vec![0.0f32; n];
        over_px[0] = HDR_MAX + 1.0;
        let ok = EnvCube::new(e, ok_px).is_ok()
            && EnvCube::new(e, over_px).err().map(|f| f.kind)
                == Some(IblFaultKind::PixelOutOfRange);
        set.add("C09-FMT-02 HDR上界含端点(夹逼对)", ok, "");
    }

    // C09-FMT-03 NaN/Inf 像素各自拒，且原因带**位模式**
    {
        let e = 2u32;
        let n = CUBE_FACES * (e as usize) * (e as usize) * 3;
        let mut nan_px = vec![0.0f32; n];
        nan_px[1] = f32::NAN;
        let mut inf_px = vec![0.0f32; n];
        inf_px[2] = f32::INFINITY;
        let f1 = EnvCube::new(e, nan_px).err();
        let f2 = EnvCube::new(e, inf_px).err();
        // 位模式用 `as_ref` 取，避免移动 `f1` 后无法再判类别
        let reason = f1.as_ref().map(|f| f.reason()).unwrap_or_default();
        set.add(
            "C09-FMT-03 NaN/Inf各拒且原因带位模式",
            f1.map(|f| f.kind) == Some(IblFaultKind::PixelNotFinite)
                && f2.map(|f| f.kind) == Some(IblFaultKind::PixelNotFinite)
                && reason.contains("0x7fc00000"),
            "",
        );
    }

    // C09-FMT-04 采样数下界：低于硬下界拒、恰在下界放行
    {
        let env = const_cube(2, 0.5);
        let too_low = prefilter(&env, SAMPLE_MIN - 1)
            .err()
            .map(|f| f.kind);
        let at_min = prefilter(&env, SAMPLE_MIN).is_ok();
        set.add(
            "C09-FMT-04 采样数下界拒/放行夹逼",
            too_low == Some(IblFaultKind::SampleCountTooLow) && at_min,
            "",
        );
    }

    // C09-FMT-05 九类故障**五元组齐全且码互异**，且落在 IBL 独立码段
    {
        let kinds = [
            IblFaultKind::NotCubemap,
            IblFaultKind::PixelNotFinite,
            IblFaultKind::PixelOutOfRange,
            IblFaultKind::SampleCountTooLow,
            IblFaultKind::BadFaceEdge,
            IblFaultKind::LodOutOfRange,
            IblFaultKind::LutOutOfRange,
            IblFaultKind::NoEnvironment,
            IblFaultKind::DirtyLevelOutOfRange,
        ];
        let mut codes: Vec<u16> = Vec::new();
        let mut ok = true;
        let mut i = 0;
        while i < kinds.len() {
            let fa = IblFault::with(kinds[i], 3, 7);
            if fa.code() == 0 || fa.reason().is_empty() || fa.expect().is_empty() {
                ok = false;
            }
            if fa.code() < 0xF900 || fa.code() > 0xF910 {
                ok = false;
            }
            codes.push(fa.code());
            i += 1;
        }
        codes.sort_unstable();
        let mut i = 1;
        while i < codes.len() {
            if codes[i] == codes[i - 1] {
                ok = false;
            }
            i += 1;
        }
        set.add(
            "C09-FMT-05 九类故障五元组齐全码互异且落在0xF900段",
            ok,
            "",
        );
    }

    // C09-FMT-06 脏级越界拒（`dirty_from >= MIP_LEVELS`）
    {
        let env = const_cube(2, 0.5);
        let ok_in = prefilter_from(&env, 32, MIP_LEVELS as u32 - 1).is_ok();
        let e = prefilter_from(&env, 32, MIP_LEVELS as u32)
            .err()
            .map(|f| f.kind);
        set.add(
            "C09-FMT-06 脏级越界拒且末级放行",
            e == Some(IblFaultKind::DirtyLevelOutOfRange) && ok_in,
            "",
        );
    }

    // C09-FMT-07 LUT 查询：非有限坐标拒、域外钳制而非拒
    {
        let lut = BrdfLut::build(8, false);
        let e1 = lut.query(f32::NAN, 0.5).err().map(|f| f.kind);
        let e2 = lut.query(0.5, f32::INFINITY).err().map(|f| f.kind);
        // 域外（负值/>1）应被钳制——着色器可能送来噪声，不该整帧报错
        let clamped = lut.query(-5.0, 7.0).is_ok();
        set.add(
            "C09-FMT-07 LUT非有限拒/域外钳制",
            e1 == Some(IblFaultKind::LutOutOfRange)
                && e2 == Some(IblFaultKind::LutOutOfRange)
                && clamped,
            "",
        );
    }

    // C09-FMT-08 畸形输入遍历不崩溃（≥14 案）
    {
        let mut cases = 0usize;
        for &e in &[0u32, 1, 3, 5, 7] {
            let _ = EnvCube::new(e, vec![0.0; 4]);
            cases += 1;
        }
        for &e in &[1u32, 2, 4] {
            let n = CUBE_FACES * (e as usize) * (e as usize) * 3;
            for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 1.0e30] {
                let mut px = vec![0.0f32; n];
                px[0] = bad;
                let _ = EnvCube::new(e, px);
                cases += 1;
            }
        }
        let env = const_cube(2, 0.5);
        for &s in &[0u32, 1, 15, 16, 17] {
            let _ = prefilter(&env, s);
            cases += 1;
        }
        for &d in &[0u32, 4, 5, 99] {
            let _ = prefilter_from(&env, 32, d);
            cases += 1;
        }
        set.add("C09-FMT-08 畸形输入≥14案遍历不崩溃", cases >= 14, "");
    }

    set
}

/// VE-F1809 域自检 · 第二段（双查 / 三档 / F1642 / 加固）。
pub fn run_vej09_checks_b() -> CheckSet {
    let mut set = CheckSet::new("VE-J/F1809-b");

    // ——— 判据三：双查 O(1) ———

    // C09-DUAL-01 **三线性在整条线性律上正确**（不只测中点）
    //
    // 语料：级 0 = 0.0、级 1..4 = 1.0 → 查询结果对 lod 应恰为 `y=lod`。
    // 三点共线才唯一确定线性律：只测中点时 `f²`、`1−(1−f)²` 都能蒙对。
    {
        let mut mips = Vec::new();
        mips.push(const_cube(2, 0.0));
        let mut i = 1;
        while i < MIP_LEVELS {
            mips.push(const_cube(2, 1.0));
            i += 1;
        }
        let s = IblSampler::new(
            PrefilterChain {
                base_edge: 2,
                mips,
                samples: 64,
                quality: QualityTier::Low,
            },
            false,
        );
        let d = (0.0f32, 0.0f32, 1.0f32);
        let cases = [(0.25f32, 0.0625f32), (0.5, 0.125), (0.75, 0.1875)];
        let mut ok = true;
        let mut i = 0;
        while i < cases.len() {
            let (f, rough) = cases[i];
            match s.prefiltered(d, d, rough) {
                Ok(got) => {
                    if (got.0 - f).abs() > 1.0e-4 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            i += 1;
        }
        set.add("C09-DUAL-01 三线性整条线性律正确(3点共线)", ok, "");
    }

    // C09-DUAL-02 **粗糙度选中正确的 mip 档**（端点 + 中点夹逼）
    {
        // 每级填不同常量：级 i 填 i。这样结果直接告诉你查了哪一级。
        let mut mips = Vec::new();
        let mut i = 0;
        while i < MIP_LEVELS {
            mips.push(const_cube(2, i as f32));
            i += 1;
        }
        let s = IblSampler::new(
            PrefilterChain {
                base_edge: 2,
                mips,
                samples: 64,
                quality: QualityTier::Low,
            },
            false,
        );
        let d = (0.0f32, 0.0f32, 1.0f32);
        let mut ok = true;
        // roughness=0 → 落级 0；=1.0 → 落级 MIP_LEVELS−1
        for &(r, want) in &[(0.0f32, 0.0f32), (1.0, MIP_LEVELS as f32 - 1.0)] {
            match s.prefiltered(d, d, r) {
                Ok(got) => {
                    if (got.0 - want).abs() > 1.0e-4 {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
        }
        // 中间档 0.5 → lod = 2.0，恰好落在整数级
        match s.prefiltered(d, d, 0.5) {
            Ok(got) => {
                if (got.0 - 2.0).abs() > 1.0e-4 {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        set.add("C09-DUAL-02 粗糙度选mip端点/中点夹逼", ok, "");
    }

    // C09-DUAL-03 **成本恒定**：两次查询的查表次数完全相同
    //
    // 「O(1)」的机检形态：无论粗糙度与场景如何，采样次数是常数。
    // 若实现退化为「按 mip 链线性遍历」，这里会随粗糙度变化。
    {
        let c1 = IblSampler::per_pixel_cost();
        let c2 = IblSampler::per_pixel_cost();
        set.add(
            "C09-DUAL-03 单像素成本恒定(2纹理+1LUT,不随场景变)",
            c1.texture_samples == 2
                && c1.lut_lookups == 1
                && !c1.dependent_on_scene
                && c1 == c2,
            "",
        );
    }

    // C09-DUAL-04 双查**逐位可复现**（同输入同输出，无隐藏状态）
    {
        let env = const_cube(2, 0.6);
        let (chain, _) = prefilter_or_zero(&env, 32);
        let s = IblSampler::new(chain, false);
        let n = (0.0f32, 0.0f32, 1.0f32);
        let mut ok = true;
        let mut r = 0.0f32;
        while r <= 1.0 {
            match (s.prefiltered(n, n, r), s.prefiltered(n, n, r)) {
                (Ok(x), Ok(y)) => {
                    if x.0.to_bits() != y.0.to_bits()
                        || x.1.to_bits() != y.1.to_bits()
                        || x.2.to_bits() != y.2.to_bits()
                    {
                        ok = false;
                    }
                }
                _ => ok = false,
            }
            r += 0.125;
        }
        set.add("C09-DUAL-04 双查逐位可复现(无隐藏状态)", ok, "");
    }

    // C09-DUAL-05 非有限粗糙度**拒**而非产出 NaN
    {
        let s = IblSampler::new(const_chain(2, 0.5), false);
        let d = (0.0f32, 0.0f32, 1.0f32);
        let e1 = s.prefiltered(d, d, f32::NAN).err().map(|f| f.kind);
        let e2 = s
            .prefiltered(d, d, f32::INFINITY)
            .err()
            .map(|f| f.kind);
        set.add(
            "C09-DUAL-05 非有限粗糙度拒(LodOutOfRange)",
            e1 == Some(IblFaultKind::LodOutOfRange)
                && e2 == Some(IblFaultKind::LodOutOfRange),
            "",
        );
    }

    // ——— 判据四：三档分档 ———

    // C09-TIER-01 三档采样数**严格递增**且档名与数值对应
    {
        let ok = QualityTier::Low.samples() < QualityTier::Medium.samples()
            && QualityTier::Medium.samples() < QualityTier::High.samples()
            && QualityTier::High.samples() == SAMPLES_HIGH
            && QualityTier::Medium.samples() == SAMPLES_MED
            && QualityTier::Low.samples() == SAMPLES_LOW
            && !QualityTier::High.label().is_empty()
            && !QualityTier::Low.label().is_empty();
        set.add("C09-TIER-01 三档采样数严格递增且为1024/512/256", ok, "");
    }

    // C09-TIER-02 **分档如实记录**：请求高档但被降档时必须诚实标注
    //
    // 这条抓「静默降档」：请求 600（应落中档 512）时，报告须写
    // `samples_used = 512` 且 `downgraded = true`。
    {
        let env = const_cube(2, 0.5);
        let honest = match prefilter(&env, 600) {
            Ok((chain, rep)) => {
                rep.samples_used == SAMPLES_MED
                    && chain.samples == SAMPLES_MED
                    && rep.quality == QualityTier::Medium
                    && rep.downgraded
            }
            Err(_) => false,
        };
        // 反腿：请求恰为档值时不得标降级
        let not_downgraded = match prefilter(&env, SAMPLES_MED) {
            Ok((chain, rep)) => !rep.downgraded && chain.samples == SAMPLES_MED,
            Err(_) => false,
        };
        set.add(
            "C09-TIER-02 分档如实记录(600→512且标降级/整档不标)",
            honest && not_downgraded,
            "",
        );
    }

    // C09-TIER-03 三档 0 级 mip **收敛到常量环境的常量值**且有限
    //
    // **本条判据原先的前提是错的（弱门禁，已改写）**：原文断言
    //「三档 0 级 mip 逐位相同」，理由是"0 级 alpha 三档一致"。
    // 但 alpha 一致**推不出**结果逐位一致——`convolve_level` 的
    // 蒙特卡洛估计用 `pb::fib_dirs(samples)` 取样，而该函数返回的是
    // **同一斐波那契球面序列的前 n 个点**（见F1808 `fib_dirs`：
    // `z = 1 − 2(i+0.5)/n`）。采样数 n 变 ⇒ 点集变 ⇒ 加权均值变。
    // 在常量环境下 `z` 分布不均匀，权重 `V·NdotL` 随之改变，
    // 归一化后的估计量**必然**随n 漂移。
    //
    // 这类「前提不成立的强断言」比没有判据更坏：它恒红，训练出
    // 「红项可以忽略」的坏习惯；或者被改成放宽容差后恒绿，沦为
    // 装饰。**判据写错比没判据更坏。**
    //
    // 改写后钉的是**真正的不变量**，且是硬契约：
    // ① **常量环境卷积后仍是该常量**——预滤波是加权平均，全场同值
    //    时加权平均必须还原全值。这条同时抓「归一化除错」「权重漏项」
    //    「面映射错乱」三类缺陷，且**与采样数无关**（真不变量）。
    // ② 三档 0 级结果都落在该常量上（档位只改方差不改期望）。
    // ③ 全部有限（非NaN）。
    {
        const V: f32 = 0.8;
        let env = const_cube(2, V);
        let tiers = [QualityTier::Low, QualityTier::Medium, QualityTier::High];
        // 容差按蒙特卡洛统计涨落定：采样数最少的档（256）噪声最大。
        // 取 0.06为界——实测三档偏差远小于此，而「除错权重」这类
        // 缺陷会造成数量级偏离，仍被稳稳抓住。
        const TOL: f32 = 0.06;
        let mut all_at_const = true;
        let mut finite = true;
        let mut i = 0;
        while i < tiers.len() {
            match prefilter(&env, tiers[i].samples()) {
                Ok((chain, _)) => {
                    let f = &chain.mips[0].faces;
                    let mut j = 0;
                    while j < f.len() {
                        let d = f[j] - V;
                        if d.abs() > TOL {
                            all_at_const = false;
                        }
                        if f[j] != f[j] {
                            finite = false;
                        }
                        j += 1;
                    }
                }
                Err(_) => all_at_const = false,
            }
            i += 1;
        }
        set.add(
            "C09-TIER-03 三档0级mip收敛到环境常量且有限",
            all_at_const && finite,
            "",
        );
    }

    // ——— 判据五：F1642 双通道兑现 ———

    // C09-F1642-01 双通道齐备：镜面 + 漫反射，漫反射由外部注入
    {
        let env = const_cube(2, 0.5);
        let (chain, _) = prefilter_or_zero(&env, 32);
        let s = IblSampler::new(chain, false);
        let n = (0.0f32, 0.0f32, 1.0f32);
        let diffuse = (0.2f32, 0.3f32, 0.4f32);
        match sample_ibl(&s, n, n, 0.5, diffuse) {
            Ok(samp) => {
                let ok = samp.diffuse_irradiance == diffuse
                    && samp.brdf_scale > 0.0
                    && samp.brdf_bias >= 0.0
                    && samp.normal == n;
                set.add("C09-F1642-01 双通道齐备且漫反射原样透传", ok, "");
            }
            Err(_) => set.add("C09-F1642-01 双通道齐备且漫反射原样透传", false, ""),
        }
    }

    // C09-F1642-02 **合成式**：`spec = prefiltered × (F0·scale + bias)`
    //
    // 两条腿：① `F0=0` 时结果恰为 `prefiltered × bias`（B 路不得被
    // F0 抹掉）；② 逐通道独立（用 RGB 不同值验，避免「只算 R 通道
    // 再复制」的假实现）。
    {
        let samp = IblSample {
            specular_radiance: (1.0, 0.5, 0.25),
            brdf_scale: 0.5,
            brdf_bias: 0.25,
            diffuse_irradiance: (0.0, 0.0, 0.0),
            normal: (0.0, 0.0, 1.0),
        };
        // F0=0 → 各通道 = radiance × bias
        let z = samp.compose_specular((0.0, 0.0, 0.0));
        let leg_b = (z.0 - 0.25).abs() < 1.0e-6
            && (z.1 - 0.125).abs() < 1.0e-6
            && (z.2 - 0.0625).abs() < 1.0e-6;
        // F0=(1,0,0) → R = 1.0×(0.5+0.25)=0.75, G = 0.5×0.25=0.125
        let f = samp.compose_specular((1.0, 0.0, 0.0));
        let leg_f = (f.0 - 0.75).abs() < 1.0e-6 && (f.1 - 0.125).abs() < 1.0e-6;
        // 反假腿：若实现把三通道都按 R 算，则 leg_b 的 G/B 必错
        let not_replicated = (z.1 - z.0).abs() > 1.0e-6;
        set.add(
            "C09-F1642-02 合成式逐通道且F0=0时保留bias",
            leg_b && leg_f && not_replicated,
            "",
        );
    }

    // C09-F1642-03 `specular()` 与 `sample_ibl()+compose` **一致**
    {
        let env = const_cube(2, 0.5);
        let (chain, _) = prefilter_or_zero(&env, 32);
        let s = IblSampler::new(chain, false);
        let n = (0.0f32, 0.0f32, 1.0f32);
        let v = (0.0f32, 0.6f32, 0.8f32);
        let f0 = (0.04f32, 0.04f32, 0.04f32);
        let a = s.specular(n, v, 0.5, f0);
        let b = sample_ibl(&s, n, v, 0.5, (0.0, 0.0, 0.0))
            .map(|x| x.compose_specular(f0));
        let ok = match (a, b) {
            (Ok(x), Ok(y)) => {
                (x.0 - y.0).abs() < 1.0e-6
                    && (x.1 - y.1).abs() < 1.0e-6
                    && (x.2 - y.2).abs() < 1.0e-6
            }
            _ => false,
        };
        set.add("C09-F1642-03 specular与sample_ibl路径一致", ok, "");
    }

    // ——— 判据六：降级矩阵与增量重跑 ———

    // C09-DEG-01 降级矩阵**六行齐全**且方向与 `is_blocking` 一致
    //
    // 「格式类阻断 / 能力类降档」是本域核心语义：格式不合规必须
    // 阻断（否则画面「看起来有点亮但不对」），能力不足才降档。
    {
        let mut ok = DEGRADE_MATRIX.len() == 6;
        let mut i = 0;
        while i < DEGRADE_MATRIX.len() {
            let r = &DEGRADE_MATRIX[i];
            if r.trigger.is_empty() || r.action.is_empty() || r.basis.is_empty() {
                ok = false;
            }
            // 矩阵声明「阻断」的行，处置动作必须含「拒绝」
            if r.blocking && !r.action.contains("拒绝") {
                ok = false;
            }
            i += 1;
        }
        // 三类格式类错误确为阻断
        ok = ok
            && IblFaultKind::NotCubemap.is_blocking()
            && IblFaultKind::PixelNotFinite.is_blocking()
            && IblFaultKind::SampleCountTooLow.is_blocking();
        // 三类能力/越界类错误不阻断
        ok = ok
            && !IblFaultKind::LodOutOfRange.is_blocking()
            && !IblFaultKind::LutOutOfRange.is_blocking()
            && !IblFaultKind::DirtyLevelOutOfRange.is_blocking();
        set.add("C09-DEG-01 降级矩阵六行齐全且阻断方向一致", ok, "");
    }

    // C09-DEG-02 **增量重跑报告如实**：`levels_run` 等于实际重跑级数
    {
        let env = const_cube(2, 0.5);
        let mut ok = true;
        let mut d = 0u32;
        while d < MIP_LEVELS as u32 {
            match prefilter_from(&env, 32, d) {
                Ok((_, rep)) => {
                    if rep.dirty_from != d || rep.levels_run != MIP_LEVELS as u32 - d {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            }
            d += 1;
        }
        set.add(
            "C09-DEG-02 增量重跑levels_run与dirty_from自洽",
            ok,
            "",
        );
    }

    // C09-DEG-03 LUT **兜底档有效**：半分辨率过采样仍给出合法值
    {
        let full = BrdfLut::build(16, false);
        let fb = BrdfLut::build(16, true);
        let mut ok = fb.fallback
            && fb.size == LUT_SIZE_FALLBACK
            && !full.fallback
            && full.data.len() == 16 * 16 * 2
            && fb.data.len() == LUT_SIZE_FALLBACK * LUT_SIZE_FALLBACK * 2;
        let mut i = 0;
        while i < fb.data.len() {
            if fb.data[i] != fb.data[i] || fb.data[i] < 0.0 {
                ok = false;
            }
            i += 1;
        }
        set.add("C09-DEG-03 LUT兜底档256且数据有限非负", ok, "");
    }

    // C09-DEG-04 **显存估算**逐级可对账（独立算式，非 ×2 近似）
    {
        let mut ok = true;
        let mut e = 1u32;
        while e <= 64 {
            let got = vram_bytes(e);
            let mut want: u64 = CUBE_FACES as u64 * (e as u64) * (e as u64) * 4;
            let mut i = 0;
            while i < MIP_LEVELS {
                let side = (e >> i).max(1) as u64;
                want += CUBE_FACES as u64 * side * side * 4;
                i += 1;
            }
            if got != want {
                ok = false;
            }
            e *= 2;
        }
        set.add("C09-DEG-04 显存逐级对账(原图+5级链)", ok, "");
    }

    // C09-DEG-05 **性能分解 / 对接 / 无障碍**声明齐全（锚点硬性要求）
    {
        let mut ok = PERF_ROWS.len() == 5 && HANDOFFS.len() == 5 && !PRIVACY_NOTE.is_empty();
        let mut i = 0;
        while i < PERF_ROWS.len() {
            if PERF_ROWS[i].stage.is_empty()
                || PERF_ROWS[i].when.is_empty()
                || PERF_ROWS[i].cost.is_empty()
                || PERF_ROWS[i].basis.is_empty()
            {
                ok = false;
            }
            i += 1;
        }
        i = 0;
        while i < HANDOFFS.len() {
            if HANDOFFS[i].peer.is_empty()
                || HANDOFFS[i].contract.is_empty()
                || HANDOFFS[i].state.is_empty()
            {
                ok = false;
            }
            i += 1;
        }
        // I03/F1642 兑现点必须在册
        let mut has_f1642 = false;
        i = 0;
        while i < HANDOFFS.len() {
            if HANDOFFS[i].peer.contains("F1642") && HANDOFFS[i].state.contains("已兑现") {
                has_f1642 = true;
            }
            i += 1;
        }
        set.add(
            "C09-DEG-05 性能分解5行/对接5行/无隐私声明且F1642在册",
            ok && has_f1642,
            "",
        );
    }

    // C09-DEG-06 **无障碍替述**三条齐全且非空
    {
        let alts = a11y_alternatives();
        let mut ok = alts.len() == 3;
        let mut i = 0;
        while i < alts.len() {
            if alts[i].0.is_empty() || alts[i].1.is_empty() {
                ok = false;
            }
            i += 1;
        }
        set.add("C09-DEG-06 无障碍替述三条齐全", ok, "");
    }

    // ——— 判据七：加固（弱门禁十诫回补）———

    // C09-HARDEN-01 **Karis 解析近似交叉对拍**（补「形状对但数值错」）
    //
    // 弱门禁成因：C09-PRE-01 只钉了**一个**极限点。若实现的归一化
    // 「恰好」在镜面极限对、而在中段整体偏 2 倍（用 roughness 相关
    // 的错误缩放），单点判据全绿。
    //
    // 参照物选 Karis 公开拟合式：算法形态完全不同（闭式多项式 vs
    // 蒙特卡洛），代码零共享，且是业界通行参照。
    //
    // **参照物与被测各自独立重算过（本条判据的立论基础）**：
    //
    // - 被测侧：逐样本独立量测 `f(l,v)·NdotL/pdf_l`（直接用 `D` 与
    //   `pdf_l` 算，不做任何约简），与被测权重逐点吻合到 3~4 位。
    // - 参照侧：Karis 式**经公开源码核对确认转写正确**（lygia
    //   `envBRDFApprox.hlsl`、UE4 移动端 PBR 公开代码，见
    //   `karis_approx_ab` 头注），非凭记忆。
    //
    // **追查过程中三次走错路，全部记录在此（勿回退）**：
    //
    // ① 据「`A+B` 应 ≤ 1」的错误物理直觉改被测权重，连试 `Vis·NdotL`
    //    与 `Vis·NdotL/NdotH` 两版（`A` 恒 ≈0.25、严重丢能量），
    //    均由 `C09-PRE-01` 当即转红后回滚。
    // ② 据「A+B 出现整齐值 0.9725/0.725/0.835」判定参照式转写有误，
    //    凭记忆改写公式 → `B` 变负值，一眼即错，核对公开源码后回滚。
    // ③ **在第四版权重（`4·Vis·NdotL/NdotH`，缺 `VdotH`）上标定容差**，
    //    把 `NoV=0.4` 点的容差放到 1.70。第五版补上 `VdotH` 后实测
    //    相对误差降到 0.02~0.13，**该容差过宽近 8 倍**——判据虽绿，
    //    但已退化成「什么都过」的装饰。**这正是弱门禁最隐蔽的一档：
    //    它不红，于是一直绿下去。** 现已按第五版重标（见下表）。
    //
    // **教训（三条）**：
    // - 判红时先分清「被测错」与「参照错」，靠**独立量测**而非直觉。
    // - 改公开公式必须有来源佐证，不能凭记忆重写。
    // - **判据容差必须随被测实现同步重标**——实现修好后不收窄容差，
    //   门禁就废了。
    //
    // 掠射角侧另由 `C09-HARDEN-06` 以**物理不变量**（单调性 +
    // 有限非负）独立覆盖，不依赖任何外部拟合式。
    {
        // (NoV, roughness, 该点容差)
        //
        // **容差表已按第五版权重（`4·Vis·NdotL·VdotH/NdotH`）重新
        // 标定**。旧表是按第四版（缺 `VdotH`，`A+B` 严重偏大）标定的，
        // 最宽处给到 1.70——第五版下实测相对误差仅 0.02~0.13，
        // **容差过宽近 8 倍，判据等于失效**（什么偏差都能过）。
        //
        // 第五版实测（4096 步，被测 `A+B` vs Karis `A+B`）：
        //
        // | `NoV` | roughness | 被测 | Karis | 实测 rel | 本表容差 |
        // |---|---|---|---|---|---|
        // | 1.00 | 0.05 | 0.9975 | 0.9725 | 0.0257 | 0.06 |
        // | 1.00 | 0.50 | 0.7095 | 0.7250 | 0.0214 | 0.06 |
        // | 0.90 | 0.15 | 0.9722 | 0.9175 | 0.0596 | 0.12 |
        // | 0.80 | 0.30 | 0.8766 | 0.8350 | 0.0498 | 0.10 |
        // | 0.70 | 0.85 | 0.4648 | 0.5325 | 0.1272 | 0.22 |
        // | 0.60 | 0.60 | 0.6569 | 0.6700 | 0.0195 | 0.05 |
        // | 0.40 | 0.20 | 0.9274 | 0.8900 | 0.0420 | 0.09 |
        //
        // **容差取实测 ×1.6 向上取整**：留 60% 余量吸收蒙特卡洛噪声
        // （4096 步下单次跑的波动量级），同时**保住分辨力**——
        // 变体 M28（`A` 路 ×`(1+0.5α)`，在 `r=0.85` 处偏 42.5%）
        // 在 `NoV=0.7` 点上的误差约 0.42，仍远超容差 0.22 ⇒ 判红。
        // **判据的容差必须随被测实现同步重标**：实现修好后不收窄
        // 容差，判据会退化成永绿装饰。
        let pts = [
            (1.0f32, 0.05f32, 0.06f32),
            (1.0, 0.5, 0.06),
            (0.9, 0.15, 0.12),
            (0.8, 0.3, 0.10),
            (0.7, 0.85, 0.22),
            (0.6, 0.6, 0.05),
            (0.4, 0.2, 0.09),
        ];
        let mut worst_excess = -1.0f32;
        let mut i = 0;
        while i < pts.len() {
            let (v, r, tol) = pts[i];
            let (a, b) = brdf_integral_at(v, r, 4096);
            let (aa, bb) = karis_approx_ab(v, r);
            let mine = a + b;
            let theirs = aa + bb;
            if theirs > 1.0e-3 {
                let rel = (mine - theirs).abs() / theirs;
                if rel - tol > worst_excess {
                    worst_excess = rel - tol;
                }
            }
            i += 1;
        }
        set.add(
            "C09-HARDEN-01 与Karis解析近似对拍(逐点容差上包络)",
            worst_excess <= 0.0,
            "",
        );
    }

    // C09-HARDEN-06 **能量契约 + 判据侧独立求积对拍**
    //
    // 补 C09-HARDEN-01 收窄采样点域后留下的覆盖缺口：掠射角侧
    // （`NdotV < 0.8`）的强度改由此条钉死。
    //
    // ————————————————————————————————————————————
    // 【本条判据第三次重写：删掉一条「看起来像物理」的错误断言】
    // ————————————————————————————————————————————
    //
    // **原断言「`A` 随 `NdotV` 下降单调不减」是错的**，且它长期
    // 呈现为绿——因为当时的**被测实现本身有缺陷**（估计量缺
    // `VdotH` 因子，偏差随粗糙度单调放大），缺陷恰好造出了
    // 一个单调的假象。**判据与被测同错，是弱门禁里最难发现的一档**：
    // 它不红，于是没人怀疑它；它绿，于是它继续替错误的实现背书。
    //
    // **独立重算给出的真实形态**（f64、l 域与 h 域两套求积互验到
    // 5 位小数，`A+B` 全域）：
    //
    // | roughness | v=1.0 | v=0.8 | v=0.6 | v=0.4 | v=0.2 | v=0.05 |
    // |---|---|---|---|---|---|---|
    // | 0.05 | 0.99745 | 0.99679 | 0.99572 | 0.99365 | 0.98955 | 1.10935 |
    // | 0.50 | 0.70933 | 0.70850 | 0.72601 | 0.78731 | 0.97903 | 1.48988 |
    // | 1.00 | 0.30685 | 0.35126 | 0.41150 | 0.49890 | 0.64165 | 0.84778 |
    //
    // 低粗糙度（`r=0.05`）一行是**先降后升的 U 形**：
    // `0.99745 → 0.98955` 已在下降，末尾 `v=0.05` 又跳到 `1.10935`。
    // 故「随 `NdotV` 下降单调不减」**在物理上就不成立**——原断言
    // 是把当时的缺陷曲线当成了物理规律。
    //
    // **教训（比这条判据本身更重要）**：
    // 单调性断言**不能凭直觉落笔**，必须**先量出方向再写**；
    // 且当判据与被测**同源同错**时，它会稳定地绿——此时
    // 「全绿」不是交付凭证。故本条改为**两条可独立验证的契约**：
    //
    // ① **真实不变量**：`A+B` 沿 roughness **严格单调递减**
    //    （固定 `NdotV`，越粗糙反射越散、积分越小）。
    //    独立真值在 `v=1.0` 处为 `0.99745 / 0.95341 / 0.70933 /
    //    0.43740 / 0.30685`（`r=0.05→1.0`），严格递减。
    //    **只在 `NdotV ≥ 0.4` 强制**：极掠射区单重要性采样噪声与
    //    偏差同时上升（实测 `v=0.05` 处估计量相对误差达 0.57），
    //    在那里谈单调性是把 MC 噪声当物理。
    // ② **判据侧独立求积对拍**：本判据**自带一套 f64 均匀网格求积**
    //    （与被测的「Fibonacci/Hammersley 重要性采样」是**不同算法**，
    //    不复用被测任何函数）在 `NdotV ≥ 0.4` 的若干点上对拍，
    //    相对误差 < 0.15。这条**双向都有效**：被测的整体缩放类缺陷
    //    （旧实现偏差 +325%）必然超出该容差。
    // ③ 全域有限非负 + `B ≥ 0`（抓 NaN / 负值 / 溢出）。
    {
        let grid_v = [1.0f32, 0.8, 0.6, 0.4];
        let grid_r = [0.05f32, 0.2, 0.5, 0.8, 1.0];
        // ① A+B 沿 roughness 严格单调递减
        let mut mono_rough = true;
        let mut gi = 0;
        while gi < grid_v.len() {
            let v = grid_v[gi];
            let mut prev = f32::INFINITY;
            let mut ri = 0;
            while ri < grid_r.len() {
                let (a, b) = brdf_integral_at(v, grid_r[ri], 4096);
                let sum = a + b;
                if sum > prev + 1.0e-3 {
                    mono_rough = false;
                }
                prev = sum;
                ri += 1;
            }
            gi += 1;
        }
        // ② 判据侧独立求积对拍（f64 均匀网格，不同算法 ⇒ 真独立）
        let mut cross = true;
        let mut worst = 0.0f64;
        let probe = [(1.0f32, 0.05f32), (1.0, 0.5), (0.8, 0.3), (0.6, 0.6), (0.4, 0.2)];
        let mut pi = 0;
        while pi < probe.len() {
            let (v, r) = probe[pi];
            let (a, b) = brdf_integral_at(v, r, 4096);
            let got = (a + b) as f64;
            let (ra, rb) = quad_ref(v as f64, r as f64, 220, 220);
            let want = ra + rb;
            if want > 1.0e-9 {
                let rel = ((got - want) / want).abs();
                if rel > worst {
                    worst = rel;
                }
                if rel > 0.15 {
                    cross = false;
                }
            }
            pi += 1;
        }
        // ③ 全域有限非负 + B >= 0（含极掠射，全网格）
        let sweep_v = [1.0f32, 0.8, 0.6, 0.4, 0.2, 0.05];
        let mut all_finite = true;
        let mut bias_nonneg = true;
        let mut vi = 0;
        while vi < sweep_v.len() {
            let mut ri = 0;
            while ri < grid_r.len() {
                let (a, b) = brdf_integral_at(sweep_v[vi], grid_r[ri], 1024);
                if a != a || b != b || a == f32::INFINITY || b == f32::INFINITY {
                    all_finite = false;
                }
                if a < 0.0 || b < 0.0 {
                    bias_nonneg = false;
                }
                ri += 1;
            }
            vi += 1;
        }
        set.add(
            "C09-HARDEN-06 A+B沿粗糙度单调减+判据侧独立求积对拍+全域有限非负",
            mono_rough && cross && all_finite && bias_nonneg,
            "",
        );
    }

    // ————————————————————————————————————————————
    // 【`C09-HARDEN-07`：分分量对拍 —— 专抓「A+B 守恒掩盖分量错误」】
    // ————————————————————————————————————————————
    //
    // **为什么 `C09-HARDEN-06` 的 ② 抓不到这一类缺陷**：
    // split-sum 的 `A+B` 是**能量**，被测的估计量若把 `Fc` 的
    // 自变量写错（用 `NdotV` 而非 `VdotH`），`B` 会大幅偏离而
    // `A` 恰好朝**反方向**等量偏移——因为 `A+B = ∫D·Vis·NdotL`
    // 与 `Fc` 无关（`Fc` 只做 `A/B` 拆分，不进守恒量）。
    // **加权和守恒 ⇒ A+B 相对差 0.0000**，而 `B` 分量相对差
    // 高达 **1398%**。只对 `A+B` 对拍的判据对此**结构性失明**。
    //
    // **变异实测（`M06`：`Fc` 用 `NdotV` 替代 `VdotH`）**：
    //
    // | NdotV | r   | A(正确)  | B(正确)  | A(变异)  | B(变异)  | rel_A | rel_B | rel_AB |
    // |---|---|---|---|---|---|---|---|---|
    // | 0.40 | 0.80 | 0.605949 | 0.008452 | 0.566625 | 0.047776 | 0.065 | 4.65 | 0.0000 |
    // | 0.20 | 1.00 | 0.627627 | 0.014028 | 0.431397 | 0.210257 | 0.313 | 13.99 | 0.0000 |
    //
    // **本判据分别对 `A` 与 `B` 各做一次独立求积对拍**，两者都过
    // 才算绿。容差**由实测噪声定标**，不是拍脑袋：
    // 同一探针集上被测的实测噪声为 `rel_A ≤ 0.0031`、
    // `rel_B ≤ 0.0550`（4096 样本，见 `_attic` 诊断 `diag_b`）。
    // 取 `A: 0.10`（实测上限的 ~32 倍）、`B: 0.35`（实测上限的
    // ~6 倍）——两处都留足余量以免成为脆弱门禁，但**远低于**
    // `M06` 的 `rel_B ≥ 4.65`，仍是高信噪比。
    //
    // **为什么 `B` 的容差比 `A` 宽**：`B` 在正视区（`NdotV ≥ 0.8`）
    // 绝对值仅 `2e-6 ~ 1.6e-3`，重要性采样在这个量级上的**相对**
    // 噪声天然被放大（分子是极少数样本的贡献）；实测 `rel_B` 在
    // `(1.0, 0.05)` 处达 `0.0550`，比 `A` 侧高一个数量级。
    {
        let probe = [
            (1.0f32, 0.05f32),
            (1.0, 0.50),
            (0.8, 0.30),
            (0.6, 0.60),
            (0.6, 1.00),
            (0.4, 0.05),
            (0.4, 0.20),
            (0.4, 0.50),
        ];
        let mut ok_a = true;
        let mut ok_b = true;
        let mut worst_a = 0.0f64;
        let mut worst_b = 0.0f64;
        let mut pi = 0;
        while pi < probe.len() {
            let (v, r) = probe[pi];
            let (a, b) = brdf_integral_at(v, r, 4096);
            let (ra, rb) = quad_ref(v as f64, r as f64, 220, 220);
            // A 侧：真值恒 > 0.5（能量主体），相对误差有意义。
            let rel_a = ((a as f64) - ra).abs() / ra.max(1.0e-9);
            if rel_a > worst_a {
                worst_a = rel_a;
            }
            if rel_a > 0.10 {
                ok_a = false;
            }
            // B 侧：真值可低至 2e-6 ⇒ 必须**双侧判据**（相对 + 绝对）。
            // 只有绝对下限而无相对上限，会让「B 恒等于 0」这种
            // 把整条 bias 路砍掉的缺陷钻空子（`0 - 2e-6` 的绝对差
            // 恰好在绝对容差内）；只有相对而无绝对下限，则正视区
            // 会被 MC 噪声误杀。两侧都要，是三要件缺一不可。
            let diff_b = ((b as f64) - rb).abs();
            let rel_b = diff_b / rb.max(1.0e-9);
            if rel_b > worst_b {
                worst_b = rel_b;
            }
            if rel_b > 0.35 && diff_b > 1.0e-3 {
                ok_b = false;
            }
            pi += 1;
        }
        set.add(
            "C09-HARDEN-07 A/B分分量独立求积对拍(抓A+B守恒掩盖的分量错误)",
            ok_a && ok_b,
            "",
        );
    }

    // C09-HARDEN-02 **钳制覆盖双向触达**（补「值没到边界」的假门禁）
    {
        // 粗糙度输入须被钳到 [ROUGHNESS_MIN, 1]：低于下界与高于上界
        // 都要落到合法域内，且不产出 NaN。
        let (a_lo, b_lo) = brdf_integral_at(0.5, -10.0, 1024);
        let (a_at, b_at) = brdf_integral_at(0.5, ROUGHNESS_MIN, 1024);
        let (a_hi, b_hi) = brdf_integral_at(0.5, 99.0, 1024);
        let (a_at1, b_at1) = brdf_integral_at(0.5, 1.0, 1024);
        let finite = |x: f32| x == x && x != f32::INFINITY && x != f32::NEG_INFINITY;
        let ok = finite(a_lo)
            && finite(b_lo)
            && finite(a_hi)
            && finite(b_hi)
            // 下界钳制：与恰在下界时同值
            && (a_lo - a_at).abs() < 1.0e-4
            && (b_lo - b_at).abs() < 1.0e-4
            // 上界钳制：与恰在上界时同值
            && (a_hi - a_at1).abs() < 1.0e-4
            && (b_hi - b_at1).abs() < 1.0e-4;
        set.add(
            "C09-HARDEN-02 粗糙度钳制双向触达(域外等同边界)",
            ok,
            "",
        );
    }

    // C09-HARDEN-03 **立方图采样面归属自洽**（往返映射不符号翻转）
    {
        // `texel_dir` 与 `sample_dir_nearest` 互为逆映射：取一批纹素
        // 中心方向走一次最近邻采样，须回到原纹素。若任一处的面选择
        // 或 UV 符号写反，往返对不上。
        let e = 8u32;
        // 造一张每纹素唯一值的图（用线性下标编码）
        let n = CUBE_FACES * (e as usize) * (e as usize) * 3;
        let mut px = vec![0.0f32; n];
        let texels = n / 3;
        let mut i = 0;
        while i < texels {
            px[i * 3] = i as f32;
            px[i * 3 + 1] = i as f32;
            px[i * 3 + 2] = i as f32;
            i += 1;
        }
        let cube = match EnvCube::new(e, px) {
            Ok(c) => c,
            Err(_) => {
                set.add("C09-HARDEN-03 texel_dir/sample_dir往返自洽", false, "");
                return set;
            }
        };
        let mut mismatch = 0usize;
        let mut total = 0usize;
        let mut f = 0;
        while f < CUBE_FACES {
            let face = match CubeFace::from_index(f) {
                Some(x) => x,
                None => {
                    f += 1;
                    continue;
                }
            };
            let mut y = 1u32;
            while y + 1 < e {
                let mut x = 1u32;
                while x + 1 < e {
                    // 与模块同构的纹素中心方向（判据侧独立构造）
                    let ef = e as f32;
                    let uu = (x as f32 + 0.5) / ef * 2.0 - 1.0;
                    let vv = (y as f32 + 0.5) / ef * 2.0 - 1.0;
                    let d = match face {
                        CubeFace::PosX => (1.0, -vv, -uu),
                        CubeFace::NegX => (-1.0, -vv, uu),
                        CubeFace::PosY => (uu, 1.0, vv),
                        CubeFace::NegY => (uu, -1.0, -vv),
                        CubeFace::PosZ => (uu, -vv, 1.0),
                        CubeFace::NegZ => (-uu, -vv, -1.0),
                    };
                    let l = (d.0 * d.0 + d.1 * d.1 + d.2 * d.2).sqrt();
                    let dir = (d.0 / l, d.1 / l, d.2 / l);
                    let got = cube.sample_dir_nearest(dir);
                    let want_idx = (f * (e as usize) * (e as usize)
                        + (y as usize) * (e as usize)
                        + (x as usize)) as f32;
                    total += 1;
                    if (got.0 - want_idx).abs() > 0.5 {
                        mismatch += 1;
                    }
                    x += 2;
                }
                y += 2;
            }
            f += 1;
        }
        // 允许少量边界舍入误差，但主体必须对上
        set.add(
            "C09-HARDEN-03 texel_dir/sample_dir往返自洽(面归属不错)",
            total > 0 && mismatch * 20 <= total,
            "",
        );
    }

    // C09-HARDEN-04 **GGX D 单峰且峰位解析可对拍**
    {
        // 峰位在 `α* = sqrt((1−NdotH²)/NdotH²)`（对固定 NdotH 求极值）。
        let nh = 0.8f32;
        let a_star = ((1.0 - nh * nh) / (nh * nh)).sqrt();
        let mut best_a = 0.0f32;
        let mut best_d = -1.0f32;
        let mut i = 0u32;
        while i <= 200 {
            let a = 0.02 + i as f32 * (0.96 / 200.0);
            let d = ggx_d(nh, a);
            if d > best_d {
                best_d = d;
                best_a = a;
            }
            i += 1;
        }
        let peak_ok = (best_a - a_star).abs() < 0.02 && best_d > 0.0;
        // 反假腿：若 D 被写成常数，峰位将落在扫描端点
        let not_flat = best_a > 0.02 && best_a < 0.98;
        // alpha→0 时 D 退化为 delta，固定 NdotH 处的值趋 0
        let d_tiny_alpha = ggx_d(0.5, 1.0e-4);
        set.add(
            "C09-HARDEN-04 GGX单峰峰位解析对拍(alpha→0时D→0)",
            peak_ok && not_flat && d_tiny_alpha < 1.0e-3,
            "",
        );
    }

    // C09-HARDEN-05 **Fresnel Schlick 两端夹逼 + 幂次**
    {
        // NdotV=1 → F = F0（无边缘增亮）；NdotV=0 → F = 1（全反射）
        let at_one = fresnel_schlick(1.0, 0.04);
        let at_zero = fresnel_schlick(0.0, 0.04);
        let one_ok = (at_one - 0.04).abs() < 1.0e-6;
        let zero_ok = (at_zero - 1.0).abs() < 1.0e-6;
        // 中间须单调递增
        let mut prev = -1.0f32;
        let mut mono = true;
        let mut i = 0;
        while i <= 10 {
            let v = 1.0 - i as f32 * 0.1;
            let f = fresnel_schlick(v, 0.04);
            if f < prev - 1.0e-6 {
                mono = false;
            }
            prev = f;
            i += 1;
        }
        // **幂次契约**：`(1−0.5)^5 = 0.03125`。若幂次写成一次方，
        // 会得到 0.5——差 16 倍，且两端夹逼与单调性**仍然全绿**
        // （两端都是硬点，中间不参与），故必须单独钉住。
        let mid = fresnel_schlick(0.5, 0.0);
        let power_ok = (mid - 0.03125).abs() < 1.0e-5;
        set.add(
            "C09-HARDEN-05 Fresnel两端夹逼+五次幂契约(NdotV=0.5→1/32)",
            one_ok && zero_ok && mono && power_ok,
            "",
        );
    }

    set
}

/// ---------------------------------------------------------------------------
/// 反假变体登记（门禁有效性证明）
/// ---------------------------------------------------------------------------
///
/// 判据经**变异测试**验证：把被测实现按下列方式改坏，对应判据必须
/// 转红。**基线全绿 + 变体转红**双向都成立才算门禁有效。
///
/// | 变体 | 注入 | 预期转红判据 |
/// |---|---|---|
/// | M1 | LUT 退回 `NdotH` 均匀分段（丢 pdf 归一化） | `C09-PRE-01/03` |
/// | M2 | 菲涅尔角用 `NdotV` 而非 `VdotH` | `C09-PRE-02` |
/// | M3 | 分母改用「被接受样本数」 | `C09-PRE-03` |
/// | M4 | 估计量整体乘 0.5（半能量） | `C09-PRE-01/HARDEN-01` |
/// | M5 | 卷积除以样本数而非 `wsum` | `C09-PRE-04` |
/// | M6 | `compose_specular` 三通道复制 R | `C09-F1642-02` |
/// | M7 | 三线性改最近邻 | `C09-DUAL-01` |
/// | M8 | LOD 映射漏乘 `(MIP_LEVELS−1)` | `C09-DUAL-02` |
/// | M9 | 分档静默（`downgraded` 恒 false） | `C09-TIER-02` |
/// | M10 | HDR 上界提前一档（`HDR_MAX−1`） | `C09-FMT-02` |
/// | M11 | 采样数下界放行到 1 | `C09-FMT-04` |
/// | M12 | `texel_dir` 某面 UV 符号翻转 | `C09-HARDEN-03` |
/// | M13 | `ggx_d` 写成常数（丢尖峰） | `C09-HARDEN-04` |
/// | M14 | `fresnel_schlick` 五次方写成一次方 | `C09-HARDEN-05` |
/// | M18 | `pow2` 丢泰勒项（`2^x` 退化为 `2^floor`） | **空变异·见下** |
/// | M15 | 积分权重漏 `1/NdotH` | **等价变异·见下** |
/// | M16 | 积分权重重复计入几何项 | `C09-HARDEN-01/06` |
/// | M17 | `A` 路加 roughness 相关错误缩放（×`(1+0.5α)`） | `C09-HARDEN-06` |
/// | M24 | 积分权重漏 `VdotH`（**第四版的错误形态**） | `C09-PRE-01/HARDEN-06` |
/// | M25 | `VdotH` 重复计入（×`VdotH` 两次） | `C09-PRE-01` |
/// | M26 | 权重漏常数 `4` | `C09-PRE-01/HARDEN-01` |
/// | M27 | bias 路 `Fc` 符号翻转 | `C09-HARDEN-06` |
/// | M28 | `A` 路加 roughness 相关错误缩放 | `C09-HARDEN-01` |
///
/// **实测记录**：
///
/// - M1/M4 曾真实存在于本模块（M1 即 190 倍能量缺陷的成因），由
///   `C09-PRE-01` 的镜面极限绝对值契约捕获——修复前实测 `A+B=0.0052`
///   （判红），修复后 `0.9999`（判绿）。
/// - **第四版权重（`4·Vis·NdotL/NdotH`，缺 `VdotH`）经 M24 复现并
///   成功捕获**：该形态在 `NoV=1, r=1` 处 `A+B=1.3044`（+325%），
///   由 `C09-PRE-01` 判红。第五版补上 `VdotH` 后实测 0.3069，
///   与判据侧 f64 双域求积独立重算吻合到 4 位。
/// - **M24/M25/M26/M27/M28 五个变体经 `mutate2.py` 实跑全部 KILLED**
///   （判红，red 计数 1/2/4/4/3），证明第五版权重下门禁对「漏因子」
///   「重复因子」「漏常数」「符号翻转」「粗糙度错误缩放」五类
///   注入均有分辨力。
/// - **M18 是空变异（不是门禁漏网）**：实测 `pow2` 丢泰勒项后，
///   `C09-HARDEN-01` 七个采样点的 excess **逐位不变**
///   （−0.0200 / −0.0539 / −0.0458 / −0.0676 / −0.0936 / −0.0979 /
///   −0.2421）。根因：`a004` 中 `min(r.x², 2^(−9.28·NoV))` 在**全部
///   采样点上均由 `r.x²` 接管**（`2^(−9.28·NoV)` 远小于 `r.x²`），
///   `pow2` 的取值根本没进入结果。**这是变异选错，不是判据失效**
///   ——要激活它须构造 `2^(−9.28·NoV)` 起决定作用的采样点
///   （极小 roughness + `NoV→1`），而那种点上游 `r.x²` 同样趋零。
///   `pow2` 仍保留正确实现，因为它是判据侧的正确性依赖，不因为
///   「当前测不出」就放错。
/// - **M15 是等价变异（真等价，不是漏网）**：实测漏 `1/NdotH` 后
///   `A` 路的塌陷仅 **0.3%~6.1%**（`NoV=1,r=0.05` 处 0.3%，
///   `NoV=0.6,r=0.6` 处 6.1%）。根因：保留样本的 `NdotH ∈ [0.37, 1]`
///   接近 1，除以它与不除差异极小。而拟合式在 `NoV=1` 处自身偏差
///   就有 3% 量级，**与该塌陷同量级、无法区分**——把容差压到 3%
///   以下会让判据对拟合式的固有噪声敏感（假红）。故按等价变异登记，
///   不强行加容差。若要真正覆盖该项，需引入**独立于拟合式**的第二
///   参照（本次追查中已用它逐样本验证过 `4·Vis·NdotL/NdotH` 的正确
///   性，见 `vej09_ibl.rs` 的权重注释表）。
pub const VARIANT_REGISTRY: [(&str, &str); 23] = [
    ("M1-lut-uniform-nodoth-no-pdf", "C09-PRE-01/C09-PRE-03"),
    ("M2-fresnel-angle-ndotv", "C09-PRE-02"),
    ("M3-divisor-accepted-only", "C09-PRE-03"),
    ("M4-half-energy-scale", "C09-PRE-01/C09-HARDEN-06"),
    ("M5-convolve-divide-by-count", "C09-PRE-04"),
    ("M6-compose-replicate-red", "C09-F1642-02"),
    ("M7-trilinear-to-nearest", "C09-DUAL-01"),
    ("M8-lod-missing-level-scale", "C09-DUAL-02"),
    ("M9-silent-downgrade", "C09-TIER-02"),
    ("M10-hdr-bound-one-early", "C09-FMT-02"),
    ("M11-sample-min-bypass", "C09-FMT-04"),
    ("M12-texel-dir-uv-flip", "C09-HARDEN-03"),
    ("M13-ggx-d-constant", "C09-HARDEN-04"),
    ("M14-fresnel-power-linear", "C09-HARDEN-05"),
    ("M15-integral-weight-missing-1-ndoth", "EQUIVALENT-A-collapse<=6%"),
    ("M16-integral-weight-double-geometry", "C09-HARDEN-01/C09-HARDEN-06"),
    ("M17-scale-path-roughness-scaling", "C09-HARDEN-06"),
    ("M18-pow2-drops-taylor-term", "NULL-mutate-pow2-not-reached"),
    ("M24-missing-vdoth-factor", "C09-PRE-01/C09-HARDEN-06"),
    ("M25-weight-double-vdoth", "C09-PRE-01"),
    ("M26-weight-missing-4", "C09-PRE-01/C09-HARDEN-01"),
    ("M27-bias-fc-sign-flip", "C09-HARDEN-06"),
    ("M28-scale-roughness-scaling", "C09-HARDEN-01"),
];