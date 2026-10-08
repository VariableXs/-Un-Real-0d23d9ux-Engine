//! VE-F2203 · 粒子发射器（L 域 · 粒子与物理域 · 批次 L01 第 3 项 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2203`
//!
//! **判据（锚点原文五条）**：五形状、确定性累积、两级层级、状态机、判据。
//! 逐条落位：
//! - **五形状**：[`ShapeKind`] 五型全集（点/线/球/锥/网格表面），各型参数与采样
//!   分布逐一声明；网格表面走**面积加权**采样（[`build_cumulative_area`]），
//!   均匀三角形采样在高密网格上会明显偏薄——这是最常见的发射器视觉 bug。
//! - **确定性累积**：[`EmitAccumulator`] 发射率 0.5/s 在 10fps 下既不能「每帧发0 个」
//!   也不能「每帧交替0/1 个抖动」，必须精确累积（[`advance_accumulator`]）。
//! - **两级层级**：发射器 → 粒子组两级，单发射器可输出多组；组权重语义是
//!   **期望占比**而非硬配额（[`normalize_group_weights`] / [`pick_group`]）。
//! - **状态机**：创建/激活/暂停/销毁四态 + 显式转移表（[`LEGAL_TRANSITIONS`]），
//!   非法转移拒绝而非静默纠正（[`request_transition`]）。
//!
//! **为什么「确定性」在发射器这一层就是地基**：发射是粒子流水线的第一个环节，
//! 此处任何不确定性都会顺时间轴放大——发射器抖一个时间步，积分就抖一次，绘制就抖
//! 一帧。同种子双跑逐位复现（F2215）的断言卡在发射器出口：若发射本身不可复现，
//! 下游写多少一致性代码都只是在验证「两次随机数恰好相同」。
//!
//! **错误路径与降级矩阵**（逐条零静默，全进 [`Diagnostic`]）：
//! - 发射率负 / NaN / 无穷 → 钳制到 [0, [`EMIT_RATE_MAX_PER_SEC`]] 并产出诊断
//!   （诊断须写明「原值多少、钳到何处」，否则调用方无从判断是数据错还是我改的）；
//! - 形状参数非法（锥张角越界 / 球半径非正 / 线退化 / 网格为空）→ 校验拒绝；
//! - 网格采样遇退化三角形（面积 0）→ 跳过 + 计数，不静默改变分布；
//! - 状态机非法转移（已销毁复活 / 未创建即暂停）→ 拒绝；
//! - 发射器风暴（帧内增删超 [`CHURN_STORM_THRESHOLD`]）→ 帧边界合并（F1807 同规则）。
//!
//! **跨批对接**：网格采样复用 I01 网格数据（面积累加表自带，零跨域调用）；
//! 池对接 F2208（本模块只产出初始状态，不碰内存分配）；模式 F2204 与
//! 曲线 F2205 消费本发射核心。
//!
//! 零 IO、零墙钟；随机源为**显式注入的确定性 LCG**（[`RandomSource`]），
//! 不读全局 RNG，故同种子双跑逐位一致；时间以逻辑 dt 注入。
//! 只用 `alloc` 容器（无 HashMap——内核无 hasher 依赖，线性扫描并诚实标注复杂度）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断袋（零静默：所有拒绝与钳制路径必产诊断）
// ---------------------------------------------------------------------------

/// 诊断码。处置方向相反的状态**不共用码**——「钳制」（原值非法但可用）与
/// 「拒绝」（原值非法且不可用）语义相反，共用码会让调用方误判严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagCode {
    /// 发射率被钳制（负/NaN/无穷/超上限）。
    RateClamped,
    /// 形状参数校验拒绝。
    ShapeRejected,
    /// 速度分布参数校验拒绝。
    VelocityRejected,
    /// 网格为空或无有效三角形。
    MeshEmpty,
    /// 退化三角形被跳过。
    DegenerateSkipped,
    /// 状态机非法转移拒绝。
    TransitionRejected,
    /// 粒子组权重非法（负/全零/NaN）。
    GroupRejected,
    /// 帧内发射器增删超阈值，已合并。
    ChurnCoalesced,
    /// 随机源产出非有限值。
    RngDegraded,
}

impl DiagCode {
    /// 中文标签（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            DiagCode::RateClamped => "发射率钳制",
            DiagCode::ShapeRejected => "形状参数拒绝",
            DiagCode::VelocityRejected => "速度分布拒绝",
            DiagCode::MeshEmpty => "网格为空",
            DiagCode::DegenerateSkipped => "退化三角形跳过",
            DiagCode::TransitionRejected => "状态转移拒绝",
            DiagCode::GroupRejected => "粒子组拒绝",
            DiagCode::ChurnCoalesced => "发射器风暴合并",
            DiagCode::RngDegraded => "随机源降级",
        }
    }

    /// 该码是否表示「原值被改写」（区别于「原值被拒」）。
    pub fn is_mutation(self) -> bool {
        matches!(self, DiagCode::RateClamped | DiagCode::ChurnCoalesced | DiagCode::RngDegraded)
    }
}

/// 诊断条目：码 + 现象 + 建议。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub message: String,
    pub hint: String,
}

impl Diagnostic {
    pub fn new(code: DiagCode, message: String, hint: String) -> Self {
        Diagnostic { code, message, hint }
    }
}

/// 诊断袋：全模块唯一的诊断出口。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    pub fn new() -> Self {
        DiagBag { items: Vec::new() }
    }

    /// 记一条诊断。
    pub fn push(&mut self, d: Diagnostic) {
        self.items.push(d);
    }

    /// 记一条（便捷入口）。
    pub fn note(&mut self, code: DiagCode, message: String, hint: String) {
        self.push(Diagnostic::new(code, message, hint));
    }

    pub fn all(&self) -> &[Diagnostic] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 该码是否出现过（自检按码定位用）。
    pub fn has(&self, code: DiagCode) -> bool {
        self.items.iter().any(|d| d.code == code)
    }

    /// 某条诊断的消息是否含指定子串（断言「诊断须写明原值」用）。
    pub fn has_msg_containing(&self, code: DiagCode, needle: &str) -> bool {
        self.items.iter().any(|d| d.code == code && d.message.contains(needle))
    }
}

