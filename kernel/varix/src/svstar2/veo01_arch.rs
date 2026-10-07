//! VE-F2801 · O 域开工与样式引擎总体架构（VE-O 域 · CSS/HTML 表面域 · O01 组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2801`
//!
//! **判据（锚点原文）**：O01 架构声明、集成边界、解析子集、判据。
//!
//! **职责定位（锚点原文）**：O 域开工——域使命声明（CSS/HTML 表面声明 UI，
//! 样式引擎编译为 N 控件树 + D 绘制指令，Web 表达力原生性能）；官方十主题范围；
//! Servo 集成决策。
//!
//! **数据结构（锚点原文·家族格式）**：数据模型与规格表（逐条规格公开、参数域
//! 钳制、枚举守卫）。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：非法输入→校验拒绝三要素；
//! 边界越界→钳制 + 告警；异常检出→立案流转。
//!
//! **性能逐项分解（锚点原文·家族格式）**：核心逻辑 O(1)-O(logN)（实测定标入域
//! 账本）；预算联动次序单源。
//!
//! **跨批对接点（锚点原文·家族格式）**：上游契约接收（哈希对账）；下游消费接口
//! （前向声明）；跨域衔接对账钩子（复用方义务）。
//!
//! **无障碍与隐私（锚点原文）**：文档替述可读；无隐私面。
//!
//! # 本项的边界（不越界施工，遵守"只做领到的任务"）
//!
//! VE-F2801 是**域开工与总架构**——它交付：七段管线的段级契约、Servo 集成边界
//! 的可执行判定、解析子集的**表结构与门禁语义**、十主题落点映射、预算次序单源、
//! 风险登记。它**不代做**后续 19 项的引擎本体。分工在册（见
//! [`DOWNSTREAM_OWNERSHIP`]）：
//!
//! - F2802 Servo 组件选型与集成边界拥有 **vendor 化落地与 MPL-2.0 合规文本**；
//!   本项只立"引入/不引入"的判定契约与禁扩面表，不做 crate 落地；
//! - F2803 样式解析子集范围声明拥有**四族属性的完整清单**；本项只立
//!   [`PropertySpec`] 的表结构、四族分桶与 [`SubsetGate`] 的门禁语义，并给出
//!   代表性种子集（架构自举所需，不是全表）；
//! - F2804 CSS 词法器与分词管线拥有**tokenizer 本体**；本项的 [`Stage::Tokenize`]
//!   只声明"入段是 token 流、出段是 token 流"的段契约与失败策略；
//! - F2805 声明解析与属性表注册拥有**PropertyId→解析函数注册表**；本项不写解析函数；
//! - F2809 样式失效与重计算调度拥有**失效源六类与 Bloom 过滤**；本项只把
//!   [`Stage::Invalidate`] 立为管线上的**独立段位**，保证"预算次序单源"覆盖它；
//! - F2816 样式性能预算与缓存拥有**三层缓存与命中率遥测**；本项只立预算账本与
//!   次序强制点，不建缓存；
//! - F2817 与 VE-N UI 框架对接协议拥有**投影协议本体**；本项只立"样式引擎产出
//!   N 控件树属性、D 绘制指令"这条单向承诺与哈希对账钩子位。
//!
//! # 设计要点（每条都是可执行的，不是标签）
//!
//! - **七段管线不是七个名字**：[`Stage`] 七段各有 [`StageSpec`]（吃/吐/失败
//!   策略/复杂度/消费方/不做清单），段契约断链由 [`Architecture::check_contracts`]
//!   检出——总纲自己先做到可追溯，否则凭什么要求别人；
//! - **"编译为 N 控件树 + D 绘制指令"是单向承诺**：样式引擎**只向** N 域吐属性、
//!   **只向** D 域吐绘制指令，**永不反向**读 N/D 的内部结构。承诺由
//!   [`IntegrationBoundary::check_no_overreach`] 逐条断言，反向依赖企图直接
//!   [`E_BOUNDARY_OVERREACH`]；
//! - **Servo 决策不是口头选择**：[`ServoDecision`] 把"引入 style/style_traits、
//!   **不引入 layout**"落成 [`IntegrationBoundary`] 的两条可查断言。layout 由
//!   N 域承担是**归属裁决**，不是"暂时没引入"——引入即越界；
//! - **预算联动次序单源是硬门**：[`BudgetLedger::register`] 只接受**按段位序递增**
//!   的注册（`stage_rank` 不得小于已注册的最大值），跳序注册 [`E_BUDGET_ORDER`]。
//!   "次序单源"的意思是：段的顺序在本模块里**只有一处真值**（[`STAGE_ORDER`]），
//!   预算、依赖、失效位全部由它派生，不允许第二处顺序声明；
//! - **参数域钳制必产告警**：[`clamp_u32`] / [`clamp_f64`] 越界不静默夹取，
//!   每次夹取都往 [`Architecture`] 的告警账里落一条 [`ClampNotice`]，消费方能
//!   回答"这个值为什么不是我写的那个"；
//! - **枚举守卫靠往返**：[`Stage`] / [`PropertyFamily`] / [`ServoCrate`] 全有
//!   `code()` ↔ `from_code()` 往返，未知码 [`E_UNKNOWN_ENUM`] 显性拒绝——
//!   枚举值从配置文件或 FFI 进来时，不许"猜一个近似的"；
//! - **异常检出→立案流转**：[`RiskRegister`] 与 [`CaseLedger`] 承接检出的异常，
//!   产出 [`CaseRecord`]（五要素：现象/影响/定位/处置/状态），流转终态是
//!   `Closed`（已裁决）而非静默消失。
//!
//! **零外部依赖**，只依赖 `crate::checks`（自检侧）与 `alloc`。
//! 确定性：逻辑 tick 注入、零墙钟、零 IO，回归可复现（对拍红线）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、总纲常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 总纲版本。契约变更走版本号，破坏性变更必须升版并留迁移说明。
pub const ARCH_VERSION: &str = "O01-arch-v1";

/// 段数上限（判据"七段管线"的硬约束）。多于七段即契约被扩，改动须走 ADR。
pub const STAGE_COUNT: usize = 7;

/// 官方主题数上限（判据"官方十主题范围"）。十项，不多不少。
pub const THEME_COUNT: usize = 10;

/// 解析子集属性族数上限（布局/视觉/效果/交互四族——家族格式）。
pub const PROPERTY_FAMILY_COUNT: usize = 4;

/// 单族属性条数上限。超限即"子集"变成了"全表"，那不是子集。
pub const MAX_PROPERTIES_PER_FAMILY: usize = 64;

/// 风险登记条数上限。
pub const MAX_RISKS: usize = 64;

/// 案件（异常检出→立案流转）条数上限。
pub const MAX_CASES: usize = 128;

/// 告警账条数上限。满后拒绝并计数——告警静默丢弃等于钳制失效。
pub const CLAMP_LOG_CAP: usize = 256;

/// 排除清单条数上限（"本域明确不管什么"也是契约）。
pub const MAX_EXCLUSIONS: usize = 32;

/// 复杂度声明（人读文本）。实现与本表逐条对应，改动必须两处同步走 ADR。
pub const COMPLEXITY_DOC: &str = "\
O 域样式引擎总架构复杂度声明（VE-F2801 · O01-arch-v1）：
C1 段契约自检 check_contracts：O(段数 + 契约数)（段数 7、契约字段 6，为常数；\
   主题/对端映射对账另计，见 C7/C8）。
C2 预算注册 register：O(1)（尾部追加 + 一趟次序比较）。
C3 预算合计 total：O(段数)（七段定长遍历）。
C4 边界校验 check_no_overreach：O(边界条目)（线性查禁扩面表）。
C5 子集门禁 admit：O(属性数)（注册表线性查；四族分桶后单族 ≤64，常数上界）。
C6 参数钳制 clamp_u32/clamp_f64：O(1)（单值比较 + 可选告警追加）。
C7 十主题落点对账 audit_themes：O(主题数 × 落点数)（十主题为定长）。
C8 哈希对账 reconcile：O(契约字节长)（FNV-1a 单遍，64 位累积）。
C9 案件立案 open_case：O(1)（尾部追加 + 三项入参校验）。
C10 风险复核 review：O(风险数)（一趟状态判定）。";

// ---------------------------------------------------------------------------
// 二、七段管线（判据一：O01 架构声明）
// ---------------------------------------------------------------------------

/// 样式引擎段位。**七段不多不少**，顺序即数据流顺序。
///
/// 段序的设计依据是"越早失败越便宜"：分词失败几乎不花钱，等到投影阶段才发现
/// 语法错就已经把整棵树算完了。七段把便宜的检查全放在前面。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// 段 1 · 分词：样式源文本 → token 流。
    Tokenize,
    /// 段 2 · 声明解析：token 流 → 声明块 + 规则节点。
    Parse,
    /// 段 3 · 选择器匹配：规则 × 元素 → 命中集。
    Select,
    /// 段 4 · 层叠裁决：命中集 → 胜出声明（阶位/重要度/来源序）。
    Cascade,
    /// 段 5 · 计算样式：胜出声明 + 继承 → 计算值。
    Compute,
    /// 段 6 · 属性投影：计算值 → N 控件树属性（**单向**，不回读 N 内部结构）。
    Project,
    /// 段 7 · 绘制指令：提升后的计算值 → D 绘制指令（**单向**）。
    Paint,
}

impl Stage {
    /// 七段全集（顺序即 [`STAGE_ORDER`]，唯一真值源）。
    pub const ALL: [Stage; STAGE_COUNT] = [
        Stage::Tokenize,
        Stage::Parse,
        Stage::Select,
        Stage::Cascade,
        Stage::Compute,
        Stage::Project,
        Stage::Paint,
    ];

    /// 中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            Stage::Tokenize => "分词",
            Stage::Parse => "声明解析",
            Stage::Select => "选择器匹配",
            Stage::Cascade => "层叠裁决",
            Stage::Compute => "计算样式",
            Stage::Project => "属性投影",
            Stage::Paint => "绘制指令",
        }
    }

    /// 英文名（标识符与文档用）。
    pub fn en(self) -> &'static str {
        match self {
            Stage::Tokenize => "tokenize",
            Stage::Parse => "parse",
            Stage::Select => "select",
            Stage::Cascade => "cascade",
            Stage::Compute => "compute",
            Stage::Project => "project",
            Stage::Paint => "paint",
        }
    }

    /// 段位序号（0 起）。**全模块唯一的顺序真值**——预算次序、依赖边、
    /// 失效位、读屏叙述全部由它派生，不允许第二处顺序声明。
    pub fn rank(self) -> u8 {
        match self {
            Stage::Tokenize => 0,
            Stage::Parse => 1,
            Stage::Select => 2,
            Stage::Cascade => 3,
            Stage::Compute => 4,
            Stage::Project => 5,
            Stage::Paint => 6,
        }
    }

    /// 段码（对外引用，如 `O01-S3`）。
    pub fn code(self) -> &'static str {
        match self {
            Stage::Tokenize => "O01-S1",
            Stage::Parse => "O01-S2",
            Stage::Select => "O01-S3",
            Stage::Cascade => "O01-S4",
            Stage::Compute => "O01-S5",
            Stage::Project => "O01-S6",
            Stage::Paint => "O01-S7",
        }
    }

    /// 枚举往返守卫：码 → 段。未知码返回 `None`（调用方必须显性拒绝，不许猜）。
    pub fn from_code(code: &str) -> Option<Stage> {
        Stage::ALL.iter().copied().find(|s| s.code() == code)
    }

    /// 该段是否为**单向输出段**（向 N 或 D 域吐东西，且不回读对端内部结构）。
    ///
    /// 这两段是"Web 表达力原生性能"的兑现点，也是越界最容易发生的地方——
    /// 投影段一旦回读 N 的控件树，样式引擎就与框架结构耦合，N 域改结构会连带
    /// 炸样式，必须在架构层就断掉。
    pub fn is_emitting(self) -> bool {
        matches!(self, Stage::Project | Stage::Paint)
    }

    /// 该段服务的判据项（判据四：架构声明/集成边界/解析子集）。
    pub fn serves(self) -> &'static [Criterion] {
        match self {
            // 分词与声明解析把"样式源"收成"子集内的声明"，是解析子集判据的执行段。
            Stage::Tokenize | Stage::Parse => &[Criterion::ParseSubset],
            // 选择器匹配与层叠是"架构声明"的兑现段。
            Stage::Select | Stage::Cascade => &[Criterion::ArchDeclaration],
            // 计算样式承接前四段的产出，向投影段交付。
            Stage::Compute => &[Criterion::ArchDeclaration, Criterion::ParseSubset],
            // 投影与绘制指令是"集成边界"的兑现段（单向承诺的执行点）。
            Stage::Project => &[Criterion::IntegrationBoundary],
            // 绘制指令段是整条管线的收口：计算值在此变成对外产物，
            // 契约自检与读屏替述也正是"对外承诺"（J4：随总纲同源交付，
            // 不另写一份以免漂移）——挂在这一段，J4 才有真正的执行段。
            Stage::Paint => &[Criterion::IntegrationBoundary, Criterion::Criterion],
        }
    }
}

/// 段序单源常量（判据"预算联动次序单源"的真值出处）。
///
/// 全模块**只有这一处**声明顺序。要改顺序就改这里，并让
/// [`BudgetLedger::register`] 的次序门与 [`Architecture::check_contracts`]
/// 一起把住——顺序散落两处等于没有顺序。
pub const STAGE_ORDER: [Stage; STAGE_COUNT] = Stage::ALL;

