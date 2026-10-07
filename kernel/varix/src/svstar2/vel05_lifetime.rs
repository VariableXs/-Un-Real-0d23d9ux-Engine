//! VE-F2205 · 粒子生命周期（VE-L 域 · 粒子与物理域 · 批次 L01 第 5 项 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2205`
//!
//! **判据（锚点原文五条）**：寿命分布、曲线单源、四态、爆裂预留、判据。
//! 逐条落位：
//! - **寿命分布**：[`LifetimeDist`] 常量 / 随机区间 `[min,max]` 两型，采样语义
//!   逐一声明；随机源**注入式**（复用 F2203 [`RandomSource`]），同种子双跑逐位一致。
//! - **曲线单源**：[`CurveSet`] 的 alpha / size / color 三通道曲线**直接复用
//!   F1407 [`FadeCurve`]**，不另写第二套淡变数学。曲线范式家族第三次复用
//!   （F1407 / F1927 / F2205）在此显式声明。
//! - **四态**：[`LifeState`] 新生 / 存活 / 淡出 / 死亡，态转移由寿命进度与
//!   [`FadeConfig::fade_start`] 曲线段共同驱动；态与属性联动由 [`CurveSet::shade`]
//!   保证（同一 t 必得同一组alpha/size/color）。
//! - **爆裂预留**：[`DeathBehavior::Burst`] 是**显性 STUB**——调用
//!   [`reserve_burst`] 必产诊断并拒绝，绝不静默noop；同时
//!   [`BURST_MAX_DEPTH`] 深度限制位**已实际生效**（先于STUB 检查，见该函数），
//!   未来实现子发射器时不得无限递归。
//!
//! **与 F2203 / F2204 的分工**：F2203 管「粒子怎样出生」，F2204 管「何时出生」，
//! 本模块管「出生之后怎样走向死亡」。故本模块**只消费** [`SpawnRequest::lifetime`]
//! 与 [`RandomSource`]，不复制发射率累积、形状采样、模式调度任一逻辑。
//!
//! **降级矩阵（锚点原文五条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 寿命参数非法（min>max / 负值 / NaN）→ 校验拒绝 | [`validate_lifetime_dist`] + [`create_particle_life`] 拒绝 |
//! | 曲线非单调致 alpha 回升 → **允许** + 文档说明 | [`CurveSet::monotonicity`] 只**咨询**不阻断，恒不产诊断 |
//! | 爆裂接口被调用 → 显性报错（F1871语义） | [`reserve_burst`] 返回 [`DiagCode::BurstReserved`] |
//! | 递归爆裂风险 → 预留接口内置深度限制位 | [`BURST_MAX_DEPTH`] + [`reserve_burst`] 前置深度门 |
//! | 随机源非确定（CPU 路径）→ 确定性断言 | [`ParticleLife::fingerprint`] + [`LifetimeLedger`] |
//!
//! **性能诚实标注**：本模块稳态路径**零分配**（LUT 于建器时预计算一次，见
//! [`CurveLut::build`]），单粒子每帧成本 = 3 次 LUT 查表（O(1)）+ 1 次 O(1) 态转移。
//! 锚点给出的「<10ns/粒子/帧」是**目标值而非本模块实测值**——内核侧尚无
//! 粒子微基准，实测由 **VE-F2212（粒子基准）** 承接，本模块只交付成本模型
//! （查表次数、分配次数、复杂度），**不代填未测数据**。LUT 量化误差另有解析上界
//! [`CurveLut::declared_max_error`]，非隐藏近似。
//!
//! **跨批对接**：曲线核单源 F1407（家族第三度复用）；池回收 F2208（本模块只产出
//! `recycled` 标志，不碰池）；爆裂预留与 F1871 STUB 同构；随机确定性 F2215 联动。
//!
//! 零 IO、零墙钟；时间以逻辑 `dt` 注入，随机源显式注入，故同输入双跑逐位一致。
//! 无隐私面。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::veh07_fade::FadeCurve;
use super::vel03_emitter::{DiagBag, DiagCode, Diagnostic, Outcome, RandomSource, SpawnRequest};

// ---------------------------------------------------------------------------
// 一、诊断码（自有码 + 映射回F2203 诊断袋，保持全链路单一诊断出口）
// ---------------------------------------------------------------------------

/// 生命周期域诊断码。
///
/// 处置方向相反的状态**不共用码**：「钳制」（原值非法但可用）与「拒绝」
/// （原值非法且不可用）语义相反，共用码会让调用方误判严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifeDiag {
    /// 寿命参数校验拒绝（min>max / ≤0 / NaN / 无穷）。
    LifetimeRejected,
    /// 寿命被钳制（负值被抬到下限）。
    LifetimeClamped,
    /// 曲线控制参数非有限，校验拒绝。
    CurveRejected,
    /// 淡出起点比例越界，钳制到 [0,1)。
    FadeStartClamped,
    /// 「缩小」死亡行为不可达：尺寸曲线终点非零，粒子永远不会缩到零。
    ShrinkUnreachable,
    /// 爆裂接口被调用——预留位显性报错，绝不静默。
    BurstReserved,
    /// 递归爆裂深度超限。
    BurstDepthExceeded,
    /// 态转移非法被拒绝。
    StateRejected,
    /// 确定性断言失败（指纹对拍不一致）。
    DeterminismViolation,
}

impl LifeDiag {
    /// 中文标签（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            LifeDiag::LifetimeRejected => "寿命参数拒绝",
            LifeDiag::LifetimeClamped => "寿命钳制",
            LifeDiag::CurveRejected => "曲线参数拒绝",
            LifeDiag::FadeStartClamped => "淡出起点钳制",
            LifeDiag::ShrinkUnreachable => "缩小行为不可达",
            LifeDiag::BurstReserved => "爆裂接口预留未实现",
            LifeDiag::BurstDepthExceeded => "爆裂递归深度超限",
            LifeDiag::StateRejected => "生命周期态转移拒绝",
            LifeDiag::DeterminismViolation => "确定性断言失败",
        }
    }

    /// 该码是否表示「原值被改写」（区别于「原值被拒」）。
    pub fn is_mutation(self) -> bool {
        matches!(self, LifeDiag::LifetimeClamped | LifeDiag::FadeStartClamped)
    }

    /// 映射到 F2203 的诊断码（复用既有诊断袋，不另立一套出口）。
    ///
    /// 映射口径：拒绝族 → `ShapeRejected`（校验拒绝的既有出口），
    /// 变更族 → `RateClamped`（钳制的既有出口），
    /// 预留未实现 → `TransitionRejected`（调用被拒的既有出口），
    /// 深度超限 → `GroupRejected`（配额类拒绝：深度本质是嵌套预算）。
    pub fn to_emitter_code(self) -> DiagCode {
        match self {
            LifeDiag::LifetimeRejected | LifeDiag::CurveRejected => DiagCode::ShapeRejected,
            LifeDiag::LifetimeClamped | LifeDiag::FadeStartClamped => DiagCode::RateClamped,
            LifeDiag::ShrinkUnreachable => DiagCode::ShapeRejected,
            LifeDiag::BurstReserved => DiagCode::TransitionRejected,
            LifeDiag::BurstDepthExceeded => DiagCode::GroupRejected,
            LifeDiag::StateRejected => DiagCode::TransitionRejected,
            LifeDiag::DeterminismViolation => DiagCode::RngDegraded,
        }
    }
}

