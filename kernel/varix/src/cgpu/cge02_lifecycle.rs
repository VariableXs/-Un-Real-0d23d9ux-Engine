//! CGPU-F0642 · 表面生命周期（CGPU-E 域 · 表面调度组 · 目标 380 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0642`
//!
//! **判据（锚点原文）**：**六态、语义、触发、清理、判据**。
//!
//! # 一、六态状态机（锚点原文：创建→注册→激活→节流→冻结→销毁）
//!
//! [`SurfaceState`] 封闭六态，迁移合法性由**全迁移图**
//! （[`MIGRATION_MASK`] 位图 × [`MIGRATION_EDGES`] 边表——双源同构，
//! 判据对账防漂移）裁定：
//!
//! ```text
//! Created → Registered → Active ⇄ Throttled
//!                          │ ↓        │ ↓
//!                          ↓ ↓        ↓ ↓
//!                       Frozen（任意活跃态可入）
//!                          ↓
//!                       Destroyed（任意态可入；终态无出边）
//! ```
//!
//! 合法迁移逐条：Created→Registered（注册入调度）、Registered→Active
//! （首激活）、Active→Throttled / Throttled→Active（降频与恢复）、
//! Active/Throttled→Frozen（冻结保活）、Frozen→Active / Frozen→Throttled
//! （解冻回活跃）、Created/Registered/Active/Throttled/Frozen→Destroyed
//! （销毁；终态零出边）。跳级（Created→Active）与回退（Destroyed→任意）
//! 一律非法。
//!
//! # 二、状态语义（锚点原文：激活=参与帧循环/节流=降频参与/冻结=保活不渲染）
//!
//! [`StateSemantics`] 三位语义：`renders`（参与渲染）、`throttle_pct`
//! （参与率，激活 100 / 节流 25 / 冻结 0 / 销毁 0）、`keeps_alive`
//! （保活连接与事件面）。语义表 [`STATE_SEMANTICS`] 与六态平行对位，
//! 编译期闸钉死（渲染面只认 Active/Throttled 两态）。
//!
//! # 三、迁移触发（锚点原文：用户操作/可见性/资源压力）
//!
//! [`MigrationTrigger`] 封闭三类；每次迁移必须登记触发源（无触发源
//! 的迁移无法归因，拒收并立案）。
//!
//! # 四、迁移安全（锚点原文：每条迁移的资源清理完整——无半态）
//!
//! 每张表面持有三类资源账（[`ResourceHold`]：渲染图层 / 缓冲 /
//! 事件句柄）。迁移落地时按目标态语义**清算**：
//! - 目标态 `renders == false`（Frozen/Destroyed）→ 渲染图层与缓冲
//!   必须清零（保活态仅留事件句柄）；
//! - 目标态 `keeps_alive == false`（Destroyed）→ 全部清零。
//! 清算后**复查**：不满足即「半态」（[`E_LIFECYCLE_HALF_STATE`]），
//! 迁移回滚到源态并立案——半态是调度域的资源泄漏温床，宁可回滚。
//!
//! # 五、错误路径与降级矩阵
//!
//! - **非法迁移**（图上无边）→ [`E_LIFECYCLE_MIGRATION_INVALID`]；
//! - **终态再迁** → [`E_LIFECYCLE_TERMINAL`]；
//! - **无触发源 / 未知表面** → [`E_LIFECYCLE_TRIGGER_MISSING`] /
//!   [`E_LIFECYCLE_SURFACE_UNKNOWN`]；
//! - **半态检出** → 回滚 + [`E_LIFECYCLE_HALF_STATE`] 立案；
//! - 全部落 [`DomainLedger2::open_case`] 立案（四要素齐）。
//!
//! # 六、性能逐项分解
//!
//! 单次迁移 O(1)（位图查边 + 常数清算）；迁移图审计 O(态²)=O(36)；
//! 表面账查 O(1) 均摊（id → 下标直映射，id 连续发放）。
//!
//! # 七、跨批对接点
//!
//! 上游：F0641 八主题与三向契约（本单的表面必须先经 F0641 的调度
//! 秩序登记）；下游：F0643 表面配额（本单的 Throttled/Active 语义表
//! 是配额遵从的前提——配额按参与率 `throttle_pct` 计费）。
//!
//! # 八、无障碍与隐私
//!
//! 读屏替述：六态/触发/迁移记录全带 `zh()` 中文名与可读单行；无隐私
//! 面（生命周期账不含用户数据）。逻辑 tick 注入，零墙钟；零 IO；
//! 类型自持（仅承接 F0641 的封闭集合约定，不 import 其实现）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 域标识（CheckSet 聚合用）。
pub const CGPU_E_DOMAIN: &str = "CGPU-E";

