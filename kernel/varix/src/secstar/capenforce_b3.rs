//! F177 能力执法可视化 · 批次三深化（secstar · G-G-07）。
//!
//! 批次三功能面（主册判据「四执法点注入准确 / 聚合正确 / 总闸全效」纵深）：
//! - [`CapTable`]：应用×能力授予位图表——32 应用槽，六能力位图一查即得
//!   （执法点问「这个应用有没有这个能力」的 O(1) 真相源）；
//! - [`DecideEngine`]：执法决策引擎——按执法点×规则动作（放/拦/记）
//!   三态裁决，总闸一关全拦（总闸全效的实现面）；
//! - [`StormGuard`]：通知风暴抑制——同应用冷却窗内再拦不重复通知
//!   （STORM_PER_MIN 线的执行面——防打扰与不静默的平衡点）；
//! - [`EscalationLadder`]：升档阶梯——同规则连拦 N 次逐级升档
//!   （观察→提示→拦截→隔离四级，只升不降——升档判据的阶梯面）。
//!
//! 零堆纪律：定长位图表 + 定长冷却表，无 alloc。

use super::capenforce::{POINT_N, STORM_PER_MIN, STORM_WINDOW_MS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 能力授予位图表
// ---------------------------------------------------------------------------

/// 应用槽上限。
pub const APP_CAP: usize = 32;
/// 六能力位宽。
pub const CAP_BITS: u8 = 6;

/// 应用×能力位图（bit i = CapKind ord i）。
pub struct CapTable {
    ids: [Option<u32>; APP_CAP],
    caps: [u8; APP_CAP],
    pub n: usize,
}

impl CapTable {
    pub const fn new() -> CapTable {
        CapTable { ids: [const { None }; APP_CAP], caps: [0; APP_CAP], n: 0 }
    }

    /// 授予：追加能力位（新应用自动开槽；满容诚实拒）。
    pub fn grant(&mut self, app_id: u32, cap_bit: u8) -> bool {
        if cap_bit >= CAP_BITS {
            return false;
        }
        for i in 0..self.n {
            if self.ids[i] == Some(app_id) {
                self.caps[i] |= 1 << cap_bit;
                return true;
            }
        }
        if self.n >= APP_CAP {
            return false;
        }
        self.ids[self.n] = Some(app_id);
        self.caps[self.n] = 1 << cap_bit;
        self.n += 1;
        true
    }

    /// 收回：清能力位（即时执法——下一查即拒）。
    pub fn revoke(&mut self, app_id: u32, cap_bit: u8) -> bool {
        for i in 0..self.n {
            if self.ids[i] == Some(app_id) {
                self.caps[i] &= !(1 << cap_bit);
                return true;
            }
        }
        false
    }

    /// 查询：应用是否持有能力位。
    pub fn has(&self, app_id: u32, cap_bit: u8) -> bool {
        (0..self.n).any(|i| self.ids[i] == Some(app_id) && self.caps[i] & (1 << cap_bit) != 0)
    }

    /// 总闸：全表清零（总闸全效——一关全拦的表面）。
    pub fn kill_all(&mut self) {
        self.caps = [0; APP_CAP];
    }
}

// ---------------------------------------------------------------------------
// 执法决策引擎
// ---------------------------------------------------------------------------

/// 决策三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    /// 放行但记录（观察态——不拦截但留痕，升档阶梯的第一级）。
    AllowLogged,
}

/// 决策引擎输入一行：能力持有 + 总闸 + 规则动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleAction {
    Observe,
    Block,
}

/// 裁决：总闸开 → 全 Deny；未持能力 → Deny；规则 Observe → AllowLogged；
/// 规则 Block + 有能力 → Allow（规则 Block 的语义是「无授权时拦」——
/// 授权在表则放行，真相源是 CapTable 不是规则）。
pub fn decide(has_cap: bool, master_off: bool, action: RuleAction) -> Decision {
    if master_off {
        return Decision::Deny;
    }
    if !has_cap {
        return Decision::Deny;
    }
    match action {
        RuleAction::Observe => Decision::AllowLogged,
        RuleAction::Block => Decision::Allow,
    }
}

// ---------------------------------------------------------------------------
// 通知风暴抑制
// ---------------------------------------------------------------------------

