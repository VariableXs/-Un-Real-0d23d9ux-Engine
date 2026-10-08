//! VE-F3001 · P 域开工与动效库总架构（VE-P 域 · 动效与体验库域 · P01 组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3001`
//!
//! **判据（锚点原文）**：三组架构、十项映射、单源分工、三底线、第一红线、判据。
//!
//! **职责定位（锚点原文）**：P 域开工与动效库总架构——P 域使命声明（P=动效与体验
//! 库域：把 N 域的骨架变成有生命感的界面——动效库/页面转场/微交互三组 + 域级收口
//! 四组架构）；官方主题动效范围映射；与 M/O04 关系（P 是消费层：动效实例最终由
//! M 域运行时执行（F2861 契约复用）——P 不另造动画运行时，只造声明与编排层——
//! 单源分工声明）；验收观（动效验收=帧帧可放大看 + 打断必处理 + 无障碍必覆盖
//! 三底线）。
//!
//! **数据结构（锚点原文·家族格式）**：三组架构接口（库/转场/微交互段间接口冻结
//! v1）；十项映射；与 M/O04 消费契约。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：运行时能力分歧（M 域能力假设
//! 错误）→ 对拍拦截；组间接口变更→ ADR；降级链（F3038 前向声明）。
//!
//! **性能逐项分解（锚点原文·家族格式）**：架构声明零运行时开销；映射 O(1) 每主题；
//! 契约核验 O(签名数)。
//!
//! **跨批对接点（锚点原文·家族格式）**：M/O04 契约接收；N 域控件集成（动效绑定
//! 目标）；F3038 降级链前向。
//!
//! **无障碍与隐私（锚点原文）**：P 域总红线（一切动效默认受 reduced-motion 覆盖
//! ——新动效无障碍豁免必须白名单理由：**覆盖是默认，动效是例外**——本域第一红线）；
//! 无隐私面。
//!
//! # 锚点计数歧义与本项的裁决（不静默丢项）
//!
//! 锚点写「官方主题动效范围映射（动效语言/令牌/转场编排/组件化/入场退场/共享元素/
//! FLIP/手势驱动/滚动驱动/微反馈/物理引擎/性能/无障碍/调试/测试——**十项**映射表）」：
//! 括号内枚举了**十五**个标签，而判据说「十项映射」。本项**不把多出的五项悄悄
//! 丢掉**，也不把十五项硬说成十项，而是按语义劈成两轴并把裁决写在代码里：
//!
//! - [`MotionTheme`] **十项**（判据「十项映射」的正式十项）：动效语言、令牌、
//!   转场编排、组件化、入场退场、共享元素、FLIP、手势驱动、滚动驱动、微反馈——
//!   这十项都是**面向使用者的动效能力**，每一项都有对外可检索的落点；
//! - [`SupportAxis`] **五轴**（物理引擎、性能、无障碍、调试、测试）——这五项不是
//!   独立的动效能力，而是**横切上述十项的支撑轴**：物理引擎给十项提供求解，
//!   性能给十项定预算，无障碍给十项定覆盖，调试给十项提供工作台，测试给十项提供
//!   断言。把它们算成「动效能力」会重复登记（物理引擎不是一种动效，是所有动效的
//!   求解器），所以单列为支撑轴，且**每轴必须声明它服务哪几个十项**——轴若不服务
//!   任何主题就是空轴，由 [`MotionArchitecture::check_axes_served`] 拦。
//!
//! 十 + 五 = 十五，与锚点括号内枚举**逐项对齐、无一遗漏**（见
//! [`check_anchor_label_coverage`] 的机检断言）。
//!
//! # 三组 vs 四组（架构层数与接口组数的区分）
//!
//! 锚点职责定位写「动效库/页面转场/微交互**三组** + 域级收口**四组**架构」，而
//! 数据结构写「**三组**架构接口（库/转场/微交互段间接口冻结 v1）」。本项按
//! 数据结构执行：三组承载**段间接口**（[`MotionGroup`]），域级收口是**闸门**
//! （[`CloseoutGate`]）而不是接口组——收口不产出可被下游消费的段间契约，它只对
//! 三组做双签与硬门裁决。把它算成第四个接口组会让「三组接口冻结 v1」变成四份
//! 签名，判据「三组架构」也就无法逐条断言了。
//!
//! # 本项的边界（不越界施工，遵守"只做领到的任务"）
//!
//! VE-F3001 是**域开工与总架构**——它交付：三组段间接口契约与冻结版本、M/O04/N/F3038
//! 四端消费契约与哈希对账钩子、十项主题落点映射与五轴支撑归属、三底线的可执行
//! 判定、第一红线（reduced-motion 默认覆盖 + 豁免白名单）的判定器、降级链的**前向
//! 声明**位。它**不代做**后续 19 项的引擎本体。分工在册（见
//! [`DOWNSTREAM_OWNERSHIP`]）：
//!
//! - F3002 动效设计语言总纲拥有**四原则与缓动家族全表**；本项只立
//!   [`MotionGroup::MotionLibrary`] 的段契约与"语言段先于令牌段"的序约束，
//!   并在 [`Segment`] 里留出取值位，不写任何缓动曲线；
//! - F3003 动效令牌体系拥有**三族令牌全集与 CSS 变量注入器**；本项不写令牌表，
//!   只声明"令牌段吃令牌名、吐已解析参数"这条段契约；
//! - F3005 转场编排器拥有**编排图与四原语编译器**；本项的
//!   [`MotionGroup::PageTransition`] 只声明"编排段委托编排器"这条**单向委派**
//!   与委派失败时的降级位，不写图算法；
//! - F3006 动效组件化拥有**四件套组件定义与注册表本体**；本项只立段契约与
//!   "组件必须声明 reduce 行为"的**注册要件位**；
//! - F3017 动效无障碍总纲拥有**五类覆盖矩阵与豁免登记簿本体**；本项只立第一红线
//!   的**判定语义**（默认覆盖 + 豁免须理由），并给出与 F3017 衔接的登记位——
//!   覆盖矩阵的逐格直达行为归 F3017；
//! - F3015/F3016/F3018/F3019 分别拥有物理求解器本体、预算分级表、工作台、断言
//!   生成器；本项只登记它们为支撑轴并声明服务关系。
//!
//! # 设计要点（每条都是可执行的，不是标签）
//!
//! - **单源分工是硬门，不是口号**：P 域**只造声明与编排层**，动画的时钟/混合/
//!   采样/速度/逆向一律归 M 域（F2861 契约复用）。[`RuntimeDivision`] 把这条
//!   落成**能力归属表**——每项运行时能力有且只有一个 owner，`claim_runtime`
//!   抢非本域 owner 直接 [`E_RUNTIME_CAPABILITY_NOT_OWNED`]。理由很实际：
//!   两套时钟会让同一动效在两个采样点取到不同相位，表现为"抖动"且极难归因；
//! - **"零运行时开销"要可核**：[`MotionArchitecture::runtime_cost`] 的返回
//!   不是一句"零"，而是**逐段把该段标为声明期（`SegmentCost::DeclarationOnly`）**
//!   的记账；任何一段被标为运行期即 [`E_RUNTIME_OVERHEAD_DECLARED`]，
//!   因为 P 域一旦在帧路径上做事，帧预算就不再是 M 域一家的事；
//! - **十项映射 O(1)**：[`MotionArchitecture::theme_landing`] 走
//!   [`MotionTheme::rank`] 索引定位，不做线性查表——查表 O(10) 虽也是常数，
//!   但**索引化让"映射缺失"与"映射为空"两种故障不再混同**（前者查无此项，
//!   后者查得此项但落点为空），这是 O(1) 真正的收益；
//! - **契约核验 O(签名数）**：[`MotionArchitecture::check_contracts`] 对
//!   [`INTERFACE_VERSION`] 下的签名逐条核对段首尾相接与冻结版本一致，
//!   签名数即段数 × 3 组，是**有上界的常量**，不是"大概扫一遍"；
//! - **对拍拦截而非事后猜**：M 域能力假设错误（锚点降级矩阵第一格）由
//!   [`CapabilityProbe`] 表达——P 侧声明"我假设 M 域能做什么"，探针实跑一遍，
//!   不符即 [`E_RUNTIME_CAPABILITY_DIVERGENCE`] 并阻断，**不降级掩盖**
//!   （能力分歧降级 = 让用户在错的动效上看到界面）；
//! - **接口变更走 ADR，不许就地改**：接口冻结版本写在
//!   [`INTERFACE_VERSION`] 里，任何段契约的增删改都必须升版本并登记
//!   [`AdrRecord`]；就地改会让下游无从判断"我该按哪版编"；
//! - **第一红线是判定器不是声明**：[`ReducedMotionPolicy::evaluate`] 对每类
//!   动效给出 `Covered` / `Exempt` / `Violation` 三态——**没登记就是默认覆盖**
//!   （不是"默认不处理"），豁免必须带理由且带期限，无理由豁免直接
//!   [`E_EXEMPTION_NO_REASON`]。锚点原话是「覆盖是默认，动效是例外」，
//!   代码必须让"例外"比"默认"更难走通，否则这条红线只是文档；
//! - **三底线各有可执行判定**：帧帧可放大看 → [`BottomLine::FrameAudit`] 要求
//!   每帧留采样点且采样密度不低于阈值；打断必处理 →
//!   [`BottomLine::InterruptAudit`] 要求每个活动实例登记打断处置策略，缺一即红；
//!   无障碍必覆盖 → [`BottomLine::CoverageAudit`] 转调第一红线判定器；
//! - **降级链只做前向声明**：[`DegradationChain`] 的每一档都带 `landed: bool`，
//!   F3038 未落地前全部 `false`。[`MotionArchitecture::check_chain_forward`]
//!   要求"前向声明齐、已落地档数如实"——**齐了但全false 是合法的**（前向阶段），
//!   **缺档或谎报已落地才是缺陷**。
//!
//! **零外部依赖**，只依赖 `crate::checks`（自检侧）与 `alloc`。
//! 确定性：逻辑 tick 注入、零墙钟、零 IO，回归可复现（对拍红线）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、总纲常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 总纲版本。契约变更走版本号，破坏性变更必须升版并留迁移说明。
pub const ARCH_VERSION: &str = "P01-arch-v1";

/// **段间接口冻结版本**（锚点原文「段间接口冻结 v1」）。
///
/// 任何段的吃/吐/失败策略发生增删改，必须升这个版本并在 [`AdrRecord`] 登记——
/// 就地改会让下游无从判断该按哪一版编，这是接口冻结的全部意义。
pub const INTERFACE_VERSION: &str = "P01-iface-v1";

/// 承载段间接口的组数（判据「三组架构」；域级收口是闸门不是接口组，见头注）。
pub const GROUP_COUNT: usize = 3;

/// 官方动效主题数（判据「十项映射」）。
pub const THEME_COUNT: usize = 10;

/// 支撑轴数（物理/性能/无障碍/调试/测试——横切十主题，不计入十项，见头注）。
pub const AXIS_COUNT: usize = 5;

/// 验收底线数（判据「三底线」）。
pub const BOTTOM_LINE_COUNT: usize = 3;

/// 判据项数（六项）。
pub const CRITERION_COUNT: usize = 6;

/// 单组段数上限。段数有上界才使"契约核验 O(签名数)"是**有界常量**而非"大概扫一遍"。
pub const MAX_SEGMENTS_PER_GROUP: usize = 8;

/// 主题落点数上限（单主题落多个条目号是常态——落点表按条目登记）。
pub const MAX_LANDINGS_PER_THEME: usize = 12;

/// 单轴服务主题数上限。
pub const MAX_THEMES_PER_AXIS: usize = THEME_COUNT;

/// 豁免登记条数上限。
pub const MAX_EXEMPTIONS: usize = 64;

/// 降级链档数上限。
pub const MAX_CHAIN_STAGES: usize = 8;

/// ADR 登记条数上限。
pub const MAX_ADRS: usize = 32;

/// 对端契约条数上限（M/O04/N/F3038 四端 + 内部三组）。
pub const MAX_PEERS: usize = 8;

/// 能力探针条数上限（M 域运行时能力项）。
pub const MAX_PROBES: usize = 32;

/// 活动实例登记条数上限（打断审计的对象）。
pub const MAX_ACTIVE_INSTANCES: usize = 256;

/// 帧采样点登记条数上限（帧帧可放大看的对象）。
pub const MAX_FRAME_SAMPLES: usize = 4096;

/// 帧采样密度下限（每帧至少这么多样本点，低于此值"帧帧可放大看"就是空话）。
pub const MIN_SAMPLES_PER_FRAME: u32 = 4;

/// 告警账条数上限。满后拒绝并计数——告警静默丢弃等于钳制失效。
pub const NOTICE_LOG_CAP: usize = 256;

/// 复杂度声明（人读文本）。实现与本表逐条对应，改动必须两处同步走 ADR。
pub const COMPLEXITY_DOC: &str = "\
P 域动效库总架构复杂度声明（VE-F3001 · P01-arch-v1）：
C1  段契约自检 check_contracts：O(签名数)= O(组数 × 段数)，三组段数上界 \
    MAX_SEGMENTS_PER_GROUP，故为有界常量；实现为一趟遍历 + 首尾相接比较。
C2  主题落点定位 theme_landing：O(1)（按 MotionTheme::rank 直接索引，\
    无线性查表；O(1) 的收益在于区分'查无此项'与'此项落点为空'两种故障）。
C3  十项映射对账 audit_themes：O(主题数 × 落点数)，十主题为定长。
C4  锚点标签覆盖机检 check_anchor_label_coverage：O(标签数)，十五标签为定长。
C5  运行时能力认领 claim_runtime：O(能力表)，能力项为定长小表。
C6  能力探针对拍 probe_runtime：O(探针数)，探针为定长小表。
C7  第一红线判定 evaluate：O(1)（豁免表线性查，表上界 MAX_EXEMPTIONS，\
    且命中即返回）。
C8  三底线判定 bottom_line_verdict：O(实例数)（打断审计需逐实例查策略；\
    帧审计需逐帧查样本密度）。
C9  降级链前向核验 check_chain_forward：O(档数)，档数上界 MAX_CHAIN_STAGES。
C10 ADR 登记 register_adr：O(1)（尾部追加 + 唯一性一趟比较）。
C11 告警入账 push_notice：O(1)（尾部追加或账满拒绝）。
C12 架构总自检 self_audit：O(签名数 + 主题数 + 能力数 + 档数)，各项均有定长上界。";

// ---------------------------------------------------------------------------
// 二、三组架构接口（判据一：三组架构 · 段间接口冻结 v1）
// ---------------------------------------------------------------------------

/// 承载段间接口的组（判据一：三组架构）。
///
/// 组的划分依据是**变更节奏不同**：动效库组随语言/令牌变，页面转场组随路由协议
/// 变，微交互组随控件族变。三者混在一份接口里，任何一处变都要重签全部。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MotionGroup {
    /// 组一：动效库组（语言 → 令牌 → 声明 → 编排）。
    MotionLibrary,
    /// 组二：页面转场组（请求 → 编排委派 → 跟手 → 交接）。
    PageTransition,
    /// 组三：微交互组（触发 → 形态 → 物理 → 断言）。
    MicroInteraction,
}

impl MotionGroup {
    /// 三组全集（顺序即 [`GROUP_ORDER`]，唯一真值源）。
    pub const ALL: [MotionGroup; GROUP_COUNT] = [
        MotionGroup::MotionLibrary,
        MotionGroup::PageTransition,
        MotionGroup::MicroInteraction,
    ];

    /// 中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            MotionGroup::MotionLibrary => "动效库组",
            MotionGroup::PageTransition => "页面转场组",
            MotionGroup::MicroInteraction => "微交互组",
        }
    }

    /// 英文名（标识符与文档用）。
    pub fn en(self) -> &'static str {
        match self {
            MotionGroup::MotionLibrary => "motion-library",
            MotionGroup::PageTransition => "page-transition",
            MotionGroup::MicroInteraction => "micro-interaction",
        }
    }

    /// 组码（对外引用，如 `P01-G1`）。
    pub fn code(self) -> &'static str {
        match self {
            MotionGroup::MotionLibrary => "P01-G1",
            MotionGroup::PageTransition => "P01-G2",
            MotionGroup::MicroInteraction => "P01-G3",
        }
    }

    /// 组位序号（0 起）。**全模块唯一的组序真值**——依赖边、预算次序、
    /// 读屏叙述全部由它派生，不允许第二处顺序声明。
    pub fn rank(self) -> u8 {
        match self {
            MotionGroup::MotionLibrary => 0,
            MotionGroup::PageTransition => 1,
            MotionGroup::MicroInteraction => 2,
        }
    }

    /// 枚举往返守卫：码 → 组。未知码返回 `None`（调用方必须显性拒绝，不许猜）。
    pub fn from_code(code: &str) -> Option<MotionGroup> {
        MotionGroup::ALL.iter().copied().find(|g| g.code() == code)
    }

    /// 该组的段位序（段在组内的序号，从 0 起）。
    ///
    /// 段序的设计依据与管线一致：**便宜且不可逆的检查放前面**。动效库组把
    /// "语言取值是否存在"放最前，因为取值错了后面全白做；页面转场组把"路由
    /// 是否已确认"放最前，因为转场是锦上添花而路由是事实源。
    pub fn segments(self) -> &'static [Segment] {
        match self {
            MotionGroup::MotionLibrary => &LIBRARY_SEGMENTS,
            MotionGroup::PageTransition => &TRANSITION_SEGMENTS,
            MotionGroup::MicroInteraction => &MICRO_SEGMENTS,
        }
    }

    /// 该组服务的判据项。
    pub fn serves(self) -> &'static [Criterion] {
        match self {
            // 动效库组是十项映射的落点承载者。
            MotionGroup::MotionLibrary => &[Criterion::ThemeMapping, Criterion::DivisionOfDuty],
            // 页面转场组兑现"单源分工"里的委派与交接。
            MotionGroup::PageTransition => &[Criterion::DivisionOfDuty, Criterion::BottomLine],
            // 微交互组最贴近三底线（打断与反馈必须即时可核）。
            MotionGroup::MicroInteraction => &[Criterion::BottomLine, Criterion::FirstRedLine],
        }
    }
}