/// 样式的域级帧预算合计（微秒）——**判据与实现的单一出处**。
///
/// 取值 10000μs = 双 6ms 分账（P 域把一帧16.6ms 拆成两段各 6ms，
/// 样式段占其中一段，与另半屏预算对齐）。七段各自预算是本常量
/// 按段位权重拆出来的，合计必须回到本常量——写两处就会漂。
pub const STAGE_BUDGET_TOTAL_MICROS: u32 = 10_000;

// ---------------------------------------------------------------------------
// 三、判据四项（锚点判据：O01 架构声明、集成边界、解析子集、判据）
// ---------------------------------------------------------------------------

/// 判据项。锚点判据列四项，本总纲把它们变成**可被逐条断言的枚举**。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Criterion {
    /// 判据一：O01 架构声明（七段管线 + 域使命 + 性能宪法）。
    ArchDeclaration,
    /// 判据二：集成边界（Servo 引入决策 + N/D 单向承诺 + 禁扩面）。
    IntegrationBoundary,
    /// 判据三：解析子集（四族分桶 + 表结构 + 门禁 + 排除清单）。
    ParseSubset,
    /// 判据四：判据本身（总纲自证可追溯——契约自检与读屏替述）。
    Criterion,
}

/// 四项判据全集（判据：一项不缺）。
pub const CRITERIA: [Criterion; 4] = [
    Criterion::ArchDeclaration,
    Criterion::IntegrationBoundary,
    Criterion::ParseSubset,
    Criterion::Criterion,
];

impl Criterion {
    /// 判据中文名（读屏播报）。
    pub fn zh(self) -> &'static str {
        match self {
            Criterion::ArchDeclaration => "O01 架构声明",
            Criterion::IntegrationBoundary => "集成边界",
            Criterion::ParseSubset => "解析子集",
            Criterion::Criterion => "判据",
        }
    }

    /// 判据码（对拍与台账引用）。
    pub fn code(self) -> &'static str {
        match self {
            Criterion::ArchDeclaration => "O01-J1",
            Criterion::IntegrationBoundary => "O01-J2",
            Criterion::ParseSubset => "O01-J3",
            Criterion::Criterion => "O01-J4",
        }
    }

    /// 该判据的承诺句（写下来就是契约，不许用"应该""尽量"这类词）。
    pub fn promise(self) -> &'static str {
        match self {
            Criterion::ArchDeclaration => {
                "七段管线每段都有吃/吐/失败策略/复杂度/消费方/不做清单六项契约，\
                 断链即阻断——总纲不许挂名。"
            }
            Criterion::IntegrationBoundary => {
                "Servo 只引 style 与 style_traits，layout 由 N 域承担不可引入；\
                 样式引擎只向 N 吐属性、只向 D 吐指令，永不回读对端内部结构。"
            }
            Criterion::ParseSubset => {
                "属性子集按布局/视觉/效果/交互四族分桶，表结构与门禁语义冻结；\
                 桶外属性一律拒绝并给出路，不静默丢弃。"
            }
            Criterion::Criterion => {
                "契约自检与读屏替述随总纲同源交付，不另写一份以免漂移；\
                 任一判据无可执行断言即视为未落实。"
            }
        }
    }
}

/// 取判据的承诺句。
pub fn criterion_promise(c: Criterion) -> &'static str {
    c.promise()
}

// ---------------------------------------------------------------------------
// 四、段契约（架构声明的可执行形态）
// ---------------------------------------------------------------------------

/// 单段契约。总纲不是标签墙——每段都要说清边界，越界即违约。
#[derive(Clone, Copy, Debug)]
pub struct StageSpec {
    /// 段位。
    pub stage: Stage,
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
}

/// 七段契约表（判据一：架构声明）。
pub const STAGE_SPECS: [StageSpec; STAGE_COUNT] = [
    StageSpec {
        stage: Stage::Tokenize,
        duty_zh: "把样式源文本切成 token 流，并在超长/坏 UTF-8 处显性截断",
        input: "样式源字节流（含BOM 与注释；分词器本体归 F2804）",
        output: "Token 流：类型/起止偏移/文本片段引用（零拷贝切片）",
        on_failure: "坏 UTF-8 -> 替换字符并记告警；源超 MAX_SOURCE_BYTES -> 截断并立案",
        complexity: "C1 O(源长)",
        consumers: "F2804 分词器本体、F2814 错误恢复、F2819 fuzz",
        not_mine: "不写分词状态机本体（F2804），不做选择器词法（F2821）",
    },
    StageSpec {
        stage: Stage::Parse,
        duty_zh: "把 token 流收成声明块与规则节点，并按四族分桶",
        input: "Token 流",
        output: "Stylesheet 骨架：规则节点 + 每规则的声明列表（属性在子集表内）",
        on_failure: "桶外属性 -> 该声明拒绝并继续解析（不拖垮整表）；括号不配对 -> 恢复到配对点",
        complexity: "C5 O(属性数)",
        consumers: "F2805 属性表注册、F2806 简写展开、F2807 样式表对象",
        not_mine: "不写声明解析函数（F2805），不写简写展开器（F2806）",
    },
    StageSpec {
        stage: Stage::Select,
        duty_zh: "把规则与元素做匹配，产出命中集",
        input: "规则节点 + 元素树快照（元素树本体归 N 域）",
        output: "命中集：元素 → 命中规则序列表",
        on_failure: "选择器语法错 -> 该规则整条退出匹配并立案；深度越界 -> 钳制并告警",
        complexity: "C1 O(规则数)",
        consumers: "F2821 选择器 AST、F2822 基础匹配、F2831 右到左矩阵",
        not_mine: "不写选择器 AST 与匹配矩阵（F2821/F2822/F2831）",
    },
    StageSpec {
        stage: Stage::Cascade,
        duty_zh: "按阶位/重要度/来源序裁决胜出声明",
        input: "命中集 + 各声明的阶位与来源",
        output: "胜出声明集：元素 × 属性 → 唯一胜出者",
        on_failure: "同级同重要度同来源 -> 取后声明者并记分歧（确定性优先于直觉）",
        complexity: "C1 O(命中数)",
        consumers: "F2836 层叠阶位、F2837 important 博弈、F2838 media 反查",
        not_mine: "不写六级阶位表本体（F2836），不做 media 反查失效（F2838）",
    },
    StageSpec {
        stage: Stage::Compute,
        duty_zh: "把胜出声明加继承与初始值，算成计算值",
        input: "胜出声明集 + 父元素计算值 + 属性元数据（继承/初始值）",
        output: "计算值集：元素 × 属性 → 计算值 + 出处",
        on_failure: "变量未定义 -> 取显式回退值并标Fallback；循环引用 -> 截断并立案",
        complexity: "C1 O(属性数)",
        consumers: "F2810 计算样式树、F2811 继承与初始值、F2812 calc",
        not_mine: "不写计算样式树本体（F2810），不写属性元数据表（F2811）",
    },
    StageSpec {
        stage: Stage::Project,
        duty_zh: "把计算值投影为 N 控件树属性（单向）",
        input: "计算值集 + 元素映射表（元素 → N 节点句柄）",
        output: "属性补丁：N 节点句柄 × 属性名 → 属性值",
        on_failure: "映射缺失 -> 该元素跳过投影并立案；属性名不在 N 契约 -> 拒绝并给出路",
        complexity: "C1 O(元素数)",
        consumers: "F2817 投影协议本体、N 域属性引擎、F2816 预算归集",
        not_mine: "不写投影协议本体（F2817），不回读 N 控件树内部结构（越界）",
    },
    StageSpec {
        stage: Stage::Paint,
        duty_zh: "把提升后的计算值编译为 D 绘制指令（单向）",
        input: "计算值集 + 需提升的效果元素子集",
        output: "绘制指令流：类型/参数/依赖纹理句柄",
        on_failure: "D 域能力不支持 -> 走降级链并记降级原因；无纹理句柄 -> 该指令跳过并告警",
        complexity: "C1 O(元素数)",
        consumers: "F2818 DisplayList 对接、D 域合成器、F2816 预算归集",
        not_mine: "不写DisplayList 指令编码本体（F2818），不建中间 RT 池（K 域）",
    },
];

/// 取单段契约。
pub fn stage_spec(s: Stage) -> &'static StageSpec {
    STAGE_SPECS
        .iter()
        .find(|sp| sp.stage == s)
        .expect("STAGE_SPECS 覆盖 STAGE_ORDER 全部七段")
}

/// 源文本显性上限（分词段参数域）。超限截断并立案，不静默吞掉尾巴。
pub const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;

/// 选择器匹配深度上限（`Select` 段参数域）。防恶意超深后代链打爆栈。
pub const MAX_SELECTOR_DEPTH: u32 = 32;

/// ---------------------------------------------------------------------------
// 五、性能宪法（判据一附属：预算联动次序单源）
// ---------------------------------------------------------------------------

/// 单段预算条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetEntry {
    /// 段位。
    pub stage: Stage,
    /// 该段预算（微秒；逻辑预算，实测由 F2816 定标入域账本）。
    pub micros: u32,
    /// 实测中位（微秒；0 表示尚未实测——**如实标注，不填乐观值**）。
    pub measured_median_micros: u32,
    /// 实测 P99（微秒；0 表示尚未实测）。
    pub measured_p99_micros: u32,
}

impl BudgetEntry {
    /// 是否已实测（未实测不得参与超标判定——否则等于给自己发免检章）。
    pub fn is_measured(&self) -> bool {
        self.measured_median_micros > 0
    }

    /// 是否超标（实测中位超预算 10% 即标红；未实测返回 `false` 并另行显性标注）。
    pub fn is_over_budget(&self) -> bool {
        self.is_measured() && self.measured_median_micros > self.micros * 11 / 10
    }

    /// 读屏单行（性能数据也要能念）。
    pub fn screen_line(&self) -> String {
        format!(
            "段 {} {}：预算 {} 微秒，实测中位 {}、P99 {}（{}）",
            self.stage.code(),
            self.stage.zh(),
            self.micros,
            if self.measured_median_micros == 0 {
                "未实测".to_string()
            } else {
                self.measured_median_micros.to_string()
            },
            if self.measured_p99_micros == 0 {
                "未实测".to_string()
            } else {
                self.measured_p99_micros.to_string()
            },
            if self.is_over_budget() {
                "超标"
            } else if self.is_measured() {
                "达标"
            } else {
                "待实测"
            }
        )
    }
}

/// 预算账本：次序单源的执行者。
///
/// **次序单源**（锚点性能逐项分解原文）：段的顺序在本模块只有 [`STAGE_ORDER`]
/// 一处真值；预算必须按该序**递增注册**，跳序即 [`E_BUDGET_ORDER`]。这样做的
/// 理由是——预算次序错位会让"上游超时导致下游白跑"这类问题在账本上显形，
/// 而不是等到 P99 爆炸才猜。
#[derive(Clone, Debug, Default)]
pub struct BudgetLedger {
    entries: Vec<BudgetEntry>,
    rejected: u64,
}

impl BudgetLedger {
    /// 空账本。
    pub fn new() -> Self {
        BudgetLedger {
            entries: Vec::new(),
            rejected: 0,
        }
    }

    /// 已注册条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 因次序违规被拒的注册次数（异常显性化：不静默丢弃）。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// 只读遍历（按注册序=段序）。
    pub fn iter(&self) -> impl Iterator<Item = &BudgetEntry> {
        self.entries.iter()
    }