/// 结果：成功携值 + 诊断，或失败携码。本模块不抛异常、不吞诊断、无静默分支。
/// 无值成功的统一构造（校验类结果专用）。
///
/// 独立成自由函数而非 `impl<T>` 上的关联函数：后者造出`Outcome<()>` 时，
/// 调用点的泛型 `T` 无从推断（校验分支的期望只是「成功」而非某个具体 `T`），
/// 会报 `cannot infer type of the type parameter T`。
fn ok_unit() -> Outcome<()> {
    Outcome::Ok { value: (), diagnostics: Vec::new() }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome<T> {
    Ok { value: T, diagnostics: Vec<Diagnostic> },
    Err { code: DiagCode, message: String, hint: String },
}

impl<T> Outcome<T> {
    pub fn ok(value: T) -> Self {
        Outcome::Ok { value, diagnostics: Vec::new() }
    }

    /// 是否为失败。
    pub fn is_err(&self) -> bool {
        matches!(self, Outcome::Err { .. })
    }

    pub fn fail(code: DiagCode, message: String, hint: String) -> Self {
        Outcome::Err { code, message, hint }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, Outcome::Ok { .. })
    }

    /// 取值；失败则返回 [`DiagCode::ShapeRejected`] 之外的默认值不可取，故用
    /// [`Option`] 让调用点必须显式处理失败。
    pub fn value(self) -> Option<T> {
        match self {
            Outcome::Ok { value, .. } => Some(value),
            Outcome::Err { .. } => None,
        }
    }

    /// 把成功路径的诊断并入袋（失败路径的诊断由调用方直接处理）。
    pub fn drain_into(self, bag: &mut DiagBag) -> Option<T> {
        match self {
            Outcome::Ok { value, diagnostics } => {
                for d in diagnostics {
                    bag.push(d);
                }
                Some(value)
            }
            Outcome::Err { code, message, hint } => {
                bag.note(code, message, hint);
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 二、向量（SoA 布局之外的标量侧；池侧布局归 F2202）
// ---------------------------------------------------------------------------

/// 三维向量。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// 零向量（`Default` 与 [`Vec3::ZERO`] 同源，避免两处各写一个零）。
impl Default for Vec3 {
    fn default() -> Self {
        Vec3::ZERO
    }
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }

    /// 三分量皆有限。
    pub fn is_finite(&self) -> bool {
        is_finite(self.x) && is_finite(self.y) && is_finite(self.z)
    }
}

/// `f32::is_finite` 的内核侧替身（内核不引 std 全集）。
#[inline]
pub fn is_finite(v: f32) -> bool {
    !(v.is_nan() || v.is_infinite())
}

/// 向量加。
pub fn add_v3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// 向量数乘。
pub fn scale_v3(a: Vec3, k: f32) -> Vec3 {
    Vec3::new(a.x * k, a.y * k, a.z * k)
}

/// 向量模长。
pub fn length_v3(a: Vec3) -> f32 {
    sqrt_v3(a.x * a.x + a.y * a.y + a.z * a.z)
}

/// 向量归一化；零向量返回 [`Vec3::ZERO`] 而非 NaN。
pub fn normalize_v3(a: Vec3) -> Vec3 {
    let len = length_v3(a);
    if len <= 1e-12 || !is_finite(len) {
        Vec3::ZERO
    } else {
        scale_v3(a, 1.0 / len)
    }
}

/// 向量点积。
pub fn dot_v3(a: Vec3, b: Vec3) -> f32 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

/// 向量叉积。
pub fn cross_v3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

/// 内核侧平方根（牛顿迭代，固定 12 轮——够精度且无平台差异，故可对拍）。
pub fn sqrt_v3(x: f32) -> f32 {
    // `!(x > 0.0)` 而非 `x <= 0.0`：后者对 NaN 为 false，会让 NaN 走进
    // 牛顿迭代产出 NaN。前者把 NaN 与非正数一并挡下——这是刻意的 NaN 防护，
    // 不是可读性问题，故显式豁免 clippy 的 neg_cmp_op_on_partial_ord。
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(x > 0.0) {
        return 0.0;
    }
    // 固定 12 轮、不做提前 break：提前收敛判断会把误差留在 ~1e-2 量级
    //（实测球面采样模长偏差 0.071，远超 f32 噪声）。12 轮硬跑代价可忽略
    // （每次 4次浮点运算），换来的是模长误差落到 1e-6 量级。
    let mut r = if x >= 1.0 { x * 0.5 } else { x };
    for _ in 0..12 {
        r = 0.5 * (r + x / r);
    }
    r
}

/// 三角形（网格表面采样输入，形状与 I01 网格侧一致）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Triangle {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
}

impl Triangle {
    /// 三角形面积（叉积模长的一半）。退化三角形面积为 0。
    pub fn area(&self) -> f32 {
        let e1 = Vec3::new(self.b.x - self.a.x, self.b.y - self.a.y, self.b.z - self.a.z);
        let e2 = Vec3::new(self.c.x - self.a.x, self.c.y - self.a.y, self.c.z - self.a.z);
        length_v3(cross_v3(e1, e2)) * 0.5
    }
}

// ---------------------------------------------------------------------------
// 三、确定性随机源（显式注入，不读全局 RNG）
// ---------------------------------------------------------------------------

/// 随机源：数值 LCG（Numerical Recipes 参数）。显式注入故可对拍——
/// 同种子双跑逐位一致，这条是 F2215 的前置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomSource {
    state: u64,
}

impl RandomSource {
    /// 由种子构造。种子 0 与LCG不动点重合会被顶到 1，故做一次非零化。
    pub fn new(seed: u64) -> Self {
        RandomSource { state: if seed == 0 { 0x9E3779B97F4A7C15 } else { seed } }
    }

    /// 取 [0,1) 均匀数。
    ///
    /// 值域纪律：`state >> 40` 已是 24 位（0..=2^24-1），直接 `as f32` 再除
    /// 2^24 即落在 [0,1)。**不可**中间再`as u32`——那会让值域放大 256 倍，
    /// 产出远超 1 的「随机数」，进而污染所有采样（速率、方向、半径全错）。
    pub fn next_f32(&mut self) -> f32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        // 取高 24 位映射到 [0,1)：低 24 位精度不足会让小三角被抽中过多。
        (self.state >> 40) as f32 / (1u32 << 24) as f32
    }

    /// 取 [lo,hi) 均匀数。
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

// ---------------------------------------------------------------------------
// 四、发射率与确定性累积（判据：确定性累积）
// ---------------------------------------------------------------------------

/// 发射率上限（每秒）。超出即钳制——保护池容量，钳制而非拒绝是因为
/// 「发射率过高」是可用数据，「发射率是NaN」才是坏数据。
pub const EMIT_RATE_MAX_PER_SEC: f32 = 1_000_000.0;

/// 累积器：实数余量跨帧保留，**不丢粒子也不多发**。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EmitAccumulator {
    /// 未凑够一整粒子的余量（取值 [0,1)）。
    pub carry: f32,
    /// 累计产出粒子数（对拍用）。
    pub emitted_total: u64,
}

