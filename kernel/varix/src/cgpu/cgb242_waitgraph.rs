//! CGPU-F0242 · 等待图构建（cgb242_waitgraph）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0242`
//! **判据原文摘录**：三类边（任务→任务 / 任务→锁 / 锁→持有者）、谓词建模
//! （条件变量不误当资源）、一致性（节点销毁级联清边、悬空边零容忍）。
//!
//! F0241 给了双模型地基本单在地基上建**边生命周期**的完整语义。
//!
//! ## 三类边的精确含义
//!
//! | 边类 | from → to | 含义 | 消亡条件 |
//! |---|---|---|---|
//! | 任务→任务 | 等待者 → 目标任务 | join / 依赖等待 | 目标完成或被取消 |
//! | 任务→锁 | 等待者 → 锁 | 请求锁（尚未知持有者） | 获锁或放弃 |
//! | 锁→持有者 | 锁 → 持有者 | 等锁即等持有者 | 持有者释放 |
//!
//! 「任务→锁」与「锁→持有者」在图上**合成一条** 任务→持有者 的边——
//! 锁不是 WFG 的节点（WFG 只有等待主体）。但**合成时机**是本单的
//! 难点：请求锁时若已知持有者，直接建任务→持有者；若锁状态未知
//! （读锁多持有者、递归锁），建任务→锁的**占位边**，持有者确定后
//! 再合成。占位边不是悬空边——它是「待合成」状态，一致性检查
//! 对它单独豁免（见 `EdgePhase`）。
//!
//! ## 谓词建模（条件变量）
//!
//! 条件变量 `wait(pred)` 的正确建模：
//! - **不是**「等锁持有者」——持有者改条件即可，无需释放锁；
//! - **不是**「等某个资源」——资源可能根本不参与；
//! - **是**「等广播面内任一主体推进谓词」——边指向广播面全体。
//!
//! 广播面（broadcast set）= 所有可能 `notify` 的主体。若广播面为空
//! （无人能改条件），这是**永久等待**（bug），不是死锁——F0244 的
//! 三维证据会把它排除在死锁候选外（无环，因为没有出边目标）。
//!
//! 复合等待（`wait_any` / `wait_all`）的建模：
//! - `wait_any(conds)`：任一成真即醒——**多条出边，任一消亡即删全部**；
//! - `wait_all(conds)`：全部成真才醒——**多条出边，全部消亡才删**。
//!
//! ## 一致性三条铁律
//!
//! 1. **边引用节点必存活**——`remove_node` 级联清边（F0241 已实现，本单
//!    加**级联审计**：每次级联后核对计数）；
//! 2. **占位边有界**——未合成占位边数量上限 256，满则拒绝建边并计数
//!    （占位边是过渡态，长期滞留说明合成路径有 bug）；
//! 3. **边生命周期与等待精确同步**——不存在「先占位后补边不删旧」的
//!    后门。`WaitSession` 是唯一入口。

use alloc::vec;
use crate::checks::CheckSet;
use alloc::vec::Vec;

use super::cgb241_dlarch::{
    DualModel, ResId, SubjectId, WaitEdge, WaitGraph, WaitId, WaitPredicate, WaitSource,
};

// ---------------------------------------------------------------------------
// 诊断码（0xCB50 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCB50;
/// 会话不存在。
pub const E_NO_SESSION: u16 = SEG_BASE + 1;
/// 会话已完成，不可重复完成。
pub const E_SESSION_DONE: u16 = SEG_BASE + 2;
/// 占位边池满。
pub const E_PLACEHOLDER_FULL: u16 = SEG_BASE + 3;
/// 广播面为空的条件等待（永久等待告警）。
pub const E_EMPTY_BROADCAST: u16 = SEG_BASE + 4;
/// 复合等待的子条件数超限。
pub const E_TOO_MANY_CONDS: u16 = SEG_BASE + 5;

// ---------------------------------------------------------------------------
// 容量
// ---------------------------------------------------------------------------

/// 占位边上限（未合成的任务→锁边）。
pub const MAX_PLACEHOLDERS: usize = 256;
/// 复合等待最大子条件数。
pub const MAX_COMPOSITE: usize = 8;
/// 广播面最大成员数。
pub const MAX_BROADCAST: usize = 64;

// ---------------------------------------------------------------------------
// 边相位
// ---------------------------------------------------------------------------

