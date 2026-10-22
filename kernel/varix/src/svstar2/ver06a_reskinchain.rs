//! VE-F3421 · 实时换肤引擎全链 —— 预演/影响面/编排/事务四件 + 两律（不中断、可回滚）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3421`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四件总纲**：一次换肤必须依次经过四个部件，缺一件就不许提交：
//!   1. **预演**（[`PreviewOutcome`]）——先在沙箱里算一遍，预演本身有
//!      耗时预算，超预算就取消（预演不许拖慢切换）；
//!   2. **影响面**（[`ImpactSummary`]）——哪些组件会变、变到什么程度；
//!   3. **编排**（[`Choreography`]）——过渡步骤与总时长；
//!   4. **事务**（[`Snapshot`] + [`ReskinEngine::commit`]）——提交前留快照，
//!      失败可整链回退；
//! - **两律**：
//!   - **不中断**：换肤期间世界照常运转。引擎在 `begin` 时要求调用方
//!     显式声明交互通道已接管（[`ReskinRequest::interaction_handoff`]），
//!     没有声明就不许开始——「忘记了」和「做不到」在门禁上是同一件事；
//!   - **可回滚**：提交前必须有快照，且快照的主题必须与请求**不同**
//!     （同主题换肤没有回滚意义，直接判 [`ReskinCode::NoOpReskin`]）；
//! - **预演先行**：`Phase` 的顺序是硬编码的，跳步一律
//!   [`ReskinCode::PhaseOutOfOrder`]。状态机比文档更可靠——文档会被跳过，
//!   状态机会红。
//!
//! # 为什么换肤不是重启
//!
//! 重启式换肤的实现最省事：卸掉整棵树、重建、重绘。代价是用户点下切换
//! 的那一瞬间界面消失几百毫秒——这在一个「实时」换肤的引擎里是不可接受
//! 的产品缺陷。所以本模块把「不中断」写成**入口条件**：请求里必须带交互
//! 接管声明，否则连预演都不开始。这不是形式主义——它逼着调用方在写换肤
//! 代码之前就想清楚「换肤时用户点按钮会发生什么」。
//!
//! # 复杂度
//!
//! - `begin`：O(变更数)（一次校验 + 一次快照记录）；
//! - `preview` / `impact` / `choreograph`：O(1)（只记账，实际计算在各自模块）；
//! - `commit` / `rollback`：O(1)；
//! - `journal`：O(事件数)（只给读屏与审计用）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`；所有状态迁移走 `match` 并返回错误。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段：E02 组独占，5401 起）
// ---------------------------------------------------------------------------

/// 换肤引擎诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReskinCode {
    /// 阶段乱序（跳步或回退到已过阶段）。
    PhaseOutOfOrder = 5401,
    /// 变更集为空。
    EmptyChangeSet = 5402,
    /// 同一路径在一次请求里出现多次（单源律）。
    DuplicateChange = 5403,
    /// 未声明交互接管（违反不中断律）。
    InteractionNotHandedOff = 5404,
    /// 预演失败。
    PreviewFailed = 5405,
    /// 预演超出耗时预算。
    PreviewOverBudget = 5406,
    /// 影响面未计算就编排。
    ImpactMissing = 5407,
    /// 编排为空（没有可执行步骤）。
    EmptyChoreography = 5408,
    /// 编排总时长超过换肤预算。
    ChoreographyOverBudget = 5409,
    /// 提交时无快照（违反可回滚律）。
    SnapshotMissing = 5410,
    /// 目标主题与当前主题相同（无操作换肤）。
    NoOpReskin = 5411,
    /// 已在执行中（重复 begin）。
    AlreadyRunning = 5412,
    /// 回滚时没有可回滚的状态。
    NothingToRollback = 5413,
    /// 主题 id 为空。
    EmptyTheme = 5414,
    /// 预算为 0（任何换肤都不可能在 0ms 内完成）。
    ZeroBudget = 5415,
}