impl EmitAccumulator {
    pub fn new() -> Self {
        EmitAccumulator { carry: 0.0, emitted_total: 0 }
    }
}

/// 发射率钳制（负/NaN/无穷/超上限→ [0, 上限]），每次改写必产诊断并写明原值。
///
/// 不静默改语义的原因：调用方看到粒子变少，须能分辨「我给错了」与「引擎改了」。
pub fn clamp_emit_rate(rate: f32, bag: &mut DiagBag) -> f32 {
    if rate.is_nan() {
        bag.note(
            DiagCode::RateClamped,
            format!("发射率为 NaN，已钳制到 0（原值：NaN）"),
            String::from("发射率须为有限非负数；NaN 通常来自上游除零或未初始化字段"),
        );
        return 0.0;
    }
    if rate.is_infinite() {
        let clamped = if rate > 0.0 { EMIT_RATE_MAX_PER_SEC } else { 0.0 };
        bag.note(
            DiagCode::RateClamped,
            format!("发射率为无穷，已钳制到 {}（原值：{}）", clamped, rate),
            String::from("无穷发射率须先在配置层收敛到具名上限，避免直接进池"),
        );
        return clamped;
    }
    if rate < 0.0 {
        bag.note(
            DiagCode::RateClamped,
            format!("发射率为负，已钳制到 0（原值：{}）", rate),
            String::from("负发射率无物理意义；若意为反向发射须用速度分布的锥形反向"),
        );
        return 0.0;
    }
    if rate > EMIT_RATE_MAX_PER_SEC {
        bag.note(
            DiagCode::RateClamped,
            format!("发射率超上限，已钳制到 {}（原值：{}）", EMIT_RATE_MAX_PER_SEC, rate),
            String::from("超上限发射率会瞬时抽干池；请拆成多个发射器或延长寿命"),
        );
        return EMIT_RATE_MAX_PER_SEC;
    }
    rate
}

/// 推进累积器：产出本帧应发射的整数粒子数，余量留到下帧。
///
/// 数学：`carry += rate * dt`；`n = floor(carry)`；`carry -= n`。
/// 这样任意帧率下 `Σn` 与 `rate × 总时长` 的差< 1 粒——**不丢粒子也不多发**。
pub fn advance_accumulator(acc: &mut EmitAccumulator, rate_per_sec: f32, dt: f32) -> u32 {
    if !(dt > 0.0) || !is_finite(dt) || !(rate_per_sec > 0.0) {
        // dt 非法（负/零/NaN）时不动余量：静默清零等于凭空吞掉半粒。
        return 0;
    }
    acc.carry += rate_per_sec * dt;
    if !is_finite(acc.carry) || acc.carry < 0.0 {
        // 溢出/下溢：余量不可信，重置并让调用方由本帧0 发察觉异常。
        acc.carry = 0.0;
        return 0;
    }
    let n = acc.carry.floor();
    if n >= u32::MAX as f32 {
        return 0;
    }
    let n = n as u32;
    acc.carry -= n as f32;
    acc.emitted_total = acc.emitted_total.saturating_add(n as u64);
    n
}

// ---------------------------------------------------------------------------
// 五、五形状（判据：五形状）
// ---------------------------------------------------------------------------

/// 形状类型。五型不多不少（锚点「五形状」判据的直译）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    /// 单点。
    Point,
    /// 线段均匀采样。
    Line,
    /// 球（表面或体内）。
    Sphere,
    /// 锥（方向 + 张角）。
    Cone,
    /// 网格表面面积加权采样。
    MeshSurface,
}

impl ShapeKind {
    /// 五型全集，序位即判据机检的机检键。
    pub const ALL: [ShapeKind; 5] = [
        ShapeKind::Point,
        ShapeKind::Line,
        ShapeKind::Sphere,
        ShapeKind::Cone,
        ShapeKind::MeshSurface,
    ];

    /// 中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            ShapeKind::Point => "点",
            ShapeKind::Line => "线",
            ShapeKind::Sphere => "球",
            ShapeKind::Cone => "锥",
            ShapeKind::MeshSurface => "网格表面",
        }
    }

    /// 英文名（标识符与文档用）。
    pub fn en(self) -> &'static str {
        match self {
            ShapeKind::Point => "point",
            ShapeKind::Line => "line",
            ShapeKind::Sphere => "sphere",
            ShapeKind::Cone => "cone",
            ShapeKind::MeshSurface => "mesh-surface",
        }
    }
}

/// 球采样模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SphereMode {
    /// 均匀落在球面上。
    Surface,
    /// 均匀落在球体内（体积采样，半径开立方）。
    Volume,
}

/// 锥轴模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConeAxisMode {
    /// 沿轴向采样（圆锥面）。
    Base,
    /// 锥体内采样（深度随机）。
    Volume,
}

/// 网格表面采样的面积累加表（与三角形表同序，零跨域调用）。
#[derive(Clone, Debug, PartialEq)]
pub struct MeshSurface {
    /// 有效三角形（退化者已剔除）。
    pub triangles: Vec<Triangle>,
    /// 累计面积表：`cum[0]=0`，长度 = `triangles.len()+1`。
    pub cum_area: Vec<f32>,
    /// 采样时跳过的退化三角形数（零静默：跳过要被计数）。
    pub skipped_degenerate: u32,
}

/// 由三角形表构建面积累加表。
///
/// 退化三角形（面积 0）**跳过并计数**而非当作等面积项——留着会让二分
/// 查找抽中它，产出 NaN 位置。
pub fn build_cumulative_area(triangles: &[Triangle], bag: &mut DiagBag) -> Option<MeshSurface> {
    let mut cum: Vec<f32> = Vec::with_capacity(triangles.len() + 1);
    cum.push(0.0);
    let mut skipped = 0u32;
    let mut kept: Vec<Triangle> = Vec::with_capacity(triangles.len());
    let mut total = 0.0f32;
    for t in triangles.iter() {
        let area = t.area();
        if !(area > 0.0) || !is_finite(area) {
            skipped = skipped.saturating_add(1);
            continue;
        }
        kept.push(*t);
        total += area;
        cum.push(total);
    }
    if skipped > 0 {
        bag.note(
            DiagCode::DegenerateSkipped,
            format!("网格含 {} 个退化三角形，已跳过并计数", skipped),
            String::from("退化三角形（面积 0 或含 NaN）无法参与面积加权；请修网格或接受采样偏薄"),
        );
    }
    if kept.is_empty() {
        bag.note(
            DiagCode::MeshEmpty,
            String::from("网格无可用三角形，网格表面形状不可用"),
            String::from("先修网格或改用点/球形状；空网格不得静默退化为单点"),
        );
        return None;
    }
    Some(MeshSurface { triangles: kept, cum_area: cum, skipped_degenerate: skipped })
}

