//! VE-F5801 · AI 域总架构（VE-AC 域 · AI 与行为 · 域开工第一单）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5801`
//!
//! 锚点原文：「AI 域总架构——四层架构（感知层、决策层、执行层、协作层）与
//! 三律（可预期：AI 行为对玩家透明一致、可解释：每个决策可回答为什么、
//! 可关闭：任何 AI 能力可降级可关闭），域色聪明得有分寸——AI 惊艳玩家但
//! 不欺骗玩家；承接 AB 域移交包（F5793 十件）。数据结构：四层架构册；AI
//! 三律；承接清单。错误路径与降级矩阵：层间越权→拒绝；三律违例→红线；
//! 承接缺件→向 AB 追补。性能：架构 O（层）。对接：AB 移交包；AC01 全组。
//! 判据：四层架构、AI 三律、可预期可解释可关闭、判据。」
//!
//! # 一、四层架构：层是**责任互斥的管道级**，不是「四个模块」
//!
//! 感知层（Perception）把世界变成 AI 看到的模型，决策层（Deliberation）
//! 把模型变成选择，执行层（Action）把选择变成行动，协作层（Collaboration）
//! 把行动变成多实体间的协同。四层责任互斥：每层持有**独占类别**
//! （[`LayerKind::exclusive_class`]），跨类产出即越界——这继承 Z 域
//! F5201 的归属判据纪律：越界不靠自觉，靠注册时的类别核对。层与层之间
//! 只有一条合法流向：感知→决策→执行→协作（[`Pipeline`] 单源表驱动）。
//! 感知层直接向执行层投递是**跳层**（[`E_AC1_PIPE_BROKEN`]），执行层反向
//! 指挥决策层是**越权**（[`E_AC1_LAYER_OVERREACH`]）——两种病分开诊断，
//! 混码会把「管道坏了」读成「有人越权」。
//!
//! # 二、AI 三律：律不是文档，是**注册面的红线闸**
//!
//! 可预期（Predictable）：AI 行为对玩家透明一致——玩家能预判 AI 会做
//! 什么，同一处境同一行为。可解释（Explainable）：每个决策可回答为什么
//! ——不是事后补理由，是决策时**必须携带**解释通道。可关闭（Closable）：
//! 任何 AI 能力可降级可关闭——降级链 Full→Half→Off 单调只降不升，升档
//! 即红线。三律违例在**注册面**就拦截（[`E_AC1_LAW_REDLINE`]）：缺任何
//! 一律申报的能力根本进不了架构册，而不是「上线后再改」。三律声明读屏
//! 可达：[`AicCore::a11y_statement`] 产出非空读屏文本，空白即
//! [`E_AC1_A11Y_BLANK`]——读屏可达是数据契约不是态度。
//!
//! # 三、承接 AB 移交包：缺件**显性追补**，不静默过关
//!
//! AB 域 F5793 移交十件（音频事件通道/调试钩子/指标口径/场景覆盖/三纪律/
//! fuzz 语料/性能册/安全隐私/文档总纲/看板链接）逐件登记核验
//! （[`HandoverManifest`]）。缺件不静默：缺哪件记哪件，追补方向写死
//! 「向 AB」（[`E_AC1_HANDOVER_MISSING`]），十件全到齐承接才算闭环——
//! 「大概移交了」等于没移交。
//!
//! # 四、域色：聪明得有分寸，落在**欺骗模式闭集**上
//!
//! 「AI 惊艳玩家但不欺骗玩家」不是 slogan，是四种欺骗模式的拒绝表
//! （[`DeceitPattern`]）：伪装玩家、隐瞒 AI 身份、伪造运气、暗中改玩家
//! 可见状态。命中任一即 [`E_AC1_DECEIT`] 拒绝——分寸感靠拒绝码执行，
//! 不靠评审自觉。
//!
//! # 五、性能：架构 O（层）
//!
//! 层是常数四，注册/投递/裁决全路径 O（层）=O(4) 常数；承接核验
//! O（件）=O(10) 常数。无动态分配增长路径，无 f32，无 libm。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误契约：独占 0x59 细分段（0x57=veab03 / 0x58=vcq02 均已有主）
// ---------------------------------------------------------------------------