impl ReskinCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ReskinCode::PhaseOutOfOrder => "换肤阶段乱序",
            ReskinCode::EmptyChangeSet => "变更集为空",
            ReskinCode::DuplicateChange => "同一路径重复变更",
            ReskinCode::InteractionNotHandedOff => "未声明交互接管",
            ReskinCode::PreviewFailed => "预演失败",
            ReskinCode::PreviewOverBudget => "预演超出耗时预算",
            ReskinCode::ImpactMissing => "影响面未计算",
            ReskinCode::EmptyChoreography => "编排为空",
            ReskinCode::ChoreographyOverBudget => "编排超出换肤预算",
            ReskinCode::SnapshotMissing => "提交时无快照",
            ReskinCode::NoOpReskin => "目标主题与当前相同",
            ReskinCode::AlreadyRunning => "换肤已在执行",
            ReskinCode::NothingToRollback => "没有可回滚的状态",
            ReskinCode::EmptyTheme => "主题标识为空",
            ReskinCode::ZeroBudget => "预算为 0",
        }
    }

    /// 是否为「违反两律」类错误（这类错误必须整改，不是调参能过的）。
    pub fn is_law_violation(self) -> bool {
        matches!(
            self,
            ReskinCode::InteractionNotHandedOff | ReskinCode::SnapshotMissing
        )
    }
}

// ---------------------------------------------------------------------------
// 二、阶段状态机
// ---------------------------------------------------------------------------

/// 换肤阶段。顺序即契约：Preview → Impact → Choreograph → Commit。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 空闲（可以 begin）。
    Idle,
    /// 预演中。
    Previewing,
    /// 影响面已就绪。
    Impacted,
    /// 编排已就绪。
    Choreographed,
    /// 已提交。
    Committed,
    /// 回滚中。
    RollingBack,
    /// 已回滚（回到 Idle 前的安全态）。
    RolledBack,
    /// 已取消（预演失败/超预算/主动取消）。
    Cancelled,
}

impl Phase {
    /// 中文名（读屏播报）。
    pub fn label(self) -> &'static str {
        match self {
            Phase::Idle => "空闲",
            Phase::Previewing => "预演中",
            Phase::Impacted => "影响面就绪",
            Phase::Choreographed => "编排就绪",
            Phase::Committed => "已提交",
            Phase::RollingBack => "回滚中",
            Phase::RolledBack => "已回滚",
            Phase::Cancelled => "已取消",
        }
    }

    /// 是否终态（终态不许自动前进）。
    pub fn is_terminal(self) -> bool {
        matches!(self, Phase::Committed | Phase::RolledBack | Phase::Cancelled)
    }
}

/// 一次阶段事件（审计链）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PhaseEvent {
    /// 从哪来。
    pub from: Phase,
    /// 到哪去。
    pub to: Phase,
    /// 说明（失败原因等）。
    pub note: String,
    /// 发生的逻辑时刻（tick，由调用方给）。
    pub tick: u64,
}

// ---------------------------------------------------------------------------
// 三、请求与四件产物
// ---------------------------------------------------------------------------

/// 一条令牌变更。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Change {
    /// 令牌路径。
    pub path: String,
    /// 旧值（渲染文本，如 `#4C8DFF`）。
    pub old: String,
    /// 新值。
    pub new: String,
}

impl Change {
    /// 构造。
    pub fn new(path: &str, old: &str, new: &str) -> Change {
        Change {
            path: path.to_string(),
            old: old.to_string(),
            new: new.to_string(),
        }
    }

    /// 是否真的变了。
    pub fn differs(&self) -> bool {
        self.old != self.new
    }
}

/// 换肤请求。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReskinRequest {
    /// 目标主题 id。
    pub theme: String,
    /// 当前主题 id。
    pub current_theme: String,
    /// 变更集。
    pub changes: Vec<Change>,
    /// 交互接管声明：换肤期间输入由谁接管（空 = 未声明）。
    pub interaction_handoff: String,
    /// 换肤总预算（毫秒）。
    pub budget_ms: u32,
}

impl ReskinRequest {
    /// 构造一个「已声明接管」的请求（最常用形态）。
    pub fn new(theme: &str, current: &str, budget_ms: u32) -> ReskinRequest {
        ReskinRequest {
            theme: theme.to_string(),
            current_theme: current.to_string(),
            changes: Vec::new(),
            interaction_handoff: "reskin.buffer".to_string(),
            budget_ms,
        }
    }

    /// 追加变更。
    pub fn with_change(mut self, path: &str, old: &str, new: &str) -> ReskinRequest {
        self.changes.push(Change::new(path, old, new));
        self
    }

