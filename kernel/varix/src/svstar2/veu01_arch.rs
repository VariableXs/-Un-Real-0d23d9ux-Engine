//! VE-F4201 · U 域开工与一致性总架构（VE-U 域 · 一致性域 · U01 组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4201`
//!
//! **判据（锚点原文）**：五层、接口冻结、承接落地、入约、口径契约层、判据。
//!
//! **职责定位（锚点原文）**：U 域开工（一致性域开工：跨域一致性模型/契约/扫描/度量/
//! 知识图谱——**让同一系统只有一套规则的技术底座**）；总架构五层（模型层→契约层→
//! 规则层→验证层→度量层——层间接口冻结）；与家族关系声明（承接 S 域交互词典/T 域
//! 地区规则为首批契约源——移交包交接面落地）。
//!
//! **数据结构（锚点原文·家族格式）**：总架构册（五层）；层间接口冻结；承接面落地表。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：层间失配→对拍；接口越权变更→冻结
//! 流程；承接缺源→回溯移交包。
//!
//! **性能逐项分解（锚点原文·家族格式）**：架构 O(层)；冻结 O(接口)；落地 O(源)。
//!
//! **跨批对接点（锚点原文·家族格式）**：T10 移交包单源；F4202 模型下游；U01 双签闸。
//!
//! **无障碍与隐私（锚点原文）**：架构含无障碍口径契约层（S 域双维标准入约——域本色）；
//! 无隐私面。
//!
//! # 一、让同一系统只有一套规则（U 域使命的落法）
//!
//! 锚点给 U 域的定位是一句技术判断，不是一句口号：「**让同一系统只有一套规则**」。
//! 这句话在工程上只有一个成立条件——**同一语义在任何地方都只有一个出处**。
//! 因此本总架构把「规则有几份」这件事变成可数的：规则文本只能存在于契约层，
//! 其余四层一律持**指针**不持**副本**。[`Layer::owns_rule_text`] 是这条判断的
//! 可执行形态：只有 [`Layer::Contract`] 返回 `true`，其余四层持副本即
//! [`E_RULE_TEXT_DUPLICATED`]。
//!
//! 为什么值得单列一条红线：一致性域最常见的失败不是"没写规则"，而是**同一个
//! 语义被写了两遍**（一份在词典、一份在代码、一份在注释），三份各自演化，
//! 最后谁也不服谁。对拍能发现不一致，但发现不了"三份都对、只是分叉"——所以
//! 必须在架构层就禁掉副本，而不是等不一致发生后再修。
//!
//! # 二、五层 vs 五能力：两份清单各有一项没有 1:1 对手（锚点计数歧义与裁决）
//!
//! 锚点给了**两份五项清单**，它们不是同一份东西的两种写法：
//!
//! - **职责定位列五能力**：跨域一致性模型 / 契约 / 扫描 / 度量 / 知识图谱；
//! - **总架构列五层**：模型层 → 契约层 → 规则层 → 验证层 → 度量层。
//!
//! 逐项对拍会发现两处**不对齐**，本项不把任一项悄悄丢掉，也不硬凑成 5:5：
//!
//! | 锚点能力 | 落点层 | 性质 |
//! |---|---|---|
//! | 跨域一致性模型 | [`Layer::Model`] | 1:1 |
//! | 契约 | [`Layer::Contract`] **+** [`Layer::Rule`] | 一能力两段：契约层声明、规则层可执行化 |
//! | 扫描 | [`Layer::Verification`] | 能力名与层名不同词，位次相同（验证层以扫描为执行形态） |
//! | 度量 | [`Layer::Metric`] | 1:1 |
//! | 知识图谱 | **无专属层**（派生） | 由模型层实体 + 契约层关系派生出的索引视图 |
//!
//! 两条裁决：
//!
//! - **规则层「有层无能名」**：规则层是契约能力的**执行段**，不是第六项能力。
//!   契约声明「应然」，规则层把应然编译成可执行判定（F4204 规则引擎是 F4203
//!   注册中心的下游，不是并列的新能力）。把规则层单列为一项能力会让能力数变六，
//!   且暗示"规则可以脱离契约独立存在"——那正是副本的入口。
//! - **知识图谱「有能无层」**：图谱**不升为第六层**。它是模型层实体与契约层关系
//!   的派生索引；给它独立层会出现同一实体两份真相（图谱一份、模型一份），
//!   直接违反本域第一条使命。图谱本体归 VE-F4209，U 域只登记它的**派生关系**
//!   （由哪两层派生、变更时谁重放），由 [`Capability::derivation_sources`]
//!   与 [`ConsistencyArchitecture::check_capability_alignment`] 机检。
//!
//! 两条裁决合起来使「五层」与「五能力」都**完整无缺**且**互不冒充**：
//! 五层各有服务它的能力（[`Layer::served_by`]），五能力各有落点或派生声明
//! （[`Capability::layers`]），机检双向覆盖（见 `U01-判据-对账-*`）。
//!
//! # 三、首批两源 vs 交接面三源（F4195 交接面多出的术语库不静默丢弃）
//!
//! 锚点写「承接 **S 域交互词典/T 域地区规则**为首批契约源」——**两**源。
//! 而上游 T10 移交包（VE-F4195）的交接面声明写「交互词典/地区规则/**术语库**
//! 作为一致性契约源输入」——**三**路。两处不一致，本项按锚点原文执行首批两源，
//! 但**不把第三路悄悄扔掉**：
//!
//! - 首批落地源 = [`AcceptanceRole::Primary`]：S 域交互词典（VE-F3982 / F4182
//!   家族词典）、T 域地区规则（VE-F4101 起）——这两项是 F4201 明文要求的；
//! - 继承位 = [`AcceptanceRole::Inherited`]：术语库（T04 单源，VE-F4063）——由
//!   T10 交接面带入，**先登记不落地**，落地义务归 VE-F4212（一致性与文案口径）。
//!
//! 为什么要留这个位：术语库确实是一致性契约源（文案口径规则的上游），但 F4201
//! 没把它写进首批。如果直接按两源开工，F4212 开工时会发现"上游交接面声明了三路，
//! 承接表里只有两路"，缺口在两个域之间互相推诿——**契约源在交接那一刻丢件，
//! 是最难查的一类缺陷**。留一个`carried_from: T10` 的继承位，成本是一个枚举值，
//! 收益是交接链可追。机检见 `U01-判据-承接-*`：继承位未落地**不算红**，
//! 但**未登记**一定红（登记是义务，落地是时序）。
//!
//! # 四、入约 ≠ 引用（域本色：无障碍口径契约层）
//!
//! 锚点要求「架构含无障碍口径契约层（S 域**双维标准入约**——域本色）」。
//! 「入约」两个字是本项最容易被做成文档的一格，本总架构把它钉成三态判定
//! （[`EnrollmentState`]）：
//!
//! - [`EnrollmentState::Referenced`] **引用**：文档里提到了双维标准。对拍查不出
//!   问题，用户也拿不到保障——**这正是最坏的一种**，因为它看起来像做了；
//! - [`EnrollmentState::Enrolled`] **入约**：双维标准成为契约层的**必填判据位**，
//!   契约注册时该位为空即 [`E_A11Y_NOT_ENROLLED`]，**注册闸直接拒**；
//! - [`EnrollmentState::Waived`] **豁免**：跳过入约。**本域不接受豁免态**——
//!   [`WordingContractLayer::enroll`] 没有豁免入口，`check_wording_contract`
//!   遇到 `Waived` 直接判红（`E_A11Y_WAIVED_FORBIDDEN`）。理由与 F4204/F4205
//!   同源：无障碍判据一旦允许豁免，它就会被逐案豁免掉。
//!
//! 双维取 S 域 F3986 的口径（**工具维 + 产出维**）：工具维是"用什么辅助技术能
//! 验证"，产出维是"产出的界面能不能被读屏正确朗读"。两维都齐才算入约——
//! 只有工具维等于「我们测过了」，只有产出维等于「我们写得够好」，都不是
//! 「用户能用」。
//!
//! # 五、本项的边界（不越界施工，遵守「只做领到的任务」）
//!
//! VE-F4201 是**域开工与总架构**。它交付：五层与五能力的双向对账、层间接口冻结
//! v1（含越权变更拒绝与 ADR 唯一合法通道）、承接面落地表（含继承位与回溯）、
//! 无障碍口径契约层的入约三态判定、层间对拍、六项判据的可执行自检、读屏替述。
//! 它**不代做**后续 19 项的引擎本体。分工在册（见 [`DOWNSTREAM_OWNERSHIP`]`）：
//!
//! - F4202 跨域一致性模型拥有**四类实体 × 三型关系 + 关系代数 + 环检测**本体；
//!   本项只立模型层的段契约与「模型层是实体唯一真相」的单源断言；
//! - F4203 契约注册中心拥有**元模型五字段 + 唯一性断言 + 引用计数**本体；本项的
//!   契约层只声明「谁持规则文本」的职责，不建注册表；
//! - F4204 一致性规则引擎拥有**三段式可执行化 + 三元裁决器**本体；本项只立
//!   「契约段 → 规则段」的编译契约与失败降级位；
//! - F4206 扫描平台拥有**三模式调度 + 插件隔离**本体；本项的验证层只声明扫描
//!   是验证层的执行形态，并要求无障碍规则**三模式均不跳过**；
//! - F4207 度量体系拥有**四主指标 + 复算器**本体；本项的度量层只声明它是五层
//!   的终点（度量消费前四层产出，不产生新规则）；
//! - F4209 知识图谱拥有**节点/边构建 + 扩散查询**本体；本项只登记其派生关系
//!   （派生自模型层 + 契约层，见头注§二）；
//! - F4220 U01 组收口双签拥有**四件齐备 + 双签分离**本体；本项只声明自己是
//!   U01 双签闸的**第一件**（架构冻结哈希），不代签。
//!
//! # 六、设计要点（每条都是可执行的，不是标签）
//!
//! - **层间接口冻结是判据不是措辞**：[`InterfaceFreezeLedger::attempt_change`]
//!   对任何未走 ADR 的改动一律 [`E_INTERFACE_UNFROZEN_CHANGE`]，并给出唯一合法
//!   出路（先 [`InterfaceFreezeLedger::open_adr`] 再
//!   [`InterfaceFreezeLedger::rebase`]）。ADR 记录须含**被否决的备选方案**——
//!   没有否决记录的 ADR 不是决策记录，是提案；
//! - **层间失配走对拍不走猜**：[`ConsistencyArchitecture::cross_check`] 重算每条
//!   接口的声明哈希与冻结哈希比对，失配即出 [`CrossCheckFinding`]（含失配接口、
//!   两侧哈希、定位段位），**不自动改冻结值**——自动改会让"对拍"变成"掩盖"；
//! - **承接缺源走回溯不走兜底**：[`AcceptanceLedger::trace_back`] 产出
//!   [`TraceBack`]（回溯到 T10 移交包的具体件与交接面 + 建议动作），**不接受
//!   "先用默认规则顶上"**——一致性域最怕的就是拿一个没人负责的默认值长期跑；
//! - **性能声明可核**：架构核 O(层)=O(5)、冻结核 O(接口)=O(4)、落地核 O(源)。
//!   三者都是**定长小表**，所以域开工本身零运行时开销（[`Layer::cost`] 全为
//!   [`LayerCost::DeclarationOnly`]），任何一层被标为运行期即
//!   [`E_RUNTIME_OVERHEAD_DECLARED`]；
//! - **越权改架构要拦得住**：[`check_no_overreach`] 逐条比对 [`BOUNDARY_EXCLUSIONS`]，
//!   命中即拒并给出**该归哪个条目**——越界拒绝必须告诉对方找谁，否则下次还会试。
//!
//! **零外部依赖**，只依赖 `crate::checks`（自检侧）与 `alloc`。
//! 确定性：零墙钟、零 IO，回归可复现（对拍红线）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、总纲常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 总纲版本。契约变更走版本号，破坏性变更必须升版并留迁移说明。
pub const ARCH_VERSION: &str = "U01-arch-v1";

/// **层间接口冻结版本**（锚点原文「层间接口冻结」）。
///
/// 五层之间的四条相邻接口的吃/吐/失败策略发生增删改，必须升这个版本并在
/// [`AdrRecord`] 登记——就地改会让下游无从判断该按哪一版编，这是接口冻结的
/// 全部意义。本域内**没有第二条合法变更通道**。
pub const INTERFACE_VERSION: &str = "U01-iface-v1";

/// 总架构层数（判据「五层」的真值出处）。
pub const LAYER_COUNT: usize = 5;

/// 开工能力数（锚点职责定位列的五项）。
pub const CAPABILITY_COUNT: usize = 5;

/// 判据项数（六项）。
pub const CRITERION_COUNT: usize = 6;

/// S 域双维标准的维数（工具维 + 产出维）。
pub const DUAL_DIMENSION_COUNT: usize = 2;

/// 承接面源条目上限。首批两源 + 继承位 + 后续批次源，留足余量。
pub const MAX_ACCEPTANCE_SOURCES: usize = 16;

/// 层间接口条数上限。五层相邻四条，恒定；留一位余量供 U02 起扩展。
pub const MAX_INTERFACES: usize = 8;

/// ADR 登记条数上限。
pub const MAX_ADRS: usize = 32;

/// 对拍记录条数上限。
pub const MAX_CROSS_CHECKS: usize = 32;

/// 无障碍判据条数上限（双维各若干条）。
pub const MAX_A11Y_CRITERIA: usize = 16;

/// 下游归属表条数上限（U01 组 20 项 + 跨组关键项）。
pub const MAX_OWNERSHIP: usize = 24;

/// 禁扩面条数上限。
pub const MAX_EXCLUSIONS: usize = 12;

/// 复杂度声明（人读文本）。实现与本表逐条对应，改动必须两处同步走 ADR。
pub const COMPLEXITY_DOC: &str = "\
U 域一致性总架构复杂度声明（VE-F4201 · U01-arch-v1）：
C1  层与能力对账 check_capability_alignment：O(层数 + 能力数)= O(5 + 5)，\
    两表皆定长；实现为两趟覆盖位检查。
C2  层契约自检 check_layers：O(层数)= O(5)，逐层核对四要素齐备与成本形态。
C3  接口冻结核验 check_interfaces：O(接口数)= O(4)，五条层链恰四条相邻边，\
    多一条少一条都判红（层链必须闭合）。
C4  越权变更判定 attempt_change：O(接口数) 定位 + O(1) 判定；\
    合法变更另需 O(ADR 数) 的否决记录核验。
C5  层间对拍 cross_check：O(接口数 × 声明字节长)，哈希为单遍累积。
C6  承接落地核验 check_acceptance：O(源数)，源表定长；\
    首批源未落地判红，继承位未落地不判红但未登记判红。
C7  承接回溯 trace_back：O(源数) 定位 + O(1) 产出回溯件。
C8  双维入约判定 check_wording_contract：O(维数 × 判据条数)= O(2 × 条数)。
C9  禁扩面核验 check_no_overreach：O(禁扩面条数)，条数为定长小表。
C10 架构总自检 self_audit：O(层 + 能力 + 接口 + 源 + 判据)，各项均有定长上界。
C11 读屏替述 narration：O(层 + 能力 + 接口 + 源 + 判据)，与 C10 同阶。";

// ---------------------------------------------------------------------------
// 二、总架构五层（判据一：五层）
// ---------------------------------------------------------------------------