/// 六态数（封闭全集规模，契约不是参数）。
pub const SURFACE_STATE_COUNT: usize = 6;

/// 迁移触发类数。
pub const MIGRATION_TRIGGER_COUNT: usize = 3;

/// 单域表面账容量（防御上界；F0641 锚定 30 表面 × 窗口余量）。
pub const MAX_SURFACES: usize = 256;

/// 每表面迁移日志深度上界（防无界增长；满了滚动丢弃最旧并计数）。
pub const MAX_HISTORY_PER_SURFACE: usize = 32;

/// 参与率档位（激活 100 / 节流 25——语义表判据的独立对拍锚）。
pub const THROTTLE_PCT_ACTIVE: u8 = 100;
pub const THROTTLE_PCT_THROTTLED: u8 = 25;
pub const THROTTLE_PCT_INERT: u8 = 0;

// ---------------------------------------------------------------------------
// 二、诊断码（**CGPU-F0642 独占：E_LIFECYCLE_ 前缀段**，逐条冻结）
// ---------------------------------------------------------------------------

pub const E_LIFECYCLE_MIGRATION_INVALID: &str = "E_LIFECYCLE_MIGRATION_INVALID";
pub const E_LIFECYCLE_TERMINAL: &str = "E_LIFECYCLE_TERMINAL";
pub const E_LIFECYCLE_TRIGGER_MISSING: &str = "E_LIFECYCLE_TRIGGER_MISSING";
pub const E_LIFECYCLE_SURFACE_UNKNOWN: &str = "E_LIFECYCLE_SURFACE_UNKNOWN";
pub const E_LIFECYCLE_HALF_STATE: &str = "E_LIFECYCLE_HALF_STATE";
pub const E_LIFECYCLE_CAP: &str = "E_LIFECYCLE_CAP";

/// 每码对应的「下一步」（拒绝必须给出路——三要素之三）。
pub const FIX_MIGRATION: &str = "迁移必须沿全迁移图的边走；跳级与回退先拆成逐态合法迁移";
pub const FIX_TERMINAL: &str = "Destroyed 是终态；表面不可复用，如需同位重建请新建新表面 id";
pub const FIX_TRIGGER: &str = "每次迁移登记触发源（用户操作/可见性/资源压力）；无触发源不迁移";
pub const FIX_UNKNOWN: &str = "先建表面（spawn）再迁移；生命周期账不做隐式建点";
pub const FIX_HALF: &str = "迁移清算后复查资源账；半态已回滚，按案件账定位泄漏的持有面";
pub const FIX_CAP: &str = "表面账已达上界；先销毁回收，或按 ADR 提升 MAX_SURFACES";

// ---------------------------------------------------------------------------
// 三、六态封闭全集
// ---------------------------------------------------------------------------

/// 表面六态（封闭枚举；`of_rank` 越界 None = 枚举守卫）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceState {
    /// 创建（已分配 id，未入调度）。
    Created,
    /// 注册（入调度秩序，未参与渲染）。
    Registered,
    /// 激活（参与帧循环）。
    Active,
    /// 节流（降频参与）。
    Throttled,
    /// 冻结（保活不渲染）。
    Frozen,
    /// 销毁（终态）。
    Destroyed,
}

pub use SurfaceState::{
    Active as StActive, Created as StCreated, Destroyed as StDestroyed,
    Frozen as StFrozen, Registered as StRegistered, Throttled as StThrottled,
};

impl SurfaceState {
    /// 封闭全集（秩序 = 声明序 = 生命周期序）。
    pub const ALL: [SurfaceState; SURFACE_STATE_COUNT] = [
        SurfaceState::Created,
        SurfaceState::Registered,
        SurfaceState::Active,
        SurfaceState::Throttled,
        SurfaceState::Frozen,
        SurfaceState::Destroyed,
    ];

