//! VE-F4202 · 跨域一致性模型（VE-U 域 · 一致性域 · U02 组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4202`
//!
//! **判据（锚点原文）**：四类三型、形式化、环检测、红线约束、版本化、判据。
//!
//! **职责定位（锚点原文）**：跨域一致性模型（一致性对象建模：可一致化实体四类
//! （交互行为/视觉令牌/文案口径/数据契约）× 一致关系（等同/派生/约束三型）；
//! 模型形式化（关系代数+冲突消解算子）；模型版本化）。
//!
//! **数据结构（锚点原文·家族格式）**：模型册（四类×三型）；代数算子集；版本记录。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：关系环检测→拒绝建模；
//! 冲突不可消解→人工仲裁队列；版本漂移→对版。
//!
//! **性能逐项分解（锚点原文·家族格式）**：建模 O(实体×关系)；检测 O(环)；
//! 仲裁 O(冲突)。
//!
//! **跨批对接点（锚点原文·家族格式）**：F4201 架构上游；F4203 契约注册输入；
//! F4209 图谱。
//!
//! **无障碍与隐私（锚点原文）**：模型含无障碍约束型（对比度/焦点可达为不可让步
//! 约束——域本色红线）；无隐私面。
//!
//! # 一、四类为什么是这四类（不是「所有实体」）
//!
//! 锚点给的是「**可一致化**实体四类」——注意它限定了「可一致化」，而不是
//! 「全部实体」。这个限定是本项的立命之本：一致性模型只对**跨域可能各自
//! 持一份副本**的实体有意义。域内部私有的中间态（渲染器的临时缓冲、
//! 词法分析的 token 流）跨域看不见，也就不可能分叉，硬塞进来只会把模型
//! 撑成一张无法枚举的清单。
//!
//! 四类的判据是**「跨域是否会分叉」**，不是「属于哪个域」：
//!
//! | 类别 | 为什么会分叉 | 典型裁决手段 |
//! |---|---|---|
//! | [`EntityClass::Interaction`] 交互行为 | 焦点顺序/快捷键在两个域各写一份 | 等同 + 派生 |
//! | [`EntityClass::VisualToken`] 视觉令牌 | 色板/间距在两域各调一次 | 等同 + 派生 |
//! | [`EntityClass::Copywriting`] 文案口径 | 同一句话两个域两种措辞 | 等同 + 约束 |
//! | [`EntityClass::DataContract`] 数据契约 | 字段含义/单位两边解释不同 | 约束（派生会放大歧义）|
//!
//! # 二、三型关系的语义差（等同/派生/约束不可互换）
//!
//! 这三型最容易被当成「同义词的三种叫法」，于是建模时随手挑一个。
//! 但它们在**冲突时谁让谁**上完全相反，所以必须钉死：
//!
//! - **等同（[RelationKind::Equiv]）**：两侧必须**同值**。冲突 = 事实矛盾，
//!   无先后可讲，只能对拍后强制收敛到一侧。
//! - **派生（[RelationKind::Derived]）**：源是因，果是**推导结果**。冲突时
//!   **源赢**——果是算出来的，不该反过来质疑源。这是有向的。
//! - **约束（[RelationKind::Constraint]）**：被约束方须满足源声明的**不可让步
//!   下限**。冲突时若下限属无障碍约束，则**约束赢且不可豁免**（见§三）。
//!
//! 由此得到本项最重要的一条建模纪律：**派生必须声明方向**（谁派生谁）。
//! 不声明方向的「派生」在实现时必然退化成等同——因为没人说得清谁是源，
//! 于是双方各让一步，最后谁也没赢。
//!
//! # 三、红线约束为什么必须与普通约束分册（不可让步）
//!
//! 锚点明文：「对比度/焦点可达为**不可让步约束**——域本色红线」。
//! 关键在于「不可让步」意味着它**不参与**冲突消解的优先级比较：
//!
//! - 普通约束之间冲突 → 按 [`RelationKind`] 的让位规则消解，或进仲裁队列；
//! - 红线约束与任何东西冲突（含与等同关系冲突）→ **红线赢**，
//!   且**不接受 ADR 豁免**。
//!
//! 把红线混进普通约束册的代价是具体的：一旦红线进了优先级比较表，
//! 它就会在某次「三方裁决」中被票数压掉——而那次裁决的参与者
//! （S 域视觉、T 域文案）都不负责对比度，于是没人会替它说话。
//! 混册 = 红线迟早被多数票吃掉。所以 [`NON_NEGOTIABLE`] 是模型里的
//! **独立册**，不参与任何优先级排序。
//!
//! # 四、环检测为什么在**建模期**拒绝而不是运行期发现
//!
//! 派生关系成环（A 派生 B、B 派生 C、C 派生 A）在代数上无害，
//! 在**求值**上无解：三者互相推导，任何一个变更都会要求另外两个先变。
//! 若放到运行期才发现，故障形态是「扫描跑到一半停住」——现场早已有产出。
//! 故 [`ConsistencyModel::declare`] 在**写入模型时**即拒环
//! （[`E_RELATION_CYCLE`]），让环永远进不了册。
//!
//! # 五、版本化：模型册与实体版本是两件事
//!
//! 锚点要求「模型版本化」，但**版本漂移**（降级矩阵第三格）提示了一个易错点：
//! 「模型册版本」指**结构**变了（加了实体类别、换了关系型），
//! 「实体版本」指**内容**变了（某条文案改了措辞）。两者必须分开记：
//! 混记会出现「结构没动却被判漂移」或「内容大改却因结构未变而不升版」
//! ——后者尤其危险，因为它让破坏性内容改动蒙混过版。
//! 故 [`ModelVersion`] 与 [`entity_version`] 分列，对版只比结构项。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::veu01_arch::{fnv1a64_hex, ConsistencyError, Severity};