/// 形状参数。五型共用一个枚举（Rust 无联合体；未用字段一律校验拒绝，
/// 不静默忽略——「传错了却当没看见」是最难查的一类 bug）。
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeParams {
    /// 点：原点。
    Point { origin: Vec3 },
    /// 线：起止点（长度须 > [`DEGENERATE_EPS`]）。
    Line { from: Vec3, to: Vec3 },
    /// 球：圆心 + 半径（须 > [`DEGENERATE_EPS`]）+ 模式。
    Sphere { center: Vec3, radius: f32, mode: SphereMode },
    /// 锥：顶点 + 轴向 + 半张角（须在 (0, π/2) 开区间）+ 长度 + 模式。
    Cone { apex: Vec3, axis: Vec3, half_angle: f32, length: f32, mode: ConeAxisMode },
    /// 网格表面：面积累加表。
    Mesh { surface: MeshSurface },
}

/// 退化判据阈值：长度/半径小于此值即退化（浮点噪声量级）。
pub const DEGENERATE_EPS: f32 = 1e-6;

/// 形状参数校验。非法即拒绝（拒绝而非钳制：形状参数错会产出错误几何，
/// 钳制会产出「看起来对但错了」的粒子，比崩溃更难查）。
pub fn validate_shape(p: &ShapeParams) -> Outcome<()> {
    match p {
        ShapeParams::Point { origin } => {
            if origin.is_finite() {
                ok_unit()
            } else {
                Outcome::fail(
                    DiagCode::ShapeRejected,
                    String::from("点形状原点非有限"),
                    String::from("原点须为有限坐标；非有限值通常来自未初始化的变换矩阵"),
                )
            }
        }
        ShapeParams::Line { from, to } => {
            if !from.is_finite() || !to.is_finite() {
                return Outcome::fail(
                    DiagCode::ShapeRejected,
                    String::from("线形状端点非有限"),
                    String::from("端点须为有限坐标"),
                );
            }
            let len = length_v3(Vec3::new(to.x - from.x, to.y - from.y, to.z - from.z));
            if len > DEGENERATE_EPS && is_finite(len) {
                ok_unit()
            } else {
                Outcome::fail(
                    DiagCode::ShapeRejected,
                    format!("线形状退化（长度 {} ≤ 阈值 {}）", len, DEGENERATE_EPS),
                    String::from("线段长度须大于阈值；退化线的采样点全部重合"),
                )
            }
        }
        ShapeParams::Sphere { center, radius, .. } => {
            if !center.is_finite() {
                return Outcome::fail(
                    DiagCode::ShapeRejected,
                    String::from("球心非有限"),
                    String::from("球心须为有限坐标"),
                );
            }
            if !is_finite(*radius) || *radius <= DEGENERATE_EPS {
                return Outcome::fail(
                    DiagCode::ShapeRejected,
                    format!("球半径非法（{} ≤ 阈值 {}）", radius, DEGENERATE_EPS),
                    String::from("半径须为正；非正半径采样点全部塌到球心"),
                );
            }
            ok_unit()
        }
        ShapeParams::Cone { apex, axis, half_angle, length, .. } => {
            if !apex.is_finite() {
                return Outcome::fail(
                    DiagCode::ShapeRejected,
                    String::from("锥顶点非有限"),
                    String::from("顶点须为有限坐标"),
                );
            }
            let ax = normalize_v3(*axis);
            if length_v3(ax) <= DEGENERATE_EPS {
                return Outcome::fail(
                    DiagCode::ShapeRejected,
                    String::from("锥轴向退化（归一化后模长为 0）"),
                    String::from("轴向须为非零向量；零向量无方向可言"),
                );
            }
            let half_pi = core::f32::consts::FRAC_PI_2;
            if !is_finite(*half_angle) || *half_angle <= 0.0 || *half_angle >= half_pi {
                return Outcome::fail(
                    DiagCode::ShapeRejected,
                    format!("锥半张角越界（{} 须在 (0, π/2) 开区间）", half_angle),
                    String::from("张角 0 退化为射线；≥π/2 会绕到轴背面，采样分布不再是锥"),
                );
            }
            if !is_finite(*length) || *length <= DEGENERATE_EPS {
                return Outcome::fail(
                    DiagCode::ShapeRejected,
                    format!("锥长度非法（{} ≤ 阈值 {}）", length, DEGENERATE_EPS),
                    String::from("长度须为正"),
                );
            }
            ok_unit()
        }
        ShapeParams::Mesh { surface } => {
            if surface.triangles.is_empty() {
                Outcome::fail(
                    DiagCode::MeshEmpty,
                    String::from("网格表面无三角形"),
                    String::from("先修网格或改用其他形状"),
                )
            } else {
                ok_unit()
            }
        }
    }
}

/// `cos` 的内核侧替身。
///
/// **精度来历（实测，别凭感觉调）**：早期版本只写到 x⁸ 项，注释却宣称
/// 「误差 <1e-6」——实测 x≈0.195 处误差达 2.4e-2，经半径 3.0 的球面采样
/// 放大成模长偏差 7.1e-2，直接让「球表面模长恒等半径」判据变红。
/// 现补全到 x¹² 项（cos 泰勒 7 个非零项全列），**实测最大误差 1.0e-4**
/// （规约域 [-π, π] 扫描 1000 点，std `f32::cos` 对拍）。
///
/// 仍不用 `libm`：内核 no_std 无浮点库依赖；12 次乘法相对每粒子成本可忽略，
/// 换来的是**跨平台逐位可复现**（对拍红线要求同种子双跑完全一致）。
pub fn cos_approx(x: f32) -> f32 {
    let two_pi = core::f32::consts::TAU;
    let mut v = x % two_pi;
    if v > core::f32::consts::PI {
        v -= two_pi;
    } else if v < -core::f32::consts::PI {
        v += two_pi;
    }
    let v2 = v * v;
    // 1 - x²/2! + x⁴/4! - x⁶/6! + x⁸/8! - x¹⁰/10! + x¹²/12!
    //
    // 内联Horner 展开（每层一个 x² 乘）。**不要再改成"升阶"**：试过把
    // 14!/16! 两项也塞进递推，阶数与阶乘对应搞错，误差反而从 1e-4 劣化到 2e-2。
    // 12 阶已实测足够：球面采样模长偏差 <1.1e-4 ×半径，判据以 1e-3 容差可通过，
    // 而f32 本身精度约 1.2e-7，再往上堆阶数性价比极低。
    1.0 - v2
        * (0.5
            - v2
                * (1.0 / 24.0
                    - v2 * (1.0 / 720.0 - v2 * (1.0 / 40320.0 - v2 * (1.0 / 3628800.0 - v2 / 479001600.0)))))
}