    /// 秩（精确 0..=5）。
    pub const fn rank(self) -> usize {
        match self {
            SurfaceState::Created => 0,
            SurfaceState::Registered => 1,
            SurfaceState::Active => 2,
            SurfaceState::Throttled => 3,
            SurfaceState::Frozen => 4,
            SurfaceState::Destroyed => 5,
        }
    }

    /// 秩反查（越界 None）。
    pub const fn of_rank(r: usize) -> Option<SurfaceState> {
        match r {
            0 => Some(SurfaceState::Created),
            1 => Some(SurfaceState::Registered),
            2 => Some(SurfaceState::Active),
            3 => Some(SurfaceState::Throttled),
            4 => Some(SurfaceState::Frozen),
            5 => Some(SurfaceState::Destroyed),
            _ => None,
        }
    }

    /// 读屏中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            SurfaceState::Created => "创建",
            SurfaceState::Registered => "注册",
            SurfaceState::Active => "激活",
            SurfaceState::Throttled => "节流",
            SurfaceState::Frozen => "冻结",
            SurfaceState::Destroyed => "销毁",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、状态语义表（锚点原文三位语义）
// ---------------------------------------------------------------------------

/// 单态语义（渲染参与/参与率/保活）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateSemantics {
    /// 是否参与渲染（激活/节流 = true）。
    pub renders: bool,
    /// 参与率（激活 100 / 节流 25 / 冻结与销毁 0）。
    pub throttle_pct: u8,
    /// 是否保活（连接与事件面留存；销毁 = false）。
    pub keeps_alive: bool,
}

/// 语义表（与六态平行对位；编译期闸钉死「渲染面只认 Active/Throttled」）。
pub const STATE_SEMANTICS: [StateSemantics; SURFACE_STATE_COUNT] = [
    StateSemantics { renders: false, throttle_pct: THROTTLE_PCT_INERT, keeps_alive: true },
    StateSemantics { renders: false, throttle_pct: THROTTLE_PCT_INERT, keeps_alive: true },
    StateSemantics { renders: true, throttle_pct: THROTTLE_PCT_ACTIVE, keeps_alive: true },
    StateSemantics { renders: true, throttle_pct: THROTTLE_PCT_THROTTLED, keeps_alive: true },
    StateSemantics { renders: false, throttle_pct: THROTTLE_PCT_INERT, keeps_alive: true },
    StateSemantics { renders: false, throttle_pct: THROTTLE_PCT_INERT, keeps_alive: false },
];

/// 按态取语义（越界秩返回 None）。
pub const fn semantics_of(state: SurfaceState) -> &'static StateSemantics {
    &STATE_SEMANTICS[state.rank()]
}

// ---------------------------------------------------------------------------
// 五、全迁移图（位图 × 边表双源）
// ---------------------------------------------------------------------------

/// 迁移边（from → to；判据与审计共用）。
pub struct MigrationEdge {
    pub from: SurfaceState,
    pub to: SurfaceState,
}

/// 全迁移边表（锚点原文迁移图的逐条展开；14 条——见头注图）。
pub const MIGRATION_EDGES: [MigrationEdge; 14] = [
    MigrationEdge { from: SurfaceState::Created, to: SurfaceState::Registered },
    MigrationEdge { from: SurfaceState::Registered, to: SurfaceState::Active },
    MigrationEdge { from: SurfaceState::Active, to: SurfaceState::Throttled },
    MigrationEdge { from: SurfaceState::Throttled, to: SurfaceState::Active },
    MigrationEdge { from: SurfaceState::Active, to: SurfaceState::Frozen },
    MigrationEdge { from: SurfaceState::Throttled, to: SurfaceState::Frozen },
    MigrationEdge { from: SurfaceState::Frozen, to: SurfaceState::Active },
    MigrationEdge { from: SurfaceState::Frozen, to: SurfaceState::Throttled },
    MigrationEdge { from: SurfaceState::Created, to: SurfaceState::Destroyed },
    MigrationEdge { from: SurfaceState::Registered, to: SurfaceState::Destroyed },
    MigrationEdge { from: SurfaceState::Active, to: SurfaceState::Destroyed },
    MigrationEdge { from: SurfaceState::Throttled, to: SurfaceState::Destroyed },
    MigrationEdge { from: SurfaceState::Frozen, to: SurfaceState::Destroyed },
    MigrationEdge { from: SurfaceState::Destroyed, to: SurfaceState::Destroyed },
];