/// 层间越权：逆向或跨类指挥（执行层指挥决策层等）。
pub const E_AC1_LAYER_OVERREACH: u16 = 0x5900;
/// 管道破坏：跳层投递（感知直接投执行）——层序是架构不是建议。
pub const E_AC1_PIPE_BROKEN: u16 = 0x5901;
/// 三律违例红线：缺律申报 / 降级升档 / 决策缺解释通道。
pub const E_AC1_LAW_REDLINE: u16 = 0x5902;
/// 层未声明能力边界即注册（UndeclaredSemantic 同构）。
pub const E_AC1_UNDECLARED_LAYER: u16 = 0x5903;
/// 重复注册：同层只能有一个现役规格。
pub const E_AC1_DUPLICATE_LAYER: u16 = 0x5904;
/// 承接缺件：十件未到齐，追补方向=向 AB 域。
pub const E_AC1_HANDOVER_MISSING: u16 = 0x5905;
/// 域色违例：命中欺骗模式闭集（惊艳可以，欺骗不行）。
pub const E_AC1_DECEIT: u16 = 0x5906;
/// 读屏声明空白：三律声明读屏可达是数据契约。
pub const E_AC1_A11Y_BLANK: u16 = 0x5907;
/// 层预算非法：零预算或超总预算上限。
pub const E_AC1_BUDGET: u16 = 0x5908;
/// 架构冻结后改语义：走 unfreeze，直接改即越权。
pub const E_AC1_AMEND_FROZEN: u16 = 0x5909;

/// 错误码总数（判据侧独立复核）。
pub const ERROR_COUNT: usize = 10;

/// 错误码→原因串（三要素呈现：码、因、向）。
pub const fn reason_of(code: u16) -> &'static str {
    match code {
        E_AC1_LAYER_OVERREACH => "层间越权：逆向或跨类指挥被拒",
        E_AC1_PIPE_BROKEN => "管道破坏：跳层投递被拒",
        E_AC1_LAW_REDLINE => "三律违例：红线级，能力不得入册",
        E_AC1_UNDECLARED_LAYER => "层未声明能力边界即注册",
        E_AC1_DUPLICATE_LAYER => "同层重复注册",
        E_AC1_HANDOVER_MISSING => "承接缺件：向 AB 域追补",
        E_AC1_DECEIT => "域色违例：欺骗模式命中",
        E_AC1_A11Y_BLANK => "读屏声明空白",
        E_AC1_BUDGET => "层预算非法",
        E_AC1_AMEND_FROZEN => "架构冻结后改语义被拒",
        _ => "未知错误码",
    }
}

/// 错误码→处置方向（降级矩阵的「向」列）。
pub const fn direction_of(code: u16) -> &'static str {
    match code {
        E_AC1_LAYER_OVERREACH | E_AC1_PIPE_BROKEN => "按层序表重排投递路径",
        E_AC1_LAW_REDLINE => "补齐三律申报或撤下能力",
        E_AC1_UNDECLARED_LAYER => "补能力边界声明后重新注册",
        E_AC1_DUPLICATE_LAYER => "先注销现役规格再注册",
        E_AC1_HANDOVER_MISSING => "向 AB 域追补缺件",
        E_AC1_DECEIT => "撤销欺骗性设计",
        E_AC1_A11Y_BLANK => "补读屏声明文本",
        E_AC1_BUDGET => "按总预算上限重排层预算",
        E_AC1_AMEND_FROZEN => "先走 unfreeze 再改",
        _ => reason_of(code),
    }
}

// ---------------------------------------------------------------------------
// 四层架构：LayerKind
// ---------------------------------------------------------------------------

/// 四层封闭枚举。层序=序数序：感知(0)→决策(1)→执行(2)→协作(3)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerKind {
    /// 感知层：世界→AI 看到的模型（承接 AB 音频事件、E 域视觉）。
    Perception,
    /// 决策层：模型→选择（预算与解释通道在此层）。
    Deliberation,
    /// 执行层：选择→行动。
    Action,
    /// 协作层：行动→多实体协同。
    Collaboration,
}

pub use LayerKind::*;

/// 层数（判据侧独立写死 4）。
pub const LAYER_COUNT: usize = 4;

impl LayerKind {
    /// 全集（判据侧序数对账用）。
    pub const ALL: [LayerKind; LAYER_COUNT] = [
        LayerKind::Perception,
        LayerKind::Deliberation,
        LayerKind::Action,
        LayerKind::Collaboration,
    ];