/// 组序单源常量（判据「三组架构」的真值出处）。
///
/// 全模块**只有这一处**声明组序。段依赖、预算次序、对端归属全部由它派生。
pub const GROUP_ORDER: [MotionGroup; GROUP_COUNT] = MotionGroup::ALL;

/// 段的成本形态（锚点性能分解「架构声明零运行时开销」的可核形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentCost {
    /// 声明期：本段只在声明/编译期做事，帧路径上不花钱。
    DeclarationOnly,
    /// 运行期：本段在帧路径上做事。
    PerFrame,
}

impl SegmentCost {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            SegmentCost::DeclarationOnly => "声明期",
            SegmentCost::PerFrame => "运行期",
        }
    }

    /// 是否零开销（判据「架构声明零运行时开销」的判定位）。
    pub fn is_zero_overhead(self) -> bool {
        matches!(self, SegmentCost::DeclarationOnly)
    }
}

/// 单段契约（段间接口冻结 v1 的行结构）。
///
/// 每段七项齐发：吃/吐/失败策略/复杂度/消费方/不做清单/成本形态。**缺一项的段
/// 不许进表**——缺"不做清单"的段会被后来人当成万能筐。
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    /// 所属组。
    pub group: MotionGroup,
    /// 段位（组内序号，从 0 起）。
    pub rank: u8,
    /// 段码（对外引用，如 `P01-G1S2`）。
    pub code: &'static str,
    /// 中文职责名。
    pub duty_zh: &'static str,
    /// 输入契约（吃什么）。
    pub input: &'static str,
    /// 输出契约（吐什么）。
    pub output: &'static str,
    /// 失败策略（锚点降级矩阵落到本段的那一格）。
    pub on_failure: &'static str,
    /// 复杂度声明（对应 [`COMPLEXITY_DOC`] 的编号）。
    pub complexity: &'static str,
    /// 下游消费方（跨批对接点）。
    pub consumers: &'static str,
    /// 本段的**不做清单**（越界即违约，防止总纲被当成万能筐）。
    pub not_mine: &'static str,
    /// 成本形态（P 域只允许声明期，见 [`SegmentCost`]）。
    pub cost: SegmentCost,
}

impl Segment {
    /// 段契约七项齐备性自检（缺一即不合格——残缺的段契约无法对拍）。
    pub fn is_complete(&self) -> bool {
        !self.duty_zh.trim().is_empty()
            && !self.input.trim().is_empty()
            && !self.output.trim().is_empty()
            && !self.on_failure.trim().is_empty()
            && !self.complexity.trim().is_empty()
            && !self.consumers.trim().is_empty()
            && !self.not_mine.trim().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "段 {}（{} · {}）：吃 {}；吐 {}；失败 {}；成本 {}；不做 {}",
            self.code,
            self.group.zh(),
            self.duty_zh,
            self.input,
            self.output,
            self.on_failure,
            self.cost.zh(),
            self.not_mine
        )
    }
}

/// 动效库组段表（组一：语言 → 令牌 → 声明 → 编排）。
///
/// 段序的依据：取值错则全错，所以语言段最先；编排最晚，因为它吃前四段的产出。
pub const LIBRARY_SEGMENTS: [Segment; 4] = [
    Segment {
        group: MotionGroup::MotionLibrary,
        rank: 0,
        code: "P01-G1S1",
        duty_zh: "动效语言段：校验四原则取值与时长分级合法性，产出已分级时长",
        input: "动效请求（场景类别 + 期望强度）+ 语言册句柄（本体归 F3002）",
        output: "已分级时长（微反馈/组件转场/页面转场/复杂编排四级之一）+ 缓动族名",
        on_failure: "场景类别未登记 -> 拒绝并给出四级建议；时长越级 -> 钳制并告警",
        complexity: "C1 O(段数)",
        consumers: "F3002 语言册、F3003 令牌体系、P01 组内全部下游段",
        not_mine: "不写四原则正文与缓动曲线（F3002），不写令牌表（F3003）",
        cost: SegmentCost::DeclarationOnly,
    },
    Segment {
        group: MotionGroup::MotionLibrary,
        rank: 1,
        code: "P01-G1S2",
        duty_zh: "令牌段：把令牌名解析为已解析参数，并施加 reduce 覆盖",
        input: "令牌名（dur-<级>/ease-<族-支>/dist-<幅>）+ 主题句柄",
        output: "已解析参数（时长毫秒 / 缓动族支号 / 位移像素）+ reduce 覆盖后终值",
        on_failure: "令牌未定义 -> 编译期拦截并列出同类可用令牌；reduce 态 -> 覆盖为直达值",
        complexity: "C1 O(段数)",
        consumers: "F3003 令牌体系、F3006 组件化、F3011 FLIP、F3015 物理引擎",
        not_mine: "不写令牌全集与 CSS 变量注入（F3003），不写硬编码 lint（F3003）",
        cost: SegmentCost::DeclarationOnly,
    },
    Segment {
        group: MotionGroup::MotionLibrary,
        rank: 2,
        code: "P01-G1S3",
        duty_zh: "声明段：把动效意图展开为元素级任务声明（含 reduce 行为声明）",
        input: "组件定义（触发条件 + 参数默认值 + 无障碍行为四件套）",
        output: "元素级任务声明列表：任务号 × 元素 × 动效类型 × 参数 × reduce 终态",
        on_failure: "组件未声明 reduce 行为 -> 注册拒绝（无障碍是注册要件）；元素缺失 -> 跳过并立案",
        complexity: "C1 O(段数)",
        consumers: "F3006 组件注册表、F3008/F3009 入退场族、F3010 共享元素",
        not_mine: "不写组件注册表本体（F3006），不写六型入退场族（F3008/F3009）",
        cost: SegmentCost::DeclarationOnly,
    },
    Segment {
        group: MotionGroup::MotionLibrary,
        rank: 3,
        code: "P01-G1S4",
        duty_zh: "编排段：把任务声明编译为编排图，并委派 M 域时间轴实例组",
        input: "元素级任务声明列表 + 编排原语（序列/并行/错开/条件分支）",
        output: "编排图（DAG）+ 交M 域执行的实例组请求（**不在本段解算**）",
        on_failure: "图含环 -> 构建期拒绝；实例数超预算 -> 降档并立案；委派失败 -> 对拍拦截",
        complexity: "C1 O(段数)",
        consumers: "F3005 编排器、F3007 组合 DSL、F3016 预算分级、F3038 降级链",
        not_mine: "不写编排图与原语编译器（F3005），不解算任何时间轴（M 域）",
        cost: SegmentCost::DeclarationOnly,
    },
];

/// 页面转场组段表（组二：请求 → 编排委派 → 跟手 → 交接）。
///
/// 段序的依据：**路由先行**——转场挂了页面必须照常切换，所以"路由是否已确认"
/// 是第一段的事，编排失败永远不许回退去改路由。
pub const TRANSITION_SEGMENTS: [Segment; 4] = [
    Segment {
        group: MotionGroup::PageTransition,
        rank: 0,
        code: "P01-G2S1",
        duty_zh: "请求段：接住路由变更，产出转场请求（源/目标/类型/手势上下文）",
        input: "路由变更事件（事实源在 N 域）+ 当前历史栈快照",
        output: "转场请求（含类型、源页句柄、目标页句柄、手势上下文可空）",
        on_failure: "转场风暴（连续导航）-> 请求合并为最新目标；目标页未就绪 -> 骨架期占位",
        complexity: "C1 O(段数)",
        consumers: "N 域路由、F3021 转场协议、F3028 打断、F3029 状态机",
        not_mine: "不拥有路由状态机（N 域），不写转场协议本体（F3021）",
        cost: SegmentCost::DeclarationOnly,
    },
    Segment {
        group: MotionGroup::PageTransition,
        rank: 1,
        code: "P01-G2S2",
        duty_zh: "委派段：把转场请求委派编排器，回收编排图（**单向委派，不复制实现**）",
        input: "转场请求 + 三段模板（退场 + 入场 + 可选共享元素）",
        output: "编排图 + 预计时长 + 复杂度分级（L1-L4）",
        on_failure: "编排失败 -> 直接切换 + 诊断（路由先行）；分级超白名单 -> 准入拒绝",
        complexity: "C1 O(段数)",
        consumers: "F3005 编排器、F3016 分级、F3031 转场预算、F3038 降级链",
        not_mine: "不写编排器（F3005），不建预算分级表（F3016/F3031）",
        cost: SegmentCost::DeclarationOnly,
    },
    Segment {
        group: MotionGroup::PageTransition,
        rank: 2,
        code: "P01-G2S3",
        duty_zh: "跟手段：把手势进度直通编排图进度（**用户驱动，不是自动播放**）",
        input: "手势位移/速度采样 + 跟手映射曲线 + 编排图",
        output: "实时进度（0..1）+ 松手时的初速度（交 M 域速度接口）",
        on_failure: "跟手延迟 > 1 帧 -> P1 立案；多点误触 -> 主触点判定；中断 -> 恢复到最近稳定点",
        complexity: "C1 O(段数)",
        consumers: "F3012 手势三型、F3028 逆向播放、F3025 抽屉吸附",
        not_mine: "不写手势识别（N 域原始手势）、不写映射曲线表本体（F3012）",
        cost: SegmentCost::DeclarationOnly,
    },
    Segment {
        group: MotionGroup::PageTransition,
        rank: 3,
        code: "P01-G2S4",
        duty_zh: "交接段：确认页面接管，对齐路由状态机与转场四步协议终点",
        input: "编排图终态 + 路由状态 + 焦点落点契约",
        output: "接管回执（转场 done）+ 双机对齐结论 + 焦点落点",
        on_failure: "对齐失败 -> P1 + 转场系统回滚；焦点丢失 -> 断线立案；双重接管 -> 幂等处理",
        complexity: "C1 O(段数)",
        consumers: "F3029 双机对齐、F3030 焦点迁移、N05 焦点系统",
        not_mine: "不拥有路由状态机所有权（N 域），不写焦点系统（N05）",
        cost: SegmentCost::DeclarationOnly,
    },
];

/// 微交互组段表（组三：触发 → 形态 → 物理 → 断言）。
///
/// 段序的依据：**即时律**——反馈必须 100ms 内可见，所以触发与形态是最前两段，
/// 物理解算可以慢但不能挡住反馈。
pub const MICRO_SEGMENTS: [Segment; 4] = [
    Segment {
        group: MotionGroup::MicroInteraction,
        rank: 0,
        code: "P01-G3S1",
        duty_zh: "触发段：把用户操作映射为反馈触发（三律之即时律的执行点）",
        input: "操作事件（悬停/按下/聚焦/勾选/提交）+ 装配规则表",
        output: "反馈触发：目标元素 × 形态名（语义由操作决定，不许反配）",
        on_failure: "语义未登记 -> 拒绝并给形态语义表；延迟 > 100ms -> P1（即时律实测）",
        complexity: "C1 O(段数)",
        consumers: "F3014 微反馈标准、N 域控件注册装配钩子、P03 组内全部下游段",
        not_mine: "不写控件族（F 域控件库），不建反馈形态库本体（F3014）",
        cost: SegmentCost::DeclarationOnly,
    },
    Segment {
        group: MotionGroup::MicroInteraction,
        rank: 1,
        code: "P01-G3S2",
        duty_zh: "形态段：施加参数化形态并保证归位（三律之归位律）",
        input: "反馈触发 + 形态参数（从令牌取）",
        output: "已施加形态 + 归位目标（**必须回原位或到新稳态，不许悬空**）",
        on_failure: "参数越界 -> 钳制并告警；归位目标缺失 -> 拒绝施加（悬空即缺陷）",
        complexity: "C1 O(段数)",
        consumers: "F3014 形态库、F3003 令牌、F3012 速度继承",
        not_mine: "不写形态参数表（F3014），不写令牌值（F3003）",
        cost: SegmentCost::DeclarationOnly,
    },
    Segment {
        group: MotionGroup::MicroInteraction,
        rank: 2,
        code: "P01-G3S3",
        duty_zh: "物理段：把弹性类反馈的物理解算委派采样流（**解算在声明期离线完成**）",
        input: "物理参数预设（snappy/soft/bouncy）+ 初值（可含手势继承速度）",
        output: "采样关键帧流（时间序列）+ 终止结论（能量阈值或时长上限）",
        on_failure: "参数病态 -> 钳制并诊断；不收敛 -> 强制收敛（5s 防线）+ 遥测",
        complexity: "C1 O(段数)",
        consumers: "F3015 物理引擎、M 域混合接口、F3012 初值来源",
        not_mine: "不做实时逐帧物理解算（M 域职责），不写预设表本体（F3015）",
        cost: SegmentCost::DeclarationOnly,
    },
    Segment {
        group: MotionGroup::MicroInteraction,
        rank: 3,
        code: "P01-G3S4",
        duty_zh: "断言段：对已施加反馈跑三类断言（终态/打断/reduce），产出可归档结论",
        input: "已施加形态 + 采样帧 + 打断事件 + reduce 态重放",
        output: "断言结论四元组（终态/时序/打断/reduce 各自通过与否）",
        on_failure: "reduce 下中间态残留 -> P1（冻结在中间比不动更糟）；打断无处置 -> 红",
        complexity: "C8 O(实例数)",
        consumers: "F3019 测试体系、F3018 工作台、本域三底线判定",
        not_mine: "不写断言生成器与参考集（F3019），不建工作台（F3018）",
        cost: SegmentCost::DeclarationOnly,
    },
];

/// 取单段契约（复杂度 C1：O(段数)）。
pub fn segment_of(group: MotionGroup, rank: u8) -> Option<&'static Segment> {
    group.segments().iter().find(|s| s.rank == rank)
}

/// 全域段总数（三组 × 各自段数；**签名数的分母**）。
pub fn total_segment_count() -> usize {
    GROUP_ORDER
        .iter()
        .map(|g| g.segments().len())
        .fold(0usize, |acc, n| acc.saturating_add(n))
}

/// 段签名总数（每段三项签名：吃/吐/失败策略——**契约核验 O(签名数)** 的分子）。
pub fn total_signature_count() -> usize {
    total_segment_count().saturating_mul(3)
}

// ---------------------------------------------------------------------------
// 三、十项主题映射（判据二：十项映射）
// ---------------------------------------------------------------------------

/// 官方动效主题（判据二「十项映射」的正式十项）。
///
/// 十项的共同特征是**面向使用者的动效能力**：每一项都能被检索到"它长什么样、
/// 它归谁、它的 reduce 终态是什么"。物理/性能/无障碍/调试/测试不满足这个特征
/// ——它们是支撑轴（[`SupportAxis`]），不是能力项。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MotionTheme {
    /// 主题一：动效语言（四原则 + 时长分级 + 缓动家族）。
    Language,
    /// 主题二：动效令牌（三族 + reduce 覆盖层）。
    Token,
    /// 主题三：转场编排（编排图 + 四原语 + 打断策略）。
    Orchestration,
    /// 主题四：动效组件化（四件套 + 三族注册 + 自定义扩展）。
    Composition,
    /// 主题五：入场退场（六型入场 + 六型退场 + 移除护栏）。
    EnterExit,
    /// 主题六：共享元素（pair 配对 + 四维插值 + 飞行体）。
    SharedElement,
    /// 主题七：布局动画 FLIP（快照 + 差分 + 反演回放）。
    Flip,
    /// 主题八：手势驱动（跟手映射 + 阈值迟滞 + 速度继承）。
    Gesture,
    /// 主题九：滚动驱动（只读滚动 + 三族映射 + 视口过滤）。
    Scroll,
    /// 主题十：微反馈（即时/归位/语义三律 + 六形态）。
    MicroFeedback,
}