    /// 注册一段预算（复杂度 C2：O(1)）。
    ///
    /// 三项拒绝：
    /// 1. **次序违规**（`stage.rank()` 小于已注册的最大段位）→ [`E_BUDGET_ORDER`]；
    /// 2. **重复注册**（同段已注册）→ [`E_BUDGET_DUP`]；
    /// 3. **预算为零** → [`E_BUDGET_ZERO`]（零预算段等于宣告该段不花钱，
    ///    要么是漏填要么是谎报，不接受）。
    pub fn register(&mut self, entry: BudgetEntry) -> Result<u8, StyleError> {
        if entry.micros == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(StyleError::new(
                E_BUDGET_ZERO,
                "预算注册被拒：预算为零",
                &format!(
                    "段 {} 的预算为 0；零预算要么是漏填要么是谎报，\
                     会让超标判定永远为假",
                    entry.stage.zh()
                ),
                "填入该段的实测定标预算（微秒），或按 ADR 声明该段无预算的正当理由",
                "架构维护方",
            ));
        }
        if self.entries.iter().any(|e| e.stage == entry.stage) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(StyleError::new(
                E_BUDGET_DUP,
                "预算注册被拒：同段重复注册",
                &format!("段 {} 已有预算条目", entry.stage.zh()),
                "改该段既有条目的实测值，不要重复登记（重复登记会让预算翻倍）",
                "架构维护方",
            ));
        }
        if let Some(last) = self.entries.last() {
            if entry.stage.rank() < last.stage.rank() {
                self.rejected = self.rejected.saturating_add(1);
                return Err(StyleError::new(
                    E_BUDGET_ORDER,
                    "预算注册被拒：次序违反单源",
                    &format!(
                        "段 {}（段位 {}）被注册，但账本已推进到段 {}（段位 {}）；\
                         预算联动次序单源要求按 STAGE_ORDER 递增",
                        entry.stage.zh(),
                        entry.stage.rank(),
                        last.stage.zh(),
                        last.stage.rank()
                    ),
                    // 修复注记（AI-ZCode-2，跨会话协同）：next 参数是 &'static str，
                    // 动态段名已在 why 中带出，此处改静态出路文案（原 &format! 不合法）。
                    "先注册正确段位的段，或把该段的预算与实测一并补在正确位置上",
                    "架构维护方",
                ));
            }
        }
        self.entries.push(entry);
        Ok(entry.stage.rank())
    }

    /// 更新某段实测值（不改次序——实测刷新是常态，不该被次序门拦）。
    pub fn record_measurement(&mut self, stage: Stage, median: u32, p99: u32) -> bool {
        match self.entries.iter_mut().find(|e| e.stage == stage) {
            Some(e) => {
                e.measured_median_micros = median;
                e.measured_p99_micros = p99;
                true
            }
            None => false,
        }
    }

    /// 预算合计（复杂度 C3：O(段数)）。**这是域级样式的帧预算上限**。
    pub fn total(&self) -> u32 {
        self.entries.iter().map(|e| e.micros).sum()
    }

    /// 超标段清单（性能数据也要能追责到段）。
    pub fn over_budget_stages(&self) -> Vec<Stage> {
        self.entries
            .iter()
            .filter(|e| e.is_over_budget())
            .map(|e| e.stage)
            .collect()
    }

    /// 未实测段清单（**如实暴露空白**——没测过的段不许当达标段汇报）。
    pub fn unmeasured_stages(&self) -> Vec<Stage> {
        self.entries
            .iter()
            .filter(|e| !e.is_measured())
            .map(|e| e.stage)
            .collect()
    }

    /// 次序一致性自检（预算是否严格按段位递增落账）。
    pub fn order_is_monotonic(&self) -> bool {
        let mut last: Option<u8> = None;
        for e in self.entries.iter() {
            if let Some(l) = last {
                if e.stage.rank() < l {
                    return false;
                }
            }
            last = Some(e.stage.rank());
        }
        true
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "样式预算账本：{} 段，合计 {} 微秒；超标 {} 段，未实测 {} 段，\
             次序违规被拒 {} 次",
            self.len(),
            self.total(),
            self.over_budget_stages().len(),
            self.unmeasured_stages().len(),
            self.rejected()
        )
    }
}

// ---------------------------------------------------------------------------
// 六、Servo 集成决策与集成边界（判据二）
// ---------------------------------------------------------------------------

/// Servo crate 决策（不透明标识，来自上游决策记录）。
pub type ServoCrateId = u16;

/// 未指定的 crate 句柄（保留值）。
pub const CRATE_UNSPECIFIED: ServoCrateId = 0;

/// `style` crate（属性/值/计算样式的类型与解析骨架——**引入**）。
pub const CRATE_STYLE: ServoCrateId = 1;

/// `style_traits` crate（可复用属性元数据——**引入**）。
pub const CRATE_STYLE_TRAITS: ServoCrateId = 2;

/// `layout` crate（布局实现——**不引入**，归属 N 域）。
pub const CRATE_LAYOUT: ServoCrateId = 3;

/// 引入决策。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServoDecision {
    /// 引入（vendor 化落地由 F2802 承担）。
    Vendorize,
    /// 不引入（归属他域或本期不做）。
    Decline,
}

impl ServoDecision {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ServoDecision::Vendorize => "引入",
            ServoDecision::Decline => "不引入",
        }
    }

    /// 决策码（对拍与台账引用）。
    pub fn code(self) -> &'static str {
        match self {
            ServoDecision::Vendorize => "VENDORIZE",
            ServoDecision::Decline => "DECLINE",
        }
    }

    /// 枚举往返守卫：未知码返回 `None`（调用方必须显性拒绝）。
    pub fn from_code(code: &str) -> Option<ServoDecision> {
        match code {
            "VENDORIZE" => Some(ServoDecision::Vendorize),
            "DECLINE" => Some(ServoDecision::Decline),
            _ => None,
        }
    }
}

/// 单个 Servo crate 的决策条目。
#[derive(Clone, Copy, Debug)]
pub struct CrateDecision {
    /// crate 句柄。
    pub crate_id: ServoCrateId,
    /// crate 名。
    pub name: &'static str,
    /// 决策。
    pub decision: ServoDecision,
    /// 决策理由（**必填**——无理由的选型会被后来人推翻）。
    pub reason: &'static str,
    /// 若不引入，归属何处（引入时为空串）。
    pub owner_elsewhere: &'static str,
}

/// Servo 选型决策表（判据二：集成边界；细节落地归 F2802）。
///
/// **layout 不引入是归属裁决，不是"暂时没引"**——引入即越界：
/// 布局是 N 域的领地（F2602 测量布局），样式引擎一旦自带布局，
/// 就会出现两套布局算法对同一棵树给出不同结果，而这种分歧在界面上表现为
/// "同一个页面在两处对不齐"，极难归因。
pub const CRATE_DECISIONS: [CrateDecision; 3] = [
    CrateDecision {
        crate_id: CRATE_STYLE,
        name: "style",
        decision: ServoDecision::Vendorize,
        reason: "属性/值/计算样式的类型骨架与解析语义成熟，vendor 化后自持可控",
        owner_elsewhere: "",
    },
    CrateDecision {
        crate_id: CRATE_STYLE_TRAITS,
        name: "style_traits",
        decision: ServoDecision::Vendorize,
        reason: "属性元数据（继承性/初始值/可解析性）复用价值高，与 style 同源引入省一份对拍",
        owner_elsewhere: "",
    },
    CrateDecision {
        crate_id: CRATE_LAYOUT,
        name: "layout",
        decision: ServoDecision::Decline,
        reason: "布局归属 N 域（控件树测量布局）；双布局并存会导致同树异结果且极难归因",
        owner_elsewhere: "VE-N（N 域 · 测量布局）",
    },
];

/// 域对接端（跨批对接点：上游契约接收 / 下游消费接口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Peer {
    /// 上游：N 域控件树（元素映射表的供给方）。
    UpstreamN,
    /// 下游：N 域属性引擎（属性投影的接收方）。
    DownstreamN,
    /// 下游：D 域 2D 合成（绘制指令的接收方）。
    DownstreamD,
    /// 上游：S 域无障碍渲染（媒体查询与偏好契约）。
    UpstreamS,
    /// 跨卷：CGPU 册效果合同（效果预算与归因）。
    CgpuContract,
}