    /// 层名（读屏与快照共用单源）。
    pub const fn name(self) -> &'static str {
        match self {
            LayerKind::Perception => "感知层",
            LayerKind::Deliberation => "决策层",
            LayerKind::Action => "执行层",
            LayerKind::Collaboration => "协作层",
        }
    }

    /// 序数=层序（0..4）。
    pub const fn ordinal(self) -> usize {
        match self {
            LayerKind::Perception => 0,
            LayerKind::Deliberation => 1,
            LayerKind::Action => 2,
            LayerKind::Collaboration => 3,
        }
    }

    /// 独占类别：责任互斥的核对锚——每层只产一类产出。
    pub const fn exclusive_class(self) -> &'static str {
        match self {
            LayerKind::Perception => "感知类产出",
            LayerKind::Deliberation => "决策类产出",
            LayerKind::Action => "行动类产出",
            LayerKind::Collaboration => "协同类产出",
        }
    }

    /// 唯一合法下游（层序单源）：协作层是终点无下游。
    pub const fn downstream(self) -> Option<LayerKind> {
        match self {
            LayerKind::Perception => Some(LayerKind::Deliberation),
            LayerKind::Deliberation => Some(LayerKind::Action),
            LayerKind::Action => Some(LayerKind::Collaboration),
            LayerKind::Collaboration => None,
        }
    }

    /// 唯一合法上游：感知层是源头无上游。
    pub const fn upstream(self) -> Option<LayerKind> {
        match self {
            LayerKind::Perception => None,
            LayerKind::Deliberation => Some(LayerKind::Perception),
            LayerKind::Action => Some(LayerKind::Deliberation),
            LayerKind::Collaboration => Some(LayerKind::Action),
        }
    }

    /// 层预算基准（千分预算单位 µs）：越靠决策越贵——决策是思考所在层。
    pub const fn budget_floor_us(self) -> u32 {
        match self {
            LayerKind::Perception => 80,
            LayerKind::Deliberation => 200,
            LayerKind::Action => 60,
            LayerKind::Collaboration => 60,
        }
    }
}

// ---------------------------------------------------------------------------
// 能力边界声明：注册面第一闸
// ---------------------------------------------------------------------------

/// 能力标签闭集：层注册必须声明自己**做什么**——不声明不入册。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    /// 看见/听见/收到事件（感知类）。
    Sense,
    /// 选择与规划（决策类）。
    Decide,
    /// 行动与施放（执行类）。
    Act,
    /// 广播/协商/编队（协作类）。
    Cooperate,
    /// 降级执行（三律-关闭律的执行面，任何层可声明）。
    Degrade,
    /// 解释通道（三律-解释律的执行面，任何层可声明）。
    Explain,
    /// 行为一致指纹（三律-预期律的执行面，任何层可声明）。
    Predict,
}

/// 能力标签总数（判据侧独立复核）。
pub const CAPABILITY_COUNT: usize = 7;

impl Capability {
    /// 全集。
    pub const ALL: [Capability; CAPABILITY_COUNT] = [
        Capability::Sense,
        Capability::Decide,
        Capability::Act,
        Capability::Cooperate,
        Capability::Degrade,
        Capability::Explain,
        Capability::Predict,
    ];

    /// 能力名（读屏与审计共用）。
    pub const fn name(self) -> &'static str {
        match self {
            Capability::Sense => "感知",
            Capability::Decide => "决策",
            Capability::Act => "行动",
            Capability::Cooperate => "协同",
            Capability::Degrade => "降级",
            Capability::Explain => "解释",
            Capability::Predict => "一致",
        }
    }

    /// 能力归属类别：与层独占类别核对——跨类能力挂在异类层上即越界。
    pub const fn class(self) -> &'static str {
        match self {
            Capability::Sense => "感知类产出",
            Capability::Decide => "决策类产出",
            Capability::Act => "行动类产出",
            Capability::Cooperate => "协同类产出",
            Capability::Degrade | Capability::Explain | Capability::Predict => "三律共通",
        }
    }
}

// ---------------------------------------------------------------------------
// AI 三律：律是类型不是文档
// ---------------------------------------------------------------------------

/// 三律封闭枚举：可预期 / 可解释 / 可关闭。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Law {
    /// 可预期：AI 行为对玩家透明一致。
    Predictable,
    /// 可解释：每个决策可回答为什么。
    Explainable,
    /// 可关闭：任何 AI 能力可降级可关闭。
    Closable,
}

/// 三律数（判据侧独立写死 3）。
pub const LAW_COUNT: usize = 3;

impl Law {
    /// 全集。
    pub const ALL: [Law; LAW_COUNT] = [
        Law::Predictable,
        Law::Explainable,
        Law::Closable,
    ];

    /// 律名（读屏单源）。
    pub const fn name(self) -> &'static str {
        match self {
            Law::Predictable => "可预期",
            Law::Explainable => "可解释",
            Law::Closable => "可关闭",
        }
    }

    /// 律的注册面声明字段名（申报缺哪律一眼可查）。
    pub const fn spec_field(self) -> &'static str {
        match self {
            Law::Predictable => "predictable",
            Law::Explainable => "explainable",
            Law::Closable => "closable",
        }
    }

    /// 律对应的执行面能力：预期律↔Predict、解释律↔Explain、关闭律↔Degrade。
    pub const fn executor(self) -> Capability {
        match self {
            Law::Predictable => Capability::Predict,
            Law::Explainable => Capability::Explain,
            Law::Closable => Capability::Degrade,
        }
    }
}