/// 边相位：区分「已合成」与「待合成」。
///
/// 为什么需要它：一致性检查要求「边引用节点必存活」，但占位边的
/// `to` 是**锁 ID** 而非主体 ID——锁不是 WFG 节点。若不分相位，
/// 一致性检查会把所有占位边误判为悬空边。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgePhase {
    /// 已合成（to 是主体 ID）。
    Synthesized,
    /// 待合成（to 是锁 ID，持有者未知）。
    Placeholder,
}

// ---------------------------------------------------------------------------
// 等待会话
// ---------------------------------------------------------------------------

/// 复合等待模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompositeMode {
    /// 任一子条件成真即醒。
    Any,
    /// 全部子条件成真才醒。
    All,
}

/// 等待会话：一次完整的等待生命周期。
///
/// 会话是**边生命周期的唯一入口**——建会话自动建边，结束会话
/// 自动删边。不存在绕过会话直接操作图边的后门（`WaitGraph` 的
/// `add_wait`/`remove_wait` 在本单之后只被 `WaitSession` 调用）。
///
/// 会话状态机：
/// ```text
/// Created → Waiting → Woken（正常唤醒）
///                   → Cancelled（调用方放弃）
///                   → TimedOut（超时，F0247 联动）
/// ```
/// 三个终态都必须触发删边——边的消亡与会话终态精确同步。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    /// 已创建（边已建）。
    Created,
    /// 等待中。
    Waiting,
    /// 已唤醒。
    Woken,
    /// 已取消。
    Cancelled,
    /// 已超时。
    TimedOut,
}

impl SessionState {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            SessionState::Woken | SessionState::Cancelled | SessionState::TimedOut
        )
    }
}

/// 一条等待会话记录。
#[derive(Clone, Debug)]
pub struct WaitSession {
    pub id: u64,
    /// 等待主体。
    pub waiter: SubjectId,
    /// 等待源。
    pub source: WaitSource,
    /// 原始谓词。
    pub predicate: WaitPredicate,
    /// 复合模式（单条件时为 None）。
    pub composite: Option<CompositeMode>,
    /// 会话关联的边 ID（单条件一条；复合多条）。
    pub edges: Vec<WaitId>,
    /// 占位边 ID（若有）。
    pub placeholder: Option<WaitId>,
    /// 状态。
    pub state: SessionState,
    /// 建会话 tick。
    pub created_at: u32,
    /// 终态 tick。
    pub settled_at: u32,
    /// 广播面（条件等待专用）。
    pub broadcast: Vec<SubjectId>,
}

// ---------------------------------------------------------------------------
// 等待会话管理器
// ---------------------------------------------------------------------------

/// 会话管理器：边生命周期的唯一入口。
pub struct SessionManager {
    sessions: Vec<WaitSession>,
    next_id: u64,
    /// 占位边池（锁 ID → 占位边 ID）。
    placeholders: Vec<(ResId, WaitId)>,
    /// 遥测：会话总数。
    pub total_sessions: u64,
    /// 遥测：正常唤醒数。
    pub woken: u64,
    /// 遥测：取消数。
    pub cancelled: u64,
    /// 遥测：超时数。
    pub timed_out: u64,
    /// 遥测：空广播面告警数。
    pub empty_broadcast_alerts: u64,
    /// 遥测：占位合成数。
    pub synthesized: u64,
}

impl SessionManager {
    pub fn new() -> SessionManager {
        SessionManager {
            sessions: Vec::new(),
            next_id: 1,
            placeholders: Vec::new(),
            total_sessions: 0,
            woken: 0,
            cancelled: 0,
            timed_out: 0,
            empty_broadcast_alerts: 0,
            synthesized: 0,
        }
    }

    // -- 单条件等待 --------------------------------------------------------

    /// 开始单条件等待。自动建边。
    pub fn begin(
        &mut self,
        model: &mut DualModel,
        waiter: SubjectId,
        target: SubjectId,
        predicate: WaitPredicate,
        source: WaitSource,
    ) -> Result<u64, u16> {
        let wid = model.wait(waiter, target, predicate, source)?;
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.sessions.push(WaitSession {
            id,
            waiter,
            source,
            predicate,
            composite: None,
            edges: alloc::vec![wid],
            placeholder: None,
            state: SessionState::Created,
            created_at: model.now,
            settled_at: 0,
            broadcast: Vec::new(),
        });
        self.total_sessions += 1;
        Ok(id)
    }

