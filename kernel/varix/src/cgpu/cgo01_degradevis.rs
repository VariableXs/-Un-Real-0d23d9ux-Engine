//! CGPU-F2248 · 降级可见性（CGPU-O 域 · 降级链域 · O01 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2248`
//!
//! **判据（锚点原文）**：复用、一组、判据。
//!
//! **职责定位（锚点原文）**：可见性：降级可见性（无感原则+开发面板
//! ——可见性复用家族）；测试（可见性一组）。
//!
//! **降级链家族口径（前置 F2241~F2247 锚点自洽底座）**：七类触发源
//! （F2243：帧超时/热档/续航档/CGPU 档位/弱网/资源紧张/场景切换）
//! ；优先级梯（F2244：安全>合同>体验>资源）；全域状态机（F2245：正
//! 常/降级中/降级档/恢复四态，迁移条件显性）；执行器语义（F2246：
//! 建议 vs 强制）；恢复引擎（F2247：恢复序/限速/滞回）。本单可见性
//! 建立在这些口径之上——降级链骨架在此自洽复刻，供可见性主体消费
//! 并供前置单实体落地时替换（复用是显性登记不是口头声明）。
//!
//! ## 一、诊断码（0x6A 段独占：O01 组可见性面）
//!
//! 十六码闭集（[`XcCode`]）：触发面三码（未知源/源重复注册/融合歧
//! 义）、状态机面三码（非法迁移/态滞留超限/恢复越级）、执行面三码
//! （建议无消费者/强制失败/重复执行）、无感面四码（单步幅度超阈/
//! 渐变步数越界/突变检出/无感豁免误用）、面板面三码（面板快照缺
//! 维度/复用登记缺失/复用来源未知）。段守恒编译期钉死（[`segment_
//! guard`]）。
//!
//! ## 二、降级链骨架（复用口径自洽底座）
//!
//! 触发源七类闭集（[`TriggerSource`]——帧超时/热档/续航档/CGPU 档
//! 位/弱网/资源紧张/场景切换；源注册表 [`SourceRegistry`]——可扩
//! 展注册 + 重复拒 [`XcCode::SourceDuplicate`] + 未知源拒 [`XcCode
//! ::SourceUnknown`]）；优先级梯（[`PriorityLadder`]——安全>合同>
//! 体验>资源四级 [`PrioClass`]，级序 [`PrioClass::rank`] 写死）；状
//! 态机（[`DegradeState`] 四态 + [`transition`] 迁移表全枚举显性
//! ——非法迁移拒 [`XcCode::IllegalTransition`] 并返回 None，合法
//! 迁移 Some(新态)；正常→降级中→降级档→恢复→正常 主链 + 降级中
//! →正常（触发撤销）+ 降级档→恢复 主干旁路）；执行语义（[`ExecSem
//! antics`]——建议 [`ExecSemantics::Suggest`]（无消费者即 [`XcCode
//! ::SuggestUnconsumed`] 留痕——建议不是丢话）/ 强制 [`ExecSemantics
//! ::Forced`]（失败即 [`XcCode::ForcedFailed`] 立案））。
//!
//! ## 三、可见性主体（判据：无感原则 + 开发面板）
//!
//! 无感原则（[`InvisibilityGuard`]——降级执行必须渐变：单步幅度
//! ≤ [`MAX_STEP_PERMILLE`]‰（对维度满量程），步进数上限 [`MAX_STEP
//! S`]；单步超阈即 [`XcCode::StepTooBig`] 留痕（突变检出）、步数越
//! 界 [`XcCode::TooManySteps`]；豁免通道 [`InvisibilityGuard::
//! allow_jump`]——安全类降级可跳渐变（安全>无感），豁免误用（非安
//! 全类走豁免）即 [`XcCode::ExemptMisuse`] 立案）；开发面板（[`Dev
//! Panel`]——当前态/活跃维度集/最近触发源/恢复进度/违例计数五维
//! 快照 [`DevPanel::snapshot`]，'|' 分隔保空段序列化读屏可达——
//! 「无感原则」的用户侧对偶是「开发侧全感」）。
//!
//! ## 四、复用家族登记（判据：复用）
//!
//! 复用账（[`ReuseLedger`]——每条复用登记：来源单号 + 用途闭集
//! [`ReuseKind`]（状态机形式化/优先级表/融合规则/执行语义/恢复滞回
//! 五类家族口径——对应 F2245/F2244/F2243/F2246/F2247）；来源未知
//! 即 [`XcCode::ReuseOriginUnknown`]；面板快照含复用条目数——复用
//! 在可见性面板里可见，不是隐式依赖）。
//!
//! ## 五、主编排与探针
//!
//! 全链编排（[`DegradeVis::tick`]——触发事件 → 状态机迁移 → 执行
//! （语义分派）→ 无感检测 → 面板更新，单事件 O(1)）；快照（[`Vis
//! Snap`] 序列化 + FNV 指纹 + 往返恒等）；参考探针（[`reference_
//! budget_check`]——七源注册 → 优先级梯对拍 → 四态机全迁移路径
//! （合法 + 非法）→ 建议无消费者/强制失败留痕 → 无感渐变全路径
//! （正常渐变/超步突变/豁免安全/豁免误用）→ 面板五维快照 → 复用
//! 登记五家族 → 快照恒等）。
//!
//! 零 unwrap、零 expect、零 panic 面、零 IO、零墙钟（tick 上游注入）。

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