/// 降级档位：关闭律的执行面。Full→Half→Off 单调只降不升。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Degrade {
    /// 全能力。
    Full,
    /// 半能力（预算减半、范围收窄）。
    Half,
    /// 关闭（能力完全下线，行为退回无 AI 基线）。
    Off,
}

impl Degrade {
    /// 下一档（更关闭的方向）。Off 已到底返回自身。
    pub const fn step_down(self) -> Degrade {
        match self {
            Degrade::Full => Degrade::Half,
            Degrade::Half | Degrade::Off => Degrade::Off,
        }
    }

    /// 档位名（读屏单源）。
    pub const fn name(self) -> &'static str {
        match self {
            Degrade::Full => "全能力",
            Degrade::Half => "半能力",
            Degrade::Off => "关闭",
        }
    }
}

// ---------------------------------------------------------------------------
// 层规格与注册制：三律红线在注册面
// ---------------------------------------------------------------------------

/// 层注册规格：注册申报即承诺——承诺缺一律，红线。
#[derive(Clone, Debug)]
pub struct LayerSpec {
    /// 层种类。
    pub kind: LayerKind,
    /// 声明的能力边界（闭集内；空=未声明，拒）。
    pub caps: Vec<Capability>,
    /// 层预算 µs（须 ≥ 层基准且 ≤ 总预算上限）。
    pub budget_us: u32,
    /// 可预期申报（律一）。
    pub predictable: bool,
    /// 可解释申报（律二）。
    pub explainable: bool,
    /// 可关闭申报（律三）。
    pub closable: bool,
}

/// 架构总预算上限 µs（四层合计不得超过——配额是缰绳的域级先声）。
pub const TOTAL_BUDGET_US: u32 = 400;

/// 注册结果：Ok=入册，Err=专属错误码。
pub type RegResult = Result<(), u16>;

/// 四层架构册：每层至多一个现役规格。
#[derive(Clone, Debug)]
pub struct LayerRegistry {
    slots: [Option<LayerSpec>; LAYER_COUNT],
    frozen: bool,
    registered_mask: u8,
}

impl Default for LayerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LayerRegistry {
    /// 空册。
    pub const fn new() -> Self {
        LayerRegistry {
            slots: [None, None, None, None],
            frozen: false,
            registered_mask: 0,
        }
    }    /// 注册一层。四闸依次：未声明能力→0x5903；类别越界→0x5903 同源
    /// （能力类别与层独占类别不符即未按层申报）；三律缺一→0x5902 红线；
    /// 预算非法→0x5908；重复→0x5904；冻结后新增→0x5909。
    pub fn register(&mut self, spec: LayerSpec) -> RegResult {
        if self.frozen {
            return Err(E_AC1_AMEND_FROZEN);
        }
        if spec.caps.is_empty() {
            return Err(E_AC1_UNDECLARED_LAYER);
        }
        // 类别核对：层可挂「本类」能力或「三律共通」能力，跨类即越界。
        let layer_class = spec.kind.exclusive_class();
        for c in &spec.caps {
            let cap_class = c.class();
            if cap_class != layer_class && cap_class != "三律共通" {
                return Err(E_AC1_UNDECLARED_LAYER);
            }
        }
        // 三律红线：缺一律即拒（注册面拦截，不上线后再改）。
        if !(spec.predictable && spec.explainable && spec.closable) {
            return Err(E_AC1_LAW_REDLINE);
        }
        if spec.budget_us < spec.kind.budget_floor_us() || spec.budget_us > TOTAL_BUDGET_US {
            return Err(E_AC1_BUDGET);
        }
        let idx = spec.kind.ordinal();
        if self.slots[idx].is_some() {
            return Err(E_AC1_DUPLICATE_LAYER);
        }
        // 总预算守恒：新层并入后四层合计不得超上限。
        let mut sum = spec.budget_us;
        for (i, slot) in self.slots.iter().enumerate() {
            if i != idx {
                if let Some(s) = slot {
                    sum = sum.saturating_add(s.budget_us);
                }
            }
        }
        if sum > TOTAL_BUDGET_US {
            return Err(E_AC1_BUDGET);
        }
        self.slots[idx] = Some(spec);
        self.registered_mask |= 1 << idx;
        Ok(())
    }