/// 记一条生命周期诊断进 F2203 的诊断袋（保持全链路单一诊断出口）。
pub fn note(bag: &mut DiagBag, code: LifeDiag, message: String, hint: String) {
    bag.push(Diagnostic::new(code.to_emitter_code(), message, hint));
}

/// 无值成功的统一构造（校验类结果专用）。
///
/// 独立成自由函数而非 `impl<T>` 关联函数：后者造出 `Outcome<()>` 时调用点泛型
/// `T` 无从推断，会报 `cannot infer type of the type parameter T`。
fn ok_unit() -> Outcome<()> {
    Outcome::Ok { value: (), diagnostics: Vec::new() }
}

// ---------------------------------------------------------------------------
// 二、寿命分布（判据一：寿命分布）
// ---------------------------------------------------------------------------

/// 寿命下限量（秒）。
///
/// 严格大于 0：寿命为 0 会让生命进度 `t = age / lifetime` 除零。故0 与负值
/// 一律**拒绝**而非钳到 0——「寿命为零」不是可用数据，是坏数据。
pub const LIFETIME_MIN_SEC: f32 = 1.0e-4;

/// 寿命上限（秒）。超过则钳制：寿命无上限会让粒子永不回收，池水位单调上涨
/// 最终撞 95% 拒绝阈值（F2208），那时的报错离病因太远。
pub const LIFETIME_MAX_SEC: f32 = 3600.0;

/// 寿命分布。
///
/// 两型不多不少（锚点：常量 / 随机区间 `[min,max]`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LifetimeDist {
    /// 常量寿命：所有粒子同寿。
    Constant(f32),
    /// 随机区间寿命：`[min,max)` 均匀采样（**左闭右开**——`RandomSource::range_f32`
    /// 的值域是 `[lo,hi)`，写成闭区间会与实际采样域不符，那是文档与实现漂移）。
    Range { min: f32, max: f32 },
}

impl LifetimeDist {
    /// 中文标签（读屏播报用）。
    pub fn zh(&self) -> &'static str {
        match self {
            LifetimeDist::Constant(_) => "常量寿命",
            LifetimeDist::Range { .. } => "随机区间寿命",
        }
    }

    /// 分布下界（常量型返回自身）。
    pub fn min_sec(&self) -> f32 {
        match self {
            LifetimeDist::Constant(v) => *v,
            LifetimeDist::Range { min, .. } => *min,
        }
    }

    /// 分布上界（常量型返回自身）。
    pub fn max_sec(&self) -> f32 {
        match self {
            LifetimeDist::Constant(v) => *v,
            LifetimeDist::Range { max, .. } => *max,
        }
    }

    /// 采样一个寿命（秒）。复杂度 **O(1)**（锚点性能分解：寿命采样 O(1)）。
    ///
    /// `rng` 显式注入而非读全局 RNG——这是 F2215 确定性根基的前提：
    /// 同种子双跑必须逐位一致，故随机流必须由调用方掌控。
    pub fn sample(&self, rng: &mut RandomSource) -> f32 {
        match self {
            LifetimeDist::Constant(v) => *v,
            LifetimeDist::Range { min, max } => rng.range_f32(*min, *max),
        }
    }
}

/// 寿命分布校验（锚点降级矩阵第一项：min>max / 负值 / NaN → 校验拒绝）。
///
/// 逐项写明拒绝理由而不只说「非法」：调用方要能分辨是自己写反了边界还是
/// 漏了初始化。
pub fn validate_lifetime_dist(d: &LifetimeDist) -> Outcome<()> {
    let (min, max) = (d.min_sec(), d.max_sec());
    if min.is_nan() || max.is_nan() {
        return Outcome::fail(
            DiagCode::ShapeRejected,
            format!("寿命分布含 NaN（min={}，max={}）", min, max),
            String::from("NaN 通常来自上游除零或未初始化字段；寿命须为有限数"),
        );
    }
    if min.is_infinite() || max.is_infinite() {
        return Outcome::fail(
            DiagCode::ShapeRejected,
            format!("寿命分布含无穷（min={}，max={}）", min, max),
            String::from("无穷寿命须先在配置层收敛到具名上限，避免粒子永不回收"),
        );
    }
    if min > max {
        return Outcome::fail(
            DiagCode::ShapeRejected,
            format!("寿命区间反向（min={} > max={}）", min, max),
            String::from("区间须满足 min <= max；写反边界是配置期错误，不做静默交换"),
        );
    }
    if max <= 0.0 {
        return Outcome::fail(
            DiagCode::ShapeRejected,
            format!("寿命上界非正（max={}）", max),
            String::from("寿命须严格大于 0：寿命为零会让生命进度 t=age/lifetime 除零"),
        );
    }
    if min < LIFETIME_MIN_SEC {
        return Outcome::fail(
            DiagCode::ShapeRejected,
            format!("寿命下界 {} 小于下限 {}", min, LIFETIME_MIN_SEC),
            String::from("过低寿命使粒子当帧生当帧死，视觉上等同不发射；请抬高下界"),
        );
    }
    if max > LIFETIME_MAX_SEC {
        return Outcome::fail(
            DiagCode::ShapeRejected,
            format!("寿命上界 {} 超过上限 {}", max, LIFETIME_MAX_SEC),
            String::from("超长寿命使粒子近乎永不回收，池水位单调上涨；请收敛到上限内"),
        );
    }
    ok_unit()
}

// ---------------------------------------------------------------------------
// 三、曲线单源：LUT + 三通道曲线集（判据二：曲线单源）
// ---------------------------------------------------------------------------

/// 线性插值查表的默认分段数。
///
/// 256 段的解析误差上界（M/(8N²)）对 F1407 五型曲线均在 1e-5 量级，
/// 远低于 f32 视觉可辨阈值（~1e-3），而查表成本是 O(1)（锚点：LUT O(1)）。
pub const LIFELINE_LUT_STEPS: usize = 256;

