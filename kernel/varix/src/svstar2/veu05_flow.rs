//! VE-F4205 · 契约变更流程引擎（VE-U 域 · 一致性域 · U01 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4205`
//!
//! **判据（锚点原文）**：六步留痕、兼容闸、灰度、会签、弱化即破坏、判据。
//!
//! **职责定位（锚点原文）**：契约变更流程引擎（变更六步流（申请→影响分析→
//! 相关方会签→灰度→切换→通告——步步留痕）；向后兼容性检查闸（破坏性变更
//! 必须升主版本+迁移指南）；回滚协议）。
//!
//! **数据结构（锚点原文·家族格式）**：流程册（六步）；会签表；兼容闸；回滚协议。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：越步→作废+回退；会签缺→挂起+催办；
//! 破坏未升版→阻断；回滚失败→立案。
//!
//! **性能逐项分解（锚点原文·家族格式）**：走步 O(变更)；分析 O(影响面)；会签 O(方)。
//!
//! **跨批对接点（锚点原文·家族格式）**：F4203 注册单源；F4226 影响分析联动；
//! 家族变更纪律同构。
//!
//! **无障碍与隐私（锚点原文）**：变更评估含无障碍判据影响专列（弱化=破坏性
//! 变更——域本色红线）；无隐私面。
//!
//! # 一、「步步留痕」的反面写法是「只在成功时留痕」
//!
//! 锚点对六步流的核心要求是「**步步留痕**」。最省事的实现是：每步成功才往
//! [`FlowTrace`] 追一条，失败就不留。那样做出来的痕迹表**只含成功路径**，
//! 于是「越步 → 作废+回退」这类**被拒绝的动作在痕迹里彻底消失**——
//! 而痕迹表的用途恰恰是回答「这个契约是怎么变成今天这样的」。
//!
//! 更糟的是它让越权动作**查不出来**：有人把「切换」直接推到「通告」，
//! 正常路径下第 4、5 步各留一条，看起来流程齐全，只是中间少了两步；
//! 而**只有留痕表自己知道第 4 步是被跳过的**，因为拒绝动作也在表里。
//!
//! 故本版的痕是**动作级**的：[`FlowTrace`] 记的是「谁在第几步做了什么、
//! 引擎怎么判的」，**被拒绝的尝试照样留痕**，outcome 取 [`TraceOutcome::Rejected`]
//! 或 [`TraceOutcome::Voided`]。痕迹表的读法因此变成：
//! **成功的路是筛出来的，不是「表里有的就是成功的」**。
//!
//! # 二、越步必须「作废 + 回退」两件事都做，缺一即等于没拦
//!
//! 降级矩阵第一格写的是「越步→作废+回退」。这三件事容易被写成一件：
//!
//! - 只**回退**（把游标退回去）：流程卡在原处，痕迹里看不出有人越权试过，
//!   而下一个读痕迹的人会以为「没人动过」——越权动作被擦除。
//! - 只**作废**（把整张申请判死）：回退没了，于是「从第 1 步重来」这条
//!   正路被一并封死——**驳回一次就永久作废**，申请人无法修正后重提。
//!
//! 正确形态：越步 → 本次变更走出的那几步**痕迹作废**（标 [`TraceOutcome::Voided`]，
//! 但**不删行**：删了就查不到发生过越权）+ 游标**退回该步之前**。
//! 关键在于作废是**标记**而非删除——留痕表的职责是记账，不是维持整洁。
//!
//! 故 [`TraceOutcome::Voided`] 与被拒绝的痕迹**都留在表里**，并在
//! [`ChangeFlow::voided_from`] 记下作废起点供回退重算。
//!
//! # 三、兼容闸的核心：破坏性**必须**升主版本，且**必须**附迁移指南
//!
//! 锚点：「破坏性变更必须升主版本+迁移指南」。这两个「必须」是**并列**的
//! AND 关系，实现时最常见的翻车是**只挡一个**：升了主版本就放行、
//! 或给了迁移指南就放行。而这两种放行都会导致下游拿到一份
//! 「版本号变大了但没人告诉他该怎么改」的契约——破坏已经发生，
//! 而消费方**无从下手**，这比阻断严重得多。
//!
//! 故 [`CompatGate`] 把结论拆成**两个独立布尔**：
//! [`CompatVerdict::major_bumped`] 与 [`CompatVerdict::migration_ok`]，
//! 放行条件是 `!breaking || (major_bumped && migration_ok)`。
//! 拆开还有一个好处：判据能分别钉「升了版但没写指南」和
//! 「写了指南但没升版」两种半吊子形态，而笼统一个 `breaking` 布尔
//! 看不见它们。
//!
//! # 四、弱化即破坏：无障碍红线**不许**被「这只是文案调整」绕过
//!
//! 锚点：「弱化=破坏性变更——域本色红线」。这句话要落在**类型与数据**上，
//! 落在注释上等于没写。
//!
//! 判据族里最容易出现的翻车是：把无障碍影响做成一个
//! `a11y_impact: bool` 让人自己填，于是「弱化了无障碍判据但填了
//! `false`」这条路径畅通无阻。**自报的门等于没门。**
//!
//! 故本版把无障碍影响做成**由变更内容推导**的 [`A11yDelta`]：变更带上
//! 「判据里被删掉的语义项」（[`A11yFacet`] 列表），引擎**自己比较**新旧
//! 判据里哪些无障碍面还在、强度是否下降，得出 [`A11yDelta`] 的结论。
//! 推导不出「强化」以外的结论——弱化只能被判为**破坏**，且这个结论
//! 直接并进 [`ChangeKind::Breaking`]，于是它必然触发主版本闸。
//!
//! 换句话说：**没有「弱化但按非破坏处理」这条路径**，
//! 因为推导器不接受「弱化=false」这种输入。
//!
//! # 五、会签表：缺签是**挂起**，不是通过
//!
//! 降级矩阵第二格：「会签缺→挂起+催办」。落空子的形态是
//! 「会签不齐也往下走，最后在痕迹里记一句『会签未完成』」——
//! 那等于把「没人批」记成了「批了但有备注」，下游读痕迹的人分不出来。
//!
//! 故 [`ChangeFlow::countersign`] 产出 [`CosignVerdict::Pending`] 时，
//! 流程**停在会签步不动**（游标不回退），同时对每个未签方发一条
//! [`DunningRecord`] 催办。挂起与催办是**同一件事的两面**：
//! 挂起是「不许过」，催办是「去催」。只挂起不催办，人就永远不来；
//! 只催办不挂起，人没来流程也过了。
//!
//! 催办**重复要累加**（[`DunningRecord::notices`]），不覆盖——
//! 覆盖会让「催了三次没人理」与「催了一次」在记录上完全一样，
//! 于是最该被升级处理的那批方恰好最不显眼。
//!
//! # 六、灰度不是「一个百分比字段」
//!
//! 六步流第四步是灰度。把它实现成 `canary_pct: u8` 是最省事也最没用的
//! 形态：门槛不可表达（100% 前必须有过一次小流量观察）、推进不可审计
//! （凭 5 → 20 → 100 这种裸数字，谁都能直接跳到 100）。
//!
//! 故本版用**档位表** [`CanaryStage::ALL`]（四档：内部→1%→10%→全量），
//! 推进只能**逐档**（[`ChangeFlow::advance_canary`]），且**必须携带观测**：
//! [`CanaryObservation`] 给出该档的健康判定。**档位与观测分离**是刻意的——
//! 观测是外部事实，档位是流程状态，把两者合成一个字段就会出现
//! 「因为选了低档位所以健康」这种自证。
//!
//! 越档推进按越步处理（作废 + 回退），与「越步」共用同一条纪律，
//! 免得「灰度越档」成为绕过六步流的暗门——这是六步流最容易破的一处：
//! 正文顺序管住了「切换」不能跳，灰度档位却能跳，于是四步形同虚设。
//!
//! # 七、回滚失败要「立案」，不是「记一条日志」
//!
//! 降级矩阵第四格：「回滚失败→立案」。回滚协议的**成功**路径容易写
//! （把当前版本退回去、流程回到会签前），难的是失败路径。
//!
//! 失败有两种截然不同的形态，处理方式不能一样：
//!
//! - **目标版本在册里根本不存在**（回滚指了个没注册过的版本）：
//!   这是**申请方写错**，可修复 → [`RollbackVerdict::TargetMissing`]。
//! - **目标在册但已被废止**（[`Lifecycle::Retired`]）：
//!   不可修复——回滚到一个已废止版本等于把契约复活，
//!   违反 F4203 的「任何回退被拒」单向前进纪律 → [`RollbackVerdict::TargetRetired`]。
//!
//! 两者都立案（[`CaseRecord`]），但**立案原因必须能区分**，否则复盘时分不出
//! 「申请写错」与「流程设计有洞」。注意 [`Lifecycle`] 判据在
//! **本文件内复核**（判据侧独立重算），不调注册中心的谓词当答案——
//! 那是自证式判据：被测方说「它没废止」，判据就信。
//!
//! # 八、容量触顶不许静默丢弃（同 F4204 纪律）
//!
//! 流程册、痕迹、会签、立案都有容量上限（内核无堆）。触顶时静默丢弃
//! 最坏：痕迹被丢之后「越权发生过」退化成「没发生过」，
//! 而流程照常返回成功。故触顶一律走 [`DiagLog`]：被丢条目降级成
//! 一条最简诊断串，**可读屏**（[`DiagLog::screen_text`]），
//! 且 [`DiagLog::truncated`] 为真时流程不得声称完整。
//!
//! # 九、跨批对接：F4203 注册单源（不复制状态）
//!
//! 锚点：「F4203 注册单源」。契约的**在册状态、生命周期、版本**一律
//! 现取现用：[`ChangeFlow::open`] 与 [`ChangeFlow::rollback`] 都接
//! `&ContractRegistry`，**只读查询**，不缓存副本。
//!
//! 落空子的形态是「把契约元数据抄一份存进流程册」——看起来流程自包含，
//! 代价是上游改了流程册那份不会跟着变，于是两处对同一契约给出不同
//! 生命周期，而**没有任何一处报错**。这与 F4204 的「四路单源不复制」
//! 是同一条红线。
//!
//! # 十、本项与 F4204 的分工（别把规则引擎的活抢过来）
//!
//! F4204 是**一致性规则引擎**（三段式 + 四路单源 + 三元裁决），
//! 判「界面/文案/令牌有没有违反一致性规则」。本项是**变更流程引擎**，
//! 判「一份契约能不能改、怎么改、改完怎么退」。
//!
//! 二者的接口在「无障碍」上：F4204 判**当下**是否合规，
//! 本项判**改动之后**会不会把无障碍判据弄弱。红线同源、判定时点不同，
//! 故本项**不复制** F4204 的执行与裁决逻辑，只在 [`A11yFacet`] 上
//! 与其无障碍判据域对齐。
//!
//! 无隐私面：本项处理的是契约元数据与流程痕迹，不触碰用户数据。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::veu01_arch::{fnv1a64_hex, ConsistencyError, Severity};
use super::veu02_model::DomainTag;
use super::veu03_registry::{ContractRegistry, Lifecycle};