/// 冷却表：每应用最近一次通知时刻（0=从未通知）。
pub struct StormGuard {
    last_notify: [u64; APP_CAP],
    app_ids: [Option<u32>; APP_CAP],
}

impl StormGuard {
    pub const fn new() -> StormGuard {
        StormGuard { last_notify: [0; APP_CAP], app_ids: [const { None }; APP_CAP] }
    }

    /// 是否应通知：冷却窗（60s/50 条线的窗面——同应用 1 分钟内不重复）。
    pub fn should_notify(&mut self, app_id: u32, now_ms: u64) -> bool {
        let mut slot = None;
        for i in 0..APP_CAP {
            if self.app_ids[i] == Some(app_id) {
                slot = Some(i);
                break;
            }
        }
        let i = match slot {
            Some(i) => i,
            None => {
                // 新应用开槽（满容复用 0 槽——通知面丢槽优于 panic）。
                let i = self.app_ids.iter().position(|a| a.is_none()).unwrap_or(0);
                self.app_ids[i] = Some(app_id);
                self.last_notify[i] = now_ms;
                return true; // 首次拦截必通知——新应用不该被旧冷却冤枉
            }
        };
        // 老槽按冷却窗判（last=0 是合法时刻不是哨兵——t=0 通知也算数）。
        let last = self.last_notify[i];
        if now_ms.saturating_sub(last) >= STORM_WINDOW_MS {
            self.last_notify[i] = now_ms;
            true
        } else {
            false
        }
    }

    /// 窗口内被抑制的通知计数（防打扰不等于吞——抑制也要有账）。
    pub fn suppressed(&self, app_id: u32, events_in_window: u64) -> u64 {
        let notified = if self.app_ids.iter().any(|a| *a == Some(app_id)) { 1 } else { 0 };
        events_in_window.saturating_sub(notified)
    }
}

// ---------------------------------------------------------------------------
// 升档阶梯（四级只升不降）
// ---------------------------------------------------------------------------

/// 阶梯四级。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EscLevel {
    Watch,
    Notice,
    Enforce,
    Isolate,
}

/// 连拦升档线：每级 5 次连拦升一级。
pub const ESCALATE_STEP: u32 = 5;

pub struct EscalationLadder {
    pub level: EscLevel,
    strikes: u32,
}

impl EscalationLadder {
    pub const fn new() -> EscalationLadder {
        EscalationLadder { level: EscLevel::Watch, strikes: 0 }
    }

    /// 记一次拦截：连拦满 5 升一级，至 Isolate 封顶（只升不降）。
    pub fn on_intercept(&mut self) {
        self.strikes += 1;
        if self.strikes >= ESCALATE_STEP {
            self.strikes = 0;
            self.level = match self.level {
                EscLevel::Watch => EscLevel::Notice,
                EscLevel::Notice => EscLevel::Enforce,
                EscLevel::Enforce | EscLevel::Isolate => EscLevel::Isolate,
            };
        }
    }

    /// 合规观察（绿期）不清级——只升不降的纪律本体。
    pub fn on_clean_tick(&self) -> EscLevel {
        self.level
    }