    /// 校验：主题非空、非无操作、预算非零、变更非空且路径唯一、接管已声明。
    pub fn validate(&self) -> Result<(), ReskinCode> {
        if self.theme.trim().is_empty() {
            return Err(ReskinCode::EmptyTheme);
        }
        if self.budget_ms == 0 {
            return Err(ReskinCode::ZeroBudget);
        }
        if self.changes.is_empty() {
            return Err(ReskinCode::EmptyChangeSet);
        }
        if self.interaction_handoff.trim().is_empty() {
            return Err(ReskinCode::InteractionNotHandedOff);
        }
        let mut seen: Vec<&str> = Vec::new();
        for c in self.changes.iter() {
            if seen.iter().any(|p| *p == c.path.as_str()) {
                return Err(ReskinCode::DuplicateChange);
            }
            seen.push(c.path.as_str());
        }
        if self.theme == self.current_theme {
            return Err(ReskinCode::NoOpReskin);
        }
        Ok(())
    }
}

/// 预演结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PreviewOutcome {
    /// 预演耗时（毫秒）。
    pub cost_ms: u32,
    /// 是否成功。
    pub ok: bool,
    /// 被拒的路径（预演里就判死的变更）。
    pub rejected: Vec<String>,
}

/// 影响分级。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ImpactLevel {
    /// 无感（不影响可见像素）。
    Imperceptible,
    /// 局部重绘。
    LocalRepaint,
    /// 全量重绘。
    FullRepaint,
}

impl ImpactLevel {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            ImpactLevel::Imperceptible => "无感",
            ImpactLevel::LocalRepaint => "局部重绘",
            ImpactLevel::FullRepaint => "全量重绘",
        }
    }
}

/// 影响面摘要（明细由 F3422 计算，本模块只消费结论）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImpactSummary {
    /// 受影响组件数。
    pub affected: u32,
    /// 分级。
    pub level: ImpactLevel,
}

/// 编排步骤。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Step {
    /// 步骤名（`fade-out` / `apply` / `fade-in` …）。
    pub name: String,
    /// 该步时长（毫秒）。
    pub duration_ms: u32,
}

/// 编排。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Choreography {
    /// 步骤序列。
    pub steps: Vec<Step>,
}

impl Choreography {
    /// 总时长。
    pub fn total_ms(&self) -> u32 {
        let mut total = 0u32;
        for s in self.steps.iter() {
            total = total.saturating_add(s.duration_ms);
        }
        total
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }
}

/// 快照（可回滚的物理依据）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Snapshot {
    /// 快照号（单调）。
    pub id: u64,
    /// 主题 id。
    pub theme: String,
    /// 令牌条数。
    pub token_count: u32,
}

// ---------------------------------------------------------------------------
// 四、引擎
// ---------------------------------------------------------------------------

/// 换肤引擎。单实例单请求：并发换肤在协议层就该被拒绝，不在引擎里做
/// 「排队」——排队会让用户点了没反应，比报错更糟。
#[derive(Clone, Debug)]
pub struct ReskinEngine {
    phase: Phase,
    request: Option<ReskinRequest>,
    preview: Option<PreviewOutcome>,
    impact: Option<ImpactSummary>,
    choreography: Option<Choreography>,
    snapshot: Option<Snapshot>,
    next_snapshot: u64,
    journal: Vec<PhaseEvent>,
    /// 已完成的换肤次数（遥测）。
    committed: u32,
    /// 回滚次数。
    rolled_back: u32,
}

impl Default for ReskinEngine {
    fn default() -> Self {
        ReskinEngine::new()
    }
}