// ---------------------------------------------------------------------------
// 一、六步流：步位与游标
// ---------------------------------------------------------------------------

/// 变更六步流步位（锚点原文：申请→影响分析→相关方会签→灰度→切换→通告）。
///
/// 顺序即流程序，**六步不多不少**。少一步会让必经关口消失，
/// 多一步会让「六步流」这个可数契约名不副实。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FlowStep {
    /// 第一步·申请。
    Request,
    /// 第二步·影响分析。
    Impact,
    /// 第三步·相关方会签。
    Cosign,
    /// 第四步·灰度。
    Canary,
    /// 第五步·切换。
    Cutover,
    /// 第六步·通告。
    Announce,
}

impl FlowStep {
    /// 全部六步（**顺序即流程序**）。
    pub const ALL: [FlowStep; 6] = [
        FlowStep::Request,
        FlowStep::Impact,
        FlowStep::Cosign,
        FlowStep::Canary,
        FlowStep::Cutover,
        FlowStep::Announce,
    ];

    /// 中文名（锚点原文用语）。
    pub fn zh(self) -> &'static str {
        match self {
            FlowStep::Request => "申请",
            FlowStep::Impact => "影响分析",
            FlowStep::Cosign => "相关方会签",
            FlowStep::Canary => "灰度",
            FlowStep::Cutover => "切换",
            FlowStep::Announce => "通告",
        }
    }

    /// 码（`FS-*`）。
    pub fn code(self) -> &'static str {
        match self {
            FlowStep::Request => "FS-REQUEST",
            FlowStep::Impact => "FS-IMPACT",
            FlowStep::Cosign => "FS-COSIGN",
            FlowStep::Canary => "FS-CANARY",
            FlowStep::Cutover => "FS-CUTOVER",
            FlowStep::Announce => "FS-ANNOUNCE",
        }
    }

    /// 下一步（**末步返回 None**）。
    pub fn next(self) -> Option<FlowStep> {
        let i = FlowStep::ALL.iter().position(|s| *s == self)?;
        FlowStep::ALL.get(i + 1).copied()
    }

    /// 流程序位（**从 1 起**，`ALL` 长度为 6，故最大为 6）。
    pub fn ordinal(self) -> usize {
        match self {
            FlowStep::Request => 1,
            FlowStep::Impact => 2,
            FlowStep::Cosign => 3,
            FlowStep::Canary => 4,
            FlowStep::Cutover => 5,
            FlowStep::Announce => 6,
        }
    }

    /// 由序位反查（**越界即 None**：序位域恰好是 1..=6）。
    pub fn from_ordinal(n: usize) -> Option<FlowStep> {
        FlowStep::ALL.iter().copied().find(|s| s.ordinal() == n)
    }

    /// 由码反查（往返一致）。
    pub fn from_code(code: &str) -> Option<FlowStep> {
        FlowStep::ALL.iter().copied().find(|s| s.code() == code)
    }
}

// ---------------------------------------------------------------------------
// 二、变更申请：种类与内容
// ---------------------------------------------------------------------------

/// 变更种类（兼容闸的**被测对象**，由申请内容推导，不许自报）。
///
/// 刻意**不提供** `ChangeKind::Unknown` 之类的「还没判定」值：
/// 种类由 [`ChangeRequest::classify`] 从内容推导，调用方无法绕过推导
/// 直接声明「这是非破坏变更」——那正是弱化红线要堵的口子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// 向后兼容变更（新增可选字段、修措辞）。
    Additive,
    /// 破坏性变更（删字段、改语义、收窄取值）。
    Breaking,
}

impl ChangeKind {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ChangeKind::Additive => "向后兼容",
            ChangeKind::Breaking => "破坏性",
        }
    }

    /// 是否破坏性。
    pub fn is_breaking(self) -> bool {
        match self {
            ChangeKind::Additive => false,
            ChangeKind::Breaking => true,
        }
    }
}

/// 无障碍判据面（**弱化即破坏的判定单元**）。
///
/// 每一项是一个**可独立增删的语义面**。变更删掉某一项、或把某一项的
/// 强度降下来，都是弱化。选这几个面是因为它们互不重叠，且合起来
/// 覆盖「读屏能不能读、能不能看清、能不能摸到」。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum A11yFacet {
    /// 对比度门槛。
    Contrast,
    /// 焦点可达。
    FocusReach,
    /// 焦点顺序。
    FocusOrder,
    /// 读屏标签。
    ScreenLabel,
    /// 命中区域尺寸。
    HitArea,
}

impl A11yFacet {
    /// 全部五面（**五面不多不少**）。
    pub const ALL: [A11yFacet; 5] = [
        A11yFacet::Contrast,
        A11yFacet::FocusReach,
        A11yFacet::FocusOrder,
        A11yFacet::ScreenLabel,
        A11yFacet::HitArea,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            A11yFacet::Contrast => "对比度",
            A11yFacet::FocusReach => "焦点可达",
            A11yFacet::FocusOrder => "焦点顺序",
            A11yFacet::ScreenLabel => "读屏标签",
            A11yFacet::HitArea => "命中区域",
        }
    }

    /// 码。
    pub fn code(self) -> &'static str {
        match self {
            A11yFacet::Contrast => "AF-CONTRAST",
            A11yFacet::FocusReach => "AF-FOCUS-REACH",
            A11yFacet::FocusOrder => "AF-FOCUS-ORDER",
            A11yFacet::ScreenLabel => "AF-SCREEN-LABEL",
            A11yFacet::HitArea => "AF-HIT-AREA",
        }
    }

    /// 由码反查（往返一致）。
    pub fn from_code(code: &str) -> Option<A11yFacet> {
        A11yFacet::ALL.iter().copied().find(|f| f.code() == code)
    }

    /// 该面的**满分强度**（判据内建上限，改动后不得超过）。
    ///
    /// 取 3 是因为本域判据的强度分档就是「无 / 基础 / 强化」三档
    /// （对照 F4204 对比度判据的达标线约定）。分档写死成常量而不是
    /// 散落各处，是为了让「强度不可能为负、不可能超上限」成为**可断言的常量关系**，
    /// 而不是一句靠自觉的约定。
    pub const MAX_STRENGTH: u8 = 3;
}

/// 变更申请（**被测输入**：一份契约要改成什么样）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRequest {
    /// 契约 ID（**F4203 注册单源的键**）。
    pub contract_id: String,
    /// 目标版本（`major.minor.patch`）。
    pub target_version: String,
    /// 申请前判据正文（**单源引用不复制**：这里存的是判据哈希，不是副本，
    /// 真正文在 F4203 注册册里——与 F4204 同一纪律）。
    pub criteria_before: String,
    /// 改动后判据正文。
    pub criteria_after: String,
    /// 变更前已声明的无障碍面及强度（**基线**）。
    ///
    /// 取值即「改动前每面的强度」，缺项表示改动前就没有这一面。
    pub a11y_before: Vec<(A11yFacet, u8)>,
    /// 改动后声明的无障碍面及强度。
    pub a11y_after: Vec<(A11yFacet, u8)>,
    /// 是否附迁移指南（**破坏性变更必须附**）。
    pub migration_guide: Option<String>,
    /// 删除的字段名（**删字段必是破坏性**）。
    pub removed_fields: Vec<String>,
}

impl ChangeRequest {
    /// 构造（判据语料用）。
    pub fn new(
        contract_id: &str,
        target_version: &str,
        criteria_before: &str,
        criteria_after: &str,
        a11y_before: Vec<(A11yFacet, u8)>,
        a11y_after: Vec<(A11yFacet, u8)>,
    ) -> ChangeRequest {
        ChangeRequest {
            contract_id: contract_id.to_string(),
            target_version: target_version.to_string(),
            criteria_before: criteria_before.to_string(),
            criteria_after: criteria_after.to_string(),
            a11y_before,
            a11y_after,
            migration_guide: None,
            removed_fields: Vec::new(),
        }
    }