impl Peer {
    /// 五端全集。
    pub const ALL: [Peer; 5] = [
        Peer::UpstreamN,
        Peer::DownstreamN,
        Peer::DownstreamD,
        Peer::UpstreamS,
        Peer::CgpuContract,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Peer::UpstreamN => "上游 N 域控件树",
            Peer::DownstreamN => "下游 N 域属性引擎",
            Peer::DownstreamD => "下游 D 域 2D 合成",
            Peer::UpstreamS => "上游 S 域无障碍渲染",
            Peer::CgpuContract => "跨卷 CGPU 效果合同",
        }
    }

    /// 对端码（契约引用）。
    pub fn code(self) -> &'static str {
        match self {
            Peer::UpstreamN => "PEER-N-UP",
            Peer::DownstreamN => "PEER-N-DOWN",
            Peer::DownstreamD => "PEER-D-DOWN",
            Peer::UpstreamS => "PEER-S-UP",
            Peer::CgpuContract => "PEER-CGPU",
        }
    }

    /// 该端关系（上游/下游/跨卷）。
    pub fn relation(self) -> &'static str {
        match self {
            Peer::UpstreamN | Peer::UpstreamS => "上游",
            Peer::DownstreamN | Peer::DownstreamD => "下游",
            Peer::CgpuContract => "跨卷",
        }
    }

    /// 该端是否**禁止反读**（对端内部结构不可回读——单向承诺的执行对象）。
    pub fn is_read_only_peer(self) -> bool {
        matches!(self, Peer::DownstreamN | Peer::DownstreamD)
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
    /// 是否已接收（上游契约必须接收才准开工）。
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

/// 集成边界表（本域**不做什么**的显式声明——边界即禁扩面）。
///
/// 这张表与 [`CRATE_DECISIONS`] 一起构成判据二的全部内容：前者管"引入什么"，
/// 后者管"绝不碰什么"。两者都有可执行断言，不是文档里的一句话。
pub const BOUNDARY_EXCLUSIONS: [(&str, &str); 6] = [
    (
        "O-N-REVERSE-READ",
        "回读 N 域控件树内部结构（元素树/属性存储布局）——单向承诺，投影段只按映射表写属性",
    ),
    (
        "O-D-REVERSE-READ",
        "回读 D 域合成器内部状态（RT 池/批队列）——绘制指令只出不进",
    ),
    (
        "O-LAYOUT-OWN",
        "自建布局算法（flex/grid/多列计算）——布局归 N 域，双布局并存即分歧",
    ),
    (
        "O-PAINT-BACKEND",
        "自建光栅化后端（扫描线/路径填充）——绘制归 D 域，本域只编译指令",
    ),
    (
        "O-SCRIPT-SANDBOX",
        "承载脚本执行（JS 引擎）——脚本归 Z 域，样式引擎对脚本只读不解",
    ),
    (
        "O-NET-FETCH",
        "发起网络取样式（@import 远程加载）——取样式由表面调度层给内容，\
         本域只消费已到手的内容",
    ),
];

/// 禁扩面**关键词表**（按 [`BOUNDARY_EXCLUSIONS`] 的 code 顺序一一对应）。
///
/// 为什么需要这张表：边界校验面对的是**人写的自然语言意图**
/// （"自建 flex 布局算法"），而 [`BOUNDARY_EXCLUSIONS`] 的 `desc`
/// 是四十来字的完整说明（"自建布局算法（flex/grid/多列计算）——布局归 N 域……"）。
/// 用 `intent.contains(desc)` 匹配，等于要求意图串把整条说明逐字抄一遍，
/// 现实中永远不会命中——边界表就成了摆设。
///
/// 所以这里给每条禁扩面配一组**能真正代表它的行为关键词**：
/// 意图串命中 code 本串、desc 全文、或任一关键词之一，即判越界。
/// 关键词刻意选**动作 + 对象**的组合（而非"布局""光栅"这类裸词），
/// 避免误伤"把布局结果消费掉"这类域内正常诉求。
pub const OVERREACH_KEYWORDS: [(&str, &[&str]); 6] = [
    (
        "O-N-REVERSE-READ",
        &["回读N", "回读控件树", "读取元素树", "读回内部结构", "读N 域结构"],
    ),
    (
        "O-D-REVERSE-READ",
        &["回读合成器", "反读合成器", "读取RT", "读回RT 池", "读合成器", "拉合成器状态"],
    ),
    (
        "O-LAYOUT-OWN",
        &["自建布局", "自研布局", "自己算布局", "自建 flex", "自建网格", "grid计算"],
    ),
    (
        "O-PAINT-BACKEND",
        &["自建光栅", "自研光栅", "自己扫描线", "自建填充", "自建绘制后端"],
    ),
    (
        "O-SCRIPT-SANDBOX",
        &["承载脚本", "内置JS", "跑 JS", "执行脚本", "JS 引擎", "js引擎", "脚本引擎"],
    ),
    (
        "O-NET-FETCH",
        &["发起网络", "网络取样式", "远程加载样式", "联网取样式"],
    ),
];

/// 集成边界表。
#[derive(Clone, Debug)]
pub struct IntegrationBoundary {
    /// 对端契约登记。
    pub peers: Vec<PeerContract>,
}

impl IntegrationBoundary {
    /// 空边界表（**未接收任何契约**——默认阻断，不是默认放行）。
    pub fn new() -> Self {
        IntegrationBoundary { peers: Vec::new() }
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
    pub fn register(&mut self, c: PeerContract) -> Result<(), StyleError> {
        if c.contract.trim().is_empty() {
            return Err(StyleError::new(
                E_EMPTY_CONTRACT,
                "对端登记被拒：契约为空",
                "空契约没有可对账的内容，登记它只会让哈希对账形同虚设",
                "填入版本化契约名（如 n-ctrl-tree/v1）",
                "对端签署方",
            ));
        }
        if self.peers.iter().any(|p| p.peer == c.peer) {
            return Err(StyleError::new(
                E_PEER_DUP,
                "对端登记被拒：对端重复登记",
                &format!(
                    "{} 已有契约条目；同一对端的契约须走变更纪律而不是并行登记",
                    c.peer.zh()
                ),
                "更新既有条目的契约版本与哈希，并走回执链",
                "对端签署方",
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
    /// 提交的字节串哈希。不一致 [`E_HASH_MISMATCH`]——哈希对账不是"看一眼
    /// 有没有填"，是重算一遍。
    pub fn reconcile(&mut self, peer: Peer, content: &str) -> Result<String, StyleError> {
        let actual = fnv1a64_hex(content.as_bytes());
        let Some(entry) = self.peers.iter_mut().find(|p| p.peer == peer) else {
            return Err(StyleError::new(
                E_PEER_UNKNOWN,
                "对账被拒：对端未登记",
                &format!("{} 没有契约条目，无从对账", peer.zh()),
                // 修复注记（AI-ZCode-2，跨会话协同）：next 需 &'static str，动态对端名已在 why 带出。
                "先 register 登记契约条目，再提交内容对账",
                "架构维护方",
            ));
        };
        if entry.content_hash != actual {
            entry.reconciled = false;
            return Err(StyleError::new(
                E_HASH_MISMATCH,
                "跨批对账未通过：哈希不一致",
                &format!(
                    "{} 登记哈希 {}，实算哈希 {}；契约内容与登记声明不是同一份",
                    peer.zh(),
                    entry.content_hash,
                    actual
                ),
                "以实算哈希为准更新登记，或撤回改动后的内容后重新对账（不猜测哪边对）",
                "架构维护方",
            ));
        }
        entry.received = true;
        entry.reconciled = true;
        Ok(actual)
    }

    /// 接收上游契约（上游未接收 ⇒ 开工阻断——家族纪律）。
    pub fn receive_upstream(&mut self, peer: Peer) -> Result<(), StyleError> {
        if !matches!(peer, Peer::UpstreamN | Peer::UpstreamS) {
            return Err(StyleError::new(
                E_NOT_UPSTREAM,
                "接收被拒：该对端不是上游",
                &format!("{} 是{}端，不走上游接收流程", peer.zh(), peer.relation()),
                "改用对应的登记/对账流程（上下游流程分开是纪律，不是形式）",
                "架构维护方",
            ));
        }
        match self.peers.iter_mut().find(|p| p.peer == peer) {
            Some(p) => {
                p.received = true;
                Ok(())
            }
            None => Err(StyleError::new(
                E_PEER_UNKNOWN,
                "接收被拒：对端未登记",
                &format!("{} 没有契约条目", peer.zh()),
                "先登记契约内容并对账，再声明接收",
                "架构维护方",
            )),
        }
    }

    /// 开工前置检查：全部上游契约必须已接收且已对账。
    ///
    /// 这是"上游契约接收（哈希对账）"判据的执行点——**缺一即阻断**，
    /// 不接受"先开工后补签"。
    pub fn check_upstream_ready(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        for p in self.peers.iter() {
            if matches!(p.peer, Peer::UpstreamN | Peer::UpstreamS) {
                if !p.received {
                    missing.push("E_UPSTREAM_NOT_RECEIVED");
                } else if !p.reconciled {
                    missing.push("E_UPSTREAM_NOT_RECONCILED");
                }
            }
        }
        // 上游对端一个都没登记时同样阻断——"没登记"不等于"不需要"。
        if self
            .peers
            .iter()
            .filter(|p| matches!(p.peer, Peer::UpstreamN | Peer::UpstreamS))
            .count()
            == 0
        {
            missing.push("E_NO_UPSTREAM_DECLARED");
        }
        missing
    }

    /// 禁扩面校验（复杂度 C4：O(边界条目)）。
    ///
    /// 传入一条"想做的事"，逐条比对 [`BOUNDARY_EXCLUSIONS`]。命中即
    /// [`E_BOUNDARY_OVERREACH`]，并把该禁扩面的**归属去处**一并给出——
    /// 越界拒绝必须告诉对方"这事该谁做"，否则下次还会有人试。
    pub fn check_no_overreach(&self, intent: &str) -> Result<&'static str, StyleError> {
        for (i, (code, desc)) in BOUNDARY_EXCLUSIONS.iter().enumerate() {
            // 三路命中：code 本串 / 描述全文 / 关键词任一。
            // 第三路是关键——意图是人写的自然语言，不会逐字抄 desc。
            // 归一化后比对：空格与大小写差异不得成为绕过边界的后门。
            let ni = normalize_for_match(intent);
            let kw_hit = OVERREACH_KEYWORDS
                .get(i)
                .map(|(_, words)| {
                    words.iter().any(|w| {
                        let nw = normalize_for_match(w);
                        !nw.is_empty() && ni.contains(&nw)
                    })
                })
                .unwrap_or(false);
            let ncode = normalize_for_match(code);
            let ndesc = normalize_for_match(desc);
            if ni.contains(&ncode) || ni.contains(&ndesc) || kw_hit {
                return Err(StyleError::new(
                    E_BOUNDARY_OVERREACH,
                    "越界被拒：此事不归样式引擎",
                    &format!("「{}」命中禁扩面 {}：{}", intent, code, desc),
                    boundary_advice(code),
                    "架构维护方",
                ));
            }
        }
        Ok("在边界内")
    }

    /// 反向依赖审计：列出所有被登记为"禁反读"的对端（单向承诺的公开账）。
    ///
    /// 这张表是**公开的承诺**——下游可以拿它来质问"你是不是读了我的内部结构"。
    pub fn reverse_read_audit(&self) -> Vec<&'static str> {
        Peer::ALL
            .iter()
            .filter(|p| p.is_read_only_peer())
            .map(|p| p.zh())
            .collect()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "集成边界：登记对端 {} 个（已接收 {}、已对账 {}），禁扩面 {} 条，禁反读对端 {} 个",
            self.len(),
            self.peers.iter().filter(|p| p.received).count(),
            self.peers.iter().filter(|p| p.reconciled).count(),
            BOUNDARY_EXCLUSIONS.len(),
            self.reverse_read_audit().len()
        )
    }
}

impl Default for IntegrationBoundary {
    fn default() -> Self {
        Self::new()
    }
}

/// 禁扩面的归属去处（越界拒绝的"下一步"内容——给路，不只是拒绝）。
/// 匹配用归一化（**去掉所有 ASCII 空白并转小写**）。
///
/// 为什么必须归一化：禁扩面关键词是中文里夹英文（"内置JS"、"RT 池"），
/// 而真实文本里中英文之间的空格**极不稳定**——IDE 格式化、输入法、
/// Markdown 往返都会插进去。实测「在本模块内置 JS 跑表达式」就因为
/// 多一个空格漏掉了关键词「内置JS」，边界校验被绕过。
///
/// 中文不含 ASCII 空白，所以对中文语义零影响；英文侧统一小写，
/// 消除「RT 池」vs「rt 池」的大小写差异。
pub fn normalize_for_match(s: &str) -> alloc::string::String {
    let mut out = alloc::string::String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            continue;
        }
        if ch.is_ascii_uppercase() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn boundary_advice(code: &str) -> &'static str {
    match code {
        "O-N-REVERSE-READ" => "改为按元素映射表向 N 域写属性；需要结构信息时由 N 域主动推送",
        "O-D-REVERSE-READ" => "改为只提交绘制指令；合成器内部状态由 D 域自持（不要去读RT 池）",
        "O-LAYOUT-OWN" => "把布局需求提给 N 域（测量布局），本域只消费布局结果",
        "O-PAINT-BACKEND" => "把光栅化需求提给 D 域，本域只编译绘制指令",
        "O-SCRIPT-SANDBOX" => "脚本执行归 Z 域；样式引擎对脚本只读不解，不做沙箱",
        "O-NET-FETCH" => "由表面调度层取回内容后交给本域；本域不发起任何网络取样式",
        _ => "先查 BOUNDARY_EXCLUSIONS 确认此事归属，再决定找哪个域",
    }
}

// ---------------------------------------------------------------------------
// 七、样式解析子集（判据三）
// ---------------------------------------------------------------------------

/// 属性族（四族分桶——布局/视觉/效果/交互）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PropertyFamily {
    /// 布局族：盒模型与定位相关。
    Layout,
    /// 视觉族：颜色/边框/字体等静态外观。
    Visual,
    /// 效果族：滤镜/变换/透明度等需要离屏合成的效果。
    Effect,
    /// 交互族：状态与指针相关。
    Interaction,
}

impl PropertyFamily {
    /// 四族全集。
    pub const ALL: [PropertyFamily; PROPERTY_FAMILY_COUNT] = [
        PropertyFamily::Layout,
        PropertyFamily::Visual,
        PropertyFamily::Effect,
        PropertyFamily::Interaction,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            PropertyFamily::Layout => "布局",
            PropertyFamily::Visual => "视觉",
            PropertyFamily::Effect => "效果",
            PropertyFamily::Interaction => "交互",
        }
    }

    /// 族码。
    pub fn code(self) -> &'static str {
        match self {
            PropertyFamily::Layout => "FAM-LAYOUT",
            PropertyFamily::Visual => "FAM-VISUAL",
            PropertyFamily::Effect => "FAM-EFFECT",
            PropertyFamily::Interaction => "FAM-INTERACTION",
        }
    }

    /// 枚举往返守卫：未知码返回 `None`。
    pub fn from_code(code: &str) -> Option<PropertyFamily> {
        PropertyFamily::ALL
            .iter()
            .copied()
            .find(|f| f.code() == code)
    }

    /// 该族属性是否天然需要**离屏合成**（效果族是"贵"的那一族）。
    pub fn needs_compositing(self) -> bool {
        matches!(self, PropertyFamily::Effect)
    }

    /// 该族是否参与**命中测试**（交互族与布局族参与；效果族不参与）。
    pub fn affects_hit_test(self) -> bool {
        matches!(self, PropertyFamily::Layout | PropertyFamily::Interaction)
    }
}

/// 属性规格（子集表的行结构——本项冻结的是**表结构**，全表归 F2803）。
#[derive(Clone, Copy, Debug)]
pub struct PropertySpec {
    /// 属性名（CSS 书写名，小写）。
    pub name: &'static str,
    /// 所属族。
    pub family: PropertyFamily,
    /// 值类型（不透明枚举；语义解释归 F2805/F2812/F2813）。
    pub value_kind: &'static str,
    /// 是否继承（语义归 F2811，本项只留位）。
    pub inherited: bool,
    /// 初值字面量（缺值时的显式兜底，不允许"空即继承"这种含糊）。
    pub initial: &'static str,
    /// 该属性的参数域下界（钳制用）。
    pub domain_low: f64,
    /// 该属性的参数域上界（钳制用）。
    pub domain_high: f64,
}

impl PropertySpec {
    /// 参数域是否合法（下界不高于上界）。
    pub fn domain_is_sane(&self) -> bool {
        self.domain_low <= self.domain_high
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "属性 {}（{}族，类型 {}，{}，初值 {}，域 [{}, {}]）",
            self.name,
            self.family.zh(),
            self.value_kind,
            if self.inherited { "继承" } else { "不继承" },
            self.initial,
            self.domain_low,
            self.domain_high
        )
    }
}

/// 属性子集种子表（**架构自举用的代表性集合，不是全表**——全表归 F2803）。
///
/// 挑选原则：每族取足以证明门禁语义可用的最小集。`display` 刻意放在布局族
/// 且 `inherited=false`——它是"属性族分类不能靠直觉"的一处反例直觉点，
/// 留着它做回归样本。
pub const PROPERTY_SEEDS: [PropertySpec; 12] = [
    PropertySpec {
        name: "display",
        family: PropertyFamily::Layout,
        value_kind: "display-keyword",
        inherited: false,
        initial: "inline",
        domain_low: 0.0,
        domain_high: 0.0,
    },
    PropertySpec {
        name: "width",
        family: PropertyFamily::Layout,
        value_kind: "length",
        inherited: false,
        initial: "auto",
        domain_low: 0.0,
        domain_high: 100000.0,
    },
    PropertySpec {
        name: "margin",
        family: PropertyFamily::Layout,
        value_kind: "length-shorthand",
        inherited: false,
        initial: "0px",
        domain_low: 0.0,
        domain_high: 100000.0,
    },
    PropertySpec {
        name: "color",
        family: PropertyFamily::Visual,
        value_kind: "color",
        inherited: true,
        initial: "canvastext",
        domain_low: 0.0,
        domain_high: 1.0,
    },
    PropertySpec {
        name: "border-radius",
        family: PropertyFamily::Visual,
        value_kind: "length-shorthand",
        inherited: false,
        initial: "0px",
        domain_low: 0.0,
        domain_high: 10000.0,
    },
    PropertySpec {
        name: "font-size",
        family: PropertyFamily::Visual,
        value_kind: "length",
        inherited: true,
        initial: "16px",
        domain_low: 1.0,
        domain_high: 512.0,
    },
    PropertySpec {
        name: "opacity",
        family: PropertyFamily::Effect,
        value_kind: "number",
        inherited: false,
        initial: "1",
        domain_low: 0.0,
        domain_high: 1.0,
    },
    PropertySpec {
        name: "transform",
        family: PropertyFamily::Effect,
        value_kind: "transform-function",
        inherited: false,
        initial: "none",
        domain_low: 0.0,
        domain_high: 0.0,
    },
    PropertySpec {
        name: "filter",
        family: PropertyFamily::Effect,
        value_kind: "filter-function",
        inherited: false,
        initial: "none",
        domain_low: 0.0,
        domain_high: 0.0,
    },
    PropertySpec {
        name: "pointer-events",
        family: PropertyFamily::Interaction,
        value_kind: "pointer-events-keyword",
        inherited: true,
        initial: "auto",
        domain_low: 0.0,
        domain_high: 0.0,
    },
    PropertySpec {
        name: "cursor",
        family: PropertyFamily::Interaction,
        value_kind: "cursor-keyword",
        inherited: true,
        initial: "auto",
        domain_low: 0.0,
        domain_high: 0.0,
    },
    PropertySpec {
        name: "outline-offset",
        family: PropertyFamily::Interaction,
        value_kind: "length",
        inherited: false,
        initial: "0px",
        domain_low: -10000.0,
        domain_high: 10000.0,
    },
];