/// 曲线 LUT 量化误差的**解析上界**。
///
/// 均匀 N 段线性插值对 `|f''| <= M` 的函数，误差上界为 `M / (8N²)`。
/// 本函数给出该上界，**不是实测误差**——实测留给 F2212，此处只承诺可推导的界。
pub fn lut_error_bound(curvature_bound: f32, steps: usize) -> f32 {
    if steps == 0 {
        return f32::INFINITY;
    }
    let n = steps as f32;
    curvature_bound / (8.0 * n * n)
}

/// F1407 五型曲线在 `t ∈ [0,1]` 上的二阶导绝对值上界（精确推导，非估值）。
///
/// 用途：给 [`CurveLut::declared_max_error`] 提供可推导的误差承诺。曲线
/// 量化误差若不声明上界就是隐藏近似——粒子淡入淡出的视觉渐变对误差敏感，
/// 「差不多」不是工程口径。
pub fn curvature_bound(curve: &FadeCurve) -> f32 {
    match curve {
        // f(t)=t，f''=0 → 线性插值**零误差**。
        FadeCurve::Linear => 0.0,
        // f(t)=t²，f''=2。
        FadeCurve::Exponential => 2.0,
        // f(t)=3t²-2t³，f''=6-12t，|f''| 最大 6（t=0 与 t=1）。
        FadeCurve::SCurve => 6.0,
        // f(t)=sin(πt/2)，f''=-(π/2)²·sin(πt/2)，上界 (π/2)²。
        FadeCurve::EqualPower => {
            let h = core::f64::consts::FRAC_PI_2;
            (h * h) as f32
        }
        // 三次贝塞尔 f(u)=3y1(u-2u²+u³)+3y2(u²-u³)+u³：
        // f''(u)=3y1(-4+6u)+3y2(2-6u)+6u，关于 u 线性 → 极值在端点，
        // 得上界 max(|-12y1+6y2|, |6y1-12y2|) = 6·max(|2y1-y2|, |y1-2y2|)。
        FadeCurve::Bezier { y1, y2, .. } => {
            let a = (2.0f64 * y1 - y2).abs();
            let b = (y1 - 2.0f64 * y2).abs();
            (6.0 * if a > b { a } else { b }) as f32
        }
    }
}

/// 曲线查表：把 F1407 [`FadeCurve`] 预计算成O(1) 查表。
///
/// **单源纪律**：本类型不实现任何曲线数学，只**搬运** F1407 的 `at()` 结果。
/// 若在此另写一份插值，就等于在粒子域偷偷开了第二套淡变语义——那正是锚点
/// 「曲线核单源」要防的漂移。
#[derive(Clone, Debug, PartialEq)]
pub struct CurveLut {
    /// 采样表，`table.len() == steps + 1`。
    table: Vec<f32>,
    steps: usize,
    /// 二阶导上界（由 [`curvature_bound`] 给出，构造时固化）。
    curvature: f32,
}

impl CurveLut {
    /// 预计算 LUT。**这是本模块唯一的分配点**（锚点性能：稳态零分配）。
    pub fn build(curve: &FadeCurve, steps: usize) -> Outcome<CurveLut> {
        if steps == 0 {
            return Outcome::fail(
                DiagCode::ShapeRejected,
                String::from("LUT 分段数为 0，查表将退化为常数"),
                String::from("分段数须 >= 1；建议 256（误差上界 ~1e-5，低于视觉可辨阈）"),
            );
        }
        let mut table: Vec<f32> = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let t = i as f64 / steps as f64;
            let v = curve.at(t);
            if !(v.is_finite()) {
                return Outcome::fail(
                    DiagCode::ShapeRejected,
                    format!("曲线在 t={} 产出非有限值 {}", t, v),
                    String::from("曲线控制点非法（NaN/无穷）；淡变曲线须全程有限"),
                );
            }
            table.push(v as f32);
        }
        // 表长不变量：必须恒为 `steps + 1`，否则下面的端点钉死与 `sample`
        // 的 `table[i + 1]` 都会越界。**显式校验而非靠推理**——「循环写了
        // `0..=steps`所以表长必然对」是隐式耦合：日后有人把循环改成
        // `0..steps`（少建一点，看着像无害优化），越界会在运行期panic，
        // 而内核 panic 面必须为零。W005 反假变体V14 实测该改法会panic。
        if table.len() != steps + 1 {
            return Outcome::fail(
                DiagCode::ShapeRejected,
                format!("LUT 表长 {} 与分段数 {} 不一致", table.len(), steps),
                String::from("表长须恒为分段数加一；这是端点钉死与插值的索引前提"),
            );
        }
        // 端点精确性是F1407 曲线族的既有纪律（端点钉死 0/1），LUT 不得破坏它：
        // 否则 t=0 的粒子 alpha 不是 0，首帧闪现一帧全亮。
        //
        // 诚实标注：实测五型曲线 `at(0)`/`at(1)` **本身已精确**（Bezier 有
        // `t<=0`/`t>=1` 早退兜底），故这两行对当前曲线族是**防御性冗余**——
        // 反假变体测试删掉它们，自检仍全绿（等价变体，非门禁缺陷）。
        // 保留理由：F1407 未来新增曲线型若不守端点纪律，这两行是唯一的
        // 兜底，且成本是两次数组写。删它们省不到可测量的性能，却会让
        // 「LUT 必精确」从构造保证退化为对上游纪律的信任。
        // 分两次取，避免 `first_mut`/`last_mut` 同时可变借用同一向量。
        if table.is_empty() {
            return Outcome::fail(
                DiagCode::ShapeRejected,
                String::from("LUT 表为空，无法钉死端点"),
                String::from("空表无法承载端点语义；请检查分段数是否合法"),
            );
        }
        if let Some(first) = table.first_mut() {
            *first = 0.0;
        }
        if let Some(last) = table.last_mut() {
            *last = 1.0;
        }
        Outcome::Ok { value: CurveLut { table, steps, curvature: curvature_bound(curve) }, diagnostics: Vec::new() }
    }

    /// 查表（**O(1)**，锚点性能：LUT O(1)）。端点精确。
    ///
    /// **索引安全**：索引一律经 [`Self::at_or_zero`] 收敛，不裸写`table[k]`。
    /// `CurveLut` 虽只能由 [`CurveLut::build`] 构造（那里以「表长==steps+1
    /// 且steps>=1」的显式校验把守），但零 panic 面的纪律是**不依赖调用方
    /// 信任**：日后若新增构造路径或改动表布局，这里应降级为「返回 0.0」
    /// 而非越界panic。W005 反假变体 V14 实测少建一个点即越界——现已在
    /// build 期被拒，且查表侧另有兜底，双重保险。
    pub fn sample(&self, t: f32) -> f32 {
        if t.is_nan() {
            return self.at_or_zero(0);
        }
        let tc = t.clamp(0.0, 1.0);
        if tc <= 0.0 {
            return self.at_or_zero(0);
        }
        if tc >= 1.0 {
            return self.at_or_zero(self.steps);
        }
        let x = tc * self.steps as f32;
        let i = x.floor() as usize;
        if i >= self.steps {
            return self.at_or_zero(self.steps);
        }
        let frac = x - i as f32;
        let a = self.at_or_zero(i);
        let b = self.at_or_zero(i + 1);
        a + (b - a) * frac
    }

    /// 安全取表：越界或表空一律返回 0.0（不可达分支，但不得 panic）。
    #[inline]
    fn at_or_zero(&self, i: usize) -> f32 {
        match self.table.get(i) {
            Some(v) => *v,
            None => 0.0,
        }
    }

    /// 量化误差的**解析上界** `M/(8N²)`（非实测值，见 [`lut_error_bound`]）。
    pub fn declared_max_error(&self) -> f32 {
        lut_error_bound(self.curvature, self.steps)
    }

    /// 分段数。
    pub fn steps(&self) -> usize {
        self.steps
    }

    /// 采样点数（`steps + 1`）。
    pub fn len(&self) -> usize {
        self.table.len()
    }

    /// 表是否为空（恒为 false——空表无法承载端点语义；保留以满足 clippy）。
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

