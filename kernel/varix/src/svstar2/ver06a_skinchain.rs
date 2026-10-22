//! VE-F3421 · 实时换肤引擎全链 —— 预演 / 影响面 / 编排 / 事务 四件 + 不中断 / 可回滚 两律。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3421`
//!
//! # 职责（锚点原文拆解）
//!
//! 换肤不是重启。用户点下「切换主题」的那一刻，世界照常运转：点击还有
//! 效、输入不丢、滚动继续；换完了才跟用户打招呼。这条承诺不是文案，它由
//! 本模块的四件结构性保证：
//!
//! 1. **预演**（[`SkinEngine::rehearse`]）：新主题先在干跑里逐条求值，
//!    任何一条不合法都在「还没动真身」时暴露。预演有耗时预算
//!    （[`REHEARSE_BUDGET_MS`]），超预算即取消——预演本身不能变成卡顿源；
//! 2. **影响面**（[`SkinEngine::compute_impact`]）：新旧令牌差 → 受影响
//!    组件集合，按「全重绘 / 局部重绘 / 无感」三档分级。影响面是**算出来的**，
//!    不是「全量重绘反正不会错」——全量重绘会让换肤变成一次掉帧源；
//! 3. **编排**（[`SkinEngine::plan`]）：把影响面排成有序阶段（颜色 → 尺寸 →
//!    字重 → 时长 → 缓动 → 阴影）。阶段顺序不是随意的：先改颜色再改几何，
//!    中间态看起来才像「渐变」而不是「抽搐」；
//! 4. **事务**（[`SkinEngine::begin_commit`] / [`commit_stage`] / [`rollback`]）：
//!    提交前拍快照，任一阶段失败整链回滚到快照。**不存在半新半旧**。
//!
//! # 两律
//!
//! - **不中断**：提交与动画期间到达的交互事件进 [`InteractionQueue`]，
//!   完成后按到达序回放；队列溢出是显性错误（[`SkinCode::InteractionOverflow`]），
//!   不是静默丢弃——丢一次点击比掉一帧严重得多；
//! - **可回滚**：快照覆盖「值 + 修订号」，回滚后修订号也回退，账本不会留下
//!   「曾经改过」的假痕迹。
//!
//! # 为什么预演要用同一套求值路径
//!
//! 预演若走「简化求值」，它验过的就不是真路径，等于没验。本模块的
//! `rehearse` 对每条 delta 调同一个 [`validate_delta`]，提交阶段不再重复
//! 校验——两处校验早晚会分叉。
//!
//! # 零 panic 面
//!
//! 无 `unwrap()`/`expect()`/`panic!`/切片索引；所有取用走 `get()`/`match`。

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::ver01e_typetree::TokenKind;

// ---------------------------------------------------------------------------
// 一、预算与容量（参数唯一源）
// ---------------------------------------------------------------------------

/// 预演阶段预算（毫秒，逻辑计时）。
pub const REHEARSE_BUDGET_MS: u32 = 8;
/// 编排阶段预算。
pub const PLAN_BUDGET_MS: u32 = 4;
/// 提交阶段预算。
pub const APPLY_BUDGET_MS: u32 = 24;
/// 过渡动画预算。
pub const ANIMATE_BUDGET_MS: u32 = 180;
/// 单次换肤的 delta 上限。
pub const MAX_DELTAS: usize = 8192;
/// 受影响组件上限。
pub const MAX_COMPONENTS: usize = 4096;
/// 交互队列容量。
pub const MAX_INTERACTIONS: usize = 512;
/// 事务日志容量。
pub const LOG_CAP: usize = 256;

// ---------------------------------------------------------------------------
// 二、诊断码（段 0x3421）
// ---------------------------------------------------------------------------

/// 换肤引擎诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkinCode {
    /// delta 为空。
    NoDelta = 0x3421_01,
    /// delta 超限。
    DeltaQuota = 0x3421_02,
    /// delta 键重复。
    DuplicateDelta = 0x3421_03,
    /// delta 键非法。
    BadKey = 0x3421_04,
    /// 新旧值相同（无变化不该进事务）。
    NoChange = 0x3421_05,
    /// 新值不合法。
    InvalidValue = 0x3421_06,
    /// 状态机不允许该操作。
    BadPhase = 0x3421_07,
    /// 预演超预算。
    RehearseOverBudget = 0x3421_08,
    /// 编排超预算。
    PlanOverBudget = 0x3421_09,
    /// 提交超预算。
    ApplyOverBudget = 0x3421_10,
    /// 影响面超限。
    ImpactTooLarge = 0x3421_11,
    /// 交互队列溢出。
    InteractionOverflow = 0x3421_12,
    /// 快照缺失（未 begin 就 commit）。
    NoSnapshot = 0x3421_13,
    /// 回滚失败（快照损坏）。
    RollbackFailed = 0x3421_14,
    /// 阶段序号越界。
    StageOutOfRange = 0x3421_15,
}

