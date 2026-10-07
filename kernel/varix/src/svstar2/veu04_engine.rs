//! VE-F4204 · 一致性规则引擎（VE-U 域 · 一致性域 · U04 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4204`
//!
//! **判据（锚点原文）**：三段式、四路单源、三元裁决、无豁免红线、可执行、判据。
//!
//! **职责定位（锚点原文）**：一致性规则引擎（规则三段式（条件→判定→处置）
//! 可执行化；规则来源四路（交互词典/视觉令牌/文案口径/契约判据——单源引用
//! 不复制）；规则冲突消解（优先级+时效+来源权威度三元裁决））。
//!
//! **数据结构（锚点原文·家族格式）**：规则册（三段式）；来源引用表；裁决器。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：规则引用断→阻断执行+登记；
//! 裁决僵局→升级人工；执行性能劣化→增量模式。
//!
//! **性能逐项分解（锚点原文·家族格式）**：执行 O(规则)；裁决 O(三元)；增量 O(变更)。
//!
//! **跨批对接点（锚点原文·家族格式）**：F4202 模型上游；F4206 扫描平台消费；
//! F4210-F4212 三源。
//!
//! **无障碍与隐私（锚点原文）**：无障碍规则处置仅两向（修复/阻断）不允许降级
//! 豁免（域本色红线）；无隐私面。
//!
//! # 一、三段式里「判定」段被写成「条件」段是本项最容易犯、也最难发现的错
//!
//! 三段式是 **条件 → 判定 → 处置**。前一版把 `condition` 字段直接拿去和
//! 实测值比相等（`if r.condition == actual`），于是产生两个后果，**都极隐蔽**：
//!
//! 1. `expect`（判定段，**唯一承载「什么算合规」的字段**）从未参与任何比较。
//!    探针实证：把 `expect` 改成「绝不可能命中的期望值」，执行结果一字不变。
//!    也就是说**规则作者写的判定标准完全不生效**，引擎自始至终没读过它。
//! 2. 语义整体反了：条件与实测值相等本应是「这条规则适用且已合规」，
//!    反过来被判成「不合规」并产出发现项。于是标准基线（`actual == expect`）
//!    反而报出 3 条发现项，而判据却写着「标准输入全量零发现」——
//!    **判据与实现必有一方是错的**，而这条判据此前从未被真正执行过。
//!
//! 本版的形态：**条件段是可求值的谓词名**（回答「这条规则此刻适不适用」），
//! **判定段是期望值**（回答「什么算合规」），**处置段是不符时怎么办**。
//! 三段各司其职，任一段被改动都会改变可观测行为——这是判据能钉住它的前提。
//!
//! # 二、四路来源「单源引用不复制」——用哈希兑现，而不是用字段形状承诺
//!
//! 锚点明文：四路来源「**单源引用不复制**」。反面写法是把判据正文**抄进规则册**：
//! 看起来规则自包含、无依赖、跑得快，代价是上游改了规则册那份不会跟着变，
//! 于是两处对同一件事给出不同答案，而**没有任何一处报错**。
//!
//! 故 [`Rule`] **只存来源引用**（[`SourceRef`]：来源路 + 上游条目号 + 来源内容
//! 哈希），不存判据正文。正文在**执行时**按引用去取：
//!
//! - 三路非契约 → [`RuleInputs`]（键为 `(来源路, 上游条目号)`）；
//! - 契约判据路 → F4203 [`ContractRegistry`]，按 **ID + 版本** 查（契约的
//!   判据正文存在注册册元模型的 `criteria` 字段里，那是它的家）。
//!
//! 取不到、或取到的内容哈希与登记时不符，都是「**引用断**」→ 阻断执行 + 登记
//! （降级矩阵第一格），**不是**「拿登记时那份哈希当合规证据继续跑」。
//!
//! 哈希这条防线不是装饰：它把「上游改了」从「不可察觉」变成「阻断级发现项」。
//!
//! # 三、三元裁决为什么会僵局，以及僵局为什么必须升级人工
//!
//! 裁决三元：**优先级 / 时效 / 来源权威度**。前两元连续可比，**来源权威度**
//! 是按域离散分级的。于是僵局形态很具体：规则 A 优先级高、规则 B 时效新，
//! 优先级指向 A、时效指向 B。
//!
//! 此时无论按什么固定顺序比，都会**永远偏向某一个维度**，而那个维度的权重
//! 本该由使用场景决定。所以 [`RuleEngine::arbitrate`] 在僵局时返回
//! [`E_ARBITRATION_STUCK`]，交人工（降级矩阵第二格）——**不得**偷偷按
//! 「优先级优先」决掉，那等于把时效维度永久作废。
//!
//! 但僵局与「全平票」必须分开：**全平票**（三元都弃权，即两条规则在三个维度
//! 上完全等价）**不构成冲突**，按登记序取前者即可。前一版把两者混为一谈
//! （三元全弃权时落到僵局分支），于是「两条完全等价的规则」会被升级给人工
//! ——这是把简单问题当成僵局推上去，人工很快就会学会忽略这类工单，真僵局
//! 反而被淹没。本版用 [`Side::Equivalent`] 显式表达第三种结果，让判据能分别
//! 钉住「僵局」与「全平票」。
//!
//! 还要说清**僵局的边界**：僵局的定义是「有维度指向 A、同时有维度指向 B」
//! （维度彼此对立），**不是**「三元必须全部指向同侧才算裁决」。后者会让
//! 权威度因同域而弃权时永远判僵局——而同域是最常见形态，于是三元裁决退化成
//! 「永不裁决」。某一元弃权不影响其余两元形成共识。
//!
//! # 四、无障碍处置只有两向：修复 / 阻断（红线，不可豁免）
//!
//! 锚点：「无障碍规则处置仅两向（修复/阻断）不允许降级豁免」。
//!
//! 关键在于**处置枚举里根本没有第三个选项**——不是「有第三项但禁止使用」，
//! 而是**类型层面不存在** [`Action::Waive`]。这比运行时拦截强：运行时拦截
//! 会被写一条 `if urgent { skip() }` 绕过去，而类型层面不存在的东西绕不过去。
//!
//! 因为枚举封闭，[`RuleEngine::request_waiver`] 只能恒拒，**不需要**任何
//! `E_A11Y_ACTION_INVALID` 之类的运行时错误码去拦一个不存在的状态——
//! 那种码是「假装类型没管住」的记账，删掉比留着诚实。
//!
//! 无障碍的第二道防线在**严重度**：无障碍发现项的严重度恒为
//! [`Severity::Blocking`]，不得降级成警告——警告意味着「可以先放行」，
//! 那就是变相豁免。
//!
//! # 五、增量模式不是「优化」，是**降级矩阵的第三条**（别当成可选项）
//!
//! 锚点把「执行性能劣化→增量模式」写在**错误路径矩阵**里，这决定了它的性质：
//! 全量是**正确**模式、增量是**降级**模式。实现上必须体现这个差别：
//!
//! - 全量：跑全册规则，产出完整发现项，覆盖范围自称 [`Coverage::Full`]；
//! - 增量：**只跑受本次变更影响的规则**，产出**局部**结果，且必须**显式标记**
//!   [`Coverage::Incremental`]，并保证下游能读到「本次结果不完整」。
//!
//! 若增量结果悄悄假装自己是全量，下游拿它当全量结论汇报，就会出现
//! 「扫描过了但漏了」——比明说「只扫了变更相关」危险得多。
//!
//! 增量的另一处易错点：白名单里写了**不存在的规则名**。静默跳过等于
//! 「变更实际没被验证却看不出来」，故未知规则名必须升级为阻断级发现项
//! （[`E_INCREMENTAL_UNKNOWN`]）。
//!
//! # 六、异常零静默：满了不许悄悄丢
//!
//! 发现项与引用断登记都受容量上限约束（内核无堆，量不能无限长）。但
//! **触顶时静默丢弃是最坏的一种处理**：登记项被丢掉之后，引用断从「有据可查」
//! 退化成「没发生过」，而执行方仍然返回了 `blocked > 0`，看上去一切正常。
//!
//! 故触顶时改为：把被丢弃的条目**降级成一条最简诊断串**追加到 [`DiagLog`]，
//! 并在 [`Verdict::diag_truncated`] 上留痕——数量对不上这件事本身必须能被读到。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::veu01_arch::{fnv1a64_hex, ConsistencyError, Severity};
use super::veu02_model::DomainTag;
use super::veu03_registry::{ContractMeta, ContractRegistry};

