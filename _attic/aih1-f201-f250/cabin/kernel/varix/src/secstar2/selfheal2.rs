//! F189 静默自愈集（secstar2 · G-G-19）——小病自己好，但要让主人知道它生过病。
//!
//! **判据（主册）**：三类损坏注入各 5 次自愈成功率 100%；通知报备 100%；
//! 连续失败升级触发实测。
//!
//! **功能定义（主册 G-G-19）**：三类常见损坏自动重建：图标缓存损坏/主题
//! 令牌缺失/缩略图库损坏——后台重建+通知中心报备（做过什么如实说）；
//! 自愈是服务不是魔术。
//!
//! 【交互设计】自愈事件通知（低优先 F077 历史档）；诊断中心「自愈记录」页
//! （时间/损坏类型/修复结果/耗时）；连续自愈同项 ≥3 次/周 → 升级为工单
//! （自愈失败=慢性病要查根因）。
//! 【数据与存储】自愈记录归档；三类检测器各自触发条件文档化。
//! 【状态与异常】重建失败 → 降级默认态（图标默认集/主题默认令牌/缩略图
//! 占位）+通知升级显目；重建占用资源超预算 → 分时批处理（F049 空闲窗口）。
//! 【设计细节】检测时机：服务启动时+运行中校验失败回调；重建顺序（缓存类
//! 即时/令牌类原子 F151 热替换/缩略图类后台 F093）；自愈与 F121 还原点协同
//! （重建前不留快照——缓存类无价值；令牌类留——配置级变更）；「3 次/周」
//! 阈值进旋钮清单。
//!
//! 依赖锚点：F049（空闲窗口）、F077（通知中心）、F093（缩略图后台）、F121（快照）、F151（令牌原子替换）。

use crate::checks::CheckSet;
use alloc::vec;
use crate::star::sbase::RingLog;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量与规格
// ---------------------------------------------------------------------------

/// 自愈记录环容量（诊断中心「自愈记录」页数据源；满覆最旧——归档由上层落盘）。
pub const HEAL_RECORD_CAP: usize = 64;

/// 升级工单阈值：连续自愈同项 ≥3 次/周（旋钮语义——界内 1..=10 可调）。
pub const ESCALATE_PER_WEEK: u32 = 3;
pub const ESCALATE_MIN: u32 = 1;
pub const ESCALATE_MAX: u32 = 10;

/// 单次重建耗时预算（ms）——超预算转分时批处理（F049 空闲窗口）。
pub const REBUILD_BUDGET_MS: u64 = 200;

/// 通知报备文案前缀（低优先档——做过什么如实说）。
pub const NOTIFY_TEXT: &str = "已自动修复";
/// 升级显目文案（降级默认态——通知升级）。
pub const DEGRADED_NOTIFY_TEXT: &str = "自动修复失败，已降级到默认状态";

/// 损坏类型（主册三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealKind {
    /// 图标缓存损坏——重建策略：即时（缓存类）。
    IconCache,
    /// 主题令牌缺失——重建策略：原子热替换（令牌类，F151）。
    ThemeToken,
    /// 缩略图库损坏——重建策略：后台分时（F093）。
    ThumbLib,
}

impl HealKind {
    pub fn name(self) -> &'static str {
        match self {
            HealKind::IconCache => "图标缓存",
            HealKind::ThemeToken => "主题令牌",
            HealKind::ThumbLib => "缩略图库",
        }
    }

    /// 重建策略（主册【设计细节】重建顺序）。
    pub fn strategy(self) -> RebuildStrategy {
        match self {
            HealKind::IconCache => RebuildStrategy::Immediate,
            HealKind::ThemeToken => RebuildStrategy::AtomicSwap,
            HealKind::ThumbLib => RebuildStrategy::Background,
        }
    }

    /// 快照协同（F121）：缓存类不留快照（无价值）；令牌类留（配置级变更）；
    /// 缩略图类不留（可再生物）。
    pub fn snapshot_before(self) -> bool {
        matches!(self, HealKind::ThemeToken)
    }
}

/// 重建策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RebuildStrategy {
    /// 即时（交互线程内小活）。
    Immediate,
    /// 原子热替换（新旧并存一次切换——失败回旧）。
    AtomicSwap,
    /// 后台分时（空闲窗口分批）。
    Background,
}