/// 单调性判定结果（**咨询性质**，见 [`CurveSet::monotonicity`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Monotonicity {
    /// 非降（视觉正常）。
    NonDecreasing,
    /// 非增（视觉正常，如淡出）。
    NonIncreasing,
    /// 非单调：存在回升段。**合法但视觉怪异**，见锚点降级矩阵第二项。
    NonMonotonic,
}

impl Monotonicity {
    pub fn zh(self) -> &'static str {
        match self {
            Monotonicity::NonDecreasing => "非降",
            Monotonicity::NonIncreasing => "非增",
            Monotonicity::NonMonotonic => "非单调（alpha 会回升）",
        }
    }

    /// 是否为视觉怪异的非单调。
    pub fn is_rebounding(self) -> bool {
        matches!(self, Monotonicity::NonMonotonic)
    }
}

/// 线性颜色（float4/ 线性空间 / 逐通道钳制 [0,1]）。
///
/// 口径对齐 F2202 属性规格表的 color 项：`float4` 线性 0~1。淡变全程在
/// **线性空间**插值——在sRGB 空间插值会让淡变中段偏暗，这是四通道皆线性
/// 的原因，不是随手选的。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Rgba {
        Rgba { r, g, b, a }
    }

    /// 全不透明黑（`Default` 与本常量同源，避免两处各写一个）。
    pub const BLACK: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

    /// 全透明（淡入起点常用）。
    pub const TRANSPARENT: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    /// 逐通道钳制到 [0,1]（非有限值落到 0——染上 NaN 的颜色画出来是随机色块）。
    pub fn clamped(self) -> Rgba {
        let cl = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        Rgba { r: cl(self.r), g: cl(self.g), b: cl(self.b), a: cl(self.a) }
    }

    /// 四分量皆在 [0,1] 且有限。
    pub fn is_valid(self) -> bool {
        let ok = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
        ok(self.r) && ok(self.g) && ok(self.b) && ok(self.a)
    }

    /// 线性插值（`k` 须在 [0,1]，越界由调用方钳制）。
    pub fn lerp(self, other: Rgba, k: f32) -> Rgba {
        let k = k.clamp(0.0, 1.0);
        Rgba {
            r: self.r + (other.r - self.r) * k,
            g: self.g + (other.g - self.g) * k,
            b: self.b + (other.b - self.b) * k,
            a: self.a + (other.a - self.a) * k,
        }
    }
}

impl Default for Rgba {
    fn default() -> Rgba {
        Rgba::BLACK
    }
}

/// 颜色渐变端点：曲线值 0 → `from`，曲线值 1 → `to`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorRamp {
    pub from: Rgba,
    pub to: Rgba,
}

impl ColorRamp {
    pub fn new(from: Rgba, to: Rgba) -> ColorRamp {
        ColorRamp { from, to }
    }
}

/// 淡出起点配置。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FadeConfig {
    /// 生命进度进入 [fade_start, 1) 即进入「淡出」态。
    ///
    /// 语义：`fade_start = 0.0` 表示**全程淡出**（出生即开始淡）；
    /// `fade_start = 1.0` 表示**不淡出**（只在末帧跳变到 0）。
    pub fade_start: f32,
}

impl FadeConfig {
    /// 全程淡出（常见默认：出生即淡）。
    pub const WHOLE_LIFE: FadeConfig = FadeConfig { fade_start: 0.0 };
    /// 全程不淡（恒亮至死）。
    pub const NONE: FadeConfig = FadeConfig { fade_start: 1.0 };

    /// 淡出起点钳制到 [0,1]（NaN → 0，负 → 0，>1 → 1）。
    ///
    /// 钳制而非拒绝的理由与 F2203 发射率一致：`fade_start` 越界是**可用数据**
    /// （取边界值仍产出合理视觉），NaN 才是在配置层就必须拦的坏数据。
    pub fn clamped(&self, bag: &mut DiagBag) -> FadeConfig {
        let raw = self.fade_start;
        let fixed = if raw.is_nan() {
            0.0
        } else {
            raw.clamp(0.0, 1.0)
        };
        if fixed != raw {
            note(
                bag,
                LifeDiag::FadeStartClamped,
                format!("淡出起点 {} 已钳制到 {}", raw, fixed),
                String::from("淡出起点须在 [0,1]：0=全程淡出，1=不淡出"),
            );
        }
        FadeConfig { fade_start: fixed }
    }
}

/// 三通道曲线集：alpha / size / color。
///
/// **曲线单源落位**：三通道全部由 F1407 [`FadeCurve`] 预计算而来，本模块
/// 不新增任何淡变数学形态。曲线范式家族：F1407（引擎服务）→ F1927（第二度
/// 复用）→ F2205（本模块第三度复用）。
#[derive(Clone, Debug, PartialEq)]
pub struct CurveSet {
    /// 透明度曲线（生命比例 → alpha 增益）。
    pub alpha: CurveLut,
    /// 尺寸曲线（生命比例 → 尺寸乘子）。
    pub size: CurveLut,
    /// 颜色混合曲线（生命比例 → [`ColorRamp`] 插值系数）。
    pub color: CurveLut,
    /// 颜色端点。
    pub ramp: ColorRamp,
}

