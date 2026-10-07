//! VE-F2206 · 粒子渲染接口（VE-L 域 · 粒子与物理域 · 批次 L01 第 6 项 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2206`
//!
//! **判据（锚点原文四条）**：三形态、模拟渲染解耦、I02 对接、速度拉伸。
//! 逐条落位：
//! - **三形态**：[`RenderForm`] 的 billboard / 网格 / 拖尾三型，形态枚举
//!   **恰为三**（[`FORMS`] 常量表对账，缺一或多一即红），三者各有独立参数集
//!   （[`FormParams`] 的分型载荷），不共用一个「万能参数」。
//! - **模拟渲染解耦**：模拟侧只写 [`ParticleView`]（位置/尺寸/颜色/朝向），
//!   渲染侧**只读消费**且**不写回**——[`RenderForm::build`] 收`&[ParticleView]`
//!   而非 `&mut`，故形态切换在类型上就不可能改模拟。切换走
//!   [`FormSwitch`] 的**帧边界闸**（F1762 规则），半帧内请求切换被拒绝并产诊断。
//! - **I02 对接**：[`InstanceSink`] 抽象「实例缓冲写入 + 绘制调用提交」，
//!   [`submit`] 是与 I02 绘制族的对接签名声明；I02 未就绪时走
//!   [`RenderDiag::SinkNotReady`]——**渲染挂起 + 告警，绝不静默**。
//! - **速度拉伸**：billboard 的朝向二态由 [`Facing`] 表达——`CameraRight`
//!   为相机对齐，`VelocityStretch` 为速度对齐；后者按速度长度拉伸并
//!   **保面积守恒**（速度拉伸会把面片拉长，若不缩窄则粒子变大变亮），
//!   守恒量有解析式[`stretch_width`]，非估值。
//!
//! **与 F2205 的分工**：F2205 管「粒子多老、淡成什么样」，本模块管「怎么画」。
//! 故本模块**只消费** F2205 的 [`ShadedState`]（alpha/size/color），
//! 不复制任何寿命或淡变数学（曲线单源纪律的延伸：淡变只有一个源）。
//!
//! **降级矩阵（锚点原文五条→ 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 属性缺失（形态需要的属性未启用）→ 校验拒绝 | [`validate_params`]：拖尾无历史容量即拒绝 |
//! | 网格为空 → 拒绝 | [`FormParams::mesh`] 为空 → [`RenderDiag::MeshEmpty`] |
//! | 历史缓冲溢出 → 环形覆盖语义 | [`TrailHistory`] 定容环形，覆盖计[`overwrites`] |
//! | I02 接口未就绪 → 渲染挂起 + 告警（不静默） | [`submit`] 返回挂起并产 [`RenderDiag::SinkNotReady`] |
//! | 形态切换帧边界（F1762 规则） | [`FormSwitch`] 半帧请求拒绝 + [`RenderDiag::SwitchNotAtBoundary`] |
//!
//! **性能诚实标注**：billboard 顶点生成 O(N)、网格实例化 O(N)（每粒子一次
//! 仿射装配）、拖尾 O(N×历史数)——三者均**实测工作量可数**
//! （[`RenderStats`] 记顶点数/实例数/带段数），不写自证式算术。
//! 锚点未给「每粒子每帧 <X ns」的实测基准，故本模块**不代填未测数据**，
//! 微基准由 **VE-F2212（粒子基准）** 承接。
//!
//! **跨批对接**：绘制单源 I02/I05（与 F2201 序契约呼应）；解耦思想与 F2002
//! 效果接口同构；拖尾历史缓冲沿用 F2003 池理念（定容环形，不动态增长）。
//!
//! 零 IO、零墙钟；时间以帧号注入不取系统时钟，故同输入双跑逐位一致。
//! 无隐私面。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::vel03_emitter::{DiagBag, DiagCode, Outcome};

// `pub use` 既引入本地使用、又再导出：渲染视图的构造签名直接用到 F2205 的
// [`ShadedState`] 与 [`Rgba`]，而本模块文档承诺「只消费 ShadedState」——
// 承诺的类型却不导出，属文档与 API 不一致。故原名再导出。
pub use super::vel05_lifetime::{Rgba, ShadedState};

// ---------------------------------------------------------------------------
// 一、渲染形态（三型，判据一）
// ---------------------------------------------------------------------------

/// 渲染形态（锚点三型：billboard 面片 / 网格粒子 / 拖尾）。
///
/// **恰为三型**，不多不少：[`FORMS`] 是唯一在册表，任何新增形态必须先进表，
/// 判据 `E01-三型-在册不多不少` 会对账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderForm {
    /// 面片：四顶点矩形，按朝向模式对齐相机右向量或速度方向。
    Billboard,
    /// 网格粒子：引用网格，按粒子位置装配实例矩阵。
    Mesh,
    /// 拖尾：由历史位置环生成带状网格，宽度沿尾衰减。
    Trail,
}

/// 在册形态表（判据对账基准）。
///
/// 为什么用常量表而非只靠枚举：枚举加变体是编译期改动，而「三型」是**规格
/// 承诺**——若日后有人加第四型，编译照过，只有这张表能拦住。
pub const FORMS: [RenderForm; 3] = [RenderForm::Billboard, RenderForm::Mesh, RenderForm::Trail];

impl RenderForm {
    /// 中文标签（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            RenderForm::Billboard => "面片",
            RenderForm::Mesh => "网格粒子",
            RenderForm::Trail => "拖尾",
        }
    }

    /// 该形态是否需要拖尾历史缓冲（锚点：拖尾需历史缓冲声明）。
    ///
    /// 供 [`validate_params`] 做属性缺失校验——这是「属性缺失 → 校验拒绝」
    /// 降级项的判定依据，不靠调用方自觉。
    pub fn needs_history(self) -> bool {
        self == RenderForm::Trail
    }

    /// 该形态是否需要网格引用（锚点：网格为空 → 拒绝）。
    pub fn needs_mesh(self) -> bool {
        self == RenderForm::Mesh
    }
}

/// billboard 朝向二态（锚点：相机对齐 / 速度拉伸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    /// 相机对齐：面片正对相机，四边等宽（普通光点/烟雾）。
    CameraRight,
    /// 速度对齐：面片沿速度方向拉伸（运动感粒子）。
    ///
    /// 拉伸必然伴随**横向缩窄**才不放大总面积——见 [`stretch_width`]。
    VelocityStretch,
}