impl MotionTheme {
    /// 十主题全集（顺序即 [`THEME_ORDER`]，唯一真值源）。
    pub const ALL: [MotionTheme; THEME_COUNT] = [
        MotionTheme::Language,
        MotionTheme::Token,
        MotionTheme::Orchestration,
        MotionTheme::Composition,
        MotionTheme::EnterExit,
        MotionTheme::SharedElement,
        MotionTheme::Flip,
        MotionTheme::Gesture,
        MotionTheme::Scroll,
        MotionTheme::MicroFeedback,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            MotionTheme::Language => "动效语言",
            MotionTheme::Token => "动效令牌",
            MotionTheme::Orchestration => "转场编排",
            MotionTheme::Composition => "动效组件化",
            MotionTheme::EnterExit => "入场退场",
            MotionTheme::SharedElement => "共享元素",
            MotionTheme::Flip => "布局动画 FLIP",
            MotionTheme::Gesture => "手势驱动",
            MotionTheme::Scroll => "滚动驱动",
            MotionTheme::MicroFeedback => "微反馈",
        }
    }

    /// 英文名。
    pub fn en(self) -> &'static str {
        match self {
            MotionTheme::Language => "motion-language",
            MotionTheme::Token => "motion-token",
            MotionTheme::Orchestration => "orchestration",
            MotionTheme::Composition => "componentization",
            MotionTheme::EnterExit => "enter-exit",
            MotionTheme::SharedElement => "shared-element",
            MotionTheme::Flip => "flip",
            MotionTheme::Gesture => "gesture",
            MotionTheme::Scroll => "scroll",
            MotionTheme::MicroFeedback => "micro-feedback",
        }
    }

    /// 主题码（台账引用）。
    pub fn code(self) -> &'static str {
        match self {
            MotionTheme::Language => "MT-LANG",
            MotionTheme::Token => "MT-TOKEN",
            MotionTheme::Orchestration => "MT-ORCH",
            MotionTheme::Composition => "MT-COMP",
            MotionTheme::EnterExit => "MT-ENTEREXIT",
            MotionTheme::SharedElement => "MT-SHARED",
            MotionTheme::Flip => "MT-FLIP",
            MotionTheme::Gesture => "MT-GESTURE",
            MotionTheme::Scroll => "MT-SCROLL",
            MotionTheme::MicroFeedback => "MT-MICROFB",
        }
    }

    /// 主题位序号（0 起）。[`MotionArchitecture::theme_landing`] 的 O(1) 索引键。
    pub fn rank(self) -> usize {
        match self {
            MotionTheme::Language => 0,
            MotionTheme::Token => 1,
            MotionTheme::Orchestration => 2,
            MotionTheme::Composition => 3,
            MotionTheme::EnterExit => 4,
            MotionTheme::SharedElement => 5,
            MotionTheme::Flip => 6,
            MotionTheme::Gesture => 7,
            MotionTheme::Scroll => 8,
            MotionTheme::MicroFeedback => 9,
        }
    }

    /// 枚举往返守卫：未知码返回 `None`（不猜近似值）。
    pub fn from_code(code: &str) -> Option<MotionTheme> {
        MotionTheme::ALL.iter().copied().find(|t| t.code() == code)
    }

    /// 该主题的锚点标签（锚点括号内枚举的原文标签——十五标签覆盖机检的比对键）。
    pub fn anchor_label(self) -> &'static str {
        match self {
            MotionTheme::Language => "动效语言",
            MotionTheme::Token => "令牌",
            MotionTheme::Orchestration => "转场编排",
            MotionTheme::Composition => "组件化",
            MotionTheme::EnterExit => "入场退场",
            MotionTheme::SharedElement => "共享元素",
            MotionTheme::Flip => "FLIP",
            MotionTheme::Gesture => "手势驱动",
            MotionTheme::Scroll => "滚动驱动",
            MotionTheme::MicroFeedback => "微反馈",
        }
    }

    /// 该主题的主责组（主题 → 组的落点，用于归属反查）。
    pub fn primary_group(self) -> MotionGroup {
        match self {
            // 语言/令牌/组件化是声明层的东西，全部归动效库组。
            MotionTheme::Language
            | MotionTheme::Token
            | MotionTheme::Composition
            | MotionTheme::EnterExit
            | MotionTheme::SharedElement
            | MotionTheme::Flip => MotionGroup::MotionLibrary,
            // 编排既是库组能力也是转场最大客户——落库组（转场是消费者）。
            MotionTheme::Orchestration => MotionGroup::MotionLibrary,
            // 跟手与滚动驱动服务转场期交互。
            MotionTheme::Gesture | MotionTheme::Scroll => MotionGroup::PageTransition,
            // 微反馈是操作级响应。
            MotionTheme::MicroFeedback => MotionGroup::MicroInteraction,
        }
    }

    /// 该主题在 reduce 态的**终态类别**（判据「无障碍必覆盖」的对象）。
    ///
    /// 终态不是"没有动画"而是"动画终点的画面"——`Direct`（直达终态）、
    /// `InstantState`（瞬时状态切换，反馈仍可见）、`UserDriven`（跟手类，
    /// 用户直接驱动故豁免自动动画限制）。
    pub fn reduce_terminal(self) -> ReduceTerminal {
        match self {
            MotionTheme::Language | MotionTheme::Token => ReduceTerminal::Direct,
            MotionTheme::Orchestration | MotionTheme::Composition => ReduceTerminal::Direct,
            MotionTheme::EnterExit => ReduceTerminal::Direct,
            MotionTheme::SharedElement => ReduceTerminal::Direct,
            MotionTheme::Flip => ReduceTerminal::Direct,
            // 跟手是用户直接控制，reduce 只限自动动画——豁免须走白名单登记。
            MotionTheme::Gesture => ReduceTerminal::UserDriven,
            MotionTheme::Scroll => ReduceTerminal::Direct,
            // 微反馈在 reduce 下转为瞬时状态变化，**反馈不许消失**。
            MotionTheme::MicroFeedback => ReduceTerminal::InstantState,
        }
    }
}

/// 主题序单源常量（判据「十项映射」的真值出处）。
pub const THEME_ORDER: [MotionTheme; THEME_COUNT] = MotionTheme::ALL;

/// reduce 态终态类别（判据「无障碍必覆盖」+ 第一红线的判定输出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReduceTerminal {
    /// 直达终态：终态画面原子应用，**不是黑屏、不是中间态**。
    Direct,
    /// 瞬时状态切换：反馈语义保留但不做动画（反馈不因 reduce 消失）。
    InstantState,
    /// 用户驱动豁免：用户手指直接控制，属反馈不属装饰。
    UserDriven,
}

impl ReduceTerminal {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ReduceTerminal::Direct => "直达终态",
            ReduceTerminal::InstantState => "瞬时状态切换",
            ReduceTerminal::UserDriven => "用户驱动豁免",
        }
    }

    /// 该终态是否**仍对用户可见**（`Direct` 是画面终态，也可见）。
    pub fn is_user_visible(self) -> bool {
        !matches!(self, ReduceTerminal::Direct)
    }
}

/// 支撑轴（横切十主题的五轴：物理/性能/无障碍/调试/测试）。
///
/// 轴与主题的关系是**多对多的服务**，不是归属：物理引擎服务编排与微反馈，
/// 性能服务全部十项，无障碍服务全部十项——所以轴若不声明服务对象就是空轴。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SupportAxis {
    /// 轴一：动效物理引擎（弹簧/惯性/阻尼三模型，固定步长确定性解算）。
    Physics,
    /// 轴二：动效性能分级与预算（L1-L4 + 两项新预算 + 降档序）。
    Performance,
    /// 轴三：动效无障碍总纲（五类覆盖矩阵 + 豁免制度 + 感知替代）。
    Accessibility,
    /// 轴四：调试器与预览工作台（实时联动 + 时间线检视 + 预览即生产）。
    Debugging,
    /// 轴五：测试与参考对比（四类断言自动生成 + SSIM 参考集）。
    Testing,
}

impl SupportAxis {
    /// 五轴全集。
    pub const ALL: [SupportAxis; AXIS_COUNT] = [
        SupportAxis::Physics,
        SupportAxis::Performance,
        SupportAxis::Accessibility,
        SupportAxis::Debugging,
        SupportAxis::Testing,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            SupportAxis::Physics => "动效物理引擎",
            SupportAxis::Performance => "动效性能分级与预算",
            SupportAxis::Accessibility => "动效无障碍总纲",
            SupportAxis::Debugging => "动效调试器与预览工作台",
            SupportAxis::Testing => "动效库测试与参考对比",
        }
    }

    /// 轴码。
    pub fn code(self) -> &'static str {
        match self {
            SupportAxis::Physics => "AX-PHYS",
            SupportAxis::Performance => "AX-PERF",
            SupportAxis::Accessibility => "AX-A11Y",
            SupportAxis::Debugging => "AX-DEBUG",
            SupportAxis::Testing => "AX-TEST",
        }
    }

    /// 轴位序号（0 起）。
    pub fn rank(self) -> usize {
        match self {
            SupportAxis::Physics => 0,
            SupportAxis::Performance => 1,
            SupportAxis::Accessibility => 2,
            SupportAxis::Debugging => 3,
            SupportAxis::Testing => 4,
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<SupportAxis> {
        SupportAxis::ALL.iter().copied().find(|a| a.code() == code)
    }

    /// 该轴的锚点标签（锚点括号内枚举的原文标签）。
    pub fn anchor_label(self) -> &'static str {
        match self {
            SupportAxis::Physics => "物理引擎",
            SupportAxis::Performance => "性能",
            SupportAxis::Accessibility => "无障碍",
            SupportAxis::Debugging => "调试",
            SupportAxis::Testing => "测试",
        }
    }

    /// 该轴的**主责条目**（轴本体的owner 归谁——防止轴被当成无主资源）。
    pub fn owner_item(self) -> &'static str {
        match self {
            SupportAxis::Physics => "VE-F3015",
            SupportAxis::Performance => "VE-F3016",
            SupportAxis::Accessibility => "VE-F3017",
            SupportAxis::Debugging => "VE-F3018",
            SupportAxis::Testing => "VE-F3019",
        }
    }

    /// 该轴**必须服务**的主题（空服务集即空轴，由自检拦）。
    ///
    /// 这里给的是**下限**（至少服务这些），不是说只服务这些。物理引擎至少服务
    /// 编排与微反馈；性能/无障碍/调试/测试四轴服务全部十项。
    pub fn must_serve(self) -> &'static [MotionTheme] {
        match self {
            SupportAxis::Physics => &[MotionTheme::Orchestration, MotionTheme::MicroFeedback],
            SupportAxis::Performance => &THEME_ORDER,
            SupportAxis::Accessibility => &THEME_ORDER,
            SupportAxis::Debugging => &THEME_ORDER,
            SupportAxis::Testing => &THEME_ORDER,
        }
    }
}

/// 轴序单源常量。
pub const AXIS_ORDER: [SupportAxis; AXIS_COUNT] = SupportAxis::ALL;

/// 锚点括号内枚举的**十五个标签**（原文照录，用于覆盖机检）。
///
/// 顺序按锚点出现次序，不重排——重排会让"逐项对齐"变成"集合相等"，
/// 丢掉"漏了第几项"这个信息。
pub const ANCHOR_LABELS: [&str; 15] = [
    "动效语言",
    "令牌",
    "转场编排",
    "组件化",
    "入场退场",
    "共享元素",
    "FLIP",
    "手势驱动",
    "滚动驱动",
    "微反馈",
    "物理引擎",
    "性能",
    "无障碍",
    "调试",
    "测试",
];

/// 锚点标签覆盖机检结论（复杂度 C4：O(标签数)）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnchorCoverage {
    /// 锚点有而本项未覆盖的标签。
    pub missing: Vec<String>,
    /// 本项有而锚点没有的标签（多登记=自造功能，必须为 0）。
    pub invented: Vec<String>,
}

impl AnchorCoverage {
    /// 是否十五标签逐项对齐、无缺无多。
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty() && self.invented.is_empty()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "锚点标签覆盖：锚点 {} 标签，缺 {} 项，多 {} 项",
            ANCHOR_LABELS.len(),
            self.missing.len(),
            self.invented.len()
        )
    }
}

/// 锚点标签覆盖机检（复杂度 C4：O(标签数)）。
///
/// 判据：锚点括号内十五标签 = 十主题标签 ∪ 五轴标签，**两侧都要查**。
/// 只查"锚点有没有被覆盖"会漏掉自造项；只查"本项有没有依据"会漏掉漏项——
/// 十五标签里任何一项被静默丢弃，都是把锚点的一部分假装完成了。
pub fn check_anchor_label_coverage() -> AnchorCoverage {
    let mut cov = AnchorCoverage::default();
    let declared: Vec<&str> = MotionTheme::ALL
        .iter()
        .map(|t| t.anchor_label())
        .chain(SupportAxis::ALL.iter().map(|a| a.anchor_label()))
        .collect();
    for label in ANCHOR_LABELS.iter() {
        if !declared.contains(label) {
            cov.missing.push((*label).to_string());
        }
    }
    for d in declared.iter() {
        if !ANCHOR_LABELS.contains(d) {
            cov.invented.push((*d).to_string());
        }
    }
    cov
}

/// 主题落点审计（十项 10/10 硬门的执行体）。
#[derive(Clone, Debug, Default)]
pub struct ThemeAudit {
    /// 落点缺失（查无此项）。
    pub missing: Vec<MotionTheme>,
    /// 落点存在但条目列表为空（**与缺失不同**：查得到但没填）。
    pub empty: Vec<MotionTheme>,
    /// 落点条目号格式非法（不含 `VE-F` 前缀）。
    pub malformed: Vec<MotionTheme>,
    /// 落点所属组与 [`MotionTheme::primary_group`] 不一致。
    pub group_mismatch: Vec<MotionTheme>,
    /// 落点数超上限。
    pub overflow: Vec<MotionTheme>,
}

impl ThemeAudit {
    /// 是否十主题全通（10/10 硬门）。
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty()
            && self.empty.is_empty()
            && self.malformed.is_empty()
            && self.group_mismatch.is_empty()
            && self.overflow.is_empty()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "十项映射审计：缺失 {} 项、空落点 {} 项、格式非法 {} 项、组不一致 {} 项、超限 {} 项",
            self.missing.len(),
            self.empty.len(),
            self.malformed.len(),
            self.group_mismatch.len(),
            self.overflow.len()
        )
    }
}

// ---------------------------------------------------------------------------
// 四、单源分工（判据三：单源分工 · P 是消费层不另造运行时）
// ---------------------------------------------------------------------------

/// 运行时能力（锚点「M 域运行时执行」的能力清单——**每一项有且只有一个 owner**）。
///
/// 这些能力全部是 M 域的领地：动画的推进靠时钟、混合靠混合器、采样靠采样器。
/// P 域声明"我假设 M 域能做什么"，但**不自己实现其中任何一项**。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuntimeCapability {
    /// 能力一：帧时钟（推进动画时间）。
    FrameClock,
    /// 能力二：姿态混合（多源混合器）。
    PoseBlend,
    /// 能力三：关键帧采样（轨道→值）。
    KeyframeSample,
    /// 能力四：速度接口（初速度注入与继承）。
    Velocity,
    /// 能力五：逆向播放（时间轴反放）。
    Reverse,
    /// 能力六：时钟零漂移对拍（跨层时间一致性）。
    ClockDriftAudit,
}

impl RuntimeCapability {
    /// 六能力全集。
    pub const ALL: [RuntimeCapability; 6] = [
        RuntimeCapability::FrameClock,
        RuntimeCapability::PoseBlend,
        RuntimeCapability::KeyframeSample,
        RuntimeCapability::Velocity,
        RuntimeCapability::Reverse,
        RuntimeCapability::ClockDriftAudit,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            RuntimeCapability::FrameClock => "帧时钟",
            RuntimeCapability::PoseBlend => "姿态混合",
            RuntimeCapability::KeyframeSample => "关键帧采样",
            RuntimeCapability::Velocity => "速度接口",
            RuntimeCapability::Reverse => "逆向播放",
            RuntimeCapability::ClockDriftAudit => "时钟零漂移对拍",
        }
    }

    /// 能力码。
    pub fn code(self) -> &'static str {
        match self {
            RuntimeCapability::FrameClock => "RC-CLOCK",
            RuntimeCapability::PoseBlend => "RC-BLEND",
            RuntimeCapability::KeyframeSample => "RC-SAMPLE",
            RuntimeCapability::Velocity => "RC-VELOCITY",
            RuntimeCapability::Reverse => "RC-REVERSE",
            RuntimeCapability::ClockDriftAudit => "RC-DRIFT",
        }
    }

    /// 能力位序号（0 起）——探针句柄派生等场合需要稳定序号。
    pub fn rank(self) -> u8 {
        match self {
            RuntimeCapability::FrameClock => 0,
            RuntimeCapability::PoseBlend => 1,
            RuntimeCapability::KeyframeSample => 2,
            RuntimeCapability::Velocity => 3,
            RuntimeCapability::Reverse => 4,
            RuntimeCapability::ClockDriftAudit => 5,
        }
    }

    /// 该能力的**唯一 owner**（单源分工的真值出处）。
    ///
    /// 六项全部归 M 域——这不是"暂时放在 M 域"，而是**归属裁决**：时钟与混合
    /// 一旦有两套，同一动效会在两个采样点取到不同相位，表现为抖动且极难归因。
    pub fn owner(self) -> &'static str {
        match self {
            RuntimeCapability::FrameClock
            | RuntimeCapability::PoseBlend
            | RuntimeCapability::KeyframeSample
            | RuntimeCapability::Velocity
            | RuntimeCapability::Reverse
            | RuntimeCapability::ClockDriftAudit => "VE-M",
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<RuntimeCapability> {
        RuntimeCapability::ALL
            .iter()
            .copied()
            .find(|c| c.code() == code)
    }
}

/// 能力归属裁决（单源分工的执行体）。
#[derive(Clone, Debug, Default)]
pub struct RuntimeDivision {
    /// P 域自有的**声明层**能力（这些是 P 的正当领地）。
    declared: Vec<RuntimeCapability>,
    /// 被拒的越界认领次数（异常显性化）。
    refused: u64,
}

impl RuntimeDivision {
    /// 空归属表（**未声明任何能力**——默认什么都不own，需显式声明）。
    pub fn new() -> Self {
        RuntimeDivision {
            declared: Vec::new(),
            refused: 0,
        }
    }

    /// 声明 P 域自有该能力。
    ///
    /// **本表按设计不接受任何运行时能力成为 P 自有**——六项全部归 M 域。
    /// 所以这个方法存在的意义是：让"我以为 P 可以自己跑时钟"这类误解在
    /// **代码里撞墙**，而不是在评审时被口头纠正。撞墙才留得住。
    pub fn claim_runtime(&mut self, cap: RuntimeCapability) -> Result<(), MotionError> {
        self.refused = self.refused.saturating_add(1);
        Err(MotionError::new(
            E_RUNTIME_CAPABILITY_NOT_OWNED,
            "能力认领被拒：运行时能力不归 P 域",
            &format!(
                "{}（{}）的唯一属主是 {}；P 域只造声明与编排层，不另造动画运行时",
                cap.zh(),
                cap.code(),
                cap.owner()
            ),
            &format!(
                "改为向 {} 提交实例请求并消费其回执；若确需 P 侧自有该能力，走 ADR 改归属表",
                cap.owner()
            ),
            "P 域架构维护方",
        ))
    }

    /// 声明一条**声明层**能力（P 域正当领地，仅用于台账留痕）。
    pub fn note_declaration(&mut self, cap: RuntimeCapability, _why: &str) -> Result<(), MotionError> {
        if _why.trim().is_empty() {
            return Err(MotionError::new(
                E_DECLARATION_NO_WHY,
                "声明被拒：无理由",
                "声明层能力也须写明理由——无理由的声明在下次架构变更时会被当成既定事实",
                "补上为什么 P 域需要声明这一项（一句话）",
                "P 域架构维护方",
            ));
        }
        if !self.declared.contains(&cap) {
            self.declared.push(cap);
        }
        Ok(())
    }

    /// 已声明的声明层能力数。
    pub fn declared_count(&self) -> usize {
        self.declared.len()
    }

    /// 被拒的越界认领次数（**这个数非零是正常的**——它证明闸门在起作用）。
    pub fn refused(&self) -> u64 {
        self.refused
    }