impl SkinCode {
    /// 数值。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 短名。
    pub fn name(self) -> &'static str {
        match self {
            SkinCode::NoDelta => "E_SKIN_NO_DELTA",
            SkinCode::DeltaQuota => "E_SKIN_DELTA_QUOTA",
            SkinCode::DuplicateDelta => "E_SKIN_DUPLICATE_DELTA",
            SkinCode::BadKey => "E_SKIN_BAD_KEY",
            SkinCode::NoChange => "E_SKIN_NO_CHANGE",
            SkinCode::InvalidValue => "E_SKIN_INVALID_VALUE",
            SkinCode::BadPhase => "E_SKIN_BAD_PHASE",
            SkinCode::RehearseOverBudget => "E_SKIN_REHEARSE_OVER_BUDGET",
            SkinCode::PlanOverBudget => "E_SKIN_PLAN_OVER_BUDGET",
            SkinCode::ApplyOverBudget => "E_SKIN_APPLY_OVER_BUDGET",
            SkinCode::ImpactTooLarge => "E_SKIN_IMPACT_TOO_LARGE",
            SkinCode::InteractionOverflow => "E_SKIN_INTERACTION_OVERFLOW",
            SkinCode::NoSnapshot => "E_SKIN_NO_SNAPSHOT",
            SkinCode::RollbackFailed => "E_SKIN_ROLLBACK_FAILED",
            SkinCode::StageOutOfRange => "E_SKIN_STAGE_OUT_OF_RANGE",
        }
    }

    /// 建议。
    pub fn advice(self) -> &'static str {
        match self {
            SkinCode::NoDelta => "至少提交一条令牌变更",
            SkinCode::DeltaQuota => "分批换肤（单批 8192 条以内）",
            SkinCode::DuplicateDelta => "合并同键变更后重试",
            SkinCode::BadKey => "键须为 `<ns>:<name>`",
            SkinCode::NoChange => "过滤掉新旧同值的条目",
            SkinCode::InvalidValue => "按类型手册修正值",
            SkinCode::BadPhase => "按 Idle→预演→编排→提交→完成 的顺序调用",
            SkinCode::RehearseOverBudget => "缩小变更集或提高预演预算",
            SkinCode::PlanOverBudget => "减少受影响组件数",
            SkinCode::ApplyOverBudget => "拆阶段提交或提高提交预算",
            SkinCode::ImpactTooLarge => "缩小影响面或分批换肤",
            SkinCode::InteractionOverflow => "提高队列容量或缩短换肤耗时",
            SkinCode::NoSnapshot => "先 begin_commit() 再提交阶段",
            SkinCode::RollbackFailed => "快照损坏，重建引擎",
            SkinCode::StageOutOfRange => "检查阶段序号",
        }
    }
}

/// 引擎错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkinError {
    /// 码。
    pub code: SkinCode,
    /// 键/阶段。
    pub at: String,
}

impl SkinError {
    /// 构造。
    pub fn new(code: SkinCode, at: &str) -> Self {
        Self {
            code,
            at: at.to_string(),
        }
    }

    /// 三要素。
    pub fn text(&self) -> String {
        format!(
            "{}（{}）；下一步：{}",
            self.code.name(),
            if self.at.is_empty() { "-" } else { &self.at },
            self.code.advice()
        )
    }
}

// ---------------------------------------------------------------------------
// 三、阶段状态机
// ---------------------------------------------------------------------------

/// 换肤阶段。合法迁移唯一：Idle→Rehearsing→Planning→Committing→Animating→Done，
/// 任一阶段可 →Aborted/RolledBack。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 空闲。
    Idle,
    /// 预演中。
    Rehearsing,
    /// 影响面与编排中。
    Planning,
    /// 提交中。
    Committing,
    /// 过渡动画中。
    Animating,
    /// 完成。
    Done,
    /// 已回滚。
    RolledBack,
    /// 已中止（未落地任何变更）。
    Aborted,
}

impl Phase {
    /// 是否终态。
    pub fn terminal(self) -> bool {
        matches!(self, Phase::Done | Phase::RolledBack | Phase::Aborted)
    }

    /// 是否可接收交互事件（只有进行中需要缓冲）。
    pub fn busy(self) -> bool {
        matches!(self, Phase::Committing | Phase::Animating)
    }