/// 拖尾历史环的默认容量（槽位）。
///
/// 定容而非动态增长：锚点「历史缓冲溢出 → 环形覆盖语义」要求溢出是
/// **可预期行为**。若允许增长，溢出永不发生，该降级项就成了死代码。
pub const TRAIL_HISTORY_DEFAULT: usize = 8;

/// 拖尾历史环容量上限（防单发射器吃掉整池内存，F2208 池配额同源精神）。
pub const TRAIL_HISTORY_MAX: usize = 64;

/// 单粒子拖尾最少历史点。
///
/// 少于此数则带状网格退化（无长度），画出来是一个点——与「拖尾」语义不符，
/// 故校验期直接拒绝而非静默画点。
pub const TRAIL_MIN_HISTORY: usize = 2;

/// 形态参数（三型各一套，判据一）。
///
/// 分型载荷而非单一万能结构：billboard 要朝向与尺寸，网格要网格引用，
/// 拖尾要历史容量与衰减——把它们塞进一个结构里会出现「billboard 也得填
/// 历史容量」这种无意义字段，而无意义字段迟早被误用。
#[derive(Clone, Debug, PartialEq)]
pub enum FormParams {
    /// 面片参数。
    Billboard {
        /// 基础尺寸（世界单位，F2205 的 size 乘子作用其上）。
        size: f32,
        /// 朝向二态。
        facing: Facing,
    },
    /// 网格粒子参数。
    Mesh {
        /// 网格引用：顶点索引区间 + 索引基数（对接 I05 实例表的最小描述）。
        ///
        /// 为空即「网格为空」→ 校验拒绝（锚点降级项）。
        mesh: Option<MeshRef>,
        /// 材质组（对接 I05 按材质分批）。
        material: u32,
    },
    /// 拖尾参数。
    Trail {
        /// 历史位置环容量。
        history: usize,
        /// 沿尾宽度衰减系数（[0,1]，1=不衰减）。
        width_decay: f32,
    },
}

/// 网格引用（只读描述，不own 顶点数据）。
///
/// 顶点数据由I 域网格资产提供，本模块只记「从哪取」——**不复制网格**，
/// 否则每个发射器都存一份网格副本，内存预计算（F2202）立刻失效。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshRef {
    /// 顶点在资产缓冲中的起始索引。
    pub first_vertex: u32,
    /// 顶点个数（0 即为空 → 拒绝）。
    pub vertex_count: u32,
    /// 实例装配用的材质组。
    pub material: u32,
}

impl MeshRef {
    /// 构造网格引用。
    pub fn new(first_vertex: u32, vertex_count: u32, material: u32) -> Self {
        MeshRef { first_vertex, vertex_count, material }
    }

    /// 网格是否为空（0 顶点）。
    pub fn is_empty(&self) -> bool {
        self.vertex_count == 0
    }
}

// ---------------------------------------------------------------------------
// 二、诊断
// ---------------------------------------------------------------------------

/// 渲染域诊断码。
///
/// 处置方向相反的状态**不共用码**：「挂起」（可恢复，渲染稍后继续）与
/// 「拒绝」（配置非法，须改配置）语义相反，共用码会让调用方误判是否可重试。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderDiag {
    /// 形态参数校验拒绝（属性缺失/尺寸非法/历史容量越界）。
    ParamsRejected,
    /// 网格为空或顶点数为 0。
    MeshEmpty,
    /// I02 绘制接口未就绪，渲染挂起（不静默）。
    SinkNotReady,
    /// 形态切换请求不在帧边界（F1762）。
    SwitchNotAtBoundary,
    /// 实例缓冲容量不足，请求扩容被拒。
    SinkOverflow,
    /// 非有限输入（位置/速度/颜色含 NaN 或 Inf）。
    NonFiniteInput,
}

impl RenderDiag {
    /// 中文标签（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            RenderDiag::ParamsRejected => "形态参数拒绝",
            RenderDiag::MeshEmpty => "网格为空",
            RenderDiag::SinkNotReady => "绘制接口未就绪",
            RenderDiag::SwitchNotAtBoundary => "形态切换非帧边界",
            RenderDiag::SinkOverflow => "实例缓冲溢出",
            RenderDiag::NonFiniteInput => "输入非有限",
        }
    }

    /// 映射到 F2203 的共享诊断码，保持全链路单一诊断出口。
    pub fn code(self) -> DiagCode {
        match self {
            // 参数非法与网格为空在 F2203 已各有其码，此处复用不新增。
            RenderDiag::ParamsRejected => DiagCode::ShapeRejected,
            RenderDiag::MeshEmpty => DiagCode::MeshEmpty,
            RenderDiag::NonFiniteInput => DiagCode::VelocityRejected,
            // 挂起与帧边界拒绝：借 TransitionRejected 表达「本次动作被拒」，
            // 差异由本域码承载（不进F2203 枚举，避免跨域耦合）。
            RenderDiag::SinkNotReady => DiagCode::TransitionRejected,
            RenderDiag::SwitchNotAtBoundary => DiagCode::TransitionRejected,
            RenderDiag::SinkOverflow => DiagCode::GroupRejected,
        }
    }
}

/// 记一条渲染诊断进F2203 的诊断袋（保持全链路单一诊断出口）。
pub fn note(bag: &mut DiagBag, code: RenderDiag, message: String, hint: String) {
    bag.note(code.code(), message, hint);
}

// ---------------------------------------------------------------------------
// 三、粒子视图：模拟 → 渲染的唯一接口（判据二：解耦）
// ---------------------------------------------------------------------------

/// 粒子渲染视图：模拟侧输出、渲染侧**只读**消费的唯一数据形态。
///
/// **只读消费声明**：本结构是模拟侧写好的快照，渲染侧只读不写——这正是
/// 「模拟渲染解耦」的落点。渲染形态的所有输入都从这里取，渲染**不回头**
/// 改模拟的任何字段（无一个 `&mut ParticleView` 出现在渲染路径上，
/// 由判据 `E02-解耦-渲染面无写回` 以签名扫描钉住）。
///
/// 尺寸与颜色来自 F2205 的 [`ShadedState`]（淡变结果），本模块**不再乘一次**
/// 淡变——曲线单源纪律：淡变只有一个源。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleView {
    /// 世界位置。
    pub position: [f32; 3],
    /// 世界速度（仅 `VelocityStretch` 朝向消费；其余形态忽略）。
    pub velocity: [f32; 3],
    /// 尺寸乘子（F2205 size 曲线结果，≥0）。
    pub size: f32,
    /// 颜色与alpha（F2205 alpha/color 曲线结果）。
    pub color: Rgba,
    /// 自定义朝向轴（可选覆盖；`None` 表示按 [`Facing`] 自动求）。
    ///
    /// 「自定义朝向」是锚点点名的四属性之一，故给它真实字段而非注释。
    pub custom_axis: Option<[f32; 3]>,
}