impl CurveSet {
    /// 由三条 F1407 曲线装配。
    ///
    /// `steps = 0` 传 [`LIFELINE_LUT_STEPS`]。三条曲线中任一非法即整体拒绝——
    /// 半合法的曲线集比非法更危险（渲染时才炸，且只炸一半通道）。
    pub fn build(
        alpha: &FadeCurve,
        size: &FadeCurve,
        color: &FadeCurve,
        ramp: ColorRamp,
        steps: usize,
    ) -> Outcome<CurveSet> {
        if !ramp.from.is_valid() {
            return Outcome::fail(
                DiagCode::ShapeRejected,
                format!("颜色渐变起点非法（r={},g={},b={},a={}）", ramp.from.r, ramp.from.g, ramp.from.b, ramp.from.a),
                String::from("颜色四分量须为 [0,1] 有限值（线性空间，对齐 F2202 规格表）"),
            );
        }
        if !ramp.to.is_valid() {
            return Outcome::fail(
                DiagCode::ShapeRejected,
                format!("颜色渐变终点非法（r={},g={},b={},a={}）", ramp.to.r, ramp.to.g, ramp.to.b, ramp.to.a),
                String::from("颜色四分量须为 [0,1] 有限值（线性空间，对齐 F2202 规格表）"),
            );
        }
        // 三条曲线逐一装配。刻意**不用 `?`**：本仓的 `Outcome` 是自定义
        // no_std 结果类型，没有 `From` 桥（`?` 需要 `From<Outcome<CurveLut>>
        // for Outcome<CurveSet>`），故显式 match 传播——顺带把失败诊断原文带出。
        let alpha = match CurveLut::build(alpha, steps) {
            Outcome::Ok { value, .. } => value,
            Outcome::Err { code, message, hint } => return Outcome::fail(code, message, hint),
        };
        let size = match CurveLut::build(size, steps) {
            Outcome::Ok { value, .. } => value,
            Outcome::Err { code, message, hint } => return Outcome::fail(code, message, hint),
        };
        let color = match CurveLut::build(color, steps) {
            Outcome::Ok { value, .. } => value,
            Outcome::Err { code, message, hint } => return Outcome::fail(code, message, hint),
        };
        Outcome::ok(CurveSet { alpha, size, color, ramp })
    }

    /// 便捷：线性三通道（不淡入不淡出、尺寸恒定、颜色恒定）。
    pub fn constant(steps: usize) -> Outcome<CurveSet> {
        CurveSet::build(
            &FadeCurve::Linear,
            &FadeCurve::Linear,
            &FadeCurve::Linear,
            ColorRamp::new(Rgba::BLACK, Rgba::BLACK),
            steps,
        )
    }

    /// 由生命进度取三通道属性值。
    ///
    /// 复杂度 **O(1)**（3 次查表 + 1 次 lerp）。这是「态与属性联动」的落点：
    /// 同一 `t` 必得同一组 alpha/size/color，故四态与视觉严格一致——
    /// 不存在「态说在淡出而 alpha 还是满值」的可能。
    ///
    /// **alpha 取补（关键语义，不可写成曲线原值）**：F1407 [`FadeCurve`] 是
    /// **包络增益**曲线，端点钉死 `f(0)=0, f(1)=1`（淡入：从无到有）；
    /// 而粒子的 alpha 须**淡出**：`t=0` 出生时最不透明、`t=1` 死亡时全透明。
    /// 故alpha = `1 - f(t)`。
    ///
    /// 写成 `f(t)` 会让粒子「越活越亮、临死前最亮」——那不是淡出，是渐亮，
    /// 且死亡帧alpha=1 会留下一个最亮的残影点，视觉上表现为「粒子闪一下才消失」。
    /// 补运算让同一族 F1407 曲线**原样**服务于淡出语义，无需在粒子域另写
    /// 第二套反向曲线（那正是「曲线核单源」要防的漂移）。
    ///
    /// size / color 通道**不取补**：尺寸与混色是「随生命进度演化」而非
    /// 「淡出」，其方向由资产侧曲线自行决定（线性尺寸曲线即恒定尺寸）。
    pub fn shade(&self, t: f32) -> ShadedState {
        let gain = self.alpha.sample(t);
        let s = self.size.sample(t);
        let ck = self.color.sample(t);
        let a = 1.0 - gain;
        ShadedState {
            alpha: if a.is_finite() { a.clamp(0.0, 1.0) } else { 0.0 },
            size: if s.is_finite() { s.max(0.0) } else { 0.0 },
            color: self.ramp.from.lerp(self.ramp.to, ck),
        }
    }

    /// alpha 曲线单调性（**咨询性质，恒不阻断、恒不产诊断**）。
    ///
    /// 锚点降级矩阵第二项：曲线非单调导致 alpha 回升「视觉怪异但合法」→
    /// **允许 + 文档说明**。故本函数只**报告**事实：调用方（调试面板 /
    /// 资产检查器）可查，但引擎不因此拒绝配置、不因此产诊断。
    ///
    /// 这条纪律的价值：若把「非单调」升级为拒绝，就是把用户的创作选择
    /// （脉冲式闪烁本就需要 alpha 回升）当成错误禁掉；而若完全不管，
    /// 用户又会困惑「为什么我的粒子在闪」。咨询 + 文档是唯一诚实的位置。
    pub fn monotonicity(&self) -> Monotonicity {
        let t = self.alpha.table.as_slice();
        let mut nondec = true;
        let mut noninc = true;
        for i in 1..t.len() {
            let d = t[i] - t[i - 1];
            if d < 0.0 {
                nondec = false;
            }
            if d > 0.0 {
                noninc = false;
            }
        }
        if nondec {
            Monotonicity::NonDecreasing
        } else if noninc {
            Monotonicity::NonIncreasing
        } else {
            Monotonicity::NonMonotonic
        }
    }

    /// 三条通道的 LUT 量化误差上界中的最大值。
    pub fn max_declared_error(&self) -> f32 {
        let a = self.alpha.declared_max_error();
        let b = self.size.declared_max_error();
        let c = self.color.declared_max_error();
        if a > b {
            if a > c {
                a
            } else {
                c
            }
        } else if b > c {
            b
        } else {
            c
        }
    }
}

/// 某一生命进度下的属性值（三通道联动结果）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadedState {
    /// 透明度增益（[0,1]）。
    pub alpha: f32,
    /// 尺寸乘子（≥0）。
    pub size: f32,
    /// 线性空间颜色。
    pub color: Rgba,
}

// ---------------------------------------------------------------------------
// 四、死亡行为与爆裂预留（判据四：爆裂预留）
// ---------------------------------------------------------------------------

/// 死亡行为（锚点三型：消失 / 缩小 / 爆裂子粒子预留）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeathBehavior {
    /// 消失：到达寿命终点直接回收。
    Vanish,
    /// 缩小：尺寸归零后回收。
    Shrink,
    /// 爆裂：死亡时生成子粒子——**预留位，尚未实现**。
    ///
    /// 本变体存在的意义是**诚实标注**：调用方配了爆裂就必须知道它没生效。
    /// 实现留待F2208（池）与子发射器对接之后；届时 [`reserve_burst`] 的
    /// STUB 分支替换为真实子发射器调用，深度限制位 [`BURST_MAX_DEPTH`] 保持有效。
    Burst,
}