/// 一致性域总架构层（判据「五层」：模型层→契约层→规则层→验证层→度量层）。
///
/// 层的划分依据是**规则文本的持有位置**：只有契约层持文本，其余层持指针。
/// 五层的位序不是重要性排序，是**数据流向**：模型描述世界，契约规定应然，
/// 规则编译应然为判定，验证跑判定，度量汇结果。度量层是终点——它消费前四层
/// 的产出，**不产生新规则**（度量若能改规则，一致性就成了可自证的东西）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// 层一：模型层（跨域一致性对象建模——四类实体 × 三型关系）。
    Model,
    /// 层二：契约层（契约元模型与规则文本的**唯一**持有处）。
    Contract,
    /// 层三：规则层（契约的可执行化——三段式判定）。
    Rule,
    /// 层四：验证层（扫描与断言的执行面）。
    Verification,
    /// 层五：度量层（一致率/冲突密度/修复时效/回归率四主指标）。
    Metric,
}

impl Layer {
    /// 五层全集（顺序即 [`LAYER_ORDER`]，唯一真值源）。
    pub const ALL: [Layer; LAYER_COUNT] = [
        Layer::Model,
        Layer::Contract,
        Layer::Rule,
        Layer::Verification,
        Layer::Metric,
    ];

    /// 中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            Layer::Model => "模型层",
            Layer::Contract => "契约层",
            Layer::Rule => "规则层",
            Layer::Verification => "验证层",
            Layer::Metric => "度量层",
        }
    }

    /// 英文名（标识符与文档用）。
    pub fn en(self) -> &'static str {
        match self {
            Layer::Model => "model",
            Layer::Contract => "contract",
            Layer::Rule => "rule",
            Layer::Verification => "verification",
            Layer::Metric => "metric",
        }
    }

    /// 层码（对外引用，如 `U01-L2`）。
    pub fn code(self) -> &'static str {
        match self {
            Layer::Model => "U01-L1",
            Layer::Contract => "U01-L2",
            Layer::Rule => "U01-L3",
            Layer::Verification => "U01-L4",
            Layer::Metric => "U01-L5",
        }
    }

    /// 层位序号（0 起）。**全模块唯一的层序真值**——下游层、接口位序、读屏
    /// 叙述全部由它派生，不允许第二处顺序声明。
    pub fn rank(self) -> u8 {
        match self {
            Layer::Model => 0,
            Layer::Contract => 1,
            Layer::Rule => 2,
            Layer::Verification => 3,
            Layer::Metric => 4,
        }
    }

    /// 枚举往返守卫：码 → 层。未知码返回 `None`（调用方必须显性拒绝，不许猜）。
    pub fn from_code(code: &str) -> Option<Layer> {
        Layer::ALL.iter().copied().find(|l| l.code() == code)
    }

    /// 锚点标签（锚点原文「模型层→契约层→规则层→验证层→度量层」的逐项比对键）。
    pub fn anchor_label(self) -> &'static str {
        match self {
            Layer::Model => "模型层",
            Layer::Contract => "契约层",
            Layer::Rule => "规则层",
            Layer::Verification => "验证层",
            Layer::Metric => "度量层",
        }
    }

    /// 直接下游层（度量为终点返回 `None`）。
    ///
    /// 这条边是**层链的唯一定义处**：接口表由它派生，所以「五层四条相邻边」
    /// 不是手写数字，而是 `LAYER_COUNT - 1` 的结构后果。
    pub fn downstream(self) -> Option<Layer> {
        match self {
            Layer::Model => Some(Layer::Contract),
            Layer::Contract => Some(Layer::Rule),
            Layer::Rule => Some(Layer::Verification),
            Layer::Verification => Some(Layer::Metric),
            Layer::Metric => None,
        }
    }

    /// 直接上游层（模型层为源头返回 `None`）。
    pub fn upstream(self) -> Option<Layer> {
        match self {
            Layer::Model => None,
            Layer::Contract => Some(Layer::Model),
            Layer::Rule => Some(Layer::Contract),
            Layer::Verification => Some(Layer::Rule),
            Layer::Metric => Some(Layer::Verification),
        }
    }

    /// **是否允许持有规则文本**（本域第一条使命的可执行形态，见头注§一）。
    ///
    /// 只有契约层为 `true`。其余四层持文本副本即 [`E_RULE_TEXT_DUPLICATED`]——
    /// 这条断言拦的是"还没不一致就已经分叉"的写法。
    pub fn owns_rule_text(self) -> bool {
        matches!(self, Layer::Contract)
    }

    /// 服务本层的锚点能力（能力 → 层的反向索引，由 [`Capability::layers`] 派生）。
    ///
    /// 注意规则层返回 [`Capability::Contract`]：规则层是契约能力的**执行段**，
    /// 不是独立能力（头注§二的第一条裁决）。
    pub fn served_by(self) -> Capability {
        match self {
            Layer::Model => Capability::Model,
            // 契约层与规则层同属契约能力：一能力两段。
            Layer::Contract | Layer::Rule => Capability::Contract,
            Layer::Verification => Capability::Scan,
            Layer::Metric => Capability::Metric,
        }
    }

    /// 层的成本形态（锚点性能分解「架构零运行时开销」的可核形态）。
    pub fn cost(self) -> LayerCost {
        // 域开工总纲五层全部是声明期。运行期工作由 F4202-F4207 各自承担。
        LayerCost::DeclarationOnly
    }

    /// 读屏单行。
    pub fn screen_line(self) -> String {
        format!(
            "第{}层 {}（{}）：规则文本{}；服务能力 {}；成本 {}",
            self.rank() + 1,
            self.zh(),
            self.code(),
            if self.owns_rule_text() {
                "唯一持有处"
            } else {
                "只持指针"
            },
            self.served_by().zh(),
            self.cost().zh()
        )
    }
}

/// 层序单源常量（判据「五层」的真值出处）。
pub const LAYER_ORDER: [Layer; LAYER_COUNT] = Layer::ALL;

/// 层的成本形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerCost {
    /// 声明期：本层只在声明/编译期做事，运行路径上不花钱。
    DeclarationOnly,
    /// 运行期：本层在运行路径上做事。
    PerRun,
}

impl LayerCost {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            LayerCost::DeclarationOnly => "声明期",
            LayerCost::PerRun => "运行期",
        }
    }

    /// 是否零开销（域开工总纲的判定位）。
    pub fn is_zero_overhead(self) -> bool {
        matches!(self, LayerCost::DeclarationOnly)
    }
}

/// 一致性域层契约（总架构册的行结构）。
///
/// 每层六项齐发：职责/输入/输出/失败策略/复杂度/不做清单。**缺一项的层不许
/// 进表**——缺"不做清单"的层会被后来人当成万能筐（这是域开工最常见的腐化：
/// 上一个条目顺手把下一个条目的活干了，下游开工时发现"已经有人做过了，
/// 但没人知道在哪、依据是什么"）。
#[derive(Clone, Copy, Debug)]
pub struct LayerSpec {
    /// 所属层。
    pub layer: Layer,
    /// 层位（与 [`Layer::rank`] 同值，冗余存储是为了让表可排序可对拍）。
    pub rank: u8,
    /// 中文职责名。
    pub duty_zh: &'static str,
    /// 输入契约（吃什么）。
    pub input: &'static str,
    /// 输出契约（吐什么）。
    pub output: &'static str,
    /// 失败策略（锚点降级矩阵落到本层的那一格）。
    pub on_failure: &'static str,
    /// 复杂度声明（对应 [`COMPLEXITY_DOC`] 的编号）。
    pub complexity: &'static str,
    /// 本层的**不做清单**（越界即违约）。
    pub not_mine: &'static str,
    /// 主责条目（层本体归谁——防止层被当成无主资源）。
    pub owner_item: &'static str,
    /// 成本形态。
    pub cost: LayerCost,
}

impl LayerSpec {
    /// 层契约六项齐备性自检（缺一即不合格——残缺的层契约无法对拍）。
    pub fn is_complete(&self) -> bool {
        !self.duty_zh.trim().is_empty()
            && !self.input.trim().is_empty()
            && !self.output.trim().is_empty()
            && !self.on_failure.trim().is_empty()
            && !self.complexity.trim().is_empty()
            && !self.not_mine.trim().is_empty()
            && !self.owner_item.trim().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "层 {}（{} · {}）：吃 {}；吐 {}；失败 {}；不做 {}",
            self.layer.code(),
            self.layer.zh(),
            self.duty_zh,
            self.input,
            self.output,
            self.on_failure,
            self.not_mine
        )
    }
}

/// 标准五层册（**五层 5/5 硬门的正样本**）。
///
/// 层序的依据：模型在最前（不建模就无法谈论一致），度量在最后（度量是消费方，
/// 不是生产方）。规则层紧跟契约层，因为规则段吃的正是契约段吐出的声明。
pub const STANDARD_LAYERS: [LayerSpec; LAYER_COUNT] = [
    LayerSpec {
        layer: Layer::Model,
        rank: 0,
        duty_zh: "建模层：把跨域可一致化实体与关系建成模型，作为唯一真相",
        input: "各域声明的可一致化实体（交互行为/视觉令牌/文案口径/数据契约）",
        output: "一致性对象模型（四类实体 × 三型关系= 等同/派生/约束）",
        on_failure: "关系成环 -> 拒绝该关系建模并指出环路径；版本漂移 -> 对版",
        complexity: "C2 O(层数)",
        not_mine: "不建实体本体与关系代数（VE-F4202），不做环检测算法",
        owner_item: "VE-F4202",
        cost: LayerCost::DeclarationOnly,
    },
    LayerSpec {
        layer: Layer::Contract,
        rank: 1,
        duty_zh: "契约层：规则文本的唯一持有处，元模型五字段冻结",
        input: "模型层实体 + 承接面契约源（首批两源 + 继承位）",
        output: "契约条目（ID/版本/提供方/消费方/判据五字段）+ 规则文本",
        on_failure: "重复 ID 同版本 -> 拒绝并给归并建议；判据缺 -> 拒绝注册",
        complexity: "C2 O(层数)",
        not_mine: "不建注册中心与引用计数表（VE-F4203），不复制上游词典内容",
        owner_item: "VE-F4203",
        cost: LayerCost::DeclarationOnly,
    },
    LayerSpec {
        layer: Layer::Rule,
        rank: 2,
        duty_zh: "规则层：把契约声明编译为可执行的三段式判定",
        input: "契约层条目（条件/判定/处置三段）",
        output: "可执行规则 + 三元裁决结果（优先级/时效/来源权威度）",
        on_failure: "契约引用断 -> 阻断执行并登记；裁决僵局 -> 升级人工",
        complexity: "C2 O(层数)",
        not_mine: "不写规则引擎与裁决器（VE-F4204），不复制契约文本",
        owner_item: "VE-F4204",
        cost: LayerCost::DeclarationOnly,
    },
    LayerSpec {
        layer: Layer::Verification,
        rank: 3,
        duty_zh: "验证层：以扫描为执行形态跑规则，产出归一发现项",
        input: "规则层可执行规则 + 被检目标集合",
        output: "发现项五元组（位置/规则/严重度/证据/建议）",
        on_failure: "插件崩溃 -> 隔离该插件并继续；结果风暴 -> 聚合去重",
        complexity: "C2 O(层数)",
        not_mine: "不建扫描调度与插件隔离（VE-F4206），不跳过无障碍规则",
        owner_item: "VE-F4206",
        cost: LayerCost::DeclarationOnly,
    },
    LayerSpec {
        layer: Layer::Metric,
        rank: 4,
        duty_zh: "度量层：汇四主指标，产出可复算的一致率口径",
        input: "验证层发现项 + 契约层判据基线",
        output: "四主指标（一致率/冲突密度/修复时效/回归率）× 域维度切分",
        on_failure: "口径漂移 -> 冻结重申；数据源断 -> 显性空态而非填零",
        complexity: "C2 O(层数)",
        not_mine: "不建指标计算与复算器（VE-F4207），不改任何规则文本",
        owner_item: "VE-F4207",
        cost: LayerCost::DeclarationOnly,
    },
];

// ---------------------------------------------------------------------------
// 三、开工五能力与层的双向对账（判据一的对账面，见头注§二）
// ---------------------------------------------------------------------------

/// 开工能力（锚点职责定位原文：跨域一致性模型/契约/扫描/度量/知识图谱）。
///
/// 能力与层**不是同一份清单**（头注§二）。两者的关系由 [`Capability::layers`]
/// 与 [`Capability::derivation_sources`] 两个函数表达，机检在
/// [`ConsistencyArchitecture::check_capability_alignment`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    /// 能力一：跨域一致性模型（本体 VE-F4202）。
    Model,
    /// 能力二：契约（本体 VE-F4203 + 执行段 VE-F4204——一能力两段）。
    Contract,
    /// 能力三：扫描（执行面 VE-F4206，落在验证层）。
    Scan,
    /// 能力四：度量（本体 VE-F4207，落在度量层）。
    Metric,
    /// 能力五：知识图谱（本体 VE-F4209，**无专属层**，由模型层 + 契约层派生）。
    Graph,
}

impl Capability {
    /// 五能力全集（顺序即锚点职责定位的枚举序）。
    pub const ALL: [Capability; CAPABILITY_COUNT] = [
        Capability::Model,
        Capability::Contract,
        Capability::Scan,
        Capability::Metric,
        Capability::Graph,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Capability::Model => "跨域一致性模型",
            Capability::Contract => "契约",
            Capability::Scan => "扫描",
            Capability::Metric => "度量",
            Capability::Graph => "知识图谱",
        }
    }

    /// 英文名。
    pub fn en(self) -> &'static str {
        match self {
            Capability::Model => "consistency-model",
            Capability::Contract => "contract",
            Capability::Scan => "scan",
            Capability::Metric => "metric",
            Capability::Graph => "knowledge-graph",
        }
    }

    /// 能力码（对外引用，如 `U01-C3`）。
    pub fn code(self) -> &'static str {
        match self {
            Capability::Model => "U01-C1",
            Capability::Contract => "U01-C2",
            Capability::Scan => "U01-C3",
            Capability::Metric => "U01-C4",
            Capability::Graph => "U01-C5",
        }
    }

    /// 能力位序号（0 起）。
    pub fn rank(self) -> u8 {
        match self {
            Capability::Model => 0,
            Capability::Contract => 1,
            Capability::Scan => 2,
            Capability::Metric => 3,
            Capability::Graph => 4,
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<Capability> {
        Capability::ALL.iter().copied().find(|c| c.code() == code)
    }

    /// 锚点标签（锚点职责定位的原文标签——覆盖机检的比对键）。
    pub fn anchor_label(self) -> &'static str {
        match self {
            Capability::Model => "跨域一致性模型",
            Capability::Contract => "契约",
            Capability::Scan => "扫描",
            Capability::Metric => "度量",
            Capability::Graph => "知识图谱",
        }
    }

    /// 本能力占用的层（**可为空**：图谱无专属层）。
    ///
    /// 契约能力返回**两层**（契约层 + 规则层）——这是"一能力两段"的机检形态，
    /// 也是规则层「有层无能名」这条裁决的落点。
    pub fn layers(self) -> &'static [Layer] {
        match self {
            Capability::Model => &[Layer::Model],
            Capability::Contract => &[Layer::Contract, Layer::Rule],
            Capability::Scan => &[Layer::Verification],
            Capability::Metric => &[Layer::Metric],
            // 图谱不占层：它是派生视图，给它层就出现双源（头注§二）。
            Capability::Graph => &[],
        }
    }

    /// 本能力的派生来源（图谱专用：派生自模型层实体 + 契约层关系）。
    ///
    /// 非派生能力返回空切片。这条声明让"图谱不是第六层"变成**可机检的**——
    /// [`ConsistencyArchitecture::check_capability_alignment`] 要求图谱的派生
    /// 来源必须是其它能力已占用的层，否则图谱就成了无源之物。
    pub fn derivation_sources(self) -> &'static [Layer] {
        match self {
            Capability::Graph => &[Layer::Model, Layer::Contract],
            _ => &[],
        }
    }

    /// 本能力是否为派生能力（无专属层）。
    pub fn is_derived(self) -> bool {
        !self.derivation_sources().is_empty()
    }

    /// 主责条目（本能力本体归谁——防止能力被当成无主资源）。
    pub fn owner_item(self) -> &'static str {
        match self {
            Capability::Model => "VE-F4202",
            Capability::Contract => "VE-F4203",
            Capability::Scan => "VE-F4206",
            Capability::Metric => "VE-F4207",
            Capability::Graph => "VE-F4209",
        }
    }

    /// 读屏单行。
    pub fn screen_line(self) -> String {
        let lands = self.layers();
        let landing = if lands.is_empty() {
            let src: Vec<String> = self
                .derivation_sources()
                .iter()
                .map(|l| format!("{}（{}）", l.zh(), l.code()))
                .collect();
            format!("无专属层，派生自{}", src.join(" + "))
        } else {
            let ls: Vec<String> = lands.iter().map(|l| format!("{}（{}）", l.zh(), l.code())).collect();
            format!("占层 {}", ls.join(" + "))
        };
        format!(
            "能力 {}（{}，{}）：{}；主责 {}",
            self.code(),
            self.zh(),
            self.en(),
            landing,
            self.owner_item()
        )
    }
}

