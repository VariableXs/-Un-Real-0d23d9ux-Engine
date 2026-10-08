//! 深化层 · F552 以管理员身份运行（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F552 节）：
//! ①「滥用防护（声明请求特权的理由字段，缺省显示『未说明用途』）」的
//!   账面化——未说明用途的提权逐笔入滥用账，连续未说明达阈值即打审计
//!   标记（滥用防护不是一句文案，是一台可查的账）；
//! ②特权会话**状态机深化**——降权幂等（重复降权零副作用）、降权后
//!   防护条消失、再提权必须重新走 F551 门（防护色边条的生命周期闭环）；
//! ③三入口链路**一致性**——应用/终端/脚本三类入口走同一确认联动，
//!   会话记录必须如实带出入口类型（三链路一个语义，不许各搞一套）。

use crate::checks::CheckSet;
use crate::istar::adminrun::{AdminRun, EntryKind, PrivSession, REASON_UNSTATED};
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::privconfirm::{PrivGate, Verdict};

// ---------------------------------------------------------------------------
// 滥用账（理由字段的防护面）
// ---------------------------------------------------------------------------

/// 未说明用途的滥用账（定长环，容量 16——审计窗口足够）。
pub struct AbuseLedger {
    unstated: [Option<u64>; 16], // 提权时刻 ms
    stated: u32,
    unstated_total: u32,
    len: usize,
    /// 连续未说明计数（达阈值即标记）。
    unstated_streak: u32,
    flagged: bool,
}

/// 连续未说明用途的审计标记阈值。
pub const ABUSE_STREAK_MARK: u32 = 3;

impl AbuseLedger {
    pub fn new() -> AbuseLedger {
        AbuseLedger {
            unstated: [None; 16],
            stated: 0,
            unstated_total: 0,
            len: 0,
            unstated_streak: 0,
            flagged: false,
        }
    }

    /// 记一笔提权的理由字段（None = 未说明）。
    pub fn record(&mut self, ms: u64, reason: Option<&str>) {
        match reason {
            Some(r) if !r.is_empty() => {
                self.stated += 1;
                self.unstated_streak = 0;
            }
            _ => {
                self.unstated_total += 1;
                self.unstated_streak += 1;
                if self.len < 16 {
                    self.unstated[self.len] = Some(ms);
                    self.len += 1;
                } else {
                    for i in 1..16 {
                        self.unstated[i - 1] = self.unstated[i];
                    }
                    self.unstated[15] = Some(ms);
                }
                if self.unstated_streak >= ABUSE_STREAK_MARK {
                    self.flagged = true;
                }
            }
        }
    }

    /// 未说明用途的最近时刻（滥用审计取数，新到旧）。
    pub fn unstated_ms(&self) -> alloc::vec::Vec<u64> {
        (0..self.len).filter_map(|i| self.unstated[i]).collect()
    }

    /// 未说明占比（千分比——0 说明全部给理由）。
    pub fn unstated_permille(&self) -> u32 {
        let total = self.stated + self.unstated_total;
        if total == 0 {
            0
        } else {
            self.unstated_total * 1000 / total
        }
    }

    pub fn flagged(&self) -> bool {
        self.flagged
    }
}

impl Default for AbuseLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 会话状态机深化（幂等降权 + 防护条生命周期）
// ---------------------------------------------------------------------------

/// 降权幂等性：第二次降权必须零副作用（返回 None、状态不变）。
pub fn demote_idempotent(ar: &mut AdminRun) -> bool {
    let first = ar.demote();
    let second = ar.demote();
    first.is_some() && second.is_none() && !ar.privileged() && ar.guard_bar().is_none()
}

/// 再提权必须重新过 F551 门：降权后 elevate 重新进入确认流
/// （MergedGrant 亦算过门——F551 的会话合并授权在其自身会话内仍有效，
/// 两层会话语义各自独立：F551 管「同类不重复问」，F552 管「单会话特权」）。
pub fn re_elevate_reenters_gate(ar: &mut AdminRun, target: &str, kind: EntryKind) -> bool {
    matches!(ar.elevate(target, kind, Some("审计需要")), Verdict::Pending | Verdict::MergedGrant)
}

/// 会话与入口一致性：确认产出的会话必须如实带入口类型与目标。
pub fn session_matches_request(s: &PrivSession, target: &str, kind: EntryKind) -> bool {
    s.target == target && s.kind == kind && !s.reason.is_empty()
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f552_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 滥用账：未说明入账、说明清连击、占比可查。
    let mut ab = AbuseLedger::new();
    ab.record(1_000, None);
    ab.record(2_000, Some("装驱动要内核签名"));
    ab.record(3_000, None);
    cs.add(
        "abuse ledger counts",
        ab.unstated_ms().len() == 2 && ab.unstated_permille() == 666 && !ab.flagged(),
        "",
    );

    // 2) 连续未说明达 3 次打审计标记；中间说明一次即断连击。
    let mut ab2 = AbuseLedger::new();
    ab2.record(1, None);
    ab2.record(2, None);
    ab2.record(3, None);
    cs.add("abuse streak marks at three", ab2.flagged(), "");
    let mut ab3 = AbuseLedger::new();
    ab3.record(1, None);
    ab3.record(2, None);
    ab3.record(3, Some("调试"));
    ab3.record(4, None);
    cs.add("stated reason breaks streak", !ab3.flagged(), "");

    // 3) 缺省理由合同：None 与空串都落「未说明用途」——不给空串过审。
    let mut ar = AdminRun::new(PrivGate::new());
    ar.tick(100);
    let _ = ar.elevate("diskmgr", EntryKind::App, None);
    let s = ar.confirm();
    cs.add(
        "unstated reason default literal",
        s.map(|s| s.reason == REASON_UNSTATED).unwrap_or(false),
        "",
    );

    // 4) 降权幂等 + 防护条生命周期闭环。
    let mut ar2 = AdminRun::new(PrivGate::new());
    ar2.tick(200);
    let _ = ar2.elevate("term", EntryKind::Terminal, Some("运维"));
    let _ = ar2.confirm();
    let bar_while_priv = ar2.guard_bar().is_some();
    cs.add(
        "demote idempotent and bar cleared",
        bar_while_priv && demote_idempotent(&mut ar2),
        "",
    );

    // 5) 降权后再提权重新走 F551 门（确认流再入）。
    cs.add("re-elevate reenters gate", re_elevate_reenters_gate(&mut ar2, "term", EntryKind::Terminal), "");

    // 6) 三入口一致性：三类入口的会话都如实带类型；确认一次性。
    let mut ok = true;
    for kind in [EntryKind::App, EntryKind::Terminal, EntryKind::Script] {
        let mut ar3 = AdminRun::new(PrivGate::new());
        ar3.tick(300);
        let _ = ar3.elevate("tool", kind, Some("x"));
        let s1 = ar3.confirm();
        let s2 = ar3.confirm();
        ok = ok
            && s1.map(|s| session_matches_request(&s, "tool", kind)).unwrap_or(false)
            && s2.is_none();
    }
    cs.add("three entries one semantics", ok, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abuse_ring_rolls() {
        let mut ab = AbuseLedger::new();
        for i in 0..20u64 {
            ab.record(i * 100, None);
        }
        assert_eq!(ab.unstated_ms().len(), 16);
        assert_eq!(ab.unstated_ms()[0], 400); // 最旧 4 笔被滚出
        assert!(ab.flagged());
    }

    #[test]
    fn empty_reason_counts_as_unstated() {
        let mut ab = AbuseLedger::new();
        ab.record(1, Some(""));
        assert_eq!(ab.unstated_permille(), 1000);
    }
}