impl DeathBehavior {
    /// 三型全集（自检按此断言「不多不少」）。
    pub const ALL: [DeathBehavior; 3] =
        [DeathBehavior::Vanish, DeathBehavior::Shrink, DeathBehavior::Burst];

    pub fn zh(self) -> &'static str {
        match self {
            DeathBehavior::Vanish => "消失",
            DeathBehavior::Shrink => "缩小",
            DeathBehavior::Burst => "爆裂（预留未实现）",
        }
    }

    /// 中文标签（读屏播报用）。
    pub fn label(self) -> &'static str {
        DeathBehavior::zh(self)
    }

    /// 是否为尚未实现的预留位。
    pub fn is_reserved(self) -> bool {
        matches!(self, DeathBehavior::Burst)
    }
}

/// 爆裂递归深度上限（前向兼容约束，**已实际生效**）。
///
/// 锚点：「递归爆裂风险（预留时防递归）→ 预留接口内置深度限制位（未来实现
/// 不得无限递归——前向兼容约束）」。
///
/// 关键设计：深度限制**不是装饰性常量**。[`reserve_burst`] 先查深度再判STUB，
/// 故「深度超限」与「爆裂未实现」是**两条可区分的诊断**——未来实现子发射器
/// 后，深度门无需改动即继续生效；反之若把深度检查放在 STUB 之后，实现那天
/// 深度门会静默失效，那才是真正的隐患。
pub const BURST_MAX_DEPTH: u8 = 4;

/// 请求爆裂（死亡时生成子粒子）——**预留接口，显性报错，绝不静默**。
///
/// 行为：
/// 1. `depth >= [`BURST_MAX_DEPTH`]` → 拒绝并报**深度超限**（前向兼容约束生效）；
/// 2. 否则 → 拒绝并报**预留未实现**（F1871 语义：不静默的预留位）。
///
/// 两条路径都返回 [`Outcome::Err`] 且**不产出任何粒子**——静默 noop 会让
/// 「配了爆裂却什么都没发生」变成一个查不出病因的问题。
pub fn reserve_burst(depth: u8, bag: &mut DiagBag) -> Outcome<Vec<SpawnRequest>> {
    if depth >= BURST_MAX_DEPTH {
        note(
            bag,
            LifeDiag::BurstDepthExceeded,
            format!("爆裂深度 {} 达到上限 {}", depth, BURST_MAX_DEPTH),
            String::from("递归爆裂必须限深，否则子粒子无限增殖；请减少嵌套层数"),
        );
        return Outcome::fail(
            DiagCode::GroupRejected,
            format!("爆裂递归深度超限（{} >= {}）", depth, BURST_MAX_DEPTH),
            String::from("深度限制是前向兼容约束：子发射器链最深不得超过上限"),
        );
    }
    note(
        bag,
        LifeDiag::BurstReserved,
        format!("爆裂接口被调用（深度 {}）——预留位未实现，已拒绝", depth),
        String::from("爆裂子粒子发射尚未实现；改用消失/缩小，或等F2208 池侧对接后启用"),
    );
    Outcome::fail(
        DiagCode::TransitionRejected,
        String::from("爆裂为预留接口：显性报错，不静默"),
        String::from("实现承接单：VE-F2208（粒子池与内存）子发射器对接"),
    )
}

// ---------------------------------------------------------------------------
// 五、四态状态机（判据三：四态）
// ---------------------------------------------------------------------------

/// 生命周期四态（锚点：新生 / 存活 / 淡出 / 死亡）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifeState {
    /// 新生：`t == 0`（尚未推进任何dt）。
    Newborn,
    /// 存活：`0 < t < fade_start`。
    Alive,
    /// 淡出：`fade_start <= t < 1`。
    Fading,
    /// 死亡：`t >= 1`，等待池回收（F2208）。
    Dead,
}

impl LifeState {
    /// 四态全集（自检按此断言「不多不少」）。
    pub const ALL: [LifeState; 4] =
        [LifeState::Newborn, LifeState::Alive, LifeState::Fading, LifeState::Dead];

    pub fn zh(self) -> &'static str {
        match self {
            LifeState::Newborn => "新生",
            LifeState::Alive => "存活",
            LifeState::Fading => "淡出",
            LifeState::Dead => "死亡",
        }
    }

    /// 是否为终态（死亡不可离开）。
    pub fn is_terminal(self) -> bool {
        matches!(self, LifeState::Dead)
    }

    /// 合法后继态。
    ///
    /// 注意 `Newborn → Fading` 是**合法直跳**：`fade_start == 0.0`（全程淡出）
    /// 时粒子不经过存活态。把它判为非法会让「全程淡出」这一常见配置直接不可用。
    /// `Alive → Newborn` 与任何态 → `Newborn` 一律非法（年龄不可回退）。
    pub fn legal_next(self) -> &'static [LifeState] {
        match self {
            LifeState::Newborn => &[LifeState::Alive, LifeState::Fading, LifeState::Dead],
            LifeState::Alive => &[LifeState::Fading, LifeState::Dead],
            LifeState::Fading => &[LifeState::Dead],
            LifeState::Dead => &[],
        }
    }

    /// 转移是否合法。
    pub fn can_next(self, to: LifeState) -> bool {
        self.legal_next().contains(&to)
    }

    /// 按生命进度与淡出配置求态（**纯函数**，四态的唯一定义处）。
    ///
    /// 口径：锚点「t=0 新生 / t∈(0,1) 存活 / t 进入淡出曲线段 / 死亡回收」。
    /// `fade_start` 把`(0,1)` 切成存活段与淡出段。
    pub fn at(t: f32, fade_start: f32) -> LifeState {
        if !(t > 0.0) {
            // t 为 0、负数或 NaN：负进度是上游错误，NaN 不可比较，
            // 两者都归为「尚未出生」——比归为死亡更安全（死亡会被池回收，
            // 误回收一个刚出生的粒子表现为粒子凭空消失）。
            return LifeState::Newborn;
        }
        if t >= 1.0 {
            return LifeState::Dead;
        }
        if t >= fade_start {
            LifeState::Fading
        } else {
            LifeState::Alive
        }
    }
}

/// 态转移校验（零静默：非法转移拒绝并产诊断）。
pub fn request_state_transition(
    from: LifeState,
    to: LifeState,
    bag: &mut DiagBag,
) -> Outcome<LifeState> {
    if from.can_next(to) {
        Outcome::ok(to)
    } else {
        note(
            bag,
            LifeDiag::StateRejected,
            format!("非法态转移 {} -> {}", from.zh(), to.zh()),
            String::from("年龄不可回退，死亡为终态；合法后继见 LifeState::legal_next"),
        );
        Outcome::fail(
            DiagCode::TransitionRejected,
            format!("非法态转移 {} -> {}", from.zh(), to.zh()),
            String::from("四态只沿新生->存活->淡出->死亡 单向推进（全程淡出时新生直跳淡出）"),
        )
    }
}