    /// 注销一层（重注册前置步）。
    pub fn unregister(&mut self, kind: LayerKind) -> bool {
        if self.frozen {
            return false;
        }
        let idx = kind.ordinal();
        if self.slots[idx].take().is_some() {
            self.registered_mask &= !(1 << idx);
            true
        } else {
            false
        }
    }

    /// 查层规格。
    pub fn layer(&self, kind: LayerKind) -> Option<&LayerSpec> {
        self.slots[kind.ordinal()].as_ref()
    }

    /// 已注册层数。
    pub fn len(&self) -> usize {
        self.registered_mask.count_ones() as usize
    }

    /// 空册判据。
    pub fn is_empty(&self) -> bool {
        self.registered_mask == 0
    }

    /// 四层齐备：管道通的必要条件。
    pub fn complete(&self) -> bool {
        self.registered_mask == 0x0F
    }

    /// 冻结（只能加不能改语义的前提）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 解冻（改语义先走 unfreeze）。
    pub fn unfreeze(&mut self) {
        self.frozen = false;
    }

    /// 冻结态查询。
    pub const fn frozen(&self) -> bool {
        self.frozen
    }

    /// 已注册层种类快照（按层序）。
    pub fn kinds(&self) -> Vec<LayerKind> {
        LayerKind::ALL
            .iter()
            .copied()
            .filter(|k| self.layer(*k).is_some())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 四层管道：层序是数据不是约定
// ---------------------------------------------------------------------------

/// 投递结果统计：合法投递与两类拒绝分开计数——混计会把病读错。
#[derive(Clone, Copy, Debug, Default)]
pub struct PipeStats {
    /// 合法投递（相邻层顺流）次数。
    pub forwarded: u32,
    /// 跳层拒绝次数（感知→执行等跨层顺流）。
    pub skipped: u32,
    /// 逆向拒绝次数（下游指挥上游）。
    pub reversed: u32,
}

/// 四层管道：只许沿 [`LayerKind::downstream`] 单源表顺流一步。
#[derive(Clone, Debug, Default)]
pub struct Pipeline {
    stats: PipeStats,
}

impl Pipeline {
    /// 空管道。
    pub const fn new() -> Self {
        Pipeline { stats: PipeStats { forwarded: 0, skipped: 0, reversed: 0 } }
    }

    /// 投递一条消息：from→to。
    /// 相邻顺流=放行；同层=no-op 放行（层内传递）；跨层顺流=跳层拒；
    /// 逆向=越权拒。O(层)=O(1)。
    pub fn submit(&mut self, from: LayerKind, to: LayerKind) -> RegResult {
        if from == to {
            self.stats.forwarded += 1;
            return Ok(());
        }
        if to.ordinal() < from.ordinal() {
            self.stats.reversed += 1;
            return Err(E_AC1_LAYER_OVERREACH);
        }
        match from.downstream() {
            Some(next) if next == to => {
                self.stats.forwarded += 1;
                Ok(())
            }
            _ => {
                self.stats.skipped += 1;
                Err(E_AC1_PIPE_BROKEN)
            }
        }
    }

    /// 统计快照。
    pub const fn stats(&self) -> &PipeStats {
        &self.stats
    }

    /// 合法流全通自检：四层相邻三跳全放行。
    pub fn self_test_flow(&mut self) -> bool {
        let mut ok = true;
        ok &= self.submit(Perception, Deliberation).is_ok();
        ok &= self.submit(Deliberation, Action).is_ok();
        ok &= self.submit(Action, Collaboration).is_ok();
        ok
    }
}

// ---------------------------------------------------------------------------
// 承接清单：AB 域 F5793 十件
// ---------------------------------------------------------------------------

/// 移交件闭集：十件定名（承接缺件可点名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandoverItem {
    /// ① 音频事件通道（AB 事件→感知层输入）。
    AudioEventChannel,
    /// ② 音频调试钩子（AB 调试→AI 调试联动）。
    AudioDebugHooks,
    /// ③ 指标口径册（AB 指标定义→AI 遥测对齐）。
    MetricSpecBook,
    /// ④ 场景覆盖表（AB 场景→AI 测试复用）。
    SceneCoverageTable,
    /// ⑤ 三纪律承诺（AB 纪律→AC 延续）。
    DisciplinePledge,
    /// ⑥ fuzz 语料袋（AB 语料→AI fuzz 复用）。
    FuzzCorpusBag,
    /// ⑦ 性能册（AB 预算口径→AI 配额对齐）。
    PerformanceBook,
    /// ⑧ 安全隐私口径（AB 口径→AI 安全对齐）。
    SafetyPrivacySpec,
    /// ⑨ 文档总纲（AB 文档→AI 文档同源）。
    DocMasterPlan,
    /// ⑩ 看板链接（AB 看板→AC 看板续接）。
    BoardLink,
}

/// 移交件数（判据侧独立写死 10）。
pub const HANDOVER_COUNT: usize = 10;

impl HandoverItem {
    /// 全集（序数稳定，判据侧逐件对账用）。
    pub const ALL: [HandoverItem; HANDOVER_COUNT] = [
        HandoverItem::AudioEventChannel,
        HandoverItem::AudioDebugHooks,
        HandoverItem::MetricSpecBook,
        HandoverItem::SceneCoverageTable,
        HandoverItem::DisciplinePledge,
        HandoverItem::FuzzCorpusBag,
        HandoverItem::PerformanceBook,
        HandoverItem::SafetyPrivacySpec,
        HandoverItem::DocMasterPlan,
        HandoverItem::BoardLink,
    ];