/// 自愈结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HealOutcome {
    /// 重建成功。
    Rebuilt,
    /// 重建失败 → 降级默认态。
    DegradedDefault,
    /// 升级工单（连续失败=慢性病）。
    Escalated,
}

/// 一条自愈记录（诊断中心「自愈记录」页一行：时间/类型/结果/耗时）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealRecord {
    /// 时间（分钟戳——调用方注入）。
    pub at_min: u64,
    pub kind: HealKind,
    pub outcome: HealOutcome,
    /// 耗时（ms）。
    pub cost_ms: u64,
    /// 本次是否报备通知（报备 100% 判据的对账字段）。
    pub notified: bool,
    /// 重建前是否留了快照（F121 协同审计）。
    pub snapshotted: bool,
}

/// 通知条目（通知中心 F077 低优先档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealNotice {
    pub at_min: u64,
    pub kind: HealKind,
    /// 文案（正常报备 or 降级显目）。
    pub text: &'static str,
    /// 显目级（降级时升档——正常是低优先历史档）。
    pub prominent: bool,
}

// ---------------------------------------------------------------------------
// 自愈主体
// ---------------------------------------------------------------------------

/// 静默自愈集。
pub struct SelfHealSet {
    /// 自愈记录环（新→旧读取）。
    records: RingLog<HealRecord, HEAL_RECORD_CAP>,
    /// 通知流（报备 100% 的证据面）。
    notices: Vec<HealNotice>,
    /// 同项周计数（升级判定用）：[kind][周序号] = 计数。
    week_counts: [Vec<(u64, u32)>; 3],
    /// 升级阈值（旋钮——界内可调）。
    pub escalate_threshold: u32,
    /// 空闲窗口注入（F049）：后台策略的重建从这些窗口扣预算。
    pub idle_budget_ms: u64,
    /// 升级工单计数（诊断面）。
    pub escalations: u64,
    /// 已立工单账（kind, 周序号）——每 kind×周只立一次，不刷屏。
    open_tickets: Vec<(HealKind, u64)>,
}

impl SelfHealSet {
    pub fn new() -> SelfHealSet {
        SelfHealSet {
            records: RingLog::new(),
            notices: Vec::new(),
            week_counts: [Vec::new(), Vec::new(), Vec::new()],
            escalate_threshold: ESCALATE_PER_WEEK,
            idle_budget_ms: REBUILD_BUDGET_MS,
            escalations: 0,
            open_tickets: Vec::new(),
        }
    }

    /// 调阈值（钳制界内——旋钮纪律）。
    pub fn set_threshold(&mut self, v: u32) {
        self.escalate_threshold = v.clamp(ESCALATE_MIN, ESCALATE_MAX);
    }

    /// **自愈主路**（判据一二三的实现核心）：
    /// 损坏注入 → 按类型策略重建 → 成功率对账 / 通知报备 100% / 周计数升级。
    ///
    /// 升级语义（主册「连续自愈同项 ≥3 次/周 → 升级为工单」）：同项一周内
    /// 反复需要自愈（无论单次成败）即为慢性病信号——阈值触发时登记工单
    /// （每 kind×周只登记一次，不刷屏）；失败且慢性 → Escalated（失败=
    /// 慢性病要查根因），成功但慢性 → 仍 Rebuilt（这次修好了，但工单照立）。
    ///
    /// `rebuild_ok`：重建动作的成败注入点（真实重建由各服务执行——本模块
    /// 是编排与账目层）。`cost_ms` 超预算时后台策略自动转分时批。
    pub fn heal(
        &mut self,
        at_min: u64,
        kind: HealKind,
        rebuild_ok: bool,
        cost_ms: u64,
    ) -> HealRecord {
        let week = at_min / 10_080; // 10080 分钟 = 7 天
        self.bump_week(kind, week);
        let chronic = self.week_count(kind, week) >= self.escalate_threshold;

        let outcome = match (rebuild_ok, chronic) {
            (true, _) => HealOutcome::Rebuilt,
            (false, false) => HealOutcome::DegradedDefault,
            (false, true) => HealOutcome::Escalated,
        };
        if chronic && !self.ticket_open(kind, week) {
            self.open_tickets.push((kind, week));
            self.escalations += 1;
        }

        let rec = HealRecord {
            at_min,
            kind,
            outcome,
            cost_ms,
            notified: true, // 报备 100%：每条自愈都产生通知（判据二）。
            snapshotted: kind.snapshot_before(),
        };
        self.records.push(rec);

        let notice = HealNotice {
            at_min,
            kind,
            text: match outcome {
                HealOutcome::Rebuilt => NOTIFY_TEXT,
                HealOutcome::Escalated => DEGRADED_NOTIFY_TEXT,
                HealOutcome::DegradedDefault => DEGRADED_NOTIFY_TEXT,
            },
            prominent: outcome != HealOutcome::Rebuilt,
        };
        self.notices.push(notice);
        rec
    }