impl ReskinEngine {
    /// 新引擎。
    pub fn new() -> ReskinEngine {
        ReskinEngine {
            phase: Phase::Idle,
            request: None,
            preview: None,
            impact: None,
            choreography: None,
            snapshot: None,
            next_snapshot: 1,
            journal: Vec::new(),
            committed: 0,
            rolled_back: 0,
        }
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 开始一次换肤。校验请求 + 建快照（快照在预演之前就建：预演也可能
    /// 改沙箱状态，早建早安全）。
    pub fn begin(&mut self, request: ReskinRequest, tick: u64) -> Result<(), ReskinCode> {
        if !matches!(self.phase, Phase::Idle | Phase::Committed | Phase::RolledBack | Phase::Cancelled)
        {
            return Err(ReskinCode::AlreadyRunning);
        }
        request.validate()?;
        let snapshot = Snapshot {
            id: self.next_snapshot,
            theme: request.current_theme.clone(),
            token_count: request.changes.len() as u32,
        };
        self.next_snapshot = self.next_snapshot.saturating_add(1);
        self.snapshot = Some(snapshot);
        self.request = Some(request);
        self.preview = None;
        self.impact = None;
        self.choreography = None;
        self.transition(Phase::Previewing, "", tick);
        Ok(())
    }

    /// 提交预演结果。失败或超预算一律取消（锚点：预演失败→取消）。
    pub fn preview(&mut self, outcome: PreviewOutcome, tick: u64) -> Result<(), ReskinCode> {
        if self.phase != Phase::Previewing {
            return Err(ReskinCode::PhaseOutOfOrder);
        }
        let budget = match self.request.as_ref() {
            Some(r) => r.budget_ms,
            None => 0,
        };
        if !outcome.ok {
            self.preview = Some(outcome);
            self.transition(Phase::Cancelled, "预演失败", tick);
            return Err(ReskinCode::PreviewFailed);
        }
        if outcome.cost_ms > budget {
            self.preview = Some(outcome);
            self.transition(Phase::Cancelled, "预演超预算", tick);
            return Err(ReskinCode::PreviewOverBudget);
        }
        self.preview = Some(outcome);
        Ok(())
    }

    /// 提交影响面。预演结论不存在时记 [`ReskinCode::ImpactMissing`]：
    /// 「没跑预演就报影响面」和「阶段不对」是两种缺陷，码必须分得开，
    /// 否则排障时要靠猜。
    pub fn impact(&mut self, summary: ImpactSummary, tick: u64) -> Result<(), ReskinCode> {
        if self.phase != Phase::Previewing {
            return Err(ReskinCode::PhaseOutOfOrder);
        }
        if self.preview.is_none() {
            return Err(ReskinCode::ImpactMissing);
        }
        self.impact = Some(summary);
        self.transition(Phase::Impacted, "", tick);
        Ok(())
    }

    /// 提交编排。为空或超预算拒绝（超预算由 F3428 降档后重提）。
    pub fn choreograph(
        &mut self,
        choreography: Choreography,
        tick: u64,
    ) -> Result<(), ReskinCode> {
        if self.phase != Phase::Impacted {
            return Err(ReskinCode::PhaseOutOfOrder);
        }
        if choreography.is_empty() {
            return Err(ReskinCode::EmptyChoreography);
        }
        let budget = match self.request.as_ref() {
            Some(r) => r.budget_ms,
            None => 0,
        };
        if choreography.total_ms() > budget {
            return Err(ReskinCode::ChoreographyOverBudget);
        }
        self.choreography = Some(choreography);
        self.transition(Phase::Choreographed, "", tick);
        Ok(())
    }

    /// 提交事务。四件齐备 + 快照存在才允许。
    pub fn commit(&mut self, tick: u64) -> Result<&Snapshot, ReskinCode> {
        if self.phase != Phase::Choreographed {
            return Err(ReskinCode::PhaseOutOfOrder);
        }
        if self.preview.is_none() || self.impact.is_none() || self.choreography.is_none() {
            return Err(ReskinCode::PhaseOutOfOrder);
        }
        if self.snapshot.is_none() {
            return Err(ReskinCode::SnapshotMissing);
        }
        self.committed = self.committed.saturating_add(1);
        self.transition(Phase::Committed, "", tick);
        match self.snapshot.as_ref() {
            Some(s) => Ok(s),
            None => Err(ReskinCode::SnapshotMissing),
        }
    }

    /// 回滚。任一非终态阶段都可回滚；回滚后状态是「上一次已提交的主题」。
    pub fn rollback(&mut self, reason: &str, tick: u64) -> Result<&Snapshot, ReskinCode> {
        if self.snapshot.is_none() {
            return Err(ReskinCode::NothingToRollback);
        }
        if matches!(self.phase, Phase::Idle) {
            return Err(ReskinCode::NothingToRollback);
        }
        self.rolled_back = self.rolled_back.saturating_add(1);
        self.transition(Phase::RollingBack, reason, tick);
        self.transition(Phase::RolledBack, reason, tick);
        match self.snapshot.as_ref() {
            Some(s) => Ok(s),
            None => Err(ReskinCode::NothingToRollback),
        }
    }

    /// 主动取消（用户点了另一套皮肤）。
    pub fn cancel(&mut self, reason: &str, tick: u64) -> Result<(), ReskinCode> {
        if self.phase.is_terminal() {
            return Err(ReskinCode::PhaseOutOfOrder);
        }
        if self.phase == Phase::Idle {
            return Err(ReskinCode::PhaseOutOfOrder);
        }
        self.transition(Phase::Cancelled, reason, tick);
        Ok(())
    }

    /// 记录一次状态迁移。
    fn transition(&mut self, to: Phase, note: &str, tick: u64) {
        let from = self.phase;
        self.phase = to;
        self.journal.push(PhaseEvent {
            from,
            to,
            note: note.to_string(),
            tick,
        });
    }

    /// 阶段日志。
    pub fn journal(&self) -> &[PhaseEvent] {
        &self.journal
    }

    /// 快照（提交后的回滚凭据）。
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.snapshot.as_ref()
    }

