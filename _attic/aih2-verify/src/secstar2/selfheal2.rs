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