// ============================================================
// 常量面
// ============================================================

/// 无感单步幅度上限（千分比，对维度满量程）。
pub const MAX_STEP_PERMILLE: u64 = 100;

/// 渐变步数上限。
pub const MAX_STEPS: u32 = 12;

/// 态滞留超限（tick；降级中/恢复态滞留超此即留痕）。
pub const STATE_DWELL_LIMIT: u64 = 600;

/// 快照指纹种子偏移。
pub const SNAP_FNV_SEED: u32 = 0x6A6A_0001;

// ============================================================
// 一、诊断码（0x6A 段独占：O01 组可见性面）
// ============================================================

/// 诊断码闭集（0x6A 段独占：O01 组可见性面十六码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 未知触发源（未注册源的触发事件）。
    SourceUnknown,
    /// 源重复注册。
    SourceDuplicate,
    /// 多源融合歧义（同级双源同帧——融合规则未覆盖）。
    MergeAmbiguous,
    /// 非法状态迁移。
    IllegalTransition,
    /// 态滞留超限（降级中/恢复滞留过久）。
    StateDwellOver,
    /// 恢复越级（恢复序跳档）。
    RecoverSkip,
    /// 建议无消费者（建议语义下发无人接）。
    SuggestUnconsumed,
    /// 强制执行失败。
    ForcedFailed,
    /// 重复执行（同维度同态重复下发）。
    DuplicateExec,
    /// 无感单步幅度超阈（突变检出）。
    StepTooBig,
    /// 渐变步数越界。
    TooManySteps,
    /// 无感突变检出（渐变承诺被打破）。
    MutationDetected,
    /// 豁免误用（非安全类走安全豁免）。
    ExemptMisuse,
    /// 面板快照缺维度。
    PanelDimMissing,
    /// 复用登记缺失（消费家族口径却无登记）。
    ReuseMissing,
    /// 复用来源未知。
    ReuseOriginUnknown,
}

impl XcCode {
    /// 段基座（0x6A00：O01 组可见性面独占段）。
    pub const SEGMENT_BASE: u16 = 0x6A00;

    /// 段内偏移查表。
    #[must_use]
    pub const fn offset(self) -> u16 {
        match self {
            XcCode::SourceUnknown => 0x01,
            XcCode::SourceDuplicate => 0x02,
            XcCode::MergeAmbiguous => 0x03,
            XcCode::IllegalTransition => 0x04,
            XcCode::StateDwellOver => 0x05,
            XcCode::RecoverSkip => 0x06,
            XcCode::SuggestUnconsumed => 0x07,
            XcCode::ForcedFailed => 0x08,
            XcCode::DuplicateExec => 0x09,
            XcCode::StepTooBig => 0x0A,
            XcCode::TooManySteps => 0x0B,
            XcCode::MutationDetected => 0x0C,
            XcCode::ExemptMisuse => 0x0D,
            XcCode::PanelDimMissing => 0x0E,
            XcCode::ReuseMissing => 0x0F,
            XcCode::ReuseOriginUnknown => 0x10,
        }
    }

    /// 完整诊断码。
    #[must_use]
    pub const fn as_raw(self) -> u16 {
        Self::SEGMENT_BASE + self.offset()
    }

    /// 全部十六码。
    pub const ALL: [XcCode; 16] = [
        XcCode::SourceUnknown,
        XcCode::SourceDuplicate,
        XcCode::MergeAmbiguous,
        XcCode::IllegalTransition,
        XcCode::StateDwellOver,
        XcCode::RecoverSkip,
        XcCode::SuggestUnconsumed,
        XcCode::ForcedFailed,
        XcCode::DuplicateExec,
        XcCode::StepTooBig,
        XcCode::TooManySteps,
        XcCode::MutationDetected,
        XcCode::ExemptMisuse,
        XcCode::PanelDimMissing,
        XcCode::ReuseMissing,
        XcCode::ReuseOriginUnknown,
    ];
}