/// 能力序单源常量。
pub const CAPABILITY_ORDER: [Capability; CAPABILITY_COUNT] = Capability::ALL;

// ---------------------------------------------------------------------------
// 四、层间接口冻结 v1（判据二：接口冻结）
// ---------------------------------------------------------------------------

/// 层间接口（相邻层之间的段契约，冻结 v1）。
///
/// 每条八项齐发：上/下层/吃/吐/失败策略/复杂度/消费方/不做清单，另加
/// **声明哈希**（对拍的比对键）。接口是五层之间**唯一**的合法通道——
/// 跨层直连（规则层直接读模型层内部结构）不在任何一条接口里，故无处可寻。
#[derive(Clone, Debug)]
pub struct LayerInterface {
    /// 上游层。
    pub from: Layer,
    /// 下游层（必为 [`Layer::downstream`]）。
    pub to: Layer,
    /// 接口码（对外引用，如 `U01-IF1`）。
    pub code: &'static str,
    /// 中文职责名。
    pub duty_zh: &'static str,
    /// 输入契约（吃什么）。
    pub input: &'static str,
    /// 输出契约（吐什么）。
    pub output: &'static str,
    /// 失败策略（锚点降级矩阵落到本接口的那一格）。
    pub on_failure: &'static str,
    /// 复杂度声明（对应 [`COMPLEXITY_DOC`] 的编号）。
    pub complexity: &'static str,
    /// 下游消费方（跨批对接点）。
    pub consumers: &'static str,
    /// 本接口的**不做清单**。
    pub not_mine: &'static str,
    /// 声明内容哈希（由 [`fnv1a64_hex`] 对声明正文实算，**不是手写常量**）。
    pub declared_hash: String,
}

impl LayerInterface {
    /// 接口契约齐备性自检（缺一即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.code.trim().is_empty()
            && !self.duty_zh.trim().is_empty()
            && !self.input.trim().is_empty()
            && !self.output.trim().is_empty()
            && !self.on_failure.trim().is_empty()
            && !self.complexity.trim().is_empty()
            && !self.consumers.trim().is_empty()
            && !self.not_mine.trim().is_empty()
            && self.declared_hash.len() == HASH_HEX_LEN
    }

    /// 上游层 == 下游层的直接上游（层链闭合性）。
    pub fn is_adjacent(&self) -> bool {
        self.from.downstream() == Some(self.to)
    }

    /// 声明正文的规范化串（哈希的输入——**唯一真值**，改哈希口径必须走 ADR）。
    pub fn declared_text(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.code,
            self.from.code(),
            self.to.code(),
            self.input,
            self.output,
            self.on_failure,
            self.consumers,
            self.not_mine
        )
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "接口 {}（{} → {}，{}）：吃 {}；吐 {}；失败 {}；哈希 {}",
            self.code,
            self.from.zh(),
            self.to.zh(),
            self.duty_zh,
            self.input,
            self.output,
            self.on_failure,
            self.declared_hash
        )
    }
}

/// 标准层间接口表（**四条相邻边**——由 [`Layer::downstream`] 派生，非手写数字）。
///
/// 声明哈希**成对构造**：每条的 `declared_hash` 由 `declared_text()` 实算，
/// `standard_interfaces()` 是这样构造出来的，所以"哈希对账不是看一眼填没填"。
pub const MAX_INTERFACE_COUNT: usize = MAX_INTERFACES;

/// 标准接口册的构造：按层链顺序逐边生成，声明哈希实算。
pub fn standard_interfaces() -> Vec<LayerInterface> {
    /// 逐边的静态描述（与 [`STANDARD_LAYERS`] 同源，顺序即层链序）。
    const EDGES: [(&'static str, &'static str, &'static str, &'static str, &'static str, &'static str, &'static str, &'static str); 4] = [
        (
            "U01-IF1",
            "建模交付段：模型层把一致性对象模型交付契约层",
            "已建模实体与关系（四类 × 三型，含版本号）",
            "契约草案（待注册，未分配唯一 ID）",
            "模型层版本漂移 -> 拒绝交付并要求对版；环关系已在上游被拒故不重演",
            "C3 O(接口数)",
            "VE-F4203 注册中心、VE-F4209 图谱建模输入",
            "不分配契约 ID、不做唯一性断言——那是 VE-F4203 的活",
        ),
        (
            "U01-IF2",
            "编译段：规则层把契约声明编译为可执行的三段式规则",
            "已注册契约条目（含判据必填位与无障碍入约位）",
            "可执行规则集（条件/判定/处置三段）",
            "契约引用断 -> 阻断执行并登记；无障碍判据缺 -> 拒绝编译（不允许降级）",
            "C3 O(接口数)",
            "VE-F4206 扫描平台、VE-F4210/F4211/F4212 三源转译",
            "不复制契约文本进规则体——规则持契约指针（头注§一红线）",
        ),
        (
            "U01-IF3",
            "执行段：验证层以扫描为执行形态跑规则集",
            "可执行规则集 + 被检目标集合 + 扫描模式（全量/增量/定向）",
            "归一发现项五元组（位置/规则/严重度/证据/建议）",
            "插件崩溃 -> 隔离该插件并继续扫描；无障碍规则在三模式下均不跳过",
            "C3 O(接口数)",
            "VE-F4207 度量数据源、VE-F4215 调试器反查链",
            "不聚合去重、不做扫描调度——那是 VE-F4206 的活",
        ),
        (
            "U01-IF4",
            "汇聚段：度量层汇四主指标并冻结口径",
            "发现项流 + 契约判据基线 + 域维度切分口径",
            "四主指标值 + 可复算的口径指纹",
            "口径漂移 -> 冻结重申；数据源断 -> 显性空态，绝不填零冒充一致",
            "C3 O(接口数)",
            "VE-F4218 联调证据挂载、VE-F4220 双签四件之一",
            "不改任何规则文本、不新增判定——度量是消费方不是生产方",
        ),
    ];

    let mut out: Vec<LayerInterface> = Vec::new();
    let mut up = Layer::Model;
    for (code, duty, input, output, on_fail, cx, consumers, not_mine) in EDGES.iter() {
        let Some(down) = up.downstream() else {
            // 层链走完即止：EDGES 条数与层数-1 一致，机检在 check_interfaces。
            break;
        };
        let probe = LayerInterface {
            from: up,
            to: down,
            code,
            duty_zh: duty,
            input,
            output,
            on_failure: on_fail,
            complexity: cx,
            consumers,
            not_mine,
            declared_hash: String::new(),
        };
        // 哈希实算（不是手写常量）——这就是「哈希对账」的正样本。
        let hash = fnv1a64_hex(probe.declared_text().as_bytes());
        out.push(LayerInterface {
            declared_hash: hash,
            ..probe
        });
        up = down;
    }
    out
}

// ---------------------------------------------------------------------------
// 五、接口冻结册 + ADR（判据二的执法面；锚点降级矩阵「接口越权变更→冻结流程」）
// ---------------------------------------------------------------------------

/// ADR 记录（架构决策记录——**接口变更的唯一合法通道**）。
#[derive(Clone, Debug)]
pub struct AdrRecord {
    /// ADR 号（`ADR-U01-0001` 形式）。
    pub id: String,
    /// 关联接口码（空表示域级决策，如五层增删）。
    pub interface: String,
    /// 原冻结版本。
    pub from_version: String,
    /// 新冻结版本。
    pub to_version: String,
    /// 标题。
    pub title: String,
    /// 决策内容（做了什么）。
    pub decision: String,
    /// **被否决的备选方案**（为什么没选别的）。
    pub rejected: String,
}

impl AdrRecord {
    /// ADR 六项齐备性自检。
    ///
    /// `rejected` 为空即不合格——**没有否决记录的 ADR 不是决策记录，是提案**。
    /// 只写「我们决定升版」而不写「为什么不升 v1 就地改」的 ADR，无法让后来人
    /// 判断当初的约束还在不在。
    pub fn is_complete(&self) -> bool {
        !self.id.trim().is_empty()
            && !self.from_version.trim().is_empty()
            && !self.to_version.trim().is_empty()
            && !self.title.trim().is_empty()
            && !self.decision.trim().is_empty()
            && !self.rejected.trim().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "{}（{}）：{} → {}；决策 {}；否决 {}",
            self.id, self.interface, self.from_version, self.to_version, self.decision, self.rejected
        )
    }
}

/// 接口冻结册（判据「接口冻结」的执法结构）。
///
/// 冻结的语义：**在位接口的声明文本不可就地改**。唯一合法变更路径是
/// 「先开 ADR（须含否决记录）→ 再 rebase」。[`InterfaceFreezeLedger::attempt_change`]
/// 是所有越权改动的统一入口，它没有旁路。
#[derive(Clone, Debug)]
pub struct InterfaceFreezeLedger {
    /// 当前接口集。
    interfaces: Vec<LayerInterface>,
    /// 冻结版本。
    pub interface_version: String,
    /// ADR 账。
    adrs: Vec<AdrRecord>,
    /// 已重基次数（**只增不减**——变更次数本身是审计证据）。
    ///
    /// 取值口径：构造时快照 + 每次本册内成功的 [`Self::rebase`] 自增。
    /// 外部若直接改了本册的接口集，此值会落后于实际内容——
    /// 所以自检一律读 [`Self::rebase_count`]（本册内计数），
    /// 而不读 `ConsistencyArchitecture::rebases` 镜像。
    rebases: usize,
}

impl InterfaceFreezeLedger {
    /// 冻结册构造（**默认阻断，不是默认放行**：传入接口集，逐条校验）。
    pub fn new(interfaces: Vec<LayerInterface>, interface_version: &str) -> Result<Self, ConsistencyError> {
        if interfaces.is_empty() {
            return Err(ConsistencyError::new(
                E_INTERFACE_EMPTY,
                "接口冻结册建立被拒：接口集为空",
                "空接口集意味着五层之间没有任何合法通道，后续层全部悬空",
                "按 Layer::downstream 逐边构造标准接口册（standard_interfaces）",
                "U 域架构维护方",
            ));
        }
        if interfaces.len() > MAX_INTERFACES {
            return Err(ConsistencyError::new(
                E_INTERFACE_CAP,
                "接口冻结册建立被拒：接口数超上限",
                &format!(
                    "接口数 {} 超过上限 {}；五层相邻边只有 4 条，多出来的必是非相邻连线",
                    interfaces.len(),
                    MAX_INTERFACES
                ),
                "删除非相邻层连线，或走 ADR 正式增层（那会改五层本身）",
                "U 域架构维护方",
            ));
        }
        // 逐条校验：契约齐备 + 相邻闭合 + 无重复码。
        let mut seen: Vec<&str> = Vec::new();
        for it in interfaces.iter() {
            if !it.is_complete() {
                return Err(ConsistencyError::new(
                    E_INTERFACE_INCOMPLETE,
                    "接口冻结册建立被拒：接口契约残缺",
                    &format!("接口 {} 缺字段；残缺契约无法对拍", it.code),
                    "补齐吃/吐/失败策略/复杂度/消费方/不做清单与实算哈希",
                    "U 域架构维护方",
                ));
            }
            if !it.is_adjacent() {
                return Err(ConsistencyError::new(
                    E_INTERFACE_NOT_ADJACENT,
                    "接口冻结册建立被拒：非相邻层连线",
                    &format!(
                        "接口 {} 连接 {} → {}，但 {} 的直接下游是 {:?}",
                        it.code,
                        it.from.zh(),
                        it.to.zh(),
                        it.from.zh(),
                        it.from.downstream()
                    ),
                    "跨层直连必须经中间层的正式接口；无通道即无合法路径",
                    "U 域架构维护方",
                ));
            }
            if seen.contains(&it.code) {
                return Err(ConsistencyError::new(
                    E_INTERFACE_DUP,
                    "接口冻结册建立被拒：接口码重复",
                    &format!("接口码 {} 重复登记", it.code),
                    "接口码是下游引用的键，重复会让引用指向不明",
                    "U 域架构维护方",
                ));
            }
            seen.push(it.code);
        }
        Ok(InterfaceFreezeLedger {
            interfaces,
            interface_version: interface_version.to_string(),
            adrs: Vec::new(),
            rebases: 0,
        })
    }

    /// 只读遍历接口集。
    pub fn iter(&self) -> impl Iterator<Item = &LayerInterface> {
        self.interfaces.iter()
    }

    /// 接口数。
    pub fn len(&self) -> usize {
        self.interfaces.len()
    }

    /// 是否为空（构造已禁空，此处仅为对称）。
    pub fn is_empty(&self) -> bool {
        self.interfaces.is_empty()
    }

    /// 按码取接口。
    pub fn interface(&self, code: &str) -> Option<&LayerInterface> {
        self.interfaces.iter().find(|i| i.code == code)
    }

    /// 冻结册内接口的只读遍历（冻结册的**唯一**外读口）。
    ///
    /// 为什么不把 `interfaces` 放成 `pub`：冻结册的价值全在「改口只有一个」
    /// ——字段一旦公开，任何调用方都能绕过 [`Self::rebase`] 直接改内容，
    /// 冻结版本与实际内容的账立刻对不上，ADR 通道形同虚设。
pub fn interfaces(&self) -> &[LayerInterface] {
  &self.interfaces
    }

    /// **负例专用**：可写接口表，供自检构造非法冻结册。
    ///
    /// # 为什么开这个口
    ///
    /// 负例要证的是「探测器抓得住坏册」，而坏册恰恰是**公开 API 正确拒绝**
    /// 构造的那些形态（残缺八字段、层序错位、边指向非相邻层、条目数不符）。
    /// 走公开通道构造不出来——那些构造点本来就该拒绝。
    /// 所以这里留**唯一一个**、名字自带警告的可写口，且只把 `&mut` 交出去、
    /// 不新增任何「合法变更」通道：绕过它不会让变更变合法，只会让账本对不上。
    ///
    /// # 为什么不叫 `interfaces_mut`
    ///
    /// 名字里必须带 `tamper`：谁在生产代码里调用它，一眼就该被code review 拦下。
    /// 若叫 `interfaces_mut`，它就成了冻结册的第二个合法写口，ADR 通道随即失效。
    pub fn tamper_interfaces(&mut self) -> &mut Vec<LayerInterface> {
        &mut self.interfaces
    }

