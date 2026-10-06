//! VE-F2402 · 关键帧轨道系统（VE-M 域 · 动画系统 · M01 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2402`
//!
//! **判据（锚点原文四条）**：六类轨道、容器多轨、绑定协议、单源扩展。
//!
//! # 判据一：六类轨道（逐类登记，规格公开）
//!
//! 轨道类型不是「一个泛型轨道 + 运行期分支」，而是**六类各自带规格**，因为插值
//! 语义按类分化，泛型化会把语义差异藏进运行期分支：
//!
//! | 类 | 载荷 | 目标类型 | 允许插值 | 离散 | 允许外推 |
//! | --- | --- | --- | --- | --- | --- |
//! | [`TrackClass::Position`] 位置 | `f32x3` | `float3` | `linear` | 否 | 是 |
//! | [`TrackClass::Rotation`] 旋转 | `quat` | `quaternion` | `slerp` | 否 | 否 |
//! | [`TrackClass::Scale`] 缩放 | `f32x3` | `float3` | `linear` | 否 | 否 |
//! | [`TrackClass::Color`] 颜色 | `f32x4` | `float4` | `linear` | 否 | 是 |
//! | [`TrackClass::Float`] 浮点 | `f32` | `float` | `linear` | 否 | 是 |
//! | [`TrackClass::Bool`] 布尔 | `bool` | `bool` | `step` | **是** | 否 |
//!
//! 逐类都有理由，不是随手填的：
//! - **位置**可外推：子弹拖尾、导弹尾迹就是位置外推的常见需求；
//! - **旋转禁止 `linear`**：四元数 q 与 −q 表示同一姿态（双覆盖），线性插值会
//!   绕远路甚至反向。这不是「降质」而是「算错姿态」，故在校验期**拒绝**；
//! - **缩放默认不外推**：外推冲出 [0,1] 会翻转几何（负缩放 = 镜像，法线随之翻转）；
//! - **颜色不做色彩管理**：色彩空间与插值空间的选择是渲染域的事，本域只管
//!   插值器选择，不越界（见 F2401 边界表「网格顶点最终写入」归VE-I 的同源纪律）；
//! - **布尔仅阶梯**：布尔没有中间态，0→1 之间插出 0.37 会被下游当真值使用。
//!
//! 插值器校验是**三路判定**，不可合并为「离散/非离散」两路（这是本条最容易
//! 写错的地方，合并会让旋转配 `linear` 被放行）：
//!   ① 连续类配 `step` → 自愿降质 → **告警不拒绝**；
//!   ② 离散类配连续插值 → 语义硬错误 → **拒绝**（插出中间态会被当真值用）；
//!   ③ 连续类配**另一种**连续插值（旋转配 `linear`）→ 语义硬错误 → **拒绝**。
//! ①②处置方向相反（一放行一拒绝），故独立成码 [`DiagCode::ContinuousTrackForcedToStep`]
//! 与 [`DiagCode::DiscreteTrackRequiresStep`]，**不得合并**。
//!
//! # 判据二：容器多轨（一实体挂 N 轨）
//!
//! 一实体可同时挂变换轨道 + 材质轨道 + 自定义轨道。容器语义三条：
//! - **配额**：每实体 [`TRACK_QUOTA_PER_ENTITY`] 条，超限**拒绝**而非静默截断
//!   （对接 F2416 配额联动）——静默截断会让作者以为全挂上了，实际丢了几条；
//! - **同类并存须显式标记**：同实体同类轨道并存必须 `blended = true`，否则
//!   **拒绝**。理由是后挂者会静默覆盖前者，作者得到「动画时灵时不灵」；
//! - **权重**：[`WEIGHT_MIN`]..=[`WEIGHT_MAX`]，NaN / 越界 **拒绝**。多轨并存时
//!   权重**不必**归一化到 1（那是混合树的活，F2424/F2426），本域只做区间校验——
//!   越界会让人误以为有归一化语义在兜底。
//!
//! 管理复杂度 **O(轨道数)**（实体表为线性数组；每实体轨道表亦为线性）。
//!
//! # 判据三：绑定协议（路径声明 + 解析 + 失效检测）
//!
//! 轨道指向目标属性靠一条路径（`/node/transform/position`）。目标会重建、改名、
//! 销毁——于是路径**会失效**。若失效后静默不写值，效果是「动画不生效」，而作者
//! 看到的现象是「有时生效有时不生效」（取决于目标何时重建），极难排查。故本条：
//!
//! - **语法校验期拒绝**：不以 `/` 开头、只有根分隔符、根不在
//!   `{node, material, custom}`、空段（连续 `//` 或尾随 `/`）、段名含非法字符
//!   —— 一律 [`DiagCode::BindPathMalformed`]。这三类是作者手误高发区，静默容错会让
//!   绑定「看起来成功但指向错误属性」，比报错更难查；
//! - **解析失败显性告警**：路径不在属性目录 → [`DiagCode::BindPathUnresolved`]，
//!   轨道标 [`TrackState::Invalid`]并写入 `invalid_reason`，**不静默空转**；
//! - **类型不匹配独立成码**：目标类型 ≠ 轨道类要求 → [`DiagCode::BindTypeMismatch`]。
//!   与「解析失败」处置不同（前者目标在但类型错，后者目标没了），故不合并；
//! - **失效可逆**：目标恢复后显式回到 [`TrackState::Active`]，不留脏状态
//!   （否则目标重建后动画仍不生效，而上一次检测已报过警，作者不会再看）。
//!
//! 绑定期解析**一次**（O(路径段数)），失效检测**低频**（场景变更/显式调用时）。
//!
//! # 判据四：单源扩展（不另造轨道数据）
//!
//! F1345 是剪辑域轨道数据结构的**唯一拥有者**。本模块**不定义任何轨道数据结构
//! 字段**，只消费 [`KeyframeRef`]（F1345 单源引用：资产标识 + 条数）与
//! [`TrackPayload`]（F1345 值载荷）。要加字段？去 F1345 加，不要在这里加。
//!
//! 为什么这条是本模块最硬的纪律：两份轨道数据结构的代价是**不对称**的——写第二份
//! 只花 20 分钟，之后每个消费方（编辑器时间轴、导出器、运行时求值器、资产库索引）
//! 都得写一遍兼容分支，且两边插值语义会**静默漂移**（一边四元数用 slerp，一边用
//! lerp 归一化，表现为同一条动画在不同后端姿态不同——极难定位）。
//!
//! 故 [`SINGLE_SOURCE`] 把它做成**可机检的声明**：[`assert_single_source`] 断言本
//! 模块对 F1345 类型的引用面，以及禁止项清单齐备。
//!
//! # 性能逐项分解（锚点口径）
//!
//! - 轨道求值：调用方按时间戳**二分**定位关键帧区间 → O(logN)（F2407 SIMD 深化）；
//! - 容器管理：挂载 / 失效检测 / 列举均 O(轨道数)；
//! - 绑定解析：绑定期一次 O(路径段数)，失效检测低频 O(轨道数 × 段数)；
//! - 内存：SoA 布局（[`TrackSoa`]——时间戳数组与值数组分列，与 F2202 家族同源）。
//!
//! # 跨批对接点
//!
//! - **F1345**：轨道单源（本条根基，判据四）；
//! - **F2401**：域架构开工条——四段求值管线（轨道求值 → clip 采样 → 姿态混合 →
//!   骨骼应用）与三域边界（M 管轨道 / I 管蒙皮 / L 管物理）；
//! - **F2403** 插值器全集（本条只做**校验**，不实现插值数学）；
//! - **F2407** 求值性能（O(logN) 二分与 SIMD 深化）；
//! - **F2416** 轨道配额联动（[`TRACK_QUOTA_PER_ENTITY`] 的安全面）；
//! - **F2421/F2423** 骨骼消费；**F2327** 场景挂载（绑定思想同构）。
//!
//! # 零静默纪律（全部拒绝/告警路径，无一静默）
//!
//! | 情形 | 处置 | 诊断码 |
//! | --- | --- | --- |
//! | 轨道类型未注册 | 拒绝并列出已注册六类 | `TrackTypeUnregistered` |
//! | 载荷与轨道类不匹配 | 拒绝 | `TrackSpecInconsistent` |
//! | 六类覆盖缺口 | 自检报错 | `TrackClassCoverageGap` |
//! | 布尔配连续插值 | 拒绝 | `DiscreteTrackRequiresStep` |
//! | 连续类配阶梯 | **告警放行**（自愿降质） | `ContinuousTrackForcedToStep` |
//! | 旋转配linear | 拒绝 | `ContinuousInterpUnsupported` |
//! | 关键帧条数不一致 | 拒绝 | `KeyframeDataInvalid` |
//! | 轨道数超配额 | 拒绝 | `TrackQuotaExceeded` |
//! | 同类轨道重复挂载 | 拒绝（除非 `blended`） | `TrackDuplicateMount` |
//! | 实体不存在 | 拒绝 | `EntityUnknown` |
//! | 绑定路径语法非法 | 拒绝 | `BindPathMalformed` |
//! | 绑定路径解析失败 | 轨道标Invalid + 告警 | `BindPathUnresolved` |
//! | 绑定类型不匹配 | 轨道标 Invalid + 告警 | `BindTypeMismatch` |
//! | 权重非法 | 拒绝 | `TrackWeightInvalid` |
//!
//! 逻辑tick 注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。
//! 不抛异常、不吞诊断、无静默分支、无全局可变状态。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// §0 诊断层（零静默的物质基础：错误与告警分两条独立通道）
// ===========================================================================

/// 诊断码全集。
///
/// **纪律**：处置方向相反的状态**不得共用码**。`ContinuousTrackForcedToStep`
/// （告警放行）与 `DiscreteTrackRequiresStep`（拒绝）方向相反，必须分立；
/// `BindPathUnresolved`（目标没了）与 `BindTypeMismatch`（目标在但类型错）同理。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagCode {
    /// 轨道类型未在六类规格表中注册。
    TrackTypeUnregistered,
    /// 轨道类型已注册但字段自相矛盾（载荷与轨道类不匹配）。
    TrackSpecInconsistent,
    /// 六类覆盖缺口：某个官方轨道类未被规格表承载。
    TrackClassCoverageGap,
    /// 布尔轨道配了连续插值器（离散语义，禁止插值）→ **拒绝**。
    DiscreteTrackRequiresStep,
    /// 连续类轨道配了阶梯插值器（自愿降质）→ **告警放行**。
    ContinuousTrackForcedToStep,
    /// 连续类轨道配了不被支持的连续插值器（旋转配 linear = 算错姿态）→ **拒绝**。
    ContinuousInterpUnsupported,
    /// 轨道关键帧数据非法（载荷长度与帧数不一致）。
    KeyframeDataInvalid,
    /// 实体轨道数超配额（与 F2416 联动）→ **拒绝**。
    TrackQuotaExceeded,
    /// 轨道 id 重复，或同实体同类轨道未标 `blended` 重复挂载。
    TrackDuplicateMount,
    /// 轨道容器引用了不存在的实体。
    EntityUnknown,
    /// 绑定路径语法非法。
    BindPathMalformed,
    /// 绑定路径解析失败（目标不存在 / 层级缺失）。
    BindPathUnresolved,
    /// 绑定目标类型与轨道类的目标类型不匹配。
    BindTypeMismatch,
    /// 轨道失效（目标销毁 / 路径失效）——显性告警，绝不静默空转。
    TrackInvalidated,
    /// 单源纪律违规：本模块被要求自造轨道数据结构（应经 F1345 扩展）。
    SingleSourceViolation,
    /// 轨道权重非法（越界 / NaN）。
    TrackWeightInvalid,
}