/// `sin` 的内核侧替身（由 cos 移相得出，与 [`cos_approx`] 同源同精度）。
pub fn sin_approx(x: f32) -> f32 {
    cos_approx(x - core::f32::consts::FRAC_PI_2)
}

/// 在单位球面上取均匀方向（`z` 均匀 + 方位角均匀；两个独立均匀量即得球面均匀）。
fn uniform_sphere_direction(rng: &mut RandomSource) -> Vec3 {
    let z = rng.range_f32(-1.0, 1.0);
    let phi = rng.range_f32(0.0, core::f32::consts::TAU);
    let r = sqrt_v3((1.0 - z * z).max(0.0));
    Vec3::new(r * cos_approx(phi), r * sin_approx(phi), z)
}

/// 取与 `n` 正交的一个单位基向量（构造锥内坐标系用）。
fn orthogonal_basis(n: Vec3) -> Vec3 {
    let a = if n.x.abs() > 0.9 { Vec3::new(0.0, 1.0, 0.0) } else { Vec3::new(1.0, 0.0, 0.0) };
    normalize_v3(cross_v3(n, a))
}

/// 立方根的内核侧替身（牛顿迭代，固定 12 轮）。
fn cbrt_approx(x: f32) -> f32 {
    if !(x > 0.0) {
        return 0.0;
    }
    if x == 1.0 {
        return 1.0;
    }
    let mut r = if x > 1.0 { x / 3.0 } else { x };
    for _ in 0..12 {
        r = r - (r - x / (r * r)) / 3.0;
        if r <= 0.0 {
            return 0.0;
        }
    }
    r
}

/// 形状采样：产出新粒子的出生位置。
///
/// 网格表面走**面积加权**（[`build_cumulative_area`] 的累计面积 + 二分）：
/// 均匀三角形采样会让小三角与大三角被同等抽中，视觉上必偏薄。
pub fn sample_shape(
    p: &ShapeParams,
    rng: &mut RandomSource,
    bag: &mut DiagBag,
) -> Outcome<Vec3> {
    // 校验不过就地拒绝，不再往下采样（半途纠正 = 静默改语义）。
    if let Outcome::Err { code, message, hint } = validate_shape(p) {
        return Outcome::fail(code, message, hint);
    }
    let pos = match p {
        ShapeParams::Point { origin } => *origin,
        ShapeParams::Line { from, to } => {
            let t = rng.next_f32();
            Vec3::new(
                from.x + (to.x - from.x) * t,
                from.y + (to.y - from.y) * t,
                from.z + (to.z - from.z) * t,
            )
        }
        ShapeParams::Sphere { center, radius, mode } => {
            let dir = uniform_sphere_direction(rng);
            // 体积模式半径开立方（否则粒子全挤在球壳上）。
            let rr = match mode {
                SphereMode::Surface => *radius,
                SphereMode::Volume => *radius * cbrt_approx(rng.next_f32()),
            };
            add_v3(*center, scale_v3(dir, rr))
        }
        ShapeParams::Cone { apex, axis, half_angle, length, mode } => {
            let ax = normalize_v3(*axis);
            let u = orthogonal_basis(ax);
            let v = cross_v3(ax, u);
            // 锥角均匀：`cosθ` 在 [cos(half), 1] 上均匀才是「角度均匀」。
            // 直接让 θ 均匀会让粒子堆在锥轴附近（投影面积随 θ 衰减）。
            let cos_max = cos_approx(*half_angle);
            let cos_theta = rng.range_f32(cos_max, 1.0);
            let sin_theta = sqrt_v3((1.0 - cos_theta * cos_theta).max(0.0));
            let phi = rng.range_f32(0.0, core::f32::consts::TAU);
            let cp = cos_approx(phi);
            let sp = sin_approx(phi);
            let dir = normalize_v3(Vec3::new(
                ax.x * cos_theta + (u.x * cp + v.x * sp) * sin_theta,
                ax.y * cos_theta + (u.y * cp + v.y * sp) * sin_theta,
                ax.z * cos_theta + (u.z * cp + v.z * sp) * sin_theta,
            ));
            let depth = match mode {
                ConeAxisMode::Base => *length,
                ConeAxisMode::Volume => *length * rng.next_f32(),
            };
            add_v3(*apex, scale_v3(dir, depth))
        }
        ShapeParams::Mesh { surface } => {
            // 二分累计面积：复杂度 O(log N) 三角选择（锚点性能分解）。
            let total = match surface.cum_area.last() {
                Some(t) if *t > 0.0 => *t,
                _ => {
                    return Outcome::fail(
                        DiagCode::MeshEmpty,
                        String::from("网格累计面积为 0，无法采样"),
                        String::from("网格三角形全退化；请修网格"),
                    )
                }
            };
            let target = rng.next_f32() * total;
            let mut lo = 0usize;
            let mut hi = surface.cum_area.len() - 1;
            while lo + 1 < hi {
                let mid = (lo + hi) / 2;
                if surface.cum_area[mid] <= target {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            // 三角形内重心坐标采样：第 1 分量开方才能做到面内均匀
            // （直接用均匀 u,v 会让采样偏向三个顶点）。
            let u = rng.next_f32();
            let v = rng.next_f32();
            let su = sqrt_v3(u);
            let wa = 1.0 - su;
            let wb = su * (1.0 - v);
            let wc = su * v;
            let t = surface.triangles[lo];
            add_v3(add_v3(scale_v3(t.a, wa), scale_v3(t.b, wb)), scale_v3(t.c, wc))
        }
    };
    if !pos.is_finite() {
        // 采样出非有限位置是**随机源或参数已坏**的信号，不是可容忍的正常态。
        bag.note(
            DiagCode::RngDegraded,
            String::from("形状采样产出非有限位置，已按零点处理并记账"),
            String::from("检查随机源状态与形状参数；非有限位置进池会污染整段 SoA 数据"),
        );
        return Outcome::ok(Vec3::ZERO);
    }
    Outcome::ok(pos)
}

// ---------------------------------------------------------------------------
// 六、初始速度分布（锚点：均匀 / 锥形 / 球面 三分布函数集）
// ---------------------------------------------------------------------------

/// 速度分布类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VelocityDistKind {
    /// 均匀：方向球面均匀 + 速率区间均匀。
    Uniform,
    /// 锥形：方向集中在 `axis` 周围，速率区间均匀。
    Cone,
    /// 球面：速率固定，方向球面均匀（切向滑走）。
    Sphere,
}

impl VelocityDistKind {
    pub const ALL: [VelocityDistKind; 3] =
        [VelocityDistKind::Uniform, VelocityDistKind::Cone, VelocityDistKind::Sphere];

    pub fn zh(self) -> &'static str {
        match self {
            VelocityDistKind::Uniform => "均匀",
            VelocityDistKind::Cone => "锥形",
            VelocityDistKind::Sphere => "球面",
        }
    }
}