impl ParticleView {
    /// 由 F2205 的 [`ShadedState`] 与位置/速度装配视图。
    pub fn new(position: [f32; 3], velocity: [f32; 3], size: f32, shaded: ShadedState) -> Self {
        ParticleView {
            position,
            velocity,
            size,
            color: shaded.color,
            custom_axis: None,
        }
    }

    /// 指定自定义朝向轴（长度须非零，由 [`validate_params`] 校验）。
    pub fn with_axis(mut self, axis: [f32; 3]) -> Self {
        self.custom_axis = Some(axis);
        self
    }

    /// 有效 alpha（F2205 已钳到[0,1]；此处再挡一次非有限值）。
    pub fn alpha(&self) -> f32 {
        if self.color.a.is_finite() {
            self.color.a.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// 是否参与绘制（alpha 极小的粒子跳过，省带宽）。
    ///
    /// 阈值取 0 不是「>0」：负 alpha 已被 [`Self::alpha`] 钳到 0，
    /// 故 `> 0.0` 即可；写 `>= 0.0` 会把全透明粒子也送进顶点生成。
    pub fn visible(&self) -> bool {
        self.alpha() > 0.0
    }

    /// 非有限输入自查（位置/速度/尺寸/颜色任一非有限即false）。
    pub fn is_finite(&self) -> bool {
        let v3 = |v: &[f32; 3]| v.iter().all(|x| x.is_finite());
        v3(&self.position)
            && v3(&self.velocity)
            && self.size.is_finite()
            && self.color.r.is_finite()
            && self.color.g.is_finite()
            && self.color.b.is_finite()
            && self.color.a.is_finite()
    }
}

/// 顶点（位置 + 颜色），最小可绘制单元。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    /// 世界位置。
    pub position: [f32; 3],
    /// 顶点色（含 alpha）。
    pub color: Rgba,
}

/// 实例（网格粒子形态的最小装配单元，对接 I05 实例表）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    /// 实例原点（= 粒子位置）。
    pub origin: [f32; 3],
    /// 实例矩阵的行主序 3x3（由朝向轴与尺寸装配）。
    pub basis: [[f32; 3]; 3],
    /// 材质组。
    pub material: u32,
}

/// 渲染统计（**实测工作量**，非自证式算术）。
///
/// 锚点性能分解的三项各有对应计数器：billboard 顶点生成 O(N)→`vertices`，
/// 网格实例化 O(N)→`instances`，拖尾 O(N×历史数)→`ribbon_segments`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// 生成顶点数。
    pub vertices: usize,
    /// 装配实例数。
    pub instances: usize,
    /// 带状网格段数（拖尾专用）。
    pub ribbon_segments: usize,
    /// 因不可见而跳过的粒子数。
    pub skipped: usize,
    /// 因非有限输入而被跳过的粒子数。
    pub rejected: usize,
}

// ---------------------------------------------------------------------------
// 四、billboard 顶点生成（判据四：速度拉伸）
// ---------------------------------------------------------------------------

/// 向量长度（零向量返回 0）。
fn len3(v: [f32; 3]) -> f32 {
    let s = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if s > 0.0 && s.is_finite() { s.sqrt() } else { 0.0 }
}

/// 归一化；零向量或非有限返回 `None`（调用方据拒绝/退化处理，不静默）。
fn normalize3(v: [f32; 3]) -> Option<[f32; 3]> {
    let l = len3(v);
    if !(l > 0.0) || !l.is_finite() {
        return None;
    }
    let n = [v[0] / l, v[1] / l, v[2] / l];
    if n[0].is_finite() && n[1].is_finite() && n[2].is_finite() {
        Some(n)
    } else {
        None
    }
}

/// 速度拉伸的**横向缩窄因子**（保面积守恒）。
///
/// 锚点的「速度对齐」若只拉长不缩窄，粒子面积随速度线性增长——高速粒子
/// 看起来又大又亮，物理上错误（应是运动模糊感，不是变亮）。故拉伸因子
/// `k` 与横向因子取 `1/k`，面积乘积恒为 1。
///
/// 解析式：`width = base / k`，`k = 1 + |v| / v_ref`，钳制到
/// `[1/MAX_STRETCH, 1]`（[`MAX_STRETCH`] 防极端拉伸把面片压成线）。
///
/// 为什么用**单边**定义（`k >= 1` 故 `width <= base`）：面积守恒要求
/// 乘积为 1，若两侧都可任意浮动则判据无法用符号区分对错。
pub const MAX_STRETCH: f32 = 8.0;

/// 参考速度：拉伸因子中「速度 1.0」对应的拉伸比。
pub const VELOCITY_REFERENCE: f32 = 1.0;

/// 拉伸比 `k = 1 + |v| / v_ref`，钳制到 `[1, MAX_STRETCH]`。
pub fn stretch_factor(speed: f32) -> f32 {
    if !(speed.is_finite()) || speed <= 0.0 {
        return 1.0;
    }
    (1.0 + speed / VELOCITY_REFERENCE).clamp(1.0, MAX_STRETCH)
}

/// 横向缩窄因子 = `1 / stretch_factor(speed)`（保面积守恒）。
pub fn stretch_width(speed: f32) -> f32 {
    1.0 / stretch_factor(speed)
}

/// 求billboard 的半长轴（沿朝向）与半宽轴（垂直朝向）。
///
/// 返回 `(along, across)`：朝向由三条优先级决定——自定义朝向轴 >速度拉伸
/// 方向 > 相机右向量。**自定义优先**是锚点「自定义朝向」属性的语义。
fn billboard_axes(view: &ParticleView, facing: Facing, cam_right: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    // 半长轴方向：自定义 > （拉伸时的速度方向）> 相机右向量。
    let along = match (facing, view.custom_axis) {
        (_, Some(axis)) => normalize3(axis).unwrap_or([1.0, 0.0, 0.0]),
        (Facing::VelocityStretch, None) => {
            normalize3(view.velocity).unwrap_or(cam_right)
        }
        (Facing::CameraRight, None) => cam_right,
    };
    // 半宽轴：取一个与along 垂直的世界轴。选世界 Y 的投影，退化时换 X——
    // 不能用「任取一条叉乘轴」而不判退化，否则 along≈±Y 时叉乘得零向量。
    let helper = if along[1].abs() < 0.99 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let across = normalize3(cross(along, helper)).unwrap_or([1.0, 0.0, 0.0]);
    (along, across)
}