    /// 附迁移指南。
    pub fn with_migration(mut self, guide: &str) -> ChangeRequest {
        self.migration_guide = Some(guide.to_string());
        self
    }

    /// 声明删了字段（**删字段即破坏性**，不可自报豁免）。
    pub fn with_removed(mut self, field: &str) -> ChangeRequest {
        self.removed_fields.push(field.to_string());
        self
    }

    /// 判定前判据哈希（**存哈希不存正文**）。
    pub fn before_digest(&self) -> String {
        fnv1a64_hex(self.criteria_before.as_bytes())
    }

    /// 判定后判据哈希。
    pub fn after_digest(&self) -> String {
        fnv1a64_hex(self.criteria_after.as_bytes())
    }

    /// 取基线某面强度（缺项即 `0`——**改动前就没有这一面**）。
    pub fn before_strength(&self, f: A11yFacet) -> u8 {
        for (k, v) in self.a11y_before.iter() {
            if *k == f {
                return *v;
            }
        }
        0
    }

    /// 取改动后某面强度（缺项即 `0`）。
    pub fn after_strength(&self, f: A11yFacet) -> u8 {
        for (k, v) in self.a11y_after.iter() {
            if *k == f {
                return *v;
            }
        }
        0
    }

    /// 迁移指南是否**实质存在**（空串与纯空白不算——那等于没给）。
    ///
    /// 单看 [`Option::is_some`] 会把 `Some("")` 当成已附指南，
    /// 于是「填了个空字符串就过了闸」。判据侧也**独立重算**这一条，
    /// 不问被测方「你觉得算不算有」。
    pub fn migration_ok(&self) -> bool {
        match &self.migration_guide {
            None => false,
            Some(s) => !s.trim().is_empty(),
        }
    }

    /// **变更种类推导**（自报不可信，故由内容推导）。
    ///
    /// 三个破坏性来源，**任一成立即破坏**：
    ///
    /// 1. 删了字段（删字段 = 下游读不到它）；
    /// 2. 判据正文变了（语义动过）；
    /// 3. 无障碍面被**削弱或删除**（红线，见 [`A11yDelta`]）。
    ///
    /// 注意第 2 条用哈希比较而非文本比较：判据「改写」也算动过——
    /// 只比「删了哪几个词」会把「把『不得低于 4.5:1』改成
    /// 『不低于 4.5:1』」这种实质未变、实质已变两态都放过。
    pub fn classify(&self) -> ChangeKind {
        if !self.removed_fields.is_empty() {
            return ChangeKind::Breaking;
        }
        if self.before_digest() != self.after_digest() {
            return ChangeKind::Breaking;
        }
        if self.a11y_delta().is_weakened() {
            return ChangeKind::Breaking;
        }
        ChangeKind::Additive
    }
}

// ---------------------------------------------------------------------------
// 三、无障碍红线：弱化即破坏（由内容推导，不接受自报）
// ---------------------------------------------------------------------------

/// 无障碍影响推导结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct A11yDelta {
    /// 被**削弱**的面（强度下降）。
    pub weakened: Vec<A11yFacet>,
    /// 被**删除**的面（改动前有、改动后没有）。
    pub removed: Vec<A11yFacet>,
    /// 被**新增**的面（改动后有、改动前没有）。
    pub added: Vec<A11yFacet>,
}

impl A11yDelta {
    /// 无变化（**空转**）。
    pub fn none() -> A11yDelta {
        A11yDelta { weakened: Vec::new(), removed: Vec::new(), added: Vec::new() }
    }

    /// 是否发生弱化（**删面计入弱化**——删掉一个面比把它降级更狠）。
    pub fn is_weakened(&self) -> bool {
        !self.weakened.is_empty() || !self.removed.is_empty()
    }

    /// 弱化面数（**删面与削弱同权同计**）。
    pub fn weakened_count(&self) -> usize {
        self.weakened.len() + self.removed.len()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        if !self.is_weakened() {
            return "无障碍影响：未弱化".to_string();
        }
        let mut names: Vec<&str> = Vec::new();
        for f in self.removed.iter() {
            names.push(f.zh());
        }
        for f in self.weakened.iter() {
            names.push(f.zh());
        }
        format!("无障碍影响：弱化 {} 面（{}）", names.len(), names.join("、"))
    }
}

impl ChangeRequest {
    /// **无障碍影响推导**（本项红线所在）。
    ///
    /// 逐面比较改动前后强度：
    ///
    /// - 改动后**没有**该面 → [`A11yDelta::removed`]；
    /// - 改动后强度**更低** → [`A11yDelta::weakened`]；
    /// - 改动后强度**相等或更高** → 不算弱化；
    /// - 改动前没有、改动后有 → [`A11yDelta::added`]（新增面不是弱化）。
    ///
    /// 强度域**在推导内自行兜底**：超过 [`A11yFacet::MAX_STRENGTH`] 的
    /// 声明值按上限截断，避免「声明 250 强度」把比较变成假弱化。
    /// 依据第十节纪律第 8 条——`pub` 纯函数不能依赖调用方先钳制。
    pub fn a11y_delta(&self) -> A11yDelta {
        let mut d = A11yDelta::none();
        for f in A11yFacet::ALL.iter().copied() {
            let b = clamp_strength(self.before_strength(f));
            let a = clamp_strength(self.after_strength(f));
            if b > 0 && a == 0 {
                d.removed.push(f);
            } else if a < b {
                d.weakened.push(f);
            } else if b == 0 && a > 0 {
                d.added.push(f);
            }
        }
        d
    }
}

/// 强度钳制（**公开以供判据侧独立调用**，钉住「域封闭」这条性质）。
pub fn clamp_strength(v: u8) -> u8 {
    if v > A11yFacet::MAX_STRENGTH {
        A11yFacet::MAX_STRENGTH
    } else {
        v
    }
}

// ---------------------------------------------------------------------------
// 四、向后兼容检查闸
// ---------------------------------------------------------------------------

/// 目标版本的主版本号（**解析失败即 `None`**——不猜、不给默认值）。
///
/// 解析失败必须能被发现：把 `v2` 当成 `2` 会让「升主版本」这一闸
/// 在最常见的书写法上失效。
pub fn parse_major(version: &str) -> Option<u32> {
    let s = version.trim();
    let s = if s.starts_with('v') || s.starts_with('V') { &s[1..] } else { s };
    let head = s.split('.').next()?;
    if head.is_empty() {
        return None;
    }
    let mut digits_only = true;
    for c in head.chars() {
        if !c.is_ascii_digit() {
            digits_only = false;
            break;
        }
    }
    if !digits_only {
        return None;
    }
    head.parse::<u32>().ok()
}

/// 兼容闸结论（**两个「必须」各占一个布尔**，见头注§三）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatVerdict {
    /// 变更种类（**推导值**）。
    pub kind: ChangeKind,
    /// 是否升了主版本。
    pub major_bumped: bool,
    /// 是否附了实质迁移指南。
    pub migration_ok: bool,
    /// 无障碍影响（**推导值**）。
    pub a11y: A11yDelta,
}

impl CompatVerdict {
    /// 是否放行：`非破坏 || (升主版本 且 有迁移指南)`。
    pub fn allows(&self) -> bool {
        !self.kind.is_breaking() || (self.major_bumped && self.migration_ok)
    }

    /// 阻断原因码（**放行时为空串**）。
    ///
    /// 两个半吊子形态给**不同码**，否则复盘时分不出
    /// 「升了版没写指南」与「写了指南没升版」——而这两者的补法完全不同。
    pub fn block_code(&self) -> &'static str {
        if self.allows() {
            return "";
        }
        if self.kind.is_breaking() && !self.major_bumped && !self.migration_ok {
            return E_BREAK_NO_MAJOR_NO_GUIDE;
        }
        if !self.major_bumped {
            return E_BREAK_NO_MAJOR;
        }
        if !self.migration_ok {
            return E_BREAK_NO_GUIDE;
        }
        E_COMPAT_INTERNAL
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "兼容闸：{}｜主版本{}｜迁移指南{}｜{}",
            self.kind.zh(),
            if self.major_bumped { "已升" } else { "未升" },
            if self.migration_ok { "已附" } else { "未附" },
            if self.allows() { "放行" } else { "阻断" }
        )
    }
}

/// 向后兼容检查闸。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompatGate {
    verdicts: Vec<CompatVerdict>,
}

impl CompatGate {
    /// 空闸。
    pub fn new() -> CompatGate {
        CompatGate { verdicts: Vec::new() }
    }

    /// 结论只读遍历。
    pub fn verdicts(&self) -> &[CompatVerdict] {
        &self.verdicts
    }

    /// **闸本体**：对一份申请出结论。
    ///
    /// 基线版本从 `current_version` 现取（F4203 单源），
    /// 缺失即 `current_version` 为空 → **不判破坏也不判放行**，
    /// 交给 [`ChangeFlow::open`] 按「契约不在册」拒掉；
    /// 闸自己只对「拿得到基线」的申请负责。
    pub fn check(&mut self, req: &ChangeRequest, current_version: &str) -> Result<CompatVerdict, ConsistencyError> {
        if self.verdicts.len() >= MAX_GATE_VERDICTS {
            return Err(ConsistencyError::new(
                E_CAP,
                "兼容闸结论册已满",
                &format!("结论条数已达上限 {}", MAX_GATE_VERDICTS),
                "先完成在途变更再受理新的；结论册不留冗余",
                "契约变更流程引擎维护方",
            ));
        }
        let base = parse_major(current_version).unwrap_or(0);
        let target = parse_major(&req.target_version).unwrap_or(0);
        let v = CompatVerdict {
            kind: req.classify(),
            // 「升主版本」判的是**严格大于**——等于不算升。
            // 用 `>` 而非 `>=`：后者会让「同主版本内的破坏」被当成已升版放行，
            // 而那正是本闸要拦的主要形态。
            major_bumped: target > base,
            migration_ok: req.migration_ok(),
            a11y: req.a11y_delta(),
        };
        self.verdicts.push(v.clone());
        Ok(v)
    }
}