/// 规则来源四路（锚点明文：交互词典 / 视觉令牌 / 文案口径 / 契约判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuleSource {
    /// 交互词典。
    InteractionDict,
    /// 视觉令牌。
    VisualToken,
    /// 文案口径。
    Copywriting,
    /// 契约判据（F4203 注册册）。
    ContractCriteria,
}

impl RuleSource {
    /// 四路（**恰四路**，少一路即有来源无人负责）。
    pub const ALL: [RuleSource; 4] = [
        RuleSource::InteractionDict,
        RuleSource::VisualToken,
        RuleSource::Copywriting,
        RuleSource::ContractCriteria,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            RuleSource::InteractionDict => "交互词典",
            RuleSource::VisualToken => "视觉令牌",
            RuleSource::Copywriting => "文案口径",
            RuleSource::ContractCriteria => "契约判据",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            RuleSource::InteractionDict => "SRC-DICT",
            RuleSource::VisualToken => "SRC-TOKEN",
            RuleSource::Copywriting => "SRC-COPY",
            RuleSource::ContractCriteria => "SRC-CRITERIA",
        }
    }

    /// 由码反查（往返一致）。
    pub fn from_code(code: &str) -> Option<RuleSource> {
        RuleSource::ALL.iter().copied().find(|s| s.code() == code)
    }

    /// 该路的**权威域**（谁立这条源的约）。
    ///
    /// 依据锚点 F4201「承接 S 域交互词典 / T 域地区规则为首批契约源」：
    /// 首批契约源只覆盖 S 与 T 两域，交互词典与文案口径同源于 S，视觉令牌与
    /// 契约判据同源于 T。**本映射是裁决而非事实**——新增来源域时必须同时
    /// 改这里并重跑 `U04-来源-权威域映射与依据一致`，否则裁决器第三元会在
    /// 无声中改变指向。
    pub fn authority_domain(self) -> DomainTag {
        match self {
            RuleSource::InteractionDict | RuleSource::Copywriting => DomainTag::S,
            RuleSource::VisualToken | RuleSource::ContractCriteria => DomainTag::T,
        }
    }

    /// 该来源路的取正文方式（**四路各有各的查法**）。
    pub fn fetch_kind(self) -> FetchKind {
        match self {
            RuleSource::InteractionDict | RuleSource::VisualToken | RuleSource::Copywriting => {
                FetchKind::Inputs
            }
            // 契约判据正文住在注册册的元模型里，按 ID+版本查。
            RuleSource::ContractCriteria => FetchKind::Registry,
        }
    }
}

/// 正文取法（把「四路各有各的查法」变成可判定的枚举，而不是散落的 if）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchKind {
    /// 从 [`RuleInputs`] 取。
    Inputs,
    /// 从 F4203 [`ContractRegistry`] 取（需版本号）。
    Registry,
}

/// 处置（**枚举里没有豁免项**——见头注§四）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 修复（改实现使其合规）。
    Fix,
    /// 阻断（不合规不得放行）。
    Block,
}

impl Action {
    /// 全部处置（**恰两向**，红线）。
    pub const ALL: [Action; 2] = [Action::Fix, Action::Block];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Action::Fix => "修复",
            Action::Block => "阻断",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            Action::Fix => "ACT-FIX",
            Action::Block => "ACT-BLOCK",
        }
    }

    /// 由码反查（往返一致）。
    pub fn from_code(code: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.code() == code)
    }

    /// 该处置是否阻断放行。
    pub fn is_blocking(self) -> bool {
        matches!(self, Action::Block)
    }
}

/// 来源引用（**只存引用，不存正文**——见头注§二）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRef {
    /// 来源路。
    pub source: RuleSource,
    /// 上游条目号（可反查，如 `VE-F3982` / `U02-PALETTE-S`）。
    pub upstream: String,
    /// 契约判据路的版本（其余路为空串）。
    pub version: String,
    /// 来源内容哈希（**执行时比对**，用来发现上游已改而规则未跟）。
    pub content_hash: String,
}

impl SourceRef {
    /// 新建来源引用（非契约路）。
    pub fn new(source: RuleSource, upstream: &str, content_hash: &str) -> SourceRef {
        SourceRef {
            source,
            upstream: upstream.to_string(),
            version: String::new(),
            content_hash: content_hash.to_string(),
        }
    }