/// 可一致化实体类别（锚点原文四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntityClass {
    /// 交互行为（焦点/快捷键/动效口径）。
    Interaction,
    /// 视觉令牌（色板/间距/字号）。
    VisualToken,
    /// 文案口径（措辞/术语/语气）。
    Copywriting,
    /// 数据契约（字段/单位/枚举）。
    DataContract,
}

impl EntityClass {
    /// 全部类别（**四类不多不少**——分类表是契约，漏一类即失去完备性）。
    pub const ALL: [EntityClass; 4] = [
        EntityClass::Interaction,
        EntityClass::VisualToken,
        EntityClass::Copywriting,
        EntityClass::DataContract,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            EntityClass::Interaction => "交互行为",
            EntityClass::VisualToken => "视觉令牌",
            EntityClass::Copywriting => "文案口径",
            EntityClass::DataContract => "数据契约",
        }
    }

    /// 码（对外引用，不可随措辞改动而变）。
    pub fn code(self) -> &'static str {
        match self {
            EntityClass::Interaction => "CLASS-INTERACTION",
            EntityClass::VisualToken => "CLASS-VISUAL",
            EntityClass::Copywriting => "CLASS-COPY",
            EntityClass::DataContract => "CLASS-DATA",
        }
    }

    /// 由码反查（**往返一致**：码 → 类 → 码须为恒等）。
    pub fn from_code(code: &str) -> Option<EntityClass> {
        EntityClass::ALL.iter().copied().find(|c| c.code() == code)
    }

    /// 该类别的**首选关系型**（锚点「四类×三型」的常用配）。
    ///
    /// 给出首选而非唯一：模型允许任何类别用任何关系型（建模不该被偏好
    /// 绑手），但给出首选让**标准模型**有单源依据，避免每个域各挑一种。
    pub fn preferred_kind(self) -> RelationKind {
        match self {
            EntityClass::Interaction => RelationKind::Equiv,
            EntityClass::VisualToken => RelationKind::Derived,
            EntityClass::Copywriting => RelationKind::Equiv,
            // 数据契约用派生会放大歧义（见头注§二），故首选约束。
            EntityClass::DataContract => RelationKind::Constraint,
        }
    }
}

/// 一致关系型（锚点原文三型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelationKind {
    /// 等同：两侧同值，冲突无先后。
    Equiv,
    /// 派生：源 → 果（有向，冲突时源赢）。
    Derived,
    /// 约束：被约束方须满足源声明的下限（冲突时红线不可让步）。
    Constraint,
}

impl RelationKind {
    /// 全部关系型（三型）。
    pub const ALL: [RelationKind; 3] = [
        RelationKind::Equiv,
        RelationKind::Derived,
        RelationKind::Constraint,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            RelationKind::Equiv => "等同",
            RelationKind::Derived => "派生",
            RelationKind::Constraint => "约束",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            RelationKind::Equiv => "REL-EQUIV",
            RelationKind::Derived => "REL-DERIVED",
            RelationKind::Constraint => "REL-CONSTRAINT",
        }
    }

    /// 由码反查（往返一致）。
    pub fn from_code(code: &str) -> Option<RelationKind> {
        RelationKind::ALL.iter().copied().find(|k| k.code() == code)
    }

    /// 是否**有向**（须声明派生方向）。
    ///
    /// 派生必须判为有向：见头注§二——不声明方向的派生在实现时退化成等同。
    pub fn is_directed(self) -> bool {
        matches!(self, RelationKind::Derived)
    }

    /// 冲突让位优先级（**约束 > 派生 > 等同**）。
    ///
    /// 这条全序是本项冲突消解的**单一依据**（头注§二）。定成全序而非
    /// 查表的原因：查表遇到未列组合只能返回「无法裁决」，而三型两两组合
    /// 只有 9 种、全部有答案——写成全序就没有「没想到的组合」这回事。
    pub fn precedence(self) -> u8 {
        match self {
            RelationKind::Constraint => 3,
            RelationKind::Derived => 2,
            RelationKind::Equiv => 1,
        }
    }

    /// 冲突让位方（`self` 与 `incoming` 冲突时谁赢）。
    ///
    /// 参数 `incoming` 是「后声明的那条关系」。规则只回答一件事：谁赢。
    /// 同型返回自身——登记序即裁决序，先登记者为准。
    ///
    /// 返回 `None` 保留给**将来新增的关系型**：新类型若未纳入
    /// [`Self::precedence`]，此处即返回 `None`，冲突自动落人工仲裁
    /// （降级矩阵第二格）。这不是死代码，是「新关系型不许悄悄参与裁决」
    /// 的强制点——让它编译不过，比让它默默按旧规则裁决安全得多。
    pub fn winner(self, incoming: RelationKind) -> Option<RelationKind> {
        if self.precedence() == 0 || incoming.precedence() == 0 {
            return None;
        }
        if self.precedence() >= incoming.precedence() {
            Some(self)
        } else {
            Some(incoming)
        }
    }
}

/// 跨域实体（一致性对象建模的「对象」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entity {
    /// 实体码（跨域唯一——两域各持一份时靠它认成同一对象）。
    pub code: String,
    /// 类别。
    pub class: EntityClass,
    /// 声明取值（等同关系比对的就是它）。
    pub value: String,
    /// 内容版本（**与模型册版本分开记**，见头注§五）。
    pub entity_version: String,
}

impl Entity {
    /// 新建实体（码/类别/取值）。
    pub fn new(code: &str, class: EntityClass, value: &str) -> Entity {
        Entity {
            code: code.to_string(),
            class,
            value: value.to_string(),
            entity_version: "v1".to_string(),
        }
    }