// ---------------------------------------------------------------------------
// 六、粒子寿命载体
// ---------------------------------------------------------------------------

/// 一次推进的结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LifeAdvance {
    /// 推进后的态。
    pub state: LifeState,
    /// 推进后的生命进度 `t = age / lifetime`（[0,1]，饱和）。
    pub t: f32,
    /// 本次推进是否跨过了寿命终点（**池回收信号**，归 F2208 消费）。
    ///
    /// 「跨过」而非「处于」：一个dt跨过终点的那一帧就该回收，
    /// 否则大dt（掉帧）会让粒子多存活若干帧。
    pub recycled: bool,
}

/// 粒子寿命载体：年龄进度 + 四态 + 死亡行为 + 确定性指纹。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleLife {
    /// 采样得到的寿命（秒）。
    pub lifetime: f32,
    /// 已活年龄（秒）。
    pub age: f32,
    /// 当前态。
    pub state: LifeState,
    /// 死亡行为。
    pub death: DeathBehavior,
    /// 淡出起点（已钳制）。
    pub fade_start: f32,
    /// 推进次数（对拍用：同种子同dt 序列下推进次数必相同）。
    pub steps: u64,
    /// 确定性指纹（逐次推进混入，bitwise 对拍用，见 [`LifetimeLedger`]）。
    pub fingerprint: u64,
}

impl ParticleLife {
    /// 生命进度 `t = age / lifetime`，饱和到 [0,1]。
    ///
    /// 寿命恒 > 0（[`validate_lifetime_dist`] 已拒<= 0），故无除零路径。
    pub fn t(&self) -> f32 {
        let t = self.age / self.lifetime;
        if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    /// 推进一个逻辑步长（零墙钟，`dt` 由调用方注入）。
    ///
    /// 复杂度 **O(1)**。`dt` 非有限或负值时**不计龄**并产诊断——负dt 是
    /// 倒流（时间倒流会让粒子永不死亡），静默吞掉会表现为「粒子卡在屏幕上」。
    pub fn advance(&mut self, dt: f32, bag: &mut DiagBag) -> LifeAdvance {
        if !(dt.is_finite()) || dt < 0.0 {
            note(
                bag,
                LifeDiag::StateRejected,
                format!("推进步长非法（dt={}），本次不计龄", dt),
                String::from("dt 须为有限非负数；负 dt 会让年龄回退，粒子永不死亡"),
            );
            let t = self.t();
            return LifeAdvance { state: self.state, t, recycled: false };
        }
        let was_dead = self.state == LifeState::Dead;
        self.age += dt;
        self.steps += 1;
        let t = self.t();
        let next = LifeState::at(t, self.fade_start);
        if next != self.state {
            // 死亡后不再复活；age 已饱和，at() 必回Dead，此分支只可能在
            // 「已死又被推进」时进入，故此处只处理合法推进。
            self.state = next;
        }
        // 指纹混入 bitwise 量：同种子同 dt 序列必得同指纹（F2215 联动）。
        // 注意 `f32::to_bits()` 给 `u32`，须先零扩展到 u64 再参与 u64 混合——
        // 直接混会静默截断到低 32 位，让高位熵丢失、指纹碰撞概率上升。
        self.fingerprint = fnv1a(
            self.fingerprint,
            (self.age.to_bits() as u64) ^ (t.to_bits() as u64).rotate_left(17)
                ^ (next as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15),
        );
        let recycled = !was_dead && self.state == LifeState::Dead;
        LifeAdvance { state: self.state, t, recycled }
    }

    /// 是否已到寿命终点（池回收判据）。
    pub fn is_expired(&self) -> bool {
        self.state.is_terminal()
    }
}

/// 指纹与台账的 FNV 拌入种子（FNV-1a64 offset basis）。
///
/// 提为具名常量而非散落字面量：指纹的正确性依赖「每个量只在一处被拌入」，
/// 具名种子让这条纪律可grep、可review。
pub const FINGERPRINT_SEED: u64 = 0xCBF2_9CE4_8422_2325;

/// 确定性台账：跨粒子累计指纹，供双跑对拍（锚点：随机源非确定 → 确定性断言）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LifetimeLedger {
    /// 累计指纹。
    pub fingerprint: u64,
    /// 累计粒子数。
    pub particles: u64,
    /// 累计推进次数。
    pub steps: u64,
    /// 累计回收次数。
    pub recycled: u64,
}

impl LifetimeLedger {
    pub fn new() -> LifetimeLedger {
        LifetimeLedger {
            fingerprint: FINGERPRINT_SEED,
            particles: 0,
            steps: 0,
            recycled: 0,
        }
    }

    /// 记入一个粒子（建器时调用）。
    ///
    /// **寿命在此单点拌入**（`life.lifetime.to_bits()`），而
    /// [`ParticleLife::fingerprint`] 只含随机流位置与推进历史。若两处都拌寿命，
    /// 两者 XOR 会相互抵消——指纹将只反映「拌了几次」而非「拌了什么」，
    /// 确定性断言随之退化为恒真。
    ///
    /// 这条纪律的**判据落点**在 `E05-降级-台账对寿命维度正交`：同种子、
    /// 仅改寿命分布，台账必须相异。少了它，把寿命从指纹里整个剔除
    /// （含本函数不拌）也全绿——W005 反假变体实测三种逃逸形态，见
    /// [`create_particle_life`] 头注的补记。
    pub fn admit(&mut self, life: &ParticleLife) {
        self.particles += 1;
        self.fingerprint =
            fnv1a(self.fingerprint, (life.lifetime.to_bits() as u64) ^ life.fingerprint);
    }

    /// 记入一次推进。
    pub fn record(&mut self, adv: &LifeAdvance) {
        self.steps += 1;
        if adv.recycled {
            self.recycled += 1;
        }
        self.fingerprint = fnv1a(
            self.fingerprint,
            (adv.t.to_bits() as u64) ^ (adv.recycled as u64).wrapping_mul(0x1000_0000_1B3),
        );
    }

    /// 合并另一台账（用于分批模拟后对拍）。
    pub fn merge(&mut self, other: &LifetimeLedger) {
        self.fingerprint = fnv1a(self.fingerprint, other.fingerprint);
        self.particles += other.particles;
        self.steps += other.steps;
        self.recycled += other.recycled;
    }

    /// 指纹是否一致（确定性断言的判定口径：比数值不比容差）。
    ///
    /// 逐位一致是刻意的强判据：生命周期是纯标量运算，同输入必同输出，
    /// 出现任何差异都说明有人在中间读了墙钟或全局 RNG。
    pub fn same_as(&self, other: &LifetimeLedger) -> bool {
        self.fingerprint == other.fingerprint
            && self.particles == other.particles
            && self.steps == other.steps
            && self.recycled == other.recycled
    }
}