    /// 由内容实算哈希（避免手写常量与上游脱钩）。
    pub fn of_content(source: RuleSource, upstream: &str, content: &str) -> SourceRef {
        SourceRef {
            source,
            upstream: upstream.to_string(),
            version: String::new(),
            content_hash: fnv1a64_hex(content.as_bytes()),
        }
    }

    /// 契约判据路：带版本。
    pub fn of_contract(upstream: &str, version: &str, content: &str) -> SourceRef {
        SourceRef {
            source: RuleSource::ContractCriteria,
            upstream: upstream.to_string(),
            version: version.to_string(),
            content_hash: fnv1a64_hex(content.as_bytes()),
        }
    }

    /// 引用是否结构齐备（**这三项缺一，引用必断**）。
    pub fn is_wellformed(&self) -> bool {
        !self.upstream.trim().is_empty()
            && !self.content_hash.trim().is_empty()
            // 契约路必须带版本，否则同一 ID 的多个版本无从区分取哪一版。
            && (self.source != RuleSource::ContractCriteria || !self.version.trim().is_empty())
    }

    /// 引用残缺项清单（指名到项）。
    pub fn missing_parts(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        if self.upstream.trim().is_empty() {
            out.push("上游条目号");
        }
        if self.content_hash.trim().is_empty() {
            out.push("来源内容哈希");
        }
        if self.source == RuleSource::ContractCriteria && self.version.trim().is_empty() {
            out.push("契约版本");
        }
        out
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "来源 {} 上游 {}{} 哈希 {}",
            self.source.zh(),
            self.upstream,
            if self.version.is_empty() {
                String::new()
            } else {
                format!("@{}", self.version)
            },
            self.content_hash
        )
    }
}

/// 规则三段式：**条件 → 判定 → 处置**（三段缺一即不可执行，见头注§一）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// 规则码（唯一）。
    pub code: String,
    /// 第一段·条件：**可求值的谓词名**，回答「这条规则此刻适不适用」。
    ///
    /// 谓词真值由 [`PredicateBindings`] 提供（真实部署里由 F4206 扫描器绑定）。
    /// 条件不成立 → 本规则不适用，**跳过且不算发现项**。
    pub condition: String,
    /// 第二段·判定：**期望值**，回答「什么算合规」。执行时与来源实测正文比对。
    pub expect: String,
    /// 第三段·处置（**仅两向**）。
    pub action: Action,
    /// 来源引用（**唯一**，单源不复制）。
    pub origin: SourceRef,
    /// 裁决三元之一·优先级（大者胜）。
    pub priority: u8,
    /// 裁决三元之二·时效（**新者胜**，记为「距今天数」，小者新）。
    pub recency_days: u16,
    /// 是否无障碍规则（**无障碍处置不得豁免**，见头注§四）。
    pub is_a11y: bool,
}

impl Rule {
    /// 新建规则（三元取中性值，登记后可调）。
    pub fn new(
        code: &str,
        condition: &str,
        expect: &str,
        action: Action,
        origin: SourceRef,
    ) -> Rule {
        Rule {
            code: code.to_string(),
            condition: condition.to_string(),
            expect: expect.to_string(),
            action,
            origin,
            priority: 5,
            recency_days: 30,
            is_a11y: false,
        }
    }

    /// 设优先级。
    pub fn with_priority(mut self, p: u8) -> Rule {
        self.priority = p;
        self
    }

    /// 设时效（距今天数，**小者更新**）。
    pub fn with_recency(mut self, days: u16) -> Rule {
        self.recency_days = days;
        self
    }

    /// 标为无障碍规则（**处置仅两向，不可豁免**）。
    pub fn as_a11y(mut self) -> Rule {
        self.is_a11y = true;
        self
    }

    /// 三段齐备性（条件/判定/处置三段都要有实质内容）。
    pub fn is_complete(&self) -> bool {
        !self.code.trim().is_empty()
            && !self.condition.trim().is_empty()
            && !self.expect.trim().is_empty()
    }

    /// 缺失段位清单（指名到段）。
    pub fn missing_segments(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        if self.code.trim().is_empty() {
            out.push("规则码");
        }
        if self.condition.trim().is_empty() {
            out.push("条件");
        }
        if self.expect.trim().is_empty() {
            out.push("判定");
        }
        out
    }

    /// 规则指纹（**不含裁决三元**——三元是裁决参数不是规则语义，
    /// 改它不该让规则变成另一条）。
    pub fn digest(&self) -> String {
        let joined = format!(
            "{}|{}|{}|{}|{}|{}",
            self.code,
            self.condition,
            self.expect,
            self.action.code(),
            self.origin.source.code(),
            self.origin.upstream
        );
        fnv1a64_hex(joined.as_bytes())
    }

    /// 读屏单行（三段齐现，读者能独立复核规则语义）。
    pub fn screen_line(&self) -> String {
        format!(
            "规则 {}：当 {} 时实测值须为 {}，处置 {}（{}）",
            self.code,
            self.condition,
            self.expect,
            self.action.zh(),
            self.origin.screen_line()
        )
    }
}

/// 谓词绑定表：条件段的求值环境（键为谓词名）。
///
/// 真实部署里这张表由 F4206 扫描平台按扫描目标填充；本项只定义契约位。
pub type PredicateBindings = Vec<(String, bool)>;

/// 三元裁决的结果（**赢家 + 生效维度**，缺一则下游无从解释）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Adjudication {
    /// 赢家侧。
    pub winner: Side,
    /// 实际生效的维度。
    pub by: Dimension,
}

/// 赢家侧。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// A 胜。
    A,
    /// B 胜。
    B,
    /// 两侧在三元上完全等价（**不是冲突**），按登记序取先者。
    Equivalent,
}

impl Side {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Side::A => "A",
            Side::B => "B",
            Side::Equivalent => "两侧等价",
        }
    }
}

/// 裁决维度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    /// 优先级。
    Priority,
    /// 时效。
    Recency,
    /// 来源权威度。
    Authority,
    /// 按登记序（仅全平票时使用——**它不是冲突的裁决依据**）。
    Registration,
}

impl Dimension {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Dimension::Priority => "优先级",
            Dimension::Recency => "时效",
            Dimension::Authority => "来源权威度",
            Dimension::Registration => "登记序（非裁决）",
        }
    }
}

/// 裁决僵局（各维度指向不同赢家）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StuckCase {
    /// 参与僵局的规则 A/B 码。
    pub pair: (String, String),
    /// 三个维度各指向谁（`Side::A`/`Side::B`）。
    pub votes: [(Dimension, Side); 3],
}