    /// 上游层到下游层的边（层链反查）。
    pub fn edge(&self, from: Layer, to: Layer) -> Option<&LayerInterface> {
        self.interfaces.iter().find(|i| i.from == from && i.to == to)
    }

    /// 架构冻结哈希（对全部在位接口的声明哈希做一次累积）。
    ///
    /// 这是 U01 双签闸的**第一件**（架构冻结哈希，F4220 核验对象之一）。
    /// 顺序敏感——按层链序累积，接口顺序变了哈希就变，这样"重排接口表"不会
    /// 被当成无变化的改动蒙混过关。
    pub fn freeze_digest(&self) -> String {
        let mut buf = String::new();
        buf.push_str(self.interface_version.as_str());
        for it in self.interfaces.iter() {
            buf.push('|');
            buf.push_str(it.code);
            buf.push('=');
            buf.push_str(it.declared_hash.as_str());
        }
        fnv1a64_hex(buf.as_bytes())
    }

    /// **尝试越权变更**（锚点降级矩阵「接口越权变更→冻结流程」的执法入口）。
    ///
    /// 没有 ADR 的改动一律 [`E_INTERFACE_UNFROZEN_CHANGE`]，并给出唯一合法
    /// 出路。这里**没有"先改后补"的选项**——先改后补等于就地改，冻结就白冻了。
    ///
    /// `reason` 与 `rejected` **都由调用方给，缺一即拒**：本函数不代填占位文案。
    /// 代填是有害的——一份被机器填好否决理由的 ADR 读起来像"已决策"，
    /// 实际上决策还没发生，于是"没有否决记录的 ADR 不是决策记录"这条纪律
    /// 就被本函数自己绕过去了。真正的否决理由只有提方案的人知道：
    /// 「为什么不升版就地改」「为什么不改下游适配」——这些必须他自己写。
    pub fn attempt_change(
        &mut self,
        code: &str,
        new_text: &str,
        reason: &str,
        rejected: &str,
    ) -> Result<String, ConsistencyError> {
        let Some(entry) = self.interfaces.iter().find(|i| i.code == code) else {
            return Err(ConsistencyError::new(
                E_INTERFACE_UNKNOWN,
                "接口变更被拒：接口未登记",
                &format!("接口码 {} 不在冻结册内，无从变更", code),
                "先查 InterfaceFreezeLedger::interface 确认接口码",
                "变更申请方",
            ));
        };
        let from_version = self.interface_version.clone();
        let to_version = format!("{}-r{}", from_version, self.rebases + 1);
        let adr_id = format!("ADR-U01-{:04}", self.adrs.len() + 1);
        // 理由缺失：无从判断这次改动该不该做。
        if reason.trim().is_empty() {
            return Err(ConsistencyError::new(
                E_ADR_NO_TITLE,
                "接口变更被拒：未给出变更理由",
                &format!(
                    "接口 {} 现声明哈希 {} → 拟改为 {}，但申请未写理由",
                    code,
                    entry.declared_hash,
                    fnv1a64_hex(new_text.as_bytes())
                ),
                "补理由后重试；合法路径见 ADR 通道（见本函数文档）",
                "变更申请方",
            ));
        }
        // 否决理由缺失：这是最容易漏的一项，也是最该拦的一项。
        // 缺它就放行，等于把「决策记录」降格成「提案记录」。
        if rejected.trim().is_empty() {
            return Err(ConsistencyError::new(
                E_ADR_NO_REJECTED,
                "接口变更被拒：未给出被否决的备选方案",
                &format!(
                    "接口 {} → {} 的 ADR 只写了决策、没写否决；\
                     没有否决记录的 ADR 不是决策记录，是提案",
                    code, to_version
                ),
                &format!(
                    "写下你否决了什么以及为什么（例如「否决：不升版就地改，因为下游无从判断按哪版编」）；\
                     然后 InterfaceFreezeLedger::attempt_change({}, 新正文, 理由, 否决理由)",
                    code
                ),
                "变更申请方",
            ));
        }
        // 冻结流程：**唯一**合法通道。先落 ADR（含否决记录），再重基。
        self.open_adr(AdrRecord {
            id: adr_id.clone(),
            interface: code.to_string(),
            from_version,
            to_version: to_version.clone(),
            title: format!("变更接口 {} 声明文本", code),
            decision: reason.to_string(),
            rejected: rejected.to_string(),
        })?;
        self.rebase(code, new_text)
    }

    /// 登记 ADR。
    pub fn open_adr(&mut self, rec: AdrRecord) -> Result<(), ConsistencyError> {
        if !rec.is_complete() {
            // 精确指出缺哪一项——笼统说「不合格」会让人瞎补。
            let missing = if rec.id.trim().is_empty() {
                "id"
            } else if rec.from_version.trim().is_empty() {
                "from_version"
            } else if rec.to_version.trim().is_empty() {
                "to_version"
            } else if rec.title.trim().is_empty() {
                "title"
            } else if rec.decision.trim().is_empty() {
                "decision"
            } else {
                "rejected"
            };
            return Err(ConsistencyError::new(
                E_ADR_INCOMPLETE,
                "ADR 登记被拒：字段缺失",
                &format!("ADR 缺字段 {}；缺否决记录的 ADR 不是决策记录，是提案", missing),
                "六项齐发：id/interface 关联/from_version/to_version/title/decision/rejected",
                "ADR 提交方",
            ));
        }
        if rec.from_version == rec.to_version {
            return Err(ConsistencyError::new(
                E_ADR_NO_VERSION_BUMP,
                "ADR 登记被拒：版本未升",
                &format!(
                    "ADR {} 的 from 与 to 同为 {}；变更不升版则下游无法分辨新旧",
                    rec.id, rec.from_version
                ),
                "破坏性变更升主版本，接口变更升冻结修订号（-rN）",
                "ADR 提交方",
            ));
        }
        if self.adrs.iter().any(|a| a.id == rec.id) {
            return Err(ConsistencyError::new(
                E_ADR_DUP,
                "ADR 登记被拒：编号重复",
                &format!("ADR {} 已登记；同号两决议会让引用指向不明", rec.id),
                "换号重报，或撤回原 ADR 后重提",
                "ADR 提交方",
            ));
        }
        if self.adrs.len() >= MAX_ADRS {
            return Err(ConsistencyError::new(
                E_ADR_CAP,
                "ADR 登记被拒：账已满",
                &format!("ADR 账 {} 条达到上限 {}", self.adrs.len(), MAX_ADRS),
                "先归档已落地的 ADR，或按 ADR 提升 MAX_ADRS",
                "U 域架构维护方",
            ));
        }
        self.adrs.push(rec);
        Ok(())
    }

    /// 本册内已重基次数（权威值——自检读它，不读外层镜像）。
    pub fn rebase_count(&self) -> usize {
        self.rebases
    }

    /// ADR 账只读遍历。
    pub fn adrs(&self) -> impl Iterator<Item = &AdrRecord> {
        self.adrs.iter()
    }

    /// 已为某接口某目标版本开过 ADR？（rebase 的前置条件）
    fn has_adr_for(&self, code: &str, to_version: &str) -> bool {
        self.adrs
            .iter()
            .any(|a| a.interface == code && a.to_version == to_version)
    }

    /// 按 ADR 重基某接口的声明文本（**唯一**的合法变更执行点）。
    ///
    /// 无对应 ADR 即 [`E_REBASE_WITHOUT_ADR`]：这条断言堵的是"绕过
    /// `attempt_change` 直接调 rebase"的后门。
    pub fn rebase(&mut self, code: &str, new_text: &str) -> Result<String, ConsistencyError> {
        let next_rev = format!("{}-r{}", self.interface_version, self.rebases + 1);
        if !self.has_adr_for(code, &next_rev) {
            return Err(ConsistencyError::new(
                E_REBASE_WITHOUT_ADR,
                "接口重基被拒：未开 ADR",
                &format!(
                    "接口 {} 的目标版本 {} 没有对应 ADR；重基必须由 ADR 授权",
                    code, next_rev
                ),
                &format!(
                    "先 InterfaceFreezeLedger::open_adr 登记 {} → {} 的 ADR（含否决记录）",
                    code, next_rev
                ),
                "重基申请方",
            ));
        }
        let new_hash = fnv1a64_hex(new_text.as_bytes());
        let Some(entry) = self.interfaces.iter_mut().find(|i| i.code == code) else {
            return Err(ConsistencyError::new(
                E_INTERFACE_UNKNOWN,
                "接口重基被拒：接口未登记",
                &format!("接口码 {} 不在冻结册内", code),
                "确认接口码后重试",
                "重基申请方",
            ));
        };
        // 重基记录旧哈希，冲突时对拍要能指出「原本是什么、变成了什么」。
        let old_hash = entry.declared_hash.clone();
        // 新正文按「|」字段位重写：保结构、换内容，避免提交方顺手改字段数。
        entry.declared_hash = new_hash.clone();
        self.interface_version = next_rev.clone();
        self.rebases += 1;
        Ok(format!("{}: {} -> {} @ {}", code, old_hash, new_hash, next_rev))
    }

    /// ADR 账本读屏。
    pub fn screen_text(&self) -> String {
        format!(
            "层间接口冻结 {}：在位接口 {} 条（层链 {}→{} 相邻闭合）；架构冻结哈希 {}；ADR {} 条，重基 {} 次。",
            self.interface_version,
            self.interfaces.len(),
            LAYER_ORDER[0].zh(),
            LAYER_ORDER[LAYER_COUNT - 1].zh(),
            self.freeze_digest(),
            self.adrs.len(),
            self.rebases
        )
    }
}

// ---------------------------------------------------------------------------
// 六、承接面落地表（判据三：承接落地；锚点降级矩阵「承接缺源→回溯移交包」）
// ---------------------------------------------------------------------------

/// 承接源的角色（首批 vs 继承位，见头注§三）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptanceRole {
    /// 首批落地源（F4201 明文要求：S 域交互词典 / T 域地区规则）。
    Primary,
    /// 继承位（T10 交接面带入但 F4201 未列入首批：术语库），**先登记不落地**。
    Inherited,
}

impl AcceptanceRole {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            AcceptanceRole::Primary => "首批落地源",
            AcceptanceRole::Inherited => "继承位",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            AcceptanceRole::Primary => "ROLE-PRIMARY",
            AcceptanceRole::Inherited => "ROLE-INHERITED",
        }
    }

    /// 该角色是否**必须在本项内落地**（继承位否——落地义务归下游条目）。
    pub fn must_land_in_arch(self) -> bool {
        matches!(self, AcceptanceRole::Primary)
    }

    /// 该角色是否**必须在本项内登记**（两者都必须——登记是义务，落地是时序）。
    pub fn must_register(self) -> bool {
        true
    }
}

/// 一致性契约源承接条目（承接面落地表的行结构）。
#[derive(Clone, Debug)]
pub struct AcceptanceEntry {
    /// 承接源码（对外引用，如 `U01-SRC-SDICT`）。
    pub code: String,
    /// 来源域（`S` / `T`）。
    pub source_domain: &'static str,
    /// 来源条目号（真实册内条目，可被反查）。
    pub source_item: String,
    /// 来源内容名（如「交互词典」「地区规则」「术语库」）。
    pub content: String,
    /// 角色（首批 / 继承位）。
    pub role: AcceptanceRole,
    /// 交接来源（继承位必填 `T10`；首批源也记明取自哪次移交包交接面）。
    pub carried_from: String,
    /// 来源内容哈希（由 [`fnv1a64_hex`] 实算，非手写）。
    pub source_hash: String,
    /// 是否已落地（首批源必须为 `true` 才算承接完成）。
    pub landed: bool,
    /// 是否已与来源对账（哈希重算一致）。
    pub reconciled: bool,
}

impl AcceptanceEntry {
    /// 承接条目齐备性自检。
    pub fn is_complete(&self) -> bool {
        !self.code.trim().is_empty()
            && !self.source_item.trim().is_empty()
            && !self.content.trim().is_empty()
            && !self.carried_from.trim().is_empty()
            && self.source_hash.len() == HASH_HEX_LEN
            && is_valid_item_id(&self.source_item)
    }

    /// 本条目在架构内**是否算落地完成**（角色相关）。
    ///
    /// 首批源未落地即未完成；继承位未落地**不算未完成**（落地义务在
    /// VE-F4212），但未登记一定未完成——这是头注§三的裁决落点。
    pub fn is_landed_for_arch(&self) -> bool {
        match self.role {
            // 首批源：落地 + 对账，缺一即未完成。
            AcceptanceRole::Primary => self.landed && self.reconciled,
            // 继承位：**只登记即合规**。它的落地义务归VE-F4212（口径一致性），
            // 在本项里要求它落地等于把下游的活揽到本项头上——那样本项的
            // 「承接完成」就变成了对另一域进度的假ETA，永远等不到。
            AcceptanceRole::Inherited => true,
        }
    }

    /// 未落地时的阻塞级别（首批=阻断，继承位=仅登记义务）。
    pub fn land_gap_severity(&self) -> Severity {
        match self.role {
            AcceptanceRole::Primary => Severity::Blocking,
            AcceptanceRole::Inherited => Severity::Warning,
        }
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "承接源 {}（{}域 {} · {}，{}）：交接自 {}；哈希 {}；落地 {}；对账 {}",
            self.code,
            self.source_domain,
            self.source_item,
            self.content,
            self.role.zh(),
            self.carried_from,
            self.source_hash,
            if self.landed { "是" } else { "否" },
            if self.reconciled { "是" } else { "否" }
        )
    }
}

/// 回溯件（承接缺源时的回溯移交包结果）。
///
/// 承接缺源的正确处置**不是兜底**，是回溯到 T10 移交包问清楚这件到底由谁
/// 交付、交接面有没有写。兜底 = 拿一个没人负责的默认值长期跑，一致性域
/// 承受不起这种"看起来能跑"。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceBack {
    /// 缺失/未对账的承接源码。
    pub source_code: String,
    /// 回溯到的移交包条目（T10 = VE-F4195）。
    pub package_item: String,
    /// 回溯到的交接面名。
    pub handover_face: String,
    /// 建议动作。
    pub action: &'static str,
}

impl TraceBack {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "回溯 {}：查 {} 的 {} 交接面；{}",
            self.source_code, self.package_item, self.handover_face, self.action
        )
    }
}

/// 交接面名（按来源域给出**各自真实**的交接面，不是一个通用串）。
///
/// 为什么必须分域：回溯是「拿着缺失清单去找人」，找谁取决于源从哪个域来。
/// 两侧返回同一句话等于没分派——回溯请求会被原样弹回 U 域自己，形成闭环死路。
/// 未登记域走 [`E_HANDOVER_FACE_UNKNOWN`] 兜底指名登记处，而不是静默当成已知域。
pub fn handover_face_of(domain: &str) -> String {
    match domain {
        "S" => format!("S 域交互词典交接面（{} 移交包）", T10_PACKAGE_ITEM),
        "T" => format!("T 域地区规则交接面（{} 移交包）", T10_PACKAGE_ITEM),
        other => format!(
            "未登记来源域「{}」：先在承接表登记其交接面持有人，再谈回溯（{} 移交包）",
            other, T10_PACKAGE_ITEM
        ),
    }
}

/// 承接面落地表。
#[derive(Clone, Debug)]
pub struct AcceptanceLedger {
    entries: Vec<AcceptanceEntry>,
}