    /// 件名（追补单与读屏共用）。
    pub const fn name(self) -> &'static str {
        match self {
            HandoverItem::AudioEventChannel => "音频事件通道",
            HandoverItem::AudioDebugHooks => "音频调试钩子",
            HandoverItem::MetricSpecBook => "指标口径册",
            HandoverItem::SceneCoverageTable => "场景覆盖表",
            HandoverItem::DisciplinePledge => "三纪律承诺",
            HandoverItem::FuzzCorpusBag => "fuzz 语料袋",
            HandoverItem::PerformanceBook => "性能册",
            HandoverItem::SafetyPrivacySpec => "安全隐私口径",
            HandoverItem::DocMasterPlan => "文档总纲",
            HandoverItem::BoardLink => "看板链接",
        }
    }

    /// 序数。
    pub const fn ordinal(self) -> usize {
        match self {
            HandoverItem::AudioEventChannel => 0,
            HandoverItem::AudioDebugHooks => 1,
            HandoverItem::MetricSpecBook => 2,
            HandoverItem::SceneCoverageTable => 3,
            HandoverItem::DisciplinePledge => 4,
            HandoverItem::FuzzCorpusBag => 5,
            HandoverItem::PerformanceBook => 6,
            HandoverItem::SafetyPrivacySpec => 7,
            HandoverItem::DocMasterPlan => 8,
            HandoverItem::BoardLink => 9,
        }
    }
}

/// 承接清单：十件逐件核验，缺件显性追补。
#[derive(Clone, Debug)]
pub struct HandoverManifest {
    arrived: [bool; HANDOVER_COUNT],
    /// 累计追补请求数（每次缺件核验 +1——「大概移交了」会在这里现形）。
    pub chase_requests: u32,
}

impl Default for HandoverManifest {
    fn default() -> Self {
        Self::new()
    }
}

impl HandoverManifest {
    /// 空清单。
    pub const fn new() -> Self {
        HandoverManifest { arrived: [false; HANDOVER_COUNT], chase_requests: 0 }
    }

    /// 登记一件到达。
    pub fn admit(&mut self, item: HandoverItem) {
        self.arrived[item.ordinal()] = true;
    }

    /// 承接闭环核验：十件全到→Ok；否则 Err(0x5905) 并返回缺件名单
    /// （缺哪件点名哪件，追补方向写死向 AB）。
    pub fn verify(&self) -> Result<(), (u16, Vec<HandoverItem>)> {
        let mut missing = Vec::new();
        for item in HandoverItem::ALL {
            if !self.arrived[item.ordinal()] {
                missing.push(item);
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err((E_AC1_HANDOVER_MISSING, missing))
        }
    }

    /// 已到件数。
    pub fn arrived_count(&self) -> usize {
        self.arrived.iter().filter(|a| **a).count()
    }

    /// 追补请求发出（域开工主流程在 verify 失败后显式调用——不静默）。
    pub fn chase(&mut self) {
        self.chase_requests += 1;
    }
}

// ---------------------------------------------------------------------------
// 域色：聪明得有分寸——欺骗模式闭集
// ---------------------------------------------------------------------------

/// 欺骗模式闭集：命中任一即拒绝（惊艳可以，欺骗不行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeceitPattern {
    /// 伪装成玩家（AI 冒充人类玩家行为）。
    ImpersonatePlayer,
    /// 隐瞒 AI 身份（该亮明时不亮明）。
    ConcealAi,
    /// 伪造运气（把脚本安排包装成随机运气）。
    FakeLuck,
    /// 暗中修改玩家可见状态（不通过玩家可见通道的状态变更）。
    HiddenMutate,
}

/// 欺骗模式数（判据侧独立写死 4）。
pub const DECEIT_COUNT: usize = 4;