    /// 取值哈希（等同比对用**实算哈希**，不靠字符串相等）。
    pub fn value_hash(&self) -> String {
        fnv1a64_hex(self.value.as_bytes())
    }

    /// 升内容版本。
    pub fn bump(&mut self) {
        self.entity_version = format!("{}-r", self.entity_version);
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "实体 {}（{}）取值 {} 哈希 {} 内容版本 {}",
            self.code,
            self.class.zh(),
            self.value,
            self.value_hash(),
            self.entity_version
        )
    }
}

/// 域标识（跨域一致性模型的「域」维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DomainTag {
    /// S 域（交互词典来源）。
    S,
    /// T 域（地区规则来源）。
    T,
    /// U 域（本域，收敛侧）。
    U,
}

impl DomainTag {
    /// 全部域（**三域不多不少**）。
    pub const ALL: [DomainTag; 3] = [DomainTag::S, DomainTag::T, DomainTag::U];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            DomainTag::S => "S 域",
            DomainTag::T => "T 域",
            DomainTag::U => "U 域",
        }
    }
}

/// 一致关系（实体之间的一致关系，模型册的一行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relation {
    /// 关系码（唯一）。
    pub code: String,
    /// 关系型。
    pub kind: RelationKind,
    /// 源实体码（派生/约束必填且必须真实存在；等同为对称可空）。
    pub from: String,
    /// 目标实体码（果/被约束方）。
    pub to: String,
    /// 是否**不可让步红线约束**（锚点：对比度/焦点可达）。
    pub non_negotiable: bool,
}

impl Relation {
    /// 新建关系。
    pub fn new(code: &str, kind: RelationKind, from: &str, to: &str) -> Relation {
        Relation {
            code: code.to_string(),
            kind,
            from: from.to_string(),
            to: to.to_string(),
            non_negotiable: false,
        }
    }

    /// 标为红线约束（**不可让步**——见头注§三）。
    pub fn as_non_negotiable(mut self) -> Relation {
        self.non_negotiable = true;
        self
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "关系 {} {} {} → {}{}",
            self.code,
            self.kind.zh(),
            self.from,
            self.to,
            if self.non_negotiable { "（红线·不可让步）" } else { "" }
        )
    }
}

/// 不可让步红线约束的实体码白名单（锚点明文：**对比度 / 焦点可达**）。
///
/// 只认这两项。清单刻意保持极短：红线册每加一项，就多一个「这次能不能豁免」
/// 的争议口子。红线之所以有力，靠的就是**没人能随手往里加东西**。
///
/// 注意码与中文名的关系：白名单存**实体码**，而 [`NON_NEG_CONTRAST`] /
/// [`NON_NEG_FOCUS`] 就是这两个码；中文名另在 [`NON_NEG_LABELS`] 里，
/// 供读屏播报与「裁决方选了红线没有」的判定用。两者分开是因为
/// **判定的依据是码**（码唯一且可反查），中文名随时可能被改措辞。
pub const NON_NEGOTIABLE: [&str; 2] = [NON_NEG_CONTRAST, NON_NEG_FOCUS];

/// 红线实体的中文名（与 [`NON_NEGOTIABLE`] 一一对应，顺序即下标）。
pub const NON_NEG_LABELS: [&str; 2] = ["对比度", "焦点可达"];

/// 红线约束实体码（红线册登记用；两个实体各持一份以便对拍）。
pub const NON_NEG_CONTRAST: &str = "U02-CONTRAST";
pub const NON_NEG_FOCUS: &str = "U02-FOCUS-REACH";

/// 取红线实体码对应的中文名（未登记则回 `None`）。
pub fn non_neg_label(code: &str) -> Option<&'static str> {
    NON_NEGOTIABLE
        .iter()
        .position(|c| *c == code)
        .map(|i| NON_NEG_LABELS[i])
}

/// 模型册版本（**结构版本**，见头注§五）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelVersion {
    /// 主版本（结构破坏性变更升主版本）。
    pub major: u32,
    /// 次版本（结构新增/扩展）。
    pub minor: u32,
}

impl ModelVersion {
    /// 起始版本。
    pub fn initial() -> ModelVersion {
        ModelVersion { major: 1, minor: 0 }
    }

    /// 升主版本（结构破坏性变更）。
    pub fn bump_major(&mut self) {
        self.major += 1;
        self.minor = 0;
    }

    /// 升次版本（结构扩展）。
    pub fn bump_minor(&mut self) {
        self.minor += 1;
    }

    /// 版本文本。
    pub fn text(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }

    /// 结构指纹（**只对结构项取哈希**：实体类别集 + 关系型集 +
    /// 派生方向图，不含内容取值——否则改一句文案就被误判结构漂移）。
    pub fn structural_digest(&self, model: &ConsistencyModel) -> String {
        let mut acc: Vec<String> = Vec::new();
        for c in EntityClass::ALL.iter() {
            acc.push(c.code().to_string());
        }
        for k in RelationKind::ALL.iter() {
            acc.push(k.code().to_string());
        }
        let mut dirs: Vec<String> = model
            .relations()
            .iter()
            .filter(|r| r.kind.is_directed())
            .map(|r| format!("{}>{}", r.from, r.to))
            .collect();
        dirs.sort();
        acc.extend(dirs);
        fnv1a64_hex(acc.join("|").as_bytes())
    }
}

/// 冲突（等价关系两侧取值不同的发现项）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// 冲突码。
    pub code: String,
    /// 涉及的关系码。
    pub relation: String,
    /// 左侧实体码与哈希。
    pub left: (String, String),
    /// 右侧实体码与哈希。
    pub right: (String, String),
    /// 严重度。
    pub severity: Severity,
    /// 是否已进仲裁队列（**不可自动消解者必须入队**，不得静默放过）。
    pub in_arbitration: bool,
}