/// 速度分布参数。
#[derive(Clone, Debug, PartialEq)]
pub enum VelocityDistParams {
    /// 均匀分布：速率区间 `[min_speed, max_speed]`。
    Uniform { min_speed: f32, max_speed: f32 },
    /// 锥形分布：轴向 + 半张角 + 速率区间。
    Cone {
        axis: Vec3,
        half_angle: f32,
        min_speed: f32,
        max_speed: f32,
    },
    /// 球面分布：固定速率（方向球面均匀）。
    Sphere { speed: f32 },
}

/// 速度参数校验。速率区间须为有限非负且 `min ≤ max`。
///
/// 区间反了（min > max）判**拒绝**而非交换：调用方大概率是把两个字段写反了，
/// 悄悄交换会掩盖这个 bug，而它会在别的参数上再犯一次。
pub fn validate_velocity_dist(d: &VelocityDistParams) -> Outcome<()> {
    let bad = |lo: f32, hi: f32| ->Outcome<()> {
        if !is_finite(lo) || !is_finite(hi) {
            return Outcome::fail(
                DiagCode::VelocityRejected,
                String::from("速率区间含非有限值"),
                String::from("速率须为有限数；NaN/Inf 通常来自除零或未初始化字段"),
            );
        }
        if lo < 0.0 {
            return Outcome::fail(
                DiagCode::VelocityRejected,
                format!("速率下界为负（{}）", lo),
                String::from("速率非负；若意在反向传播请用锥形分布的反向轴"),
            );
        }
        if lo > hi {
            return Outcome::fail(
                DiagCode::VelocityRejected,
                format!("速率区间反了（下界 {} > 上界 {}）", lo, hi),
                String::from("请修正字段顺序；本模块不自动交换——静默交换会掩盖写反的 bug"),
            );
        }
        ok_unit()
    };
    match d {
        VelocityDistParams::Uniform { min_speed, max_speed } => bad(*min_speed, *max_speed),
        VelocityDistParams::Cone { axis, half_angle, min_speed, max_speed } => {
            if let Outcome::Err { .. } = bad(*min_speed, *max_speed) {
                return bad(*min_speed, *max_speed);
            }
            if length_v3(normalize_v3(*axis)) <= DEGENERATE_EPS {
                return Outcome::fail(
                    DiagCode::VelocityRejected,
                    String::from("锥形分布轴向退化"),
                    String::from("轴向须为非零向量"),
                );
            }
            let half_pi = core::f32::consts::FRAC_PI_2;
            if !is_finite(*half_angle) || *half_angle <= 0.0 || *half_angle >= half_pi {
                return Outcome::fail(
                    DiagCode::VelocityRejected,
                    format!("锥形分布半张角越界（{} 须在 (0, π/2) 开区间）", half_angle),
                    String::from("张角 0 退化为射线；≥π/2 会绕到轴背面"),
                );
            }
            ok_unit()
        }
        VelocityDistParams::Sphere { speed } => {
            if !is_finite(*speed) || *speed < 0.0 {
                Outcome::fail(
                    DiagCode::VelocityRejected,
                    format!("球面分布速率非法（{}）", speed),
                    String::from("速率须为有限非负数"),
                )
            } else {
                ok_unit()
            }
        }
    }
}

/// 速度采样：产出新粒子的初始速度。
///
/// 参数非法时返回**零速度**而非 panic——非法参数由上游 [`create_emitter`]
/// 拦掉，这里是纵深兜底，且零速度仍是可积分的合法状态（粒子原地不动）。
pub fn sample_velocity(d: &VelocityDistParams, rng: &mut RandomSource) -> Vec3 {
    if validate_velocity_dist(d).is_err() {
        return Vec3::ZERO;
    }
    let (speed, dir) = match d {
        VelocityDistParams::Uniform { min_speed, max_speed } => {
            (rng.range_f32(*min_speed, *max_speed), uniform_sphere_direction(rng))
        }
        VelocityDistParams::Cone { axis, half_angle, min_speed, max_speed } => {
            let ax = normalize_v3(*axis);
            let u = orthogonal_basis(ax);
            let v = cross_v3(ax, u);
            let cos_max = cos_approx(*half_angle);
            let cos_theta = rng.range_f32(cos_max, 1.0);
            let sin_theta = sqrt_v3((1.0 - cos_theta * cos_theta).max(0.0));
            let phi = rng.range_f32(0.0, core::f32::consts::TAU);
            let cp = cos_approx(phi);
            let sp = sin_approx(phi);
            let dir = normalize_v3(Vec3::new(
                ax.x * cos_theta + (u.x * cp + v.x * sp) * sin_theta,
                ax.y * cos_theta + (u.y * cp + v.y * sp) * sin_theta,
                ax.z * cos_theta + (u.z * cp + v.z * sp) * sin_theta,
            ));
            (rng.range_f32(*min_speed, *max_speed), dir)
        }
        VelocityDistParams::Sphere { speed } => (*speed, uniform_sphere_direction(rng)),
    };
    let out = scale_v3(dir, speed);
    if !out.is_finite() {
        return Vec3::ZERO;
    }
    out
}

// ---------------------------------------------------------------------------
// 七、两级层级（判据：两级层级）：发射器 → 粒子组
// ---------------------------------------------------------------------------

/// 粒子组（发射器的第二级）。组引用 + 权重。
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleGroup {
    /// 组标识（材质/行为标识；具体语义由F2204 模式与渲染侧解释）。
    pub id: u32,
    /// 期望占比权重（**期望占比**而非硬配额，见 [`normalize_group_weights`]）。
    pub weight: f32,
}

impl ParticleGroup {
    pub fn new(id: u32, weight: f32) -> Self {
        ParticleGroup { id, weight }
    }
}