impl DeceitPattern {
    /// 全集。
    pub const ALL: [DeceitPattern; DECEIT_COUNT] = [
        DeceitPattern::ImpersonatePlayer,
        DeceitPattern::ConcealAi,
        DeceitPattern::FakeLuck,
        DeceitPattern::HiddenMutate,
    ];

    /// 模式名（审计与读屏共用）。
    pub const fn name(self) -> &'static str {
        match self {
            DeceitPattern::ImpersonatePlayer => "伪装玩家",
            DeceitPattern::ConcealAi => "隐瞒AI身份",
            DeceitPattern::FakeLuck => "伪造运气",
            DeceitPattern::HiddenMutate => "暗改玩家可见状态",
        }
    }
}

/// 域色闸：智能行为方案过闸——命中欺骗模式闭集即拒。
#[derive(Clone, Debug, Default)]
pub struct DomainColorGate {
    /// 拦截计数（按模式分账）。
    pub rejections: [u32; DECEIT_COUNT],
}

impl DomainColorGate {
    /// 方案过闸：`flags` 是方案命中的模式集合。命中任一→Err(0x5906)；
    /// 空集=分寸内放行。
    pub fn screen(&mut self, flags: &[DeceitPattern]) -> RegResult {
        let mut hit: Option<DeceitPattern> = None;
        for f in flags {
            for (i, known) in DeceitPattern::ALL.iter().enumerate() {
                if f == known {
                    self.rejections[i] += 1;
                    if hit.is_none() {
                        hit = Some(*f);
                    }
                }
            }
        }
        match hit {
            Some(_) => Err(E_AC1_DECEIT),
            None => Ok(()),
        }
    }

    /// 拦截总数。
    pub fn total_rejections(&self) -> u32 {
        self.rejections.iter().sum()
    }
}

// ---------------------------------------------------------------------------
// 三律执行面：降级链与裁决
// ---------------------------------------------------------------------------

/// 能力降级账：每能力一份当前档位，只降不升。
#[derive(Clone, Debug)]
pub struct DegradeLedger {
    entries: Vec<(Capability, Degrade)>,
    /// 越档升降拦截计数（升档即红线）。
    pub escalations: u32,
}

impl Default for DegradeLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl DegradeLedger {
    /// 空账。
    pub fn new() -> Self {
        DegradeLedger { entries: Vec::new(), escalations: 0 }
    }

    fn idx_of(&self, cap: Capability) -> Option<usize> {
        self.entries.iter().position(|(c, _)| *c == cap)
    }

    /// 挂账：新能力从 Full 起账（能力存在即须可关闭）。
    pub fn enlist(&mut self, cap: Capability) {
        if self.idx_of(cap).is_none() {
            self.entries.push((cap, Degrade::Full));
        }
    }

    /// 降一档：Full→Half→Off。未挂账能力拒绝（降级面与注册面同源）。
    pub fn step_down(&mut self, cap: Capability) -> RegResult {
        match self.idx_of(cap) {
            None => Err(E_AC1_UNDECLARED_LAYER),
            Some(i) => {
                let next = self.entries[i].1.step_down();
                self.entries[i].1 = next;
                Ok(())
            }
        }
    }

    /// 升档尝试：一律拒绝（关闭律单调性=红线）。
    pub fn escalate(&mut self, cap: Capability) -> RegResult {
        if self.idx_of(cap).is_none() {
            return Err(E_AC1_UNDECLARED_LAYER);
        }
        self.escalations += 1;
        Err(E_AC1_LAW_REDLINE)
    }

    /// 当前档位。
    pub fn level_of(&self, cap: Capability) -> Option<Degrade> {
        self.entries.iter().find(|(c, _)| *c == cap).map(|(_, d)| *d)
    }

    /// 已挂账能力数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判据。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// AicCore 主门面：总装与架构快照
// ---------------------------------------------------------------------------

/// AI 域架构快照：五面聚合一份，供判据层逐面对账。
#[derive(Clone, Debug)]
pub struct AicSnapshot {
    /// 已注册层名（层序）。
    pub layers: Vec<&'static str>,
    /// 四层齐备。
    pub pipeline_complete: bool,
    /// 管道统计。
    pub pipe: PipeStats,
    /// 已到移交件数。
    pub handover_arrived: usize,
    /// 承接闭环。
    pub handover_closed: bool,
    /// 欺骗拦截总数。
    pub deceit_rejections: u32,
    /// 已挂账降级能力数。
    pub degrade_entries: usize,
}

/// AI 域总架构门面：册 + 管道 + 承接 + 域色 + 降级账总装。
#[derive(Clone, Debug, Default)]
pub struct AicCore {
    pub registry: LayerRegistry,
    pub pipeline: Pipeline,
    pub handover: HandoverManifest,
    pub color: DomainColorGate,
    pub degrade: DegradeLedger,
}

impl AicCore {
    /// 新门面。
    pub fn new() -> Self {
        AicCore::default()
    }