/// 排除清单（"子集之外明确不支持什么"——比清单本身更重要）。
///
/// 排除项必须写明**去哪找**。只写"不支持"而不给去处，等于把用户往死路上引。
pub const SUBSET_EXCLUSIONS: [(&str, &str); 6] = [
    ("@page", "打印分页语义——本域面向屏幕表面，打印归应用层"),
    ("@font-face src(local)", "本地字体导入需许可校验——归 F2908 字体加载"),
    ("::part / ::slotted", "影子 DOM 部件样式——本域无影子树概念"),
    (":has()", "关系选择器代价不可界——归 F2821 预留，本期不纳子集"),
    ("@container", "容器查询需容器轴——容器归 N 域，查询归后续组"),
    ("attr() 复杂值", "attr 取值语义版本分歧大——先只收 id/class 单值场景"),
];

/// 子集门禁（判据三的执行点）。
///
/// 门禁只有一条规则：**桶外属性拒绝并给出路**。不静默丢弃，也不"先接受
/// 以后再说"——子集一旦破口，样式引擎就要开始为子集外的东西买单，而那份账单
/// 没有人为它批预算。
#[derive(Clone, Debug)]
pub struct SubsetGate {
    specs: Vec<PropertySpec>,
    admitted: u64,
    rejected: u64,
}

impl SubsetGate {
    /// 以种子表构造。
    pub fn standard() -> Self {
        SubsetGate {
            specs: PROPERTY_SEEDS.to_vec(),
            admitted: 0,
            rejected: 0,
        }
    }

    /// 在册属性数。
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// 只读遍历（按族分桶后的稳定次序：族序 → 族内注册序）。
    pub fn iter(&self) -> impl Iterator<Item = &PropertySpec> {
        self.specs.iter()
    }

    /// 累计放行数。
    pub fn admitted(&self) -> u64 {
        self.admitted
    }

    /// 累计拒绝数。
    pub fn rejected(&self) -> u64 {
        self.rejected
    }

    /// 按名查属性规格。
    pub fn spec(&self, name: &str) -> Option<&PropertySpec> {
        self.specs.iter().find(|s| s.name == name)
    }

    /// 门禁判定（复杂度 C5：O(属性数)）。
    ///
    /// 放行返回 `Ok(&PropertySpec)`；桶外属性 [`E_PROPERTY_OUT_OF_SUBSET`]，
    /// 错误里带**该属性应归哪一族**的提示（若能猜到族）与两条出路
    /// （"改用子集内属性" / "走 ADR 扩表"）。
    pub fn admit(&mut self, name: &str) -> Result<&PropertySpec, StyleError> {
        let lower = name.trim().to_ascii_lowercase();
        if lower.is_empty() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(StyleError::new(
                E_PROPERTY_EMPTY,
                "属性判定被拒：属性名为空",
                "空属性名无法定位也无法归族，门禁无从判定",
                "传入选集内的 CSS 属性名（如 opacity）",
                "解析段 F2805",
            ));
        }
        // 修复注记（AI-ZCode-2，跨会话协同）：先判定后记账，避免返回的 &PropertySpec
        // 与 self.admitted 的赋值借用冲突（原 match 持借跨越赋值，E0506）。
        if self.spec(&lower).is_some() {
            self.admitted = self.admitted.saturating_add(1);
            Ok(self.spec(&lower).unwrap())
        } else {
            self.rejected = self.rejected.saturating_add(1);
                let guess = self.guess_family(&lower);
                Err(StyleError::new(
                    E_PROPERTY_OUT_OF_SUBSET,
                    "属性判定被拒：不在解析子集内",
                    &format!(
                        "属性 {} 不在四族子集内{}；子集外属性会让样式引擎为它\
                         无预算地买单",
                        lower,
                        match guess {
                            Some(f) => format!("（它看起来属于{}族）", f.zh()),
                            None => String::new(),
                        }
                    ),
                    // 修复注记（AI-ZCode-2）：next 需 &'static str，动态属性名已在 why 带出。
                    "改用子集内属性；若确需子集外属性，走 ADR 扩表并补齐规格\
                     （族/类型/继承性/初值/参数域五项缺一不可）",
                    "解析段 F2805 / 子集声明 F2803",
                ))
        }
    }

    /// 族猜测（仅用于错误提示，**不作判定依据**——猜错也不能放行）。
    fn guess_family(&self, name: &str) -> Option<PropertyFamily> {
        const HINTS: [(&str, PropertyFamily); 8] = [
            ("width", PropertyFamily::Layout),
            ("height", PropertyFamily::Layout),
            ("margin", PropertyFamily::Layout),
            ("padding", PropertyFamily::Layout),
            ("color", PropertyFamily::Visual),
            ("border", PropertyFamily::Visual),
            ("font", PropertyFamily::Visual),
            ("transition", PropertyFamily::Interaction),
        ];
        HINTS
            .iter()
            .find(|(k, _)| name.starts_with(k) || name.contains(k))
            .map(|(_, f)| *f)
    }

    /// 注册一条属性规格（F2803 扩表时的入口——本项只提供入口与校验）。
    ///
    /// 四项拒绝：重复名、族名非法（枚举守卫）、参数域倒挂、初值为空。
    /// **单族条数超 [`MAX_PROPERTIES_PER_FAMILY`] 亦拒**——那说明"子集"
    /// 已经长成全表，该重新审视子集边界而不是继续加。
    pub fn register(&mut self, spec: PropertySpec) -> Result<(), StyleError> {
        if spec.name.trim().is_empty() {
            return Err(StyleError::new(
                E_PROPERTY_EMPTY,
                "注册被拒：属性名为空",
                "无名属性无法去重也无法归族",
                "补上 CSS 属性名",
                "子集声明 F2803",
            ));
        }
        if self.spec(spec.name).is_some() {
            return Err(StyleError::new(
                E_PROPERTY_DUP,
                "注册被拒：属性重复",
                &format!("属性 {} 已在子集表内", spec.name),
                "改属性名，或修改既有条目的规格（不要并行登记两份）",
                "子集声明 F2803",
            ));
        }
        if spec.initial.trim().is_empty() {
            return Err(StyleError::new(
                E_NO_INITIAL,
                "注册被拒：缺初值",
                &format!(
                    "属性 {} 没有初值；缺值时无处兜底，\
                     '空即继承'是含糊而非策略",
                    spec.name
                ),
                "补上显式初值字面量（如 0px / none / auto）",
                "子集声明 F2803",
            ));
        }
        if !spec.domain_is_sane() {
            return Err(StyleError::new(
                E_DOMAIN_INVERTED,
                "注册被拒：参数域倒挂",
                &format!(
                    "属性 {} 的参数域 [{}, {}] 下界高于上界，\
                     任何值都会被判越界",
                    spec.name, spec.domain_low, spec.domain_high
                ),
                "修正参数域顺序；若该属性无界（如 display），上下界取同值并注明",
                "子集声明 F2803",
            ));
        }
        let in_family = self
            .specs
            .iter()
            .filter(|s| s.family == spec.family)
            .count();
        if in_family >= MAX_PROPERTIES_PER_FAMILY {
            return Err(StyleError::new(
                E_SUBSET_OVERGROWN,
                "注册被拒：子集已越界",
                &format!(
                    "{}族已有 {} 条，达到上限 {}；继续加就不是子集了",
                    spec.family.zh(),
                    in_family,
                    MAX_PROPERTIES_PER_FAMILY
                ),
                "先审视子集边界（该属性是否真属本域），确需扩容走 ADR 提上限",
                "架构维护方",
            ));
        }
        self.specs.push(spec);
        Ok(())
    }

    /// 四族覆盖自检：每族至少一条（空族说明该族被整体漏掉——门禁会有盲区）。
    pub fn family_coverage(&self) -> Vec<PropertyFamily> {
        PropertyFamily::ALL
            .iter()
            .copied()
            .filter(|f| !self.specs.iter().any(|s| s.family == *f))
            .collect()
    }

    /// 枚举守卫自检：四族码往返全通。
    pub fn enum_guard_ok(&self) -> bool {
        PropertyFamily::ALL
            .iter()
            .all(|f| PropertyFamily::from_code(f.code()) == Some(*f))
    }

    /// 读屏摘要（子集表要能念，且**排除清单必须一并念出**）。
    pub fn screen_text(&self) -> String {
        let mut per_family = Vec::new();
        for f in PropertyFamily::ALL.iter() {
            let n = self.specs.iter().filter(|s| s.family == *f).count();
            per_family.push(format!("{}族{}条", f.zh(), n));
        }
        let mut excl = Vec::new();
        for (what, why) in SUBSET_EXCLUSIONS.iter() {
            excl.push(format!("{}（{}）", what, why));
        }
        format!(
            "解析子集：在册 {} 条（{}）；累计放行 {} 次、拒绝 {} 次；\
             明确排除 {} 项：{}",
            self.len(),
            per_family.join("，"),
            self.admitted(),
            self.rejected(),
            SUBSET_EXCLUSIONS.len(),
            excl.join("；")
        )
    }
}

// ---------------------------------------------------------------------------
// 八、官方十主题范围（锚点职责定位：官方十主题范围）
// ---------------------------------------------------------------------------

/// 官方主题（判据"官方十主题范围"的落点表）。
///
/// **十项不多不少**（[`THEME_COUNT`]）。每主题恰好一组落点——多落点必须
/// 在 [`ThemeAudit`] 里显式声明为多组，不许默默重复。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    /// 主题一：Servo 集成。
    ServoIntegration,
    /// 主题二：样式解析子集。
    ParseSubset,
    /// 主题三：backdrop-filter。
    BackdropFilter,
    /// 主题四：transform。
    Transform,
    /// 主题五：CSS 动画。
    Animation,
    /// 主题六：CSS 变量。
    CssVariable,
    /// 主题七：字体加载。
    FontLoading,
    /// 主题八：渲染到表面。
    RenderToSurface,
    /// 主题九：选择器匹配与层叠。
    SelectorCascade,
    /// 主题十：视觉效果（filter 链与混合）。
    VisualEffect,
}

impl Theme {
    /// 十主题全集。
    pub const ALL: [Theme; THEME_COUNT] = [
        Theme::ServoIntegration,
        Theme::ParseSubset,
        Theme::BackdropFilter,
        Theme::Transform,
        Theme::Animation,
        Theme::CssVariable,
        Theme::FontLoading,
        Theme::RenderToSurface,
        Theme::SelectorCascade,
        Theme::VisualEffect,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Theme::ServoIntegration => "Servo 集成",
            Theme::ParseSubset => "样式解析子集",
            Theme::BackdropFilter => "backdrop-filter",
            Theme::Transform => "transform",
            Theme::Animation => "CSS 动画",
            Theme::CssVariable => "CSS 变量",
            Theme::FontLoading => "字体加载",
            Theme::RenderToSurface => "渲染到表面",
            Theme::SelectorCascade => "选择器匹配与层叠",
            Theme::VisualEffect => "视觉效果",
        }
    }

    /// 主题码（台账引用）。
    pub fn code(self) -> &'static str {
        match self {
            Theme::ServoIntegration => "THEME-SERVO",
            Theme::ParseSubset => "THEME-SUBSET",
            Theme::BackdropFilter => "THEME-BACKDROP",
            Theme::Transform => "THEME-TRANSFORM",
            Theme::Animation => "THEME-ANIM",
            Theme::CssVariable => "THEME-VAR",
            Theme::FontLoading => "THEME-FONT",
            Theme::RenderToSurface => "THEME-SURFACE",
            Theme::SelectorCascade => "THEME-CASCADE",
            Theme::VisualEffect => "THEME-EFFECT",
        }
    }

    /// 枚举往返守卫：未知码返回 `None`（不猜近似值）。
    pub fn from_code(code: &str) -> Option<Theme> {
        Theme::ALL.iter().copied().find(|t| t.code() == code)
    }

    /// 该主题的主责段（主题 → 段位的落点，用于预算归因）。
    pub fn primary_stage(self) -> Stage {
        match self {
            Theme::ServoIntegration => Stage::Tokenize,
            Theme::ParseSubset => Stage::Parse,
            Theme::SelectorCascade => Stage::Cascade,
            Theme::VisualEffect | Theme::BackdropFilter | Theme::Transform => Stage::Paint,
            // CSS 变量的落点是继承与初始值解析（计算段）。
            Theme::CssVariable => Stage::Compute,
            // 动画落will-change 与提升策略（F2850 图层提升决策）——绘制段。
            Theme::Animation => Stage::Paint,
            // 字体加载的落点是 calc() 与长度单位解析（F2812）——计算段。
            Theme::FontLoading => Stage::Compute,
            Theme::RenderToSurface => Stage::Project,
        }
    }

    /// 该主题是否有效果族属性（决定是否吃合成预算）。
    pub fn needs_compositing(self) -> bool {
        PropertyFamily::Effect.needs_compositing()
            && matches!(
                self,
                Theme::BackdropFilter | Theme::Transform | Theme::VisualEffect
            )
    }
}

/// 主题落点审计（十主题 10/10 硬门的执行体）。
#[derive(Clone, Debug, Default)]
pub struct ThemeAudit {
    /// 落点缺失的主题。
    pub missing: Vec<Theme>,
    /// 落点重复的主题（>1 组且未声明多组）。
    pub duplicated: Vec<Theme>,
    /// 落点重复但已显式声明为多组的主题（合法）。
    pub declared_multi: Vec<Theme>,
    /// 落点与主责段不一致的主题。
    pub stage_mismatch: Vec<Theme>,
}