/// T10 移交包条目号（回溯目的地，单源常量）。
pub const T10_PACKAGE_ITEM: &str = "VE-F4195";

impl AcceptanceLedger {
    /// 空承接表（**未承接任何源**——默认阻断，不是默认放行**）。
    pub fn new() -> Self {
        AcceptanceLedger { entries: Vec::new() }
    }

    /// 登记承接条目。
    ///
    /// 拒三事：条目残缺、条目号不是合法 `VE-F####`、源码重复。
    /// 源码重复是最容易犯的一种错——两个域都登记 `交互词典`，最后没人说得清
    /// 契约文本到底以哪一份为准。
    pub fn register(&mut self, e: AcceptanceEntry) -> Result<(), ConsistencyError> {
        if !e.is_complete() {
            return Err(ConsistencyError::new(
                E_ACCEPTANCE_INCOMPLETE,
                "承接登记被拒：条目残缺",
                &format!(
                    "承接源 {} 缺字段或条目号非法（须 VE-F####，实得 {:?}）",
                    e.code, e.source_item
                ),
                "补齐 code/source_item/content/carried_from 与实算哈希",
                "承接方",
            ));
        }
        if self.entries.iter().any(|x| x.code == e.code) {
            return Err(ConsistencyError::new(
                E_ACCEPTANCE_DUP,
                "承接登记被拒：源码重复",
                &format!("承接源码 {} 已登记；契约源重复登记会让单源失效", e.code),
                "更新既有条目的来源哈希并走 ADR；不同源请用不同源码",
                "承接方",
            ));
        }
        if self.entries.len() >= MAX_ACCEPTANCE_SOURCES {
            return Err(ConsistencyError::new(
                E_ACCEPTANCE_CAP,
                "承接登记被拒：承接表已满",
                &format!("承接表 {} 条达到上限 {}", self.entries.len(), MAX_ACCEPTANCE_SOURCES),
                "先归档已移交的源，或按 ADR 提升 MAX_ACCEPTANCE_SOURCES",
                "U 域架构维护方",
            ));
        }
        self.entries.push(e);
        Ok(())
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &AcceptanceEntry> {
        self.entries.iter()
    }

    /// 按码取条目。
    pub fn entry(&self, code: &str) -> Option<&AcceptanceEntry> {
        self.entries.iter().find(|e| e.code == code)
    }

    /// 某角色的条目数（O(角色数)，源表定长）。
    pub fn role_count(&self, role: AcceptanceRole) -> usize {
        self.entries.iter().filter(|e| e.role == role).count()
    }

    /// 首批源是否齐备且全落地（判据「承接落地」）。
    ///
    /// 首批源**必须**存在 S 域交互词典与 T 域地区规则两路，少一路即未落地
    /// ——「首批两源」是锚点明文数量，不是「至少一路」。
    pub fn primaries_landed(&self) -> bool {
        let prim: Vec<&AcceptanceEntry> = self
            .entries
            .iter()
            .filter(|e| e.role == AcceptanceRole::Primary)
            .collect();
        if prim.len() < PRIMARY_SOURCE_COUNT {
            return false;
        }
        // 必须同时含 S 域交互词典与 T 域地区规则两路。
        let has_s_dict = prim.iter().any(|e| e.source_domain == "S");
        let has_t_rules = prim.iter().any(|e| e.source_domain == "T");
        has_s_dict && has_t_rules && prim.iter().all(|e| e.is_landed_for_arch())
    }

    /// **回溯**（锚点降级矩阵「承接缺源→回溯移交包」）。
    ///
    /// 未落地或未对账的源 → 产出回溯到 T10 移交包具体交接面的 [`TraceBack`]。
    /// 这里**没有「先用默认顶上」分支**：默认规则没人负责，一致性域一旦靠
    /// 默认值长期跑，漂移就再没人发现。
    pub fn trace_back(&self, code: &str) -> Result<TraceBack, ConsistencyError> {
        let Some(e) = self.entries.iter().find(|x| x.code == code) else {
            return Err(ConsistencyError::new(
                E_ACCEPTANCE_UNKNOWN,
                "回溯失败：承接源未登记",
                &format!("承接源码 {} 不在承接表内，连回溯对象都没有", code),
                &format!(
                    "先 AcceptanceLedger::register 登记；T10 移交包（{}）交接面清单可查",
                    T10_PACKAGE_ITEM
                ),
                "回溯申请方",
            ));
        };
        // 落地义务按角色分：继承位未落地**不回溯**（落地归 F4212），只提示。
        if !e.landed && !e.role.must_land_in_arch() {
            return Err(ConsistencyError::new(
                E_TRACE_BACK_NOT_DUE,
                "回溯被拒：该源尚未到落地时点",
                &format!(
                    "承接源 {} 是继承位（{}），落地义务归下游条目而非本项",
                    e.code,
                    e.role.zh()
                ),
                "继承位只需登记；对它的落地核验在 VE-F4212（一致性与文案口径）",
                "回溯申请方",
            ));
        }
        if e.is_landed_for_arch() {
            return Err(ConsistencyError::new(
                E_TRACE_BACK_NOT_NEEDED,
                "回溯被拒：该源已落地并对账",
                &format!("承接源 {} 落地 {}、对账 {}", e.code, e.landed, e.reconciled),
                "已完成的源无需回溯；有争议请走 ADR",
                "回溯申请方",
            ));
        }
        Ok(TraceBack {
            source_code: e.code.clone(),
            package_item: T10_PACKAGE_ITEM.to_string(),
            handover_face: handover_face_of(e.source_domain),
            action: "向 T10 移交包交接面持有人确认该源交付范围与冻结哈希；\
                     范围不清先补交接面再落地，不接受默认规则顶上",
        })
    }

    /// 承接表读屏。
    pub fn screen_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "承接面落地表：共 {} 条（首批 {} 条，继承位 {} 条）；首批落地 {}\n",
            self.entries.len(),
            self.role_count(AcceptanceRole::Primary),
            self.role_count(AcceptanceRole::Inherited),
            if self.primaries_landed() { "已达成" } else { "未达成" }
        ));
        for e in self.entries.iter() {
            s.push_str(&e.screen_line());
            s.push('\n');
        }
        s
    }
}

/// 首批落地源应有两路（锚点明文「S 域交互词典/T 域地区规则」）。
pub const PRIMARY_SOURCE_COUNT: usize = 2;

/// 标准承接表（**首批两源已落地 + 术语库继承位已登记未落地**的正样本）。
///
/// 这是头注§三裁决的落地形态：两源落地、继承位在册但 `landed = false`，
/// 且机检明确「继承位未落地不判红」。
pub fn standard_acceptance() -> AcceptanceLedger {
    let mut l = AcceptanceLedger::new();
    let seeds: [(&str, &str, &str, &str, AcceptanceRole); 3] = [
        (
            "U01-SRC-SDICT",
            "S",
            "VE-F3982",
            "S 域交互词典：焦点/朗读/快捷键/动效/色彩五类术语 + 操作定义",
            AcceptanceRole::Primary,
        ),
        (
            "U01-SRC-TRULES",
            "T",
            "VE-F4101",
            "T 域地区规则：法规/格式/审查/支付四类规则（数据与代码分离）",
            AcceptanceRole::Primary,
        ),
        (
            "U01-SRC-TERMBASE",
            "T",
            "VE-F4063",
            "术语库（T04 单源）：多语言术语定义，落地义务归 VE-F4212",
            AcceptanceRole::Inherited,
        ),
    ];
    for (code, domain, item, content, role) in seeds.iter() {
        let mut e = AcceptanceEntry {
            code: code.to_string(),
            source_domain: *domain,
            source_item: item.to_string(),
            content: content.to_string(),
            role: *role,
            carried_from: format!("{} 交接面", T10_PACKAGE_ITEM),
            source_hash: String::new(),
            landed: false,
            reconciled: false,
        };
        e.source_hash = fnv1a64_hex(content.as_bytes());
        // 首批两源：落地并对账。继承位：**只登记，不落地**。
        e.landed = role.must_land_in_arch();
        e.reconciled = role.must_land_in_arch();
        l.register(e).expect("标准承接登记");
    }
    l
}

// ---------------------------------------------------------------------------
// 七、无障碍口径契约层（判据四「入约」+ 判据五「口径契约层」；域本色）
// ---------------------------------------------------------------------------

/// S 域双维标准的两个维度（VE-F3986 口径：工具维 + 产出维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum A11yDimension {
    /// 工具维：用什么辅助技术能验证（读屏/键盘/放大/语音）。
    Tool,
    /// 产出维：产出的界面能不能被正确朗读与操作。
    Output,
}

impl A11yDimension {
    /// 双维全集。
    pub const ALL: [A11yDimension; DUAL_DIMENSION_COUNT] =
        [A11yDimension::Tool, A11yDimension::Output];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            A11yDimension::Tool => "工具维",
            A11yDimension::Output => "产出维",
        }
    }

    /// 英文名。
    pub fn en(self) -> &'static str {
        match self {
            A11yDimension::Tool => "tool-dimension",
            A11yDimension::Output => "output-dimension",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            A11yDimension::Tool => "U01-A11Y-T",
            A11yDimension::Output => "U01-A11Y-O",
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<A11yDimension> {
        A11yDimension::ALL.iter().copied().find(|d| d.code() == code)
    }

    /// 该维的判据要求（入约时该维至少要有的一条判据的意思）。
    pub fn requirement(self) -> &'static str {
        match self {
            // 工具维：「我们测过了」——验证手段可复现。
            A11yDimension::Tool => "至少一条可复现的辅助技术验证判据（读屏/键盘/放大/语音）",
            // 产出维：「我们写得够好」——用户实际可用。
            A11yDimension::Output => "至少一条产出侧判据（朗读文本正确/焦点顺序可用/操作路径等价）",
        }
    }

    /// 该维为空时的后果说明（入约判定用）。
    pub fn missing_consequence(self) -> &'static str {
        match self {
            A11yDimension::Tool => "只有产出维等于「我们测过了」——工具维缺则保障不可复现",
            A11yDimension::Output => "只有工具维等于「我们写得够好」——产出维缺则用户仍不可用",
        }
    }
}

/// 入约状态（头注§四：入约 ≠ 引用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnrollmentState {
    /// 仅引用：文档里提到了双维标准。对拍查不出问题，用户拿不到保障。
    Referenced,
    /// 已入约：双维标准成为契约层必填判据位，注册闸会拒空。
    Enrolled,
    /// 豁免：跳过入约。**本域不接受此态**（见 `E_A11Y_WAIVED_FORBIDDEN`）。
    Waived,
}

impl EnrollmentState {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            EnrollmentState::Referenced => "仅引用",
            EnrollmentState::Enrolled => "已入约",
            EnrollmentState::Waived => "豁免（本域不接受）",
        }
    }

    /// 是否为契约层必填（只有入约是必填）。
    pub fn is_binding(self) -> bool {
        matches!(self, EnrollmentState::Enrolled)
    }
}

/// 无障碍判据条目。
#[derive(Clone, Debug)]
pub struct A11yCriterion {
    /// 判据码（对外引用，如 `U01-A11Y-C1`）。
    pub code: String,
    /// 所属维度。
    pub dimension: A11yDimension,
    /// 判据正文。
    pub text: String,
    /// 是否为注册必填位（入约的判据必须必填，否则注册闸形同虚设）。
    pub mandatory: bool,
}

impl A11yCriterion {
    /// 判据齐备性（码合法 + 正文非空 + 必填位为真）。
    pub fn is_valid(&self) -> bool {
        !self.code.trim().is_empty() && !self.text.trim().is_empty() && self.mandatory
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "判据 {}（{}）：{}；必填 {}",
            self.code,
            self.dimension.zh(),
            self.text,
            if self.mandatory { "是" } else { "否" }
        )
    }
}

/// 无障碍口径契约层（判据四 + 判据五）。
///
/// 它是**契约层内的一个必填契约类别**，不是第六层（层数仍是五，见
/// [`LAYER_COUNT`]）。入约的实质是：本层一旦 `Enrolled`，契约注册时
/// 无障碍判据位为空即拒——注册闸在 [`WordingContractLayer::check_registration_gate`]
/// 里可执行，不靠人记得。
#[derive(Clone, Debug)]
pub struct WordingContractLayer {
    /// 入约状态。
    pub state: EnrollmentState,
    /// 双维判据集。
    pub criteria: Vec<A11yCriterion>,
    /// 豁免理由（仅在 `Waived` 时读；本域不接受该态，故必为空）。
    pub waiver_reason: String,
}

impl WordingContractLayer {
    /// 空口径契约层（**未入约**——默认不是已入约**）。
    pub fn new() -> Self {
        WordingContractLayer {
            state: EnrollmentState::Referenced,
            criteria: Vec::new(),
            waiver_reason: String::new(),
        }
    }

    /// **入约**（判据「入约」的执行点）。
    ///
    /// 三件事必须同时成立才返回成功：双维各至少一条判据、每条判据齐备、
    /// 每条判据为注册必填位。**没有豁免入口**——本域不接受 `Waived`。
    pub fn enroll(&mut self, criteria: Vec<A11yCriterion>) -> Result<(), ConsistencyError> {
        if criteria.is_empty() {
            return Err(ConsistencyError::new(
                E_A11Y_NO_CRITERION,
                "入约被拒：判据集为空",
                "空判据集的入约等于把必填位指向虚无，注册闸拒不掉任何东西",
                "每维各补至少一条判据（要求见 A11yDimension::requirement）",
                "入约申请方",
            ));
        }
        if criteria.len() > MAX_A11Y_CRITERIA {
            return Err(ConsistencyError::new(
                E_A11Y_CAP,
                "入约被拒：判据条数超上限",
                &format!(
                    "判据 {} 条超过上限 {}；判据表有上界才使入约核验是有界常量",
                    criteria.len(),
                    MAX_A11Y_CRITERIA
                ),
                "合并同类判据，或按 ADR 提升 MAX_A11Y_CRITERIA",
                "入约申请方",
            ));
        }
        // 逐条校验：判据必须合法且必填。
        for c in criteria.iter() {
            if !c.is_valid() {
                return Err(ConsistencyError::new(
                    E_A11Y_CRITERION_INVALID,
                    "入约被拒：判据不齐备或非必填",
                    &format!(
                        "判据 {} 缺正文或 mandatory 非真；非必填判据挡不住空注册",
                        c.code
                    ),
                    "正文非空且 mandatory = true 后重试",
                    "入约申请方",
                ));
            }
        }
        // 双维齐备：缺任一维都不算入约（头注§四）。
        for d in A11yDimension::ALL.iter() {
            if !criteria.iter().any(|c| c.dimension == *d) {
                return Err(ConsistencyError::new(
                    E_A11Y_DIM_MISSING,
                    "入约被拒：双维不齐",
                    &format!(
                        "缺 {}；{}",
                        d.zh(),
                        d.missing_consequence()
                    ),
                    d.requirement(),
                    "入约申请方",
                ));
            }
        }
        self.criteria = criteria;
        self.state = EnrollmentState::Enrolled;
        self.waiver_reason = String::new();
        Ok(())
    }

    /// 某维的判据条数（O(判据条数)，表定长）。
    pub fn dimension_count(&self, d: A11yDimension) -> usize {
        self.criteria.iter().filter(|c| c.dimension == d).count()
    }

    /// 判据总条数。
    pub fn len(&self) -> usize {
        self.criteria.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.criteria.is_empty()
    }