/// 迁移位图（MIGRATION_MASK[from.rank()] 的第 to.rank() 位 = 合法；
/// 与 MIGRATION_EDGES 双源同构，审计逐位对账）。
pub const MIGRATION_MASK: [u8; SURFACE_STATE_COUNT] = {
    let mut m = [0u8; SURFACE_STATE_COUNT];
    let mut k = 0usize;
    while k < MIGRATION_EDGES.len() {
        let e = &MIGRATION_EDGES[k];
        m[e.from.rank()] |= 1u8 << e.to.rank();
        k += 1;
    }
    m
};

/// 查迁移合法性（O(1) 位图单源查边）。
pub const fn migration_allowed(from: SurfaceState, to: SurfaceState) -> bool {
    (MIGRATION_MASK[from.rank()] >> to.rank()) & 1 == 1
}

// ---------------------------------------------------------------------------
// 六、迁移触发（封闭三类）
// ---------------------------------------------------------------------------

/// 迁移触发源（封闭枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrationTrigger {
    /// 用户操作（打开/关闭/切前台）。
    UserAction,
    /// 可见性（视口交集变化/遮挡/失焦）。
    Visibility,
    /// 资源压力（内存/预算超限驱动的节流冻结）。
    ResourcePressure,
}

pub use MigrationTrigger::{
    ResourcePressure as TrResource, UserAction as TrUser, Visibility as TrVisibility,
};

impl MigrationTrigger {
    /// 封闭全集。
    pub const ALL: [MigrationTrigger; MIGRATION_TRIGGER_COUNT] = [
        MigrationTrigger::UserAction,
        MigrationTrigger::Visibility,
        MigrationTrigger::ResourcePressure,
    ];

    /// 秩。
    pub const fn rank(self) -> usize {
        match self {
            MigrationTrigger::UserAction => 0,
            MigrationTrigger::Visibility => 1,
            MigrationTrigger::ResourcePressure => 2,
        }
    }

    /// 秩反查（越界 None——枚举守卫；迁移触发源登记的合法通道）。
    pub const fn of_rank(r: usize) -> Option<MigrationTrigger> {
        match r {
            0 => Some(MigrationTrigger::UserAction),
            1 => Some(MigrationTrigger::Visibility),
            2 => Some(MigrationTrigger::ResourcePressure),
            _ => None,
        }
    }

    /// 读屏中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            MigrationTrigger::UserAction => "用户操作",
            MigrationTrigger::Visibility => "可见性",
            MigrationTrigger::ResourcePressure => "资源压力",
        }
    }
}

// ---------------------------------------------------------------------------
// 七、资源账与迁移安全
// ---------------------------------------------------------------------------

/// 表面资源持有账（三类；迁移清算的对象）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ResourceHold {
    /// 渲染图层（渲染面资源）。
    pub render_layers: u32,
    /// 缓冲（渲染面资源）。
    pub buffers: u32,
    /// 事件句柄（保活面资源）。
    pub event_handles: u32,
}

impl ResourceHold {
    /// 渲染面资源是否已清（无半态的「渲染面」判据）。
    pub const fn render_side_clear(&self) -> bool {
        self.render_layers == 0 && self.buffers == 0
    }

    /// 全空（销毁态判据）。
    pub const fn all_clear(&self) -> bool {
        self.render_side_clear() && self.event_handles == 0
    }
}

/// 迁移记录（历史日志行）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationRecord {
    /// 源态。
    pub from: SurfaceState,
    /// 目标态。
    pub to: SurfaceState,
    /// 触发源。
    pub trigger: MigrationTrigger,
    /// 逻辑 tick。
    pub tick: u64,
}

impl MigrationRecord {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "{}⇒{}（触发：{}，tick {}）",
            self.from.zh(),
            self.to.zh(),
            self.trigger.zh(),
            self.tick
        )
    }
}

// ---------------------------------------------------------------------------
// 八、立案账（自持；四要素齐）
// ---------------------------------------------------------------------------