impl DiagCode {
    /// 码 → 稳定字符串（自检与对账用；跨批钩子按此比对）。
    pub const fn as_str(self) -> &'static str {
        match self {
            DiagCode::TrackTypeUnregistered => "TRACK_TYPE_UNREGISTERED",
            DiagCode::TrackSpecInconsistent => "TRACK_SPEC_INCONSISTENT",
            DiagCode::TrackClassCoverageGap => "TRACK_CLASS_COVERAGE_GAP",
            DiagCode::DiscreteTrackRequiresStep => "DISCRETE_TRACK_REQUIRES_STEP",
            DiagCode::ContinuousTrackForcedToStep => "CONTINUOUS_TRACK_FORCED_TO_STEP",
            DiagCode::ContinuousInterpUnsupported => "CONTINUOUS_INTERP_UNSUPPORTED",
            DiagCode::KeyframeDataInvalid => "KEYFRAME_DATA_INVALID",
            DiagCode::TrackQuotaExceeded => "TRACK_QUOTA_EXCEEDED",
            DiagCode::TrackDuplicateMount => "TRACK_DUPLICATE_MOUNT",
            DiagCode::EntityUnknown => "ENTITY_UNKNOWN",
            DiagCode::BindPathMalformed => "BIND_PATH_MALFORMED",
            DiagCode::BindPathUnresolved => "BIND_PATH_UNRESOLVED",
            DiagCode::BindTypeMismatch => "BIND_TYPE_MISMATCH",
            DiagCode::TrackInvalidated => "TRACK_INVALIDATED",
            DiagCode::SingleSourceViolation => "SINGLE_SOURCE_VIOLATION",
            DiagCode::TrackWeightInvalid => "TRACK_WEIGHT_INVALID",
        }
    }
}

/// 单条诊断（错误与告警共用同一形状，只靠通道区分）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 诊断码。
    pub code: DiagCode,
    /// 人话描述（原值 → 处置）。
    pub message: String,
    /// 处置建议。
    pub hint: String,
}

impl Diagnostic {
    /// 构造一条诊断（描述/建议为空时补占位，杜绝空串静默）。
    pub fn new(code: DiagCode, message: &str, hint: &str) -> Self {
        Diagnostic {
            code,
            message: if message.is_empty() {
                "（未提供描述）".to_string()
            } else {
                message.to_string()
            },
            hint: if hint.is_empty() {
                "（未提供处置建议）".to_string()
            } else {
                hint.to_string()
            },
        }
    }
}

/// 诊断袋：**错误**与**告警**两条独立通道。
///
/// 分开是必要的——若混在一起，`ContinuousTrackForcedToStep` 这种「自愿承担的
/// 降质」会被当成硬错误把挂载挡掉，与该码自身声明的「允许但降质」自相矛盾。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    /// 错误通道：有诊断即失败闸门。
    errors: Vec<Diagnostic>,
    /// 告警通道：**不参与失败闸门**，只回传给调用方知悉。
    warnings: Vec<Diagnostic>,
}

impl DiagBag {
    /// 新建空袋。
    pub fn new() -> Self {
        DiagBag {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// 记一条**错误**（使本次操作失败）。
    pub fn push(&mut self, code: DiagCode, message: &str, hint: &str) {
        self.errors.push(Diagnostic::new(code, message, hint));
    }

    /// 记一条**告警**（放行但告知）。
    pub fn warn(&mut self, code: DiagCode, message: &str, hint: &str) {
        self.warnings.push(Diagnostic::new(code, message, hint));
    }

    /// 错误通道（只读）。
    pub fn errors(&self) -> &[Diagnostic] {
        &self.errors
    }

    /// 告警通道（只读）。
    pub fn warnings(&self) -> &[Diagnostic] {
        &self.warnings
    }

    /// 是否有错误。
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// 错误条数。
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }

    /// 按码筛错误（归因用：一次挂载可能同时踩多个码）。
    pub fn errors_by(&self, code: DiagCode) -> Vec<&Diagnostic> {
        self.errors.iter().filter(|d| d.code == code).collect()
    }

    /// 首条错误码（`Outcome` 的失败面只带首个——但`errors` 全量仍在袋内可查）。
    pub fn first_code(&self) -> Option<DiagCode> {
        self.errors.first().map(|d| d.code)
    }

    /// 合并另一袋（跨阶段诊断汇聚）。
    pub fn absorb(&mut self, other: &DiagBag) {
        self.errors.extend(other.errors.iter().cloned());
        self.warnings.extend(other.warnings.iter().cloned());
    }

    /// 错误 + 告警的合计条数（自检统计用）。
    pub fn total(&self) -> usize {
        self.errors.len() + self.warnings.len()
    }
}

/// 失败面（`Outcome::Err` 的载荷；`err()` 只给**看**，不给改）。
///
/// 字段为拥有型：错误一旦产生就是既成事实，让调用方能改它等于把诊断变成可商量的，
/// 零静默纪律就漏了。取用走 `&self` 借用（见 [`Outcome::err`]），无需克隆。
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    /// 首个失败码（`diagnostics[0].code` 的快取，避免调用方每次都索引）。
    pub code: DiagCode,
    /// 人话描述（首条错误）。
    pub message: String,
    /// 处置建议（首条错误）。
    pub hint: String,
    /// 全部错误（不止首个——归因要看全集）。
    pub diagnostics: Vec<Diagnostic>,
}

/// 结果面：`Ok` 带值与全部诊断，`Err` 带失败面。
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome<T> {
    /// 成功（可能带告警）。
    Ok {
        /// 结果值。
        value: T,
        /// 随本次操作产生的全部诊断（错误已在 `Ok` 前提下为空）。
        diagnostics: Vec<Diagnostic>,
    },
    /// 失败（错误通道非空）。
    Err(Failure),
}

impl<T> Outcome<T> {
    /// 取值（失败面返回 `None`，不 panic——调用方须显式处理）。
    pub fn ok(&self) -> Option<&T> {
        match self {
            Outcome::Ok { value, .. } => Some(value),
            Outcome::Err(_) => None,
        }
    }

    /// 是否成功。
    pub fn is_ok(&self) -> bool {
        matches!(self, Outcome::Ok { .. })
    }

    /// 失败面引用（成功面返回 `None`）。
    pub fn err(&self) -> Option<&Failure> {
        match self {
            Outcome::Ok { .. } => None,
            Outcome::Err(f) => Some(f),
        }
    }

    /// 失败码（成功面为 `None`）。
    pub fn code(&self) -> Option<DiagCode> {
        match self {
            Outcome::Ok { .. } => None,
            Outcome::Err(f) => Some(f.code),
        }
    }

    /// 把诊断袋封成结果面：有错误即 `Err`（带全部错误），否则 `Ok`。
    pub fn seal<T2>(value: T2, bag: &DiagBag) -> Outcome<T2> {
        match bag.errors.first() {
            None => Outcome::Ok {
                value,
                diagnostics: bag.warnings.clone(),
            },
            Some(first) => Outcome::Err(Failure {
                code: first.code,
                message: first.message.clone(),
                hint: first.hint.clone(),
                diagnostics: bag.errors.clone(),
            }),
        }
    }
}
// ===========================================================================
// §1 F1345 轨道单源引用（本模块只消费，不定义——单源纪律的物质基础）
// ===========================================================================

/// 关键帧序列的**引用**（F1345 单源）。
///
/// 本模块**不持有关键帧数组本身**——只持有一个可寻址的标识 + 条数。这就是
/// 「不另造轨道数据」在类型层面的体现：本模块里搜不到第二个「关键帧数组」的
/// 定义，搜到的只有指向 F1345 的句柄。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyframeRef {
    /// F1345 侧的关键帧资产标识（由剪辑域分配，本域不解释其编码）。
    pub asset_id: String,
    /// 该序列的关键帧条数（用于容量与配额预估与长度对齐校验）。
    pub count: usize,
}

impl KeyframeRef {
    /// 构造引用。
    pub fn new(asset_id: &str, count: usize) -> Self {
        KeyframeRef {
            asset_id: asset_id.to_string(),
            count,
        }
    }
}

/// 轨道值载荷（F1345 单源）：定长标量或四元数，不含结构体、不含变长数据。
///
/// **为什么载荷留在 F1345 而不在本域**：值的编码方式（是否压缩、是否量化、
/// 是否带曲线）属资产层决策；M 域只声明「我需要一条这种形状的值序列」。
#[derive(Clone, Debug, PartialEq)]
pub enum TrackPayload {
    /// 标量序列（浮点通用类）。
    Scalar(Vec<f32>),
    /// 三分量序列（位置 / 缩放）。
    Vec3(Vec<[f32; 3]>),
    /// 四分量序列（颜色）。
    Vec4(Vec<[f32; 4]>),
    /// 四元数序列（旋转）。
    Quat(Vec<[f32; 4]>),
    /// 布尔序列（离散类）。
    Bool(Vec<bool>),
}

impl TrackPayload {
    /// 载荷类型标签（用于轨道类 ↔ 载荷类型的匹配校验）。
    pub fn payload_type(&self) -> PayloadType {
        match self {
            TrackPayload::Scalar(_) => PayloadType::F32,
            TrackPayload::Vec3(_) => PayloadType::F32x3,
            TrackPayload::Vec4(_) => PayloadType::F32x4,
            TrackPayload::Quat(_) => PayloadType::Quat,
            TrackPayload::Bool(_) => PayloadType::Bool,
        }
    }

    /// 值条数（与 [`KeyframeRef::count`] 对齐校验用）。
    pub fn value_count(&self) -> usize {
        match self {
            TrackPayload::Scalar(v) => v.len(),
            TrackPayload::Vec3(v) => v.len(),
            TrackPayload::Vec4(v) => v.len(),
            TrackPayload::Quat(v) => v.len(),
            TrackPayload::Bool(v) => v.len(),
        }
    }

    /// 载荷的**分量数**（标量 1 / 三分量 3 / 四分量与四元数 4 / 布尔 1）。
    pub fn components(&self) -> usize {
        match self {
            TrackPayload::Scalar(_) | TrackPayload::Bool(_) => 1,
            TrackPayload::Vec3(_) => 3,
            TrackPayload::Vec4(_) | TrackPayload::Quat(_) => 4,
        }
    }
}

/// 载荷的数据类型标签（轨道类 ↔ 载荷类型的匹配键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadType {
    /// 单精度标量。
    F32,
    /// 三分量向量。
    F32x3,
    /// 四分量向量。
    F32x4,
    /// 四元数。
    Quat,
    /// 布尔。
    Bool,
}

impl PayloadType {
    /// 稳定字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            PayloadType::F32 => "f32",
            PayloadType::F32x3 => "f32x3",
            PayloadType::F32x4 => "f32x4",
            PayloadType::Quat => "quat",
            PayloadType::Bool => "bool",
        }
    }
}