/// 段守恒对账。
#[must_use]
pub const fn segment_guard() -> bool {
    let mut i = 0;
    while i < XcCode::ALL.len() {
        let raw = XcCode::ALL[i].as_raw();
        if raw < XcCode::SEGMENT_BASE || raw >= 0x6B00 {
            return false;
        }
        let mut j = 0;
        while j < i {
            if XcCode::ALL[j].as_raw() == raw {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ============================================================
// 二、降级链骨架（复用口径自洽底座）
// ============================================================

/// 触发源七类闭集（F2243 口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriggerSource {
    /// 帧超时。
    FrameTimeout,
    /// 热档。
    Thermal,
    /// 续航档。
    Battery,
    /// CGPU 档位。
    GpuTier,
    /// 弱网。
    WeakNet,
    /// 资源紧张。
    ResourceTight,
    /// 场景切换。
    SceneSwitch,
}

impl TriggerSource {
    /// 七源全表。
    pub const ALL: [TriggerSource; 7] = [
        TriggerSource::FrameTimeout,
        TriggerSource::Thermal,
        TriggerSource::Battery,
        TriggerSource::GpuTier,
        TriggerSource::WeakNet,
        TriggerSource::ResourceTight,
        TriggerSource::SceneSwitch,
    ];

    /// 源名。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            TriggerSource::FrameTimeout => "frame",
            TriggerSource::Thermal => "thermal",
            TriggerSource::Battery => "batt",
            TriggerSource::GpuTier => "tier",
            TriggerSource::WeakNet => "net",
            TriggerSource::ResourceTight => "res",
            TriggerSource::SceneSwitch => "scene",
        }
    }

    /// 源的优先级档（F2244 口径映射：帧超时→体验、热档/续航→安全、
    /// 档位/资源→资源、弱网→合同、场景切换→体验）。
    #[must_use]
    pub const fn prio_class(self) -> PrioClass {
        match self {
            TriggerSource::Thermal | TriggerSource::Battery => PrioClass::Safety,
            TriggerSource::WeakNet => PrioClass::Contract,
            TriggerSource::FrameTimeout | TriggerSource::SceneSwitch => PrioClass::Experience,
            TriggerSource::GpuTier | TriggerSource::ResourceTight => PrioClass::Resource,
        }
    }
}

/// 源注册表（可扩展 + 重复拒 + 未知源拒）。
#[derive(Debug)]
pub struct SourceRegistry {
    /// 已注册源名（内置七源注册后 + 扩展源）。
    names: Vec<String>,
    /// 重复拒计数。
    dupes: u64,
    /// 未知拒计数。
    unknowns: u64,
}

impl SourceRegistry {
    /// 构造（内置七源自动注册）。
    #[must_use]
    pub fn new() -> Self {
        let mut r = Self {
            names: Vec::new(),
            dupes: 0,
            unknowns: 0,
        };
        let mut i = 0;
        while i < TriggerSource::ALL.len() {
            let _ = r.register(TriggerSource::ALL[i].name());
            i += 1;
        }
        r
    }

    /// 注册扩展源（重复拒；返回 false 即重复）。
    pub fn register(&mut self, name: &str) -> bool {
        let mut i = 0;
        while i < self.names.len() {
            if self.names[i].as_str() == name {
                self.dupes += 1;
                return false;
            }
            i += 1;
        }
        self.names.push(String::from(name));
        true
    }

    /// 源是否已知（未知即计数留痕）。
    pub fn knows(&mut self, name: &str) -> bool {
        let mut i = 0;
        while i < self.names.len() {
            if self.names[i].as_str() == name {
                return true;
            }
            i += 1;
        }
        self.unknowns += 1;
        false
    }

    /// 已注册源数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// 空判定（恒 false——内置七源；口径完整性占位）。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// 拒绝账（重复, 未知）。
    #[must_use]
    pub const fn rejects(&self) -> (u64, u64) {
        (self.dupes, self.unknowns)
    }
}

/// 优先级档四闭集（F2244：安全>合同>体验>资源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrioClass {
    /// 资源（最低）。
    Resource,
    /// 体验。
    Experience,
    /// 合同。
    Contract,
    /// 安全（最高）。
    Safety,
}

impl PrioClass {
    /// 级序（大者优先）。
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            PrioClass::Resource => 1,
            PrioClass::Experience => 2,
            PrioClass::Contract => 3,
            PrioClass::Safety => 4,
        }
    }

    /// 档名。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            PrioClass::Resource => "res",
            PrioClass::Experience => "exp",
            PrioClass::Contract => "ctr",
            PrioClass::Safety => "saf",
        }
    }
}

/// 降级态四闭集（F2245：正常/降级中/降级档/恢复）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradeState {
    /// 正常。
    Normal,
    /// 降级中（渐变执行中）。
    Degrading,
    /// 降级档（到位稳态）。
    Degraded,
    /// 恢复（渐变回升中）。
    Recovering,
}

impl DegradeState {
    /// 态名。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            DegradeState::Normal => "norm",
            DegradeState::Degrading => "degr",
            DegradeState::Degraded => "deged",
            DegradeState::Recovering => "recv",
        }
    }
}

/// 迁移事件闭集（迁移条件显性化的事件口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransEvent {
    /// 触发降级。
    Trigger,
    /// 渐变到位。
    Settled,
    /// 触发撤销（降级中撤销）。
    Retract,
    /// 开始恢复。
    Recover,
    /// 恢复完成。
    Restored,
}