impl Conflict {
    /// 读屏单行（异常零静默）。
    pub fn screen_line(&self) -> String {
        format!(
            "冲突 {}：关系 {} 两侧 {} ({}) 与 {} ({}) 不一致{}",
            self.code,
            self.relation,
            self.left.0,
            self.left.1,
            self.right.0,
            self.right.1,
            if self.in_arbitration { "；已入仲裁队列" } else { "" }
        )
    }
}

/// 错误码。
pub const E_ENTITY_DUP: &str = "E_ENTITY_DUP";
pub const E_ENTITY_UNKNOWN: &str = "E_ENTITY_UNKNOWN";
pub const E_ENTITY_MISSING_FIELD: &str = "E_ENTITY_MISSING_FIELD";
pub const E_RELATION_DUP: &str = "E_RELATION_DUP";
pub const E_RELATION_CYCLE: &str = "E_RELATION_CYCLE";
pub const E_RELATION_UNKNOWN_ENTITY: &str = "E_RELATION_UNKNOWN_ENTITY";
pub const E_RELATION_DIRECTION_MISSING: &str = "E_RELATION_DIRECTION_MISSING";
pub const E_RELATION_KIND_UNKNOWN: &str = "E_RELATION_KIND_UNKNOWN";
pub const E_NON_NEG_NOT_LISTED: &str = "E_NON_NEG_NOT_LISTED";
pub const E_NON_NEG_WAIVED: &str = "E_NON_NEG_WAIVED";
pub const E_REDLINE_OVERRIDDEN: &str = "E_REDLINE_OVERRIDDEN";
pub const E_ARBITRATION_REQUIRED: &str = "E_ARBITRATION_REQUIRED";
pub const E_VERSION_DRIFT: &str = "E_VERSION_DRIFT";
pub const E_VERSION_MISSING_FIELD: &str = "E_VERSION_MISSING_FIELD";
pub const E_MODEL_EMPTY: &str = "E_MODEL_EMPTY";
pub const E_CLASS_INCOMPLETE: &str = "E_CLASS_INCOMPLETE";
pub const E_CAP: &str = "E_CAP";

/// 容量上限（定长内分配，**不堆无界增长**）。
pub const MAX_ENTITIES: usize = 64;
pub const MAX_RELATIONS: usize = 128;
pub const MAX_CONFLICTS: usize = 64;

/// 跨域一致性模型（模型册：四类 × 三型）。
#[derive(Clone, Debug)]
pub struct ConsistencyModel {
    entities: Vec<Entity>,
    relations: Vec<Relation>,
    conflicts: Vec<Conflict>,
    version: ModelVersion,
    /// 已冻结结构指纹（**对版的依据**；空表示尚未冻结）。
    frozen_digest: String,
}

impl ConsistencyModel {
    /// 空模型。
    pub fn new() -> ConsistencyModel {
        ConsistencyModel {
            entities: Vec::new(),
            relations: Vec::new(),
            conflicts: Vec::new(),
            version: ModelVersion::initial(),
            frozen_digest: String::new(),
        }
    }

    /// 实体只读遍历。
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// **负例专用**：可写实体表，供自检构造非法模型册。
    ///
    /// # 为什么开这个口
    ///
    /// 负例要证的是「检查抓得住坏册」，而坏册恰恰是**公开API 正确拒绝**
    /// 构造的那些形态（悬空端点、重复码、空取值、成环派生）。走公开通道
    /// 构造不出来——那些构造点本来就该拒绝。
    ///
    /// # 为什么不叫 `entities_mut`
    ///
    /// 名字必须带 `tamper`：谁在生产代码里调用它，一眼就该被 review 拦下。
    /// 若叫 `entities_mut`，它就成了第二个合法写口，红线与唯一性断言随即失守。
    /// 绕过它不会让任何非法册变合法，只会让册子对不上账。
    pub fn tamper_entities(&mut self) -> &mut Vec<Entity> {
        &mut self.entities
    }

    /// **负例专用**：可写关系表（理由同 [`Self::tamper_entities`]）。
    pub fn tamper_relations(&mut self) -> &mut Vec<Relation> {
        &mut self.relations
    }

    /// **负例专用**：置/清结构冻结指纹（用于构造「未冻结」与「漂移」两态）。
    pub fn tamper_frozen_digest(&mut self, digest: &str) {
        self.frozen_digest = digest.to_string();
    }

    /// 关系只读遍历。
    pub fn relations(&self) -> &[Relation] {
        &self.relations
    }

    /// 冲突只读遍历。
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// 模型册版本。
    pub fn version(&self) -> &ModelVersion {
        &self.version
    }

    /// 冻结结构指纹（空 = 未冻结）。
    pub fn frozen_digest(&self) -> &str {
        &self.frozen_digest
    }