    /// 已提交次数。
    pub fn committed_count(&self) -> u32 {
        self.committed
    }

    /// 已回滚次数。
    pub fn rollback_count(&self) -> u32 {
        self.rolled_back
    }

    /// 两律自检：返回未满足的律名（空 = 两律成立）。
    ///
    /// - 不中断：请求里声明了交互接管；
    /// - 可回滚：存在快照且快照主题与目标主题不同。
    pub fn law_check(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        match self.request.as_ref() {
            Some(r) => {
                if r.interaction_handoff.trim().is_empty() {
                    out.push("不中断律：未声明交互接管");
                }
                match self.snapshot.as_ref() {
                    Some(s) => {
                        if s.theme == r.theme {
                            out.push("可回滚律：快照主题与目标相同");
                        }
                    }
                    None => out.push("可回滚律：无快照"),
                }
            }
            None => {
                out.push("不中断律：无请求");
                out.push("可回滚律：无快照");
            }
        }
        out
    }

    /// 全链状态读屏播报（锚点「全链状态读屏播报」）。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "换肤阶段：{}；已提交 {} 次，回滚 {} 次",
            self.phase.label(),
            self.committed,
            self.rolled_back
        ));
        if let Some(r) = self.request.as_ref() {
            out.push(format!(
                "主题 {} ← {}，变更 {} 条，预算 {}ms，交互接管 {}",
                r.theme,
                r.current_theme,
                r.changes.len(),
                r.budget_ms,
                r.interaction_handoff
            ));
        }
        if let Some(p) = self.preview.as_ref() {
            out.push(format!(
                "预演：{}，耗时 {}ms，拒绝 {} 条",
                if p.ok { "成功" } else { "失败" },
                p.cost_ms,
                p.rejected.len()
            ));
        }
        if let Some(i) = self.impact.as_ref() {
            out.push(format!(
                "影响面：{} 个组件，分级 {}",
                i.affected,
                i.level.label()
            ));
        }
        if let Some(c) = self.choreography.as_ref() {
            out.push(format!(
                "编排：{} 步，共 {}ms",
                c.steps.len(),
                c.total_ms()
            ));
        }
        for law in self.law_check() {
            out.push(format!("两律未满足：{}", law));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ReskinRequest {
        ReskinRequest::new("deep-ocean", "daylight", 400)
            .with_change("color.bg", "#FFFFFF", "#0B1020")
            .with_change("color.fg", "#111111", "#E8F0FF")
    }

    fn choreo() -> Choreography {
        Choreography {
            steps: alloc::vec![
                Step {
                    name: "fade-out".to_string(),
                    duration_ms: 80,
                },
                Step {
                    name: "apply".to_string(),
                    duration_ms: 20,
                },
                Step {
                    name: "fade-in".to_string(),
                    duration_ms: 100,
                },
            ],
        }
    }

    fn engine_at_choreographed() -> ReskinEngine {
        let mut e = ReskinEngine::new();
        e.begin(request(), 1).expect("开始");
        e.preview(
            PreviewOutcome {
                cost_ms: 30,
                ok: true,
                rejected: Vec::new(),
            },
            2,
        )
        .expect("预演");
        e.impact(
            ImpactSummary {
                affected: 12,
                level: ImpactLevel::LocalRepaint,
            },
            3,
        )
        .expect("影响面");
        e.choreograph(choreo(), 4).expect("编排");
        e
    }

    #[test]
    fn full_chain_commits_in_order() {
        let mut e = engine_at_choreographed();
        assert_eq!(e.phase(), Phase::Choreographed);
        let snap = e.commit(5).expect("提交成功");
        assert_eq!(snap.theme, "daylight");
        assert_eq!(e.phase(), Phase::Committed);
        assert_eq!(e.committed_count(), 1);
        assert!(e.law_check().is_empty());
        // 日志至少四段。
        assert!(e.journal().len() >= 4);
    }

    #[test]
    fn skipping_preview_is_rejected() {
        let mut e = ReskinEngine::new();
        e.begin(request(), 1).expect("开始");
        let err = e
            .impact(
                ImpactSummary {
                    affected: 1,
                    level: ImpactLevel::Imperceptible,
                },
                2,
            )
            .err()
            .expect("跳步必拒");
        assert_eq!(err, ReskinCode::ImpactMissing);
    }

    #[test]
    fn preview_failure_cancels_chain() {
        let mut e = ReskinEngine::new();
        e.begin(request(), 1).expect("开始");
        let err = e
            .preview(
                PreviewOutcome {
                    cost_ms: 10,
                    ok: false,
                    rejected: alloc::vec!["color.bg".to_string()],
                },
                2,
            )
            .err()
            .expect("预演失败必报");
        assert_eq!(err, ReskinCode::PreviewFailed);
        assert_eq!(e.phase(), Phase::Cancelled);
        // 取消后可以开新请求。
        assert!(e.begin(request(), 3).is_ok());
    }

    #[test]
    fn preview_over_budget_cancels() {
        let mut e = ReskinEngine::new();
        e.begin(request(), 1).expect("开始");
        let err = e
            .preview(
                PreviewOutcome {
                    cost_ms: 401,
                    ok: true,
                    rejected: Vec::new(),
                },
                2,
            )
            .err()
            .expect("超预算必报");
        assert_eq!(err, ReskinCode::PreviewOverBudget);
        assert_eq!(e.phase(), Phase::Cancelled);
    }

    #[test]
    fn missing_handoff_violates_no_interrupt_law() {
        let mut r = request();
        r.interaction_handoff = String::new();
        let err = r.validate().err().expect("缺接管必拒");
        assert_eq!(err, ReskinCode::InteractionNotHandedOff);
        assert!(err.is_law_violation());
    }

    #[test]
    fn duplicate_change_and_noop_theme_are_rejected() {
        let mut r = request();
        r.changes.push(Change::new("color.bg", "#000000", "#111111"));
        assert_eq!(r.validate().err(), Some(ReskinCode::DuplicateChange));

        let mut r2 = request();
        r2.current_theme = "deep-ocean".to_string();
        assert_eq!(r2.validate().err(), Some(ReskinCode::NoOpReskin));
    }

    #[test]
    fn over_budget_choreography_is_rejected() {
        let mut e = ReskinEngine::new();
        e.begin(request(), 1).expect("开始");
        e.preview(
            PreviewOutcome {
                cost_ms: 10,
                ok: true,
                rejected: Vec::new(),
            },
            2,
        )
        .expect("预演");
        e.impact(
            ImpactSummary {
                affected: 3,
                level: ImpactLevel::FullRepaint,
            },
            3,
        )
        .expect("影响面");
        let big = Choreography {
            steps: alloc::vec![Step {
                name: "slow".to_string(),
                duration_ms: 900,
            }],
        };
        assert_eq!(
            e.choreograph(big, 4).err(),
            Some(ReskinCode::ChoreographyOverBudget)
        );
    }

    #[test]
    fn rollback_returns_snapshot_and_is_idempotent() {
        let mut e = engine_at_choreographed();
        e.commit(5).expect("提交");
        let snap = e.rollback("用户反悔", 6).expect("回滚");
        assert_eq!(snap.theme, "daylight");
        assert_eq!(e.phase(), Phase::RolledBack);
        assert_eq!(e.rollback_count(), 1);
        // 再次回滚仍拿得到同一个快照（回滚是幂等的，不消耗凭据）。
        assert!(e.rollback("再滚一次", 7).is_ok());
    }

    #[test]
    fn double_begin_while_running_is_rejected() {
        let mut e = ReskinEngine::new();
        e.begin(request(), 1).expect("开始");
        assert_eq!(e.begin(request(), 2).err(), Some(ReskinCode::AlreadyRunning));
    }

    #[test]
    fn reading_lines_report_phase_and_laws() {
        let e = engine_at_choreographed();
        let lines = e.reading_lines();
        assert!(lines
            .first()
            .map(|l| l.contains("编排就绪"))
            .unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("影响面")));
        assert!(lines.iter().any(|l| l.contains("编排")));
    }
}