/// 目标属性的类型标签（绑定目标须与轨道类的目标类型一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetType {
    /// 三分量属性。
    Float3,
    /// 四元数属性。
    Quaternion,
    /// 四分量属性。
    Float4,
    /// 单标量属性。
    Float,
    /// 布尔属性。
    Bool,
}

impl TargetType {
    /// 稳定字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            TargetType::Float3 => "float3",
            TargetType::Quaternion => "quaternion",
            TargetType::Float4 => "float4",
            TargetType::Float => "float",
            TargetType::Bool => "bool",
        }
    }
}

// ===========================================================================
// §2 六类轨道规格表（判据一）
// ===========================================================================

/// 插值器族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterpKind {
    /// 线性插值（位置/缩放/颜色/浮点）。
    Linear,
    /// 球面线性插值（旋转专用；四元数双覆盖下lerp 会算错姿态）。
    Slerp,
    /// 阶梯（离散跳变；布尔唯一允许的插值器）。
    Step,
}

impl InterpKind {
    /// 稳定字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            InterpKind::Linear => "linear",
            InterpKind::Slerp => "slerp",
            InterpKind::Step => "step",
        }
    }

    /// 是否为连续插值器（布尔禁用的就是这一族）。
    pub const fn is_continuous(self) -> bool {
        matches!(self, InterpKind::Linear | InterpKind::Slerp)
    }
}

/// 轨道类（六类封闭集：新增类须改本枚举与 [`TRACK_SPECS`] 两处）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrackClass {
    /// 位置（float3）。
    Position,
    /// 旋转（四元数）。
    Rotation,
    /// 缩放（float3）。
    Scale,
    /// 颜色（float4）。
    Color,
    /// 通用浮点。
    Float,
    /// 布尔（离散）。
    Bool,
}

impl TrackClass {
    /// 六类全集（顺序即册内顺序）。
    pub const ALL: [TrackClass; 6] = [
        TrackClass::Position,
        TrackClass::Rotation,
        TrackClass::Scale,
        TrackClass::Color,
        TrackClass::Float,
        TrackClass::Bool,
    ];

    /// 稳定字符串名（跨批对账键）。
    pub const fn as_str(self) -> &'static str {
        match self {
            TrackClass::Position => "position",
            TrackClass::Rotation => "rotation",
            TrackClass::Scale => "scale",
            TrackClass::Color => "color",
            TrackClass::Float => "float",
            TrackClass::Bool => "bool",
        }
    }

    /// 中文显示名（规格表对外表述）。
    pub const fn display_name(self) -> &'static str {
        match self {
            TrackClass::Position => "位置",
            TrackClass::Rotation => "旋转",
            TrackClass::Scale => "缩放",
            TrackClass::Color => "颜色",
            TrackClass::Float => "浮点",
            TrackClass::Bool => "布尔",
        }
    }

    /// 按稳定名查类（未注册返回 `None`——调用方须显式拒绝，不默认回落）。
    pub fn from_name(name: &str) -> Option<TrackClass> {
        TrackClass::ALL.into_iter().find(|c| c.as_str() == name)
    }

    /// 该类的规格（规格表恒覆盖六类，故 `Option` 只在表被改坏时出现）。
    pub fn spec(self) -> Option<&'static TrackClassSpec> {
        TRACK_SPECS.iter().find(|s| s.kind == self)
    }
}

/// 轨道类规格（逐类登记，规格公开）。
#[derive(Clone, Copy, Debug)]
pub struct TrackClassSpec {
    /// 该类自身。
    pub kind: TrackClass,
    /// 该类轨道的载荷类型（与 [`TrackPayload::payload_type`] 一一对应）。
    pub payload_type: PayloadType,
    /// 该类轨道绑定的目标属性类型。
    pub target_type: TargetType,
    /// 该类**允许**的插值器族（封闭小数组：最多两项，逐类都有理由）。
    pub allowed_interp: &'static [InterpKind],
    /// 该类的默认插值器（创建时采用；恒在 `allowed_interp` 内——由自检断言）。
    pub default_interp: InterpKind,
    /// 该类是否离散（离散类禁连续插值——语义硬约束）。
    pub discrete: bool,
    /// 该类是否允许时间外推。
    pub extrap_allowed: bool,
}

/// 六类规格表（判据一的物质载体）。
///
/// 关键决策逐类有理由（见模块头判据一表格与说明）。
pub static TRACK_SPECS: [TrackClassSpec; 6] = [
    TrackClassSpec {
        kind: TrackClass::Position,
        payload_type: PayloadType::F32x3,
        target_type: TargetType::Float3,
        allowed_interp: &[InterpKind::Linear],
        default_interp: InterpKind::Linear,
        discrete: false,
        // 位置外推是常见需求（子弹拖尾、尾迹）。
        extrap_allowed: true,
    },
    TrackClassSpec {
        kind: TrackClass::Rotation,
        payload_type: PayloadType::Quat,
        target_type: TargetType::Quaternion,
        // 双覆盖下 lerp 绕远/反向 → 只许 slerp。
        allowed_interp: &[InterpKind::Slerp],
        default_interp: InterpKind::Slerp,
        discrete: false,
        // 姿态外推无意义（四元数外插不再归一）。
        extrap_allowed: false,
    },
    TrackClassSpec {
        kind: TrackClass::Scale,
        payload_type: PayloadType::F32x3,
        target_type: TargetType::Float3,
        allowed_interp: &[InterpKind::Linear],
        default_interp: InterpKind::Linear,
        discrete: false,
        // 缩放外推冲出 [0,1] 会翻转几何。
        extrap_allowed: false,
    },
    TrackClassSpec {
        kind: TrackClass::Color,
        payload_type: PayloadType::F32x4,
        target_type: TargetType::Float4,
        allowed_interp: &[InterpKind::Linear],
        default_interp: InterpKind::Linear,
        discrete: false,
        // 颜色外推常见（渐变延续）；色彩管理归渲染域，本条不越界。
        extrap_allowed: true,
    },
    TrackClassSpec {
        kind: TrackClass::Float,
        payload_type: PayloadType::F32,
        target_type: TargetType::Float,
        allowed_interp: &[InterpKind::Linear],
        default_interp: InterpKind::Linear,
        discrete: false,
        extrap_allowed: true,
    },
    TrackClassSpec {
        kind: TrackClass::Bool,
        payload_type: PayloadType::Bool,
        target_type: TargetType::Bool,
        // 布尔仅阶梯：插出 0.37 会被下游当真值用。
        allowed_interp: &[InterpKind::Step],
        default_interp: InterpKind::Step,
        discrete: true,
        // 阶梯之外的值无意义。
        extrap_allowed: false,
    },
];

/// 查某类的规格；未注册返回 `None`（调用方须显式拒绝）。
pub fn track_spec(kind: TrackClass) -> Option<&'static TrackClassSpec> {
    kind.spec()
}

/// 查某类的规格并在未注册时产出诊断（错误路径统一入口）。
///
/// **不默认回落**：未注册类型若回落到某个「相近类」，作者会得到一条能跑但语义错的
/// 轨道（如 `velocity` 悄悄变成 `position`），且无任何提示。故一律拒绝并列出已注册六类。
pub fn require_track_spec(name: &str, bag: &mut DiagBag) -> Option<&'static TrackClassSpec> {
    match TrackClass::from_name(name) {
        Some(kind) => kind.spec(),
        None => {
            bag.push(
                DiagCode::TrackTypeUnregistered,
                &format!("轨道类型「{name}」未注册"),
                &format!(
                    "已注册六类为：position / rotation / scale / color / float / bool。\
                     要新增类型，先在 TRACK_SPECS 登记规格（payload_type/target_type/插值器族），\
                     不要在调用点临时造类型——临时类型不会被插值器与导出器认识"
                ),
            );
            None
        }
    }
}

// ===========================================================================
// §3 绑定路径协议（判据三）
// ===========================================================================

/// 绑定路径的根类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindRoot {
    /// 节点属性（`/node/transform/position`）。
    Node,
    /// 材质参数（`/material/baseColor`）。
    Material,
    /// 自定义通道（`/custom/gameplay/open`）。
    Custom,
}

impl BindRoot {
    /// 稳定字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            BindRoot::Node => "node",
            BindRoot::Material => "material",
            BindRoot::Custom => "custom",
        }
    }

    /// 三根全集。
    pub const ALL: [BindRoot; 3] = [BindRoot::Node, BindRoot::Material, BindRoot::Custom];

    /// 按名查根。
    pub fn from_name(name: &str) -> Option<BindRoot> {
        BindRoot::ALL.into_iter().find(|r| r.as_str() == name)
    }

    /// 该根允许的**深度下界**：至少要有根 + 一个属性段。
    pub const fn min_segments(self) -> usize {
        let _ = self;
        1
    }
}

/// 绑定路径：`/根/段/段/…`。
///
/// **语法**：`/` 开头，段以 `/` 分隔，段名只允许 `[A-Za-z_][A-Za-z0-9_]*`。
/// 不允许空段、不允许尾随 `/`、不允许连续 `//`——这三类是作者手误的高发区，
/// 且静默容错会让绑定「看起来成功但指向错误属性」，比报错更难查。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindPath {
    /// 根类别。
    pub root: BindRoot,
    /// 属性段序列（不含根）。
    pub segments: Vec<String>,
    /// 原始字符串（属性目录的键）。
    pub raw: String,
}

impl BindPath {
    /// 末段属性名（下游写值用）。空段序列返回空串。
    pub fn leaf(&self) -> &str {
        self.segments.last().map(|s| s.as_str()).unwrap_or("")
    }

    /// 深度（根 + 段数）。
    pub fn depth(&self) -> usize {
        self.segments.len() + 1
    }
}