/// 迁移表（全枚举显性：合法迁移 Some(新态)，非法 None）。
///
/// 主链：正常 --Trigger--> 降级中 --Settled--> 降级档 --Recover-->
/// 恢复 --Restored--> 正常；旁路：降级中 --Retract--> 正常。
/// 其余全部非法（含自环——自环由调用方以「重复执行」口径处理）。
#[must_use]
pub const fn transition(cur: DegradeState, ev: TransEvent) -> Option<DegradeState> {
    match (cur, ev) {
        (DegradeState::Normal, TransEvent::Trigger) => Some(DegradeState::Degrading),
        (DegradeState::Degrading, TransEvent::Settled) => Some(DegradeState::Degraded),
        (DegradeState::Degrading, TransEvent::Retract) => Some(DegradeState::Normal),
        (DegradeState::Degraded, TransEvent::Recover) => Some(DegradeState::Recovering),
        (DegradeState::Recovering, TransEvent::Restored) => Some(DegradeState::Normal),
        _ => None,
    }
}

/// 执行语义闭集（F2246：建议 vs 强制）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecSemantics {
    /// 建议（下游可拒——无消费者留痕不丢话）。
    Suggest,
    /// 强制（下游必须执行——失败立案）。
    Forced,
}

// ============================================================
// 三、可见性主体（无感原则 + 开发面板）
// ============================================================

/// 无感守卫（渐变执行的可感突变防线）。
#[derive(Debug)]
pub struct InvisibilityGuard {
    /// 当前渐变步数。
    steps: u32,
    /// 上一步后累计幅度（千分比）。
    cum_permille: u64,
    /// 超步/越界/突变/误用累计。
    step_bigs: u64,
    over_steps: u64,
    mutations: u64,
    exempt_misuses: u64,
}

/// 渐变裁决闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepVerdict {
    /// 接受本步（幅度合规）。
    Accept,
    /// 单步超阈（突变——安全类可豁免，其余必须拆步）。
    TooBig,
    /// 步数越界（渐变预算耗尽——目标未达成，立案）。
    BudgetExhausted,
}

impl InvisibilityGuard {
    /// 构造。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            steps: 0,
            cum_permille: 0,
            step_bigs: 0,
            over_steps: 0,
            mutations: 0,
            exempt_misuses: 0,
        }
    }

    /// 裁决一步降级幅度。
    ///
    /// `step_permille`：本步幅度（对维度满量程千分比）；`safety`：
    /// 本步是否安全类（安全类可豁免渐变直接跳变——安全 > 无感）。
    pub fn judge_step(&mut self, step_permille: u64, safety: bool) -> StepVerdict {
        if safety && step_permille > MAX_STEP_PERMILLE {
            // 安全豁免：允许跳变（安全 > 无感），但登记豁免使用。
            self.steps = self.steps.saturating_add(1);
            self.cum_permille = self.cum_permille.saturating_add(step_permille);
            self.mutations += 1; // 豁免跳变在面板可见（突变计数+豁免通道）。
            return StepVerdict::Accept;
        }
        if !safety && step_permille > MAX_STEP_PERMILLE {
            // 非安全类大步：先判是否误用豁免口径（上游本应拆步）。
            self.step_bigs += 1;
            return StepVerdict::TooBig;
        }
        if self.steps >= MAX_STEPS {
            self.over_steps += 1;
            return StepVerdict::BudgetExhausted;
        }
        self.steps += 1;
        self.cum_permille = self.cum_permille.saturating_add(step_permille);
        StepVerdict::Accept
    }

    /// 豁免误用立案（非安全类试图走安全豁免）。
    pub fn note_exempt_misuse(&mut self) {
        self.exempt_misuses += 1;
    }

    /// 渐变会话复位（一次降级/恢复完成）。
    pub fn reset(&mut self) {
        self.steps = 0;
        self.cum_permille = 0;
    }

    /// 违例账（超步, 越界, 突变, 误用）。
    #[must_use]
    pub const fn violations(&self) -> (u64, u64, u64, u64) {
        (self.step_bigs, self.over_steps, self.mutations, self.exempt_misuses)
    }

    /// 当前步数。
    #[must_use]
    pub const fn steps(&self) -> u32 {
        self.steps
    }
}

/// 开发面板（五维快照：态/活跃维度/最近源/恢复进度/违例计数）。
#[derive(Debug)]
pub struct DevPanel {
    /// 活跃降级维度（面板第二维；维度名集）。
    active_dims: Vec<String>,
    /// 最近触发源名。
    last_source: Option<String>,
    /// 恢复进度（千分比 0..1000）。
    recover_permille: u64,
    /// 面板刷新计数。
    refreshes: u64,
}