    /// 归属审计：逐能力核对 owner 非空且唯一（复杂度 C5：O(能力数)）。
    pub fn audit_owners(&self) -> Vec<&'static str> {
        let mut bad = Vec::new();
        for c in RuntimeCapability::ALL.iter() {
            let owner = c.owner();
            if owner.trim().is_empty() {
                bad.push("E_CAPABILITY_NO_OWNER");
            }
        }
        // 同一能力被两个组声明为自有 = 归属冲突。
        if self.declared.len() != self.declared.iter().collect::<Vec<_>>().len() {
            bad.push("E_CAPABILITY_DUP");
        }
        bad
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "单源分工：运行时能力 {} 项全部归VE-M；P 域声明层能力 {} 项；\
             越界认领被拒 {} 次",
            RuntimeCapability::ALL.len(),
            self.declared_count(),
            self.refused()
        )
    }
}

impl RuntimeDivision {
    /// 标准归属表（未声明任何声明层能力——待显式登记）。
    pub fn standard() -> Self {
        RuntimeDivision::new()
    }
}

/// 能力探针（锚点降级矩阵第一格「运行时能力分歧 → 对拍拦截」）。
///
/// 探针的语义是：**P 侧声明"我假设 M 域能做什么"，实跑一遍，不符即阻断**。
/// 刻意**不降级掩盖**——能力分歧若降级，用户会在错的动效上看到一个"能动但不对"
/// 的界面，那比明确报错更难排查。
#[derive(Clone, Debug)]
pub struct CapabilityProbe {
    /// 被探能力。
    pub capability: RuntimeCapability,
    /// P 侧假设的能力句柄（不透明标识，来自 F2861 契约）。
    pub assumed_handle: u16,
    /// 假设的能力语义摘要（可对拍字符串）。
    pub assumed_semantics: String,
    /// 实测是否可用。
    pub observed_available: bool,
    /// 实测语义摘要。
    pub observed_semantics: String,
    /// 逻辑 tick。
    pub tick: u64,
}

/// 未指定的能力句柄（保留值）。
pub const HANDLE_UNSPECIFIED: u16 = 0;

impl CapabilityProbe {
    /// 探针判定（复杂度 C6：O(探针数) 由调用方遍历，本函数 O(1)）。
    ///
    /// 三种不符各自给出不同码：**不可用**、**语义漂移**、**句柄缺失**——
    /// 合并成一句"能力不符"会让归因退化成猜。
    pub fn verify(&self) -> Result<(), MotionError> {
        if self.assumed_handle == HANDLE_UNSPECIFIED {
            return Err(MotionError::new(
                E_PROBE_HANDLE_MISSING,
                "能力探针被拒：句柄缺失",
                &format!(
                    "{}（{}）的假设句柄为未指定，无从对拍",
                    self.capability.zh(),
                    self.capability.code()
                ),
                "从 F2861 契约取真实句柄后重跑探针；不接受'未指定也能跑'",
                "P 域架构维护方",
            ));
        }
        if !self.observed_available {
            return Err(MotionError::new(
                E_RUNTIME_CAPABILITY_DIVERGENCE,
                "能力分歧：对拍拦截",
                &format!(
                    "{}（{}）假设可用，实测不可用；M 域能力假设错误必须拦截而不是降级",
                    self.capability.zh(),
                    self.capability.code()
                ),
                &format!(
                    "撤回依赖该能力的动效声明，或与 {} 协商补齐能力后重跑探针",
                    self.capability.owner()
                ),
                "P 域架构维护方",
            ));
        }
        if self.observed_semantics != self.assumed_semantics {
            return Err(MotionError::new(
                E_RUNTIME_SEMANTICS_DRIFT,
                "能力分歧：语义漂移",
                &format!(
                    "{}（{}）假设语义「{}」，实测「{}」；句柄可用不等于语义相同",
                    self.capability.zh(),
                    self.capability.code(),
                    self.assumed_semantics,
                    self.observed_semantics
                ),
                &format!(
                    "以实测语义为准修正假设并更新总纲，或要求 {} 对齐语义",
                    self.capability.owner()
                ),
                "P 域架构维护方",
            ));
        }
        Ok(())
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "探针 {}（{}）：假设句柄 {}「{}」，实测{}「{}」（tick {}）",
            self.capability.zh(),
            self.capability.code(),
            self.assumed_handle,
            self.assumed_semantics,
            if self.observed_available {
                "可用"
            } else {
                "不可用"
            },
            self.observed_semantics,
            self.tick
        )
    }
}

/// 探针对拍账。
#[derive(Clone, Debug, Default)]
pub struct ProbeLedger {
    probes: Vec<CapabilityProbe>,
    blocked: u64,
}

impl ProbeLedger {
    /// 空对拍账。
    pub fn new() -> Self {
        ProbeLedger {
            probes: Vec::new(),
            blocked: 0,
        }
    }

    /// 在册探针数。
    pub fn len(&self) -> usize {
        self.probes.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.probes.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityProbe> {
        self.probes.iter()
    }

    /// 因分歧被阻断的次数。
    pub fn blocked(&self) -> u64 {
        self.blocked
    }

    /// 登记并对拍一个探针（复杂度 C6：O(探针数)）。
    ///
    /// 分歧即 [`E_RUNTIME_CAPABILITY_DIVERGENCE`] 族错误，**阻断计数**——
    /// 阻断数非零且未解决时 [`MotionArchitecture::preflight`] 不会放行。
    pub fn submit(&mut self, probe: CapabilityProbe) -> Result<(), MotionError> {
        if self.probes.len() >= MAX_PROBES {
            self.blocked = self.blocked.saturating_add(1);
            return Err(MotionError::new(
                E_PROBE_LEDGER_FULL,
                "探针入账被拒：对拍账已满",
                &format!(
                    "对拍账{} 条达到上限 {}，本条未入账（累计阻断 {} 次）",
                    self.probes.len(),
                    MAX_PROBES,
                    self.blocked
                ),
                "先归档并轮转对拍账，或按 ADR 提升 MAX_PROBES",
                "P 域架构维护方",
            ));
        }
        probe.verify()?;
        self.probes.push(probe);
        Ok(())
    }

    /// 已对拍通过的能力码清单（**六项齐备才是完整对拍**）。
    pub fn verified_codes(&self) -> Vec<&'static str> {
        self.probes.iter().map(|p| p.capability.code()).collect()
    }

    /// 对拍完备性：六项能力是否全部有通过的探针。
    pub fn is_complete(&self) -> bool {
        let codes = self.verified_codes();
        RuntimeCapability::ALL
            .iter()
            .all(|c| codes.contains(&c.code()))
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "能力对拍账：{} 条（{}/{} 项能力已对拍通过），累计阻断 {} 次",
            self.len(),
            self.probes.len(),
            RuntimeCapability::ALL.len(),
            self.blocked()
        )
    }
}// ---------------------------------------------------------------------------
// 五、验收三底线（判据四：三底线 · 帧帧可放大看 / 打断必处理 / 无障碍必覆盖）
// ---------------------------------------------------------------------------

/// 验收底线（判据四「三底线」）。
///
/// 三条都不是审美偏好，是**可失败的判据**：帧采样密度不够就是「帧帧可放大看」
/// 说了假话；活动实例没有打断处置就是「打断必处理」说了假话；reduce 下有
/// 中间态就是「无障碍必覆盖」说了假话。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BottomLine {
    /// 底线一：帧帧可放大看（每一帧的中间态都经得起放大检查）。
    FrameAudit,
    /// 底线二：打断必处理（每个活动实例都有打断处置策略）。
    InterruptAudit,
    /// 底线三：无障碍必覆盖（全部动效默认受 reduced-motion 覆盖）。
    CoverageAudit,
}

impl BottomLine {
    /// 三底线全集（顺序即 [`BOTTOM_LINE_ORDER`]）。
    pub const ALL: [BottomLine; BOTTOM_LINE_COUNT] = [
        BottomLine::FrameAudit,
        BottomLine::InterruptAudit,
        BottomLine::CoverageAudit,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            BottomLine::FrameAudit => "帧帧可放大看",
            BottomLine::InterruptAudit => "打断必处理",
            BottomLine::CoverageAudit => "无障碍必覆盖",
        }
    }

    /// 底线码。
    pub fn code(self) -> &'static str {
        match self {
            BottomLine::FrameAudit => "BL-FRAME",
            BottomLine::InterruptAudit => "BL-INTERRUPT",
            BottomLine::CoverageAudit => "BL-COVERAGE",
        }
    }

    /// 底线序号（0 起）。
    pub fn rank(self) -> usize {
        match self {
            BottomLine::FrameAudit => 0,
            BottomLine::InterruptAudit => 1,
            BottomLine::CoverageAudit => 2,
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<BottomLine> {
        BottomLine::ALL.iter().copied().find(|b| b.code() == code)
    }

    /// 该底线的**可执行判定说明**（怎么算通过，写下来就是判据）。
    pub fn promise(self) -> &'static str {
        match self {
            BottomLine::FrameAudit => {
                "每一帧至少留 MIN_SAMPLES_PER_FRAME 个采样点且已放大检查；\
                 密度不足即「帧帧可放大看」为假，不许用「动画很流畅」代替可放大检查。"
            }
            BottomLine::InterruptAudit => {
                "每个活动实例必须登记打断处置策略（快速完成/原地保持/回滚三选一）；\
                 无策略的实例在被打断时必然留下半途态。"
            }
            BottomLine::CoverageAudit => {
                "全部动效默认受 reduced-motion 覆盖；未登记豁免且reduce 下\
                 非终态即为缺陷——冻结在中间比不动更糟。"
            }
        }
    }
}

/// 底线序单源常量。
pub const BOTTOM_LINE_ORDER: [BottomLine; BOTTOM_LINE_COUNT] = BottomLine::ALL;

/// 帧采样登记（底线一的判定对象）。
#[derive(Clone, Debug)]
pub struct FrameSample {
    /// 所属实例号。
    pub instance: u32,
    /// 逻辑帧号。
    pub frame: u64,
    /// 本帧采样点数。
    pub sample_count: u32,
    /// 是否已放大检查（人工或工具看过中间态）。
    pub zoom_inspected: bool,
}

impl FrameSample {
    /// 该帧是否满足密度下限**且**已放大检查。
    ///
    /// 两个条件缺一不可：密度够但没人看过，等于采样了但没检查。
    pub fn passes(&self) -> bool {
        self.sample_count >= MIN_SAMPLES_PER_FRAME && self.zoom_inspected
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "帧采样：实例 {} 第 {} 帧，样本 {} 个（下限 {}），{}，{}",
            self.instance,
            self.frame,
            self.sample_count,
            MIN_SAMPLES_PER_FRAME,
            if self.zoom_inspected {
                "已放大检查"
            } else {
                "未放大检查"
            },
            if self.passes() { "通过" } else { "不通过" }
        )
    }
}

/// 打断处置策略（底线二的三选一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterruptPolicy {
    /// 快速完成：把剩余进度快速推到终态。
    QuickFinish,
    /// 原地保持：停在当前帧等待恢复。
    HoldInPlace,
    /// 回滚：回到起始态。
    Rollback,
}

impl InterruptPolicy {
    /// 三策略全集。
    pub const ALL: [InterruptPolicy; 3] = [
        InterruptPolicy::QuickFinish,
        InterruptPolicy::HoldInPlace,
        InterruptPolicy::Rollback,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            InterruptPolicy::QuickFinish => "快速完成",
            InterruptPolicy::HoldInPlace => "原地保持",
            InterruptPolicy::Rollback => "回滚",
        }
    }

    /// 策略码。
    pub fn code(self) -> &'static str {
        match self {
            InterruptPolicy::QuickFinish => "IP-QUICK",
            InterruptPolicy::HoldInPlace => "IP-HOLD",
            InterruptPolicy::Rollback => "IP-ROLLBACK",
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<InterruptPolicy> {
        InterruptPolicy::ALL
            .iter()
            .copied()
            .find(|p| p.code() == code)
    }
}

/// 活动实例登记（底线二的判定对象）。
#[derive(Clone, Debug)]
pub struct ActiveInstance {
    /// 实例号。
    pub id: u32,
    /// 实例所属主题（用于归因到十项之一）。
    pub theme: MotionTheme,
    /// 打断处置策略（**`None` 即底线二红项**）。
    pub interrupt: Option<InterruptPolicy>,
    /// reduce 终态是否已声明。
    pub reduce_declared: bool,
}

impl ActiveInstance {
    /// 该实例是否满足底线二（必须登记打断策略）。
    pub fn interrupt_ready(&self) -> bool {
        self.interrupt.is_some()
    }

    /// 该实例是否满足底线三（必须声明 reduce 终态）。
    pub fn coverage_ready(&self) -> bool {
        self.reduce_declared
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "活动实例 #{}（{}）：打断策略 {}；reduce 终态{}",
            self.id,
            self.theme.zh(),
            match self.interrupt {
                Some(p) => p.zh().to_string(),
                None => "未登记（红线）".to_string(),
            },
            if self.reduce_declared {
                "已声明"
            } else {
                "未声明（红线）"
            }
        )
    }
}

/// 三底线判定结论（逐条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineVerdict {
    /// 通过。
    Pass,
    /// 不通过（附缺口数）。
    Fail(u32),
    /// **无数据**（没有样本/没有实例——空集不许判通过）。
    NoData,
}

impl LineVerdict {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            LineVerdict::Pass => "通过",
            LineVerdict::Fail(_) => "不通过",
            LineVerdict::NoData => "无数据",
        }
    }

    /// 是否通过（`NoData` **不算通过**——没测不等于过了）。
    pub fn is_pass(self) -> bool {
        matches!(self, LineVerdict::Pass)
    }

    /// 缺口数（`NoData` 记 1——空集本身就是缺口）。
    pub fn gap(self) -> u32 {
        match self {
            LineVerdict::Fail(n) => n,
            LineVerdict::NoData => 1,
            LineVerdict::Pass => 0,
        }
    }

    /// 带缺口数的构造辅助（便于自检里简写）。
    pub fn fail(n: u32) -> LineVerdict {
        LineVerdict::Fail(n)
    }
}

/// 三底线判定账。
#[derive(Clone, Debug, Default)]
pub struct BottomLineLedger {
    samples: Vec<FrameSample>,
    instances: Vec<ActiveInstance>,
}

impl BottomLineLedger {
    /// 空判定账。
    pub fn new() -> Self {
        BottomLineLedger {
            samples: Vec::new(),
            instances: Vec::new(),
        }
    }

    /// 活动实例在册数。
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// 是否为空账（空账三底线判定全为「无数据」）。
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty() && self.samples.is_empty()
    }

    /// 帧采样在册数。
    pub fn sample_len(&self) -> usize {
        self.samples.len()
    }

    /// 登记一个帧采样（复杂度 C11：O(1) 追加或拒绝）。
    pub fn push_sample(&mut self, s: FrameSample) -> Result<(), MotionError> {
        if self.samples.len() >= MAX_FRAME_SAMPLES {
            return Err(MotionError::new(
                E_FRAME_LEDGER_FULL,
                "帧采样入账被拒：账已满",
                &format!(
                    "帧采样账 {} 条达到上限 {}",
                    self.samples.len(),
                    MAX_FRAME_SAMPLES
                ),
                "归档并轮转采样账，或按 ADR 提升 MAX_FRAME_SAMPLES",
                "P 域架构维护方",
            ));
        }
        self.samples.push(s);
        Ok(())
    }

    /// 登记一个活动实例（复杂度 C11：O(1) 追加或拒绝）。
    pub fn push_instance(&mut self, i: ActiveInstance) -> Result<(), MotionError> {
        if self.instances.len() >= MAX_ACTIVE_INSTANCES {
            return Err(MotionError::new(
                E_INSTANCE_LEDGER_FULL,
                "实例入账被拒：账已满",
                &format!(
                    "活动实例账 {} 条达到上限 {}",
                    self.instances.len(),
                    MAX_ACTIVE_INSTANCES
                ),
                "先回收已终态实例，或按 ADR 提升 MAX_ACTIVE_INSTANCES",
                "P 域架构维护方",
            ));
        }
        self.instances.push(i);
        Ok(())
    }

    /// 底线判定（复杂度 C8：O(实例数)）。
    ///
    /// 逐条独立判定，**任何一条不过即整体不过**——三条底线是并列的合取，
    /// 拿「帧帧可放大看」达标去抵消「无障碍必覆盖」缺失是典型的以长补短。
    pub fn verdict(&self, line: BottomLine) -> LineVerdict {
        match line {
            BottomLine::FrameAudit => {
                if self.samples.is_empty() {
                    return LineVerdict::NoData;
                }
                let bad = self.samples.iter().filter(|s| !s.passes()).count() as u32;
                if bad == 0 {
                    LineVerdict::Pass
                } else {
                    LineVerdict::Fail(bad)
                }
            }
            BottomLine::InterruptAudit => {
                if self.instances.is_empty() {
                    return LineVerdict::NoData;
                }
                let bad = self
                    .instances
                    .iter()
                    .filter(|i| !i.interrupt_ready())
                    .count() as u32;
                if bad == 0 {
                    LineVerdict::Pass
                } else {
                    LineVerdict::Fail(bad)
                }
            }
            BottomLine::CoverageAudit => {
                if self.instances.is_empty() {
                    return LineVerdict::NoData;
                }
                let bad = self
                    .instances
                    .iter()
                    .filter(|i| !i.coverage_ready())
                    .count() as u32;
                if bad == 0 {
                    LineVerdict::Pass
                } else {
                    LineVerdict::Fail(bad)
                }
            }
        }
    }

    /// 三底线是否全通。
    pub fn all_pass(&self) -> bool {
        BOTTOM_LINE_ORDER.iter().all(|l| self.verdict(*l).is_pass())
    }

    /// 不通过的底线清单（**红项要能点名，不能只报「有个红」**）。
    pub fn failing_lines(&self) -> Vec<BottomLine> {
        BOTTOM_LINE_ORDER
            .iter()
            .copied()
            .filter(|l| !self.verdict(*l).is_pass())
            .collect()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut parts = Vec::new();
        for l in BOTTOM_LINE_ORDER.iter() {
            parts.push(format!("{}：{}", l.zh(), self.verdict(*l).zh()));
        }
        format!(
            "三底线判定：帧采样 {} 条、活动实例 {} 条；{}",
            self.samples.len(),
            self.instances.len(),
            parts.join("；")
        )
    }
}

