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

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——降级默认态映射 / 快照协同
// 对账 / 通知升级显目。判据源：主册【状态与异常】「重建失败 → 降级默认态
// （图标默认集/主题默认令牌/缩略图占位）+通知升级显目」+【设计细节】
// 「自愈与 F121 还原点协同（重建前不留快照——缓存类无价值；令牌类留）」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：FallbackMap —— 降级默认态映射（每类损坏失败后的落点+人话——
// 降级是设计出来的出口，不是碰运气的残局）
// ---------------------------------------------------------------------------

/// 降级默认态（三类各自的兜底）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackState {
    /// 图标默认集（缓存类——重建失败回到出厂图标）。
    IconDefaults,
    /// 主题默认令牌（令牌类——回到 F151 默认 24 色）。
    TokenDefaults,
    /// 缩略图占位图（库类——占位图直到下次重建成功）。
    ThumbPlaceholder,
}

impl FallbackState {
    /// 兜底人话（通知正文——三要素的「下一步」）。
    pub fn text(self) -> &'static str {
        match self {
            FallbackState::IconDefaults => "图标已回到默认集，显示不受影响",
            FallbackState::TokenDefaults => "主题已回到默认令牌，可重新应用你的主题",
            FallbackState::ThumbPlaceholder => "缩略图暂以占位图显示，后台会再次尝试重建",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FallbackState::IconDefaults => "icon-defaults",
            FallbackState::TokenDefaults => "token-defaults",
            FallbackState::ThumbPlaceholder => "thumb-placeholder",
        }
    }
}

/// 损坏种类 → 兜底态（映射是查表不是分支逻辑——一处一事实）。
pub fn fallback_of(kind: HealKind) -> Option<FallbackState> {
    // 三类各有兜底（穷尽匹配——新增种类时编译器会强制补映射）。
    match kind {
        HealKind::IconCache => Some(FallbackState::IconDefaults),
        HealKind::ThemeToken => Some(FallbackState::TokenDefaults),
        HealKind::ThumbLib => Some(FallbackState::ThumbPlaceholder),
    }
}

// ---------------------------------------------------------------------------
// v3-二：SnapshotAudit —— 快照协同对账（主册【设计细节】逐字：重建前
// 不留快照——缓存类无价值；令牌类留——配置级变更。对账=执行账与策略
// 表逐位等值，豁免也要留痕）
// ---------------------------------------------------------------------------

/// 快照协同账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapAuditEntry {
    pub kind: HealKind,
    /// 是否实际留了快照。
    pub taken: bool,
    /// 策略豁免原因（taken=false 时非空）。
    pub why_not: &'static str,
}

/// 对账账本。
pub struct SnapshotAudit {
    pub entries: Vec<SnapAuditEntry>,
}

impl SnapshotAudit {
    pub fn new() -> SnapshotAudit {
        SnapshotAudit { entries: Vec::new() }
    }

    /// 重建前登记（策略唯一源=HealKind::snapshot_before）。
    pub fn record(&mut self, kind: HealKind) {
        let taken = kind.snapshot_before();
        let why_not = if taken { "" } else { "缓存类无快照价值——重建即全新" };
        self.entries.push(SnapAuditEntry { kind, taken, why_not });
    }

    /// 守恒式：执行账与策略表逐位等值、豁免必带因。
    pub fn consistent(&self) -> bool {
        self.entries.iter().all(|e| e.taken == e.kind.snapshot_before() && (e.taken || !e.why_not.is_empty()))
    }
}

impl Default for SnapshotAudit {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v3-三：NoticeEscalation —— 通知升级显目（主册【状态与异常】：重建失败
// → 降级默认态+通知**升级显目**——成功是低优先历史档，失败必须抢眼）
// ---------------------------------------------------------------------------

/// 通知优先级（F077 档位语义——自愈域只用两档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NoticePriority {
    /// 低优先历史档（成功自愈——「做过什么如实说」）。
    Low,
    /// 显目档（降级失败——必须被看见）。
    Prominent,
}

/// 通知升级判定：成功 → Low；降级/升级工单 → Prominent。
pub fn notice_priority(outcome: HealOutcome) -> NoticePriority {
    match outcome {
        HealOutcome::Rebuilt => NoticePriority::Low,
        HealOutcome::DegradedDefault | HealOutcome::Escalated => NoticePriority::Prominent,
    }
}