impl StuckCase {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        let mut s = format!("裁决僵局 {} vs {}：", self.pair.0, self.pair.1);
        for (i, (dim, side)) in self.votes.iter().enumerate() {
            if i > 0 {
                s.push('、');
            }
            s.push_str(&format!("{}→{}", dim.zh(), side.zh()));
        }
        s.push_str("，三指向不一致");
        s
    }
}

/// 发现项（规则执行的产物；F4206 扫描平台消费的形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// 触发规则码。
    pub rule: String,
    /// 实测值（来源正文原样）。
    pub actual: String,
    /// 期望值（规则的判定段）。
    pub expect: String,
    /// 处置（仅两向）。
    pub action: Action,
    /// 严重度（无障碍规则恒为阻断）。
    pub severity: Severity,
    /// 是否无障碍发现项。
    pub is_a11y: bool,
    /// 发现项性质（**不混用字段**：让下游能分辨「不合规」与「引用断」）。
    pub kind: FindingKind,
}

/// 发现项性质。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FindingKind {
    /// 判定段与实测不符（**正常的不合规**）。
    Mismatch,
    /// 引用断：正文取不到或哈希不符（**判据不可信**，比不合规更严重）。
    BrokenRef,
    /// 增量白名单里写了册内不存在的规则名。
    UnknownRule,
}

impl FindingKind {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            FindingKind::Mismatch => "判定不符",
            FindingKind::BrokenRef => "引用断",
            FindingKind::UnknownRule => "未知规则",
        }
    }
}

impl Finding {
    /// 读屏单行（**异常零静默**）。
    pub fn screen_line(&self) -> String {
        format!(
            "发现[{}] {}：实测 {} 期望 {} 处置 {}{}",
            self.kind.zh(),
            self.rule,
            self.actual,
            self.expect,
            self.action.zh(),
            if self.is_a11y { "（无障碍·不得豁免）" } else { "" }
        )
    }
}

/// 执行结果（**显式声明覆盖范围**，见头注§五）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// 发现项。
    pub findings: Vec<Finding>,
    /// 覆盖范围：**全量** 或 **仅变更相关**。
    pub coverage: Coverage,
    /// 阻断数（处置为阻断的发现项数）。
    pub blocked: usize,
    /// 本次**实际执行**的规则数（**判据据此钉住「增量真的只跑了一部分」**）。
    pub executed: usize,
    /// 条件不成立而跳过的规则数（跳过 ≠ 合规，必须能被分开计数）。
    pub skipped: usize,
    /// 因容量触顶而未能完整登记的诊断条数（**见头注§六**，非零即须查）。
    pub diag_truncated: usize,
}

impl Verdict {
    /// 合规数（**已执行 − 不合规 − 引用断 − 未知规则**）。
    ///
    /// 单列出来是因为「跳过」与「合规」是两件完全不同的事，混在一起就会出现
    /// 「因为条件没成立所以零发现」被当成「全绿」。
    ///
    /// 口径要点：**`skipped` 不在这里再减一次**。跳过的规则根本没进
    /// `executed`（谓词为假 → `continue`，本轮不执行），若再减一次就成了
    /// 重复扣减——实测 `executed=3, skipped=1` 的混合场会算出 `compliant=2`
    /// 而非 3，于是「合规数」既不等于执行数也不等于任何真实量纲，
    /// 按它做的断言全部失去意义。
    pub fn compliant(&self) -> usize {
        self.executed.saturating_sub(self.findings.len())
    }
}

/// 覆盖范围（增量必须显式标记，否则下游会当全量用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// 全量（全册规则都跑过）。
    Full,
    /// 增量（只跑了受变更影响的规则，**结果不完整**）。
    Incremental,
}

impl Coverage {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Coverage::Full => "全量",
            Coverage::Incremental => "增量（结果不完整，不得当全量结论）",
        }
    }

    /// 是否完整（**下游据此决定能不能汇报**）。
    pub fn is_complete(self) -> bool {
        matches!(self, Coverage::Full)
    }
}

/// 引用断登记（降级矩阵第一格：阻断执行 + 登记）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokenRef {
    /// 规则码。
    pub rule: String,
    /// 上游条目号（断在哪）。
    pub upstream: String,
    /// 登记时的期望哈希。
    pub expect_hash: String,
    /// 执行时实得的哈希（取不到正文时为空串）。
    pub actual_hash: String,
    /// 断因。
    pub cause: BrokenCause,
}

/// 引用断成因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrokenCause {
    /// 正文取不到（输入未提供 / 注册册无此条目）。
    NotFound,
    /// 正文取到了但哈希与登记时不符（**上游改了而规则未跟**）。
    HashDrift,
}

impl BrokenCause {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            BrokenCause::NotFound => "取不到正文",
            BrokenCause::HashDrift => "上游已改，规则未跟",
        }
    }
}

impl BrokenRef {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "引用断 {}：上游 {} 因{}，登记哈希 {} 实得 {}",
            self.rule,
            self.upstream,
            self.cause.zh(),
            self.expect_hash,
            if self.actual_hash.is_empty() {
                "(无)"
            } else {
                &self.actual_hash
            }
        )
    }
}

/// 错误码。
pub const E_RULE_DUP: &str = "E_RULE_DUP";
pub const E_RULE_INCOMPLETE: &str = "E_RULE_INCOMPLETE";
pub const E_RULE_REF_MALFORMED: &str = "E_RULE_REF_MALFORMED";
pub const E_RULE_REF_BROKEN: &str = "E_RULE_REF_BROKEN";
pub const E_ARBITRATION_STUCK: &str = "E_ARBITRATION_STUCK";
pub const E_A11Y_WAIVE_FORBIDDEN: &str = "E_A11Y_WAIVE_FORBIDDEN";
pub const E_SOURCE_COVERAGE: &str = "E_SOURCE_COVERAGE";
pub const E_INCREMENTAL_UNKNOWN: &str = "E_INCREMENTAL_UNKNOWN";
pub const E_CAP: &str = "E_CAP";

/// 容量上限。
pub const MAX_RULES: usize = 64;
pub const MAX_FINDINGS: usize = 64;
pub const MAX_BROKEN: usize = 64;
pub const MAX_STUCK: usize = 32;
pub const MAX_DIAG: usize = 16;

/// 执行输入（**执行时按来源引用去取正文**，不在规则册里存）。
///
/// 键为 `(来源路, 上游条目号)`。
pub type RuleInputs = Vec<(RuleSource, String, String)>;