// ---------------------------------------------------------------------------
// 五、会签表与催办
// ---------------------------------------------------------------------------

/// 某一方的会签态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignState {
    /// 已签。
    Signed,
    /// 已拒（**一票否决**）。
    Refused,
}

impl SignState {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            SignState::Signed => "已签",
            SignState::Refused => "已拒",
        }
    }
}

/// 会签结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CosignVerdict {
    /// 全签通过。
    Passed,
    /// 有方未签（**挂起**，见头注§五）。
    Pending,
    /// 有方已拒（**驳回**）。
    Refused,
}

impl CosignVerdict {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CosignVerdict::Passed => "会签通过",
            CosignVerdict::Pending => "会签挂起",
            CosignVerdict::Refused => "会签驳回",
        }
    }

    /// 是否可继续（**只有 Passed 可继续**）。
    pub fn may_advance(self) -> bool {
        match self {
            CosignVerdict::Passed => true,
            CosignVerdict::Pending => false,
            CosignVerdict::Refused => false,
        }
    }
}

/// 会签表的一行（**谁、按什么、签没签**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CosignRow {
    /// 域（**相关方**）。
    pub domain: DomainTag,
    /// 会签态（**None = 未表态**）。
    pub state: Option<SignState>,
    /// 该方要求的会签要点数（**并**关系：要点全中才算签）。
    pub required_points: u8,
    /// 该方已同意的要点数。
    pub agreed_points: u8,
}

impl CosignRow {
    /// 构造。
    pub fn new(domain: DomainTag, required_points: u8) -> CosignRow {
        CosignRow { domain, state: None, required_points, agreed_points: 0 }
    }

    /// 是否**已表态且通过**（`None` 即未表态，不算通过）。
    pub fn is_signed(&self) -> bool {
        match self.state {
            Some(SignState::Signed) => self.agreed_points >= self.required_points,
            Some(SignState::Refused) => false,
            None => false,
        }
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        let s = match self.state {
            Some(SignState::Signed) => "已签",
            Some(SignState::Refused) => "已拒",
            None => "未表态",
        };
        format!(
            "{}：{}（要点 {}/{}）",
            self.domain.zh(),
            s,
            self.agreed_points,
            self.required_points
        )
    }
}

/// 催办记录（**重复催办累加不覆盖**，见头注§五）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DunningRecord {
    /// 契约 ID。
    pub contract_id: String,
    /// 被催方。
    pub domain: DomainTag,
    /// 催办次数（**累加**）。
    pub notices: u32,
}

impl DunningRecord {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "催办：{} 请 {} 会签，已催 {} 次",
            self.contract_id,
            self.domain.zh(),
            self.notices
        )
    }
}

// ---------------------------------------------------------------------------
// 六、灰度：档位与观测分离
// ---------------------------------------------------------------------------

/// 灰度档位（**四档**，锚点判据「灰度」落点）。
///
/// 门槛随档位收紧，最后一档是全量。**不能用裸百分比**——那让「100% 前
/// 必须有一次小流量观察」不可表达（见头注§六）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanaryStage {
    /// 内部（仅开发侧）。
    Internal,
    /// 1%。
    OnePercent,
    /// 10%。
    TenPercent,
    /// 全量。
    Full,
}

impl CanaryStage {
    /// 全部四档（**顺序即推进序**）。
    pub const ALL: [CanaryStage; 4] =
        [CanaryStage::Internal, CanaryStage::OnePercent, CanaryStage::TenPercent, CanaryStage::Full];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CanaryStage::Internal => "内部",
            CanaryStage::OnePercent => "1%",
            CanaryStage::TenPercent => "10%",
            CanaryStage::Full => "全量",
        }
    }

    /// 流量百分比（**内部档记 `0`**——它不对外放量）。
    pub fn percent(self) -> u8 {
        match self {
            CanaryStage::Internal => 0,
            CanaryStage::OnePercent => 1,
            CanaryStage::TenPercent => 10,
            CanaryStage::Full => 100,
        }
    }

    /// 下一档（**末档返回 None**）。
    pub fn next(self) -> Option<CanaryStage> {
        let i = CanaryStage::ALL.iter().position(|s| *s == self)?;
        CanaryStage::ALL.get(i + 1).copied()
    }
}

/// 一档灰度的观测结论（**外部事实**，与档位分离）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanaryObservation {
    /// 观测档位。
    pub stage: CanaryStage,
    /// 该档是否健康。
    pub healthy: bool,
    /// 观测方（**留名，便于追责**）。
    pub observer: DomainTag,
}

/// 灰度结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanaryVerdict {
    /// 本档健康，可进下一档。
    Healthy,
    /// 本档不健康（**须回滚**）。
    Unhealthy,
}

impl CanaryVerdict {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CanaryVerdict::Healthy => "灰度健康",
            CanaryVerdict::Unhealthy => "灰度异常",
        }
    }
}

// ---------------------------------------------------------------------------
// 七、痕迹与立案
// ---------------------------------------------------------------------------

/// 痕迹结论（**动作级**：被拒与作废都留痕，见头注§一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceOutcome {
    /// 通过，流程推进。
    Advanced,
    /// 被拒（**越步等**，见头注§一/§二）。
    Rejected,
    /// 作废（**该步之前走出的痕迹被标作废，但不删行**）。
    Voided,
    /// 挂起（**会签缺等**：不许过，我去催）。
    Held,
}

impl TraceOutcome {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            TraceOutcome::Advanced => "通过",
            TraceOutcome::Rejected => "被拒",
            TraceOutcome::Voided => "作废",
            TraceOutcome::Held => "挂起",
        }
    }

    /// 是否为**有效推进**（**只有它推进了流程**）。
    pub fn is_progress(self) -> bool {
        match self {
            TraceOutcome::Advanced => true,
            _ => false,
        }
    }
}

/// 一条流程痕迹（**步步留痕**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowTrace {
    /// 契约 ID。
    pub contract_id: String,
    /// 步位。
    pub step: FlowStep,
    /// 结论。
    pub outcome: TraceOutcome,
    /// 原因码（**通过时为空串**）。
    pub code: &'static str,
    /// 留痕人（**执行者留名**）。
    pub actor: DomainTag,
    /// 说明。
    pub detail: String,
}

impl FlowTrace {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "[{}] {} · {}：{}{}",
            self.step.ordinal(),
            self.step.zh(),
            self.outcome.zh(),
            self.actor.zh(),
            if self.detail.is_empty() {
                String::new()
            } else {
                format!("· {}", self.detail)
            }
        )
    }
}

/// 立案（**回滚失败等不可自愈情形**，见头注§七）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseRecord {
    /// 契约 ID。
    pub contract_id: String,
    /// 原因码（**两种失败必须可区分**，见头注§七）。
    pub code: &'static str,
    /// 严重度。
    pub severity: Severity,
    /// 现象。
    pub symptom: String,
    /// 建议（**拒绝必须给出路**）。
    pub advice: String,
}

impl CaseRecord {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!("立案[{}]：{}｜{}｜{}：{}", self.code, self.severity.zh(), self.contract_id, self.symptom, self.advice)
    }
}

// ---------------------------------------------------------------------------
// 八、回滚协议
// ---------------------------------------------------------------------------

/// 回滚结论。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RollbackVerdict {
    /// 回滚成功（**版本已退回**）。
    Ok {
        /// 退回到的版本。
        restored: String,
    },
    /// 回滚目标在册里不存在（**申请方写错，可修复**）。
    TargetMissing(String),
    /// 回滚目标已废止（**不可修复**：回滚等于复活）。
    TargetRetired(String),
}

impl RollbackVerdict {
    /// 是否成功。
    pub fn is_ok(&self) -> bool {
        match self {
            RollbackVerdict::Ok { .. } => true,
            _ => false,
        }
    }

    /// 中文名。
    pub fn zh(&self) -> String {
        match self {
            RollbackVerdict::Ok { restored } => format!("回滚成功（退回 {}）", restored),
            RollbackVerdict::TargetMissing(v) => format!("回滚失败：目标 {} 不在册", v),
            RollbackVerdict::TargetRetired(v) => format!("回滚失败：目标 {} 已废止", v),
        }
    }
}

// ---------------------------------------------------------------------------
// 九、错误码与容量
// ---------------------------------------------------------------------------