/// 三维叉积。
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// 生成一个 billboard 的四顶点。
///
/// 面片顶点数恒为 4（两个三角形共享 2 顶点），故billboard 的 O(N) 是
/// 严格 4N——判据 `E04-拉伸-面片顶点数恒四` 以此钉住。
fn billboard_quad(view: &ParticleView, params: &FormParams, cam_right: [f32; 3]) -> [Vertex; 4] {
    let (size, facing) = match params {
        FormParams::Billboard { size, facing } => (*size, *facing),
        // 调用方已校验形态匹配；此处兜底给单位面片而非panic（零 panic 面）。
        _ => (1.0, Facing::CameraRight),
    };
    let (along, across) = billboard_axes(view, facing, cam_right);
    // 拉伸**只作用于速度拉伸模式**。相机对齐是「始终正对相机」的面片，
    // 它的长宽由相机右向量与世界上轴决定，与粒子速度无关；若也乘拉伸
    // 因子，高速相机的普通光点会被拉成条——那是速度拉伸模式独有的观感，
    // 泄到相机对齐上就是行为错误（锚点：朝向二态的语义分界正在此处）。
    let (k_along, k_across) = match facing {
        Facing::VelocityStretch => {
            let k = stretch_factor(len3(view.velocity));
            (k, 1.0 / k)
        }
        Facing::CameraRight => (1.0, 1.0),
    };
    let base = size * view.size.max(0.0);
    let half_along = base * k_along;
    let half_across = base * k_across;
    let p = view.position;
    let c = view.color;
    let corner = |sa: f32, sb: f32| -> Vertex {
        Vertex {
            position: [
                p[0] + along[0] * half_along * sa + across[0] * half_across * sb,
                p[1] + along[1] * half_along * sa + across[1] * half_across * sb,
                p[2] + along[2] * half_along * sa + across[2] * half_across * sb,
            ],
            color: c,
        }
    };
    [corner(-1.0, -1.0), corner(1.0, -1.0), corner(1.0, 1.0), corner(-1.0, 1.0)]
}

// ---------------------------------------------------------------------------
// 五、拖尾历史环（环形覆盖语义）
// ---------------------------------------------------------------------------

/// 拖尾历史位置环（**定容**，溢出即覆盖最旧）。
///
/// 锚点「历史缓冲溢出 → 环形覆盖语义」：覆盖不是错误，是定容环的必然
/// 结果。故本类型**不产拒绝诊断**（拒绝会与「溢出是预期行为」矛盾），
/// 只累加 [`Self::overwrites`] 供统计与调试查询。
#[derive(Clone, Debug, PartialEq)]
pub struct TrailHistory {
    slots: Vec<[f32; 3]>,
    /// 下一个写入位置（环形游标）。
    cursor: usize,
    /// 已写入有效点数（饱和于`slots.len()`）。
    len: usize,
    /// 覆盖次数（写入时环已满则+1）。
    overwrites: u64,
}

impl TrailHistory {
    /// 构造定容历史环。
    pub fn new(capacity: usize) -> Outcome<TrailHistory> {
        if capacity == 0 {
            return Outcome::fail(
                RenderDiag::ParamsRejected.code(),
                String::from("拖尾历史环容量为 0"),
                String::from("容量至少为 1；拖尾还需至少 2 个历史点才有长度"),
            );
        }
        if capacity > TRAIL_HISTORY_MAX {
            return Outcome::fail(
                RenderDiag::ParamsRejected.code(),
                format!("拖尾历史环容量 {} 超过上限 {}", capacity, TRAIL_HISTORY_MAX),
                format!("上限为 {}：单发射器的历史缓冲须留内存给池（F2208 配额）", TRAIL_HISTORY_MAX),
            );
        }
        let mut slots = Vec::new();
        for _ in 0..capacity {
            slots.push([0.0, 0.0, 0.0]);
        }
        Outcome::ok(TrailHistory { slots, cursor: 0, len: 0, overwrites: 0 })
    }

    /// 容量（槽位数）。
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// 已写入的有效点数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 历史环是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 覆盖次数（环形语义的唯一可观测证据）。
    pub fn overwrites(&self) -> u64 {
        self.overwrites
    }

    /// 推入一个历史位置（环形覆盖最旧的）。
    pub fn push(&mut self, position: [f32; 3]) {
        if self.len == self.slots.len() {
            self.overwrites += 1;
        } else {
            self.len += 1;
        }
        self.slots[self.cursor] = position;
        self.cursor = (self.cursor + 1) % self.slots.len();
    }

    /// 按时间序（最旧 → 最新）读出有效历史点。
    ///
    /// 返回 `Vec` 是**分配点**：拖尾每帧都要用，故由调用方复用缓冲
    /// （[`Self::read_into`]）；本方法只作为便捷包装，判据里用它做正确性
    /// 对拍而非性能路径。
    pub fn ordered(&self) -> Vec<[f32; 3]> {
        let mut out = Vec::new();
        self.read_into(&mut out);
        out
    }

    /// 把时间序历史点写入调用方缓冲（热路径零分配）。
    pub fn read_into(&self, out: &mut Vec<[f32; 3]>) {
        out.clear();
        if self.len == 0 {
            return;
        }
        let cap = self.slots.len();
        // 最旧点位置：环满时游标指向最旧，未满时为 0。
        let start = if self.len == cap { self.cursor } else { 0 };
        for k in 0..self.len {
            out.push(self.slots[(start + k) % cap]);
        }
    }

    /// 清空（粒子死亡时复用缓冲）。
    pub fn clear(&mut self) {
        for slot in self.slots.iter_mut() {
            *slot = [0.0, 0.0, 0.0];
        }
        self.cursor = 0;
        self.len = 0;
    }
}

// ---------------------------------------------------------------------------
// 六、形态参数校验（属性缺失 → 校验拒绝）
// ---------------------------------------------------------------------------