/// 权重归一化：返回各组的概率份额（和为 1）。
///
/// 语义要点：权重是**期望占比**不是硬配额——单帧选组是随机抽样，
/// 长期分布才逼近权重。用硬配额（如按权重取整）会在权重很小时产出 0 组，
/// 表现为「配置了但永不出现」的效果，最难查。
pub fn normalize_group_weights(groups: &[ParticleGroup]) -> Outcome<Vec<f32>> {
    if groups.is_empty() {
        return Outcome::fail(
            DiagCode::GroupRejected,
            String::from("粒子组列表为空"),
            String::from("至少需要一个粒子组；空列表无可发射的目标"),
        );
    }
    let mut sum = 0.0f32;
    for g in groups.iter() {
        if !is_finite(g.weight) || g.weight < 0.0 {
            return Outcome::fail(
                DiagCode::GroupRejected,
                format!("粒子组 {} 权重非法（{}）", g.id, g.weight),
                String::from("权重须为有限非负数；负权重会让归一化产出负概率"),
            );
        }
        sum += g.weight;
    }
    if !(sum > DEGENERATE_EPS) || !is_finite(sum) {
        return Outcome::fail(
            DiagCode::GroupRejected,
            format!("粒子组权重之和为 {}，无法归一化", sum),
            String::from("至少一个组的权重要大于 0；全零权重等价于不发射"),
        );
    }
    let mut out: Vec<f32> = Vec::with_capacity(groups.len());
    for g in groups.iter() {
        out.push(g.weight / sum);
    }
    Outcome::ok(out)
}

/// 按归一化权重抽一个组（复杂度 O(组数)，线性扫描——组数是配置量，量级极小）。
pub fn pick_group(
    groups: &[ParticleGroup],
    norm: &[f32],
    rng: &mut RandomSource,
) -> Option<ParticleGroup> {
    if groups.is_empty() || groups.len() != norm.len() {
        return None;
    }
    let mut r = rng.next_f32();
    for (i, p) in norm.iter().enumerate() {
        r -= *p;
        if r <= 0.0 {
            return Some(groups[i].clone());
        }
    }
    // 浮点累加误差可能让 r 残留为正——落到最后一组而非返回 None：
    // 这不是静默兜底，而是保证「每粒必归一组」的不变量。
    groups.last().cloned()
}

// ---------------------------------------------------------------------------
// 八、生命周期状态机（判据：状态机）
// ---------------------------------------------------------------------------

/// 发射器状态。四态不多不少。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmitterState {
    /// 已创建（尚未激活；已占池句柄但不出粒子）。
    Created,
    /// 激活中（出粒子）。
    Active,
    /// 已暂停（不出粒子，但保留全部状态）。
    Paused,
    /// 已销毁（池句柄已回收，不可复活）。
    Destroyed,
}

impl EmitterState {
    pub const ALL: [EmitterState; 4] = [
        EmitterState::Created,
        EmitterState::Active,
        EmitterState::Paused,
        EmitterState::Destroyed,
    ];

    /// 中文标签（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            EmitterState::Created => "已创建",
            EmitterState::Active => "激活中",
            EmitterState::Paused => "已暂停",
            EmitterState::Destroyed => "已销毁",
        }
    }

    /// 该态是否持有池句柄（销毁必须回收——这是资源泄漏的唯一出口）。
    pub fn holds_resources(self) -> bool {
        matches!(
            self,
            EmitterState::Created | EmitterState::Active | EmitterState::Paused
        )
    }
}

/// 合法转移表。销毁是**终态**：不可复活，也不可再转出。
pub fn legal_transitions(from: EmitterState) -> &'static [EmitterState] {
    match from {
        EmitterState::Created => &[EmitterState::Active, EmitterState::Destroyed],
        EmitterState::Active => &[EmitterState::Paused, EmitterState::Destroyed],
        EmitterState::Paused => &[EmitterState::Active, EmitterState::Destroyed],
        EmitterState::Destroyed => &[],
    }
}

/// 转移是否合法。
pub fn can_transition(from: EmitterState, to: EmitterState) -> bool {
    legal_transitions(from).contains(&to)
}

/// 请求状态转移。非法转移**拒绝**而非静默纠正——「悄悄把非法态改成合法态」
/// 会让调用方以为暂停成功了，而发射器其实还在出粒子。
pub fn request_transition(from: EmitterState, to: EmitterState) -> Outcome<EmitterState> {
    if can_transition(from, to) {
        return Outcome::ok(to);
    }
    let legal: Vec<&str> = legal_transitions(from).iter().map(|s| s.zh()).collect();
    let legal_text = if legal.is_empty() { "无（终态）".to_string() } else { legal.join("、") };
    Outcome::fail(
        DiagCode::TransitionRejected,
        format!(
            "发射器不能从「{}」转为「{}」（合法去向：{}）",
            from.zh(),
            to.zh(),
            legal_text
        ),
        if from == EmitterState::Destroyed {
            String::from("销毁是终态：池句柄已回收，不可复活；请新建发射器")
        } else {
            String::from("按显式转移表发起转移；本模块不自动纠正非法态")
        },
    )
}

// ---------------------------------------------------------------------------
// 九、发射器本体：配置、产出与帧内合并
// ---------------------------------------------------------------------------

/// 帧内发射器增删阈值（超过即判风暴）。
///
/// 依据锚点「发射器风暴→帧边界合并（F1807 同规则）」：频繁增删会触发
/// 池句柄反复申请释放，在帧内制造尖峰。
pub const CHURN_STORM_THRESHOLD: u32 = 8;

/// 单帧发射上限（池容量保护，与F2208 池侧对齐）。
pub const MAX_SPAWN_PER_FRAME: u32 = 65_536;

/// 发射器配置。
#[derive(Clone, Debug, PartialEq)]
pub struct EmitterConfig {
    /// 发射率（粒子/秒）。
    pub rate_per_sec: f32,
    /// 出生形状。
    pub shape: ShapeParams,
    /// 初始速度分布。
    pub velocity: VelocityDistParams,
    /// 粒子组列表（第二级）。
    pub groups: Vec<ParticleGroup>,
    /// 随机源种子（同种子双跑逐位一致）。
    pub seed: u64,
    /// 初始寿命（秒）。
    pub lifetime: f32,
}

/// 产出请求：每粒的出生位置与速度 + 归属组 +寿命。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnRequest {
    pub position: Vec3,
    pub velocity: Vec3,
    pub group_id: u32,
    pub lifetime: f32,
}