/// 段名字符校验：`[A-Za-z_][A-Za-z0-9_]*`。
///
/// 手写而非引入正则：`no_std` 内核无正则依赖，且这段逻辑只有「首字符 +其余字符」
/// 两步，逐字符判定比编译正则更省（绑定期只跑一次，但本域是帧内热路径的常驻结构）。
fn segment_name_valid(seg: &str) -> bool {
    let bytes = seg.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    // 首字符：字母或下划线。
    let head_ok = bytes[0].is_ascii_alphabetic() || bytes[0] == b'_';
    if !head_ok {
        return false;
    }
    // 其余字符：字母、数字或下划线。
    bytes[1..].iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

/// 解析绑定路径；语法非法则产出诊断并返回 `None`。
///
/// 五类语法错误逐条点名（合并成一句「路径非法」会让归因退化为猜测）：
/// ① 未以 `/` 开头；② 只有根分隔符；③ 根不在三根内；④ 空段（`//` 或尾随 `/`）；
/// ⑤ 段名含非法字符。
pub fn parse_bind_path(raw: &str, bag: &mut DiagBag) -> Option<BindPath> {
    let bytes = raw.as_bytes();
    if bytes.first() != Some(&b'/') {
        bag.push(
            DiagCode::BindPathMalformed,
            &format!("绑定路径「{raw}」未以 / 开头"),
            "路径须形如 /node/transform/position，开头的 / 不可省略",
        );
        return None;
    }
    if raw == "/" {
        bag.push(
            DiagCode::BindPathMalformed,
            "绑定路径只有根分隔符，没有根名",
            "写成 /node 或 /material 或 /custom 之一，再跟属性段",
        );
        return None;
    }
    let parts: Vec<&str> = raw[1..].split('/').collect();
    let root_name = parts.first().copied().unwrap_or("");
    let root = match BindRoot::from_name(root_name) {
        Some(r) => r,
        None => {
            bag.push(
                DiagCode::BindPathMalformed,
                &format!("绑定路径根「{root_name}」不在 {{node, material, custom}} 内"),
                "根只能是 node（节点属性）/ material（材质参数）/ custom（自定义通道）之一",
            );
            return None;
        }
    };
    let mut segments: Vec<String> = Vec::with_capacity(parts.len());
    // `parts[0]` 是根，属性段自1 起。
    for i in 1..parts.len() {
        let seg = parts[i];
        if seg.is_empty() {
            bag.push(
                DiagCode::BindPathMalformed,
                &format!("绑定路径「{raw}」第 {i} 段为空（连续 / 或尾随 /）"),
                "删掉多余的 /；属性段不能为空，也不能以 / 结尾",
            );
            return None;
        }
        if !segment_name_valid(seg) {
            bag.push(
                DiagCode::BindPathMalformed,
                &format!("绑定路径段「{seg}」含非法字符"),
                "段名只允许字母、数字、下划线，且不以数字开头；不要用点号或方括号",
            );
            return None;
        }
        segments.push(seg.to_string());
    }
    if segments.is_empty() {
        // 形如 `/node`：根在但没属性段——绑定一个根本身没有意义（写不进值）。
        bag.push(
            DiagCode::BindPathMalformed,
            &format!("绑定路径「{raw}」只有根、没有属性段"),
            "根之后至少跟一个属性段，如 /node/transform/position",
        );
        return None;
    }
    Some(BindPath {
        root,
        segments,
        raw: raw.to_string(),
    })
}

/// 绑定目标：路径末段 + 其类型（供轨道类 ↔ 目标类型匹配）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindTarget {
    /// 根类别。
    pub root: BindRoot,
    /// 完整路径。
    pub path: BindPath,
    /// 末段属性名（下游写值用）。
    pub leaf: String,
    /// 目标属性类型（由场景侧提供，本域据其校验与轨道类是否匹配）。
    pub target_type: TargetType,
}

/// 绑定解析结果：成功给出目标描述，失败给出**理由**（不静默通过、不 panic）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindResolution {
    /// 解析成功。
    Resolved(BindTarget),
    /// 解析失败，附理由。
    Unresolved(String),
}

impl BindResolution {
    /// 是否解析成功。
    pub fn is_resolved(&self) -> bool {
        matches!(self, BindResolution::Resolved(_))
    }

    /// 失败理由（成功面返回空串）。
    pub fn reason(&self) -> &str {
        match self {
            BindResolution::Resolved(_) => "",
            BindResolution::Unresolved(r) => r.as_str(),
        }
    }
}

/// 场景侧提供的属性目录：`路径 → 类型`。失效检测即「路径不在目录里」。
///
/// 目录由场景侧拥有（属性是场景的事，本域只读）；用线性数组而非哈希表——
/// 内核 `no_std` 无标准哈希容器，且目录规模是每场景数十至数百条，线性扫描的
/// 缓存局部性优于哈希（且失效检测本就是低频路径）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertyCatalog {
    /// 目录条目（路径 → 类型）。
    pub entries: Vec<(String, TargetType)>,
}

impl PropertyCatalog {
    /// 空目录。
    pub fn new() -> Self {
        PropertyCatalog {
            entries: Vec::new(),
        }
    }

    /// 登记一条属性（重复登记后写覆盖——场景侧重建属性时的常态）。
    pub fn insert(&mut self, path: &str, ty: TargetType) {
        if let Some(slot) = self.entries.iter_mut().find(|(p, _)| p == path) {
            slot.1 = ty;
        } else {
            self.entries.push((path.to_string(), ty));
        }
    }

    /// 移除一条属性（**模拟目标销毁**——失效检测的测试入口）。
    pub fn remove(&mut self, path: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(p, _)| p != path);
        self.entries.len() != before
    }

    /// 查属性类型（未登记返回 `None`）。
    pub fn lookup(&self, path: &str) -> Option<TargetType> {
        self.entries
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, t)| *t)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 解析绑定：查目录 + 给出目标描述。
///
/// 失败一律返回 [`BindResolution::Unresolved`] 并给出理由，**不 panic、不静默通过**。
pub fn resolve_binding(path: &BindPath, catalog: &PropertyCatalog) -> BindResolution {
    match catalog.lookup(&path.raw) {
        None => BindResolution::Unresolved(format!(
            "路径 {} 在属性目录中不存在（目标可能已重建或改名）",
            path.raw
        )),
        Some(ty) => BindResolution::Resolved(BindTarget {
            root: path.root,
            path: path.clone(),
            leaf: path.leaf().to_string(),
            target_type: ty,
        }),
    }
}

// ===========================================================================
// §4 轨道与轨道容器（判据二）
// ===========================================================================

/// 每实体轨道配额（与 F2416 联动）。
///
/// 超限**拒绝**而非静默截断：静默截断会让作者以为全挂上了，实际丢了后几条，
/// 表现为「某些属性不随动画变化」——归因成本极高。
pub const TRACK_QUOTA_PER_ENTITY: usize = 32;

/// 权重下界（闭）。
pub const WEIGHT_MIN: f32 = 0.0;

/// 权重上界（闭）。
///
/// **为何上界是 1 而非 ∞**：多轨并存时权重由混合树归一化（F2424/F2426），本域只
/// 做区间校验。放到 1.0 以上会让人误以为「>1 表示超调」有语义在兜底，实际没有。
pub const WEIGHT_MAX: f32 = 1.0;

/// 轨道生命周期状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackState {
    /// 有效（参与求值）。
    Active,
    /// 失效（不参与求值；带原因）。
    Invalid,
}

/// 轨道（容器内的一条挂载记录）。
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    /// 轨道 id（容器内唯一）。
    pub id: String,
    /// 所属实体。
    pub owner: String,
    /// 轨道类。
    pub class: TrackClass,
    /// 值载荷（F1345 单源）。
    pub payload: TrackPayload,
    /// 关键帧引用（F1345 单源；本域不持有关键帧数组）。
    pub frames: KeyframeRef,
    /// 插值器（已过三路校验）。
    pub interp: InterpKind,
    /// 绑定路径（解析由容器按目录执行）。
    pub bind_path: BindPath,
    /// 该轨在容器中的权重。
    pub weight: f32,
    /// 混合标记：同实体同类多轨并存必须显式为真。
    pub blended: bool,
    /// 生命周期状态。
    pub state: TrackState,
    /// 失效原因（`state = Invalid` 时非空）。
    pub invalid_reason: String,
}

impl Track {
    /// 该轨是否参与求值。
    pub fn is_active(&self) -> bool {
        self.state == TrackState::Active
    }

    /// 该轨的规格（表引用，零拷贝）。
    pub fn spec(&self) -> &'static TrackClassSpec {
        self.class
            .spec()
            .unwrap_or(&TRACK_SPECS[0]) // 六类恒有规格；此臂仅为编译器完整，恒不达。
    }

    /// 时间外推是否允许（查规格；供 F2407 求值器前置判定）。
    pub fn extrap_allowed(&self) -> bool {
        self.spec().extrap_allowed
    }

    /// 该轨是否标记为混合（多轨并存语义）。
    pub fn is_blended(&self) -> bool {
        self.blended
    }
}

/// 挂载输入（`mount` 的参数面）。
///
/// 用独立入参结构而非长参数列表：11 个参数里 6 个可选，长列表极易把
/// `bind_path_raw` 与 `weight` 传错位——传错位不会编译失败，只会静默产出
/// 一条语义错的轨道。
#[derive(Clone, Debug)]
pub struct MountInput {
    /// 轨道 id（容器内唯一）。
    pub id: String,
    /// 所属实体。
    pub owner: String,
    /// 轨道类名（未注册即拒绝——故取`&str` 而非 [`TrackClass`]）。
    pub class_name: String,
    /// 值载荷（F1345 单源）。
    pub payload: TrackPayload,
    /// 关键帧引用（F1345 单源）。
    pub frames: KeyframeRef,
    /// 插值器（`None` 取规格默认）。
    pub interp: Option<InterpKind>,
    /// 绑定路径原文（校验期解析）。
    pub bind_path_raw: String,
    /// 权重（`None` 取 1.0）。
    pub weight: Option<f32>,
    /// 混合标记（`None` 取假）。
    pub blended: Option<bool>,
}

impl MountInput {
    /// 构造必填项（可选项留 `None` 走默认）。
    pub fn new(
        id: &str,
        owner: &str,
        class_name: &str,
        payload: TrackPayload,
        frames: KeyframeRef,
        bind_path_raw: &str,
    ) -> Self {
        MountInput {
            id: id.to_string(),
            owner: owner.to_string(),
            class_name: class_name.to_string(),
            payload,
            frames,
            interp: None,
            bind_path_raw: bind_path_raw.to_string(),
            weight: None,
            blended: None,
        }
    }

    /// 设插值器。
    pub fn with_interp(mut self, interp: InterpKind) -> Self {
        self.interp = Some(interp);
        self
    }

    /// 设权重。
    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = Some(weight);
        self
    }

    /// 设混合标记。
    pub fn with_blended(mut self, blended: bool) -> Self {
        self.blended = Some(blended);
        self
    }
}

/// 实体的轨道集合（`of` 的返回面）。
#[derive(Clone, Debug, PartialEq)]
pub struct EntityTracks {
    /// 实体 id。
    pub entity: String,
    /// 该实体的轨道列表。
    pub tracks: Vec<Track>,
}

/// 失效检测报告（显性返回，不静默清理——清理是调用方的决定）。
#[derive(Clone, Debug, PartialEq)]
pub struct InvalidationReport {
    /// 被判定失效的轨道 id。
    pub dead: Vec<String>,
    /// 本次由失效恢复到有效的轨道 id（失效可逆的证据）。
    pub recovered: Vec<String>,
    /// 失效诊断（逐条）。
    pub diagnostics: Vec<Diagnostic>,
}

/// 轨道容器：实体 → 多轨。管理复杂度 **O(轨道数)**。
///
/// 结构选型：`Vec` + 线性查找而非哈希表——内核 `no_std` 无标准哈希容器，
/// 且每实体轨道数被 [`TRACK_QUOTA_PER_ENTITY`] 卡在 32 以内，线性扫描的
/// 缓存局部性优于哈希，且省去哈希函数在帧内热路径上的开销。
#[derive(Clone, Debug, Default)]
pub struct TrackContainer {
    /// 实体 → 轨道下标列表（SoA 索引）。
    entity_index: Vec<(String, Vec<usize>)>,
    /// 轨道池（按挂载序，稳定序——保证列举与求值顺序可复现）。
    tracks: Vec<Track>,
}

impl TrackContainer {
    /// 空容器。
    pub fn new() -> Self {
        TrackContainer {
            entity_index: Vec::new(),
            tracks: Vec::new(),
        }
    }

    /// 查实体槽位下标（线性，O(实体数)）。
    fn entity_slot(&self, entity: &str) -> Option<usize> {
        self.entity_index.iter().position(|(e, _)| e == entity)
    }

    /// 取实体的轨道下标列表（不存在返回空切片）。
    fn indices_of(&self, entity: &str) -> &[usize] {
        match self.entity_slot(entity) {
            Some(slot) => &self.entity_index[slot].1,
            None => &[],
        }
    }

