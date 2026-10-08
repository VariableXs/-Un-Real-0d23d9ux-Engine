//! F038 深化批次三 · 档位执行流水线面（compatstar2/deep2 · G-A-38）。
//!
//! 批次一深化覆盖九宫格矩阵/临时放行/继承，批次二覆盖进程代际/生效
//! 时点；本批补齐【功能定义】档位裁决全语义对齐的执行/边界/注入面：
//! 操作拦截判定六步流水（请求→规则→档位→配额→审计→落点，deny 即止并
//! 带原因码）、配额令牌桶（容量 500/750/1000 三档对应严格/标准/宽松，
//! 速率补充、取用不足 deny 并计数——主册【功能定义】「配额随档联动」）、
//! 档位降级观察窗（错误率超阈值 5% 持续 60s → 建议降档事件入账，只建议
//! 不擅改）、审计聚合视图（按档位×操作类型二维计数表 3×4，行/列合计
//! 一致性自检）。
//!
//! 判据对账：主册 G-A-38【功能定义】三档隔离 + 配额联动 +【状态与异常】
//! 「宽松档滥用 → 审计告警建议升档」的对偶面（降档建议）；令牌桶为
//! 通行配额算法口径（域内量化，无 MS 面——如实注明）。
//!
//! 零堆纪律：定长表 + 定长计数矩阵，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 六步流水步骤名（请求→规则→档位→配额→审计→落点）。
pub const PIPELINE_STEPS: [&str; 6] = ["request", "rule", "level", "quota", "audit", "landing"];
/// 各步拒绝原因码（deny 即止并带原因码——显性化纪律）。
pub const DENY_REASONS: [&str; 6] =
    ["deny-request", "deny-rule", "deny-level", "deny-quota", "deny-audit", "deny-landing"];
/// 配额桶容量三档：严格 500 / 标准 750 / 宽松 1000（主册【功能定义】
/// 「配额（F195）随档联动」；三档次序与 isolevel::LEVELS 对齐）。
pub const TOKEN_CAPS: [u32; 3] = [500, 750, 1000];
/// 降档观察错误率阈值 5%（permille 50）。
pub const DOWNGRADE_ERR_PERMILLE: u32 = 50;
/// 降档观察窗 60s（持续超阈值才建议——防抖）。
pub const DOWNGRADE_WINDOW_S: u64 = 60;
/// 审计聚合表操作类型四列。
pub const AUDIT_OPS: [&str; 4] = ["exec", "net", "doc-write", "clipboard"];

// ---------------------------------------------------------------------------
// 六步拦截流水
// ---------------------------------------------------------------------------

/// 六步流水裁决：deny 即止并带原因码；全过才放行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Verdict {
    pub allowed: bool,
    /// 停在哪一步（allowed 时为 6 = 走完全部步骤）。
    pub stopped_at: usize,
    pub reason: &'static str,
}

/// 六步拦截判定：请求→规则→档位→配额→审计→落点，逐步 pass/deny，
/// 首个 deny 即止（后续步骤不再评估——流水即止语义）。
pub fn run_pipeline(
    req_valid: bool,
    rule_allow: bool,
    level_allow: bool,
    quota_ok: bool,
    audit_ok: bool,
    landing_ok: bool,
) -> Verdict {
    let gates = [req_valid, rule_allow, level_allow, quota_ok, audit_ok, landing_ok];
    for (i, &g) in gates.iter().enumerate() {
        if !g {
            return Verdict { allowed: false, stopped_at: i, reason: DENY_REASONS[i] };
        }
    }
    Verdict { allowed: true, stopped_at: PIPELINE_STEPS.len(), reason: "allow" }
}

// ---------------------------------------------------------------------------
// 配额令牌桶
// ---------------------------------------------------------------------------

/// 配额令牌桶：容量随档（500/750/1000），速率补充，取用不足 deny 并计数。
pub struct TokenBucket {
    pub capacity: u32,
    pub tokens: u32,
    /// 取用不足的拒绝计数（显性化账面）。
    pub deny_count: u32,
}