impl ThemeAudit {
    /// 是否十主题全通。
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty() && self.duplicated.is_empty() && self.stage_mismatch.is_empty()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "十主题落点审计：缺失 {} 项，重复未声明 {} 项，已声明多组 {} 项，\
             段位不匹配 {} 项",
            self.missing.len(),
            self.duplicated.len(),
            self.declared_multi.len(),
            self.stage_mismatch.len()
        )
    }
}

/// 十主题落点审计（复杂度 C7：O(主题数 × 落点数)）。
///
/// 落点表形如 `主题 → 条目号列表`。三条判定：
/// 1. **无缺**：十主题每项至少一组落点；
/// 2. **无重**：落点多于一组必须在 `declared_multi` 里显式声明——
///    默默多组会让"这条到底归谁"变成口头传承；
/// 3. **段位对齐**：落点所属段应与 [`Theme::primary_stage`] 一致，不一致
///    说明主题被挂在了一段管不了它的段上（预算归因会失真）。
pub fn audit_themes(
    landings: &[(Theme, &[&str])],
    declared_multi: &[Theme],
) -> ThemeAudit {
    let mut audit = ThemeAudit::default();
    for t in Theme::ALL.iter() {
        let hits: Vec<&[&str]> =
            landings.iter().filter(|(th, _)| th == t).map(|(_, items)| *items).collect();
        match hits.len() {
            0 => audit.missing.push(*t),
            1 => {
                let items = hits[0];
                if items.is_empty() {
                    audit.missing.push(*t);
                } else if items
                    .iter()
                    .any(|item| item_stage(item) != Some(t.primary_stage()))
                {
                    audit.stage_mismatch.push(*t);
                }
            }
            _ => {
                if declared_multi.contains(t) {
                    audit.declared_multi.push(*t);
                } else {
                    audit.duplicated.push(*t);
                }
            }
        }
    }
    audit
}

/// 从条目号推段位（`VE-F28xx` → 段位）。
///
/// 映射按 VE-O 域组内条目段位区间冻结，**不做模糊推断**——推不出来返回
/// `None`，由调用方显性处理。猜段位会让预算归因悄悄错位。
pub fn item_stage(item: &str) -> Option<Stage> {
    // O01 组 F2801-F2802：架构与集成边界（分词/解析段前置）。
    match item {
        "VE-F2801" | "VE-F2802" => Some(Stage::Tokenize),
        "VE-F2803" | "VE-F2804" | "VE-F2805" | "VE-F2806" | "VE-F2807" | "VE-F2808" => {
            Some(Stage::Parse)
        }
        "VE-F2809" | "VE-F2810" | "VE-F2811" | "VE-F2812" | "VE-F2813" | "VE-F2814"
        | "VE-F2815" | "VE-F2816" => Some(Stage::Compute),
        // F2817「与VE-N UI 框架对接协议（元素映射表 + 属性投影）」归**投影段**，
        // 不归绘制段——投影段才是写N 域属性的那一段。
        "VE-F2817" => Some(Stage::Project),
        // F2818「与 VE-D 2D 合成对接（DisplayList 指令生成）」归绘制段。
        "VE-F2818" => Some(Stage::Paint),
        // O03 批次 F2841-F2859：视觉效果组（filter/backdrop-filter/transform/
        // 效果与混合/效果性能预算），全部在绘制段产出绘制指令。
        "VE-F2841" | "VE-F2842" | "VE-F2843" | "VE-F2844" | "VE-F2845" | "VE-F2846"
        | "VE-F2847" | "VE-F2848" | "VE-F2849" | "VE-F2850" | "VE-F2851" | "VE-F2852"
        | "VE-F2853" | "VE-F2854" | "VE-F2855" | "VE-F2856" | "VE-F2857" | "VE-F2858"
        | "VE-F2859" => Some(Stage::Paint),
        "VE-F2819" | "VE-F2820" => Some(Stage::Cascade),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 九、参数域钳制（边界越界→钳制 + 告警）
// ---------------------------------------------------------------------------

/// 钳制告警（**每次夹取都留痕**——静默夹取等于撒谎）。
#[derive(Clone, Debug, PartialEq)]
pub struct ClampNotice {
    /// 字段名（哪个属性/参数被夹了）。
    pub field: String,
    /// 原值。
    pub original: f64,
    /// 夹取后的值。
    pub clamped: f64,
    /// 所属域（[下界, 上界]）。
    pub low: f64,
    pub high: f64,
    /// 逻辑 tick。
    pub tick: u64,
}

impl ClampNotice {
    /// 读屏单行（钳制要能念，且要说清"为什么不是我写的那个"）。
    pub fn screen_line(&self) -> String {
        // NaN 原值**不能念成一个数**——它既不是 0 也不是任何数值，
        // 念出来等于编造。单独措辞，如实说「原值非数（NaN）」。
        let orig = if self.original.is_nan() {
            "非数(NaN)".to_string()
        } else {
            format!("{}", self.original)
        };
        let lo = if self.low.is_nan() {
            "非数(NaN)".to_string()
        } else {
            format!("{}", self.low)
        };
        let hi = if self.high.is_nan() {
            "非数(NaN)".to_string()
        } else {
            format!("{}", self.high)
        };
        format!(
            "钳制告警：{} 原值 {} 越出域 [{}, {}]，已夹为 {}（tick {}）",
            self.field,
            orig,
            lo,
            hi,
            self.clamped,
            self.tick
        )
    }

    /// 旧的单行格式（保留原措辞供工具消费）。
    pub fn screen_line_legacy(&self) -> String {
        format!(
            "钳制告警：{} 原值 {} 越出域 [{}, {}]，已夹为 {}（tick {}）",
            self.field, self.original, self.low, self.high, self.clamped, self.tick
        )
    }
}

/// 告警账（钳制告警的容器；满后拒绝并计数）。
#[derive(Clone, Debug, Default)]
pub struct ClampLog {
    notices: Vec<ClampNotice>,
    dropped: u64,
}

impl ClampLog {
    /// 空告警账。
    pub fn new() -> Self {
        ClampLog {
            notices: Vec::new(),
            dropped: 0,
        }
    }

    /// 在册告警数。
    pub fn len(&self) -> usize {
        self.notices.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.notices.is_empty()
    }

    /// 因账满而**未落账**的告警数（异常显性化）。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &ClampNotice> {
        self.notices.iter()
    }

    /// 某字段的告警次数（反复越界的字段要能被一眼看出）。
    pub fn count_for(&self, field: &str) -> usize {
        self.notices.iter().filter(|n| n.field == field).count()
    }

    /// 追加一条告警。账满则 [`E_CLAMP_LOG_FULL`]——不静默丢弃。
    pub fn push(&mut self, n: ClampNotice) -> Result<(), StyleError> {
        if self.notices.len() >= CLAMP_LOG_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(StyleError::new(
                E_CLAMP_LOG_FULL,
                "告警入账被拒：告警账已满",
                &format!(
                    "告警账 {} 条达到上限 {}，本条未落账（累计丢弃 {} 条）",
                    self.notices.len(),
                    CLAMP_LOG_CAP,
                    self.dropped
                ),
                "先归档并轮转告警账，或按 ADR 提升 CLAMP_LOG_CAP；\
                 在此之前不得把丢弃的那几条当成'没发生过'",
                "架构维护方",
            ));
        }
        self.notices.push(n);
        Ok(())
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "钳制告警账：{} 条（丢弃 {} 条）",
            self.len(),
            self.dropped()
        )
    }
}

/// 无告警的钳制（域内值合法时的快路径）。
pub fn clamp_u32(value: u32, low: u32, high: u32) -> u32 {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// 浮点钳制（复杂度 C6：O(1)）。
///
/// 越界即夹取并**同时**落一条 [`ClampNotice`]——返回值与告警成对出现，
/// 调用方无法只要其一（想要"安静地夹"就得自己吞掉返回值，代码评审能看见）。
pub fn clamp_f64(log: &mut ClampLog, field: &str, value: f64, low: f64, high: f64, tick: u64) -> f64 {
    if value.is_nan() {
        // NaN 无序，任何比较都为假——显式当作越界处理，不让它穿过钳制。
        // 原值**如实记 NaN**：记0.0 会在读屏里被念成「原值 0」，那是编造。
        let _ = log.push(ClampNotice {
            field: field.to_string(),
            original: value,
            clamped: low,
            low,
            high,
            tick,
        });
        return low;
    }
    if value < low {
        let _ = log.push(ClampNotice {
            field: field.to_string(),
            original: value,
            clamped: low,
            low,
            high,
            tick,
        });
        low
    } else if value > high {
        let _ = log.push(ClampNotice {
            field: field.to_string(),
            original: value,
            clamped: high,
            low,
            high,
            tick,
        });
        high
    } else {
        value
    }
}

// ---------------------------------------------------------------------------
// 十、异常检出 → 立案流转（降级矩阵第三格）
// ---------------------------------------------------------------------------

/// 案件状态（流转终态是 `Closed`，不是静默消失）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseState {
    /// 已立案，待归因。
    Open,
    /// 归因中。
    Attributing,
    /// 已裁决（终态）。
    Closed,
    /// 判定为非缺陷（终态；须写明理由）。
    Dismissed,
}

impl CaseState {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CaseState::Open => "待归因",
            CaseState::Attributing => "归因中",
            CaseState::Closed => "已裁决",
            CaseState::Dismissed => "判非缺陷",
        }
    }

    /// 是否终态。
    pub fn is_terminal(self) -> bool {
        matches!(self, CaseState::Closed | CaseState::Dismissed)
    }

    /// 合法迁移表（案件流转不是随便改的）。
    pub fn can_transition_to(self, next: CaseState) -> bool {
        match (self, next) {
            (CaseState::Open, CaseState::Attributing) => true,
            (CaseState::Open, CaseState::Dismissed) => true,
            (CaseState::Attributing, CaseState::Closed) => true,
            (CaseState::Attributing, CaseState::Open) => true,
            (CaseState::Attributing, CaseState::Dismissed) => true,
            _ => false,
        }
    }
}

/// 案件记录（异常检出的显性载体）。
#[derive(Clone, Debug)]
pub struct CaseRecord {
    /// 案件号（单调）。
    pub id: u64,
    /// 现象（人话）。
    pub symptom: String,
    /// 影响面（谁会受影响、多大范围）。
    pub impact: String,
    /// 定位（哪个段/哪个对端）。
    pub locus: String,
    /// 处置（做了什么或该做什么）。
    pub disposition: String,
    /// 当前状态。
    pub state: CaseState,
    /// 立案逻辑 tick。
    pub tick: u64,
}

impl CaseRecord {
    /// 读屏单行（案件要能念给用户听——异常零静默）。
    pub fn screen_line(&self) -> String {
        format!(
            "案件 #{}（{}）：{}；影响 {}；定位 {}；处置 {}",
            self.id,
            self.state.zh(),
            self.symptom,
            self.impact,
            self.locus,
            self.disposition
        )
    }
}

/// 案件账。
#[derive(Clone, Debug, Default)]
pub struct CaseLedger {
    cases: Vec<CaseRecord>,
    next_id: u64,
}

impl CaseLedger {
    /// 空案件账。
    pub fn new() -> Self {
        CaseLedger {
            cases: Vec::new(),
            next_id: 1,
        }
    }

    /// 在册案件数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &CaseRecord> {
        self.cases.iter()
    }

    /// 立案（复杂度 C9：O(1)）。
    ///
    /// 拒绝三事：现象为空（无现象的案件无法归因）、影响面为空、
    /// 案件账已满（满后拒绝并计数，不静默丢弃——丢了就等于没检出）。
    pub fn open_case(
        &mut self,
        symptom: &str,
        impact: &str,
        locus: &str,
        disposition: &str,
        tick: u64,
    ) -> Result<u64, StyleError> {
        if symptom.trim().is_empty() {
            return Err(StyleError::new(
                E_CASE_NO_SYMPTOM,
                "立案被拒：无现象",
                "没有现象的案件无法归因，也无法验证是否已修好",
                "写清现象（看到了什么/发生了什么），再立案",
                "检出方",
            ));
        }
        if impact.trim().is_empty() {
            return Err(StyleError::new(
                E_CASE_NO_IMPACT,
                "立案被拒：无影响面",
                &format!("案件「{}」没写影响面，优先级无从判断", symptom),
                "补上影响面（谁受影响/多大范围），哪怕是'仅本段'",
                "检出方",
            ));
        }
        if self.cases.len() >= MAX_CASES {
            return Err(StyleError::new(
                E_CASE_LEDGER_FULL,
                "立案被拒：案件账已满",
                &format!("案件账 {} 条达到上限 {}", self.cases.len(), MAX_CASES),
                "先裁决并归档存量案件，或按 ADR 提升 MAX_CASES；\
                 未裁决的存量未清空前不得继续堆",
                "架构维护方",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.cases.push(CaseRecord {
            id,
            symptom: symptom.to_string(),
            impact: impact.to_string(),
            locus: locus.to_string(),
            disposition: disposition.to_string(),
            state: CaseState::Open,
            tick,
        });
        Ok(id)
    }

    /// 流转一个案件。非法迁移 [`E_CASE_ILLEGAL_TRANSITION`]。
    pub fn transition(&mut self, id: u64, next: CaseState) -> Result<(), StyleError> {
        let Some(case) = self.cases.iter_mut().find(|c| c.id == id) else {
            return Err(StyleError::new(
                E_CASE_UNKNOWN,
                "流转被拒：案件不存在",
                &format!("案件 #{} 不在账内", id),
                "核对案件号；已归档案件不再流转",
                "检出方",
            ));
        };
        if !case.state.can_transition_to(next) {
            return Err(StyleError::new(
                E_CASE_ILLEGAL_TRANSITION,
                "流转被拒：非法状态迁移",
                &format!(
                    "案件 #{} 现为「{}」，不可直接迁到「{}」",
                    id,
                    case.state.zh(),
                    next.zh()
                ),
                match case.state {
                    CaseState::Closed | CaseState::Dismissed => {
                        "该案件已是终态；发现问题请另立新案并引用本案号"
                    }
                    CaseState::Open => "先归因（Attributing）再裁决，或直接判非缺陷并写明理由",
                    CaseState::Attributing => "归因后可裁决为已裁决，或退回待归因",
                },
                "检出方",
            ));
        }
        case.state = next;
        Ok(())
    }

    /// 未终态案件数（**收口硬门**：非零即不允许宣告收官）。
    pub fn open_case_count(&self) -> usize {
        self.cases.iter().filter(|c| !c.state.is_terminal()).count()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "案件账：{} 条（未终态 {} 条）",
            self.len(),
            self.open_case_count()
        )
    }
}

// ---------------------------------------------------------------------------
// 十一、风险登记（锚点职责定位附属：开工即登记风险）
// ---------------------------------------------------------------------------

/// 风险等级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskLevel {
    /// 高：影响开工或里程碑。
    High,
    /// 中：影响单组交付。
    Medium,
    /// 低：影响体验细节。
    Low,
}