/// 错误码。
pub const E_STEP_OUT_OF_ORDER: &str = "E_STEP_OUT_OF_ORDER";
pub const E_CANARY_SKIP: &str = "E_CANARY_SKIP";
pub const E_COMPAT_BLOCKED: &str = "E_COMPAT_BLOCKED";
pub const E_BREAK_NO_MAJOR: &str = "E_BREAK_NO_MAJOR";
pub const E_BREAK_NO_GUIDE: &str = "E_BREAK_NO_GUIDE";
pub const E_BREAK_NO_MAJOR_NO_GUIDE: &str = "E_BREAK_NO_MAJOR_NO_GUIDE";
pub const E_COMPAT_INTERNAL: &str = "E_COMPAT_INTERNAL";
pub const E_COSIGN_PENDING: &str = "E_COSIGN_PENDING";
pub const E_COSIGN_REFUSED: &str = "E_COSIGN_REFUSED";
pub const E_CANARY_UNHEALTHY: &str = "E_CANARY_UNHEALTHY";
pub const E_ROLLBACK_MISSING: &str = "E_ROLLBACK_MISSING";
pub const E_ROLLBACK_RETIRED: &str = "E_ROLLBACK_RETIRED";
pub const E_A11Y_WEAKENED: &str = "E_A11Y_WEAKENED";
pub const E_VERSION_MALFORMED: &str = "E_VERSION_MALFORMED";
pub const E_CONTRACT_UNKNOWN: &str = "E_CONTRACT_UNKNOWN";
pub const E_FLOW_VOIDED: &str = "E_FLOW_VOIDED";
pub const E_STEP_STATE: &str = "E_STEP_STATE";
pub const E_CAP: &str = "E_CAP";

/// 容量上限。
pub const MAX_FLOWS: usize = 16;
pub const MAX_TRACES: usize = 64;
pub const MAX_DUNNING: usize = 32;
pub const MAX_CASES: usize = 16;
pub const MAX_GATE_VERDICTS: usize = 32;
pub const MAX_DIAG: usize = 16;