    /// 注册一层并挂三律执行面能力账（注册成功即把 Predict/Explain/
    /// Degrade 三执行面入降级账——三律不是申报完就完，执行面必须存在）。
    pub fn register_layer(&mut self, spec: LayerSpec) -> RegResult {
        self.registry.register(spec)?;
        for law in Law::ALL {
            self.degrade.enlist(law.executor());
        }
        Ok(())
    }

    /// 承接闭环核验（缺件时显性发追补请求再返回 Err）。
    pub fn verify_handover(&mut self) -> RegResult {
        match self.handover.verify() {
            Ok(()) => Ok(()),
            Err((code, missing)) => {
                self.handover.chase();
                let _ = missing;
                Err(code)
            }
        }
    }

    /// 三律声明读屏文本：逐律一行，非空即达（空白→0x5907）。
    pub fn a11y_statement(&self) -> Result<String, u16> {
        let mut out = String::new();
        for law in Law::ALL {
            out.push_str(law.name());
            out.push_str("：");
            out.push_str(match law {
                Law::Predictable => "AI 行为对玩家透明一致，同一处境同一行为。",
                Law::Explainable => "每个决策可回答为什么，解释通道随决策存在。",
                Law::Closable => "任何 AI 能力可降级可关闭，降级链只降不升。",
            });
            out.push('\n');
        }
        if out.trim().is_empty() {
            return Err(E_AC1_A11Y_BLANK);
        }
        Ok(out)
    }

    /// 架构快照（五面聚合）。
    pub fn snapshot(&self) -> AicSnapshot {
        AicSnapshot {
            layers: self.registry.kinds().iter().map(|k| k.name()).collect(),
            pipeline_complete: self.registry.complete(),
            pipe: *self.pipeline.stats(),
            handover_arrived: self.handover.arrived_count(),
            handover_closed: self.handover.verify().is_ok(),
            deceit_rejections: self.color.total_rejections(),
            degrade_entries: self.degrade.len(),
        }
    }

    /// 架构健康自检：四层齐 + 管道自通 + 承接闭环 + 降级账覆盖三执行面。
    pub fn healthy(&mut self) -> bool {
        if !self.registry.complete() {
            return false;
        }
        // 管道自检会推进统计——用副本试，不污染现役统计。
        let mut probe = self.pipeline.clone();
        if !probe.self_test_flow() {
            return false;
        }
        if self.handover.verify().is_err() {
            return false;
        }
        for law in Law::ALL {
            if self.degrade.level_of(law.executor()).is_none() {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 内嵌确定性微验证（供 checks 层直接消费的原子事实）
// ---------------------------------------------------------------------------

/// 层序表自检：downstream/upstream 互逆且序数单调。
pub fn layer_order_invariant() -> bool {
    for k in LayerKind::ALL {
        let (d, u) = (k.downstream(), k.upstream());
        // 恰协作层无下游、恰感知层无上游（终点与源头各唯一）。
        if d.is_none() && k != LayerKind::Collaboration {
            return false;
        }
        if u.is_none() && k != LayerKind::Perception {
            return false;
        }
        // 双缺不可能：每层至少有上游或下游之一（否则层孤立于管道外）。
        if d.is_none() && u.is_none() {
            return false;
        }
        // 下游序数严格递增、上游序数严格递减（层序是全序）。
        if let Some(x) = d {
            if x.ordinal() <= k.ordinal() {
                return false;
            }
        }
        if let Some(x) = u {
            if x.ordinal() >= k.ordinal() {
                return false;
            }
        }
    }
    true
}

/// 错误码互异自检：十码两两不同且全落 0x59 段。
pub fn error_codes_disjoint() -> bool {
    let codes = [
        E_AC1_LAYER_OVERREACH,
        E_AC1_PIPE_BROKEN,
        E_AC1_LAW_REDLINE,
        E_AC1_UNDECLARED_LAYER,
        E_AC1_DUPLICATE_LAYER,
        E_AC1_HANDOVER_MISSING,
        E_AC1_DECEIT,
        E_AC1_A11Y_BLANK,
        E_AC1_BUDGET,
        E_AC1_AMEND_FROZEN,
    ];
    if codes.len() != ERROR_COUNT {
        return false;
    }
    for i in 0..codes.len() {
        if codes[i] >> 8 != 0x59 {
            return false;
        }
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                return false;
            }
        }
    }
    true
}