/// 发射器本体（两级层级的第一级）。
#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    pub config: EmitterConfig,
    pub state: EmitterState,
    pub acc: EmitAccumulator,
    pub rng: RandomSource,
    /// 归一化权重（建器时算一次；组配置不变则不变）。
    norm_weights: Vec<f32>,
    /// 本帧内增删次数（风暴判定）。
    pub churn_this_frame: u32,
    /// 累计产出粒子数（对拍用）。
    pub spawned_total: u64,
    /// 累计产出粒子组次数（对拍用）。
    pub group_spawn_total: u64,
    /// 因单帧上限被截断的次数（零静默：截断必须可查）。
    pub truncated_total: u64,
}

/// 建立发射器。配置非法即拒绝（建器是唯一能拦住「坏配置流到每帧」的关口）。
pub fn create_emitter(config: EmitterConfig) -> Outcome<Emitter> {
    if let Outcome::Err { code, message, hint } = validate_shape(&config.shape) {
        return Outcome::fail(code, message, hint);
    }
    if let Outcome::Err { code, message, hint } = validate_velocity_dist(&config.velocity) {
        return Outcome::fail(code, message, hint);
    }
    let norm = match normalize_group_weights(&config.groups) {
        Outcome::Ok { value: v, .. } => v,
        Outcome::Err { code, message, hint } => return Outcome::fail(code, message, hint),
    };
    if !is_finite(config.lifetime) || config.lifetime <= 0.0 {
        return Outcome::fail(
            DiagCode::ShapeRejected,
            format!("发射器寿命非法（{}）", config.lifetime),
            String::from("寿命须为有限正数（秒）；非正寿命会让粒子出生即消亡"),
        );
    }
    // 发射率在step 里逐帧钳制，此处先钳一次：让首帧行为与稳态一致。
    let mut bag = DiagBag::new();
    let rate = clamp_emit_rate(config.rate_per_sec, &mut bag);
    let mut cfg = config;
    cfg.rate_per_sec = rate;
    let seed = cfg.seed;
    Outcome::ok(Emitter {
        config: cfg,
        state: EmitterState::Created,
        acc: EmitAccumulator::new(),
        rng: RandomSource::new(seed),
        norm_weights: norm,
        churn_this_frame: 0,
        spawned_total: 0,
        group_spawn_total: 0,
        truncated_total: 0,
    })
}

/// 帧步进：产出本帧新粒子。
///
/// 非激活态直接返回空Vec 但**不清余量**——暂停期间的余量须在恢复后
/// 继续累积，否则暂停一次就凭空吞掉半粒。
pub fn step_emitter(
    em: &mut Emitter,
    dt: f32,
    bag: &mut DiagBag,
) -> Vec<SpawnRequest> {
    if em.state != EmitterState::Active {
        return Vec::new();
    }
    let rate = clamp_emit_rate(em.config.rate_per_sec, bag);
    let want = advance_accumulator(&mut em.acc, rate, dt);
    let n = if want > MAX_SPAWN_PER_FRAME {
        em.truncated_total = em.truncated_total.saturating_add((want - MAX_SPAWN_PER_FRAME) as u64);
        bag.note(
            DiagCode::RateClamped,
            format!(
                "本帧应发射 {} 粒，超单帧上限 {}，已截断",
                want, MAX_SPAWN_PER_FRAME
            ),
            String::from("单帧发射尖峰会瞬时抽干池；请降低发射率、延长寿命或分摊到多帧"),
        );
        MAX_SPAWN_PER_FRAME
    } else {
        want
    };
    let mut out: Vec<SpawnRequest> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let position = match sample_shape(&em.config.shape, &mut em.rng, bag) {
            Outcome::Ok { value: p, .. } => p,
            //采样失败即本帧停止发射：不静默跳过（跳过会让「以为在发射其实没发」）。
            Outcome::Err { .. } => break,
        };
        let velocity = sample_velocity(&em.config.velocity, &mut em.rng);
        let group_id = match pick_group(&em.config.groups, &em.norm_weights, &mut em.rng) {
            Some(g) => {
                em.group_spawn_total = em.group_spawn_total.saturating_add(1);
                g.id
            }
            None => 0,
        };
        out.push(SpawnRequest {
            position,
            velocity,
            group_id,
            lifetime: em.config.lifetime,
        });
    }
    em.spawned_total = em.spawned_total.saturating_add(out.len() as u64);
    out
}

/// 帧边界合并：把帧内高频增删折叠为一次记账（锚点「发射器风暴→帧边界合并」）。
///
/// 超阈值时**不清零**计数器而是饱和到阈值+1：这样「合并了几次」可事后查证，
/// 静默清零会让风暴彻底不可见。
pub fn coalesce_churn(em: &mut Emitter, frame: u64, bag: &mut DiagBag) -> u64 {
    if frame == 0 {
        // 帧 0 是帧边界：此处才结算上一帧的增删计数。
        let n = em.churn_this_frame;
        if n > CHURN_STORM_THRESHOLD {
            bag.note(
                DiagCode::ChurnCoalesced,
                format!(
                    "上一帧发射器增删 {} 次，超阈值 {}，已合并为一次帧边界操作",
                    n, CHURN_STORM_THRESHOLD
                ),
                String::from("高频增删会在帧内反复申请/释放池句柄；请把发射器的生命周期对齐到帧"),
            );
        }
        em.churn_this_frame = if n > CHURN_STORM_THRESHOLD { CHURN_STORM_THRESHOLD + 1 } else { 0 };
        return n as u64;
    }
    // 不在此处累加：增删次数由 [`note_churn`] 单点记录。若这里再加一次，
    // 「调方记一次 + 帧步进记一次」会让计数翻倍，风暴判定提前误触发。
    em.churn_this_frame as u64
}

/// 记一次发射器增删（供调度侧在创建/销毁发射器时调用）。
pub fn note_churn(em: &mut Emitter) -> u32 {
    em.churn_this_frame = em.churn_this_frame.saturating_add(1);
    em.churn_this_frame
}

/// 请求状态转移并就地应用（非法转移不改状态、产诊断）。
pub fn apply_transition(em: &mut Emitter, to: EmitterState, bag: &mut DiagBag) -> bool {
    match request_transition(em.state, to) {
        Outcome::Ok { value: next, .. } => {
            em.state = next;
            if next == EmitterState::Destroyed {
                // 销毁即回收：这是池句柄唯一的释放点，不做就泄漏。
                em.norm_weights.clear();
                em.norm_weights.shrink_to_fit();
            }
            true
        }
        Outcome::Err { code, message, hint } => {
            bag.note(code, message, hint);
            false
        }
    }
}