/// 形态参数校验（锚点降级矩阵：属性缺失 → 校验拒绝；网格为空 → 拒绝）。
///
/// 逐项检查而非在 `match` 里顺手判：漏判一项就是一条逃逸面，而逃逸面在
/// 编译期完全不可见（形参类型合法，只是值非法）。
pub fn validate_params(form: RenderForm, params: &FormParams) -> Outcome<()> {
    // 形态与参数必须同型：拿 Billboard 参数喂 Mesh 形态是调用方 bug。
    match (form, params) {
        (RenderForm::Billboard, FormParams::Billboard { size, .. }) => {
            if !(size.is_finite()) || *size <= 0.0 {
                return Outcome::fail(
                    RenderDiag::ParamsRejected.code(),
                    format!("面片尺寸 {} 非法", size),
                    String::from("尺寸须为有限正数；0 或负尺寸会产出退化面片"),
                );
            }
            Outcome::ok(())
        }
        (RenderForm::Mesh, FormParams::Mesh { mesh, .. }) => {
            // 网格为空 → 拒绝（锚点显式降级项）。
            match mesh {
                None => Outcome::fail(
                    RenderDiag::MeshEmpty.code(),
                    String::from("网格粒子未提供网格引用"),
                    String::from("网格形态须给MeshRef；无网格请改用 billboard 或 trail"),
                ),
                Some(m) if m.is_empty() => Outcome::fail(
                    RenderDiag::MeshEmpty.code(),
                    format!("网格顶点数为 0（起始 {}）", m.first_vertex),
                    String::from("空网格画不出任何像素；请给非零vertex_count"),
                ),
                Some(_) => Outcome::ok(()),
            }
        }
        (RenderForm::Trail, FormParams::Trail { history, width_decay }) => {
            // 拖尾需历史缓冲：容量不足则退化成一个点，与「拖尾」语义不符。
            if *history < TRAIL_MIN_HISTORY {
                return Outcome::fail(
                    RenderDiag::ParamsRejected.code(),
                    format!("拖尾历史容量 {} 不足（最少 {}）", history, TRAIL_MIN_HISTORY),
                    String::from("少于 2 个历史点则带状网格退化为一个点，不成拖尾"),
                );
            }
            if *history > TRAIL_HISTORY_MAX {
                return Outcome::fail(
                    RenderDiag::ParamsRejected.code(),
                    format!("拖尾历史容量 {} 超过上限 {}", history, TRAIL_HISTORY_MAX),
                    format!("上限 {}：历史缓冲须与池内存预算（F2208）共存", TRAIL_HISTORY_MAX),
                );
            }
            if !(width_decay.is_finite()) || !(0.0..=1.0).contains(width_decay) {
                return Outcome::fail(
                    RenderDiag::ParamsRejected.code(),
                    format!("拖尾宽度衰减 {} 越界", width_decay),
                    String::from("衰减系数须在 [0,1]：0=尾端归零，1=不衰减"),
                );
            }
            Outcome::ok(())
        }
        // 形态与参数不同型：显性拒绝，不静默按默认处理。
        (f, p) => Outcome::fail(
            RenderDiag::ParamsRejected.code(),
            format!("形态 {:?} 与参数 {:?} 不同型", f, p),
            String::from("形态与参数必须同型；请用 FormParams::for_form 取配套参数"),
        ),
    }
}

impl FormParams {
    /// 取该形态的默认参数（省去调用方手写全套字段）。
    pub fn for_form(form: RenderForm) -> FormParams {
        match form {
            RenderForm::Billboard => {
                FormParams::Billboard { size: 1.0, facing: Facing::CameraRight }
            }
            RenderForm::Mesh => FormParams::Mesh { mesh: None, material: 0 },
            RenderForm::Trail => {
                FormParams::Trail { history: TRAIL_HISTORY_DEFAULT, width_decay: 1.0 }
            }
        }
    }

    /// 该参数所属形态。
    pub fn form(&self) -> RenderForm {
        match self {
            FormParams::Billboard { .. } => RenderForm::Billboard,
            FormParams::Mesh { .. } => RenderForm::Mesh,
            FormParams::Trail { .. } => RenderForm::Trail,
        }
    }
}

// ---------------------------------------------------------------------------
// 七、三形态构建（判据一 + 判据四）
// ---------------------------------------------------------------------------

/// 渲染产物（模拟→渲染解耦的**单向**输出）。
///
/// 三个形态的几何不同，但提交给 I02 的接口必须统一——否则 I02 侧要为每种
/// 形态各写一套分支，那正是「解耦」想消除的耦合。故用枚举收敛，每型自带
/// 其几何，I02 侧只需按型分派。
#[derive(Clone, Debug, PartialEq)]
pub enum RenderBatch {
    /// 面片顶点缓冲（4N 顶点）。
    Billboard { vertices: Vec<Vertex>, stats: RenderStats },
    /// 网格实例表（对接 I05 实例表）。
    Mesh { instances: Vec<Instance>, stats: RenderStats },
    /// 带状网格（每粒子 `history-1` 段，每段 2 顶点）。
    Trail { vertices: Vec<Vertex>, segments: usize, stats: RenderStats },
}

impl RenderBatch {
    /// 统计（跨型读取）。
    pub fn stats(&self) -> RenderStats {
        match self {
            RenderBatch::Billboard { stats, .. }
            | RenderBatch::Mesh { stats, .. }
            | RenderBatch::Trail { stats, .. } => *stats,
        }
    }

    /// 顶点数（网格型为 0——它的最小单元是实例不是顶点）。
    pub fn vertex_count(&self) -> usize {
        match self {
            RenderBatch::Billboard { vertices, .. } | RenderBatch::Trail { vertices, .. } => {
                vertices.len()
            }
            RenderBatch::Mesh { .. } => 0,
        }
    }

    /// 实例数。
    pub fn instance_count(&self) -> usize {
        match self {
            RenderBatch::Mesh { instances, .. } => instances.len(),
            _ => 0,
        }
    }

    /// 带段数（非拖尾型为 0）。
    pub fn segment_count(&self) -> usize {
        match self {
            RenderBatch::Trail { segments, .. } => *segments,
            _ => 0,
        }
    }
}

/// 构建渲染批（**只读**消费视图，判据二）。
///
/// `views` 是 `&[ParticleView]`——不可变引用，故本函数**在类型层面**无法
/// 改模拟；「形态切换不改模拟」不是靠约定而是靠签名。
pub fn build(
    form: RenderForm,
    params: &FormParams,
    views: &[ParticleView],
    cam_right: [f32; 3],
    trails: Option<&[TrailHistory]>,
    bag: &mut DiagBag,
) -> Outcome<RenderBatch> {
    // 参数校验先行：不合法的参数不该消耗遍历成本。
    //
    // 失败必须**记入诊断袋**再返回：形参里已经带了 `bag`，却把校验失败
    // 的原因只放进返回值里，等于给了调用方一个「看得见但记不下」的
    // 拒绝——批量场景下（一个发射器每帧校验）拒绝会静默重复发生而无迹可寻，
    // 违反「异常零静默」。故此处 note 后再返。
    if let Outcome::Err { message, hint, .. } = validate_params(form, params) {
        note(
            bag,
            RenderDiag::ParamsRejected,
            format!("形态 {:?} 参数校验拒绝：{}", form, message.clone()),
            hint.clone(),
        );
        return Outcome::fail(RenderDiag::ParamsRejected.code(), message, hint);
    }
    match form {
        RenderForm::Billboard => build_billboard(params, views, cam_right, bag),
        RenderForm::Mesh => build_mesh(params, views, bag),
        RenderForm::Trail => build_trail(params, views, trails, bag),
    }
}