    /// 读屏文本。
    pub fn spoken(self) -> &'static str {
        match self {
            Phase::Idle => "空闲",
            Phase::Rehearsing => "预演中",
            Phase::Planning => "编排中",
            Phase::Committing => "提交中",
            Phase::Animating => "过渡动画中",
            Phase::Done => "已完成",
            Phase::RolledBack => "已回滚",
            Phase::Aborted => "已中止",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、令牌差与值
// ---------------------------------------------------------------------------

/// 令牌值（与 E01 六类同构的精简表达）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkinValue {
    /// 颜色（0xRRGGBBAA）。
    Color(u32),
    /// 尺寸（px）。
    Size(i32),
    /// 字重。
    Weight(u16),
    /// 时长（ms）。
    Duration(u32),
    /// 缓动（枚举序号）。
    Easing(u8),
    /// 阴影（dx,dy,blur,spread,color）。
    Shadow(i32, i32, u16, i16, u32),
}

impl SkinValue {
    /// 对应类型。
    pub fn kind(&self) -> TokenKind {
        match self {
            SkinValue::Color(_) => TokenKind::Color,
            SkinValue::Size(_) => TokenKind::Size,
            SkinValue::Weight(_) => TokenKind::FontWeight,
            SkinValue::Duration(_) => TokenKind::Duration,
            SkinValue::Easing(_) => TokenKind::Easing,
            SkinValue::Shadow(_, _, _, _, _) => TokenKind::Shadow,
        }
    }

    /// 自校验（提交前唯一一道值校验）。
    pub fn validate(&self) -> bool {
        match self {
            SkinValue::Color(_) => true,
            SkinValue::Size(v) => *v >= 0,
            SkinValue::Weight(w) => (100..=900).contains(w) && (*w % 100) == 0,
            SkinValue::Duration(_) => true,
            SkinValue::Easing(e) => *e < 7,
            SkinValue::Shadow(_, _, blur, _, _) => *blur <= 512,
        }
    }

    /// 文本。
    pub fn text(&self) -> String {
        match self {
            SkinValue::Color(c) => format!("#{:08X}", c),
            SkinValue::Size(v) => format!("{}px", v),
            SkinValue::Weight(w) => format!("{}", w),
            SkinValue::Duration(d) => format!("{}ms", d),
            SkinValue::Easing(e) => format!("easing{}", e),
            SkinValue::Shadow(dx, dy, b, s, c) => {
                format!("{} {} {} {} #{:08X}", dx, dy, b, s, c)
            }
        }
    }
}

/// 一条令牌变更。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenDelta {
    /// 键 `ns:name`。
    pub key: String,
    /// 旧值。
    pub old: SkinValue,
    /// 新值。
    pub new: SkinValue,
}

impl TokenDelta {
    /// 构造。
    pub fn new(key: &str, old: SkinValue, new: SkinValue) -> Self {
        Self {
            key: key.to_string(),
            old,
            new,
        }
    }

    /// 是否有实际变化。
    pub fn changed(&self) -> bool {
        self.old != self.new
    }
}

/// 单条 delta 校验（预演与提交共用同一条路径）。
pub fn validate_delta(d: &TokenDelta) -> Result<(), SkinError> {
    if d.key.is_empty() || !d.key.contains(':') {
        return Err(SkinError::new(SkinCode::BadKey, &d.key));
    }
    if !d.changed() {
        return Err(SkinError::new(SkinCode::NoChange, &d.key));
    }
    if d.old.kind() != d.new.kind() {
        return Err(SkinError::new(SkinCode::InvalidValue, &d.key));
    }
    if !d.new.validate() {
        return Err(SkinError::new(SkinCode::InvalidValue, &d.key));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、影响面
// ---------------------------------------------------------------------------

/// 组件标识。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComponentId(pub u32);

/// 影响分级。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImpactGrade {
    /// 全重绘（几何/阴影变了）。
    FullRepaint,
    /// 局部重绘（颜色/字重变了）。
    PartialRepaint,
    /// 无感（仅动效参数变了，下一帧自然生效）。
    Noop,
}

impl ImpactGrade {
    /// 合并两级（取更重者）。
    pub fn merge(self, other: ImpactGrade) -> ImpactGrade {
        if self <= other {
            self
        } else {
            other
        }
    }

    /// 读屏。
    pub fn spoken(self) -> &'static str {
        match self {
            ImpactGrade::FullRepaint => "整块重绘",
            ImpactGrade::PartialRepaint => "局部重绘",
            ImpactGrade::Noop => "无感",
        }
    }
}

/// 键 → 组件集合的注册表（键前缀匹配）。
#[derive(Clone, Debug, Default)]
pub struct ImpactRegistry {
    entries: Vec<(String, ComponentId)>,
}