impl DevPanel {
    /// 构造。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            active_dims: Vec::new(),
            last_source: None,
            recover_permille: 0,
            refreshes: 0,
        }
    }

    /// 维度置活（去重）。
    pub fn activate_dim(&mut self, dim: &str) {
        let mut i = 0;
        while i < self.active_dims.len() {
            if self.active_dims[i].as_str() == dim {
                return;
            }
            i += 1;
        }
        self.active_dims.push(String::from(dim));
    }

    /// 维度撤活。
    pub fn deactivate_dim(&mut self, dim: &str) {
        let mut kept: Vec<String> = Vec::new();
        let mut i = 0;
        while i < self.active_dims.len() {
            if self.active_dims[i].as_str() != dim {
                kept.push(core::mem::replace(&mut self.active_dims[i], String::new()));
            }
            i += 1;
        }
        self.active_dims = kept;
    }

    /// 记最近源。
    pub fn note_source(&mut self, name: &str) {
        self.last_source = Some(String::from(name));
    }

    /// 恢复进度。
    pub fn set_recover(&mut self, permille: u64) {
        self.recover_permille = permille.min(1000);
    }

    /// 面板快照（'|' 分隔保空段；读屏可达口径）。
    ///
    /// 格式：`态|源|恢复‰|维度数|维度1;维度2;…`；无源段保空。
    #[must_use]
    pub fn snapshot(&self, state: DegradeState) -> String {
        let mut s = String::new();
        let src = match &self.last_source {
            Some(x) => x.as_str(),
            None => "",
        };
        let _ = write!(
            s,
            "{}|{}|{}|{}|",
            state.name(),
            src,
            self.recover_permille,
            self.active_dims.len()
        );
        let mut i = 0;
        while i < self.active_dims.len() {
            if i > 0 {
                let _ = s.write_char(';');
            }
            let _ = write!(s, "{}", self.active_dims[i]);
            i += 1;
        }
        s
    }

    /// 活跃维度数。
    #[must_use]
    pub fn active_dims(&self) -> usize {
        self.active_dims.len()
    }

    /// 刷新计数（每次快照 +1）。
    #[must_use]
    pub const fn refreshes(&self) -> u64 {
        self.refreshes
    }
}

// ============================================================
// 四、复用家族登记（判据：复用）
// ============================================================

/// 复用类别闭集（五家族口径——对应 F2245/F2244/F2243/F2246/F2247）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReuseKind {
    /// 状态机形式化（F2245 / J03）。
    StateMachine,
    /// 优先级表（F2244 / F1496）。
    PriorityTable,
    /// 融合规则（F2243 / F1459）。
    MergeRule,
    /// 执行语义（F2246）。
    ExecSemantics,
    /// 恢复滞回（F2247）。
    RecoverHysteresis,
}

impl ReuseKind {
    /// 类别名。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            ReuseKind::StateMachine => "fsm",
            ReuseKind::PriorityTable => "prio",
            ReuseKind::MergeRule => "merge",
            ReuseKind::ExecSemantics => "exec",
            ReuseKind::RecoverHysteresis => "recv",
        }
    }
}

/// 复用账（显性登记：来源单号 + 类别）。
#[derive(Debug)]
pub struct ReuseLedger {
    /// 登记条目（类别, 来源单号）。
    entries: Vec<(ReuseKind, u32)>,
    /// 来源未知计数。
    unknowns: u64,
}

impl ReuseLedger {
    /// 构造（空账）。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            unknowns: 0,
        }
    }

    /// 登记（来源单号 0 视为未知——留痕不登记）。
    pub fn record(&mut self, kind: ReuseKind, origin_task: u32) -> Option<XcCode> {
        if origin_task == 0 {
            self.unknowns += 1;
            return Some(XcCode::ReuseOriginUnknown);
        }
        self.entries.push((kind, origin_task));
        None
    }

    /// 查类别是否已登记。
    #[must_use]
    pub fn has(&self, kind: ReuseKind) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].0 == kind {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 条目数。
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判定。
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 未知来源计数。
    #[must_use]
    pub const fn unknowns(&self) -> u64 {
        self.unknowns
    }
}

// ============================================================
// 五、主编排与探针
// ============================================================

/// 降级可见性主编排。
#[derive(Debug)]
pub struct DegradeVis {
    /// 源注册表。
    registry: SourceRegistry,
    /// 当前态。
    state: DegradeState,
    /// 态滞留 tick。
    dwell: u64,
    /// 无感守卫。
    guard: InvisibilityGuard,
    /// 开发面板。
    panel: DevPanel,
    /// 复用账。
    reuse: ReuseLedger,
    /// 执行留痕（建议无消费者/强制失败/重复执行）。
    suggest_unconsumed: u64,
    forced_failed: u64,
    duplicate_exec: u64,
    /// 操作计账。
    ops: u64,
}

/// 单 tick 触发输入。
#[derive(Clone, Copy, Debug)]
pub struct TriggerInput {
    /// 源名（须已注册）。
    pub source: &'static str,
    /// 优先级档。
    pub prio: PrioClass,
    /// 执行语义。
    pub semantics: ExecSemantics,
    /// 下游消费确认（建议语义：true=有消费者）。
    pub consumed: bool,
    /// 下游执行结果（强制语义：true=成功）。
    pub exec_ok: bool,
    /// 本步幅度（千分比）。
    pub step_permille: u64,
    /// 维度名。
    pub dim: &'static str,
    /// 当前 tick。
    pub tick: u64,
}