/// 诊断日志（**容量触顶零静默**，见头注§八）。
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

    /// 记一条（**满了返回 false，不静默丢弃**）。
    pub fn note(&mut self, text: String) -> bool {
        if self.items.len() >= MAX_DIAG {
            return false;
        }
        self.items.push(text);
        true
    }

    /// 是否发生过溢出（**读屏方据此知道「记录不全」**）。
    pub fn truncated(&self) -> bool {
        self.items.len() >= MAX_DIAG
    }

    /// 读屏单行（**溢出必须能被读出来**）。
    pub fn screen_text(&self) -> String {
        let mut s = String::new();
        for it in self.items.iter() {
            s.push_str(it);
            s.push('\n');
        }
        if self.truncated() {
            s.push_str("（痕迹/记录已触顶：以上为降级留痕，非全量）\n");
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 十、变更流程（一份申请的完整生命周期）
// ---------------------------------------------------------------------------

/// 一份契约的变更流程（**流程册一行**：六步 + 会签 + 灰度 + 回滚）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeFlow {
    /// 申请。
    pub req: ChangeRequest,
    /// **基线版本**（受理时从 F4203 现取，见头注§九）。
    ///
    /// 单独存一份而不每次回查，是因为「回滚 = 退回基线」与
    /// 「升主版本 = 高于基线」两条判定都要用它；而**它只在受理时取一次**，
    /// 此后不再刷新——它是**这次变更的起点快照**，不是注册册的镜像。
    /// 拿它当实时状态用就会出现「变更进行中注册册被别人改了，
    /// 于是本流程的基线跟着漂移」，而基线漂移会让兼容闸的比较失去参照。
    pub base_version: String,
    /// 游标：当前**已推进到的步位**（`None` = 一步未走）。
    pub cursor: Option<FlowStep>,
    /// 兼容闸结论（`None` = 尚未过闸）。
    pub verdict: Option<CompatVerdict>,
    /// 会签表（**须与消费方域集合对齐**）。
    pub cosign: Vec<CosignRow>,
    /// 当前灰度档（`None` = 尚未进灰度步）。
    pub canary: Option<CanaryStage>,
    /// 作废起点（`None` = 未发生作废；见头注§二）。
    pub voided_from: Option<FlowStep>,
    /// 回滚记录（`None` = 未回滚）。
    pub last_rollback: Option<RollbackVerdict>,
}

impl ChangeFlow {
    /// 取会签表行。
    pub fn cosign_row(&self, d: DomainTag) -> Option<&CosignRow> {
        self.cosign.iter().find(|r| r.domain == d)
    }

    /// 会签表行可变引用（**供调用方表态**）。
    pub fn cosign_row_mut(&mut self, d: DomainTag) -> Option<&mut CosignRow> {
        self.cosign.iter_mut().find(|r| r.domain == d)
    }

    /// 下一应走步位（**游标为 `None` 时即第一步**）。
    pub fn expected_step(&self) -> FlowStep {
        match self.cursor {
            None => FlowStep::Request,
            Some(s) => match s.next() {
                Some(n) => n,
                None => FlowStep::Announce,
            }
        }
    }

    /// 是否已全程走完六步。
    pub fn is_complete(&self) -> bool {
        self.cursor == Some(FlowStep::Announce) && self.voided_from.is_none()
    }

    /// **未表态方清单**（判据侧要指名到域，不能只说「有人没签」）。
    pub fn pending_parties(&self) -> Vec<DomainTag> {
        let mut out: Vec<DomainTag> = Vec::new();
        for r in self.cosign.iter() {
            if !r.is_signed() {
                out.push(r.domain);
            }
        }
        out
    }

    /// 会签结论（**逐行独立判定，不看总数**）。
    ///
    /// 顺序上先看「有拒签」再看「有未签」：拒签是**终局**（一票否决，
    /// 再等其余方也没意义），挂起是**可恢复**。
    pub fn cosign_verdict(&self) -> CosignVerdict {
        let mut any_pending = false;
        for r in self.cosign.iter() {
            match r.state {
                Some(SignState::Refused) => return CosignVerdict::Refused,
                Some(SignState::Signed) => {
                    if r.agreed_points < r.required_points {
                        any_pending = true;
                    }
                }
                None => any_pending = true,
            }
        }
        if any_pending {
            CosignVerdict::Pending
        } else {
            CosignVerdict::Passed
        }
    }
}

// ---------------------------------------------------------------------------
// 十一、流程引擎
// ---------------------------------------------------------------------------

/// 契约变更流程引擎（六步流 + 兼容闸 + 会签 + 灰度 + 回滚）。
#[derive(Clone, Debug, Default)]
pub struct ChangeFlowEngine {
    flows: Vec<ChangeFlow>,
    traces: Vec<FlowTrace>,
    dunning: Vec<DunningRecord>,
    cases: Vec<CaseRecord>,
    gate: CompatGate,
    diag: DiagLog,
}

impl ChangeFlowEngine {
    /// 空引擎。
    pub fn new() -> ChangeFlowEngine {
        ChangeFlowEngine {
            flows: Vec::new(),
            traces: Vec::new(),
            dunning: Vec::new(),
            cases: Vec::new(),
            gate: CompatGate::new(),
            diag: DiagLog::new(),
        }
    }

    /// 流程册只读遍历。
    pub fn flows(&self) -> &[ChangeFlow] {
        &self.flows
    }

    /// 痕迹只读遍历（**含被拒与作废**，见头注§一）。
    pub fn traces(&self) -> &[FlowTrace] {
        &self.traces
    }

    /// 催办只读遍历。
    pub fn dunning(&self) -> &[DunningRecord] {
        &self.dunning
    }

    /// 立案只读遍历。
    pub fn cases(&self) -> &[CaseRecord] {
        &self.cases
    }

    /// 兼容闸只读。
    pub fn gate(&self) -> &CompatGate {
        &self.gate
    }

    /// 诊断日志只读。
    pub fn diag(&self) -> &DiagLog {
        &self.diag
    }

    /// 取流程。
    pub fn flow(&self, contract_id: &str) -> Option<&ChangeFlow> {
        self.flows.iter().find(|f| f.req.contract_id == contract_id)
    }

    /// 取流程可变引用。
    pub fn flow_mut(&mut self, contract_id: &str) -> Option<&mut ChangeFlow> {
        self.flows.iter_mut().find(|f| f.req.contract_id == contract_id)
    }

    /// 某契约的痕迹（**按步位序**）。
    pub fn traces_of(&self, contract_id: &str) -> Vec<&FlowTrace> {
        let mut out: Vec<&FlowTrace> = Vec::new();
        for t in self.traces.iter() {
            if t.contract_id == contract_id {
                out.push(t);
            }
        }
        out
    }

    /// **留痕（唯一的痕迹写入口）**——触顶时降级为诊断串，**绝不静默丢**。
    fn push_trace(&mut self, t: FlowTrace) -> bool {
        if self.traces.len() < MAX_TRACES {
            self.traces.push(t);
            return true;
        }
        let line = format!(
            "痕迹溢出降级：{} 第{}步 {}",
            t.contract_id,
            t.step.ordinal(),
            t.outcome.zh()
        );
        let _ = self.diag.note(line);
        false
    }

    /// **受理一份变更申请**（第一步·申请）。
    ///
    /// F4203 注册单源：契约必须在册，且**基线版本现取**。
    /// 会签表**按消费方域集合铺开**——须签的方来自注册册的消费方声明，
    /// 不来自申请里的一句话（否则「申请方说只有一方要签」就绕过了会签）。
    pub fn open(&mut self, req: ChangeRequest, reg: &ContractRegistry) -> Result<(), ConsistencyError> {
        if self.flows.len() >= MAX_FLOWS {
            return Err(ConsistencyError::new(
                E_CAP,
                "流程册已满",
                &format!("在途流程数已达上限 {}", MAX_FLOWS),
                "先完成或回滚在途变更再受理新的",
                "契约变更流程引擎维护方",
            ));
        }
        if self.flow(&req.contract_id).is_some() {
            return Err(ConsistencyError::new(
                E_STEP_STATE,
                "该契约已有在途变更",
                &format!("契约 {} 已有流程在途，不接受并行申请", req.contract_id),
                "先完成或回滚在途流程；同一契约同时只允许一条变更",
                "契约变更流程引擎维护方",
            ));
        }
        if parse_major(&req.target_version).is_none() {
            return Err(ConsistencyError::new(
                E_VERSION_MALFORMED,
                "目标版本号不可解析",
                &format!("目标版本 {:?} 不含可解析的主版本段", req.target_version),
                "写成 major.minor.patch（如 3.1.0）；破坏性变更须主版本高于基线",
                "变更申请方",
            ));
        }
        let base_v = match reg.current(&req.contract_id) {
            Some(e) => e.meta.version.clone(),
            None => {
                return Err(ConsistencyError::new(
                    E_CONTRACT_UNKNOWN,
                    "契约不在册，申请不受理",
                    &format!("注册册中查不到契约 {}", req.contract_id),
                    "先在契约注册中心登记该契约，再走变更流程",
                    "变更申请方",
                ));
            }
        };

        // 会签表**按注册册声明的消费方铺开**（F4203 单源，见头注§九）：
        // 须签的方来自注册册，不来自申请里的一句话。
        let consumers = reg.consumers(&req.contract_id, &base_v);
        let mut cosign: Vec<CosignRow> = Vec::new();
        for d in consumers.iter().copied() {
            cosign.push(CosignRow::new(d, 1));
        }
        let detail = format!("受理，基线版本 {}", base_v);
        let flow = ChangeFlow {
            req,
            // **受理即完成第一步**（申请步的痕迹在下面落定）。
            // 不置 Some(Request) 会让 expected_step() 永远停在申请步，
            // 整个流程第一步走不动——故游标在此初始化为已走完第一步。
            base_version: base_v,
            cursor: Some(FlowStep::Request),
            verdict: None,
            cosign,
            canary: None,
            voided_from: None,
            last_rollback: None,
        };
        let cid = flow.req.contract_id.clone();
        self.flows.push(flow);
        let _ = self.push_trace(FlowTrace {
            contract_id: cid,
            step: FlowStep::Request,
            outcome: TraceOutcome::Advanced,
            code: "",
            actor: DomainTag::U,
            detail,
        });
        Ok(())
    }

    /// **走步**（第二至第六步统一入口，**越步在此拦**）。
    ///
    /// 六步流与灰度档位共用这条纪律（头注§六）：灰度越档若不走这里，
    /// 第四步就成了绕过整条流程的暗门。
    pub fn advance(&mut self, contract_id: &str, actor: DomainTag) -> Result<FlowStep, ConsistencyError> {
        let (expect, voided, canary_now, next) = {
            let f = match self.flow(contract_id) {
                Some(f) => f,
                None => {
                    return Err(ConsistencyError::new(
                        E_CONTRACT_UNKNOWN,
                        "无此流程",
                        &format!("找不到契约 {} 的变更流程", contract_id),
                        "先调用 open 受理申请",
                        "流程引擎调用方",
                    ));
                }
            };
            let expect = f.expected_step();
            let next = match expect.next() {
                Some(n) => n,
                None => expect,
            };
            (expect, f.voided_from.is_some(), f.canary, next)
        };

        // 已作废的流程不许再走（**先于一切其它检查**）。
        if voided {
            return Err(ConsistencyError::new(
                E_FLOW_VOIDED,
                "流程已作废，不接受推进",
                &format!("契约 {} 的流程已被作废", contract_id),
                "重开一条变更流程；已作废的痕迹保留供复盘",
                "流程引擎调用方",
            ));
        }

        match expect {
            FlowStep::Request => {
                // open 已落定申请步；走到这里说明游标被外部改回，
                // 按越步处理（**不静默放行**）。
                self.void_flow(contract_id, FlowStep::Request, actor, E_STEP_OUT_OF_ORDER, "申请步已受理，不可重复推进");
                Err(ConsistencyError::new(
                    E_STEP_OUT_OF_ORDER,
                    "申请步已受理，不接受重复推进",
                    &format!("契约 {} 的游标被退回申请步", contract_id),
                    "从影响分析步继续；受理已成立，重复申请不予受理",
                    "流程引擎调用方",
                ))
            }
            FlowStep::Impact => {
                self.step_impact(contract_id, actor)?;
                Ok(next)
            }
            FlowStep::Cosign => {
                self.step_cosign(contract_id, actor)?;
                Ok(next)
            }
            FlowStep::Canary => {
                self.step_canary_enter(contract_id, actor)?;
                Ok(next)
            }
            FlowStep::Cutover => {
                // 切换步要求**灰度已到全量档**——不然「灰度」形同虚设。
                if canary_now != Some(CanaryStage::Full) {
                    let at = match canary_now {
                        Some(s) => s.zh(),
                        None => "未进灰度",
                    };
                    let _ = self.push_trace(FlowTrace {
                        contract_id: contract_id.to_string(),
                        step: FlowStep::Canary,
                        outcome: TraceOutcome::Rejected,
                        code: E_CANARY_SKIP,
                        actor,
                        detail: format!("灰度未到全量（{}），切换步被拒", at),
                    });
                    return Err(ConsistencyError::new(
                        E_CANARY_SKIP,
                        "灰度未到全量，拒绝切换",
                        &format!("灰度停在 {} 档，未到全量", at),
                        "先逐档推进灰度并在每档给出健康观测；越档不被接受",
                        "变更执行方",
                    ));
                }
                // 兼容闸与会签**在各自那一步已把关**（未过关则游标不会推进到这里），
                // 故此处只做纵深复核：任一不成立即拒，不靠「前面应该拦住了」。
                self.gate_cutover(contract_id, actor)?;
                self.mark_cursor(contract_id, FlowStep::Cutover, actor, true, "切换完成")?;
                Ok(next)
            }
            FlowStep::Announce => {
                self.gate_cutover(contract_id, actor)?;
                self.mark_cursor(contract_id, FlowStep::Announce, actor, true, "通告发布")?;
                Ok(next)
            }
        }
    }

    /// **切换/通告步的纵深复核**（兼容闸结论 + 会签齐备 + 灰度全量）。
    ///
    /// 写成显式复核而非依赖上游，是因为「上游一定拦住了」是**推断**不是**保证**：
    /// 一旦将来有人在 `mark_cursor` 之前插一段代码改了游标，
    /// 这里就是唯一还能拦住「未过闸就切换」的地方。
    fn gate_cutover(&mut self, contract_id: &str, actor: DomainTag) -> Result<(), ConsistencyError> {
        let (ok_verdict, cv, stage) = {
            let f = match self.flow(contract_id) {
                Some(f) => f,
                None => return Err(ConsistencyError::new(E_CONTRACT_UNKNOWN, "无此流程", "流程已不存在", "重开申请", "流程引擎调用方")),
            };
            (
                f.verdict.as_ref().map(|v| v.allows()).unwrap_or(false),
                f.cosign_verdict(),
                f.canary,
            )
        };
        if !ok_verdict {
            return Err(ConsistencyError::new(
                E_COMPAT_BLOCKED,
                "兼容闸未放行，拒绝切换/通告",
                "本流程的兼容闸结论不满足放行条件",
                "重走影响分析步：破坏性变更须升主版本并附实质迁移指南",
                "变更申请方",
            ));
        }
        if !cv.may_advance() {
            let _ = self.push_trace(FlowTrace {
                contract_id: contract_id.to_string(),
                step: FlowStep::Cosign,
                outcome: TraceOutcome::Held,
                code: E_COSIGN_PENDING,
                actor,
                detail: "切换/通告步复核：会签未齐".to_string(),
            });
            return Err(ConsistencyError::new(
                E_COSIGN_PENDING,
                "会签未齐，拒绝切换/通告",
                "会签结论不是通过",
                "催办未签方，齐签后重走会签步",
                "会签召集方",
            ));
        }
        if stage != Some(CanaryStage::Full) {
            return Err(ConsistencyError::new(
                E_CANARY_SKIP,
                "灰度未到全量，拒绝切换/通告",
                "灰度档位未到全量档",
                "先逐档推进灰度并在每档给出健康观测",
                "变更执行方",
            ));
        }
        Ok(())
    }

    /// 第二步·影响分析（**兼容闸在此执行**）。
    fn step_impact(&mut self, contract_id: &str, actor: DomainTag) -> Result<(), ConsistencyError> {
        let (req, base_v) = {
            let f = match self.flow(contract_id) {
                Some(f) => f,
                None => {
                    return Err(ConsistencyError::new(
                        E_CONTRACT_UNKNOWN,
                        "无此流程",
                        "流程已不存在",
                        "重新受理申请",
                        "流程引擎调用方",
                    ));
                }
            };
            // **基线版本取自受理时的快照**（见 `ChangeFlow::base_version`），
            // 不在此处回查注册册——回查会让比较基准随他人变更漂移。
            (f.req.clone(), f.base_version.clone())
        };
        let verdict = self.gate.check(&req, &base_v)?;
        let allowed = verdict.allows();
        let weakened = verdict.a11y.is_weakened();
        // 阻断码与 a11y 读屏**先算出来**再挪动 verdict——
        // verdict 随后要存进流程册（被移走），而错误构造仍要读它。
        let block_code = verdict.block_code();
        let detail = format!("{}｜{}", verdict.screen_line(), verdict.a11y.screen_line());

        // 弱化单独再留一条痕：**红线必须在痕迹里看得见**，
        // 不能只藏在兼容闸结论的一个布尔里（布尔一翻转就看不出曾弱化过）。
        if weakened {
            let _ = self.push_trace(FlowTrace {
                contract_id: contract_id.to_string(),
                step: FlowStep::Impact,
                outcome: TraceOutcome::Rejected,
                code: E_A11Y_WEAKENED,
                actor,
                detail: format!(
                    "{}；按红线判为破坏性变更，须升主版本并附迁移指南",
                    verdict.a11y.screen_line()
                ),
            });
        }

        if let Some(f) = self.flow_mut(contract_id) {
            f.verdict = Some(verdict);
        }
        self.mark_cursor(contract_id, FlowStep::Impact, actor, allowed, &detail)?;
        if !allowed {
            return Err(ConsistencyError::new(
                E_COMPAT_BLOCKED,
                "兼容闸阻断：破坏性变更未履行「升主版本 + 迁移指南」",
                &format!("{}（阻断码 {}）", detail, block_code),
                "升主版本（严格大于基线）并附实质迁移指南后重走影响分析步",
                "变更申请方",
            ));
        }
        Ok(())
    }

    /// 第三步·相关方会签（**缺签挂起 + 催办**，见头注§五）。
    fn step_cosign(&mut self, contract_id: &str, actor: DomainTag) -> Result<(), ConsistencyError> {
        let (pending, refused) = {
            let f = match self.flow(contract_id) {
                Some(f) => f,
                None => return Err(ConsistencyError::new(E_CONTRACT_UNKNOWN, "无此流程", "流程已不存在", "重开申请", "流程引擎调用方")),
            };
            let v = f.cosign_verdict();
            (f.pending_parties(), v == CosignVerdict::Refused)
        };

        if refused {
            let _ = self.push_trace(FlowTrace {
                contract_id: contract_id.to_string(),
                step: FlowStep::Cosign,
                outcome: TraceOutcome::Rejected,
                code: E_COSIGN_REFUSED,
                actor,
                detail: "有方拒签，一票否决".to_string(),
            });
            return Err(ConsistencyError::new(
                E_COSIGN_REFUSED,
                "会签被拒",
                "至少一个相关方已拒签",
                "修正后重新发起变更；拒签为一票否决，不接受其余方同意的抵消",
                "变更申请方",
            ));
        }

        if pending.is_empty() {
            self.mark_cursor(contract_id, FlowStep::Cosign, actor, true, "会签全通过")?;
            return Ok(());
        }

        // 挂起：**游标不退**（不许过），同时逐一催办（去催）。
        for d in pending.iter().copied() {
            self.dun(contract_id, d);
        }
        let names: Vec<&str> = pending.iter().map(|d| d.zh()).collect();
        let detail = format!("会签缺：{}", names.join("、"));
        let _ = self.push_trace(FlowTrace {
            contract_id: contract_id.to_string(),
            step: FlowStep::Cosign,
            outcome: TraceOutcome::Held,
            code: E_COSIGN_PENDING,
            actor,
            detail,
        });
        Err(ConsistencyError::new(
            E_COSIGN_PENDING,
            "会签未齐，流程挂起",
            &format!("未完成会签的方：{}", names.join("、")),
            "催办已发出（见 dunning）；齐签后方可进入灰度步。缺签不得跳过本步",
            "会签召集方",
        ))
    }

    /// 第四步·进入灰度（**初始落在内部档**）。
    fn step_canary_enter(&mut self, contract_id: &str, actor: DomainTag) -> Result<(), ConsistencyError> {
        if let Some(f) = self.flow_mut(contract_id) {
            f.canary = Some(CanaryStage::Internal);
        }
        self.mark_cursor(contract_id, FlowStep::Canary, actor, true, "进入内部档")?;
        Ok(())
    }

    /// **灰度逐档推进**（**越档即越步**，见头注§六）。
    ///
    /// `target` 是**请求的目标档**，不是「加一档」——请求方可以要任何档，
    /// 但只有恰好等于当前档的下一档才被接受。这让「跳档」在数据上可表达，
    /// 从而可被拒。
    pub fn advance_canary(
        &mut self,
        contract_id: &str,
        target: CanaryStage,
        obs: CanaryObservation,
        actor: DomainTag,
    ) -> Result<CanaryVerdict, ConsistencyError> {
        let cur = {
            let f = match self.flow(contract_id) {
                Some(f) => f,
                None => {
                    return Err(ConsistencyError::new(
                        E_CONTRACT_UNKNOWN,
                        "无此流程",
                        &format!("找不到契约 {} 的变更流程", contract_id),
                        "先受理申请并走到灰度步",
                        "流程引擎调用方",
                    ));
                }
            };
            match f.canary {
                Some(c) => c,
                None => {
                    return Err(ConsistencyError::new(
                        E_STEP_OUT_OF_ORDER,
                        "尚未进入灰度步",
                        &format!("契约 {} 的灰度档位未初始化", contract_id),
                        "先推进到灰度步，再逐档放流量",
                        "变更执行方",
                    ));
                }
            }
        };

        // 观测与档位必须一致：**观测别的档等于没观测这一档**。
        if obs.stage != cur {
            return Err(ConsistencyError::new(
                E_STEP_STATE,
                "观测档位与当前档不符",
                &format!("当前档 {}，观测却报 {}", cur.zh(), obs.stage.zh()),
                "每档的观测必须针对该档本身",
                "灰度观测方",
            ));
        }

        let expect_next = match cur.next() {
            Some(n) => n,
            None => {
                return Err(ConsistencyError::new(
                    E_CANARY_SKIP,
                    "灰度已到全量档",
                    "全量档是末档，无下一档",
                    "进入切换步",
                    "变更执行方",
                ));
            }
        };

        if target != expect_next {
            // 越档：与越步同等处理——**作废 + 回退**（头注§二/§六）。
            self.void_flow(contract_id, FlowStep::Canary, actor, E_CANARY_SKIP, &format!(
                "灰度越档：请求 {}，当前档 {} 的下一档是 {}",
                target.zh(),
                cur.zh(),
                expect_next.zh()
            ));
            return Err(ConsistencyError::new(
                E_CANARY_SKIP,
                "灰度越档，申请作废并回退",
                &format!("请求档 {} 不是当前档 {} 的下一档 {}", target.zh(), cur.zh(), expect_next.zh()),
                "灰度必须逐档推进；若确需跳档，先撤回本次变更再按新方案重发",
                "变更执行方",
            ));
        }

        if !obs.healthy {
            // 不健康：**先落痕再报**，让「灰度异常」在痕迹里可查。
            let _ = self.push_trace(FlowTrace {
                contract_id: contract_id.to_string(),
                step: FlowStep::Canary,
                outcome: TraceOutcome::Rejected,
                code: E_CANARY_UNHEALTHY,
                actor: obs.observer,
                detail: format!("{} 档不健康", cur.zh()),
            });
            return Err(ConsistencyError::new(
                E_CANARY_UNHEALTHY,
                "灰度观测不健康",
                &format!("{} 档观测为不健康", cur.zh()),
                "回滚本次变更（见 rollback）；健康观测齐备前不得推进下一档",
                "变更执行方",
            ));
        }

        if let Some(f) = self.flow_mut(contract_id) {
            f.canary = Some(target);
        }
        let _ = self.push_trace(FlowTrace {
            contract_id: contract_id.to_string(),
            step: FlowStep::Canary,
            outcome: TraceOutcome::Advanced,
            code: "",
            actor: obs.observer,
            detail: format!("{} 档健康，进入 {} 档", cur.zh(), target.zh()),
        });
        Ok(CanaryVerdict::Healthy)
    }

    /// **回滚协议**（成功则退回基线版本，失败则**立案**，见头注§七）。
    ///
    /// 目标即**本次变更的基线版本**（回滚 = 退回起点），故不需要调用方
    /// 传目标——传了反而多一条「申请人回滚到某个非基线版本」的歧义路径。
    pub fn rollback(&mut self, contract_id: &str, reg: &ContractRegistry) -> RollbackVerdict {
        let base_v = self
            .flow(contract_id)
            .map(|f| f.base_version.clone())
            .unwrap_or_default();
        self.rollback_to(contract_id, &base_v, reg)
    }

    /// **回滚到指定版本**（回滚协议本体，`&ContractRegistry` 为 F4203 单源）。
    pub fn rollback_to(
        &mut self,
        contract_id: &str,
        target_version: &str,
        reg: &ContractRegistry,
    ) -> RollbackVerdict {
        let v = self.probe_rollback(contract_id, target_version, reg);
        if let Some(f) = self.flow_mut(contract_id) {
            f.last_rollback = Some(v.clone());
            // 回滚后流程回到**未启动**（已走的步全部不再成立）。
            if v.is_ok() {
                f.cursor = None;
                f.canary = None;
                f.voided_from = None;
            }
        }
        v
    }

    /// **回滚目标可回滚性判定**（**需注册册**；判据侧另走不依赖本函数的独立重算）。
    pub fn probe_rollback(
        &mut self,
        contract_id: &str,
        target_version: &str,
        reg: &ContractRegistry,
    ) -> RollbackVerdict {
        let entry = reg.lookup(contract_id, target_version);
        match entry {
            None => {
                let v = RollbackVerdict::TargetMissing(target_version.to_string());
                self.open_case(contract_id, E_ROLLBACK_MISSING, Severity::Blocking, v.zh(), &format!(
                    "确认版本号写法（{}）与注册册一致；回滚目标必须是在册版本",
                    target_version
                ));
                v
            }
            Some(e) => {
                // **独立复核生命周期**：以 F4203 的语义自行判定，
                // 不采信「注册册说它没废止」作为判据依据。
                if lifecycle_is_retired(e.lifecycle) {
                    let v = RollbackVerdict::TargetRetired(target_version.to_string());
                    self.open_case(contract_id, E_ROLLBACK_RETIRED, Severity::Blocking, v.zh(), &format!(
                        "改选一个未废止的基线版本；回滚到已废止版本等于复活契约，违反单向前进纪律"
                    ));
                    v
                } else {
                    RollbackVerdict::Ok { restored: target_version.to_string() }
                }
            }
        }
    }

    /// 立案（**容量触顶不静默**，见头注§八）。
    fn open_case(&mut self, contract_id: &str, code: &'static str, sev: Severity, symptom: String, advice: &str) {
        if self.cases.len() < MAX_CASES {
            self.cases.push(CaseRecord {
                contract_id: contract_id.to_string(),
                code,
                severity: sev,
                symptom,
                advice: advice.to_string(),
            });
            return;
        }
        let _ = self.diag.note(format!("立案溢出降级：{} {}", contract_id, code));
    }

    /// **催办**（重复**累加**不覆盖，见头注§五）。
    fn dun(&mut self, contract_id: &str, d: DomainTag) {
        for r in self.dunning.iter_mut() {
            if r.contract_id == contract_id && r.domain == d {
                r.notices += 1;
                return;
            }
        }
        if self.dunning.len() < MAX_DUNNING {
            self.dunning.push(DunningRecord {
                contract_id: contract_id.to_string(),
                domain: d,
                notices: 1,
            });
            return;
        }
        let _ = self.diag.note(format!("催办溢出降级：{} {}", contract_id, d.zh()));
    }

    /// **推进游标并留痕**（痕迹是动作级的，故拒与过都走这里）。
    fn mark_cursor(
        &mut self,
        contract_id: &str,
        step: FlowStep,
        actor: DomainTag,
        ok: bool,
        detail: &str,
    ) -> Result<(), ConsistencyError> {
        if ok {
            if let Some(f) = self.flow_mut(contract_id) {
                f.cursor = Some(step);
            }
        }
        let _ = self.push_trace(FlowTrace {
            contract_id: contract_id.to_string(),
            step,
            outcome: if ok { TraceOutcome::Advanced } else { TraceOutcome::Rejected },
            code: if ok { "" } else { E_STEP_STATE },
            actor,
            detail: detail.to_string(),
        });
        Ok(())
    }

    /// **作废 + 回退**（越步/越档共用，见头注§二、§六）。
    ///
    /// 作废是**标记**而非删除：把该步之前已走出的痕迹标成
    /// [`TraceOutcome::Voided`]，**行还在**——删了就查不到越权发生过。
    /// 回退则把游标退回该步之前，让申请人可以修正后重走。
    fn void_flow(&mut self, contract_id: &str, at: FlowStep, actor: DomainTag, code: &'static str, detail: &str) {
        let victim_ord = at.ordinal();
        for t in self.traces.iter_mut() {
            if t.contract_id == contract_id
                && t.step.ordinal() < victim_ord
                && t.outcome == TraceOutcome::Advanced
            {
                t.outcome = TraceOutcome::Voided;
                t.code = code;
                t.detail = format!("因 {} 处越步/越档而作废（原：{}）", at.zh(), t.detail);
            }
        }
        if let Some(f) = self.flow_mut(contract_id) {
            f.voided_from = Some(at);
            // 回退到该步**之前**：即退回上一步（无上一步则退回未启动）。
            f.cursor = match at.ordinal() {
                1 => None,
                n => FlowStep::from_ordinal(n - 1),
            };
        }
        let _ = self.push_trace(FlowTrace {
            contract_id: contract_id.to_string(),
            step: at,
            outcome: TraceOutcome::Rejected,
            code,
            actor,
            detail: detail.to_string(),
        });
    }

    /// 自审（**标准册自身须干净**）。
    pub fn self_audit(&self) -> Vec<String> {
        let mut issues: Vec<String> = Vec::new();
        if self.flows.len() > MAX_FLOWS {
            issues.push(format!("{}:流程册超容", E_CAP));
        }
        for f in self.flows.iter() {
            // 走完六步却没经过兼容闸 = 流程被绕过。
            if f.is_complete() && f.verdict.is_none() {
                issues.push(format!("{}:{} 全程走完却未过兼容闸", E_STEP_STATE, f.req.contract_id));
            }
            // 已切档却未过会签 = 会签被绕过。
            if f.canary.is_some() && !f.cosign.iter().all(|r| r.is_signed()) {
                issues.push(format!("{}:{} 未齐签却已进灰度", E_COSIGN_PENDING, f.req.contract_id));
            }
        }
        issues
    }

    /// 读屏总览。
    pub fn screen_text(&self) -> String {
        let mut s = String::new();
        for f in self.flows.iter() {
            s.push_str(&format!(
                "{}：游标 {}｜{}\n",
                f.req.contract_id,
                match f.cursor {
                    Some(c) => c.zh(),
                    None => "未启动",
                },
                match &f.verdict {
                    Some(v) => v.screen_line(),
                    None => "兼容闸未过".to_string(),
                }
            ));
            for r in f.cosign.iter() {
                s.push_str(&format!("  会签 {}\n", r.screen_line()));
            }
            if let Some(c) = f.canary {
                s.push_str(&format!("  灰度 {} 档\n", c.zh()));
            }
            if let Some(v) = &f.last_rollback {
                s.push_str(&format!("  {}\n", v.zh()));
            }
        }
        s.push_str("— 痕迹 —\n");
        for t in self.traces.iter() {
            s.push_str(&format!("{}\n", t.screen_line()));
        }
        if !self.cases.is_empty() {
            s.push_str("— 立案 —\n");
            for c in self.cases.iter() {
                s.push_str(&format!("{}\n", c.screen_line()));
            }
        }
        if !self.dunning.is_empty() {
            s.push_str("— 催办 —\n");
            for d in self.dunning.iter() {
                s.push_str(&format!("{}\n", d.screen_line()));
            }
        }
        s.push_str(&self.diag.screen_text());
        s
    }
}

// ---------------------------------------------------------------------------
// 十二、独立复核用的纯函数（判据侧不采信被测方的判断）
// ---------------------------------------------------------------------------

/// **生命周期是否已废止**（判据侧独立重算用，**公开**）。
///
/// 之所以不复用注册中心的谓词，是为了让判据能真正**独立**地重算这件事：
/// 若判据调的是被测方给的同一个谓词，那么「判据全绿」与「实现错了」
/// 会同时发生——两者同源，一个 bug 看不出来。
pub fn lifecycle_is_retired(l: Lifecycle) -> bool {
    match l {
        Lifecycle::Draft => false,
        Lifecycle::Registered => false,
        Lifecycle::Frozen => false,
        Lifecycle::Retired => true,
    }
}

/// 版本是否可在册回滚（**纯函数**：判据侧据此重算）。
pub fn version_recoverable(in_registry: bool, lifecycle: Lifecycle) -> bool {
    if !in_registry {
        return false;
    }
    !lifecycle_is_retired(lifecycle)
}

/// 取契约当前版本（**F4203 单源**；不在册则空串）。
pub fn current_version_of(reg: &ContractRegistry, id: &str) -> String {
    match reg.current(id) {
        Some(e) => e.meta.version.clone(),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// 十三、标准环境（判据语料）
// ---------------------------------------------------------------------------

/// 标准注册册（在 F4203 标准册上追加一条本项专用契约）。
pub fn standard_flow_registry() -> ContractRegistry {
    use super::veu03_registry::ContractMeta;
    let mut reg = super::veu03_registry::standard_registry();
    reg.register(ContractMeta::new(
        "U05-CTR-A11Y",
        "1.0.0",
        DomainTag::S,
        vec![DomainTag::T, DomainTag::U],
        "对比度不低于 4.5:1 且焦点可达；命中区域不小于 44px",
    ))
    .expect("标准契约注册");
    reg.add_consumer("U05-CTR-A11Y", "1.0.0", DomainTag::T)
        .expect("引用登记");
    reg.add_consumer("U05-CTR-A11Y", "1.0.0", DomainTag::U)
        .expect("引用登记");
    reg
}

/// 标准基线无障碍面（**五面齐、强度拉满**）。
pub fn standard_a11y_full() -> Vec<(A11yFacet, u8)> {
    vec![
        (A11yFacet::Contrast, 3),
        (A11yFacet::FocusReach, 3),
        (A11yFacet::FocusOrder, 3),
        (A11yFacet::ScreenLabel, 3),
        (A11yFacet::HitArea, 3),
    ]
}

/// 标准变更申请（**纯新增：不删字段、不改判据、无障碍面不弱化**）。
///
/// 这条语料存在的意义是给兼容闸一个**「应当放行」的基线**：
/// 只测阻断侧的话，一个「永远阻断」的闸也能全绿。
pub fn standard_additive_request() -> ChangeRequest {
    ChangeRequest::new(
        "U05-CTR-A11Y",
        "1.1.0",
        "对比度不低于 4.5:1 且焦点可达；命中区域不小于 44px",
        "对比度不低于 4.5:1 且焦点可达；命中区域不小于 44px",
        standard_a11y_full(),
        standard_a11y_full(),
    )
}