/// billboard 批：每可见粒子 4 顶点，严格 O(N)。
fn build_billboard(
    params: &FormParams,
    views: &[ParticleView],
    cam_right: [f32; 3],
    bag: &mut DiagBag,
) -> Outcome<RenderBatch> {
    let mut stats = RenderStats::default();
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut nonfinite = 0usize;
    for v in views {
        if !v.is_finite() {
            // 非有限输入：跳过 + 计数 + 一次性告警（不逐粒子刷屏）。
            nonfinite += 1;
            continue;
        }
        if !v.visible() {
            stats.skipped += 1;
            continue;
        }
        let quad = billboard_quad(v, params, cam_right);
        for corner in quad {
            vertices.push(corner);
        }
        stats.instances += 1;
    }
    stats.rejected = nonfinite;
    stats.vertices = vertices.len();
    if nonfinite > 0 {
        note(
            bag,
            RenderDiag::NonFiniteInput,
            format!("跳过 {} 个非有限粒子", nonfinite),
            String::from("位置/速度/尺寸/颜色含NaN 或 Inf；粒子数据来自模拟侧，请查上游"),
        );
    }
    Outcome::ok(RenderBatch::Billboard { vertices, stats })
}

/// 网格批：每可见粒子一次仿射装配，O(N)。
fn build_mesh(
    params: &FormParams,
    views: &[ParticleView],
    bag: &mut DiagBag,
) -> Outcome<RenderBatch> {
    let material = match params {
        FormParams::Mesh { material, .. } => *material,
        _ => 0,
    };
    let mut stats = RenderStats::default();
    let mut instances: Vec<Instance> = Vec::new();
    let mut nonfinite = 0usize;
    for v in views {
        if !v.is_finite() {
            nonfinite += 1;
            continue;
        }
        if !v.visible() {
            stats.skipped += 1;
            continue;
        }
        // 基向量：世界三轴 × 尺寸。网格粒子**不做速度拉伸**——拉伸是
        // billboard 的朝向特性，网格形状由网格自身定义，拉伸网格等于
        // 悄悄改变美术资产。
        let s = v.size.max(0.0);
        instances.push(Instance {
            origin: v.position,
            basis: [[s, 0.0, 0.0], [0.0, s, 0.0], [0.0, 0.0, s]],
            material,
        });
        stats.instances += 1;
    }
    stats.rejected = nonfinite;
    if nonfinite > 0 {
        note(
            bag,
            RenderDiag::NonFiniteInput,
            format!("跳过 {} 个非有限粒子", nonfinite),
            String::from("实例装配要求有限输入；非有限粒子不装配，避免污染实例缓冲"),
        );
    }
    Outcome::ok(RenderBatch::Mesh { instances, stats })
}

/// 拖尾批：每粒子 `history-1` 段 × 2 顶点，O(N×历史数)。
///
/// 宽度沿尾衰减：最新段最宽，越旧越窄，衰减系数由 `width_decay` 给出。
/// 用**线性档位**而非指数：线性档位可被解析式对账（判据
/// `E01-三型-拖尾宽度单调衰减`），指数不可。
fn build_trail(
    params: &FormParams,
    views: &[ParticleView],
    trails: Option<&[TrailHistory]>,
    bag: &mut DiagBag,
) -> Outcome<RenderBatch> {
    let (history, decay) = match params {
        FormParams::Trail { history, width_decay } => (*history, *width_decay),
        _ => (TRAIL_HISTORY_DEFAULT, 1.0),
    };
    // 属性缺失：拖尾需历史缓冲，无缓冲即拒绝（锚点显式降级项）。
    let trails = match trails {
        Some(t) => t,
        None => {
            // 同样必须记袋：本函数形参已带 bag，拒绝原因只走返回值等于
            // 「拒绝发生过但查不到」——批量场景下无法定位是哪批粒子被拒。
            note(
                bag,
                RenderDiag::ParamsRejected,
                String::from("拖尾形态未提供历史缓冲"),
                String::from("拖尾需逐粒子 TrailHistory；请传trails 或改用其他形态"),
            );
            return Outcome::fail(
                RenderDiag::ParamsRejected.code(),
                String::from("拖尾形态未提供历史缓冲"),
                String::from("拖尾需逐粒子 TrailHistory；请传trails 或改用其他形态"),
            )
        }
    };
    let mut stats = RenderStats::default();
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut segments = 0usize;
    let mut hist: Vec<[f32; 3]> = Vec::new();
    let mut nonfinite = 0usize;
    // 历史缓冲数应与粒子数一致；不一致只处理交集，不猜谁错。
    let n = if views.len() < trails.len() { views.len() } else { trails.len() };
    for i in 0..n {
        let v = &views[i];
        if !v.is_finite() {
            nonfinite += 1;
            continue;
        }
        if !v.visible() {
            stats.skipped += 1;
            continue;
        }
        let ring = &trails[i];
        ring.read_into(&mut hist);
        if hist.len() < TRAIL_MIN_HISTORY {
            // 历史不足两点：退化成一个点，计入 skipped 而非画零长度带。
            stats.skipped += 1;
            continue;
        }
        let used = if hist.len() > history { history } else { hist.len() };
        let start = hist.len() - used;
        for k in 0..(used - 1) {
            let p0 = hist[start + k];
            let p1 = hist[start + k + 1];
            // 段 k 距尾端的档位：k=0（最新段）最宽。
            let level = (k + 1) as f32 / used as f32;
            let w = decay * (1.0 - level);
            let half = v.size.max(0.0) * w * 0.5;
            // 带状宽度方向：段方向与世界上轴的叉积，退化时换轴重试。
            let seg_dir = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let side = normalize3(cross(seg_dir, [0.0, 1.0, 0.0]))
                .or_else(|| normalize3(cross(seg_dir, [1.0, 0.0, 0.0])))
                .unwrap_or([1.0, 0.0, 0.0]);
            let color = v.color;
            vertices.push(Vertex {
                position: [
                    p0[0] - side[0] * half,
                    p0[1] - side[1] * half,
                    p0[2] - side[2] * half,
                ],
                color,
            });
            vertices.push(Vertex {
                position: [
                    p0[0] + side[0] * half,
                    p0[1] + side[1] * half,
                    p0[2] + side[2] * half,
                ],
                color,
            });
            segments += 1;
        }
        stats.instances += 1;
    }
    stats.rejected = nonfinite;
    stats.ribbon_segments = segments;
    stats.vertices = vertices.len();
    if nonfinite > 0 {
        note(
            bag,
            RenderDiag::NonFiniteInput,
            format!("跳过 {} 个非有限粒子（拖尾）", nonfinite),
            String::from("拖尾顶点由历史位置生成，非有限会产出 NaN 顶点"),
        );
    }
    Outcome::ok(RenderBatch::Trail { vertices, segments, stats })
}