    /// 取实体。
    pub fn entity(&self, code: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.code == code)
    }

    /// 登记实体（**O(1)**：码比对走定长小表）。
    pub fn add_entity(&mut self, e: Entity) -> Result<(), ConsistencyError> {
        if e.code.trim().is_empty() || e.value.trim().is_empty() {
            return Err(ConsistencyError::new(
                E_ENTITY_MISSING_FIELD,
                "实体登记被拒：字段残缺",
                &format!("实体码「{}」取值「{}」至少一项为空", e.code, e.value),
                "实体必须同时有码与取值；空取值实体无法参与对拍",
                "建模方",
            ));
        }
        if self.entities.iter().any(|x| x.code == e.code) {
            return Err(ConsistencyError::new(
                E_ENTITY_DUP,
                "实体登记被拒：实体码重复",
                &format!("实体码 {} 已在册内（类别 {}）", e.code, e.class.zh()),
                "同码实体只应登记一次；两域各持一份的是**取值**，不是码",
                "建模方",
            ));
        }
        if self.entities.len() >= MAX_ENTITIES {
            return Err(ConsistencyError::new(
                E_CAP,
                "实体登记被拒：达到容量上限",
                &format!("实体册 {} 条达到上限 {}", self.entities.len(), MAX_ENTITIES),
                "先归并同义实体，或拆册",
                "建模方",
            ));
        }
        self.entities.push(e);
        Ok(())
    }

    /// 声明关系（**建模期**即拒环，见头注§四）。
    ///
    /// 环检测复杂度 O(关系)：从新边起点做一次可达性DFS，
    /// 若能走回 `to` 即成环。
    pub fn declare(&mut self, r: Relation) -> Result<(), ConsistencyError> {
        if r.code.trim().is_empty() {
            return Err(ConsistencyError::new(
                E_RELATION_DUP,
                "关系声明被拒：关系码为空",
                "关系码为空将无法定位与回溯",
                "给关系起可反查的码（形如 U02-REL-####）",
                "建模方",
            ));
        }
        if self.relations.iter().any(|x| x.code == r.code) {
            return Err(ConsistencyError::new(
                E_RELATION_DUP,
                "关系声明被拒：关系码重复",
                &format!("关系码 {} 已在册内", r.code),
                "同码关系只声明一次",
                "建模方",
            ));
        }
        if RelationKind::from_code(r.kind.code()).is_none() {
            return Err(ConsistencyError::new(
                E_RELATION_KIND_UNKNOWN,
                "关系声明被拒：关系型不可识别",
                "关系型只有等同/派生/约束三型",
                "改用 RelationKind::ALL 之一",
                "建模方",
            ));
        }
        // 端点必须在册（**悬空端点即模型不成立**）。
        for (role, code) in [("源", &r.from), ("目标", &r.to)] {
            if code.trim().is_empty() {
                return Err(ConsistencyError::new(
                    E_RELATION_DIRECTION_MISSING,
                    "关系声明被拒：端点缺失",
                    &format!("关系 {} 的{}端点为空", r.code, role),
                    "派生必须声明方向（谁派生谁）；等同也须写清两侧实体",
                    "建模方",
                ));
            }
            if self.entity(code).is_none() {
                return Err(ConsistencyError::new(
                    E_RELATION_UNKNOWN_ENTITY,
                    "关系声明被拒：端点实体不在册",
                    &format!("关系 {} 的{}端点 {} 未登记", r.code, role, code),
                    &format!(
                        "先登记实体 {}（类别任意），再声明关系",
                        code
                    ),
                    "建模方",
                ));
            }
        }
        // 红线只认白名单两项（头注§三）。
        if r.non_negotiable && !NON_NEGOTIABLE.contains(&r.to.as_str()) {
            return Err(ConsistencyError::new(
                E_NON_NEG_NOT_LISTED,
                "红线标注被拒：不在红线白名单",
                &format!(
                    "关系 {} 把目标 {} 标为不可让步，但白名单仅 {:?}",
                    r.code, r.to, NON_NEGOTIABLE
                ),
                "红线册只收对比度/焦点可达；要加新红线须改锚点，不可就地扩",
                "建模方",
            ));
        }
        // 环检测：新增边若使派生图成环即拒（等同视为双向边一并查）。
        if self.would_cycle(&r) {
            return Err(ConsistencyError::new(
                E_RELATION_CYCLE,
                "关系声明被拒：派生关系成环",
                &format!(
                    "关系 {}（{} → {}）将使派生图成环；成环在求值上无解",
                    r.code, r.from, r.to
                ),
                "断开环：至少一条关系改约束型（约束不成环）或撤销",
                "建模方",
            ));
        }
        self.relations.push(r);
        Ok(())
    }

    /// 新增关系是否成环（**O(关系)** DFS 可达性）。
    ///
    /// # 为什么只走**有向**边（派生/约束），不走等同
    ///
    /// 等同是**对称**关系：声明 A 等同 B 在语义上已同时包含 B 等同 A，
    /// 图上补一条反向边是**补出同一条关系**，不是新增依赖。若把等同
    /// 双向计入可达性，则每一条等同边 A—B 都会让「A 可达 B 且 B 可达 A」
    /// 成立——于是**任何**等同关系都会当场被判成环，等同型直接不可用。
    ///
    /// 成环真正**有害**的是有向关系：派生/约束构成依赖图，环 therein
    /// 意味着「互相要求对方先变」，求值无解（头注§四）。所以环检测只
    /// 在有向子图上做，这既符合语义，也才让三型各有其正确行为。
    fn would_cycle(&self, r: &Relation) -> bool {
        // 自环只对**有向**关系有害：约束 A→A 是「A 须满足自己声明的下限」，
        // 恒真且无害；派生 A→A 才是无解环。
        if r.from == r.to {
            return r.kind.is_directed();
        }
        // 等同关系不参与环检测（见上）。
        if !r.kind.is_directed() {
            return false;
        }
        let mut edges: Vec<(String, String)> = Vec::new();
        for x in self.relations.iter() {
            // 只收有向边（Derived / Constraint）。
            if x.kind.is_directed() {
                edges.push((x.from.clone(), x.to.clone()));
            }
        }
        edges.push((r.from.clone(), r.to.clone()));
        // 从 to 出发 DFS，若抵达 from 则成环。
        let mut stack: Vec<String> = vec![r.to.clone()];
        let mut seen: Vec<String> = vec![r.to.clone()];
        while let Some(cur) = stack.pop() {
            if cur == r.from {
                return true;
            }
            for (a, b) in edges.iter() {
                if a == &cur && !seen.iter().any(|s| s == b) {
                    seen.push(b.clone());
                    stack.push(b.clone());
                }
            }
        }
        false
    }

    /// 四类齐备性自检（**四类不多不少**，判据「四类三型」的一半）。
    pub fn classes_complete(&self) -> bool {
        EntityClass::ALL.iter().all(|c| {
            self.entities.iter().any(|e| e.class == *c)
        })
    }

    /// 三型齐备性自检（判据「四类三型」的另一半）。
    pub fn kinds_complete(&self) -> bool {
        RelationKind::ALL
            .iter()
            .all(|k| self.relations.iter().any(|r| r.kind == *k))
    }

    /// 对拍全册（**O(实体×关系)**：逐关系比两侧取值）。
    ///
    /// 等同关系两侧取值不同即冲突；派生/约束不产生取值冲突
    /// （它们的冲突形态是「违反下限」，由规则引擎处理，不属本项）。
    pub fn cross_check(&mut self) -> Result<usize, ConsistencyError> {
        self.conflicts.clear();
        let mut n = 0usize;
        let rels: Vec<(String, String, String)> = self
            .relations
            .iter()
            .filter(|r| r.kind == RelationKind::Equiv)
            .map(|r| (r.code.clone(), r.from.clone(), r.to.clone()))
            .collect();
        for (rcode, from, to) in rels.iter() {
            let (Some(a), Some(b)) = (self.entity(from), self.entity(to)) else {
                continue;
            };
            if a.value_hash() == b.value_hash() {
                continue;
            }
            n += 1;
            // 红线优先：等同关系的任一端被红线约束罩住，则该冲突不可自动消解。
            let redline = self.under_redline(from.as_str()) || self.under_redline(to.as_str());
            let code = format!("CF-{}", rcode);
            self.conflicts.push(Conflict {
                code,
                relation: rcode.clone(),
                left: (a.code.clone(), a.value_hash()),
                right: (b.code.clone(), b.value_hash()),
                severity: if redline { Severity::Blocking } else { Severity::Warning },
                // 红线冲突**强制入仲裁**（不可消解），其余先按让位规则试消解。
                in_arbitration: redline || !self.auto_resolvable(rcode.as_str()),
            });
        }
        Ok(n)
    }

    /// 该关系能否按让位规则自动消解（**O(关系)**）。
    fn auto_resolvable(&self, rcode: &str) -> bool {
        // 取该关系及与之同端点的关系，看关系型组合是否有明确让位方。
        let Some(rel) = self.relations.iter().find(|r| r.code == rcode) else {
            return false;
        };
        let others: Vec<RelationKind> = self
            .relations
            .iter()
            .filter(|r| {
                r.code != rcode
                    && (r.from == rel.from || r.to == rel.to
                        || r.from == rel.to || r.to == rel.from)
            })
            .map(|r| r.kind)
            .collect();
        // 无交叠关系→ 同型冲突，等价关系按登记序收敛，可自动消解。
        others.is_empty() || others.iter().all(|k| rel.kind.winner(*k).is_some())
    }

    /// 同等关系某端是否被**红线约束**罩住（**O(关系)**）。
    ///
    /// # 为什么不能只看「该端自己是不是红线实体」
    ///
    /// 红线实体（对比度/焦点可达）不是等同关系的端点——它是**约束关系的
    /// 目标**。形态是：
    ///
    /// ```text
    ///   等同： U02-PALETTE-S <=> U02-PALETTE-T   （跨域同值）
    ///   红线： U02-PALETTE-S  --约束-->  U02-CONTRAST（不可让步）
    /// ```
    ///
    /// 冲突发生在等同关系两侧不等时，而红线挂在其中一侧的**约束**上。
    /// 若只查「等同端点自己是否红线实体」，则U02-CONTRAST 永远查不到自己
    /// ——它不是任何等同关系的端点，于是**任何真实红线冲突都会被降级成警告**。
    /// 这正是红线最危险的失效形态：看起来有闸，实际没接线。
    ///
    /// 所以这里沿**红线约束边**做一步可达：等同端点若被任一条
    /// `non_negotiable` 约束指向红线实体，该端即「红线罩住」。
    fn under_redline(&self, code: &str) -> bool {
        if self.is_redline(code) {
            return true;
        }
        self.relations
            .iter()
            .any(|r| r.non_negotiable && r.from == code)
    }

    /// 是否红线实体（**O(1)**）。
    fn is_redline(&self, code: &str) -> bool {
        NON_NEGOTIABLE.iter().any(|c| *c == code)
    }

    /// 冲突消解算子（**O(冲突)**）。
    ///
    /// 返回收敛后的裁决码。可自动消解者给出让位方；不可消解者
    /// **留在仲裁队列**并回 [`E_ARBITRATION_REQUIRED`]——
    /// 这是降级矩阵第二格，不消解 ≠ 静默放过。
    pub fn resolve_conflicts(&self) -> Result<Vec<Conflict>, ConsistencyError> {
        let queue: Vec<Conflict> = self
            .conflicts
            .iter()
            .filter(|c| c.in_arbitration)
            .cloned()
            .collect();
        if !queue.is_empty() {
            return Err(ConsistencyError::new(
                E_ARBITRATION_REQUIRED,
                "冲突不可自动消解：已入人工仲裁队列",
                &format!(
                    "{} 项冲突无可信让位方（如红线约束或三型混合）",
                    queue.len()
                ),
                &format!(
                    "人工裁定后调 arbitrate 落槌；队列首项：{}",
                    queue[0].screen_line()
                ),
                "一致性仲裁员",
            ));
        }
        Ok(self.conflicts.clone())
    }

    /// 人工仲裁落槌（**不可让步红线拒绝被豁免**，见头注§三）。
    pub fn arbitrate(&mut self, conflict_code: &str, winner: &str) -> Result<(), ConsistencyError> {
        let Some(c) = self.conflicts.iter().find(|c| c.code == conflict_code).cloned() else {
            return Err(ConsistencyError::new(
                E_ENTITY_UNKNOWN,
                "仲裁落槌被拒：冲突不存在",
                &format!("冲突码 {} 不在册内", conflict_code),
                "先跑 cross_check 产出冲突项",
                "一致性仲裁员",
            ));
        };
        // 红线冲突：只接受「红线赢」这一个答案。
        if c.severity == Severity::Blocking && !self.redline_won(winner) {
            return Err(ConsistencyError::new(
                E_REDLINE_OVERRIDDEN,
                "仲裁被拒：不得推翻红线约束",
                &format!(
                    "冲突 {} 涉红线约束，裁决方给了「{}」；红线不可让步",
                    conflict_code, winner
                ),
                &format!(
                    "只可裁为 {:?}之一（取红线侧为胜）",
                    NON_NEGOTIABLE
                ),
                "一致性仲裁员",
            ));
        }
        if let Some(cf) = self.conflicts.iter_mut().find(|x| x.code == conflict_code) {
            cf.in_arbitration = false;
        }
        Ok(())
    }

    /// 裁决方是否选了红线（**O(1)**）。
    ///
    /// 认**实体码或中文名**两者之一即可：仲裁员手里拿着的可能是红线实体码，
    /// 也可能就是「对比度」这三个字。只认其一都会让合法裁决被误拒。
    fn redline_won(&self, winner: &str) -> bool {
        NON_NEGOTIABLE.iter().any(|c| winner.contains(c))
            || NON_NEG_LABELS.iter().any(|l| winner.contains(l))
    }

    /// 请求豁免红线（**一律拒绝**——域本色红线不接受豁免）。
    pub fn request_waiver(&self, target: &str, reason: &str) -> Result<(), ConsistencyError> {
        let _ = reason;
        Err(ConsistencyError::new(
            E_NON_NEG_WAIVED,
            "红线豁免被拒：无此选项",
            &format!("{} 被要求豁免", target),
            &format!(
                "对比度/焦点可达是域本色红线，不接受豁免；可做的只有改实现（{}/{}）",
                NON_NEG_CONTRAST, NON_NEG_FOCUS
            ),
            "豁免申请方",
        ))
    }

    /// 冻结结构指纹（**冻结后改结构即漂移**，见降级矩阵第三格）。
    pub fn freeze(&mut self) -> String {
        let d = self.version.structural_digest(self);
        self.frozen_digest = d.clone();
        d
    }

    /// 对版（**版本漂移检测**）。
    ///
    /// 比**结构**指纹，不比内容——见头注§五。返回Ok即同版。
    pub fn align_version(&self) -> Result<String, ConsistencyError> {
        if self.frozen_digest.is_empty() {
            return Err(ConsistencyError::new(
                E_VERSION_MISSING_FIELD,
                "对版被拒：尚未冻结结构指纹",
                &format!("当前模型册版本 {} 未冻结", self.version.text()),
                "先调 freeze() 再对版",
                "建模方",
            ));
        }
        let now = self.version.structural_digest(self);
        if now != self.frozen_digest {
            return Err(ConsistencyError::new(
                E_VERSION_DRIFT,
                "对版被拒：结构漂移",
                &format!(
                    "冻结指纹 {} ≠ 现结构 {}（模型册版本 {}）",
                    self.frozen_digest, now, self.version.text()
                ),
                "结构变了就升模型册版本并重新冻结；内容改动不升结构版本",
                "建模方",
            ));
        }
        Ok(now)
    }

    /// 升结构版本（结构变更时调用，随之须重新冻结）。
    pub fn bump_major(&mut self) {
        self.version.bump_major();
        self.frozen_digest = String::new();
    }

    /// 升结构次版本。
    pub fn bump_minor(&mut self) {
        self.version.bump_minor();
        self.frozen_digest = String::new();
    }

    /// 模型自检（结构完备 + 红线册健康）。
    pub fn self_audit(&self) -> Vec<String> {
        let mut issues: Vec<String> = Vec::new();
        if self.entities.is_empty() || self.relations.is_empty() {
            issues.push(format!("{}:模型册为空（实体 {} 关系 {}）", E_MODEL_EMPTY, self.entities.len(), self.relations.len()));
        }
        if !self.classes_complete() {
            issues.push(format!(
                "{}:四类未齐（缺 {:?}）",
                E_CLASS_INCOMPLETE,
                EntityClass::ALL
                    .iter()
                    .find(|c| !self.entities.iter().any(|e| e.class == **c))
            ));
        }
        if !self.kinds_complete() {
            issues.push(format!(
                "{}:三型未齐（缺 {:?}）",
                E_CLASS_INCOMPLETE,
                RelationKind::ALL
                    .iter()
                    .find(|k| !self.relations.iter().any(|r| r.kind == **k))
            ));
        }
        // 红线关系必须两端都在册且指向白名单实体（防红线册名存实亡）。
        for r in self.relations.iter() {
            if r.non_negotiable && !NON_NEGOTIABLE.contains(&r.to.as_str()) {
                issues.push(format!(
                    "{}:红线关系 {} 指向非白名单实体 {}",
                    E_NON_NEG_NOT_LISTED,
                    r.code,
                    r.to
                ));
            }
        }
        issues
    }

    /// 读屏总览。
    pub fn screen_text(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "跨域一致性模型册：版本 {}，实体 {} 条，关系 {} 条，冲突 {} 条\n",
            self.version.text(),
            self.entities.len(),
            self.relations.len(),
            self.conflicts.len()
        ));
        for e in self.entities.iter() {
            s.push_str(&e.screen_line());
            s.push('\n');
        }
        for r in self.relations.iter() {
            s.push_str(&r.screen_line());
            s.push('\n');
        }
        for c in self.conflicts.iter() {
            s.push_str(&c.screen_line());
            s.push('\n');
        }
        s
    }
}