// ---------------------------------------------------------------------------
// 六、第一红线（判据五：一切动效默认受 reduced-motion 覆盖）
// ---------------------------------------------------------------------------

/// 豁免类别（锚点「新动效无障碍豁免必须白名单理由」的三类默认豁免）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExemptionKind {
    /// 跟手反馈：用户手指直接驱动，属反馈不属装饰。
    FollowFinger,
    /// 焦点环：焦点位置必须可见，否则键盘用户失去位置。
    FocusRing,
    /// 进度指示：进度是状态信息，抹掉等于丢失信息。
    ProgressIndicator,
}

impl ExemptionKind {
    /// 三类全集。
    pub const ALL: [ExemptionKind; 3] = [
        ExemptionKind::FollowFinger,
        ExemptionKind::FocusRing,
        ExemptionKind::ProgressIndicator,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ExemptionKind::FollowFinger => "跟手反馈",
            ExemptionKind::FocusRing => "焦点环",
            ExemptionKind::ProgressIndicator => "进度指示",
        }
    }

    /// 类别码。
    pub fn code(self) -> &'static str {
        match self {
            ExemptionKind::FollowFinger => "EX-FOLLOW",
            ExemptionKind::FocusRing => "EX-FOCUS",
            ExemptionKind::ProgressIndicator => "EX-PROGRESS",
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<ExemptionKind> {
        ExemptionKind::ALL
            .iter()
            .copied()
            .find(|e| e.code() == code)
    }
}

/// 豁免登记项（白名单条目——**理由与期限缺一不可**）。
#[derive(Clone, Debug)]
pub struct Exemption {
    /// 豁免类别。
    pub kind: ExemptionKind,
    /// 豁免理由（**必填**，锚点原文「必须白名单理由」）。
    pub reason: String,
    /// 登记期限的逻辑帧号（**必填**：无期限的豁免等于永久绕过红线）。
    pub expires_at_frame: u64,
    /// 登记的逻辑 tick。
    pub tick: u64,
}

impl Exemption {
    /// 该豁免在给定帧是否仍有效（**过期即失效，不看登记人的心情**）。
    pub fn is_active(&self, frame: u64) -> bool {
        frame < self.expires_at_frame
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "豁免 {}（{}）：{}；期限至第 {} 帧",
            self.kind.zh(),
            self.kind.code(),
            self.reason,
            self.expires_at_frame
        )
    }
}

/// 第一红线判定结论（覆盖三态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoverageVerdict {
    /// 默认覆盖：reduce 态直达终态（**这是绝大多数动效的常态**）。
    Covered,
    /// 已登记豁免且仍在期限内。
    Exempt(ExemptionKind),
    /// 违规：既未覆盖也未登记豁免。
    Violation(&'static str),
}

impl CoverageVerdict {
    /// 是否合规（覆盖或合规豁免都算合规）。
    pub fn is_compliant(&self) -> bool {
        !matches!(self, CoverageVerdict::Violation(_))
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        match self {
            CoverageVerdict::Covered => "默认覆盖：reduce 态直达终态".to_string(),
            CoverageVerdict::Exempt(k) => {
                format!("已登记豁免：{}（{}）", k.zh(), k.code())
            }
            CoverageVerdict::Violation(why) => format!("违规：{}", why),
        }
    }
}

/// reduced-motion 策略（第一红线的判定器）。
///
/// **默认覆盖**是这个类型最重要的性质：新建的策略账本里**没有任何豁免**，
/// 所以任何新动效在登记豁免之前一律走 `Covered`。锚点原话是「覆盖是默认，
/// 动效是例外」——要让例外成立，就得让例外比默认更难走通。
#[derive(Clone, Debug, Default)]
pub struct ReducedMotionPolicy {
    exemptions: Vec<Exemption>,
    refused: u64,
}

impl ReducedMotionPolicy {
    /// 空策略账（**零豁免 = 全部默认覆盖**）。
    pub fn new() -> Self {
        ReducedMotionPolicy {
            exemptions: Vec::new(),
            refused: 0,
        }
    }

    /// 在册豁免数。
    pub fn len(&self) -> usize {
        self.exemptions.len()
    }

    /// 是否为空（空账 = 全默认覆盖）。
    pub fn is_empty(&self) -> bool {
        self.exemptions.is_empty()
    }

    /// 因不合规被拒的登记次数。
    pub fn refused(&self) -> u64 {
        self.refused
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &Exemption> {
        self.exemptions.iter()
    }

    /// 登记一条豁免（复杂度 C7：O(1) 查找 + O(1) 追加）。
    ///
    /// 三项拒绝：
    /// 1. **无理由** → [`E_EXEMPTION_NO_REASON`]（锚点「必须白名单理由」）；
    /// 2. **期限为零** → [`E_EXEMPTION_NO_DEADLINE`]（永久豁免=永久绕过红线）；
    /// 3. **同类重复** → [`E_EXEMPTION_DUP`]（同一类别两条豁免=绕过口径不唯一）。
    pub fn register(&mut self, ex: Exemption) -> Result<(), MotionError> {
        if ex.reason.trim().is_empty() {
            self.refused = self.refused.saturating_add(1);
            return Err(MotionError::new(
                E_EXEMPTION_NO_REASON,
                "豁免登记被拒：无理由",
                &format!(
                    "{}（{}）的豁免没有写理由；无理由的豁免无法被评审，也无法被后来人复核",
                    ex.kind.zh(),
                    ex.kind.code()
                ),
                "写清为什么这类动效不能被 reduce 覆盖（理由应落到「信息会丢失」这一类上）",
                "P域无障碍责任人",
            ));
        }
        if ex.expires_at_frame == 0 {
            self.refused = self.refused.saturating_add(1);
            return Err(MotionError::new(
                E_EXEMPTION_NO_DEADLINE,
                "豁免登记被拒：期限为零",
                &format!(
                    "{}（{}）的豁免期限为 0，等于永久绕过第一红线",
                    ex.kind.zh(),
                    ex.kind.code()
                ),
                "给出有限期限的逻辑帧号；续期须重新评审，不接受静默永久",
                "P 域无障碍责任人",
            ));
        }
        if self
            .exemptions
            .iter()
            .any(|e| e.kind == ex.kind && e.is_active(ex.tick))
        {
            self.refused = self.refused.saturating_add(1);
            return Err(MotionError::new(
                E_EXEMPTION_DUP,
                "豁免登记被拒：同类重复",
                &format!(
                    "{}（{}）已有生效中的豁免；两条豁免会让绕过口径不唯一",
                    ex.kind.zh(),
                    ex.kind.code()
                ),
                "改既有豁免的期限或理由，不要并行登记第二条",
                "P 域无障碍责任人",
            ));
        }
        if self.exemptions.len() >= MAX_EXEMPTIONS {
            self.refused = self.refused.saturating_add(1);
            return Err(MotionError::new(
                E_EXEMPTION_CAP,
                "豁免登记被拒：登记表已满",
                &format!(
                    "豁免登记表 {} 条达到上限 {}（累计拒绝 {} 次）",
                    self.exemptions.len(),
                    MAX_EXEMPTIONS,
                    self.refused
                ),
                "先评审并归档失效豁免，或按 ADR 提升 MAX_EXEMPTIONS；\
                 在此之前不得把被拒的那些当成「没申请过」",
                "P 域架构维护方",
            ));
        }
        self.exemptions.push(ex);
        Ok(())
    }

    /// 第一红线判定（复杂度 C7：O(1)）。
    ///
    /// 顺序很重要：**先查豁免、再看终态声明**。反过来的话，一个只声明了
    /// `UserDriven` 终态却没登记豁免的主题会被误判为合规——而锚点要求豁免
    /// **必须登记**，仅仅声明自己该豁免是不够的。
    pub fn evaluate(&self, theme: MotionTheme, frame: u64) -> CoverageVerdict {
        for ex in self.exemptions.iter() {
            if ex.is_active(frame) && kind_serves(ex.kind, theme) {
                return CoverageVerdict::Exempt(ex.kind);
            }
        }
        // 未登记豁免 ⇒ 默认覆盖；终态类别决定直达行为，非终态即违规。
        match theme.reduce_terminal() {
            ReduceTerminal::UserDriven => CoverageVerdict::Violation(
                "声明为用户驱动却未登记豁免——用户驱动是理由，不是许可证",
            ),
            _ => CoverageVerdict::Covered,
        }
    }

    /// 已过期的豁免（**过期不删除，但必须能被看见**）。
    pub fn expired(&self, frame: u64) -> Vec<&Exemption> {
        self.exemptions
            .iter()
            .filter(|e| !e.is_active(frame))
            .collect()
    }

    /// 十主题覆盖全检（复杂度 C7：O(主题数 × 豁免数)，均为定长）。
    pub fn audit_all_themes(&self, frame: u64) -> Vec<(MotionTheme, CoverageVerdict)> {
        MotionTheme::ALL
            .iter()
            .copied()
            .map(|t| (t, self.evaluate(t, frame)))
            .collect()
    }

    /// 读屏摘要（**第一红线要单独念，不能混在别的摘要里**）。
    pub fn screen_text(&self, frame: u64) -> String {
        let verdicts = self.audit_all_themes(frame);
        let exempt = verdicts
            .iter()
            .filter(|(_, v)| matches!(v, CoverageVerdict::Exempt(_)))
            .count();
        let bad = verdicts.iter().filter(|(_, v)| !v.is_compliant()).count();
        let expired = self.expired(frame).len();
        format!(
            "第一红线（reduced-motion 默认覆盖）：在册豁免 {} 条（累计拒绝 {} 次）；\
             十主题：豁免生效 {} 项、违规 {} 项、默认覆盖 {} 项；已过期豁免 {} 项",
            self.len(),
            self.refused(),
            exempt,
            bad,
            THEME_COUNT - exempt - bad,
            expired
        )
    }
}

/// 某豁免类别是否服务某主题（豁免不是全局开关）。
///
/// 跟手只服务手势驱动；焦点环与进度指示是状态信息，任何动效里都要保住。
pub fn kind_serves(kind: ExemptionKind, theme: MotionTheme) -> bool {
    match kind {
        ExemptionKind::FollowFinger => theme == MotionTheme::Gesture,
        ExemptionKind::FocusRing | ExemptionKind::ProgressIndicator => true,
    }
}// ---------------------------------------------------------------------------
// 七、对端消费契约（跨批对接点：M/O04 契约接收 · N 域控件集成 · F3038 前向）
// ---------------------------------------------------------------------------

/// 对端（跨批对接点全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Peer {
    /// 上游/对端：VE-M 动画运行时（F2861 契约复用——P 的实例最终由 M 执行）。
    RuntimeM,
    /// 对端：O04 CSS 声明编译（动效声明→CSS 动画/transition/合成通道）。
    CompilerO04,
    /// 上游：VE-N 控件（动效绑定目标）。
    WidgetN,
    /// 下游：F3038 降级链（前向声明，P 域只留链位）。
    DegradeF3038,
}

impl Peer {
    /// 四端全集。
    pub const ALL: [Peer; 4] = [
        Peer::RuntimeM,
        Peer::CompilerO04,
        Peer::WidgetN,
        Peer::DegradeF3038,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Peer::RuntimeM => "VE-M 动画运行时",
            Peer::CompilerO04 => "O04 声明编译",
            Peer::WidgetN => "VE-N 控件",
            Peer::DegradeF3038 => "F3038 降级链",
        }
    }

    /// 对端码。
    pub fn code(self) -> &'static str {
        match self {
            Peer::RuntimeM => "PEER-M",
            Peer::CompilerO04 => "PEER-O04",
            Peer::WidgetN => "PEER-N",
            Peer::DegradeF3038 => "PEER-F3038",
        }
    }

    /// 关系（上游/对端/下游）。
    pub fn relation(self) -> &'static str {
        match self {
            Peer::WidgetN => "上游",
            Peer::RuntimeM | Peer::CompilerO04 => "对端",
            Peer::DegradeF3038 => "下游",
        }
    }

    /// 是否为**开工阻断级**对端（M 与 O04 未接收契约即不得开工）。
    ///
    /// N 域不是阻断级：控件绑定可以后补，但运行时与编译器的能力假设错了
    /// 会让整域动效跑在错误的能力面上，那不是"后补"能救的。
    pub fn is_blocking(&self) -> bool {
        matches!(self, Peer::RuntimeM | Peer::CompilerO04)
    }
}

/// 对端契约登记项。
#[derive(Clone, Debug)]
pub struct PeerContract {
    /// 对端。
    pub peer: Peer,
    /// 契约名（版本化）。
    pub contract: String,
    /// 契约内容哈希（FNV-1a 64 位十六进制——跨批哈希对账用）。
    pub content_hash: String,
    /// 是否已接收。
    pub received: bool,
    /// 是否已对账通过。
    pub reconciled: bool,
}

impl PeerContract {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "{}（{}）契约 {}：哈希 {}，{}，{}",
            self.peer.zh(),
            self.peer.code(),
            self.contract,
            self.content_hash,
            if self.received { "已接收" } else { "未接收" },
            if self.reconciled { "已对账" } else { "未对账" }
        )
    }
}

/// 消费契约账（P 域与 M/O04/N/F3038 的四条对接）。
///
/// 这张表管"P 域从谁那里拿什么"与"P 域给谁什么"。**单向性**是它的核心纪律：
/// P 域向 M 域提实例请求、向 O04 提交编译请求，但**不从 M/O04 回读内部状态**
/// ——回读会让P 域与运行时内部结构耦合，M 域一次重构就连带炸整域动效。
#[derive(Clone, Debug, Default)]
pub struct PeerLedger {
    peers: Vec<PeerContract>,
}

impl PeerLedger {
    /// 空契约账（**未接收任何契约**——默认阻断，不是默认放行）。
    pub fn new() -> Self {
        PeerLedger { peers: Vec::new() }
    }

    /// 已登记对端数。
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &PeerContract> {
        self.peers.iter()
    }

    /// 登记一条对端契约。
    ///
    /// 拒绝两事：对端重复登记（契约有唯一归属）、契约为空（空契约无法对账）。
    pub fn register(&mut self, c: PeerContract) -> Result<(), MotionError> {
        if c.contract.trim().is_empty() {
            return Err(MotionError::new(
                E_EMPTY_CONTRACT,
                "对端登记被拒：契约为空",
                "空契约没有可对账的内容，登记它只会让哈希对账形同虚设",
                "填入版本化契约名（如 m-motion-runtime/v1）",
                "对端签署方",
            ));
        }
        if self.peers.iter().any(|p| p.peer == c.peer) {
            return Err(MotionError::new(
                E_PEER_DUP,
                "对端登记被拒：对端重复登记",
                &format!(
                    "{} 已有契约条目；同一对端的契约须走变更纪律而不是并行登记",
                    c.peer.zh()
                ),
                "更新既有条目的契约版本与哈希，并走 ADR",
                "对端签署方",
            ));
        }
        if self.peers.len() >= MAX_PEERS {
            return Err(MotionError::new(
                E_PEER_CAP,
                "对端登记被拒：契约账已满",
                &format!("契约账 {} 条达到上限 {}", self.peers.len(), MAX_PEERS),
                "先归档失效对端，或按 ADR 提升 MAX_PEERS",
                "P 域架构维护方",
            ));
        }
        self.peers.push(c);
        Ok(())
    }

    /// 某对端条目。
    pub fn peer(&self, peer: Peer) -> Option<&PeerContract> {
        self.peers.iter().find(|p| p.peer == peer)
    }

    /// 跨批哈希对账（复杂度 C8：O(契约字节长)）。
    ///
    /// 用**重算哈希**与登记哈希比对：登记方声明的内容哈希必须等于本次实际
    /// 提交的字节串哈希。不一致 [`E_HASH_MISMATCH`]——哈希对账不是「看一眼
    /// 有没有填」，是重算一遍。
    pub fn reconcile(&mut self, peer: Peer, content: &str) -> Result<String, MotionError> {
        let actual = fnv1a64_hex(content.as_bytes());
        let Some(entry) = self.peers.iter_mut().find(|p| p.peer == peer) else {
            return Err(MotionError::new(
                E_PEER_UNKNOWN,
                "对账被拒：对端未登记",
                &format!("{} 没有契约条目，无从对账", peer.zh()),
                &format!("先 register({}，...) 登记契约，再提交内容对账", peer.code()),
                "P 域架构维护方",
            ));
        };
        if entry.content_hash != actual {
            entry.reconciled = false;
            return Err(MotionError::new(
                E_HASH_MISMATCH,
                "跨批对账未通过：哈希不一致",
                &format!(
                    "{} 登记哈希 {}，实算哈希 {}；契约内容与登记声明不是同一份",
                    peer.zh(),
                    entry.content_hash,
                    actual
                ),
                "以实算哈希为准更新登记，或撤回改动后的内容后重新对账（不猜测哪边对）",
                "P 域架构维护方",
            ));
        }
        entry.received = true;
        entry.reconciled = true;
        Ok(actual)
    }

    /// 开工前置检查：全部阻断级对端必须已接收且已对账。
    ///
    /// 这是「M/O04 契约接收」判据的执行点——**缺一即阻断**，不接受
    /// 「先开工后补签」。
    pub fn check_blocking_ready(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        let declared_blocking = self.peers.iter().filter(|p| p.peer.is_blocking()).count();
        if declared_blocking == 0 {
            missing.push("E_NO_BLOCKING_PEER_DECLARED");
            return missing;
        }
        for p in self.peers.iter() {
            if !p.peer.is_blocking() {
                continue;
            }
            if !p.received {
                missing.push("E_BLOCKING_NOT_RECEIVED");
            } else if !p.reconciled {
                missing.push("E_BLOCKING_NOT_RECONCILED");
            }
        }
        missing
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "消费契约账：登记对端 {}/{} 个（已接收 {}、已对账 {}），阻断级对端缺口 {} 项",
            self.len(),
            Peer::ALL.len(),
            self.peers.iter().filter(|p| p.received).count(),
            self.peers.iter().filter(|p| p.reconciled).count(),
            self.check_blocking_ready().len()
        )
    }
}