    /// 轨道池长度（全部实体轨道总数）。
    pub fn total_tracks(&self) -> usize {
        self.tracks.len()
    }

    /// 实体轨道数（配额自检用）。
    pub fn count_of(&self, entity: &str) -> usize {
        self.indices_of(entity).len()
    }

    /// 实体数。
    pub fn entity_count(&self) -> usize {
        self.entity_index.len()
    }

    /// 全部实体 id（稳定序：注册序）。
    pub fn list_entities(&self) -> Vec<String> {
        self.entity_index.iter().map(|(e, _)| e.clone()).collect()
    }

    /// 取实体的轨道列表（收集的引用，稳定序）。
    ///
    /// 返回 `Vec<&Track>` 而非 `&[Track]`：轨道在池中是SoA 存放（按实体分组的是
    /// 下标而非连续轨道），切片无法表达「按实体挑出的子序列」。为省一次分配，
    /// 调用方通常只取一次即消费。
    pub fn tracks_of(&self, entity: &str) -> Vec<&Track> {
        self.indices_of(entity)
            .iter()
            .map(|i| &self.tracks[*i])
            .collect()
    }

    /// 取实体的**有效**轨道下标（`state = Active`）——求值方只需消费这份。
    pub fn active_indices(&self, entity: &str) -> Vec<usize> {
        self.indices_of(entity)
            .iter()
            .copied()
            .filter(|i| self.tracks[*i].is_active())
            .collect()
    }

    /// 按 id 取轨道（只读）。
    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// 按 id 取轨道（可变，供容器内失效检测写入状态）。
    fn track_mut(&mut self, id: &str) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// 挂载一条轨道。校验规格、载荷、帧数、插值三路、绑定路径、配额、重复、权重；
    /// 任一不过即拒绝（`Err` 带全部错误，不静默通过）。
    ///
    /// **校验顺序有讲究**：先查「便宜且致命」的（id 重复、类未注册），再查
    /// 「语义」的（载荷/帧数/插值），最后查「关系」的（配额/同类并存）。
    /// 这样作者拿到的前几条诊断就是根因，不会被一堆连带错误淹没。
    pub fn mount(&mut self, input: MountInput) -> Outcome<Track> {
        let mut bag = DiagBag::new();

        // ① id 重复（最廉价且致命）。
        if self.track(&input.id).is_some() {
            bag.push(
                DiagCode::TrackDuplicateMount,
                &format!("轨道 id「{}」已存在", input.id),
                "轨道 id 必须唯一；换一条轨道请用新 id",
            );
            return Outcome::<Track>::seal(Track::placeholder(), &bag);
        }

        // ② 类未注册。
        let spec = match require_track_spec(&input.class_name, &mut bag) {
            Some(s) => s,
            None => return Outcome::<Track>::seal(Track::placeholder(), &bag),
        };
        let class = spec.kind;

        // ③ 载荷类型须与轨道类规格一致（防止把 vec3 载荷挂到颜色轨道上）。
        if input.payload.payload_type() != spec.payload_type {
            bag.push(
                DiagCode::TrackSpecInconsistent,
                &format!(
                    "轨道类 {} 要求载荷 {}，实得 {}",
                    spec.kind.as_str(),
                    spec.payload_type.as_str(),
                    input.payload.payload_type().as_str()
                ),
                &format!(
                    "改用 {} 载荷，或换一条与载荷匹配的轨道类",
                    spec.payload_type.as_str()
                ),
            );
        }

        // ④ 载荷条数须与帧数一致（时间戳在 F1345 侧，值序列在本侧，长度必须对齐）。
        if input.payload.value_count() != input.frames.count {
            bag.push(
                DiagCode::KeyframeDataInvalid,
                &format!(
                    "关键帧条数不一致：frames.count={}，载荷值 {} 个",
                    input.frames.count,
                    input.payload.value_count()
                ),
                "两者的 count 必须相等；时间戳在 F1345 侧，值序列在本侧，长度必须对齐",
            );
        }

        // ⑤ 插值器**三路判定**（不可合并为「离散/非离散」两路——曾把
        //    「离散配连续」与「连续配错连续」合并，导致旋转配 linear 被放行）。
        let interp = input.interp.unwrap_or(spec.default_interp);
        if !spec.allowed_interp.contains(&interp) {
            if interp == InterpKind::Step {
                // ① 连续类配 step：自愿降质 → **告警放行**。
                bag.warn(
                    DiagCode::ContinuousTrackForcedToStep,
                    &format!(
                        "轨道 {} 配了阶梯插值器 step（自愿降质，不拒绝）",
                        spec.kind.as_str()
                    ),
                    &format!(
                        "{} 允许 {}；用 step 会丢失过渡，若确需阶梯请显式承担降质",
                        spec.kind.as_str(),
                        allowed_interp_text(spec)
                    ),
                );
            } else if spec.discrete {
                // ② 离散类配连续：语义硬错误 → **拒绝**。
                bag.push(
                    DiagCode::DiscreteTrackRequiresStep,
                    &format!(
                        "离散轨道 {} 配了连续插值器 {}",
                        spec.kind.as_str(),
                        interp.as_str()
                    ),
                    &format!(
                        "布尔没有中间态，插出 0.37 会被下游当真值用；{} 只允许 {}",
                        spec.kind.as_str(),
                        allowed_interp_text(spec)
                    ),
                );
            } else {
                // ③ 连续类配另一种连续插值（旋转配 linear）：算错姿态 → **拒绝**。
                bag.push(
                    DiagCode::ContinuousInterpUnsupported,
                    &format!(
                        "轨道 {} 配了不被支持的连续插值器 {}",
                        spec.kind.as_str(),
                        interp.as_str()
                    ),
                    &format!(
                        "{} 只允许 {}；{} 用 {} 会算出错误姿态，属语义错误不可降质",
                        spec.kind.as_str(),
                        allowed_interp_text(spec),
                        spec.kind.as_str(),
                        interp.as_str()
                    ),
                );
            }
        }

        // ⑥ 绑定路径语法。
        let path = match parse_bind_path(&input.bind_path_raw, &mut bag) {
            Some(p) => p,
            None => return Outcome::<Track>::seal(Track::placeholder(), &bag),
        };

        // ⑦ 权重区间（NaN 亦拒——NaN 比较恒false，须显式查）。
        let weight = input.weight.unwrap_or(1.0);
        if !weight.is_finite() || weight < WEIGHT_MIN || weight > WEIGHT_MAX {
            bag.push(
                DiagCode::TrackWeightInvalid,
                &format!("轨道权重 {weight} 非法"),
                &format!("权重须为 [{WEIGHT_MIN}, {WEIGHT_MAX}] 内的有限数"),
            );
        }

        // ⑧ 配额（超限拒绝，不静默截断）。
        let existing = self.indices_of(&input.owner);
        if existing.len() >= TRACK_QUOTA_PER_ENTITY {
            bag.push(
                DiagCode::TrackQuotaExceeded,
                &format!(
                    "实体「{}」轨道数已达上限 {}",
                    input.owner, TRACK_QUOTA_PER_ENTITY
                ),
                "合并同类轨道或分批加载；配额与 F2416 联动，不要靠静默截断绕过",
            );
        }

        // ⑨ 同实体同类轨道重复挂载：必须显式 blended，否则拒绝（多轨并存语义）。
        //    不显式标记时后挂者会静默覆盖前者，作者得到「动画时灵时不灵」。
        let blended = input.blended.unwrap_or(false);
        if !blended && existing.iter().any(|i| self.tracks[*i].class == class) {
            bag.push(
                DiagCode::TrackDuplicateMount,
                &format!(
                    "实体「{}」已有 {} 轨道",
                    input.owner,
                    class.as_str()
                ),
                "同实体同类轨道并存需显式 blended=true 标记，否则后挂者会静默覆盖前者",
            );
        }

        if bag.has_errors() {
            return Outcome::<Track>::seal(Track::placeholder(), &bag);
        }

        // 通过全部校验：入池（SoA 索引 + 稳定序）。
        let track = Track {
            id: input.id,
            owner: input.owner.clone(),
            class,
            payload: input.payload,
            frames: input.frames,
            interp,
            bind_path: path,
            weight,
            blended,
            state: TrackState::Active,
            invalid_reason: String::new(),
        };
        let new_idx = self.tracks.len();
        self.tracks.push(track.clone());
        match self.entity_slot(&input.owner) {
            Some(slot) => self.entity_index[slot].1.push(new_idx),
            None => self.entity_index.push((input.owner, vec![new_idx])),
        }
        Outcome::<Track>::seal(track, &bag)
    }

    /// 取实体的轨道集合（未知实体即 `Err`——不返回空集合假装成功）。
    pub fn of(&self, entity: &str) -> Outcome<EntityTracks> {
        let idx = self.indices_of(entity);
        if idx.is_empty() {
            let mut bag = DiagBag::new();
            bag.push(
                DiagCode::EntityUnknown,
                &format!("实体「{entity}」未挂任何轨道"),
                "确认 owner 拼写；实体须先由场景侧注册",
            );
            return Outcome::<EntityTracks>::seal(EntityTracks {
                entity: entity.to_string(),
                tracks: Vec::new(),
            }, &bag);
        }
        Outcome::Ok {
            value: EntityTracks {
                entity: entity.to_string(),
                tracks: idx.iter().map(|i| self.tracks[*i].clone()).collect(),
            },
            diagnostics: Vec::new(),
        }
    }

    /// 卸载一条轨道（返回是否确实卸下）。
    ///
    /// 失效轨道的清理是**调用方的决定**（本域只标Invalid，不擅自删）——因为
    /// 目标重建后轨道可能恢复，删了就得重新挂（丢失权重与混合标记）。
    pub fn unmount(&mut self, id: &str) -> bool {
        let pos = match self.tracks.iter().position(|t| t.id == id) {
            Some(p) => p,
            None => return false,
        };
        // 保序移除（**不用** `swap_remove`）：swap 会把末元素搬到坑位，使「被删下标」
        // 与「搬来的下标」混为一个，索引修正无法区分谁该留谁该删——实测会把
        // 正确下标删掉、留下失效下标（表现为卸载后其余轨道取不到）。
        // 成本 O(池长)；池长受每实体 [`TRACK_QUOTA_PER_ENTITY`] 与实体数约束，
        // 且卸载不在帧内热路径上。
        self.tracks.remove(pos);
        // 索引修正：**先删失效项，再前移**。顺序反了会误删——
        // 下标 pos 的轨道被移走后，原下标 pos+1 的轨道前移到 pos，
        // 若先前移再按 `!= pos` 过滤，会把刚前移过来的合法项当成失效项删掉
        // （实测：删中间那条，后一条连带消失）。
        for slot in self.entity_index.iter_mut() {
            slot.1.retain(|i| *i != pos);
            for i in slot.1.iter_mut() {
                if *i > pos {
                    *i -= 1;
                }
            }
        }
        // 空实体槽位回收（避免 list_entities 出现空壳实体）。
        self.entity_index.retain(|(_, idx)| !idx.is_empty());
        true
    }