impl RiskLevel {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            RiskLevel::High => "高",
            RiskLevel::Medium => "中",
            RiskLevel::Low => "低",
        }
    }
}

/// 风险条目（五要素：现象/影响/概率/缓解/触发信号）。
#[derive(Clone, Debug)]
pub struct RiskEntry {
    /// 风险名。
    pub name: String,
    /// 等级。
    pub level: RiskLevel,
    /// 现象（风险长什么样）。
    pub phenomenon: String,
    /// 影响（踩中会怎样）。
    pub impact: String,
    /// 概率（高/中/低三档，人话）。
    pub likelihood: &'static str,
    /// 缓解（已经做了什么把概率压下去）。
    pub mitigation: String,
    /// 触发信号（看到什么就该认定风险兑现了）。
    pub trigger: String,
    /// 是否已触发。
    pub fired: bool,
}

impl RiskEntry {
    /// 五要素齐备性自检（缺一即不合格——缺要素的风险没法跟踪）。
    pub fn is_complete(&self) -> bool {
        !self.name.trim().is_empty()
            && !self.phenomenon.trim().is_empty()
            && !self.impact.trim().is_empty()
            && !self.mitigation.trim().is_empty()
            && !self.trigger.trim().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "风险「{}」（{}级，概率{}）：{}；影响 {}；缓解 {}；触发信号 {}；{}",
            self.name,
            self.level.zh(),
            self.likelihood,
            self.phenomenon,
            self.impact,
            self.mitigation,
            self.trigger,
            if self.fired { "已触发" } else { "未触发" }
        )
    }
}

/// 风险登记册。
#[derive(Clone, Debug, Default)]
pub struct RiskRegister {
    risks: Vec<RiskEntry>,
}

impl RiskRegister {
    /// 空登记册。
    pub fn new() -> Self {
        RiskRegister { risks: Vec::new() }
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.risks.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.risks.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &RiskEntry> {
        self.risks.iter()
    }

    /// 登记一条风险。缺五要素任一 [`E_RISK_INCOMPLETE`]。
    pub fn register(&mut self, r: RiskEntry) -> Result<(), StyleError> {
        if !r.is_complete() {
            let missing = [
                (!r.name.trim().is_empty(), "风险名"),
                (!r.phenomenon.trim().is_empty(), "现象"),
                (!r.impact.trim().is_empty(), "影响"),
                (!r.mitigation.trim().is_empty(), "缓解"),
                (!r.trigger.trim().is_empty(), "触发信号"),
            ]
            .iter()
            .filter(|(ok, _)| !ok)
            .map(|(_, n)| *n)
            .collect::<Vec<_>>()
            .join("、");
            return Err(StyleError::new(
                E_RISK_INCOMPLETE,
                "风险登记被拒：五要素不全",
                &format!("缺 {}", missing),
                "补齐五要素（现象/影响/概率/缓解/触发信号）；\
                 缺要素的风险既排不了期也验不了是否兑现",
                "架构维护方",
            ));
        }
        if self.len() >= MAX_RISKS {
            return Err(StyleError::new(
                E_RISK_CAP,
                "风险登记被拒：登记册已满",
                &format!("在册 {} 条达到上限 {}", self.len(), MAX_RISKS),
                "先复核并关闭已解除风险，或按 ADR 提升 MAX_RISKS",
                "架构维护方",
            ));
        }
        if self.risks.iter().any(|x| x.name == r.name) {
            return Err(StyleError::new(
                E_RISK_DUP,
                "风险登记被拒：重名",
                &format!("风险「{}」已在册", r.name),
                "更新既有条目的缓解或触发信号，不要并行登记同名风险",
                "架构维护方",
            ));
        }
        self.risks.push(r);
        Ok(())
    }

    /// 按名触发一条风险（看到触发信号时调用——显性，不静默）。
    pub fn fire(&mut self, name: &str) -> bool {
        match self.risks.iter_mut().find(|r| r.name == name) {
            Some(r) => {
                r.fired = true;
                true
            }
            None => false,
        }
    }

    /// 已触发风险数。
    pub fn fired_count(&self) -> usize {
        self.risks.iter().filter(|r| r.fired).count()
    }

    /// 高等级且已触发的风险（开工阻断项）。
    pub fn blocking(&self) -> Vec<&RiskEntry> {
        self.risks
            .iter()
            .filter(|r| r.fired && r.level == RiskLevel::High)
            .collect()
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "风险登记册：{} 条（已触发 {} 条，其中高等级 {} 条）",
            self.len(),
            self.fired_count(),
            self.blocking().len()
        )
    }
}

// ---------------------------------------------------------------------------
// 十二、哈希工具（跨批对账用；FNV-1a 64 位，零依赖、确定性）
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
}

// ---------------------------------------------------------------------------
// 十三、域开工总纲本体
// ---------------------------------------------------------------------------

/// 契约问题（五元组：码/现象/根因/建议/严重度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractIssue {
    /// 问题码。
    pub code: &'static str,
    /// 现象。
    pub symptom: String,
    /// 根因。
    pub cause: String,
    /// 建议。
    pub advice: String,
    /// 严重度：`blocker` 阻断 / `warn` 告警。
    pub severity: &'static str,
}

/// 样式引擎总纲本体。
#[derive(Clone, Debug)]
pub struct StyleEngineArchitecture {
    /// 总纲版本。
    pub version: &'static str,
    /// 四项判据。
    pub criteria: Vec<Criterion>,
    /// 七段段位（按 [`STAGE_ORDER`] 落序）。
    pub stages: Vec<Stage>,
    /// 预算账本。
    pub budget: BudgetLedger,
    /// 集成边界。
    pub boundary: IntegrationBoundary,
    /// 解析子集门禁。
    pub gate: SubsetGate,
    /// 钳制告警账。
    pub clamps: ClampLog,
    /// 案件账。
    pub cases: CaseLedger,
    /// 风险登记册。
    pub risks: RiskRegister,
}

impl StyleEngineArchitecture {
    /// 标准总纲（空账本 + 空边界 + 种子子集——全部显式构造，不留隐式默认）。
    pub fn standard() -> Self {
        StyleEngineArchitecture {
            version: ARCH_VERSION,
            criteria: CRITERIA.to_vec(),
            stages: STAGE_ORDER.to_vec(),
            budget: BudgetLedger::new(),
            boundary: IntegrationBoundary::new(),
            gate: SubsetGate::standard(),
            clamps: ClampLog::new(),
            cases: CaseLedger::new(),
            risks: RiskRegister::new(),
        }
    }