// ---------------------------------------------------------------------------
// 八、I02 绘制对接（判据三）
// ---------------------------------------------------------------------------

/// I02 绘制接口（对接签名声明）。
///
/// **本类型是「对接契约」而非「绘制实现」**：真正的 I02 绘制由I 域承接，
/// 本域只声明「粒子渲染需要被提交什么、能拿到什么就绪信号」。把它做成
/// trait 而非直接调I02 函数，是为了让未就绪状态**可被构造与测试**——
/// 若直连真实I02，「接口未就绪」这条降级路径在测试里根本无法复现。
pub trait InstanceSink {
    /// 实例缓冲是否就绪。
    ///
    /// 未就绪时 [`Self::submit`] 必挂起并告警（锚点：I02 接口未就绪 →
    /// 渲染挂起 + 告警，不静默）。
    fn is_ready(&self) -> bool;

    /// 写入实例缓冲的容量（槽位）。
    fn capacity(&self) -> usize;

    /// 提交一批渲染产物（对接 I02 绘制调用）。
    ///
    /// 返回是否成功提交。容量不足时**拒绝**而非截断——截断会让粒子
    /// 无声消失，锚点要求拒绝显性（错误三要素由 [`submit`] 给出）。
    fn submit(&mut self, batch: &RenderBatch) -> bool;
}

/// 提交结果（区分「已提交」「挂起」「拒绝」三种语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmitState {
    /// 已提交。
    Submitted,
    /// 挂起：I02 未就绪，可重试（不丢数据）。
    Suspended,
    /// 拒绝：容量不足或参数非法，**不可重试**，须改配置。
    Rejected,
}

impl SubmitState {
    /// 是否已提交。
    pub fn is_submitted(self) -> bool {
        self == SubmitState::Submitted
    }

    /// 是否可重试（仅挂起可重试）。
    pub fn is_retryable(self) -> bool {
        self == SubmitState::Suspended
    }
}

/// 提交渲染批到 I02（判据三的落点）。
///
/// 三条降级路径各有专属诊断码，处置方向相反的状态不共用码：
/// -未就绪 → 挂起（可重试，`SinkNotReady`）
/// - 容量不足 → 拒绝（不可重试，`SinkOverflow`），并给出错误三要素
pub fn submit(sink: &mut dyn InstanceSink, batch: &RenderBatch, bag: &mut DiagBag) -> SubmitState {
    // 挂起优先于容量：未就绪时谈容量没有意义。
    if !sink.is_ready() {
        note(
            bag,
            RenderDiag::SinkNotReady,
            format!("I02 绘制接口未就绪，本批{} 已挂起", batch_kind(batch)),
            String::from("渲染挂起不丢数据：I02 就绪后重投即可；请查 I02 初始化与设备选择"),
        );
        return SubmitState::Suspended;
    }
    // 容量对账：顶点型比顶点数，网格型比实例数。
    let need = match batch {
        RenderBatch::Mesh { instances, .. } => instances.len(),
        other => other.vertex_count(),
    };
    let cap = sink.capacity();
    if need > cap {
        // 错误三要素：当前需求/上限/建议。
        note(
            bag,
            RenderDiag::SinkOverflow,
            format!("实例缓冲容量不足：需{} 槽，上限 {}", need, cap),
            format!(
                "扩容到至少 {} 槽，或降发射率（F2204）/ 开排序免对齐（F2207 加法混合免排序）",
                need
            ),
        );
        return SubmitState::Rejected;
    }
    if sink.submit(batch) {
        SubmitState::Submitted
    } else {
        // sink 自身拒绝：按挂起处理（可重试），不谎报成功。
        note(
            bag,
            RenderDiag::SinkNotReady,
            String::from("I02 拒绝了本批提交"),
            String::from("I02 侧拒绝但未给出原因；本批挂起待重投"),
        );
        SubmitState::Suspended
    }
}

/// 批次形态名（诊断文案用）。
fn batch_kind(batch: &RenderBatch) -> &'static str {
    match batch {
        RenderBatch::Billboard { .. } => "面片批",
        RenderBatch::Mesh { .. } => "网格实例批",
        RenderBatch::Trail { .. } => "拖尾批",
    }
}

// ---------------------------------------------------------------------------
// 九、形态切换（帧边界闸，F1762 规则）
// ---------------------------------------------------------------------------

/// 形态切换器（帧边界闸）。
///
/// 锚点：形态切换走**帧边界**（F1762 规则，与 F2002 效果接口同款图重编译
/// 帧边界同源）。理由不是「惯例」而是**硬约束**：切换发生在半帧内会让本帧
/// 已写入的顶点缓冲与本帧要用的形态不一致——要么撕裂，要么读到上一形态
/// 的残留。故半帧请求必须**拒绝并产诊断**，而非「下次生效」（那样调用方
/// 以为已切换，下一帧的绘制参数却是错的）。
#[derive(Clone, Debug, PartialEq)]
pub struct FormSwitch {
    /// 当前生效形态。
    current: RenderForm,
    /// 当前形态参数。
    params: FormParams,
    /// 帧号（单调递增，注入而非取系统时钟）。
    frame: u64,
    /// 是否已到帧边界（每帧开始由 [`Self::begin_frame`] 置真）。
    at_boundary: bool,
    /// 累计切换次数（统计用）。
    switches: u64,
}

impl FormSwitch {
    /// 构造切换器（起始帧 0 且在边界上）。
    pub fn new(form: RenderForm, params: FormParams) -> Self {
        FormSwitch { current: form, params, frame: 0, at_boundary: true, switches: 0 }
    }