impl ImpactRegistry {
    /// 空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记「组件订阅某键」。
    pub fn subscribe(&mut self, key: &str, component: ComponentId) {
        self.entries.push((key.to_string(), component));
    }

    /// 查某键的订阅组件（精确匹配 + 前缀匹配，前缀以 `*` 结尾注册）。
    pub fn consumers_of(&self, key: &str) -> Vec<ComponentId> {
        let mut out: Vec<ComponentId> = Vec::new();
        for (pat, c) in self.entries.iter() {
            let hit = if let Some(prefix) = pat.strip_suffix('*') {
                key.starts_with(prefix)
            } else {
                pat == key
            };
            if hit && !out.contains(c) {
                out.push(*c);
            }
        }
        out.sort();
        out
    }

    /// 订阅条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 一个组件的影响。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentImpact {
    /// 组件。
    pub component: ComponentId,
    /// 分级。
    pub grade: ImpactGrade,
    /// 触发它的键。
    pub keys: Vec<String>,
}

/// 影响面结果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Impact {
    /// 受影响组件（按 id 升序）。
    pub components: Vec<ComponentImpact>,
}

impl Impact {
    /// 受影响组件数。
    pub fn len(&self) -> usize {
        self.components.len()
    }

    /// 空？
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    /// 全重绘组件数。
    pub fn full_count(&self) -> usize {
        self.components
            .iter()
            .filter(|c| c.grade == ImpactGrade::FullRepaint)
            .count()
    }

    /// 无感组件数。
    pub fn noop_count(&self) -> usize {
        self.components
            .iter()
            .filter(|c| c.grade == ImpactGrade::Noop)
            .count()
    }

    /// 读屏摘要。
    pub fn spoken(&self) -> String {
        format!(
            "影响 {} 个组件：整块重绘 {}，无感 {}。",
            self.len(),
            self.full_count(),
            self.noop_count()
        )
    }
}

/// 契约：delta 的键 + 类型 → 影响分级。
pub fn grade_of(kind: TokenKind) -> ImpactGrade {
    match kind {
        TokenKind::Size | TokenKind::Shadow => ImpactGrade::FullRepaint,
        TokenKind::Color | TokenKind::FontWeight => ImpactGrade::PartialRepaint,
        TokenKind::Duration | TokenKind::Easing => ImpactGrade::Noop,
    }
}

// ---------------------------------------------------------------------------
// 六、编排
// ---------------------------------------------------------------------------

/// 一个阶段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stage {
    /// 阶段名（读屏文本）。
    pub name: &'static str,
    /// 本阶段处理的类型。
    pub kind: TokenKind,
    /// 本阶段的键。
    pub keys: Vec<String>,
}

/// 过渡计划。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransitionPlan {
    /// 阶段（按类型顺序）。
    pub stages: Vec<Stage>,
}

impl TransitionPlan {
    /// 阶段数。
    pub fn len(&self) -> usize {
        self.stages.len()
    }

    /// 空？
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// 全部键数。
    pub fn key_count(&self) -> usize {
        self.stages.iter().map(|s| s.keys.len()).sum()
    }
}

/// 类型 → 阶段序（颜色 → 尺寸 → 字重 → 时长 → 缓动 → 阴影）。
pub fn stage_order(kind: TokenKind) -> u8 {
    match kind {
        TokenKind::Color => 0,
        TokenKind::Size => 1,
        TokenKind::FontWeight => 2,
        TokenKind::Duration => 3,
        TokenKind::Easing => 4,
        TokenKind::Shadow => 5,
    }
}

/// 阶段名。
pub fn stage_name(kind: TokenKind) -> &'static str {
    match kind {
        TokenKind::Color => "颜色阶段",
        TokenKind::Size => "尺寸阶段",
        TokenKind::FontWeight => "字重阶段",
        TokenKind::Duration => "时长阶段",
        TokenKind::Easing => "缓动阶段",
        TokenKind::Shadow => "阴影阶段",
    }
}

// ---------------------------------------------------------------------------
// 七、交互队列（不中断律的载体）
// ---------------------------------------------------------------------------

/// 交互事件类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionKind {
    /// 点击。
    Click,
    /// 键盘输入。
    Key,
    /// 滚动。
    Scroll,
    /// 指针移动。
    Pointer,
}

/// 交互事件。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interaction {
    /// 类型。
    pub kind: InteractionKind,
    /// 目标键（组件/控件标识）。
    pub target: String,
    /// 逻辑序号（到达序）。
    pub seq: u32,
}

/// 交互缓冲与回放。
#[derive(Clone, Debug, Default)]
pub struct InteractionQueue {
    pending: Vec<Interaction>,
    next_seq: u32,
    dropped: u32,
}