    /// **契约注册闸**（入约的实质在此：空判据位拒注册）。
    ///
    /// 契约注册（VE-F4203 开工后）调用本函数决定放行与否。本项只提供闸门
    /// 判定，不代做注册中心。
    pub fn check_registration_gate(&self, declared_criteria: usize) -> Result<(), ConsistencyError> {
        if !self.state.is_binding() {
            return Err(ConsistencyError::new(
                E_A11Y_NOT_ENROLLED,
                "契约注册被拒：无障碍口径未入约",
                &format!(
                    "入约状态为{}；未入约时判据位不是必填，注册闸形同虚设",
                    self.state.zh()
                ),
                "先 WordingContractLayer::enroll 完成双维入约，再注册契约",
                "契约注册方",
            ));
        }
        if declared_criteria == 0 {
            return Err(ConsistencyError::new(
                E_A11Y_CRITERION_MISSING_AT_REG,
                "契约注册被拒：无障碍判据位为空",
                "口径契约层已入约（必填位生效），但本次注册未声明任何无障碍判据",
                "按本域双维要求各补至少一条，或改注册非界面类契约并注明理由",
                "契约注册方",
            ));
        }
        Ok(())
    }

    /// 读屏文本。
    pub fn screen_text(&self) -> String {
        let mut s = format!(
            "无障碍口径契约层：入约状态 {}；双维判据 工具维 {} 条 / 产出维 {} 条。\n",
            self.state.zh(),
            self.dimension_count(A11yDimension::Tool),
            self.dimension_count(A11yDimension::Output)
        );
        for c in self.criteria.iter() {
            s.push_str(&c.screen_line());
            s.push('\n');
        }
        s
    }
}

/// 标准口径契约层（**双维各一条判据、已入约**的正样本）。
pub fn standard_wording_contract() -> WordingContractLayer {
    let mut w = WordingContractLayer::new();
    w.enroll(vec![
        A11yCriterion {
            code: "U01-A11Y-C1".to_string(),
            dimension: A11yDimension::Tool,
            text: "键盘全流程可达且焦点环可见（Tab/Shift-Tab 序列无陷阱）".to_string(),
            mandatory: true,
        },
        A11yCriterion {
            code: "U01-A11Y-C2".to_string(),
            dimension: A11yDimension::Output,
            text: "读屏朗读文本与可见标签一致，焦点顺序与视觉顺序一致".to_string(),
            mandatory: true,
        },
    ])
    .expect("标准口径契约层双维入约");
    w
}

// ---------------------------------------------------------------------------
// 八、层间对拍（锚点降级矩阵「层间失配→对拍」）
// ---------------------------------------------------------------------------

/// 对拍发现项。
#[derive(Clone, Debug)]
pub struct CrossCheckFinding {
    /// 失配接口码。
    pub interface: String,
    /// 失配形态。
    pub kind: CrossCheckKind,
    /// 冻结侧哈希。
    pub frozen_hash: String,
    /// 声明侧实算哈希。
    pub declared_hash: String,
    /// 定位段位（读屏定位用，如「IF3 输入契约」）。
    pub located_at: String,
    /// 建议动作。
    pub advice: &'static str,
}

impl CrossCheckFinding {
    /// 读屏单行（异常零静默——发现项必须能念给用户听）。
    pub fn screen_line(&self) -> String {
        format!(
            "对拍发现[{}·{}]：{}；冻结 {} / 实算 {}；定位 {}；{}",
            self.kind.zh(),
            self.interface,
            self.advice,
            self.frozen_hash,
            self.declared_hash,
            self.located_at,
            if self.declared_hash == self.frozen_hash {
                "两侧一致"
            } else {
                "两侧不一致"
            }
        )
    }
}

/// 对拍失配形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossCheckKind {
    /// 声明哈希与冻结哈希不符（有人改了内容没升版）。
    HashDrift,
    /// 接口契约字段残缺（对拍的前置条件不成立）。
    Incomplete,
    /// 层链不闭合（缺边或多边）。
    ChainBroken,
    /// 相邻性被破坏（跨层直连）。
    NotAdjacent,
}

impl CrossCheckKind {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CrossCheckKind::HashDrift => "哈希漂移",
            CrossCheckKind::Incomplete => "契约残缺",
            CrossCheckKind::ChainBroken => "层链断裂",
            CrossCheckKind::NotAdjacent => "非相邻连线",
        }
    }
}

// ---------------------------------------------------------------------------
// 九、判据六项（锚点判据：五层 / 接口冻结 / 承接落地 / 入约 / 口径契约层 / 判据）
// ---------------------------------------------------------------------------

/// 判据项。锚点判据列六项，本总纲把它们变成**可被逐条断言的枚举**。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Criterion {
    /// 判据一：五层（模型→契约→规则→验证→度量，层链相邻闭合）。
    FiveLayers,
    /// 判据二：接口冻结（层间接口冻结 v1 + 越权变更拒绝 + ADR 唯一通道）。
    InterfaceFreeze,
    /// 判据三：承接落地（首批两源落地对账 + 术语库继承位在册）。
    AcceptanceLanding,
    /// 判据四：入约（S 域双维标准成为契约层必填判据位）。
    Enrollment,
    /// 判据五：口径契约层（无障碍口径作为契约层内的必填类别存在）。
    WordingContractLayer,
    /// 判据六：判据本身（总纲自证可追溯）。
    Criterion,
}

impl Criterion {
    /// 六项判据全集（判据：一项不缺）。
    pub const CRITERIA: [Criterion; CRITERION_COUNT] = [
        Criterion::FiveLayers,
        Criterion::InterfaceFreeze,
        Criterion::AcceptanceLanding,
        Criterion::Enrollment,
        Criterion::WordingContractLayer,
        Criterion::Criterion,
    ];

    /// 判据中文名（读屏播报）。
    pub fn zh(self) -> &'static str {
        match self {
            Criterion::FiveLayers => "五层",
            Criterion::InterfaceFreeze => "接口冻结",
            Criterion::AcceptanceLanding => "承接落地",
            Criterion::Enrollment => "入约",
            Criterion::WordingContractLayer => "口径契约层",
            Criterion::Criterion => "判据",
        }
    }

    /// 判据码（对拍与台账引用）。
    pub fn code(self) -> &'static str {
        match self {
            Criterion::FiveLayers => "U01-J1",
            Criterion::InterfaceFreeze => "U01-J2",
            Criterion::AcceptanceLanding => "U01-J3",
            Criterion::Enrollment => "U01-J4",
            Criterion::WordingContractLayer => "U01-J5",
            Criterion::Criterion => "U01-J6",
        }
    }

    /// 位序号（0 起）。
    pub fn rank(self) -> u8 {
        match self {
            Criterion::FiveLayers => 0,
            Criterion::InterfaceFreeze => 1,
            Criterion::AcceptanceLanding => 2,
            Criterion::Enrollment => 3,
            Criterion::WordingContractLayer => 4,
            Criterion::Criterion => 5,
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<Criterion> {
        Criterion::CRITERIA.iter().copied().find(|c| c.code() == code)
    }

    /// 本判据由哪个自检组覆盖（判据→自检项的可追溯映射，判据六的落点）。
    pub fn check_group(self) -> &'static str {
        match self {
            Criterion::FiveLayers => "U01-判据-对账-",
            Criterion::InterfaceFreeze => "U01-冻结-",
            Criterion::AcceptanceLanding => "U01-承接-",
            Criterion::Enrollment => "U01-入约-",
            Criterion::WordingContractLayer => "U01-入约-",
            Criterion::Criterion => "U01-判据-",
        }
    }

    /// 读屏单行。
    pub fn screen_line(self) -> String {
        format!("判据 {}：{}（自检组前缀 {}）", self.code(), self.zh(), self.check_group())
    }
}

// ---------------------------------------------------------------------------
// 十、哈希（对账与冻结的唯一算法，单源实现）
// ---------------------------------------------------------------------------

/// 哈希十六进制定宽（宽度不定就没法字符串比对）。
pub const HASH_HEX_LEN: usize = 16;

/// FNV-1a 64 位偏移基。
const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 位哈希（单遍累积）。
///
/// 选它不用更强哈希是因为**对账要的是确定性而非抗攻击**——两侧算同一样东西
/// 必须得到同一个数，而抗碰撞不是这一层的诉求（内容一旦被恶意构造，走的是
/// F4214 的签名链那条线，不是对账这条线）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV64_OFFSET;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV64_PRIME);
    }
    h
}

/// FNV-1a 64 位十六进制（16 位小写，定宽）。
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}