/// FNV-1a64 混入（单步、确定性、跨平台一致）。
#[inline]
fn fnv1a(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01B3)
}

// ---------------------------------------------------------------------------
// 七、建器：把分布 + 曲线 + 死亡行为装配成粒子寿命
// ---------------------------------------------------------------------------

/// 寿命建器入参。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LifeConfig {
    /// 寿命分布。
    pub dist: LifetimeDist,
    /// 淡出配置。
    pub fade: FadeConfig,
    /// 死亡行为。
    pub death: DeathBehavior,
}

impl LifeConfig {
    pub fn new(dist: LifetimeDist, fade: FadeConfig, death: DeathBehavior) -> LifeConfig {
        LifeConfig { dist, fade, death }
    }
}

/// 建立一个粒子寿命。
///
/// 校验顺序有讲究：先校验**配置**（不变量），再校验**跨域一致性**
/// （死亡行为 vs 曲线），最后才采样。顺序颠倒会在坏配置上白采样一次随机数，
/// 那会污染 RNG 流——同种子对拍随即失效（F2215 确定性根基）。
pub fn create_particle_life(
    cfg: &LifeConfig,
    curves: &CurveSet,
    rng: &mut RandomSource,
    bag: &mut DiagBag,
) -> Outcome<ParticleLife> {
    if let Outcome::Err { code, message, hint } = validate_lifetime_dist(&cfg.dist) {
        note(bag, LifeDiag::LifetimeRejected, message.clone(), hint.clone());
        return Outcome::fail(code, message, hint);
    }
    let fade = cfg.fade.clamped(bag);
    // 跨域一致性：「缩小」要求尺寸曲线终点归零，否则粒子永远不会缩到零——
    // 配了缩小却看起来是凭空消失，是最难查的一类配置缺陷。
    if cfg.death == DeathBehavior::Shrink {
        let end = curves.size.sample(1.0);
        if end > 1.0e-3 {
            note(
                bag,
                LifeDiag::ShrinkUnreachable,
                format!("「缩小」死亡行为不可达：尺寸曲线终点为 {}（应为 0）", end),
                String::from("缩小要求粒子缩到零才回收；请把尺寸曲线终点设为 0，或改用「消失」"),
            );
            return Outcome::fail(
                DiagCode::ShapeRejected,
                format!("「缩小」死亡行为不可达：尺寸曲线终点 {} 非零", end),
                String::from("死亡行为与尺寸曲线不一致；缩小须配终点为 0 的尺寸曲线"),
            );
        }
    }
    let lifetime = cfg.dist.sample(rng);
    // 初始指纹 = FNV 种子 **只拌入随机流位置**，不拌入寿命本身。
    //
    // 寿命由 [`LifetimeLedger::admit`] 单点拌入，指纹只由 `advance` 演化。
    // 理由：同一个量在两处各参与一次 XOR，两次会相互抵消——若这里写成
    // `stream_pos ^ lifetime` 而 `admit` 又拌 `lifetime ^ fingerprint`，
    // 寿命就完全从台账中消失，指纹只认「拌了几次」不认「拌了什么」，
    // 确定性断言随之退化为「两个不同寿命的粒子批指纹相同」。
    //
    // **W005 反假变体实测补记（此条纠正前序注释的错误说法）**：前序注释
    // 声称该缺陷「会导致异种子台账相异判据长红」——**实测不成立**：该判据
    // 靠 `RandomSource`的种子差异分辨批次，而寿命来自采样结果、与种子无
    // 因果关系，故寿命被剔除后它依然全绿。真正的暴露面是「同种子、仅改
    // 寿命」这一维度，此前**无人看守**。已补两条正交判据
    // （`E05-降级-台账对寿命维度正交` / `E05-降级-区间台账对采样值敏感`）
    // 封住该逃逸面，实测三种抵消形态（完全抵消 / admit 不拌 / 分布摘要代替
    // 采样值）全部被捕获。
    //
    // 注意 `let mut seed_rng = *rng` 取的是**副本**：`stream_pos` 故不受
    // 前序采样影响，而常量寿命建器也不白消耗随机数（常量型零消耗、区间型
    // 恰一步，由 `E05-降级-随机流消耗符合分布语义` 钉住）。若改成从 `rng`
    // 本体取，常量型会平移后续所有粒子的寿命序列——「改一处动全批」，
    // 编译不报错。
    let mut seed_rng = *rng;
    let stream_pos = seed_rng.next_f32().to_bits() as u64;
    Outcome::ok(ParticleLife {
        lifetime,
        age: 0.0,
        state: LifeState::Newborn,
        death: cfg.death,
        fade_start: fade.fade_start,
        steps: 0,
        fingerprint: fnv1a(FINGERPRINT_SEED, stream_pos),
    })
}

/// 便捷：全默认寿命（常量 1秒、全程淡出、消失）。
pub fn default_life() -> LifeConfig {
    LifeConfig::new(
        LifetimeDist::Constant(1.0),
        FadeConfig::WHOLE_LIFE,
        DeathBehavior::Vanish,
    )
}

/// 退化为等价的默认寿命配置（供需要 `Copy` 默认值的调用点）。
pub fn default_life_config() -> LifeConfig {
    default_life()
}

/// 全命周期推进一个粒子到死亡（便捷路径，仍O(1)/步）。
///
/// 返回是否已回收。步数上限是**硬门**：dt 极小时步数会爆炸（dt=1e-6 且寿命 1 秒
/// 需一百万步），无上限就是一次隐藏的死循环——上限触发即产诊断，不静默截断。
pub const MAX_ADVANCE_STEPS: u64 = 4_000_000;

pub fn advance_to_death(
    life: &mut ParticleLife,
    dt: f32,
    bag: &mut DiagBag,
) -> (bool, u64) {
    let mut steps = 0u64;
    loop {
        if steps >= MAX_ADVANCE_STEPS {
            note(
                bag,
                LifeDiag::StateRejected,
                format!("推进步数达上限 {}仍未死亡（寿命 {}，dt {}）", MAX_ADVANCE_STEPS, life.lifetime, dt),
                String::from("dt 相对寿命过小；请放大 dt 或缩短寿命，避免每帧步数爆炸"),
            );
            return (life.is_expired(), steps);
        }
        let adv = life.advance(dt, bag);
        steps += 1;
        if adv.recycled || life.is_expired() {
            return (true, steps);
        }
    }
}

/// 无值成功构造的公开别名（供调用点做 `Outcome<()>` 成功值，避免各自造一个）。
pub fn ok() -> Outcome<()> {
    ok_unit()
}