/// 禁扩面（本域**绝不做什么**的显式声明——边界即禁扩面）。
///
/// 这张表与 [`RuntimeCapability::owner`] 一起构成判据三「单源分工」的全部内容：
/// 前者管「运行时能力归谁」，后者管「P 域绝不碰什么」。两者都有可执行断言。
pub const BOUNDARY_EXCLUSIONS: [(&str, &str); 7] = [
    (
        "P-OWN-CLOCK",
        "自建帧时钟/推进器——动画推进归VE-M，两套时钟必现采样相位漂移",
    ),
    (
        "P-OWN-BLENDER",
        "自建姿态混合器——混合归 VE-M，混合算法分叉会让同一动画有两套结果",
    ),
    (
        "P-REVERSE-READ-M",
        "回读 M 域运行时内部状态（实例表/采样缓冲布局）——单向消费，回读即耦合",
    ),
    (
        "P-REVERSE-READ-O04",
        "回读 O04 编译器内部结构（规则表/中间表示）——只提交声明，不窥探实现",
    ),
    (
        "P-OWN-LAYOUT",
        "自建布局算法或布局变更检测——布局归VE-N，双布局并存即同树异结果",
    ),
    (
        "P-OWN-COMPOSITOR",
        "自建合成器与图层提升——提升归渲染域，P 域只声明「要提升什么」",
    ),
    (
        "P-ANIMATE-CONTENT",
        "在动效里携带业务内容数据——动效只搬运几何与样式，内容归属应用层",
    ),
];

/// 禁扩面的归属去处（越界拒绝的「下一步」内容——给路，不只是拒绝）。
pub fn boundary_advice(code: &str) -> &'static str {
    match code {
        "P-OWN-CLOCK" => "把推进需求写成实例请求交 VE-M；P 域只在声明期计算参数",
        "P-OWN-BLENDER" => "混合需求写进实例请求交 VE-M；P 域不持有混合器状态",
        "P-REVERSE-READ-M" => "改为消费 M 域回执；需要状态时由 M 域主动推送",
        "P-REVERSE-READ-O04" => "改为只提交动效声明；编译策略由 O04 自持",
        "P-OWN-LAYOUT" => "把布局需求提给 VE-N；FLIP 只消费布局变更通知",
        "P-OWN-COMPOSITOR" => "把提升需求写进声明的编译目标；合成由渲染域执行",
        "P-ANIMATE-CONTENT" => "动效只传几何与样式句柄；内容由应用层在绑定时注入",
        _ => "先查 BOUNDARY_EXCLUSIONS 确认此事归属，再决定找哪个域",
    }
}

/// 禁扩面校验（复杂度 C5：O(禁扩面条数)）。
///
/// 传入一条「想做的事」，逐条比对 [`BOUNDARY_EXCLUSIONS`]。命中即
/// [`E_BOUNDARY_OVERREACH`]，并把该禁扩面的**归属去处**一并给出——
/// 越界拒绝必须告诉对方「这事该谁做」，否则下次还会有人试。
pub fn check_no_overreach(intent: &str) -> Result<&'static str, MotionError> {
    for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
        if intent.contains(desc) || intent.contains(code) {
            return Err(MotionError::new(
                E_BOUNDARY_OVERREACH,
                "越界被拒：此事不归 P 域",
                &format!("「{}」命中禁扩面 {}：{}", intent, code, desc),
                boundary_advice(code),
                "P 域架构维护方",
            ));
        }
    }
    Ok("在边界内")
}

// ---------------------------------------------------------------------------
// 八、降级链前向声明（锚点：降级链 F3038 前向声明）
// ---------------------------------------------------------------------------

/// 降级档位（性能压力下的逐级降档——**降档不得面目全非**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DegradeStage {
    /// 档一：错开取消（去掉 stagger，保留主时序）。
    DropStagger,
    /// 档二：视差取消（去掉装饰位移，保留主转场）。
    DropParallax,
    /// 档三：fade 化（复杂形态退化为交叉淡入淡出）。
    FadeOnly,
    /// 档四：直达（跳变为终态，等效性须另行验收）。
    DirectJump,
}

impl DegradeStage {
    /// 四档全集（顺序即降级序，**从轻到重**）。
    pub const ALL: [DegradeStage; 4] = [
        DegradeStage::DropStagger,
        DegradeStage::DropParallax,
        DegradeStage::FadeOnly,
        DegradeStage::DirectJump,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            DegradeStage::DropStagger => "错开取消",
            DegradeStage::DropParallax => "视差取消",
            DegradeStage::FadeOnly => "fade 化",
            DegradeStage::DirectJump => "直达",
        }
    }

    /// 档位码。
    pub fn code(self) -> &'static str {
        match self {
            DegradeStage::DropStagger => "DG-STAGGER",
            DegradeStage::DropParallax => "DG-PARALLAX",
            DegradeStage::FadeOnly => "DG-FADE",
            DegradeStage::DirectJump => "DG-DIRECT",
        }
    }

    /// 档位序号（0 起，最轻为 0）。
    pub fn rank(self) -> u8 {
        match self {
            DegradeStage::DropStagger => 0,
            DegradeStage::DropParallax => 1,
            DegradeStage::FadeOnly => 2,
            DegradeStage::DirectJump => 3,
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<DegradeStage> {
        DegradeStage::ALL.iter().copied().find(|d| d.code() == code)
    }

    /// 该档是否属于**面目全非**档（须走等效性验收）。
    ///
    /// 只有 `DirectJump` 算——它把整个动效变成跳变，视觉差异最大。把它标出来
    /// 是为了让等效性验收有明确对象，而不是"降档后看着还行"。
    pub fn is_identity_breaking(self) -> bool {
        matches!(self, DegradeStage::DirectJump)
    }
}

/// 降级链登记项（**F3038 未落地前`landed` 全为 false——如实标注**）。
#[derive(Clone, Copy, Debug)]
pub struct ChainEntry {
    /// 档位。
    pub stage: DegradeStage,
    /// 是否已落地（F3038 实施前为 false）。
    pub landed: bool,
    /// 该档的等效性验收结论（未验收为空串——**空即未验收，不许当通过**）。
    pub equivalence: &'static str,
}

impl ChainEntry {
    /// 该档是否已通过等效性验收。
    ///
    /// 判据三条同时成立才算：已落地 + 非面目全非档 + 有验收结论。
    /// `DirectJump` 恒需验收；其余档落地即可用。
    pub fn equivalence_ok(&self) -> bool {
        if !self.landed {
            return false;
        }
        if !self.stage.is_identity_breaking() {
            return true;
        }
        !self.equivalence.trim().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "降级档 {}（{}）：{}；等效性验收{}",
            self.stage.zh(),
            self.stage.code(),
            if self.landed { "已落地" } else { "未落地（前向声明）" },
            if self.equivalence_ok() {
                "通过"
            } else {
                "未通过"
            }
        )
    }
}

/// 降级链（前向声明）。
///
/// 链在P 域**只做前向声明**：F3038 落地前四档全`landed=false`。前向阶段的
/// 合法性判据是「**档齐 + 如实**」——齐了但全 false 合法，缺档或谎报已落地
/// 才是缺陷。这样P 域能在F3038 动工前就把接口位与降级序冻住，又不会假装
/// 降级链已经存在。
#[derive(Clone, Debug)]
pub struct DegradeChain {
    entries: Vec<ChainEntry>,
}

impl DegradeChain {
    /// 标准降级链（四档全部前向声明，`landed=false`）。
    pub fn forward() -> Self {
        DegradeChain {
            entries: DegradeStage::ALL
                .iter()
                .map(|s| ChainEntry {
                    stage: *s,
                    landed: false,
                    equivalence: "",
                })
                .collect(),
        }
    }

    /// 以给定档表构造（供对拍与负样本演练用；F3038 落地时也走这个入口）。
    pub fn from_entries(entries: Vec<ChainEntry>) -> Self {
        DegradeChain { entries }
    }

    /// 在册档数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 只读遍历（按档位序）。
    pub fn iter(&self) -> impl Iterator<Item = &ChainEntry> {
        self.entries.iter()
    }

    /// 前向核验（复杂度 C9：O(档数)）。
    ///
    /// 三条判定：四档齐备、档序严格递增、**已落地档数与 `landed` 声明一致**
    /// （本域不实现落地，所以只核"没谎报"——谎报的形态是 `landed=true` 而
    /// F3038 尚未接手，那会让上层以为降级链可用而无人负责）。
    pub fn check_forward(&self) -> Vec<&'static str> {
        let mut issues = Vec::new();
        if self.entries.len() != DegradeStage::ALL.len() {
            issues.push("E_CHAIN_INCOMPLETE");
            return issues;
        }
        let mut last: Option<u8> = None;
        for e in self.entries.iter() {
            if let Some(l) = last {
                if e.stage.rank() <= l {
                    issues.push("E_CHAIN_ORDER");
                    break;
                }
            }
            last = Some(e.stage.rank());
        }
        // 档位覆盖：四档各恰好一次。
        for d in DegradeStage::ALL.iter() {
            let n = self.entries.iter().filter(|e| e.stage == *d).count();
            if n != 1 {
                issues.push("E_CHAIN_DUP");
                break;
            }
        }
        // 面目全非档若已落地，必须有等效性验收结论。
        for e in self.entries.iter() {
            if e.stage.is_identity_breaking() && e.landed && e.equivalence.trim().is_empty() {
                issues.push("E_CHAIN_EQUIV_MISSING");
            }
        }
        issues
    }

    /// 已落地档数（前向阶段应为零）。
    pub fn landed_count(&self) -> usize {
        self.entries.iter().filter(|e| e.landed).count()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "降级链（前向声明）：{} 档，已落地 {} 档，前向核验缺口 {} 项",
            self.len(),
            self.landed_count(),
            self.check_forward().len()
        )
    }
}

// ---------------------------------------------------------------------------
// 九、ADR 登记（组间接口变更 → ADR；降级矩阵第二格）
// ---------------------------------------------------------------------------

/// ADR 记录（架构决策记录——**组间接口变更的唯一合法通道**）。
#[derive(Clone, Debug)]
pub struct AdrRecord {
    /// ADR 号（单调）。
    pub id: u64,
    /// 关联组（接口变更影响哪一组）。
    pub group: MotionGroup,
    /// 决策标题。
    pub title: String,
    /// 决策内容（为什么这么定）。
    pub decision: String,
    /// 被否决的方案与否决理由（**必填**：不写否决理由的 ADR 无法复核）。
    pub rejected: String,
    /// 旧接口版本（新建时为空串）。
    pub from_version: String,
    /// 新接口版本。
    pub to_version: String,
    /// 逻辑 tick。
    pub tick: u64,
}

impl AdrRecord {
    /// ADR 五要素齐备性自检（缺一即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.title.trim().is_empty()
            && !self.decision.trim().is_empty()
            && !self.rejected.trim().is_empty()
            && !self.to_version.trim().is_empty()
    }

    /// 是否为**升版**记录（升版必须写清旧版本）。
    pub fn is_version_bump(&self) -> bool {
        !self.from_version.trim().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "ADR #{}（{} · {}→{}）：{}；决策 {}；否决 {}",
            self.id,
            self.group.zh(),
            if self.from_version.is_empty() {
                "新建"
            } else {
                self.from_version.as_str()
            },
            self.to_version,
            self.title,
            self.decision,
            self.rejected
        )
    }
}

/// ADR 账。
#[derive(Clone, Debug, Default)]
pub struct AdrLedger {
    records: Vec<AdrRecord>,
    next_id: u64,
}

impl AdrLedger {
    /// 空 ADR 账。
    pub fn new() -> Self {
        AdrLedger {
            records: Vec::new(),
            next_id: 1,
        }
    }

    /// 在册 ADR 数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &AdrRecord> {
        self.records.iter()
    }

    /// 登记一条 ADR（复杂度 C10：O(1)）。
    ///
    /// 四项拒绝：标题/决策/否决理由/新版本为空（缺一即无法复核）；
    /// 声明升版但未填旧版本（[`E_ADR_BUMP_NO_FROM`]）；账满。
    pub fn register(&mut self, mut rec: AdrRecord) -> Result<u64, MotionError> {
        if rec.title.trim().is_empty() {
            return Err(MotionError::new(
                E_ADR_NO_TITLE,
                "ADR 登记被拒：无标题",
                "无标题的 ADR 无法被检索，等于把决策藏起来",
                "写清决策标题（改了什么）",
                "P 域架构维护方",
            ));
        }
        if rec.decision.trim().is_empty() {
            return Err(MotionError::new(
                E_ADR_NO_DECISION,
                "ADR 登记被拒：无决策内容",
                &format!("ADR「{}」没写决策内容，后来人无法判断当前实现是否符合本决策", rec.title),
                "写清决策内容（为什么这么定）",
                "P 域架构维护方",
            ));
        }
        if rec.rejected.trim().is_empty() {
            return Err(MotionError::new(
                E_ADR_NO_REJECTED,
                "ADR 登记被拒：无否决记录",
                &format!(
                    "ADR「{}」没写否决方案与理由；不写否决理由的 ADR 无法复核，\
                     半年后有人会把否决方案再提一遍",
                    rec.title
                ),
                "写清被否决的方案与否决理由",
                "P 域架构维护方",
            ));
        }
        if rec.to_version.trim().is_empty() {
            return Err(MotionError::new(
                E_ADR_NO_TO_VERSION,
                "ADR 登记被拒：无新版本号",
                &format!("ADR「{}」没写新接口版本，下游无从判断该按哪版编", rec.title),
                "填入新接口版本号（如 P01-iface-v2）",
                "P 域架构维护方",
            ));
        }
        if !rec.from_version.trim().is_empty() && rec.from_version == rec.to_version {
            rec.from_version = String::new();
            return Err(MotionError::new(
                E_ADR_BUMP_NO_FROM,
                "ADR 登记被拒：升版信息矛盾",
                &format!(
                    "ADR「{}」声明从 {} 升到 {}，新旧版本相同",
                    rec.title, rec.from_version, rec.to_version
                ),
                "若确为新建则清空旧版本；若确为升版则填不同的新版本号",
                "P 域架构维护方",
            ));
        }
        if self.records.len() >= MAX_ADRS {
            return Err(MotionError::new(
                E_ADR_CAP,
                "ADR 登记被拒：账已满",
                &format!("ADR 账 {} 条达到上限 {}", self.records.len(), MAX_ADRS),
                "先归档旧 ADR，或按 ADR 提升 MAX_ADRS",
                "P 域架构维护方",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        rec.id = id;
        self.records.push(rec);
        Ok(id)
    }

    /// 某组的历史 ADR 数（**组间接口变更多少次要能一眼看出**）。
    pub fn count_for(&self, group: MotionGroup) -> usize {
        self.records.iter().filter(|r| r.group == group).count()
    }

    /// 不完整的 ADR 数（自检用）。
    pub fn incomplete_count(&self) -> usize {
        self.records.iter().filter(|r| !r.is_complete()).count()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "ADR 账：{} 条（三组分别 {} / {} / {}），不完整 {} 条",
            self.len(),
            self.count_for(MotionGroup::MotionLibrary),
            self.count_for(MotionGroup::PageTransition),
            self.count_for(MotionGroup::MicroInteraction),
            self.incomplete_count()
        )
    }
}

// ---------------------------------------------------------------------------
// 十、哈希工具（跨批对账用；FNV-1a 64 位，零依赖、确定性）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 位哈希（单遍累积）。
///
/// 选它不用更强哈希是因为**对账要的是确定性而非抗攻击**——两侧算同一样东西
/// 必须得到同一个数，而抗碰撞不是这一层的诉求（内容一旦被恶意构造，走的是
/// 供应链签名那条线，不是对账这条线）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV64_OFFSET;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV64_PRIME);
    }
    h
}

/// FNV-1a 64 位十六进制（16 位小写，定宽——宽度不定就没法字符串比对）。
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}// ---------------------------------------------------------------------------
// 十一、判据六项（锚点判据：三组架构 / 十项映射 / 单源分工 / 三底线 / 第一红线 / 判据）
// ---------------------------------------------------------------------------

/// 判据项。锚点判据列六项，本总纲把它们变成**可被逐条断言的枚举**。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Criterion {
    /// 判据一：三组架构（库/转场/微交互段间接口冻结 v1）。
    ThreeGroups,
    /// 判据二：十项映射（官方主题动效范围映射）。
    ThemeMapping,
    /// 判据三：单源分工（P 是消费层，不另造动画运行时）。
    DivisionOfDuty,
    /// 判据四：三底线（帧帧可放大看 + 打断必处理 + 无障碍必覆盖）。
    BottomLine,
    /// 判据五：第一红线（一切动效默认受 reduced-motion 覆盖，豁免须白名单理由）。
    FirstRedLine,
    /// 判据六：判据本身（总纲自证可追溯）。
    Criterion,
}

/// 六项判据全集（判据：一项不缺）。
pub const CRITERIA: [Criterion; CRITERION_COUNT] = [
    Criterion::ThreeGroups,
    Criterion::ThemeMapping,
    Criterion::DivisionOfDuty,
    Criterion::BottomLine,
    Criterion::FirstRedLine,
    Criterion::Criterion,
];