    /// 契约自检：四项判据、七段契约、Servo 决策、子集表不许断链、不许挂名、
    /// 不许越界。
    ///
    /// 检出五类问题（总纲自己先做到可追溯，否则凭什么要求别人）：
    /// - `CONTRACT_DUP`：判据/段位重复或数量不符；
    /// - `CONTRACT_EMPTY`：契约字段留空（挂名）；
    /// - `CONTRACT_ORPHAN`：段位/主题无引用者，或契约引用了未宣告的判据；
    /// - `CONTRACT_STAGE_GAP`：七段里有段没落到契约表；
    /// - `CONTRACT_BUDGET_ORDER`：预算次序被破坏。
    pub fn check_contracts(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();

        // 判据四项：数量、唯一性、承诺句非空、每项有执行段。
        if self.criteria.len() != CRITERIA.len() {
            issues.push(ContractIssue {
                code: "CONTRACT_DUP",
                symptom: format!(
                    "判据在册 {} 项，应为 {} 项",
                    self.criteria.len(),
                    CRITERIA.len()
                ),
                cause: "判据被增删——四项是固定契约，不是可增长清单".to_string(),
                advice: "恢复为架构声明/集成边界/解析子集/判据四项".to_string(),
                severity: "blocker",
            });
        }
        for c in CRITERIA.iter() {
            let registered = self.criteria.iter().filter(|x| *x == c).count();
            if registered != 1 {
                issues.push(ContractIssue {
                    code: "CONTRACT_DUP",
                    symptom: format!("判据 {} 在册 {} 次", c.zh(), registered),
                    cause: "同一判据被重复登记".to_string(),
                    advice: "去重后重新登记".to_string(),
                    severity: "blocker",
                });
            }
            if c.promise().trim().is_empty() {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("判据 {} 承诺句为空", c.zh()),
                    cause: "判据没有承诺句就只剩编号——编号不能被验证".to_string(),
                    advice: format!("补全判据 {} 的承诺句", c.zh()),
                    severity: "blocker",
                });
            }
            let served = Stage::ALL
                .iter()
                .filter(|s| s.serves().contains(c))
                .count();
            if served == 0 {
                issues.push(ContractIssue {
                    code: "CONTRACT_ORPHAN",
                    symptom: format!("判据 {} 没有任何段在执行它", c.zh()),
                    cause: "判据被宣告但无执行者——纸面判据".to_string(),
                    advice: format!("指派至少一段来服务判据 {}", c.zh()),
                    severity: "blocker",
                });
            }
        }

        // 七段：数量、唯一性、段位序严格递增、契约六字段非空、所服务判据在册。
        if self.stages.len() != STAGE_COUNT {
            issues.push(ContractIssue {
                code: "CONTRACT_STAGE_GAP",
                symptom: format!(
                    "段位在册 {} 个，应为 {} 个",
                    self.stages.len(),
                    STAGE_COUNT
                ),
                cause: "段位数被增删——七段是固定契约".to_string(),
                advice: "恢复为分词/声明解析/选择器匹配/层叠裁决/计算样式/属性投影/绘制指令".to_string(),
                severity: "blocker",
            });
        }
        for s in STAGE_ORDER.iter() {
            let registered = self.stages.iter().filter(|x| *x == s).count();
            if registered != 1 {
                issues.push(ContractIssue {
                    code: "CONTRACT_DUP",
                    symptom: format!("段 {} 在册 {} 次", s.zh(), registered),
                    cause: "同一段被重复登记".to_string(),
                    advice: "去重后重新登记".to_string(),
                    severity: "blocker",
                });
            }
            let spec = stage_spec(*s);
            let fields = [
                spec.duty_zh,
                spec.input,
                spec.output,
                spec.on_failure,
                spec.complexity,
                spec.consumers,
                spec.not_mine,
            ];
            if fields.iter().any(|f| f.trim().is_empty()) {
                issues.push(ContractIssue {
                    code: "CONTRACT_EMPTY",
                    symptom: format!("段 {} 契约字段留空（挂名）", s.zh()),
                    cause: "契约字段留空——总纲不允许出现只有名字没有边界的段".to_string(),
                    advice: format!("补全段 {} 的契约字段", s.zh()),
                    severity: "blocker",
                });
            }
            for c in s.serves() {
                if !self.criteria.contains(c) {
                    issues.push(ContractIssue {
                        code: "CONTRACT_ORPHAN",
                        symptom: format!("段 {} 服务的判据 {} 不在册", s.zh(), c.zh()),
                        cause: "段引用了未宣告的判据".to_string(),
                        advice: format!("把 {} 补入判据表，或改该段的服务对象", c.zh()),
                        severity: "blocker",
                    });
                }
            }
        }
        // 段位序必须严格按 rank 递增落序（次序单源在总纲层也成立）。
        for w in self.stages.windows(2) {
            if w[1].rank() <= w[0].rank() {
                issues.push(ContractIssue {
                    code: "CONTRACT_STAGE_GAP",
                    symptom: format!(
                        "段序错位：{}（段位 {}）之后是 {}（段位 {}）",
                        w[0].zh(),
                        w[0].rank(),
                        w[1].zh(),
                        w[1].rank()
                    ),
                    cause: "段位落序不按 rank 递增——次序出现第二处真值".to_string(),
                    advice: "按 STAGE_ORDER 重建段位序列（唯一真值在 Stage::rank）".to_string(),
                    severity: "blocker",
                });
                break;
            }
        }

        // Servo 决策：layout 必须处于"不引入"且写明归属（引入即越界）。
        match CRATE_DECISIONS.iter().find(|c| c.crate_id == CRATE_LAYOUT) {
            Some(d) => {
                if d.decision != ServoDecision::Decline {
                    issues.push(ContractIssue {
                        code: "CONTRACT_BOUNDARY",
                        symptom: "Servo layout 决策为引入".to_string(),
                        cause: "布局归属 N 域；引入 layout 会产生两套布局算法".to_string(),
                        advice: "把 layout 决策改回不引入，归属 VE-N".to_string(),
                        severity: "blocker",
                    });
                }
                if d.owner_elsewhere.trim().is_empty() {
                    issues.push(ContractIssue {
                        code: "CONTRACT_EMPTY",
                        symptom: "Servo layout 决策未写归属去处".to_string(),
                        cause: "不引入却不写归属——等于让它悬空，日后必被重新捡起".to_string(),
                        advice: "在 owner_elsewhere 写明归属域（如 VE-N · 测量布局）".to_string(),
                        severity: "blocker",
                    });
                }
            }
            None => issues.push(ContractIssue {
                code: "CONTRACT_STAGE_GAP",
                symptom: "Servo 决策表缺 layout 条目".to_string(),
                cause: "layout 是本域最容易被误引入的 crate，缺条目等于没表态".to_string(),
                advice: "补上 layout 的不引入决策与归属".to_string(),
                severity: "blocker",
            }),
        }

        // 预算次序。
        if !self.budget.order_is_monotonic() {
            issues.push(ContractIssue {
                code: "CONTRACT_BUDGET_ORDER",
                symptom: "预算账本次序被破坏".to_string(),
                cause: "预算未按 STAGE_ORDER 递增落账——次序单源失效".to_string(),
                advice: "按段位序重建预算账本；次序门由 BudgetLedger::register 把守".to_string(),
                severity: "blocker",
            });
        }

        issues
    }

    /// 开工前置检查汇总（返回全部阻断项；空数组即准开工）。
    ///
    /// 四查：契约自检、上游契约接收、布局归属、子集四族覆盖。
    pub fn preflight(&self) -> Vec<String> {
        let mut blockers = Vec::new();
        for i in self.check_contracts() {
            if i.severity == "blocker" {
                blockers.push(format!("[{}] {}（{}）", i.code, i.symptom, i.advice));
            }
        }
        for code in self.boundary.check_upstream_ready() {
            let why = match code {
                "E_UPSTREAM_NOT_RECEIVED" => "上游契约已登记但未接收——开工前必须接收",
                "E_UPSTREAM_NOT_RECONCILED" => "上游契约已接收但哈希未对账——对账不过不算接收",
                _ => "未声明任何上游契约——'没登记'不等于'不需要'",
            };
            blockers.push(format!("[{}] {}", code, why));
        }
        for f in self.gate.family_coverage() {
            blockers.push(format!(
                "[E_SUBSET_FAMILY_EMPTY] {}族在子集表内无属性——该族门禁存在盲区",
                f.zh()
            ));
        }
        for r in self.risks.blocking() {
            blockers.push(format!(
                "[RISK_FIRED] 高等级风险「{}」已触发：{}",
                r.name, r.impact
            ));
        }
        blockers
    }

    /// 十主题落点（标准落点表——架构自举用的最小完整集）。
    pub fn theme_landings() -> Vec<(Theme, &'static [&'static str])> {
        vec![
            (Theme::ServoIntegration, &["VE-F2801", "VE-F2802"]),
            (Theme::ParseSubset, &["VE-F2803", "VE-F2804", "VE-F2805"]),
            // CSS 变量落继承与初始值解析（变量是继承链上的计算值）。
            (Theme::CssVariable, &["VE-F2811"]),
            // filter 属性子集是视觉效果的兑现点。
            (Theme::VisualEffect, &["VE-F2844"]),
            (Theme::SelectorCascade, &["VE-F2819"]),
            (Theme::RenderToSurface, &["VE-F2817"]),
            // backdrop-filter 基础兑现（F2842）。
            (Theme::BackdropFilter, &["VE-F2842"]),
            // transform 2D 解析与矩阵（F2846）。
            (Theme::Transform, &["VE-F2846"]),
            // 动画落will-change 与提升策略（F2850）。
            (Theme::Animation, &["VE-F2850"]),
            // 字体加载依赖长度单位解析（F2812）。
            (Theme::FontLoading, &["VE-F2812"]),
        ]
    }

    /// 无障碍：架构文档的读屏替述（判据点名"文档替述可读"）。
    ///
    /// 架构图对读屏用户不可达——本函数产出**线性文字版**：七段是什么、
    /// 数据怎么流、Servo 怎么裁、子集装什么、失败会怎样，全部念得出来。
    /// 文字版与图版**同源**（同一份 `STAGE_SPECS` / `CRATE_DECISIONS` /
    /// `PROPERTY_SEEDS` / `BOUNDARY_EXCLUSIONS`），不另写一份以免漂移。
    pub fn architecture_narration(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "CSS HTML 表面域样式引擎总架构 {}，共 {} 项判据 {} 段管线。域使命：\
             用 CSS 与 HTML 表面声明界面，样式引擎把它编译为 N 域控件树与 D 域\
             绘制指令，取 Web 表达力而不付解释器与运行时的代价。",
            self.version,
            self.criteria.len(),
            self.stages.len()
        ));
        out.push_str("管线七段依次是：");
        let flow: Vec<&str> = STAGE_ORDER.iter().map(|s| s.zh()).collect();
        out.push_str(&flow.join("，"));
        out.push_str("。逐段说明：");
        for s in STAGE_ORDER.iter() {
            let sp = stage_spec(*s);
            out.push_str(&format!(
                "第 {} 段，{}，{}；吃{}，吐{}；失败则{}；复杂度{}；下游是{}；本段不做{}。",
                s.rank() + 1,
                s.zh(),
                sp.duty_zh,
                sp.input,
                sp.output,
                sp.on_failure,
                sp.complexity,
                sp.consumers,
                sp.not_mine
            ));
        }
        out.push_str("四项判据：");
        for c in CRITERIA.iter() {
            out.push_str(&format!(
                "{}（{}），承诺：{}。",
                c.code(),
                c.zh(),
                criterion_promise(*c).replace('\n', "")
            ));
        }
        out.push_str("Servo 集成决策：");
        for d in CRATE_DECISIONS.iter() {
            out.push_str(&format!(
                "{} crate 决策为{}，理由：{}{}。",
                d.name,
                d.decision.zh(),
                d.reason,
                if d.owner_elsewhere.is_empty() {
                    String::new()
                } else {
                    format!("归属：{}", d.owner_elsewhere)
                }
            ));
        }
        out.push_str(&format!(
            "本域明确不做的事共 {} 条：",
            BOUNDARY_EXCLUSIONS.len()
        ));
        for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
            out.push_str(&format!(
                "{}：{}；该找{}。",
                code,
                desc,
                boundary_advice(code)
            ));
        }
        out.push_str(&format!("解析子集：{}", self.gate.screen_text()));
        out.push_str(&format!("预算：{}", self.budget.screen_text()));
        out.push_str(&format!("对接：{}", self.boundary.screen_text()));
        out.push_str(&format!("异常：{}；{}", self.cases.screen_text(), self.clamps.screen_text()));
        out.push_str(&format!("风险：{}", self.risks.screen_text()));
        out
    }

    /// 读屏摘要（一行版）。
    pub fn screen_text(&self) -> String {
        format!(
            "样式引擎总架构 {}：判据 {} 项，管线 {} 段（预算合计 {} 微秒），\
             子集 {} 条属性，禁扩面 {} 条，对端 {} 个，案件 {} 条未终态 {} 条，{}",
            self.version,
            CRITERIA.len(),
            STAGE_COUNT,
            self.budget.total(),
            self.gate.len(),
            BOUNDARY_EXCLUSIONS.len(),
            self.boundary.len(),
            self.cases.len(),
            self.cases.open_case_count(),
            self.risks.screen_text()
        )
    }
}

impl Default for StyleEngineArchitecture {
    fn default() -> Self {
        Self::standard()
    }
}

/// 下游归属表（防止总纲被当成万能筐，也防止各组互相抢活）。
pub const DOWNSTREAM_OWNERSHIP: [(&str, &str); 8] = [
    ("VE-F2802", "Servo crate vendor 化落地、MPL-2.0 合规文本与回退预案"),
    ("VE-F2803", "样式解析子集四族完整属性清单（本项只冻结表结构与门禁语义）"),
    ("VE-F2804", "CSS Syntax L3 分词器本体与零拷贝切片实现"),
    ("VE-F2809", "样式失效与重计算调度、失效源六类与 Bloom 过滤"),
    ("VE-F2810", "计算样式树与共享结构"),
    ("VE-F2816", "样式性能预算分解、三层缓存与命中率遥测"),
    ("VE-F2817", "与 VE-N UI 框架的投影协议本体与失效桥接"),
    ("VE-F2818", "与 VE-D 2D 合成的 DisplayList 指令生成"),
];

// ---------------------------------------------------------------------------
// 十四、错误五元组（零静默）
// ---------------------------------------------------------------------------

/// 段位数量不符（契约层）。
pub const E_STAGE_COUNT: &str = "E_STAGE_COUNT";
/// 预算为零。
pub const E_BUDGET_ZERO: &str = "E_BUDGET_ZERO";
/// 同段重复登记预算。
pub const E_BUDGET_DUP: &str = "E_BUDGET_DUP";
/// 预算次序违反单源。
pub const E_BUDGET_ORDER: &str = "E_BUDGET_ORDER";
/// 对端契约为空。
pub const E_EMPTY_CONTRACT: &str = "E_EMPTY_CONTRACT";
/// 对端重复登记。
pub const E_PEER_DUP: &str = "E_PEER_DUP";
/// 对端未登记。
pub const E_PEER_UNKNOWN: &str = "E_PEER_UNKNOWN";
/// 跨批哈希不一致。
pub const E_HASH_MISMATCH: &str = "E_HASH_MISMATCH";
/// 接收流程用错（非上游端）。
pub const E_NOT_UPSTREAM: &str = "E_NOT_UPSTREAM";
/// 越界（命中禁扩面）。
pub const E_BOUNDARY_OVERREACH: &str = "E_BOUNDARY_OVERREACH";
/// 属性名为空。
pub const E_PROPERTY_EMPTY: &str = "E_PROPERTY_EMPTY";
/// 属性不在子集内。
pub const E_PROPERTY_OUT_OF_SUBSET: &str = "E_PROPERTY_OUT_OF_SUBSET";
/// 属性重复登记。
pub const E_PROPERTY_DUP: &str = "E_PROPERTY_DUP";
/// 属性缺初值。
pub const E_NO_INITIAL: &str = "E_NO_INITIAL";
/// 参数域倒挂。
pub const E_DOMAIN_INVERTED: &str = "E_DOMAIN_INVERTED";
/// 子集越界膨胀。
pub const E_SUBSET_OVERGROWN: &str = "E_SUBSET_OVERGROWN";
/// 告警账满。
pub const E_CLAMP_LOG_FULL: &str = "E_CLAMP_LOG_FULL";
/// 案件无现象。
pub const E_CASE_NO_SYMPTOM: &str = "E_CASE_NO_SYMPTOM";
/// 案件无影响面。
pub const E_CASE_NO_IMPACT: &str = "E_CASE_NO_IMPACT";
/// 案件账满。
pub const E_CASE_LEDGER_FULL: &str = "E_CASE_LEDGER_FULL";
/// 案件不存在。
pub const E_CASE_UNKNOWN: &str = "E_CASE_UNKNOWN";
/// 案件非法迁移。
pub const E_CASE_ILLEGAL_TRANSITION: &str = "E_CASE_ILLEGAL_TRANSITION";
/// 风险五要素不全。
pub const E_RISK_INCOMPLETE: &str = "E_RISK_INCOMPLETE";
/// 风险登记册满。
pub const E_RISK_CAP: &str = "E_RISK_CAP";
/// 风险重名。
pub const E_RISK_DUP: &str = "E_RISK_DUP";

/// 样式引擎错误五元组（发生了什么/为什么/下一步/责任方/错误码）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleError {
    /// 错误码。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
    /// 下一步（必填——拒绝必须给出路）。
    pub next: &'static str,
    /// 责任方。
    pub who: String,
}

impl StyleError {
    /// 构造（五元组齐发，构造点强制写全）。
    pub fn new(
        code: &'static str,
        what: &'static str,
        why: &str,
        next: &'static str,
        who: &str,
    ) -> Self {
        StyleError {
            code,
            what,
            why: why.to_string(),
            next,
            who: who.to_string(),
        }
    }

    /// 读屏可读的完整错误（三要素齐发：现象/原因/怎么办）。
    pub fn screen_text(&self) -> String {
        format!(
            "错误 {}：{}；原因：{}；下一步：{}；责任方：{}",
            self.code, self.what, self.why, self.next, self.who
        )
    }
}