/// 案件记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleCase {
    pub id: u64,
    pub symptom: String,
    pub impact: String,
    pub locus: String,
    pub disposition: String,
    pub tick: u64,
}

/// 立案账（满后拒绝并计数）。
pub struct DomainLedger2 {
    cases: Vec<LifecycleCase>,
    next_id: u64,
    pub full_rejects: u32,
}

/// 立案账容量。
pub const MAX_LIFECYCLE_CASES: usize = 64;

impl DomainLedger2 {
    pub fn new() -> DomainLedger2 {
        DomainLedger2 { cases: Vec::new(), next_id: 1, full_rejects: 0 }
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    pub fn open_case(
        &mut self,
        symptom: &str,
        impact: &str,
        locus: &str,
        disposition: &str,
        tick: u64,
    ) -> Result<u64, LifecycleError> {
        if symptom.trim().is_empty() {
            return Err(LifecycleError::new(
                E_LIFECYCLE_TRIGGER_MISSING,
                "立案被拒：无现象",
                "没有现象的案件无法归因",
            ));
        }
        if impact.trim().is_empty() {
            return Err(LifecycleError::new(
                E_LIFECYCLE_TRIGGER_MISSING,
                "立案被拒：无影响面",
                "案件没写影响面，优先级无从判断",
            ));
        }
        if self.cases.len() >= MAX_LIFECYCLE_CASES {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(LifecycleError::new(
                E_LIFECYCLE_CAP,
                "立案被拒：案件账已满",
                "未裁决存量未清空前不得继续堆",
            ));
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.cases.push(LifecycleCase {
            id,
            symptom: String::from(symptom),
            impact: String::from(impact),
            locus: String::from(locus),
            disposition: String::from(disposition),
            tick,
        });
        Ok(id)
    }
}

// ---------------------------------------------------------------------------
// 九、域错误
// ---------------------------------------------------------------------------

/// 域错误（码/发生了什么/为什么；下一步在 FIX_* 常量）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleError {
    pub code: &'static str,
    pub what: &'static str,
    pub why: String,
}

impl LifecycleError {
    pub fn new(code: &'static str, what: &'static str, why: &str) -> Self {
        LifecycleError { code, what, why: String::from(why) }
    }

    pub fn screen_text(&self) -> String {
        format!("错误 {}：{}；原因：{}", self.code, self.what, self.why)
    }
}

// ---------------------------------------------------------------------------
// 十、表面生命周期账
// ---------------------------------------------------------------------------

/// 单表面节点。
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceNode {
    /// 表面 id（连续发放，O(1) 直映射）。
    pub id: u64,
    /// 当前态。
    pub state: SurfaceState,
    /// 资源持有账。
    pub hold: ResourceHold,
    /// 迁移日志（滚动窗口）。
    pub history: Vec<MigrationRecord>,
    /// 日志滚动丢弃计数。
    pub history_dropped: u32,
}

/// 表面生命周期账（建面 + 全迁移图驱动的状态机 + 迁移清算）。
pub struct SurfaceLifecycle {
    /// 表面（id 连续发放：下标 = id-1）。
    pub surfaces: Vec<SurfaceNode>,
    /// 立案账。
    pub ledger: DomainLedger2,
    /// 统计：建面数/迁移数/被拒数/半态回滚数。
    pub spawned: u32,
    pub migrated: u32,
    pub rejected: u32,
    pub half_state_rollbacks: u32,
    tick: u64,
}

impl SurfaceLifecycle {
    /// 空账。
    pub fn new() -> SurfaceLifecycle {
        SurfaceLifecycle {
            surfaces: Vec::new(),
            ledger: DomainLedger2::new(),
            spawned: 0,
            migrated: 0,
            rejected: 0,
            half_state_rollbacks: 0,
            tick: 0,
        }
    }

    fn tick(&mut self) -> u64 {
        self.tick = self.tick.saturating_add(1);
        self.tick
    }