impl Criterion {
    /// 判据中文名（读屏播报）。
    pub fn zh(self) -> &'static str {
        match self {
            Criterion::ThreeGroups => "三组架构",
            Criterion::ThemeMapping => "十项映射",
            Criterion::DivisionOfDuty => "单源分工",
            Criterion::BottomLine => "三底线",
            Criterion::FirstRedLine => "第一红线",
            Criterion::Criterion => "判据",
        }
    }

    /// 判据码（对拍与台账引用）。
    pub fn code(self) -> &'static str {
        match self {
            Criterion::ThreeGroups => "P01-J1",
            Criterion::ThemeMapping => "P01-J2",
            Criterion::DivisionOfDuty => "P01-J3",
            Criterion::BottomLine => "P01-J4",
            Criterion::FirstRedLine => "P01-J5",
            Criterion::Criterion => "P01-J6",
        }
    }

    /// 该判据的承诺句（写下来就是契约，不许用「应该」「尽量」这类词）。
    pub fn promise(self) -> &'static str {
        match self {
            Criterion::ThreeGroups => {
                "库/转场/微交互三组各有段契约（吃/吐/失败策略/复杂度/消费方/不做清单/\
                 成本形态七项），段间接口冻结 v1；任何段的增删改必须升版并走 ADR，\
                 就地改下游无从判断该按哪版编。"
            }
            Criterion::ThemeMapping => {
                "官方动效范围按十主题（能力）+ 五支撑轴（横切）双轴登记，\
                 十五标签与锚点逐项对齐无遗漏；十主题每项必须有落点，\
                 五支撑轴每轴必须声明服务对象——空轴即缺陷。"
            }
            Criterion::DivisionOfDuty => {
                "P 域只造声明与编排层；帧时钟/姿态混合/关键帧采样/速度/逆向/零漂移对拍\
                 六项运行时能力唯一属主是 VE-M，越界认领直接拒绝且计数。"
            }
            Criterion::BottomLine => {
                "帧帧可放大看=每帧样本密度达标且已放大检查；打断必处理=每个活动实例\
                 都登记三选一处置策略；无障碍必覆盖=reduce 态必须到终态。\
                 三条并列合取，不许以长补短；空集判无数据，不判通过。"
            }
            Criterion::FirstRedLine => {
                "一切动效默认受 reduced-motion 覆盖；豁免是例外，须白名单 + 理由 + 期限\
                 三者齐备，无理由/无期限/同类重复一律拒绝——让例外比默认更难走通。"
            }
            Criterion::Criterion => {
                "判据自证可追溯：契约自检、锚点标签覆盖机检、读屏替述随总纲同源交付；\
                 任一判据无可执行断言即视为未落实。"
            }
        }
    }

    /// 枚举往返守卫：未知码返回 `None`（不猜近似值）。
    pub fn from_code(code: &str) -> Option<Criterion> {
        CRITERIA.iter().copied().find(|c| c.code() == code)
    }

    /// 该判据的执行体名称（自检项前缀，便于台账反查）。
    pub fn enforced_by(self) -> &'static str {
        match self {
            Criterion::ThreeGroups => "check_contracts",
            Criterion::ThemeMapping => "audit_themes + check_anchor_label_coverage",
            Criterion::DivisionOfDuty => "audit_owners + check_no_overreach",
            Criterion::BottomLine => "BottomLineLedger::verdict",
            Criterion::FirstRedLine => "ReducedMotionPolicy::evaluate",
            Criterion::Criterion => "self_audit + architecture_narration",
        }
    }
}

/// 取判据的承诺句。
pub fn criterion_promise(c: Criterion) -> &'static str {
    c.promise()
}

// ---------------------------------------------------------------------------
// 十二、错误码与错误类型（五元组：码/现象/原因/下一步/责任方）
// ---------------------------------------------------------------------------

/// 组位超限。
pub const E_GROUP_CAP: &str = "E_GROUP_CAP";
/// 段数超限。
pub const E_SEGMENT_CAP: &str = "E_SEGMENT_CAP";
/// 段契约不完整（缺七项之一）。
pub const E_SEGMENT_INCOMPLETE: &str = "E_SEGMENT_INCOMPLETE";
/// 段序非严格递增。
pub const E_SEGMENT_ORDER: &str = "E_SEGMENT_ORDER";
/// 段码重复。
pub const E_SEGMENT_DUP: &str = "E_SEGMENT_DUP";
/// 段被标为运行期（违反「架构声明零运行时开销」）。
pub const E_RUNTIME_OVERHEAD_DECLARED: &str = "E_RUNTIME_OVERHEAD_DECLARED";
/// 主题落点缺失。
pub const E_THEME_LANDING_MISSING: &str = "E_THEME_LANDING_MISSING";
/// 主题落点为空。
pub const E_THEME_LANDING_EMPTY: &str = "E_THEME_LANDING_EMPTY";
/// 主题落点条目号格式非法。
pub const E_THEME_LANDING_MALFORMED: &str = "E_THEME_LANDING_MALFORMED";
/// 主题主责组不一致。
pub const E_THEME_GROUP_MISMATCH: &str = "E_THEME_GROUP_MISMATCH";
/// 主题落点数超限。
pub const E_THEME_LANDING_OVERFLOW: &str = "E_THEME_LANDING_OVERFLOW";
/// 支撑轴无服务对象（空轴）。
pub const E_AXIS_SERVES_NONE: &str = "E_AXIS_SERVES_NONE";
/// 支撑轴服务不足（少于 must_serve 下限）。
pub const E_AXIS_SERVES_SHORT: &str = "E_AXIS_SERVES_SHORT";
/// 运行时能力无属主。
pub const E_CAPABILITY_NO_OWNER: &str = "E_CAPABILITY_NO_OWNER";
/// 运行时能力认领越界（不归 P 域）。
pub const E_RUNTIME_CAPABILITY_NOT_OWNED: &str = "E_RUNTIME_CAPABILITY_NOT_OWNED";
/// 运行时能力分歧（对拍拦截）。
pub const E_RUNTIME_CAPABILITY_DIVERGENCE: &str = "E_RUNTIME_CAPABILITY_DIVERGENCE";
/// 运行时语义漂移。
pub const E_RUNTIME_SEMANTICS_DRIFT: &str = "E_RUNTIME_SEMANTICS_DRIFT";
/// 探针句柄缺失。
pub const E_PROBE_HANDLE_MISSING: &str = "E_PROBE_HANDLE_MISSING";
/// 探针对账账已满。
pub const E_PROBE_LEDGER_FULL: &str = "E_PROBE_LEDGER_FULL";
/// 声明层能力无理由。
pub const E_DECLARATION_NO_WHY: &str = "E_DECLARATION_NO_WHY";
/// 帧采样账已满。
pub const E_FRAME_LEDGER_FULL: &str = "E_FRAME_LEDGER_FULL";
/// 活动实例账已满。
pub const E_INSTANCE_LEDGER_FULL: &str = "E_INSTANCE_LEDGER_FULL";
/// 豁免无理由。
pub const E_EXEMPTION_NO_REASON: &str = "E_EXEMPTION_NO_REASON";
/// 豁免无期限。
pub const E_EXEMPTION_NO_DEADLINE: &str = "E_EXEMPTION_NO_DEADLINE";
/// 豁免同类重复。
pub const E_EXEMPTION_DUP: &str = "E_EXEMPTION_DUP";
/// 豁免登记表已满。
pub const E_EXEMPTION_CAP: &str = "E_EXEMPTION_CAP";
/// 契约为空。
pub const E_EMPTY_CONTRACT: &str = "E_EMPTY_CONTRACT";
/// 对端重复登记。
pub const E_PEER_DUP: &str = "E_PEER_DUP";
/// 对端未登记。
pub const E_PEER_UNKNOWN: &str = "E_PEER_UNKNOWN";
/// 契约账已满。
pub const E_PEER_CAP: &str = "E_PEER_CAP";
/// 哈希不一致。
pub const E_HASH_MISMATCH: &str = "E_HASH_MISMATCH";
/// 阻断级对端未接收。
pub const E_BLOCKING_NOT_RECEIVED: &str = "E_BLOCKING_NOT_RECEIVED";
/// 阻断级对端未对账。
pub const E_BLOCKING_NOT_RECONCILED: &str = "E_BLOCKING_NOT_RECONCILED";
/// 阻断级对端一个都没登记。
pub const E_NO_BLOCKING_PEER_DECLARED: &str = "E_NO_BLOCKING_PEER_DECLARED";
/// 越界（命中禁扩面）。
pub const E_BOUNDARY_OVERREACH: &str = "E_BOUNDARY_OVERREACH";
/// 降级链缺档。
pub const E_CHAIN_INCOMPLETE: &str = "E_CHAIN_INCOMPLETE";
/// 降级链档序错。
pub const E_CHAIN_ORDER: &str = "E_CHAIN_ORDER";
/// 降级链档位重复。
pub const E_CHAIN_DUP: &str = "E_CHAIN_DUP";
/// 面目全非档缺等效性验收。
pub const E_CHAIN_EQUIV_MISSING: &str = "E_CHAIN_EQUIV_MISSING";
/// ADR 无标题。
pub const E_ADR_NO_TITLE: &str = "E_ADR_NO_TITLE";
/// ADR 无决策内容。
pub const E_ADR_NO_DECISION: &str = "E_ADR_NO_DECISION";
/// ADR 无否决记录。
pub const E_ADR_NO_REJECTED: &str = "E_ADR_NO_REJECTED";
/// ADR 无新版本号。
pub const E_ADR_NO_TO_VERSION: &str = "E_ADR_NO_TO_VERSION";
/// ADR 升版信息矛盾。
pub const E_ADR_BUMP_NO_FROM: &str = "E_ADR_BUMP_NO_FROM";
/// ADR 账已满。
pub const E_ADR_CAP: &str = "E_ADR_CAP";
/// 接口版本与冻结版本不符。
pub const E_INTERFACE_VERSION_DRIFT: &str = "E_INTERFACE_VERSION_DRIFT";
/// 组序非严格递增。
pub const E_GROUP_ORDER: &str = "E_GROUP_ORDER";
/// 支撑轴声明了未登记的主责条目。
pub const E_AXIS_OWNER_MISMATCH: &str = "E_AXIS_OWNER_MISMATCH";

/// P 域错误（五元组：码/现象/原因/下一步/责任方）。
///
/// **拒绝必须给出路**：五元组齐发是构造点强制，`next` 为空即视为不合格——
/// 只说「不行」而不说「那该怎么做」的拒绝，会让人换个写法再来一遍。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionError {
    /// 错误码。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
    /// 下一步（必填——拒绝必须给出路）。
    ///
    /// 取`String` 而非 `&'static str`：下一步往往要指名"该找哪个域/改哪个常量"，
    /// 这类动态指引用静态串写不出来，强行静态化只会逼着人写成"见文档"这种废话。
    pub next: String,
    /// 责任方。
    pub who: String,
}

impl MotionError {
    /// 构造（五元组齐发，构造点强制写全）。
    pub fn new(
        code: &'static str,
        what: &'static str,
        why: &str,
        next: &str,
        who: &str,
    ) -> Self {
        MotionError {
            code,
            what,
            why: why.to_string(),
            next: next.to_string(),
            who: who.to_string(),
        }
    }

    /// 五元组齐备性自检（`next` 为空即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.code.trim().is_empty()
            && !self.what.trim().is_empty()
            && !self.why.trim().is_empty()
            && !self.next.trim().is_empty()
            && !self.who.trim().is_empty()
    }

    /// 读屏可读的完整错误（三要素齐发：现象/原因/怎么办）。
    pub fn screen_text(&self) -> String {
        format!(
            "错误 {}：{}；原因：{}；下一步：{}；责任方：{}",
            self.code, self.what, self.why, self.next, self.who
        )
    }
}

// ---------------------------------------------------------------------------
// 十三、下游归属表（防止抢活与漏活）
// ---------------------------------------------------------------------------

/// 下游条目归属（**谁拥有什么**——本项只立总纲，不代做后续 19 项）。
///
/// 这张表的作用是**防止抢活**：域开工最常见的失败不是做不出来，而是总纲
/// 顺手把下游的活也干了，然后下游条目开工时发现"已经有人做过了，但没人
/// 知道在哪、依据是什么"。
pub const DOWNSTREAM_OWNERSHIP: [(&str, &str); 10] = [
    ("VE-F3002", "动效设计语言总纲：四原则、时长分级表、缓动家族全表与命名登记"),
    ("VE-F3003", "动效令牌体系：三族令牌全集、CSS 变量注入器、硬编码 lint 规则集"),
    ("VE-F3005", "转场编排器：编排图数据结构、四原语编译器、打断策略表"),
    ("VE-F3006", "动效组件化：四件套组件定义、三族注册表、参数校验器与沙箱边界"),
    ("VE-F3010", "共享元素转场：pair 配对表、四维几何插值器、飞行体管理器"),
    ("VE-F3012", "手势驱动动效：三型映射曲线、阈值迟滞表、速度采样器"),
    ("VE-F3015", "动效物理引擎：三档参数预设、固定步长求解器、采样缓存与终止器"),
    ("VE-F3016", "动效性能分级与预算：L1-L4 分级表、预算表、P 段四子段归因"),
    ("VE-F3017", "动效无障碍总纲：五类覆盖矩阵逐格直达行为、豁免登记簿、冗余检查"),
    ("VE-F3018", "动效调试器与预览工作台：预览引擎、时间线视图模型、A/B 对比器"),
];

// ---------------------------------------------------------------------------
// 十四、域开工总纲本体
// ---------------------------------------------------------------------------

/// 契约问题（五元组：码/现象/根因/建议/严重度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractIssue {
    /// 问题码。
    pub code: &'static str,
    /// 现象。
    pub symptom: String,
    /// 根因。
    pub root_cause: String,
    /// 建议。
    pub advice: &'static str,
    /// 严重度。
    pub severity: Severity,
}

/// 严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// 阻断：不修不得开工。
    Blocking,
    /// 警告：可开工但须限期修。
    Warning,
}

impl Severity {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Severity::Blocking => "阻断",
            Severity::Warning => "警告",
        }
    }
}

impl ContractIssue {
    /// 读屏单行（问题要能念给用户听——异常零静默）。
    pub fn screen_line(&self) -> String {
        format!(
            "契约问题[{}·{}]：{}；根因 {}；建议 {}",
            self.severity.zh(),
            self.code,
            self.symptom,
            self.root_cause,
            self.advice
        )
    }
}

/// 主题落点（主题 → 条目号列表 + 落点所属组）。
#[derive(Clone, Debug)]
pub struct ThemeLanding {
    /// 主题。
    pub theme: MotionTheme,
    /// 落点所属组（须与 [`MotionTheme::primary_group`] 一致）。
    pub group: MotionGroup,
    /// 落点条目号列表（格式须为 `VE-Fxxxx`）。
    pub items: Vec<String>,
}

/// 支撑轴服务登记（轴 → 服务的主题集）。
#[derive(Clone, Debug)]
pub struct AxisService {
    /// 支撑轴。
    pub axis: SupportAxis,
    /// 该轴服务的主题。
    pub serves: Vec<MotionTheme>,
}

/// P 域开工总纲（本项交付物本体）。
#[derive(Clone, Debug)]
pub struct MotionArchitecture {
    /// 总纲版本。
    pub version: &'static str,
    /// 段间接口冻结版本。
    pub interface_version: &'static str,
    /// 十项主题落点表。
    pub landings: Vec<ThemeLanding>,
    /// 五支撑轴服务表。
    pub axes: Vec<AxisService>,
    /// 单源分工归属。
    pub division: RuntimeDivision,
    /// 能力探针对拍账。
    pub probes: ProbeLedger,
    /// 对端消费契约账。
    pub peers: PeerLedger,
    /// 三底线判定账。
    pub bottom_lines: BottomLineLedger,
    /// 第一红线策略。
    pub policy: ReducedMotionPolicy,
    /// 降级链（前向声明）。
    pub chain: DegradeChain,
    /// ADR 账。
    pub adrs: AdrLedger,
}

impl MotionArchitecture {
    /// 标准总纲（十项落点齐备 + 五轴服务齐备 + 降级链前向声明）。
    ///
    /// **注意这里没有伪造"已接收契约"**：标准态的阻断级对端是未接收的，
    /// 所以 [`MotionArchitecture::preflight`] 会报缺口——这是诚实的，
    /// 总纲交付不等于契约已签署。
    pub fn standard() -> Self {
        MotionArchitecture {
            version: ARCH_VERSION,
            interface_version: INTERFACE_VERSION,
            landings: standard_landings(),
            axes: standard_axes(),
            division: RuntimeDivision::standard(),
            probes: ProbeLedger::new(),
            peers: PeerLedger::new(),
            bottom_lines: BottomLineLedger::new(),
            policy: ReducedMotionPolicy::new(),
            chain: DegradeChain::forward(),
            adrs: AdrLedger::new(),
        }
    }

    /// 主题落点定位（复杂度 C2：**O(1)**）。
    ///
    /// 按 [`MotionTheme::rank`] 直接索引，不做线性查表。返回
    /// `Ok(None)` 表示「查无此项」，`Ok(Some(empty))` 表示「查得此项但落点为空」
    /// ——**两种故障必须可区分**，否则空落点会被当成"没这项"而被放过。
    pub fn theme_landing(&self, theme: MotionTheme) -> Result<Option<&ThemeLanding>, MotionError> {
        match self.landings.get(theme.rank()) {
            Some(l) if l.theme == theme => Ok(Some(l)),
            Some(_) => Err(MotionError::new(
                E_THEME_LANDING_MALFORMED,
                "落点定位被拒：索引位主题不符",
                &format!(
                    "rank {} 位上的主题是 {}，查的是 {}；索引与主题不同步",
                    theme.rank(),
                    self.landings[theme.rank()].theme.zh(),
                    theme.zh()
                ),
                "重建落点表时保持 rank 与枚举一一对应（用 theme_landings() 构造）",
                "P 域架构维护方",
            )),
            None => Ok(None),
        }
    }

    /// 十项映射审计（复杂度 C3：O(主题数 × 落点数)）。
    ///
    /// 五条判定：无缺（十项每项都在表）、无空（落点条目号非空）、
    /// 格式合法（条目号须 `VE-F` 前缀）、组一致、落点数不超上限。
    pub fn audit_themes(&self) -> ThemeAudit {
        let mut audit = ThemeAudit::default();
        for t in MotionTheme::ALL.iter() {
            match self.theme_landing(*t) {
                Ok(None) => audit.missing.push(*t),
                Ok(Some(l)) => {
                    if l.items.is_empty() {
                        audit.empty.push(*t);
                        continue;
                    }
                    if l.items.len() > MAX_LANDINGS_PER_THEME {
                        audit.overflow.push(*t);
                    }
                    if l.items.iter().any(|i| !is_valid_item_id(i)) {
                        audit.malformed.push(*t);
                    }
                    if l.group != t.primary_group() {
                        audit.group_mismatch.push(*t);
                    }
                }
                Err(_) => audit.malformed.push(*t),
            }
        }
        audit
    }