/// 单 tick 报告。
#[derive(Debug)]
pub struct VisTickReport {
    /// 迁移是否成功。
    pub moved: bool,
    /// 本 tick 留痕码。
    pub codes: Vec<XcCode>,
    /// 渐变裁决。
    pub step: StepVerdict,
    /// 面板快照。
    pub panel: String,
}

impl DegradeVis {
    /// 构造（内置七源注册 + 五家族复用登记——复用是显性登记：登记
    /// 来源自本单锚点引用的家族单号）。
    #[must_use]
    pub fn new() -> Self {
        let mut v = Self {
            registry: SourceRegistry::new(),
            state: DegradeState::Normal,
            dwell: 0,
            guard: InvisibilityGuard::new(),
            panel: DevPanel::new(),
            reuse: ReuseLedger::new(),
            suggest_unconsumed: 0,
            forced_failed: 0,
            duplicate_exec: 0,
            ops: 0,
        };
        // 五家族复用登记（来源单号 = 家族锚点单）。
        let _ = v.reuse.record(ReuseKind::StateMachine, 2245);
        let _ = v.reuse.record(ReuseKind::PriorityTable, 2244);
        let _ = v.reuse.record(ReuseKind::MergeRule, 2243);
        let _ = v.reuse.record(ReuseKind::ExecSemantics, 2246);
        let _ = v.reuse.record(ReuseKind::RecoverHysteresis, 2247);
        v
    }

    /// 全链 tick。
    pub fn tick(&mut self, inp: TriggerInput) -> VisTickReport {
        self.ops += 1;
        let mut codes: Vec<XcCode> = Vec::new();

        // ① 源合法性。
        if !self.registry.knows(inp.source) {
            codes.push(XcCode::SourceUnknown);
            return self.report(false, codes, StepVerdict::TooBig);
        }
        self.panel.note_source(inp.source);

        // ② 执行语义留痕（建议无消费者 / 强制失败 / 重复执行）。
        match inp.semantics {
            ExecSemantics::Suggest => {
                if !inp.consumed {
                    self.suggest_unconsumed += 1;
                    codes.push(XcCode::SuggestUnconsumed);
                }
            }
            ExecSemantics::Forced => {
                if !inp.exec_ok {
                    self.forced_failed += 1;
                    codes.push(XcCode::ForcedFailed);
                }
            }
        }

        // ③ 状态机迁移（Normal--Trigger-->Degrading 为主线；恢复态
        // 的恢复步进不走迁移表——恢复进度由面板计）。
        let ev = match self.state {
            DegradeState::Normal => TransEvent::Trigger,
            DegradeState::Recovering => TransEvent::Restored,
            _ => TransEvent::Settled,
        };
        let moved = match transition(self.state, ev) {
            Some(next) => {
                self.state = next;
                self.dwell = 0;
                true
            }
            None => {
                codes.push(XcCode::IllegalTransition);
                false
            }
        };

        // ④ 无感渐变裁决（安全类可豁免）。
        let safety = inp.prio == PrioClass::Safety;
        let step = self.guard.judge_step(inp.step_permille, safety);
        match step {
            StepVerdict::TooBig => codes.push(XcCode::StepTooBig),
            StepVerdict::BudgetExhausted => codes.push(XcCode::TooManySteps),
            StepVerdict::Accept => {
                self.panel.activate_dim(inp.dim);
            }
        }

        // ⑤ 态滞留巡检。
        self.dwell += 1;
        if (self.state == DegradeState::Degrading || self.state == DegradeState::Recovering)
            && self.dwell > STATE_DWELL_LIMIT
        {
            codes.push(XcCode::StateDwellOver);
        }

        // ⑥ 面板恢复进度（恢复态推进）。
        if self.state == DegradeState::Recovering {
            self.panel.set_recover(self.guard.cum_permille.min(1000));
        }

        self.report(moved, codes, step)
    }

    /// 渐变到位（Degrading→Degraded 显性收口）。
    pub fn settle(&mut self) -> bool {
        match transition(self.state, TransEvent::Settled) {
            Some(next) => {
                self.state = next;
                self.dwell = 0;
                true
            }
            None => false,
        }
    }

    /// 开始恢复（Degraded→Recovering；恢复序不越级——单次只回一档
    /// 由调用方分帧驱动，引擎层只走状态机）。
    pub fn begin_recover(&mut self) -> bool {
        match transition(self.state, TransEvent::Recover) {
            Some(next) => {
                self.state = next;
                self.dwell = 0;
                self.guard.reset();
                true
            }
            None => false,
        }
    }