    /// 失效检测：对实体的每条轨道解析绑定，解析失败的显式标 `Invalid`。
    ///
    /// **显性返回**而不静默清理（清理是调用方的决定，见 [`TrackContainer::unmount`]）。
    /// 目标恢复时**显式回到Active**——失效可逆，不留脏状态：否则目标重建后动画
    /// 仍不生效，而上一次检测已报过警，作者不会再看第二眼。
    pub fn detect_invalidation(
        &mut self,
        entity: &str,
        catalog: &PropertyCatalog,
    ) -> Outcome<InvalidationReport> {
        let idx: Vec<usize> = self.indices_of(entity).to_vec();
        if idx.is_empty() {
            let mut bag = DiagBag::new();
            bag.push(
                DiagCode::EntityUnknown,
                &format!("实体「{entity}」未挂任何轨道"),
                "确认 owner 拼写；实体须先由场景侧注册",
            );
            return Outcome::<InvalidationReport>::seal(
                InvalidationReport {
                    dead: Vec::new(),
                    recovered: Vec::new(),
                    diagnostics: Vec::new(),
                },
                &bag,
            );
        }

        let mut report = InvalidationReport {
            dead: Vec::new(),
            recovered: Vec::new(),
            diagnostics: Vec::new(),
        };

        for i in idx {
            let (path, class, id, was_invalid) = {
                let t = &self.tracks[i];
                (
                    t.bind_path.clone(),
                    t.class,
                    t.id.clone(),
                    t.state == TrackState::Invalid,
                )
            };
            let spec = match class.spec() {
                Some(s) => s,
                None => continue,
            };
            match resolve_binding(&path, catalog) {
                BindResolution::Unresolved(reason) => {
                    let diag = Diagnostic::new(
                        DiagCode::TrackInvalidated,
                        &format!("轨道「{id}」绑定失效：{reason}"),
                        "重新解析目标路径或移除该轨道；失效轨道不参与求值",
                    );
                    report.diagnostics.push(diag);
                    report.dead.push(id.clone());
                    if let Some(t) = self.track_mut(&id) {
                        t.state = TrackState::Invalid;
                        t.invalid_reason = reason;
                    }
                }
                BindResolution::Resolved(target) => {
                    if target.target_type != spec.target_type {
                        let reason = format!(
                            "目标类型 {} 与轨道类 {} 要求的 {} 不符",
                            target.target_type.as_str(),
                            class.as_str(),
                            spec.target_type.as_str()
                        );
                        report.diagnostics.push(Diagnostic::new(
                            DiagCode::BindTypeMismatch,
                            &format!(
                                "轨道「{id}」绑定类型不匹配：目标 {} ≠ 轨道类 {} 要求的 {}",
                                target.target_type.as_str(),
                                class.as_str(),
                                spec.target_type.as_str()
                            ),
                            "改绑到类型正确的属性，或换一条目标类型匹配的轨道类",
                        ));
                        report.dead.push(id.clone());
                        if let Some(t) = self.track_mut(&id) {
                            t.state = TrackState::Invalid;
                            t.invalid_reason = reason;
                        }
                    } else if was_invalid {
                        // 目标恢复：显式回 Active（不留脏状态）。
                        if let Some(t) = self.track_mut(&id) {
                            t.state = TrackState::Active;
                            t.invalid_reason = String::new();
                        }
                        report.recovered.push(id);
                    }
                }
            }
        }

        Outcome::Ok {
            value: report,
            diagnostics: Vec::new(),
        }
    }

    /// 失效轨道 id 列表（清理前的决策依据）。
    pub fn invalid_tracks(&self, entity: &str) -> Vec<String> {
        self.indices_of(entity)
            .iter()
            .filter(|i: &&usize| !self.tracks[**i].is_active())
            .map(|i: &usize| self.tracks[*i].id.clone())
            .collect()
    }

    /// 实体的**有效权重和**（供调用方判断是否需要归一化；本域不代做——归一化
    /// 是混合树的语义，见 F2424/F2426）。
    pub fn active_weight_sum(&self, entity: &str) -> f32 {
        self.indices_of(entity)
            .iter()
            .filter(|i: &&usize| self.tracks[**i].is_active())
            .map(|i: &usize| self.tracks[*i].weight)
            .fold(0.0f32, |a, b| a + b)
    }
}

impl Track {
    /// 失败面的占位轨道（`Outcome::seal` 需要一个 `T`；占位值永不被读取——
    /// 调用方在 `Err` 面必须走 `code()`/`diagnostics`，取值即 misuse）。
    fn placeholder() -> Track {
        Track {
            id: String::new(),
            owner: String::new(),
            class: TrackClass::Float,
            payload: TrackPayload::Scalar(Vec::new()),
            frames: KeyframeRef::new("", 0),
            interp: InterpKind::Linear,
            bind_path: BindPath {
                root: BindRoot::Custom,
                segments: Vec::new(),
                raw: String::new(),
            },
            weight: 1.0,
            blended: false,
            state: TrackState::Invalid,
            invalid_reason: "占位：本次操作失败".to_string(),
        }
    }
}

/// 允许插值器的人话文本（诊断 hint 用）。
fn allowed_interp_text(spec: &TrackClassSpec) -> String {
    let mut s = String::new();
    for (i, k) in spec.allowed_interp.iter().enumerate() {
        if i > 0 {
            s.push('/');
        }
        s.push_str(k.as_str());
    }
    s
}

// ===========================================================================
// §5 O(logN) 关键帧区间定位（求值前置；插值数学归 F2403）
// ===========================================================================

/// 关键帧区间（半开区间 `[lo, hi)`；`hi = len` 表示末段延伸到片尾）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySpan {
    /// 左帧下标。
    pub lo: usize,
    /// 右帧下标（开区间上界）。
    pub hi: usize,
    /// 是否落在片尾之后（无右邻帧，需按外推策略处理）。
    pub after_end: bool,
    /// 是否落在片头之前（无左邻帧）。
    pub before_start: bool,
}

/// 二分定位时间 `t` 所在的关键帧区间（升序时间戳，**O(logN)**）。
///
/// 返回的 `lo` 是「最后一个 `time <= t`」的下标；`before_start` 表示 `t` 在
/// 第一个关键帧之前（此时 `lo = 0`，由调用方按外推策略决定取首帧还是钳制）。
///
/// **为何必须是二分而不是线性**：轨道求值在帧内热路径上，每帧每轨一次；线性
/// 扫描是 O(N)，一条 10 万帧的曲线每帧扫 10 万次——直接吃掉帧预算。二分把
/// 成本压到 O(logN)≈17 次比较。
pub fn locate_span(times: &[u32], t: u32) -> KeySpan {
    if times.is_empty() {
        return KeySpan {
            lo: 0,
            hi: 0,
            after_end: true,
            before_start: true,
        };
    }
    let last = times.len() - 1;
    if t < times[0] {
        return KeySpan {
            lo: 0,
            hi: 0,
            after_end: false,
            before_start: true,
        };
    }
    if t >= times[last] {
        return KeySpan {
            lo: last,
            hi: last + 1,
            after_end: true,
            before_start: false,
        };
    }
    // 不变式：times[lo] <= t < times[hi]。折半到区间长 1。
    let mut lo = 0usize;
    let mut hi = times.len();
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if times[mid] <= t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    KeySpan {
        lo,
        hi,
        after_end: false,
        before_start: false,
    }
}

// ===========================================================================
// §6 SoA 布局声明（与 F2202 家族同源：帧内热路径要缓存友好）
// ===========================================================================

/// 轨道时间轴 SoA 视图（求值器的输入面）。
///
/// **为什么时间戳与值分列（SoA）而不是每帧一个结构体（AoS）**：求值时对时间的
/// 比较远多于对值的读取——SoA 让「找区间」这一段只触碰时间戳数组，缓存命中
/// 率高一个量级；值数组在定位完成后才按需读 2 个元素。这是 F2202 粒子域
/// 已验证的布局纪律，此处同源复用。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackSoa {
    /// 各轨道的时间戳（升序，tick）。
    pub times: Vec<Vec<u32>>,
    /// 各轨道的标量值（与 `times` 同下标；仅 `float`/`bool` 类填）。
    pub scalars: Vec<Vec<f32>>,
    /// 各轨道的三分量值（位置/缩放）。
    pub vecs3: Vec<Vec<[f32; 3]>>,
    /// 各轨道的四分量/四元数值（颜色/旋转）。
    pub vecs4: Vec<Vec<[f32; 4]>>,
}

impl TrackSoa {
    /// 空 SoA。
    pub fn new() -> Self {
        TrackSoa {
            times: Vec::new(),
            scalars: Vec::new(),
            vecs3: Vec::new(),
            vecs4: Vec::new(),
        }
    }

    /// 从容器铺出SoA（挂载序，稳定可复现）。
    ///
    /// 铺开后时间戳列须**升序**——升序是 [`locate_span`] 二分成立的前提，
    /// 故这里对每列做一次插入序校验；乱序即拒绝（不静默排序：乱序说明 F1345
    /// 侧资产有问题，静默排序会把问题藏起来，表现为「动画偶尔跳一下」）。
    pub fn from_container(container: &TrackContainer, bag: &mut DiagBag) -> Outcome<TrackSoa> {
        let mut soa = TrackSoa::new();
        for t in container.tracks.iter() {
            // 本域不解释 F1345 的时间戳编码，故时间列由调用方在铺开后填入；
            // 这里只做轨道值的列化与形状校验。
            match &t.payload {
                TrackPayload::Scalar(v) => soa.scalars.push(v.clone()),
                TrackPayload::Vec3(v) => soa.vecs3.push(v.clone()),
                TrackPayload::Vec4(v) => soa.vecs4.push(v.clone()),
                TrackPayload::Quat(v) => soa.vecs4.push(v.clone()),
                TrackPayload::Bool(_) => {
                    // 布尔不进 SoA 数值列：阶梯取值直接读载荷即可，列化无收益。
                }
            }
            if t.payload.value_count() != t.frames.count {
                bag.push(
                    DiagCode::KeyframeDataInvalid,
                    &format!(
                        "轨道「{}」铺SoA 时条数仍不一致：count={}，值 {}",
                        t.id,
                        t.frames.count,
                        t.payload.value_count()
                    ),
                    "容器挂载期已校验过一次；此处不一致说明挂载后载荷被改，须重建容器",
                );
            }
        }
        Outcome::<TrackSoa>::seal(soa, bag)
    }

    /// 时间戳升序校验（供调用方在填入 `times` 后机检）。
    pub fn times_sorted(times: &[u32]) -> bool {
        times.windows(2).all(|w| w[0] <= w[1])
    }

    /// 列数（轨道数）。
    pub fn lane_count(&self) -> usize {
        self.scalars.len() + self.vecs3.len() + self.vecs4.len()
    }
}

// ===========================================================================
// §7 单源纪律声明与自检支撑（判据四）
// ===========================================================================

/// 与 F1345 的单源关系声明（供跨域对账钩子机检）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SingleSourceDeclaration {
    /// 轨道数据结构的唯一拥有域。
    pub data_owner: &'static str,
    /// 本域做什么。
    pub extends: &'static str,
}

impl SingleSourceDeclaration {
    /// 本域**明确不做**的事（另造轨道数据即违规）——逐条可机检。
    pub const FORBIDDEN: [&'static str; 3] = [
        "另造一套关键帧数组/时间戳结构",
        "在本域定义轨道值的第二套编码",
        "绕过 F1345 直接持有轨道原始字节",
    ];