/// 显目通知的完整文案（三要素：发生了什么/为什么/下一步——降级态的兜底
/// 人话由 FallbackMap 提供）。
pub fn prominent_notice(kind: HealKind) -> (&'static str, NoticePriority) {
    let body = match fallback_of(kind) {
        Some(fb) => fb.text(),
        None => "已自动修复",
    };
    (body, NoticePriority::Prominent)
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F189 v3 自检（聚合进 secstar2 域）。
pub fn run_selfheal2_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F189-v3");

    // v3-一：降级映射——三类各有兜底+人话；未知类诚实 None。
    set.add("fb icon", fallback_of(HealKind::IconCache) == Some(FallbackState::IconDefaults), "");
    set.add("fb token", fallback_of(HealKind::ThemeToken) == Some(FallbackState::TokenDefaults), "");
    set.add("fb thumb", fallback_of(HealKind::ThumbLib) == Some(FallbackState::ThumbPlaceholder), "");
    set.add("fb text human", [HealKind::IconCache, HealKind::ThemeToken, HealKind::ThumbLib]
        .iter().all(|k| fallback_of(*k).map(|f| f.text().len() >= 10).unwrap_or(false)), "");
    set.add("fb names", FallbackState::TokenDefaults.name() == "token-defaults", "");

    // v3-二：快照协同——策略执行逐位等值、豁免带因。
    let mut sa = SnapshotAudit::new();
    sa.record(HealKind::IconCache);
    sa.record(HealKind::ThemeToken);
    sa.record(HealKind::ThumbLib);
    set.add("snap consistent", sa.consistent(), "");
    // 策略面：令牌类留、缓存类不留（主册逐字的对账）。
    let token = sa.entries.iter().find(|e| e.kind == HealKind::ThemeToken).unwrap();
    let cache = sa.entries.iter().find(|e| e.kind == HealKind::IconCache).unwrap();
    set.add("snap token kept", token.taken, "");
    set.add("snap cache exempt", !cache.taken && cache.why_not.contains("无快照价值"), "");

    // v3-三：通知升级——成功低档、失败显目；显目文案带兜底人话。
    set.add("prio low on ok", notice_priority(HealOutcome::Rebuilt) == NoticePriority::Low, "");
    set.add("prio prominent on degraded", notice_priority(HealOutcome::DegradedDefault) == NoticePriority::Prominent, "");
    set.add("prio prominent on escalate", notice_priority(HealOutcome::Escalated) == NoticePriority::Prominent, "");
    let (text, prio) = prominent_notice(HealKind::ThemeToken);
    set.add("prominent text", prio == NoticePriority::Prominent && text.contains("默认令牌"), "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f189_v3_fallback_covers_every_heal_kind() {
        // 枚举全覆盖：HealKind 的每个成员要么有兜底要么有明确理由（不落空）。
        let kinds = [HealKind::IconCache, HealKind::ThemeToken, HealKind::ThumbLib];
        for k in kinds {
            assert!(fallback_of(k).is_some(), "{:?} must have a fallback", k);
        }
    }

    #[test]
    fn f189_v3_snapshot_audit_survives_mixed_sequence() {
        // 十轮混合序列：账随执行增长且守恒式始终绿。
        let mut sa = SnapshotAudit::new();
        let kinds = [HealKind::IconCache, HealKind::ThemeToken, HealKind::ThumbLib];
        for i in 0..10 {
            sa.record(kinds[i % 3]);
            assert!(sa.consistent());
        }
        assert_eq!(sa.entries.len(), 10);
    }

    #[test]
    fn f189_v3_run_checks_pass() {
        assert!(run_selfheal2_deep2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v4 批次（第四轮深化 2026-09-26）——空闲窗口规划器 / 诊断页渲染模型 /
// 检测器自检 / 通知合并。判据源：主册【状态与异常】「重建占用资源超预算 →
// 分时批处理（F049 空闲窗口）」+【交互设计】「诊断中心自愈记录页（时间/
// 损坏类型/修复结果/耗时）」+【数据与存储】「三类检测器触发条件文档化」
// + F077 风暴合并联动。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v4-一：IdleWindowPlanner —— F049 空闲窗口分时计划（多作业 × 有限空闲
// 窗口的排程：后台类让路即时类、每窗口预算不超、窗口不够诚实给 ETA）
// ---------------------------------------------------------------------------

/// 待排作业（从 RebuildJob 提炼的最小排程面：类型+剩余条目+每窗预算）。
#[derive(Clone, Copy, Debug)]
pub struct PlanJob {
    pub kind: HealKind,
    pub remaining: u32,
    pub budget_per_window: u32,
}

/// 单窗口分配决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSlice {
    pub kind: HealKind,
    /// 本窗口分到的条目数。
    pub items: u32,
}

/// 排程结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduleResult {
    /// 各窗口分配序列（空窗口不落行——空闲就是空闲，不硬塞）。
    pub slices: alloc::vec::Vec<WindowSlice>,
    /// 完成全部作业所需窗口数（ETA——给进度条「预计还有 N 个空闲窗口」）。
    pub windows_needed: u32,
    /// 排程窗数不足以完成（诚实：false 时 slices 只是前缀进度）。
    pub all_fit: bool,
}

/// 排序纪律：即时(Immediate)=0 → 原子(AtomicSwap)=1 → 后台(Background)=2
/// （交互影响小的先做——主册重建顺序的排程面）。
fn strategy_rank(k: HealKind) -> u8 {
    match k.strategy() {
        RebuildStrategy::Immediate => 0,
        RebuildStrategy::AtomicSwap => 1,
        RebuildStrategy::Background => 2,
    }
}

/// 排程：按策略序逐窗口分配（每窗口内先到先得，预算封顶）。
pub fn schedule_windows(jobs: &[PlanJob], idle_windows: u32) -> ScheduleResult {
    let mut left: alloc::vec::Vec<(u8, u32, u32, HealKind)> = jobs
        .iter()
        .map(|j| (strategy_rank(j.kind), j.remaining, j.budget_per_window.max(1), j.kind))
        .collect();
    let mut slices = alloc::vec::Vec::new();
    let mut windows_used = 0u32;
    let mut all_done = false;
    for _ in 0..idle_windows {
        // 策略序稳定排序（rank 升序——同 rank 保持原序）。
        left.sort_by(|a, b| a.0.cmp(&b.0));
        let mut made_progress = false;
        for e in left.iter_mut() {
            if e.1 == 0 {
                continue;
            }
            let take = e.1.min(e.2);
            slices.push(WindowSlice { kind: e.3, items: take });
            e.1 -= take;
            made_progress = true;
        }
        windows_used += 1;
        if !made_progress {
            windows_used -= 1; // 全员完成后的空窗口不计 ETA。
            all_done = true;
            break;
        }
        if left.iter().all(|e| e.1 == 0) {
            all_done = true;
            break;
        }
    }
    let all_fit = all_done && left.iter().all(|e| e.1 == 0);
    ScheduleResult { slices, windows_needed: windows_used, all_fit }
}

// ---------------------------------------------------------------------------
// v4-二：HealPageModel —— 诊断中心「自愈记录」页渲染模型（时间/类型/
// 结果/耗时四列行；新→旧排序；结果三态过滤统计；空态三件套）
// ---------------------------------------------------------------------------

/// 记录页一行（渲染契约——列序固定，UI 层照此对齐）。
pub const HEAL_PAGE_COLUMNS: [&str; 4] = ["时间", "损坏类型", "结果", "耗时"];

/// 一行渲染数据（结果列带语义 token——正常绿 / 降级琥珀 / 升级红）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealPageRow {
    pub at_min: u64,
    pub kind_name: &'static str,
    pub outcome_text: &'static str,
    pub cost_ms: u64,
    pub token: &'static str,
}