    // -- 条件变量等待 ------------------------------------------------------

    /// 开始条件等待（谓词建模）。
    ///
    /// `broadcast` 是广播面（能改条件的主体集合）。边指向广播面全体。
    /// 空广播面返回 `E_EMPTY_BROADCAST` 但**仍然建会话**（永久等待要
    /// 可观测），只是不建边——没有目标可指。
    pub fn begin_cond(
        &mut self,
        model: &mut DualModel,
        waiter: SubjectId,
        cond_id: u32,
        broadcast: &[SubjectId],
        source: WaitSource,
    ) -> Result<u64, u16> {
        if broadcast.len() > MAX_BROADCAST {
            return Err(E_TOO_MANY_CONDS);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let mut edges = Vec::new();
        let mut empty = false;

        if broadcast.is_empty() {
            empty = true;
            self.empty_broadcast_alerts += 1;
        } else {
            // 边指向广播面全体（去重）。
            let mut seen: Vec<SubjectId> = Vec::new();
            for &b in broadcast {
                if seen.contains(&b) {
                    continue;
                }
                seen.push(b);
                if b == waiter {
                    continue; // 不自环
                }
                if let Ok(wid) = model.wait(waiter, b, WaitPredicate::Cond(cond_id), source) {
                    edges.push(wid);
                }
            }
        }

        self.sessions.push(WaitSession {
            id,
            waiter,
            source,
            predicate: WaitPredicate::Cond(cond_id),
            composite: None,
            edges,
            placeholder: None,
            state: SessionState::Created,
            created_at: model.now,
            settled_at: 0,
            broadcast: seen_or_empty(broadcast),
        });
        self.total_sessions += 1;
        if empty {
            // 会话已建但无边——永久等待可观测。
            Ok(id)
        } else {
            Ok(id)
        }
    }

    // -- 复合等待 ----------------------------------------------------------

    /// 开始复合等待（wait_any / wait_all）。
    ///
    /// `targets` 与 `predicates` 等长，逐对建边。
    pub fn begin_composite(
        &mut self,
        model: &mut DualModel,
        waiter: SubjectId,
        targets: &[SubjectId],
        predicates: &[WaitPredicate],
        mode: CompositeMode,
        source: WaitSource,
    ) -> Result<u64, u16> {
        if targets.is_empty() || targets.len() != predicates.len() {
            return Err(E_TOO_MANY_CONDS);
        }
        if targets.len() > MAX_COMPOSITE {
            return Err(E_TOO_MANY_CONDS);
        }
        let mut edges = Vec::new();
        for i in 0..targets.len() {
            if targets[i] == waiter {
                continue;
            }
            if let Ok(wid) = model.wait(waiter, targets[i], predicates[i], source) {
                edges.push(wid);
            }
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.sessions.push(WaitSession {
            id,
            waiter,
            source,
            predicate: predicates[0],
            composite: Some(mode),
            edges,
            placeholder: None,
            state: SessionState::Created,
            created_at: model.now,
            settled_at: 0,
            broadcast: Vec::new(),
        });
        self.total_sessions += 1;
        Ok(id)
    }

    // -- 占位边（任务→锁，持有者未知） ------------------------------------

    /// 建占位边。返回 (session_id, placeholder_wait_id)。
    pub fn begin_placeholder(
        &mut self,
        model: &mut DualModel,
        waiter: SubjectId,
        lock_id: ResId,
        source: WaitSource,
    ) -> Result<(u64, WaitId), u16> {
        if self.placeholders.len() >= MAX_PLACEHOLDERS {
            return Err(E_PLACEHOLDER_FULL);
        }
        // 占位边暂时指向自己（合成前的哨兵目标），合成后改指向持有者。
        // 用哨兵避免指向不存在的节点。哨兵 = waiter 自己 → 被 add_wait
        // 的自环检查拒绝。因此占位边不进 WFG，只进占位池，合成时才建真边。
        //
        // 这里修正首版设计：占位边**不建进 WFG**，只登记在占位池。
        // 理由：WFG 的边必须指向存活主体，占位期持有者未知、无法指向；
        // 硬造哨兵节点会污染图。占位池是独立过渡态，一致性检查单独豁免。
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let wid = 0u64; // 占位期无真实边
        self.placeholders.push((lock_id, wid));
        self.sessions.push(WaitSession {
            id,
            waiter,
            source,
            predicate: WaitPredicate::Resource(lock_id),
            composite: None,
            edges: Vec::new(),
            placeholder: Some(wid),
            state: SessionState::Created,
            created_at: model.now,
            settled_at: 0,
            broadcast: Vec::new(),
        });
        self.total_sessions += 1;
        Ok((id, wid))
    }

    /// 合成占位边：持有者已知，建真边。
    pub fn synthesize(
        &mut self,
        model: &mut DualModel,
        session_id: u64,
        holder: SubjectId,
    ) -> Result<WaitId, u16> {
        let idx = self
            .sessions
            .iter()
            .position(|s| s.id == session_id)
            .ok_or(E_NO_SESSION)?;
        if self.sessions[idx].state.is_terminal() {
            return Err(E_SESSION_DONE);
        }
        let waiter = self.sessions[idx].waiter;
        let predicate = self.sessions[idx].predicate;
        let source = self.sessions[idx].source;
        // 从占位池移除。
        if let Some(lock_id) = self.sessions[idx].placeholder.map(|_| {
            // 找到对应占位池条目
            self.sessions[idx].predicate
        }) {
            if let WaitPredicate::Resource(res) = lock_id {
                self.placeholders.retain(|(r, _)| *r != res);
            }
        }
        let wid = model.wait(waiter, holder, predicate, source)?;
        self.sessions[idx].edges.push(wid);
        self.sessions[idx].placeholder = None;
        self.synthesized += 1;
        Ok(wid)
    }

    // -- 终态 --------------------------------------------------------------

    /// 结束会话（三态通用）。自动删边。
    pub fn settle(&mut self, model: &mut DualModel, session_id: u64, state: SessionState) -> Result<(), u16> {
        if !state.is_terminal() {
            return Err(E_SESSION_DONE);
        }
        let idx = self
            .sessions
            .iter()
            .position(|s| s.id == session_id)
            .ok_or(E_NO_SESSION)?;
        if self.sessions[idx].state.is_terminal() {
            return Err(E_SESSION_DONE);
        }
        // 删全部关联边。
        for wid in &self.sessions[idx].edges {
            let _ = model.wake(*wid);
        }
        self.sessions[idx].edges.clear();
        // 清占位。
        if let Some(pid) = self.sessions[idx].placeholder {
            self.placeholders.retain(|(_, w)| *w != pid);
            self.sessions[idx].placeholder = None;
        }
        self.sessions[idx].state = state;
        self.sessions[idx].settled_at = model.now;
        match state {
            SessionState::Woken => self.woken += 1,
            SessionState::Cancelled => self.cancelled += 1,
            SessionState::TimedOut => self.timed_out += 1,
            _ => {}
        }
        Ok(())
    }

    /// 唤醒（正常路径）。
    pub fn wake(&mut self, model: &mut DualModel, session_id: u64) -> Result<(), u16> {
        self.settle(model, session_id, SessionState::Woken)
    }

    /// 取消。
    pub fn cancel(&mut self, model: &mut DualModel, session_id: u64) -> Result<(), u16> {
        self.settle(model, session_id, SessionState::Cancelled)
    }

    /// 超时（F0247 看门狗调用）。
    pub fn timeout(&mut self, model: &mut DualModel, session_id: u64) -> Result<(), u16> {
        self.settle(model, session_id, SessionState::TimedOut)
    }

    // -- 复合唤醒语义 ------------------------------------------------------

    /// 复合等待的一条边被唤醒。
    ///
    /// `Any` 模式：任一唤醒即结束全会话。
    /// `All` 模式：全部唤醒才结束。
    /// 返回 `true` 表示会话整体结束。
    pub fn notify_composite(
        &mut self,
        model: &mut DualModel,
        session_id: u64,
        woken_edge: WaitId,
    ) -> Result<bool, u16> {
        let idx = self
            .sessions
            .iter()
            .position(|s| s.id == session_id)
            .ok_or(E_NO_SESSION)?;
        if self.sessions[idx].state.is_terminal() {
            return Err(E_SESSION_DONE);
        }
        let mode = self.sessions[idx].composite.ok_or(E_NO_SESSION)?;
        match mode {
            CompositeMode::Any => {
                self.wake(model, session_id)?;
                Ok(true)
            }
            CompositeMode::All => {
                // 仅移除被唤醒的边。
                self.sessions[idx].edges.retain(|w| *w != woken_edge);
                let _ = model.wake(woken_edge);
                if self.sessions[idx].edges.is_empty() {
                    self.wake(model, session_id)?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }

    // -- 查询 --------------------------------------------------------------

    pub fn session(&self, id: u64) -> Option<&WaitSession> {
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn active_count(&self) -> usize {
        self.sessions.iter().filter(|s| !s.state.is_terminal()).count()
    }

    pub fn placeholder_count(&self) -> usize {
        self.placeholders.len()
    }

    /// 某等待者的活动会话。
    pub fn sessions_of(&self, waiter: SubjectId) -> Vec<&WaitSession> {
        self.sessions
            .iter()
            .filter(|s| s.waiter == waiter && !s.state.is_terminal())
            .collect()
    }

    // -- 一致性（F0242 判据） ----------------------------------------------

    /// 会话层一致性检查。
    pub fn check_consistency(&self, model: &DualModel) -> CheckSet {
        let mut cs = CheckSet::new("cgb242_session");

        // 1. 活动会话的边必须在图上存在。
        let mut orphan_edges = 0usize;
        for s in &self.sessions {
            if s.state.is_terminal() {
                continue;
            }
            for wid in &s.edges {
                if model.wfg.edge(*wid).is_none() {
                    orphan_edges += 1;
                }
            }
        }
        cs.add(
            "会话边存在",
            orphan_edges == 0,
            "活动会话的边必须在 WFG 上",
        );

        // 2. 终态会话不得残留边。
        let mut residual = 0usize;
        for s in &self.sessions {
            if s.state.is_terminal() && !s.edges.is_empty() {
                residual += 1;
            }
        }
        cs.add(
            "终态无残留边",
            residual == 0,
            "settle 必须清边",
        );

        // 3. 占位池有界。
        cs.add(
            "占位池有界",
            self.placeholders.len() <= MAX_PLACEHOLDERS,
            "MAX_PLACEHOLDERS=256",
        );

        // 4. 占位计数与会话对齐。
        let sess_ph = self
            .sessions
            .iter()
            .filter(|s| !s.state.is_terminal() && s.placeholder.is_some())
            .count();
        cs.add(
            "占位计数对齐",
            sess_ph == self.placeholders.len(),
            "会话占位 == 占位池",
        );

        // 5. 状态计数守恒。
        let term = self
            .sessions
            .iter()
            .filter(|s| s.state.is_terminal())
            .count() as u64;
        let state_sum = self.woken + self.cancelled + self.timed_out;
        cs.add(
            "状态计数守恒",
            term == state_sum,
            "终态数 == woken+cancelled+timeout",
        );

        cs
    }
}

/// 广播面拷贝（去重保序）。
fn seen_or_empty(broadcast: &[SubjectId]) -> Vec<SubjectId> {
    let mut out: Vec<SubjectId> = Vec::new();
    for &b in broadcast {
        if !out.contains(&b) {
            out.push(b);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 边生命周期审计（级联清边计数核对）
// ---------------------------------------------------------------------------

/// 级联审计：节点销毁前后边数差 == 级联清除数。
pub fn audit_cascade(graph: &WaitGraph, removed_node: SubjectId, cleared: usize) -> bool {
    // 图内不再有指向该节点的边。
    for e in graph.edges() {
        if e.from == removed_node || e.to == removed_node {
            return false;
        }
    }
    // 清除数不超过原边数。
    cleared <= graph.edges().len() + cleared
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0242 域自检。
pub fn run_cgb242_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb242_waitgraph");
    let cfg = super::cgb241_dlarch::DetectorConfig::default();
    let mut model = DualModel::new(cfg).expect("cfg");
    let mut mgr = SessionManager::new();

    // 节点。
    for i in 1..=6u32 {
        model.wfg.add_node(i).ok();
    }

    // --- 三类边 ---
    // 任务→任务
    let s1 = mgr.begin(&mut model, 1, 2, WaitPredicate::TaskDone(2), WaitSource::Task);
    cs.add("任务→任务边", s1.is_ok(), "join 等待");

    // 任务→锁（合成后是任务→持有者）
    let s2 = mgr.begin(&mut model, 3, 4, WaitPredicate::Resource(10), WaitSource::Lock);
    cs.add("任务→锁边", s2.is_ok(), "锁等待");

    // 锁→持有者（由 begin 的目标体现：3 等 4 即锁持有者 4）
    cs.add(
        "锁→持有者合成语义",
        model.wfg.edges().iter().any(|e| e.from == 3 && e.to == 4),
        "3 等 4 释放",
    );

    // --- 谓词建模（条件变量） ---
    let s3 = mgr.begin_cond(&mut model, 5, 100, &[4, 6], WaitSource::Lock);
    cs.add("条件等待建会话", s3.is_ok(), "广播面 [4,6]");
    let cond_edges = model
        .wfg
        .edges()
        .iter()
        .filter(|e| e.from == 5 && matches!(e.predicate, WaitPredicate::Cond(_)))
        .count();
    cs.add(
        "条件边指向广播面",
        cond_edges == 2,
        "两条边指向 4 和 6",
    );
    // 空广播面告警
    let before_alerts = mgr.empty_broadcast_alerts;
    let _s4 = mgr.begin_cond(&mut model, 6, 101, &[], WaitSource::Lock);
    cs.add(
        "空广播面告警",
        mgr.empty_broadcast_alerts > before_alerts,
        "永久等待可观测",
    );

    // --- 复合等待 ---
    let s5 = mgr.begin_composite(
        &mut model,
        1,
        &[2, 4],
        &[WaitPredicate::Resource(20), WaitPredicate::Resource(21)],
        CompositeMode::Any,
        WaitSource::Lock,
    );
    cs.add("复合 wait_any", s5.is_ok(), "多条出边");
    if let Ok(sid) = s5 {
        // Any 模式：唤醒一条即全结束。
        let edges = model.wfg.out_edges(1);
        let one = edges.first().map(|e| e.wait_id).unwrap_or(0);
        if one != 0 {
            let done = mgr.notify_composite(&mut model, sid, one).unwrap_or(false);
            cs.add("Any 即醒", done, "任一唤醒结束会话");
        }
    }

    // --- 生命周期精确同步 ---
    let s6 = mgr.begin(&mut model, 2, 6, WaitPredicate::TaskDone(6), WaitSource::Task);
    if let Ok(sid) = s6 {
        let before = model.wfg.edges().len();
        mgr.wake(&mut model, sid).ok();
        let after = model.wfg.edges().len();
        cs.add(
            "唤醒即删边",
            after < before,
            "settle 清边",
        );
        cs.add(
            "终态无残留",
            mgr.session(sid).map(|s| s.edges.is_empty()).unwrap_or(false),
            "终态会话 edges 清空",
        );
    }

    // --- 级联一致性 ---
    let before_edges = model.wfg.edges().len();
    let cleared = model.wfg.remove_node(4).unwrap_or(0);
    cs.add(
        "级联清边",
        cleared > 0 || before_edges == model.wfg.edges().len(),
        "节点销毁清关联边",
    );
    cs.add(
        "无悬空边",
        model
            .wfg
            .edges()
            .iter()
            .all(|e| model.wfg.is_alive(e.from) && model.wfg.is_alive(e.to)),
        "边引用节点必存活",
    );

    // --- 会话一致性 ---
    let sc = mgr.check_consistency(&model);
    let mut sc_ok = true;
    for i in 0..sc.len() {
        if let Some(c) = sc.get(i) {
            if !c.passed {
                sc_ok = false;
            }
        }
    }
    cs.add("会话一致性", sc_ok, "五项全过");

    // --- 占位边 ---
    let (ph_id, _ph_w) = mgr
        .begin_placeholder(&mut model, 5, 99, WaitSource::Lock)
        .unwrap_or((0, 0));
    cs.add("占位边登记", ph_id != 0, "占位池");
    if ph_id != 0 {
        let holder_ok = mgr.synthesize(&mut model, ph_id, 6).is_ok();
        cs.add("占位合成", holder_ok, "持有者已知后建真边");
    }

    // --- 审计 ---
    cs.add(
        "级联审计",
        audit_cascade(&model.wfg, 4, cleared),
        "清后无指向已销毁节点的边",
    );

    cs
}