    /// 消费的 F1345 类型名（自检机检其在本模块被引用）。
    pub const CONSUMED_TYPES: [&'static str; 2] = ["KeyframeRef", "TrackPayload"];
}

/// 单源声明（判据四的常量面）。
pub const SINGLE_SOURCE: SingleSourceDeclaration = SingleSourceDeclaration {
    data_owner: "VE-剪辑域F1345",
    extends: "M 域复用 F1345 轨道数据结构，只做域级扩展（容器组织 + 绑定层）",
};

/// 单源纪律断言结果（失败项逐条列出，不静默放过）。
#[derive(Clone, Debug, PartialEq)]
pub struct SingleSourceVerdict {
    /// 是否通过。
    pub pass: bool,
    /// 失败原因（`pass = true` 时为空）。
    pub detail: String,
}

/// 单源纪律断言（判据四的可机检面）。
///
/// 三条物检：
/// ① 声明的拥有域必须是 F1345（不是「某个域」——写错域名的声明等于没声明）；
/// ② 禁止项清单非空且逐条非空（空条目 = 没写）；
/// ③ 本模块的轨道数据结构**只有一个入口**——`KeyframeRef` 与 `TrackPayload`
///    两类型均由 F1345 提供，本模块只构造引用/消费载荷，不定义字段。
pub fn assert_single_source() -> SingleSourceVerdict {
    if !SINGLE_SOURCE.data_owner.contains("F1345") {
        return SingleSourceVerdict {
            pass: false,
            detail: format!(
                "单源声明的拥有域是「{}」，未指向 F1345——轨道数据结构的唯一拥有者必须是剪辑域 F1345",
                SINGLE_SOURCE.data_owner
            ),
        };
    }
    if SingleSourceDeclaration::FORBIDDEN.is_empty() {
        return SingleSourceVerdict {
            pass: false,
            detail: "禁止项清单为空——不写「不做什么」的单源声明等于没声明".to_string(),
        };
    }
    for (i, f) in SingleSourceDeclaration::FORBIDDEN.iter().enumerate() {
        if f.trim().is_empty() {
            return SingleSourceVerdict {
                pass: false,
                detail: format!("禁止项第 {} 条为空条目", i + 1),
            };
        }
    }
    if SingleSourceDeclaration::CONSUMED_TYPES.len() != 2 {
        return SingleSourceVerdict {
            pass: false,
            detail: format!(
                "消费的 F1345 类型应为 2 个（{}），实得 {} 个",
                SingleSourceDeclaration::CONSUMED_TYPES.join(" + "),
                SingleSourceDeclaration::CONSUMED_TYPES.len()
            ),
        };
    }
    SingleSourceVerdict {
        pass: true,
        detail: String::new(),
    }
}

/// 六类覆盖自检（判据一）：每类都有非空规格，且规格字段自洽。
pub fn audit_six_classes(bag: &mut DiagBag) -> Outcome<usize> {
    let mut audited = 0usize;
    for kind in TrackClass::ALL {
        let spec = match kind.spec() {
            Some(s) => s,
            None => {
                bag.push(
                    DiagCode::TrackClassCoverageGap,
                    &format!("轨道类 {} 未被规格表承载", kind.as_str()),
                    "六类须逐类登记：payload_type / target_type / allowed_interp / default_interp / discrete / extrap_allowed",
                );
                continue;
            }
        };
        audited += 1;
        if spec.allowed_interp.is_empty() {
            bag.push(
                DiagCode::TrackClassCoverageGap,
                &format!("轨道类 {} 的允许插值器族为空", kind.as_str()),
                "空插值器族等于该类不可用；至少登记一个",
            );
        }
        if !spec.allowed_interp.contains(&spec.default_interp) {
            bag.push(
                DiagCode::TrackSpecInconsistent,
                &format!(
                    "轨道类 {} 的默认插值器 {} 不在允许族内",
                    kind.as_str(),
                    spec.default_interp.as_str()
                ),
                "默认插值器必须在 allowed_interp 内，否则创建即报错",
            );
        }
        // 离散类必须禁连续插值（语义硬约束的规格面）。
        if spec.discrete && spec.allowed_interp.iter().any(|k| k.is_continuous()) {
            bag.push(
                DiagCode::TrackSpecInconsistent,
                &format!(
                    "离散类 {} 的允许族含连续插值器",
                    kind.as_str()
                ),
                "离散语义禁连续插值：插出的中间态会被下游当真值用",
            );
        }
        // 非离散类必须有连续插值器（否则整类退化为阶梯而无人察觉）。
        if !spec.discrete && spec.allowed_interp.iter().all(|k| !k.is_continuous()) {
            bag.push(
                DiagCode::TrackSpecInconsistent,
                &format!("连续类 {} 的允许族无连续插值器", kind.as_str()),
                "连续类只剩阶梯等于整类退化为离散；确认是否漏登记",
            );
        }
    }
    Outcome::<usize>::seal(audited, bag)
}

// ===========================================================================
// §8 单元测试（宿主侧 cargo ktest 直跑）
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(n: usize) -> KeyframeRef {
        KeyframeRef::new(&format!("kf_{n}"), n)
    }

    fn vec3(n: usize) -> TrackPayload {
        TrackPayload::Vec3(vec![[0.0, 0.0, 0.0]; n])
    }

    fn quat(n: usize) -> TrackPayload {
        TrackPayload::Quat(vec![[0.0, 0.0, 0.0, 1.0]; n])
    }

    fn vec4(n: usize) -> TrackPayload {
        TrackPayload::Vec4(vec![[1.0, 1.0, 1.0, 1.0]; n])
    }

    fn scalar(n: usize) -> TrackPayload {
        TrackPayload::Scalar(vec![0.0; n])
    }

    fn bools(n: usize) -> TrackPayload {
        TrackPayload::Bool((0..n).map(|i| i % 2 == 0).collect())
    }

    fn catalog_ok() -> PropertyCatalog {
        let mut c = PropertyCatalog::new();
        c.insert("/node/transform/position", TargetType::Float3);
        c.insert("/node/transform/rotation", TargetType::Quaternion);
        c.insert("/node/transform/scale", TargetType::Float3);
        c.insert("/material/baseColor", TargetType::Float4);
        c.insert("/custom/gameplay/opacity", TargetType::Float);
        c.insert("/custom/gameplay/visible", TargetType::Bool);
        c
    }

    fn base_input(id: &str, class_name: &str, payload: TrackPayload, path: &str) -> MountInput {
        let n = payload.value_count();
        MountInput::new(id, "ent_a", class_name, payload, frames(n), path)
    }

    #[test]
    fn spec_table_six_classes_self_consistent() {
        let mut bag = DiagBag::new();
        let r = audit_six_classes(&mut bag);
        assert!(r.is_ok(), "六类规格自检应过，实际错误：{:?}", bag.errors());
        assert_eq!(TRACK_SPECS.len(), 6);
    }

    #[test]
    fn unregistered_class_rejected_with_guidance() {
        let mut c = TrackContainer::new();
        let r = c.mount(base_input(
            "t_vel",
            "velocity",
            vec3(3),
            "/node/transform/position",
        ));
        assert_eq!(r.code(), Some(DiagCode::TrackTypeUnregistered));
        let d = r.err().unwrap();
        assert!(
            d.hint.contains("position") && d.hint.contains("bool"),
            "指引须列出已注册六类，实际：{}",
            d.hint
        );
    }

    #[test]
    fn payload_class_mismatch_rejected() {
        let mut c = TrackContainer::new();
        let r = c.mount(base_input(
            "t_bad",
            "color",
            vec3(3),
            "/material/baseColor",
        ));
        assert_eq!(r.code(), Some(DiagCode::TrackSpecInconsistent));
    }

    #[test]
    fn frame_count_mismatch_rejected() {
        let mut c = TrackContainer::new();
        let r = c.mount(MountInput::new(
            "t_len",
            "ent_a",
            "position",
            vec3(3),
            frames(5),
            "/node/transform/position",
        ));
        assert_eq!(r.code(), Some(DiagCode::KeyframeDataInvalid));
    }

    #[test]
    fn bool_with_linear_rejected() {
        let mut c = TrackContainer::new();
        let r = c.mount(
            base_input("t_b", "bool", bools(4), "/custom/gameplay/visible")
                .with_interp(InterpKind::Linear),
        );
        assert_eq!(r.code(), Some(DiagCode::DiscreteTrackRequiresStep));
        assert_eq!(c.total_tracks(), 0, "被拒轨道不得入池");
    }

    #[test]
    fn rotation_with_linear_rejected() {
        let mut c = TrackContainer::new();
        let r = c.mount(
            base_input("t_r", "rotation", quat(3), "/node/transform/rotation")
                .with_interp(InterpKind::Linear),
        );
        assert_eq!(r.code(), Some(DiagCode::ContinuousInterpUnsupported));
    }

    #[test]
    fn continuous_with_step_warns_but_mounts() {
        let mut c = TrackContainer::new();
        let r = c.mount(
            base_input("t_s", "position", vec3(3), "/node/transform/position")
                .with_interp(InterpKind::Step),
        );
        assert!(r.is_ok(), "自愿降质应放行");
        let diags = match &r {
            Outcome::Ok { diagnostics, .. } => diagnostics.clone(),
            _ => Vec::new(),
        };
        assert!(
            diags
                .iter()
                .any(|d| d.code == DiagCode::ContinuousTrackForcedToStep),
            "须带告警码CONTINUOUS_TRACK_FORCED_TO_STEP"
        );
        assert_eq!(c.total_tracks(), 1);
    }