/// 诊断日志（容量触顶时的降级留痕，见头注§六）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagLog {
    /// 条目。
    pub items: Vec<String>,
}

impl DiagLog {
    /// 空日志。
    pub fn new() -> DiagLog {
        DiagLog { items: Vec::new() }
    }

    /// 记一条（**满了即溢出计数，不静默丢弃**）。
    pub fn note(&mut self, text: String, cap: usize) -> bool {
        if self.items.len() >= cap {
            return false;
        }
        self.items.push(text);
        true
    }

    /// 是否发生过溢出。
    pub fn truncated(&self) -> bool {
        !self.items.is_empty() && self.items.len() >= MAX_DIAG
    }

    /// 读屏单行（**溢出必须能被读出来**）。
    pub fn screen_text(&self) -> String {
        let mut s = String::new();
        for it in self.items.iter() {
            s.push_str(it);
            s.push('\n');
        }
        s
    }
}

/// 一致性规则引擎（三段式 + 四路单源 + 三元裁决）。
#[derive(Clone, Debug, Default)]
pub struct RuleEngine {
    rules: Vec<Rule>,
    findings: Vec<Finding>,
    broken: Vec<BrokenRef>,
    stuck: Vec<StuckCase>,
    diag: DiagLog,
}

impl RuleEngine {
    /// 空引擎。
    pub fn new() -> RuleEngine {
        RuleEngine {
            rules: Vec::new(),
            findings: Vec::new(),
            broken: Vec::new(),
            stuck: Vec::new(),
            diag: DiagLog::new(),
        }
    }