impl TokenBucket {
    /// 按档位建桶：满桶起步（level 0=严格 / 1=标准 / 2=宽松）。
    pub fn for_level(level: usize) -> Self {
        let cap = TOKEN_CAPS[level.min(2)];
        TokenBucket { capacity: cap, tokens: cap, deny_count: 0 }
    }

    /// 速率补充：不超过容量（溢出令牌丢弃——令牌桶标准语义）。
    pub fn refill(&mut self, by: u32) {
        self.tokens = self.tokens.saturating_add(by).min(self.capacity);
    }

    /// 取用 n：足则扣减返回 true；不足 deny 返回 false 并计数（不静默）。
    pub fn take(&mut self, n: u32) -> bool {
        if self.tokens >= n {
            self.tokens -= n;
            true
        } else {
            self.deny_count += 1;
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 档位降级观察窗
// ---------------------------------------------------------------------------

/// 档位降级观察窗：错误率超阈值 5% 持续 60s → 建议降档事件入账。
/// 只建议不擅改——档位变更必须用户确认（主册治理纪律的对偶面）。
pub struct DowngradeWatch {
    /// 观察窗起点（None = 未在观察）。
    since: Option<u64>,
    /// 建议降档事件数（入账面）。
    pub suggestions: u32,
}

impl DowngradeWatch {
    pub const fn new() -> Self {
        DowngradeWatch { since: None, suggestions: 0 }
    }

    /// 是否在观察中（账面可视）。
    pub fn watching(&self) -> bool {
        self.since.is_some()
    }

    /// 观察：err_permille > 50 持续 ≥60s → 发出建议（true）并重置窗口；
    /// 回落到阈值内 → 窗口清零（不积累陈旧观察）。
    pub fn observe(&mut self, err_permille: u32, now_s: u64) -> bool {
        if err_permille <= DOWNGRADE_ERR_PERMILLE {
            self.since = None;
            return false;
        }
        match self.since {
            None => {
                self.since = Some(now_s);
                false
            }
            Some(t) if now_s.saturating_sub(t) >= DOWNGRADE_WINDOW_S => {
                self.suggestions += 1;
                self.since = None;
                true
            }
            Some(_) => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 审计聚合视图
// ---------------------------------------------------------------------------

/// 审计聚合视图：档位 × 操作类型二维计数表（3×4）。
pub struct AuditMatrix {
    pub cells: [[u32; 4]; 3],
}

impl AuditMatrix {
    pub const fn new() -> Self {
        AuditMatrix { cells: [[0; 4]; 3] }
    }

    /// 记一账（档位 0..3、操作 0..4；越界钳入表——边界不静默丢）。
    pub fn record(&mut self, level: usize, op: usize) {
        self.cells[level.min(2)][op.min(3)] += 1;
    }

    pub fn row_sum(&self, level: usize) -> u32 {
        self.cells[level].iter().sum()
    }

    pub fn col_sum(&self, op: usize) -> u32 {
        (0..3).map(|l| self.cells[l][op]).sum()
    }

    pub fn total(&self) -> u32 {
        (0..3).map(|l| self.row_sum(l)).sum()
    }

    /// 一致性自检：行合计之和 == 列合计之和 == 格子总和（三条独立求和路
    /// 径互证——聚合视图账面自洽）。
    pub fn consistent(&self) -> bool {
        let row_tot: u32 = (0..3).map(|l| self.row_sum(l)).sum();
        let col_tot: u32 = (0..4).map(|o| self.col_sum(o)).sum();
        row_tot == col_tot && col_tot == self.total()
    }
}

/// 域自检（深化批次三）。
pub fn run_f038e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F038-isolevel-d3");
    // 1) 六步全过 → 放行且走满六步。
    let v = run_pipeline(true, true, true, true, true, true);
    cs.add("pipeline_all_pass", v.allowed && v.stopped_at == 6 && v.reason == "allow", "");
    // 2) 规则步 deny 即止：带 deny-rule 原因码（后续步骤不再评估）。
    let vr = run_pipeline(true, false, false, false, false, false);
    cs.add(
        "pipeline_deny_rule",
        !vr.allowed && vr.stopped_at == 1 && vr.reason == DENY_REASONS[1],
        "",
    );
    // 3) 配额步 deny 即止：前面全过、quota 挡下（后续 audit/landing 未达）。
    let vq = run_pipeline(true, true, true, false, false, false);
    cs.add(
        "pipeline_deny_quota_stops",
        !vq.allowed && vq.stopped_at == 3 && vq.reason == DENY_REASONS[3],
        "",
    );
    // 4) 令牌桶容量随档：500/750/1000，满桶起步。
    cs.add(
        "token_caps_by_level",
        TokenBucket::for_level(0).capacity == 500
            && TokenBucket::for_level(1).capacity == 750
            && TokenBucket::for_level(2).capacity == 1000
            && TokenBucket::for_level(2).tokens == 1000,
        "",
    );
    // 5) 取用/拒绝/补充：耗尽后 deny 并计数；refill 溢出钳容量。
    let mut tb = TokenBucket::for_level(0);
    let ok = tb.take(499);
    let over = tb.take(2);
    tb.refill(10);
    let ok2 = tb.take(11);
    tb.refill(1000);
    cs.add(
        "token_take_deny_refill",
        ok && !over && tb.deny_count == 1 && ok2 && tb.tokens == 500 && tb.tokens <= tb.capacity,
        "",
    );
    // 6) 阈值内不观察不建议（窗口永不开启）。
    let mut w1 = DowngradeWatch::new();
    let _ = w1.observe(50, 0);
    let _ = w1.observe(50, 100);
    cs.add("downgrade_below_threshold", w1.suggestions == 0 && !w1.watching(), "");
    // 7) 持续超阈值 ≥60s → 建议入账一次，窗口重置（只建议不擅改档位）。
    let mut w2 = DowngradeWatch::new();
    let start = w2.observe(60, 0);
    let mid = w2.observe(60, 59);
    let fired = w2.observe(60, 60);
    cs.add(
        "downgrade_sustained_suggests",
        !start && !mid && fired && w2.suggestions == 1 && !w2.watching(),
        "",
    );
    // 8) 观察中断（回落阈值内）→ 窗口清零不积累；重新开始不足 60s 不建议。
    let mut w3 = DowngradeWatch::new();
    let _ = w3.observe(60, 0);
    let _ = w3.observe(60, 30);
    let cleared = w3.observe(40, 40);
    let _ = w3.observe(60, 100);
    let not_yet = w3.observe(60, 150);
    cs.add(
        "downgrade_interrupted_resets",
        !cleared && !not_yet && w3.suggestions == 0 && w3.watching(),
        "",
    );
    // 9) 审计聚合表：记账后行/列/总计三路一致。
    let mut am = AuditMatrix::new();
    am.record(0, 0);
    am.record(0, 1);
    am.record(1, 1);
    am.record(2, 3);
    cs.add(
        "audit_matrix_consistent",
        am.consistent()
            && am.row_sum(0) == 2
            && am.col_sum(1) == 2
            && am.total() == 4,
        "",
    );
    // 10) 越界记账钳入表内（不静默丢账）且一致自检仍闭合。
    am.record(5, 9);
    cs.add(
        "audit_matrix_clamp",
        am.cells[2][3] == 2 && am.consistent() && am.total() == 5,
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observe_window_boundary() {
        let mut w = DowngradeWatch::new();
        let _ = w.observe(80, 0);
        assert!(!w.observe(80, 59), "持续 59s 不足窗口 → 不建议");
        assert!(w.observe(80, 60), "持续恰 60s → 建议入账");
        assert_eq!(w.suggestions, 1);
    }

    #[test]
    fn refill_never_exceeds_cap() {
        let mut tb = TokenBucket::for_level(1);
        let _ = tb.take(750);
        assert!(!tb.take(1) && tb.deny_count == 1, "空桶取用拒绝并计数");
        tb.refill(600);
        assert_eq!(tb.tokens, 600);
        tb.refill(600);
        assert_eq!(tb.tokens, 750, "补充溢出钳在容量（不超发）");
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f038e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