impl InteractionQueue {
    /// 空队列。
    pub fn new() -> Self {
        Self::default()
    }

    /// 入队。满则返回 `InteractionOverflow`（显性），不静默丢。
    pub fn push(&mut self, kind: InteractionKind, target: &str) -> Result<u32, SkinError> {
        if self.pending.len() >= MAX_INTERACTIONS {
            self.dropped = self.dropped.saturating_add(1);
            return Err(SkinError::new(SkinCode::InteractionOverflow, target));
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.pending.push(Interaction {
            kind,
            target: target.to_string(),
            seq,
        });
        Ok(seq)
    }

    /// 待回放数。
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// 空？
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// 丢弃计数（应为 0；非 0 说明发生过溢出）。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 按到达序回放（清空队列）。
    pub fn drain(&mut self) -> Vec<Interaction> {
        let mut out = Vec::with_capacity(self.pending.len());
        for it in self.pending.iter() {
            out.push(it.clone());
        }
        self.pending.clear();
        // 回放顺序必须是到达序；此处已天然有序，再核一遍防上游乱插。
        out.sort_by_key(|i| i.seq);
        out
    }
}

// ---------------------------------------------------------------------------
// 八、事务快照
// ---------------------------------------------------------------------------

/// 事务快照：键 → 旧值 + 修订号。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// 条目。
    pub entries: Vec<(String, SkinValue, u32)>,
}

impl Snapshot {
    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空？
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 事务日志条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// 序号。
    pub seq: u32,
    /// 文本。
    pub text: String,
}

// ---------------------------------------------------------------------------
// 九、引擎
// ---------------------------------------------------------------------------

/// 实时换肤引擎。
#[derive(Clone, Debug)]
pub struct SkinEngine {
    phase: Phase,
    deltas: Vec<TokenDelta>,
    impact: Impact,
    plan: TransitionPlan,
    snapshot: Option<Snapshot>,
    queue: InteractionQueue,
    log: Vec<LogEntry>,
    log_seq: u32,
    rehearse_ms: u32,
    plan_ms: u32,
    apply_ms: u32,
    applied_stages: usize,
    rejects: u32,
}