    /// 规则册只读遍历。
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// 发现项只读遍历。
    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }

    /// 引用断只读遍历。
    pub fn broken_refs(&self) -> &[BrokenRef] {
        &self.broken
    }

    /// 僵局只读遍历。
    pub fn stuck_cases(&self) -> &[StuckCase] {
        &self.stuck
    }

    /// 诊断日志只读遍历。
    pub fn diag(&self) -> &DiagLog {
        &self.diag
    }

    /// 取规则。
    pub fn rule(&self, code: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.code == code)
    }

    /// 登记规则（四路单源、三段齐备）。
    pub fn register(&mut self, r: Rule) -> Result<(), ConsistencyError> {
        if !r.is_complete() {
            return Err(ConsistencyError::new(
                E_RULE_INCOMPLETE,
                "规则登记被拒：三段式残缺",
                &format!(
                    "规则 {} 缺段位：{:?}（三段=条件→判定→处置）",
                    r.code,
                    r.missing_segments()
                ),
                "三段缺一即不可执行；缺判定期望值的规则只能靠猜",
                "规则作者",
            ));
        }
        // 引用结构校验：上一版这里写的是
        // `RuleSource::from_code(r.origin.source.code()).is_none()`——那是
        // **恒假的死守卫**：`source` 是枚举，`code()` 的返回值按定义必然能
        // `from_code` 反查回自己，于是这个分支永远不会走，而它看起来像个
        // 「来源合法性检查」。真正会失败的是引用三要素缺失，改为查它。
        if !r.origin.is_wellformed() {
            return Err(ConsistencyError::new(
                E_RULE_REF_MALFORMED,
                "规则登记被拒：来源引用结构不完整",
                &format!(
                    "规则 {} 的来源引用缺项：{:?}（引用三要素=上游条目号+内容哈希{}）",
                    r.code,
                    r.origin.missing_parts(),
                    if r.origin.source == RuleSource::ContractCriteria {
                        "+契约版本"
                    } else {
                        ""
                    }
                ),
                "引用残缺必然导致执行期引用断；须在登记时补齐，别等跑起来才发现判据不可信",
                "规则作者",
            ));
        }
        if self.rules.iter().any(|x| x.code == r.code) {
            return Err(ConsistencyError::new(
                E_RULE_DUP,
                "规则登记被拒：规则码重复",
                &format!("规则码 {} 已在册内（指纹 {}）", r.code, r.digest()),
                "同码规则只登记一次；改内容须改码或走来源更新",
                "规则作者",
            ));
        }
        if self.rules.len() >= MAX_RULES {
            return Err(ConsistencyError::new(
                E_CAP,
                "规则登记被拒：达到容量上限",
                &format!("规则册 {} 条达到上限 {}", self.rules.len(), MAX_RULES),
                "先废止失效规则，或分册",
                "规则作者",
            ));
        }
        self.rules.push(r);
        Ok(())
    }

    /// 废止规则（**只摘自己发出的那条**，返回是否确实摘掉）。
    pub fn revoke(&mut self, code: &str) -> bool {
        let before = self.rules.len();
        self.rules.retain(|r| r.code != code);
        self.rules.len() != before
    }

    /// 四路来源齐备自检（**四路单源**，少一路即有来源无人负责）。
    pub fn sources_complete(&self) -> bool {
        RuleSource::ALL
            .iter()
            .all(|s| self.rules.iter().any(|r| r.origin.source == *s))
    }

    /// 缺失的来源路（指名是哪一路，不是只说「不齐」）。
    pub fn missing_sources(&self) -> Vec<RuleSource> {
        RuleSource::ALL
            .iter()
            .copied()
            .filter(|s| !self.rules.iter().any(|r| r.origin.source == *s))
            .collect()
    }

    /// 查输入（**O(输入)** 线性小表）。
    fn fetch_input<'a>(
        inputs: &'a RuleInputs,
        src: RuleSource,
        upstream: &str,
    ) -> Option<&'a str> {
        inputs
            .iter()
            .find(|(s, u, _)| *s == src && u == upstream)
            .map(|(_, _, c)| c.as_str())
    }

    /// 按来源路取正文（**四路各有各的查法**）。
    fn fetch_body<'a>(
        origin: &SourceRef,
        inputs: &'a RuleInputs,
        registry: &'a ContractRegistry,
    ) -> Option<&'a str> {
        match origin.source.fetch_kind() {
            FetchKind::Inputs => Self::fetch_input(inputs, origin.source, &origin.upstream),
            FetchKind::Registry => registry
                .lookup(&origin.upstream, &origin.version)
                .map(|e| e.meta.criteria.as_str()),
        }
    }

    /// 求值条件段（谓词查表）。
    fn condition_holds(r: &Rule, bindings: &PredicateBindings) -> Option<bool> {
        bindings
            .iter()
            .find(|(name, _)| *name == r.condition)
            .map(|(_, v)| *v)
    }

    /// **执行规则册**（全量，O(规则)）。
    ///
    /// 每条规则走三段：条件段（谓词是否成立）→ 取正文与哈希核对（引用是否断）
    /// → 判定段（实测 vs 期望）→ 处置段。
    ///
    /// **引用断即阻断执行**（降级矩阵第一格）：不断在本地跑、也不静默跳过，
    /// 而是产出一条阻断级发现项并登记断点——因为引用断意味着**判据不可信**，
    /// 此时任何「合规」结论都是无根据的。
    pub fn run_full(
        &mut self,
        inputs: &RuleInputs,
        registry: &ContractRegistry,
        bindings: &PredicateBindings,
    ) -> Verdict {
        self.begin_run();
        let rules = self.rules.clone();
        let mut executed = 0usize;
        let mut skipped = 0usize;
        for r in rules.iter() {
            // 段一·条件：谓词未绑定 → **不适用**（跳过，不是不合规）。
            match Self::condition_holds(r, bindings) {
                Some(true) => {}
                Some(false) => {
                    skipped += 1;
                    continue;
                }
                None => {
                    // 未绑定谓词：既不能判合规也不能判不合规，且**不可静默**——
                    // 记一条引用断（判据无法求值 = 判据不可信）。
                    // 计数口径：**只记一次**。上一版这里既 `executed += 1`
                    // 又 `skipped += 1`，同一条规则被数了两遍，于是
                    // 「执行数 + 跳过数」恒大于规则总数，按总数算的判据全错。
                    // 语义上它属于「尝试过但无法判定」：计入 executed，
                    // 因为它确实走完了执行路径并产出了阻断级发现项。
                    self.note_broken(
                        r,
                        BrokenCause::NotFound,
                        &r.origin.content_hash.clone(),
                        String::new(),
                    );
                    self.push_finding(r, "(条件未绑定)", r.condition.as_str(), FindingKind::BrokenRef);
                    executed += 1;
                    continue;
                }
            }
            executed += 1;
            // 段二·取正文 + 哈希核对（单源不复制在此兑现）。
            let body = match Self::fetch_body(&r.origin, inputs, registry) {
                Some(b) => b.to_string(),
                None => {
                    self.note_broken(
                        r,
                        BrokenCause::NotFound,
                        &r.origin.content_hash.clone(),
                        String::new(),
                    );
                    self.push_finding(r, "(引用断)", r.origin.upstream.as_str(), FindingKind::BrokenRef);
                    continue;
                }
            };
            let now_hash = fnv1a64_hex(body.as_bytes());
            if now_hash != r.origin.content_hash {
                // 上游改了而规则未跟：**判断据不可信**，不是「算通过」。
                self.note_broken(
                    r,
                    BrokenCause::HashDrift,
                    &r.origin.content_hash.clone(),
                    now_hash,
                );
                self.push_finding(r, body.as_str(), r.origin.upstream.as_str(), FindingKind::BrokenRef);
                continue;
            }
            // 段三·判定：实测（来源正文）vs 期望（判定段）。
            if body == r.expect {
                continue;
            }
            self.push_finding(r, body.as_str(), r.expect.as_str(), FindingKind::Mismatch);
        }
        self.finish(Coverage::Full, executed, skipped)
    }

    /// **增量执行**（降级矩阵第三格，见头注§五）。
    ///
    /// 只跑 `changed` 列出的规则，但**结果必须显式标记不完整**。
    pub fn run_incremental(
        &mut self,
        changed: &[String],
        inputs: &RuleInputs,
        registry: &ContractRegistry,
        bindings: &PredicateBindings,
    ) -> Verdict {
        self.begin_run();
        // 未知规则名 → 显性登记为阻断项（静默跳过 = 变更白名单写了错字没人知道）。
        // 这些项在**收集完之后**才与规则发现项合并，不能中途被清。
        let mut unknowns: Vec<Finding> = Vec::new();
        for c in changed.iter() {
            if self.rule(c).is_none() {
                unknowns.push(Finding {
                    rule: c.clone(),
                    actual: "(册内无此规则)".to_string(),
                    expect: "规则须在册内".to_string(),
                    action: Action::Block,
                    severity: Severity::Blocking,
                    is_a11y: false,
                    kind: FindingKind::UnknownRule,
                });
            }
        }
        // 只把 changed 内的**已登记**规则纳入本次执行（白名单重复列出时只跑一次）。
        let mut inc = RuleEngine::new();
        for c in changed.iter() {
            if inc.rule(c).is_some() {
                continue;
            }
            match self.rule(c) {
                Some(r) => {
                    // 登记失败必须出声：源规则来自本册、且本册已过登记闸，
                    // 走到这里失败说明**引擎内部状态不自洽**，不能 let _ 吞掉。
                    if let Err(err) = inc.register(r.clone()) {
                        unknowns.push(Finding {
                            rule: c.clone(),
                            actual: "(增量装配失败)".to_string(),
                            expect: err.screen_text(),
                            action: Action::Block,
                            severity: Severity::Blocking,
                            is_a11y: r.is_a11y,
                            kind: FindingKind::BrokenRef,
                        });
                    }
                }
                None => {}
            }
        }
        // 复用全量执行（**同一份三段语义**，增量与全量的差别只在规则子集）。
        let v = inc.run_full(inputs, registry, bindings);
        let executed = v.executed;
        let skipped = v.skipped;
        self.findings = v.findings;
        for u in unknowns {
            if self.findings.len() >= MAX_FINDINGS {
                self.note_diag(format!(
                    "增量未知规则 {} 因发现项触顶未登记",
                    u.rule
                ));
                break;
            }
            self.findings.push(u);
        }
        self.broken = inc.broken.clone();
        self.diag = inc.diag.clone();
        self.finish(Coverage::Incremental, executed, skipped)
    }

    /// 执行前清场（**僵局/诊断一并清**：上一次执行的痕迹不得冒充本次）。
    fn begin_run(&mut self) {
        self.findings.clear();
        self.broken.clear();
        self.stuck.clear();
        self.diag = DiagLog::new();
    }

    /// 执行后汇总（`blocked` 由发现项实算，不手填）。
    fn finish(&mut self, coverage: Coverage, executed: usize, skipped: usize) -> Verdict {
        let blocked = self
            .findings
            .iter()
            .filter(|f| f.action.is_blocking())
            .count();
        Verdict {
            findings: self.findings.clone(),
            coverage,
            blocked,
            executed,
            skipped,
            diag_truncated: self.diag.items.len(),
        }
    }

    /// 记一条引用断（**触顶时转诊断，不静默丢**，见头注§六）。
    fn note_broken(
        &mut self,
        r: &Rule,
        cause: BrokenCause,
        expect_hash: &str,
        actual_hash: String,
    ) {
        let item = BrokenRef {
            rule: r.code.clone(),
            upstream: r.origin.upstream.clone(),
            expect_hash: expect_hash.to_string(),
            actual_hash,
            cause,
        };
        if self.broken.len() < MAX_BROKEN {
            self.broken.push(item);
        } else {
            self.note_diag(format!("引用断 {} 登记触顶被降级为诊断", r.code));
        }
    }

    /// 记一条诊断。
    fn note_diag(&mut self, text: String) {
        if !self.diag.note(text, MAX_DIAG) {
            // 连诊断都满了：只能记在计数上，由 `diag_truncated` 暴露。
            self.diag.items.push(String::from("(诊断亦已触顶)"));
        }
    }

    /// 产出一条发现项（**无障碍发现项严重度恒为阻断**）。
    fn push_finding(
        &mut self,
        r: &Rule,
        actual: &str,
        expect: &str,
        kind: FindingKind,
    ) {
        if self.findings.len() >= MAX_FINDINGS {
            self.note_diag(format!("发现项 {} 触顶被丢弃", r.code));
            return;
        }
        // 无障碍发现项**严重度恒为阻断**（降级成警告就是变相豁免）。
        let severity = if r.is_a11y {
            Severity::Blocking
        } else if kind == FindingKind::Mismatch {
            Severity::Warning
        } else {
            // 引用断与未知规则：判据不可信 / 变更没被验证，一律阻断。
            Severity::Blocking
        };
        self.findings.push(Finding {
            rule: r.code.clone(),
            actual: actual.to_string(),
            expect: expect.to_string(),
            // **处置不得沿用规则自声明的值**（除非该规则本就阻断）。
            // 上一版这里写 `action: r.action`：一条声明「修复」的普通规则一旦
            // 引用断，产出的发现项处置是「修复」→ 不计入 blocked → 下游看
            // `blocked == 0` 放行。**判据不可信时唯一安全的处置是阻断**，
            // 「修复」是拿一个不可信的判据去改实现，改完仍然不可信。
            // 这也是「引用断即阻断执行」在数据面上的兑现点。
            action: if r.action.is_blocking() || kind != FindingKind::Mismatch {
                Action::Block
            } else {
                r.action
            },
            severity,
            is_a11y: r.is_a11y,
            kind,
        });
    }

    /// **三元裁决**（优先级 / 时效 / 来源权威度，O(三元)）。
    ///
    /// 返回**赢家 + 生效维度**：只返回维度而不返回赢家，下游无从知道该听谁的。
    /// 全平票（三元皆等价）→ [`Side::Equivalent`]，**不算冲突**。
    /// 僵局（各维度指向不同赢家）→ 登记并返回 [`E_ARBITRATION_STUCK`]，
    /// **不擅自按某一维度决掉**（见头注§三）。
    pub fn arbitrate(
        &mut self,
        a: &Rule,
        b: &Rule,
    ) -> Result<Adjudication, ConsistencyError> {
        let votes = [
            (
                Dimension::Priority,
                Self::pick_by_priority(a, b),
            ),
            (Dimension::Recency, Self::pick_by_recency(a, b)),
            (Dimension::Authority, Self::pick_by_authority(a, b)),
        ];
        let mut a_votes = 0usize;
        let mut b_votes = 0usize;
        for (_, side) in votes.iter() {
            match side {
                Side::A => a_votes += 1,
                Side::B => b_votes += 1,
                Side::Equivalent => {}
            }
        }
        // 三元全等价 → 不是冲突。
        if a_votes == 0 && b_votes == 0 {
            return Ok(Adjudication {
                winner: Side::Equivalent,
                by: Dimension::Registration,
            });
        }
        // 一致指向同侧 → 指名**实际生效**的那个维度（下游据此解释裁决依据）。
        //
        // 口径说明（本版修正过一次）：曾写成「三元全指向同侧（3:0）才算一致」，
        // 那是把判据写错逼成了实现错——后果是**只要权威度那一元因同域而
        // 不表态，就永远判僵局**。而权威度同域恰恰是最常见形态（词典与文案
        // 同源 S，令牌与契约判据同源 T），于是三元裁决在多数真实场景下
        // 退化成「永不裁决」，比没有三元裁决更坏。
        //
        // 正确口径：**僵局的定义是「有维度指向 A、同时有维度指向 B」**，
        // 即维度彼此对立；某一元弃权不影响其余两元形成共识。
        if a_votes == 0 {
            for (dim, side) in votes.iter() {
                if *side == Side::B {
                    return Ok(Adjudication { winner: Side::B, by: *dim });
                }
            }
        }
        if b_votes == 0 {
            for (dim, side) in votes.iter() {
                if *side == Side::A {
                    return Ok(Adjudication { winner: Side::A, by: *dim });
                }
            }
        }
        // 僵局：登记并拒（**不按固定维度顺序决掉**）。
        let case = StuckCase {
            pair: (a.code.clone(), b.code.clone()),
            votes,
        };
        let line = case.screen_line();
        if self.stuck.len() < MAX_STUCK {
            self.stuck.push(case);
        } else {
            self.note_diag(format!("裁决僵局登记触顶：{}", line));
        }
        Err(ConsistencyError::new(
            E_ARBITRATION_STUCK,
            "规则裁决僵局：已升级人工",
            &line,
            "人工裁定权重（哪个维度对本场景更重要），或改规则使三元不再冲突；\
             本项不按固定维度顺序默认决——那会让某一维度永久作废",
            "一致性仲裁员",
        ))
    }

    /// 维度一：优先级（高者胜；相等即该维度不表态）。
    fn pick_by_priority(a: &Rule, b: &Rule) -> Side {
        if a.priority > b.priority {
            Side::A
        } else if b.priority > a.priority {
            Side::B
        } else {
            Side::Equivalent
        }
    }

    /// 维度二：时效（**距今天数小者更新**，新者胜）。
    fn pick_by_recency(a: &Rule, b: &Rule) -> Side {
        if a.recency_days < b.recency_days {
            Side::A
        } else if b.recency_days < a.recency_days {
            Side::B
        } else {
            Side::Equivalent
        }
    }

    /// 维度三：来源权威度（权威域优先；同域即该维度不表态）。
    fn pick_by_authority(a: &Rule, b: &Rule) -> Side {
        let aa = a.origin.source.authority_domain();
        let ba = b.origin.source.authority_domain();
        if aa > ba {
            Side::A
        } else if ba > aa {
            Side::B
        } else {
            Side::Equivalent
        }
    }

    /// 请求豁免无障碍处置（**恒拒**，见头注§四）。
    ///
    /// 因 [`Action`] 只有两向，编译器保证这里无需运行时状态检查——
    /// 「无豁免」不是一条会被绕过的规则，而是类型层面不存在的一条路。
    pub fn request_waiver(&self, rule: &str, reason: &str) -> Result<(), ConsistencyError> {
        let _ = reason;
        Err(ConsistencyError::new(
            E_A11Y_WAIVE_FORBIDDEN,
            "无障碍处置豁免被拒：无此选项",
            &format!("规则 {} 被要求豁免其无障碍处置", rule),
            "无障碍规则处置只有修复/阻断两向；要做的是改实现，\
             而不是让规则对自己的失败放行",
            "豁免申请方",
        ))
    }

    /// 引擎自检（四路齐备 + 三段齐备 + 引用结构齐备）。
    pub fn self_audit(&self) -> Vec<String> {
        let mut issues: Vec<String> = Vec::new();
        let missing = self.missing_sources();
        if !missing.is_empty() {
            let names: Vec<&str> = missing.iter().map(|s| s.zh()).collect();
            issues.push(format!("{}:四路来源未齐（缺 {}）", E_SOURCE_COVERAGE, names.join("、")));
        }
        for r in self.rules.iter() {
            if !r.is_complete() {
                issues.push(format!("{}:规则 {} 三段残缺", E_RULE_INCOMPLETE, r.code));
            }
            if !r.origin.is_wellformed() {
                issues.push(format!(
                    "{}:规则 {} 引用残缺（缺 {:?}）",
                    E_RULE_REF_MALFORMED,
                    r.code,
                    r.origin.missing_parts()
                ));
            }
        }
        issues
    }

    /// 读屏总览。
    pub fn screen_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "规则册：{} 条规则、{} 条发现项、{} 项引用断、{} 项裁决僵局、{} 条诊断\n",
            self.rules.len(),
            self.findings.len(),
            self.broken.len(),
            self.stuck.len(),
            self.diag.items.len()
        ));
        for r in self.rules.iter() {
            s.push_str(&r.screen_line());
            s.push('\n');
        }
        for f in self.findings.iter() {
            s.push_str(&f.screen_line());
            s.push('\n');
        }
        for b in self.broken.iter() {
            s.push_str(&b.screen_line());
            s.push('\n');
        }
        s.push_str(&self.diag.screen_text());
        s
    }
}