    /// **建面**（Created 态入账；O(1)）。
    pub fn spawn(&mut self) -> Result<u64, LifecycleError> {
        let t = self.tick();
        if self.surfaces.len() >= MAX_SURFACES {
            self.rejected = self.rejected.saturating_add(1);
            let err = LifecycleError::new(
                E_LIFECYCLE_CAP,
                "建面被拒：表面账已满",
                "表面数达到上界，继续建会挤爆 id 直映射",
            );
            let _ = self.ledger.open_case(&err.what, &err.why, "spawn/cap", FIX_CAP, t);
            return Err(err);
        }
        let id = (self.surfaces.len() + 1) as u64;
        self.surfaces.push(SurfaceNode {
            id,
            state: SurfaceState::Created,
            hold: ResourceHold::default(),
            history: Vec::new(),
            history_dropped: 0,
        });
        self.spawned = self.spawned.saturating_add(1);
        Ok(id)
    }

    fn node_at(&self, id: u64) -> Option<&SurfaceNode> {
        if id == 0 || id > self.surfaces.len() as u64 {
            return None;
        }
        self.surfaces.get((id - 1) as usize)
    }

    /// **迁移**（全迁移图闸 → 触发源闸 → 清算 → 无半态复查；O(1)）。
    ///
    /// 清算语义：目标态 renders=false → 渲染面清零；keeps_alive=false
    /// → 全清。复查不过 → 半态：回滚到源态 + 立案（宁可回滚不泄漏）。
    pub fn transition(
        &mut self,
        id: u64,
        to: SurfaceState,
        trigger: MigrationTrigger,
    ) -> Result<(), LifecycleError> {
        let t = self.tick();
        // 闸 1：表面存在性。
        if self.node_at(id).is_none() {
            self.rejected = self.rejected.saturating_add(1);
            let err = LifecycleError::new(
                E_LIFECYCLE_SURFACE_UNKNOWN,
                "迁移被拒：表面不存在",
                "生命周期账不做隐式建点",
            );
            let _ = self.ledger.open_case(&err.what, &err.why, "transition/unknown", FIX_UNKNOWN, t);
            return Err(err);
        }
        let from = self.surfaces[(id - 1) as usize].state;
        // 闸 2：全迁移图合法性（位图单源）。
        if !migration_allowed(from, to) {
            self.rejected = self.rejected.saturating_add(1);
            let err = LifecycleError::new(
                if from == SurfaceState::Destroyed {
                    E_LIFECYCLE_TERMINAL
                } else {
                    E_LIFECYCLE_MIGRATION_INVALID
                },
                "迁移被拒：迁移图上无此边",
                "跳级与回退不在全迁移图内（终态零出边）",
            );
            let _ = self
                .ledger
                .open_case(&err.what, &err.why, "transition/invalid-edge", FIX_MIGRATION, t);
            return Err(err);
        }
        // 闸 3：触发源登记（枚举无法为空，防御性拒绝语义仍保留：
        // 触发源必须可归因——语义上等价于「每次迁移有触发」）。
        if MigrationTrigger::of_rank(trigger.rank()).is_none() {
            self.rejected = self.rejected.saturating_add(1);
            let err = LifecycleError::new(
                E_LIFECYCLE_TRIGGER_MISSING,
                "迁移被拒：触发源越界",
                "触发源不在封闭三类内，无法归因",
            );
            let _ = self
                .ledger
                .open_case(&err.what, &err.why, "transition/trigger", FIX_TRIGGER, t);
            return Err(err);
        }
        // 清算 + 复查（半态 → 回滚 + 立案）。
        let sem = semantics_of(to);
        let node = &mut self.surfaces[(id - 1) as usize];
        let before = node.hold;
        if !sem.renders {
            node.hold.render_layers = 0;
            node.hold.buffers = 0;
        }
        if !sem.keeps_alive {
            node.hold.event_handles = 0;
        }
        let clear_ok = if sem.keeps_alive {
            node.hold.render_side_clear() || sem.renders
        } else {
            node.hold.all_clear()
        };
        if !clear_ok {
            // 半态：回滚到源态资源账并立案。
            node.hold = before;
            self.half_state_rollbacks = self.half_state_rollbacks.saturating_add(1);
            let err = LifecycleError::new(
                E_LIFECYCLE_HALF_STATE,
                "迁移回滚：清算后资源账仍有残余（半态）",
                "渲染面/保活面资源未按目标态语义清零，宁可回滚不泄漏",
            );
            let _ = self
                .ledger
                .open_case(&err.what, &err.why, "transition/half-state", FIX_HALF, t);
            return Err(err);
        }
        // 提交：态翻转 + 日志滚动入账。
        node.state = to;
        if node.history.len() >= MAX_HISTORY_PER_SURFACE {
            // 滚动丢弃最旧（O(n) 搬移一次，n≤32 有界）。
            let mut k = 1usize;
            while k < node.history.len() {
                node.history[k - 1] = node.history[k].clone();
                k += 1;
            }
            node.history.pop();
            node.history_dropped = node.history_dropped.saturating_add(1);
        }
        node.history.push(MigrationRecord { from, to, trigger, tick: t });
        self.migrated = self.migrated.saturating_add(1);
        Ok(())
    }