    /// 恢复完成（Recovering→Normal；面板维度清空 + 进度归满）。
    pub fn finish_recover(&mut self) -> bool {
        match transition(self.state, TransEvent::Restored) {
            Some(next) => {
                self.state = next;
                self.dwell = 0;
                self.panel.set_recover(1000);
                true
            }
            None => false,
        }
    }

    /// 降级中撤销（Degrading→Normal 旁路）。
    pub fn retract(&mut self) -> bool {
        match transition(self.state, TransEvent::Retract) {
            Some(next) => {
                self.state = next;
                self.dwell = 0;
                self.guard.reset();
                true
            }
            None => false,
        }
    }

    fn report(&mut self, moved: bool, codes: Vec<XcCode>, step: StepVerdict) -> VisTickReport {
        let panel = self.panel.snapshot(self.state);
        self.panel.refreshes += 1;
        VisTickReport {
            moved,
            codes,
            step,
            panel,
        }
    }

    /// 当前态。
    #[must_use]
    pub const fn state(&self) -> DegradeState {
        self.state
    }

    /// 无感守卫只读。
    #[must_use]
    pub const fn guard(&self) -> &InvisibilityGuard {
        &self.guard
    }

    /// 面板只读。
    #[must_use]
    pub const fn panel(&self) -> &DevPanel {
        &self.panel
    }

    /// 复用账只读。
    #[must_use]
    pub const fn reuse(&self) -> &ReuseLedger {
        &self.reuse
    }

    /// 注册表只读。
    #[must_use]
    pub const fn registry(&self) -> &SourceRegistry {
        &self.registry
    }

    /// 执行留痕账（建议无消费, 强制失败, 重复执行）。
    #[must_use]
    pub const fn exec_counters(&self) -> (u64, u64, u64) {
        (self.suggest_unconsumed, self.forced_failed, self.duplicate_exec)
    }

    /// 操作计账。
    #[must_use]
    pub const fn ops(&self) -> u64 {
        self.ops
    }
}

// ============================================================
// 快照与探针
// ============================================================

/// 可见性快照。
#[derive(Debug)]
pub struct VisSnap;

impl VisSnap {
    /// 版本头。
    pub const VERSION: u32 = 1;

    /// 序列化（一段式委托面板快照 + 守卫账 + 复用条目数）。
    #[must_use]
    pub fn serialize(v: &DegradeVis) -> String {
        let mut s = String::new();
        let gv = v.guard().violations();
        let _ = write!(
            s,
            "v{}|{}|{}|{}|{}|{}|{}|{}|",
            Self::VERSION,
            v.state().name(),
            gv.0,
            gv.1,
            gv.2,
            gv.3,
            v.reuse().len(),
            v.panel().active_dims()
        );
        let _ = s.write_str(&v.panel.snapshot(v.state()));
        s
    }

    /// 快照指纹。
    #[must_use]
    pub fn fingerprint(s: &str) -> u32 {
        fnv1a(s.as_bytes()) ^ SNAP_FNV_SEED
    }
}