/// 标准规则册（**四路齐备、三段可执行的正样本**，兼作回归基线）。
///
/// 四路各一：交互词典（焦点顺序）、视觉令牌（主色）、文案口径（术语）、
/// 契约判据（对比度，且**标为无障碍**）。
///
/// **注意条件段与判定段的关系**：条件段是**谓词名**（「实测值是否可取」），
/// 判定段才是**期望值**。上一版把两者混同，导致判定段从未参与判定。
pub fn standard_engine() -> RuleEngine {
    let mut e = RuleEngine::new();

    // 路一·交互词典（S 域）：焦点顺序须为指定串。
    e.register(
        Rule::new(
            "U04-RULE-FOCUS",
            "焦点顺序可取",
            "标题→主区→辅区→状态栏",
            Action::Fix,
            SourceRef::of_content(RuleSource::InteractionDict, "VE-F3982", "标题→主区→辅区→状态栏"),
        )
        .with_priority(8)
        .with_recency(5),
    )
    .expect("词典路规则登记");

    // 路二·视觉令牌（T 域）：主色。
    e.register(
        Rule::new(
            "U04-RULE-PALETTE",
            "主色可取",
            "#1A1A1A",
            Action::Fix,
            SourceRef::of_content(RuleSource::VisualToken, "U02-PALETTE-S", "#1A1A1A"),
        )
        .with_priority(7)
        .with_recency(20),
    )
    .expect("令牌路规则登记");

    // 路三·文案口径（S 域）：术语。
    e.register(
        Rule::new(
            "U04-RULE-TERM",
            "保存按钮文案可取",
            "保存",
            Action::Fix,
            SourceRef::of_content(RuleSource::Copywriting, "U02-TERM-SAVE-S", "保存"),
        )
        .with_priority(6)
        .with_recency(1),
    )
    .expect("口径路规则登记");

    // 路四·契约判据（T 域）+ **无障碍**（处置阻断，不得豁免）。
    e.register(
        Rule::new(
            "U04-RULE-CONTRAST",
            "对比度判据可取",
            "对比度不低于 4.5:1 且焦点可达",
            Action::Block,
            SourceRef::of_contract(
                "U03-CTR-CONTRAST",
                "v1",
                "对比度不低于 4.5:1 且焦点可达",
            ),
        )
        .with_priority(9)
        .with_recency(3)
        .as_a11y(),
    )
    .expect("判据路规则登记");

    e
}