    pub fn strikes(&self) -> u32 {
        self.strikes
    }
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_capenforce_b3_checks() -> CheckSet {
    use EscLevel as EL;
    let mut cs = CheckSet::new("F177-b3");

    // 1) 位图表授予-查询：授 2 位查 2 位（O(1) 真相源两面全对）。
    let mut t = CapTable::new();
    t.grant(7, 0);
    t.grant(7, 2);
    cs.add("captable_grant_query", t.has(7, 0) && t.has(7, 2) && !t.has(7, 1), "");

    // 2) 收回即时：revoke 后下一查即拒（收回即时性的表面）。
    t.revoke(7, 2);
    cs.add("captable_revoke_immediate", !t.has(7, 2) && t.has(7, 0), "");

    // 3) 越界能力位诚实拒：bit 6 不存在（六能力上限）。
    cs.add("captable_bit_bounded", !t.grant(7, 6) && !t.has(7, 6), "");

    // 4) 总闸：kill_all 后全表拒（总闸全效）。
    let mut t2 = CapTable::new();
    t2.grant(1, 0);
    t2.grant(2, 5);
    t2.kill_all();
    cs.add("captable_kill_all", (0..CAP_BITS).all(|b| !t2.has(1, b)) && !t2.has(2, 5), "");

    // 5) 满容诚实：32 应用满后第 33 个拒（容量不静默扩）。
    let mut t3 = CapTable::new();
    let mut all = true;
    for a in 0..APP_CAP as u32 {
        all &= t3.grant(a, 0);
    }
    cs.add("captable_cap_honest", all && t3.n == APP_CAP && !t3.grant(99, 0), "");

    // 6) 决策四路：总闸 Deny / 无能力 Deny / 观察 AllowLogged / 命中 Allow。
    cs.add(
        "decide_four_paths",
        decide(true, true, RuleAction::Block) == Decision::Deny
            && decide(false, false, RuleAction::Block) == Decision::Deny
            && decide(true, false, RuleAction::Observe) == Decision::AllowLogged
            && decide(true, false, RuleAction::Block) == Decision::Allow,
        "",
    );

    // 7) 决策真相源纪律：规则 Block 但能力已收回 → Deny（表不是摆设）。
    let mut t4 = CapTable::new();
    t4.grant(5, 1);
    let d1 = decide(t4.has(5, 1), false, RuleAction::Block);
    t4.revoke(5, 1);
    let d2 = decide(t4.has(5, 1), false, RuleAction::Block);
    cs.add("decide_table_is_truth", d1 == Decision::Allow && d2 == Decision::Deny, "");

    // 8) 风暴抑制：冷却窗内第二次拦不重复通知（防打扰面）。
    let mut g = StormGuard::new();
    let n1 = g.should_notify(9, 1_000);
    let n2 = g.should_notify(9, 2_000);
    let n3 = g.should_notify(9, 61_500);
    cs.add("storm_cooldown", n1 && !n2 && n3, "");

    // 9) 抑制有账：窗口 3 事件通知 1 → 抑制 2（不静默吞）。
    cs.add("storm_suppressed_counted", g.suppressed(9, 3) == 2, "");

    // 10) 升档阶梯：连拦 5 升一级、20 连拦封顶 Isolate（只升不降）。
    let mut l = EscalationLadder::new();
    for _ in 0..ESCALATE_STEP {
        l.on_intercept();
    }
    let one = l.level == EL::Notice;
    for _ in 0..(ESCALATE_STEP * 3) {
        l.on_intercept();
    }
    cs.add("ladder_escalates_to_isolate", one && l.level == EL::Isolate, "");

    // 11) 只升不降：绿期不清级（Isolate 不会自己回落）。
    cs.add("ladder_never_descends", l.on_clean_tick() == EL::Isolate, "");

    // 12) 阶梯与执法点数贯通：四执法点 / 风暴线常量一处一事实。
    cs.add(
        "consts_aligned",
        POINT_N == 4 && STORM_PER_MIN == 50 && STORM_WINDOW_MS == 60_000,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;
    use EscLevel as EL;

    #[test]
    fn captable_regrant_after_revoke() {
        // 收回后再授予：同位复用无残留（表的读写对称性）。
        let mut t = CapTable::new();
        t.grant(3, 4);
        t.revoke(3, 4);
        assert!(!t.has(3, 4));
        t.grant(3, 4);
        assert!(t.has(3, 4));
        assert_eq!(t.n, 1, "收回不并槽、重授不开槽");
    }

    #[test]
    fn ladder_strikes_reset_per_level() {
        // 每级连拦计数独立：4 拦 + 升级清零 → 再 4 拦仍在 Notice。
        let mut l = EscalationLadder::new();
        for _ in 0..(ESCALATE_STEP - 1) {
            l.on_intercept();
        }
        assert_eq!(l.level, EL::Watch);
        assert_eq!(l.strikes(), 4);
        l.on_intercept();
        assert_eq!(l.level, EL::Notice);
        assert_eq!(l.strikes(), 0);
        for _ in 0..(ESCALATE_STEP - 1) {
            l.on_intercept();
        }
        assert_eq!(l.level, EL::Notice, "未满 5 不升");
    }

    #[test]
    fn storm_per_app_isolation() {
        // 冷却按应用隔离：应用 A 冷却中不影响应用 B 通知。
        let mut g = StormGuard::new();
        let a1 = g.should_notify(1, 0);
        let b1 = g.should_notify(2, 100);
        let a2 = g.should_notify(1, 200);
        assert!(a1 && b1 && !a2);
    }
}