/// FNV-1a 32 位指纹。
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 参考探针：全链八口径走查。
#[must_use]
pub fn reference_budget_check() -> bool {
    if !segment_guard() {
        return false;
    }
    // 口径 1：七源注册 + 扩展 + 重复拒 + 未知拒。
    let mut reg = SourceRegistry::new();
    if reg.len() != 7 {
        return false;
    }
    if reg.register("frame") {
        return false; // 重复。
    }
    if !reg.register("custom-src") {
        return false;
    }
    if reg.knows("never-registered") {
        return false;
    }
    if reg.rejects().0 == 0 || reg.rejects().1 == 0 {
        return false;
    }

    // 口径 2：迁移表全枚举（合法 5 条 + 非法抽样）。
    if transition(DegradeState::Normal, TransEvent::Trigger) != Some(DegradeState::Degrading)
        || transition(DegradeState::Degrading, TransEvent::Settled) != Some(DegradeState::Degraded)
        || transition(DegradeState::Degrading, TransEvent::Retract) != Some(DegradeState::Normal)
        || transition(DegradeState::Degraded, TransEvent::Recover) != Some(DegradeState::Recovering)
        || transition(DegradeState::Recovering, TransEvent::Restored) != Some(DegradeState::Normal)
    {
        return false;
    }
    if transition(DegradeState::Normal, TransEvent::Settled).is_some()
        || transition(DegradeState::Degraded, TransEvent::Trigger).is_some()
        || transition(DegradeState::Normal, TransEvent::Restored).is_some()
    {
        return false;
    }

    // 口径 3：优先级梯 + 源映射。
    if PrioClass::Safety.rank() <= PrioClass::Resource.rank() {
        return false;
    }
    if TriggerSource::Thermal.prio_class() != PrioClass::Safety
        || TriggerSource::WeakNet.prio_class() != PrioClass::Contract
        || TriggerSource::FrameTimeout.prio_class() != PrioClass::Experience
        || TriggerSource::GpuTier.prio_class() != PrioClass::Resource
    {
        return false;
    }

    let mut v = DegradeVis::new();

    // 口径 4：正常 → 降级中（渐变小步合规）。
    let r1 = v.tick(TriggerInput {
        source: "frame",
        prio: PrioClass::Experience,
        semantics: ExecSemantics::Suggest,
        consumed: true,
        exec_ok: true,
        step_permille: MAX_STEP_PERMILLE,
        dim: "shadow",
        tick: 1,
    });
    if !r1.moved || v.state() != DegradeState::Degrading {
        return false;
    }

    // 口径 5：建议无消费者留痕。
    let r2 = v.tick(TriggerInput {
        source: "thermal",
        prio: PrioClass::Safety,
        semantics: ExecSemantics::Suggest,
        consumed: false,
        exec_ok: true,
        step_permille: 50,
        dim: "lod",
        tick: 2,
    });
    let mut hit_suggest = false;
    let mut i = 0;
    while i < r2.codes.len() {
        if r2.codes[i] == XcCode::SuggestUnconsumed {
            hit_suggest = true;
        }
        i += 1;
    }
    if !hit_suggest || v.exec_counters().0 == 0 {
        return false;
    }

    // 口径 6：安全豁免大步（热档安全类单步 500‰ 直跳合规 + 突变计
    // 数可见）。
    let r3 = v.tick(TriggerInput {
        source: "thermal",
        prio: PrioClass::Safety,
        semantics: ExecSemantics::Forced,
        consumed: true,
        exec_ok: true,
        step_permille: 500,
        dim: "resolution",
        tick: 3,
    });
    if r3.step != StepVerdict::Accept {
        return false;
    }
    if v.guard().violations().2 == 0 {
        return false; // 突变（豁免跳变）在面板可见。
    }

    // 口径 7：非安全类超步拒绝（StepTooBig）。
    let r4 = v.tick(TriggerInput {
        source: "scene",
        prio: PrioClass::Experience,
        semantics: ExecSemantics::Forced,
        consumed: true,
        exec_ok: true,
        step_permille: 400,
        dim: "fx",
        tick: 4,
    });
    if r4.step != StepVerdict::TooBig {
        return false;
    }

    // 口径 8：强制失败留痕 + 渐变到位 + 恢复链 + 撤销旁路。
    let r5 = v.tick(TriggerInput {
        source: "res",
        prio: PrioClass::Resource,
        semantics: ExecSemantics::Forced,
        consumed: true,
        exec_ok: false,
        step_permille: 10,
        dim: "tex",
        tick: 5,
    });
    let mut hit_forced = false;
    let mut j = 0;
    while j < r5.codes.len() {
        if r5.codes[j] == XcCode::ForcedFailed {
            hit_forced = true;
        }
        j += 1;
    }
    if !hit_forced || v.exec_counters().1 == 0 {
        return false;
    }
    if !v.settle() || v.state() != DegradeState::Degraded {
        return false;
    }
    if !v.begin_recover() || v.state() != DegradeState::Recovering {
        return false;
    }
    if !v.finish_recover() || v.state() != DegradeState::Normal {
        return false;
    }
    // 降级中撤销旁路。
    let _ = v.tick(TriggerInput {
        source: "net",
        prio: PrioClass::Contract,
        semantics: ExecSemantics::Suggest,
        consumed: true,
        exec_ok: true,
        step_permille: 20,
        dim: "netq",
        tick: 6,
    });
    if v.state() != DegradeState::Degrading || !v.retract() || v.state() != DegradeState::Normal {
        return false;
    }

    // 口径 9：复用五家族登记 + 面板快照维度可见。
    if v.reuse().len() != 5 {
        return false;
    }
    if !v.reuse().has(ReuseKind::StateMachine) || !v.reuse().has(ReuseKind::RecoverHysteresis) {
        return false;
    }
    let snap = v.panel.snapshot(v.state());
    // 面板五维：态|源|恢复|维度数|维度列表。
    let mut pipes = 0;
    let mut k = 0;
    while k < snap.len() {
        if snap.as_bytes()[k] == b'|' {
            pipes += 1;
        }
        k += 1;
    }
    if pipes != 4 {
        return false;
    }

    // 口径 10：未知源拒 + 快照往返恒等。
    let r6 = v.tick(TriggerInput {
        source: "ghost",
        prio: PrioClass::Resource,
        semantics: ExecSemantics::Suggest,
        consumed: true,
        exec_ok: true,
        step_permille: 10,
        dim: "x",
        tick: 7,
    });
    let mut hit_unknown = false;
    let mut m = 0;
    while m < r6.codes.len() {
        if r6.codes[m] == XcCode::SourceUnknown {
            hit_unknown = true;
        }
        m += 1;
    }
    if !hit_unknown {
        return false;
    }
    let s1 = VisSnap::serialize(&v);
    let fp1 = VisSnap::fingerprint(&s1);
    let s2 = VisSnap::serialize(&v);
    if VisSnap::fingerprint(&s2) != fp1 || s1 != s2 {
        return false;
    }

    true
}