/// 标准输入（**与标准规则册逐条对应**，供全量执行）。
pub fn standard_inputs() -> RuleInputs {
    vec![
        (
            RuleSource::InteractionDict,
            "VE-F3982".to_string(),
            "标题→主区→辅区→状态栏".to_string(),
        ),
        (
            RuleSource::VisualToken,
            "U02-PALETTE-S".to_string(),
            "#1A1A1A".to_string(),
        ),
        (RuleSource::Copywriting, "U02-TERM-SAVE-S".to_string(), "保存".to_string()),
    ]
}

/// 标准谓词绑定（**四路条件全部为真**，即全部规则都适用）。
pub fn standard_bindings() -> PredicateBindings {
    vec![
        ("焦点顺序可取".to_string(), true),
        ("主色可取".to_string(), true),
        ("保存按钮文案可取".to_string(), true),
        ("对比度判据可取".to_string(), true),
    ]
}

/// 标准契约登记册增量（**契约判据路的正文住在 F4203 注册册里**）。
///
/// 与 [`veu03_registry::standard_registry`] 合并即得标准执行环境——
/// 规则册不存契约判据正文，只存 ID+版本+哈希（单源引用不复制）。
pub fn standard_contract_input() -> ContractMeta {
    ContractMeta::new(
        "U03-CTR-CONTRAST",
        "v1",
        DomainTag::T,
        vec![DomainTag::S, DomainTag::U],
        "对比度不低于 4.5:1 且焦点可达",
    )
}