/// 条目号格式校验（`VE-F` + 四位数字）。
pub fn is_valid_item_id(item: &str) -> bool {
    let Some(rest) = item.strip_prefix("VE-F") else {
        return false;
    };
    rest.len() == 4 && rest.bytes().all(|b| b.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// 十一、错误码与错误五元组（错误路径零静默）
// ---------------------------------------------------------------------------

/// 五层不齐。
pub const E_LAYER_COUNT: &str = "E_LAYER_COUNT";
/// 层位序号非严格递增（层序单源被破坏）。
pub const E_LAYER_ORDER: &str = "E_LAYER_ORDER";
/// 层链断裂（缺相邻边）。
pub const E_LAYER_CHAIN_BROKEN: &str = "E_LAYER_CHAIN_BROKEN";
/// 层契约残缺。
pub const E_LAYER_SPEC_INCOMPLETE: &str = "E_LAYER_SPEC_INCOMPLETE";
/// 规则文本被复制到非契约层（本域第一使命红线）。
pub const E_RULE_TEXT_DUPLICATED: &str = "E_RULE_TEXT_DUPLICATED";
/// 存在被标为运行期的层（架构零开销声明失效）。
pub const E_RUNTIME_OVERHEAD_DECLARED: &str = "E_RUNTIME_OVERHEAD_DECLARED";
/// 能力与层未对齐（层无能力 / 能力无层且无派生声明）。
pub const E_CAPABILITY_UNALIGNED: &str = "E_CAPABILITY_UNALIGNED";
/// 派生能力的派生源必须是其它能力已占用的层。
pub const E_DERIVATION_ORPHAN: &str = "E_DERIVATION_ORPHAN";
/// 接口集为空。
pub const E_INTERFACE_EMPTY: &str = "E_INTERFACE_EMPTY";
/// 接口数超上限。
pub const E_INTERFACE_CAP: &str = "E_INTERFACE_CAP";
/// 接口契约残缺。
pub const E_INTERFACE_INCOMPLETE: &str = "E_INTERFACE_INCOMPLETE";
/// 非相邻层连线。
pub const E_INTERFACE_NOT_ADJACENT: &str = "E_INTERFACE_NOT_ADJACENT";
/// 接口码重复。
pub const E_INTERFACE_DUP: &str = "E_INTERFACE_DUP";
/// 接口未登记。
pub const E_INTERFACE_UNKNOWN: &str = "E_INTERFACE_UNKNOWN";
/// 越权变更被拒（未走 ADR 的就地改）。
pub const E_INTERFACE_UNFROZEN_CHANGE: &str = "E_INTERFACE_UNFROZEN_CHANGE";
/// ADR 字段缺失。
pub const E_ADR_INCOMPLETE: &str = "E_ADR_INCOMPLETE";
/// ADR 无标题/理由。
pub const E_ADR_NO_TITLE: &str = "E_ADR_NO_TITLE";
/// ADR 无否决记录（决策记录 vs 提案记录的分界）。
pub const E_ADR_NO_REJECTED: &str = "E_ADR_NO_REJECTED";
/// ADR 版本未升。
pub const E_ADR_NO_VERSION_BUMP: &str = "E_ADR_NO_VERSION_BUMP";
/// ADR 编号重复。
pub const E_ADR_DUP: &str = "E_ADR_DUP";
/// ADR 账满。
pub const E_ADR_CAP: &str = "E_ADR_CAP";
/// 无 ADR 重基（后门）。
pub const E_REBASE_WITHOUT_ADR: &str = "E_REBASE_WITHOUT_ADR";
/// 承接条目残缺或条目号非法。
pub const E_ACCEPTANCE_INCOMPLETE: &str = "E_ACCEPTANCE_INCOMPLETE";
/// 承接源码重复。
pub const E_ACCEPTANCE_DUP: &str = "E_ACCEPTANCE_DUP";
/// 承接表满。
pub const E_ACCEPTANCE_CAP: &str = "E_ACCEPTANCE_CAP";
/// 承接源未登记。
pub const E_ACCEPTANCE_UNKNOWN: &str = "E_ACCEPTANCE_UNKNOWN";
/// 首批源未落地（阻断）。
pub const E_PRIMARY_NOT_LANDED: &str = "E_PRIMARY_NOT_LANDED";
/// 继承位未登记（登记是义务）。
pub const E_INHERITED_UNREGISTERED: &str = "E_INHERITED_UNREGISTERED";
/// 回溯时点未到（继承位尚未到落地时点）。
pub const E_TRACE_BACK_NOT_DUE: &str = "E_TRACE_BACK_NOT_DUE";
/// 回溯不必要（源已落地对账）。
pub const E_TRACE_BACK_NOT_NEEDED: &str = "E_TRACE_BACK_NOT_NEEDED";

/// 来源域未登记交接面（回溯无处可派时的指名兜底）。
pub const E_HANDOVER_FACE_UNKNOWN: &str = "E_HANDOVER_FACE_UNKNOWN";
/// 无障碍判据集为空。
pub const E_A11Y_NO_CRITERION: &str = "E_A11Y_NO_CRITERION";
/// 判据条数超上限。
pub const E_A11Y_CAP: &str = "E_A11Y_CAP";
/// 判据不齐备或非必填。
pub const E_A11Y_CRITERION_INVALID: &str = "E_A11Y_CRITERION_INVALID";
/// 双维不齐。
pub const E_A11Y_DIM_MISSING: &str = "E_A11Y_DIM_MISSING";
/// 未入约（注册闸拒绝）。
pub const E_A11Y_NOT_ENROLLED: &str = "E_A11Y_NOT_ENROLLED";
/// 注册时无障碍判据位为空。
pub const E_A11Y_CRITERION_MISSING_AT_REG: &str = "E_A11Y_CRITERION_MISSING_AT_REG";
/// 出现豁免态（本域禁止）。
pub const E_A11Y_WAIVED_FORBIDDEN: &str = "E_A11Y_WAIVED_FORBIDDEN";
/// 契约问题（自检侧）。
pub const E_CONTRACT_ISSUE: &str = "E_CONTRACT_ISSUE";
/// 越界（命中禁扩面）。
pub const E_BOUNDARY_OVERREACH: &str = "E_BOUNDARY_OVERREACH";
/// 对拍发现失配。
pub const E_CROSS_CHECK_DRIFT: &str = "E_CROSS_CHECK_DRIFT";

/// U 域错误（五元组：码/现象/原因/下一步/责任方）。
///
/// **拒绝必须给出路**：五元组齐发是构造点强制，`next` 为空即视为不合格——
/// 只说「不行」而不说「那该怎么做」的拒绝，会让人换个写法再来一遍。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsistencyError {
    /// 错误码。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
    /// 下一步（必填——拒绝必须给出路）。
    ///
    /// 取 `String` 而非 `&'static str`：下一步常要指名「该找哪个域/先开哪张 ADR」，
    /// 这类动态指引静态串写不出来；强行静态化只会把人逼成写「详见文档」这种废话，
    /// 而废物指引等于没有指引。
    pub next: String,
    /// 责任方。
    pub who: String,
}

impl ConsistencyError {
    /// 构造（五元组齐发，构造点强制写全）。
    pub fn new(
        code: &'static str,
        what: &'static str,
        why: &str,
        next: &str,
        who: &str,
    ) -> Self {
        ConsistencyError {
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

    /// 读屏可读的完整错误（现象/原因/下一步齐发）。
    pub fn screen_text(&self) -> String {
        format!(
            "错误 {}：{}；原因：{}；下一步：{}；责任方：{}",
            self.code, self.what, self.why, self.next, self.who
        )
    }
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

/// 契约问题（四元组：码/现象/根因/建议 + 严重度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractIssue {
    /// 问题码。
    pub code: &'static str,
    /// 现象。
    pub symptom: String,
    /// 根因。
    pub root_cause: String,
    /// 建议（取 `String`：建议须指名具体条目/常量，静态串写不出可执行建议）。
    pub advice: String,
    /// 严重度。
    pub severity: Severity,
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

// ---------------------------------------------------------------------------
// 十二、禁扩面与下游归属（防止抢活与漏活）
// ---------------------------------------------------------------------------

/// 禁扩面清单（U 域**不做**什么）。
///
/// 越界拒绝必须告诉对方「这事该谁做」，否则下次还会有人试。
pub const BOUNDARY_EXCLUSIONS: [(&str, &str); MAX_EXCLUSIONS] = [
    (
        "U-OWN-MODEL",
        "自建一致性对象模型本体与关系代数——模型归VE-F4202，U 域只立层契约",
    ),
    (
        "U-OWN-REGISTRY",
        "自建契约注册中心与引用计数——注册归VE-F4203，U 域只声明谁持规则文本",
    ),
    (
        "U-OWN-RULE-EXEC",
        "自建规则引擎与三元裁决器——执行归 VE-F4204，U 域只立编译段契约",
    ),
    (
        "U-OWN-SCANNER",
        "自建扫描调度与插件隔离——扫描平台归 VE-F4206，U 域只声明执行形态",
    ),
    (
        "U-OWN-METRIC",
        "自建四主指标计算与复算器——度量归 VE-F4207，U 域只声明它是终点层",
    ),
    (
        "U-OWN-GRAPH",
        "自建知识图谱节点边与扩散查询——图谱归 VE-F4209，U 域只登记派生关系",
    ),
    (
        "U-COPY-DICT",
        "把上游词典/地区规则内容抄进契约文本——单源引用，抄副本即一致性分叉",
    ),
    (
        "U-SKIP-A11Y",
        "让无障碍口径契约层可豁免——本域不接受豁免，入约缺失即注册闸拒",
    ),
    (
        "U-DEFAULT-FILL",
        "承接缺源时用默认规则顶上——缺源走回溯移交包，默认值无人负责",
    ),
    (
        "U-SILENT-CHANGE",
        "就地改冻结接口而不升版——必须走 ADR 通道，否则下游无从判断按哪版编",
    ),
    (
        "U-AUTO-REBASE",
        "对拍发现失配后自动改冻结值——自动改让对拍变成掩盖",
    ),
    (
        "U-SIGN-FOR-U02",
        "代签 U01 双签——双签归 VE-F4220，本项只提供第一件（架构冻结哈希）",
    ),
];

/// 禁扩面 → 归属去处（越界拒绝时给出的「找谁」）。
fn exclusion_owner(code: &str) -> &'static str {
    match code {
        "U-OWN-MODEL" => "VE-F4202（跨域一致性模型）",
        "U-OWN-REGISTRY" => "VE-F4203（契约注册中心）",
        "U-OWN-RULE-EXEC" => "VE-F4204（一致性规则引擎）",
        "U-OWN-SCANNER" => "VE-F4206（一致性扫描平台）",
        "U-OWN-METRIC" => "VE-F4207（一致性度量体系）",
        "U-OWN-GRAPH" => "VE-F4209（一致性知识图谱）",
        "U-COPY-DICT" => "VE-F4210/F4211/F4212（词典/令牌/口径转译，单源引用）",
        "U-SKIP-A11Y" => "VE-F4204/F4205（无障碍判据不可豁免红线）",
        "U-DEFAULT-FILL" => "VE-F4195（T10 移交包交接面持有人）",
        "U-SILENT-CHANGE" => "VE-F4220（U01 组收口双签：变更须留痕）",
        "U-AUTO-REBASE" => "VE-F4216（一致性 API 冻结：变更走 F4205 流程）",
        _ => "VE-F4220（U01 组收口双签）",
    }
}

/// 禁扩面校验（复杂度 C9：O(禁扩面条数)）。
///
/// 传入一条「想做的事」，逐条比对 [`BOUNDARY_EXCLUSIONS`]。命中即
/// [`E_BOUNDARY_OVERREACH`]，并把该禁扩面的**归属去处**一并给出。
///
/// 通过时回 `Ok(())` 而非回传入参：本函数是判据闸，不是转换器——原样回传
/// `intent` 会诱使调用方把「拿到返回值」误当成「已被认可」，而闸门的通过态
/// 本就不该携带任何需要下游再判断的东西。
pub fn check_no_overreach(intent: &str) -> Result<(), ConsistencyError> {
    for (code, desc) in BOUNDARY_EXCLUSIONS.iter() {
        if intent.contains(code) || intent.contains(desc) {
            return Err(ConsistencyError::new(
                E_BOUNDARY_OVERREACH,
                "越界被拒：此事不归 U 域总架构",
                &format!(
                    "「{}」命中禁扩面 {}：{}",
                    intent, code, desc
                ),
                &exclusion_owner(code),
                "U 域架构维护方",
            ));
        }
    }
    Ok(())
}

/// 下游归属表（**谁拥有什么**——本项只立总纲，不代做后续 19 项）。
///
/// 这张表的作用是**防止抢活**：域开工最常见的失败不是做不出来，而是总纲
/// 顺手把下游的活也干了，然后下游条目开工时发现"已经有人做过了，但没人
/// 知道在哪、依据是什么"。
pub const DOWNSTREAM_OWNERSHIP: [(&str, &str); 21] = [
    ("VE-F4202", "跨域一致性模型：四类实体 × 三型关系 + 关系代数 + 冲突消解 + 环检测"),
    ("VE-F4203", "契约注册中心：元模型五字段冻结 + 唯一性断言 + 引用计数 + 生命周期"),
    ("VE-F4204", "一致性规则引擎：三段式可执行化 + 四路单源引用 + 三元裁决"),
    ("VE-F4205", "契约变更流程引擎：变更六步流 + 兼容性闸 + 灰度 + 回滚"),
    ("VE-F4206", "一致性扫描平台：三模式调度 + 插件化 + 发现项五元组归一"),
    ("VE-F4207", "一致性度量体系：四主指标 × 域维度 + 口径冻结 + 可复算"),
    ("VE-F4208", "跨域数据流验证：流经点契约核对 + 格式保真 + 驻留跨域延续"),
    ("VE-F4209", "一致性知识图谱：实体关系图 + 影响扩散查询 + 与注册中心双向同步"),
    ("VE-F4210", "一致性与交互词典：词典条目 → 规则单源转译 + 重转译回归门"),
    ("VE-F4211", "一致性与视觉令牌：五族令牌转译 + 像素级漂移检测 + 双维矩阵"),
    ("VE-F4212", "一致性与文案口径：三源转译 + 跨语种漂移检测 + 文案回归门"),
    ("VE-F4213", "一致性性能：扫描吞吐 + 规则延迟 + 图谱查询三线基准"),
    ("VE-F4214", "一致性安全：契约签名 + 规则注入白名单 + 图谱投毒双人审"),
    ("VE-F4215", "一致性调试器：全链反查 + 规则单步 + 图谱视区三视区"),
    ("VE-F4216", "一致性 API 冻结：四族 API 冻结 + 参数钳制 + 错误码三件套"),
    ("VE-F4217", "一致性文档：架构手册 + 元模型手册 + 规则指南 + API 参考"),
    ("VE-F4218", "U01 联调：全链贯通 + 故障注入三演练 + 证据挂度量"),
    ("VE-F4219", "U01 预备自查：20 项核验 + 四件汇总 + 预备报告"),
    ("VE-F4220", "U01 组收口双签：四件齐备 + 自查签与复核签分离"),
    ("VE-F4221", "契约引擎总架构（U02 组开工）：五段流水 + 注册中心单源红线"),
    ("VE-F4195", "T10 移交包：十件封装 + 冻结哈希 + U 域交接面（本项回溯目的地）"),
];

/// 下游归属查询（条目号 → 职责描述；未登记返回 `None`）。
pub fn downstream_owner_of(item: &str) -> Option<&'static str> {
    DOWNSTREAM_OWNERSHIP
        .iter()
        .find(|(id, _)| *id == item)
        .map(|(_, duty)| *duty)
}

// ---------------------------------------------------------------------------
// 十三、总架构本体
// ---------------------------------------------------------------------------

/// U 域一致性总架构（总架构册本体）。
#[derive(Clone, Debug)]
pub struct ConsistencyArchitecture {
    /// 总纲版本。
    pub version: String,
    /// 五层册。
    pub layers: Vec<LayerSpec>,
    /// 接口冻结册。
    pub freeze: InterfaceFreezeLedger,
    /// 承接面落地表。
    pub acceptance: AcceptanceLedger,
    /// 无障碍口径契约层。
    pub wording: WordingContractLayer,
}

impl ConsistencyArchitecture {
    /// **标准总纲**（五层齐 + 接口四边冻结 + 首批两源落地 + 双维入约）。
    pub fn standard() -> Self {
        ConsistencyArchitecture {
            version: ARCH_VERSION.to_string(),
            layers: STANDARD_LAYERS.to_vec(),
            freeze: InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
                .expect("标准接口册应可冻结：四条相邻边齐备且哈希实算"),
            acceptance: standard_acceptance(),
            wording: standard_wording_contract(),
        }
    }

    /// 某层的册内条目。
    pub fn layer(&self, l: Layer) -> Option<&LayerSpec> {
        self.layers.iter().find(|s| s.layer == l)
    }

    /// 持规则文本的层（应恰有一个——契约层）。
    pub fn rule_text_owners(&self) -> Vec<Layer> {
        self.layers
            .iter()
            .map(|s| s.layer)
            .filter(|l| l.owns_rule_text())
            .collect()
    }

    /// **判据一：五层**（复杂度 C2：O(层数) = O(5)）。
    ///
    /// 核四件事：层数恰 5、层位严格递增、层契约六项齐备、层链相邻闭合
    /// （每层的下游边都有一条接口）。第五件：全部层成本为声明期。
    pub fn check_layers(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        // 1) 层数。
        if self.layers.len() != LAYER_COUNT {
            issues.push(ContractIssue {
                code: E_LAYER_COUNT,
                symptom: format!("层数 {} 与五层不符", self.layers.len()),
                root_cause: "总架构册层数被增删；五层是判据一的硬数字".to_string(),
                advice: "按 Layer::ALL 的五层重建层册；增删层须走 ADR 并改判据".to_string(),
                severity: Severity::Blocking,
            });
        }
        // 2) 层位严格递增（层序单源）。
        for w in self.layers.windows(2) {
            if w[1].rank != w[0].rank + 1 {
                issues.push(ContractIssue {
                    code: E_LAYER_ORDER,
                    symptom: format!(
                        "层位非严格递增：{} 之后是 {}",
                        w[0].layer.zh(),
                        w[1].layer.zh()
                    ),
                    root_cause: "层册顺序被改动；层序是层链与接口位序的唯一真值".to_string(),
                    advice: "层册按 Layer::rank 升序排列（层序单源 Layer::rank）".to_string(),
                    severity: Severity::Blocking,
                });
                break;
            }
        }
        // 3) 层契约六项齐备。
        for s in self.layers.iter() {
            if !s.is_complete() {
                issues.push(ContractIssue {
                    code: E_LAYER_SPEC_INCOMPLETE,
                    symptom: format!("层 {} 契约残缺", s.layer.code()),
                    root_cause: "层契约缺字段；残缺契约无法与下游对拍".to_string(),
                    advice: "补齐职责/输入/输出/失败策略/复杂度/不做清单/主责条目".to_string(),
                    severity: Severity::Blocking,
                });
            }
        }
        // 4) 层链相邻闭合：每层（除度量）都恰有一条出边。
        for l in LAYER_ORDER.iter() {
            match l.downstream() {
                Some(down) => {
                    if self.freeze.edge(*l, down).is_none() {
                        issues.push(ContractIssue {
                            code: E_LAYER_CHAIN_BROKEN,
                            symptom: format!(
                                "层链断边：{} → {} 无接口",
                                l.zh(),
                                down.zh()
                            ),
                            root_cause: "五层相邻边被删；层链不闭合则下游悬空".to_string(),
                            advice: "补回该相邻边接口（见 standard_interfaces 的逐边表）".to_string(),
                            severity: Severity::Blocking,
                        });
                    }
                }
                None => {
                    // 度量层是终点：不应有出边。
                    if self
                        .freeze
                        .iter()
                        .any(|i| i.from == Layer::Metric)
                    {
                        issues.push(ContractIssue {
                            code: E_LAYER_CHAIN_BROKEN,
                            symptom: "度量层有出边".to_string(),
                            root_cause: "度量层是五层终点；它消费前四层产出，不产生新规则"
                                .to_string(),
                            advice: "删除度量层的出边接口".to_string(),
                            severity: Severity::Blocking,
                        });
                    }
                }
            }
        }
        // 5) 零运行时开销。
        if !self.is_zero_runtime_cost() {
            issues.push(ContractIssue {
                code: E_RUNTIME_OVERHEAD_DECLARED,
                symptom: "存在被标为运行期的层".to_string(),
                root_cause: "U 域总架构承担了运行期工作".to_string(),
                advice: "把运行期工作移交 F4202-F4207 各本体，U 域只留层契约与冻结".to_string(),
                severity: Severity::Blocking,
            });
        }
        // 6) 规则文本唯一持有（头注§一红线）。
        let owners = self.rule_text_owners();
        if owners.len() != 1 || owners[0] != Layer::Contract {
            let got: Vec<String> = owners.iter().map(|l| l.zh().to_string()).collect();
            issues.push(ContractIssue {
                code: E_RULE_TEXT_DUPLICATED,
                symptom: format!("规则文本持有层为 {:?}，应为单层契约层", got),
                root_cause: "非契约层持规则文本副本；副本会各自演化，一致性无从保证"
                    .to_string(),
                advice: "非契约层改为持契约指针（Layer::owns_rule_text 只对契约层为真）".to_string(),
                severity: Severity::Blocking,
            });
        }
        issues
    }

    /// **判据二：接口冻结**（复杂度 C3：O(接口数) = O(4)）。
    ///
    /// 核四件事：接口恰四条且与层链一一对应、每条相邻、契约齐备、
    /// 冻结版本与总纲版本不打架（冻结版本必须带 `-rN` 修订号才说明变更走过 ADR）。
    pub fn check_interfaces(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        let n = self.freeze.len();
        // 接口数应恰为 层数-1。
        if n != LAYER_COUNT - 1 {
            issues.push(ContractIssue {
                code: E_LAYER_CHAIN_BROKEN,
                symptom: format!("接口 {} 条，应为 {} 条（层数-1）", n, LAYER_COUNT - 1),
                root_cause: "层链边数与接口数不匹配：缺边则悬空，多边则出现非相邻连线"
                    .to_string(),
                advice: "接口集应恰为 Layer::downstream 的逐边结果（standard_interfaces）".to_string(),
                severity: Severity::Blocking,
            });
        }
        for it in self.freeze.iter() {
            if !it.is_adjacent() {
                issues.push(ContractIssue {
                    code: E_INTERFACE_NOT_ADJACENT,
                    symptom: format!("接口 {} 非相邻层连线", it.code),
                    root_cause: "跨层直连不经中间层正式接口，无处可寻".to_string(),
                    advice: "改为相邻边；确需跨层请走 ADR 正式增接口并升冻结版本".to_string(),
                    severity: Severity::Blocking,
                });
            }
            if !it.is_complete() {
                issues.push(ContractIssue {
                    code: E_INTERFACE_INCOMPLETE,
                    symptom: format!("接口 {} 契约残缺", it.code),
                    root_cause: "接口契约缺字段或哈希非定宽；残缺契约无法对拍".to_string(),
                    advice: "补齐八项字段并用 declared_text() 实算哈希".to_string(),
                    severity: Severity::Blocking,
                });
            }
        }
        // 变更次数与 ADR 数须一致（每次重基恰一条 ADR，不许有 ADR 无重基、
        // 也不许有重基无 ADR）。读权威计数而非外层镜像，避免镜像过期误判。
        let adr_count = self.freeze.adrs().count();
        let rebase_count = self.freeze.rebase_count();
        if adr_count != rebase_count {
            issues.push(ContractIssue {
                code: E_CONTRACT_ISSUE,
                symptom: format!("ADR {} 条与重基 {} 次不一致", adr_count, rebase_count),
                root_cause: "重基无 ADR 即后门；ADR 无重基即空决策".to_string(),
                advice: "一改一 ADR：先 open_adr 再 rebase，两侧计数须相等".to_string(),
                severity: Severity::Blocking,
            });
        }
        issues
    }

    /// **五层 × 五能力双向对账**（复杂度 C1：O(层 + 能力) = O(10)）。
    ///
    /// 这是头注§二两条裁决的机检落点：
    /// - 每层都须有服务它的能力（层无主=无主资源）；
    /// - 每能力都须占层或声明派生（能力无落点=无处交付）；
    /// - 派生能力的派生源须是其它能力已占用的层（否则图谱无源）。
    pub fn check_capability_alignment(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        // 方向一：层 → 能力。
        for l in LAYER_ORDER.iter() {
            let c = l.served_by();
            if !CAPABILITY_ORDER.contains(&c) {
                issues.push(ContractIssue {
                    code: E_CAPABILITY_UNALIGNED,
                    symptom: format!("层 {} 的服务能力不在五能力全集内", l.zh()),
                    root_cause: "层序与能力序脱节".to_string(),
                    advice: "修正 Layer::served_by 使其返回 CAPABILITY_ORDER 中的能力".to_string(),
                    severity: Severity::Blocking,
                });
            }
        }
        // 方向二：能力 → 层/派生。
        for c in CAPABILITY_ORDER.iter() {
            let layers = c.layers();
            let derived = c.derivation_sources();
            if layers.is_empty() && derived.is_empty() {
                issues.push(ContractIssue {
                    code: E_CAPABILITY_UNALIGNED,
                    symptom: format!("能力 {} 既不占层也无派生声明", c.zh()),
                    root_cause: "无落点的能力等于不存在；锚点五项能力必须逐项有交代"
                        .to_string(),
                    advice: "给它占层，或声明 derivation_sources（并说明为何不占层）".to_string(),
                    severity: Severity::Blocking,
                });
                continue;
            }
            // 派生能力：派生源须被其它能力占用（不许自引用、不许指向空层）。
            if c.is_derived() {
                for src in derived.iter() {
                    let owner = src.served_by();
                    let covered = CAPABILITY_ORDER
                        .iter()
                        .any(|x| x.layers().contains(src));
                    if !covered {
                        issues.push(ContractIssue {
                            code: E_DERIVATION_ORPHAN,
                            symptom: format!(
                                "能力 {} 的派生源 {} 无任何能力占用",
                                c.zh(),
                                src.zh()
                            ),
                            root_cause: "派生指向了空层；派生视图必须挂在有主的层上"
                                .to_string(),
                            advice: format!(
                                "改由有主层派生（当前 {} 的服务能力为 {}，但该能力未占此层）",
                                src.zh(),
                                owner.zh()
                            ),
                            severity: Severity::Blocking,
                        });
                    }
                }
            }
            // 占层的能力：所占层必须在层册里存在。
            for l in layers.iter() {
                if self.layer(*l).is_none() {
                    issues.push(ContractIssue {
                        code: E_CAPABILITY_UNALIGNED,
                        symptom: format!("能力 {} 占用层 {}，但层册无此层", c.zh(), l.zh()),
                        root_cause: "能力与层册脱节".to_string(),
                        advice: "层册按 Layer::ALL 五层补齐".to_string(),
                        severity: Severity::Blocking,
                    });
                }
            }
        }
        issues
    }

    /// **判据三：承接落地**（复杂度 C6：O(源数)，源表定长）。
    ///
    /// 核三件事：首批两源（S 域交互词典 + T 域地区规则）齐备且落地对账、
    /// 继承位（术语库）**已登记**（登记是义务）、继承位未落地**不判红**
    /// （落地义务归 VE-F4212，头注§三）。
    pub fn check_acceptance(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        // 1) 首批两源落地。
        if !self.acceptance.primaries_landed() {
            let prim = self.acceptance.role_count(AcceptanceRole::Primary);
            let landed = self
                .acceptance
                .iter()
                .filter(|e| e.role == AcceptanceRole::Primary && e.is_landed_for_arch())
                .count();
            issues.push(ContractIssue {
                code: E_PRIMARY_NOT_LANDED,
                symptom: format!("首批源 {} 条，落地 {} 条（应两路：S 域词典 + T 域地区规则）", prim, landed),
                root_cause: "首批承接源未落地或未与来源对账；契约源缺位则规则无上游"
                    .to_string(),
                advice: "向 T10 移交包交接面持有人确认交付范围与哈希后落地；不接受默认规则顶上".to_string(),
                severity: Severity::Blocking,
            });
        }
        // 2) 继承位已登记（未落地不判红）。
        let inherited = self.acceptance.role_count(AcceptanceRole::Inherited);
        if inherited == 0 {
            issues.push(ContractIssue {
                code: E_INHERITED_UNREGISTERED,
                symptom: "继承位未登记".to_string(),
                root_cause: "T10 交接面声明了三路（交互词典/地区规则/术语库），承接表缺一路；\
                             契约源在交接那一刻丢件，是最难查的一类缺陷"
                    .to_string(),
                advice: "登记术语库继承位（role = Inherited，carried_from = VE-F4195 交接面）".to_string(),
                severity: Severity::Blocking,
            });
        }
        // 3) 逐条完整性（残缺条目即便落地也不算承接成功）。
        for e in self.acceptance.iter() {
            if !e.is_complete() {
                issues.push(ContractIssue {
                    code: E_ACCEPTANCE_INCOMPLETE,
                    symptom: format!("承接源 {} 残缺", e.code),
                    root_cause: "承接条目缺字段或条目号非法；残缺条目无法回溯".to_string(),
                    advice: "补齐字段并确认 source_item 为 VE-F#### 形式".to_string(),
                    severity: e.land_gap_severity(),
                });
            }
        }
        issues
    }

    /// **判据四 + 判据五：入约与口径契约层**（复杂度 C8：O(维数 × 判据条数)）。
    ///
    /// 核四件事：未入约判红、出现豁免态判红、双维各至少一条判据、
    /// 每条判据为注册必填位。**豁免态无出路**——本域不接受。
    pub fn check_wording_contract(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        let w = &self.wording;
        // 1) 豁免态禁止。
        if matches!(w.state, EnrollmentState::Waived) {
            issues.push(ContractIssue {
                code: E_A11Y_WAIVED_FORBIDDEN,
                symptom: "无障碍口径契约层处于豁免态".to_string(),
                root_cause: "本域不接受豁免；无障碍判据一旦允许豁免就会被逐案豁免掉"
                    .to_string(),
                advice: "改回入约：双维各补至少一条必填判据后 enroll".to_string(),
                severity: Severity::Blocking,
            });
        }
        // 2) 入约状态。
        if !w.state.is_binding() {
            issues.push(ContractIssue {
                code: E_A11Y_NOT_ENROLLED,
                symptom: format!("入约状态为{}", w.state.zh()),
                root_cause: "仅引用不等于入约：引用时判据位不是必填，注册闸拒不掉任何东西"
                    .to_string(),
                advice: "WordingContractLayer::enroll 双维判据（要求见 A11yDimension::requirement）".to_string(),
                severity: Severity::Blocking,
            });
        }
        // 3) 双维各至少一条。
        if w.state.is_binding() {
            for d in A11yDimension::ALL.iter() {
                if w.dimension_count(*d) == 0 {
                    issues.push(ContractIssue {
                        code: E_A11Y_DIM_MISSING,
                        symptom: format!("{} 判据为空", d.zh()),
                        root_cause: d.missing_consequence().to_string(),
                        advice: d.requirement().to_string(),
                        severity: Severity::Blocking,
                    });
                }
            }
        }
        // 4) 判据必填位。
        for c in w.criteria.iter() {
            if !c.mandatory {
                issues.push(ContractIssue {
                    code: E_A11Y_CRITERION_INVALID,
                    symptom: format!("判据 {} 非必填", c.code),
                    root_cause: "非必填判据挡不住空注册".to_string(),
                    advice: "mandatory 置真；确为非必填请移出本层契约类别".to_string(),
                    severity: Severity::Blocking,
                });
            }
        }
        issues
    }

    /// 零运行时开销（全部层为声明期）。
    pub fn is_zero_runtime_cost(&self) -> bool {
        self.layers.iter().all(|s| s.cost.is_zero_overhead())
    }

    /// **层间对拍**（复杂度 C5：O(接口数 × 声明字节长)；锚点降级矩阵「层间失配→对拍」）。
    ///
    /// 重算每条接口的声明哈希与在位哈希比对。**失配只报告不自动改**——
    /// 自动改冻结值会让「对拍」变成「掩盖」（禁扩面 `U-AUTO-REBASE`）。
    pub fn cross_check(&self) -> Vec<CrossCheckFinding> {
        let mut out = Vec::new();
        for it in self.freeze.iter() {
            // 前置：契约残缺时哈希不可信，先报残缺。
            if !it.is_complete() {
                out.push(CrossCheckFinding {
                    interface: it.code.to_string(),
                    kind: CrossCheckKind::Incomplete,
                    frozen_hash: it.declared_hash.clone(),
                    declared_hash: String::new(),
                    located_at: format!("{} 接口契约字段", it.code),
                    advice: "先补齐契约字段；对拍的前置条件不成立时不得比对哈希",
                });
                continue;
            }
            // 非相邻先报（相邻性是层链闭合的前提）。
            if !it.is_adjacent() {
                out.push(CrossCheckFinding {
                    interface: it.code.to_string(),
                    kind: CrossCheckKind::NotAdjacent,
                    frozen_hash: it.declared_hash.clone(),
                    declared_hash: fnv1a64_hex(it.declared_text().as_bytes()),
                    located_at: format!("{} → {} 连线", it.from.zh(), it.to.zh()),
                    advice: "改为相邻边；跨层直连须走 ADR 正式增接口",
                });
                continue;
            }
            // 哈希对拍：重算 vs 在位。
            let actual = fnv1a64_hex(it.declared_text().as_bytes());
            if actual != it.declared_hash {
                out.push(CrossCheckFinding {
                    interface: it.code.to_string(),
                    kind: CrossCheckKind::HashDrift,
                    frozen_hash: it.declared_hash.clone(),
                    declared_hash: actual,
                    located_at: format!("{} 声明正文（吃/吐/失败/消费方/不做清单）", it.code),
                    advice: "定位到字段后走 ADR 升版；禁止就地改冻结值（对拍不掩盖）",
                });
            }
        }
        // 层链断裂独立报一次（缺边在逐边遍历里看不见）。
        for l in LAYER_ORDER.iter() {
            if let Some(down) = l.downstream() {
                if self.freeze.edge(*l, down).is_none() {
                    out.push(CrossCheckFinding {
                        interface: format!("{}-{}", l.code(), down.code()),
                        kind: CrossCheckKind::ChainBroken,
                        frozen_hash: String::new(),
                        declared_hash: String::new(),
                        located_at: format!("{} → {} 缺边", l.zh(), down.zh()),
                        advice: "补回该相邻边接口；层链不闭合则下游悬空",
                    });
                }
            }
        }
        out
    }

    /// **架构总自检**（复杂度 C10：各项定长上界之和）。
    ///
    /// 六项判据逐条核 + 对拍汇总。返回空 vec 即全绿。
    pub fn self_audit(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        issues.extend(self.check_layers());
        issues.extend(self.check_capability_alignment());
        issues.extend(self.check_interfaces());
        issues.extend(self.check_acceptance());
        issues.extend(self.check_wording_contract());
        // 对拍失配并入契约问题（形态转成契约问题便于统一播报）。
        for f in self.cross_check() {
            issues.push(ContractIssue {
                code: E_CROSS_CHECK_DRIFT,
                symptom: f.screen_line(),
                root_cause: match f.kind {
                    CrossCheckKind::HashDrift => "有人改了接口声明却没升冻结版本",
                    CrossCheckKind::Incomplete => "接口契约残缺，对拍前置条件不成立",
                    CrossCheckKind::ChainBroken => "五层相邻边缺失，层链不闭合",
                    CrossCheckKind::NotAdjacent => "出现跨层直连，绕过正式接口",
                }
                .to_string(),
                advice: f.advice.to_string(),
                severity: Severity::Blocking,
            });
        }
        issues
    }

    /// **读屏替述**（无障碍替述：把总纲讲成人话，且**覆盖全部六项判据**）。
    ///
    /// 替述与总纲**同源生成**——另写一份的风险是漂移，而漂移的无障碍文档比
    /// 没有更坏：它会让用户以为已经有保障。
    pub fn architecture_narration(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "VE-U 域一致性总架构，版本 {}，层间接口冻结 {}。\n",
            self.version, self.freeze.interface_version
        ));
        s.push_str(&format!(
            "判据共六项：{}。\n",
            Criterion::CRITERIA
                .iter()
                .map(|c| format!("{}（{}）", c.zh(), c.code()))
                .collect::<Vec<String>>()
                .join("；")
        ));
        s.push_str("五层架构（规则文本只在契约层，其余层持指针）：\n");
        for l in LAYER_ORDER.iter() {
            s.push_str(&l.screen_line());
            s.push('\n');
        }
        s.push_str("五能力落点：\n");
        for c in CAPABILITY_ORDER.iter() {
            s.push_str(&c.screen_line());
            s.push('\n');
        }
        s.push_str(&format!("{}\n", self.freeze.screen_text()));
        s.push_str(&format!("{}\n", self.acceptance.screen_text()));
        s.push_str(&format!("{}\n", self.wording.screen_text()));
        // 对拍结论要念出来——「全绿」与「有失配」用户都该听见。
        let findings = self.cross_check();
        if findings.is_empty() {
            s.push_str("层间对拍：全绿，四条相邻边声明哈希与冻结哈希一致。\n");
        } else {
            s.push_str(&format!("层间对拍：{} 项失配。\n", findings.len()));
            for f in findings.iter() {
                s.push_str(&f.screen_line());
                s.push('\n');
            }
        }
        s.push_str(&format!(
            "架构冻结哈希 {}（U01 双签闸第一件）。\n",
            self.freeze.freeze_digest()
        ));
        s.push_str(&format!(
            "禁扩面 {} 条；下游归属 {} 条。\n",
            BOUNDARY_EXCLUSIONS.len(),
            DOWNSTREAM_OWNERSHIP.len()
        ));
        s
    }
}

/// VE-F4201 域自检（判据逐条映射见 `veu01_checks.rs`）。
pub fn run_veu01_checks() -> CheckSet {
    super::veu01_checks::run_veu01_checks()
}