    /// 后台策略预算切分：`work_ms` 总量按 `idle_budget_ms` 分批——返回批数
    /// （分时批处理 F049：永不超空闲窗口预算）。
    pub fn background_batches(&self, work_ms: u64) -> u64 {
        (work_ms + self.idle_budget_ms - 1) / self.idle_budget_ms.max(1)
    }

    fn bump_week(&mut self, kind: HealKind, week: u64) {
        let slot = kind_slot(kind);
        let v = &mut self.week_counts[slot];
        if let Some(pos) = v.iter().position(|(w, _)| *w == week) {
            v[pos].1 += 1;
        } else {
            v.push((week, 1));
        }
    }

    /// 该 kind×周是否已立过工单。
    fn ticket_open(&self, kind: HealKind, week: u64) -> bool {
        self.open_tickets.iter().any(|(k, w)| *k == kind && *w == week)
    }

    fn week_count(&self, kind: HealKind, week: u64) -> u32 {
        self.week_counts[kind_slot(kind)]
            .iter()
            .find(|(w, _)| *w == week)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    /// 自愈记录页数据（新→旧）。
    pub fn recent_records(&self) -> Vec<HealRecord> {
        self.records.newest_first()
    }

    /// 通知流快照。
    pub fn pending_notices(&self) -> &[HealNotice] {
        &self.notices
    }

    /// 通知已消费（F077 取走后清——不留悬挂状态）。
    pub fn drain_notices(&mut self) -> Vec<HealNotice> {
        core::mem::take(&mut self.notices)
    }

    /// 自愈成功率对账（判据一）：指定类型的 Rebuilt / 总数。
    pub fn success_rate_permille(&self, kind: HealKind) -> Option<u64> {
        let recs = self.recent_records();
        let total = recs.iter().filter(|r| r.kind == kind).count();
        if total == 0 {
            return None;
        }
        let ok = recs.iter().filter(|r| r.kind == kind && r.outcome == HealOutcome::Rebuilt).count();
        Some(ok as u64 * 1000 / total as u64)
    }

    /// 报备率对账（判据二）：notified==true 的比例——恒 1000‰。
    pub fn notify_rate_permille(&self) -> Option<u64> {
        let recs = self.recent_records();
        if recs.is_empty() {
            return None;
        }
        Some(recs.iter().filter(|r| r.notified).count() as u64 * 1000 / recs.len() as u64)
    }
}

impl Default for SelfHealSet {
    fn default() -> Self {
        Self::new()
    }
}

fn kind_slot(kind: HealKind) -> usize {
    match kind {
        HealKind::IconCache => 0,
        HealKind::ThemeToken => 1,
        HealKind::ThumbLib => 2,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F189 自检（聚合进 secstar2 域）。
pub fn run_selfheal2_checks() -> CheckSet {
    let mut set = CheckSet::new("F189-selfheal");

    // 策略与快照协同（一处一事实：令牌类留快照，其余不留）。
    set.add("strategy map", HealKind::IconCache.strategy() == RebuildStrategy::Immediate
        && HealKind::ThemeToken.strategy() == RebuildStrategy::AtomicSwap
        && HealKind::ThumbLib.strategy() == RebuildStrategy::Background, "");
    set.add("snapshot policy", HealKind::IconCache.snapshot_before() == false
        && HealKind::ThemeToken.snapshot_before() == true
        && HealKind::ThumbLib.snapshot_before() == false, "");

    // 判据一：三类损坏注入各 5 次全成功 → 成功率 1000‰。
    let mut h = SelfHealSet::new();
    for kind in [HealKind::IconCache, HealKind::ThemeToken, HealKind::ThumbLib] {
        for i in 0..5u64 {
            let rec = h.heal(i, kind, true, 10);
            set.add("rebuilt 5x", rec.outcome == HealOutcome::Rebuilt, "");
        }
    }
    set.add("success 100%", h.success_rate_permille(HealKind::IconCache) == Some(1000)
        && h.success_rate_permille(HealKind::ThemeToken) == Some(1000)
        && h.success_rate_permille(HealKind::ThumbLib) == Some(1000), "");
    // 慢性信号：同项一周 5 次自愈（虽次次成功）→ 工单已立（每 kind 一次）。
    set.add("chronic ticket", h.escalations == 3, "");

    // 判据二：通知报备 100%。
    set.add("notify 100%", h.notify_rate_permille() == Some(1000), "");
    set.add("notice count", h.pending_notices().len() == 15, "");
    set.add("notice text", h.pending_notices()[0].text == NOTIFY_TEXT && !h.pending_notices()[0].prominent, "");

    // 判据三：连续失败升级——同项 3 次失败 → 第 3 次 Escalated。
    let mut h2 = SelfHealSet::new();
    let r1 = h2.heal(10, HealKind::IconCache, false, 5);
    let r2 = h2.heal(20, HealKind::IconCache, false, 5);
    set.add("fail 1-2 degraded", r1.outcome == HealOutcome::DegradedDefault && r2.outcome == HealOutcome::DegradedDefault, "");
    let r3 = h2.heal(30, HealKind::IconCache, false, 5);
    set.add("fail 3 escalated", r3.outcome == HealOutcome::Escalated, "");
    set.add("escalation counted", h2.escalations == 1, "");
    // 跨周重置：下一周重新计。
    let r4 = h2.heal(10_080 + 10, HealKind::IconCache, false, 5);
    set.add("week reset", r4.outcome == HealOutcome::DegradedDefault, "");

    // 降级显目：失败通知升档。
    let ns = h2.drain_notices();
    set.add("degraded prominent", ns.iter().all(|n| n.prominent && n.text == DEGRADED_NOTIFY_TEXT), "");
    set.add("drain clears", h2.pending_notices().is_empty(), "");

    // 阈值旋钮钳制。
    h2.set_threshold(99);
    set.add("threshold clamp", h2.escalate_threshold == ESCALATE_MAX, "");

    // 分时批处理：600ms 活 / 200ms 预算 → 3 批。
    set.add("batches", h.background_batches(600) == 3 && h.background_batches(1) == 1, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f189_record_ring_caps() {
        let mut h = SelfHealSet::new();
        for i in 0..(HEAL_RECORD_CAP as u64 + 10) {
            h.heal(i, HealKind::ThumbLib, true, 1);
        }
        let recs = h.recent_records();
        assert_eq!(recs.len(), HEAL_RECORD_CAP);
        assert_eq!(recs[0].at_min, HEAL_RECORD_CAP as u64 + 9, "newest first");
    }

    #[test]
    fn f189_threshold_one_escalates_immediately() {
        let mut h = SelfHealSet::new();
        h.set_threshold(1);
        let r = h.heal(5, HealKind::ThemeToken, false, 1);
        assert_eq!(r.outcome, HealOutcome::Escalated);
    }

    #[test]
    fn f189_mixed_kinds_do_not_cross_count() {
        let mut h = SelfHealSet::new();
        // 图标缓存失败 2 次 + 缩略图失败 2 次 → 都不到 3，不升级。
        let a = h.heal(1, HealKind::IconCache, false, 1);
        let b = h.heal(2, HealKind::IconCache, false, 1);
        let c = h.heal(3, HealKind::ThumbLib, false, 1);
        let d = h.heal(4, HealKind::ThumbLib, false, 1);
        assert_eq!([a.outcome, b.outcome, c.outcome, d.outcome], [HealOutcome::DegradedDefault; 4]);
        assert_eq!(h.escalations, 0);
    }

    #[test]
    fn f189_snapshot_field_audited() {
        let mut h = SelfHealSet::new();
        let token = h.heal(1, HealKind::ThemeToken, true, 3);
        let cache = h.heal(2, HealKind::IconCache, true, 3);
        assert!(token.snapshotted);
        assert!(!cache.snapshotted);
    }

    #[test]
    fn f189_success_rate_unknown_kind_none() {
        let h = SelfHealSet::new();
        assert_eq!(h.success_rate_permille(HealKind::IconCache), None, "no data = no fake rate");
        assert_eq!(h.notify_rate_permille(), None);
    }

    #[test]
    fn f189_background_batches_rounding() {
        let mut h = SelfHealSet::new();
        h.idle_budget_ms = 100;
        assert_eq!(h.background_batches(0), 0, "no work no batch");
        assert_eq!(h.background_batches(101), 2);
        assert_eq!(h.background_batches(200), 2);
        assert_eq!(h.background_batches(201), 3);
    }

    #[test]
    fn f189_run_checks_pass() {
        assert!(run_selfheal2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——五个真功能面。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 深一：RebuildJob —— 重建作业（后台分时批处理的可恢复执行体）
// ---------------------------------------------------------------------------

/// 作业状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    /// 让路暂停（F049 空闲窗口结束——下个窗口续跑）。
    Paused,
    Done,
    Failed,
}

/// 一份重建作业：条目化推进（缩略图库=逐文件、图标缓存=逐桶、令牌=原子单件）。
pub struct RebuildJob {
    pub kind: HealKind,
    pub state: JobState,
    /// 总条目 / 已完成条目。
    pub total: u32,
    pub done: u32,
    /// 每窗口预算（条目数——空闲窗口让路纪律的最小单位）。
    pub budget_per_window: u32,
    /// 重试计数（单条目失败重试 ≤2，仍败 → Failed）。
    retries: u32,
    pub fail_records: u64,
}

impl RebuildJob {
    pub fn new(kind: HealKind, total: u32, budget_per_window: u32) -> RebuildJob {
        RebuildJob {
            kind,
            state: JobState::Queued,
            total: total.max(1),
            done: 0,
            budget_per_window: budget_per_window.max(1),
            retries: 0,
            fail_records: 0,
        }
    }

    /// 推进一个空闲窗口：至多 budget 条；返回本窗口完成数。
    /// `item_ok` 逐条注入（真实重建由各服务执行——本层是调度与账目）。
    pub fn run_window(&mut self, item_ok: impl Fn(u32) -> bool) -> u32 {
        match self.state {
            JobState::Done | JobState::Failed => return 0,
            JobState::Queued | JobState::Paused => self.state = JobState::Running,
            JobState::Running => {}
        }
        let mut built = 0u32;
        while self.done < self.total && built < self.budget_per_window {
            let idx = self.done;
            if item_ok(idx) {
                self.done += 1;
                built += 1;
                self.retries = 0;
            } else {
                self.retries += 1;
                self.fail_records += 1;
                if self.retries > 2 {
                    self.state = JobState::Failed;
                    return built;
                }
            }
        }
        if self.done >= self.total {
            self.state = JobState::Done;
        }
        built
    }

    /// 窗口结束让路（空闲窗口关闭——暂停不丢进度）。
    pub fn yield_window(&mut self) {
        if self.state == JobState::Running {
            self.state = JobState::Paused;
        }
    }

    pub fn progress_permille(&self) -> u64 {
        self.done as u64 * 1000 / self.total as u64
    }
}

// ---------------------------------------------------------------------------
// 深二：EscalationTicket —— 工单生命周期（自愈失败=慢性病要查根因）
// ---------------------------------------------------------------------------

/// 工单状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TicketState {
    Open,
    Investigating,
    /// 已结案（根因与处置入册）。
    Resolved { root_cause: &'static str },
}

/// 一张工单。
pub struct Ticket {
    pub kind: HealKind,
    /// 立票周序号。
    pub week: u64,
    pub state: TicketState,
    /// 票龄（天——立案至今未结的对账面）。
    pub opened_day: u64,
}

/// 工单簿。
pub struct TicketBook {
    pub tickets: Vec<Ticket>,
    /// 结案须附根因（无根因结案 = 违规，计数暴露流程缺陷）。
    pub invalid_closures: u64,
}

impl TicketBook {
    pub fn new() -> TicketBook {
        TicketBook { tickets: Vec::new(), invalid_closures: 0 }
    }

    /// 立票（同 kind 同周幂等——不刷屏）。
    pub fn open(&mut self, kind: HealKind, week: u64, opened_day: u64) -> bool {
        if self.tickets.iter().any(|t| t.kind == kind && t.week == week && t.state != TicketState::Resolved { root_cause: "" }) {
            return false;
        }
        self.tickets.push(Ticket { kind, week, state: TicketState::Open, opened_day });
        true
    }

    /// 推进调查。
    pub fn investigate(&mut self, kind: HealKind, week: u64) -> bool {
        match self.tickets.iter_mut().find(|t| t.kind == kind && t.week == week && matches!(t.state, TicketState::Open)) {
            Some(t) => {
                t.state = TicketState::Investigating;
                true
            }
            None => false,
        }
    }

    /// 结案：必须给根因（空根因 = 违规计数，票不结）。
    pub fn resolve(&mut self, kind: HealKind, week: u64, root_cause: &'static str) -> bool {
        match self.tickets.iter_mut().find(|t| t.kind == kind && t.week == week && !matches!(t.state, TicketState::Resolved { .. })) {
            Some(t) => {
                if root_cause.is_empty() {
                    self.invalid_closures += 1;
                    return false;
                }
                t.state = TicketState::Resolved { root_cause };
                true
            }
            None => false,
        }
    }

    /// 未结工单（立案超 14 天的排前——慢性病清单）。
    pub fn open_aged(&self, now_day: u64) -> Vec<(&Ticket, u64)> {
        let mut out: Vec<(&Ticket, u64)> = self
            .tickets
            .iter()
            .filter(|t| !matches!(t.state, TicketState::Resolved { .. }))
            .map(|t| (t, now_day.saturating_sub(t.opened_day)))
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1));
        out
    }
}

impl Default for TicketBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深三：RecordQuery —— 自愈记录页查询（按类型/结果/时间窗过滤）
// ---------------------------------------------------------------------------

/// 记录页查询条件（全 None = 全量）。
#[derive(Clone, Copy, Debug, Default)]
pub struct RecordQuery {
    pub kind: Option<HealKind>,
    pub outcome: Option<HealOutcome>,
    pub from_min: Option<u64>,
    pub to_min: Option<u64>,
}

impl RecordQuery {
    pub fn matches(&self, r: &HealRecord) -> bool {
        if let Some(k) = self.kind {
            if r.kind != k {
                return false;
            }
        }
        if let Some(o) = self.outcome {
            if r.outcome != o {
                return false;
            }
        }
        if let Some(f) = self.from_min {
            if r.at_min < f {
                return false;
            }
        }
        if let Some(t) = self.to_min {
            if r.at_min > t {
                return false;
            }
        }
        true
    }

    pub fn run(&self, records: &[HealRecord]) -> Vec<HealRecord> {
        records.iter().filter(|r| self.matches(r)).copied().collect()
    }
}

// ---------------------------------------------------------------------------
// 深四：DetectorPolicy —— 双时机检测（服务启动自检 + 运行中校验回调）
// ---------------------------------------------------------------------------

/// 检测时机（主册【设计细节】：服务启动时+运行中校验失败回调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectTiming {
    /// 服务启动自检（一次性全量）。
    BootScan,
    /// 运行中校验失败回调（事件驱动增量）。
    RuntimeCallback,
}

/// 每类的检测定义（触发条件文档化——判据「三类检测器各自触发条件在册」）。
#[derive(Clone, Copy, Debug)]
pub struct DetectorSpec {
    pub kind: HealKind,
    /// 启动自检项（描述性——真实校验由各服务执行）。
    pub boot_check: &'static str,
    /// 运行中回调触发条件。
    pub runtime_trigger: &'static str,
    /// 检测开销档（轻/中/重——启动预算分配依据）。
    pub cost_tier: u8,
}

pub const DETECTORS: [DetectorSpec; 3] = [
    DetectorSpec { kind: HealKind::IconCache, boot_check: "图标桶校验和逐桶核对", runtime_trigger: "渲染层图标哈希失配回调", cost_tier: 1 },
    DetectorSpec { kind: HealKind::ThemeToken, boot_check: "令牌表完整性+缺项扫描", runtime_trigger: "主题服务热替换失败回调", cost_tier: 2 },
    DetectorSpec { kind: HealKind::ThumbLib, boot_check: "缩略图库索引抽样核对", runtime_trigger: "资源管理器解码失败回调", cost_tier: 3 },
];

/// 启动自检计划：按开销档排序（轻→重），超预算的尾部让位下窗口。
pub fn boot_scan_plan(budget_tiers: u8) -> Vec<HealKind> {
    let mut ordered: Vec<&DetectorSpec> = DETECTORS.iter().collect();
    ordered.sort_by_key(|d| d.cost_tier);
    let mut acc = 0u8;
    let mut out = Vec::new();
    for d in ordered {
        if acc + d.cost_tier <= budget_tiers {
            acc += d.cost_tier;
            out.push(d.kind);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 深五：TokenSwap —— 令牌原子热替换（失败回滚旧令牌——F151 联动）
// ---------------------------------------------------------------------------

/// 原子替换执行器：新令牌全量就绪 → 原子切换 → 校验 → 失败回旧。
pub struct TokenSwap {
    /// 旧令牌指纹（回滚目标——Replace 前登记）。
    old_fingerprint: u64,
    /// 新令牌指纹（切换后应等于新集指纹）。
    pub new_fingerprint: Option<u64>,
    /// 当前生效指纹。
    pub active_fingerprint: u64,
    /// 回滚计数（新集校验失败的次数——审计面）。
    pub reverts: u64,
    pub swaps: u64,
}

impl TokenSwap {
    pub fn new(old_fingerprint: u64) -> TokenSwap {
        TokenSwap { old_fingerprint, new_fingerprint: None, active_fingerprint: old_fingerprint, reverts: 0, swaps: 0 }
    }

    /// 阶段一：新令牌就绪（登记新指纹——尚未生效）。
    pub fn stage(&mut self, new_fp: u64) {
        self.new_fingerprint = Some(new_fp);
    }

    /// 阶段二：原子切换（指纹生效）。
    pub fn commit(&mut self) -> Result<(), &'static str> {
        match self.new_fingerprint {
            Some(fp) => {
                self.active_fingerprint = fp;
                self.swaps += 1;
                Ok(())
            }
            None => Err("新令牌未就绪（先 stage）"),
        }
    }

    /// 阶段三：校验失败 → 回滚旧令牌（用户无感——原子语义的承诺）。
    pub fn verify_or_revert(&mut self, verify_ok: bool) -> bool {
        if verify_ok {
            return true;
        }
        self.active_fingerprint = self.old_fingerprint;
        self.reverts += 1;
        false
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F189 深化自检（聚合进 secstar2 域）。
pub fn run_selfheal2_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F189-deep");

    // 深一：重建作业——窗口预算推进/暂停续跑/完成冻结/失败三次止步/进度对账。
    let mut job = RebuildJob::new(HealKind::ThumbLib, 10, 3);
    set.add("job queued", job.state == JobState::Queued, "");
    let w1 = job.run_window(|_| true);
    set.add("job window budget", w1 == 3 && job.done == 3, "");
    job.yield_window();
    set.add("job paused", job.state == JobState::Paused, "");
    let w2 = job.run_window(|_| true);
    set.add("job resume", w2 == 3 && job.done == 6, "paused job continues where it left");
    set.add("job progress", job.progress_permille() == 600, "");
    job.run_window(|_| true);
    job.run_window(|_| true);
    set.add("job done", job.state == JobState::Done && job.progress_permille() == 1000, "");
    set.add("job done frozen", job.run_window(|_| true) == 0, "done job never runs again");
    // 失败路径：同一坏条目重试 ≤2，仍败 → Failed（调度面不再排它）。
    let mut j2 = RebuildJob::new(HealKind::IconCache, 5, 5);
    j2.run_window(|i| i < 2);
    set.add("job failed after retries", j2.state == JobState::Failed && j2.fail_records == 3, "");
    set.add("job failed frozen", j2.run_window(|_| true) == 0, "");

    // 深二：工单——立票幂等/调查/无根因结案违规/结案/老票排序。
    let mut book = TicketBook::new();
    set.add("ticket open", book.open(HealKind::IconCache, 3, 100), "");
    set.add("ticket idempotent", !book.open(HealKind::IconCache, 3, 100), "");
    set.add("ticket investigate", book.investigate(HealKind::IconCache, 3), "");
    set.add("ticket no cause", !book.resolve(HealKind::IconCache, 3, "") && book.invalid_closures == 1, "");
    set.add("ticket resolve", book.resolve(HealKind::IconCache, 3, "图标桶哈希算法在 4K 档溢出"), "");
    book.open(HealKind::ThumbLib, 4, 200);
    book.open(HealKind::ThemeToken, 4, 180);
    let aged = book.open_aged(400);
    set.add("ticket aged sort", aged.len() == 2 && aged[0].0.kind == HealKind::ThemeToken, "older first (180d vs 200d)");

    // 深三：记录查询——三维过滤 AND。
    let recs = [
        HealRecord { at_min: 10, kind: HealKind::IconCache, outcome: HealOutcome::Rebuilt, cost_ms: 5, notified: true, snapshotted: false },
        HealRecord { at_min: 20, kind: HealKind::IconCache, outcome: HealOutcome::DegradedDefault, cost_ms: 5, notified: true, snapshotted: false },
        HealRecord { at_min: 30, kind: HealKind::ThumbLib, outcome: HealOutcome::Rebuilt, cost_ms: 9, notified: true, snapshotted: false },
    ];
    let q1 = RecordQuery { kind: Some(HealKind::IconCache), ..Default::default() };
    set.add("query kind", q1.run(&recs).len() == 2, "");
    let q2 = RecordQuery { kind: Some(HealKind::IconCache), outcome: Some(HealOutcome::Rebuilt), ..Default::default() };
    set.add("query kind+outcome", q2.run(&recs).len() == 1, "");
    let q3 = RecordQuery { from_min: Some(15), to_min: Some(25), ..Default::default() };
    set.add("query window", q3.run(&recs).len() == 1 && q3.run(&recs)[0].at_min == 20, "");

    // 深四：检测计划——轻→重排序，预算裁尾。
    let plan = boot_scan_plan(4);
    set.add("scan plan order", plan == vec![HealKind::IconCache, HealKind::ThemeToken], "tier 1+2 ≤ 4, tier 3 waits");
    let full = boot_scan_plan(6);
    set.add("scan plan full", full.len() == 3, "");
    set.add("detector specs", DETECTORS.iter().all(|d| !d.boot_check.is_empty() && !d.runtime_trigger.is_empty()), "");

    // 深五：令牌原子替换——stage→commit→校验失败回旧。
    let mut swap = TokenSwap::new(0xAAAA);
    set.add("swap needs stage", swap.commit().is_err(), "");
    swap.stage(0xBBBB);
    swap.commit().ok();
    set.add("swap active new", swap.active_fingerprint == 0xBBBB && swap.swaps == 1, "");
    set.add("swap revert", !swap.verify_or_revert(false) && swap.active_fingerprint == 0xAAAA && swap.reverts == 1, "");
    swap.stage(0xCCCC);
    swap.commit().ok();
    set.add("swap verify ok", swap.verify_or_revert(true) && swap.active_fingerprint == 0xCCCC, "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f189_deep_job_exact_budget_boundary() {
        // 10 条 4 预算：3 窗（4+4+2）——尾窗不满预算也完成。
        let mut job = RebuildJob::new(HealKind::IconCache, 10, 4);
        assert_eq!(job.run_window(|_| true), 4);
        assert_eq!(job.run_window(|_| true), 4);
        assert_eq!(job.run_window(|_| true), 2);
        assert_eq!(job.state, JobState::Done);
        assert_eq!(job.progress_permille(), 1000);
    }

    #[test]
    fn f189_deep_ticket_reopen_next_week() {
        // 同 kind 下周再坏 → 新票（旧票已结案不挡新周立票）。
        let mut book = TicketBook::new();
        book.open(HealKind::ThemeToken, 1, 10);
        book.resolve(HealKind::ThemeToken, 1, "令牌文件被第三方写入");
        assert!(book.open(HealKind::ThemeToken, 2, 20), "next week is a new ticket");
        assert_eq!(book.tickets.len(), 2);
    }

    #[test]
    fn f189_deep_token_swap_never_loses_old() {
        // 三次全失败回滚：active 始终回到旧指纹（用户无感语义）。
        let mut swap = TokenSwap::new(777);
        for fp in [888u64, 999, 111] {
            swap.stage(fp);
            swap.commit().unwrap();
            assert!(!swap.verify_or_revert(false));
            assert_eq!(swap.active_fingerprint, 777);
        }
        assert_eq!(swap.reverts, 3);
        assert_eq!(swap.swaps, 3);
    }

    #[test]
    fn f189_deep_detector_spec_costs_sorted() {
        // 开销档严格递增（启动预算分配的确定性前提）。
        assert!(DETECTORS[0].cost_tier < DETECTORS[1].cost_tier);
        assert!(DETECTORS[1].cost_tier < DETECTORS[2].cost_tier);
        assert_eq!(boot_scan_plan(1), vec![HealKind::IconCache], "tier-1 only budget");
    }

    #[test]
    fn f189_deep_run_checks_pass() {
        assert!(run_selfheal2_deep_checks().all_passed());
    }
}