    /// 当前形态。
    pub fn current(&self) -> RenderForm {
        self.current
    }

    /// 当前参数。
    pub fn params(&self) -> &FormParams {
        &self.params
    }

    /// 当前帧号。
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// 是否处于帧边界。
    pub fn at_boundary(&self) -> bool {
        self.at_boundary
    }

    /// 累计切换次数。
    pub fn switches(&self) -> u64 {
        self.switches
    }

    /// 帧推进：离开帧边界。
    ///
    /// 必须在每帧的模拟之后、渲染之前调用一次——这是「帧边界」的唯一
    /// 真实来源。若允许调用方自己置位，那这道闸就形同虚设。
    pub fn advance_frame(&mut self) {
        self.frame = self.frame.saturating_add(1);
        self.at_boundary = false;
    }

    /// 请求切换形态（**仅帧边界生效**，否则拒绝并产诊断）。
    pub fn request_switch(
        &mut self,
        form: RenderForm,
        params: FormParams,
        bag: &mut DiagBag,
    ) -> Outcome<()> {
        if !self.at_boundary {
            // 半帧切换被拒必须**记袋**：调用方传了 bag 就是要收集拒绝原因，
            // 只返不记等于「拒绝发生过但查不到」。原实现此处静默返回，
            // 而配套判据又写成断言袋长不变——判据名声称「有诊断」而断言
            // 要求「无诊断」，名实相反，恰好把这条静默面洗成绿灯。
            let message =
                format!("帧 {} 中途请求切换形态 → 非帧边界", self.frame);
            let hint = String::from("形态切换只许在帧边界：半帧切换会使顶点缓冲与形态不一致");
            note(
                bag,
                RenderDiag::SwitchNotAtBoundary,
                message.clone(),
                hint.clone(),
            );
            return Outcome::fail(
                RenderDiag::SwitchNotAtBoundary.code(),
                message,
                hint,
            );
        }
        // 切换前先校验新参数：切到一个非法参数比切不过更糟。
        if let Outcome::Err { message, hint, .. } = validate_params(form, &params) {
            // 先clone 再用：诊断入袋与返回失败都要用到这份文本。
            note(
                bag,
                RenderDiag::ParamsRejected,
                format!("切换被拒：{}", message.clone()),
                hint.clone(),
            );
            return Outcome::fail(RenderDiag::ParamsRejected.code(), message, hint);
        }
        self.current = form;
        self.params = params;
        self.switches += 1;
        Outcome::ok(())
    }

    /// 帧边界重开（供帧末或下一次 [`Self::new`] 后的首次使用）。
    pub fn open_boundary(&mut self) {
        self.at_boundary = true;
    }
}

// ---------------------------------------------------------------------------
// 十、渲染器（解耦装配面）
// ---------------------------------------------------------------------------

/// 粒子渲染器：持有形态切换器并提供「一步到位」的渲染入口。
///
/// 只持**形态与参数**，不持粒子池、不持模拟状态——这是解耦的最后一环：
/// 渲染器连「模拟存在哪里」都不知道，故绝无可能顺手改模拟。
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleRenderer {
    switch: FormSwitch,
    /// 相机右向量（billboard 相机对齐朝向用）。
    cam_right: [f32; 3],
}

impl ParticleRenderer {
    /// 构造渲染器。
    pub fn new(form: RenderForm, params: FormParams, cam_right: [f32; 3]) -> Self {
        ParticleRenderer { switch: FormSwitch::new(form, params), cam_right }
    }

    /// 以 billboard 相机对齐构造（最常见形态）。
    pub fn billboard(size: f32) -> Self {
        ParticleRenderer::new(
            RenderForm::Billboard,
            FormParams::Billboard { size, facing: Facing::CameraRight },
            [1.0, 0.0, 0.0],
        )
    }

    /// 当前形态。
    pub fn form(&self) -> RenderForm {
        self.switch.current()
    }

    /// 当前参数。
    pub fn params(&self) -> &FormParams {
        self.switch.params()
    }

    /// 更新相机右向量（每帧由I 域相机给出）。
    pub fn set_cam_right(&mut self, v: [f32; 3]) {
        if normalize3(v).is_some() {
            self.cam_right = normalize3(v).unwrap_or(self.cam_right);
        }
    }

    /// 帧推进（离开帧边界）。
    pub fn advance_frame(&mut self) {
        self.switch.advance_frame();
    }

    /// 帧边界重开。
    pub fn open_boundary(&mut self) {
        self.switch.open_boundary();
    }

    /// 当前是否处于帧边界（帧边界闸的对外只读视图）。
    ///
    /// 帧边界是形态切换的硬前置，调用方常需先查再切，故对外暴露而不是
    /// 让每个调用方自己去摸私有字段。
    pub fn at_boundary(&self) -> bool {
        self.switch.at_boundary()
    }

    /// 当前帧号。
    pub fn frame(&self) -> u64 {
        self.switch.frame()
    }

    /// 当前相机右向量（billboard 相机对齐朝向所用）。
    pub fn cam_right(&self) -> [f32; 3] {
        self.cam_right
    }

    /// 请求切换形态（帧边界闸）。
    pub fn request_switch(
        &mut self,
        form: RenderForm,
        params: FormParams,
        bag: &mut DiagBag,
    ) -> Outcome<()> {
        self.switch.request_switch(form, params, bag)
    }

    /// 累计切换次数。
    pub fn switches(&self) -> u64 {
        self.switch.switches()
    }

    /// 按当前形态构建渲染批（**只读**消费视图）。
    pub fn build(
        &self,
        views: &[ParticleView],
        trails: Option<&[TrailHistory]>,
        bag: &mut DiagBag,
    ) -> Outcome<RenderBatch> {
        build(self.switch.current(), self.switch.params(), views, self.cam_right, trails, bag)
    }

    /// 构建并提交（一步到位）。
    pub fn render(
        &self,
        views: &[ParticleView],
        trails: Option<&[TrailHistory]>,
        sink: &mut dyn InstanceSink,
        bag: &mut DiagBag,
    ) -> (Option<RenderBatch>, SubmitState) {
        match self.build(views, trails, bag) {
            Outcome::Ok { value, .. } => {
                let state = submit(sink, &value, bag);
                if state.is_submitted() {
                    (Some(value), state)
                } else {
                    // 挂起/拒绝时**不返回批次**，避免调用方误以为已绘制。
                    (None, state)
                }
            }
            Outcome::Err { .. } => (None, SubmitState::Rejected),
        }
    }
}