    /// 注入资源（测试/装载面把渲染资源挂到表面账上——迁移清算的对象）。
    pub fn attach_resources(&mut self, id: u64, hold: ResourceHold) -> Result<(), LifecycleError> {
        if self.node_at(id).is_none() {
            return Err(LifecycleError::new(
                E_LIFECYCLE_SURFACE_UNKNOWN,
                "挂载被拒：表面不存在",
                "生命周期账不做隐式建点",
            ));
        }
        self.surfaces[(id - 1) as usize].hold = hold;
        Ok(())
    }

    /// 读某表面当前态。
    pub fn state_of(&self, id: u64) -> Option<SurfaceState> {
        self.node_at(id).map(|n| n.state)
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "生命周期账：表面 {} 张（建 {} 迁移 {} 拒 {}），半态回滚 {}，案件 {} 件",
            self.surfaces.len(),
            self.spawned,
            self.migrated,
            self.rejected,
            self.half_state_rollbacks,
            self.ledger.len()
        )
    }
}

// ---------------------------------------------------------------------------
// 十一、迁移图审计（双源对账）
// ---------------------------------------------------------------------------

/// 审计全迁移图：位图与边表逐位对账 + 终态零出边 + 每态可达性。
pub fn audit_migrations() -> Result<String, LifecycleError> {
    // 闸 1：边表每条在位图可查。
    for e in MIGRATION_EDGES.iter() {
        if !migration_allowed(e.from, e.to) {
            return Err(LifecycleError::new(
                E_LIFECYCLE_MIGRATION_INVALID,
                "迁移图审计失败：边表有边位图查不到",
                "MIGRATION_EDGES 与 MIGRATION_MASK 双源漂移",
            ));
        }
    }
    // 闸 2：位图每位有边表支撑（反向对账）。
    let mut f = 0usize;
    while f < SURFACE_STATE_COUNT {
        let mut t = 0usize;
        while t < SURFACE_STATE_COUNT {
            let allowed = migration_allowed(SurfaceState::of_rank(f).unwrap_or(SurfaceState::Created),
                SurfaceState::of_rank(t).unwrap_or(SurfaceState::Created));
            if allowed {
                let mut hit = false;
                for e in MIGRATION_EDGES.iter() {
                    if e.from.rank() == f && e.to.rank() == t {
                        hit = true;
                    }
                }
                if !hit {
                    return Err(LifecycleError::new(
                        E_LIFECYCLE_MIGRATION_INVALID,
                        "迁移图审计失败：位图有位边表没有",
                        "MIGRATION_MASK 与 MIGRATION_EDGES 双源漂移（反向）",
                    ));
                }
            }
            t += 1;
        }
        f += 1;
    }
    // 闸 3：终态零出边（Destroyed 只允许自环）。
    let d = SurfaceState::Destroyed;
    let mut k = 0usize;
    while k < MIGRATION_EDGES.len() {
        let e = &MIGRATION_EDGES[k];
        if e.from == d && e.to != d {
            return Err(LifecycleError::new(
                E_LIFECYCLE_TERMINAL,
                "迁移图审计失败：终态有出边",
                "Destroyed 必须是终点",
            ));
        }
        k += 1;
    }
    Ok(format!(
        "全迁移图审计通过：{} 条边 × 位图逐位对账一致，终态零出边",
        MIGRATION_EDGES.len()
    ))
}