    /// 支撑轴服务自检（复杂度 C5：O(轴数 × 主题数)）。
    ///
    /// 两条判定：非空轴（至少服务一项）、服务达标（不少于 [`SupportAxis::must_serve`]
    /// 的下限）、主责条目与轴表一致。
    pub fn check_axes_served(&self) -> Vec<&'static str> {
        let mut issues = Vec::new();
        for a in SupportAxis::ALL.iter() {
            let Some(rec) = self.axes.iter().find(|s| s.axis == *a) else {
                issues.push(E_AXIS_SERVES_NONE);
                continue;
            };
            if rec.serves.is_empty() {
                issues.push(E_AXIS_SERVES_NONE);
            }
            let need = a.must_serve().len();
            if rec.serves.len() < need {
                issues.push(E_AXIS_SERVES_SHORT);
            }
            // 服务的主题必须在十项之内（不存在"服务了一个不存在的主题"）。
            if rec.serves.iter().any(|t| !MotionTheme::ALL.contains(t)) {
                issues.push(E_AXIS_SERVES_NONE);
            }
        }
        if self.axes.len() != AXIS_COUNT {
            issues.push(E_AXIS_SERVES_NONE);
        }
        issues
    }

    /// 段契约自检（复杂度 C1：**O(签名数)**）。
    ///
    /// 逐段核五件事：七项齐备、段序严格递增、段码唯一、成本形态为声明期、
    /// 组数与段数在上界内。**成本形态这一条是判据「架构声明零运行时开销」的
    /// 执行点**——任何段被标 `PerFrame` 即 [`E_RUNTIME_OVERHEAD_DECLARED`]，
    /// 因为 P 域一旦在帧路径上做事，帧预算就不再是 M 域一家的事。
    pub fn check_contracts(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();

        if self.interface_version != INTERFACE_VERSION {
            issues.push(ContractIssue {
                code: E_INTERFACE_VERSION_DRIFT,
                symptom: format!(
                    "接口冻结版本为 {}，与总纲声明的 {} 不一致",
                    self.interface_version, INTERFACE_VERSION
                ),
                root_cause: "接口版本被就地改动而未升总纲版本".to_string(),
                advice: "按 ADR 升接口版本并同步 INTERFACE_VERSION 常量",
                severity: Severity::Blocking,
            });
        }

        if GROUP_ORDER.len() != GROUP_COUNT {
            issues.push(ContractIssue {
                code: E_GROUP_CAP,
                symptom: format!("组数 {} 与声明的 {} 不符", GROUP_ORDER.len(), GROUP_COUNT),
                root_cause: "组枚举被增删而未同步常量".to_string(),
                advice: "同步 GROUP_COUNT 或恢复组枚举",
                severity: Severity::Blocking,
            });
        }

        // 组序须严格递增（与 rank() 一致）。
        let mut last_group: Option<u8> = None;
        for g in GROUP_ORDER.iter() {
            if let Some(l) = last_group {
                if g.rank() <= l {
                    issues.push(ContractIssue {
                        code: E_GROUP_ORDER,
                        symptom: format!("组 {} 出现在组位 {} 之后，组序非递增", g.zh(), g.rank()),
                        root_cause: "GROUP_ORDER 被手工重排而未改 rank()".to_string(),
                        advice: "以MotionGroup::ALL 为唯一组序源，重建 GROUP_ORDER",
                        severity: Severity::Blocking,
                    });
                    break;
                }
            }
            last_group = Some(g.rank());
        }

        // 逐组逐段核契约。
        let mut seen_codes: Vec<&'static str> = Vec::new();
        for g in GROUP_ORDER.iter() {
            let segs = g.segments();
            if segs.len() > MAX_SEGMENTS_PER_GROUP {
                issues.push(ContractIssue {
                    code: E_SEGMENT_CAP,
                    symptom: format!(
                        "{} 有 {} 段，超过上界 {}",
                        g.zh(),
                        segs.len(),
                        MAX_SEGMENTS_PER_GROUP
                    ),
                    root_cause: "段数无上界会让「契约核验 O(签名数)」不再是常量".to_string(),
                    advice: "拆分该组或按 ADR 提升 MAX_SEGMENTS_PER_GROUP 并复核算法规格",
                    severity: Severity::Blocking,
                });
            }
            let mut last_rank: Option<u8> = None;
            for s in segs.iter() {
                if !s.is_complete() {
                    issues.push(ContractIssue {
                        code: E_SEGMENT_INCOMPLETE,
                        symptom: format!("段 {} 契约七项不齐", s.code),
                        root_cause: "段契约缺项（多半是漏写「不做清单」）".to_string(),
                        advice: "补齐段契约七项；不做清单不许留空",
                        severity: Severity::Blocking,
                    });
                }
                if let Some(l) = last_rank {
                    if s.rank <= l {
                        issues.push(ContractIssue {
                            code: E_SEGMENT_ORDER,
                            symptom: format!("段 {}（位 {}）出现在段位 {} 之后", s.code, s.rank, l),
                            root_cause: "段序被改而未改段位".to_string(),
                            advice: "以「越便宜越靠前」重排段序并同步 rank",
                            severity: Severity::Blocking,
                        });
                    }
                }
                last_rank = Some(s.rank);
                if seen_codes.contains(&s.code) {
                    issues.push(ContractIssue {
                        code: E_SEGMENT_DUP,
                        symptom: format!("段码 {} 重复", s.code),
                        root_cause: "复制段表时未改段码".to_string(),
                        advice: "段码全局唯一，改码或合并重复段",
                        severity: Severity::Blocking,
                    });
                }
                seen_codes.push(s.code);
                if !s.cost.is_zero_overhead() {
                    issues.push(ContractIssue {
                        code: E_RUNTIME_OVERHEAD_DECLARED,
                        symptom: format!("段 {}（{}）被标为运行期", s.code, s.duty_zh),
                        root_cause: "把运行期工作写进了 P 域声明层段".to_string(),
                        advice: "把该工作移出P 域（多半属 M 域），或改标为委派请求",
                        severity: Severity::Blocking,
                    });
                }
            }
        }
        issues
    }

    /// 降级链前向核验代理（复杂度 C9：O(档数)）。
    pub fn check_chain_forward(&self) -> Vec<&'static str> {
        self.chain.check_forward()
    }

    /// 运行时开销核验（判据「架构声明零运行时开销」）。
    ///
    /// 返回**逐段成本形态列表**而不是一句"零"——只有把每一段都摊开，
    /// "零开销"才是可核的结论而不是一句承诺。
    pub fn runtime_cost(&self) -> Vec<(&'static str, SegmentCost)> {
        let mut out = Vec::new();
        for g in GROUP_ORDER.iter() {
            for s in g.segments().iter() {
                out.push((s.code, s.cost));
            }
        }
        out
    }

    /// 是否全部段零开销。
    pub fn is_zero_runtime_cost(&self) -> bool {
        self.runtime_cost()
            .iter()
            .all(|(_, c)| c.is_zero_overhead())
    }

    /// 开工前置检查（**任一缺口即不得开工**）。
    ///
    /// 汇集五处：契约自检、阻断级对端、能力对拍完备、三底线有数据、
    /// 降级链前向。**全绿才叫可开工**——总纲交付与可开工是两件事。
    pub fn preflight(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        for i in self.check_contracts() {
            missing.push(i.code);
        }
        if !self.audit_themes().is_complete() {
            missing.push(E_THEME_LANDING_MISSING);
        }
        missing.extend(self.check_axes_served());
        missing.extend(self.peers.check_blocking_ready());
        missing.extend(self.division.audit_owners());
        if !self.probes.is_complete() {
            missing.push(E_RUNTIME_CAPABILITY_DIVERGENCE);
        }
        for l in BOTTOM_LINE_ORDER.iter() {
            if !self.bottom_lines.verdict(*l).is_pass() {
                missing.push(match l {
                    BottomLine::FrameAudit => E_FRAME_LEDGER_FULL,
                    BottomLine::InterruptAudit => E_INSTANCE_LEDGER_FULL,
                    BottomLine::CoverageAudit => E_INSTANCE_LEDGER_FULL,
                });
            }
        }
        missing.extend(self.check_chain_forward());
        if self.adrs.incomplete_count() > 0 {
            missing.push(E_ADR_NO_DECISION);
        }
        missing
    }

    /// 域级自检（复杂度 C12：一次跑完全部判据）。
    pub fn self_audit(&self) -> Vec<ContractIssue> {
        let mut issues = self.check_contracts();
        let audit = self.audit_themes();
        if !audit.missing.is_empty() {
            issues.push(ContractIssue {
                code: E_THEME_LANDING_MISSING,
                symptom: format!("十项映射缺 {} 项", audit.missing.len()),
                root_cause: "落点表未覆盖全部十项".to_string(),
                advice: "补齐缺失主题的落点（用 theme_landings() 构造）",
                severity: Severity::Blocking,
            });
        }
        if !audit.empty.is_empty() {
            issues.push(ContractIssue {
                code: E_THEME_LANDING_EMPTY,
                symptom: format!("{} 项主题落点为空", audit.empty.len()),
                root_cause: "落点条目存在但条目号列表为空".to_string(),
                advice: "填入 VE-Fxxxx 条目号；确实无条目的主题应从十项中移出并走 ADR",
                severity: Severity::Blocking,
            });
        }
        if !audit.malformed.is_empty() {
            issues.push(ContractIssue {
                code: E_THEME_LANDING_MALFORMED,
                symptom: format!("{} 项主题落点条目号格式非法", audit.malformed.len()),
                root_cause: "条目号不是 VE-Fxxxx 形式".to_string(),
                advice: "统一改为 VE-Fxxxx 形式，便于台账机检",
                severity: Severity::Blocking,
            });
        }
        if !audit.group_mismatch.is_empty() {
            issues.push(ContractIssue {
                code: E_THEME_GROUP_MISMATCH,
                symptom: format!("{} 项主题落点组与主责组不一致", audit.group_mismatch.len()),
                root_cause: "落点挂到了不管它的组上（归属反查会失真）".to_string(),
                advice: "把落点改挂到 MotionTheme::primary_group() 指定的组",
                severity: Severity::Blocking,
            });
        }
        if !audit.overflow.is_empty() {
            issues.push(ContractIssue {
                code: E_THEME_LANDING_OVERFLOW,
                symptom: format!("{} 项主题落点数超上限", audit.overflow.len()),
                root_cause: "单主题落点膨胀".to_string(),
                advice: "拆分落点或按 ADR 提升 MAX_LANDINGS_PER_THEME",
                severity: Severity::Warning,
            });
        }
        if !check_anchor_label_coverage().is_complete() {
            issues.push(ContractIssue {
                code: E_AXIS_OWNER_MISMATCH,
                symptom: check_anchor_label_coverage().screen_text(),
                root_cause: "锚点十五标签未逐项对齐（缺项或自造项）".to_string(),
                advice: "补齐缺失标签或删除自造标签；两轴合计必须等于十五",
                severity: Severity::Blocking,
            });
        }
        for code in self.check_axes_served() {
            issues.push(ContractIssue {
                code,
                symptom: "支撑轴服务登记不合规".to_string(),
                root_cause: "轴未登记/ 空轴 / 服务不足 / 服务了不存在的主题".to_string(),
                advice: "按SupportAxis::must_serve() 补齐服务对象",
                severity: Severity::Blocking,
            });
        }
        for code in self.peers.check_blocking_ready() {
            issues.push(ContractIssue {
                code,
                symptom: "阻断级对端契约未齐备".to_string(),
                root_cause: "M/O04 契约未接收或未对账".to_string(),
                advice: "先register 再 reconcile；不接受先开工后补签",
                severity: Severity::Blocking,
            });
        }
        if !self.probes.is_complete() {
            issues.push(ContractIssue {
                code: E_RUNTIME_CAPABILITY_DIVERGENCE,
                symptom: self.probes.screen_text(),
                root_cause: "六项运行时能力未全部对拍通过".to_string(),
                advice: "对 VE-M 六项能力逐项跑探针；分歧须拦截不得降级掩盖",
                severity: Severity::Blocking,
            });
        }
        for code in self.check_chain_forward() {
            issues.push(ContractIssue {
                code,
                symptom: "降级链前向核验未过".to_string(),
                root_cause: "档不齐/ 档序错 / 面目全非档缺等效性验收".to_string(),
                advice: "按 DegradeChain::forward() 重建并如实标注 landed",
                severity: Severity::Blocking,
            });
        }
        if !self.is_zero_runtime_cost() {
            issues.push(ContractIssue {
                code: E_RUNTIME_OVERHEAD_DECLARED,
                symptom: "存在被标为运行期的段".to_string(),
                root_cause: "P 域声明层承担了帧路径工作".to_string(),
                advice: "把帧路径工作移交 M 域，P 域只留委派请求",
                severity: Severity::Blocking,
            });
        }
        issues
    }

    /// 读屏替述（无障碍替述：把总纲讲成人话，且**覆盖全部六项判据**）。
    ///
    /// 替述与总纲同源生成——另写一份的风险是漂移，而漂移的无障碍文档比没有
    /// 更坏：它会让用户以为已经有保障。
    pub fn architecture_narration(&self, frame: u64) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "VE-P 域动效与体验库总架构，版本 {}，段间接口冻结 {}。\n",
            self.version, self.interface_version
        ));
        s.push_str("判据共六项：");
        for c in CRITERIA.iter() {
            s.push_str(&format!("{}（{}）；", c.zh(), c.code()));
        }
        s.push_str("\n三组架构：");
        for g in GROUP_ORDER.iter() {
            s.push_str(&format!(
                "{}（{}）共 {} 段；",
                g.zh(),
                g.code(),
                g.segments().len()
            ));
        }
        s.push_str(&format!(
            "\n全域{} 段、{} 项签名，逐段成本形态均为声明期：零运行时开销。\n",
            total_segment_count(),
            total_signature_count()
        ));
        s.push_str("十项主题映射：");
        for t in MotionTheme::ALL.iter() {
            let landings = self
                .theme_landing(*t)
                .ok()
                .flatten()
                .map(|l| l.items.len())
                .unwrap_or(0);
            s.push_str(&format!("{}（{}，落点 {} 条）；", t.zh(), t.code(), landings));
        }
        s.push_str(&format!(
            "\n五支撑轴：{}；合计与锚点十五标签对齐：{}\n",
            SupportAxis::ALL.len(),
            if check_anchor_label_coverage().is_complete() {
                "逐项对齐无遗漏".to_string()
            } else {
                check_anchor_label_coverage().screen_text()
            }
        ));
        // 轴逐项念出（含服务对象）——只报"五轴"而不点名，读屏用户等于没听到。
        for x in SupportAxis::ALL.iter() {
            let served = self
                .axes
                .iter()
                .find(|r| r.axis == *x)
                .map(|r| r.serves.len())
                .unwrap_or(0);
            s.push_str(&format!(
                "{}（{}，主责 {}，服务 {} 项主题）；",
                x.zh(),
                x.code(),
                x.owner_item(),
                served
            ));
        }
        s.push('\n');
        s.push_str(&format!("{}\n", self.division.screen_text()));
        s.push_str(&format!("{}\n", self.peers.screen_text()));
        s.push_str(&format!("{}\n", self.bottom_lines.screen_text()));
        s.push_str(&format!("{}\n", self.policy.screen_text(frame)));
        s.push_str(&format!("{}\n", self.chain.screen_text()));
        s.push_str(&format!("{}\n", self.adrs.screen_text()));
        s.push_str("验收三底线：");
        for l in BOTTOM_LINE_ORDER.iter() {
            s.push_str(&format!(
                "{}——{}；",
                l.zh(),
                l.promise()
            ));
        }
        s.push('\n');
        s.push_str(&format!(
            "禁扩面 {} 条：",
            BOUNDARY_EXCLUSIONS.len()
        ));
        for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
            s.push_str(&format!("{}（{}）；", code, desc));
        }
        s.push('\n');
        s
    }
}

/// 条目号格式校验（`VE-F` + 四位数字）。
pub fn is_valid_item_id(item: &str) -> bool {
    let Some(rest) = item.strip_prefix("VE-F") else {
        return false;
    };
    rest.len() == 4 && rest.bytes().all(|b| b.is_ascii_digit())
}

/// 标准十项落点表（**十项 10/10 硬门的正样本**）。
///
/// 每项落点都指向**真实的册内条目号**，且落点组与 [`MotionTheme::primary_group`]
/// 一致——这两条都由 [`MotionArchitecture::audit_themes`] 逐项机检。
pub fn standard_landings() -> Vec<ThemeLanding> {
    let seeds: [(MotionTheme, &[&str]); THEME_COUNT] = [
        (MotionTheme::Language, &["VE-F3002"]),
        (MotionTheme::Token, &["VE-F3003"]),
        (
            MotionTheme::Orchestration,
            &["VE-F3005", "VE-F3007", "VE-F3031"],
        ),
        (MotionTheme::Composition, &["VE-F3006"]),
        (
            MotionTheme::EnterExit,
            &["VE-F3008", "VE-F3009", "VE-F3027"],
        ),
        (MotionTheme::SharedElement, &["VE-F3010", "VE-F3027"]),
        (MotionTheme::Flip, &["VE-F3011"]),
        (MotionTheme::Gesture, &["VE-F3012", "VE-F3028"]),
        (
            MotionTheme::Scroll,
            &["VE-F3013", "VE-F2726"],
        ),
        (MotionTheme::MicroFeedback, &["VE-F3014"]),
    ];
    seeds
        .iter()
        .map(|(theme, items)| ThemeLanding {
            theme: *theme,
            group: theme.primary_group(),
            items: items.iter().map(|s| (*s).to_string()).collect(),
        })
        .collect()
}

/// 标准五轴服务表（**每轴非空且达标**）。
pub fn standard_axes() -> Vec<AxisService> {
    SupportAxis::ALL
        .iter()
        .map(|a| AxisService {
            axis: *a,
            // 标准态服务 must_serve 的全集（不夸大也不缩小）。
            serves: a.must_serve().to_vec(),
        })
        .collect()
}

/// VE-F3001 域自检（判据逐条映射见 `vep01_checks.rs`）。
pub fn run_vep01_checks() -> CheckSet {
    super::vep01_checks::run_vep01_checks()
}