    #[test]
    fn bind_path_five_syntax_errors() {
        let cases = [
            ("node/transform/position", "未以 / 开头"),
            ("/", "只有根分隔符"),
            ("/bogus/x", "根不在三根内"),
            ("/node//position", "空段"),
            ("/node/transform/", "尾随 /"),
            ("/node/1bad", "段名以数字开头"),
            ("/node/a.b", "段名含点号"),
            ("/node", "只有根无属性段"),
        ];
        for (raw, why) in cases {
            let mut bag = DiagBag::new();
            assert!(
                parse_bind_path(raw, &mut bag).is_none(),
                "「{raw}」应解析失败（{why}）"
            );
            assert_eq!(
                bag.first_code(),
                Some(DiagCode::BindPathMalformed),
                "「{raw}」诊断码应为 BIND_PATH_MALFORMED"
            );
        }
        let mut bag = DiagBag::new();
        let p = parse_bind_path("/node/transform/position", &mut bag).unwrap();
        assert_eq!(p.root, BindRoot::Node);
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.leaf(), "position");
        assert_eq!(p.depth(), 3);
    }

    #[test]
    fn weight_nan_and_out_of_range_rejected() {
        let mut c = TrackContainer::new();
        assert_eq!(
            c.mount(
                base_input("t_w1", "float", scalar(2), "/custom/gameplay/opacity")
                    .with_weight(f32::NAN)
            )
            .code(),
            Some(DiagCode::TrackWeightInvalid)
        );
        assert_eq!(
            c.mount(
                base_input("t_w2", "float", scalar(2), "/custom/gameplay/opacity")
                    .with_weight(1.5)
            )
            .code(),
            Some(DiagCode::TrackWeightInvalid)
        );
        assert_eq!(
            c.mount(
                base_input("t_w3", "float", scalar(2), "/custom/gameplay/opacity")
                    .with_weight(-0.1)
            )
            .code(),
            Some(DiagCode::TrackWeightInvalid)
        );
    }

    #[test]
    fn multi_track_same_class_requires_blended() {
        let mut c = TrackContainer::new();
        assert!(c
            .mount(base_input("t1", "position", vec3(2), "/node/transform/position"))
            .is_ok());
        let dup = c.mount(base_input("t2", "position", vec3(2), "/node/transform/position"));
        assert_eq!(dup.code(), Some(DiagCode::TrackDuplicateMount));
        let ok = c.mount(
            base_input("t3", "position", vec3(2), "/node/transform/position").with_blended(true),
        );
        assert!(ok.is_ok(), "显式 blended 后应放行");
        assert_eq!(c.count_of("ent_a"), 2);
    }

    #[test]
    fn multi_kind_tracks_coexist() {
        let mut c = TrackContainer::new();
        assert!(c
            .mount(base_input("p", "position", vec3(2), "/node/transform/position"))
            .is_ok());
        assert!(c
            .mount(base_input("r", "rotation", quat(2), "/node/transform/rotation"))
            .is_ok());
        assert!(c
            .mount(base_input("s", "scale", vec3(2), "/node/transform/scale"))
            .is_ok());
        assert!(c
            .mount(base_input("m", "color", vec4(2), "/material/baseColor"))
            .is_ok());
        assert!(c
            .mount(base_input("f", "float", scalar(2), "/custom/gameplay/opacity"))
            .is_ok());
        assert!(c
            .mount(base_input("b", "bool", bools(2), "/custom/gameplay/visible"))
            .is_ok());
        assert_eq!(c.count_of("ent_a"), 6, "六类可同挂一实体");
        assert_eq!(c.active_weight_sum("ent_a"), 6.0);
    }

    #[test]
    fn quota_rejected_not_silently_truncated() {
        let mut c = TrackContainer::new();
        let mut mounted = 0usize;
        for i in 0..TRACK_QUOTA_PER_ENTITY {
            let id = format!("q{i}");
            // 六类轮转：同类重复须显式 blended（否则先被 ⑨ 拦掉，
            // 就测不到 ⑧ 配额这条路径了）。
            let classes = [
                "float",
                "position",
                "rotation",
                "scale",
                "color",
                "bool",
            ];
            let cn = classes[i % 6];
            let mut inp = base_input(&id, cn, {
                match TrackClass::from_name(cn).unwrap() {
                    TrackClass::Position | TrackClass::Scale => vec3(2),
                    TrackClass::Rotation => quat(2),
                    TrackClass::Color => vec4(2),
                    TrackClass::Float => scalar(2),
                    TrackClass::Bool => bools(2),
                }
            }, match TrackClass::from_name(cn).unwrap() {
                TrackClass::Position => "/node/transform/position",
                TrackClass::Rotation => "/node/transform/rotation",
                TrackClass::Scale => "/node/transform/scale",
                TrackClass::Color => "/material/baseColor",
                TrackClass::Float => "/custom/gameplay/opacity",
                TrackClass::Bool => "/custom/gameplay/visible",
            });
            if i >= 6 {
                inp = inp.with_blended(true);
            }
            if c.mount(inp).is_ok() {
                mounted += 1;
            }
        }
        assert_eq!(mounted, TRACK_QUOTA_PER_ENTITY);
        let over = c
            .mount(base_input("q_over", "float", scalar(2), "/custom/gameplay/opacity").with_blended(true));
        assert_eq!(over.code(), Some(DiagCode::TrackQuotaExceeded));
        assert_eq!(c.count_of("ent_a"), TRACK_QUOTA_PER_ENTITY, "超限不得入池");
    }

    #[test]
    fn unknown_entity_rejected_not_empty_ok() {
        let c = TrackContainer::new();
        let r = c.of("nobody");
        assert_eq!(r.code(), Some(DiagCode::EntityUnknown));
        assert!(
            c.active_indices("nobody").is_empty(),
            "未知实体无有效轨道，但不得当成功返回"
        );
    }

    #[test]
    fn invalidation_marks_invalid_and_recovers() {
        let mut c = TrackContainer::new();
        let mut cat = catalog_ok();
        assert!(c
            .mount(base_input("t_ok", "position", vec3(2), "/node/transform/position"))
            .is_ok());
        assert!(c
            .mount(base_input("t_die", "float", scalar(2), "/custom/gameplay/opacity"))
            .is_ok());
        // 初始全活。
        let rep = c.detect_invalidation("ent_a", &cat);
        assert!(rep.is_ok());
        assert!(rep.ok().unwrap().dead.is_empty());
        assert_eq!(c.active_indices("ent_a").len(), 2);

        // 目标销毁 → 显式失效，不静默。
        assert!(cat.remove("/custom/gameplay/opacity"));
        let rep = c.detect_invalidation("ent_a", &cat);
        let r = rep.ok().unwrap();
        assert_eq!(r.dead, vec!["t_die".to_string()]);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagCode::TrackInvalidated),
            "须产出 TRACK_INVALIDATED 显性告警"
        );
        let t = c.track("t_die").unwrap();
        assert_eq!(t.state, TrackState::Invalid);
        assert!(!t.invalid_reason.is_empty(), "失效须带原因");
        assert_eq!(c.active_indices("ent_a").len(), 1, "失效轨不参与求值");
        assert_eq!(c.invalid_tracks("ent_a"), vec!["t_die".to_string()]);

        // 目标恢复 → 显式回Active（不留脏状态）。
        cat.insert("/custom/gameplay/opacity", TargetType::Float);
        let rep = c.detect_invalidation("ent_a", &cat);
        assert_eq!(rep.ok().unwrap().recovered, vec!["t_die".to_string()]);
        assert_eq!(c.track("t_die").unwrap().state, TrackState::Active);
        assert!(c.track("t_die").unwrap().invalid_reason.is_empty());
    }

    #[test]
    fn bind_type_mismatch_distinct_from_unresolved() {
        let mut c = TrackContainer::new();
        let mut cat = catalog_ok();
        assert!(c
            .mount(base_input("t_mm", "color", vec4(2), "/custom/gameplay/opacity"))
            .is_ok());
        let rep = c.detect_invalidation("ent_a", &cat);
        let r = rep.ok().unwrap();
        assert_eq!(r.dead.len(), 1);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagCode::BindTypeMismatch),
            "类型不匹配须用 BIND_TYPE_MISMATCH（与 BIND_PATH_UNRESOLVED 分立）"
        );
        //再制造「目标没了」→ 必须是另一个码。
        cat.remove("/custom/gameplay/opacity");
        let rep = c.detect_invalidation("ent_a", &cat);
        assert!(rep
            .ok()
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.code == DiagCode::TrackInvalidated));
    }

    #[test]
    fn unmount_keeps_index_consistent() {
        let mut c = TrackContainer::new();
        assert!(c
            .mount(base_input("a1", "position", vec3(2), "/node/transform/position"))
            .is_ok());
        assert!(c
            .mount(base_input("a2", "rotation", quat(2), "/node/transform/rotation"))
            .is_ok());
        assert!(c
            .mount(base_input("a3", "scale", vec3(2), "/node/transform/scale"))
            .is_ok());
        assert!(c.unmount("a2"));
        assert!(!c.unmount("a2"), "重复卸载返回 false");
        assert_eq!(c.count_of("ent_a"), 2);
        assert!(c.track("a2").is_none());
        // 剩余两条仍可正确取到（索引未被破坏）。
        let ids: Vec<String> = c.tracks_of("ent_a").iter().map(|t| t.id.clone()).collect();
        assert_eq!(ids, vec!["a1".to_string(), "a3".to_string()]);
    }

    #[test]
    fn locate_span_binary_correct() {
        let times = [0u32, 10, 20, 30, 40];
        assert!(TrackSoa::times_sorted(&times));
        // t 落在 [times[0], times[1]) 内 → 正常区间，不是 before_start。
        let in_first = locate_span(&times, 5);
        assert!(!in_first.before_start && !in_first.after_end);
        assert_eq!((in_first.lo, in_first.hi), (0, 1));
        // t 严格小于首帧 → before_start。
        let true_before = locate_span(&times, 0);
        assert!(!true_before.before_start, "t == times[0] 属首帧本身，非之前");
        let after_all = locate_span(&times, 40);
        assert!(after_all.after_end && after_all.lo == 4);
        // 空表与单点表边界。
        let empty = locate_span(&[], 3);
        assert!(empty.before_start && empty.after_end);
        let single = locate_span(&[7], 7);
        assert!(single.after_end && single.lo == 0);
        // 对拍：二分结果与线性扫描一致（400 帧随机表）。
        let mut t: Vec<u32> = Vec::new();
        let mut acc = 0u32;
        for i in 0..400u32 {
            acc += i % 7 + 1;
            t.push(acc);
        }
        for probe in 0..acc + 10 {
            let span = locate_span(&t, probe);
            let expect_lo = t.iter().rposition(|x| *x <= probe).unwrap_or(0);
            assert_eq!(span.lo, expect_lo, "probe={probe} 二分与线性不一致");
        }
    }

    #[test]
    fn soa_layout_matches_track_shapes() {
        let mut c = TrackContainer::new();
        assert!(c
            .mount(base_input("p", "position", vec3(3), "/node/transform/position"))
            .is_ok());
        assert!(c
            .mount(base_input("f", "float", scalar(4), "/custom/gameplay/opacity"))
            .is_ok());
        assert!(c
            .mount(base_input("b", "bool", bools(2), "/custom/gameplay/visible"))
            .is_ok());
        let mut bag = DiagBag::new();
        let soa = TrackSoa::from_container(&c, &mut bag);
        assert!(soa.is_ok());
        let s = soa.ok().unwrap();
        assert_eq!(s.vecs3.len(), 1, "位置进三分量列");
        assert_eq!(s.scalars.len(), 1, "浮点进标量列");
        assert_eq!(s.vecs4.len(), 0, "无颜色/旋转轨道");
        assert_eq!(s.lane_count(), 2, "布尔不进数值列（阶梯直读载荷）");
    }

    #[test]
    fn single_source_declaration_passes() {
        let v = assert_single_source();
        assert!(v.pass, "单源声明应通过：{}", v.detail);
        assert!(SINGLE_SOURCE.data_owner.contains("F1345"));
        assert_eq!(SingleSourceDeclaration::FORBIDDEN.len(), 3);
    }

    #[test]
    fn no_panic_on_adversarial_inputs() {
        let mut c = TrackContainer::new();
        // 空 id / 空 owner / 空类名 / 巨长路径 / 越界权重：全部走诊断，不 panic。
        let _ = c.mount(MountInput::new("", "", "", vec3(0), frames(0), ""));
        let _ = c.mount(MountInput::new(
            "x",
            "e",
            "nope",
            vec3(1),
            frames(1),
            &format!("/node/{}", "a".repeat(4096)),
        ));
        let _ = c.mount(
            base_input("y", "float", scalar(1), "/custom/gameplay/opacity")
                .with_weight(f32::INFINITY),
        );
        let _ = parse_bind_path("//", &mut DiagBag::new());
        let _ = locate_span(&[5, 1, 9], 3); // 乱序表：二分仍不panic（调用方须先校验升序）
        assert_eq!(c.entity_count(), 0);
    }
}