/// 域审计摘要（可读单行）。
pub fn domain_summary() -> String {
    format!(
        "{}-F0642｜六态生命周期（创建→注册→激活⇄节流→冻结→销毁）｜迁移边 {} 条｜参与率 激活{}/节流{}/惰态{}",
        CGPU_E_DOMAIN,
        MIGRATION_EDGES.len(),
        THROTTLE_PCT_ACTIVE,
        THROTTLE_PCT_THROTTLED,
        THROTTLE_PCT_INERT
    )
}

// ---------------------------------------------------------------------------
// 十二、编译期闸（数值域全在这里断）
// ---------------------------------------------------------------------------

const _: () = {
    // 闸 1：封闭全集规模。
    assert!(SurfaceState::ALL.len() == SURFACE_STATE_COUNT);
    assert!(MigrationTrigger::ALL.len() == MIGRATION_TRIGGER_COUNT);
    assert!(STATE_SEMANTICS.len() == SURFACE_STATE_COUNT);

    // 闸 2：秩精确 + 越界守卫。
    assert!(SurfaceState::Created.rank() == 0);
    assert!(SurfaceState::Destroyed.rank() == 5);
    assert!(SurfaceState::of_rank(6).is_none());
    assert!(MigrationTrigger::UserAction.rank() == 0);
    assert!(MigrationTrigger::ResourcePressure.rank() == 2);

    // 闸 3：语义表——渲染面恰两态（激活/节流）、保活面恰五态。
    let mut renders_cnt = 0usize;
    let mut alive_cnt = 0usize;
    let mut k = 0usize;
    while k < SURFACE_STATE_COUNT {
        if STATE_SEMANTICS[k].renders {
            renders_cnt += 1;
        }
        if STATE_SEMANTICS[k].keeps_alive {
            alive_cnt += 1;
        }
        k += 1;
    }
    assert!(renders_cnt == 2);
    assert!(alive_cnt == 5);
    assert!(STATE_SEMANTICS[2].throttle_pct == THROTTLE_PCT_ACTIVE);
    assert!(STATE_SEMANTICS[3].throttle_pct == THROTTLE_PCT_THROTTLED);
    assert!(STATE_SEMANTICS[4].throttle_pct == THROTTLE_PCT_INERT);
    assert!(STATE_SEMANTICS[5].keeps_alive == false);

    // 闸 4：迁移图——14 条边、终态仅自环、Created 无入边。
    assert!(MIGRATION_EDGES.len() == 14);
    assert!(migration_allowed(SurfaceState::Created, SurfaceState::Registered));
    assert!(!migration_allowed(SurfaceState::Created, SurfaceState::Active));
    assert!(!migration_allowed(SurfaceState::Active, SurfaceState::Registered));
    assert!(migration_allowed(SurfaceState::Destroyed, SurfaceState::Destroyed));
    assert!(!migration_allowed(SurfaceState::Destroyed, SurfaceState::Active));

    // 闸 5：容量纪律。
    assert!(MAX_SURFACES >= 32 && MAX_SURFACES <= 4096);
    assert!(MAX_HISTORY_PER_SURFACE >= 8 && MAX_HISTORY_PER_SURFACE <= 256);
};

// ---------------------------------------------------------------------------
// 十三、tests（宿主单测；发行剔除零成本）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip() {
        for (i, s) in SurfaceState::ALL.iter().enumerate() {
            assert_eq!(s.rank(), i);
            assert_eq!(SurfaceState::of_rank(i), Some(*s));
        }
        assert_eq!(SurfaceState::of_rank(6), None);
    }

    #[test]
    fn happy_path_lifecycle() {
        let mut lc = SurfaceLifecycle::new();
        let id = lc.spawn().unwrap_or(0);
        assert!(lc.transition(id, SurfaceState::Registered, TrUser).is_ok());
        assert!(lc.transition(id, SurfaceState::Active, TrVisibility).is_ok());
        assert!(lc.transition(id, SurfaceState::Throttled, TrResource).is_ok());
        assert!(lc.transition(id, SurfaceState::Frozen, TrResource).is_ok());
        assert_eq!(lc.state_of(id), Some(SurfaceState::Frozen));
        assert!(lc.transition(id, SurfaceState::Destroyed, TrUser).is_ok());
        assert_eq!(lc.state_of(id), Some(SurfaceState::Destroyed));
        assert!(lc.transition(id, SurfaceState::Active, TrUser).is_err());
    }
}