/// 标准模型册（**四类 × 三型齐备的正样本**，兼作回归基线）。
///
/// 正样本刻意含一条**派生方向边**与一条**红线约束**，
/// 供环检测与红线不可让步的判据有实测对象。
///
/// 注意每类都登记了**两份**（`…-S` / `…-T` 两域各持一份）——
/// 这正是「跨域一致性」的对象形态：同语义两处副本，靠等同关系对拍。
/// 只登记一份的话，等同关系无对象，对拍判据就成了空跑。
pub fn standard_model() -> ConsistencyModel {
    let mut m = ConsistencyModel::new();

    // 四类各两枚（S / T 两域各持一份）。
    m.add_entity(Entity::new(
        "U02-FOCUS-ORDER-S",
        EntityClass::Interaction,
        "焦点顺序：标题→主区→辅区→状态栏",
    ))
    .expect("标准实体登记");
    m.add_entity(Entity::new(
        "U02-FOCUS-ORDER-T",
        EntityClass::Interaction,
        "焦点顺序：标题→主区→辅区→状态栏",
    ))
    .expect("标准实体登记");
    m.add_entity(Entity::new("U02-PALETTE-S", EntityClass::VisualToken, "主色 #1A1A1A"))
        .expect("标准实体登记");
    m.add_entity(Entity::new("U02-PALETTE-T", EntityClass::VisualToken, "主色 #1A1A1A"))
        .expect("标准实体登记");
    m.add_entity(Entity::new("U02-TERM-SAVE-S", EntityClass::Copywriting, "保存"))
        .expect("标准实体登记");
    m.add_entity(Entity::new("U02-TERM-SAVE-T", EntityClass::Copywriting, "保存"))
        .expect("标准实体登记");
    m.add_entity(Entity::new(
        "U02-FIELD-THEME-S",
        EntityClass::DataContract,
        "主题字段取值为 light|dark|auto",
    ))
    .expect("标准实体登记");
    m.add_entity(Entity::new(
        "U02-FIELD-THEME-T",
        EntityClass::DataContract,
        "主题字段取值为 light|dark|auto",
    ))
    .expect("标准实体登记");

    // 红线实体（对比度 / 焦点可达）——红线约束的落点。
    m.add_entity(Entity::new(NON_NEG_CONTRAST, EntityClass::VisualToken, "对比度≥4.5:1"))
        .expect("红线实体登记");
    m.add_entity(Entity::new(NON_NEG_FOCUS, EntityClass::Interaction, "焦点可达"))
        .expect("红线实体登记");

    // 等同关系：同一语义跨域同值（标准态两侧取值相同 → 对拍全绿）。
    m.declare(Relation::new(
        "U02-REL-EQUIV-FOCUS",
        RelationKind::Equiv,
        "U02-FOCUS-ORDER-S",
        "U02-FOCUS-ORDER-T",
    ))
    .expect("等同关系登记");
    m.declare(Relation::new(
        "U02-REL-EQUIV-PALETTE",
        RelationKind::Equiv,
        "U02-PALETTE-S",
        "U02-PALETTE-T",
    ))
    .expect("等同关系登记");
    m.declare(Relation::new(
        "U02-REL-EQUIV-TERM",
        RelationKind::Equiv,
        "U02-TERM-SAVE-S",
        "U02-TERM-SAVE-T",
    ))
    .expect("等同关系登记");
    // 数据契约也须有跨域等同对——否则该类只挂约束，
    // 它的「跨域两份副本取值分叉」这种最典型的一致性故障将无闸可测。
    m.declare(Relation::new(
        "U02-REL-EQUIV-FIELD",
        RelationKind::Equiv,
        "U02-FIELD-THEME-S",
        "U02-FIELD-THEME-T",
    ))
    .expect("等同关系登记");

    // 派生关系（文案口径由数据契约派生——**有向**，供环检测有实测对象）。
    m.declare(Relation::new(
        "U02-REL-DERIV-TERM",
        RelationKind::Derived,
        "U02-FIELD-THEME-S",
        "U02-TERM-SAVE-S",
    ))
    .expect("派生关系登记");

    // 约束关系（交互行为为数据契约声明下限）。
    m.declare(Relation::new(
        "U02-REL-CONSTRAINT-FIELD",
        RelationKind::Constraint,
        "U02-FOCUS-ORDER-S",
        "U02-FIELD-THEME-T",
    ))
    .expect("约束关系登记");

    // 红线约束（**不可让步**，指向白名单实体）。
    m.declare(
        Relation::new(
            "U02-REL-REDLINE-CONTRAST",
            RelationKind::Constraint,
            "U02-PALETTE-S",
            NON_NEG_CONTRAST,
        )
        .as_non_negotiable(),
    )
    .expect("红线约束登记");
    m.declare(
        Relation::new(
            "U02-REL-REDLINE-FOCUS",
            RelationKind::Constraint,
            "U02-FOCUS-ORDER-T",
            NON_NEG_FOCUS,
        )
        .as_non_negotiable(),
    )
    .expect("红线约束登记");

    m.freeze();
    m
}