/// 结果 → 人话+token（三态全覆盖——穷尽匹配，加枚举变体先改这里）。
fn outcome_render(o: HealOutcome) -> (&'static str, &'static str) {
    match o {
        HealOutcome::Rebuilt => ("已重建", "success"),
        HealOutcome::DegradedDefault => ("已降级默认态", "warning"),
        HealOutcome::Escalated => ("已升级工单", "danger"),
    }
}

/// 渲染记录页（新→旧；(records 任意序进入，输出恒时间倒序)）。
pub fn heal_page_rows(records: &[HealRecord]) -> alloc::vec::Vec<HealPageRow> {
    let mut rows: alloc::vec::Vec<(u64, HealPageRow)> = records
        .iter()
        .map(|r| {
            let (text, token) = outcome_render(r.outcome);
            (
                r.at_min,
                HealPageRow {
                    at_min: r.at_min,
                    kind_name: r.kind.name(),
                    outcome_text: text,
                    cost_ms: r.cost_ms,
                    token,
                },
            )
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    rows.into_iter().map(|(_, r)| r).collect()
}

/// 记录页空态文案（三件套纪律 F210——发生了什么/为什么/下一步）。
pub const HEAL_PAGE_EMPTY: &str = "暂无自愈记录。系统运行正常，或损坏刚被预防。发生自愈时这里会逐条留痕。";

/// 耗时合计（页脚「本周自愈总耗时」——资源去向诚实呈现）。
pub fn heal_page_total_cost(records: &[HealRecord]) -> u64 {
    records.iter().map(|r| r.cost_ms).sum()
}

// ---------------------------------------------------------------------------
// v4-三：detector_selftest —— 检测器自检（「触发条件文档化」的自证面：
// 三类齐、文案非空、开销档合法、启动计划轻重有序——坏了先于用户知道）
// ---------------------------------------------------------------------------

/// 自检结论（逐条可断言）。
pub struct DetectorSelftest {
    pub three_kinds: bool,
    pub docs_nonempty: bool,
    pub cost_tier_valid: bool,
    pub boot_plan_ordered: bool,
}

impl DetectorSelftest {
    pub fn ok(&self) -> bool {
        self.three_kinds && self.docs_nonempty && self.cost_tier_valid && self.boot_plan_ordered
    }
}

pub fn detector_selftest() -> DetectorSelftest {
    let three_kinds = DETECTORS.len() == 3
        && DETECTORS[0].kind != DETECTORS[1].kind
        && DETECTORS[1].kind != DETECTORS[2].kind
        && DETECTORS[0].kind != DETECTORS[2].kind;
    let docs_nonempty = DETECTORS
        .iter()
        .all(|d| !d.boot_check.is_empty() && !d.runtime_trigger.is_empty());
    let cost_tier_valid = DETECTORS.iter().all(|d| (1..=3).contains(&d.cost_tier));
    // 启动计划有序：预算=6（全装）时输出按 cost_tier 升序。
    let plan = boot_scan_plan(6);
    let boot_plan_ordered = plan.len() == 3
        && plan[0] == HealKind::IconCache
        && plan[1] == HealKind::ThemeToken
        && plan[2] == HealKind::ThumbLib;
    DetectorSelftest { three_kinds, docs_nonempty, cost_tier_valid, boot_plan_ordered }
}

// ---------------------------------------------------------------------------
// v4-四：NotifyDedup —— 通知合并窗（同类型通知在合并窗内只出一条+次数
// 合计——F077 风暴合并联动：连续自愈不让通知中心刷屏）
// ---------------------------------------------------------------------------

/// 合并窗（分钟）。
pub const NOTIFY_MERGE_MIN: u64 = 5;

/// 合并器状态。
pub struct NotifyDedup {
    /// 各类型最近一次发出的分钟戳。
    last_sent: [Option<u64>; 3],
    /// 窗内被合并吞掉的通知数（按类型累计——报备 100% 的「合并也算报备」账）。
    pub merged_away: [u64; 3],
}

impl NotifyDedup {
    pub fn new() -> NotifyDedup {
        NotifyDedup { last_sent: [None; 3], merged_away: [0; 3] }
    }

    fn slot(kind: HealKind) -> usize {
        match kind {
            HealKind::IconCache => 0,
            HealKind::ThemeToken => 1,
            HealKind::ThumbLib => 2,
        }
    }

    /// 提交通知 → 返回是否真正发出（窗内同类 = 合并吞掉并计数）。
    /// 降级显目通知（prominent）不受合并窗约束——失败通知永不吞。
    pub fn submit(&mut self, kind: HealKind, at_min: u64, prominent: bool) -> bool {
        let s = Self::slot(kind);
        if !prominent {
            if let Some(last) = self.last_sent[s] {
                if at_min.saturating_sub(last) < NOTIFY_MERGE_MIN {
                    self.merged_away[s] += 1;
                    return false;
                }
            }
        }
        self.last_sent[s] = Some(at_min);
        true
    }

    /// 合并总数（诊断对账）。
    pub fn merged_total(&self) -> u64 {
        self.merged_away.iter().sum()
    }
}

impl Default for NotifyDedup {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v4 自检
// ---------------------------------------------------------------------------

/// F189 v4 自检（聚合进 secstar2 域）。
pub fn run_selfheal2_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F189-v4");

    // v4-一：空闲窗口排程——策略序、预算封顶、ETA、不足诚实。
    let jobs = [
        PlanJob { kind: HealKind::ThumbLib, remaining: 10, budget_per_window: 3 },
        PlanJob { kind: HealKind::IconCache, remaining: 4, budget_per_window: 2 },
        PlanJob { kind: HealKind::ThemeToken, remaining: 1, budget_per_window: 1 },
    ];
    let sch = schedule_windows(&jobs, 10);
    set.add("sched fits", sch.all_fit, "10 窗足够完成");
    set.add("sched eta", sch.windows_needed == 4, "IconCache 2 窗 + 令牌 1 窗 + 缩略图 4 窗 = 4 窗（并行语义每窗全员出力）");
    set.add("sched budget cap", sch.slices.iter().all(|s| s.items <= 3), "");
    // 窗口不足：2 窗只够前缀（all_fit=false 诚实）。
    let sch2 = schedule_windows(&jobs, 2);
    set.add("sched shortfall honest", !sch2.all_fit && sch2.windows_needed == 2, "");
    // 策略序：首窗首个切片必是即时类（IconCache）。
    set.add("sched strategy order", sch.slices[0].kind == HealKind::IconCache, "即时类最先");

    // v4-二：记录页渲染——倒序、token 三态、空态、耗时合计。
    let recs = [
        HealRecord { at_min: 100, kind: HealKind::IconCache, outcome: HealOutcome::Rebuilt, cost_ms: 12, notified: true, snapshotted: false },
        HealRecord { at_min: 105, kind: HealKind::ThemeToken, outcome: HealOutcome::DegradedDefault, cost_ms: 30, notified: true, snapshotted: true },
        HealRecord { at_min: 98, kind: HealKind::ThumbLib, outcome: HealOutcome::Escalated, cost_ms: 5, notified: true, snapshotted: false },
    ];
    let page = heal_page_rows(&recs);
    set.add("page columns", HEAL_PAGE_COLUMNS == ["时间", "损坏类型", "结果", "耗时"], "");
    set.add("page newest first", page[0].at_min == 105 && page[2].at_min == 98, "");
    set.add("page tokens", page[0].token == "warning" && page[1].token == "success" && page[2].token == "danger", "");
    set.add("page names", page[0].kind_name == "主题令牌" && page[1].kind_name == "图标缓存", "");
    set.add("page total cost", heal_page_total_cost(&recs) == 47, "");
    set.add("page empty text", HEAL_PAGE_EMPTY.contains("暂无自愈记录"), "");
    set.add("page empty render", heal_page_rows(&[]).is_empty(), "");

    // v4-三：检测器自检——四结论全绿。
    let st = detector_selftest();
    set.add("det three kinds", st.three_kinds, "");
    set.add("det docs", st.docs_nonempty, "");
    set.add("det tiers", st.cost_tier_valid, "");
    set.add("det plan order", st.boot_plan_ordered, "");
    set.add("det all ok", st.ok(), "");

    // v4-四：通知合并——窗内吞、窗外发、显目不吞、合并留账。
    let mut dd = NotifyDedup::new();
    set.add("dedup first out", dd.submit(HealKind::IconCache, 10, false), "");
    set.add("dedup window merged", !dd.submit(HealKind::IconCache, 12, false), "5 分钟窗内吞掉");
    set.add("dedup merged counted", dd.merged_away[0] == 1, "");
    set.add("dedup after window", dd.submit(HealKind::IconCache, 16, false), "窗过即发");
    set.add("dedup prominent always", dd.submit(HealKind::IconCache, 16, true), "降级显目永不吞");
    set.add("dedup per kind", dd.submit(HealKind::ThumbLib, 10, false), "不同类型互不合并");
    set.add("dedup total", dd.merged_total() == 1, "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn f189_v4_schedule_all_background_stress() {
        // 全后台类大作业压测：预算/窗与窗口数的关系是纯算术（可预算 ETA）。
        let jobs = [PlanJob { kind: HealKind::ThumbLib, remaining: 99, budget_per_window: 10 }];
        let sch = schedule_windows(&jobs, 20);
        // 99 条 / 10 条每窗 = 向上取整 10 窗，20 窗预算装得下。
        assert!(sch.all_fit);
        assert_eq!(sch.windows_needed, 10);
        let done: u32 = sch.slices.iter().map(|s| s.items).sum();
        assert_eq!(done, 99, "全部条目分配且不超不欠");
    }

    #[test]
    fn f189_v4_schedule_zero_remaining_edge() {
        // 剩余 0 的作业不占窗口（空转防线）。
        let jobs = [
            PlanJob { kind: HealKind::IconCache, remaining: 0, budget_per_window: 5 },
            PlanJob { kind: HealKind::ThumbLib, remaining: 2, budget_per_window: 2 },
        ];
        let sch = schedule_windows(&jobs, 5);
        assert!(sch.all_fit && sch.windows_needed == 1);
        assert!(sch.slices.iter().all(|s| s.kind == HealKind::ThumbLib));
    }

    #[test]
    fn f189_v4_page_sort_stability() {
        // 同分钟记录保序（稳定排序——同刻记录按输入序展示，不抖动）。
        let recs = [
            HealRecord { at_min: 50, kind: HealKind::IconCache, outcome: HealOutcome::Rebuilt, cost_ms: 1, notified: true, snapshotted: false },
            HealRecord { at_min: 50, kind: HealKind::ThemeToken, outcome: HealOutcome::Rebuilt, cost_ms: 2, notified: true, snapshotted: true },
        ];
        let page = heal_page_rows(&recs);
        assert_eq!(page[0].kind_name, "图标缓存");
        assert_eq!(page[1].kind_name, "主题令牌");
    }

    #[test]
    fn f189_v4_dedup_window_boundary() {
        // 边界：恰好 NOTIFY_MERGE_MIN 分钟差 = 窗外（发）。
        let mut dd = NotifyDedup::new();
        assert!(dd.submit(HealKind::ThumbLib, 0, false));
        assert!(!dd.submit(HealKind::ThumbLib, NOTIFY_MERGE_MIN - 1, false));
        assert!(dd.submit(HealKind::ThumbLib, NOTIFY_MERGE_MIN, false));
        assert_eq!(dd.merged_away[2], 1);
    }

    #[test]
    fn f189_v4_run_checks_pass() {
        assert!(run_selfheal2_deep3_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v5 批次（第五轮深化 · 上限口径冲刺）——周统计账 + 重建 ETA 行。
// 判据源：主册【交互设计】「连续自愈同项 ≥3 次/周 → 升级为工单」的
// 统计数据源 + 进度 ETA 人话化。
// ---------------------------------------------------------------------------

use alloc::string::String;

/// 周统计行（kind × 周序 → 次数——升级判定的账面）。
pub fn heal_week_stats(records: &[HealRecord], week_len_min: u64) -> Vec<(HealKind, u64, usize)> {
    let mut out: Vec<(HealKind, u64, usize)> = Vec::new();
    for r in records {
        let week = r.at_min / week_len_min.max(1);
        match out.iter_mut().find(|(k, w, _)| *k == r.kind && *w == week) {
            Some((_, _, n)) => *n += 1,
            None => out.push((r.kind, week, 1)),
        }
    }
    out
}

/// 升级预警（周内同项 >=3 次——与 SelfHealSet 的升级语义同尺）。
pub fn week_escalation_candidates(records: &[HealRecord], week_len_min: u64, threshold: u32) -> Vec<(HealKind, u64)> {
    heal_week_stats(records, week_len_min)
        .into_iter()
        .filter(|(_, _, n)| *n as u32 >= threshold)
        .map(|(k, w, _)| (k, w))
        .collect()
}

/// 重建 ETA 人话行（剩余条目 / 每窗预算 → 「预计 N 个空闲窗口」）。
pub fn rebuild_eta_text(remaining: u32, budget_per_window: u32) -> String {
    if remaining == 0 {
        return String::from("重建已完成");
    }
    let windows = remaining.div_ceil(budget_per_window.max(1));
    alloc::format!("预计还需 {} 个空闲窗口（每窗 {} 条）", windows, budget_per_window.max(1))
}

/// F189 v5 自检（deep4 表）。
pub fn run_selfheal2_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F189-v5");

    let recs = [
        HealRecord { at_min: 10, kind: HealKind::IconCache, outcome: HealOutcome::Rebuilt, cost_ms: 5, notified: true, snapshotted: false },
        HealRecord { at_min: 20, kind: HealKind::IconCache, outcome: HealOutcome::Rebuilt, cost_ms: 6, notified: true, snapshotted: false },
        HealRecord { at_min: 30, kind: HealKind::IconCache, outcome: HealOutcome::Rebuilt, cost_ms: 7, notified: true, snapshotted: false },
        HealRecord { at_min: 2000, kind: HealKind::ThumbLib, outcome: HealOutcome::Rebuilt, cost_ms: 40, notified: true, snapshotted: false },
    ];
    // 周统计：week_len=1000min → 周 0（3 次图标）+ 周 2（1 次缩略图）。
    let stats = heal_week_stats(&recs, 1000);
    set.add("week stats", stats.iter().any(|(k, w, n)| *k == HealKind::IconCache && *w == 0 && *n == 3), "");
    set.add("week other", stats.iter().any(|(k, w, n)| *k == HealKind::ThumbLib && *w == 2 && *n == 1), "");
    // 升级候选：阈值 3 → 图标缓存命中。
    let cand = week_escalation_candidates(&recs, 1000, 3);
    set.add("week escalate", cand.len() == 1 && cand[0].0 == HealKind::IconCache, "");
    let cand2 = week_escalation_candidates(&recs, 1000, 4);
    set.add("week escalate below", cand2.is_empty(), "阈值 4 无命中");

    // ETA 行——0 剩余、正常除法、向上取整。
    set.add("eta done", rebuild_eta_text(0, 10).contains("已完成"), "");
    set.add("eta exact", rebuild_eta_text(20, 10).contains("2 个"), "");
    set.add("eta ceil", rebuild_eta_text(21, 10).contains("3 个"), "21/10 向上取整");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn f189_v4_week_stats_multiple_kinds() {
        // 三类混排 12 条：各 kind 各周的计数互不串账。
        let mut recs = Vec::new();
        for i in 0..12u64 {
            recs.push(HealRecord {
                at_min: i * 10,
                kind: match i % 3 {
                    0 => HealKind::IconCache,
                    1 => HealKind::ThemeToken,
                    _ => HealKind::ThumbLib,
                },
                outcome: HealOutcome::Rebuilt,
                cost_ms: 1,
                notified: true,
                snapshotted: false,
            });
        }
        let stats = heal_week_stats(&recs, 40);
        let total: usize = stats.iter().map(|(_, _, n)| n).sum();
        assert_eq!(total, 12);
        assert_eq!(stats.len(), 9, "3 类 x 3 周 = 9 组");
    }

    #[test]
    fn f189_v4_run_checks_pass() {
        assert!(run_selfheal2_deep4_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v6 批次（第六轮深化 · 上限口径收官）——各类平均耗时统计。
// 判据源：主册【交互设计】诊断中心「自愈记录页（时间/损坏类型/修复结果/
// 耗时）」的聚合面。
// ---------------------------------------------------------------------------

/// 各类平均耗时（µs 级精度不装——ms 均值四舍五入）。
pub fn heal_avg_cost(records: &[HealRecord]) -> Vec<(HealKind, u64)> {
    let mut out: Vec<(HealKind, (u64, u64))> = Vec::new(); // (kind, (sum, n))
    for r in records {
        match out.iter_mut().find(|(k, _)| *k == r.kind) {
            Some((_, (s, n))) => {
                *s += r.cost_ms;
                *n += 1;
            }
            None => out.push((r.kind, (r.cost_ms, 1))),
        }
    }
    out.into_iter().map(|(k, (s, n))| (k, s / n.max(1))).collect()
}

/// F189 v6 自检（deep5 表）。
pub fn run_selfheal2_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F189-v6");

    let recs = [
        HealRecord { at_min: 1, kind: HealKind::IconCache, outcome: HealOutcome::Rebuilt, cost_ms: 10, notified: true, snapshotted: false },
        HealRecord { at_min: 2, kind: HealKind::IconCache, outcome: HealOutcome::Rebuilt, cost_ms: 20, notified: true, snapshotted: false },
        HealRecord { at_min: 3, kind: HealKind::ThumbLib, outcome: HealOutcome::Rebuilt, cost_ms: 90, notified: true, snapshotted: false },
    ];
    let avg = heal_avg_cost(&recs);
    set.add("avg icon", avg.iter().any(|(k, c)| *k == HealKind::IconCache && *c == 15), "(10+20)/2 = 15");
    set.add("avg thumb", avg.iter().any(|(k, c)| *k == HealKind::ThumbLib && *c == 90), "");
    set.add("avg empty", heal_avg_cost(&[]).is_empty(), "");

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn f189_v5_avg_rounding() {
        // 整除截断语义（10+11)/2 = 10——均值向下取整，页脚注明口径。
        let recs = [
            HealRecord { at_min: 1, kind: HealKind::ThemeToken, outcome: HealOutcome::Rebuilt, cost_ms: 10, notified: true, snapshotted: true },
            HealRecord { at_min: 2, kind: HealKind::ThemeToken, outcome: HealOutcome::Rebuilt, cost_ms: 11, notified: true, snapshotted: true },
        ];
        let avg = heal_avg_cost(&recs);
        assert_eq!(avg[0].1, 10);
    }

    #[test]
    fn f189_v5_run_checks_pass() {
        assert!(run_selfheal2_deep5_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v7 批次（第七轮深化 · 上限口径收官）——自愈健康度评分。
// 判据源：主册【验收判据】「三类损坏注入各 5 次自愈成功率 100%」的评分面。
// ---------------------------------------------------------------------------

/// 健康度评分（成功率 permille + 升级工单数 → 0-1000 分）。
pub fn selfheal_health_score(records: &[HealRecord], open_tickets: usize) -> u64 {
    if records.is_empty() {
        return 1000; // 无自愈=满分（没生病就是健康）。
    }
    let rebuilt = records.iter().filter(|r| r.outcome == HealOutcome::Rebuilt).count() as u64;
    let rate = rebuilt * 1000 / records.len() as u64;
    // 每张未闭工单扣 100 分（慢性病直接拉低健康度）。
    rate.saturating_sub(open_tickets as u64 * 100)
}

/// F189 v7 自检（deep6 表）。
pub fn run_selfheal2_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F189-v7");

    let perfect = [
        HealRecord { at_min: 1, kind: HealKind::IconCache, outcome: HealOutcome::Rebuilt, cost_ms: 1, notified: true, snapshotted: false },
        HealRecord { at_min: 2, kind: HealKind::IconCache, outcome: HealOutcome::Rebuilt, cost_ms: 2, notified: true, snapshotted: false },
    ];
    set.add("score perfect", selfheal_health_score(&perfect, 0) == 1000, "全愈无票=1000");
    set.add("score empty", selfheal_health_score(&[], 0) == 1000, "无自愈=满分");

    let degraded = [
        HealRecord { at_min: 1, kind: HealKind::ThemeToken, outcome: HealOutcome::DegradedDefault, cost_ms: 5, notified: true, snapshotted: true },
        HealRecord { at_min: 2, kind: HealKind::ThemeToken, outcome: HealOutcome::Rebuilt, cost_ms: 5, notified: true, snapshotted: true },
    ];
    set.add("score half", selfheal_health_score(&degraded, 0) == 500, "1/2 愈 = 500");
    set.add("score ticket penalty", selfheal_health_score(&degraded, 2) == 300, "500 - 200（2 票）= 300");
    set.add("score floor", selfheal_health_score(&degraded, 9) == 0, "扣到底不转负（saturating）");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn f189_v6_score_all_escalated() {
        // 全升级场景：0% 成功率 - 票 → 0（不转负）。
        let recs = [HealRecord { at_min: 1, kind: HealKind::ThumbLib, outcome: HealOutcome::Escalated, cost_ms: 1, notified: true, snapshotted: false }];
        assert_eq!(selfheal_health_score(&recs, 1), 0);
    }

    #[test]
    fn f189_v6_run_checks_pass() {
        assert!(run_selfheal2_deep6_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b9：探活节奏旋钮（自愈探测间隔的可调账）。
// 判据源：主册【设计细节】「自愈探活节奏分级（忙时稀疏/闲时密集）」。
// ---------------------------------------------------------------------------

/// 探活间隔决策（系统负载档 → 探活间隔秒；忙时稀疏不打扰）。
pub fn probe_interval_s(busy_level: u8) -> u64 {
    match busy_level {
        0 => 30,  // 空闲：密集探活。
        1 => 120, // 中载。
        _ => 600, // 忙时：稀疏（10 分钟一探）。
    }
}

/// F189 v8 自检（deep7 表）。
pub fn run_selfheal2_deep7_checks() -> CheckSet {
    let mut set = CheckSet::new("F189-v8");

    set.add("probe idle", probe_interval_s(0) == 30, "空闲 30s 一探");
    set.add("probe busy", probe_interval_s(2) == 600, "忙时 10min 一探");
    set.add("probe monotone", probe_interval_s(0) < probe_interval_s(1) && probe_interval_s(1) < probe_interval_s(2), "越忙越稀疏");

    set
}

#[cfg(test)]
mod deep7_tests {
    use super::*;

    #[test]
    fn f189_v8_probe_high_level() {
        // 任意高负载档都取最稀疏档（兜底分支覆盖）。
        assert_eq!(probe_interval_s(9), 600);
    }

    #[test]
    fn f189_v8_run_checks_pass() {
        assert!(run_selfheal2_deep7_checks().all_passed());
    }
}