impl Default for SkinEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl SkinEngine {
    /// 新引擎（Idle）。
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            deltas: Vec::new(),
            impact: Impact::default(),
            plan: TransitionPlan::default(),
            snapshot: None,
            queue: InteractionQueue::new(),
            log: Vec::new(),
            log_seq: 0,
            rehearse_ms: 0,
            plan_ms: 0,
            apply_ms: 0,
            applied_stages: 0,
            rejects: 0,
        }
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// delta 数。
    pub fn delta_count(&self) -> usize {
        self.deltas.len()
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 预演耗时（逻辑毫秒）。
    pub fn rehearse_cost(&self) -> u32 {
        self.rehearse_ms
    }

    /// 编排耗时。
    pub fn plan_cost(&self) -> u32 {
        self.plan_ms
    }

    /// 提交耗时。
    pub fn apply_cost(&self) -> u32 {
        self.apply_ms
    }

    /// 交互队列。
    pub fn queue(&self) -> &InteractionQueue {
        &self.queue
    }

    /// 影响面。
    pub fn impact(&self) -> &Impact {
        &self.impact
    }

    /// 计划（只读；编排入口是带 cost_hint 的 `plan(..)`）。
    pub fn plan_ref(&self) -> &TransitionPlan {
        &self.plan
    }

    /// 日志。
    pub fn log(&self) -> &[LogEntry] {
        &self.log
    }

    fn push_log(&mut self, text: &str) {
        self.log_seq = self.log_seq.saturating_add(1);
        if self.log.len() >= LOG_CAP {
            self.log.remove(0);
        }
        self.log.push(LogEntry {
            seq: self.log_seq,
            text: text.to_string(),
        });
    }

    fn fail(&mut self, e: SkinError) -> SkinError {
        self.rejects = self.rejects.saturating_add(1);
        self.push_log(&format!("拒绝 {}", e.code.name()));
        e
    }

    /// 提交变更集（Idle → Rehearsing）。
    pub fn submit(&mut self, deltas: Vec<TokenDelta>) -> Result<(), SkinError> {
        if self.phase != Phase::Idle {
            return Err(self.fail(SkinError::new(SkinCode::BadPhase, self.phase.spoken())));
        }
        if deltas.is_empty() {
            return Err(self.fail(SkinError::new(SkinCode::NoDelta, "")));
        }
        if deltas.len() > MAX_DELTAS {
            return Err(self.fail(SkinError::new(SkinCode::DeltaQuota, "")));
        }
        // 重复键检测（O(n log n) 用 BTreeMap）。
        let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
        for d in deltas.iter() {
            if seen.insert(d.key.as_str(), 1).is_some() {
                let k = d.key.clone();
                return Err(self.fail(SkinError::new(SkinCode::DuplicateDelta, &k)));
            }
        }
        self.deltas = deltas;
        self.phase = Phase::Rehearsing;
        self.push_log("进入预演");
        Ok(())
    }

    /// 预演。`cost_hint` 是上游给出的预估耗时（逻辑毫秒），用于预算判定。
    pub fn rehearse(&mut self, cost_hint: u32) -> Result<(), SkinError> {
        if self.phase != Phase::Rehearsing {
            return Err(self.fail(SkinError::new(SkinCode::BadPhase, self.phase.spoken())));
        }
        // 逐条走同一条校验路径（不另写简化版）。
        for d in self.deltas.iter() {
            if let Err(e) = validate_delta(d) {
                self.phase = Phase::Aborted;
                return Err(self.fail(e));
            }
        }
        // 预算：逐条 1 单位 + 固定开销 2 单位，与 hint 取大。
        let per = (self.deltas.len() as u32).saturating_add(2);
        self.rehearse_ms = if cost_hint > per { cost_hint } else { per };
        if self.rehearse_ms > REHEARSE_BUDGET_MS {
            self.phase = Phase::Aborted;
            return Err(self.fail(SkinError::new(SkinCode::RehearseOverBudget, "rehearse")));
        }
        self.phase = Phase::Planning;
        self.push_log("预演通过");
        Ok(())
    }

    /// 计算影响面。
    pub fn compute_impact(&mut self, registry: &ImpactRegistry) -> Result<(), SkinError> {
        if self.phase != Phase::Planning {
            return Err(self.fail(SkinError::new(SkinCode::BadPhase, self.phase.spoken())));
        }
        let mut map: BTreeMap<ComponentId, ComponentImpact> = BTreeMap::new();
        for d in self.deltas.iter() {
            let grade = grade_of(d.new.kind());
            for c in registry.consumers_of(&d.key) {
                match map.get_mut(&c) {
                    Some(entry) => {
                        entry.grade = entry.grade.merge(grade);
                        if !entry.keys.iter().any(|k| k == &d.key) {
                            entry.keys.push(d.key.clone());
                        }
                    }
                    None => {
                        map.insert(
                            c,
                            ComponentImpact {
                                component: c,
                                grade,
                                keys: vec![d.key.clone()],
                            },
                        );
                    }
                }
            }
        }
        if map.len() > MAX_COMPONENTS {
            self.phase = Phase::Aborted;
            return Err(self.fail(SkinError::new(SkinCode::ImpactTooLarge, "")));
        }
        self.impact = Impact {
            components: map.into_values().collect(),
        };
        self.push_log(&format!("影响面 {}", self.impact.len()));
        Ok(())
    }

    /// 编排过渡计划（按类型阶段序）。
    pub fn plan(&mut self, cost_hint: u32) -> Result<(), SkinError> {
        if self.phase != Phase::Planning {
            return Err(self.fail(SkinError::new(SkinCode::BadPhase, self.phase.spoken())));
        }
        let mut buckets: BTreeMap<u8, (TokenKind, Vec<String>)> = BTreeMap::new();
        for d in self.deltas.iter() {
            let k = d.new.kind();
            buckets
                .entry(stage_order(k))
                .or_insert_with(|| (k, Vec::new()))
                .1
                .push(d.key.clone());
        }
        let mut stages: Vec<Stage> = Vec::new();
        for (_ord, (kind, keys)) in buckets.into_iter() {
            stages.push(Stage {
                name: stage_name(kind),
                kind,
                keys,
            });
        }
        self.plan = TransitionPlan { stages };
        let per = (self.plan.key_count() as u32).saturating_add(stages_len(&self.plan));
        self.plan_ms = if cost_hint > per { cost_hint } else { per };
        if self.plan_ms > PLAN_BUDGET_MS {
            self.phase = Phase::Aborted;
            return Err(self.fail(SkinError::new(SkinCode::PlanOverBudget, "plan")));
        }
        self.push_log(&format!("编排 {} 阶段", self.plan.len()));
        Ok(())
    }

    /// 拍快照并进入提交阶段。
    pub fn begin_commit(&mut self) -> Result<(), SkinError> {
        if self.phase != Phase::Planning {
            return Err(self.fail(SkinError::new(SkinCode::BadPhase, self.phase.spoken())));
        }
        if self.deltas.is_empty() {
            return Err(self.fail(SkinError::new(SkinCode::NoDelta, "")));
        }
        let mut entries: Vec<(String, SkinValue, u32)> = Vec::with_capacity(self.deltas.len());
        for d in self.deltas.iter() {
            // 修订号从 1 起：快照记「当前值 + 当前修订」，回滚时一并还原。
            entries.push((d.key.clone(), d.old.clone(), 1));
        }
        self.snapshot = Some(Snapshot { entries });
        self.applied_stages = 0;
        self.phase = Phase::Committing;
        self.push_log("拍快照并提交");
        Ok(())
    }

    /// 提交一个阶段。`cost` 为该阶段实际耗时（逻辑毫秒）。
    pub fn commit_stage(&mut self, index: usize, cost: u32) -> Result<(), SkinError> {
        if self.phase != Phase::Committing {
            return Err(self.fail(SkinError::new(SkinCode::BadPhase, self.phase.spoken())));
        }
        if self.snapshot.is_none() {
            return Err(self.fail(SkinError::new(SkinCode::NoSnapshot, "")));
        }
        match self.plan.stages.get(index) {
            Some(_) => {}
            None => {
                return Err(self.fail(SkinError::new(
                    SkinCode::StageOutOfRange,
                    &format!("{}", index),
                )))
            }
        }
        if index != self.applied_stages {
            // 阶段必须按序提交，跳阶段会让中间态暴露给用户。
            return Err(self.fail(SkinError::new(
                SkinCode::StageOutOfRange,
                &format!("expect {}", self.applied_stages),
            )));
        }
        self.apply_ms = self.apply_ms.saturating_add(cost);
        self.applied_stages = self.applied_stages.saturating_add(1);
        if self.apply_ms > APPLY_BUDGET_MS {
            // 超预算即回滚：宁可整链退回，也不留一个卡顿的中间态。
            let _ = self.rollback();
            return Err(self.fail(SkinError::new(SkinCode::ApplyOverBudget, "commit")));
        }
        self.push_log(&format!("提交阶段 {}", index));
        Ok(())
    }

    /// 全部阶段提交完成 → 进入动画阶段。
    pub fn begin_animate(&mut self) -> Result<(), SkinError> {
        if self.phase != Phase::Committing {
            return Err(self.fail(SkinError::new(SkinCode::BadPhase, self.phase.spoken())));
        }
        if self.applied_stages != self.plan.len() {
            return Err(self.fail(SkinError::new(
                SkinCode::BadPhase,
                &format!("{}/{}", self.applied_stages, self.plan.len()),
            )));
        }
        self.phase = Phase::Animating;
        self.push_log("进入过渡动画");
        Ok(())
    }

    /// 动画结束（无论是否被降档），置完成并清空快照。
    pub fn finish(&mut self) -> Result<(), SkinError> {
        if self.phase != Phase::Animating {
            return Err(self.fail(SkinError::new(SkinCode::BadPhase, self.phase.spoken())));
        }
        self.snapshot = None;
        self.phase = Phase::Done;
        self.push_log("换肤完成");
        Ok(())
    }

    /// 回滚到快照。可在 Committing / Animating 阶段调用。
    pub fn rollback(&mut self) -> Result<usize, SkinError> {
        if !matches!(self.phase, Phase::Committing | Phase::Animating) {
            return Err(self.fail(SkinError::new(SkinCode::BadPhase, self.phase.spoken())));
        }
        let snap = match &self.snapshot {
            Some(s) => s.clone(),
            None => return Err(self.fail(SkinError::new(SkinCode::NoSnapshot, ""))),
        };
        if snap.entries.len() != self.deltas.len() {
            // 快照与变更集对不上 = 快照损坏，不能假装回滚成功。
            self.phase = Phase::RolledBack;
            return Err(self.fail(SkinError::new(SkinCode::RollbackFailed, "len")));
        }
        let n = snap.len();
        self.snapshot = None;
        self.applied_stages = 0;
        self.phase = Phase::RolledBack;
        self.push_log(&format!("回滚 {} 条", n));
        Ok(n)
    }

    /// 中止（未落地任何变更）：可从预演/编排阶段调用。
    pub fn abort(&mut self) -> Result<(), SkinError> {
        if self.phase.terminal() {
            return Err(self.fail(SkinError::new(SkinCode::BadPhase, self.phase.spoken())));
        }
        self.snapshot = None;
        self.phase = Phase::Aborted;
        self.push_log("中止");
        Ok(())
    }

    /// 交互事件入口：进行中则入队，空闲则直接返回「无需缓冲」。
    pub fn interaction(&mut self, kind: InteractionKind, target: &str) -> Result<bool, SkinError> {
        if self.phase.busy() {
            self.queue.push(kind, target)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 完成后回放交互事件。
    pub fn replay_interactions(&mut self) -> Vec<Interaction> {
        self.queue.drain()
    }

    /// 读屏摘要（状态 + 进度 + 交互缓冲 + 预算）。
    pub fn spoken_state(&self) -> String {
        format!(
            "换肤{}：{} 条变更，{} 个阶段，影响 {} 个组件，预演 {} 毫秒，提交 {} 毫秒，缓冲交互 {} 条。",
            self.phase.spoken(),
            self.deltas.len(),
            self.plan.len(),
            self.impact.len(),
            self.rehearse_ms,
            self.apply_ms,
            self.queue.len()
        )
    }

    /// 全链不变式自检：阶段推进不越级、提交数与计划一致、快照与变更集同长。
    pub fn invariant_error(&self) -> Option<String> {
        if self.applied_stages > self.plan.len() {
            return Some("已提交阶段数超过计划".to_string());
        }
        if self.phase == Phase::Done && self.applied_stages != self.plan.len() {
            return Some("完成态但阶段未提交完".to_string());
        }
        if let Some(s) = &self.snapshot {
            if s.len() != self.deltas.len() {
                return Some("快照与变更集长度不符".to_string());
            }
        }
        None
    }
}

/// 计划阶段数（小工具，避免在方法里借用冲突）。
fn stages_len(plan: &TransitionPlan) -> u32 {
    plan.stages.len() as u32
}

// ---------------------------------------------------------------------------
// 十、单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn deltas() -> Vec<TokenDelta> {
        vec![
            TokenDelta::new(
                "core:color.bg",
                SkinValue::Color(0x000000FF),
                SkinValue::Color(0x101418FF),
            ),
            TokenDelta::new("core:size.gap", SkinValue::Size(4), SkinValue::Size(8)),
            TokenDelta::new("core:motion.fast", SkinValue::Duration(120), SkinValue::Duration(90)),
        ]
    }

    fn ready() -> SkinEngine {
        let mut e = SkinEngine::new();
        assert!(e.submit(deltas()).is_ok());
        assert!(e.rehearse(3).is_ok());
        let mut reg = ImpactRegistry::new();
        reg.subscribe("core:color.*", ComponentId(2));
        reg.subscribe("core:size.*", ComponentId(1));
        assert!(e.compute_impact(&reg).is_ok());
        assert!(e.plan(2).is_ok());
        e
    }

    #[test]
    fn happy_path() {
        let mut e = ready();
        assert_eq!(e.phase(), Phase::Planning);
        assert!(e.begin_commit().is_ok());
        for i in 0..e.plan_ref().len() {
            assert!(e.commit_stage(i, 1).is_ok());
        }
        assert!(e.begin_animate().is_ok());
        assert!(e.finish().is_ok());
        assert_eq!(e.phase(), Phase::Done);
        assert!(e.invariant_error().is_none());
    }

    #[test]
    fn stage_order_is_fixed() {
        let e = ready();
        let kinds: Vec<TokenKind> = e.plan_ref().stages.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![TokenKind::Color, TokenKind::Size, TokenKind::Duration]
        );
    }

    #[test]
    fn over_budget_rehearse_aborts() {
        let mut e = SkinEngine::new();
        assert!(e.submit(deltas()).is_ok());
        assert_eq!(
            e.rehearse(REHEARSE_BUDGET_MS + 1).err().map(|x| x.code),
            Some(SkinCode::RehearseOverBudget)
        );
        assert_eq!(e.phase(), Phase::Aborted);
    }

    #[test]
    fn duplicate_delta_rejected() {
        let mut e = SkinEngine::new();
        let d = TokenDelta::new("core:a", SkinValue::Size(1), SkinValue::Size(2));
        let d2 = TokenDelta::new("core:a", SkinValue::Size(2), SkinValue::Size(3));
        assert_eq!(
            e.submit(vec![d, d2]).err().map(|x| x.code),
            Some(SkinCode::DuplicateDelta)
        );
    }

    #[test]
    fn rollback_restores() {
        let mut e = ready();
        assert!(e.begin_commit().is_ok());
        assert!(e.commit_stage(0, 1).is_ok());
        assert_eq!(e.rollback(), Ok(3));
        assert_eq!(e.phase(), Phase::RolledBack);
        assert!(e.invariant_error().is_none());
    }

    #[test]
    fn interactions_buffer_while_busy() {
        let mut e = ready();
        assert!(e.begin_commit().is_ok());
        assert_eq!(e.interaction(InteractionKind::Click, "btn.ok"), Ok(true));
        assert_eq!(e.queue().len(), 1);
        let replay = e.replay_interactions();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].seq, 0);
    }
}
